//! Load-priority bidding election: picks the node that plans each request.
//!
//! Every serving node broadcasts a load score for the request; the lowest score wins and ties go
//! to the lower node ID. If the winner goes silent, the next-lowest bidder takes over.
//!
//! Pure logic: no network, no clock. `node/` drives rounds by feeding in bids and timer events,
//! and passes in every time.
//!
//! Owner: B (bidding election). `node/bids.rs` drives it.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::proto::{NodeId, RequestId};

/// Inputs to one node's load score, sampled when the bid is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LoadInputs {
    /// Planner jobs waiting for a worker thread.
    pub queued: usize,
    /// Planner jobs running now.
    pub running: usize,
    /// This node's own count from [`BidRounds::just_won`]; never a count about its peers.
    pub just_won: usize,
    /// Recent mean planning time per job, in microseconds. See [`ServiceTime`].
    pub recent_service_us: u64,
    /// Worker threads on this node.
    pub workers: usize,
    /// CPU busy fraction in permille (0..=1000).
    pub cpu_permille: u32,
}

/// Which load score a node bids with. `ExpectedWait` is the starting choice; experiment E6
/// compares all four.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScoreVariant {
    /// `(queued + running + just_won + 1) × recent_service_us ÷ workers`: microseconds of
    /// expected wait.
    ExpectedWait,
    /// `ExpectedWait` plus `cpu_weight × cpu_permille`.
    ExpectedWaitCpu { cpu_weight: u64 },
    /// `queued + running + just_won`: a count of jobs.
    QueueOnly,
    /// `cpu_permille`.
    CpuOnly,
}

/// The score a node bids with. Lower means less loaded. Saturates instead of overflowing.
pub fn load_score(inputs: &LoadInputs, variant: ScoreVariant) -> u64 {
    match variant {
        ScoreVariant::ExpectedWait => expected_wait_us(inputs),
        ScoreVariant::ExpectedWaitCpu { cpu_weight } => expected_wait_us(inputs)
            .saturating_add(cpu_weight.saturating_mul(u64::from(inputs.cpu_permille))),
        ScoreVariant::QueueOnly => jobs(inputs) as u64,
        ScoreVariant::CpuOnly => u64::from(inputs.cpu_permille),
    }
}

/// The `+ 1` counts the request being bid on, so an idle node scores its own service time and a
/// fast idle node beats a slow one.
fn expected_wait_us(inputs: &LoadInputs) -> u64 {
    let in_flight = jobs(inputs).saturating_add(1) as u64;
    let workers = inputs.workers.max(1) as u64;
    in_flight.saturating_mul(inputs.recent_service_us) / workers
}

fn jobs(inputs: &LoadInputs) -> usize {
    inputs
        .queued
        .saturating_add(inputs.running)
        .saturating_add(inputs.just_won)
}

/// The winning bidder: lowest score, ties to the lower node ID. `None` if nobody bid.
pub fn pick_winner(bids: &[(NodeId, u64)]) -> Option<NodeId> {
    bids.iter()
        .min_by_key(|&&(node, score)| (score, node))
        .map(|&(node, _)| node)
}

/// Every bidder ordered by (score, node ID): the winner first, then the takeover order.
/// A node listed twice keeps only its better bid.
pub fn ranking(bids: &[(NodeId, u64)]) -> Vec<NodeId> {
    let mut sorted = bids.to_vec();
    sorted.sort_unstable_by_key(|&(node, score)| (score, node));
    let mut seen = BTreeSet::new();
    sorted
        .into_iter()
        .map(|(node, _)| node)
        .filter(|node| seen.insert(*node))
        .collect()
}

/// The node that should plan the request now: the best-ranked bidder not suspected of being
/// silent. Takeover is this with the silent winner suspected; the caller only moves the request
/// while nothing is committed for it. `None` if every bidder is suspected.
pub fn executor(ranking: &[NodeId], mut suspected: impl FnMut(NodeId) -> bool) -> Option<NodeId> {
    ranking.iter().copied().find(|&node| !suspected(node))
}

/// Recent planning time: a running average that moves 1/8 of the way towards each new job, the
/// gain of TCP's round-trip estimator (Jacobson 1988). Shifting truncates, so the average settles
/// within 7 µs of a steady input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceTime {
    average_us: u64,
    has_sample: bool,
}

impl ServiceTime {
    /// Starts at `initial_us`, so a node that has not planned yet still scores above zero and its
    /// queue length still counts.
    pub fn new(initial_us: u64) -> Self {
        Self {
            average_us: initial_us,
            has_sample: false,
        }
    }

    /// Adds one job's planning time. The first job replaces the starting value.
    pub fn record(&mut self, sample_us: u64) {
        if !self.has_sample {
            self.has_sample = true;
            self.average_us = sample_us;
        } else if sample_us >= self.average_us {
            self.average_us += (sample_us - self.average_us) >> 3;
        } else {
            self.average_us -= (self.average_us - sample_us) >> 3;
        }
    }

    pub fn average_us(self) -> u64 {
        self.average_us
    }
}

/// One bid round: a request and the client's attempt number. A retry gets a fresh round, so the
/// bids of a silent node's old attempt never mix with the new one.
pub type RoundKey = (RequestId, u32);

/// What a node's own `just_won` count includes. The burst test compares the three.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum JustWonMode {
    /// Always zero.
    Off,
    /// Rounds this node won whose jobs have not reached the worker pool.
    WinsOnly,
    /// `WinsOnly` plus open rounds this node is leading. In a burst, rounds overlap: without these
    /// a node would look idle to every round that opens before its first win closes.
    #[default]
    WonAndLeading,
}

/// What happened to a bid offered to a round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BidStatus {
    Accepted,
    /// The round has closed, so the bid is ignored.
    Late,
    /// This node already bid in this round; its first bid stands.
    Duplicate,
}

/// A round that has just closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedRound {
    pub key: RoundKey,
    /// Winner first, then the takeover order. Empty if nobody bid.
    pub ranking: Vec<NodeId>,
    pub i_won: bool,
    pub bids_seen: usize,
    /// This node's own bid, if it made one.
    pub my_score: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// A peer's bid arrived before the request; the window has not started.
    AwaitingRequest {
        first_bid: Instant,
    },
    Open {
        deadline: Instant,
    },
    Closed {
        at: Instant,
    },
}

#[derive(Debug, Clone)]
struct Round {
    phase: Phase,
    bids: BTreeMap<NodeId, u64>,
}

impl Round {
    /// Whether this node holds the best bid so far. False until it has bid itself.
    fn led_by(&self, node: NodeId) -> bool {
        let Some(&mine) = self.bids.get(&node) else {
            return false;
        };
        self.bids
            .iter()
            .all(|(&other, &score)| (mine, node) <= (score, other))
    }
}

/// Every bid round one node knows about, and its `just_won` count.
///
/// A window runs from when the request reaches this node until `bid_window` later, by this node's
/// own clock. A closed round is kept for a while so that late bids do nothing, then forgotten with
/// [`BidRounds::forget_older_than`]; the caller picks that grace period.
#[derive(Debug, Clone)]
pub struct BidRounds {
    me: NodeId,
    bid_window: Duration,
    rounds: BTreeMap<RoundKey, Round>,
    /// The rounds with a running window, so counting leading rounds stays cheap however many
    /// closed rounds are being kept.
    open: BTreeSet<RoundKey>,
    /// Rounds this node won whose jobs have not reached the worker pool. A set, not a counter, so
    /// settling the same round twice cannot undercount.
    won: BTreeSet<RoundKey>,
}

impl BidRounds {
    pub fn new(me: NodeId, bid_window: Duration) -> Self {
        Self {
            me,
            bid_window,
            rounds: BTreeMap::new(),
            open: BTreeSet::new(),
            won: BTreeSet::new(),
        }
    }

    /// The request reached this node at `now`: start its window. Bids that came early are kept.
    /// A repeat of the same attempt changes nothing. Returns the deadline if a window is running.
    pub fn open(&mut self, key: RoundKey, now: Instant) -> Option<Instant> {
        let round = self.rounds.entry(key).or_insert_with(|| Round {
            phase: Phase::AwaitingRequest { first_bid: now },
            bids: BTreeMap::new(),
        });
        match round.phase {
            Phase::AwaitingRequest { .. } => {
                let deadline = now + self.bid_window;
                round.phase = Phase::Open { deadline };
                self.open.insert(key);
                Some(deadline)
            }
            Phase::Open { deadline } => Some(deadline),
            Phase::Closed { .. } => None,
        }
    }

    /// Offers a bid, from a peer or from this node. A bid for an unknown round starts one that
    /// waits for the request. A bid at or after the deadline is late, even if the round has not
    /// been closed yet, so the result does not depend on when the timer task runs.
    pub fn add_bid(&mut self, key: RoundKey, node: NodeId, score: u64, now: Instant) -> BidStatus {
        let round = self.rounds.entry(key).or_insert_with(|| Round {
            phase: Phase::AwaitingRequest { first_bid: now },
            bids: BTreeMap::new(),
        });
        let late = match round.phase {
            Phase::AwaitingRequest { .. } => false,
            Phase::Open { deadline } => now >= deadline,
            Phase::Closed { .. } => true,
        };
        if late {
            return BidStatus::Late;
        }
        if round.bids.contains_key(&node) {
            return BidStatus::Duplicate;
        }
        round.bids.insert(node, score);
        BidStatus::Accepted
    }

    /// Closes one round whose window has ended and reports it, counting this node's win. `None`
    /// if the round is unknown, already closed, or its window is still running.
    pub fn close(&mut self, key: RoundKey, now: Instant) -> Option<ClosedRound> {
        let round = self.rounds.get_mut(&key)?;
        if !matches!(round.phase, Phase::Open { deadline } if now >= deadline) {
            return None;
        }
        round.phase = Phase::Closed { at: now };
        self.open.remove(&key);
        let bids: Vec<(NodeId, u64)> = round.bids.iter().map(|(&n, &s)| (n, s)).collect();
        let order = ranking(&bids);
        let i_won = order.first() == Some(&self.me);
        if i_won {
            self.won.insert(key);
        }
        Some(ClosedRound {
            key,
            ranking: order,
            i_won,
            bids_seen: bids.len(),
            my_score: round.bids.get(&self.me).copied(),
        })
    }

    /// A closed round's bids in ranking order: the winner and its bid first, then the takeover
    /// order. `None` unless the round has closed and has not been forgotten.
    pub fn ranked_bids(&self, key: RoundKey) -> Option<Vec<(NodeId, u64)>> {
        let round = self.rounds.get(&key)?;
        if !matches!(round.phase, Phase::Closed { .. }) {
            return None;
        }
        let mut bids: Vec<(NodeId, u64)> = round.bids.iter().map(|(&n, &s)| (n, s)).collect();
        bids.sort_unstable_by_key(|&(node, score)| (score, node));
        Some(bids)
    }

    /// The winner's job reached the worker pool (or was refused), where the pool's own load
    /// figures take over from `just_won`.
    pub fn job_queued(&mut self, key: RoundKey) {
        self.won.remove(&key);
    }

    /// A result for this request was committed, whichever node planned it. Every attempt stops
    /// counting towards `just_won`, and any window still running ends without a winner.
    pub fn request_decided(&mut self, request: RequestId, now: Instant) {
        let attempts = (request, u32::MIN)..=(request, u32::MAX);
        let keys: Vec<RoundKey> = self.rounds.range(attempts).map(|(&key, _)| key).collect();
        for key in keys {
            self.won.remove(&key);
            if self.open.remove(&key) {
                if let Some(round) = self.rounds.get_mut(&key) {
                    round.phase = Phase::Closed { at: now };
                }
            }
        }
    }

    /// This node's own count for its next bid. Read it before adding the bid, so a round does not
    /// count itself.
    pub fn just_won(&self, mode: JustWonMode) -> usize {
        match mode {
            JustWonMode::Off => 0,
            JustWonMode::WinsOnly => self.won.len(),
            JustWonMode::WonAndLeading => self.won.len() + self.leading(),
        }
    }

    fn leading(&self) -> usize {
        self.open
            .iter()
            .filter_map(|key| self.rounds.get(key))
            .filter(|round| round.led_by(self.me))
            .count()
    }

    /// Drops rounds closed at least `grace` ago, and rounds whose request has not arrived that
    /// long after their first bid. A dropped round's win stops counting, so `just_won` cannot
    /// stay raised by a job the caller never reported.
    pub fn forget_older_than(&mut self, now: Instant, grace: Duration) {
        let won = &mut self.won;
        self.rounds.retain(|key, round| {
            let since = match round.phase {
                Phase::AwaitingRequest { first_bid } => first_bid,
                Phase::Closed { at } => at,
                Phase::Open { .. } => return true,
            };
            let keep = now.saturating_duration_since(since) < grace;
            if !keep {
                won.remove(key);
            }
            keep
        });
    }

    /// Rounds being tracked, in any phase.
    pub fn len(&self) -> usize {
        self.rounds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rounds.is_empty()
    }
}

// TODO(B): takeover: the next bidder in `ranking` takes over after
// `timers.takeover_missed_heartbeats` missed heartbeats with nothing committed for the request.

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Duration = Duration::from_millis(20);
    const GRACE: Duration = Duration::from_secs(2);
    const ME: NodeId = 1;

    fn inputs(queued: usize, running: usize, just_won: usize, service_us: u64) -> LoadInputs {
        LoadInputs {
            queued,
            running,
            just_won,
            recent_service_us: service_us,
            workers: 4,
            cpu_permille: 0,
        }
    }

    fn key(seq: u64) -> RoundKey {
        (RequestId { client: 1, seq }, 1)
    }

    fn rounds() -> BidRounds {
        BidRounds::new(ME, WINDOW)
    }

    /// Closes every round that is due, as each request's own task would.
    fn close_all(r: &mut BidRounds, now: Instant) -> Vec<ClosedRound> {
        let keys: Vec<RoundKey> = r.rounds.keys().copied().collect();
        keys.into_iter().filter_map(|k| r.close(k, now)).collect()
    }

    // --- load_score ---

    #[test]
    fn expected_wait_matches_formula() {
        // (2 + 1 + 0 + 1) × 10_000 / 4.
        assert_eq!(
            load_score(&inputs(2, 1, 0, 10_000), ScoreVariant::ExpectedWait),
            10_000
        );
    }

    #[test]
    fn expected_wait_ignores_cpu() {
        let i = LoadInputs {
            cpu_permille: 1000,
            ..inputs(2, 1, 0, 10_000)
        };
        assert_eq!(load_score(&i, ScoreVariant::ExpectedWait), 10_000);
    }

    #[test]
    fn expected_wait_cpu_adds_weighted_cpu() {
        let i = LoadInputs {
            cpu_permille: 500,
            ..inputs(2, 1, 0, 10_000)
        };
        let variant = ScoreVariant::ExpectedWaitCpu { cpu_weight: 10 };
        // 10_000 + 10 × 500.
        assert_eq!(load_score(&i, variant), 15_000);
    }

    #[test]
    fn queue_only_counts_jobs() {
        assert_eq!(
            load_score(&inputs(2, 1, 3, 10_000), ScoreVariant::QueueOnly),
            6
        );
    }

    #[test]
    fn cpu_only_is_the_cpu_reading() {
        let i = LoadInputs {
            cpu_permille: 730,
            ..inputs(9, 9, 9, 10_000)
        };
        assert_eq!(load_score(&i, ScoreVariant::CpuOnly), 730);
    }

    #[test]
    fn idle_slow_node_loses_to_idle_fast_node() {
        let slow = load_score(&inputs(0, 0, 0, 40_000), ScoreVariant::ExpectedWait);
        let fast = load_score(&inputs(0, 0, 0, 8_000), ScoreVariant::ExpectedWait);
        assert_eq!(slow, 10_000);
        assert!(fast < slow);
    }

    #[test]
    fn just_won_raises_the_score_like_queued_work() {
        let score = |i: LoadInputs| load_score(&i, ScoreVariant::ExpectedWait);
        assert_eq!(
            score(inputs(0, 0, 1, 40_000)),
            score(inputs(1, 0, 0, 40_000))
        );
        assert!(score(inputs(0, 0, 1, 40_000)) > score(inputs(0, 0, 0, 40_000)));
    }

    #[test]
    fn zero_workers_counts_as_one() {
        let i = LoadInputs {
            workers: 0,
            ..inputs(0, 0, 0, 5_000)
        };
        assert_eq!(load_score(&i, ScoreVariant::ExpectedWait), 5_000);
    }

    #[test]
    fn huge_inputs_saturate_instead_of_panicking() {
        let i = LoadInputs {
            queued: usize::MAX,
            running: usize::MAX,
            just_won: usize::MAX,
            recent_service_us: u64::MAX,
            workers: 1,
            cpu_permille: u32::MAX,
        };
        let cpu = ScoreVariant::ExpectedWaitCpu {
            cpu_weight: u64::MAX,
        };
        assert_eq!(load_score(&i, ScoreVariant::ExpectedWait), u64::MAX);
        assert_eq!(load_score(&i, cpu), u64::MAX);
        assert_eq!(load_score(&i, ScoreVariant::QueueOnly), usize::MAX as u64);
    }

    // --- pick_winner and ranking ---

    #[test]
    fn lowest_score_wins() {
        assert_eq!(pick_winner(&[(1, 50), (2, 30), (3, 40)]), Some(2));
    }

    #[test]
    fn ties_go_to_lower_node_id() {
        assert_eq!(pick_winner(&[(3, 10), (1, 10), (2, 10)]), Some(1));
    }

    #[test]
    fn nobody_bid_means_no_winner() {
        assert_eq!(pick_winner(&[]), None);
        assert!(ranking(&[]).is_empty());
    }

    #[test]
    fn ranking_orders_by_score_then_node_id() {
        assert_eq!(ranking(&[(3, 20), (2, 10), (1, 20)]), vec![2, 1, 3]);
    }

    #[test]
    fn ranking_starts_with_the_winner() {
        let bids = [(1, 7), (2, 3), (3, 3)];
        assert_eq!(ranking(&bids).first().copied(), pick_winner(&bids));
    }

    #[test]
    fn ranking_lists_each_node_once() {
        assert_eq!(ranking(&[(1, 50), (2, 30), (1, 10)]), vec![1, 2]);
    }

    #[test]
    fn input_order_never_changes_the_result() {
        let a = [(1, 5), (2, 5), (3, 1)];
        let b = [(3, 1), (2, 5), (1, 5)];
        assert_eq!(ranking(&a), ranking(&b));
        assert_eq!(pick_winner(&a), pick_winner(&b));
    }

    #[test]
    fn executor_is_the_winner_when_nobody_is_suspected() {
        assert_eq!(executor(&[2, 1, 3], |_| false), Some(2));
    }

    #[test]
    fn executor_skips_suspected_nodes() {
        assert_eq!(executor(&[2, 1, 3], |n| n == 2), Some(1));
        assert_eq!(executor(&[2, 1, 3], |n| n != 3), Some(3));
        assert_eq!(executor(&[2, 1, 3], |_| true), None);
    }

    // --- ServiceTime ---

    #[test]
    fn service_time_starts_at_the_initial_value() {
        assert_eq!(ServiceTime::new(1000).average_us(), 1000);
    }

    #[test]
    fn first_job_replaces_the_initial_value() {
        let mut service = ServiceTime::new(1000);
        service.record(40_000);
        assert_eq!(service.average_us(), 40_000);
    }

    #[test]
    fn later_jobs_move_the_average_one_eighth_of_the_way() {
        let mut service = ServiceTime::new(1000);
        service.record(10_000);
        service.record(50_000);
        assert_eq!(service.average_us(), 15_000);
        service.record(7_000);
        assert_eq!(service.average_us(), 14_000);
    }

    #[test]
    fn service_time_converges_to_a_steady_input() {
        for start in [1_000u64, 900_000] {
            let mut service = ServiceTime::new(1000);
            service.record(start);
            for _ in 0..200 {
                service.record(50_000);
            }
            assert!(service.average_us().abs_diff(50_000) <= 7);
        }
    }

    #[test]
    fn extreme_samples_do_not_overflow() {
        let mut service = ServiceTime::new(0);
        service.record(u64::MAX);
        service.record(0);
        service.record(u64::MAX);
        assert!(service.average_us() > 0);
    }

    // --- rounds ---

    #[test]
    fn round_closes_exactly_one_window_after_the_request() {
        let t0 = Instant::now();
        let mut r = rounds();
        assert_eq!(r.open(key(1), t0), Some(t0 + WINDOW));
        assert!(close_all(&mut r, t0 + WINDOW - Duration::from_micros(1)).is_empty());
        assert_eq!(close_all(&mut r, t0 + WINDOW).len(), 1);
        assert!(r.open.is_empty());
    }

    #[test]
    fn closing_needs_a_running_window_that_has_ended() {
        let t0 = Instant::now();
        let mut r = rounds();
        assert_eq!(r.close(key(1), t0), None);
        r.add_bid(key(1), 2, 10, t0);
        assert_eq!(r.close(key(1), t0 + WINDOW), None);
        r.open(key(1), t0);
        assert_eq!(
            r.close(key(1), t0 + WINDOW - Duration::from_micros(1)),
            None
        );
        assert!(r.close(key(1), t0 + WINDOW).is_some());
        assert_eq!(r.close(key(1), t0 + WINDOW), None);
    }

    #[test]
    fn ranked_bids_are_readable_only_after_closing() {
        let t0 = Instant::now();
        let mut r = rounds();
        r.open(key(1), t0);
        r.add_bid(key(1), 1, 30, t0);
        r.add_bid(key(1), 2, 10, t0);
        r.add_bid(key(1), 3, 10, t0);
        assert_eq!(r.ranked_bids(key(1)), None);
        r.close(key(1), t0 + WINDOW);
        assert_eq!(r.ranked_bids(key(1)), Some(vec![(2, 10), (3, 10), (1, 30)]));
        r.forget_older_than(t0 + WINDOW + GRACE, GRACE);
        assert_eq!(r.ranked_bids(key(1)), None);
    }

    #[test]
    fn closing_reports_the_ranking_and_this_nodes_part() {
        let t0 = Instant::now();
        let mut r = rounds();
        r.open(key(1), t0);
        r.add_bid(key(1), 1, 30, t0);
        r.add_bid(key(1), 2, 10, t0);
        r.add_bid(key(1), 3, 20, t0);
        let closed = close_all(&mut r, t0 + WINDOW);
        assert_eq!(
            closed,
            vec![ClosedRound {
                key: key(1),
                ranking: vec![2, 3, 1],
                i_won: false,
                bids_seen: 3,
                my_score: Some(30),
            }]
        );
    }

    #[test]
    fn a_round_nobody_bid_in_has_no_winner() {
        let t0 = Instant::now();
        let mut r = rounds();
        r.open(key(1), t0);
        let closed = close_all(&mut r, t0 + WINDOW);
        assert!(closed[0].ranking.is_empty());
        assert!(!closed[0].i_won);
        assert_eq!(closed[0].my_score, None);
    }

    #[test]
    fn a_bid_before_the_request_is_kept_and_does_not_start_the_window() {
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_millis(5);
        let mut r = rounds();
        assert_eq!(r.add_bid(key(1), 2, 10, t0), BidStatus::Accepted);
        assert!(r.open.is_empty());
        assert!(close_all(&mut r, t0 + WINDOW + WINDOW).is_empty());
        assert_eq!(r.open(key(1), t1), Some(t1 + WINDOW));
        let closed = close_all(&mut r, t1 + WINDOW);
        assert_eq!(closed[0].ranking, vec![2]);
    }

    #[test]
    fn a_bid_at_or_after_the_deadline_is_ignored() {
        let t0 = Instant::now();
        let mut r = rounds();
        r.open(key(1), t0);
        assert_eq!(
            r.add_bid(key(1), 2, 0, t0 + WINDOW - Duration::from_micros(1)),
            BidStatus::Accepted
        );
        // Late by the clock, before the timer has closed the round.
        assert_eq!(r.add_bid(key(1), 3, 0, t0 + WINDOW), BidStatus::Late);
        let closed = close_all(&mut r, t0 + WINDOW);
        assert_eq!(closed[0].ranking, vec![2]);
        // Late after closing.
        assert_eq!(r.add_bid(key(1), 3, 0, t0 + WINDOW), BidStatus::Late);
    }

    #[test]
    fn a_duplicate_bid_keeps_the_first() {
        let t0 = Instant::now();
        let mut r = rounds();
        r.open(key(1), t0);
        assert_eq!(r.add_bid(key(1), 3, 50, t0), BidStatus::Accepted);
        assert_eq!(r.add_bid(key(1), 3, 5, t0), BidStatus::Duplicate);
        assert_eq!(r.add_bid(key(1), 2, 20, t0), BidStatus::Accepted);
        assert_eq!(close_all(&mut r, t0 + WINDOW)[0].ranking, vec![2, 3]);
    }

    #[test]
    fn a_repeated_request_keeps_the_first_window() {
        let t0 = Instant::now();
        let mut r = rounds();
        r.open(key(1), t0);
        assert_eq!(
            r.open(key(1), t0 + Duration::from_millis(5)),
            Some(t0 + WINDOW)
        );
        close_all(&mut r, t0 + WINDOW);
        assert_eq!(r.open(key(1), t0 + WINDOW), None);
    }

    #[test]
    fn attempts_of_one_request_never_mix() {
        let t0 = Instant::now();
        let request = RequestId { client: 1, seq: 7 };
        let (first, retry) = ((request, 1), (request, 2));
        let mut r = rounds();
        r.open(first, t0);
        r.add_bid(first, 2, 10, t0);
        close_all(&mut r, t0 + WINDOW);

        let t1 = t0 + Duration::from_secs(2);
        assert_eq!(r.open(retry, t1), Some(t1 + WINDOW));
        r.add_bid(retry, 3, 40, t1);
        // A straggler from the first attempt stays in the first attempt.
        assert_eq!(r.add_bid(first, 1, 0, t1), BidStatus::Late);
        assert_eq!(close_all(&mut r, t1 + WINDOW)[0].ranking, vec![3]);
    }

    #[test]
    fn forgetting_keeps_rounds_inside_the_grace_period_and_open_rounds() {
        let t0 = Instant::now();
        let mut r = rounds();
        r.open(key(1), t0);
        close_all(&mut r, t0 + WINDOW);
        let closed_at = t0 + WINDOW;

        r.forget_older_than(closed_at + GRACE - Duration::from_micros(1), GRACE);
        assert_eq!(r.len(), 1);
        // A late bid to a kept round does nothing.
        assert_eq!(r.add_bid(key(1), 2, 0, closed_at), BidStatus::Late);

        r.open(key(2), closed_at + GRACE);
        r.forget_older_than(closed_at + GRACE, GRACE);
        assert_eq!(r.len(), 1);
        assert!(!r.open.is_empty());
    }

    #[test]
    fn a_request_that_never_arrives_is_forgotten() {
        let t0 = Instant::now();
        let mut r = rounds();
        r.add_bid(key(1), 2, 10, t0);
        r.forget_older_than(t0 + GRACE, GRACE);
        assert!(r.is_empty());
    }

    // --- just_won ---

    /// Opens a round at `t0` with this node's bid and a worse bid from node 2, so this node wins.
    fn win(r: &mut BidRounds, k: RoundKey, t0: Instant) {
        r.open(k, t0);
        r.add_bid(k, ME, 10, t0);
        r.add_bid(k, 2, 20, t0);
    }

    #[test]
    fn off_is_always_zero() {
        let t0 = Instant::now();
        let mut r = rounds();
        win(&mut r, key(1), t0);
        close_all(&mut r, t0 + WINDOW);
        win(&mut r, key(2), t0 + WINDOW);
        assert_eq!(r.just_won(JustWonMode::Off), 0);
    }

    #[test]
    fn a_win_counts_until_the_job_is_queued() {
        let t0 = Instant::now();
        let mut r = rounds();
        win(&mut r, key(1), t0);
        assert_eq!(r.just_won(JustWonMode::WinsOnly), 0);
        assert!(close_all(&mut r, t0 + WINDOW)[0].i_won);
        assert_eq!(r.just_won(JustWonMode::WinsOnly), 1);
        r.job_queued(key(1));
        assert_eq!(r.just_won(JustWonMode::WinsOnly), 0);
    }

    #[test]
    fn settling_twice_never_goes_below_zero_or_loses_another_win() {
        let t0 = Instant::now();
        let mut r = rounds();
        win(&mut r, key(1), t0);
        win(&mut r, key(2), t0);
        close_all(&mut r, t0 + WINDOW);
        assert_eq!(r.just_won(JustWonMode::WinsOnly), 2);
        r.job_queued(key(1));
        r.job_queued(key(1));
        r.request_decided(key(1).0, t0 + WINDOW);
        assert_eq!(r.just_won(JustWonMode::WinsOnly), 1);
    }

    #[test]
    fn a_lost_round_does_not_count() {
        let t0 = Instant::now();
        let mut r = rounds();
        r.open(key(1), t0);
        r.add_bid(key(1), ME, 30, t0);
        r.add_bid(key(1), 2, 20, t0);
        assert!(!close_all(&mut r, t0 + WINDOW)[0].i_won);
        assert_eq!(r.just_won(JustWonMode::WonAndLeading), 0);
    }

    #[test]
    fn a_win_decided_elsewhere_stops_counting_for_every_attempt() {
        let t0 = Instant::now();
        let request = key(1).0;
        let mut r = rounds();
        win(&mut r, (request, 1), t0);
        win(&mut r, (request, 2), t0);
        win(&mut r, key(2), t0);
        close_all(&mut r, t0 + WINDOW);
        r.request_decided(request, t0 + WINDOW);
        assert_eq!(r.just_won(JustWonMode::WinsOnly), 1);
    }

    #[test]
    fn a_decided_request_ends_its_open_round_without_a_winner() {
        let t0 = Instant::now();
        let mut r = rounds();
        win(&mut r, key(1), t0);
        r.request_decided(key(1).0, t0);
        assert_eq!(r.just_won(JustWonMode::WonAndLeading), 0);
        assert!(r.open.is_empty());
        assert!(close_all(&mut r, t0 + WINDOW).is_empty());
        assert_eq!(r.just_won(JustWonMode::WinsOnly), 0);
    }

    #[test]
    fn leading_rounds_count_only_in_won_and_leading_mode() {
        let t0 = Instant::now();
        let mut r = rounds();
        win(&mut r, key(1), t0);
        assert_eq!(r.just_won(JustWonMode::WinsOnly), 0);
        assert_eq!(r.just_won(JustWonMode::WonAndLeading), 1);
    }

    #[test]
    fn leading_follows_the_bids_seen_so_far() {
        let t0 = Instant::now();
        let mut r = rounds();
        r.open(key(1), t0);
        // No own bid yet: the round does not count itself.
        assert_eq!(r.just_won(JustWonMode::WonAndLeading), 0);
        r.add_bid(key(1), ME, 20, t0);
        assert_eq!(r.just_won(JustWonMode::WonAndLeading), 1);
        r.add_bid(key(1), 2, 10, t0);
        assert_eq!(r.just_won(JustWonMode::WonAndLeading), 0);
    }

    #[test]
    fn a_tie_is_led_by_the_lower_node_id() {
        let t0 = Instant::now();
        let mut low = BidRounds::new(1, WINDOW);
        low.open(key(1), t0);
        low.add_bid(key(1), 1, 10, t0);
        low.add_bid(key(1), 2, 10, t0);
        assert_eq!(low.just_won(JustWonMode::WonAndLeading), 1);

        let mut high = BidRounds::new(2, WINDOW);
        high.open(key(1), t0);
        high.add_bid(key(1), 1, 10, t0);
        high.add_bid(key(1), 2, 10, t0);
        assert_eq!(high.just_won(JustWonMode::WonAndLeading), 0);
    }

    #[test]
    fn a_leading_round_becomes_one_win_when_it_closes() {
        let t0 = Instant::now();
        let mut r = rounds();
        win(&mut r, key(1), t0);
        assert_eq!(r.just_won(JustWonMode::WonAndLeading), 1);
        close_all(&mut r, t0 + WINDOW);
        assert_eq!(r.just_won(JustWonMode::WonAndLeading), 1);
    }

    #[test]
    fn counting_leading_rounds_raises_each_bid_in_a_burst() {
        let t0 = Instant::now();
        let mut r = rounds();
        let service = 4_000;
        let mut scores = Vec::new();
        for seq in 0..3 {
            r.open(key(seq), t0);
            let i = inputs(0, 0, r.just_won(JustWonMode::WonAndLeading), service);
            let score = load_score(&i, ScoreVariant::ExpectedWait);
            r.add_bid(key(seq), ME, score, t0);
            scores.push(score);
        }
        // With wins only, all three would bid 1_000 and this node would take the whole burst.
        assert_eq!(scores, vec![1_000, 2_000, 3_000]);
    }

    #[test]
    fn forgetting_a_round_drops_its_win() {
        let t0 = Instant::now();
        let mut r = rounds();
        win(&mut r, key(1), t0);
        close_all(&mut r, t0 + WINDOW);
        r.forget_older_than(t0 + WINDOW + GRACE, GRACE);
        assert_eq!(r.just_won(JustWonMode::WinsOnly), 0);
    }
}
