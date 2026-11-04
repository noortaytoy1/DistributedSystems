//! Frees responders after their simulated time on scene (D39, D111).
//!
//! Every serving node watches the holds in its copy of the replicated state. The executor of a
//! hold proposes `ReleaseResponder` after the service time; every other node proposes it after
//! twice that, so a silent executor cannot leak a responder and no node has a special role.
//! Timers are local, but the release goes through the log, so state stays deterministic. Two
//! nodes releasing the same hold is harmless: the command is idempotent.
//!
//! This is a rule about responders, so it is application code like `service.rs` (D156). It reaches
//! the layer below through `src/middleware.rs` alone, and `tests/layers.rs` holds it to that.
//!
//! Owner: E (planner and concurrency).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::task::{JoinHandle, JoinSet};
use tokio::time::Instant;

use crate::metrics::Event;
use crate::middleware::NodeContext;
use crate::proto::{Command, CommittedResult, NodeId, RequestId, ResponderId};

/// How often a node looks at the holds. Small against the shortest service time (200 ms).
const SCAN_INTERVAL: Duration = Duration::from_millis(50);

/// A backup release waits at least this long past the service time (D244).
const BACKUP_MARGIN_MS: u64 = 1000;

/// One release this node should propose now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Due {
    responder: ResponderId,
    request: RequestId,
    backup: bool,
}

/// When this node first saw each hold. Rebuilt from the replicated state after a restart, which
/// at worst holds a responder a little longer.
#[derive(Debug, Default)]
struct Tracker {
    seen: BTreeMap<(ResponderId, RequestId), Instant>,
}

impl Tracker {
    /// The holds whose time is up. A due hold's timer restarts, so a proposal that fails is tried
    /// again one service time later rather than on every scan.
    fn scan(
        &mut self,
        me: NodeId,
        holds: &[(ResponderId, &CommittedResult)],
        ms_per_severity: u64,
        now: Instant,
    ) -> Vec<Due> {
        let mut seen = BTreeMap::new();
        let mut due = Vec::new();
        for (responder, result) in holds {
            let key = (*responder, result.request);
            let mut first = self.seen.get(&key).copied().unwrap_or(now);
            let backup = result.executor != me;
            let service = u64::from(result.incident.severity) * ms_per_severity;
            // A backup is for an executor that went silent, not for a slow log (D244).
            let backup_wait = (2 * service).max(service + BACKUP_MARGIN_MS);
            let wait = Duration::from_millis(if backup { backup_wait } else { service });
            if now.duration_since(first) >= wait {
                due.push(Due {
                    responder: *responder,
                    request: result.request,
                    backup,
                });
                first = now;
            }
            seen.insert(key, first);
        }
        // A hold that is gone was released; forget it.
        self.seen = seen;
        due
    }
}

/// Runs for the node's lifetime; the node aborts it on shutdown.
pub fn spawn(ctx: &Arc<NodeContext>) -> JoinHandle<()> {
    let ctx = ctx.clone();
    tokio::spawn(async move {
        let mut tracker = Tracker::default();
        let mut tick = tokio::time::interval(SCAN_INTERVAL);
        // Owned here, so aborting this task ends them. Scanning never waits for a slow log (D244).
        let mut in_flight = JoinSet::new();
        loop {
            tick.tick().await;
            while in_flight.try_join_next().is_some() {}
            // A lone node cannot commit a release; its local table does the job meanwhile (D124).
            if !ctx.serving.load(Ordering::SeqCst) || ctx.degraded.is_active() {
                continue;
            }
            let ms = ctx.config.campus.service_ms_per_severity;
            let due = ctx
                .state
                .with(|s| tracker.scan(ctx.id, &s.holds(), ms, Instant::now()));
            for d in due {
                let ctx = ctx.clone();
                in_flight.spawn(async move { propose(&ctx, d).await });
            }
        }
    })
}

async fn propose(ctx: &NodeContext, due: Due) {
    ctx.log.log(Event::ReleaseProposed {
        responder: due.responder,
        request: due.request,
        backup: due.backup,
    });
    let cmd = Command::ReleaseResponder {
        responder: due.responder,
        request: due.request,
    };
    if let Err(e) = ctx.group.send_ordered(cmd).await {
        tracing::debug!(request = %due.request, "release not proposed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{Capability, Category, Incident, Plan, Point, UserId};

    fn result(seq: u64, executor: NodeId, severity: u8) -> CommittedResult {
        CommittedResult {
            request: RequestId { client: 1, seq },
            incident: Incident {
                owner: UserId { client: 1, user: 0 },
                category: Category::MedicalAssistance,
                zone: 0,
                location: Point { x: 0, y: 0 },
                severity,
                needs: Capability::Medical,
                reported_at_ms: 0,
                ttl_ms: 60_000,
            },
            executor,
            term: 1,
            plan: Plan {
                responder: Some(1),
                cost: 0,
                priority: 200,
            },
            age_ms: 0,
            assigned_at_ms: 0,
            log_index: 1,
        }
    }

    #[test]
    fn the_executor_releases_after_the_service_time() {
        let r = result(1, 1, 2);
        let t0 = Instant::now();
        let mut tracker = Tracker::default();
        assert!(tracker.scan(1, &[(9, &r)], 200, t0).is_empty());
        let early = t0 + Duration::from_millis(399);
        assert!(tracker.scan(1, &[(9, &r)], 200, early).is_empty());
        let due = tracker.scan(1, &[(9, &r)], 200, t0 + Duration::from_millis(400));
        assert_eq!(
            due,
            vec![Due {
                responder: 9,
                request: r.request,
                backup: false
            }]
        );
    }

    #[test]
    fn another_node_waits_longer() {
        // Severity 2 at 200 ms a point: the margin decides, 400 + 1000 ms.
        let r = result(1, 1, 2);
        let t0 = Instant::now();
        let mut tracker = Tracker::default();
        tracker.scan(2, &[(9, &r)], 200, t0);
        let early = t0 + Duration::from_millis(1399);
        assert!(tracker.scan(2, &[(9, &r)], 200, early).is_empty());
        let due = tracker.scan(2, &[(9, &r)], 200, t0 + Duration::from_millis(1400));
        assert!(due[0].backup);
    }

    #[test]
    fn a_failed_release_is_tried_again_one_wait_later() {
        let r = result(1, 1, 1);
        let t0 = Instant::now();
        let mut tracker = Tracker::default();
        tracker.scan(1, &[(9, &r)], 200, t0);
        let first = t0 + Duration::from_millis(200);
        assert_eq!(tracker.scan(1, &[(9, &r)], 200, first).len(), 1);
        let next = first + Duration::from_millis(100);
        assert!(tracker.scan(1, &[(9, &r)], 200, next).is_empty());
        let later = first + Duration::from_millis(200);
        assert_eq!(tracker.scan(1, &[(9, &r)], 200, later).len(), 1);
    }

    #[test]
    fn a_released_hold_is_forgotten() {
        let r = result(1, 1, 1);
        let t0 = Instant::now();
        let mut tracker = Tracker::default();
        tracker.scan(1, &[(9, &r)], 200, t0);
        tracker.scan(1, &[], 200, t0);
        assert!(tracker.seen.is_empty());
    }
}
