//! What tarpc really does on an unreliable transport, and what `node/datagram.rs` does about it. Each
//! test states a claim made from reading tarpc's and tokio-util's sources, and checks it by running
//! it, so a version bump that changes one fails here (D170).
#![allow(clippy::unwrap_used)]

mod common;

use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use common::wait_until;
use futures::{Sink, SinkExt, Stream};
use rescuemesh::metrics::UdpCounters;
use rescuemesh::node::datagram::{self, BincodeDatagram, ClientEnd, ServerEnd, WireMessage};
use rescuemesh::node::transport::run_calls;
use rescuemesh::proto;
use tarpc::server::{BaseChannel, Channel};
use tarpc::{ClientMessage, Request, Response, context};
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;
use tokio_util::codec::{BytesCodec, Decoder};
use tokio_util::udp::UdpFramed;

const PATIENCE: Duration = Duration::from_secs(10);

#[tarpc::service]
trait Echo {
    async fn echo(v: u32) -> u32;
    /// Never answers.
    async fn hang(v: u32) -> u32;
    /// Counts each time the handler starts.
    async fn count(v: u32) -> u32;
}

#[derive(Clone)]
struct Server {
    started: Arc<AtomicU32>,
    aborted: Arc<AtomicU32>,
}

struct DropCounter(Arc<AtomicU32>);

impl Drop for DropCounter {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

impl Echo for Server {
    async fn echo(self, _: context::Context, v: u32) -> u32 {
        v
    }

    async fn hang(self, _: context::Context, _: u32) -> u32 {
        self.started.fetch_add(1, Ordering::SeqCst);
        let _guard = DropCounter(self.aborted.clone());
        std::future::pending().await
    }

    async fn count(self, _: context::Context, v: u32) -> u32 {
        self.started.fetch_add(1, Ordering::SeqCst);
        v
    }
}

struct TestServer {
    addr: SocketAddr,
    started: Arc<AtomicU32>,
    aborted: Arc<AtomicU32>,
    counters: Arc<UdpCounters>,
    task: JoinHandle<()>,
}

async fn start_server_with(idle: Duration, channels_ended: Arc<AtomicU32>) -> TestServer {
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let addr = socket.local_addr().unwrap();
    let started = Arc::new(AtomicU32::new(0));
    let aborted = Arc::new(AtomicU32::new(0));
    let counters = Arc::new(UdpCounters::default());
    let server = Server {
        started: started.clone(),
        aborted: aborted.clone(),
    };
    let task = tokio::spawn(datagram::serve(
        socket,
        counters.clone(),
        idle,
        |_: &ClientMessage<EchoRequest>| true,
        move |end: ServerEnd<EchoRequest, EchoResponse>| {
            let ended = DropCounter(channels_ended.clone());
            let calls = BaseChannel::with_defaults(end).execute(server.clone().serve());
            async move {
                run_calls(calls).await;
                drop(ended);
            }
        },
    ));
    TestServer {
        addr,
        started,
        aborted,
        counters,
        task,
    }
}

async fn start_server() -> TestServer {
    start_server_with(datagram::CHANNEL_IDLE, Arc::default()).await
}

/// A stub on the adapter, with the dispatch it needs. Dropping the guard ends both.
struct AdapterClient {
    stub: EchoClient,
    _dispatch: tokio::task::JoinSet<()>,
}

fn adapter_client_on(
    socket: UdpSocket,
    node: SocketAddr,
    counters: Arc<UdpCounters>,
) -> AdapterClient {
    let end = ClientEnd::new(socket, node, counters);
    let tarpc::client::NewClient { client, dispatch } =
        EchoClient::new(tarpc::client::Config::default(), end);
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(async move {
        let _ = dispatch.await;
    });
    AdapterClient {
        stub: client,
        _dispatch: tasks,
    }
}

fn adapter_client(node: SocketAddr) -> AdapterClient {
    let socket = datagram::client_socket(node).unwrap();
    adapter_client_on(socket, node, Arc::default())
}

fn deadline_ctx(wait: Duration) -> context::Context {
    let mut ctx = context::current();
    ctx.deadline = Instant::now() + wait;
    ctx
}

/// The thin adapter one would write first: `UdpFramed`, with every error passed on to tarpc.
struct Naive {
    frames: UdpFramed<BincodeDatagram<Response<EchoResponse>>, Arc<UdpSocket>>,
    socket: Arc<UdpSocket>,
    peer: SocketAddr,
}

impl Stream for Naive {
    type Item = io::Result<Response<EchoResponse>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        Pin::new(&mut this.frames)
            .poll_next(cx)
            .map(|m| m.map(|r| r.map(|(msg, _)| msg)))
    }
}

fn wire_message(bytes: &[u8]) -> ClientMessage<EchoRequest> {
    let wire: WireMessage<EchoRequest> = proto::decode(bytes).unwrap();
    wire.into_message()
}

impl Sink<ClientMessage<EchoRequest>> for Naive {
    type Error = io::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.socket.poll_send_ready(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: ClientMessage<EchoRequest>) -> io::Result<()> {
        let bytes = proto::encode(&WireMessage::from_message(item).unwrap()).unwrap();
        // A send error is passed on too, as a thin adapter would.
        self.socket.try_send_to(&bytes, self.peer).map(|_| ())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.socket.poll_send_ready(cx)
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

fn naive_client(socket: UdpSocket, peer: SocketAddr) -> EchoClient {
    let socket = Arc::new(socket);
    let transport = Naive {
        frames: UdpFramed::new(
            socket.clone(),
            BincodeDatagram::new(Arc::new(UdpCounters::default())),
        ),
        socket,
        peer,
    };
    EchoClient::new(tarpc::client::Config::default(), transport).spawn()
}

/// Call until a call ends in `Shutdown`, or fail: a transport's death reaches callers a moment later.
async fn until_shutdown(client: &EchoClient) {
    let deadline = Instant::now() + PATIENCE;
    loop {
        let result = client
            .echo(deadline_ctx(Duration::from_millis(100)), 1)
            .await;
        if matches!(result, Err(tarpc::client::RpcError::Shutdown)) {
            return;
        }
        assert!(Instant::now() < deadline, "still alive: {result:?}");
    }
}

/// CLAIM: a call whose request is lost ends at its deadline with DeadlineExceeded, and the channel
/// keeps working afterwards. Nothing in tarpc resends.
#[tokio::test]
async fn a_lost_request_ends_at_the_deadline_and_the_channel_lives_on() {
    let server = start_server().await;
    // Nobody listens on this port, so every request is lost.
    let dead = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let dead_addr = dead.local_addr().unwrap();
    drop(dead);
    let client = adapter_client(dead_addr);
    let t = Instant::now();
    let err = client
        .stub
        .echo(deadline_ctx(Duration::from_millis(200)), 1)
        .await
        .unwrap_err();
    assert!(matches!(err, tarpc::client::RpcError::DeadlineExceeded));
    assert!(t.elapsed() >= Duration::from_millis(190));
    // Calling again still works in the same way: not Shutdown.
    let err = client
        .stub
        .echo(deadline_ctx(Duration::from_millis(100)), 2)
        .await
        .unwrap_err();
    assert!(matches!(err, tarpc::client::RpcError::DeadlineExceeded));
    // And a working server answers on its own channel.
    let ok = adapter_client(server.addr);
    assert_eq!(ok.stub.echo(deadline_ctx(PATIENCE), 3).await.unwrap(), 3);
}

/// CLAIM: dropping a call sends one Cancel, which aborts the handler at once. A call that ends at
/// its deadline sends no Cancel, and the server aborts the handler by itself at the deadline. This is
/// what the node's work depends on and what `udp_counters` reports (D169, D172).
#[tokio::test]
async fn a_cancel_aborts_the_handler_and_a_deadline_needs_no_cancel() {
    let server = start_server().await;
    let counters = Arc::new(UdpCounters::default());
    let socket = datagram::client_socket(server.addr).unwrap();
    let client = adapter_client_on(socket, server.addr, counters.clone());

    // Deadline: no Cancel on the wire, handler dropped by the server's own timer.
    let err = client
        .stub
        .hang(deadline_ctx(Duration::from_millis(200)), 1)
        .await
        .unwrap_err();
    assert!(matches!(err, tarpc::client::RpcError::DeadlineExceeded));
    wait_until(PATIENCE, "the handler to end at its deadline", || {
        server.aborted.load(Ordering::SeqCst) == 1
    })
    .await;
    assert_eq!(counters.tx_cancels.load(Ordering::SeqCst), 0);

    // Dropped call: one Cancel, handler aborted long before the 30 s deadline.
    let call = client.stub.hang(deadline_ctx(Duration::from_secs(30)), 2);
    let timeout = tokio::time::timeout(Duration::from_millis(200), call).await;
    assert!(timeout.is_err());
    wait_until(PATIENCE, "the Cancel to end the handler", || {
        server.aborted.load(Ordering::SeqCst) == 2
    })
    .await;
    assert_eq!(counters.tx_cancels.load(Ordering::SeqCst), 1);
    assert_eq!(server.counters.rx_cancels.load(Ordering::SeqCst), 1);
}

/// Sends raw tarpc messages to a server, as the network may deliver them.
struct Raw {
    socket: UdpSocket,
    to: SocketAddr,
}

impl Raw {
    async fn new(to: SocketAddr) -> Self {
        Self {
            socket: UdpSocket::bind("127.0.0.1:0").await.unwrap(),
            to,
        }
    }

    async fn request(&self, id: u64, v: u32, count: bool, wait: Duration) {
        let message = if count {
            EchoRequest::Count { v }
        } else {
            EchoRequest::Hang { v }
        };
        let msg = ClientMessage::Request(Request {
            context: deadline_ctx(wait),
            id,
            message,
        });
        self.socket
            .send_to(
                &proto::encode(&WireMessage::from_message(msg).unwrap()).unwrap(),
                self.to,
            )
            .await
            .unwrap();
    }

    async fn cancel(&self, id: u64) {
        let msg: ClientMessage<EchoRequest> = ClientMessage::Cancel {
            trace_context: context::current().trace_context,
            request_id: id,
        };
        self.socket
            .send_to(
                &proto::encode(&WireMessage::from_message(msg).unwrap()).unwrap(),
                self.to,
            )
            .await
            .unwrap();
    }

    /// Wait for one answer. On a single-thread test, by the time a later request has been answered,
    /// everything sent before it has been handled, so this also stands in for "wait and see".
    async fn answer(&self) -> Response<EchoResponse> {
        let mut buf = [0u8; 1500];
        let (n, _) = tokio::time::timeout(PATIENCE, self.socket.recv_from(&mut buf))
            .await
            .expect("an answer")
            .unwrap();
        proto::decode(&buf[..n]).unwrap()
    }
}

/// CLAIM: duplicates and reordering do no harm to tarpc itself.
/// - The same request ID while the first is in flight: ignored.
/// - The same ID after the first finished: handled again (no memory of finished IDs).
/// - A Cancel that overtakes its request: does nothing; the request then runs until its deadline.
#[tokio::test]
async fn duplicates_and_reordering_do_no_harm() {
    let server = start_server().await;
    let raw = Raw::new(server.addr).await;
    let long = Duration::from_secs(30);

    raw.request(1, 0, false, long).await;
    raw.request(1, 0, false, long).await;
    raw.request(10, 7, true, long).await;
    raw.answer().await;
    assert_eq!(
        server.started.load(Ordering::SeqCst),
        2,
        "one hang, once, and the marker"
    );

    raw.cancel(2).await;
    raw.request(2, 0, false, Duration::from_millis(300)).await;
    raw.request(11, 7, true, long).await;
    raw.answer().await;
    assert_eq!(
        server.started.load(Ordering::SeqCst),
        4,
        "the late request still starts"
    );
    assert_eq!(
        server.aborted.load(Ordering::SeqCst),
        0,
        "the early cancel did nothing"
    );
    wait_until(PATIENCE, "the deadline to clean it up", || {
        server.aborted.load(Ordering::SeqCst) == 1
    })
    .await;

    let before = server.started.load(Ordering::SeqCst);
    raw.request(3, 7, true, long).await;
    raw.answer().await;
    raw.request(3, 7, true, long).await;
    raw.answer().await;
    assert_eq!(server.started.load(Ordering::SeqCst) - before, 2);
}

/// CLAIM: a response nobody waits for any more is ignored, and a response with an unknown ID too.
#[tokio::test]
async fn late_and_unknown_responses_are_ignored() {
    // A stub server that answers late and with a made-up ID.
    let stub = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = stub.local_addr().unwrap();
    let client = adapter_client(addr);
    let err = client
        .stub
        .echo(deadline_ctx(Duration::from_millis(100)), 1)
        .await
        .unwrap_err();
    assert!(matches!(err, tarpc::client::RpcError::DeadlineExceeded));
    let mut buf = [0u8; 1500];
    let (n, from) = stub.recv_from(&mut buf).await.unwrap();
    let ClientMessage::Request(req) = wire_message(&buf[..n]) else {
        panic!("expected a request")
    };
    for id in [req.id, 999_999] {
        let late = Response {
            request_id: id,
            message: Ok(EchoResponse::Echo(5)),
        };
        stub.send_to(&proto::encode(&late).unwrap(), from)
            .await
            .unwrap();
    }
    // The next call is answered normally: the channel is still alive.
    let second = client.stub.echo(deadline_ctx(PATIENCE), 2);
    let answer = async {
        let (n, from) = stub.recv_from(&mut buf).await.unwrap();
        let ClientMessage::Request(req) = wire_message(&buf[..n]) else {
            panic!("expected a request")
        };
        let answer = Response {
            request_id: req.id,
            message: Ok(EchoResponse::Echo(2)),
        };
        stub.send_to(&proto::encode(&answer).unwrap(), from)
            .await
            .unwrap();
    };
    let (reply, ()) = tokio::join!(second, answer);
    assert_eq!(reply.unwrap(), 2);
}

/// CLAIM: one stray datagram kills a naive client: the Err reaches tarpc, the dispatch ends, every
/// call on it fails from then on. The adapter counts it and carries on, on both ends.
#[tokio::test]
async fn a_bad_datagram_kills_a_naive_transport_but_not_the_adapter() {
    let server = start_server().await;

    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_addr = socket.local_addr().unwrap();
    let naive = naive_client(socket, server.addr);
    assert_eq!(naive.echo(deadline_ctx(PATIENCE), 1).await.unwrap(), 1);
    let stray = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    stray
        .send_to(b"\xff\xff\xff\xff garbage", client_addr)
        .await
        .unwrap();
    until_shutdown(&naive).await;

    let counters = Arc::new(UdpCounters::default());
    let socket = datagram::client_socket(server.addr).unwrap();
    let client_addr: SocketAddr = format!("127.0.0.1:{}", socket.local_addr().unwrap().port())
        .parse()
        .unwrap();
    let good = adapter_client_on(socket, server.addr, counters.clone());
    stray
        .send_to(b"\xff\xff\xff\xff garbage", client_addr)
        .await
        .unwrap();
    // A valid message from the wrong address must not be matched to a call either.
    let from_a_stranger = Response {
        request_id: 0,
        message: Ok(EchoResponse::Echo(666)),
    };
    stray
        .send_to(&proto::encode(&from_a_stranger).unwrap(), client_addr)
        .await
        .unwrap();
    assert_eq!(good.stub.echo(deadline_ctx(PATIENCE), 3).await.unwrap(), 3);
    wait_until(PATIENCE, "both counted on the client", || {
        counters.rx_bad.load(Ordering::SeqCst) == 1
            && counters.rx_foreign.load(Ordering::SeqCst) == 1
    })
    .await;

    // The server side: garbage from one source must not disturb the channel of another.
    stray
        .send_to(b"\xff\xff\xff\xff garbage", server.addr)
        .await
        .unwrap();
    wait_until(PATIENCE, "the garbage counted on the server", || {
        server.counters.rx_bad.load(Ordering::SeqCst) == 1
    })
    .await;
    assert_eq!(good.stub.echo(deadline_ctx(PATIENCE), 4).await.unwrap(), 4);
}

/// CLAIM: decoding a request adds its deadline to the clock, and past what an `Instant` can hold that
/// panics. A deadline past about two years panics tarpc's deadline queue instead, later. Neither may
/// cost the node its client port.
#[tokio::test]
async fn a_huge_deadline_is_a_bad_datagram() {
    let server = start_server().await;
    let stray = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let trace_context = context::current().trace_context;
    let hostile = |secs: u64| {
        // A request, field by field, as bincode lays one out.
        proto::encode(&(
            0u32,
            Duration::new(secs, 0),
            trace_context,
            7u64,
            EchoRequest::Echo { v: 1 },
        ))
        .unwrap()
    };
    stray
        .send_to(&hostile(u64::MAX), server.addr)
        .await
        .unwrap();
    stray
        .send_to(&hostile(3 * 365 * 24 * 3600), server.addr)
        .await
        .unwrap();
    wait_until(PATIENCE, "both counted as bad", || {
        server.counters.rx_bad.load(Ordering::SeqCst) == 2
    })
    .await;

    let client = adapter_client(server.addr);
    assert_eq!(
        client.stub.echo(deadline_ctx(PATIENCE), 3).await.unwrap(),
        3
    );
}

/// CLAIM: on a *connected* UDP socket the ICMP "port unreachable" from a dead node comes back as
/// ECONNREFUSED on the next send. On a request that fails only that call. On a Cancel it is terminal
/// (`start_send(cancel)` maps to `ChannelError::Write`), so the whole client dies. An unconnected
/// socket never sees any of it, which is why the client's sockets are unconnected.
#[tokio::test]
async fn a_dead_node_kills_a_connected_naive_client_only() {
    let dead = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let dead_addr = dead.local_addr().unwrap();
    drop(dead);

    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    socket.connect(dead_addr).await.unwrap();
    let connected = naive_client(socket, dead_addr);
    let wait = Duration::from_millis(300);
    let first = connected.echo(deadline_ctx(wait), 1).await;
    let second = connected.echo(deadline_ctx(wait), 2).await;
    assert!(matches!(
        first,
        Err(tarpc::client::RpcError::DeadlineExceeded)
    ));
    assert!(matches!(second, Err(tarpc::client::RpcError::Send(_))));
    // This request's send works and draws a new ICMP error; dropping the call sends a Cancel.
    let call = connected.hang(deadline_ctx(Duration::from_secs(30)), 3);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), call)
            .await
            .is_err()
    );
    // Give the dispatch its turn to write that Cancel. A request written first would take the
    // socket's waiting error for itself, and only that call would fail.
    for _ in 0..100 {
        tokio::task::yield_now().await;
    }
    until_shutdown(&connected).await;

    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let unconnected = naive_client(socket, dead_addr);
    let first = unconnected.echo(deadline_ctx(wait), 1).await;
    let second = unconnected.echo(deadline_ctx(wait), 2).await;
    assert!(matches!(
        first,
        Err(tarpc::client::RpcError::DeadlineExceeded)
    ));
    assert!(matches!(
        second,
        Err(tarpc::client::RpcError::DeadlineExceeded)
    ));
}

/// CLAIM: `UdpFramed`'s Sink keeps a datagram whose send failed, and the next message is appended to
/// it. So after one EMSGSIZE every later flush fails too: it can never recover, which is why the
/// adapter sends with `try_send_to` instead.
#[tokio::test]
async fn udpframed_sink_never_recovers_from_a_failed_send() {
    let receiver = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let to = receiver.local_addr().unwrap();
    let mut framed = UdpFramed::new(
        UdpSocket::bind("127.0.0.1:0").await.unwrap(),
        BytesCodec::new(),
    );
    let too_big = Bytes::from(vec![0u8; 70_000]);
    assert!(framed.send((too_big, to)).await.is_err());
    let small = framed.send((Bytes::from_static(b"small"), to)).await;
    assert!(small.is_err(), "the failed bytes are still in the buffer");
    let mut buf = [0u8; 16];
    let got = tokio::time::timeout(Duration::from_millis(200), receiver.recv_from(&mut buf)).await;
    assert!(got.is_err(), "the small message never left");
}

/// CLAIM: a message too large for a datagram is dropped and counted, and the channel lives on. The
/// caller sees only its deadline, as with any lost packet.
#[tokio::test]
async fn a_message_too_large_is_counted_and_the_channel_lives_on() {
    #[tarpc::service]
    trait Big {
        async fn big(bytes: u32) -> Vec<u8>;
    }

    #[derive(Clone)]
    struct BigServer;

    impl Big for BigServer {
        async fn big(self, _: context::Context, bytes: u32) -> Vec<u8> {
            vec![0; bytes as usize]
        }
    }

    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let addr = socket.local_addr().unwrap();
    let counters = Arc::new(UdpCounters::default());
    let _server = tokio::spawn(datagram::serve(
        socket,
        counters.clone(),
        datagram::CHANNEL_IDLE,
        |_: &ClientMessage<BigRequest>| true,
        |end: ServerEnd<BigRequest, BigResponse>| {
            run_calls(BaseChannel::with_defaults(end).execute(BigServer.serve()))
        },
    ));
    let end = ClientEnd::new(datagram::client_socket(addr).unwrap(), addr, Arc::default());
    let client = BigClient::new(tarpc::client::Config::default(), end).spawn();

    let err = client
        .big(deadline_ctx(Duration::from_millis(300)), 2_000)
        .await
        .unwrap_err();
    assert!(matches!(err, tarpc::client::RpcError::DeadlineExceeded));
    assert_eq!(counters.tx_encode_errors.load(Ordering::SeqCst), 1);
    assert_eq!(counters.tx_datagrams.load(Ordering::SeqCst), 0);

    let small = client.big(deadline_ctx(PATIENCE), 500).await.unwrap();
    assert_eq!(small.len(), 500);
}

/// CLAIM: `BincodeDatagram` leaves nothing behind after a bad datagram, as `UdpFramed`'s docs ask.
#[test]
fn the_decoder_consumes_a_bad_datagram() {
    let mut codec = BincodeDatagram::<u32>::new(Arc::default());
    let mut buf = BytesMut::from(&b"\xff\xff\xff\xff\xff\xff"[..]);
    assert!(codec.decode(&mut buf).is_err());
    assert!(buf.is_empty());
    assert!(codec.decode(&mut buf).unwrap().is_none());
}

/// CLAIM: a client that restarts on the same source address restarts its IDs at 0. While the old call
/// with that ID is in flight, the server ignores the new one, so it hangs until its deadline. Rare,
/// and the client's retry uses a new ID, so it costs one wait (D173).
#[tokio::test]
async fn a_reused_source_port_can_collide_with_an_old_id() {
    let server = start_server().await;
    let old = Raw::new(server.addr).await;
    let port = old.socket.local_addr().unwrap();
    old.request(0, 0, false, Duration::from_secs(5)).await;
    wait_until(PATIENCE, "the old call to start", || {
        server.started.load(Ordering::SeqCst) == 1
    })
    .await;
    drop(old);

    let new = Raw {
        socket: UdpSocket::bind(port).await.unwrap(),
        to: server.addr,
    };
    new.request(0, 9, true, Duration::from_secs(2)).await;
    // The next ID works, and by then the one before it has been handled.
    new.request(1, 9, true, Duration::from_secs(2)).await;
    let answer = new.answer().await;
    assert_eq!(answer.request_id, 1, "ID 0 was ignored as a duplicate");
    assert_eq!(server.started.load(Ordering::SeqCst), 2);
}

/// CLAIM: UDP has no close, so a program that stops sending loses its channel after the idle time,
/// and comes back with a new one if it was only quiet.
#[tokio::test]
async fn an_idle_program_loses_its_channel_and_gets_a_new_one() {
    let ended = Arc::new(AtomicU32::new(0));
    let server = start_server_with(Duration::from_millis(100), ended.clone()).await;
    let client = adapter_client(server.addr);
    assert_eq!(
        client.stub.echo(deadline_ctx(PATIENCE), 1).await.unwrap(),
        1
    );
    wait_until(PATIENCE, "the idle channel to end", || {
        ended.load(Ordering::SeqCst) == 1
    })
    .await;
    assert_eq!(
        client.stub.echo(deadline_ctx(PATIENCE), 2).await.unwrap(),
        2
    );
    assert_eq!(server.counters.channels_opened.load(Ordering::SeqCst), 2);
}

/// CLAIM: aborting `serve`, which is what a node's shutdown does, ends every channel and every call
/// on them, and gives the port back. Nothing is left running on its own.
#[tokio::test]
async fn aborting_serve_ends_every_channel_and_call_and_frees_the_port() {
    let ended = Arc::new(AtomicU32::new(0));
    let server = start_server_with(datagram::CHANNEL_IDLE, ended.clone()).await;
    let raw = Raw::new(server.addr).await;
    raw.request(1, 0, false, Duration::from_secs(30)).await;
    wait_until(PATIENCE, "the call to start", || {
        server.started.load(Ordering::SeqCst) == 1
    })
    .await;
    assert_eq!(server.aborted.load(Ordering::SeqCst), 0);

    server.task.abort();
    wait_until(PATIENCE, "the call and its channel to end", || {
        server.aborted.load(Ordering::SeqCst) == 1 && ended.load(Ordering::SeqCst) == 1
    })
    .await;
    wait_until(PATIENCE, "the port to come free", || {
        std::net::UdpSocket::bind(server.addr).is_ok()
    })
    .await;
}
