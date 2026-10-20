//! Load-priority bidding election: picks the node that plans each request.
//!
//! Every synced node broadcasts a load score for the request; the lowest score wins and ties go
//! to the lower node ID. If the winner goes silent, the next-lowest bidder takes over.
//!
//! Pure logic: no network, no clock. `node/` drives rounds by feeding in bids and timer events.
//!
//! Owner: B (bidding election). Template: nothing calls these yet.

use crate::proto::NodeId;

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
/// CPU. Lower means less loaded.
pub fn load_score(_inputs: &LoadInputs) -> u64 {
    // TODO(B): implement the formula and justify the CPU weight with E6.
    0
}

/// The winning bidder: lowest score, ties to the lower node ID. `None` if nobody bid.
pub fn pick_winner(_bids: &[(NodeId, u64)]) -> Option<NodeId> {
    // TODO(B)
    None
}

/// Every bidder ordered by (score, node ID): the winner first, then the takeover order.
pub fn ranking(_bids: &[(NodeId, u64)]) -> Vec<NodeId> {
    // TODO(B)
    Vec::new()
}

// TODO(B): `BidRound` state machine: open on request arrival, collect bids for
// `timers.bid_window_ms`, close and report the winner. Track `just_won` between rounds.
// TODO(B): takeover: the next bidder in `ranking` takes over after
// `timers.takeover_missed_heartbeats` missed heartbeats with nothing committed for the request.
// TODO(B): wire `AssignmentPolicy::Bidding` into `node::decide_executor` (ask A to review).
