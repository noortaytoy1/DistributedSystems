//! Phase 2 resources as rows of the responder table (Phase 2, Part I, D491-D498).
//!
//! The planner sends a [`ResponderId`], and everything after it (holds, releases, choosing again
//! at apply, status, gossip's assignment updates, metrics) reads one. So an online volunteer from
//! the presence directory (Part G) and a resource offer (Part J) each get rows in the same table
//! as the synthetic campus, with IDs from [`FIRST_PHASE2_ID`] up, and the rest of the system needs
//! no new case (D491). This file keeps which row is whose; [`CloudState`](crate::state::CloudState)
//! makes and frees them as it applies the presence and offer commands, and decides from the
//! directory and the claims whether a row may be sent now.
//!
//! - **IDs (D491).** Minted in log order from a counter, so every node gives the same resource
//!   the same ID. A user or a resource keeps its rows for as long as it has them.
//! - **Volunteers (D492, D493).** One row per volunteer, at its zone's centre, with every
//!   capability it lists. The planner sees one entry per capability, all with the row's ID and
//!   next to each other ([`Phase2Rows::entries`]), so a person is one hold and one free responder.
//! - **Offers (D494).** One row per unit, at most [`MAX_OFFER_UNITS`]. Planner holds and Part J's
//!   claims share the capacity: a unit is free only while holds plus claimed units are below it.
//! - **Gone (D495).** A row stays while it is held, hidden once its source is gone, and is
//!   removed when it is both gone and free.
//!
//! Pure: no network, no clock, no randomness, and `BTreeMap`s only.
//!
//! Owner: I (zone publish-subscribe, and the planner's Phase 2 inputs).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::campus;
use crate::proto::{
    Capability, PresenceRecord, PresenceRole, ResourceId, ResourceOffer, Responder, ResponderId,
    UserId, ZoneId,
};
use crate::state::ResponderSlot;

/// The first ID a Phase 2 row gets. The synthetic campus numbers its responders from 1, far below
/// this, so the two never meet, and every Phase 1 read of the table can stop here (D491, D496).
pub const FIRST_PHASE2_ID: ResponderId = 1 << 31;

/// Most rows one offer gets, whatever its capacity: an offer of a million units must not add a
/// million rows to every node's table (D494). Units above this are still counted for claims.
pub const MAX_OFFER_UNITS: u32 = 16;

/// Whether `id` is a Phase 2 row.
pub fn is_phase2(id: ResponderId) -> bool {
    id >= FIRST_PHASE2_ID
}

/// What a Phase 2 row stands for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    /// An online volunteer responder, with every capability its record lists, sorted.
    Volunteer {
        user: UserId,
        capabilities: Vec<Capability>,
    },
    /// One unit of a resource offer.
    Offer { resource: ResourceId },
}

impl Source {
    /// `volunteer` or `offer`, for `phase2_assigned` (D497).
    pub fn kind(&self) -> &'static str {
        match self {
            Source::Volunteer { .. } => "volunteer",
            Source::Offer { .. } => "offer",
        }
    }

    /// The user or the resource, as text, for `phase2_assigned` (D497).
    pub fn name(&self) -> String {
        match self {
            Source::Volunteer { user, .. } => user.to_string(),
            Source::Offer { resource } => resource.to_string(),
        }
    }
}

/// Which Phase 2 row is whose. Held by `CloudState`, beside the rows themselves.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Phase2Rows {
    /// IDs handed out so far; the next one is `FIRST_PHASE2_ID + minted`.
    minted: u64,
    volunteers: BTreeMap<UserId, ResponderId>,
    /// An offer's unit rows, oldest first.
    offers: BTreeMap<ResourceId, Vec<ResponderId>>,
    sources: BTreeMap<ResponderId, Source>,
}

impl Phase2Rows {
    /// How many Phase 2 rows there are.
    pub fn len(&self) -> usize {
        self.sources.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// What row `id` stands for, if it is a Phase 2 row.
    pub fn source(&self, id: ResponderId) -> Option<&Source> {
        self.sources.get(&id)
    }

    /// Every Phase 2 row and its source, in ID order.
    pub fn sources(&self) -> impl Iterator<Item = (ResponderId, &Source)> {
        self.sources.iter().map(|(id, source)| (*id, source))
    }

    /// An offer's unit rows, oldest first.
    pub fn offer_rows(&self, resource: &ResourceId) -> &[ResponderId] {
        self.offers.get(resource).map_or(&[], Vec::as_slice)
    }

    /// Units of `resource` the planner holds now: what claims must leave room for (D494).
    pub fn held_units(
        &self,
        table: &BTreeMap<ResponderId, ResponderSlot>,
        resource: &ResourceId,
    ) -> u32 {
        let held = self
            .offer_rows(resource)
            .iter()
            .filter(|id| table.get(id).is_some_and(|slot| slot.held_by.is_some()))
            .count();
        u32::try_from(held).unwrap_or(u32::MAX)
    }

    /// The planner's entries for one row: one per capability for a volunteer, all with the row's
    /// ID and next to each other (D493), and the row itself for an offer unit.
    pub fn entries(&self, slot: &ResponderSlot) -> Vec<Responder> {
        match self.sources.get(&slot.responder.id) {
            Some(Source::Volunteer { capabilities, .. }) => capabilities
                .iter()
                .map(|capability| Responder {
                    capability: *capability,
                    ..slot.responder.clone()
                })
                .collect(),
            _ => vec![slot.responder.clone()],
        }
    }

    /// Bring a volunteer's row in line with its directory record after a presence command: made
    /// or updated if the record is a volunteer with a capability, freed otherwise (D492, D493).
    pub fn sync_volunteer(
        &mut self,
        table: &mut BTreeMap<ResponderId, ResponderSlot>,
        user: UserId,
        record: Option<&PresenceRecord>,
        zones: ZoneId,
    ) {
        let record = record
            .filter(|r| r.role == PresenceRole::VolunteerResponder && !r.capabilities.is_empty());
        let Some(record) = record else {
            if let Some(id) = self.volunteers.get(&user).copied() {
                self.remove_if_free(table, id);
            }
            return;
        };
        let mut capabilities = record.capabilities.clone();
        capabilities.sort();
        capabilities.dedup();
        let id = match self.volunteers.get(&user) {
            Some(id) => *id,
            None => {
                let Some(id) = self.mint() else { return };
                self.volunteers.insert(user, id);
                id
            }
        };
        let responder = Responder {
            id,
            zone: record.zone,
            location: campus::zone_centre(zones, record.zone),
            capability: capabilities[0],
        };
        upsert(table, responder);
        self.sources
            .insert(id, Source::Volunteer { user, capabilities });
    }

    /// Bring an offer's unit rows in line with the offer just applied: one per unit up to
    /// [`MAX_OFFER_UNITS`], each where the offer is (D494). Rows above a lowered capacity go if
    /// free; a held one stays until it is released.
    pub fn sync_offer(
        &mut self,
        table: &mut BTreeMap<ResponderId, ResponderSlot>,
        offer: &ResourceOffer,
    ) {
        let wanted = offer.capacity.min(MAX_OFFER_UNITS) as usize;
        let mut rows = self.offers.remove(&offer.id).unwrap_or_default();
        while rows.len() < wanted {
            let Some(id) = self.mint() else { break };
            rows.push(id);
        }
        let held = |table: &BTreeMap<ResponderId, ResponderSlot>, id: &ResponderId| {
            table.get(id).is_some_and(|slot| slot.held_by.is_some())
        };
        // Held rows always stay; free ones fill what is left of `wanted`, oldest first.
        let mut room = wanted.saturating_sub(rows.iter().filter(|id| held(table, id)).count());
        let mut keep = Vec::with_capacity(rows.len());
        for id in rows {
            let free = !held(table, &id);
            if free && room == 0 {
                table.remove(&id);
                self.sources.remove(&id);
                continue;
            }
            upsert(
                table,
                Responder {
                    id,
                    zone: offer.zone,
                    location: offer.location,
                    capability: offer.capability,
                },
            );
            self.sources
                .insert(id, Source::Offer { resource: offer.id });
            if free {
                room -= 1;
            }
            keep.push(id);
        }
        if !keep.is_empty() {
            self.offers.insert(offer.id, keep);
        }
    }

    /// Remove every row whose source `gone` says is gone and that nobody holds (D495).
    pub fn sweep(
        &mut self,
        table: &mut BTreeMap<ResponderId, ResponderSlot>,
        gone: impl Fn(&Source) -> bool,
    ) {
        let gone: Vec<ResponderId> = self
            .sources
            .iter()
            .filter(|(_, source)| gone(source))
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            self.remove_if_free(table, id);
        }
    }

    fn remove_if_free(
        &mut self,
        table: &mut BTreeMap<ResponderId, ResponderSlot>,
        id: ResponderId,
    ) {
        if table.get(&id).is_some_and(|slot| slot.held_by.is_some()) {
            return;
        }
        table.remove(&id);
        match self.sources.remove(&id) {
            Some(Source::Volunteer { user, .. }) => {
                self.volunteers.remove(&user);
            }
            Some(Source::Offer { resource }) => {
                if let Some(rows) = self.offers.get_mut(&resource) {
                    rows.retain(|row| *row != id);
                    if rows.is_empty() {
                        self.offers.remove(&resource);
                    }
                }
            }
            None => {}
        }
    }

    /// The next ID, or `None` once all 2^31 are used: then no more rows are made, and the
    /// resource is simply not planned with.
    fn mint(&mut self) -> Option<ResponderId> {
        let id = u64::from(FIRST_PHASE2_ID).checked_add(self.minted)?;
        let id = ResponderId::try_from(id).ok()?;
        self.minted += 1;
        Some(id)
    }
}

/// Put `responder` in the table, keeping a hold the row already has.
fn upsert(table: &mut BTreeMap<ResponderId, ResponderSlot>, responder: Responder) {
    table
        .entry(responder.id)
        .and_modify(|slot| slot.responder = responder.clone())
        .or_insert_with(|| ResponderSlot {
            responder,
            held_by: None,
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::{self, CostKnob};
    use crate::proto::{
        Command, Incident, Outcome, Plan, Point, PresenceLease, RequestId, ResultProposal,
    };
    use crate::state::{CloudState, sample_mix};

    const ZONES: ZoneId = 6;
    const LEASE_MS: u64 = 1_000;

    fn user(n: u32) -> UserId {
        UserId { client: 7, user: n }
    }

    fn record(n: u32, role: PresenceRole, zone: ZoneId, caps: &[Capability]) -> PresenceRecord {
        PresenceRecord {
            user: user(n),
            role,
            zone,
            peer_addr: "10.0.0.7:20000".into(),
            capabilities: caps.to_vec(),
            incarnation: 1,
        }
    }

    fn register(record: PresenceRecord, at_ms: u64) -> Command {
        Command::PresenceRegister {
            record,
            lease: PresenceLease {
                at_ms,
                lease_ms: LEASE_MS,
                grace_ms: LEASE_MS,
            },
        }
    }

    fn volunteer(n: u32, zone: ZoneId, caps: &[Capability], at_ms: u64) -> Command {
        register(
            record(n, PresenceRole::VolunteerResponder, zone, caps),
            at_ms,
        )
    }

    fn resource(seq: u64) -> ResourceId {
        ResourceId { provider: 8, seq }
    }

    fn offer(seq: u64, capacity: u32, capability: Capability, until_ms: u64) -> Command {
        Command::OfferResource {
            offer: ResourceOffer {
                id: resource(seq),
                provider: UserId { client: 8, user: 0 },
                capability,
                zone: 0,
                location: Point { x: 40, y: 40 },
                capacity,
                available_until_ms: until_ms,
            },
        }
    }

    fn claim(seq: u64, claimant: u32, quantity: u32) -> Command {
        Command::ClaimResource {
            resource: resource(seq),
            claimant: user(claimant),
            quantity,
            speculative: false,
            at_ms: 0,
        }
    }

    fn incident(needs: Capability, severity: u8) -> Incident {
        Incident {
            owner: UserId { client: 1, user: 0 },
            category: needs.categories()[0],
            zone: 0,
            location: Point { x: 50, y: 50 },
            severity,
            needs,
            reported_at_ms: 0,
            ttl_ms: 60_000,
        }
    }

    fn commit(seq: u64, needs: Capability, responder: Option<ResponderId>) -> Command {
        Command::CommitResult(ResultProposal {
            request: RequestId { client: 1, seq },
            incident: incident(needs, 3),
            executor: 1,
            term: 1,
            plan: Plan {
                responder,
                cost: 0,
                priority: 300,
            },
            age_ms: 0,
            assigned_at_ms: 0,
        })
    }

    fn campus(ids: impl IntoIterator<Item = ResponderId>, capability: Capability) -> Command {
        Command::RegisterResponders {
            responders: ids
                .into_iter()
                .map(|id| Responder {
                    id,
                    zone: 1,
                    location: Point { x: 900, y: 900 },
                    capability,
                })
                .collect(),
        }
    }

    /// A state with the config's zone count, and `cmds` applied at indexes 1, 2, ...
    fn state(cmds: &[Command]) -> CloudState {
        let mut s = CloudState::new();
        s.set_campus_zones(ZONES);
        for (i, cmd) in cmds.iter().enumerate() {
            s.apply(i as u64 + 1, cmd);
        }
        s
    }

    fn apply(s: &mut CloudState, cmd: &Command) -> Outcome {
        s.apply(s.last_index() + 1, cmd)
    }

    fn free(s: &CloudState) -> Vec<Responder> {
        s.free_phase2_part(0, 256, 512)
    }

    fn committed_responder(outcome: Outcome) -> Option<ResponderId> {
        match outcome {
            Outcome::Committed(done) => done.plan.responder,
            other => panic!("expected a commit, got {other:?}"),
        }
    }

    #[test]
    fn a_volunteer_gets_one_row_at_its_zone_centre_with_every_skill() {
        let s = state(&[
            campus([1, 2, 3], Capability::Fire),
            volunteer(
                1,
                4,
                &[Capability::Rescue, Capability::Medical, Capability::Rescue],
                0,
            ),
        ]);
        assert_eq!(s.responder_count(), 3, "Phase 2 rows are not campus rows");
        assert_eq!(s.phase2().len(), 1);
        let slot = s.responder(FIRST_PHASE2_ID).expect("the first Phase 2 ID");
        assert_eq!(slot.responder.zone, 4);
        assert_eq!(slot.responder.location, campus::zone_centre(ZONES, 4));
        assert_eq!(
            s.phase2().source(FIRST_PHASE2_ID),
            Some(&Source::Volunteer {
                user: user(1),
                capabilities: vec![Capability::Medical, Capability::Rescue],
            })
        );
        let entries = free(&s);
        assert_eq!(entries.len(), 2, "one entry per distinct skill");
        assert!(entries.iter().all(|r| r.id == FIRST_PHASE2_ID));
    }

    #[test]
    fn a_volunteer_with_three_skills_counts_as_one_free_responder() {
        let s = state(&[volunteer(
            1,
            0,
            &[Capability::Medical, Capability::Fire, Capability::Rescue],
            0,
        )]);
        let entries = free(&s);
        assert_eq!(entries.len(), 3);
        let inc = incident(Capability::Fire, 2);
        let seen_as_three = planner::plan(&inc, &entries, 0, CostKnob::default());
        let one = [Responder {
            capability: Capability::Fire,
            ..entries[0].clone()
        }];
        let seen_as_one = planner::plan(&inc, &one, 0, CostKnob::default());
        assert_eq!(seen_as_three.responder, Some(FIRST_PHASE2_ID));
        assert_eq!(
            seen_as_three, seen_as_one,
            "same pick and same availability points"
        );
    }

    #[test]
    fn every_skill_is_matched_and_one_hold_takes_the_person() {
        let mut s = state(&[volunteer(
            1,
            0,
            &[Capability::Medical, Capability::Rescue],
            0,
        )]);
        let plan = planner::plan(
            &incident(Capability::Rescue, 2),
            &free(&s),
            0,
            CostKnob::default(),
        );
        assert_eq!(
            plan.responder,
            Some(FIRST_PHASE2_ID),
            "its second skill matched"
        );
        let taken = apply(&mut s, &commit(1, Capability::Rescue, plan.responder));
        assert_eq!(committed_responder(taken), Some(FIRST_PHASE2_ID));
        assert!(
            free(&s).is_empty(),
            "every entry of the person goes with one hold"
        );
    }

    #[test]
    fn ordinary_users_and_volunteers_without_skills_get_no_row() {
        let s = state(&[
            register(record(1, PresenceRole::Ordinary, 0, &[Capability::Fire]), 0),
            volunteer(2, 0, &[], 0),
            register(
                record(3, PresenceRole::ResourceProvider, 0, &[Capability::Fire]),
                0,
            ),
        ]);
        assert!(s.phase2().is_empty());
    }

    #[test]
    fn registering_again_updates_the_same_row_and_becoming_ordinary_frees_it() {
        let mut s = state(&[volunteer(1, 0, &[Capability::Medical], 0)]);
        apply(&mut s, &volunteer(1, 3, &[Capability::Technical], 10));
        assert_eq!(s.phase2().len(), 1);
        let slot = s.responder(FIRST_PHASE2_ID).expect("same row");
        assert_eq!(slot.responder.zone, 3);
        assert_eq!(slot.responder.capability, Capability::Technical);
        apply(
            &mut s,
            &register(record(1, PresenceRole::Ordinary, 3, &[]), 20),
        );
        assert!(s.phase2().is_empty());
        assert!(s.responder(FIRST_PHASE2_ID).is_none());
    }

    #[test]
    fn a_volunteer_whose_lease_ended_is_hidden_and_a_tick_removes_it_once_free() {
        let mut s = state(&[
            volunteer(1, 0, &[Capability::Medical], 0),
            volunteer(2, 0, &[Capability::Medical], 0),
        ]);
        let held = FIRST_PHASE2_ID + 1;
        committed_responder(apply(&mut s, &commit(1, Capability::Medical, Some(held))));
        // Both leases end at 1000; the tick at 1500 is inside the grace, so both records stay.
        apply(&mut s, &Command::PresenceTick { at_ms: 1_500 });
        assert!(free(&s).is_empty(), "a late record is not sent");
        assert_eq!(
            s.phase2().len(),
            2,
            "late can still renew, so it keeps its row"
        );
        // Past the grace: the free row goes, the held one stays until it is released.
        apply(&mut s, &Command::PresenceTick { at_ms: 2_500 });
        assert_eq!(s.phase2().len(), 1);
        assert!(s.responder(held).is_some_and(|slot| slot.held_by.is_some()));
        apply(
            &mut s,
            &Command::ReleaseResponder {
                responder: held,
                request: RequestId { client: 1, seq: 1 },
            },
        );
        apply(&mut s, &Command::PresenceTick { at_ms: 2_600 });
        assert!(s.phase2().is_empty());
    }

    #[test]
    fn a_plan_for_a_volunteer_who_left_is_chosen_again() {
        let mut s = state(&[
            campus([1], Capability::Medical),
            volunteer(1, 0, &[Capability::Medical], 0),
        ]);
        apply(&mut s, &Command::PresenceTick { at_ms: 1_500 });
        let sent = committed_responder(apply(
            &mut s,
            &commit(1, Capability::Medical, Some(FIRST_PHASE2_ID)),
        ));
        assert_eq!(sent, Some(1), "the campus responder instead");
        assert_eq!(s.counters().repicked, 1);
    }

    #[test]
    fn an_offer_gets_one_row_per_unit_up_to_the_cap() {
        let s = state(&[
            offer(1, 3, Capability::Medical, 10_000),
            offer(2, 1_000_000, Capability::Fire, 10_000),
        ]);
        assert_eq!(s.phase2().offer_rows(&resource(1)).len(), 3);
        assert_eq!(
            s.phase2().offer_rows(&resource(2)).len(),
            MAX_OFFER_UNITS as usize
        );
        let slot = s
            .responder(s.phase2().offer_rows(&resource(1))[0])
            .expect("row");
        assert_eq!(slot.responder.location, Point { x: 40, y: 40 });
        assert_eq!(slot.responder.capability, Capability::Medical);
    }

    #[test]
    fn claims_leave_the_planner_only_the_units_left() {
        let mut s = state(&[offer(1, 3, Capability::Medical, 10_000), claim(1, 1, 2)]);
        assert_eq!(free(&s).len(), 1, "3 units, 2 claimed");
        let last = free(&s)[0].id;
        committed_responder(apply(&mut s, &commit(1, Capability::Medical, Some(last))));
        assert!(free(&s).is_empty());
        // Any other unit row is taken too: a plan for it is chosen again, and finds nobody.
        let other = s
            .phase2()
            .offer_rows(&resource(1))
            .iter()
            .copied()
            .find(|id| *id != last)
            .expect("3 rows");
        let sent = committed_responder(apply(&mut s, &commit(2, Capability::Medical, Some(other))));
        assert_eq!(sent, None);
    }

    #[test]
    fn the_planners_holds_leave_claims_only_the_units_left() {
        let mut s = state(&[offer(1, 2, Capability::Medical, 10_000)]);
        let unit = free(&s)[0].id;
        committed_responder(apply(&mut s, &commit(1, Capability::Medical, Some(unit))));
        assert!(matches!(
            apply(&mut s, &claim(1, 1, 1)),
            Outcome::Claim(crate::proto::ClaimOutcome::Claimed { .. })
        ));
        assert!(matches!(
            apply(&mut s, &claim(1, 2, 1)),
            Outcome::Claim(crate::proto::ClaimOutcome::Refused { available: 0, .. })
        ));
        assert!(free(&s).is_empty());
    }

    #[test]
    fn an_expired_offer_is_hidden_and_removed_once_free() {
        let mut s = state(&[offer(1, 2, Capability::Medical, 1_000)]);
        assert_eq!(free(&s).len(), 2);
        apply(&mut s, &Command::PresenceTick { at_ms: 1_000 });
        assert!(free(&s).is_empty());
        assert!(
            s.phase2().is_empty(),
            "nobody held it, so the tick removed it"
        );
    }

    #[test]
    fn a_lowered_capacity_drops_free_units_and_keeps_held_ones() {
        let mut s = state(&[offer(1, 3, Capability::Medical, 10_000)]);
        let rows = s.phase2().offer_rows(&resource(1)).to_vec();
        committed_responder(apply(
            &mut s,
            &commit(1, Capability::Medical, Some(rows[2])),
        ));
        apply(&mut s, &offer(1, 1, Capability::Medical, 10_000));
        assert_eq!(s.phase2().offer_rows(&resource(1)), &[rows[2]]);
        assert!(free(&s).is_empty(), "the one unit is held");
    }

    #[test]
    fn applying_presence_and_offers_twice_equals_once() {
        let cmds = [
            volunteer(1, 2, &[Capability::Fire, Capability::Medical], 0),
            offer(1, 4, Capability::Rescue, 10_000),
        ];
        let once = state(&cmds);
        let mut twice = state(&cmds);
        for cmd in &cmds {
            apply(&mut twice, cmd);
        }
        assert_eq!(once.phase2(), twice.phase2());
        assert_eq!(free(&once), free(&twice));
    }

    #[test]
    fn many_rows_are_sampled_in_their_own_part_and_never_in_the_campus_part() {
        let mut cmds = vec![campus(1..=300, Capability::Medical)];
        cmds.extend((0..400).map(|n| volunteer(n, 0, &[Capability::Medical], 0)));
        let s = state(&cmds);
        let a = s.free_phase2_part(sample_mix(1), 256, 100);
        let b = s.free_phase2_part(sample_mix(2), 256, 100);
        assert_eq!(a.len(), 100);
        assert_ne!(a, b, "different salts look at different rows");
        assert!(a.iter().chain(&b).all(|r| is_phase2(r.id)));
        for salt in 0..50 {
            let campus_part = s.free_responders_part(sample_mix(salt), 100);
            assert_eq!(campus_part.len(), 100);
            assert!(campus_part.iter().all(|r| !is_phase2(r.id)));
        }
        assert_eq!(s.responder_count(), 300);
        assert_eq!(s.free_responders().len(), 300);
    }

    #[test]
    fn a_plan_whose_campus_responder_was_taken_can_go_to_a_volunteer() {
        let mut s = state(&[
            campus([1], Capability::Medical),
            volunteer(1, 0, &[Capability::Medical], 0),
            commit(1, Capability::Medical, Some(1)),
        ]);
        let sent = committed_responder(apply(&mut s, &commit(2, Capability::Medical, Some(1))));
        assert_eq!(sent, Some(FIRST_PHASE2_ID));
    }

    #[test]
    fn counts_split_by_kind_live_and_held() {
        let mut s = state(&[
            volunteer(1, 0, &[Capability::Medical], 0),
            offer(1, 2, Capability::Fire, 10_000),
        ]);
        committed_responder(apply(
            &mut s,
            &commit(1, Capability::Medical, Some(FIRST_PHASE2_ID)),
        ));
        let c = s.phase2_counts();
        assert_eq!(
            (c.volunteers, c.volunteers_live, c.volunteers_held),
            (1, 1, 1)
        );
        assert_eq!(
            (c.offer_units, c.offer_units_live, c.offer_units_held),
            (2, 2, 0)
        );
    }

    #[test]
    fn phase2_rows_survive_a_snapshot() {
        let s = state(&[
            volunteer(1, 0, &[Capability::Medical], 0),
            offer(1, 2, Capability::Fire, 10_000),
        ]);
        let back: CloudState =
            bincode::deserialize(&bincode::serialize(&s).expect("serialize")).expect("read");
        assert_eq!(back, s);
        assert_eq!(free(&back), free(&s));
    }

    #[test]
    fn the_source_names_the_user_or_the_resource() {
        let s = state(&[
            volunteer(1, 0, &[Capability::Medical], 0),
            offer(5, 1, Capability::Fire, 10_000),
        ]);
        let kinds: Vec<(&str, String)> = s
            .phase2()
            .sources()
            .map(|(_, src)| (src.kind(), src.name()))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("volunteer", "7/1".into()),
                ("offer", resource(5).to_string())
            ]
        );
    }

    /// FNV-1a, so the pinned value below does not depend on the standard library's hasher.
    struct Fnv(u64);

    impl std::hash::Hasher for Fnv {
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

    /// Phase 1 is unchanged (D498): a log with only the campus gives the digest, the outcomes and
    /// the parts of the table that `main` gave before Phase 2 rows existed. The two values were
    /// computed on `main` at eb8e549 with this same function, and the branch gives the same.
    #[test]
    fn a_phase1_log_gives_what_it_gave_before_phase2_rows() {
        use std::hash::{Hash, Hasher};
        let cfg = crate::config::CampusConfig {
            responders: 600,
            ..crate::config::CampusConfig::default()
        };
        let mut s = CloudState::new();
        let mut i = 1;
        s.apply(
            i,
            &Command::RegisterResponders {
                responders: campus::responders(&cfg),
            },
        );
        let mut h = Fnv(0xcbf2_9ce4_8422_2325);
        for seq in 0..400u64 {
            i += 1;
            let needs = Capability::ALL[(seq % 4) as usize];
            let id = (seq % 50) as u32 + 1;
            let cmd = Command::CommitResult(ResultProposal {
                request: RequestId { client: 3, seq },
                incident: Incident {
                    owner: UserId { client: 3, user: 0 },
                    category: needs.categories()[0],
                    zone: (seq % 6) as u16,
                    location: Point {
                        x: (seq * 37 % 1500) as i32,
                        y: (seq * 91 % 1000) as i32,
                    },
                    severity: (seq % 5) as u8 + 1,
                    needs,
                    reported_at_ms: 0,
                    ttl_ms: 60_000,
                },
                executor: 1,
                term: 1,
                plan: Plan {
                    responder: Some(id),
                    cost: 1,
                    priority: 100,
                },
                age_ms: 0,
                assigned_at_ms: seq * 10,
            });
            let out = s.apply(i, &cmd);
            format!("{out:?}").hash(&mut h);
            if seq % 7 == 0 {
                i += 1;
                s.apply(
                    i,
                    &Command::ReleaseResponder {
                        responder: id,
                        request: RequestId {
                            client: 3,
                            seq: seq / 2,
                        },
                    },
                );
            }
        }
        for salt in 0..20 {
            format!("{:?}", s.free_responders_part(sample_mix(salt), 64)).hash(&mut h);
            format!("{:?}", s.responders_part(sample_mix(salt), 64)).hash(&mut h);
        }
        format!("{:?} {}", s.free_responders(), s.responder_count()).hash(&mut h);
        assert_eq!(
            (s.digest(), h.finish()),
            (14_414_007_029_443_976_425, 12_808_352_294_428_833_797)
        );
    }
}
