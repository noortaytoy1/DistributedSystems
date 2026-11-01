//! Fault injection: silence a node for a while, by hand (`ctl silence`) or on a schedule.
//!
//! A silenced node drops all peer and client traffic (`FaultGate` in transport.rs) but keeps its
//! state, like a node cut off by the network. The spec allows failures of up to 20 s.
//!
//! One fault at a time per node: a silence order for a node that is already silenced is
//! rejected, never merged or restarted (D71). Durations of zero or above `MAX_FAULT` are
//! rejected too, so every `silenced`/`restored` pair in the logs is a real, in-spec fault.
//!
//! `faults.enabled = false` in the config (or `node --faults off`) turns all of this off: the
//! schedule never starts and `silence_for` refuses every order, for no-fault baseline runs (D74).
//!
//! The schedule (D12, D75) runs on every node but acts only while the node is the serving Raft
//! leader. It waits a random `faults.interval_*`, picks a victim uniformly from all nodes (itself
//! included, which exercises leader failover), orders it silent for a random `faults.silence_*`,
//! and waits the silence out before the next round, so faults never overlap. The order travels
//! as `PeerBody::FaultOrder` in an envelope carrying the leader's term: a victim with a newer
//! term drops it by the term rule (D21). A victim that is still recovering refuses it.
//!
//! Owner: C (faults and failover).

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::config::FaultConfig;
use crate::metrics::Event;
use crate::node::NodeContext;
use crate::proto::{NodeId, PeerBody, PeerEnvelope, PeerReply, Term};

/// Longest fault the system must tolerate transparently.
pub const MAX_FAULT: Duration = Duration::from_secs(20);

/// Silence this node now and restore it after `duration` (`ctl silence`).
pub fn silence_for(ctx: &Arc<NodeContext>, duration: Duration) -> Result<(), String> {
    if !ctx.config.faults.enabled {
        return Err("fault injection is disabled on this node (faults.enabled = false)".into());
    }
    check_duration(duration)?;
    if !ctx.gate.try_silence() {
        return Err("already silenced; wait for the current fault to end".into());
    }
    let ms = duration.as_millis() as u64;
    ctx.log.log(Event::Silenced { ms });
    tracing::info!(node = ctx.id, ms, "silenced");

    // The task ends after one sleep, so it needs no handle. If the node shuts down first, the
    // runtime drops it with everything else.
    let ctx = ctx.clone();
    tokio::spawn(async move {
        tokio::time::sleep(duration).await;
        ctx.gate.restore();
        ctx.log.log(Event::Restored);
        tracing::info!(node = ctx.id, "restored");
    });
    Ok(())
}

/// A fault must last at least 1 ms and at most `MAX_FAULT`.
fn check_duration(duration: Duration) -> Result<(), String> {
    if duration.is_zero() {
        return Err("silence must last at least 1 ms".into());
    }
    if duration > MAX_FAULT {
        return Err(format!(
            "silence of {} ms is above the {} ms limit",
            duration.as_millis(),
            MAX_FAULT.as_millis()
        ));
    }
    Ok(())
}

/// A fault order from the leader's schedule (`PeerBody::FaultOrder`, or the leader to itself).
/// The term rule has already been applied; a node still catching up refuses (D75).
pub fn on_order(ctx: &Arc<NodeContext>, duration: Duration) -> Result<(), String> {
    if !ctx.serving.load(Ordering::SeqCst) {
        return Err("recovering: not serving yet".into());
    }
    silence_for(ctx, duration)
}

/// One round of the schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Round {
    /// How long to wait before ordering the fault.
    pub wait: Duration,
    pub victim: NodeId,
    pub silence: Duration,
}

/// Draw the next round. Pure, so a seed gives the same sequence everywhere.
pub fn draw(rng: &mut impl Rng, nodes: &[NodeId], cfg: &FaultConfig) -> Round {
    Round {
        wait: between(rng, cfg.interval()),
        victim: nodes[rng.gen_range(0..nodes.len())],
        silence: between(rng, cfg.silence()),
    }
}

fn between(rng: &mut impl Rng, (lo, hi): (Duration, Duration)) -> Duration {
    Duration::from_millis(rng.gen_range(lo.as_millis() as u64..=hi.as_millis() as u64))
}

/// Start the scheduled fault injector.
pub fn spawn_schedule(ctx: &Arc<NodeContext>) -> Option<JoinHandle<()>> {
    if !ctx.config.faults.enabled {
        return None;
    }
    let seed = ctx.config.faults.seed.unwrap_or_else(rand::random);
    tracing::info!(node = ctx.id, seed, "fault schedule armed");
    Some(tokio::spawn(run_schedule(ctx.clone(), seed)))
}

async fn run_schedule(ctx: Arc<NodeContext>, seed: u64) {
    let nodes: Vec<NodeId> = ctx.config.node_ids().collect();
    let poll = Duration::from_millis(ctx.config.timers.raft_heartbeat_ms);
    loop {
        let term = loop {
            let s = ctx.consensus.status();
            if leads(&ctx, s.term) {
                break s.term;
            }
            tokio::time::sleep(poll).await;
        };
        // Mixing in the term means a new leader does not replay its predecessor's sequence.
        let mut rng = StdRng::seed_from_u64(seed ^ term.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        while leads(&ctx, term) {
            let round = draw(&mut rng, &nodes, &ctx.config.faults);
            if !wait_while_leading(&ctx, term, round.wait, poll).await {
                break;
            }
            let ms = round.silence.as_millis() as u64;
            ctx.log.log(Event::FaultOrdered {
                victim: round.victim,
                ms,
                term,
            });
            match order(&ctx, round.victim, term, round.silence).await {
                // Wait the silence out even if we were the victim, so faults never overlap.
                Ok(()) => tokio::time::sleep(round.silence).await,
                Err(reason) => {
                    tracing::warn!(node = ctx.id, victim = round.victim, %reason, "fault refused");
                    ctx.log.log(Event::FaultRefused {
                        victim: round.victim,
                        reason,
                    });
                }
            }
        }
    }
}

/// This node is the serving leader of `term`.
fn leads(ctx: &NodeContext, term: Term) -> bool {
    let s = ctx.consensus.status();
    s.leader == Some(ctx.id) && s.term == term && ctx.serving.load(Ordering::SeqCst)
}

/// Sleep `wait`, checking leadership every `poll`. False if leadership was lost first.
async fn wait_while_leading(ctx: &NodeContext, term: Term, wait: Duration, poll: Duration) -> bool {
    let deadline = Instant::now() + wait;
    loop {
        if !leads(ctx, term) {
            return false;
        }
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        tokio::time::sleep(poll.min(deadline - now)).await;
    }
}

/// Deliver one order: to ourselves directly, to a peer as `PeerBody::FaultOrder`.
async fn order(
    ctx: &Arc<NodeContext>,
    victim: NodeId,
    term: Term,
    silence: Duration,
) -> Result<(), String> {
    if victim == ctx.id {
        return on_order(ctx, silence);
    }
    let Some(peer) = ctx.peers.get(victim) else {
        return Err(format!("node {victim} is not a peer"));
    };
    let envelope = PeerEnvelope {
        from: ctx.id,
        term,
        body: PeerBody::FaultOrder {
            ms: silence.as_millis() as u64,
        },
    };
    match peer
        .call(envelope, ctx.config.timers.peer_rpc_timeout())
        .await
    {
        Ok(PeerReply::FaultOrder(result)) => result,
        Ok(PeerReply::StaleTerm { current }) => {
            Err(format!("stale term: victim is at term {current}"))
        }
        Ok(_) => Err("unexpected reply".into()),
        Err(e) => Err(format!("unreachable: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::transport::FaultGate;

    #[test]
    fn durations_from_1_ms_to_the_limit_are_accepted() {
        assert!(check_duration(Duration::from_millis(1)).is_ok());
        assert!(check_duration(Duration::from_secs(5)).is_ok());
        assert!(check_duration(MAX_FAULT).is_ok());
    }

    #[test]
    fn zero_and_over_the_limit_are_rejected() {
        assert!(check_duration(Duration::ZERO).is_err());
        assert!(check_duration(MAX_FAULT + Duration::from_millis(1)).is_err());
    }

    #[test]
    fn rounds_stay_inside_the_configured_ranges_and_cover_every_node() {
        let cfg = FaultConfig::default();
        let nodes = [1, 2, 3];
        let mut rng = StdRng::seed_from_u64(7);
        let mut victims = std::collections::BTreeSet::new();
        for _ in 0..1_000 {
            let r = draw(&mut rng, &nodes, &cfg);
            assert!((cfg.interval().0..=cfg.interval().1).contains(&r.wait));
            assert!((cfg.silence().0..=cfg.silence().1).contains(&r.silence));
            assert!(r.silence <= MAX_FAULT);
            victims.insert(r.victim);
        }
        assert_eq!(victims.into_iter().collect::<Vec<_>>(), nodes);
    }

    #[test]
    fn the_same_seed_gives_the_same_schedule() {
        let cfg = FaultConfig::default();
        let run = |seed| {
            let mut rng = StdRng::seed_from_u64(seed);
            (0..20)
                .map(|_| draw(&mut rng, &[1, 2, 3], &cfg))
                .collect::<Vec<_>>()
        };
        assert_eq!(run(42), run(42));
        assert_ne!(run(42), run(43));
    }

    #[test]
    fn a_silenced_gate_cannot_be_silenced_again_until_restored() {
        let gate = FaultGate::default();
        assert!(gate.try_silence());
        assert!(!gate.try_silence());
        assert!(gate.is_silenced());
        gate.restore();
        assert!(gate.try_silence());
    }
}
