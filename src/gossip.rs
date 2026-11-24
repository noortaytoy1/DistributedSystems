//! Zone publish-subscribe among peers: the pure dissemination core (Phase 2, Part I, D340–D348).
//!
//! A peer subscribed to a zone gets each update for that zone pushed to it, and pushes it on to a
//! few other subscribers of the zone, so the cloud publishes once and peers spread it (D340,
//! `docs/architecture.md` section 13). This file holds only the rules; the peer endpoint, the
//! subscriber lists from the directory and the events come with the wiring (PR 2).
//!
//! - **Duplicate suppression (D342, D343).** A seen-cache keeps, per item, the highest version this
//!   peer has seen. A copy with the same version is a duplicate, an older one is stale; neither is
//!   delivered or forwarded. A newer version replaces the entry. The cache holds at most
//!   [`DEFAULT_SEEN_CAP`] items and forgets an item [`DEFAULT_SEEN_MAX_AGE_MS`] after it saw its
//!   version, oldest first, so memory stays bounded however long a peer runs.
//! - **Bounded forwarding (D341, D346).** Each copy carries the hops it has travelled: the
//!   publisher's sends carry 1, and a peer forwards (with one more) only while the copy's hops are
//!   below the hop limit. With fan-out `k` and hop limit `h` one update reaches at most
//!   `k + k² + … + k^h` peers and costs at most `k` sends per peer that delivers it. This hop
//!   count is not `Incident.ttl_ms` (D143), and the word TTL is kept for that.
//! - **No storms.** A peer never sends to itself, to the peer it heard the copy from, or to the
//!   copy's origin, and never forwards a copy it has seen.
//! - **Who gets it (D344).** The `k` targets are the subscribers that rank highest by an FNV-1a hash
//!   of (item, version, this peer, candidate), as rendezvous hashing does. There is no randomness,
//!   so a run repeats exactly, yet each peer and each update picks different targets.
//! - **Generic (D345).** [`Gossip`] works over any peer ID `P` and item ID `I`, so it does not wait
//!   on the peer envelope (H) or the directory (G), and does not decide whether a peer is a user or
//!   a program.
//! - **Counters (D347).** [`GossipStats`] counts what the report measures: duplicates per delivered
//!   update, and sends per update. PR 2 turns them into `metrics::Event` lines.
//!
//! No network, no clock and no randomness: time comes in as `now_ms`, and the subscriber list is
//! passed in on every call. Only `BTreeMap`s are iterated.
//!
//! Owner: I (zone publish-subscribe).

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};

use thiserror::Error;

/// Subscribers each peer forwards a new update to (D341).
pub const DEFAULT_FANOUT: usize = 3;

/// Most links one copy crosses (D341, D346). With [`DEFAULT_FANOUT`] it bounds the reach of one
/// update at 3 + 9 + … + 6,561 = 9,840 peers. Overlapping picks keep the real reach far below
/// that: about 93% of a 500-peer zone and 85% of a 2,500-peer zone, measured with this core.
pub const DEFAULT_HOP_LIMIT: u8 = 8;

/// Most items the seen-cache holds (D343).
pub const DEFAULT_SEEN_CAP: usize = 10_000;

/// How long the seen-cache remembers an item after it saw its version, in ms (D343). Matches the
/// 5-minute store-and-forward retention planned for Part K, so a late stored copy is still caught.
pub const DEFAULT_SEEN_MAX_AGE_MS: u64 = 300_000;

/// The bounds of dissemination. Constants in PR 1; config keys with the wiring (D348).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GossipConfig {
    /// Subscribers a peer forwards a new update to.
    pub fanout: usize,
    /// Most links one copy crosses.
    pub hop_limit: u8,
    /// Most items the seen-cache holds.
    pub seen_cap: usize,
    /// How long an item stays in the seen-cache, in ms.
    pub seen_max_age_ms: u64,
}

impl Default for GossipConfig {
    fn default() -> Self {
        Self {
            fanout: DEFAULT_FANOUT,
            hop_limit: DEFAULT_HOP_LIMIT,
            seen_cap: DEFAULT_SEEN_CAP,
            seen_max_age_ms: DEFAULT_SEEN_MAX_AGE_MS,
        }
    }
}

/// A [`GossipConfig`] that cannot work.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GossipConfigError {
    #[error("gossip fanout must be at least 1")]
    ZeroFanout,
    #[error("gossip hop_limit must be at least 1")]
    ZeroHopLimit,
    #[error("gossip seen_cap must be at least 1")]
    ZeroSeenCap,
    #[error("gossip seen_max_age_ms must be at least 1")]
    ZeroSeenAge,
}

impl GossipConfig {
    /// Refuses values with which nothing would spread or nothing would be remembered.
    pub fn validate(&self) -> Result<(), GossipConfigError> {
        if self.fanout == 0 {
            return Err(GossipConfigError::ZeroFanout);
        }
        if self.hop_limit == 0 {
            return Err(GossipConfigError::ZeroHopLimit);
        }
        if self.seen_cap == 0 {
            return Err(GossipConfigError::ZeroSeenCap);
        }
        if self.seen_max_age_ms == 0 {
            return Err(GossipConfigError::ZeroSeenAge);
        }
        Ok(())
    }

    /// Most peers one update can reach, publisher excluded: `k + k² + … + k^h`, saturating.
    pub fn reach_bound(&self) -> u64 {
        let k = u64::try_from(self.fanout).unwrap_or(u64::MAX);
        let mut level = 1u64;
        let mut total = 0u64;
        for _ in 0..self.hop_limit {
            level = level.saturating_mul(k);
            total = total.saturating_add(level);
        }
        total
    }
}

/// The gossip header of one copy of an update: what the core needs from the peer envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rumor<P, I> {
    /// The item the update is about, for example an incident.
    pub item: I,
    /// The item's version; a higher one replaces a lower one (D342).
    pub version: u64,
    /// The peer that published it. Never sent back to.
    pub origin: P,
    /// Links this copy has crossed: 1 on the publisher's sends (D346).
    pub hops: u8,
}

/// Where a peer sends an update on, and the hop count the copies carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forward<P> {
    /// The targets, at most the fan-out, best ranked first.
    pub to: Vec<P>,
    /// The hop count to put on each copy sent.
    pub hops: u8,
}

/// What a peer does with a copy it received or an update it publishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Receipt<P> {
    /// A version this peer had not seen: hand it to the application and send it to `Forward::to`,
    /// which is empty at the hop limit or when there is nobody left to send to.
    Deliver(Forward<P>),
    /// The same version was seen already. Dropped.
    Duplicate,
    /// A newer version was seen already. Dropped.
    Older,
}

/// Counters for the report (D347). Duplicate rate = `duplicates / delivered`; forwarding overhead
/// per update = `sent / (published + delivered)` summed over all peers, divided by the updates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GossipStats {
    /// Updates this peer published.
    pub published: u64,
    /// Copies received from other peers.
    pub received: u64,
    /// Received copies of a new version, handed to the application.
    pub delivered: u64,
    /// Received copies of a version already seen.
    pub duplicates: u64,
    /// Received copies older than a version already seen.
    pub older: u64,
    /// Copies this peer sent, publishing and forwarding.
    pub sent: u64,
    /// Delivered copies not forwarded because they had reached the hop limit.
    pub hop_limited: u64,
    /// Items forgotten because the cache was full.
    pub evicted: u64,
    /// Items forgotten because they were older than the age limit.
    pub expired: u64,
}

/// One peer's dissemination state: its seen-cache and its counters.
#[derive(Debug, Clone)]
pub struct Gossip<P, I> {
    me: P,
    cfg: GossipConfig,
    seen: SeenCache<I>,
    stats: GossipStats,
}

impl<P, I> Gossip<P, I>
where
    P: Ord + Copy + Hash,
    I: Ord + Copy + Hash,
{
    /// A peer `me` with the given bounds.
    pub fn new(me: P, cfg: GossipConfig) -> Result<Self, GossipConfigError> {
        cfg.validate()?;
        Ok(Self {
            me,
            cfg,
            seen: SeenCache::default(),
            stats: GossipStats::default(),
        })
    }

    pub fn me(&self) -> P {
        self.me
    }

    pub fn config(&self) -> &GossipConfig {
        &self.cfg
    }

    pub fn stats(&self) -> &GossipStats {
        &self.stats
    }

    /// Items in the seen-cache now.
    pub fn seen_len(&self) -> usize {
        self.seen.len()
    }

    /// Publish `version` of `item` to the zone's `subscribers`. The copies carry hops 1. A version
    /// this peer has already seen, or an older one, is not sent again.
    pub fn publish(&mut self, item: I, version: u64, now_ms: u64, subscribers: &[P]) -> Receipt<P> {
        let rumor = Rumor {
            item,
            version,
            origin: self.me,
            hops: 0,
        };
        let receipt = self.accept(&rumor, self.me, now_ms, subscribers);
        if matches!(receipt, Receipt::Deliver(_)) {
            self.stats.published += 1;
        }
        receipt
    }

    /// A copy `rumor` arrived from peer `from`. `subscribers` is the zone's subscriber list.
    pub fn receive(
        &mut self,
        rumor: &Rumor<P, I>,
        from: P,
        now_ms: u64,
        subscribers: &[P],
    ) -> Receipt<P> {
        self.stats.received += 1;
        let receipt = self.accept(rumor, from, now_ms, subscribers);
        match &receipt {
            Receipt::Deliver(_) => self.stats.delivered += 1,
            Receipt::Duplicate => self.stats.duplicates += 1,
            Receipt::Older => self.stats.older += 1,
        }
        receipt
    }

    fn accept(
        &mut self,
        rumor: &Rumor<P, I>,
        from: P,
        now_ms: u64,
        subscribers: &[P],
    ) -> Receipt<P> {
        let forgotten = self.seen.expire(now_ms, self.cfg.seen_max_age_ms);
        self.stats.expired += forgotten;
        match self.seen.version_of(&rumor.item) {
            Some(v) if rumor.version == v => return Receipt::Duplicate,
            Some(v) if rumor.version < v => return Receipt::Older,
            _ => {}
        }
        let evicted = self
            .seen
            .record(rumor.item, rumor.version, now_ms, self.cfg.seen_cap);
        self.stats.evicted += evicted;

        if rumor.hops >= self.cfg.hop_limit {
            self.stats.hop_limited += 1;
            return Receipt::Deliver(Forward {
                to: Vec::new(),
                hops: rumor.hops,
            });
        }
        let to = pick_targets(
            &self.me,
            &rumor.item,
            rumor.version,
            subscribers,
            &[self.me, from, rumor.origin],
            self.cfg.fanout,
        );
        self.stats.sent += u64::try_from(to.len()).unwrap_or(u64::MAX);
        Receipt::Deliver(Forward {
            to,
            hops: rumor.hops + 1,
        })
    }
}

/// The `k` candidates that rank highest for (`picker`, `item`, `version`), leaving out `exclude`
/// and repeats (D344). Public so that the cloud's single publish into a zone picks its first
/// targets by the same rule.
pub fn pick_targets<P, I>(
    picker: &P,
    item: &I,
    version: u64,
    candidates: &[P],
    exclude: &[P],
    k: usize,
) -> Vec<P>
where
    P: Ord + Copy + Hash,
    I: Hash,
{
    let unique: BTreeSet<P> = candidates
        .iter()
        .copied()
        .filter(|c| !exclude.contains(c))
        .collect();
    let mut ranked: Vec<(u64, P)> = unique
        .into_iter()
        .map(|c| (rank(picker, item, version, &c), c))
        .collect();
    // Highest score first; equal scores (vanishingly rare) fall back to ID order.
    ranked.sort_unstable_by_key(|&(score, c)| (Reverse(score), c));
    ranked.into_iter().take(k).map(|(_, c)| c).collect()
}

fn rank<P: Hash, I: Hash>(picker: &P, item: &I, version: u64, candidate: &P) -> u64 {
    let mut h = Fnv::new();
    item.hash(&mut h);
    version.hash(&mut h);
    picker.hash(&mut h);
    candidate.hash(&mut h);
    // FNV-1a mixes its last bytes poorly into the high bits; the finalizer spreads them.
    crate::state::sample_mix(h.finish())
}

/// FNV-1a with integers written little-endian, so a ranking is the same on every machine.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

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

    fn write_u16(&mut self, i: u16) {
        self.write(&i.to_le_bytes());
    }

    fn write_u32(&mut self, i: u32) {
        self.write(&i.to_le_bytes());
    }

    fn write_u64(&mut self, i: u64) {
        self.write(&i.to_le_bytes());
    }

    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64);
    }

    fn write_i16(&mut self, i: i16) {
        self.write(&i.to_le_bytes());
    }

    fn write_i32(&mut self, i: i32) {
        self.write(&i.to_le_bytes());
    }

    fn write_i64(&mut self, i: i64) {
        self.write(&i.to_le_bytes());
    }

    fn write_isize(&mut self, i: isize) {
        self.write_i64(i as i64);
    }
}

/// The highest version seen per item, forgotten oldest first by count and by age (D343).
#[derive(Debug, Clone)]
struct SeenCache<I> {
    by_item: BTreeMap<I, SeenEntry>,
    /// Items in the order their current version was recorded.
    by_order: BTreeMap<u64, I>,
    next_order: u64,
}

#[derive(Debug, Clone, Copy)]
struct SeenEntry {
    version: u64,
    at_ms: u64,
    order: u64,
}

impl<I> Default for SeenCache<I> {
    fn default() -> Self {
        Self {
            by_item: BTreeMap::new(),
            by_order: BTreeMap::new(),
            next_order: 0,
        }
    }
}

impl<I: Ord + Copy> SeenCache<I> {
    fn len(&self) -> usize {
        self.by_item.len()
    }

    fn version_of(&self, item: &I) -> Option<u64> {
        self.by_item.get(item).map(|e| e.version)
    }

    /// Forgets items older than `max_age_ms`; returns how many. A clock that went back forgets
    /// nothing.
    fn expire(&mut self, now_ms: u64, max_age_ms: u64) -> u64 {
        let mut forgotten = 0;
        while let Some((&order, &item)) = self.by_order.first_key_value() {
            let at_ms = self.by_item.get(&item).map_or(0, |e| e.at_ms);
            if now_ms.saturating_sub(at_ms) <= max_age_ms {
                break;
            }
            self.by_order.remove(&order);
            self.by_item.remove(&item);
            forgotten += 1;
        }
        forgotten
    }

    /// Records `version` of `item` as the newest seen, then forgets the oldest past `cap`; returns
    /// how many it forgot.
    fn record(&mut self, item: I, version: u64, now_ms: u64, cap: usize) -> u64 {
        if let Some(old) = self.by_item.get(&item) {
            self.by_order.remove(&old.order);
        }
        let order = self.next_order;
        self.next_order += 1;
        self.by_item.insert(
            item,
            SeenEntry {
                version,
                at_ms: now_ms,
                order,
            },
        );
        self.by_order.insert(order, item);
        let mut evicted = 0;
        while self.by_item.len() > cap {
            let Some((_, oldest)) = self.by_order.pop_first() else {
                break;
            };
            self.by_item.remove(&oldest);
            evicted += 1;
        }
        evicted
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    type Peer = u32;
    type Item = u64;

    fn peer(me: Peer, cfg: GossipConfig) -> Gossip<Peer, Item> {
        Gossip::new(me, cfg).expect("valid config")
    }

    fn deliver(r: Receipt<Peer>) -> Forward<Peer> {
        match r {
            Receipt::Deliver(f) => f,
            other => panic!("expected Deliver, got {other:?}"),
        }
    }

    fn rumor(item: Item, version: u64, origin: Peer, hops: u8) -> Rumor<Peer, Item> {
        Rumor {
            item,
            version,
            origin,
            hops,
        }
    }

    #[test]
    fn defaults_are_the_logged_values() {
        let cfg = GossipConfig::default();
        assert_eq!(cfg.fanout, 3);
        assert_eq!(cfg.hop_limit, 8);
        assert_eq!(cfg.seen_cap, 10_000);
        assert_eq!(cfg.seen_max_age_ms, 300_000);
        assert_eq!(cfg.reach_bound(), 9_840);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn zero_values_are_refused() {
        let ok = GossipConfig::default();
        for (cfg, err) in [
            (
                GossipConfig { fanout: 0, ..ok },
                GossipConfigError::ZeroFanout,
            ),
            (
                GossipConfig { hop_limit: 0, ..ok },
                GossipConfigError::ZeroHopLimit,
            ),
            (
                GossipConfig { seen_cap: 0, ..ok },
                GossipConfigError::ZeroSeenCap,
            ),
            (
                GossipConfig {
                    seen_max_age_ms: 0,
                    ..ok
                },
                GossipConfigError::ZeroSeenAge,
            ),
        ] {
            assert_eq!(Gossip::<Peer, Item>::new(1, cfg).err(), Some(err));
        }
    }

    #[test]
    fn publish_sends_fanout_copies_with_hops_one() {
        let mut g = peer(1, GossipConfig::default());
        let subs: Vec<Peer> = (1..=10).collect();
        let f = deliver(g.publish(7, 1, 0, &subs));
        assert_eq!(f.hops, 1);
        assert_eq!(f.to.len(), 3);
        assert!(!f.to.contains(&1), "never to itself");
        assert_eq!(g.stats().published, 1);
        assert_eq!(g.stats().sent, 3);
    }

    #[test]
    fn same_version_is_a_duplicate_and_is_not_forwarded() {
        let mut g = peer(5, GossipConfig::default());
        let subs: Vec<Peer> = (1..=10).collect();
        deliver(g.receive(&rumor(7, 1, 1, 1), 1, 0, &subs));
        assert_eq!(
            g.receive(&rumor(7, 1, 1, 2), 3, 10, &subs),
            Receipt::Duplicate
        );
        assert_eq!(g.stats().delivered, 1);
        assert_eq!(g.stats().duplicates, 1);
        assert_eq!(g.stats().sent, 3);
    }

    #[test]
    fn a_newer_version_spreads_and_an_older_one_is_dropped() {
        let mut g = peer(5, GossipConfig::default());
        let subs: Vec<Peer> = (1..=10).collect();
        deliver(g.receive(&rumor(7, 2, 1, 1), 1, 0, &subs));
        assert_eq!(g.receive(&rumor(7, 1, 1, 1), 2, 1, &subs), Receipt::Older);
        deliver(g.receive(&rumor(7, 3, 1, 1), 2, 2, &subs));
        assert_eq!(g.receive(&rumor(7, 2, 1, 1), 3, 3, &subs), Receipt::Older);
        assert_eq!(g.stats().delivered, 2);
        assert_eq!(g.stats().older, 2);
        assert_eq!(g.seen_len(), 1);
    }

    #[test]
    fn publishing_a_version_already_seen_sends_nothing() {
        let mut g = peer(1, GossipConfig::default());
        let subs: Vec<Peer> = (1..=10).collect();
        deliver(g.publish(7, 1, 0, &subs));
        assert_eq!(g.publish(7, 1, 1, &subs), Receipt::Duplicate);
        assert_eq!(g.stats().published, 1);
    }

    #[test]
    fn never_back_to_the_sender_the_origin_or_itself() {
        let subs: Vec<Peer> = (1..=5).collect();
        for me in 2..=5 {
            for from in 1..=5 {
                let mut g = peer(me, GossipConfig::default());
                let f = deliver(g.receive(&rumor(9, 1, 1, 1), from, 0, &subs));
                assert!(!f.to.contains(&me));
                assert!(!f.to.contains(&from));
                assert!(!f.to.contains(&1), "origin");
            }
        }
    }

    #[test]
    fn at_the_hop_limit_it_is_delivered_but_not_forwarded() {
        let cfg = GossipConfig {
            hop_limit: 2,
            ..GossipConfig::default()
        };
        let subs: Vec<Peer> = (1..=10).collect();
        let mut g = peer(5, cfg);
        let f = deliver(g.receive(&rumor(7, 1, 1, 1), 1, 0, &subs));
        assert_eq!((f.to.len(), f.hops), (3, 2));
        let f = deliver(g.receive(&rumor(8, 1, 1, 2), 1, 0, &subs));
        assert!(f.to.is_empty());
        assert_eq!(g.stats().hop_limited, 1);
    }

    #[test]
    fn small_zones_send_to_whoever_is_left() {
        let mut g = peer(2, GossipConfig::default());
        let f = deliver(g.receive(&rumor(7, 1, 1, 1), 1, 0, &[1, 2, 3, 3]));
        assert_eq!(f.to, vec![3], "repeats count once");
        let mut g = peer(2, GossipConfig::default());
        let f = deliver(g.receive(&rumor(7, 1, 1, 1), 1, 0, &[]));
        assert!(f.to.is_empty());
    }

    #[test]
    fn targets_repeat_and_still_differ_by_peer_and_update() {
        let subs: Vec<Peer> = (1..=100).collect();
        let a = pick_targets(&1u32, &7u64, 1, &subs, &[1], 3);
        assert_eq!(a, pick_targets(&1u32, &7u64, 1, &subs, &[1], 3));
        let others = [
            pick_targets(&2u32, &7u64, 1, &subs, &[2], 3),
            pick_targets(&1u32, &8u64, 1, &subs, &[1], 3),
            pick_targets(&1u32, &7u64, 2, &subs, &[1], 3),
        ];
        assert!(others.iter().all(|o| *o != a));
        // Order of the subscriber list does not matter.
        let mut rev = subs.clone();
        rev.reverse();
        assert_eq!(a, pick_targets(&1u32, &7u64, 1, &rev, &[1], 3));
    }

    #[test]
    fn targets_spread_evenly_over_the_zone() {
        // Over many updates, every subscriber is picked about equally often.
        let subs: Vec<Peer> = (1..=20).collect();
        let mut counts = BTreeMap::<Peer, u32>::new();
        for item in 0..6_000u64 {
            for t in pick_targets(&0u32, &item, 1, &subs, &[], 3) {
                *counts.entry(t).or_default() += 1;
            }
        }
        // 18,000 picks over 20 peers: 900 each on average.
        assert_eq!(counts.len(), 20);
        assert!(
            counts.values().all(|&c| (750..=1_050).contains(&c)),
            "{counts:?}"
        );
    }

    #[test]
    fn the_cache_forgets_the_oldest_past_its_cap() {
        let cfg = GossipConfig {
            seen_cap: 3,
            ..GossipConfig::default()
        };
        let mut g = peer(1, cfg);
        for item in 0..5 {
            deliver(g.publish(item, 1, item, &[]));
        }
        assert_eq!(g.seen_len(), 3);
        assert_eq!(g.stats().evicted, 2);
        // Item 0 was forgotten, so it is new again; item 4 is still known.
        deliver(g.publish(0, 1, 10, &[]));
        assert_eq!(g.publish(4, 1, 10, &[]), Receipt::Duplicate);
    }

    #[test]
    fn a_newer_version_moves_the_item_to_the_back() {
        let cfg = GossipConfig {
            seen_cap: 2,
            ..GossipConfig::default()
        };
        let mut g = peer(1, cfg);
        deliver(g.publish(1, 1, 0, &[]));
        deliver(g.publish(2, 1, 1, &[]));
        deliver(g.publish(1, 2, 2, &[])); // item 1 is now the newest
        deliver(g.publish(3, 1, 3, &[])); // so item 2 goes
        assert_eq!(g.publish(1, 2, 4, &[]), Receipt::Duplicate);
        deliver(g.publish(2, 1, 4, &[]));
    }

    #[test]
    fn the_cache_forgets_by_age() {
        let cfg = GossipConfig {
            seen_max_age_ms: 100,
            ..GossipConfig::default()
        };
        let mut g = peer(1, cfg);
        deliver(g.publish(1, 1, 0, &[]));
        deliver(g.publish(2, 1, 50, &[]));
        assert_eq!(
            g.publish(1, 1, 100, &[]),
            Receipt::Duplicate,
            "at the limit"
        );
        deliver(g.publish(3, 1, 101, &[])); // item 1 is now older than 100 ms
        assert_eq!(g.stats().expired, 1);
        assert_eq!(g.seen_len(), 2);
        // A clock that went back forgets nothing.
        deliver(g.publish(4, 1, 0, &[]));
        assert_eq!(g.seen_len(), 3);
    }

    /// A zone of `n` peers on a lossless network, delivered in send order. Returns how many
    /// peers got the update (the publisher excluded), the copies sent and the duplicates.
    fn flood(n: Peer, cfg: GossipConfig, item: Item) -> (u64, u64, u64) {
        let subs: Vec<Peer> = (1..=n).collect();
        let mut peers: BTreeMap<Peer, Gossip<Peer, Item>> =
            subs.iter().map(|&p| (p, peer(p, cfg))).collect();
        let mut queue = VecDeque::new();
        let first = deliver(
            peers
                .get_mut(&1)
                .expect("peer 1")
                .publish(item, 1, 0, &subs),
        );
        for to in first.to {
            queue.push_back((1, to, rumor(item, 1, 1, first.hops)));
        }
        while let Some((from, to, r)) = queue.pop_front() {
            let g = peers.get_mut(&to).expect("subscriber");
            if let Receipt::Deliver(f) = g.receive(&r, from, 0, &subs) {
                for next in f.to {
                    queue.push_back((to, next, Rumor { hops: f.hops, ..r }));
                }
            }
        }
        let total = peers.values().fold(GossipStats::default(), |mut a, g| {
            let s = g.stats();
            a.delivered += s.delivered;
            a.sent += s.sent;
            a.duplicates += s.duplicates;
            a
        });
        (total.delivered, total.sent, total.duplicates)
    }

    #[test]
    fn a_zone_of_fifty_is_mostly_covered_with_bounded_cost() {
        // Forward-once push gossip misses a peer that nobody picks, about e^-k of them (D341), so
        // coverage is a share, not all: about 95% here with k = 3.
        let cfg = GossipConfig::default();
        let mut reached = 0;
        for item in 0..100 {
            let (delivered, sent, duplicates) = flood(50, cfg, item);
            assert!(
                delivered >= 40,
                "item {item} reached only {delivered} of 49"
            );
            // At most k sends per peer that has the update, publisher included.
            assert!(sent <= 3 * (delivered + 1), "sent {sent}");
            // Every copy sent was either delivered once or suppressed.
            assert_eq!(sent, delivered + duplicates);
            reached += delivered;
        }
        assert!(
            reached >= 100 * 49 * 92 / 100,
            "mean coverage {reached}/4900"
        );
    }

    #[test]
    fn the_hop_limit_bounds_the_reach() {
        let cfg = GossipConfig {
            fanout: 2,
            hop_limit: 2,
            ..GossipConfig::default()
        };
        // 2 + 4 = 6 peers at most, however large the zone.
        for item in 0..20 {
            let (delivered, sent, _) = flood(200, cfg, item);
            assert!(delivered <= cfg.reach_bound(), "delivered {delivered}");
            assert!(sent <= cfg.reach_bound());
        }
    }
}
