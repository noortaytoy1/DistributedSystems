//! Fault injection: silence a node for a while, by hand (`ctl silence`) or on a schedule.
//!
//! A silenced node drops all peer and client traffic (`FaultGate` in transport.rs) but keeps its
//! state, like a node cut off by the network. The spec allows failures of up to 20 s.
//!
//! Owner: C (faults and failover). Template: not implemented.

use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;

use crate::node::NodeContext;

/// Longest fault the system must tolerate transparently.
pub const MAX_FAULT: Duration = Duration::from_secs(20);

/// Silence this node now and restore it after `duration` (`ctl silence`).
pub fn silence_for(_ctx: &Arc<NodeContext>, _duration: Duration) -> Result<(), String> {
    // TODO(C): close `ctx.gate`, log `Event::Silenced`, reopen after `duration` and log
    // `Event::Restored`. Overlapping faults must not cut each other short.
    Err("fault injection is not implemented yet".into())
}

/// Start the scheduled fault injector.
pub fn spawn_schedule(_ctx: &Arc<NodeContext>) -> Option<JoinHandle<()>> {
    // TODO(C): periodically the Raft leader picks one victim (it may pick itself, which
    // exercises coordinator failover) and orders it silent for up to `MAX_FAULT`. Tag the order
    // with the leader's term so a stale leader's order is dropped.
    None
}
