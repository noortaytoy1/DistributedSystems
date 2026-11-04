//! Replicated state machine: incidents, responders, results and duplicate detection.
//!
//! Every node applies the same committed commands in the same order, so this code must be
//! deterministic: no clock, no randomness, no network, no `HashMap` iteration.
//!
//! - **First commit wins.** The first `CommitResult` applied for a request ID becomes its result.
//!   Later ones (a retry, or a second executor after a takeover) get `Outcome::Duplicate` with
//!   the winner and change nothing.
//! - **Stale plans.** A result whose responder is already held by another request, or unknown, is
//!   not recorded (`Outcome::Stale`); the executor plans again.
//! - **Digest.** A hash chain over every applied command and its outcome. Two nodes with the
//!   same digest at the same index applied the same log and hold the same state.
//! - **Status.** An incident's status is worked out when someone reads it, from the result and the
//!   hold (D142). The caller passes the time in; nothing here looks at a clock.
//!
//! Owner: A (Raft and replication).

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

use crate::proto::{
    Command, CommandGroup, CommittedResult, IncidentQuery, IncidentStatus, IncidentView,
    MAX_INCIDENT_ROWS, Outcome, RequestId, Responder, ResponderId, ResultProposal, StaleReason,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponderSlot {
    pub responder: Responder,
    /// Request currently holding this responder.
    pub held_by: Option<RequestId>,
}

/// How many commands of each kind were applied, for reports and tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counters {
    pub committed: u64,
    pub duplicates: u64,
    pub stale: u64,
    pub released: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudState {
    responders: BTreeMap<ResponderId, ResponderSlot>,
    results: BTreeMap<RequestId, CommittedResult>,
    counters: Counters,
    /// Log index of the last command applied (internal Raft entries are not counted).
    last_index: u64,
    digest: u64,
}

impl CloudState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply one committed log entry: its commands in order, each seeing what the one before it
    /// did. Each goes through [`apply`](Self::apply), so a group changes the state and the digest
    /// exactly as the same commands would one after another at this index (D133).
    pub fn apply_group(&mut self, index: u64, group: &CommandGroup) -> Vec<Outcome> {
        group.0.iter().map(|cmd| self.apply(index, cmd)).collect()
    }

    /// Apply one committed command found at Raft log index `index`.
    pub fn apply(&mut self, index: u64, cmd: &Command) -> Outcome {
        let outcome = match cmd {
            Command::RegisterResponders { responders } => {
                for r in responders {
                    self.responders
                        .entry(r.id)
                        .or_insert_with(|| ResponderSlot {
                            responder: r.clone(),
                            held_by: None,
                        });
                }
                Outcome::Applied
            }
            Command::CommitResult(proposal) => self.commit_result(index, proposal),
            Command::ReleaseResponder { responder, request } => {
                if let Some(slot) = self.responders.get_mut(responder) {
                    if slot.held_by == Some(*request) {
                        slot.held_by = None;
                        self.counters.released += 1;
                    }
                }
                Outcome::Applied
            }
        };
        self.last_index = index;
        self.digest = chain(self.digest, index, cmd, &outcome);
        outcome
    }

    fn commit_result(&mut self, index: u64, p: &ResultProposal) -> Outcome {
        if let Some(winner) = self.results.get(&p.request) {
            self.counters.duplicates += 1;
            return Outcome::Duplicate(winner.clone());
        }
        if let Some(responder) = p.plan.responder {
            let reason = match self.responders.get_mut(&responder) {
                None => Some(StaleReason::UnknownResponder),
                Some(slot) => match slot.held_by {
                    Some(holder) => Some(StaleReason::HeldBy(holder)),
                    None => {
                        slot.held_by = Some(p.request);
                        None
                    }
                },
            };
            if let Some(reason) = reason {
                self.counters.stale += 1;
                return Outcome::Stale {
                    request: p.request,
                    responder,
                    reason,
                };
            }
        }
        let result = CommittedResult {
            request: p.request,
            incident: p.incident.clone(),
            executor: p.executor,
            term: p.term,
            plan: p.plan.clone(),
            age_ms: p.age_ms,
            assigned_at_ms: p.assigned_at_ms,
            log_index: index,
        };
        self.results.insert(p.request, result.clone());
        self.counters.committed += 1;
        Outcome::Committed(result)
    }

    pub fn result(&self, id: &RequestId) -> Option<&CommittedResult> {
        self.results.get(id)
    }

    /// Where an incident stands at `now_ms`, or `None` if nothing is committed for it.
    pub fn status_at(&self, id: &RequestId, now_ms: u64) -> Option<IncidentStatus> {
        let result = self.results.get(id)?;
        Some(result.status(self.is_held(result), now_ms))
    }

    /// Whether the responder this result sent is still held for it.
    fn is_held(&self, result: &CommittedResult) -> bool {
        result
            .plan
            .responder
            .and_then(|responder| self.responders.get(&responder))
            .is_some_and(|slot| slot.held_by == Some(result.request))
    }

    /// The records `query` asks for, read at `now_ms` (D145). One client's range costs only the rows
    /// returned, and sequence numbers start at the current time (D113), so the highest is the newest.
    pub fn incident_rows(
        &self,
        query: &IncidentQuery,
        now_ms: u64,
        ms_per_severity: u64,
    ) -> Vec<IncidentView> {
        let view = |result: &CommittedResult| IncidentView {
            id: result.request,
            incident: result.incident.clone(),
            status: result.status(self.is_held(result), now_ms),
            responder: result.plan.responder,
            priority: result.plan.priority,
            executor: result.executor,
            age_ms: result.age_ms,
            assigned_at_ms: result.assigned_at_ms,
            expires_at_ms: result.planned_end_ms(ms_per_severity),
            log_index: result.log_index,
        };
        match *query {
            IncidentQuery::One(id) => self.results.get(&id).map(view).into_iter().collect(),
            IncidentQuery::OfClient { client, limit } => {
                let first = RequestId { client, seq: 0 };
                let last = RequestId {
                    client,
                    seq: u64::MAX,
                };
                self.results
                    .range(first..=last)
                    .rev()
                    .take(limit.min(MAX_INCIDENT_ROWS) as usize)
                    .map(|(_, result)| view(result))
                    .collect()
            }
        }
    }

    pub fn results(&self) -> impl Iterator<Item = &CommittedResult> {
        self.results.values()
    }

    pub fn result_count(&self) -> usize {
        self.results.len()
    }

    pub fn responder(&self, id: ResponderId) -> Option<&ResponderSlot> {
        self.responders.get(&id)
    }

    pub fn responder_count(&self) -> usize {
        self.responders.len()
    }

    /// Every responder, held or not, in ID order.
    pub fn responders(&self) -> Vec<Responder> {
        self.responders
            .values()
            .map(|s| s.responder.clone())
            .collect()
    }

    /// Responders not held by any request, in ID order.
    pub fn free_responders(&self) -> Vec<Responder> {
        self.responders
            .values()
            .filter(|s| s.held_by.is_none())
            .map(|s| s.responder.clone())
            .collect()
    }

    /// About one in `one_in` of the free responders, chosen by `salt`, in ID order (D238). Reads only.
    /// Different salts give different parts, so plans running at once rarely pick the same responder.
    pub fn free_responders_sample(&self, salt: u64, one_in: u64) -> Vec<Responder> {
        self.responders
            .values()
            .filter(|s| s.held_by.is_none())
            .filter(|s| sample_mix(salt ^ u64::from(s.responder.id)) % one_in == 0)
            .map(|s| s.responder.clone())
            .collect()
    }

    /// Every held responder with its holder's committed result, in responder ID order.
    pub fn holds(&self) -> Vec<(ResponderId, &CommittedResult)> {
        self.responders
            .iter()
            .filter_map(|(id, slot)| {
                let result = self.results.get(&slot.held_by?)?;
                Some((*id, result))
            })
            .collect()
    }

    pub fn counters(&self) -> Counters {
        self.counters
    }

    /// Log index of the last applied command, 0 before the first.
    pub fn last_index(&self) -> u64 {
        self.last_index
    }

    /// Hash chain over every applied command; see the module docs.
    pub fn digest(&self) -> u64 {
        self.digest
    }
}

/// FNV-1a: fixed constants, so every node computes the same value.
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

fn chain(prev: u64, index: u64, cmd: &Command, outcome: &Outcome) -> u64 {
    let mut h = Fnv(0xcbf2_9ce4_8422_2325);
    prev.hash(&mut h);
    index.hash(&mut h);
    cmd.hash(&mut h);
    outcome.hash(&mut h);
    h.finish()
}

/// Spreads an integer over all 64 bits, for [`CloudState::free_responders_sample`].
pub fn sample_mix(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^ (x >> 33)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{Capability, Category, Incident, Plan, Point, UserId};

    fn rid(seq: u64) -> RequestId {
        RequestId { client: 1, seq }
    }

    fn responder(id: ResponderId) -> Responder {
        Responder {
            id,
            zone: 0,
            location: Point { x: 0, y: 0 },
            capability: Capability::Medical,
        }
    }

    fn proposal(seq: u64, executor: u64, responder: Option<ResponderId>) -> Command {
        Command::CommitResult(ResultProposal {
            request: rid(seq),
            incident: Incident {
                owner: UserId { client: 1, user: 0 },
                category: Category::MedicalAssistance,
                zone: 0,
                location: Point { x: 1, y: 1 },
                severity: 3,
                needs: Capability::Medical,
                reported_at_ms: 0,
                ttl_ms: 60_000,
            },
            executor,
            term: 1,
            plan: Plan {
                responder,
                cost: 7,
                priority: 300,
            },
            age_ms: 4,
            assigned_at_ms: 1_000,
        })
    }

    fn with_responders(ids: &[ResponderId]) -> CloudState {
        let mut s = CloudState::new();
        s.apply(
            1,
            &Command::RegisterResponders {
                responders: ids.iter().map(|id| responder(*id)).collect(),
            },
        );
        s
    }

    #[test]
    fn a_sample_is_part_of_the_free_list_and_differs_by_salt() {
        let mut state = CloudState::default();
        let responders: Vec<Responder> = (0..1600)
            .map(|id| Responder {
                id,
                zone: 0,
                location: Point { x: 0, y: 0 },
                capability: Capability::Medical,
            })
            .collect();
        state.apply(1, &Command::RegisterResponders { responders });
        let (a, b) = (
            state.free_responders_sample(sample_mix(1), 16),
            state.free_responders_sample(sample_mix(2), 16),
        );
        assert!((50..=150).contains(&a.len()), "{}", a.len());
        assert_ne!(a, b);
        assert_eq!(a, state.free_responders_sample(sample_mix(1), 16));
    }

    #[test]
    fn first_commit_wins() {
        let mut s = with_responders(&[1, 2]);
        let Outcome::Committed(first) = s.apply(2, &proposal(10, 1, Some(1))) else {
            panic!("first proposal must commit");
        };
        assert_eq!(first.executor, 1);
        assert_eq!(first.log_index, 2);

        // A second executor's plan for the same request is discarded, whatever it planned.
        assert_eq!(
            s.apply(3, &proposal(10, 2, Some(2))),
            Outcome::Duplicate(first.clone())
        );
        assert_eq!(s.result(&rid(10)), Some(&first));
        assert_eq!(s.responder(2).unwrap().held_by, None);
        assert_eq!(s.counters().committed, 1);
        assert_eq!(s.counters().duplicates, 1);
    }

    #[test]
    fn plan_for_a_taken_responder_is_stale() {
        let mut s = with_responders(&[1, 2]);
        s.apply(2, &proposal(10, 1, Some(1)));
        assert_eq!(
            s.apply(3, &proposal(11, 2, Some(1))),
            Outcome::Stale {
                request: rid(11),
                responder: 1,
                reason: StaleReason::HeldBy(rid(10)),
            }
        );
        assert!(s.result(&rid(11)).is_none());
        // Re-planned with a free responder, the same request commits.
        assert!(matches!(
            s.apply(4, &proposal(11, 2, Some(2))),
            Outcome::Committed(_)
        ));
    }

    /// What lets a node answer after three stale plans (D220): with every responder held, a plan that
    /// names nobody still commits, and it holds nothing.
    #[test]
    fn plan_naming_nobody_commits_when_every_responder_is_held() {
        let mut s = with_responders(&[1]);
        s.apply(2, &proposal(10, 1, Some(1)));
        let Outcome::Committed(done) = s.apply(3, &proposal(11, 2, None)) else {
            panic!("a plan naming nobody must commit");
        };
        assert_eq!(done.plan.responder, None);
        assert_eq!(s.responder(1).unwrap().held_by, Some(rid(10)));
        assert_eq!(s.counters().stale, 0);
    }

    #[test]
    fn plan_for_an_unknown_responder_is_stale() {
        let mut s = with_responders(&[1]);
        assert!(matches!(
            s.apply(2, &proposal(10, 1, Some(9))),
            Outcome::Stale {
                reason: StaleReason::UnknownResponder,
                ..
            }
        ));
        assert_eq!(s.counters().stale, 1);
    }

    #[test]
    fn plan_without_responder_commits_without_holding() {
        let mut s = with_responders(&[1]);
        assert!(matches!(
            s.apply(2, &proposal(10, 1, None)),
            Outcome::Committed(_)
        ));
        assert_eq!(s.free_responders().len(), 1);
    }

    #[test]
    fn release_only_frees_the_holder_and_is_idempotent() {
        let mut s = with_responders(&[1]);
        s.apply(2, &proposal(10, 1, Some(1)));
        let release = |request| Command::ReleaseResponder {
            responder: 1,
            request,
        };
        s.apply(3, &release(rid(99)));
        assert_eq!(s.responder(1).unwrap().held_by, Some(rid(10)));
        s.apply(4, &release(rid(10)));
        assert_eq!(s.responder(1).unwrap().held_by, None);
        s.apply(5, &release(rid(10)));
        assert_eq!(s.counters().released, 1);
        assert_eq!(s.free_responders().len(), 1);
    }

    #[test]
    fn registering_again_keeps_assignments() {
        let mut s = with_responders(&[1, 2]);
        s.apply(2, &proposal(10, 1, Some(1)));
        s.apply(
            3,
            &Command::RegisterResponders {
                responders: vec![responder(1), responder(3)],
            },
        );
        assert_eq!(s.responder(1).unwrap().held_by, Some(rid(10)));
        assert_eq!(s.responder_count(), 3);
    }

    #[test]
    fn same_log_same_digest_different_log_different_digest() {
        let log = [
            proposal(1, 1, Some(1)),
            proposal(2, 2, Some(1)),
            proposal(1, 3, None),
        ];
        let replay = |order: &[usize]| {
            let mut s = with_responders(&[1, 2]);
            for (i, k) in order.iter().enumerate() {
                s.apply(i as u64 + 2, &log[*k]);
            }
            s
        };
        let a = replay(&[0, 1, 2]);
        let b = replay(&[0, 1, 2]);
        let c = replay(&[1, 0, 2]);
        assert_eq!(a, b);
        assert_eq!(a.digest(), b.digest());
        assert_ne!(a.digest(), c.digest());
        assert_eq!(a.last_index(), 4);
    }

    #[test]
    fn a_commit_keeps_the_proposals_age_and_time() {
        let mut s = with_responders(&[1]);
        let Outcome::Committed(result) = s.apply(2, &proposal(10, 1, Some(1))) else {
            panic!("the proposal must commit");
        };
        assert_eq!((result.age_ms, result.assigned_at_ms), (4, 1_000));
        assert_eq!(s.result(&rid(10)), Some(&result));
    }

    #[test]
    fn status_at_follows_the_hold_through_release() {
        let mut s = with_responders(&[1]);
        assert_eq!(s.status_at(&rid(10), 1_000), None);
        s.apply(2, &proposal(10, 1, Some(1)));
        assert_eq!(s.status_at(&rid(10), 1_000), Some(IncidentStatus::Assigned));
        // A release for someone else changes nothing.
        s.apply(
            3,
            &Command::ReleaseResponder {
                responder: 1,
                request: rid(99),
            },
        );
        assert_eq!(s.status_at(&rid(10), 1_000), Some(IncidentStatus::Assigned));
        s.apply(
            4,
            &Command::ReleaseResponder {
                responder: 1,
                request: rid(10),
            },
        );
        assert_eq!(s.status_at(&rid(10), 1_000), Some(IncidentStatus::Resolved));
        // The responder going out again for another request does not undo that.
        s.apply(5, &proposal(11, 1, Some(1)));
        assert_eq!(s.status_at(&rid(10), 1_000), Some(IncidentStatus::Resolved));
        assert_eq!(s.status_at(&rid(11), 1_000), Some(IncidentStatus::Assigned));
    }

    #[test]
    fn reading_status_changes_neither_state_nor_digest() {
        let mut s = with_responders(&[1]);
        s.apply(2, &proposal(10, 1, None));
        let before = s.clone();
        // Past the deadline, the same record reads differently and nothing has moved.
        assert_eq!(
            s.status_at(&rid(10), 1_000),
            Some(IncidentStatus::Unassigned)
        );
        assert_eq!(
            s.status_at(&rid(10), 1_000_000),
            Some(IncidentStatus::Expired)
        );
        assert_eq!(s, before);
        assert_eq!(s.digest(), before.digest());
    }

    /// Commits one nobody-sent report for each (client, seq), at indexes from 2.
    fn with_results(ids: &[(u64, u64)]) -> CloudState {
        let mut s = CloudState::new();
        for (i, (client, seq)) in ids.iter().enumerate() {
            let Command::CommitResult(mut p) = proposal(*seq, 1, None) else {
                panic!("proposal() builds a CommitResult");
            };
            p.request = RequestId {
                client: *client,
                seq: *seq,
            };
            s.apply(i as u64 + 2, &Command::CommitResult(p));
        }
        s
    }

    fn row_ids(rows: &[IncidentView]) -> Vec<(u64, u64)> {
        rows.iter().map(|r| (r.id.client, r.id.seq)).collect()
    }

    #[test]
    fn rows_of_one_client_come_newest_first_up_to_the_limit() {
        let s = with_results(&[(3, 1), (3, 2), (3, 3), (3, 4)]);
        let rows = s.incident_rows(
            &IncidentQuery::OfClient {
                client: 3,
                limit: 2,
            },
            1_000,
            200,
        );
        assert_eq!(row_ids(&rows), vec![(3, 4), (3, 3)]);
    }

    #[test]
    fn a_client_query_never_returns_more_than_the_cap() {
        let ids: Vec<(u64, u64)> = (0..MAX_INCIDENT_ROWS as u64 + 5).map(|s| (3, s)).collect();
        let s = with_results(&ids);
        let rows = s.incident_rows(
            &IncidentQuery::OfClient {
                client: 3,
                limit: u32::MAX,
            },
            1_000,
            200,
        );
        assert_eq!(rows.len(), MAX_INCIDENT_ROWS as usize);
    }

    #[test]
    fn a_higher_client_id_does_not_hide_a_lower_one() {
        let s = with_results(&[(1, 5), (2, 6), (3, 7), (3, 8), (3, 9)]);
        let of = |client| {
            let query = IncidentQuery::OfClient { client, limit: 2 };
            row_ids(&s.incident_rows(&query, 1_000, 200))
        };
        assert_eq!(of(1), vec![(1, 5)]);
        assert_eq!(of(2), vec![(2, 6)]);
        assert_eq!(of(3), vec![(3, 9), (3, 8)]);
        assert_eq!(of(4), Vec::<(u64, u64)>::new());
    }

    #[test]
    fn a_row_shows_the_record_and_its_status() {
        let mut s = with_responders(&[1]);
        s.apply(2, &proposal(10, 2, Some(1)));
        let one = IncidentQuery::One(rid(10));
        let rows = s.incident_rows(&one, 1_000, 200);
        let [row] = rows.as_slice() else {
            panic!("expected one row, got {rows:?}");
        };
        assert_eq!(row.id, rid(10));
        assert_eq!(row.incident.severity, 3);
        assert_eq!(row.status, IncidentStatus::Assigned);
        assert_eq!(row.responder, Some(1));
        assert_eq!(row.priority, 300);
        assert_eq!(row.executor, 2);
        assert_eq!((row.age_ms, row.assigned_at_ms), (4, 1_000));
        assert_eq!(row.expires_at_ms, Some(1_600));
        assert_eq!(row.log_index, 2);
        assert!(
            s.incident_rows(&IncidentQuery::One(rid(11)), 1_000, 200)
                .is_empty()
        );
    }

    fn release(responder: ResponderId, seq: u64) -> Command {
        Command::ReleaseResponder {
            responder,
            request: rid(seq),
        }
    }

    #[test]
    fn a_group_gives_the_same_state_and_digest_on_two_copies() {
        let group = CommandGroup(vec![
            proposal(1, 1, Some(1)),
            proposal(2, 2, Some(1)),
            release(1, 1),
            proposal(3, 3, Some(1)),
            proposal(4, 1, None),
        ]);
        let mut a = with_responders(&[1, 2]);
        let mut b = with_responders(&[1, 2]);
        let from_a = a.apply_group(2, &group);
        let from_b = b.apply_group(2, &group);
        assert_eq!(from_a, from_b);
        assert_eq!(a, b);
        assert_eq!(a.digest(), b.digest());
        assert_eq!(a.last_index(), 2);
        assert_eq!(from_a.len(), 5, "one outcome per command, in order");
        assert!(matches!(from_a[0], Outcome::Committed(_)));
        assert!(matches!(from_a[1], Outcome::Stale { .. }));
        assert_eq!(from_a[2], Outcome::Applied);
        assert!(matches!(from_a[3], Outcome::Committed(_)));
    }

    #[test]
    fn a_group_is_the_same_as_its_commands_one_after_another_at_one_index() {
        let commands = vec![
            proposal(1, 1, Some(1)),
            release(1, 1),
            proposal(2, 2, Some(1)),
        ];
        let mut grouped = with_responders(&[1]);
        let outcomes = grouped.apply_group(2, &CommandGroup(commands.clone()));
        let mut single = with_responders(&[1]);
        let one_by_one: Vec<_> = commands.iter().map(|c| single.apply(2, c)).collect();
        assert_eq!(outcomes, one_by_one);
        assert_eq!(grouped, single);
    }

    #[test]
    fn the_order_inside_a_group_changes_the_digest() {
        let first = proposal(1, 1, Some(1));
        let second = proposal(2, 2, Some(1));
        let mut a = with_responders(&[1]);
        a.apply_group(2, &CommandGroup(vec![first.clone(), second.clone()]));
        let mut b = with_responders(&[1]);
        b.apply_group(2, &CommandGroup(vec![second, first]));
        assert_ne!(a.digest(), b.digest());
    }

    #[test]
    fn results_of_one_group_share_its_log_index() {
        let mut s = with_responders(&[1, 2]);
        let outcomes = s.apply_group(
            7,
            &CommandGroup(vec![proposal(1, 1, Some(1)), proposal(2, 1, Some(2))]),
        );
        for outcome in &outcomes {
            let Outcome::Committed(result) = outcome else {
                panic!("expected a commit, got {outcome:?}");
            };
            assert_eq!(result.log_index, 7);
        }
    }

    #[test]
    fn a_command_in_a_group_is_idempotent() {
        // Inside one group: the retry of a result gets the winner, and a second release does nothing.
        let mut s = with_responders(&[1, 2]);
        let outcomes = s.apply_group(
            2,
            &CommandGroup(vec![
                proposal(1, 1, Some(1)),
                proposal(1, 2, Some(2)),
                release(1, 1),
                release(1, 1),
            ]),
        );
        let Outcome::Committed(winner) = &outcomes[0] else {
            panic!("the first proposal must commit");
        };
        assert_eq!(outcomes[1], Outcome::Duplicate(winner.clone()));
        assert_eq!(s.counters().released, 1);
        assert_eq!(s.free_responders().len(), 2);

        // The whole group again, as after a leader change: nothing moves.
        let (results, free) = (s.result_count(), s.free_responders());
        let again = s.apply_group(
            3,
            &CommandGroup(vec![proposal(1, 1, Some(1)), release(1, 1)]),
        );
        assert_eq!(again[0], Outcome::Duplicate(winner.clone()));
        assert_eq!(s.result_count(), results);
        assert_eq!(s.free_responders(), free);
        assert_eq!(s.counters().committed, 1);
        assert_eq!(s.counters().released, 1);
    }

    #[test]
    fn survives_serialization() {
        let mut s = with_responders(&[1, 2]);
        s.apply(2, &proposal(10, 1, Some(1)));
        let bytes = bincode::serialize(&s).unwrap();
        let back: CloudState = bincode::deserialize(&bytes).unwrap();
        assert_eq!(back, s);
        assert_eq!(back.digest(), s.digest());
    }
}
