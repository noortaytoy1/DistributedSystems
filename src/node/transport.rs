//! TCP framing and the fault gate.
//!
//! Every connection carries length-delimited frames (4-byte big-endian length, then a body
//! encoded with `proto::encode`).
//!
//! - Peer port: request/reply. [`PeerClient`] keeps one connection per peer and matches replies
//!   to requests by `seq`, so many calls can be in flight at once. After a failed connect it
//!   fails calls to that peer fast for a short cooldown instead of reconnecting on every call
//!   (D89–D92).
//! - Client port: one-way frames in both directions; replies are matched by request ID.
//!
//! The [`FaultGate`] drops all traffic in and out of a silenced node, except `ctl` control
//! frames, so an operator can still query and restore a silenced node, and the "ok" answering
//! the fault order that silenced it (D78).
//!
//! Owner: C (faults and failover). The plumbing here was written with Part A because Raft cannot
//! run without it; fault injection itself lives in faults.rs. Only `node/` and `raft/network.rs`
//! open sockets.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use futures::future::BoxFuture;
use futures::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinSet;
use tokio::time::Instant;
use tokio_util::codec::{Framed, LengthDelimitedCodec};

use crate::config::ClusterConfig;
use crate::metrics::{Event, EventLog, MessageCounters, MsgKind};
use crate::proto::{
    self, ClientFrame, NodeId, PeerEnvelope, PeerFrame, PeerReply, PeerReplyFrame, ServerFrame,
};

/// Drops all traffic while set. Cheap to clone; clones share the flag.
///
/// The atomic is what every frame checks. The watch channel carries the same value to tasks that
/// must react when a fault starts or ends (the rejoin gate, D75).
// TODO(C): richer faults if experiments need them (one direction only, added delay, loss).
#[derive(Debug, Clone)]
pub struct FaultGate {
    silenced: Arc<AtomicBool>,
    changes: Arc<watch::Sender<bool>>,
}

impl Default for FaultGate {
    fn default() -> Self {
        Self {
            silenced: Arc::default(),
            changes: Arc::new(watch::channel(false).0),
        }
    }
}

impl FaultGate {
    pub fn is_silenced(&self) -> bool {
        self.silenced.load(Ordering::SeqCst)
    }

    pub fn silence(&self) {
        self.silenced.store(true, Ordering::SeqCst);
        self.changes.send_replace(true);
    }

    /// Close the gate only if it is open. False if the node is already silenced, so two faults
    /// that race each other cannot both start (D74).
    pub fn try_silence(&self) -> bool {
        let closed = self
            .silenced
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok();
        if closed {
            self.changes.send_replace(true);
        }
        closed
    }

    pub fn restore(&self) {
        self.silenced.store(false, Ordering::SeqCst);
        self.changes.send_replace(false);
    }

    /// Follow the gate: the value is true while silenced.
    pub fn subscribe(&self) -> watch::Receiver<bool> {
        self.changes.subscribe()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("this node is silenced")]
    Silenced,
    #[error("cannot connect to {addr}: {source}")]
    Connect {
        addr: String,
        source: std::io::Error,
    },
    #[error("no reply within {0:?}")]
    Timeout(Duration),
    #[error("connection closed")]
    Closed,
    #[error("unknown peer {0}")]
    UnknownPeer(NodeId),
    /// A recent connect to this peer failed; no new attempt until the cooldown ends (D91).
    #[error("peer {peer} unreachable, next connect in {retry_in:?}")]
    Backoff { peer: NodeId, retry_in: Duration },
    #[error("encoding: {0}")]
    Wire(#[from] proto::WireError),
}

pub fn framed(stream: TcpStream) -> Framed<TcpStream, LengthDelimitedCodec> {
    // Small messages dominate; Nagle would add up to 40 ms to each one.
    let _ = stream.set_nodelay(true);
    LengthDelimitedCodec::builder()
        .max_frame_length(proto::MAX_FRAME_BYTES)
        .new_framed(stream)
}

// ---------------------------------------------------------------------------------------------
// Peer client
// ---------------------------------------------------------------------------------------------

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<PeerReply>>>>;

struct Conn {
    tx: mpsc::UnboundedSender<Bytes>,
    pending: Pending,
    closed: Arc<AtomicBool>,
}

/// A peer whose last connect failed. Cleared by the next successful connect.
#[derive(Debug, Clone, Copy)]
struct Outage {
    since: Instant,
    retry_at: Instant,
}

/// Request/reply client for one peer. Connects lazily and reconnects after failures, at most once
/// per `reconnect_backoff` while the peer stays unreachable.
pub struct PeerClient {
    peer: NodeId,
    addr: String,
    gate: FaultGate,
    log: EventLog,
    reconnect_backoff: Duration,
    conn: tokio::sync::Mutex<Option<Arc<Conn>>>,
    outage: Mutex<Option<Outage>>,
    next_seq: AtomicU64,
}

impl std::fmt::Debug for PeerClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PeerClient")
            .field("peer", &self.peer)
            .field("addr", &self.addr)
            .finish()
    }
}

impl PeerClient {
    pub fn new(
        peer: NodeId,
        addr: String,
        gate: FaultGate,
        log: EventLog,
        reconnect_backoff: Duration,
    ) -> Self {
        Self {
            peer,
            addr,
            gate,
            log,
            reconnect_backoff,
            conn: tokio::sync::Mutex::new(None),
            outage: Mutex::new(None),
            next_seq: AtomicU64::new(1),
        }
    }

    pub fn peer(&self) -> NodeId {
        self.peer
    }

    /// Send one request and wait for its reply, all within `timeout`.
    pub async fn call(
        &self,
        envelope: PeerEnvelope,
        timeout: Duration,
    ) -> Result<PeerReply, TransportError> {
        if self.gate.is_silenced() {
            return Err(TransportError::Silenced);
        }
        // Checked before waiting on the connect lock, so a call never queues behind a hanging
        // connect to a peer already known to be down.
        self.check_backoff()?;
        let deadline = Instant::now() + timeout;
        let kind = MsgKind::of(&envelope.body);
        let conn = self.connection(deadline, timeout).await?;

        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        let bytes = Bytes::from(proto::encode(&PeerFrame { seq, envelope })?);
        let (reply_tx, reply_rx) = oneshot::channel();
        lock(&conn.pending).insert(seq, reply_tx);
        if conn.tx.send(bytes).is_err() {
            lock(&conn.pending).remove(&seq);
            conn.closed.store(true, Ordering::SeqCst);
            return Err(TransportError::Closed);
        }
        self.log.msgs().sent(kind);

        match tokio::time::timeout_at(deadline, reply_rx).await {
            // A reply that arrives after the node was silenced is dropped like any other traffic.
            Ok(Ok(_)) if self.gate.is_silenced() => Err(TransportError::Silenced),
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(_)) => Err(TransportError::Closed),
            Err(_) => {
                lock(&conn.pending).remove(&seq);
                Err(TransportError::Timeout(timeout))
            }
        }
    }

    async fn connection(
        &self,
        deadline: Instant,
        timeout: Duration,
    ) -> Result<Arc<Conn>, TransportError> {
        let mut slot = tokio::time::timeout_at(deadline, self.conn.lock())
            .await
            .map_err(|_| TransportError::Timeout(timeout))?;
        if let Some(conn) = slot.as_ref() {
            if !conn.closed.load(Ordering::SeqCst) {
                return Ok(conn.clone());
            }
        }
        // Calls that queued behind a connect that just failed fail fast too.
        self.check_backoff()?;
        let stream = match tokio::time::timeout_at(deadline, TcpStream::connect(&self.addr)).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(source)) => {
                self.connect_failed(&source.to_string());
                return Err(TransportError::Connect {
                    addr: self.addr.clone(),
                    source,
                });
            }
            // A connect that outlives the call (cable pulled on a real network) counts as a
            // failed connect too (D89).
            Err(_) => {
                self.connect_failed("connect timed out");
                return Err(TransportError::Timeout(timeout));
            }
        };
        self.connected();
        let conn = Arc::new(spawn_conn(stream));
        *slot = Some(conn.clone());
        Ok(conn)
    }

    /// Fails fast while the peer is in its reconnect cooldown.
    fn check_backoff(&self) -> Result<(), TransportError> {
        let now = Instant::now();
        match *lock(&self.outage) {
            Some(Outage { retry_at, .. }) if now < retry_at => Err(TransportError::Backoff {
                peer: self.peer,
                retry_in: retry_at - now,
            }),
            _ => Ok(()),
        }
    }

    /// Starts or extends the outage. Only its first failed connect is reported (D92).
    fn connect_failed(&self, error: &str) {
        let now = Instant::now();
        let first = {
            let mut outage = lock(&self.outage);
            let first = outage.is_none();
            let since = outage.map_or(now, |o| o.since);
            *outage = Some(Outage {
                since,
                retry_at: now + self.reconnect_backoff,
            });
            first
        };
        if first {
            tracing::warn!(peer = self.peer, addr = %self.addr, "peer unreachable: {error}");
            self.log.log(Event::PeerUnreachable {
                peer: self.peer,
                error: error.to_string(),
            });
        }
    }

    /// Ends the outage, if any, and reports how long it lasted on this node's clock (D92).
    fn connected(&self) {
        let Some(outage) = lock(&self.outage).take() else {
            return;
        };
        let down_ms = u64::try_from(outage.since.elapsed().as_millis()).unwrap_or(u64::MAX);
        tracing::info!(peer = self.peer, down_ms, "peer reachable again");
        self.log.log(Event::PeerReconnected {
            peer: self.peer,
            down_ms,
        });
    }
}

fn spawn_conn(stream: TcpStream) -> Conn {
    let (mut sink, mut stream) = framed(stream).split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Bytes>();
    let pending: Pending = Arc::default();
    let closed = Arc::new(AtomicBool::new(false));

    let writer_closed = closed.clone();
    tokio::spawn(async move {
        while let Some(bytes) = rx.recv().await {
            if sink.send(bytes).await.is_err() {
                break;
            }
        }
        writer_closed.store(true, Ordering::SeqCst);
    });

    let reader_pending = pending.clone();
    let reader_closed = closed.clone();
    tokio::spawn(async move {
        while let Some(Ok(frame)) = stream.next().await {
            match proto::decode::<PeerReplyFrame>(&frame) {
                Ok(PeerReplyFrame { seq, reply }) => {
                    if let Some(waiter) = lock(&reader_pending).remove(&seq) {
                        let _ = waiter.send(reply);
                    }
                }
                Err(e) => {
                    tracing::warn!("bad peer reply frame: {e}");
                    break;
                }
            }
        }
        reader_closed.store(true, Ordering::SeqCst);
        // Dropping the waiters makes in-flight calls fail fast with `Closed`.
        lock(&reader_pending).clear();
    });

    Conn {
        tx,
        pending,
        closed,
    }
}

/// Clients for every other node in the cluster.
#[derive(Debug)]
pub struct PeerSet {
    me: NodeId,
    peers: BTreeMap<NodeId, Arc<PeerClient>>,
}

impl PeerSet {
    pub fn new(
        me: NodeId,
        cluster: &ClusterConfig,
        gate: &FaultGate,
        log: &EventLog,
        reconnect_backoff: Duration,
    ) -> Self {
        let peers = cluster
            .nodes
            .iter()
            .filter(|n| n.id != me)
            .map(|n| {
                let client = PeerClient::new(
                    n.id,
                    n.peer_addr.clone(),
                    gate.clone(),
                    log.clone(),
                    reconnect_backoff,
                );
                (n.id, Arc::new(client))
            })
            .collect();
        Self { me, peers }
    }

    pub fn me(&self) -> NodeId {
        self.me
    }

    pub fn get(&self, id: NodeId) -> Option<&Arc<PeerClient>> {
        self.peers.get(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Arc<PeerClient>> {
        self.peers.values()
    }
}

// ---------------------------------------------------------------------------------------------
// Servers
// ---------------------------------------------------------------------------------------------

/// Handles one peer request. The returned future is spawned, so it must be `'static`.
pub type PeerHandler = Arc<dyn Fn(PeerEnvelope) -> BoxFuture<'static, PeerReply> + Send + Sync>;

/// Accept peer connections forever. Each request runs in its own task.
pub async fn serve_peers(
    listener: TcpListener,
    gate: FaultGate,
    msgs: Arc<MessageCounters>,
    handler: PeerHandler,
) {
    // Owned here, so stopping the server also closes its connections.
    let mut connections = JoinSet::new();
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                while connections.try_join_next().is_some() {}
                connections.spawn(peer_connection(
                    stream,
                    gate.clone(),
                    msgs.clone(),
                    handler.clone(),
                ));
            }
            Err(e) => {
                tracing::warn!("peer accept failed: {e}");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }
}

async fn peer_connection(
    stream: TcpStream,
    gate: FaultGate,
    msgs: Arc<MessageCounters>,
    handler: PeerHandler,
) {
    let (mut sink, mut frames) = framed(stream).split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Bytes>();
    tokio::spawn(async move {
        while let Some(bytes) = rx.recv().await {
            if sink.send(bytes).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(frame)) = frames.next().await {
        if gate.is_silenced() {
            continue;
        }
        let PeerFrame { seq, envelope } = match proto::decode(&frame) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("bad peer frame: {e}");
                return;
            }
        };
        msgs.received(MsgKind::of(&envelope.body));
        let (handler, gate, tx) = (handler.clone(), gate.clone(), tx.clone());
        tokio::spawn(async move {
            let reply = handler(envelope).await;
            // Obeying a fault order closes the gate; its "ok" is the victim's last word, so the
            // leader can tell an obeyed order from an unreachable victim (D78).
            let last_word = matches!(reply, PeerReply::FaultOrder(Ok(())));
            if gate.is_silenced() && !last_word {
                return;
            }
            match proto::encode(&PeerReplyFrame { seq, reply }) {
                Ok(bytes) => {
                    let _ = tx.send(Bytes::from(bytes));
                }
                Err(e) => tracing::warn!("cannot encode peer reply: {e}"),
            }
        });
    }
}

/// Sends frames back to one connected client. Cheap to clone.
#[derive(Clone)]
pub struct ReplyTx {
    tx: mpsc::UnboundedSender<Bytes>,
    gate: FaultGate,
    msgs: Arc<MessageCounters>,
}

impl ReplyTx {
    /// Queue a frame for the client. Request replies are dropped while silenced; control
    /// replies always go out. Returns false if the frame was dropped.
    pub fn send(&self, frame: &ServerFrame) -> bool {
        let is_reply = matches!(frame, ServerFrame::Reply(_));
        if is_reply && self.gate.is_silenced() {
            return false;
        }
        let Ok(bytes) = proto::encode(frame) else {
            return false;
        };
        let sent = self.tx.send(Bytes::from(bytes)).is_ok();
        if sent && is_reply {
            self.msgs.sent(MsgKind::ClientReply);
        }
        sent
    }
}

/// Handles one client frame. Must not block: spawn any real work.
pub type ClientHandler = Arc<dyn Fn(ClientFrame, ReplyTx) + Send + Sync>;

/// Accept client connections forever.
pub async fn serve_clients(
    listener: TcpListener,
    gate: FaultGate,
    msgs: Arc<MessageCounters>,
    handler: ClientHandler,
) {
    let mut connections = JoinSet::new();
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                while connections.try_join_next().is_some() {}
                connections.spawn(client_connection(
                    stream,
                    gate.clone(),
                    msgs.clone(),
                    handler.clone(),
                ));
            }
            Err(e) => {
                tracing::warn!("client accept failed: {e}");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }
}

async fn client_connection(
    stream: TcpStream,
    gate: FaultGate,
    msgs: Arc<MessageCounters>,
    handler: ClientHandler,
) {
    let (mut sink, mut frames) = framed(stream).split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Bytes>();
    tokio::spawn(async move {
        while let Some(bytes) = rx.recv().await {
            if sink.send(bytes).await.is_err() {
                break;
            }
        }
    });
    let reply_tx = ReplyTx {
        tx,
        gate: gate.clone(),
        msgs: msgs.clone(),
    };

    while let Some(Ok(frame)) = frames.next().await {
        let frame: ClientFrame = match proto::decode(&frame) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("bad client frame: {e}");
                return;
            }
        };
        if let ClientFrame::Request(_) = frame {
            if gate.is_silenced() {
                continue;
            }
            msgs.received(MsgKind::ClientRequest);
        }
        handler(frame, reply_tx.clone());
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::PeerBody;

    fn serve_echo(listener: TcpListener, gate: FaultGate) {
        let handler: PeerHandler = Arc::new(|env: PeerEnvelope| {
            Box::pin(async move { PeerReply::StaleTerm { current: env.term } })
        });
        tokio::spawn(serve_peers(listener, gate, Arc::default(), handler));
    }

    async fn echo_server(gate: FaultGate) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        serve_echo(listener, gate);
        addr
    }

    fn peer_client(addr: String, gate: FaultGate, reconnect_backoff: Duration) -> PeerClient {
        PeerClient::new(2, addr, gate, EventLog::disabled("test"), reconnect_backoff)
    }

    fn heartbeat(term: u64) -> PeerEnvelope {
        PeerEnvelope {
            from: 1,
            term,
            body: PeerBody::Heartbeat,
        }
    }

    #[tokio::test]
    async fn concurrent_calls_get_their_own_replies() {
        let addr = echo_server(FaultGate::default()).await;
        let client = Arc::new(peer_client(
            addr,
            FaultGate::default(),
            Duration::from_millis(100),
        ));
        let calls = (0..50u64).map(|term| {
            let client = client.clone();
            tokio::spawn(async move {
                let reply = client
                    .call(heartbeat(term), Duration::from_secs(2))
                    .await
                    .unwrap();
                assert!(matches!(reply, PeerReply::StaleTerm { current } if current == term));
            })
        });
        for call in calls {
            call.await.unwrap();
        }
    }

    /// Also guards D89's trigger: a timeout on an open connection must not start a cooldown, or
    /// the call right after `restore` would fail with `Backoff`.
    #[tokio::test]
    async fn silenced_receiver_times_out_then_recovers() {
        let server_gate = FaultGate::default();
        let addr = echo_server(server_gate.clone()).await;
        let client = peer_client(addr, FaultGate::default(), Duration::from_secs(10));

        server_gate.silence();
        let err = client
            .call(heartbeat(1), Duration::from_millis(100))
            .await
            .unwrap_err();
        assert!(matches!(err, TransportError::Timeout(_)));

        server_gate.restore();
        let reply = client
            .call(heartbeat(2), Duration::from_secs(2))
            .await
            .unwrap();
        assert!(matches!(reply, PeerReply::StaleTerm { current: 2 }));
    }

    #[tokio::test]
    async fn silenced_sender_fails_fast() {
        let gate = FaultGate::default();
        gate.silence();
        let client = peer_client("127.0.0.1:9".into(), gate, Duration::from_millis(100));
        let err = client
            .call(heartbeat(1), Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(matches!(err, TransportError::Silenced));
    }

    #[tokio::test]
    async fn refused_connect_backs_off_then_reconnects() {
        // Reserve a free port, then close it so connects to it are refused.
        let addr = TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap()
            .local_addr()
            .unwrap()
            .to_string();
        let client = peer_client(
            addr.clone(),
            FaultGate::default(),
            Duration::from_millis(200),
        );

        let err = client
            .call(heartbeat(1), Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(matches!(err, TransportError::Connect { .. }), "{err}");
        let err = client
            .call(heartbeat(1), Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(
            matches!(err, TransportError::Backoff { peer: 2, .. }),
            "{err}"
        );

        // The peer comes back: once the cooldown ends, the next call connects.
        serve_echo(
            TcpListener::bind(&addr).await.unwrap(),
            FaultGate::default(),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match client.call(heartbeat(2), Duration::from_secs(1)).await {
                Ok(reply) => {
                    assert!(matches!(reply, PeerReply::StaleTerm { current: 2 }));
                    break;
                }
                Err(TransportError::Backoff { .. }) if Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                Err(e) => panic!("peer did not come back: {e}"),
            }
        }
        assert!(lock(&client.outage).is_none(), "a connect ends the outage");
    }
}
