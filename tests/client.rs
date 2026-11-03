//! The client middleware against real nodes, and against stub nodes for what real nodes cannot
//! be made to do on demand (going away and coming back on the same address).
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use bytes::Bytes;
use common::{TestCluster, wait_until};
use futures::{SinkExt, StreamExt};
use rescuemesh::client::{ClientError, MeshClient};
use rescuemesh::config::{ClusterConfig, Config, NodeAddr};
use rescuemesh::metrics::EventLog;
use rescuemesh::node::transport::framed;
use rescuemesh::proto::{
    self, Capability, Category, ClientFrame, ClientReply, CommittedResult, ControlReply, Incident,
    Plan, Point, ReplyBody, ServerFrame, UserId,
};
use tokio::net::TcpListener;
use tokio::task::JoinSet;

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

/// A node stand-in: answers pings if asked to, counts connections, and answers requests.
async fn serve_stub(
    listener: TcpListener,
    answer_pings: bool,
    answer_requests: bool,
    connections: Arc<AtomicU32>,
) {
    loop {
        let (stream, _) = listener.accept().await.unwrap();
        connections.fetch_add(1, Ordering::SeqCst);
        tokio::spawn(async move {
            let mut conn = framed(stream);
            while let Some(Ok(frame)) = conn.next().await {
                let answer = match proto::decode::<ClientFrame>(&frame).unwrap() {
                    ClientFrame::Control(_) if answer_pings => {
                        Some(ServerFrame::Control(ControlReply::Done))
                    }
                    ClientFrame::Request(req) if answer_requests => {
                        Some(ServerFrame::Reply(ClientReply {
                            id: req.id,
                            from: 1,
                            body: ReplyBody::Done(CommittedResult {
                                request: req.id,
                                incident: req.incident,
                                executor: 1,
                                term: 1,
                                plan: Plan {
                                    responder: None,
                                    cost: 0,
                                },
                                log_index: 1,
                            }),
                        }))
                    }
                    _ => None,
                };
                if let Some(answer) = answer {
                    let bytes = Bytes::from(proto::encode(&answer).unwrap());
                    if conn.send(bytes).await.is_err() {
                        return;
                    }
                }
            }
        });
    }
}

#[tokio::test]
async fn gives_up_with_an_error_after_the_retry_budget() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    tokio::spawn(serve_stub(listener, true, false, Arc::default()));
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
async fn reconnects_when_a_node_comes_back_on_the_same_address() {
    // Bind once to get a free port, then keep the node away.
    let first = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = first.local_addr().unwrap();
    drop(first);
    let mut config = stub_config(&[addr.to_string()]);
    config.timers.client_max_retries = 50;
    let client = MeshClient::new(10, &config, EventLog::disabled("c"));
    let submit = tokio::spawn(async move { client.submit(incident()).await });

    let connections = Arc::new(AtomicU32::new(0));
    let back = TcpListener::bind(addr).await.unwrap();
    tokio::spawn(serve_stub(back, true, true, connections.clone()));
    let done = submit.await.unwrap().unwrap();
    assert_eq!(done.from, 1);
    assert!(connections.load(Ordering::SeqCst) >= 1);
}

#[tokio::test]
async fn a_connection_that_stops_answering_pings_is_replaced() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let connections = Arc::new(AtomicU32::new(0));
    tokio::spawn(serve_stub(listener, false, false, connections.clone()));
    let config = stub_config(&[addr]);
    let _client = MeshClient::new(11, &config, EventLog::disabled("c"));
    wait_until(Duration::from_secs(10), "a second connection", || {
        connections.load(Ordering::SeqCst) >= 2
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_silenced_node_keeps_its_connection_because_pings_pass_the_gate() {
    let cluster = TestCluster::start(3).await;
    ready(&cluster).await;
    let dir = std::env::temp_dir().join(format!("rescuemesh-ping-{}", std::process::id()));
    let log = EventLog::open(Some(&dir), "client-12").unwrap();
    let client = MeshClient::new(12, &cluster.config, log.clone());
    cluster.node(2).ctx.gate.silence();
    let path = dir.join("client-12.jsonl");
    // Longer than the ping timeout: a dropped link would have been logged by then.
    let started = tokio::time::Instant::now();
    wait_until(Duration::from_secs(10), "the log to cover 4 s", || {
        started.elapsed() > Duration::from_secs(4)
    })
    .await;
    drop(client);
    tokio::task::spawn_blocking(move || log.close())
        .await
        .unwrap();
    let text = std::fs::read_to_string(path).unwrap_or_default();
    assert!(!text.contains("client_link_down"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
    cluster.node(2).ctx.gate.restore();
    cluster.shutdown().await;
}

#[tokio::test]
async fn a_retry_keeps_the_request_id_and_increments_the_attempt() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = seen.clone();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut conn = framed(stream);
        while let Some(Ok(frame)) = conn.next().await {
            let ClientFrame::Request(req) = proto::decode::<ClientFrame>(&frame).unwrap() else {
                continue;
            };
            recorded.lock().unwrap().push((req.id, req.attempt));
            if req.attempt < 2 {
                continue;
            }
            let answer = ServerFrame::Reply(ClientReply {
                id: req.id,
                from: 1,
                body: ReplyBody::Done(CommittedResult {
                    request: req.id,
                    incident: req.incident,
                    executor: 1,
                    term: 1,
                    plan: Plan {
                        responder: None,
                        cost: 0,
                    },
                    log_index: 1,
                }),
            });
            let bytes = Bytes::from(proto::encode(&answer).unwrap());
            conn.send(bytes).await.unwrap();
        }
    });
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
