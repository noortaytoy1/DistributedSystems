//! Fault injection: silence a node for a while, by hand (`ctl silence`) or on a schedule.
//!
//! A silenced node drops all peer and client traffic (`FaultGate` in transport.rs) but keeps its
//! state, like a node cut off by the network. The spec allows failures of up to 20 s.
//!
//! One fault at a time per node: a silence order for a node that is already silenced is
//! rejected, never merged or restarted (D71). Durations of zero or above `MAX_FAULT` are
//! rejected too, so every `silenced`/`restored` pair in the logs is a real, in-spec fault.
//!
//! Owner: C (faults and failover). Built: `silence_for`. Template: `spawn_schedule`.

use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;

use crate::metrics::Event;
use crate::node::NodeContext;

/// Longest fault the system must tolerate transparently.
pub const MAX_FAULT: Duration = Duration::from_secs(20);

/// Silence this node now and restore it after `duration` (`ctl silence`).
pub fn silence_for(ctx: &Arc<NodeContext>, duration: Duration) -> Result<(), String> {
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

/// Start the scheduled fault injector.
pub fn spawn_schedule(_ctx: &Arc<NodeContext>) -> Option<JoinHandle<()>> {
    // TODO(C): periodically the Raft leader picks one victim (it may pick itself, which
    // exercises coordinator failover) and orders it silent for up to `MAX_FAULT`. Tag the order
    // with the leader's term so a stale leader's order is dropped.
    None
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
    fn a_silenced_gate_cannot_be_silenced_again_until_restored() {
        let gate = FaultGate::default();
        assert!(gate.try_silence());
        assert!(!gate.try_silence());
        assert!(gate.is_silenced());
        gate.restore();
        assert!(gate.try_silence());
    }
}
