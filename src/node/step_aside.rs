//! A busy leader steps aside, so that an ordinary vote picks another one (D218).
//!
//! Every result passes through the Raft leader, so an overloaded leader machine slows every commit,
//! and the bid can move planning away from it but not the ordering. Once a second the leader looks
//! at the last second. Its load is its mean bid, or the mean time one log write took if that is
//! higher and over the bar in [`LogReading::is_slow`]. The bid sees planning, and bids even out,
//! because a loaded node loses rounds. The write time sees a machine too busy to keep the log, which
//! the bid can miss (D218). It reads one write and not the time a proposal waited, because a queue
//! grows with what arrives, on whichever node leads, and moving the leader cannot shorten it (D228).
//!
//! A leader whose load has stayed at least `policy.leader_overload_ratio` times its lowest peer's
//! mean bid for `policy.leader_overload_for_ms`, and above a small floor, goes quiet if it is safe
//! to. Safe means it is serving and not degraded and not silenced, it has all members in its group
//! view and suspects nobody, no node it knows of is in a silence the failure draw named, every peer
//! bid in that second (a node that is not serving does not bid), and
//! `policy.leader_overload_cooldown_ms` have passed since the last term change. The cooldown is what
//! stops a storm: every node starts it again on every new term, so the cluster moves its leader this
//! way once per cooldown at most.
//!
//! **Going quiet** is the whole mechanism, because openraft 0.9.25 has no hand-over command and a
//! follower refuses votes while it has heard from its leader within the lease. The leader stops its
//! heartbeats and holds new writes in the proposal queue. The followers' timers then run out, as
//! they would for a leader that failed, and a vote picks a new leader by the rules in D115 and D125,
//! which this does not change. So Raft's safety is not touched either: all this can cause is an
//! election, and one can happen at any time. The vote knows nothing of load, so the new leader is
//! the follower whose timer ends first.
//!
//! The old leader sees the higher term, steps down, and ends its quiet at once. If nobody takes over
//! within [`QUIET_LIMIT`] it ends its quiet anyway, carries on, says why, and waits a full cooldown.
//!
//! No new message: the leader reads the bids it already receives, so nothing here can reach a
//! silenced node or get past the fault gate. This is middleware: the incident service never knows
//! it happened (D156, `tests/layers.rs`).
//!
//! Owner: A (Raft and replication).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior};

use super::{NodeContext, lock};
use crate::config::{AssignmentPolicy, Policy, ScoreName};
use crate::metrics::Event;
use crate::proto::{NodeId, Term};
use crate::raft::{ConsensusStatus, LogWork};

/// How often a node looks at the bids. Also how much of them one reading covers.
const CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// How long a leader stays quiet waiting for a new leader. A normal vote takes under 1 s with the
/// default timers (D115), and this is also how long a proposal may wait (`propose_timeout_ms`).
pub const QUIET_LIMIT: Duration = Duration::from_secs(3);

/// How often a quiet leader looks for a higher term.
const WATCH_INTERVAL: Duration = Duration::from_millis(20);

/// A mean write time at or below this never counts. A healthy write took about 9 ms at 300 users and
/// 14 ms at 3000 on one PC, at most 16.5, because grouping keeps it nearly flat under load (D228).
const WRITE_FLOOR: Duration = Duration::from_millis(20);

/// Nor does one at or below this many of this node's own disk saves, in tenths: 2.5. A write is a
/// save here and a save on a follower, so a healthy one is about 2 saves on any disk: at most 2.21
/// with 25 ms added to each save, at 3000 users, steady or all at once. So a slow disk alone does not
/// pass for a busy leader. Over 20 ms the floor is no help any more, and this is what is left.
const WRITE_SAVES_TENTHS: u64 = 25;

/// A node at or below this score is never overloaded, so that tiny numbers on an idle cluster cannot
/// make a large ratio. In the score's own unit: 1 ms of expected wait, jobs or permille of CPU.
fn floor(score: ScoreName) -> u64 {
    match score {
        ScoreName::ExpectedWait | ScoreName::ExpectedWaitCpu => 1_000,
        ScoreName::QueueOnly => 8,
        ScoreName::CpuOnly => 500,
    }
}

/// The deciding rule: at least `ratio` times the lowest peer, and above the floor. Never with no
/// peers to compare with, and never at ratio 0, which is off. Comparing with a peer is what keeps a
/// cluster whose planners are all busy from moving its leader for nothing. A log at its limit does
/// not show in the bids, and [`LogReading::is_slow`] is what keeps that out (D228).
fn overloaded(load: u64, peer_scores: &[u64], ratio: u64, floor: u64) -> bool {
    if ratio == 0 || load <= floor {
        return false;
    }
    let Some(&lowest) = peer_scores.iter().min() else {
        return false;
    };
    load >= ratio.saturating_mul(lowest)
}

/// What this node's log did in one reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LogReading {
    /// The mean time one write took, from `client_write` to its answer.
    write_us: u64,
    /// The mean time per proposal, from entering the queue to being answered. For the log only: it
    /// grows with the queue, so it says nothing about whose machine is slow (D228).
    wait_us: u64,
    /// The mean disk save.
    save_us: u64,
}

impl LogReading {
    /// Between two readings of the log's counters, `over` apart. A write in flight at the first
    /// reading with none finished by the second took at least all of `over`: without this, a leader
    /// too slow to finish one write in a second would have no write time at all.
    fn between(before: LogWork, now: LogWork, over: Duration) -> Self {
        let done = now.writes_done.saturating_sub(before.writes_done);
        let write_us = if done > 0 {
            now.write_us.saturating_sub(before.write_us) / done
        } else if before.writes > before.writes_done {
            over.as_micros() as u64
        } else {
            0
        };
        let answered = now.answered.saturating_sub(before.answered);
        let wait_us = now.answered_us.saturating_sub(before.answered_us) / answered.max(1);
        let saves = now.saves.saturating_sub(before.saves);
        let save_us = now.save_us.saturating_sub(before.save_us) / saves.max(1);
        Self {
            write_us,
            wait_us,
            save_us,
        }
    }

    fn is_slow(&self) -> bool {
        let bar = self.save_us.saturating_mul(WRITE_SAVES_TENTHS) / 10;
        self.write_us > bar.max(WRITE_FLOOR.as_micros() as u64)
    }
}

/// What the rule compares with the peers' bids: this node's bid, or how long one log write takes if
/// that is slow and higher. Only a score that is itself a time can be compared with a time, so a
/// score counted in jobs or in permille of CPU is left as it is.
fn leader_load(score: u64, log: LogReading, kind: ScoreName) -> u64 {
    let same_unit = matches!(kind, ScoreName::ExpectedWait | ScoreName::ExpectedWaitCpu);
    if same_unit && log.is_slow() {
        score.max(log.write_us)
    } else {
        score
    }
}

/// This node's score and each peer's, if every one of them bid in this reading. A peer that is
/// catching up does not bid, so a leader never steps aside with a peer that cannot serve yet.
fn split_scores(
    me: NodeId,
    peers: &[NodeId],
    scores: &BTreeMap<NodeId, u64>,
) -> Option<(u64, BTreeMap<NodeId, u64>)> {
    let mine = *scores.get(&me)?;
    let others = peers
        .iter()
        .map(|peer| scores.get(peer).map(|score| (*peer, *score)))
        .collect::<Option<_>>()?;
    Some((mine, others))
}

/// What must hold before a node may go quiet, all read at one moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Safety {
    leads: bool,
    serving: bool,
    degraded: bool,
    silenced: bool,
    /// Every configured node is in this node's group view.
    whole_view: bool,
    peer_suspected: bool,
    /// A node the failure draw named is still meant to be silent.
    drawn_silence: bool,
    /// Since the last term change, or the end of the last try.
    calm_for: Duration,
}

impl Safety {
    fn holds(&self, cooldown: Duration) -> bool {
        self.leads
            && self.serving
            && !self.degraded
            && !self.silenced
            && self.whole_view
            && !self.peer_suspected
            && !self.drawn_silence
            && self.calm_for >= cooldown
    }
}

/// Whether something has held without a break for long enough.
#[derive(Debug, Default)]
struct InARow {
    since: Option<Instant>,
}

impl InARow {
    fn observe(&mut self, holds: bool, wanted: Duration, now: Instant) -> bool {
        if !holds {
            self.since = None;
            return false;
        }
        let since = *self.since.get_or_insert(now);
        now.saturating_duration_since(since) >= wanted
    }
}

/// How a step-aside ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ended {
    /// Another node leads.
    Changed,
    /// The vote picked this node again.
    ReElected,
    /// No new leader in time.
    TimedOut,
    /// This node was silenced, stopped serving or went degraded, so the try is off.
    Stopped,
}

impl Ended {
    fn as_str(self) -> &'static str {
        match self {
            Ended::Changed => "changed",
            Ended::ReElected => "re_elected",
            Ended::TimedOut => "timed_out",
            Ended::Stopped => "stopped",
        }
    }
}

/// What a status says about a try begun in `term`. `None` is still waiting: either this node still
/// leads in that term, or a vote is running and nobody has won it yet.
fn how_it_ended(me: NodeId, term: Term, status: &ConsensusStatus) -> Option<Ended> {
    match status.leader {
        Some(leader) if leader != me => Some(Ended::Changed),
        Some(_) if status.term != term => Some(Ended::ReElected),
        _ => None,
    }
}

/// Start the check, unless it is off. Under leader dispatch there are no bids to read.
pub fn spawn(ctx: &Arc<NodeContext>) -> Option<JoinHandle<()>> {
    let policy = ctx.config.policy;
    if policy.leader_overload_ratio == 0 || policy.assignment != AssignmentPolicy::Bidding {
        return None;
    }
    Some(tokio::spawn(run(ctx.clone(), policy)))
}

/// Runs on every node, so that each one knows when the term last changed if it becomes leader, and
/// reads each second's bids away whether it leads or not.
async fn run(ctx: Arc<NodeContext>, policy: Policy) {
    let (ratio, kind) = (policy.leader_overload_ratio, policy.score);
    let floor = floor(kind);
    let (wanted, cooldown) = (
        policy.leader_overload_for(),
        policy.leader_overload_cooldown(),
    );
    let peers: Vec<NodeId> = ctx.peers.iter().map(|peer| peer.peer()).collect();
    let mut term = ctx.consensus.status().term;
    let mut calm_since = Instant::now();
    let mut in_a_row = InARow::default();
    let mut last_work = ctx.consensus.log_work();
    let mut tick = tokio::time::interval(CHECK_INTERVAL);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        let scores = ctx.bids.take_mean_scores();
        let work = ctx.consensus.log_work();
        let log = LogReading::between(last_work, work, CHECK_INTERVAL);
        last_work = work;
        let status = ctx.consensus.status();
        let now = Instant::now();
        if status.term != term {
            term = status.term;
            calm_since = now;
        }
        let safe = safety(
            &ctx,
            &status,
            now.saturating_duration_since(calm_since),
            now,
        );
        let over = split_scores(ctx.id, &peers, &scores).filter(|(score, peer_scores)| {
            let peer_scores: Vec<u64> = peer_scores.values().copied().collect();
            let load = leader_load(*score, log, kind);
            safe.holds(cooldown) && overloaded(load, &peer_scores, ratio, floor)
        });
        let held_long_enough = in_a_row.observe(over.is_some(), wanted, now);
        let (true, Some((score, peer_scores))) = (held_long_enough, over) else {
            continue;
        };
        let load = leader_load(score, log, kind);
        tracing::info!(
            node = ctx.id,
            term,
            score,
            ?log,
            load,
            ?peer_scores,
            "overloaded: going quiet"
        );
        ctx.log.log(Event::LeaderSteppedAside {
            term,
            score,
            write_us: log.write_us,
            log_wait_us: log.wait_us,
            save_us: log.save_us,
            load,
            peer_scores,
        });
        step_aside(&ctx, term, QUIET_LIMIT).await;
        // Whatever happened, a full cooldown starts now.
        term = ctx.consensus.status().term;
        calm_since = Instant::now();
        in_a_row = InARow::default();
        last_work = ctx.consensus.log_work();
    }
}

fn safety(ctx: &NodeContext, status: &ConsensusStatus, calm_for: Duration, now: Instant) -> Safety {
    Safety {
        leads: status.leader == Some(ctx.id),
        serving: ctx.serving.load(Ordering::SeqCst),
        degraded: ctx.degraded.is_active(),
        silenced: ctx.gate.is_silenced(),
        whole_view: ctx.group.view().members.len() == ctx.config.cluster.nodes.len(),
        peer_suspected: !lock(&ctx.suspicion).suspected().is_empty(),
        drawn_silence: ctx.fault_election.drawn_silence_going_on(now),
        calm_for,
    }
}

/// Go quiet until another leader appears or `limit` is up, then log how it went. Bids and client
/// handling go on as usual throughout, and only the Raft traffic this node starts stops.
async fn step_aside(ctx: &NodeContext, term: Term, limit: Duration) -> Ended {
    let started = Instant::now();
    ctx.consensus.set_quiet(true);
    let mut quiet = true;
    let ended = loop {
        let status = ctx.consensus.status();
        // It has stepped down. A vote may still be running, and if this node is to stand again or
        // answer as a follower it must not be holding anything back.
        if quiet && (status.term != term || status.leader != Some(ctx.id)) {
            ctx.consensus.set_quiet(false);
            quiet = false;
        }
        if let Some(ended) = how_it_ended(ctx.id, term, &status) {
            break ended;
        }
        if ctx.gate.is_silenced() || !ctx.serving.load(Ordering::SeqCst) || ctx.degraded.is_active()
        {
            break Ended::Stopped;
        }
        if started.elapsed() >= limit {
            break Ended::TimedOut;
        }
        tokio::time::sleep(WATCH_INTERVAL).await;
    };
    ctx.consensus.set_quiet(false);

    let status = ctx.consensus.status();
    let waited_ms = started.elapsed().as_millis() as u64;
    ctx.log.log(Event::LeaderStepAsideEnded {
        term,
        outcome: ended.as_str(),
        new_term: status.term,
        new_leader: status.leader,
        waited_ms,
    });
    match ended {
        Ended::TimedOut => tracing::warn!(
            node = ctx.id,
            term,
            new_term = status.term,
            leader = ?status.leader,
            waited_ms,
            "no new leader in time, so heartbeats are back on; the next try is a full cooldown away"
        ),
        _ => tracing::info!(
            node = ctx.id,
            term,
            new_term = status.term,
            leader = ?status.leader,
            waited_ms,
            outcome = ended.as_str(),
            "step-aside ended"
        ),
    }
    ended
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::EventLog;
    use crate::node::testing::{ScriptedConsensus, context, log_dir, logged, until};
    use crate::raft::Role;

    const RATIO: u64 = 3;
    const FLOOR: u64 = 1_000;
    const COOLDOWN: Duration = Duration::from_secs(60);
    const LONG: Duration = Duration::from_secs(30);

    fn safe() -> Safety {
        Safety {
            leads: true,
            serving: true,
            degraded: false,
            silenced: false,
            whole_view: true,
            peer_suspected: false,
            drawn_silence: false,
            calm_for: COOLDOWN,
        }
    }

    fn status(term: Term, leader: Option<NodeId>) -> ConsensusStatus {
        ConsensusStatus {
            id: 1,
            term,
            leader,
            role: Role::Follower,
            last_log_index: None,
            last_applied: None,
        }
    }

    #[test]
    fn at_the_ratio_is_overloaded_and_just_under_is_not() {
        assert!(overloaded(3_000, &[1_000, 5_000], RATIO, FLOOR));
        assert!(!overloaded(2_999, &[1_000, 5_000], RATIO, FLOOR));
    }

    #[test]
    fn the_lowest_peer_is_the_one_compared_with() {
        assert!(overloaded(9_000, &[8_000, 2_000], RATIO, FLOOR));
        assert!(!overloaded(9_000, &[8_000, 4_000], RATIO, FLOOR));
    }

    #[test]
    fn even_scores_are_not_overloaded() {
        assert!(!overloaded(2_500, &[2_500, 2_500], RATIO, FLOOR));
    }

    #[test]
    fn an_idle_cluster_with_tiny_numbers_never_fires() {
        // A hundred times the lowest peer, and still nothing: 1 ms of expected wait is not overload.
        assert!(!overloaded(1_000, &[10, 10], RATIO, FLOOR));
        assert!(!overloaded(100, &[1, 1], RATIO, FLOOR));
        assert!(!overloaded(1, &[0, 0], RATIO, FLOOR));
        assert!(overloaded(1_001, &[10, 10], RATIO, FLOOR));
    }

    #[test]
    fn ratio_0_is_off_and_no_peers_means_no_comparison() {
        assert!(!overloaded(u64::MAX, &[1, 1], 0, FLOOR));
        assert!(!overloaded(u64::MAX, &[], RATIO, FLOOR));
    }

    #[test]
    fn huge_scores_saturate_instead_of_overflowing() {
        assert!(!overloaded(u64::MAX - 1, &[u64::MAX], RATIO, FLOOR));
        assert!(overloaded(u64::MAX, &[u64::MAX], RATIO, FLOOR));
    }

    fn log(write_us: u64, save_us: u64) -> LogReading {
        LogReading {
            write_us,
            wait_us: 0,
            save_us,
        }
    }

    #[test]
    fn a_slow_log_counts_only_over_its_bar_and_only_for_a_score_that_is_a_time() {
        let floor = WRITE_FLOOR.as_micros() as u64;
        // A healthy log: the bid alone decides.
        let healthy = log(floor, 2_800);
        assert_eq!(leader_load(2_000, healthy, ScoreName::ExpectedWait), 2_000);
        // A slow log on a leader whose bids look fine.
        let slow = log(floor + 1, 2_800);
        assert_eq!(leader_load(2_000, slow, ScoreName::ExpectedWait), floor + 1);
        assert_eq!(
            leader_load(2_000, slow, ScoreName::ExpectedWaitCpu),
            floor + 1
        );
        // The higher of the two.
        assert_eq!(
            leader_load(9 * floor, slow, ScoreName::ExpectedWait),
            9 * floor
        );
        // Jobs and permille are not microseconds.
        assert_eq!(leader_load(5, slow, ScoreName::QueueOnly), 5);
        assert_eq!(leader_load(700, slow, ScoreName::CpuOnly), 700);
    }

    /// The numbers measured on one PC, a slow disk included (D228).
    #[test]
    fn a_slow_disk_is_not_a_slow_log_and_a_starved_leader_is() {
        // 300 users, then 3000 users: under the floor.
        assert!(!log(9_245, 4_233).is_slow());
        assert!(!log(16_491, 5_996).is_slow());
        // 25 ms added to each save: over the floor, and about 2 saves, at most 2.21.
        assert!(!log(62_698, 30_400).is_slow());
        assert!(!log(69_014, 31_400).is_slow());
        // The six seconds that moved a leader starved of CPU.
        for (write_us, save_us) in [
            (21_781, 6_919),
            (26_174, 9_881),
            (42_429, 12_398),
            (25_935, 8_474),
            (47_974, 9_393),
            (29_004, 8_375),
        ] {
            assert!(log(write_us, save_us).is_slow(), "{write_us} {save_us}");
        }
        // No disk saves at all: the floor alone.
        assert!(!log(20_000, 0).is_slow());
        assert!(log(20_001, 0).is_slow());
    }

    /// What moved the leader every cooldown on an even cluster (D228): 3000 users on a slow disk,
    /// groups full, 3 s in the queue, and every write the usual two saves long.
    #[test]
    fn a_full_queue_with_writes_of_the_usual_length_is_not_a_slow_log() {
        let before = LogWork::default();
        let now = LogWork {
            answered: 3_400,
            answered_us: 3_400 * 3_000_000,
            writes: 17,
            writes_done: 17,
            write_us: 17 * 60_000,
            saves: 34,
            save_us: 34 * 29_000,
        };
        let reading = LogReading::between(before, now, Duration::from_secs(1));
        assert_eq!(reading.wait_us, 3_000_000);
        assert_eq!(reading.write_us, 60_000);
        assert!(!reading.is_slow());
        let load = leader_load(5_000, reading, ScoreName::ExpectedWait);
        assert_eq!(load, 5_000);
        assert!(!overloaded(load, &[4_300, 4_400], RATIO, FLOOR));
    }

    /// This is what the write time is for: bids even, and a leader that cannot keep the log.
    #[test]
    fn a_slow_log_is_overloaded_against_idle_peers_but_not_against_busy_ones() {
        let load = leader_load(2_000, log(300_000, 9_000), ScoreName::ExpectedWait);
        assert!(overloaded(load, &[2_000, 2_500], RATIO, FLOOR));
        assert!(!overloaded(2_000, &[2_000, 2_500], RATIO, FLOOR));
        // Everyone is overloaded, so moving the leader would help nobody.
        assert!(!overloaded(load, &[200_000, 150_000], RATIO, FLOOR));
    }

    #[test]
    fn a_log_reading_is_of_this_second_only() {
        let at = |answered, answered_us, writes, writes_done, write_us, saves, save_us| LogWork {
            answered,
            answered_us,
            writes,
            writes_done,
            write_us,
            saves,
            save_us,
        };
        let over = Duration::from_secs(1);
        let before = at(10, 500, 3, 3, 90, 4, 80);
        let reading = |write_us, wait_us, save_us| LogReading {
            write_us,
            wait_us,
            save_us,
        };
        assert_eq!(
            LogReading::between(before, at(14, 900, 5, 5, 130, 6, 180), over),
            reading(20, 100, 50)
        );
        // Nothing asked for.
        assert_eq!(LogReading::between(before, before, over), reading(0, 0, 0));
        // A write just started, and nothing finished: that says nothing yet.
        assert_eq!(
            LogReading::between(before, at(10, 500, 4, 3, 90, 4, 80), over),
            reading(0, 0, 0)
        );
        // In flight the whole reading, and not finished: at least all of that time.
        let in_flight = at(10, 500, 4, 3, 90, 4, 80);
        assert_eq!(
            LogReading::between(in_flight, in_flight, over),
            reading(1_000_000, 0, 0)
        );
    }

    #[test]
    fn each_score_kind_has_a_floor_in_its_own_unit() {
        assert_eq!(floor(ScoreName::ExpectedWait), 1_000);
        assert_eq!(floor(ScoreName::ExpectedWaitCpu), 1_000);
        assert_eq!(floor(ScoreName::QueueOnly), 8);
        assert_eq!(floor(ScoreName::CpuOnly), 500);
    }

    #[test]
    fn a_reading_needs_this_node_and_every_peer() {
        let scores = BTreeMap::from([(1, 900), (2, 100), (3, 200)]);
        let (mine, peers) = split_scores(1, &[2, 3], &scores).unwrap();
        assert_eq!(mine, 900);
        assert_eq!(peers, BTreeMap::from([(2, 100), (3, 200)]));

        // A peer that is catching up does not bid.
        let one_missing = BTreeMap::from([(1, 900), (2, 100)]);
        assert_eq!(split_scores(1, &[2, 3], &one_missing), None);
        // Nor does a node with no requests.
        let own_missing = BTreeMap::from([(2, 100), (3, 200)]);
        assert_eq!(split_scores(1, &[2, 3], &own_missing), None);
    }

    /// A peer that has stopped bidding has no old number left to decide anything with.
    #[tokio::test]
    async fn a_peer_that_stopped_bidding_is_gone_from_the_next_reading() {
        let ctx = context(
            ScriptedConsensus::new(1),
            EventLog::disabled("node-1"),
            |_| {},
        );
        ctx.bids.saw_bid(1, 900);
        ctx.bids.saw_bid(2, 100);
        ctx.bids.saw_bid(2, 300);
        ctx.bids.saw_bid(3, 200);
        let first = ctx.bids.take_mean_scores();
        assert_eq!(first, BTreeMap::from([(1, 900), (2, 200), (3, 200)]));

        ctx.bids.saw_bid(1, 900);
        ctx.bids.saw_bid(2, 100);
        let second = ctx.bids.take_mean_scores();
        assert_eq!(split_scores(1, &[2, 3], &second), None);
        assert!(ctx.bids.take_mean_scores().is_empty());
    }

    #[test]
    fn every_safety_condition_can_stop_it() {
        assert!(safe().holds(COOLDOWN));
        let unsafe_ones = [
            Safety {
                leads: false,
                ..safe()
            },
            Safety {
                serving: false,
                ..safe()
            },
            Safety {
                degraded: true,
                ..safe()
            },
            Safety {
                silenced: true,
                ..safe()
            },
            Safety {
                whole_view: false,
                ..safe()
            },
            Safety {
                peer_suspected: true,
                ..safe()
            },
            Safety {
                drawn_silence: true,
                ..safe()
            },
            Safety {
                calm_for: COOLDOWN - Duration::from_millis(1),
                ..safe()
            },
        ];
        for one in unsafe_ones {
            assert!(!one.holds(COOLDOWN), "{one:?}");
        }
    }

    #[test]
    fn it_must_hold_without_a_break_for_the_whole_time() {
        let t0 = Instant::now();
        let wanted = Duration::from_secs(5);
        let at = |s| t0 + Duration::from_secs(s);
        let mut in_a_row = InARow::default();
        assert!(!in_a_row.observe(true, wanted, at(0)));
        assert!(!in_a_row.observe(true, wanted, at(4)));
        // A break starts it over.
        assert!(!in_a_row.observe(false, wanted, at(5)));
        assert!(!in_a_row.observe(true, wanted, at(6)));
        assert!(!in_a_row.observe(true, wanted, at(10)));
        assert!(in_a_row.observe(true, wanted, at(11)));
    }

    #[test]
    fn a_status_says_how_a_try_ended() {
        // Still leading in the same term.
        assert_eq!(how_it_ended(1, 5, &status(5, Some(1))), None);
        // A vote is running.
        assert_eq!(how_it_ended(1, 5, &status(6, None)), None);
        assert_eq!(how_it_ended(1, 5, &status(5, None)), None);
        assert_eq!(
            how_it_ended(1, 5, &status(6, Some(2))),
            Some(Ended::Changed)
        );
        assert_eq!(
            how_it_ended(1, 5, &status(7, Some(1))),
            Some(Ended::ReElected)
        );
    }

    /// A leader in term 1 on a scripted consensus, with `step_aside` running.
    fn quiet_leader(
        test: &str,
        limit: Duration,
    ) -> (
        Arc<ScriptedConsensus>,
        Arc<NodeContext>,
        std::path::PathBuf,
        JoinHandle<Ended>,
    ) {
        let dir = log_dir(test);
        let fake = ScriptedConsensus::new(1);
        fake.set_leader(Some(1));
        let log = EventLog::to_dir(&dir, "node-1").unwrap();
        let ctx = context(fake.clone(), log, |_| {});
        let task = {
            let ctx = ctx.clone();
            tokio::spawn(async move { step_aside(&ctx, 1, limit).await })
        };
        (fake, ctx, dir, task)
    }

    fn last_ended(ctx: &NodeContext, dir: &std::path::Path) -> serde_json::Value {
        logged(&ctx.log, dir)
            .into_iter()
            .rfind(|e| e["ev"] == "leader_step_aside_ended")
            .expect("logged")
    }

    #[tokio::test]
    async fn a_new_leader_ends_the_quiet_as_soon_as_this_node_steps_down() {
        let (fake, ctx, dir, task) = quiet_leader("step-aside-changed", LONG);
        until("the quiet", || fake.is_quiet()).await;

        // A vote is running: the old leader has stepped down, and nobody has won yet.
        fake.set_term(2);
        fake.set_leader(None);
        until("the quiet to end", || !fake.is_quiet()).await;
        assert!(!task.is_finished());

        fake.set_leader(Some(2));
        assert_eq!(task.await.unwrap(), Ended::Changed);
        assert!(!fake.is_quiet());
        let event = last_ended(&ctx, &dir);
        assert_eq!(event["term"], 1);
        assert_eq!(event["outcome"], "changed");
        assert_eq!(event["new_term"], 2);
        assert_eq!(event["new_leader"], 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn the_same_node_winning_the_vote_is_said_so() {
        let (fake, ctx, dir, task) = quiet_leader("step-aside-again", LONG);
        until("the quiet", || fake.is_quiet()).await;
        fake.set_term(3);
        assert_eq!(task.await.unwrap(), Ended::ReElected);
        assert!(!fake.is_quiet());
        assert_eq!(last_ended(&ctx, &dir)["outcome"], "re_elected");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn no_new_leader_in_time_ends_the_quiet_and_this_node_carries_on() {
        let limit = Duration::from_millis(100);
        let (fake, ctx, dir, task) = quiet_leader("step-aside-timeout", limit);
        assert_eq!(task.await.unwrap(), Ended::TimedOut);
        assert!(!fake.is_quiet());
        let event = last_ended(&ctx, &dir);
        assert_eq!(event["outcome"], "timed_out");
        assert_eq!(event["new_term"], 1);
        assert_eq!(event["new_leader"], 1);
        assert!(event["waited_ms"].as_u64().unwrap() >= 100);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn being_silenced_stops_the_try_so_this_node_returns_with_heartbeats_on() {
        let (fake, ctx, dir, task) = quiet_leader("step-aside-silenced", LONG);
        until("the quiet", || fake.is_quiet()).await;
        ctx.gate.silence();
        assert_eq!(task.await.unwrap(), Ended::Stopped);
        assert!(!fake.is_quiet());
        assert_eq!(last_ended(&ctx, &dir)["outcome"], "stopped");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn off_or_under_leader_dispatch_nothing_is_started() {
        let log = EventLog::disabled("node-1");
        let on = context(ScriptedConsensus::new(1), log.clone(), |_| {});
        let task = spawn(&on).expect("on by default");
        task.abort();

        let off = context(ScriptedConsensus::new(1), log.clone(), |c| {
            c.policy.leader_overload_ratio = 0;
        });
        assert!(spawn(&off).is_none());

        let dispatch = context(ScriptedConsensus::new(1), log, |c| {
            c.policy.assignment = AssignmentPolicy::LeaderDispatch;
        });
        assert!(spawn(&dispatch).is_none());
    }

    /// A leader whose bids look fine and whose log does not: the same loop, on the log wait alone.
    #[tokio::test]
    async fn the_loop_steps_aside_on_a_slow_log_too() {
        let dir = log_dir("step-aside-slow-log");
        let fake = ScriptedConsensus::new(1);
        fake.set_leader(Some(1));
        let log = EventLog::to_dir(&dir, "node-1").unwrap();
        let ctx = context(fake.clone(), log, |c| {
            c.policy.leader_overload_for_ms = 1;
            c.policy.leader_overload_cooldown_ms = 3_000;
        });
        let task = spawn(&ctx).unwrap();
        let busy = {
            let (ctx, fake) = (ctx.clone(), fake.clone());
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_millis(20));
                loop {
                    tick.tick().await;
                    ctx.bids.saw_bid(1, 2_000);
                    ctx.bids.saw_bid(2, 2_000);
                    ctx.bids.saw_bid(3, 2_000);
                    fake.add_writes(1, 300_000);
                    fake.add_answers(2, 450_000);
                    fake.add_saves(2, 3_000);
                }
            })
        };

        until("the quiet", || fake.is_quiet()).await;
        fake.set_term(2);
        fake.set_leader(Some(2));
        until("the quiet to end", || !fake.is_quiet()).await;
        busy.abort();
        task.abort();

        let events = logged(&ctx.log, &dir);
        let aside = events
            .iter()
            .find(|e| e["ev"] == "leader_stepped_aside")
            .expect("logged");
        assert_eq!(aside["score"], 2_000);
        assert_eq!(aside["write_us"], 300_000);
        assert_eq!(aside["log_wait_us"], 450_000);
        assert_eq!(aside["save_us"], 3_000);
        assert_eq!(aside["load"], 300_000);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The whole loop, on a leader whose bids say it is overloaded: it goes quiet once, and logs the
    /// mean scores of the second that decided it.
    #[tokio::test]
    async fn the_loop_steps_aside_after_the_time_in_a_row_and_logs_the_scores() {
        let dir = log_dir("step-aside-loop");
        let fake = ScriptedConsensus::new(1);
        fake.set_leader(Some(1));
        let log = EventLog::to_dir(&dir, "node-1").unwrap();
        let ctx = context(fake.clone(), log, |c| {
            c.policy.leader_overload_for_ms = 1;
            c.policy.leader_overload_cooldown_ms = 3_000;
        });
        let task = spawn(&ctx).unwrap();
        let bidding = {
            let ctx = ctx.clone();
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_millis(20));
                loop {
                    tick.tick().await;
                    ctx.bids.saw_bid(1, 9_000);
                    ctx.bids.saw_bid(2, 1_000);
                    ctx.bids.saw_bid(3, 2_000);
                }
            })
        };

        until("the quiet", || fake.is_quiet()).await;
        fake.set_term(2);
        fake.set_leader(Some(2));
        until("the quiet to end", || !fake.is_quiet()).await;
        bidding.abort();
        task.abort();

        let events = logged(&ctx.log, &dir);
        let aside: Vec<_> = events
            .iter()
            .filter(|e| e["ev"] == "leader_stepped_aside")
            .collect();
        assert_eq!(aside.len(), 1, "{aside:?}");
        assert_eq!(aside[0]["term"], 1);
        assert_eq!(aside[0]["score"], 9_000);
        assert_eq!(aside[0]["load"], 9_000);
        assert_eq!(aside[0]["write_us"], 0);
        assert_eq!(aside[0]["peer_scores"]["2"], 1_000);
        assert_eq!(aside[0]["peer_scores"]["3"], 2_000);
        let _ = std::fs::remove_dir_all(dir);
    }
}
