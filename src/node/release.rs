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
use crate::proto::{Command, NodeId, RequestId, ResponderId};
use crate::state::HoldMade;

/// How often a node looks at the holds. Small against the shortest service time (200 ms).
const SCAN_INTERVAL: Duration = Duration::from_millis(50);

/// A backup release waits at least this long past the service time (D244).
const BACKUP_MARGIN_MS: u64 = 1000;

/// How many new holds one look takes in, so a node sent a long log does not hold the state's lock
/// for all of it at once.
const TAKE_IN_PER_LOOK: usize = 8192;

/// One release this node should propose now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Due {
    responder: ResponderId,
    request: RequestId,
    backup: bool,
}

/// How long this node leaves a hold before it proposes the release, and whether that is as a backup.
fn wait_for(me: NodeId, hold: &HoldMade, ms_per_severity: u64) -> (Duration, bool) {
    let backup = hold.executor != me;
    let service = u64::from(hold.severity) * ms_per_severity;
    // A backup is for an executor that went silent, not for a slow log (D244).
    let backup_wait = (2 * service).max(service + BACKUP_MARGIN_MS);
    let wait = if backup { backup_wait } else { service };
    (Duration::from_millis(wait), backup)
}

/// The holds this node follows, by when it next looks at each (D253). It reads the list of holds as
/// it grows, so a look costs what changed and not the whole table. Rebuilt from the replicated state
/// after a restart, which at worst holds a responder a little longer.
#[derive(Debug, Default)]
struct Tracker {
    /// How many holds of the state's list this has taken in.
    taken: usize,
    next: BTreeMap<(Instant, u64), HoldMade>,
    added: u64,
}

impl Tracker {
    fn add(&mut self, hold: HoldMade, at: Instant) {
        self.added += 1;
        self.next.insert((at, self.added), hold);
    }

    fn take_in(&mut self, me: NodeId, new: &[HoldMade], ms_per_severity: u64, now: Instant) {
        for hold in new {
            let (wait, _) = wait_for(me, hold, ms_per_severity);
            self.add(*hold, now + wait);
        }
        self.taken += new.len();
    }

    /// The holds whose time is up, no longer followed unless added again.
    fn time_up(&mut self, now: Instant) -> Vec<HoldMade> {
        let later = self.next.split_off(&(now, u64::MAX));
        std::mem::replace(&mut self.next, later)
            .into_values()
            .collect()
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
            let now = Instant::now();
            let up = tracker.time_up(now);
            // A hold that is gone was released, and is forgotten here.
            let still_held: Vec<HoldMade> = ctx.state.with(|s| {
                let new = s.holds_made_after(tracker.taken);
                let new = &new[..new.len().min(TAKE_IN_PER_LOOK)];
                tracker.take_in(ctx.id, new, ms, now);
                up.into_iter()
                    .filter(|hold| s.is_holding(hold.responder, hold.request))
                    .collect()
            });
            for hold in still_held {
                let (wait, backup) = wait_for(ctx.id, &hold, ms);
                // The timer restarts, so a proposal that fails is tried again one wait later.
                tracker.add(hold, now + wait);
                let ctx = ctx.clone();
                let due = Due {
                    responder: hold.responder,
                    request: hold.request,
                    backup,
                };
                in_flight.spawn(async move { propose(&ctx, due).await });
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

    fn hold(seq: u64, executor: NodeId, severity: u8) -> HoldMade {
        HoldMade {
            responder: 9,
            request: RequestId { client: 1, seq },
            executor,
            severity,
        }
    }

    #[test]
    fn the_executor_releases_after_the_service_time() {
        let h = hold(1, 1, 2);
        let t0 = Instant::now();
        let mut tracker = Tracker::default();
        tracker.take_in(1, &[h], 200, t0);
        assert!(tracker.time_up(t0 + Duration::from_millis(399)).is_empty());
        assert_eq!(tracker.time_up(t0 + Duration::from_millis(400)), vec![h]);
        assert!(!wait_for(1, &h, 200).1);
    }

    #[test]
    fn another_node_waits_longer() {
        // Severity 2 at 200 ms a point: the margin decides, 400 + 1000 ms.
        let h = hold(1, 1, 2);
        let t0 = Instant::now();
        let mut tracker = Tracker::default();
        tracker.take_in(2, &[h], 200, t0);
        assert!(tracker.time_up(t0 + Duration::from_millis(1399)).is_empty());
        assert_eq!(tracker.time_up(t0 + Duration::from_millis(1400)), vec![h]);
        assert!(wait_for(2, &h, 200).1);
    }

    #[test]
    fn a_hold_whose_time_is_up_is_followed_no_more_unless_added_again() {
        let h = hold(1, 1, 1);
        let t0 = Instant::now();
        let mut tracker = Tracker::default();
        tracker.take_in(1, &[h], 200, t0);
        let first = t0 + Duration::from_millis(200);
        assert_eq!(tracker.time_up(first).len(), 1);
        assert!(tracker.next.is_empty());
        tracker.add(h, first + Duration::from_millis(200));
        assert!(
            tracker
                .time_up(first + Duration::from_millis(100))
                .is_empty()
        );
        assert_eq!(tracker.time_up(first + Duration::from_millis(200)).len(), 1);
    }

    #[test]
    fn it_remembers_how_far_it_has_read() {
        let t0 = Instant::now();
        let mut tracker = Tracker::default();
        tracker.take_in(1, &[hold(1, 1, 1), hold(2, 1, 1)], 200, t0);
        assert_eq!(tracker.taken, 2);
        tracker.take_in(1, &[hold(3, 1, 1)], 200, t0);
        assert_eq!(tracker.taken, 3);
        assert_eq!(tracker.time_up(t0 + Duration::from_millis(200)).len(), 3);
    }
}
