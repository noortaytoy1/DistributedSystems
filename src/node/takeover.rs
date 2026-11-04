//! Takeover: a node that lost a bid round stands by, and plans the request itself if the winner
//! goes silent, or stays silent too long, before anything is committed for it (D42, D110).
//!
//! The order comes from the closed round (`bidding::executor` over its ranking) and silence from
//! the peer heartbeat detector (`heartbeat.rs`). If two nodes take over at once, the first commit
//! wins, as for any split decision.
//!
//! Owner: C (fault handling).

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use super::{NodeContext, lock};
use crate::bidding::{ScoreVariant, executor};
use crate::metrics::Event;
use crate::proto::{ClientRequest, NodeId};

/// The backstop never fires sooner than this, so a slow plan or a slow commit is not duplicated. It
/// stays under the client's first retry, 2 s less 20% of jitter (D235).
const BACKSTOP_MIN: Duration = Duration::from_millis(1500);

/// The backstop waits this many times the winner's bid when the bid is an expected wait.
const BACKSTOP_FACTOR: u64 = 3;

/// How often a standing-by node looks again. Suspicion itself changes once per heartbeat, and every
/// request in flight runs this loop, so more checks only cost CPU when commits are slow (D239).
const CHECKS_PER_HEARTBEAT: u32 = 1;
/// A node stands by for most reports, so this runs once per check for each of them (D253).
const STAND_BY_CHECK_AT_MOST: Duration = Duration::from_millis(200);

/// Why a node took over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reason {
    /// The heartbeat detector suspects the winner.
    Suspected,
    /// The winner still answers heartbeats, but nothing was committed within the backstop.
    Backstop,
}

impl Reason {
    fn as_str(self) -> &'static str {
        match self {
            Reason::Suspected => "suspected",
            Reason::Backstop => "backstop",
        }
    }
}

/// How long the winner gets to commit while it still answers heartbeats. Only an expected-wait
/// score is in microseconds; queue-only and CPU-only scores get the minimum.
fn backstop(variant: ScoreVariant, winner_bid: u64) -> Duration {
    match variant {
        ScoreVariant::ExpectedWait | ScoreVariant::ExpectedWaitCpu { .. } => BACKSTOP_MIN.max(
            Duration::from_micros(winner_bid.saturating_mul(BACKSTOP_FACTOR)),
        ),
        ScoreVariant::QueueOnly | ScoreVariant::CpuOnly => BACKSTOP_MIN,
    }
}

/// Called after this node lost the round for `req`. Waits until the request is committed, or this
/// node should plan it; returns true to plan. Gives up after `client_retry_ms`, when the client's
/// retry starts a new round. A node that is not serving never takes over.
pub async fn stand_by(ctx: &NodeContext, req: &ClientRequest) -> bool {
    let key = (req.id, req.attempt);
    let Some(bids) = ctx.bids.with_rounds(|r| r.ranked_bids(key)) else {
        return false;
    };
    let ranking: Vec<NodeId> = bids.iter().map(|&(node, _)| node).collect();
    // Rank 0 is the winner; a winner that did not plan (the result was already there) has
    // nothing to take over.
    let Some(rank) = ranking.iter().position(|&node| node == ctx.id) else {
        return false;
    };
    if rank == 0 {
        return false;
    }
    let (winner, winner_bid) = bids[0];
    let timers = &ctx.config.timers;
    let started = Instant::now();
    let backstop_at = started + backstop(ctx.config.policy.score_variant(), winner_bid);
    let give_up = started + timers.client_retry();
    // Lower ranks wait one heartbeat more each, so two nodes with different views of who is
    // silent rarely plan at once.
    let stagger = timers.peer_heartbeat() * (rank as u32 - 1);

    let mut tick = tokio::time::interval(
        (timers.peer_heartbeat() / CHECKS_PER_HEARTBEAT).max(STAND_BY_CHECK_AT_MOST),
    );
    // A late tick must not be made up in a burst: that would add load when the node is behind.
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut due: Option<Instant> = None;
    loop {
        tick.tick().await;
        let now = Instant::now();
        if now >= give_up || ctx.state.with(|s| s.result(&req.id).is_some()) {
            return false;
        }
        if !ctx.serving.load(Ordering::SeqCst) {
            due = None;
            continue;
        }
        let (target, reason) = {
            let suspicion = lock(&ctx.suspicion);
            let past_backstop = now >= backstop_at;
            let target = executor(&ranking, |node| {
                suspicion.is_suspected(node) || (node == winner && past_backstop)
            });
            let reason = if suspicion.is_suspected(winner) {
                Reason::Suspected
            } else {
                Reason::Backstop
            };
            (target, reason)
        };
        if target != Some(ctx.id) {
            due = None;
            continue;
        }
        if now < *due.get_or_insert(now + stagger) {
            continue;
        }
        ctx.log.log(Event::TakeoverStarted {
            request: req.id,
            attempt: req.attempt,
            winner,
            rank,
            reason: reason.as_str().into(),
            waited_us: now.saturating_duration_since(started).as_micros() as u64,
        });
        return true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backstop_is_three_bids_but_at_least_the_minimum() {
        let wait = ScoreVariant::ExpectedWait;
        assert_eq!(backstop(wait, 1_000), BACKSTOP_MIN);
        assert_eq!(backstop(wait, 500_000), Duration::from_millis(1500));
        let cpu = ScoreVariant::ExpectedWaitCpu { cpu_weight: 10 };
        assert_eq!(backstop(cpu, 500_000), Duration::from_millis(1500));
        assert_eq!(backstop(wait, u64::MAX), Duration::from_micros(u64::MAX));
    }

    #[test]
    fn scores_that_are_not_times_get_the_minimum() {
        assert_eq!(backstop(ScoreVariant::QueueOnly, 1_000_000), BACKSTOP_MIN);
        assert_eq!(backstop(ScoreVariant::CpuOnly, 1_000_000), BACKSTOP_MIN);
    }
}
