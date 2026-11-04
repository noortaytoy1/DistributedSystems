//! tarpc over UDP: messages to one address wait a moment and share one datagram (D167, D170, D236).
//!
//! tarpc keeps every job an RPC system has: the generated stubs, call IDs, matching replies,
//! deadlines and cancels. This file only moves tarpc messages in packets, because tarpc's own
//! transport needs a byte stream and UDP has none.
//!
//! **Combining (D236).** A message waits up to [`COMBINE_WAIT`] for others to the same address, so a
//! busy program sends far fewer packets: a shared Wi-Fi cell carries packets, not bytes, as its limit.
//! A datagram holding one message is that message alone, as before. One holding several starts with
//! [`COMBINED`], then has a 2-byte length before each message.
//!
//! tarpc wants a `Stream` of incoming messages and a `Sink` of outgoing ones, and an error from
//! either ends the channel and every call on it. So the one rule here is: **no error ever reaches
//! tarpc**. A bad datagram, an ICMP error or a failed send counts as a lost message, and the client's
//! retry (D52) covers it exactly as it covers a packet lost on the wire.
//!
//! - **Read side.** `UdpFramed` with a decoder that takes the whole datagram.
//! - **Write side.** An [`Outbox`] per address, sent with `try_send_to`. `UdpFramed`'s own `Sink`
//!   keeps a datagram whose send failed and appends the next message to it, so it would never recover
//!   (tests/datagram.rs).
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
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, ready};
use std::time::{Duration, Instant};

use bytes::{Buf, BytesMut};
use futures::{Sink, Stream, StreamExt};
use serde::Serialize;
use serde::de::DeserializeOwned;
use socket2::{Domain, Protocol, SockRef, Socket, Type};
use tarpc::{ClientMessage, Response};
use tokio::net::UdpSocket;
use tokio::sync::{Notify, mpsc};
use tokio::task::{JoinHandle, JoinSet};
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

/// How long a message waits for others to the same address (D236, D246). It adds at most this much to
/// each direction of a call. With nothing waiting, no timer runs.
pub const COMBINE_WAIT: Duration = Duration::from_millis(10);

/// First byte of a datagram holding several messages. bincode's variable-length integers never start
/// with it, and a `ClientMessage` or a `Response` starts with one, so a lone message cannot be taken
/// for this. Held to that by `a_lone_message_never_starts_like_a_combined_datagram`.
pub const COMBINED: u8 = 0xFF;

/// The length before each message in a combined datagram.
const LENGTH_BYTES: usize = 2;

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

/// Decodes a datagram into its one message, or its several, one call at a time.
pub struct BincodeDatagram<T> {
    counters: Arc<UdpCounters>,
    /// Partway through a combined datagram: what is left in the buffer is more of its messages.
    combined: bool,
    _message: PhantomData<fn() -> T>,
}

impl<T> BincodeDatagram<T> {
    pub fn new(counters: Arc<UdpCounters>) -> Self {
        Self {
            counters,
            combined: false,
            _message: PhantomData,
        }
    }
}

/// tarpc adds a request's deadline to the clock while decoding, which panics on a huge one. A panic
/// here would end the reader, and with it the node's whole client port, so one stray packet counts as
/// a bad datagram like any other.
fn decode_message<T: DeserializeOwned>(bytes: &[u8]) -> io::Result<T> {
    match catch_unwind(AssertUnwindSafe(|| proto::decode(bytes))) {
        Ok(Ok(message)) => Ok(message),
        _ => Err(io::Error::from(io::ErrorKind::InvalidData)),
    }
}

impl<T: DeserializeOwned> Decoder for BincodeDatagram<T> {
    type Item = T;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> io::Result<Option<T>> {
        if src.is_empty() {
            self.combined = false;
            return Ok(None);
        }
        if !self.combined {
            self.counters.received(src.len());
            if src[0] != COMBINED {
                // Takes the whole datagram, so an error leaves nothing behind for the next call.
                let datagram = src.split();
                return decode_message(&datagram).map(Some);
            }
            src.advance(1);
            self.combined = true;
            if src.is_empty() {
                self.combined = false;
                return Err(io::Error::from(io::ErrorKind::InvalidData));
            }
        }
        let length = if src.len() >= LENGTH_BYTES {
            usize::from(u16::from_be_bytes([src[0], src[1]]))
        } else {
            usize::MAX
        };
        if length == usize::MAX || src.len() - LENGTH_BYTES < length {
            // A cut or false length: the rest cannot be trusted, and a retry covers what it held.
            src.clear();
            self.combined = false;
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        src.advance(LENGTH_BYTES);
        let message = src.split_to(length);
        if src.is_empty() {
            self.combined = false;
        }
        // One bad message costs itself only: the rest stay in the buffer for the next call.
        decode_message(&message).map(Some)
    }
}

/// Messages waiting to share a datagram to one address.
struct Outbox {
    socket: Arc<UdpSocket>,
    to: SocketAddr,
    counters: Arc<UdpCounters>,
    waiting: Mutex<Waiting>,
    first_arrived: Notify,
}

#[derive(Default)]
struct Waiting {
    messages: Vec<Vec<u8>>,
    /// What a combined datagram of these would take, with its first byte.
    bytes: usize,
    /// Datagrams already full, for the sender task to send now.
    full: Vec<Waiting>,
}

impl Outbox {
    fn waiting(&self) -> std::sync::MutexGuard<'_, Waiting> {
        self.waiting.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Encode one message and add it. Never reports an error and never waits.
    fn push<T: Serialize>(&self, message: &T) {
        let bytes = match proto::encode(message) {
            Ok(bytes) if bytes.len() <= proto::MAX_DATAGRAM_BYTES => bytes,
            // A datagram this large might be split on the wire, and every fragment would have to
            // arrive. A test holds the largest report and reply under the limit, so this means a bug.
            encoded => {
                let size = encoded.map_or(0, |bytes| bytes.len());
                tracing::warn!(size, "message does not fit one datagram: dropped");
                self.counters.tx_encode_errors.fetch_add(1, Relaxed);
                return;
            }
        };
        let mut waiting = self.waiting();
        let needs = LENGTH_BYTES + bytes.len();
        let mut wake = false;
        if !waiting.messages.is_empty() && waiting.bytes + needs > proto::MAX_DATAGRAM_BYTES {
            let full = Waiting {
                messages: std::mem::take(&mut waiting.messages),
                bytes: waiting.bytes,
                full: Vec::new(),
            };
            waiting.full.push(full);
            wake = true;
        }
        if waiting.messages.is_empty() {
            waiting.bytes = 1;
            wake = true;
        }
        waiting.bytes += needs;
        waiting.messages.push(bytes);
        drop(waiting);
        if wake {
            self.first_arrived.notify_one();
        }
    }

    fn has_full(&self) -> bool {
        !self.waiting().full.is_empty()
    }

    /// Sends what is full, then what waited, oldest first.
    fn flush(&self) {
        let mut waiting = std::mem::take(&mut *self.waiting());
        for full in std::mem::take(&mut waiting.full) {
            self.send(full);
        }
        self.send(waiting);
    }

    /// Send what waited, or count why it was lost. Never reports an error and never waits.
    fn send(&self, waiting: Waiting) {
        let result = match waiting.messages.as_slice() {
            [] => return,
            [only] => self.socket.try_send_to(only, self.to),
            several => {
                let mut datagram = Vec::with_capacity(waiting.bytes);
                datagram.push(COMBINED);
                for message in several {
                    let length = u16::try_from(message.len())
                        .expect("a message is at most MAX_DATAGRAM_BYTES, which fits two bytes");
                    datagram.extend_from_slice(&length.to_be_bytes());
                    datagram.extend_from_slice(message);
                }
                self.socket.try_send_to(&datagram, self.to)
            }
        };
        match result {
            Ok(n) => self.counters.sent(n),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                self.counters.tx_would_block.fetch_add(1, Relaxed);
            }
            Err(_) => {
                self.counters.tx_errors.fetch_add(1, Relaxed);
            }
        }
    }
}

/// An [`Outbox`] and the task that sends what waited. Dropping it sends what is left: a client's last
/// cancels and a channel's last replies.
struct Sender {
    outbox: Arc<Outbox>,
    task: JoinHandle<()>,
}

impl Sender {
    fn new(socket: Arc<UdpSocket>, to: SocketAddr, counters: Arc<UdpCounters>) -> Self {
        let outbox = Arc::new(Outbox {
            socket,
            to,
            counters,
            waiting: Mutex::default(),
            first_arrived: Notify::new(),
        });
        let task = tokio::spawn({
            let outbox = outbox.clone();
            async move {
                loop {
                    outbox.first_arrived.notified().await;
                    if !outbox.has_full() {
                        tokio::time::sleep(COMBINE_WAIT).await;
                    }
                    // A combined datagram holds many messages, so it waits for room in the kernel's
                    // send buffer. Many channels share a node's socket, and this is safe for that:
                    // `writable` keeps a waker for each waiter (D236).
                    let _ = outbox.socket.writable().await;
                    outbox.flush();
                }
            }
        });
        Self { outbox, task }
    }

    fn push<T: Serialize>(&self, message: &T) {
        self.outbox.push(message);
    }
}

impl Drop for Sender {
    fn drop(&mut self) {
        self.task.abort();
        self.outbox.flush();
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
    sender: Sender,
    _requests: PhantomData<fn(Req)>,
}

impl<Req, Resp> ClientEnd<Req, Resp> {
    pub fn new(socket: UdpSocket, node: SocketAddr, counters: Arc<UdpCounters>) -> Self {
        let socket = Arc::new(socket);
        Self {
            frames: UdpFramed::new(socket.clone(), BincodeDatagram::new(counters.clone())),
            sender: Sender::new(socket.clone(), node, counters.clone()),
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
        self.sender.push(&item);
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
    sender: Sender,
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
        self.sender.push(&item);
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
                            sender: Sender::new(socket.clone(), from, counters.clone()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use tarpc::context;

    fn counters() -> Arc<UdpCounters> {
        Arc::new(UdpCounters::default())
    }

    fn combined(messages: &[&[u8]]) -> BytesMut {
        let mut datagram = BytesMut::new();
        datagram.extend_from_slice(&[COMBINED]);
        for m in messages {
            datagram.extend_from_slice(&(m.len() as u16).to_be_bytes());
            datagram.extend_from_slice(m);
        }
        datagram
    }

    #[test]
    fn a_lone_message_never_starts_like_a_combined_datagram() {
        // A request starts with its variant, and a response with its call ID, the largest there is.
        let request: ClientMessage<u32> = ClientMessage::Request(tarpc::Request {
            context: context::current(),
            id: u64::MAX,
            message: 7,
        });
        assert_ne!(proto::encode(&request).unwrap()[0], COMBINED);
        let cancel: ClientMessage<u32> = ClientMessage::Cancel {
            trace_context: context::current().trace_context,
            request_id: u64::MAX,
        };
        assert_ne!(proto::encode(&cancel).unwrap()[0], COMBINED);
        let response = Response::<u32> {
            request_id: u64::MAX,
            message: Ok(7),
        };
        assert_ne!(proto::encode(&response).unwrap()[0], COMBINED);
    }

    #[test]
    fn a_combined_datagram_gives_each_of_its_messages_and_counts_as_one() {
        let counters = counters();
        let mut decoder = BincodeDatagram::<u32>::new(counters.clone());
        let (a, b) = (
            proto::encode(&5u32).unwrap(),
            proto::encode(&900u32).unwrap(),
        );
        let mut src = combined(&[&a, &b]);
        assert_eq!(decoder.decode(&mut src).unwrap(), Some(5));
        assert_eq!(decoder.decode(&mut src).unwrap(), Some(900));
        assert_eq!(decoder.decode(&mut src).unwrap(), None);
        assert_eq!(counters.rx_datagrams.load(Relaxed), 1);
    }

    #[test]
    fn a_lone_message_still_decodes_after_a_combined_datagram() {
        let mut decoder = BincodeDatagram::<u32>::new(counters());
        let a = proto::encode(&5u32).unwrap();
        let mut src = combined(&[&a]);
        assert_eq!(decoder.decode(&mut src).unwrap(), Some(5));
        assert_eq!(decoder.decode(&mut src).unwrap(), None);
        let mut src = BytesMut::from(&a[..]);
        assert_eq!(decoder.decode(&mut src).unwrap(), Some(5));
    }

    #[test]
    fn a_bad_message_costs_only_itself_and_a_false_length_ends_the_datagram() {
        let mut decoder = BincodeDatagram::<u32>::new(counters());
        let good = proto::encode(&5u32).unwrap();
        let mut src = combined(&[&[0xFF, 0xFF, 0xFF], &good]);
        assert!(decoder.decode(&mut src).is_err());
        assert_eq!(decoder.decode(&mut src).unwrap(), Some(5));
        assert_eq!(decoder.decode(&mut src).unwrap(), None);

        let mut src = BytesMut::from(&[COMBINED, 0x00, 0x09, 0x01][..]);
        assert!(decoder.decode(&mut src).is_err());
        assert!(src.is_empty());
        let mut src = BytesMut::from(&good[..]);
        assert_eq!(decoder.decode(&mut src).unwrap(), Some(5));

        let mut src = BytesMut::from(&[COMBINED][..]);
        assert!(decoder.decode(&mut src).is_err());
        let mut src = BytesMut::from(&good[..]);
        assert_eq!(decoder.decode(&mut src).unwrap(), Some(5));
    }

    async fn pair() -> (Arc<UdpSocket>, UdpSocket, SocketAddr) {
        let any: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let receiver = UdpSocket::bind(any).await.unwrap();
        let to = receiver.local_addr().unwrap();
        (Arc::new(UdpSocket::bind(any).await.unwrap()), receiver, to)
    }

    #[tokio::test]
    async fn messages_sent_together_share_one_datagram() {
        let (socket, receiver, to) = pair().await;
        let counters = counters();
        let sender = Sender::new(socket, to, counters.clone());
        for n in 0..5u32 {
            sender.push(&n);
        }
        let mut buf = [0u8; 2048];
        let n = tokio::time::timeout(Duration::from_secs(5), receiver.recv(&mut buf))
            .await
            .unwrap()
            .unwrap();
        let mut src = BytesMut::from(&buf[..n]);
        let mut decoder = BincodeDatagram::<u32>::new(Arc::new(UdpCounters::default()));
        for want in 0..5u32 {
            assert_eq!(decoder.decode(&mut src).unwrap(), Some(want));
        }
        assert_eq!(counters.tx_datagrams.load(Relaxed), 1);
    }

    #[tokio::test]
    async fn a_full_datagram_goes_at_once_and_none_passes_the_limit() {
        let (socket, receiver, to) = pair().await;
        let sender = Sender::new(socket, to, counters());
        let large = vec![7u8; 600];
        for _ in 0..3 {
            sender.push(&large);
        }
        let mut buf = [0u8; 4096];
        let n = tokio::time::timeout(Duration::from_secs(5), receiver.recv(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert!(n <= proto::MAX_DATAGRAM_BYTES, "{n}");
        assert_eq!(buf[0], COMBINED);
    }

    #[tokio::test]
    async fn dropping_the_sender_sends_what_waited() {
        let (socket, receiver, to) = pair().await;
        // A socket says it can send only once it has been asked.
        socket.writable().await.unwrap();
        let sender = Sender::new(socket, to, counters());
        sender.push(&9u32);
        drop(sender);
        let mut buf = [0u8; 64];
        let n = tokio::time::timeout(Duration::from_secs(5), receiver.recv(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(proto::decode::<u32>(&buf[..n]).unwrap(), 9);
    }
}
