//! tarpc over UDP: one message is one datagram (D167, D170).
//!
//! tarpc keeps every job an RPC system has: the generated stubs, call IDs, matching replies,
//! deadlines and cancels. This file only moves one tarpc message in one packet, because tarpc's own
//! transport needs a byte stream and UDP has none.
//!
//! tarpc wants a `Stream` of incoming messages and a `Sink` of outgoing ones, and an error from
//! either ends the channel and every call on it. So the one rule here is: **no error ever reaches
//! tarpc**. A bad datagram, an ICMP error or a failed send counts as a lost message, and the client's
//! retry (D52) covers it exactly as it covers a packet lost on the wire.
//!
//! - **Read side.** `UdpFramed` with a decoder that takes the whole datagram.
//! - **Write side.** `try_send_to` directly. `UdpFramed`'s own `Sink` keeps a datagram whose send
//!   failed and appends the next message to it, so it would never recover (tests/datagram.rs).
//! - **Client end.** One unconnected socket per node. Unconnected, so an ICMP "port unreachable" from
//!   a dead node never comes back as a socket error (tests/datagram.rs).
//! - **Node end.** One socket, whose datagrams [`serve`] splits by source address into one tarpc
//!   channel per client program. UDP has no "close", so a program with no traffic for
//!   [`CHANNEL_IDLE`] loses its channel.
//!
//! Every task here is owned: [`serve`] holds its channels in a `JoinSet`, so aborting it (node
//! shutdown) ends them, and [`crate::node::transport::run_calls`] does the same for their calls.
//!
//! Owner: C (faults and failover). Only `node/` and `raft/network.rs` open sockets.

use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::marker::PhantomData;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::task::{Context, Poll, ready};
use std::time::{Duration, Instant};

use bytes::BytesMut;
use futures::{Sink, Stream, StreamExt};
use serde::Serialize;
use serde::de::DeserializeOwned;
use socket2::{Domain, Protocol, SockRef, Socket, Type};
use tarpc::{ClientMessage, Response};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_util::codec::Decoder;
use tokio_util::udp::UdpFramed;

use crate::metrics::{Event, EventLog, UdpCounters};
use crate::proto;

/// A client program with no traffic for this long loses its channel on the node. Longer than any
/// call at the default timers (36 s), so a program waiting on a reply keeps its channel.
pub const CHANNEL_IDLE: Duration = Duration::from_secs(60);

/// Messages waiting for one channel. More than this are dropped and counted: the reader must never
/// wait, or every other program's datagrams would pile up in the kernel behind it.
pub const CHANNEL_QUEUE: usize = 16 * 1024;

/// The receive buffer every socket asks for (D171). One copy from each of 3000 users at once needs
/// 2.7 MB at the 896 bytes the kernel charges per packet on loopback, and about 7 MB if a network
/// card's driver charges a whole 2 KB buffer. The kernel caps the request at `net.core.rmem_max`,
/// which is why the runbook raises that first.
pub const RECEIVE_BUFFER_BYTES: usize = 8 * 1024 * 1024;

/// Longest a call may run. tarpc's deadline queue panics past about two years, so a request asking
/// for more is treated as a bad datagram, and a client never asks for more whatever its timers say.
pub const MAX_CALL_DEADLINE: Duration = Duration::from_secs(365 * 24 * 3600);

/// A socket on `local` with the large receive buffer. It must be a socket2 socket because the
/// buffer has to be set before the bind to apply to datagrams arriving straight away, and the
/// standard library cannot set it at all.
pub fn open_socket(local: SocketAddr) -> io::Result<UdpSocket> {
    let socket = Socket::new(Domain::for_address(local), Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_nonblocking(true)?;
    socket.set_recv_buffer_size(RECEIVE_BUFFER_BYTES)?;
    socket.bind(&local.into())?;
    UdpSocket::from_std(socket.into())
}

/// Log the receive buffer the kernel really gave. It never refuses, it silently caps, so this line
/// is the only sign that a machine missed the sysctl step (D171).
pub fn report_buffer(socket: &UdpSocket, log: &EventLog) {
    let got = SockRef::from(socket).recv_buffer_size().unwrap_or(0);
    let local_addr = socket
        .local_addr()
        .map_or_else(|_| "?".to_string(), |addr| addr.to_string());
    log.log(Event::UdpSocket {
        local_addr: local_addr.clone(),
        rcvbuf_asked_bytes: RECEIVE_BUFFER_BYTES as u64,
        rcvbuf_got_bytes: got as u64,
    });
    if got < RECEIVE_BUFFER_BYTES {
        tracing::warn!(
            local_addr,
            got,
            asked = RECEIVE_BUFFER_BYTES,
            "UDP receive buffer capped by the kernel, so a burst can lose reports: run \
             sudo sysctl -w net.core.rmem_max={RECEIVE_BUFFER_BYTES}"
        );
    } else {
        tracing::info!(local_addr, got, "UDP receive buffer");
    }
}

/// Decodes one whole datagram into one message.
pub struct BincodeDatagram<T> {
    counters: Arc<UdpCounters>,
    _message: PhantomData<fn() -> T>,
}

impl<T> BincodeDatagram<T> {
    pub fn new(counters: Arc<UdpCounters>) -> Self {
        Self {
            counters,
            _message: PhantomData,
        }
    }
}

impl<T: DeserializeOwned> Decoder for BincodeDatagram<T> {
    type Item = T;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> io::Result<Option<T>> {
        if src.is_empty() {
            return Ok(None);
        }
        self.counters.received(src.len());
        // Takes the whole datagram, so an error leaves nothing behind for the next call.
        let datagram = src.split();
        // tarpc adds a request's deadline to the clock while decoding, which panics on a huge one.
        // A panic here would end the reader, and with it the node's whole client port, so one stray
        // packet counts as a bad datagram like any other.
        let decoded = catch_unwind(AssertUnwindSafe(|| proto::decode(&datagram)));
        match decoded {
            Ok(Ok(message)) => Ok(Some(message)),
            _ => Err(io::Error::from(io::ErrorKind::InvalidData)),
        }
    }
}

/// Send one message, or count why it was lost. Never reports an error and never waits.
fn send_datagram<T: Serialize>(
    socket: &UdpSocket,
    to: SocketAddr,
    message: &T,
    counters: &UdpCounters,
) {
    let bytes = match proto::encode(message) {
        Ok(bytes) if bytes.len() <= proto::MAX_DATAGRAM_BYTES => bytes,
        // A datagram this large might be split on the wire, and every fragment would have to arrive.
        // A test holds the largest report and reply under the limit, so this means a bug.
        encoded => {
            let size = encoded.map_or(0, |bytes| bytes.len());
            tracing::warn!(size, "message does not fit one datagram: dropped");
            counters.tx_encode_errors.fetch_add(1, Relaxed);
            return;
        }
    };
    match socket.try_send_to(&bytes, to) {
        Ok(n) => counters.sent(n),
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
            counters.tx_would_block.fetch_add(1, Relaxed);
        }
        Err(_) => {
            counters.tx_errors.fetch_add(1, Relaxed);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Client end
// ---------------------------------------------------------------------------------------------

/// A socket for talking to `node`. Not connected, so ICMP errors never come back as socket errors.
pub fn client_socket(node: SocketAddr) -> io::Result<UdpSocket> {
    let any: IpAddr = if node.is_ipv4() {
        Ipv4Addr::UNSPECIFIED.into()
    } else {
        Ipv6Addr::UNSPECIFIED.into()
    };
    open_socket(SocketAddr::new(any, 0))
}

/// The client's transport for one node: what tarpc's stub reads and writes.
pub struct ClientEnd<Req, Resp> {
    frames: UdpFramed<BincodeDatagram<Response<Resp>>, Arc<UdpSocket>>,
    socket: Arc<UdpSocket>,
    node: SocketAddr,
    counters: Arc<UdpCounters>,
    _requests: PhantomData<fn(Req)>,
}

impl<Req, Resp> ClientEnd<Req, Resp> {
    pub fn new(socket: UdpSocket, node: SocketAddr, counters: Arc<UdpCounters>) -> Self {
        let socket = Arc::new(socket);
        Self {
            frames: UdpFramed::new(socket.clone(), BincodeDatagram::new(counters.clone())),
            socket,
            node,
            counters,
            _requests: PhantomData,
        }
    }
}

impl<Req, Resp: DeserializeOwned> Stream for ClientEnd<Req, Resp> {
    type Item = io::Result<Response<Resp>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            match ready!(Pin::new(&mut this.frames).poll_next(cx)) {
                // An unconnected socket takes anyone's packets. Call IDs are small numbers, so a
                // stray one must not be matched to a call.
                Some(Ok((message, from))) if from == this.node => {
                    return Poll::Ready(Some(Ok(message)));
                }
                Some(Ok(_)) => {
                    this.counters.rx_foreign.fetch_add(1, Relaxed);
                }
                // Must not reach tarpc: any Err here ends the dispatch and every call on it.
                Some(Err(_)) => {
                    this.counters.rx_bad.fetch_add(1, Relaxed);
                }
                None => return Poll::Ready(None),
            }
        }
    }
}

impl<Req: Serialize, Resp> Sink<ClientMessage<Req>> for ClientEnd<Req, Resp> {
    type Error = io::Error;

    /// Waits for room in the kernel's send buffer, so a burst of users queues in tarpc and not on
    /// the floor. This socket has one reader and one writer, the stub's dispatch task, so the single
    /// waker tokio keeps for writing is always the right one.
    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let _ = ready!(self.socket.poll_send_ready(cx));
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: ClientMessage<Req>) -> io::Result<()> {
        match &item {
            ClientMessage::Request(_) => self.counters.tx_requests.fetch_add(1, Relaxed),
            ClientMessage::Cancel { .. } => self.counters.tx_cancels.fetch_add(1, Relaxed),
            _ => 0,
        };
        send_datagram(&self.socket, self.node, &item, &self.counters);
        Ok(())
    }

    /// tarpc loops on "not ready, so flush", so a flush that says ready while `poll_ready` says
    /// pending would spin that loop and never yield to the reactor.
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let _ = ready!(self.socket.poll_send_ready(cx));
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

// ---------------------------------------------------------------------------------------------
// Node end
// ---------------------------------------------------------------------------------------------

/// The node's transport for one client program: messages from [`serve`], replies to the socket.
pub struct ServerEnd<Req, Resp> {
    messages: mpsc::Receiver<ClientMessage<Req>>,
    socket: Arc<UdpSocket>,
    client: SocketAddr,
    counters: Arc<UdpCounters>,
    _responses: PhantomData<fn(Resp)>,
}

impl<Req, Resp> Stream for ServerEnd<Req, Resp> {
    type Item = io::Result<ClientMessage<Req>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().messages.poll_recv(cx).map(|m| m.map(Ok))
    }
}

impl<Req, Resp: Serialize> Sink<Response<Resp>> for ServerEnd<Req, Resp> {
    type Error = io::Error;

    /// Always ready: a reply goes out now or counts as lost. All channels share one socket, and tokio
    /// keeps one waker per socket for writing, so a channel that waited here could lose its wake-up
    /// to another and sit until its next message.
    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: Response<Resp>) -> io::Result<()> {
        self.counters.tx_replies.fetch_add(1, Relaxed);
        send_datagram(&self.socket, self.client, &item, &self.counters);
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

struct Program<Req> {
    messages: mpsc::Sender<ClientMessage<Req>>,
    last_seen: Instant,
}

/// Read one socket until aborted, splitting datagrams by source address into one channel per client
/// program. `keep` drops a message before tarpc sees it, which is where the fault gate belongs: a
/// silenced node should not track a call it will never answer. `open` turns each new channel's
/// transport into the task that runs it.
pub async fn serve<Req, Resp, Channel>(
    socket: Arc<UdpSocket>,
    counters: Arc<UdpCounters>,
    idle: Duration,
    keep: impl Fn(&ClientMessage<Req>) -> bool,
    mut open: impl FnMut(ServerEnd<Req, Resp>) -> Channel,
) where
    Req: DeserializeOwned,
    Channel: Future<Output = ()> + Send + 'static,
{
    let mut frames = UdpFramed::new(
        socket.clone(),
        BincodeDatagram::<ClientMessage<Req>>::new(counters.clone()),
    );
    let mut programs: HashMap<SocketAddr, Program<Req>> = HashMap::new();
    let mut channels = JoinSet::new();
    let mut sweep = tokio::time::interval(idle / 4);
    loop {
        tokio::select! {
            item = frames.next() => match item {
                Some(Ok((message, from))) => {
                    if !admit(&message, &keep, &counters) {
                        continue;
                    }
                    let now = Instant::now();
                    let program = programs.entry(from).or_insert_with(|| {
                        let (messages, rx) = mpsc::channel(CHANNEL_QUEUE);
                        counters.channels_opened.fetch_add(1, Relaxed);
                        channels.spawn(open(ServerEnd {
                            messages: rx,
                            socket: socket.clone(),
                            client: from,
                            counters: counters.clone(),
                            _responses: PhantomData,
                        }));
                        Program { messages, last_seen: now }
                    });
                    program.last_seen = now;
                    match program.messages.try_send(message) {
                        Ok(()) => {}
                        Err(mpsc::error::TrySendError::Full(_)) => {
                            counters.rx_queue_full.fetch_add(1, Relaxed);
                        }
                        // Its task ended (a panic), so the next datagram gets a fresh channel.
                        Err(mpsc::error::TrySendError::Closed(_)) => {
                            counters.rx_queue_full.fetch_add(1, Relaxed);
                            programs.remove(&from);
                        }
                    }
                }
                // Must not end this loop: garbage from one address cannot be allowed to cost every
                // other program its service.
                Some(Err(_)) => {
                    counters.rx_bad.fetch_add(1, Relaxed);
                }
                None => return,
            },
            _ = sweep.tick() => {
                let now = Instant::now();
                // Dropping the sender ends that channel's stream. Calls still in flight finish first.
                programs.retain(|_, p| now.duration_since(p.last_seen) < idle);
            }
            Some(_) = channels.join_next(), if !channels.is_empty() => {}
        }
    }
}

/// Counts a message by kind, and says whether it goes on to tarpc: not a request whose deadline tarpc
/// could not hold, and not one `keep` turns away.
fn admit<Req>(
    message: &ClientMessage<Req>,
    keep: &impl Fn(&ClientMessage<Req>) -> bool,
    counters: &UdpCounters,
) -> bool {
    match message {
        ClientMessage::Request(request) => {
            counters.rx_requests.fetch_add(1, Relaxed);
            let wait = request
                .context
                .deadline
                .saturating_duration_since(Instant::now());
            if wait > MAX_CALL_DEADLINE {
                counters.rx_bad.fetch_add(1, Relaxed);
                return false;
            }
        }
        ClientMessage::Cancel { .. } => {
            counters.rx_cancels.fetch_add(1, Relaxed);
        }
        _ => {}
    }
    let kept = keep(message);
    if !kept {
        counters.rx_gate_dropped.fetch_add(1, Relaxed);
    }
    kept
}
