//! Zone publish-subscribe among peers (Phase 2, Part I, D340–D368): the pure dissemination core,
//! and the agent that runs it in every client program.
//!
//! A program whose users subscribe to a zone gets each update for that zone pushed to it, and
//! pushes it on to a few other programs in the zone, so an update is published once and peers
//! spread it (D340, `docs/architecture.md` section 13.3).
//!
//! **The core** ([`Gossip`]) holds only the rules. No network, no clock and no randomness: time
//! comes in as `now_ms`, and the zone's programs are passed in on every call. Only `BTreeMap`s are
//! iterated.
//!
//! - **A peer is a program (D341).** Targets are [`ClientId`]s: all users of one program share its
//!   peer endpoint (D319), and a zone message reaching a program reaches all of its users in the
//!   zone, so picking users would send one program the same copy several times.
//! - **Duplicate suppression, two checks (D342, D343).** An [`UpdateId`] seen before is a
//!   [`Receipt::Duplicate`]. A zone update whose version is at most the highest this program has
//!   seen for its [`Subject`] is [`Receipt::Obsolete`], so a late old version never spreads over a
//!   newer one (D318). Neither is delivered or forwarded.
//! - **Bounded caches (D344).** Each of the two caches holds at most [`DEFAULT_SEEN_CAP`] entries
//!   and forgets one [`DEFAULT_SEEN_MAX_AGE_MS`] after it was recorded, oldest first.
//! - **Bounded forwarding (D345, D346).** The hop rule is D316's: the origin sends with `hops = 0`,
//!   and a program forwards, with one more, only while `hops < min(max_hops, cap)`, where the cap is
//!   [`DEFAULT_HOP_CAP`], the same on every peer. A copy received at the bound is delivered, not
//!   forwarded. This hop count is not `Incident.ttl_ms` (D143), and the word TTL is kept for that.
//! - **No storms (D347).** A program never sends to itself, to the program it received the copy
//!   from, or to the origin; never forwards a copy it has seen; and sends at most `k` copies of an
//!   update, [`DEFAULT_FANOUT`]. The `k` targets rank highest by an FNV-1a hash of (update, this
//!   program, candidate), as rendezvous hashing does: no randomness, yet each program and each
//!   update picks differently.
//! - **Who forwards (D348).** Only zone messages (`to: None`), and only by a program with a user
//!   subscribed to the zone; any other program gets [`Receipt::NotSubscribed`]. A direct message
//!   (`to: Some(user)`) is checked for a repeated ID, delivered, and never forwarded.
//! - **Acks (D349, D353).** [`Receipt::ack`] maps an outcome to a `P2pAck`. Part H's endpoint
//!   answers `deliver` before gossip sees an update, so gossip's outcomes are counted where they
//!   land, in events, and never reach the sender (D353).
//! - **Counters (D350).** [`GossipStats`] counts what the report measures.
//!
//! **The agent** ([`GossipAgent`], D360, D361) is the core's one user: a task per program, started by
//! `MeshClient::new`. It takes the P2P inbox, judges each arriving update with the core, forwards
//! the new ones and hands them to the application through `MeshClient::take_peer_updates`. When a
//! report's answer is committed, `MeshClient` hands it here and the agent publishes the incident's
//! summary and assignment into its zone as origin (D351, D355, D356, D366). A zone's programs come
//! from Part G's directory, which answers a zone query with one record per program first (D288),
//! kept as one address per program, cached and asked for again at most every `refresh_ms` (D357,
//! D358). The program subscribes to the zones of its joined users (D359). Copies go out
//! through `send_peer`, concurrently and at most `max_in_flight` at once (D362, D363).
//!
//! Copies that cannot get through, because the target is briefly away or busy, go through Part
//! K's store-and-forward and are sent again later (D415). One stand-in is left, marked `TODO(I)`:
//! the origin passed as the sender of each copy, until the lab shows whether it matters (D354,
//! D369).
//!
//! Owner: I (zone publish-subscribe).

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use thiserror::Error;
use tokio::sync::{Semaphore, mpsc};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::Instant;

use crate::config::GossipSettings;
use crate::metrics::{Event, EventLog, now_ms};
use crate::offline::Forwarder;
use crate::p2p::{Delivered, INBOX_CAPACITY, PeerError, PeerLink};
use crate::presence::Presence;
use crate::proto::{
    AssignmentUpdate, ClientId, CommittedResult, IncidentId, IncidentSummary, MAX_DISCOVER_RECORDS,
    P2pAck, P2pBody, P2pEnvelope, PresenceQuery, ResourceId, UpdateId, ZoneId,
};

/// Programs each program sends a new zone update to (D345).
pub const DEFAULT_FANOUT: usize = 3;

/// The `max_hops` an origin puts on a zone update, and the most any peer obeys (D345, D346). With
/// `hops` starting at 0, a copy crosses at most `DEFAULT_HOP_CAP + 1` links.
pub const DEFAULT_HOP_CAP: u8 = 7;

/// Most entries each seen-cache holds: update IDs, and versions per subject (D344).
pub const DEFAULT_SEEN_CAP: usize = 10_000;

/// How long a seen-cache entry is kept after it was recorded, in ms (D344). Matches the 5-minute
/// store-and-forward retention planned for Part K, so a late stored copy is still recognised.
pub const DEFAULT_SEEN_MAX_AGE_MS: u64 = 300_000;

/// How often a zone's list of programs is asked for again, at most, in ms (D358).
pub const DEFAULT_REFRESH_MS: u64 = 10_000;

/// Most gossip sends in flight at once per program (D363). Matches the stub cache (D463).
pub const DEFAULT_MAX_IN_FLIGHT: usize = 256;

/// Committed answers waiting for the agent to publish them. One past it is not published, so
/// `submit` never waits on gossip (D366).
pub const AGENT_QUEUE: usize = 4_096;

/// The reason in `P2pAck::Refused` for an older version of a subject (D349).
pub const REFUSED_OBSOLETE: &str = "obsolete";

/// The reason in `P2pAck::Refused` for a zone message to a program with no subscriber (D348).
pub const REFUSED_NOT_SUBSCRIBED: &str = "not subscribed";

/// The bounds of dissemination. Constants in PR 1; `[gossip]` config keys with the wiring (D350).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GossipConfig {
    /// Programs a program sends a new zone update to.
    pub fanout: usize,
    /// The `max_hops` this program puts on what it publishes, and the most it obeys on what it
    /// receives.
    pub hop_cap: u8,
    /// Most entries each seen-cache holds.
    pub seen_cap: usize,
    /// How long a seen-cache entry is kept, in ms.
    pub seen_max_age_ms: u64,
}

impl Default for GossipConfig {
    fn default() -> Self {
        Self {
            fanout: DEFAULT_FANOUT,
            hop_cap: DEFAULT_HOP_CAP,
            seen_cap: DEFAULT_SEEN_CAP,
            seen_max_age_ms: DEFAULT_SEEN_MAX_AGE_MS,
        }
    }
}

/// A [`GossipConfig`] that cannot work. A hop cap of 0 is allowed: the origin's copies still go
/// out, and nobody forwards them.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GossipConfigError {
    #[error("gossip fanout must be at least 1")]
    ZeroFanout,
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
        if self.seen_cap == 0 {
            return Err(GossipConfigError::ZeroSeenCap);
        }
        if self.seen_max_age_ms == 0 {
            return Err(GossipConfigError::ZeroSeenAge);
        }
        Ok(())
    }

    /// Most programs one update can reach, origin excluded: `k + k² + … + k^(cap+1)`, saturating.
    /// Overlapping picks keep the real reach well below it.
    pub fn reach_bound(&self) -> u64 {
        let k = u64::try_from(self.fanout).unwrap_or(u64::MAX);
        let mut level = 1u64;
        let mut total = 0u64;
        for _ in 0..=self.hop_cap {
            level = level.saturating_mul(k);
            total = total.saturating_add(level);
        }
        total
    }
}

/// What versions are counted per: one subject per kind and its natural key (D343). An incident's
/// summary and its assignment are separate subjects, so neither makes the other look old.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Subject {
    Incident(IncidentId),
    Assignment(IncidentId),
    /// One offer, by the ID its provider gave it (D372, D490). Two offers of the same capability
    /// from one provider are two subjects.
    Offer(ResourceId),
}

impl Subject {
    /// The subject an update is about.
    pub fn of(body: &P2pBody) -> Self {
        match body {
            P2pBody::IncidentSummary(s) => Self::Incident(s.incident),
            P2pBody::AssignmentUpdate(a) => Self::Assignment(a.incident),
            P2pBody::ResourceOffer(o) => Self::Offer(o.id),
        }
    }
}

/// Where a program sends an update on, and the `hops` the copies carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forward {
    /// The targets, at most the fan-out, best ranked first. Empty at the hop bound, for a direct
    /// message, or when nobody is left to send to.
    pub to: Vec<ClientId>,
    /// The `hops` to put on each copy sent.
    pub hops: u8,
}

impl Forward {
    /// The copy of `envelope` to send to each of [`Forward::to`].
    pub fn copy_of(&self, envelope: &P2pEnvelope) -> P2pEnvelope {
        P2pEnvelope {
            hops: self.hops,
            ..envelope.clone()
        }
    }
}

/// What a program does with an update it received or publishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Receipt {
    /// New here: hand it to the application, and send copies as [`Forward`] says.
    New(Forward),
    /// This update ID was seen already. Dropped.
    Duplicate,
    /// The same or a newer version of its subject was seen already. Dropped.
    Obsolete,
    /// A zone message, and no user of this program subscribes to the zone. Dropped, not recorded,
    /// so it is new if the program subscribes later.
    NotSubscribed,
}

impl Receipt {
    /// The `P2pAck` for this receipt (D349). Part H's endpoint sends its own reply today; whether
    /// this one replaces it is open (D353).
    pub fn ack(&self) -> P2pAck {
        match self {
            Self::New(_) => P2pAck::Accepted,
            Self::Duplicate => P2pAck::Duplicate,
            Self::Obsolete => P2pAck::Refused(REFUSED_OBSOLETE.to_string()),
            Self::NotSubscribed => P2pAck::Refused(REFUSED_NOT_SUBSCRIBED.to_string()),
        }
    }
}

/// The origin's envelope is not this program's to publish.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PublishError {
    #[error("update {id} names origin {}, not this program {me}", id.origin)]
    NotOrigin { id: UpdateId, me: ClientId },
    #[error("update {id} has hops {hops}; an origin sends with 0")]
    NotFresh { id: UpdateId, hops: u8 },
}

/// Counters for the report (D350). Duplicate rate = `duplicates / delivered`; forwarding overhead
/// = `sent` summed over all programs, per update; coverage = programs that delivered it over the
/// zone's programs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GossipStats {
    /// Updates this program published.
    pub published: u64,
    /// Copies received from other programs.
    pub received: u64,
    /// Received copies that were new, handed to the application.
    pub delivered: u64,
    /// Received copies whose update ID was seen already.
    pub duplicates: u64,
    /// Received copies of a version already seen or older.
    pub obsolete: u64,
    /// Received zone copies for a zone this program does not subscribe to.
    pub not_subscribed: u64,
    /// Copies this program sent, publishing and forwarding.
    pub sent: u64,
    /// Delivered zone copies not forwarded because they had reached the hop bound.
    pub hop_limited: u64,
    /// Cache entries forgotten because a cache was full.
    pub evicted: u64,
    /// Cache entries forgotten because they were older than the age limit.
    pub expired: u64,
}

/// One program's dissemination state: its two seen-caches and its counters.
#[derive(Debug, Clone)]
pub struct Gossip {
    me: ClientId,
    cfg: GossipConfig,
    ids: Recent<UpdateId>,
    versions: Recent<Subject>,
    stats: GossipStats,
}

impl Gossip {
    /// The gossip state of program `me`, with the given bounds.
    pub fn new(me: ClientId, cfg: GossipConfig) -> Result<Self, GossipConfigError> {
        cfg.validate()?;
        Ok(Self {
            me,
            cfg,
            ids: Recent::default(),
            versions: Recent::default(),
            stats: GossipStats::default(),
        })
    }

    pub fn me(&self) -> ClientId {
        self.me
    }

    pub fn config(&self) -> &GossipConfig {
        &self.cfg
    }

    pub fn stats(&self) -> &GossipStats {
        &self.stats
    }

    /// Entries in the update-ID cache and in the version cache now.
    pub fn seen_len(&self) -> (usize, usize) {
        (self.ids.len(), self.versions.len())
    }

    /// Publish an update this program made into its zone's `zone_programs`. The envelope must name
    /// this program as origin and carry `hops = 0`; set its `max_hops` from [`GossipConfig::hop_cap`].
    /// The origin's own sends are not forwards, so they go out whatever `max_hops` says. A repeated
    /// ID or a version already seen is not sent again.
    pub fn publish(
        &mut self,
        envelope: &P2pEnvelope,
        now_ms: u64,
        zone_programs: &[ClientId],
    ) -> Result<Receipt, PublishError> {
        if envelope.id.origin != self.me {
            return Err(PublishError::NotOrigin {
                id: envelope.id,
                me: self.me,
            });
        }
        if envelope.hops != 0 {
            return Err(PublishError::NotFresh {
                id: envelope.id,
                hops: envelope.hops,
            });
        }
        self.expire(now_ms);
        if let Some(dropped) = self.check_and_record(envelope, now_ms) {
            return Ok(dropped);
        }
        self.stats.published += 1;
        if envelope.to.is_some() {
            return Ok(Receipt::New(Forward {
                to: Vec::new(),
                hops: 0,
            }));
        }
        let to = self.targets(envelope, self.me, zone_programs);
        Ok(Receipt::New(Forward { to, hops: 0 }))
    }

    /// A copy `envelope` arrived from program `sender`, the one that sent this copy, which is the
    /// origin only on the first hop; until D354 is settled the wiring may pass the origin, which only
    /// costs an extra duplicate. `subscribed` says whether a user of this program subscribes to
    /// `envelope.zone`; it matters only for zone messages. `zone_programs` are the zone's programs.
    pub fn receive(
        &mut self,
        envelope: &P2pEnvelope,
        sender: ClientId,
        subscribed: bool,
        now_ms: u64,
        zone_programs: &[ClientId],
    ) -> Receipt {
        self.stats.received += 1;
        self.expire(now_ms);
        let receipt = self.accept(envelope, sender, subscribed, now_ms, zone_programs);
        match &receipt {
            Receipt::New(_) => self.stats.delivered += 1,
            Receipt::Duplicate => self.stats.duplicates += 1,
            Receipt::Obsolete => self.stats.obsolete += 1,
            Receipt::NotSubscribed => self.stats.not_subscribed += 1,
        }
        receipt
    }

    fn accept(
        &mut self,
        envelope: &P2pEnvelope,
        sender: ClientId,
        subscribed: bool,
        now_ms: u64,
        zone_programs: &[ClientId],
    ) -> Receipt {
        let zone_message = envelope.to.is_none();
        if zone_message && !subscribed {
            return Receipt::NotSubscribed;
        }
        if let Some(dropped) = self.check_and_record(envelope, now_ms) {
            return dropped;
        }
        let none = Forward {
            to: Vec::new(),
            hops: envelope.hops,
        };
        if !zone_message {
            return Receipt::New(none);
        }
        if envelope.hops >= envelope.max_hops.min(self.cfg.hop_cap) {
            self.stats.hop_limited += 1;
            return Receipt::New(none);
        }
        let to = self.targets(envelope, sender, zone_programs);
        Receipt::New(Forward {
            to,
            hops: envelope.hops + 1,
        })
    }

    /// The two duplicate checks (D342). Records the update when it is new and returns `None`; a
    /// direct message is checked by ID only and leaves the version cache alone, so that a direct
    /// copy never makes the zone's copy of the same version look old.
    fn check_and_record(&mut self, envelope: &P2pEnvelope, now_ms: u64) -> Option<Receipt> {
        if self.ids.get(&envelope.id).is_some() {
            return Some(Receipt::Duplicate);
        }
        let zone_message = envelope.to.is_none();
        let subject = Subject::of(&envelope.body);
        if zone_message {
            if let Some(v) = self.versions.get(&subject) {
                if envelope.version <= v {
                    return Some(Receipt::Obsolete);
                }
            }
        }
        self.stats.evicted += self.ids.record(envelope.id, 0, now_ms, self.cfg.seen_cap);
        if zone_message {
            self.stats.evicted +=
                self.versions
                    .record(subject, envelope.version, now_ms, self.cfg.seen_cap);
        }
        None
    }

    fn expire(&mut self, now_ms: u64) {
        let age = self.cfg.seen_max_age_ms;
        self.stats.expired += self.ids.expire(now_ms, age) + self.versions.expire(now_ms, age);
    }

    fn targets(
        &mut self,
        envelope: &P2pEnvelope,
        sender: ClientId,
        zone_programs: &[ClientId],
    ) -> Vec<ClientId> {
        let to = pick_targets(
            self.me,
            envelope.id,
            zone_programs,
            &[self.me, sender, envelope.id.origin],
            self.cfg.fanout,
        );
        self.stats.sent += u64::try_from(to.len()).unwrap_or(u64::MAX);
        to
    }
}

/// The `k` candidates that rank highest for (`picker`, `update`), leaving out `exclude` and repeats
/// (D347). The order of `candidates` does not matter. Public so that the cloud's single publish
/// into a zone (PR 2) picks its first targets by the same rule.
pub fn pick_targets(
    picker: ClientId,
    update: UpdateId,
    candidates: &[ClientId],
    exclude: &[ClientId],
    k: usize,
) -> Vec<ClientId> {
    let unique: BTreeSet<ClientId> = candidates
        .iter()
        .copied()
        .filter(|c| !exclude.contains(c))
        .collect();
    let mut ranked: Vec<(u64, ClientId)> = unique
        .into_iter()
        .map(|c| (rank(picker, update, c), c))
        .collect();
    // Highest score first; equal scores (vanishingly rare) fall back to ID order.
    ranked.sort_unstable_by_key(|&(score, c)| (Reverse(score), c));
    ranked.into_iter().take(k).map(|(_, c)| c).collect()
}

fn rank(picker: ClientId, update: UpdateId, candidate: ClientId) -> u64 {
    let mut h = Fnv::new();
    update.hash(&mut h);
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

    fn write_u32(&mut self, i: u32) {
        self.write(&i.to_le_bytes());
    }

    fn write_u64(&mut self, i: u64) {
        self.write(&i.to_le_bytes());
    }

    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64);
    }
}

/// Keys recorded recently, each with a value, forgotten oldest first by count and by age (D344).
#[derive(Debug, Clone)]
struct Recent<K> {
    by_key: BTreeMap<K, Entry>,
    /// Keys in the order they were last recorded.
    by_order: BTreeMap<u64, K>,
    next_order: u64,
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    value: u64,
    at_ms: u64,
    order: u64,
}

impl<K> Default for Recent<K> {
    fn default() -> Self {
        Self {
            by_key: BTreeMap::new(),
            by_order: BTreeMap::new(),
            next_order: 0,
        }
    }
}

impl<K: Ord + Copy> Recent<K> {
    fn len(&self) -> usize {
        self.by_key.len()
    }

    fn get(&self, key: &K) -> Option<u64> {
        self.by_key.get(key).map(|e| e.value)
    }

    /// Forgets entries older than `max_age_ms`, oldest recorded first; returns how many. A clock
    /// that went back forgets nothing.
    fn expire(&mut self, now_ms: u64, max_age_ms: u64) -> u64 {
        let mut forgotten = 0;
        while let Some((&order, &key)) = self.by_order.first_key_value() {
            let at_ms = self.by_key.get(&key).map_or(0, |e| e.at_ms);
            if now_ms.saturating_sub(at_ms) <= max_age_ms {
                break;
            }
            self.by_order.remove(&order);
            self.by_key.remove(&key);
            forgotten += 1;
        }
        forgotten
    }

    /// Records `value` for `key` as the newest entry, then forgets the oldest past `cap`; returns
    /// how many it forgot.
    fn record(&mut self, key: K, value: u64, now_ms: u64, cap: usize) -> u64 {
        if let Some(old) = self.by_key.get(&key) {
            self.by_order.remove(&old.order);
        }
        let order = self.next_order;
        self.next_order += 1;
        self.by_key.insert(
            key,
            Entry {
                value,
                at_ms: now_ms,
                order,
            },
        );
        self.by_order.insert(order, key);
        let mut evicted = 0;
        while self.by_key.len() > cap {
            let Some((_, oldest)) = self.by_order.pop_first() else {
                break;
            };
            self.by_key.remove(&oldest);
            evicted += 1;
        }
        evicted
    }
}

// ---------------------------------------------------------------------------------------------
// The agent: the core at work in one client program (D360, D361)
// ---------------------------------------------------------------------------------------------

/// What an update is, for the events.
fn kind_name(body: &P2pBody) -> &'static str {
    match body {
        P2pBody::IncidentSummary(_) => "summary",
        P2pBody::AssignmentUpdate(_) => "assignment",
        P2pBody::ResourceOffer(_) => "offer",
    }
}

fn outcome_name(receipt: &Receipt) -> &'static str {
    match receipt {
        Receipt::New(_) => "new",
        Receipt::Duplicate => "duplicate",
        Receipt::Obsolete => "obsolete",
        Receipt::NotSubscribed => "not_subscribed",
    }
}

/// The two updates a committed answer is published as (D355): the summary at version 1, since a
/// report's facts never change, and the assignment at the commit's log index, which only grows and
/// is the same whoever reads it.
pub fn published_bodies(result: &CommittedResult) -> [(u64, P2pBody); 2] {
    let incident = &result.incident;
    [
        (
            1,
            P2pBody::IncidentSummary(IncidentSummary {
                incident: result.request,
                category: incident.category,
                severity: incident.severity,
                zone: incident.zone,
                location: incident.location,
                needs: incident.needs,
                reported_at_ms: incident.reported_at_ms,
            }),
        ),
        (
            result.log_index,
            P2pBody::AssignmentUpdate(AssignmentUpdate {
                incident: result.request,
                responder: result.plan.responder,
                priority: result.plan.priority,
                assigned_at_ms: result.assigned_at_ms,
            }),
        ),
    ]
}

/// One zone's programs, as the directory last listed them (D357, D358).
#[derive(Default)]
struct ZoneList {
    /// One peer address per program.
    programs: BTreeMap<ClientId, String>,
    /// When the last answer came; `None` until one has.
    fetched: Option<Instant>,
    /// A refresh is running in the background.
    refreshing: bool,
}

/// The agent's handle, held by `MeshClient`. Dropping it ends the task.
pub(crate) struct GossipAgent {
    committed: mpsc::Sender<CommittedResult>,
    updates: Mutex<Option<mpsc::Receiver<P2pEnvelope>>>,
    task: JoinHandle<()>,
}

impl GossipAgent {
    /// Start the agent of program `me`. `inbox` is the P2P endpoint's (D460). Must run inside a
    /// tokio runtime.
    pub(crate) fn start(
        me: ClientId,
        settings: GossipSettings,
        peer: Arc<PeerLink>,
        forwarder: Arc<Forwarder>,
        presence: Arc<Presence>,
        inbox: mpsc::Receiver<P2pEnvelope>,
        log: EventLog,
    ) -> Self {
        let (committed_tx, committed_rx) = mpsc::channel(AGENT_QUEUE);
        let (out_tx, out_rx) = mpsc::channel(INBOX_CAPACITY);
        let task = tokio::spawn(
            Run {
                me,
                settings,
                peer,
                forwarder,
                presence,
                out: out_tx,
                log,
                zones: BTreeMap::new(),
                sends: JoinSet::new(),
                in_flight: Arc::new(Semaphore::new(settings.max_in_flight)),
            }
            .run(inbox, committed_rx),
        );
        Self {
            committed: committed_tx,
            updates: Mutex::new(Some(out_rx)),
            task,
        }
    }

    /// Publish a committed answer into its zone, in the background (D356, D366). Never waits: past
    /// [`AGENT_QUEUE`] waiting answers this one is not published.
    pub(crate) fn publish(&self, result: &CommittedResult) {
        if self.committed.try_send(result.clone()).is_err() {
            tracing::debug!(request = %result.request, "gossip queue full, not published");
        }
    }

    /// The updates gossip found new, for the application, once (D360).
    pub(crate) fn take_updates(&self) -> Option<mpsc::Receiver<P2pEnvelope>> {
        self.updates
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
    }
}

impl Drop for GossipAgent {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// The agent's task and everything only it touches.
struct Run {
    me: ClientId,
    settings: GossipSettings,
    peer: Arc<PeerLink>,
    /// Part K's store-and-forward: a copy that cannot get through now is held and tried again
    /// (D362, D415).
    forwarder: Arc<Forwarder>,
    presence: Arc<Presence>,
    out: mpsc::Sender<P2pEnvelope>,
    log: EventLog,
    zones: BTreeMap<ZoneId, ZoneList>,
    sends: JoinSet<()>,
    in_flight: Arc<Semaphore>,
}

/// A background refresh's answer: the zone, its programs or `None`, and how long it took.
type Refreshed = (ZoneId, Option<BTreeMap<ClientId, String>>, Duration);

impl Run {
    async fn run(
        mut self,
        mut inbox: mpsc::Receiver<P2pEnvelope>,
        mut committed: mpsc::Receiver<CommittedResult>,
    ) {
        let mut core = match Gossip::new(self.me, self.settings.core()) {
            Ok(core) => core,
            Err(e) => {
                // `Config::validate` refuses these, so only a config built by hand gets here.
                tracing::warn!("gossip is off: {e}");
                return;
            }
        };
        let mut refreshes: JoinSet<Refreshed> = JoinSet::new();
        loop {
            while self.sends.try_join_next().is_some() {}
            tokio::select! {
                Some(envelope) = inbox.recv() => self.arrived(&mut core, envelope, &mut refreshes).await,
                Some(result) = committed.recv() => self.publish(&mut core, &result, &mut refreshes).await,
                Some(Ok(refreshed)) = refreshes.join_next(), if !refreshes.is_empty() => {
                    self.refreshed(refreshed);
                }
                else => break,
            }
        }
    }

    /// An update the endpoint let through (D360). The sender passed to the core is the origin,
    /// because the inbox does not say which program sent this copy (D354).
    async fn arrived(
        &mut self,
        core: &mut Gossip,
        envelope: P2pEnvelope,
        refreshes: &mut JoinSet<Refreshed>,
    ) {
        let zone_message = envelope.to.is_none();
        let subscribed = zone_message && self.presence.zones().contains(&envelope.zone);
        let programs = if subscribed {
            self.programs_of(envelope.zone, refreshes).await
        } else {
            BTreeMap::new()
        };
        let ids: Vec<ClientId> = programs.keys().copied().collect();
        // TODO(I): pass the program this copy came from, if D354 is revisited after the lab runs.
        let receipt = core.receive(&envelope, envelope.id.origin, subscribed, now_ms(), &ids);
        let forwarded = match &receipt {
            Receipt::New(f) => f.to.len(),
            _ => 0,
        };
        self.log.log(Event::GossipReceived {
            update: envelope.id,
            zone: envelope.zone,
            kind: kind_name(&envelope.body),
            hops: envelope.hops,
            outcome: outcome_name(&receipt),
            forwarded: u32::try_from(forwarded).unwrap_or(u32::MAX),
            age_ms: now_ms().saturating_sub(envelope.created_at_ms),
        });
        if let Receipt::New(f) = receipt {
            self.send_copies(&f, &envelope, &programs).await;
            // An application that does not read, like loadgen today, must not stop the forwarding.
            let _ = self.out.try_send(envelope);
        }
    }

    /// Publish a committed answer as origin (D351, D355). A retry's answer has the same versions,
    /// so the core finds it obsolete and nothing is sent again.
    async fn publish(
        &mut self,
        core: &mut Gossip,
        result: &CommittedResult,
        refreshes: &mut JoinSet<Refreshed>,
    ) {
        let zone = result.incident.zone;
        let programs = self.programs_of(zone, refreshes).await;
        let ids: Vec<ClientId> = programs.keys().copied().collect();
        for (version, body) in published_bodies(result) {
            let kind = kind_name(&body);
            let envelope = self.peer.new_update(
                result.incident.owner,
                None,
                zone,
                version,
                self.settings.hop_cap,
                body,
            );
            match core.publish(&envelope, now_ms(), &ids) {
                Ok(Receipt::New(f)) => {
                    // Logged with no targets too, so every publish shows in the log (D367).
                    self.log.log(Event::GossipPublished {
                        update: envelope.id,
                        zone,
                        kind,
                        version,
                        targets: u32::try_from(f.to.len()).unwrap_or(u32::MAX),
                    });
                    self.send_copies(&f, &envelope, &programs).await;
                }
                Ok(_) => {}
                Err(e) => tracing::warn!("gossip publish refused: {e}"),
            }
        }
    }

    /// Send one copy to each target, off this task, at most `max_in_flight` at once: past it this
    /// waits for a slot, so the inbox fills and the endpoint answers `busy` (D362, D363).
    async fn send_copies(
        &mut self,
        forward: &Forward,
        envelope: &P2pEnvelope,
        programs: &BTreeMap<ClientId, String>,
    ) {
        for target in &forward.to {
            let Some(addr) = programs.get(target).cloned() else {
                continue;
            };
            let Ok(permit) = self.in_flight.clone().acquire_owned().await else {
                return;
            };
            let copy = forward.copy_of(envelope);
            let (peer, forwarder, log) =
                (self.peer.clone(), self.forwarder.clone(), self.log.clone());
            self.sends.spawn(async move {
                // A peer that is briefly away, or busy, gets the copy later through Part K's
                // store-and-forward, held for at most 300 s, as long as receivers remember IDs
                // (D362, D408, D415). Any other refusal is final and only logged.
                let update = copy.id;
                let reason = match peer.send_peer(&addr, copy.clone()).await {
                    Ok(Delivered {
                        ack: P2pAck::Refused(reason),
                        ..
                    }) if reason == "busy" => {
                        forwarder.hold(&addr, copy, &log);
                        None
                    }
                    Ok(Delivered {
                        ack: P2pAck::Refused(reason),
                        ..
                    }) => Some(reason),
                    Ok(_) => None,
                    Err(PeerError::Offline | PeerError::GaveUp { .. }) => {
                        forwarder.hold(&addr, copy, &log);
                        None
                    }
                    Err(e) => Some(e.to_string()),
                };
                if let Some(reason) = reason {
                    log.log(Event::GossipSendFailed {
                        update,
                        to_addr: addr,
                        reason,
                    });
                }
                drop(permit);
            });
        }
    }

    /// The zone's programs. The first time, this waits for the directory, at most `refresh_ms`; after
    /// that it answers from the cache and refreshes in the background once the list is older than
    /// `refresh_ms` (D358).
    async fn programs_of(
        &mut self,
        zone: ZoneId,
        refreshes: &mut JoinSet<Refreshed>,
    ) -> BTreeMap<ClientId, String> {
        let refresh = self.settings.refresh();
        let list = self.zones.entry(zone).or_default();
        match list.fetched {
            None if !list.refreshing => {
                list.refreshing = true;
                let fetch = discover_programs(self.presence.clone(), zone);
                let answer = match tokio::time::timeout(refresh, fetch).await {
                    Ok(answer) => answer,
                    Err(_) => (zone, None, refresh),
                };
                self.refreshed(answer);
            }
            Some(at) if at.elapsed() >= refresh && !list.refreshing => {
                list.refreshing = true;
                refreshes.spawn(discover_programs(self.presence.clone(), zone));
            }
            _ => {}
        }
        self.zones
            .get(&zone)
            .map(|list| list.programs.clone())
            .unwrap_or_default()
    }

    /// A directory answer for one zone. A failed one keeps the old list and is tried again after
    /// `refresh_ms` (D358).
    fn refreshed(&mut self, (zone, programs, took): Refreshed) {
        let list = self.zones.entry(zone).or_default();
        list.refreshing = false;
        list.fetched = Some(Instant::now());
        let ok = programs.is_some();
        if let Some(programs) = programs {
            list.programs = programs;
        }
        self.log.log(Event::GossipZoneRefreshed {
            zone,
            programs: u32::try_from(list.programs.len()).unwrap_or(u32::MAX),
            ok,
            latency_us: took.as_micros() as u64,
        });
    }
}

/// Ask the directory for `zone` and keep one peer address per program (D357). At most 16 records
/// come back (D321), one per program before any program's second (D288), so a list reaches up to
/// 16 programs.
async fn discover_programs(presence: Arc<Presence>, zone: ZoneId) -> Refreshed {
    let started = Instant::now();
    let answer = presence
        .discover(PresenceQuery {
            zone: Some(zone),
            capability: None,
            limit: MAX_DISCOVER_RECORDS,
        })
        .await;
    let programs = answer.ok().map(|found| {
        let mut programs = BTreeMap::new();
        for record in found.records {
            programs
                .entry(record.user.client)
                .or_insert(record.peer_addr);
        }
        programs
    });
    (zone, programs, started.elapsed())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::proto::{Capability, Category, Point, RequestId, ResourceOffer, UserId};

    const ZONE: u16 = 2;

    fn gossip(me: ClientId, cfg: GossipConfig) -> Gossip {
        Gossip::new(me, cfg).expect("valid config")
    }

    fn incident(n: u64) -> IncidentId {
        RequestId { client: 9, seq: n }
    }

    fn summary(n: u64) -> P2pBody {
        P2pBody::IncidentSummary(IncidentSummary {
            incident: incident(n),
            category: Category::LabIncident,
            severity: 3,
            zone: ZONE,
            location: Point { x: 10, y: 20 },
            needs: Capability::Fire,
            reported_at_ms: 0,
        })
    }

    /// A zone update from `origin`, number `seq`, about incident `n` at `version`.
    fn update(origin: ClientId, seq: u64, n: u64, version: u64) -> P2pEnvelope {
        P2pEnvelope {
            id: UpdateId { origin, seq },
            from: UserId {
                client: origin,
                user: 0,
            },
            to: None,
            zone: ZONE,
            version,
            created_at_ms: 0,
            hops: 0,
            max_hops: DEFAULT_HOP_CAP,
            body: summary(n),
        }
    }

    fn at_hops(mut e: P2pEnvelope, hops: u8) -> P2pEnvelope {
        e.hops = hops;
        e
    }

    fn new(r: Receipt) -> Forward {
        match r {
            Receipt::New(f) => f,
            other => panic!("expected New, got {other:?}"),
        }
    }

    fn programs(n: ClientId) -> Vec<ClientId> {
        (1..=n).collect()
    }

    #[test]
    fn defaults_are_the_logged_values() {
        let cfg = GossipConfig::default();
        assert_eq!(cfg.fanout, 3);
        assert_eq!(cfg.hop_cap, 7);
        assert_eq!(cfg.seen_cap, 10_000);
        assert_eq!(cfg.seen_max_age_ms, 300_000);
        // 3 + 9 + ... + 3^8.
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
            assert_eq!(Gossip::new(1, cfg).err(), Some(err));
        }
        assert!(Gossip::new(1, GossipConfig { hop_cap: 0, ..ok }).is_ok());
    }

    #[test]
    fn publish_sends_fanout_copies_with_hops_zero() {
        let mut g = gossip(1, GossipConfig::default());
        let f = new(g
            .publish(&update(1, 1, 7, 1), 0, &programs(10))
            .expect("own update"));
        assert_eq!(f.hops, 0);
        assert_eq!(f.to.len(), 3);
        assert!(!f.to.contains(&1), "never to itself");
        assert_eq!(g.stats().published, 1);
        assert_eq!(g.stats().sent, 3);
    }

    #[test]
    fn only_the_origin_publishes_and_only_fresh_copies() {
        let mut g = gossip(1, GossipConfig::default());
        let e = update(2, 1, 7, 1);
        assert_eq!(
            g.publish(&e, 0, &programs(5)),
            Err(PublishError::NotOrigin { id: e.id, me: 1 })
        );
        let e = at_hops(update(1, 1, 7, 1), 1);
        assert_eq!(
            g.publish(&e, 0, &programs(5)),
            Err(PublishError::NotFresh { id: e.id, hops: 1 })
        );
        assert_eq!(g.stats().published, 0);
    }

    #[test]
    fn a_seen_id_is_a_duplicate_and_is_not_forwarded() {
        let mut g = gossip(5, GossipConfig::default());
        let subs = programs(10);
        new(g.receive(&update(1, 1, 7, 1), 1, true, 0, &subs));
        let again = g.receive(&at_hops(update(1, 1, 7, 1), 1), 3, true, 10, &subs);
        assert_eq!(again, Receipt::Duplicate);
        assert_eq!(again.ack(), P2pAck::Duplicate);
        assert_eq!(g.stats().delivered, 1);
        assert_eq!(g.stats().duplicates, 1);
        assert_eq!(g.stats().sent, 3);
    }

    #[test]
    fn a_newer_version_spreads_and_an_old_or_equal_one_is_obsolete() {
        let mut g = gossip(5, GossipConfig::default());
        let subs = programs(10);
        new(g.receive(&update(1, 2, 7, 2), 1, true, 0, &subs));
        // A late v1, and a second send of v2 under a new ID: neither spreads.
        let late = g.receive(&update(1, 1, 7, 1), 1, true, 1, &subs);
        assert_eq!(late, Receipt::Obsolete);
        assert_eq!(late.ack(), P2pAck::Refused(REFUSED_OBSOLETE.to_string()));
        assert_eq!(
            g.receive(&update(1, 3, 7, 2), 1, true, 2, &subs),
            Receipt::Obsolete
        );
        new(g.receive(&update(1, 4, 7, 3), 1, true, 3, &subs));
        assert_eq!(g.stats().delivered, 2);
        assert_eq!(g.stats().obsolete, 2);
    }

    #[test]
    fn subjects_keep_kinds_apart() {
        let mut g = gossip(5, GossipConfig::default());
        let subs = programs(10);
        new(g.receive(&update(1, 1, 7, 5), 1, true, 0, &subs));
        // The assignment of the same incident at a lower version is its own subject.
        let mut a = update(1, 2, 7, 1);
        a.body = P2pBody::AssignmentUpdate(AssignmentUpdate {
            incident: incident(7),
            responder: None,
            priority: 10,
            assigned_at_ms: 0,
        });
        new(g.receive(&a, 1, true, 1, &subs));
        // An offer is its own subject, by its ID (D490): one provider's two Fire offers are two
        // subjects, and only a repeat of the same offer at the same or an older version is obsolete.
        let offer = |update_seq, resource_seq, capability, version| {
            let mut o = update(1, update_seq, 0, version);
            o.body = P2pBody::ResourceOffer(ResourceOffer {
                id: ResourceId {
                    provider: 1,
                    seq: resource_seq,
                },
                provider: UserId { client: 1, user: 4 },
                capability,
                zone: ZONE,
                location: Point { x: 0, y: 0 },
                capacity: 1,
                available_until_ms: 0,
            });
            o
        };
        new(g.receive(&offer(3, 1, Capability::Fire, 1), 1, true, 2, &subs));
        new(g.receive(&offer(4, 2, Capability::Fire, 1), 1, true, 3, &subs));
        assert_eq!(
            g.receive(&offer(5, 1, Capability::Fire, 1), 1, true, 4, &subs),
            Receipt::Obsolete
        );
        new(g.receive(&offer(6, 1, Capability::Fire, 2), 1, true, 5, &subs));
    }

    #[test]
    fn a_non_subscriber_refuses_a_zone_message_and_remembers_nothing() {
        let mut g = gossip(5, GossipConfig::default());
        let subs = programs(10);
        let r = g.receive(&update(1, 1, 7, 1), 1, false, 0, &subs);
        assert_eq!(r, Receipt::NotSubscribed);
        assert_eq!(r.ack(), P2pAck::Refused(REFUSED_NOT_SUBSCRIBED.to_string()));
        assert_eq!(g.seen_len(), (0, 0));
        assert_eq!(g.stats().sent, 0);
        // Once subscribed, the same update is new.
        let f = new(g.receive(&update(1, 1, 7, 1), 1, true, 1, &subs));
        assert_eq!(f.to.len(), 3);
    }

    #[test]
    fn a_direct_message_is_delivered_once_and_never_forwarded() {
        let mut g = gossip(5, GossipConfig::default());
        let subs = programs(10);
        let mut d = update(1, 1, 7, 2);
        d.to = Some(UserId { client: 5, user: 0 });
        // Subscription does not matter for a direct message.
        let f = new(g.receive(&d, 1, false, 0, &subs));
        assert!(f.to.is_empty());
        assert_eq!(g.receive(&d, 1, false, 1, &subs), Receipt::Duplicate);
        // It left the version cache alone: the zone's copy of v2 still spreads.
        let f = new(g.receive(&update(1, 2, 7, 2), 1, true, 2, &subs));
        assert_eq!(f.to.len(), 3);
        assert_eq!(g.stats().sent, 3);
    }

    #[test]
    fn never_back_to_the_sender_the_origin_or_itself() {
        let subs = programs(5);
        for me in 2..=5 {
            for sender in 1..=5 {
                let mut g = gossip(me, GossipConfig::default());
                let e = at_hops(update(1, 1, 9, 1), 1);
                let f = new(g.receive(&e, sender, true, 0, &subs));
                assert!(!f.to.contains(&me));
                assert!(!f.to.contains(&sender));
                assert!(!f.to.contains(&1), "origin");
            }
        }
    }

    #[test]
    fn forwards_while_hops_are_below_the_envelopes_bound() {
        let subs = programs(10);
        let mut g = gossip(5, GossipConfig::default());
        let mut e = update(1, 1, 7, 1);
        e.max_hops = 2;
        let f = new(g.receive(&at_hops(e.clone(), 1), 1, true, 0, &subs));
        assert_eq!((f.to.len(), f.hops), (3, 2));
        assert_eq!(f.copy_of(&e).hops, 2);
        let mut e = update(1, 2, 8, 1);
        e.max_hops = 2;
        let f = new(g.receive(&at_hops(e, 2), 1, true, 0, &subs));
        assert!(f.to.is_empty(), "at the bound: delivered, not forwarded");
        assert_eq!(g.stats().hop_limited, 1);
    }

    #[test]
    fn the_shared_cap_holds_against_a_large_max_hops() {
        let subs = programs(10);
        let mut g = gossip(5, GossipConfig::default());
        let mut e = update(1, 1, 7, 1);
        e.max_hops = u8::MAX;
        let f = new(g.receive(&at_hops(e.clone(), DEFAULT_HOP_CAP - 1), 1, true, 0, &subs));
        assert_eq!(f.to.len(), 3);
        e.id.seq = 2;
        e.body = summary(8);
        let f = new(g.receive(&at_hops(e, DEFAULT_HOP_CAP), 1, true, 0, &subs));
        assert!(f.to.is_empty());
    }

    #[test]
    fn small_zones_send_to_whoever_is_left() {
        let mut g = gossip(2, GossipConfig::default());
        let f = new(g.receive(&update(1, 1, 7, 1), 1, true, 0, &[1, 2, 3, 3]));
        assert_eq!(f.to, vec![3], "repeats count once");
        let mut g = gossip(2, GossipConfig::default());
        let f = new(g.receive(&update(1, 1, 7, 1), 1, true, 0, &[]));
        assert!(f.to.is_empty());
    }

    #[test]
    fn targets_repeat_and_still_differ_by_program_and_update() {
        let subs = programs(100);
        let id = UpdateId { origin: 1, seq: 7 };
        let a = pick_targets(1, id, &subs, &[1], 3);
        assert_eq!(a, pick_targets(1, id, &subs, &[1], 3));
        let others = [
            pick_targets(2, id, &subs, &[2], 3),
            pick_targets(1, UpdateId { origin: 1, seq: 8 }, &subs, &[1], 3),
            pick_targets(1, UpdateId { origin: 2, seq: 7 }, &subs, &[1], 3),
        ];
        assert!(others.iter().all(|o| *o != a));
        let mut rev = subs.clone();
        rev.reverse();
        assert_eq!(
            a,
            pick_targets(1, id, &rev, &[1], 3),
            "list order does not matter"
        );
    }

    #[test]
    fn targets_spread_evenly_over_the_zone() {
        // 18,000 picks over 20 programs: 900 each on average.
        let subs = programs(20);
        let mut counts = BTreeMap::<ClientId, u32>::new();
        for seq in 0..6_000u64 {
            for t in pick_targets(0, UpdateId { origin: 0, seq }, &subs, &[], 3) {
                *counts.entry(t).or_default() += 1;
            }
        }
        assert_eq!(counts.len(), 20);
        assert!(
            counts.values().all(|&c| (750..=1_050).contains(&c)),
            "{counts:?}"
        );
    }

    #[test]
    fn the_caches_forget_the_oldest_past_their_cap() {
        let cfg = GossipConfig {
            seen_cap: 3,
            ..GossipConfig::default()
        };
        let mut g = gossip(1, cfg);
        for n in 0..5 {
            new(g.publish(&update(1, n, n, 1), n, &[]).expect("own"));
        }
        assert_eq!(g.seen_len(), (3, 3));
        assert_eq!(g.stats().evicted, 4, "two from each cache");
        // Update 0 and incident 0 were forgotten, so it is new again; update 4 is still known.
        new(g.publish(&update(1, 0, 0, 1), 10, &[]).expect("own"));
        assert_eq!(
            g.publish(&update(1, 4, 4, 1), 10, &[]),
            Ok(Receipt::Duplicate)
        );
    }

    #[test]
    fn a_newer_version_moves_its_subject_to_the_back() {
        let cfg = GossipConfig {
            seen_cap: 2,
            ..GossipConfig::default()
        };
        let mut g = gossip(5, cfg);
        let subs = programs(10);
        new(g.receive(&update(1, 1, 1, 1), 1, true, 0, &subs));
        new(g.receive(&update(1, 2, 2, 1), 1, true, 1, &subs));
        new(g.receive(&update(1, 3, 1, 2), 1, true, 2, &subs)); // incident 1 is now newest
        new(g.receive(&update(1, 4, 3, 1), 1, true, 3, &subs)); // so incident 2 goes
        assert_eq!(
            g.receive(&update(1, 5, 1, 2), 1, true, 4, &subs),
            Receipt::Obsolete
        );
        new(g.receive(&update(1, 6, 2, 1), 1, true, 4, &subs));
    }

    #[test]
    fn the_caches_forget_by_age() {
        let cfg = GossipConfig {
            seen_max_age_ms: 100,
            ..GossipConfig::default()
        };
        let mut g = gossip(5, cfg);
        let subs = programs(10);
        new(g.receive(&update(1, 1, 1, 1), 1, true, 0, &subs));
        new(g.receive(&update(1, 2, 2, 1), 1, true, 50, &subs));
        assert_eq!(
            g.receive(&update(1, 1, 1, 1), 1, true, 100, &subs),
            Receipt::Duplicate,
            "at the limit"
        );
        new(g.receive(&update(1, 3, 3, 1), 1, true, 101, &subs));
        assert_eq!(g.stats().expired, 2, "update 1 and incident 1");
        assert_eq!(g.seen_len(), (2, 2));
        // A clock that went back forgets nothing.
        new(g.receive(&update(1, 4, 4, 1), 1, true, 0, &subs));
        assert_eq!(g.seen_len(), (3, 3));
    }

    /// One flood's totals over a zone.
    #[derive(Debug, Default)]
    struct Flood {
        /// Programs that delivered it, the origin excluded.
        delivered: u64,
        sent: u64,
        duplicates: u64,
    }

    /// A zone of `n` programs, all subscribed, on a lossless network, copies delivered in send
    /// order. Program 1 publishes update `seq`.
    fn flood(n: ClientId, cfg: GossipConfig, seq: u64) -> Flood {
        let subs = programs(n);
        let mut zone: BTreeMap<ClientId, Gossip> =
            subs.iter().map(|&p| (p, gossip(p, cfg))).collect();
        let mut e = update(1, seq, seq, 1);
        e.max_hops = cfg.hop_cap;
        let first = new(zone
            .get_mut(&1)
            .expect("program 1")
            .publish(&e, 0, &subs)
            .expect("own"));
        let mut queue: VecDeque<(ClientId, ClientId, P2pEnvelope)> = first
            .to
            .iter()
            .map(|&to| (1, to, first.copy_of(&e)))
            .collect();
        while let Some((from, to, copy)) = queue.pop_front() {
            let g = zone.get_mut(&to).expect("in the zone");
            if let Receipt::New(f) = g.receive(&copy, from, true, 0, &subs) {
                for &next in &f.to {
                    queue.push_back((to, next, f.copy_of(&copy)));
                }
            }
        }
        zone.values().fold(Flood::default(), |mut t, g| {
            t.delivered += g.stats().delivered;
            t.sent += g.stats().sent;
            t.duplicates += g.stats().duplicates;
            t
        })
    }

    #[test]
    fn a_zone_of_five_programs_is_always_covered() {
        // The size the lab runs: 10 programs per client PC, 3 PCs, 6 zones (D341).
        for seq in 0..100 {
            let f = flood(5, GossipConfig::default(), seq);
            assert_eq!(f.delivered, 4, "update {seq}");
            assert_eq!(f.sent, f.delivered + f.duplicates);
        }
    }

    #[test]
    fn a_zone_of_fifty_is_mostly_covered_with_bounded_cost() {
        // Forward-once push gossip misses a program nobody picks, about e^-k of them (D345).
        let cfg = GossipConfig::default();
        let mut reached = 0;
        for seq in 0..100 {
            let f = flood(50, cfg, seq);
            assert!(
                f.delivered >= 40,
                "update {seq} reached {} of 49",
                f.delivered
            );
            // At most k sends per program that has the update, origin included.
            assert!(f.sent <= 3 * (f.delivered + 1), "sent {}", f.sent);
            // Every copy sent was delivered once or suppressed.
            assert_eq!(f.sent, f.delivered + f.duplicates);
            reached += f.delivered;
        }
        assert!(
            reached >= 100 * 49 * 92 / 100,
            "mean coverage {reached}/4900"
        );
    }

    #[test]
    fn the_hop_bound_bounds_the_reach() {
        let cfg = GossipConfig {
            fanout: 2,
            hop_cap: 1,
            ..GossipConfig::default()
        };
        // 2 + 4 = 6 programs at most, however large the zone.
        for seq in 0..20 {
            let f = flood(200, cfg, seq);
            assert!(
                f.delivered <= cfg.reach_bound(),
                "delivered {}",
                f.delivered
            );
            assert!(f.sent <= cfg.reach_bound());
        }
    }

    /// The table in D345. Run with
    /// `cargo test --lib gossip::tests::measure_coverage -- --ignored --nocapture`.
    #[test]
    #[ignore = "a measurement for the design log, not a check"]
    fn measure_coverage() {
        let settings: [(usize, u8); 5] = [(3, 3), (3, 5), (3, 7), (4, 5), (2, 7)];
        println!("zone  k  max_hops  coverage%  sends/update  duplicates/delivered");
        for n in [5, 50, 500] {
            for (fanout, hop_cap) in settings {
                let cfg = GossipConfig {
                    fanout,
                    hop_cap,
                    ..GossipConfig::default()
                };
                let runs = 100u64;
                let mut t = Flood::default();
                for seq in 0..runs {
                    let f = flood(n, cfg, seq);
                    t.delivered += f.delivered;
                    t.sent += f.sent;
                    t.duplicates += f.duplicates;
                }
                let others = runs * (n - 1);
                println!(
                    "{n:>4}  {fanout}  {hop_cap:>8}  {:>9.1}  {:>12.1}  {:>20.2}",
                    100.0 * t.delivered as f64 / others as f64,
                    t.sent as f64 / runs as f64,
                    t.duplicates as f64 / t.delivered.max(1) as f64,
                );
            }
        }
    }
}
