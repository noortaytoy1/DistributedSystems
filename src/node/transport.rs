//! TCP framing and the fault gate.
//!
//! Every connection carries length-delimited frames (4-byte big-endian length, then a body
//! encoded with `proto::encode`).
//!
//! - Peer port: request/reply. [`PeerClient`] keeps one connection per peer and matches replies
//!   to requests by `seq`, so many calls can be in flight at once.
//! - Client port: one-way frames in both directions; replies are matched by request ID.
//!
//! The [`FaultGate`] drops all traffic in and out of a silenced node, except `ctl` control
//! frames, so an operator can still query and restore a silenced node.
//!
//! Owner: C (faults and failover). The plumbing here was written with Part A because Raft cannot
//! run without it; fault injection itself lives in faults.rs. Only `node/` and `raft/network.rs`
//! open sockets.
// TODO(C): reconnect backoff for peers that stay down, and connection-level metrics if E3 needs
// them.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use futures::future::BoxFuture;
use futures::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use tokio_util::codec::{Framed, LengthDelimitedCodec};

use crate::config::ClusterConfig;
use crate::metrics::{MessageCounters, MsgKind};
use crate::proto::{
    self, ClientFrame, NodeId, PeerEnvelope, PeerFrame, PeerReply, PeerReplyFrame, ServerFrame,
};

/// Drops all traffic while set. Cheap to clone; clones share the flag.
// TODO(C): richer faults if experiments need them (one direction only, added delay, loss).
#[derive(Debug, Clone, Default)]
pub struct FaultGate {
    silenced: Arc<AtomicBool>,
}

impl FaultGate {
    pub fn is_silenced(&self) -> bool {
        self.silenced.load(Ordering::SeqCst)
    }

    pub fn silence(&self) {
        self.silenced.store(true, Ordering::SeqCst);
    }

    pub fn restore(&self) {
        self.silenced.store(false, Ordering::SeqCst);
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

/// Request/reply client for one peer. Connects lazily and reconnects after failures.
pub struct PeerClient {
    peer: NodeId,
    addr: String,
    gate: FaultGate,
    msgs: Arc<MessageCounters>,
    conn: tokio::sync::Mutex<Option<Arc<Conn>>>,
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
    pub fn new(peer: NodeId, addr: String, gate: FaultGate, msgs: Arc<MessageCounters>) -> Self {
        Self {
            peer,
            addr,
            gate,
            msgs,
            conn: tokio::sync::Mutex::new(None),
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
        let deadline = Instant::now() + timeout;
        let kind = MsgKind::of(&envelope.body);
        let conn = tokio::time::timeout_at(deadline, self.connection())
            .await
            .map_err(|_| TransportError::Timeout(timeout))??;

        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        let bytes = Bytes::from(proto::encode(&PeerFrame { seq, envelope })?);
        let (reply_tx, reply_rx) = oneshot::channel();
        lock(&conn.pending).insert(seq, reply_tx);
        if conn.tx.send(bytes).is_err() {
            lock(&conn.pending).remove(&seq);
            conn.closed.store(true, Ordering::SeqCst);
            return Err(TransportError::Closed);
        }
        self.msgs.sent(kind);

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

    async fn connection(&self) -> Result<Arc<Conn>, TransportError> {
        let mut slot = self.conn.lock().await;
        if let Some(conn) = slot.as_ref() {
            if !conn.closed.load(Ordering::SeqCst) {
                return Ok(conn.clone());
            }
        }
        let stream =
            TcpStream::connect(&self.addr)
                .await
                .map_err(|source| TransportError::Connect {
                    addr: self.addr.clone(),
                    source,
                })?;
        let conn = Arc::new(spawn_conn(stream));
        *slot = Some(conn.clone());
        Ok(conn)
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
        msgs: &Arc<MessageCounters>,
    ) -> Self {
        let peers = cluster
            .nodes
            .iter()
            .filter(|n| n.id != me)
            .map(|n| {
                let client = PeerClient::new(n.id, n.peer_addr.clone(), gate.clone(), msgs.clone());
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
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tokio::spawn(peer_connection(
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
            if gate.is_silenced() {
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
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tokio::spawn(client_connection(
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

    async fn echo_server(gate: FaultGate) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let handler: PeerHandler = Arc::new(|env: PeerEnvelope| {
            Box::pin(async move { PeerReply::StaleTerm { current: env.term } })
        });
        tokio::spawn(serve_peers(listener, gate, Arc::default(), handler));
        addr
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
        let client = Arc::new(PeerClient::new(
            2,
            addr,
            FaultGate::default(),
            Arc::default(),
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

    #[tokio::test]
    async fn silenced_receiver_times_out_then_recovers() {
        let server_gate = FaultGate::default();
        let addr = echo_server(server_gate.clone()).await;
        let client = PeerClient::new(2, addr, FaultGate::default(), Arc::default());

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
        let client = PeerClient::new(2, "127.0.0.1:9".into(), gate, Arc::default());
        let err = client
            .call(heartbeat(1), Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(matches!(err, TransportError::Silenced));
    }
}
