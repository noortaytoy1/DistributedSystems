//! Deterministic incident planner with a cost knob.
//!
//! Given an incident and the responders that are free, choose one. The same inputs must always
//! give the same plan. `CostKnob::extra_work` adds CPU work per plan so experiments can make
//! planning CPU-bound and create load imbalance.
//!
//! Pure logic: no network, no clock. Runs on the worker pool (node/workers.rs), never on the
//! async runtime.
//!
//! Owner: E (planner and concurrency). Template: returns an empty plan.

use crate::proto::{Incident, Plan, Responder};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CostKnob {
    /// Units of busy work added to each plan.
    pub extra_work: u32,
}

/// Choose a responder for `incident` from `free`.
pub fn plan(_incident: &Incident, _free: &[Responder], _knob: CostKnob) -> Plan {
    // TODO(E): combine severity, age, distance, availability and capability [Spec p.3];
    // burn `knob.extra_work` units of CPU; break ties deterministically.
    Plan {
        responder: None,
        cost: 0,
    }
}
