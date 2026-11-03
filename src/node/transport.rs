//! Connections and the fault gate.
//!
//! Every connection carries length-delimited frames (4-byte big-endian length, then a body).
//!
//! - Peer port: remote invocation of [`PeerRpc`] through tarpc (D119), which matches replies to
//!   requests, so many calls can be in flight at once. [`PeerClient`] keeps one connection per
//!   peer. After a failed connect it fails calls to that peer fast for a short cooldown instead of
//!   reconnecting on every call (D89–D92). A connection whose calls have gone unanswered for
//!   [`STALE_CONNECTION`] is replaced, because a machine that lost power sends no close. Every
//!   call ends by its own deadline, even one tarpc has not managed to write yet (D150).
//! - Client port: one-way frames in both directions, encoded with `proto::encode`; replies are
//!   matched by request ID.
//!
//! The [`FaultGate`] drops all traffic in and out of a silenced node, except `ctl` control
//! frames, so an operator can still query and restore a silenced node, and the "ok" answering
//! the fault order that silenced it (D78).
//!
//! Owner: C (faults and failover). The plumbing here was written with Part A because Raft cannot
//! run without it; fault injection itself lives in faults.rs. Only `node/` and `raft/network.rs`
//! open sockets.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use futures::future::BoxFuture;
use futures::{SinkExt, StreamExt};
use tarpc::client::RpcError;
use tarpc::server::{BaseChannel, Channel};
use tarpc::tokio_serde::formats::Bincode;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;
use tokio::time::Instant;
use tokio_util::codec::{Framed, LengthDelimitedCodec};

use crate::config::ClusterConfig;
use crate::metrics::{Event, EventLog, MessageCounters, MsgKind};
use crate::proto::{
    self, Bid, ClientFrame, Command, NodeId, PeerBody, PeerEnvelope, PeerReply, PeerRpc,
    PeerRpcClient, RaftRpc, ServerFrame, Term,
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

/// Calls unanswered this long mean the connection is replaced. A silenced peer loses nothing by
/// it: its listener stays up, so the connect succeeds (D119).
const STALE_CONNECTION: Duration = Duration::from_secs(1);

/// tarpc's defaults (1000 in flight, 100 buffered) are too low for bids at thousands of users.
const MAX_IN_FLIGHT_CALLS: usize = 20_000;
const CALL_BUFFER: usize = 2_000;

struct Conn {
    client: PeerRpcClient,
    closed: AtomicBool,
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
    /// When a call on the current connection last got an answer, or the connection opened.
    last_answer: Mutex<Instant>,
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
            last_answer: Mutex::new(Instant::now()),
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

        self.log.msgs().sent(kind);
        // tarpc starts a call's timer only once it has written the call. Behind a full socket (a
        // machine that lost power stops taking data) the call would wait in its queue for ever,
        // and so would the group send and the heartbeat loop above it (D150). Dropping the call
        // here is safe: tarpc cancels it.
        let result = tokio::time::timeout_at(deadline, invoke(&conn.client, envelope, deadline))
            .await
            .unwrap_or(Err(RpcError::DeadlineExceeded));
        match result {
            // A reply that arrives after the node was silenced is dropped like any other traffic.
            Ok(_) if self.gate.is_silenced() => Err(TransportError::Silenced),
            Ok(reply) => {
                *lock(&self.last_answer) = Instant::now();
                Ok(reply)
            }
            Err(RpcError::DeadlineExceeded) => {
                if lock(&self.last_answer).elapsed() >= STALE_CONNECTION {
                    conn.closed.store(true, Ordering::SeqCst);
                }
                Err(TransportError::Timeout(timeout))
            }
            Err(e) => {
                tracing::debug!(peer = self.peer, "peer call failed: {e}");
                conn.closed.store(true, Ordering::SeqCst);
                Err(TransportError::Closed)
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
        *lock(&self.last_answer) = Instant::now();
        let conn = Arc::new(open_conn(stream));
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

fn open_conn(stream: TcpStream) -> Conn {
    let transport = tarpc::serde_transport::new(framed(stream), Bincode::default());
    let mut config = tarpc::client::Config::default();
    config.max_in_flight_requests = MAX_IN_FLIGHT_CALLS;
    config.pending_request_buffer = CALL_BUFFER;
    Conn {
        client: PeerRpcClient::new(config, transport).spawn(),
        closed: AtomicBool::new(false),
    }
}

/// Make the remote call that matches this message.
async fn invoke(
    client: &PeerRpcClient,
    envelope: PeerEnvelope,
    deadline: Instant,
) -> Result<PeerReply, RpcError> {
    let mut ctx = tarpc::context::current();
    ctx.deadline = deadline.into_std();
    let PeerEnvelope { from, term, body } = envelope;
    match body {
        PeerBody::Raft(RaftRpc::AppendEntries(rpc)) => {
            client.append_entries(ctx, from, term, rpc).await
        }
        PeerBody::Raft(RaftRpc::Vote(rpc)) => client.vote(ctx, from, term, rpc).await,
        PeerBody::Raft(RaftRpc::InstallSnapshot(rpc)) => {
            client.install_snapshot(ctx, from, term, rpc).await
        }
        PeerBody::Propose(command) => client.propose(ctx, from, term, command).await,
        PeerBody::ReadIndex => client.read_index(ctx, from, term).await,
        PeerBody::FaultOrder { ms } => client.fault_order(ctx, from, term, ms).await,
        PeerBody::Bid(bid) => client.bid(ctx, from, term, bid).await,
        PeerBody::Heartbeat => client.heartbeat(ctx, from, term).await,
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

    /// Group send: one copy of a message to every peer, sent at once, so a slow peer holds up
    /// nobody else. Best effort, with no ordering across senders: a peer that misses a copy is
    /// reported here and not retried (D126). Anything that must be agreed goes through Raft.
    pub async fn broadcast(
        &self,
        term: Term,
        timeout: Duration,
        body: impl Fn() -> PeerBody,
    ) -> Vec<(NodeId, Result<PeerReply, TransportError>)> {
        let calls = self.peers.values().map(|peer| {
            let envelope = PeerEnvelope {
                from: self.me,
                term,
                body: body(),
            };
            async move { (peer.peer(), peer.call(envelope, timeout).await) }
        });
        futures::future::join_all(calls).await
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
    let server = PeerServer {
        gate,
        msgs,
        handler,
    };
    // Owned here, so stopping the server also closes its connections.
    let mut connections = JoinSet::new();
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                while connections.try_join_next().is_some() {}
                connections.spawn(peer_connection(stream, server.clone()));
            }
            Err(e) => {
                tracing::warn!("peer accept failed: {e}");
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    }
}

async fn peer_connection(stream: TcpStream, server: PeerServer) {
    let transport = tarpc::serde_transport::new(framed(stream), Bincode::default());
    BaseChannel::with_defaults(transport)
        .execute(server.serve())
        .for_each(|call| async {
            // Ends when the call is answered or its deadline passes.
            tokio::spawn(call);
        })
        .await;
}

/// The server side of [`PeerRpc`]: each method wraps its arguments as a [`PeerEnvelope`] for the
/// node's handler.
#[derive(Clone)]
struct PeerServer {
    gate: FaultGate,
    msgs: Arc<MessageCounters>,
    handler: PeerHandler,
}

impl PeerServer {
    /// A silenced node never answers: the call stays open until the caller's deadline, which is
    /// what a machine that ignores the network looks like from outside (D22).
    async fn deliver(self, from: NodeId, term: Term, body: PeerBody) -> PeerReply {
        if self.gate.is_silenced() {
            return std::future::pending().await;
        }
        self.msgs.received(MsgKind::of(&body));
        // In its own task: tarpc drops a call at its deadline, and a Raft RPC must not be cut
        // off halfway through.
        let work = (self.handler)(PeerEnvelope { from, term, body });
        let Ok(reply) = tokio::spawn(work).await else {
            return std::future::pending().await;
        };
        // Obeying a fault order closes the gate; its "ok" is the victim's last word, so the
        // leader can tell an obeyed order from an unreachable victim (D78).
        let last_word = matches!(reply, PeerReply::FaultOrder(Ok(())));
        if self.gate.is_silenced() && !last_word {
            return std::future::pending().await;
        }
        reply
    }
}

impl PeerRpc for PeerServer {
    async fn append_entries(
        self,
        _: tarpc::context::Context,
        from: NodeId,
        term: Term,
        rpc: openraft::raft::AppendEntriesRequest<crate::raft::TypeConfig>,
    ) -> PeerReply {
        let body = PeerBody::Raft(RaftRpc::AppendEntries(rpc));
        self.deliver(from, term, body).await
    }

    async fn vote(
        self,
        _: tarpc::context::Context,
        from: NodeId,
        term: Term,
        rpc: openraft::raft::VoteRequest<NodeId>,
    ) -> PeerReply {
        self.deliver(from, term, PeerBody::Raft(RaftRpc::Vote(rpc)))
            .await
    }

    async fn install_snapshot(
        self,
        _: tarpc::context::Context,
        from: NodeId,
        term: Term,
        rpc: openraft::raft::InstallSnapshotRequest<crate::raft::TypeConfig>,
    ) -> PeerReply {
        let body = PeerBody::Raft(RaftRpc::InstallSnapshot(rpc));
        self.deliver(from, term, body).await
    }

    async fn propose(
        self,
        _: tarpc::context::Context,
        from: NodeId,
        term: Term,
        command: Command,
    ) -> PeerReply {
        self.deliver(from, term, PeerBody::Propose(command)).await
    }

    async fn read_index(self, _: tarpc::context::Context, from: NodeId, term: Term) -> PeerReply {
        self.deliver(from, term, PeerBody::ReadIndex).await
    }

    async fn fault_order(
        self,
        _: tarpc::context::Context,
        from: NodeId,
        term: Term,
        ms: u64,
    ) -> PeerReply {
        self.deliver(from, term, PeerBody::FaultOrder { ms }).await
    }

    async fn bid(
        self,
        _: tarpc::context::Context,
        from: NodeId,
        term: Term,
        bid: Bid,
    ) -> PeerReply {
        self.deliver(from, term, PeerBody::Bid(bid)).await
    }

    async fn heartbeat(self, _: tarpc::context::Context, from: NodeId, term: Term) -> PeerReply {
        self.deliver(from, term, PeerBody::Heartbeat).await
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
    use std::sync::atomic::AtomicUsize;

    use super::*;
    use crate::config::NodeAddr;
    use crate::proto::{Capability, PeerBody, Point, Responder};

    /// About 2 MiB per call on the wire, at about 9 bytes per responder.
    const FLOOD_RESPONDERS: u32 = 250_000;
    /// Far more than a loopback socket holds (on Ubuntu, up to 4 MiB to send and 6 MiB to receive),
    /// so the last of these calls cannot be written.
    const FLOOD_CALLS: usize = 12;
    /// A call still out after this has hung: nothing is timing it.
    const HUNG: Duration = Duration::from_secs(10);
    /// How far past its deadline a call may return on a busy machine.
    const SLACK: Duration = Duration::from_secs(2);

    /// Accepts connections and never reads them, so the socket fills up and stays full. From this
    /// side that is what a machine that lost power looks like: no reply, no close, no reset.
    async fn never_reading_server() -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let accepted = Arc::new(AtomicUsize::new(0));
        let count = accepted.clone();
        tokio::spawn(async move {
            // Kept open: a closed socket would fail the calls instead of stalling them.
            let mut open = Vec::new();
            while let Ok((stream, _)) = listener.accept().await {
                count.fetch_add(1, Ordering::SeqCst);
                open.push(stream);
            }
        });
        (addr, accepted)
    }

    fn big_propose() -> PeerEnvelope {
        let responders = (0..FLOOD_RESPONDERS)
            .map(|i| Responder {
                id: u32::MAX - i,
                zone: 0,
                location: Point { x: 0, y: 0 },
                capability: Capability::Medical,
            })
            .collect();
        PeerEnvelope {
            from: 1,
            term: 1,
            body: PeerBody::Propose(Command::RegisterResponders { responders }),
        }
    }

    fn proposes_sent(log: &EventLog) -> u64 {
        match log.msgs().snapshot() {
            Event::Messages { sent, .. } => sent["propose"],
            _ => 0,
        }
    }

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

    /// tarpc times a call only once it has written it. Behind a full socket a call waits in
    /// tarpc's queue with no timer at all, so without a deadline of our own it never returns, and
    /// a heartbeat loop waiting on it stops for good (D150).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_call_to_a_peer_that_never_reads_returns_by_its_deadline() {
        let (addr, accepted) = never_reading_server().await;
        let client = Arc::new(peer_client(
            addr,
            FaultGate::default(),
            Duration::from_millis(100),
        ));
        let deadline = Duration::from_millis(300);

        let envelopes: Vec<_> = (0..FLOOD_CALLS).map(|_| big_propose()).collect();
        let calls = envelopes.into_iter().map(|envelope| {
            let client = client.clone();
            tokio::spawn(async move {
                let started = Instant::now();
                let result = client.call(envelope, deadline).await;
                (result, started.elapsed())
            })
        });
        let answers = tokio::time::timeout(HUNG, futures::future::join_all(calls))
            .await
            .expect("a call queued behind a full socket hung past its deadline");
        for answer in answers {
            let (result, took) = answer.unwrap();
            assert!(
                matches!(result, Err(TransportError::Timeout(_))),
                "{result:?}"
            );
            assert!(took < deadline + SLACK, "returned after {took:?}");
        }

        // Nothing has answered for over `STALE_CONNECTION` soon, so a timeout replaces the
        // connection. The stuck socket must not hold up the calls that do this.
        let until = Instant::now() + HUNG;
        while accepted.load(Ordering::SeqCst) < 2 {
            assert!(Instant::now() < until, "the connection was never replaced");
            let result =
                tokio::time::timeout(HUNG, client.call(heartbeat(1), Duration::from_millis(100)))
                    .await
                    .expect("a call hung past its deadline");
            assert!(
                matches!(result, Err(TransportError::Timeout(_))),
                "{result:?}"
            );
        }
    }

    /// The same stall through the group send: a peer that lost power must not hold up the answer
    /// about the healthy one, or suspicion and degraded mode stop with it (D150).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_broadcast_returns_with_one_stuck_peer_and_reports_the_healthy_one() {
        let healthy = echo_server(FaultGate::default()).await;
        let (stuck, _) = never_reading_server().await;
        let node = |id, peer_addr: String| NodeAddr {
            id,
            peer_addr,
            client_addr: "127.0.0.1:9".into(),
        };
        let cluster = ClusterConfig {
            nodes: vec![
                node(1, "127.0.0.1:9".into()),
                node(2, healthy),
                node(3, stuck),
            ],
        };
        let log = EventLog::disabled("test");
        let peers = PeerSet::new(
            1,
            &cluster,
            &FaultGate::default(),
            &log,
            Duration::from_millis(100),
        );

        // Fill node 3's socket and keep it full for the whole test.
        for envelope in (0..FLOOD_CALLS).map(|_| big_propose()) {
            let peer = peers.get(3).unwrap().clone();
            tokio::spawn(async move {
                let _ = peer.call(envelope, HUNG * 2).await;
            });
        }
        let until = Instant::now() + HUNG;
        while proposes_sent(&log) < FLOOD_CALLS as u64 {
            assert!(Instant::now() < until, "the flood never started");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        let deadline = Duration::from_millis(200);
        let started = Instant::now();
        let answers =
            tokio::time::timeout(HUNG, peers.broadcast(1, deadline, || PeerBody::Heartbeat))
                .await
                .expect("one stuck peer held up the whole group send");
        assert!(started.elapsed() < deadline + SLACK);
        assert!(
            matches!(answers[0], (2, Ok(PeerReply::StaleTerm { current: 1 }))),
            "{answers:?}"
        );
        assert!(
            matches!(answers[1], (3, Err(TransportError::Timeout(_)))),
            "{answers:?}"
        );
    }
}
