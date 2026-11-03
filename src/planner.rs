//! Deterministic incident planner with a cost knob.
//!
//! Given an incident and the responders that are free, choose one (D49, D93, D94, D101):
//!
//! - Only responders with the capability the incident needs count. Each gets a score: Manhattan
//!   distance in metres, plus [`ZONE_PENALTY_M`] if it is in another zone than the incident. The
//!   lowest score wins; a tie goes to a responder in the incident's zone, then to the lowest ID.
//! - If none of them is free and the incident is severe (severity 4 or 5) or old (age of at least
//!   [`OLD_AGE_MS`]), the free responder of any capability with the lowest score is sent instead,
//!   with the same tie-breaks. Otherwise the plan has no responder and cost 0.
//! - `Plan::cost` is the distance in metres from the chosen responder to the incident, without the
//!   penalty.
//!
//! `Plan::priority` says how urgent the incident is, in priority points: higher is more urgent, and
//! one step of severity is worth 100 points (D148). It adds up the five factors the spec names:
//!
//! | Factor       | Points                                                     | Range      |
//! |--------------|------------------------------------------------------------|------------|
//! | severity     | 100 per point of severity                                  | 100 to 500 |
//! | age          | 1 per 50 ms, so waiting [`OLD_AGE_MS`] is one severity step | 0 to 400   |
//! | distance     | 1 per 10 m to the responder; nobody sent counts as the top | 0 to 100   |
//! | availability | 100 / (free responders + 1), of any capability             | 0 to 100   |
//! | capability   | 100 if no free responder has the needed capability         | 0 or 100   |
//!
//! So a severity 4 incident, 1 s old, with a matching responder 300 m away and 9 free scores
//! 400 + 20 + 30 + 10 + 0 = 460. Priority never changes who is sent: it is an answer the plan
//! carries, worked out from what the pick already found.
//!
//! The planner has no clock: the node passes the incident's age in as an argument (D95).
//!
//! `CostKnob::extra_work` adds simulated CPU load (D96): each unit runs [`ROUNDS_PER_UNIT`] rounds
//! of an integer mixing function. It never changes the plan. 0 means only the regular work.
//!
//! Pure logic: no network, no clock, no randomness. Runs on the worker pool (node/workers.rs),
//! never on the async runtime.
//!
//! Owner: E (planner and concurrency).

use crate::proto::{Incident, Plan, Responder};

/// Rounds of busy work per unit of `CostKnob::extra_work`; about 4 µs on the machine it was calibrated
/// on (D129). The `service_us` field of `bid_sent` shows the real cost on any machine.
pub const ROUNDS_PER_UNIT: u64 = 1_000;

/// Lowest severity at which a responder of another capability is sent when no match is free.
pub const SEVERE: u8 = 4;

/// Age at which an incident gets the same fallback as a severe one (D95).
pub const OLD_AGE_MS: u64 = 5_000;

/// Metres added to the score of a responder outside the incident's zone (D101).
pub const ZONE_PENALTY_M: u64 = 200;

/// Priority points per point of severity: the unit the other factors are measured against (D148).
pub const SEVERITY_POINTS: u32 = 100;

/// Milliseconds of age per priority point, so that waiting [`OLD_AGE_MS`] counts for as much as one
/// step of severity, the same age at which the fallback treats an incident as severe (D95).
pub const AGE_MS_PER_POINT: u64 = 50;

/// Most points age can add: enough to lift the mildest incident level with a fresh severity 5, and
/// no further, so an old minor report never outranks a fresh life-threatening one. Reached at 20 s.
pub const MAX_AGE_POINTS: u32 = 4 * SEVERITY_POINTS;

/// Metres to the responder per priority point: help that has farther to come needs attention sooner.
pub const DISTANCE_M_PER_POINT: u64 = 10;

/// Most points distance can add, reached at 1000 m, and what a plan that sends nobody gets.
pub const MAX_DISTANCE_POINTS: u32 = SEVERITY_POINTS;

/// Points when no responder at all is free; they halve and go on falling as more are free.
pub const MAX_AVAILABILITY_POINTS: u32 = SEVERITY_POINTS;

/// Points when no free responder has the capability the incident needs.
pub const NO_MATCH_POINTS: u32 = SEVERITY_POINTS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CostKnob {
    /// Units of busy work added to each plan.
    pub extra_work: u32,
}

/// Choose a responder for `incident` from `free`, and work out how urgent the incident is.
pub fn plan(incident: &Incident, free: &[Responder], age_ms: u64, knob: CostKnob) -> Plan {
    burn(knob.extra_work);
    let matching = nearest(
        incident,
        free.iter().filter(|r| r.capability == incident.needs),
    );
    let needed_capability_free = matching.is_some();
    let chosen = matching.or_else(|| {
        if incident.severity >= SEVERE || age_ms >= OLD_AGE_MS {
            nearest(incident, free.iter())
        } else {
            None
        }
    });
    let (responder, cost) = match chosen {
        Some(r) => (Some(r.id), incident.location.manhattan(r.location)),
        None => (None, 0),
    };
    let priority = Urgency {
        severity: incident.severity,
        age_ms,
        distance_m: responder.map(|_| cost),
        free_responders: free.len(),
        needed_capability_free,
    }
    .points();
    Plan {
        responder,
        cost,
        priority,
    }
}

/// What a report too old to plan gets (D143): nobody sent, and priority 0, because the planner never
/// looked at it. Anything it does look at scores at least [`SEVERITY_POINTS`].
pub fn not_planned() -> Plan {
    Plan {
        responder: None,
        cost: 0,
        priority: 0,
    }
}

/// The five factors of an incident's priority, as the planner found them.
#[derive(Debug, Clone, Copy)]
struct Urgency {
    severity: u8,
    age_ms: u64,
    /// Metres to the chosen responder; `None` if nobody is sent.
    distance_m: Option<u64>,
    /// Free responders of any capability.
    free_responders: usize,
    needed_capability_free: bool,
}

impl Urgency {
    /// The priority in points; the table is in the module docs. At most 1200 for severity 5, so a
    /// `u32` cannot overflow at any severity a `u8` can hold.
    fn points(&self) -> u32 {
        let severity = u32::from(self.severity) * SEVERITY_POINTS;
        let age = capped(self.age_ms / AGE_MS_PER_POINT, MAX_AGE_POINTS);
        let distance = match self.distance_m {
            Some(metres) => capped(metres / DISTANCE_M_PER_POINT, MAX_DISTANCE_POINTS),
            // Nobody is on the way: as far as help can be.
            None => MAX_DISTANCE_POINTS,
        };
        let availability = u32::try_from(self.free_responders)
            .map_or(0, |free| MAX_AVAILABILITY_POINTS / free.saturating_add(1));
        let capability = if self.needed_capability_free {
            0
        } else {
            NO_MATCH_POINTS
        };
        severity + age + distance + availability + capability
    }
}

fn capped(points: u64, cap: u32) -> u32 {
    u32::try_from(points).map_or(cap, |points| points.min(cap))
}

/// The candidate with the lowest score; ties go to the incident's zone, then to the lowest ID.
/// Responder IDs are unique, so the order of `candidates` never matters.
fn nearest<'a>(
    incident: &Incident,
    candidates: impl Iterator<Item = &'a Responder>,
) -> Option<&'a Responder> {
    candidates.min_by_key(|r| {
        let other_zone = r.zone != incident.zone;
        (score(incident, r), other_zone, r.id)
    })
}

/// Distance in metres, plus [`ZONE_PENALTY_M`] for a responder in another zone.
fn score(incident: &Incident, r: &Responder) -> u64 {
    let distance = incident.location.manhattan(r.location);
    if r.zone == incident.zone {
        distance
    } else {
        distance.saturating_add(ZONE_PENALTY_M)
    }
}

/// Simulated CPU load: `units × ROUNDS_PER_UNIT` rounds of the SplitMix64 finaliser. Returns the
/// number of rounds run. `black_box` keeps the compiler from skipping the loop.
fn burn(units: u32) -> u64 {
    let rounds = u64::from(units) * ROUNDS_PER_UNIT;
    let mut x: u64 = 0;
    for _ in 0..rounds {
        x = std::hint::black_box(mix(x));
    }
    std::hint::black_box(x);
    rounds
}

fn mix(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{Capability, Point, ResponderId, UserId, ZoneId};

    fn responder(
        id: ResponderId,
        zone: ZoneId,
        x: i32,
        y: i32,
        capability: Capability,
    ) -> Responder {
        Responder {
            id,
            zone,
            location: Point { x, y },
            capability,
        }
    }

    fn incident(zone: ZoneId, x: i32, y: i32, severity: u8, needs: Capability) -> Incident {
        Incident {
            owner: UserId { client: 1, user: 0 },
            category: needs.categories()[0],
            zone,
            location: Point { x, y },
            severity,
            needs,
            reported_at_ms: 1_000,
            ttl_ms: 60_000,
        }
    }

    fn plan0(incident: &Incident, free: &[Responder]) -> Plan {
        plan(incident, free, 0, CostKnob::default())
    }

    /// Who goes and how far: what the pick rule decides. Priority has its own tests below.
    fn picked(p: &Plan) -> (Option<ResponderId>, u64) {
        (p.responder, p.cost)
    }

    #[test]
    fn only_the_needed_capability_counts() {
        let free = [
            responder(1, 0, 10, 0, Capability::Fire),
            responder(2, 0, 200, 0, Capability::Medical),
        ];
        let p = plan0(&incident(0, 0, 0, 1, Capability::Medical), &free);
        assert_eq!(
            p.responder,
            Some(2),
            "the closer Fire responder does not count"
        );
    }

    #[test]
    fn the_nearest_match_wins_and_cost_is_metres() {
        let free = [
            responder(1, 0, 300, 0, Capability::Medical),
            responder(2, 0, 40, 30, Capability::Medical),
            responder(3, 0, 100, 100, Capability::Medical),
        ];
        let p = plan0(&incident(0, 0, 0, 2, Capability::Medical), &free);
        assert_eq!(picked(&p), (Some(2), 70));
    }

    #[test]
    fn a_tie_goes_to_the_incident_zone_then_the_lowest_id() {
        let inc = incident(1, 500, 0, 1, Capability::Fire);
        // All score 300: 100 m away plus the penalty, or 300 m away in the incident's zone.
        let other_zone = responder(1, 0, 400, 0, Capability::Fire);
        let own_zone_high = responder(9, 1, 800, 0, Capability::Fire);
        let own_zone_low = responder(5, 1, 500, 300, Capability::Fire);
        let p = plan0(&inc, &[other_zone.clone(), own_zone_high.clone()]);
        assert_eq!(p.responder, Some(9), "own zone beats a lower ID");
        let p = plan0(&inc, &[own_zone_high, own_zone_low, other_zone]);
        assert_eq!(p.responder, Some(5), "then the lowest ID");
    }

    #[test]
    fn the_own_zone_wins_unless_another_zone_is_more_than_the_penalty_closer() {
        let inc = incident(1, 500, 0, 2, Capability::Rescue);
        let own = responder(1, 1, 500, 300, Capability::Rescue); // 300 m
        let gap = |closer_by: u64| {
            let d = 300 - i32::try_from(closer_by).expect("small");
            responder(2, 0, 500 - d, 0, Capability::Rescue)
        };
        for closer_by in [0, 1, 150, ZONE_PENALTY_M - 1, ZONE_PENALTY_M] {
            let p = plan0(&inc, &[gap(closer_by), own.clone()]);
            assert_eq!(p.responder, Some(1), "other zone only {closer_by} m closer");
        }
        for closer_by in [ZONE_PENALTY_M + 1, 250] {
            let p = plan0(&inc, &[gap(closer_by), own.clone()]);
            assert_eq!(p.responder, Some(2), "other zone {closer_by} m closer");
        }
    }

    #[test]
    fn cost_is_metres_without_the_penalty() {
        let inc = incident(1, 500, 0, 2, Capability::Medical);
        let p = plan0(&inc, &[responder(4, 0, 450, 0, Capability::Medical)]);
        assert_eq!(picked(&p), (Some(4), 50));
    }

    #[test]
    fn the_severe_fallback_uses_the_zone_penalty_too() {
        let inc = incident(1, 500, 0, 5, Capability::Medical);
        let free = [
            responder(1, 0, 400, 0, Capability::Fire), // 100 m, other zone: score 300
            responder(2, 1, 750, 0, Capability::Technical), // 250 m, own zone: score 250
        ];
        assert_eq!(picked(&plan0(&inc, &free)), (Some(2), 250));
    }

    #[test]
    fn no_match_for_a_minor_incident_gives_no_responder() {
        let free = [responder(1, 0, 10, 0, Capability::Fire)];
        for severity in 1..SEVERE {
            let p = plan0(&incident(0, 0, 0, severity, Capability::Medical), &free);
            assert_eq!(picked(&p), (None, 0), "severity {severity}");
        }
    }

    #[test]
    fn no_match_for_a_severe_incident_sends_the_nearest_of_any_capability() {
        let free = [
            responder(1, 0, 90, 0, Capability::Technical),
            responder(2, 0, 20, 30, Capability::Fire),
            responder(3, 0, 0, 50, Capability::Rescue),
        ];
        for severity in SEVERE..=5 {
            let p = plan0(&incident(0, 0, 0, severity, Capability::Medical), &free);
            assert_eq!(
                picked(&p),
                (Some(2), 50),
                "severity {severity}: tie on 50 m goes to the lower ID"
            );
        }
    }

    #[test]
    fn a_severe_incident_still_prefers_a_farther_match() {
        let free = [
            responder(1, 0, 10, 0, Capability::Fire),
            responder(2, 0, 900, 0, Capability::Medical),
        ];
        let p = plan0(&incident(0, 0, 0, 5, Capability::Medical), &free);
        assert_eq!(p.responder, Some(2));
    }

    #[test]
    fn no_free_responders_gives_no_responder_at_any_severity() {
        for severity in 1..=5 {
            let p = plan0(&incident(0, 0, 0, severity, Capability::Rescue), &[]);
            assert_eq!(picked(&p), (None, 0));
        }
    }

    #[test]
    fn the_same_input_always_gives_the_same_plan_in_any_order() {
        let mut free: Vec<Responder> = (1..=20)
            .map(|id| {
                let cap = Capability::ALL[(id % 4) as usize];
                // Several responders share a distance, so tie-breaks matter.
                responder(id, (id % 3) as ZoneId, (id as i32 % 5) * 50, 100, cap)
            })
            .collect();
        let inc = incident(1, 0, 0, 3, Capability::Technical);
        let first = plan0(&inc, &free);
        assert!(first.responder.is_some());
        for _ in 0..3 {
            assert_eq!(plan0(&inc, &free), first);
        }
        free.reverse();
        assert_eq!(plan0(&inc, &free), first, "reversed");
        free.rotate_left(7);
        assert_eq!(plan0(&inc, &free), first, "rotated");
    }

    #[test]
    fn the_knob_adds_work_but_never_changes_the_plan() {
        assert_eq!(burn(0), 0);
        assert_eq!(burn(1), ROUNDS_PER_UNIT);
        assert_eq!(burn(50), 50 * ROUNDS_PER_UNIT);

        let free = [
            responder(1, 0, 10, 0, Capability::Fire),
            responder(2, 0, 30, 0, Capability::Medical),
        ];
        let inc = incident(0, 0, 0, 4, Capability::Medical);
        assert_eq!(
            plan(&inc, &free, 0, CostKnob { extra_work: 50 }),
            plan0(&inc, &free)
        );
    }

    #[test]
    fn an_old_incident_gets_the_any_capability_fallback() {
        let inc = incident(0, 0, 0, 1, Capability::Medical);
        let free = [responder(7, 0, 5, 5, Capability::Fire)];
        let knob = CostKnob::default();
        assert_eq!(plan(&inc, &free, OLD_AGE_MS - 1, knob).responder, None);
        assert_eq!(plan(&inc, &free, OLD_AGE_MS, knob).responder, Some(7));
    }

    #[test]
    fn age_does_not_change_a_plan_with_a_matching_responder() {
        let inc = incident(0, 0, 0, 1, Capability::Medical);
        let free = [
            responder(1, 0, 900, 0, Capability::Medical),
            responder(2, 0, 1, 0, Capability::Fire),
        ];
        let knob = CostKnob::default();
        assert_eq!(
            picked(&plan(&inc, &free, 0, knob)),
            picked(&plan(&inc, &free, 60_000, knob))
        );
    }

    /// A fresh severity 3 incident with a responder at the door and plenty free: severity only.
    fn urgency() -> Urgency {
        Urgency {
            severity: 3,
            age_ms: 0,
            distance_m: Some(0),
            free_responders: 1_000,
            needed_capability_free: true,
        }
    }

    #[test]
    fn the_worked_example_from_the_module_docs_is_right() {
        let mut free = vec![responder(1, 0, 300, 0, Capability::Medical)];
        free.extend((2..=9).map(|id| responder(id, 0, 900, 900, Capability::Fire)));
        let inc = incident(0, 0, 0, 4, Capability::Medical);
        let p = plan(&inc, &free, 1_000, CostKnob::default());
        assert_eq!(picked(&p), (Some(1), 300));
        assert_eq!(p.priority, 400 + 20 + 30 + 10);
    }

    #[test]
    fn each_step_of_severity_is_worth_the_same() {
        let points = |severity| {
            Urgency {
                severity,
                ..urgency()
            }
            .points()
        };
        for severity in 1..=5 {
            assert_eq!(points(severity), u32::from(severity) * SEVERITY_POINTS);
        }
    }

    #[test]
    fn age_raises_priority_up_to_a_cap() {
        let points = |age_ms| {
            Urgency {
                age_ms,
                ..urgency()
            }
            .points()
                - urgency().points()
        };
        assert_eq!(points(AGE_MS_PER_POINT - 1), 0);
        assert_eq!(points(AGE_MS_PER_POINT), 1);
        assert_eq!(
            points(OLD_AGE_MS),
            SEVERITY_POINTS,
            "old is one severity step"
        );
        assert_eq!(points(20_000), MAX_AGE_POINTS);
        assert_eq!(points(60_000), MAX_AGE_POINTS);
        // A test helper or a wrong client clock can state an age since 1970.
        assert_eq!(points(u64::MAX), MAX_AGE_POINTS);
    }

    #[test]
    fn the_mildest_incident_can_wait_its_way_level_with_a_fresh_worst_one_and_no_further() {
        let oldest_mild = Urgency {
            severity: 1,
            age_ms: u64::MAX,
            ..urgency()
        };
        let fresh_worst = Urgency {
            severity: 5,
            ..urgency()
        };
        assert_eq!(oldest_mild.points(), fresh_worst.points());
    }

    #[test]
    fn a_farther_responder_raises_priority_and_nobody_sent_counts_as_farthest() {
        let points = |distance_m| {
            Urgency {
                distance_m,
                ..urgency()
            }
            .points()
                - urgency().points()
        };
        assert_eq!(points(Some(DISTANCE_M_PER_POINT - 1)), 0);
        assert_eq!(points(Some(DISTANCE_M_PER_POINT)), 1);
        assert_eq!(points(Some(500)), 50);
        assert_eq!(points(Some(1_000)), MAX_DISTANCE_POINTS);
        assert_eq!(points(Some(2_500)), MAX_DISTANCE_POINTS);
        assert_eq!(points(None), MAX_DISTANCE_POINTS);
    }

    #[test]
    fn fewer_free_responders_raise_priority() {
        let points = |free_responders| {
            Urgency {
                free_responders,
                ..urgency()
            }
            .points()
                - urgency().points()
        };
        assert_eq!(points(0), MAX_AVAILABILITY_POINTS);
        assert_eq!(points(1), 50);
        assert_eq!(points(9), 10);
        assert_eq!(points(100), 0);
    }

    #[test]
    fn no_free_match_raises_priority() {
        let no_match = Urgency {
            needed_capability_free: false,
            ..urgency()
        };
        assert_eq!(no_match.points() - urgency().points(), NO_MATCH_POINTS);
    }

    #[test]
    fn the_capability_points_also_apply_when_a_fallback_is_sent() {
        let inc = incident(0, 0, 0, 5, Capability::Medical);
        let far_fire = responder(1, 0, 100, 0, Capability::Fire);
        let near_medic = responder(2, 0, 100, 0, Capability::Medical);
        let fallback = plan0(&inc, std::slice::from_ref(&far_fire));
        let matched = plan0(&inc, std::slice::from_ref(&near_medic));
        assert_eq!(picked(&fallback), (Some(1), 100));
        assert_eq!(picked(&matched), (Some(2), 100));
        assert_eq!(fallback.priority - matched.priority, NO_MATCH_POINTS);
    }

    #[test]
    fn with_nobody_free_every_factor_but_age_is_at_its_top() {
        let p = plan0(&incident(0, 0, 0, 1, Capability::Rescue), &[]);
        assert_eq!(
            p.priority,
            SEVERITY_POINTS + MAX_DISTANCE_POINTS + MAX_AVAILABILITY_POINTS + NO_MATCH_POINTS
        );
    }

    #[test]
    fn a_report_not_planned_has_no_priority_and_anything_planned_has_some() {
        assert_eq!(
            not_planned(),
            Plan {
                responder: None,
                cost: 0,
                priority: 0
            }
        );
        let lowest = Urgency {
            severity: 1,
            ..urgency()
        };
        assert_eq!(lowest.points(), SEVERITY_POINTS);
    }

    #[test]
    fn priority_never_changes_who_is_sent() {
        // Every responder wins or loses on distance alone, whatever the age or severity adds.
        let free = [
            responder(1, 0, 300, 0, Capability::Medical),
            responder(2, 0, 40, 30, Capability::Medical),
        ];
        for severity in 1..=5 {
            for age_ms in [0, OLD_AGE_MS, 60_000] {
                let inc = incident(0, 0, 0, severity, Capability::Medical);
                let p = plan(&inc, &free, age_ms, CostKnob::default());
                assert_eq!(picked(&p), (Some(2), 70), "{severity} {age_ms}");
            }
        }
    }
}
