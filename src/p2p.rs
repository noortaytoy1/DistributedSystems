//! The P2P link: client programs talking to each other directly (Phase 2, Part H, D336–D339, D460–D465).
//!
//! Every client program listens on a peer endpoint of its own and serves [`P2pRpc`] there, over UDP
//! with tarpc like the rest (D312). After discovery, programs send each other updates directly: the
//! cloud carries none of it (D310), and `tests/p2p.rs` counts the cloud's bytes to show it.
//!
//! - **Receiving (D460, D461, D462).** An update addressed to a user of this program, or to a whole zone,
//!   goes to a bounded inbox the application or Part I's gossip reads. An update ID seen in the last
//!   300 s (at most 10,000 kept) is answered `Duplicate` and not handed on twice. A full inbox
//!   answers `Refused("busy")`, so the sender may try again.
//! - **Sending (D339, D463).** [`PeerLink::new_update`] gives an update a fresh ID;
//!   [`PeerLink::send_peer`] delivers it, up to `[p2p] tries` times with `try_ms` each and the same ID,
//!   and returns the ack with the round trip on this program's clock. One stub per peer address is
//!   kept, at most 256, least recently used dropped first.
//! - **Offline (D338).** The endpoint and every stub sit behind the program's offline gate (D402):
//!   while it is closed nothing is sent and every arriving message is dropped.
//!
//! Sockets are opened in `node/transport.rs`, never here (D311).
//!
//! Owner: H (client presence and P2P link).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tarpc::client::NewClient;
use tokio::sync::mpsc;
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::Instant;

use crate::config::{Config, P2pConfig};
use crate::metrics::{Event, EventLog, UdpCounters, now_us};
use crate::node::transport::{self, FaultGate};
use crate::proto::{
    ClientId, P2pAck, P2pBody, P2pEnvelope, P2pRpc, P2pRpcClient, UpdateId, UserId, ZoneId,
};

/// How many updates wait for the application before the endpoint answers `Refused("busy")` (D460).
pub const INBOX_CAPACITY: usize = 1_024;
/// Update IDs remembered for duplicate suppression, and for how long (D461). The same bounds as Part
/// I's seen-cache (Yahia's D342) and Part K's planned retention.
pub const SEEN_CAPACITY: usize = 10_000;
pub const SEEN_FOR: Duration = Duration::from_secs(300);
/// Peer stubs kept for reuse (D463).
pub const STUB_CAPACITY: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PeerError {
    /// This program is offline (Part K): nothing was sent.
    #[error("this program is offline")]
    Offline,
    /// This program has no peer endpoint: its port range was full, or the socket would not open.
    #[error("this program has no peer endpoint")]
    NoEndpoint,
    #[error("bad peer address {0:?}")]
    BadAddress(String),
    #[error("cannot open a socket to {0}")]
    Socket(String),
    /// No ack after every try.
    #[error("no ack after {tries} tries")]
    GaveUp { tries: u32 },
}

/// A peer's ack to one update, and how long it took on this program's clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delivered {
    pub ack: P2pAck,
    /// From the first try to the ack: the P2P delivery latency (D464).
    pub latency: Duration,
    pub tries: u32,
}

// ---------------------------------------------------------------------------------------------
// Receiving
// ---------------------------------------------------------------------------------------------

/// Update IDs seen lately, oldest first (D461).
#[derive(Default)]
struct Seen {
    at: BTreeMap<UpdateId, Instant>,
    order: VecDeque<(Instant, UpdateId)>,
}

impl Seen {
    fn forget_old(&mut self, now: Instant) {
        while let Some(&(at, id)) = self.order.front() {
            let too_many = self.order.len() > SEEN_CAPACITY;
            if !too_many && now.saturating_duration_since(at) < SEEN_FOR {
                break;
            }
            self.order.pop_front();
            if self.at.get(&id) == Some(&at) {
                self.at.remove(&id);
            }
        }
    }

    fn contains(&self, id: &UpdateId) -> bool {
        self.at.contains_key(id)
    }

    fn insert(&mut self, id: UpdateId, now: Instant) {
        self.at.insert(id, now);
        self.order.push_back((now, id));
        self.forget_old(now);
    }
}

/// What a program's peer endpoint answers with. Cheap to clone: tarpc clones it per call.
#[derive(Clone)]
pub(crate) struct Inbox {
    me: ClientId,
    seen: Arc<Mutex<Seen>>,
    updates: mpsc::Sender<P2pEnvelope>,
    log: EventLog,
}

impl Inbox {
    fn take(&self, envelope: P2pEnvelope, now: Instant) -> P2pAck {
        // Addressed to a user of another program (D462).
        if let Some(to) = envelope.to {
            if to.client != self.me {
                return P2pAck::Refused("not here".into());
            }
        }
        let mut seen = lock(&self.seen);
        seen.forget_old(now);
        if seen.contains(&envelope.id) {
            return P2pAck::Duplicate;
        }
        let id = envelope.id;
        match self.updates.try_send(envelope) {
            Ok(()) => {
                seen.insert(id, now);
                P2pAck::Accepted
            }
            // Not marked as seen, so a later try of the same update can still get in.
            Err(mpsc::error::TrySendError::Full(_)) => P2pAck::Refused("busy".into()),
            Err(mpsc::error::TrySendError::Closed(_)) => P2pAck::Refused("closed".into()),
        }
    }
}

impl P2pRpc for Inbox {
    async fn deliver(self, _: tarpc::context::Context, envelope: P2pEnvelope) -> P2pAck {
        let (update, from, hops) = (envelope.id, envelope.from, envelope.hops);
        let ack = self.take(envelope, Instant::now());
        self.log.log(Event::P2pReceived {
            update,
            from,
            hops,
            outcome: ack_name(&ack),
        });
        ack
    }
}

fn ack_name(ack: &P2pAck) -> &'static str {
    match ack {
        P2pAck::Accepted => "accepted",
        P2pAck::Duplicate => "duplicate",
        P2pAck::Refused(_) => "refused",
    }
}

// ---------------------------------------------------------------------------------------------
// Sending, and the link a `MeshClient` holds
// ---------------------------------------------------------------------------------------------

/// Peer stubs by address, the least recently used dropped first past [`STUB_CAPACITY`] (D463).
#[derive(Default)]
struct Stubs {
    by_addr: HashMap<SocketAddr, (P2pRpcClient, u64)>,
    uses: u64,
}

/// A program's P2P link: its endpoint, its inbox and its way to other programs.
pub(crate) struct PeerLink {
    me: ClientId,
    config: P2pConfig,
    gate: FaultGate,
    counters: Arc<UdpCounters>,
    log: EventLog,
    /// `ip:port` advertised in presence records, if the endpoint opened (D336, D337).
    addr: Option<String>,
    updates: Mutex<Option<mpsc::Receiver<P2pEnvelope>>>,
    next_seq: AtomicU64,
    stubs: Mutex<Stubs>,
    dispatches: Mutex<JoinSet<()>>,
    endpoint: Option<JoinHandle<()>>,
}

impl PeerLink {
    /// Open the endpoint and start serving it. Must run inside a tokio runtime. A port range with no
    /// free port leaves the program without an endpoint, which `join` and `peer_addr` then report.
    pub(crate) fn new(me: ClientId, config: &Config, gate: FaultGate, log: EventLog) -> Self {
        let p2p = config.p2p;
        let counters = Arc::new(UdpCounters::default());
        let (tx, rx) = mpsc::channel(INBOX_CAPACITY);
        let inbox = Inbox {
            me,
            seen: Arc::default(),
            updates: tx,
            log: log.clone(),
        };
        let toward = config
            .cluster
            .nodes
            .first()
            .map(|n| n.client_addr.as_str())
            .unwrap_or_default();
        let (addr, endpoint) = match transport::bind_peer_endpoint(p2p.port_min, p2p.port_max) {
            Ok(socket) => {
                let port = socket.local_addr().map(|a| a.port()).unwrap_or_default();
                let ip = transport::local_ip_toward(toward);
                match ip {
                    Some(ip) => {
                        let addr = SocketAddr::new(ip, port).to_string();
                        log.log(Event::P2pListening { addr: addr.clone() });
                        let task = tokio::spawn(transport::serve_p2p(
                            socket,
                            inbox,
                            counters.clone(),
                            gate.clone(),
                        ));
                        (Some(addr), Some(task))
                    }
                    None => {
                        tracing::warn!("no local address toward {toward:?}, so no peer endpoint");
                        (None, None)
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    "no free peer port in {}..={}: {e}",
                    p2p.port_min,
                    p2p.port_max
                );
                (None, None)
            }
        };
        Self {
            me,
            config: p2p,
            gate,
            counters,
            log,
            addr,
            updates: Mutex::new(Some(rx)),
            // From the current time, so a rerun with the same client ID never reuses an update ID,
            // as for request IDs (D113, D315).
            next_seq: AtomicU64::new(now_us()),
            stubs: Mutex::default(),
            dispatches: Mutex::default(),
            endpoint,
        }
    }

    pub(crate) fn addr(&self) -> Option<&str> {
        self.addr.as_deref()
    }

    /// The updates other programs delivered here, once: the application or Part I's gossip reads them.
    pub(crate) fn take_updates(&self) -> Option<mpsc::Receiver<P2pEnvelope>> {
        lock(&self.updates).take()
    }

    pub(crate) fn counters(&self) -> &Arc<UdpCounters> {
        &self.counters
    }

    /// A new update from `from`, with a fresh ID, `hops` 0 and the time now (D315, D318).
    pub(crate) fn new_update(
        &self,
        from: UserId,
        to: Option<UserId>,
        zone: ZoneId,
        version: u64,
        max_hops: u8,
        body: P2pBody,
    ) -> P2pEnvelope {
        P2pEnvelope {
            id: UpdateId {
                origin: self.me,
                seq: self.next_seq.fetch_add(1, Ordering::Relaxed),
            },
            from,
            to,
            zone,
            version,
            created_at_ms: now_us() / 1_000,
            hops: 0,
            max_hops,
            body,
        }
    }

    /// Deliver `envelope` to the program at `peer_addr`, trying up to `[p2p] tries` times with the
    /// same update ID, so a repeat is only `Duplicate` (D339).
    pub(crate) async fn send_peer(
        &self,
        peer_addr: &str,
        envelope: P2pEnvelope,
    ) -> Result<Delivered, PeerError> {
        if self.gate.is_silenced() {
            return Err(PeerError::Offline);
        }
        let peer: SocketAddr = peer_addr
            .parse()
            .map_err(|_| PeerError::BadAddress(peer_addr.into()))?;
        let stub = self.stub(peer)?;
        let started = Instant::now();
        let tries = self.config.tries.max(1);
        for attempt in 1..=tries {
            if self.gate.is_silenced() {
                return Err(PeerError::Offline);
            }
            let mut ctx = tarpc::context::current();
            ctx.deadline = std::time::Instant::now() + self.config.try_timeout();
            if let Ok(ack) = stub.deliver(ctx, envelope.clone()).await {
                let latency = started.elapsed();
                self.log.log(Event::P2pSent {
                    update: envelope.id,
                    to_addr: peer_addr.into(),
                    ack: ack_name(&ack),
                    tries: attempt,
                    latency_us: latency.as_micros() as u64,
                });
                return Ok(Delivered {
                    ack,
                    latency,
                    tries: attempt,
                });
            }
        }
        self.log.log(Event::P2pSent {
            update: envelope.id,
            to_addr: peer_addr.into(),
            ack: "none",
            tries,
            latency_us: started.elapsed().as_micros() as u64,
        });
        Err(PeerError::GaveUp { tries })
    }

    /// The stub for `peer`, made on first use; past [`STUB_CAPACITY`] the least recently used goes.
    fn stub(&self, peer: SocketAddr) -> Result<P2pRpcClient, PeerError> {
        let mut stubs = lock(&self.stubs);
        stubs.uses += 1;
        let uses = stubs.uses;
        if let Some((stub, used)) = stubs.by_addr.get_mut(&peer) {
            *used = uses;
            return Ok(stub.clone());
        }
        let NewClient { client, dispatch } =
            transport::p2p_client(peer, self.counters.clone(), self.gate.clone())
                .map_err(|e| PeerError::Socket(format!("{peer}: {e}")))?;
        {
            let mut dispatches = lock(&self.dispatches);
            while dispatches.try_join_next().is_some() {}
            dispatches.spawn(dispatch);
        }
        if stubs.by_addr.len() >= STUB_CAPACITY {
            if let Some(oldest) = stubs
                .by_addr
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(addr, _)| *addr)
            {
                // Dropping the last handle ends that stub's dispatch task.
                stubs.by_addr.remove(&oldest);
            }
        }
        stubs.by_addr.insert(peer, (client.clone(), uses));
        Ok(client)
    }
}

impl Drop for PeerLink {
    fn drop(&mut self) {
        if let Some(endpoint) = &self.endpoint {
            endpoint.abort();
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{Capability, Point, ResourceOffer};

    fn envelope(origin: ClientId, seq: u64, to: Option<UserId>) -> P2pEnvelope {
        P2pEnvelope {
            id: UpdateId { origin, seq },
            from: UserId {
                client: origin,
                user: 0,
            },
            to,
            zone: 1,
            version: 1,
            created_at_ms: 0,
            hops: 0,
            max_hops: 4,
            body: P2pBody::ResourceOffer(ResourceOffer {
                provider: UserId {
                    client: origin,
                    user: 0,
                },
                capability: Capability::Medical,
                zone: 1,
                location: Point { x: 0, y: 0 },
                available_until_ms: 0,
            }),
        }
    }

    fn inbox(me: ClientId, capacity: usize) -> (Inbox, mpsc::Receiver<P2pEnvelope>) {
        let (tx, rx) = mpsc::channel(capacity);
        let inbox = Inbox {
            me,
            seen: Arc::default(),
            updates: tx,
            log: EventLog::disabled("t"),
        };
        (inbox, rx)
    }

    #[test]
    fn a_repeat_is_a_duplicate_and_reaches_the_application_once() {
        let (inbox, mut rx) = inbox(7, 8);
        let now = Instant::now();
        assert_eq!(inbox.take(envelope(3, 1, None), now), P2pAck::Accepted);
        assert_eq!(inbox.take(envelope(3, 1, None), now), P2pAck::Duplicate);
        assert_eq!(inbox.take(envelope(3, 2, None), now), P2pAck::Accepted);
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn a_message_for_another_programs_user_is_refused() {
        let (inbox, _rx) = inbox(7, 8);
        let mine = Some(UserId { client: 7, user: 2 });
        let theirs = Some(UserId { client: 8, user: 2 });
        let now = Instant::now();
        assert_eq!(inbox.take(envelope(3, 1, mine), now), P2pAck::Accepted);
        assert!(matches!(
            inbox.take(envelope(3, 2, theirs), now),
            P2pAck::Refused(_)
        ));
    }

    #[test]
    fn a_full_inbox_refuses_and_a_later_try_still_gets_in() {
        let (inbox, mut rx) = inbox(7, 1);
        let now = Instant::now();
        assert_eq!(inbox.take(envelope(3, 1, None), now), P2pAck::Accepted);
        assert_eq!(
            inbox.take(envelope(3, 2, None), now),
            P2pAck::Refused("busy".into())
        );
        rx.try_recv().unwrap();
        assert_eq!(inbox.take(envelope(3, 2, None), now), P2pAck::Accepted);
    }

    #[test]
    fn seen_ids_are_forgotten_by_age_and_by_count() {
        let mut seen = Seen::default();
        let start = Instant::now();
        let id = UpdateId { origin: 1, seq: 0 };
        seen.insert(id, start);
        assert!(seen.contains(&id));
        seen.forget_old(start + SEEN_FOR);
        assert!(!seen.contains(&id));
        for seq in 0..=SEEN_CAPACITY as u64 {
            seen.insert(UpdateId { origin: 2, seq }, start);
        }
        assert!(!seen.contains(&UpdateId { origin: 2, seq: 0 }));
        assert!(seen.contains(&UpdateId {
            origin: 2,
            seq: SEEN_CAPACITY as u64
        }));
        assert_eq!(seen.at.len(), SEEN_CAPACITY);
    }
}
