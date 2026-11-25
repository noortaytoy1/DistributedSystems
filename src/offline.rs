//! Offline operation on a client (Phase 2, Part K, D400–D406): the reports a user makes while the
//! client is cut off, kept until it is back.
//!
//! - **The switch (D402).** Going offline closes a [`FaultGate`](crate::node::transport::FaultGate)
//!   inside every one of the client's sockets, as the node's fault gate does: nothing leaves and
//!   nothing arrives, so even an answer already on its way is lost, as with a real disconnection.
//!   `MeshClient::go_offline` and `go_online` drive it.
//! - **The outbox (D401).** A report made while offline gets its request ID at once and goes into
//!   the outbox instead of to the cloud. Each entry is one JSON line in
//!   `<metrics.dir>/outbox-client-<id>.jsonl`, so a program that restarts while offline still has
//!   it; with no metrics directory it is kept in memory only. At [`OUTBOX_CAP`] entries a new report
//!   is refused with an error, so a long outage cannot fill the disk (D405).
//! - **Back online (D403, D404).** Every entry is sent, oldest first and [`FLUSH_IN_FLIGHT`] at a
//!   time, with its original ID, so a report the cloud already has is answered from its record and
//!   never planned twice. A report too old by then is not dropped here: the cloud refuses it and
//!   commits the refusal (D143), so the rule is one rule, on record on every node. An entry that
//!   gets no answer stays for the next reconnect.
//! - **Store-and-forward for peers (D407–D411).** A peer update that cannot get through, because
//!   this program is offline or the peer is, is held by the sender in [`HeldUpdates`] and tried
//!   again every `client_retry_ms` and at once on reconnect. Limits: [`HOLD_PER_PEER`] per peer,
//!   [`HOLD_TOTAL`] in all, oldest dropped first, and [`HOLD_FOR_MS`] from when the update was made,
//!   the time receivers remember IDs, so a late resend is still only a duplicate. A newer version
//!   of the same subject replaces an older one, and an offer whose time has run out is dropped as
//!   obsolete.
//!
//! File reads and writes go through `tokio::fs`, so they never block the runtime.
//!
//! Owner: K (offline operation).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use crate::client::Submitted;
use crate::metrics::{Event, EventLog, now_ms};
use crate::p2p::{Delivered, PeerError, PeerLink};
use crate::proto::{
    Capability, ClientId, Incident, P2pAck, P2pBody, P2pEnvelope, RequestId, UpdateId, UserId,
};

/// Most reports an outbox holds; past it a new report is refused (D405).
pub const OUTBOX_CAP: usize = 10_000;

/// Queued reports sent at once when the client is back (D404).
pub const FLUSH_IN_FLIGHT: usize = 16;

/// One report made while offline, as kept in the outbox and its file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Queued {
    /// Given when the report was made, and kept for every send, so a resend is harmless.
    pub id: RequestId,
    pub incident: Incident,
    /// When it was queued: unix milliseconds on this client's clock.
    pub queued_at_ms: u64,
}

/// What became of a report handed to `MeshClient::submit_or_queue` (D400).
#[derive(Debug, Clone)]
pub enum Delivery {
    /// The client was online and the cloud answered.
    Answered(Submitted),
    /// The client was offline, so the report waits in the outbox under this ID.
    Queued(RequestId),
}

/// What one flush of the outbox did. `took` is the offline reconciliation delay: from the start of
/// the flush to the last answer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reconciled {
    pub sent: usize,
    /// Answered with a plan, committed or provisional.
    pub answered: usize,
    /// Refused by the cloud as too old (D143). Also on record, so also delivered.
    pub expired: usize,
    /// No answer; still in the outbox.
    pub failed: usize,
    pub took: Duration,
}

/// Why a report could not be queued.
#[derive(Debug, thiserror::Error)]
pub enum OutboxError {
    #[error("the outbox is full ({cap} reports)")]
    Full { cap: usize },
    #[error("cannot write the outbox file {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// The reports a client made while offline, oldest first. Safe to share: every method takes `&self`.
pub struct Outbox {
    path: Option<PathBuf>,
    cap: usize,
    state: tokio::sync::Mutex<State>,
}

#[derive(Default)]
struct State {
    /// False until the file has been read, which happens on first use, inside the runtime.
    loaded: bool,
    entries: Vec<Queued>,
}

impl Outbox {
    /// An outbox kept in `path`, or in memory only for `None`.
    pub fn new(path: Option<PathBuf>) -> Self {
        Self::with_cap(path, OUTBOX_CAP)
    }

    pub fn with_cap(path: Option<PathBuf>, cap: usize) -> Self {
        Self {
            path,
            cap,
            state: tokio::sync::Mutex::new(State::default()),
        }
    }

    /// Where client `client` keeps its outbox: next to its event logs, or nowhere.
    pub fn path_for(dir: Option<&Path>, client: ClientId) -> Option<PathBuf> {
        dir.map(|dir| dir.join(format!("outbox-client-{client}.jsonl")))
    }

    /// Add a report at the end, and to the file before returning.
    pub async fn push(&self, queued: Queued) -> Result<(), OutboxError> {
        let mut state = self.loaded().await;
        if state.entries.len() >= self.cap {
            return Err(OutboxError::Full { cap: self.cap });
        }
        if let Some(path) = &self.path {
            append(path, &queued)
                .await
                .map_err(|source| OutboxError::Write {
                    path: path.clone(),
                    source,
                })?;
        }
        state.entries.push(queued);
        Ok(())
    }

    /// Every waiting report, oldest first.
    pub async fn entries(&self) -> Vec<Queued> {
        self.loaded().await.entries.clone()
    }

    pub async fn len(&self) -> usize {
        self.loaded().await.entries.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }

    /// Take out the reports that reached the cloud, and rewrite the file with the rest.
    pub async fn remove(&self, delivered: &BTreeSet<RequestId>) -> Result<(), OutboxError> {
        let mut state = self.loaded().await;
        state.entries.retain(|q| !delivered.contains(&q.id));
        match &self.path {
            Some(path) => {
                rewrite(path, &state.entries)
                    .await
                    .map_err(|source| OutboxError::Write {
                        path: path.clone(),
                        source,
                    })
            }
            None => Ok(()),
        }
    }

    /// The state, read from the file the first time. A line that does not parse is skipped with a
    /// warning: one torn write must not lose the rest.
    async fn loaded(&self) -> tokio::sync::MutexGuard<'_, State> {
        let mut state = self.state.lock().await;
        if !state.loaded {
            state.loaded = true;
            if let Some(path) = &self.path {
                state.entries = read(path).await;
            }
        }
        state
    }
}

async fn read(path: &Path) -> Vec<Queued> {
    let text = match tokio::fs::read_to_string(path).await {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            tracing::warn!(path = %path.display(), "cannot read the outbox, starting empty: {e}");
            return Vec::new();
        }
    };
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| match serde_json::from_str(line) {
            Ok(queued) => Some(queued),
            Err(e) => {
                tracing::warn!(path = %path.display(), "skipping a damaged outbox line: {e}");
                None
            }
        })
        .collect()
}

fn line(queued: &Queued) -> String {
    let mut line = serde_json::to_string(queued).expect("a queued report always serializes");
    line.push('\n');
    line
}

async fn append(path: &Path, queued: &Queued) -> std::io::Result<()> {
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(line(queued).as_bytes()).await?;
    file.flush().await
}

/// Written to a temporary file and renamed over the old one, so a crash leaves one or the other.
async fn rewrite(path: &Path, entries: &[Queued]) -> std::io::Result<()> {
    let text: String = entries.iter().map(line).collect();
    let tmp = path.with_extension("jsonl.tmp");
    tokio::fs::write(&tmp, text).await?;
    tokio::fs::rename(&tmp, path).await
}

// ---------------------------------------------------------------------------------------------
// Store-and-forward for peer updates (D407–D411)
// ---------------------------------------------------------------------------------------------

/// Updates held for one peer address at most; past it the oldest is dropped (D408).
pub const HOLD_PER_PEER: usize = 100;
/// Updates held in all at most; past it the oldest of all is dropped (D408).
pub const HOLD_TOTAL: usize = 10_000;
/// How long an update is held, from when its origin made it: the time a receiver remembers update
/// IDs (`p2p::SEEN_FOR`), so a resend can never be taken twice (D408).
pub const HOLD_FOR_MS: u64 = 300_000;

/// What became of an update handed to `MeshClient::send_peer_or_hold` (D407).
#[derive(Debug, Clone)]
pub enum PeerSend {
    /// It got through now.
    Delivered(Delivered),
    /// This program or the peer is away; it is held and will be tried again.
    Held,
}

/// Why a held update was let go without being delivered (D408, D409).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dropped {
    /// Held for [`HOLD_FOR_MS`].
    Age,
    /// Pushed out by a newer update past [`HOLD_PER_PEER`] or [`HOLD_TOTAL`].
    Limit,
    /// A newer version of the same subject came, or the offer it carries has run out.
    Obsolete,
    /// The peer refused it for good (for example not addressed to a user there), or the address is
    /// unusable.
    Refused,
}

impl Dropped {
    pub fn as_str(self) -> &'static str {
        match self {
            Dropped::Age => "age",
            Dropped::Limit => "limit",
            Dropped::Obsolete => "obsolete",
            Dropped::Refused => "refused",
        }
    }
}

/// One update let go, and for which peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldDrop {
    pub peer: String,
    pub update: UpdateId,
    pub reason: Dropped,
}

/// What an update is about, so that a newer version replaces an older one (D409).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Subject {
    Incident(RequestId),
    Offer(UserId, Capability),
}

fn subject(envelope: &P2pEnvelope) -> Subject {
    match &envelope.body {
        P2pBody::IncidentSummary(s) => Subject::Incident(s.incident),
        P2pBody::AssignmentUpdate(a) => Subject::Incident(a.incident),
        P2pBody::ResourceOffer(o) => Subject::Offer(o.provider, o.capability),
    }
}

/// Same subject, and the same kind, so an assignment never replaces a summary of its incident.
fn same_subject(a: &P2pEnvelope, b: &P2pEnvelope) -> bool {
    std::mem::discriminant(&a.body) == std::mem::discriminant(&b.body)
        && subject(a) == subject(b)
        && a.to == b.to
}

/// True once an update is past use whatever else is held: an offer whose time has run out.
fn expired_offer(envelope: &P2pEnvelope, now_ms: u64) -> bool {
    matches!(&envelope.body, P2pBody::ResourceOffer(o) if o.available_until_ms <= now_ms)
}

/// Peer updates held by their sender until the peer can take them, oldest first per peer. Pure: the
/// time is passed in (D407).
#[derive(Debug, Default)]
pub struct HeldUpdates {
    by_peer: BTreeMap<String, Vec<P2pEnvelope>>,
    total: usize,
}

impl HeldUpdates {
    pub fn len(&self) -> usize {
        self.total
    }

    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// Hold `envelope` for `peer`. Returns what was let go: possibly the new update itself, if it is
    /// already obsolete.
    pub fn hold(&mut self, peer: &str, envelope: P2pEnvelope, now_ms: u64) -> Vec<HeldDrop> {
        let mut dropped = self.expire(now_ms);
        let gone = |reason| HeldDrop {
            peer: peer.to_string(),
            update: envelope.id,
            reason,
        };
        if expired_offer(&envelope, now_ms) || held_too_long(&envelope, now_ms) {
            let reason = if expired_offer(&envelope, now_ms) {
                Dropped::Obsolete
            } else {
                Dropped::Age
            };
            dropped.push(gone(reason));
            return dropped;
        }
        let held = self.by_peer.entry(peer.to_string()).or_default();
        if let Some(at) = held.iter().position(|h| same_subject(h, &envelope)) {
            if held[at].version >= envelope.version {
                dropped.push(gone(Dropped::Obsolete));
                return dropped;
            }
            let old = held.remove(at);
            self.total -= 1;
            dropped.push(HeldDrop {
                peer: peer.to_string(),
                update: old.id,
                reason: Dropped::Obsolete,
            });
        }
        held.push(envelope);
        self.total += 1;
        if held.len() > HOLD_PER_PEER {
            let old = held.remove(0);
            self.total -= 1;
            dropped.push(HeldDrop {
                peer: peer.to_string(),
                update: old.id,
                reason: Dropped::Limit,
            });
        }
        while self.total > HOLD_TOTAL {
            match self.drop_oldest() {
                Some(drop) => dropped.push(drop),
                None => break,
            }
        }
        dropped
    }

    /// Let go of everything held too long or obsolete by now.
    pub fn expire(&mut self, now_ms: u64) -> Vec<HeldDrop> {
        let mut dropped = Vec::new();
        for (peer, held) in &mut self.by_peer {
            held.retain(|envelope| {
                let reason = if expired_offer(envelope, now_ms) {
                    Some(Dropped::Obsolete)
                } else if held_too_long(envelope, now_ms) {
                    Some(Dropped::Age)
                } else {
                    None
                };
                if let Some(reason) = reason {
                    dropped.push(HeldDrop {
                        peer: peer.clone(),
                        update: envelope.id,
                        reason,
                    });
                }
                reason.is_none()
            });
        }
        self.total -= dropped.len();
        self.by_peer.retain(|_, held| !held.is_empty());
        dropped
    }

    /// Every peer with something held, in address order.
    pub fn peers(&self) -> Vec<String> {
        self.by_peer.keys().cloned().collect()
    }

    /// What is held for `peer`, oldest first.
    pub fn held_for(&self, peer: &str) -> Vec<P2pEnvelope> {
        self.by_peer.get(peer).cloned().unwrap_or_default()
    }

    /// Take one update out, once it was delivered or let go. False if it is no longer held.
    pub fn remove(&mut self, peer: &str, update: UpdateId) -> bool {
        let Some(held) = self.by_peer.get_mut(peer) else {
            return false;
        };
        let Some(at) = held.iter().position(|h| h.id == update) else {
            return false;
        };
        held.remove(at);
        self.total -= 1;
        if held.is_empty() {
            self.by_peer.remove(peer);
        }
        true
    }

    /// The oldest update of all, by when it was made; ties to the lower peer address.
    fn drop_oldest(&mut self) -> Option<HeldDrop> {
        let peer = self
            .by_peer
            .iter()
            .filter_map(|(peer, held)| held.first().map(|h| (h.created_at_ms, peer.clone())))
            .min()?
            .1;
        let held = self.by_peer.get_mut(&peer)?;
        let old = held.remove(0);
        self.total -= 1;
        if held.is_empty() {
            self.by_peer.remove(&peer);
        }
        Some(HeldDrop {
            peer,
            update: old.id,
            reason: Dropped::Limit,
        })
    }
}

fn held_too_long(envelope: &P2pEnvelope, now_ms: u64) -> bool {
    now_ms.saturating_sub(envelope.created_at_ms) >= HOLD_FOR_MS
}

pub(crate) fn log_drops(log: &EventLog, drops: Vec<HeldDrop>) {
    for drop in drops {
        log.log(Event::PeerUpdateDropped {
            update: drop.update,
            to_addr: drop.peer,
            reason: drop.reason.as_str(),
        });
    }
}

/// A program's held updates, and the lock that keeps two redelivery rounds (the timer's and
/// `go_online`'s) from sending the same update at once.
#[derive(Default)]
pub(crate) struct Forwarder {
    pub(crate) held: Mutex<HeldUpdates>,
    round: tokio::sync::Mutex<()>,
}

impl Forwarder {
    /// Hold `envelope` for `peer` and log what happened to it and to anything it pushed out.
    pub(crate) fn hold(&self, peer: &str, envelope: P2pEnvelope, log: &EventLog) {
        let update = envelope.id;
        let (drops, held) = {
            let mut held = lock(&self.held);
            let drops = held.hold(peer, envelope, now_ms());
            (drops, held.len())
        };
        if !drops.iter().any(|d| d.update == update) {
            log.log(Event::PeerUpdateHeld {
                update,
                to_addr: peer.to_string(),
                held,
            });
        }
        log_drops(log, drops);
    }
}

/// Every `every`, while this program is online and holds something, run one redelivery round (D410).
/// Ends when its client drops it.
pub(crate) async fn redeliver_every(
    peer_link: std::sync::Arc<PeerLink>,
    forwarder: std::sync::Arc<Forwarder>,
    gate: crate::node::transport::FaultGate,
    log: EventLog,
    every: Duration,
) {
    let mut tick = tokio::time::interval(every);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        if gate.is_silenced() || lock(&forwarder.held).is_empty() {
            continue;
        }
        redeliver(&peer_link, &forwarder, &log).await;
    }
}

/// One round of redelivery (D410): each peer's held updates, oldest first. A peer that still does
/// not answer, or answers `busy`, is left for the next round after its first failure, so a peer that
/// is away costs one update's tries per round, not all of them. Returns how many got through.
pub(crate) async fn redeliver(
    peer_link: &PeerLink,
    forwarder: &Forwarder,
    log: &EventLog,
) -> usize {
    let _round = forwarder.round.lock().await;
    let held = &forwarder.held;
    let peers = {
        let mut held = lock(held);
        let drops = held.expire(now_ms());
        log_drops(log, drops);
        held.peers()
    };
    let mut delivered = 0;
    for peer in peers {
        let updates = lock(held).held_for(&peer);
        for envelope in updates {
            let update = envelope.id;
            let made = envelope.created_at_ms;
            match peer_link.send_peer(&peer, envelope).await {
                Ok(sent) => {
                    let keep = matches!(&sent.ack, P2pAck::Refused(reason) if reason == "busy");
                    if keep {
                        break;
                    }
                    lock(held).remove(&peer, update);
                    match sent.ack {
                        P2pAck::Accepted | P2pAck::Duplicate => {
                            delivered += 1;
                            log.log(Event::PeerUpdateForwarded {
                                update,
                                to_addr: peer.clone(),
                                ack: if sent.ack == P2pAck::Accepted {
                                    "accepted"
                                } else {
                                    "duplicate"
                                },
                                since_made_ms: now_ms().saturating_sub(made),
                            });
                        }
                        P2pAck::Refused(_) => log_drops(
                            log,
                            vec![HeldDrop {
                                peer: peer.clone(),
                                update,
                                reason: Dropped::Refused,
                            }],
                        ),
                    }
                }
                // This program went offline again: nothing more can go this round.
                Err(PeerError::Offline) => return delivered,
                // The peer is still away: try it again next round.
                Err(PeerError::GaveUp { .. }) => break,
                Err(PeerError::NoEndpoint | PeerError::BadAddress(_) | PeerError::Socket(_)) => {
                    lock(held).remove(&peer, update);
                    log_drops(
                        log,
                        vec![HeldDrop {
                            peer: peer.clone(),
                            update,
                            reason: Dropped::Refused,
                        }],
                    );
                }
            }
        }
    }
    delivered
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{
        AssignmentUpdate, Category, IncidentSummary, Point, ResourceId, ResourceOffer,
    };

    fn queued(seq: u64) -> Queued {
        Queued {
            id: RequestId { client: 7, seq },
            incident: Incident {
                owner: UserId { client: 7, user: 0 },
                category: Category::MedicalAssistance,
                zone: 1,
                location: Point { x: 3, y: 4 },
                severity: 3,
                needs: Capability::Medical,
                reported_at_ms: 1_000 + seq,
                ttl_ms: 60_000,
            },
            queued_at_ms: 2_000 + seq,
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rescuemesh-outbox-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join(name);
        let _ = std::fs::remove_file(&path);
        path
    }

    #[tokio::test]
    async fn a_full_outbox_refuses_and_keeps_what_it_has() {
        let outbox = Outbox::with_cap(None, 2);
        outbox.push(queued(1)).await.expect("room");
        outbox.push(queued(2)).await.expect("room");
        assert!(matches!(
            outbox.push(queued(3)).await,
            Err(OutboxError::Full { cap: 2 })
        ));
        assert_eq!(outbox.entries().await, vec![queued(1), queued(2)]);
    }

    #[tokio::test]
    async fn the_file_survives_a_restart_and_keeps_the_order() {
        let path = temp_path("restart.jsonl");
        let first = Outbox::new(Some(path.clone()));
        for seq in 1..=3 {
            first.push(queued(seq)).await.expect("written");
        }
        drop(first);

        let second = Outbox::new(Some(path.clone()));
        assert_eq!(
            second.entries().await,
            vec![queued(1), queued(2), queued(3)]
        );
    }

    #[tokio::test]
    async fn removing_delivered_reports_rewrites_the_file() {
        let path = temp_path("remove.jsonl");
        let outbox = Outbox::new(Some(path.clone()));
        for seq in 1..=3 {
            outbox.push(queued(seq)).await.expect("written");
        }
        let delivered = BTreeSet::from([queued(1).id, queued(3).id]);
        outbox.remove(&delivered).await.expect("rewritten");
        assert_eq!(outbox.entries().await, vec![queued(2)]);

        let reread = Outbox::new(Some(path));
        assert_eq!(reread.entries().await, vec![queued(2)]);
    }

    #[tokio::test]
    async fn a_damaged_line_is_skipped_and_the_rest_kept() {
        let path = temp_path("damaged.jsonl");
        let good = line(&queued(1)) + "{not json\n" + &line(&queued(2));
        std::fs::write(&path, good).expect("written");
        let outbox = Outbox::new(Some(path));
        assert_eq!(outbox.entries().await, vec![queued(1), queued(2)]);
    }

    #[test]
    fn the_file_sits_next_to_the_event_logs() {
        assert_eq!(
            Outbox::path_for(Some(Path::new("runs/a")), 4),
            Some(PathBuf::from("runs/a/outbox-client-4.jsonl"))
        );
        assert_eq!(Outbox::path_for(None, 4), None);
    }

    // --- store-and-forward ---

    const T0: u64 = 1_000_000;

    fn update(seq: u64, version: u64, body: P2pBody) -> P2pEnvelope {
        P2pEnvelope {
            id: UpdateId { origin: 7, seq },
            from: UserId { client: 7, user: 0 },
            to: Some(UserId { client: 8, user: 0 }),
            zone: 1,
            version,
            created_at_ms: T0 + seq,
            hops: 0,
            max_hops: 4,
            body,
        }
    }

    fn summary(incident: u64) -> P2pBody {
        P2pBody::IncidentSummary(IncidentSummary {
            incident: RequestId {
                client: 7,
                seq: incident,
            },
            category: Category::MedicalAssistance,
            severity: 3,
            zone: 1,
            location: Point { x: 0, y: 0 },
            needs: Capability::Medical,
            reported_at_ms: T0,
        })
    }

    fn offer(until_ms: u64) -> P2pBody {
        P2pBody::ResourceOffer(ResourceOffer {
            id: ResourceId {
                provider: 7,
                seq: 0,
            },
            provider: UserId { client: 7, user: 0 },
            capability: Capability::Medical,
            zone: 1,
            location: Point { x: 0, y: 0 },
            capacity: 1,
            available_until_ms: until_ms,
        })
    }

    fn ids(held: &HeldUpdates, peer: &str) -> Vec<u64> {
        held.held_for(peer).iter().map(|e| e.id.seq).collect()
    }

    #[test]
    fn a_newer_version_replaces_an_older_one_and_an_older_one_is_obsolete() {
        let mut held = HeldUpdates::default();
        assert!(held.hold("p", update(1, 1, summary(5)), T0).is_empty());
        let dropped = held.hold("p", update(2, 2, summary(5)), T0);
        assert_eq!(dropped.len(), 1);
        assert_eq!(
            (dropped[0].update.seq, dropped[0].reason),
            (1, Dropped::Obsolete)
        );
        let dropped = held.hold("p", update(3, 1, summary(5)), T0);
        assert_eq!(
            (dropped[0].update.seq, dropped[0].reason),
            (3, Dropped::Obsolete)
        );
        assert_eq!(ids(&held, "p"), vec![2]);
        // Another incident, or the same incident's assignment, is another subject.
        held.hold("p", update(4, 1, summary(6)), T0);
        let assignment = P2pBody::AssignmentUpdate(AssignmentUpdate {
            incident: RequestId { client: 7, seq: 5 },
            responder: None,
            priority: 1,
            assigned_at_ms: T0,
        });
        held.hold("p", update(5, 1, assignment), T0);
        assert_eq!(ids(&held, "p"), vec![2, 4, 5]);
        assert_eq!(held.len(), 3);
    }

    #[test]
    fn each_peer_keeps_its_newest_hold_per_peer_updates() {
        let mut held = HeldUpdates::default();
        for seq in 0..HOLD_PER_PEER as u64 + 2 {
            held.hold("p", update(seq, 1, summary(seq)), T0);
        }
        held.hold("q", update(999, 1, summary(999)), T0);
        assert_eq!(held.held_for("p").len(), HOLD_PER_PEER);
        assert_eq!(ids(&held, "p")[0], 2, "the two oldest went first");
        assert_eq!(held.len(), HOLD_PER_PEER + 1);
    }

    #[test]
    fn past_the_total_the_oldest_of_all_goes() {
        let mut held = HeldUpdates::default();
        let mut seq = 0;
        for peer in 0..=HOLD_TOTAL / HOLD_PER_PEER {
            for _ in 0..HOLD_PER_PEER {
                held.hold(&format!("p{peer:04}"), update(seq, 1, summary(seq)), T0);
                seq += 1;
            }
        }
        assert_eq!(held.len(), HOLD_TOTAL);
        assert!(held.held_for("p0000").is_empty(), "the oldest peer's went");
    }

    #[test]
    fn updates_held_too_long_and_offers_run_out_are_let_go() {
        let mut held = HeldUpdates::default();
        held.hold("p", update(1, 1, summary(1)), T0);
        held.hold("p", update(2, 1, offer(T0 + 10_000)), T0);
        assert!(
            held.expire(T0 + 1 + HOLD_FOR_MS - 1).len() == 1,
            "the offer ran out first"
        );
        let dropped = held.expire(T0 + 1 + HOLD_FOR_MS);
        assert_eq!(
            (dropped[0].update.seq, dropped[0].reason),
            (1, Dropped::Age)
        );
        assert!(held.is_empty());
        assert!(held.peers().is_empty());
        // An offer already over is not held at all.
        let dropped = held.hold("p", update(3, 1, offer(T0)), T0);
        assert_eq!(dropped[0].reason, Dropped::Obsolete);
        assert!(held.is_empty());
    }

    #[test]
    fn removing_a_delivered_update_counts_down() {
        let mut held = HeldUpdates::default();
        held.hold("p", update(1, 1, summary(1)), T0);
        held.hold("p", update(2, 1, summary(2)), T0);
        assert!(held.remove("p", UpdateId { origin: 7, seq: 1 }));
        assert!(!held.remove("p", UpdateId { origin: 7, seq: 1 }));
        assert!(!held.remove("q", UpdateId { origin: 7, seq: 2 }));
        assert_eq!((held.len(), ids(&held, "p")), (1, vec![2]));
    }
}
