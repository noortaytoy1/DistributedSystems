//! Resource offers and claims as leases with fencing tokens (Phase 2, Part J, D370-D380).
//!
//! [`CloudState`](crate::state::CloudState) holds one [`Claims`] and hands it the four claims
//! commands, so every node applies the same commands in the same order and holds the same
//! claims. Like `discovery.rs` this file is pure: no network, no clock, no randomness, and
//! `BTreeMap`s only. The caller passes in the current time, read from
//! [`Directory::clock_ms`](crate::discovery::Directory::clock_ms) (D371): claims use the same
//! log-round clock the presence directory does, not a second one.
//!
//! - **One offer, many claims (D374).** An offer names a capacity; claims on it are counted, and
//!   a claim is refused only once the total would go over capacity.
//! - **Fencing tokens (D375).** A claim's token is the log index it was accepted at. A release or
//!   a resolve naming the wrong token is a no-op: the claim was already released, or superseded.
//! - **First in log order wins (D376).** An over-capacity claim from an online client is refused
//!   at once. One marked `speculative` (first made while its claimant was offline, D377) instead
//!   opens a reconciliation entry: the claimant already acted on believing it held the resource,
//!   so the conflict is surfaced, not silently dropped.
//! - **Reconciliation (D378).** Only the offer's own owner may resolve an entry, naming either the
//!   challenger (the incumbents it conflicts with are evicted, most recently accepted first, to
//!   make room) or the incumbents (the challenger's claim is finally refused). An entry nobody
//!   resolves is dropped once the offer itself expires, the same lazy-expiry style as the
//!   presence directory's grace period: nothing watches for it, a later read just stops finding it.
//! - **Expiry (D379).** A claim lives until its offer's own `available_until_ms` or an explicit
//!   release: one expiry, not a second lease duration to keep in sync with the offer.
//!
//! Owner: J (resource offers, leases and claims).

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

use crate::proto::{ClaimOutcome, ReconciliationWinner, ResourceId, ResourceOffer, UserId};

/// One claimant's hold on a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct ClaimEntry {
    quantity: u32,
    /// The log index this claim was accepted at: its fencing token.
    token: u64,
}

/// One attempt to claim a resource, bundled so [`Claims::claim`] keeps a reasonable argument
/// count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClaimAttempt {
    pub resource: ResourceId,
    pub claimant: UserId,
    pub quantity: u32,
    /// Marks a claim first made while `claimant`'s client was offline (D377).
    pub speculative: bool,
    /// The claimant's own clock when it first tried: kept only for the reconciliation report,
    /// never to order anything (D376).
    pub at_ms: u64,
}

/// A speculative claim that did not fit, waiting on the owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Reconciliation {
    quantity: u32,
    /// The claimant's own clock when it first tried, for the report (D380). Never used to decide
    /// anything: the log order already has.
    at_ms: u64,
    /// The log index this entry was opened at: names it for `resolve`.
    token: u64,
}

/// How many commands of each kind did what, for reports and tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimsCounters {
    pub offered: u64,
    pub claimed: u64,
    /// Over-capacity claims from an online claimant, refused at once.
    pub refused: u64,
    /// Over-capacity speculative claims that opened a reconciliation entry.
    pub reconciling: u64,
    pub released: u64,
    /// Releases whose token did not match the active claim.
    pub stale_release: u64,
    pub resolved: u64,
    /// Claims, releases or resolves naming a resource or entry this node does not hold.
    pub unknown: u64,
}

/// Resource offers and the claims on them, as one state machine on the shared log (D370).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    offers: BTreeMap<ResourceId, ResourceOffer>,
    /// Active claims per resource, by claimant: at most one active claim per claimant per
    /// resource (a second claim from the same claimant replaces the first).
    active: BTreeMap<ResourceId, BTreeMap<UserId, ClaimEntry>>,
    /// Open reconciliation entries per resource, by the challenger that opened them.
    reconciling: BTreeMap<ResourceId, BTreeMap<UserId, Reconciliation>>,
    counters: ClaimsCounters,
}

impl Claims {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record `offer`, or replace this provider's own offer at the same ID with new terms.
    /// Applying the same offer twice leaves one record (D371).
    pub fn offer(&mut self, index: u64, offer: &ResourceOffer) -> ClaimOutcome {
        self.offers.insert(offer.id, offer.clone());
        self.counters.offered += 1;
        ClaimOutcome::Offered {
            capacity: offer.capacity,
            log_index: index,
        }
    }

    /// Quantity currently held against `resource`, this claimant's own hold excepted.
    fn held_by_others(&self, resource: ResourceId, claimant: UserId) -> u32 {
        self.active.get(&resource).map_or(0, |claims| {
            claims
                .iter()
                .filter(|(user, _)| **user != claimant)
                .map(|(_, entry)| entry.quantity)
                .sum()
        })
    }

    /// Claim `attempt.quantity` units of `attempt.resource`, in log order (D374, D376).
    /// `clock_ms` is the directory's log clock (D371).
    pub fn claim(&mut self, index: u64, clock_ms: u64, attempt: ClaimAttempt) -> ClaimOutcome {
        self.claim_beside(index, clock_ms, attempt, 0)
    }

    /// [`claim`](Self::claim), with `reserved` units of the offer already held by the planner,
    /// which a claim must leave alone (Part I, D494). Units are counted the same whether a claim
    /// or a plan took them, so the two together never pass the capacity.
    pub fn claim_beside(
        &mut self,
        index: u64,
        clock_ms: u64,
        attempt: ClaimAttempt,
        reserved: u32,
    ) -> ClaimOutcome {
        let ClaimAttempt {
            resource,
            claimant,
            quantity,
            speculative,
            at_ms,
        } = attempt;
        let Some(offer) = self.offers.get(&resource) else {
            self.counters.unknown += 1;
            return ClaimOutcome::Unknown { log_index: index };
        };
        if offer.available_until_ms <= clock_ms {
            self.counters.unknown += 1;
            return ClaimOutcome::Unknown { log_index: index };
        }
        let capacity = offer.capacity;
        let held = self
            .held_by_others(resource, claimant)
            .saturating_add(reserved);
        if held.saturating_add(quantity) <= capacity {
            let entry = ClaimEntry {
                quantity,
                token: index,
            };
            self.active
                .entry(resource)
                .or_default()
                .insert(claimant, entry);
            self.counters.claimed += 1;
            return ClaimOutcome::Claimed {
                token: index,
                log_index: index,
            };
        }
        if speculative {
            self.reconciling.entry(resource).or_default().insert(
                claimant,
                Reconciliation {
                    quantity,
                    at_ms,
                    token: index,
                },
            );
            self.counters.reconciling += 1;
            return ClaimOutcome::Reconciling {
                token: index,
                log_index: index,
            };
        }
        self.counters.refused += 1;
        ClaimOutcome::Refused {
            available: capacity.saturating_sub(held),
            log_index: index,
        }
    }

    /// Give up the claim on `resource` held under `token`. A no-op if it no longer matches the
    /// active claim (D375).
    pub fn release(
        &mut self,
        index: u64,
        resource: ResourceId,
        claimant: UserId,
        token: u64,
    ) -> ClaimOutcome {
        let matches = self
            .active
            .get(&resource)
            .and_then(|claims| claims.get(&claimant))
            .is_some_and(|entry| entry.token == token);
        if !matches {
            self.counters.stale_release += 1;
            return ClaimOutcome::StaleRelease { log_index: index };
        }
        if let Some(claims) = self.active.get_mut(&resource) {
            claims.remove(&claimant);
            if claims.is_empty() {
                self.active.remove(&resource);
            }
        }
        self.counters.released += 1;
        ClaimOutcome::Released { log_index: index }
    }

    /// Settle the reconciliation entry named `token` on `resource`. Only `resolver` being the
    /// offer's own owner may (D378); naming the challenger evicts enough incumbents, most
    /// recently accepted first, to make room.
    pub fn resolve(
        &mut self,
        index: u64,
        resource: ResourceId,
        resolver: UserId,
        token: u64,
        winner: ReconciliationWinner,
    ) -> ClaimOutcome {
        let Some(offer) = self.offers.get(&resource) else {
            self.counters.unknown += 1;
            return ClaimOutcome::Unknown { log_index: index };
        };
        if offer.provider != resolver {
            return ClaimOutcome::NotOwner { log_index: index };
        }
        let Some(entries) = self.reconciling.get_mut(&resource) else {
            self.counters.unknown += 1;
            return ClaimOutcome::Unknown { log_index: index };
        };
        let Some((&challenger, &entry)) = entries.iter().find(|(_, r)| r.token == token) else {
            self.counters.unknown += 1;
            return ClaimOutcome::Unknown { log_index: index };
        };
        entries.remove(&challenger);
        if entries.is_empty() {
            self.reconciling.remove(&resource);
        }
        if winner == ReconciliationWinner::Challenger {
            self.make_room(resource, entry.quantity);
            self.active.entry(resource).or_default().insert(
                challenger,
                ClaimEntry {
                    quantity: entry.quantity,
                    token: index,
                },
            );
        }
        self.counters.resolved += 1;
        ClaimOutcome::Resolved {
            winner,
            challenger,
            opened_at_ms: entry.at_ms,
            log_index: index,
        }
    }

    /// Evicts the most recently accepted claims on `resource` until at least `needed` units are
    /// free, so a resolve in the challenger's favour always fits (a challenger's quantity can
    /// never exceed the offer's own capacity).
    fn make_room(&mut self, resource: ResourceId, needed: u32) {
        let Some(claims) = self.active.get_mut(&resource) else {
            return;
        };
        let mut by_token: Vec<(UserId, ClaimEntry)> =
            claims.iter().map(|(user, entry)| (*user, *entry)).collect();
        by_token.sort_by(|a, b| b.1.token.cmp(&a.1.token));
        let mut freed = 0u32;
        for (user, entry) in by_token {
            if freed >= needed {
                break;
            }
            claims.remove(&user);
            freed += entry.quantity;
        }
        if claims.is_empty() {
            self.active.remove(&resource);
        }
    }

    /// The offer at this ID, if this node still holds one.
    pub fn offer_at(&self, resource: &ResourceId) -> Option<&ResourceOffer> {
        self.offers.get(resource)
    }

    /// Quantity currently claimed against `resource`, live or not: a read, never a decision
    /// (D142's rule applied here too).
    pub fn claimed(&self, resource: &ResourceId) -> u32 {
        self.active
            .get(resource)
            .map_or(0, |claims| claims.values().map(|e| e.quantity).sum())
    }

    /// Open reconciliation entries on `resource`.
    pub fn reconciling_on(&self, resource: &ResourceId) -> usize {
        self.reconciling.get(resource).map_or(0, |m| m.len())
    }

    /// The token of one open reconciliation entry on `resource`, for a caller (an operator, or
    /// `ctl`) that needs to name it in a resolve. `None` if there is none; the lowest challenger
    /// first if there is more than one.
    pub fn reconciling_token(&self, resource: &ResourceId) -> Option<u64> {
        self.reconciling
            .get(resource)
            .and_then(|m| m.values().next())
            .map(|entry| entry.token)
    }

    pub fn offers_len(&self) -> usize {
        self.offers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.offers.is_empty()
    }

    pub fn reconciliations_len(&self) -> usize {
        self.reconciling.values().map(|m| m.len()).sum()
    }

    pub fn counters(&self) -> ClaimsCounters {
        self.counters
    }

    /// A hash of the offers and active claims, and nothing else: two nodes whose claims hold the
    /// same thing have the same value, whatever else their logs did. FNV-1a, as the state digest,
    /// so it is the same on every node.
    pub fn content_digest(&self) -> u64 {
        let mut h = Fnv(0xcbf2_9ce4_8422_2325);
        for (id, offer) in &self.offers {
            id.hash(&mut h);
            offer.hash(&mut h);
        }
        for (id, claims) in &self.active {
            id.hash(&mut h);
            for (user, entry) in claims {
                user.hash(&mut h);
                entry.hash(&mut h);
            }
        }
        h.finish()
    }
}

struct Fnv(u64);

impl Hasher for Fnv {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{Capability, ClientId, Point, ZoneId};

    const ZONE: ZoneId = 1;

    fn user(client: ClientId, n: u32) -> UserId {
        UserId { client, user: n }
    }

    fn attempt(
        resource: ResourceId,
        claimant: UserId,
        quantity: u32,
        speculative: bool,
        at_ms: u64,
    ) -> ClaimAttempt {
        ClaimAttempt {
            resource,
            claimant,
            quantity,
            speculative,
            at_ms,
        }
    }

    fn offer(provider: ClientId, seq: u64, capacity: u32, until_ms: u64) -> ResourceOffer {
        ResourceOffer {
            id: ResourceId { provider, seq },
            provider: user(provider, 0),
            capability: Capability::Medical,
            zone: ZONE,
            location: Point { x: 0, y: 0 },
            capacity,
            available_until_ms: until_ms,
        }
    }

    #[test]
    fn offering_twice_leaves_one_record() {
        let mut c = Claims::new();
        let o = offer(1, 1, 2, 10_000);
        c.offer(1, &o);
        c.offer(2, &o);
        assert_eq!(c.offers_len(), 1);
        assert_eq!(c.counters().offered, 2);
    }

    #[test]
    fn claims_up_to_capacity_are_all_granted() {
        let mut c = Claims::new();
        let resource = offer(1, 1, 2, 10_000).id;
        c.offer(1, &offer(1, 1, 2, 10_000));
        let a = user(2, 0);
        let b = user(3, 0);
        assert!(matches!(
            c.claim(2, 0, attempt(resource, a, 1, false, 0)),
            ClaimOutcome::Claimed { .. }
        ));
        assert!(matches!(
            c.claim(3, 0, attempt(resource, b, 1, false, 0)),
            ClaimOutcome::Claimed { .. }
        ));
        assert_eq!(c.claimed(&resource), 2);
    }

    #[test]
    fn an_online_claim_over_capacity_is_refused_with_whats_left() {
        let mut c = Claims::new();
        let resource = offer(1, 1, 2, 10_000).id;
        c.offer(1, &offer(1, 1, 2, 10_000));
        c.claim(2, 0, attempt(resource, user(2, 0), 2, false, 0));
        let outcome = c.claim(3, 0, attempt(resource, user(3, 0), 1, false, 0));
        assert_eq!(
            outcome,
            ClaimOutcome::Refused {
                available: 0,
                log_index: 3
            }
        );
        assert_eq!(c.counters().refused, 1);
    }

    #[test]
    fn a_speculative_claim_over_capacity_opens_reconciliation_instead_of_refusing() {
        let mut c = Claims::new();
        let resource = offer(1, 1, 1, 10_000).id;
        c.offer(1, &offer(1, 1, 1, 10_000));
        c.claim(2, 0, attempt(resource, user(2, 0), 1, false, 0));
        let outcome = c.claim(3, 0, attempt(resource, user(3, 0), 1, true, 500));
        assert_eq!(
            outcome,
            ClaimOutcome::Reconciling {
                token: 3,
                log_index: 3
            }
        );
        assert_eq!(c.reconciling_on(&resource), 1);
        assert_eq!(c.counters().reconciling, 1);
        // The incumbent still holds it: a reconciliation entry does not grant the challenger.
        assert_eq!(c.claimed(&resource), 1);
    }

    #[test]
    fn re_claiming_by_the_same_claimant_does_not_double_count() {
        let mut c = Claims::new();
        let resource = offer(1, 1, 1, 10_000).id;
        c.offer(1, &offer(1, 1, 1, 10_000));
        let a = user(2, 0);
        c.claim(2, 0, attempt(resource, a, 1, false, 0));
        let outcome = c.claim(3, 0, attempt(resource, a, 1, false, 0));
        assert!(matches!(outcome, ClaimOutcome::Claimed { .. }));
        assert_eq!(c.claimed(&resource), 1);
    }

    #[test]
    fn a_claim_on_an_expired_offer_is_unknown() {
        let mut c = Claims::new();
        let resource = offer(1, 1, 1, 1_000).id;
        c.offer(1, &offer(1, 1, 1, 1_000));
        let outcome = c.claim(2, 1_000, attempt(resource, user(2, 0), 1, false, 0));
        assert_eq!(outcome, ClaimOutcome::Unknown { log_index: 2 });
    }

    #[test]
    fn release_with_the_right_token_frees_capacity() {
        let mut c = Claims::new();
        let resource = offer(1, 1, 1, 10_000).id;
        c.offer(1, &offer(1, 1, 1, 10_000));
        let a = user(2, 0);
        let ClaimOutcome::Claimed { token, .. } = c.claim(2, 0, attempt(resource, a, 1, false, 0))
        else {
            panic!("expected Claimed");
        };
        let outcome = c.release(3, resource, a, token);
        assert_eq!(outcome, ClaimOutcome::Released { log_index: 3 });
        assert_eq!(c.claimed(&resource), 0);
    }

    #[test]
    fn release_with_a_stale_token_is_a_no_op() {
        let mut c = Claims::new();
        let resource = offer(1, 1, 1, 10_000).id;
        c.offer(1, &offer(1, 1, 1, 10_000));
        let a = user(2, 0);
        let ClaimOutcome::Claimed { token, .. } = c.claim(2, 0, attempt(resource, a, 1, false, 0))
        else {
            panic!("expected Claimed");
        };
        // A stray resend of the same release, after a fresh claim already replaced it.
        c.release(3, resource, a, token);
        c.claim(4, 0, attempt(resource, a, 1, false, 0));
        let outcome = c.release(5, resource, a, token);
        assert_eq!(outcome, ClaimOutcome::StaleRelease { log_index: 5 });
        assert_eq!(c.claimed(&resource), 1);
    }

    #[test]
    fn resolve_for_incumbents_closes_the_entry_and_changes_nothing() {
        let mut c = Claims::new();
        let resource = offer(1, 1, 1, 10_000).id;
        c.offer(1, &offer(1, 1, 1, 10_000));
        let incumbent = user(2, 0);
        let owner = user(1, 0);
        c.claim(2, 0, attempt(resource, incumbent, 1, false, 0));
        let ClaimOutcome::Reconciling { token, .. } =
            c.claim(3, 0, attempt(resource, user(3, 0), 1, true, 0))
        else {
            panic!("expected Reconciling");
        };
        let outcome = c.resolve(4, resource, owner, token, ReconciliationWinner::Incumbents);
        assert_eq!(
            outcome,
            ClaimOutcome::Resolved {
                winner: ReconciliationWinner::Incumbents,
                challenger: user(3, 0),
                opened_at_ms: 0,
                log_index: 4
            }
        );
        assert_eq!(c.reconciling_on(&resource), 0);
        assert_eq!(c.claimed(&resource), 1);
    }

    #[test]
    fn resolve_for_the_challenger_evicts_the_incumbent_and_grants_it() {
        let mut c = Claims::new();
        let resource = offer(1, 1, 1, 10_000).id;
        c.offer(1, &offer(1, 1, 1, 10_000));
        let incumbent = user(2, 0);
        let challenger = user(3, 0);
        let owner = user(1, 0);
        c.claim(2, 0, attempt(resource, incumbent, 1, false, 0));
        let ClaimOutcome::Reconciling { token, .. } =
            c.claim(3, 0, attempt(resource, challenger, 1, true, 0))
        else {
            panic!("expected Reconciling");
        };
        c.resolve(4, resource, owner, token, ReconciliationWinner::Challenger);
        assert_eq!(c.claimed(&resource), 1);
        // The challenger holds it now, with a fresh token from the resolve's own index.
        let outcome = c.release(5, resource, challenger, 4);
        assert_eq!(outcome, ClaimOutcome::Released { log_index: 5 });
    }

    #[test]
    fn resolve_from_someone_other_than_the_owner_is_refused() {
        let mut c = Claims::new();
        let resource = offer(1, 1, 1, 10_000).id;
        c.offer(1, &offer(1, 1, 1, 10_000));
        c.claim(2, 0, attempt(resource, user(2, 0), 1, false, 0));
        let ClaimOutcome::Reconciling { token, .. } =
            c.claim(3, 0, attempt(resource, user(3, 0), 1, true, 0))
        else {
            panic!("expected Reconciling");
        };
        let outcome = c.resolve(
            4,
            resource,
            user(9, 0),
            token,
            ReconciliationWinner::Challenger,
        );
        assert_eq!(outcome, ClaimOutcome::NotOwner { log_index: 4 });
        // Unresolved: the entry is still open.
        assert_eq!(c.reconciling_on(&resource), 1);
    }
}
