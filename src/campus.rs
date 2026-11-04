//! Synthetic campus: zones, the responders spread across them, and random incidents.
//!
//! Layout (D97): zones are [`ZONE_SIZE_M`]-metre squares in a near-square grid, filled row by
//! row (6 zones: 3 columns × 2 rows), with zone 0's corner at (0, 0). Responder `i` (0-based)
//! gets ID `i + 1`, zone `i mod zones` and capability `i mod 4`, so zones and capabilities are
//! covered evenly. Its position inside the zone comes from the seed.
//!
//! Responders must be a pure function of `CampusConfig`, so every node and client derives the
//! same campus from the same config. They use our own SplitMix64 generator (D98), whose output
//! no library upgrade can change.
//!
//! Incidents (D99) are drawn uniformly: zone, position inside it, severity 1–5 and needed
//! capability, then a category that capability answers (D141). [`random_incident`] draws from the
//! caller's RNG, so a seeded run can be repeated. [`unseeded_incident`] draws from OS randomness,
//! so every run differs (D102).
//!
//! Owner: E (planner and concurrency).

use rand::Rng;

use crate::config::CampusConfig;
use crate::proto::{Capability, Incident, Point, Responder, UserId, ZoneId};

/// Side of a square zone, in metres.
pub const ZONE_SIZE_M: i32 = 500;

/// The campus responders, registered through Raft when the cluster first forms.
pub fn responders(cfg: &CampusConfig) -> Vec<Responder> {
    let zones = u32::from(cfg.zones.max(1));
    // The least common multiple of the zones and the 4 capabilities.
    let repeat = zones * 4 / [4, 2, 1].into_iter().find(|d| zones % d == 0).unwrap_or(1);
    let mut rng = SplitMix64(cfg.seed);
    (0..cfg.responders)
        .map(|i| {
            let zone = ZoneId::try_from(i % zones).expect("i mod zones fits in a zone ID");
            // `i % 4` alone would tie a zone to half of the capabilities with an even number of zones,
            // and reports would pile onto responders across a border. Moving on by one each time the
            // pairs repeat gives every zone every capability, and stays even (D240).
            let capability = Capability::ALL[((i + i / repeat) % 4) as usize];
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

/// A random incident, as the simulated user `owner` would report it.
pub fn random_incident(
    rng: &mut impl Rng,
    cfg: &CampusConfig,
    owner: UserId,
    now_ms: u64,
) -> Incident {
    let zone = rng.gen_range(0..cfg.zones.max(1));
    let dx = rng.gen_range(0..ZONE_SIZE_M);
    let dy = rng.gen_range(0..ZONE_SIZE_M);
    let severity = rng.gen_range(1..=5);
    // The capability comes first so each stays a quarter of the load (D99). Drawing the category
    // first would make Technical 3/7 of it, because three categories need it.
    let needs = Capability::ALL[rng.gen_range(0..Capability::ALL.len())];
    let categories = needs.categories();
    Incident {
        owner,
        category: categories[rng.gen_range(0..categories.len())],
        zone,
        location: offset(zone_corner(cfg.zones, zone), dx, dy),
        severity,
        needs,
        reported_at_ms: now_ms,
        ttl_ms: cfg.incident_ttl_ms,
    }
}

/// A random incident drawn from OS randomness: no seed, so no two runs see the same incidents
/// (D102). Clients only; the deterministic core never calls this.
pub fn unseeded_incident(cfg: &CampusConfig, owner: UserId, now_ms: u64) -> Incident {
    random_incident(&mut rand::thread_rng(), cfg, owner, now_ms)
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
    use crate::proto::Category;
    use rand::SeedableRng;
    use rand::rngs::StdRng;
    use std::collections::{BTreeMap, BTreeSet};

    fn cfg(seed: u64, zones: u16, responders: u32) -> CampusConfig {
        CampusConfig {
            seed,
            zones,
            responders,
            service_ms_per_severity: 200,
            incident_ttl_ms: 60_000,
        }
    }

    fn owner() -> UserId {
        UserId {
            client: 3,
            user: 17,
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
    fn every_zone_has_every_capability() {
        for zones in [1, 2, 4, 5, 6, 12, 60] {
            let rs = responders(&cfg(1, zones, u32::from(zones) * 4));
            let pairs: std::collections::BTreeSet<_> =
                rs.iter().map(|r| (r.zone, r.capability)).collect();
            assert_eq!(pairs.len(), usize::from(zones) * 4, "{zones} zones");
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
        let inc = random_incident(&mut rng, &cfg(42, 1, 8), owner(), 5);
        assert_eq!(inc.zone, 0);
    }

    #[test]
    fn incidents_stay_in_range_and_cover_every_value() {
        let c = CampusConfig::default();
        let mut rng = StdRng::seed_from_u64(11);
        let (mut zones, mut severities, mut needs) =
            (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
        for i in 0..2_000 {
            let inc = random_incident(&mut rng, &c, owner(), i);
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
    fn every_category_occurs_and_agrees_with_needs() {
        let c = CampusConfig::default();
        let mut rng = StdRng::seed_from_u64(12);
        let mut categories = BTreeSet::new();
        for i in 0..2_000 {
            let inc = random_incident(&mut rng, &c, owner(), i);
            assert_eq!(inc.category.needs(), inc.needs, "{inc:?}");
            categories.insert(inc.category);
        }
        assert_eq!(categories, Category::ALL.into_iter().collect());
    }

    #[test]
    fn each_capability_stays_about_a_quarter_of_the_load() {
        let c = CampusConfig::default();
        let mut rng = StdRng::seed_from_u64(13);
        let mut per_need = BTreeMap::new();
        for i in 0..8_000 {
            let inc = random_incident(&mut rng, &c, owner(), i);
            *per_need.entry(inc.needs).or_insert(0) += 1;
        }
        // 2000 expected for each; three categories needing Technical must not raise its share.
        for (need, count) in per_need {
            assert!((1_800..=2_200).contains(&count), "{need:?}: {count}");
        }
    }

    #[test]
    fn a_report_carries_its_owner_and_the_configured_ttl() {
        let c = CampusConfig {
            incident_ttl_ms: 1_234,
            ..CampusConfig::default()
        };
        let mut rng = StdRng::seed_from_u64(14);
        let inc = random_incident(&mut rng, &c, owner(), 5);
        assert_eq!(inc.owner, owner());
        assert_eq!(inc.ttl_ms, 1_234);
        assert_eq!(unseeded_incident(&c, owner(), 5).ttl_ms, 1_234);
    }

    #[test]
    fn the_same_rng_seed_gives_the_same_incidents() {
        let c = CampusConfig::default();
        let draw = |seed| {
            let mut rng = StdRng::seed_from_u64(seed);
            (0..50)
                .map(|i| random_incident(&mut rng, &c, owner(), i))
                .collect::<Vec<_>>()
        };
        assert_eq!(draw(5), draw(5));
        assert_ne!(draw(5), draw(6));
    }

    #[test]
    fn unseeded_incidents_stay_in_range() {
        let c = CampusConfig::default();
        for i in 0..500 {
            let inc = unseeded_incident(&c, owner(), i);
            assert_eq!(inc.reported_at_ms, i);
            assert!(inc.zone < c.zones, "{inc:?}");
            assert!(inside_zone(c.zones, inc.zone, inc.location), "{inc:?}");
            assert!((1..=5).contains(&inc.severity), "{inc:?}");
        }
    }

    #[test]
    fn unseeded_batches_differ() {
        // 50 incidents drawn twice from OS randomness match with negligible probability.
        let c = CampusConfig::default();
        let draw = || {
            (0..50)
                .map(|i| unseeded_incident(&c, owner(), i))
                .collect::<Vec<_>>()
        };
        assert_ne!(draw(), draw());
    }
}
