//! Rejoin and state recovery.
//!
//! A node that comes back from a fault catches up from the leader's log (Raft replication does
//! the copying) and serves nothing until it is current: no votes for itself, no bids, no
//! replies. `NodeContext::serving` is the switch other parts check.
//!
//! Owner: D (rejoin and recovery). Template: a node always counts as current.

use std::sync::Arc;

use tokio::task::JoinHandle;

use crate::node::NodeContext;

/// A node is current once its applied index reaches the leader's commit index.
pub fn is_current(_local_applied: Option<u64>, _leader_commit: Option<u64>) -> bool {
    // TODO(D)
    true
}

/// Start watching for returns from silence.
pub fn spawn(_ctx: &Arc<NodeContext>) -> Option<JoinHandle<()>> {
    // TODO(D): when the node returns from a fault, clear `ctx.serving`, learn the leader's
    // commit index, wait until `is_current`, then set `ctx.serving` again. Log both transitions
    // for E4.
    None
}
