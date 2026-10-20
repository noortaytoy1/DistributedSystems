//! Load-priority bidding election: picks the node that plans each request.
//!
//! Every synced node broadcasts a load score for the request; the lowest score wins and ties go
//! to the lower node ID. If the winner goes silent, the next-lowest bidder takes over.
//!
//! Pure logic: no network, no clock. `node/` drives rounds by feeding in bids and timer events.
//!
//! Owner: B (bidding election). Not wired into `node/` yet.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use crate::proto::NodeId;

/// Score microseconds added per permille of CPU busy time: 100% CPU adds 10 ms. It is in the
/// queue term's units so that CPU visibly moves the score.
// TODO(B): justify the CPU weight with E6.
pub const CPU_WEIGHT_US_PER_PERMILLE: u64 = 10;

/// Inputs to one node's load score, sampled when the bid is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LoadInputs {
    /// Planner jobs waiting for a worker thread.
    pub queued: usize,
    /// Planner jobs running now.
    pub running: usize,
    /// Rounds this node won whose jobs have not reached the queue yet.
    pub just_won: usize,
    /// Recent mean planning time per job, in microseconds.
    pub recent_service_us: u64,
    /// Worker threads on this node.
    pub workers: usize,
    /// CPU busy fraction in permille (0..=1000).
    pub cpu_permille: u32,
}

/// Load score: `(queued + running + just won + 1) × recent service time ÷ workers`, plus weighted
/// CPU. Lower means less loaded. The unit is microseconds of expected wait.
///
/// The `+ 1` counts the request being bid on, so an idle node scores its own service time and a
/// fast idle node beats a slow one. Saturates instead of overflowing.
pub fn load_score(inputs: &LoadInputs) -> u64 {
    let in_flight = inputs
        .queued
        .saturating_add(inputs.running)
        .saturating_add(inputs.just_won)
        .saturating_add(1) as u64;
    let workers = inputs.workers.max(1) as u64;
    let queue_us = in_flight.saturating_mul(inputs.recent_service_us) / workers;
    let cpu_us = u64::from(inputs.cpu_permille).saturating_mul(CPU_WEIGHT_US_PER_PERMILLE);
    queue_us.saturating_add(cpu_us)
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

/// What happened to a bid offered to a round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BidStatus {
    Accepted,
    /// Arrived at or after the deadline, so it is ignored.
    Late,
    /// This node already bid in this round; its first bid stands.
    Duplicate,
}

/// One request's bid round. It opens when the request arrives and accepts one bid per node
/// until `bid_window` has passed. The caller passes in every time and closes the round once
/// [`BidRound::deadline`] is reached.
#[derive(Debug, Clone)]
pub struct BidRound {
    deadline: Instant,
    bids: BTreeMap<NodeId, u64>,
}

impl BidRound {
    pub fn open(arrived: Instant, bid_window: Duration) -> Self {
        Self {
            deadline: arrived + bid_window,
            bids: BTreeMap::new(),
        }
    }

    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    pub fn is_closed(&self, now: Instant) -> bool {
        now >= self.deadline
    }

    pub fn submit(&mut self, node: NodeId, score: u64, at: Instant) -> BidStatus {
        if self.is_closed(at) {
            return BidStatus::Late;
        }
        match self.bids.entry(node) {
            Entry::Occupied(_) => BidStatus::Duplicate,
            Entry::Vacant(slot) => {
                slot.insert(score);
                BidStatus::Accepted
            }
        }
    }

    /// The round's [`ranking`]: winner first, then the takeover order. Empty if nobody bid.
    pub fn close(self) -> Vec<NodeId> {
        let bids: Vec<(NodeId, u64)> = self.bids.into_iter().collect();
        ranking(&bids)
    }
}

/// Rounds this node won whose jobs have not reached the worker queue yet. Without it, an idle
/// node wins every request in a burst, because its queue has not grown when the next round opens.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JustWon(usize);

impl JustWon {
    pub fn won(&mut self) {
        self.0 = self.0.saturating_add(1);
    }

    /// The job reached the worker queue, where `LoadInputs::queued` counts it from now on.
    pub fn enqueued(&mut self) {
        self.0 = self.0.saturating_sub(1);
    }

    pub fn count(self) -> usize {
        self.0
    }
}

// TODO(B): wire `AssignmentPolicy::Bidding` into `node::decide_executor` (ask A to review).

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Duration = Duration::from_millis(20);

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

    #[test]
    fn load_score_matches_formula() {
        let i = LoadInputs {
            cpu_permille: 500,
            ..inputs(2, 1, 0, 10_000)
        };
        // (2 + 1 + 0 + 1) × 10_000 / 4 = 10_000, plus 500 × 10 = 5_000.
        assert_eq!(load_score(&i), 15_000);
    }

    #[test]
    fn idle_node_scores_its_service_time_per_worker() {
        assert_eq!(load_score(&inputs(0, 0, 0, 40_000)), 10_000);
    }

    #[test]
    fn just_won_raises_the_score_like_queued_work() {
        assert_eq!(
            load_score(&inputs(0, 0, 1, 40_000)),
            load_score(&inputs(1, 0, 0, 40_000))
        );
        assert!(load_score(&inputs(0, 0, 1, 40_000)) > load_score(&inputs(0, 0, 0, 40_000)));
    }

    #[test]
    fn full_cpu_adds_ten_ms() {
        let idle = inputs(0, 0, 0, 40_000);
        let busy = LoadInputs {
            cpu_permille: 1000,
            ..idle
        };
        assert_eq!(load_score(&busy) - load_score(&idle), 10_000);
    }

    #[test]
    fn zero_workers_counts_as_one() {
        let i = LoadInputs {
            workers: 0,
            ..inputs(0, 0, 0, 5_000)
        };
        assert_eq!(load_score(&i), 5_000);
    }

    #[test]
    fn huge_inputs_saturate_instead_of_panicking() {
        let i = LoadInputs {
            queued: usize::MAX,
            recent_service_us: u64::MAX,
            workers: 1,
            cpu_permille: u32::MAX,
            ..LoadInputs::default()
        };
        assert_eq!(load_score(&i), u64::MAX);
    }

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
    fn ranking_ignores_bid_order() {
        let a = ranking(&[(1, 5), (2, 5), (3, 1)]);
        let b = ranking(&[(3, 1), (2, 5), (1, 5)]);
        assert_eq!(a, b);
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

    #[test]
    fn round_accepts_bids_before_the_deadline() {
        let t0 = Instant::now();
        let mut round = BidRound::open(t0, WINDOW);
        assert_eq!(round.deadline(), t0 + WINDOW);
        assert_eq!(round.submit(1, 30, t0), BidStatus::Accepted);
        assert_eq!(
            round.submit(2, 10, t0 + Duration::from_millis(19)),
            BidStatus::Accepted
        );
        assert_eq!(round.close(), vec![2, 1]);
    }

    #[test]
    fn round_rejects_bids_at_or_after_the_deadline() {
        let t0 = Instant::now();
        let mut round = BidRound::open(t0, WINDOW);
        assert!(!round.is_closed(t0 + Duration::from_millis(19)));
        assert!(round.is_closed(t0 + WINDOW));
        assert_eq!(round.submit(1, 0, t0 + WINDOW), BidStatus::Late);
        assert!(round.close().is_empty());
    }

    #[test]
    fn round_keeps_a_nodes_first_bid() {
        let t0 = Instant::now();
        let mut round = BidRound::open(t0, WINDOW);
        assert_eq!(round.submit(1, 50, t0), BidStatus::Accepted);
        assert_eq!(round.submit(1, 5, t0), BidStatus::Duplicate);
        assert_eq!(round.submit(2, 20, t0), BidStatus::Accepted);
        assert_eq!(round.close(), vec![2, 1]);
    }

    #[test]
    fn just_won_counts_up_and_down_without_underflow() {
        let mut jw = JustWon::default();
        jw.won();
        jw.won();
        assert_eq!(jw.count(), 2);
        jw.enqueued();
        jw.enqueued();
        jw.enqueued();
        assert_eq!(jw.count(), 0);
    }
}
