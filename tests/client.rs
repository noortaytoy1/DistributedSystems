//! The client middleware against real nodes, and against stub nodes for what real nodes cannot
//! be made to do on demand (going away and coming back on the same address).
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{TestCluster, wait_until};
use futures::future::BoxFuture;
use rescuemesh::client::{ClientError, MeshClient, control, udp_answers};
use rescuemesh::config::{AssignmentPolicy, ClusterConfig, Config, NodeAddr};
use rescuemesh::metrics::EventLog;
use rescuemesh::node::datagram::{self, ServerEnd};
use rescuemesh::node::transport::{datagram_client, framed, run_calls};
use rescuemesh::proto::{
    Capability, Category, ClientReply, ClientRequest, ClientRpc, ClientRpcRequest,
    ClientRpcResponse, CommittedResult, ControlReply, ControlRequest, Incident, IncidentQuery,
    IncidentStatus, Plan, Point, ReplyBody, RequestId, UserId,
};
use tarpc::client::{NewClient, RpcError};
use tarpc::server::{BaseChannel, Channel};
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::Notify;
use tokio::task::{JoinHandle, JoinSet};

fn incident() -> Incident {
    Incident {
        owner: UserId { client: 5, user: 0 },
        category: Category::MedicalAssistance,
        zone: 0,
        location: Point { x: 10, y: 20 },
        severity: 2,
        needs: Capability::Medical,
        reported_at_ms: rescuemesh::metrics::now_us() / 1000,
        ttl_ms: 60_000,
    }
}

async fn ready(cluster: &TestCluster) {
    wait_until(Duration::from_secs(10), "a leader on every node", || {
        cluster
            .nodes
            .iter()
            .all(|n| n.ctx.consensus.status().leader.is_some())
    })
    .await;
}

fn fast_retries(c: &mut Config) {
    c.timers.client_retry_ms = 300;
    c.timers.client_retry_max_ms = 600;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_submit_returns_the_committed_result_and_logs_its_events() {
    let cluster = TestCluster::start(3).await;
    ready(&cluster).await;
    let dir = std::env::temp_dir().join(format!("rescuemesh-client-{}", std::process::id()));
    let log = EventLog::open(Some(&dir), "client-5").unwrap();
    let client = MeshClient::new(5, &cluster.config, log.clone());

    let done = client.submit(incident()).await.unwrap();
    let result = done.committed.as_ref().unwrap();
    assert!(!done.provisional);
    assert_eq!(result.request.client, 5);
    assert_eq!(done.attempts, 1);
    let stored = cluster
        .node(done.from)
        .ctx
        .state
        .with(|s| s.result(&result.request).cloned());
    assert_eq!(stored.as_ref(), Some(result));

    drop(client);
    tokio::task::spawn_blocking(move || log.close())
        .await
        .unwrap();
    let text = std::fs::read_to_string(dir.join("client-5.jsonl")).unwrap();
    assert!(text.contains(r#""ev":"client_sent""#), "{text}");
    assert!(text.contains(r#""ev":"client_done""#), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn many_users_share_one_client() {
    let cluster = TestCluster::start(3).await;
    ready(&cluster).await;
    let client = Arc::new(MeshClient::new(6, &cluster.config, EventLog::disabled("c")));
    let mut users = JoinSet::new();
    for _ in 0..50 {
        let client = client.clone();
        users.spawn(async move {
            client
                .submit(incident())
                .await
                .unwrap()
                .committed
                .unwrap()
                .request
        });
    }
    let mut seen = std::collections::BTreeSet::new();
    while let Some(id) = users.join_next().await {
        assert!(seen.insert(id.unwrap()), "two users got the same request");
    }
    assert_eq!(seen.len(), 50);
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_silenced_follower_does_not_disturb_service() {
    let cluster = TestCluster::start(3).await;
    ready(&cluster).await;
    let leader = cluster.node(1).ctx.consensus.status().leader.unwrap();
    let follower = (1..=3).find(|id| *id != leader).unwrap();
    cluster.node(follower).ctx.gate.silence();

    let client = MeshClient::new(7, &cluster.config, EventLog::disabled("c"));
    let done = client.submit(incident()).await.unwrap();
    assert_ne!(done.from, follower);
    cluster.node(follower).ctx.gate.restore();
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_silenced_leader_is_covered_by_a_retry_with_the_same_id() {
    let cluster = TestCluster::start_with(3, fast_retries).await;
    ready(&cluster).await;
    let leader = cluster.node(1).ctx.consensus.status().leader.unwrap();
    cluster.node(leader).ctx.gate.silence();

    let client = MeshClient::new(8, &cluster.config, EventLog::disabled("c"));
    let done = client.submit(incident()).await.unwrap();
    assert_ne!(done.executor, leader);
    let request = done.committed.unwrap().request;
    assert!(done.attempts >= 1);
    // One result for the ID, however many attempts it took.
    for id in (1..=3u64).filter(|id| *id != leader) {
        let node = cluster.node(id);
        wait_until(
            Duration::from_secs(5),
            "the result on every live node",
            || node.ctx.state.with(|s| s.result(&request).is_some()),
        )
        .await;
    }
    cluster.node(leader).ctx.gate.restore();
    cluster.shutdown().await;
}

fn stub_config(addrs: &[String]) -> Config {
    let nodes = addrs
        .iter()
        .enumerate()
        .map(|(i, addr)| NodeAddr {
            id: i as u64 + 1,
            peer_addr: format!("127.0.0.1:{}", 1 + i),
            client_addr: addr.clone(),
        })
        .collect();
    let mut config = Config {
        cluster: ClusterConfig { nodes },
        timers: Default::default(),
        policy: Default::default(),
        workers: Default::default(),
        campus: Default::default(),
        metrics: Default::default(),
        faults: Default::default(),
        storage: Default::default(),
    };
    fast_retries(&mut config);
    config
}

/// A node stand-in: `answer` says what to do with each report. It may never return, which is what a
/// node that stays quiet looks like.
#[derive(Clone)]
struct Stub {
    answer: Arc<dyn Fn(ClientRequest) -> BoxFuture<'static, ClientReply> + Send + Sync>,
}

impl Stub {
    fn new(
        answer: impl Fn(ClientRequest) -> BoxFuture<'static, ClientReply> + Send + Sync + 'static,
    ) -> Self {
        Self {
            answer: Arc::new(answer),
        }
    }

    /// Never answers a report.
    fn quiet() -> Self {
        Self::new(|_| Box::pin(std::future::pending()))
    }

    /// Answers every report at once.
    fn answering() -> Self {
        Self::new(|req| Box::pin(std::future::ready(done_reply(1, &req))))
    }
}

impl ClientRpc for Stub {
    async fn submit(self, _: tarpc::context::Context, request: ClientRequest) -> ClientReply {
        (self.answer)(request).await
    }

    async fn status(self, _: tarpc::context::Context) -> ControlReply {
        ControlReply::Done
    }

    async fn silence(self, _: tarpc::context::Context, _: u64) -> ControlReply {
        ControlReply::Done
    }

    async fn skew_load(self, _: tarpc::context::Context, _: u32) -> ControlReply {
        ControlReply::Done
    }

    async fn incidents(self, _: tarpc::context::Context, _: IncidentQuery) -> ControlReply {
        ControlReply::Done
    }
}

fn done_reply(from: u64, req: &ClientRequest) -> ClientReply {
    ClientReply {
        id: req.id,
        from,
        body: ReplyBody::Done(CommittedResult {
            request: req.id,
            incident: req.incident.clone(),
            executor: from,
            term: 1,
            plan: Plan {
                responder: None,
                cost: 0,
                priority: 200,
            },
            age_ms: req.age_ms,
            assigned_at_ms: 0,
            log_index: 1,
        }),
    }
}

/// Serve `stub` on `socket` until the returned task is aborted or the test ends.
fn serve_stub(socket: UdpSocket, stub: Stub) -> JoinHandle<()> {
    tokio::spawn(datagram::serve(
        Arc::new(socket),
        Arc::default(),
        datagram::CHANNEL_IDLE,
        |_: &tarpc::ClientMessage<ClientRpcRequest>| true,
        move |end: ServerEnd<ClientRpcRequest, ClientRpcResponse>| {
            run_calls(BaseChannel::with_defaults(end).execute(stub.clone().serve()))
        },
    ))
}

async fn stub_at_a_free_port(stub: Stub) -> String {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap().to_string();
    serve_stub(socket, stub);
    addr
}

/// An address nobody listens on: the kernel answers each datagram with an ICMP error.
async fn dead_address() -> SocketAddr {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    socket.local_addr().unwrap()
}

#[tokio::test]
async fn gives_up_with_an_error_after_the_retry_budget() {
    let addr = stub_at_a_free_port(Stub::quiet()).await;
    let mut config = stub_config(&[addr]);
    config.timers.client_retry_ms = 20;
    config.timers.client_retry_max_ms = 40;
    let client = MeshClient::new(9, &config, EventLog::disabled("c"));
    match client.submit(incident()).await {
        Err(ClientError::GaveUp { attempts }) => assert_eq!(attempts, 6),
        other => panic!("expected GaveUp, got {other:?}"),
    }
}

#[tokio::test]
async fn a_node_that_comes_back_on_the_same_address_just_answers() {
    // Bind once to get a free port, then keep the node away.
    let addr = dead_address().await;
    let mut config = stub_config(&[addr.to_string()]);
    config.timers.client_max_retries = 50;
    let client = Arc::new(MeshClient::new(10, &config, EventLog::disabled("c")));
    let submit = {
        let client = client.clone();
        tokio::spawn(async move { client.submit(incident()).await })
    };
    // A datagram that has left, not one still waiting to be combined (D236).
    wait_until(Duration::from_secs(10), "a send into nothing", || {
        client.udp_counts().tx_datagrams >= 1
    })
    .await;

    serve_stub(UdpSocket::bind(addr).await.unwrap(), Stub::answering());
    let done = submit.await.unwrap().unwrap();
    assert_eq!(done.from, 1);
    assert!(done.attempts > 1);
}

#[tokio::test]
async fn a_retry_keeps_the_request_id_and_increments_the_attempt() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = seen.clone();
    let stub = Stub::new(move |req| {
        recorded.lock().unwrap().push((req.id, req.attempt));
        Box::pin(async move {
            if req.attempt < 2 {
                return std::future::pending().await;
            }
            done_reply(1, &req)
        })
    });
    let addr = stub_at_a_free_port(stub).await;
    let mut config = stub_config(&[addr]);
    config.timers.client_retry_ms = 30;
    config.timers.client_retry_max_ms = 60;
    let client = MeshClient::new(13, &config, EventLog::disabled("c"));
    let done = client.submit(incident()).await.unwrap();
    assert_eq!(done.attempts, 3);
    let seen = seen.lock().unwrap();
    let attempts: Vec<u32> = seen.iter().map(|(_, a)| *a).collect();
    assert_eq!(attempts, vec![0, 1, 2]);
    let request = done.committed.unwrap().request;
    assert!(seen.iter().all(|(id, _)| *id == request));
}

#[tokio::test]
async fn each_retry_states_a_larger_age() {
    let ages = Arc::new(Mutex::new(Vec::new()));
    let recorded = ages.clone();
    let stub = Stub::new(move |req| {
        recorded.lock().unwrap().push(req.age_ms);
        Box::pin(std::future::pending())
    });
    let addr = stub_at_a_free_port(stub).await;
    let mut config = stub_config(&[addr]);
    config.timers.client_retry_ms = 20;
    config.timers.client_retry_max_ms = 40;
    let client = MeshClient::new(15, &config, EventLog::disabled("c"));
    assert!(matches!(
        client.submit(incident()).await,
        Err(ClientError::GaveUp { .. })
    ));
    let last_retry = Duration::from_secs(5);
    wait_until(last_retry, "every send at the stub", || {
        ages.lock().unwrap().len() == 6
    })
    .await;
    // In the order they were sent: each attempt's datagram leaves before the next attempt's wait.
    let ages = ages.lock().unwrap();
    // Waits of at least 16 ms between sends, at the lowest jitter.
    assert!(
        ages.windows(2).all(|pair| pair[1] >= pair[0] + 15),
        "{ages:?}"
    );
    assert!(ages[0] < ages[5], "{ages:?}");
}

/// The trap of a send loop that says "Done or Provisional returns, anything else keeps waiting":
/// this reply would be ignored, and a report the cloud has answered would end in `GaveUp`.
#[tokio::test]
async fn an_expired_reply_ends_the_wait_at_once() {
    let stub = Stub::new(|req| {
        Box::pin(std::future::ready(ClientReply {
            id: req.id,
            from: 1,
            body: ReplyBody::Expired(CommittedResult {
                request: req.id,
                incident: req.incident,
                executor: 1,
                term: 1,
                plan: Plan {
                    responder: None,
                    cost: 0,
                    priority: 0,
                },
                age_ms: req.age_ms,
                assigned_at_ms: 0,
                log_index: 1,
            }),
        }))
    });
    let addr = stub_at_a_free_port(stub).await;
    let mut config = stub_config(&[addr]);
    // A retry an hour away: only the reply itself can end this submit in time.
    config.timers.client_retry_ms = 3_600_000;
    config.timers.client_retry_max_ms = 3_600_000;
    let client = MeshClient::new(16, &config, EventLog::disabled("c"));
    let report = Incident {
        ttl_ms: 0,
        ..incident()
    };
    let done = tokio::time::timeout(Duration::from_secs(10), client.submit(report))
        .await
        .expect("an Expired reply must end the wait")
        .unwrap();
    assert_eq!(done.status, IncidentStatus::Expired);
    assert_eq!(done.attempts, 1);
    assert_eq!(done.plan.responder, None);
    assert!(!done.provisional);
    assert_eq!(done.committed.unwrap().request, done.id);
}

/// Calls from earlier attempts stay open (D169). A deadline per attempt would lose this answer and
/// turn every slow commit into another attempt.
#[tokio::test]
async fn a_late_answer_from_an_earlier_attempt_counts() {
    let second_attempt_sent = Arc::new(Notify::new());
    let stub = {
        let second_attempt_sent = second_attempt_sent.clone();
        Stub::new(move |req| {
            let second_attempt_sent = second_attempt_sent.clone();
            Box::pin(async move {
                if req.attempt == 0 {
                    second_attempt_sent.notified().await;
                    return done_reply(100, &req);
                }
                second_attempt_sent.notify_one();
                std::future::pending().await
            })
        })
    };
    let addr = stub_at_a_free_port(stub).await;
    let mut config = stub_config(&[addr]);
    config.timers.client_retry_ms = 30;
    config.timers.client_retry_max_ms = 60;
    let dir = std::env::temp_dir().join(format!("rescuemesh-late-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let log = EventLog::open(Some(&dir), "client-18").unwrap();
    let client = MeshClient::new(18, &config, log.clone());

    let done = client.submit(incident()).await.unwrap();
    assert_eq!(done.from, 100, "the answer to attempt 0");
    assert_eq!(done.attempts, 2);

    drop(client);
    tokio::task::spawn_blocking(move || log.close())
        .await
        .unwrap();
    let text = std::fs::read_to_string(dir.join("client-18.jsonl")).unwrap();
    assert_eq!(text.matches(r#""ev":"client_done""#).count(), 1, "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A quiet node is the normal case under bidding, so it must not cost a user anything.
#[tokio::test]
async fn a_quiet_node_does_not_hold_up_another() {
    let quiet = stub_at_a_free_port(Stub::quiet()).await;
    let answering = stub_at_a_free_port(Stub::answering()).await;
    let mut config = stub_config(&[quiet, answering]);
    // Any retry would show up in the assertion.
    config.timers.client_retry_ms = 3_600_000;
    config.timers.client_retry_max_ms = 3_600_000;
    let client = MeshClient::new(19, &config, EventLog::disabled("c"));
    let done = tokio::time::timeout(Duration::from_secs(10), client.submit(incident()))
        .await
        .expect("the other node's answer must end the wait")
        .unwrap();
    assert_eq!(done.attempts, 1);
}

/// tarpc's own limit is 1000 calls in flight on one stub, and a client program can have that many
/// users waiting on one node at once (D173). A copy lost from this burst is covered by the retry.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn more_than_a_thousand_calls_can_wait_on_one_stub() {
    const USERS: usize = 1500;
    let everyone_here = Arc::new(Notify::new());
    let seen = Arc::new(Mutex::new(HashSet::new()));
    let stub = {
        let (everyone_here, seen) = (everyone_here.clone(), seen.clone());
        Stub::new(move |req| {
            let everyone_here = everyone_here.clone();
            let all_here = {
                let mut seen = seen.lock().unwrap();
                seen.insert(req.id);
                seen.len() == USERS
            };
            Box::pin(async move {
                if all_here {
                    everyone_here.notify_waiters();
                } else {
                    let arrived = everyone_here.notified();
                    let _ = arrived.await;
                }
                done_reply(1, &req)
            })
        })
    };
    let addr = stub_at_a_free_port(stub).await;
    // A call that was in flight when `notify_waiters` fired has its retry to fall back on.
    let mut config = stub_config(&[addr]);
    config.timers = Default::default();
    let client = Arc::new(MeshClient::new(20, &config, EventLog::disabled("c")));
    let mut users = JoinSet::new();
    for _ in 0..USERS {
        let client = client.clone();
        users.spawn(async move { client.submit(incident()).await });
    }
    let mut answered = 0;
    while let Some(outcome) = users.join_next().await {
        outcome.unwrap().unwrap();
        answered += 1;
    }
    assert_eq!(answered, USERS);
    assert!(client.udp_counts().tx_requests >= USERS as u64);
}

/// A node with nobody listening draws ICMP errors, and those must not end the submit (D170).
#[tokio::test]
async fn a_dead_node_does_not_end_a_submit() {
    let dead = dead_address().await.to_string();
    let answering = stub_at_a_free_port(Stub::answering()).await;
    let client = MeshClient::new(
        21,
        &stub_config(&[dead, answering]),
        EventLog::disabled("c"),
    );
    for _ in 0..3 {
        let done = client.submit(incident()).await.unwrap();
        assert_eq!(done.attempts, 1);
    }
    let counts = client.udp_counts();
    assert_eq!(counts.tx_errors, 0, "{counts:?}");
    assert_eq!(counts.rx_bad, 0, "{counts:?}");
}

/// With no calls at all the select has only its timer left: a loop on the empty set of calls would
/// spin here and use up the budget at once.
#[tokio::test]
async fn a_submit_with_no_open_calls_waits_for_its_timer() {
    let mut config = stub_config(&["this is not an address".to_string()]);
    config.timers.client_retry_ms = 20;
    config.timers.client_retry_max_ms = 40;
    let client = MeshClient::new(22, &config, EventLog::disabled("c"));
    let started = tokio::time::Instant::now();
    match client.submit(incident()).await {
        Err(ClientError::GaveUp { attempts }) => assert_eq!(attempts, 6),
        other => panic!("expected GaveUp, got {other:?}"),
    }
    // 20 + 40 x 5 ms, at the lowest jitter.
    assert!(started.elapsed() >= Duration::from_millis(176));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn many_users_share_one_client_under_bidding() {
    let cluster = TestCluster::start_with(3, |c| {
        c.policy.assignment = AssignmentPolicy::Bidding;
    })
    .await;
    ready(&cluster).await;
    let client = Arc::new(MeshClient::new(
        23,
        &cluster.config,
        EventLog::disabled("c"),
    ));
    let mut users = JoinSet::new();
    for _ in 0..50 {
        let client = client.clone();
        users.spawn(async move { client.submit(incident()).await.unwrap().id });
    }
    let mut seen = std::collections::BTreeSet::new();
    while let Some(id) = users.join_next().await {
        assert!(seen.insert(id.unwrap()), "two users got the same request");
    }
    assert_eq!(seen.len(), 50);
    cluster.shutdown().await;
}

/// The answer ends the call on the other nodes, not their work: one node's reply must not cut off
/// another's bid or commit (D169). And the cancels really are sent and counted (D172).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cancel_does_not_stop_the_nodes_work_and_is_counted() {
    let cluster = TestCluster::start_with(3, |c| {
        c.policy.assignment = AssignmentPolicy::Bidding;
    })
    .await;
    ready(&cluster).await;
    let client = MeshClient::new(24, &cluster.config, EventLog::disabled("c"));

    let done = client.submit(incident()).await.unwrap();
    assert_eq!(done.attempts, 1);
    let request = done.id;
    for node in &cluster.nodes {
        wait_until(
            Duration::from_secs(10),
            "the result on every node after the cancels",
            || node.ctx.state.with(|s| s.result(&request).is_some()),
        )
        .await;
    }
    let cancels_read = || {
        cluster
            .nodes
            .iter()
            .map(|n| n.udp_counters.snapshot().rx_cancels)
            .sum::<u64>()
    };
    wait_until(Duration::from_secs(10), "both cancels at the nodes", || {
        client.udp_counts().tx_cancels == 2 && cancels_read() == 2
    })
    .await;
    assert_eq!(client.udp_counts().tx_requests, 3);
    assert_eq!(client.udp_counts().rx_datagrams, 1, "one reply");
    cluster.shutdown().await;
}

/// A stub for one node's client port over UDP, and the dispatch it needs.
fn direct_stub(addr: &str) -> (rescuemesh::proto::ClientRpcClient, JoinSet<()>) {
    let NewClient { client, dispatch } = datagram_client(addr, Arc::default(), None).unwrap();
    let mut dispatches = JoinSet::new();
    dispatches.spawn(dispatch);
    (client, dispatches)
}

fn ctx_for(wait: Duration) -> tarpc::context::Context {
    let mut ctx = tarpc::context::current();
    ctx.deadline = std::time::Instant::now() + wait;
    ctx
}

fn direct_request(seq: u64) -> ClientRequest {
    ClientRequest {
        id: RequestId { client: 25, seq },
        attempt: 0,
        age_ms: 0,
        incident: incident(),
    }
}

/// The fault gate: a silenced node drops report datagrams at the reader and logs none of them, while
/// control still gets through, on TCP for `ctl` and on UDP for the check `ctl status` makes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_silenced_node_drops_submit_and_answers_control() {
    let cluster = TestCluster::start(3).await;
    ready(&cluster).await;
    // A follower answers a decided request from its state (D37), so the same call shows the gate
    // both ways, and its silence moves no leader.
    let leader = cluster.node(1).ctx.consensus.status().leader.unwrap();
    let id = (1..=3).find(|id| *id != leader).unwrap();
    let node = cluster.node(id);
    let decided = common::first_reply(&cluster.config, direct_request(1), Duration::from_secs(10))
        .await
        .expect("a reply");
    assert!(matches!(decided.body, ReplyBody::Done(_)));
    let request = direct_request(1).id;
    wait_until(
        Duration::from_secs(10),
        "the result on the follower",
        || node.ctx.state.with(|s| s.result(&request).is_some()),
    )
    .await;
    let addr = node.client_addr.to_string();
    let (stub, _dispatch) = direct_stub(&addr);
    let ask = |wait| stub.submit(ctx_for(wait), direct_request(1));

    let open = ask(Duration::from_secs(10)).await.unwrap();
    assert!(matches!(open.body, ReplyBody::Done(_)));

    node.ctx.gate.silence();
    let before = node.udp_counters.snapshot();
    let silenced = ask(Duration::from_millis(300)).await;
    assert!(
        matches!(silenced, Err(RpcError::DeadlineExceeded)),
        "{silenced:?}"
    );
    let after = node.udp_counters.snapshot();
    assert_eq!(after.rx_gate_dropped, before.rx_gate_dropped + 1);
    assert_eq!(after.tx_replies, before.tx_replies, "nothing went back");

    // Control passes on both transports.
    let over_udp = stub.status(ctx_for(Duration::from_secs(10))).await.unwrap();
    let ControlReply::Status(status) = over_udp else {
        panic!("expected a status, got {over_udp:?}");
    };
    assert!(status.silenced);
    let over_tcp = control(&addr, ControlRequest::Status, Duration::from_secs(10))
        .await
        .unwrap();
    assert!(matches!(over_tcp, ControlReply::Status(s) if s.silenced));

    node.ctx.gate.restore();
    wait_until(Duration::from_secs(10), "serving again", || {
        node.ctx.serving.load(Ordering::SeqCst)
    })
    .await;
    let back = ask(Duration::from_secs(10)).await.unwrap();
    assert!(matches!(back.body, ReplyBody::Done(_)));
    cluster.shutdown().await;
}

/// A node that lost the bid does not answer with a result (D36): its call stays open with nothing
/// sent, until its deadline here, or the client's cancel in real use (D169).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_node_that_cannot_answer_stays_quiet_until_the_deadline() {
    let cluster = TestCluster::start(3).await;
    ready(&cluster).await;
    let leader = cluster.node(1).ctx.consensus.status().leader.unwrap();
    let follower = (1..=3).find(|id| *id != leader).unwrap();
    let addr = cluster.node(follower).client_addr.to_string();
    let (stub, _dispatch) = direct_stub(&addr);
    // Under leader dispatch a follower is the node that was not picked.
    let quiet = stub
        .submit(ctx_for(Duration::from_millis(400)), direct_request(1))
        .await;
    assert!(
        matches!(quiet, Err(RpcError::DeadlineExceeded)),
        "{quiet:?}"
    );
    assert_eq!(cluster.node(follower).udp_counters.snapshot().tx_replies, 0);
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_node_answers_control_over_tcp_with_every_method() {
    let cluster = TestCluster::start(1).await;
    ready(&cluster).await;
    let addr = cluster.node(1).client_addr.to_string();
    let wait = Duration::from_secs(10);
    let status = control(&addr, ControlRequest::Status, wait).await.unwrap();
    assert!(matches!(status, ControlReply::Status(_)));
    let skew = ControlRequest::SkewLoad { extra_work: 7 };
    assert_eq!(
        control(&addr, skew, wait).await.unwrap(),
        ControlReply::Done
    );
    assert_eq!(cluster.node(1).ctx.plan_work.load(Ordering::SeqCst), 7);
    let query = IncidentQuery::OfClient {
        client: 1,
        limit: 1,
    };
    let rows = control(&addr, ControlRequest::Incidents(query), wait)
        .await
        .unwrap();
    assert!(matches!(rows, ControlReply::Incidents { .. }));
    let silence = ControlRequest::Silence { ms: 50 };
    assert_eq!(
        control(&addr, silence, wait).await.unwrap(),
        ControlReply::Done
    );
    cluster.shutdown().await;
}

/// A control server over TCP only: what a node looks like with the UDP firewall rule missing.
async fn tcp_only_stub() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    tokio::spawn(async move {
        let mut connections = JoinSet::new();
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let transport = tarpc::serde_transport::new(
                framed(stream),
                tarpc::tokio_serde::formats::Bincode::default(),
            );
            let calls = BaseChannel::with_defaults(transport).execute(Stub::quiet().serve());
            connections.spawn(run_calls(calls));
        }
    });
    addr
}

async fn run_ctl_status(config_text: &str) -> String {
    let path = std::env::temp_dir().join(format!(
        "rescuemesh-ctl-{}-{}.toml",
        std::process::id(),
        NEXT_FILE.fetch_add(1, Ordering::SeqCst)
    ));
    tokio::fs::write(&path, config_text).await.unwrap();
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_ctl"))
        .arg("--config")
        .arg(&path)
        .arg("status")
        .output()
        .await
        .unwrap();
    let _ = tokio::fs::remove_file(&path).await;
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout).unwrap()
}

static NEXT_FILE: AtomicUsize = AtomicUsize::new(0);

fn config_text(client_addrs: &[String]) -> String {
    client_addrs
        .iter()
        .enumerate()
        .map(|(i, addr)| {
            format!(
                "[[cluster.nodes]]\nid = {}\npeer_addr = \"127.0.0.1:{}\"\nclient_addr = \"{addr}\"\n",
                i + 1,
                1 + i
            )
        })
        .collect()
}

/// The first check in the runbook. Over TCP alone, a missing firewall rule looks like a healthy node
/// whose copies vanish (D168). This runs the real `ctl`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ctl_status_says_when_the_udp_port_does_not_answer() {
    let tcp_only = tcp_only_stub().await;
    assert!(!udp_answers(&tcp_only, Duration::from_millis(300)).await);

    let cluster = TestCluster::start(1).await;
    ready(&cluster).await;
    let working = cluster.node(1).client_addr.to_string();
    assert!(udp_answers(&working, Duration::from_secs(10)).await);

    let stdout = run_ctl_status(&config_text(&[working, tcp_only])).await;
    assert!(stdout.contains("node 1: udp ok"), "{stdout}");
    assert!(stdout.contains("node 2: udp no answer"), "{stdout}");
    assert!(stdout.contains("/udp"), "{stdout}");
    cluster.shutdown().await;
}

/// Silenced is not the same as a closed port, and `ctl status` must not make it look so.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ctl_status_says_udp_ok_for_a_silenced_node() {
    let cluster = TestCluster::start(1).await;
    ready(&cluster).await;
    cluster.node(1).ctx.gate.silence();
    let addr = cluster.node(1).client_addr.to_string();
    let stdout = run_ctl_status(&config_text(&[addr])).await;
    assert!(stdout.contains("node 1: udp ok"), "{stdout}");
    cluster.node(1).ctx.gate.restore();
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_fresh_report_gets_a_responder_and_reads_assigned() {
    let cluster = TestCluster::start(3).await;
    ready(&cluster).await;
    let node = cluster.node(1);
    wait_until(Duration::from_secs(10), "the campus registered", || {
        node.ctx.status().responders > 0
    })
    .await;
    let dir = std::env::temp_dir().join(format!("rescuemesh-fresh-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let log = EventLog::open(Some(&dir), "client-17").unwrap();
    let client = MeshClient::new(17, &cluster.config, log.clone());

    // Guards the age's units: milliseconds read as microseconds, or the wrong clock, would refuse
    // every report and look like a very fast run in which nobody is ever sent.
    let done = client.submit(incident()).await.unwrap();
    assert!(done.plan.responder.is_some());
    assert_eq!(done.status, IncidentStatus::Assigned);
    assert!(done.plan.priority >= 200, "severity 2 alone is 200 points");
    let result = done.committed.unwrap();
    assert!(!result.too_old_on_arrival());
    assert!(result.age_ms < 10_000, "age {} ms", result.age_ms);
    assert!(result.assigned_at_ms >= result.incident.reported_at_ms);

    drop(client);
    tokio::task::spawn_blocking(move || log.close())
        .await
        .unwrap();
    let text = std::fs::read_to_string(dir.join("client-17.jsonl")).unwrap();
    assert!(text.contains(r#""status":"assigned""#), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bidding_serves_a_client_with_a_node_silenced() {
    let cluster = TestCluster::start_with(3, |c| {
        c.policy.assignment = rescuemesh::config::AssignmentPolicy::Bidding;
        fast_retries(c);
    })
    .await;
    ready(&cluster).await;
    cluster.node(3).ctx.gate.silence();
    let client = MeshClient::new(14, &cluster.config, EventLog::disabled("c"));
    for _ in 0..5 {
        let done = client.submit(incident()).await.unwrap();
        assert_ne!(done.executor, 3);
    }
    cluster.node(3).ctx.gate.restore();
    cluster.shutdown().await;
}
