//! The claims service: a cloud node's side of resource offers and claims (Phase 2, Part J,
//! D370-D380).
//!
//! The application layer, like the presence service (`node/directory.rs`), which this mirrors:
//! - **Writes (`offer`, `claim`, `release`, `resolve`).** A client sends each to every node. One
//!   node proposes it: the member of this node's view that the resource hashes to (D376). The
//!   others stay quiet, which means "not me" (D36), and the client takes the one answer. Commit,
//!   then reply, as for a report.
//! - **No tick of its own.** Claims judge expiry against the presence directory's log clock
//!   (D371): nothing here moves a clock, so there is no background task to spawn beyond the
//!   report.
//! - **A report a second.** `claims_state`: offers, open reconciliation entries, counters and this
//!   node's own digest, for the plots and to compare nodes at one index (D380).
//!
//! Owner: J (resource offers, leases and claims).

use std::sync::Arc;
use std::sync::atomic::Ordering;

use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::metrics::{Event, now_ms};
use crate::middleware::{ClaimAnswer, ClaimCall, NodeContext};
use crate::proto::{ClaimOutcome, ClaimReply, Command, Outcome, ResourceId};
use crate::state::sample_mix;

/// Handle one claims call. `None` means stay quiet: the call stays open until the client cancels
/// it or its deadline passes, as for a report this node does not plan (D36).
pub(super) async fn handle(ctx: Arc<NodeContext>, call: ClaimCall) -> Option<ClaimAnswer> {
    if !ctx.serving.load(Ordering::SeqCst) {
        return None;
    }
    let started = Instant::now();
    let resource = call.resource();
    if ctx.degraded.is_active() || !proposes_for(&ctx, resource) {
        return None;
    }
    let outcome = match ctx.group.send_ordered(command(&call)).await {
        Ok(Outcome::Claim(outcome)) => outcome,
        Ok(other) => {
            tracing::warn!("unexpected outcome {} for a claims call", other.kind());
            return None;
        }
        // The client's retry covers it, as for a report.
        Err(e) => {
            tracing::debug!(%resource, "claims call not committed: {e}");
            return None;
        }
    };
    // It may have stopped serving while it waited on the commit (D136).
    if !ctx.serving.load(Ordering::SeqCst) {
        return None;
    }
    log_write(&ctx, &call, &outcome, started);
    Some(ClaimAnswer(reply(outcome)))
}

/// The command for a write, from the call's own fields: no clock or lease to stamp, unlike
/// presence (D373): a claim's expiry is its offer's own `available_until_ms`.
fn command(call: &ClaimCall) -> Command {
    match call.clone() {
        ClaimCall::Offer(offer) => Command::OfferResource { offer },
        ClaimCall::Claim {
            resource,
            claimant,
            quantity,
            speculative,
            at_ms,
        } => Command::ClaimResource {
            resource,
            claimant,
            quantity,
            speculative,
            at_ms,
        },
        ClaimCall::Release {
            resource,
            claimant,
            token,
        } => Command::ReleaseClaim {
            resource,
            claimant,
            token,
        },
        ClaimCall::Resolve {
            resource,
            resolver,
            token,
            winner,
        } => Command::ResolveReconciliation {
            resource,
            resolver,
            token,
            winner,
        },
    }
}

/// What the client is told (D371-D378): a trimmed `ClaimOutcome`, with the internal-only variants
/// turned into a refusal or a rejection.
fn reply(outcome: ClaimOutcome) -> ClaimReply {
    match outcome {
        ClaimOutcome::Offered { log_index, .. } => ClaimReply::Offered { log_index },
        ClaimOutcome::Claimed { token, log_index } => ClaimReply::Claimed { token, log_index },
        ClaimOutcome::Reconciling { token, log_index } => {
            ClaimReply::Reconciling { token, log_index }
        }
        ClaimOutcome::Released { log_index } => ClaimReply::Released { log_index },
        ClaimOutcome::Resolved { log_index, .. } => ClaimReply::Resolved { log_index },
        ClaimOutcome::Refused { available, .. } => {
            ClaimReply::Refused(format!("only {available} left"))
        }
        ClaimOutcome::StaleRelease { .. } => {
            ClaimReply::Refused("already released, or superseded by a later claim".into())
        }
        ClaimOutcome::NotOwner { .. } => {
            ClaimReply::Refused("not this resource's offer owner".into())
        }
        ClaimOutcome::Unknown { .. } => {
            ClaimReply::Rejected("no offer, claim or reconciliation entry at that ID".into())
        }
    }
}

/// Logs the write's own event, plus a conflict or reconciliation event where the outcome is one
/// (D376, D378, D380).
fn log_write(ctx: &NodeContext, call: &ClaimCall, outcome: &ClaimOutcome, started: Instant) {
    let latency_us = started.elapsed().as_micros() as u64;
    match (call, outcome) {
        (
            ClaimCall::Offer(offer),
            ClaimOutcome::Offered {
                capacity,
                log_index,
            },
        ) => {
            ctx.log.log(Event::ResourceOffered {
                resource: offer.id,
                provider: offer.provider,
                capability: offer.capability,
                zone: offer.zone,
                capacity: *capacity,
                available_until_ms: offer.available_until_ms,
                log_index: *log_index,
                latency_us,
            });
        }
        (ClaimCall::Claim { .. } | ClaimCall::Release { .. }, _) => {
            let (resource, claimant) = (call.resource(), call.claimant());
            let quantity = if let ClaimCall::Claim { quantity, .. } = call {
                *quantity
            } else {
                0
            };
            ctx.log.log(Event::ClaimWrite {
                resource,
                claimant,
                quantity,
                kind: if matches!(call, ClaimCall::Claim { .. }) {
                    "claim"
                } else {
                    "release"
                },
                outcome: outcome.kind(),
                speculative: matches!(
                    call,
                    ClaimCall::Claim {
                        speculative: true,
                        ..
                    }
                ),
                log_index: outcome.log_index(),
                latency_us,
            });
            match outcome {
                ClaimOutcome::Refused { .. } => ctx.log.log(Event::ClaimConflict {
                    resource,
                    claimant,
                    kind: "online_refused",
                    log_index: outcome.log_index(),
                }),
                ClaimOutcome::Reconciling { .. } => ctx.log.log(Event::ClaimConflict {
                    resource,
                    claimant,
                    kind: "reconciliation_opened",
                    log_index: outcome.log_index(),
                }),
                _ => {}
            }
        }
        (
            ClaimCall::Resolve { .. },
            ClaimOutcome::Resolved {
                winner,
                challenger,
                opened_at_ms,
                log_index,
            },
        ) => {
            ctx.log.log(Event::ReconciliationResolved {
                resource: call.resource(),
                challenger: *challenger,
                winner: *winner,
                wait_ms: now_ms().saturating_sub(*opened_at_ms),
                log_index: *log_index,
            });
        }
        _ => {}
    }
}

/// Whether this node proposes `resource`'s writes: the member of its view it hashes to (D376).
pub(super) fn proposes_for(ctx: &NodeContext, resource: ResourceId) -> bool {
    let members = ctx.group.view().members;
    proposer(&members.iter().copied().collect::<Vec<_>>(), resource) == Some(ctx.id)
}

/// The member of `members` (in ID order) that proposes `resource`'s writes.
pub(super) fn proposer(members: &[u64], resource: ResourceId) -> Option<u64> {
    if members.is_empty() {
        return None;
    }
    let hash = sample_mix(sample_mix(resource.provider) ^ resource.seq);
    members.get((hash % members.len() as u64) as usize).copied()
}

/// Logs `claims_state` and Part I's `phase2_rows` once a second (D380, D497).
pub(super) fn spawn_report(ctx: &Arc<NodeContext>) -> JoinHandle<()> {
    let ctx = ctx.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            tick.tick().await;
            let event = ctx.state.with(|s| {
                let c = s.claims();
                let counters = c.counters();
                Event::ClaimsState {
                    last_applied: s.last_index(),
                    digest: c.content_digest(),
                    offers: c.offers_len(),
                    reconciling: c.reconciliations_len(),
                    claimed: counters.claimed,
                    refused: counters.refused,
                    resolved: counters.resolved,
                }
            });
            ctx.log.log(event);
            // Part I's rows for the same resources, and the volunteers beside them (D497).
            let rows = ctx.state.with(|s| {
                let c = s.phase2_counts();
                Event::Phase2Rows {
                    last_applied: s.last_index(),
                    volunteers: c.volunteers,
                    volunteers_live: c.volunteers_live,
                    volunteers_held: c.volunteers_held,
                    offer_units: c.offer_units,
                    offer_units_live: c.offer_units_live,
                    offer_units_held: c.offer_units_held,
                }
            });
            ctx.log.log(rows);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_resource_has_one_proposer_and_resources_spread_over_the_members() {
        let members = [1, 2, 3];
        let mut per_node = [0u32; 3];
        for provider in 1..=10 {
            for seq in 0..300 {
                let resource = ResourceId { provider, seq };
                let chosen = proposer(&members, resource).unwrap();
                assert_eq!(
                    proposer(&members, resource),
                    Some(chosen),
                    "the same every time"
                );
                per_node[(chosen - 1) as usize] += 1;
            }
        }
        for count in per_node {
            assert!((900..1100).contains(&count), "{per_node:?}");
        }
    }
}
