//! The presence directory: a second state machine on the shared log (Phase 2, Part G, D69, D280–D292).
//!
//! One record per user (D319): who it is, its role, its zone, its program's peer address, what it can
//! do, and its incarnation. [`CloudState`](crate::state::CloudState) holds one [`Directory`] and hands
//! it the four presence commands, so every node applies the same commands in the same order and holds
//! the same directory. Like `state.rs` this file is pure: no network, no clock, no randomness, and
//! `BTreeMap`s only.
//!
//! - **The log clock (D281).** Every presence command carries the time its proposing node stamped.
//!   The directory keeps the highest one applied, [`Directory::clock_ms`], and judges every lease by
//!   it, so all nodes expire the same records at the same log position. `PresenceTick` moves the clock
//!   when no client is sending.
//! - **Live, late, gone (D285).** A record is found by `discover` until its lease ends, kept but hidden
//!   for the grace period after that, where a renewal brings it back, and removed once the grace is
//!   over.
//! - **Duplicate registrations (D286).** The higher incarnation wins. An equal one replaces the record
//!   and keeps the later lease end, which is also what makes a register applied twice the same as once.
//!   A lower one is refused with `Superseded`.
//! - **Zone moves (D282).** A register with the same incarnation and another zone.
//! - **Queries (D288).** Read from the applied state at the log clock, so two nodes at the same index
//!   give the same answer. A zone's answer takes one record per program before any program's second.
//!
//! Owner: G (discovery directory).

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

use crate::proto::{
    Capability, ClientId, MAX_DISCOVER_RECORDS, PresenceLease, PresenceOutcome, PresenceQuery,
    PresenceRecord, UserId, ZoneId,
};

/// One user's record and its lease, on the directory's log clock.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Entry {
    pub record: PresenceRecord,
    /// Found by `discover` while the log clock is below this.
    pub lease_ends_ms: u64,
    /// Removed once the log clock reaches this: the lease end plus the grace (D285).
    pub gone_at_ms: u64,
}

impl Entry {
    /// Whether `discover` finds it at log time `clock_ms`.
    pub fn is_live(&self, clock_ms: u64) -> bool {
        clock_ms < self.lease_ends_ms
    }
}

/// Where a user's record stands at a log time (D285).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecordState {
    Live,
    /// The lease ended, the grace has not: hidden from `discover`, still renewable.
    Late,
    /// No record.
    Gone,
}

/// What the directory has done, for reports and tests. Not in the digest: the commands decide it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectoryCounters {
    /// Registrations of a user with no record.
    pub registered: u64,
    /// Registrations that replaced a record: the same program again, or a restart.
    pub replaced: u64,
    /// Registrations that changed the zone.
    pub moved: u64,
    /// Registrations refused because a higher incarnation is held (D286).
    pub superseded: u64,
    pub renewed: u64,
    /// Renewals of a late record, which a delayed heartbeat brought back (D285).
    pub revived: u64,
    /// Renewals and deregistrations that found no record at their incarnation.
    pub unknown: u64,
    pub deregistered: u64,
    /// Records that ran out of grace and were removed.
    pub expired: u64,
}

/// The presence directory. See the module docs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Directory {
    entries: BTreeMap<UserId, Entry>,
    /// Every record by when it goes, so expiry costs the records that go and not the table.
    by_gone: BTreeSet<(u64, UserId)>,
    by_zone: BTreeMap<ZoneId, BTreeSet<UserId>>,
    by_capability: BTreeMap<Capability, BTreeSet<UserId>>,
    /// The highest time any applied presence command carried (D281).
    clock_ms: u64,
    counters: DirectoryCounters,
}

impl Directory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Put a user in, or replace its record (D282, D286). Applied at log index `index`.
    pub fn register(
        &mut self,
        index: u64,
        record: &PresenceRecord,
        lease: &PresenceLease,
    ) -> PresenceOutcome {
        self.advance(lease.at_ms);
        let (lease_ends_ms, gone_at_ms) = ends(lease);
        if let Some(held) = self.entries.get(&record.user) {
            if held.record.incarnation > record.incarnation {
                self.counters.superseded += 1;
                return PresenceOutcome::Superseded {
                    held: held.record.incarnation,
                    log_index: index,
                };
            }
        }
        // A command so late that its whole lease is over: putting it in would only take it out again.
        if gone_at_ms <= self.clock_ms {
            self.counters.unknown += 1;
            return PresenceOutcome::Unknown { log_index: index };
        }
        let entry = match self.remove(&record.user) {
            Some(old) => {
                self.counters.replaced += 1;
                if old.record.zone != record.zone {
                    self.counters.moved += 1;
                }
                if old.record.incarnation == record.incarnation {
                    // The same program again: keep the later lease, so a repeat changes nothing.
                    Entry {
                        record: record.clone(),
                        lease_ends_ms: lease_ends_ms.max(old.lease_ends_ms),
                        gone_at_ms: gone_at_ms.max(old.gone_at_ms),
                    }
                } else {
                    Entry {
                        record: record.clone(),
                        lease_ends_ms,
                        gone_at_ms,
                    }
                }
            }
            None => {
                self.counters.registered += 1;
                Entry {
                    record: record.clone(),
                    lease_ends_ms,
                    gone_at_ms,
                }
            }
        };
        let lease_ends_ms = entry.lease_ends_ms;
        self.insert(entry);
        PresenceOutcome::Registered {
            lease_ms: lease.lease_ms,
            lease_ends_ms,
            log_index: index,
        }
    }

    /// Extend the lease of the user's record at this incarnation, live or late (D285).
    pub fn renew(
        &mut self,
        index: u64,
        user: UserId,
        incarnation: u64,
        lease: &PresenceLease,
    ) -> PresenceOutcome {
        self.advance(lease.at_ms);
        let (lease_ends_ms, gone_at_ms) = ends(lease);
        let clock_ms = self.clock_ms;
        let Some(entry) = self
            .entries
            .get_mut(&user)
            .filter(|e| e.record.incarnation == incarnation)
        else {
            self.counters.unknown += 1;
            return PresenceOutcome::Unknown { log_index: index };
        };
        if !entry.is_live(clock_ms) {
            self.counters.revived += 1;
        }
        self.counters.renewed += 1;
        let old_gone = entry.gone_at_ms;
        entry.lease_ends_ms = entry.lease_ends_ms.max(lease_ends_ms);
        entry.gone_at_ms = old_gone.max(gone_at_ms);
        let (new_gone, ends_ms) = (entry.gone_at_ms, entry.lease_ends_ms);
        if new_gone != old_gone {
            self.by_gone.remove(&(old_gone, user));
            self.by_gone.insert((new_gone, user));
        }
        PresenceOutcome::Renewed {
            lease_ms: lease.lease_ms,
            lease_ends_ms: ends_ms,
            log_index: index,
        }
    }

    /// Take the user out if its record is at this incarnation.
    pub fn deregister(
        &mut self,
        index: u64,
        user: UserId,
        incarnation: u64,
        at_ms: u64,
    ) -> PresenceOutcome {
        self.advance(at_ms);
        let matches = self
            .entries
            .get(&user)
            .is_some_and(|e| e.record.incarnation == incarnation);
        if !matches {
            self.counters.unknown += 1;
            return PresenceOutcome::Unknown { log_index: index };
        }
        self.remove(&user);
        self.counters.deregistered += 1;
        PresenceOutcome::Deregistered { log_index: index }
    }

    /// Move the log clock and drop what ran out of grace (D281).
    pub fn tick(&mut self, index: u64, at_ms: u64) -> PresenceOutcome {
        let expired = self.advance(at_ms);
        PresenceOutcome::Ticked {
            expired,
            log_index: index,
        }
    }

    /// The clock only moves forward. Returns how many records went.
    fn advance(&mut self, at_ms: u64) -> u32 {
        self.clock_ms = self.clock_ms.max(at_ms);
        let mut expired = 0;
        while let Some(&(gone_at, user)) = self.by_gone.first() {
            if gone_at > self.clock_ms {
                break;
            }
            self.remove(&user);
            expired += 1;
        }
        self.counters.expired += u64::from(expired);
        expired
    }

    fn insert(&mut self, entry: Entry) {
        let user = entry.record.user;
        self.by_gone.insert((entry.gone_at_ms, user));
        self.by_zone
            .entry(entry.record.zone)
            .or_default()
            .insert(user);
        for capability in &entry.record.capabilities {
            self.by_capability
                .entry(*capability)
                .or_default()
                .insert(user);
        }
        self.entries.insert(user, entry);
    }

    fn remove(&mut self, user: &UserId) -> Option<Entry> {
        let entry = self.entries.remove(user)?;
        self.by_gone.remove(&(entry.gone_at_ms, *user));
        unindex(&mut self.by_zone, entry.record.zone, user);
        for capability in &entry.record.capabilities {
            unindex(&mut self.by_capability, *capability, user);
        }
        Some(entry)
    }

    /// The live records `query` matches, at most its limit and never more than
    /// [`MAX_DISCOVER_RECORDS`], in user order with one record per program first (D288, D357).
    pub fn discover(&self, query: &PresenceQuery) -> Vec<PresenceRecord> {
        let limit = usize::from(query.limit.min(MAX_DISCOVER_RECORDS));
        if limit == 0 {
            return Vec::new();
        }
        let empty = BTreeSet::new();
        // The narrowest index the query names; the other filter is checked on each record.
        let candidates: Box<dyn Iterator<Item = &UserId>> = match (query.zone, query.capability) {
            (Some(zone), _) => Box::new(self.by_zone.get(&zone).unwrap_or(&empty).iter()),
            (None, Some(capability)) => {
                Box::new(self.by_capability.get(&capability).unwrap_or(&empty).iter())
            }
            (None, None) => Box::new(self.entries.keys()),
        };
        let matching: Vec<&Entry> = candidates
            .filter_map(|user| self.entries.get(user))
            .filter(|e| e.is_live(self.clock_ms))
            .filter(|e| query.zone.is_none_or(|z| e.record.zone == z))
            .filter(|e| {
                query
                    .capability
                    .is_none_or(|c| e.record.capabilities.contains(&c))
            })
            .collect();
        let mut programs: BTreeSet<ClientId> = BTreeSet::new();
        let mut first: Vec<&Entry> = Vec::new();
        let mut rest: Vec<&Entry> = Vec::new();
        for entry in matching {
            if programs.insert(entry.record.user.client) {
                first.push(entry);
                if first.len() == limit {
                    break;
                }
            } else if rest.len() < limit {
                rest.push(entry);
            }
        }
        first
            .into_iter()
            .chain(rest)
            .take(limit)
            .map(|e| e.record.clone())
            .collect()
    }

    /// Where the user's record stands at the current log time.
    pub fn state_of(&self, user: &UserId) -> RecordState {
        match self.entries.get(user) {
            Some(e) if e.is_live(self.clock_ms) => RecordState::Live,
            Some(_) => RecordState::Late,
            None => RecordState::Gone,
        }
    }

    pub fn entry(&self, user: &UserId) -> Option<&Entry> {
        self.entries.get(user)
    }

    /// Records held, live or late.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Records `discover` would find, in any zone.
    pub fn live_count(&self) -> usize {
        self.entries
            .values()
            .filter(|e| e.is_live(self.clock_ms))
            .count()
    }

    /// The directory's log clock: the highest time an applied presence command carried (D281).
    pub fn clock_ms(&self) -> u64 {
        self.clock_ms
    }

    pub fn counters(&self) -> DirectoryCounters {
        self.counters
    }

    /// A hash of the records and the clock, and nothing else: two nodes whose directories hold the
    /// same thing have the same value, whatever else their logs did. For the presence-convergence
    /// measurement (D293). FNV-1a, as the state digest, so it is the same on every node.
    pub fn content_digest(&self) -> u64 {
        let mut h = Fnv(0xcbf2_9ce4_8422_2325);
        self.clock_ms.hash(&mut h);
        for entry in self.entries.values() {
            entry.hash(&mut h);
        }
        h.finish()
    }
}

/// When a lease starting at `lease.at_ms` ends, and when its grace does.
fn ends(lease: &PresenceLease) -> (u64, u64) {
    let lease_ends = lease.at_ms.saturating_add(lease.lease_ms);
    (lease_ends, lease_ends.saturating_add(lease.grace_ms))
}

fn unindex<K: Ord>(index: &mut BTreeMap<K, BTreeSet<UserId>>, key: K, user: &UserId) {
    if let Some(users) = index.get_mut(&key) {
        users.remove(user);
        if users.is_empty() {
            index.remove(&key);
        }
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
    use crate::proto::PresenceRole;

    /// One command, applied to a directory at a log index.
    type Apply = Box<dyn Fn(&mut Directory, u64)>;

    const LEASE: u64 = 10_000;
    const GRACE: u64 = 5_000;

    fn user(client: ClientId, n: u32) -> UserId {
        UserId { client, user: n }
    }

    fn record(u: UserId, zone: ZoneId, incarnation: u64) -> PresenceRecord {
        PresenceRecord {
            user: u,
            role: PresenceRole::VolunteerResponder,
            zone,
            peer_addr: format!("10.0.0.{}:20000", u.client),
            capabilities: vec![Capability::Medical],
            incarnation,
        }
    }

    fn lease(at_ms: u64) -> PresenceLease {
        PresenceLease {
            at_ms,
            lease_ms: LEASE,
            grace_ms: GRACE,
        }
    }

    fn query(zone: Option<ZoneId>, capability: Option<Capability>) -> PresenceQuery {
        PresenceQuery {
            zone,
            capability,
            limit: MAX_DISCOVER_RECORDS,
        }
    }

    fn found(d: &Directory, q: &PresenceQuery) -> Vec<UserId> {
        d.discover(q).into_iter().map(|r| r.user).collect()
    }

    #[test]
    fn a_registered_user_is_found_by_zone_and_capability() {
        let mut d = Directory::new();
        let a = user(1, 0);
        let outcome = d.register(1, &record(a, 3, 100), &lease(1_000));
        assert_eq!(
            outcome,
            PresenceOutcome::Registered {
                lease_ms: LEASE,
                lease_ends_ms: 11_000,
                log_index: 1
            }
        );
        assert_eq!(found(&d, &query(Some(3), None)), vec![a]);
        assert_eq!(found(&d, &query(None, Some(Capability::Medical))), vec![a]);
        assert_eq!(
            found(&d, &query(Some(3), Some(Capability::Medical))),
            vec![a]
        );
        assert!(found(&d, &query(Some(4), None)).is_empty());
        assert!(found(&d, &query(None, Some(Capability::Fire))).is_empty());
        assert_eq!(found(&d, &query(None, None)), vec![a]);
    }

    /// The spec's "delayed heartbeats": a renewal after the lease but inside the grace brings the
    /// record back with no new registration.
    #[test]
    fn a_late_renewal_inside_the_grace_revives_the_record() {
        let mut d = Directory::new();
        let a = user(1, 0);
        d.register(1, &record(a, 3, 100), &lease(0));
        d.tick(2, LEASE + 1);
        assert_eq!(d.state_of(&a), RecordState::Late);
        assert!(
            found(&d, &query(Some(3), None)).is_empty(),
            "late is hidden"
        );

        let outcome = d.renew(3, a, 100, &lease(LEASE + 2_000));
        assert!(
            matches!(outcome, PresenceOutcome::Renewed { .. }),
            "{outcome:?}"
        );
        assert_eq!(d.state_of(&a), RecordState::Live);
        assert_eq!(found(&d, &query(Some(3), None)), vec![a]);
        assert_eq!(d.counters().revived, 1);
    }

    /// The spec's "expired registrations".
    #[test]
    fn a_record_past_its_grace_is_gone_and_a_renewal_is_told_so() {
        let mut d = Directory::new();
        let a = user(1, 0);
        d.register(1, &record(a, 3, 100), &lease(0));
        assert_eq!(
            d.tick(2, LEASE + GRACE),
            PresenceOutcome::Ticked {
                expired: 1,
                log_index: 2
            }
        );
        assert_eq!(d.state_of(&a), RecordState::Gone);
        assert!(d.is_empty());
        assert_eq!(
            d.renew(3, a, 100, &lease(LEASE + GRACE)),
            PresenceOutcome::Unknown { log_index: 3 }
        );
        assert_eq!(d.counters().expired, 1);
    }

    #[test]
    fn expiry_happens_one_millisecond_before_and_not_after_the_grace() {
        let mut d = Directory::new();
        let a = user(1, 0);
        d.register(1, &record(a, 3, 100), &lease(0));
        d.tick(2, LEASE + GRACE - 1);
        assert_eq!(d.state_of(&a), RecordState::Late);
        d.tick(3, LEASE + GRACE);
        assert_eq!(d.state_of(&a), RecordState::Gone);
    }

    /// The spec's "duplicate registrations".
    #[test]
    fn the_higher_incarnation_wins_in_either_order() {
        let a = user(1, 0);
        let old = record(a, 3, 100);
        let new = PresenceRecord {
            peer_addr: "10.0.0.1:20001".into(),
            ..record(a, 3, 200)
        };

        let mut first_old = Directory::new();
        first_old.register(1, &old, &lease(0));
        first_old.register(2, &new, &lease(10));

        let mut first_new = Directory::new();
        first_new.register(1, &new, &lease(10));
        let refused = first_new.register(2, &old, &lease(0));
        assert_eq!(
            refused,
            PresenceOutcome::Superseded {
                held: 200,
                log_index: 2
            }
        );

        for d in [&first_old, &first_new] {
            assert_eq!(d.entry(&a).unwrap().record, new);
        }
        // An old program's renewal and deregistration touch nothing.
        assert_eq!(
            first_old.renew(3, a, 100, &lease(20)),
            PresenceOutcome::Unknown { log_index: 3 }
        );
        assert_eq!(
            first_old.deregister(4, a, 100, 20),
            PresenceOutcome::Unknown { log_index: 4 }
        );
        assert_eq!(first_old.entry(&a).unwrap().record, new);
    }

    /// The spec's "a user moving between zones".
    #[test]
    fn a_zone_move_is_a_registration_with_the_new_zone() {
        let mut d = Directory::new();
        let a = user(1, 0);
        d.register(1, &record(a, 3, 100), &lease(0));
        d.register(2, &record(a, 5, 100), &lease(1_000));
        assert!(found(&d, &query(Some(3), None)).is_empty());
        assert_eq!(found(&d, &query(Some(5), None)), vec![a]);
        assert_eq!(d.counters().moved, 1);
        assert_eq!(d.len(), 1);
    }

    #[test]
    fn every_command_applied_twice_is_the_same_as_once() {
        let a = user(1, 0);
        let commands: Vec<Apply> = vec![
            Box::new(move |d, i| {
                d.register(i, &record(a, 3, 100), &lease(1_000));
            }),
            Box::new(move |d, i| {
                d.renew(i, a, 100, &lease(4_000));
            }),
            Box::new(move |d, i| {
                d.tick(i, 9_000);
            }),
            Box::new(move |d, i| {
                d.deregister(i, a, 100, 9_500);
            }),
        ];
        let mut once = Directory::new();
        let mut twice = Directory::new();
        for (i, cmd) in commands.iter().enumerate() {
            cmd(&mut once, i as u64);
            cmd(&mut twice, i as u64);
            cmd(&mut twice, i as u64);
            assert_eq!(once.entries, twice.entries, "after command {i}");
            assert_eq!(once.clock_ms, twice.clock_ms);
            assert_eq!(once.content_digest(), twice.content_digest());
        }
    }

    /// The log clock never moves back, so a node with a slower clock cannot bring a lease back.
    #[test]
    fn a_tick_from_a_slower_clock_does_not_move_the_clock_back() {
        let mut d = Directory::new();
        d.tick(1, 50_000);
        d.tick(2, 40_000);
        assert_eq!(d.clock_ms(), 50_000);
        // A registration stamped so far back that its grace is already over is not put in.
        let a = user(1, 0);
        assert_eq!(
            d.register(3, &record(a, 3, 100), &lease(30_000)),
            PresenceOutcome::Unknown { log_index: 3 }
        );
        assert!(d.is_empty());
    }

    /// The same commands in the same order give the same directory, whichever node stamped the ticks.
    #[test]
    fn two_copies_given_the_same_commands_are_the_same() {
        let mut a = Directory::new();
        let mut b = Directory::new();
        for d in [&mut a, &mut b] {
            for n in 0..50u32 {
                let u = user(u64::from(n % 5), n);
                d.register(
                    u64::from(n),
                    &record(u, (n % 3) as ZoneId, 1),
                    &lease(u64::from(n) * 100),
                );
            }
            d.tick(60, 3_000 + LEASE + GRACE);
        }
        assert_eq!(a, b);
        assert_eq!(a.content_digest(), b.content_digest());
        assert_eq!(a.len(), 19, "users registered after 3.0 s stay");
    }

    /// D357: 16 records from a zone cover up to 16 programs before any program gets a second.
    #[test]
    fn a_zone_answer_takes_one_record_per_program_first() {
        let mut d = Directory::new();
        // Programs 1 to 20, each with 5 users in zone 0.
        for client in 1..=20 {
            for n in 0..5 {
                d.register(1, &record(user(client, n), 0, 1), &lease(0));
            }
        }
        let answer = d.discover(&query(Some(0), None));
        assert_eq!(answer.len(), usize::from(MAX_DISCOVER_RECORDS));
        let programs: BTreeSet<ClientId> = answer.iter().map(|r| r.user.client).collect();
        assert_eq!(programs.len(), usize::from(MAX_DISCOVER_RECORDS));

        // Three programs: one record each, then the rest in user order.
        let mut small = Directory::new();
        for client in 1..=3 {
            for n in 0..2 {
                small.register(1, &record(user(client, n), 0, 1), &lease(0));
            }
        }
        assert_eq!(
            found(&small, &query(Some(0), None)),
            vec![
                user(1, 0),
                user(2, 0),
                user(3, 0),
                user(1, 1),
                user(2, 1),
                user(3, 1)
            ]
        );
    }

    #[test]
    fn the_limit_is_capped_and_zero_finds_nothing() {
        let mut d = Directory::new();
        for n in 0..40 {
            d.register(1, &record(user(1, n), 0, 1), &lease(0));
        }
        let big = PresenceQuery {
            limit: 1_000,
            ..query(None, None)
        };
        assert_eq!(d.discover(&big).len(), usize::from(MAX_DISCOVER_RECORDS));
        let none = PresenceQuery {
            limit: 0,
            ..query(None, None)
        };
        assert!(d.discover(&none).is_empty());
        let three = PresenceQuery {
            limit: 3,
            ..query(None, None)
        };
        assert_eq!(d.discover(&three).len(), 3);
    }

    #[test]
    fn a_deregistered_user_leaves_every_index() {
        let mut d = Directory::new();
        let a = user(1, 0);
        d.register(1, &record(a, 3, 100), &lease(0));
        assert_eq!(
            d.deregister(2, a, 100, 10),
            PresenceOutcome::Deregistered { log_index: 2 }
        );
        assert!(d.is_empty());
        assert!(d.by_zone.is_empty());
        assert!(d.by_capability.is_empty());
        assert!(d.by_gone.is_empty());
    }

    #[test]
    fn a_renewal_keeps_the_later_lease_end() {
        let mut d = Directory::new();
        let a = user(1, 0);
        d.register(1, &record(a, 3, 100), &lease(5_000));
        // A renewal stamped by a node with a slower clock cannot shorten the lease.
        d.renew(2, a, 100, &lease(1_000));
        assert_eq!(d.entry(&a).unwrap().lease_ends_ms, 15_000);
        d.renew(3, a, 100, &lease(8_000));
        assert_eq!(d.entry(&a).unwrap().lease_ends_ms, 18_000);
        assert_eq!(d.by_gone.len(), 1);
    }
}
