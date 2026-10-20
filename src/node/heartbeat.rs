//! Peer heartbeats and failure suspicion.
//!
//! Raft heartbeats only flow from the leader to followers, so a follower cannot tell when
//! another follower (for example a bidding winner) goes silent. Every node sends
//! `PeerBody::Heartbeat` to every peer each `timers.peer_heartbeat_ms` and suspects a peer after
//! `timers.takeover_missed_heartbeats` intervals with nothing heard from it.
//!
//! Owner: C (faults and failover). Template: nobody is ever suspected.

use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::node::NodeContext;
use crate::proto::NodeId;

/// When each peer was last heard from. Takes the time as an argument so it is easy to test.
#[derive(Debug, Clone)]
pub struct Suspicion {
    interval: Duration,
    missed: u32,
}

impl Suspicion {
    pub fn new(interval: Duration, missed: u32) -> Self {
        Self { interval, missed }
    }

    /// Called by the node's peer dispatcher for every heartbeat received.
    pub fn heard_from(&mut self, _peer: NodeId, _at: Instant) {
        // TODO(C): remember the latest time per peer.
    }

    /// True once `missed` intervals pass with nothing heard from `peer`.
    pub fn is_suspected(&self, _peer: NodeId, _now: Instant) -> bool {
        // TODO(C): compare against `self.interval * self.missed`.
        let _ = (self.interval, self.missed);
        false
    }
}

/// Start sending heartbeats to every peer.
pub fn spawn(_ctx: &Arc<NodeContext>) -> Option<JoinHandle<()>> {
    // TODO(C): every `timers.peer_heartbeat()`, call each peer in `ctx.peers` with
    // `PeerBody::Heartbeat` (short timeout, ignore errors) and record replies in `ctx.suspicion`.
    None
}
