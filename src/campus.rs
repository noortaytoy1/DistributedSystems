//! Synthetic campus: zones, the responders spread across them, and random incidents.
//!
//! Layout (D78): zones are [`ZONE_SIZE_M`]-metre squares in a near-square grid, filled row by
//! row (6 zones: 3 columns × 2 rows), with zone 0's corner at (0, 0). Responder `i` (0-based)
//! gets ID `i + 1`, zone `i mod zones` and capability `i mod 4`, so zones and capabilities are
//! covered evenly. Its position inside the zone comes from the seed.
//!
//! Responders must be a pure function of `CampusConfig`, so every node and client derives the
//! same campus from the same config. They use our own SplitMix64 generator (D79), whose output
//! no library upgrade can change.
//!
//! Incidents (D80) are drawn uniformly from the caller's RNG: zone, position inside it,
//! severity 1–5 and needed capability.
//!
//! Owner: E (planner and concurrency).

use rand::Rng;

use crate::config::CampusConfig;
use crate::proto::{Capability, Incident, Point, Responder, ZoneId};

/// Side of a square zone, in metres.
pub const ZONE_SIZE_M: i32 = 500;

/// The campus responders, registered through Raft when the cluster first forms.
pub fn responders(cfg: &CampusConfig) -> Vec<Responder> {
    let zones = u32::from(cfg.zones.max(1));
    let mut rng = SplitMix64(cfg.seed);
    (0..cfg.responders)
        .map(|i| {
            let zone = ZoneId::try_from(i % zones).expect("i mod zones fits in a zone ID");
            let capability = Capability::ALL[(i % 4) as usize];
            let dx = rng.below(ZONE_SIZE_M);
            let dy = rng.below(ZONE_SIZE_M);
            Responder {
                id: i + 1,
                zone,
                location: offset(zone_corner(cfg.zones, zone), dx, dy),
                capability,
            }
        })
        .collect()
}

/// A random incident, as a simulated user would report it.
pub fn random_incident(rng: &mut impl Rng, cfg: &CampusConfig, now_ms: u64) -> Incident {
    let zone = rng.gen_range(0..cfg.zones.max(1));
    let dx = rng.gen_range(0..ZONE_SIZE_M);
    let dy = rng.gen_range(0..ZONE_SIZE_M);
    Incident {
        zone,
        location: offset(zone_corner(cfg.zones, zone), dx, dy),
        severity: rng.gen_range(1..=5),
        needs: Capability::ALL[rng.gen_range(0..Capability::ALL.len())],
        reported_at_ms: now_ms,
    }
}

/// Columns of the zone grid: the smallest `c` with `c × c ≥ zones`.
fn columns(zones: ZoneId) -> u32 {
    let zones = u32::from(zones.max(1));
    let mut c = 1;
    while c * c < zones {
        c += 1;
    }
    c
}

/// South-west corner of `zone`.
fn zone_corner(zones: ZoneId, zone: ZoneId) -> Point {
    let cols = columns(zones);
    let zone = u32::from(zone);
    // At most 256 columns or rows (zones ≤ 65 535), so the corner fits easily in an i32.
    let col = i32::try_from(zone % cols).expect("column fits in i32");
    let row = i32::try_from(zone / cols).expect("row fits in i32");
    Point {
        x: col * ZONE_SIZE_M,
        y: row * ZONE_SIZE_M,
    }
}

fn offset(corner: Point, dx: i32, dy: i32) -> Point {
    Point {
        x: corner.x + dx,
        y: corner.y + dy,
    }
}

/// SplitMix64 (Steele, Lea and Flood, 2014): a tiny, fixed integer generator.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `0..n` for a positive `n`. The modulo bias is below 1e-16 for small `n`.
    fn below(&mut self, n: i32) -> i32 {
        let n = u64::try_from(n.max(1)).expect("positive");
        i32::try_from(self.next_u64() % n).expect("below an i32 bound")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;
    use std::collections::{BTreeMap, BTreeSet};

    fn cfg(seed: u64, zones: u16, responders: u32) -> CampusConfig {
        CampusConfig {
            seed,
            zones,
            responders,
        }
    }

    fn inside_zone(zones: ZoneId, zone: ZoneId, p: Point) -> bool {
        let c = zone_corner(zones, zone);
        (c.x..c.x + ZONE_SIZE_M).contains(&p.x) && (c.y..c.y + ZONE_SIZE_M).contains(&p.y)
    }

    #[test]
    fn the_same_seed_gives_the_same_campus() {
        let c = CampusConfig::default();
        assert_eq!(responders(&c), responders(&c));
    }

    #[test]
    fn different_seeds_give_different_campuses() {
        let a = responders(&cfg(42, 6, 30));
        let b = responders(&cfg(43, 6, 30));
        assert_ne!(a, b);
        // Only positions come from the seed; IDs, zones and capabilities do not.
        let shape = |rs: &[Responder]| -> Vec<_> {
            rs.iter().map(|r| (r.id, r.zone, r.capability)).collect()
        };
        assert_eq!(shape(&a), shape(&b));
    }

    #[test]
    fn ids_run_from_one_to_n() {
        let rs = responders(&cfg(7, 6, 30));
        let ids: Vec<_> = rs.iter().map(|r| r.id).collect();
        assert_eq!(ids, (1..=30).collect::<Vec<_>>());
    }

    #[test]
    fn six_zones_form_a_three_by_two_grid() {
        assert_eq!(columns(6), 3);
        assert_eq!(zone_corner(6, 0), Point { x: 0, y: 0 });
        assert_eq!(zone_corner(6, 2), Point { x: 1000, y: 0 });
        assert_eq!(zone_corner(6, 3), Point { x: 0, y: 500 });
        assert_eq!(zone_corner(6, 5), Point { x: 1000, y: 500 });
        assert_eq!(columns(1), 1);
        assert_eq!(columns(9), 3);
        assert_eq!(columns(10), 4);
    }

    #[test]
    fn every_responder_is_inside_its_zone() {
        for (zones, n) in [(6, 30), (1, 5), (7, 50), (10, 3)] {
            for r in responders(&cfg(99, zones, n)) {
                assert!(r.zone < zones, "{r:?}");
                assert!(inside_zone(zones, r.zone, r.location), "{r:?}");
            }
        }
    }

    #[test]
    fn zones_and_capabilities_are_spread_evenly() {
        for (zones, n) in [(6, 30), (5, 23), (4, 4)] {
            let rs = responders(&cfg(1, zones, n));
            let mut per_zone = BTreeMap::new();
            let mut per_cap = BTreeMap::new();
            for r in &rs {
                *per_zone.entry(r.zone).or_insert(0) += 1;
                *per_cap.entry(r.capability).or_insert(0) += 1;
            }
            for counts in [
                per_zone.values().copied().collect::<Vec<i32>>(),
                per_cap.values().copied().collect(),
            ] {
                let (lo, hi) = (counts.iter().min(), counts.iter().max());
                assert!(hi.zip(lo).is_some_and(|(h, l)| h - l <= 1), "{counts:?}");
            }
            assert_eq!(per_zone.len(), usize::from(zones).min(n as usize));
        }
    }

    #[test]
    fn empty_and_single_zone_campuses_work() {
        assert!(responders(&cfg(42, 6, 0)).is_empty());
        let rs = responders(&cfg(42, 1, 8));
        assert_eq!(rs.len(), 8);
        assert!(rs.iter().all(|r| r.zone == 0));
        let mut rng = StdRng::seed_from_u64(3);
        let inc = random_incident(&mut rng, &cfg(42, 1, 8), 5);
        assert_eq!(inc.zone, 0);
    }

    #[test]
    fn incidents_stay_in_range_and_cover_every_value() {
        let c = CampusConfig::default();
        let mut rng = StdRng::seed_from_u64(11);
        let (mut zones, mut severities, mut needs) =
            (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
        for i in 0..2_000 {
            let inc = random_incident(&mut rng, &c, i);
            assert_eq!(inc.reported_at_ms, i);
            assert!(inc.zone < c.zones, "{inc:?}");
            assert!(inside_zone(c.zones, inc.zone, inc.location), "{inc:?}");
            assert!((1..=5).contains(&inc.severity), "{inc:?}");
            zones.insert(inc.zone);
            severities.insert(inc.severity);
            needs.insert(inc.needs);
        }
        assert_eq!(zones.len(), usize::from(c.zones));
        assert_eq!(severities, (1..=5).collect());
        assert_eq!(needs, Capability::ALL.into_iter().collect());
    }

    #[test]
    fn the_same_rng_seed_gives_the_same_incidents() {
        let c = CampusConfig::default();
        let draw = |seed| {
            let mut rng = StdRng::seed_from_u64(seed);
            (0..50)
                .map(|i| random_incident(&mut rng, &c, i))
                .collect::<Vec<_>>()
        };
        assert_eq!(draw(5), draw(5));
        assert_ne!(draw(5), draw(6));
    }
}
