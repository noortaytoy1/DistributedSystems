//! Replicated state machine: incidents, responders, results and duplicate detection.
//!
//! Every node applies the same committed commands in the same order, so this code must be
//! deterministic: no clock, no randomness, no network, no `HashMap` iteration.
//!
//! Owner: A (Raft and replication).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::proto::{CommittedResult, RequestId, Responder, ResponderId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponderSlot {
    pub responder: Responder,
    /// Request currently holding this responder.
    pub held_by: Option<RequestId>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudState {
    responders: BTreeMap<ResponderId, ResponderSlot>,
    results: BTreeMap<RequestId, CommittedResult>,
}

impl CloudState {
    pub fn new() -> Self {
        Self::default()
    }

    // TODO(A): `apply(index, &Command) -> Outcome` with first-commit-wins per request ID,
    // stale-plan detection and a running digest of the applied log.

    pub fn result(&self, id: &RequestId) -> Option<&CommittedResult> {
        self.results.get(id)
    }

    /// Responders not held by any request, in ID order.
    pub fn free_responders(&self) -> Vec<Responder> {
        self.responders
            .values()
            .filter(|s| s.held_by.is_none())
            .map(|s| s.responder.clone())
            .collect()
    }
}
