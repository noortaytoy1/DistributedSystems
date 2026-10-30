//! Deterministic incident planner with a cost knob.
//!
//! Given an incident and the responders that are free, choose one (D49, D74, D75):
//!
//! - Only responders with the capability the incident needs count. The nearest one wins
//!   (Manhattan distance in metres); a tie goes to a responder in the incident's zone, then to the
//!   lowest responder ID.
//! - If none of them is free and the incident is severe (severity 4 or 5), the nearest free
//!   responder of any capability is sent instead, with the same tie-breaks. Otherwise the plan
//!   has no responder and cost 0.
//! - `Plan::cost` is the distance in metres from the chosen responder to the incident.
//!
//! Age is not used yet: the planner has no clock, and the node does not pass a time in (D76).
//!
//! `CostKnob::extra_work` adds simulated CPU load (D77): each unit runs [`ROUNDS_PER_UNIT`] rounds
//! of an integer mixing function. It never changes the plan. 0 means only the regular work.
//!
//! Pure logic: no network, no clock, no randomness. Runs on the worker pool (node/workers.rs),
//! never on the async runtime.
//!
//! Owner: E (planner and concurrency).

use crate::proto::{Incident, Plan, Responder};

/// Rounds of busy work per unit of `CostKnob::extra_work`; roughly 1 µs, to be calibrated (D77).
pub const ROUNDS_PER_UNIT: u64 = 1_000;

/// Lowest severity at which a responder of another capability is sent when no match is free.
pub const SEVERE: u8 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CostKnob {
    /// Units of busy work added to each plan.
    pub extra_work: u32,
}

/// Choose a responder for `incident` from `free`.
pub fn plan(incident: &Incident, free: &[Responder], knob: CostKnob) -> Plan {
    burn(knob.extra_work);
    let matching = nearest(
        incident,
        free.iter().filter(|r| r.capability == incident.needs),
    );
    let chosen = matching.or_else(|| {
        if incident.severity >= SEVERE {
            nearest(incident, free.iter())
        } else {
            None
        }
    });
    match chosen {
        Some(r) => Plan {
            responder: Some(r.id),
            cost: incident.location.manhattan(r.location),
        },
        None => Plan {
            responder: None,
            cost: 0,
        },
    }
}

/// The nearest candidate; ties go to the incident's zone, then to the lowest ID. Responder IDs
/// are unique, so the order of `candidates` never matters.
fn nearest<'a>(
    incident: &Incident,
    candidates: impl Iterator<Item = &'a Responder>,
) -> Option<&'a Responder> {
    candidates.min_by_key(|r| {
        (
            incident.location.manhattan(r.location),
            r.zone != incident.zone,
            r.id,
        )
    })
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
    use crate::proto::{Capability, Point, ResponderId, ZoneId};

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
            zone,
            location: Point { x, y },
            severity,
            needs,
            reported_at_ms: 1_000,
        }
    }

    fn plan0(incident: &Incident, free: &[Responder]) -> Plan {
        plan(incident, free, CostKnob::default())
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
        assert_eq!(
            p,
            Plan {
                responder: Some(2),
                cost: 70
            }
        );
    }

    #[test]
    fn a_tie_goes_to_the_incident_zone_then_the_lowest_id() {
        let inc = incident(1, 500, 0, 1, Capability::Fire);
        // All 100 m away.
        let other_zone = responder(1, 0, 400, 0, Capability::Fire);
        let own_zone_high = responder(9, 1, 600, 0, Capability::Fire);
        let own_zone_low = responder(5, 1, 500, 100, Capability::Fire);
        let p = plan0(&inc, &[other_zone.clone(), own_zone_high.clone()]);
        assert_eq!(p.responder, Some(9), "own zone beats a lower ID");
        let p = plan0(&inc, &[own_zone_high, own_zone_low, other_zone]);
        assert_eq!(p.responder, Some(5), "then the lowest ID");
    }

    #[test]
    fn no_match_for_a_minor_incident_gives_no_responder() {
        let free = [responder(1, 0, 10, 0, Capability::Fire)];
        for severity in 1..SEVERE {
            let p = plan0(&incident(0, 0, 0, severity, Capability::Medical), &free);
            assert_eq!(
                p,
                Plan {
                    responder: None,
                    cost: 0
                },
                "severity {severity}"
            );
        }
    }

    #[test]
    fn no_match_for_a_severe_incident_sends_the_nearest_of_any_capability() {
        let free = [
            responder(1, 0, 90, 0, Capability::Security),
            responder(2, 0, 20, 30, Capability::Fire),
            responder(3, 0, 0, 50, Capability::Rescue),
        ];
        for severity in SEVERE..=5 {
            let p = plan0(&incident(0, 0, 0, severity, Capability::Medical), &free);
            assert_eq!(
                p,
                Plan {
                    responder: Some(2),
                    cost: 50
                },
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
            assert_eq!(
                p,
                Plan {
                    responder: None,
                    cost: 0
                }
            );
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
        let inc = incident(1, 0, 0, 3, Capability::Security);
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
            plan(&inc, &free, CostKnob { extra_work: 50 }),
            plan0(&inc, &free)
        );
    }
}
