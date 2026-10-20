//! Synthetic campus: zones, the responders spread across them, and random incidents.
//!
//! Responders must be a pure function of `CampusConfig`, so every node and client derives the
//! same campus from the same config.
//!
//! Owner: E (planner and concurrency). Template: an empty campus.

use rand::Rng;

use crate::config::CampusConfig;
use crate::proto::{Capability, Incident, Point, Responder};

/// The campus responders, registered through Raft when the cluster first forms.
pub fn responders(_cfg: &CampusConfig) -> Vec<Responder> {
    // TODO(E): place `cfg.responders` responders across `cfg.zones` zones from `cfg.seed`.
    Vec::new()
}

/// A random incident, as a simulated user would report it.
pub fn random_incident(_rng: &mut impl Rng, _cfg: &CampusConfig, now_ms: u64) -> Incident {
    // TODO(E): random zone, location inside it, severity and needed capability.
    Incident {
        zone: 0,
        location: Point { x: 0, y: 0 },
        severity: 1,
        needs: Capability::Medical,
        reported_at_ms: now_ms,
    }
}
