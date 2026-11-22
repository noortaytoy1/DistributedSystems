//! Part K: offline operation of a client against three real nodes (Phase 2, D400–D406). Reports
//! made while offline wait in the outbox, keep their IDs, and reach every node on reconnect.
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::{TestCluster, hold_jobs, wait_until};
use rescuemesh::client::{ClientError, MeshClient};
use rescuemesh::metrics::{EventLog, now_ms};
use rescuemesh::offline::Delivery;
use rescuemesh::proto::{Capability, Category, Incident, IncidentStatus, Point, RequestId, UserId};
use tokio::time::Instant;

const WAIT: Duration = Duration::from_secs(10);

fn incident(client: u64, ttl_ms: u64) -> Incident {
    Incident {
        owner: UserId { client, user: 0 },
        category: Category::MedicalAssistance,
        zone: 0,
        location: Point { x: 10, y: 20 },
        severity: 2,
        needs: Capability::Medical,
        reported_at_ms: now_ms(),
        ttl_ms,
    }
}

async fn ready(cluster: &TestCluster) {
    wait_until(WAIT, "a leader on every node", || {
        cluster
            .nodes
            .iter()
            .all(|n| n.ctx.consensus.status().leader.is_some())
    })
    .await;
}

fn on_every_node(cluster: &TestCluster, id: RequestId) -> bool {
    cluster
        .nodes
        .iter()
        .all(|n| n.ctx.state.with(|s| s.result(&id).is_some()))
}

fn on_any_node(cluster: &TestCluster, id: RequestId) -> bool {
    cluster
        .nodes
        .iter()
        .any(|n| n.ctx.state.with(|s| s.result(&id).is_some()))
}

async fn queue(client: &MeshClient, incident: Incident) -> RequestId {
    match client.submit_or_queue(incident).await.unwrap() {
        Delivery::Queued(id) => id,
        Delivery::Answered(done) => panic!("answered while offline: {done:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reports_made_offline_reach_every_node_with_their_ids() {
    let cluster = TestCluster::start(3).await;
    ready(&cluster).await;
    let client = MeshClient::new(41, &cluster.config, EventLog::disabled("client-41"));

    client.go_offline();
    assert!(client.is_offline());
    let started = Instant::now();
    assert!(matches!(
        client.submit(incident(41, 60_000)).await,
        Err(ClientError::Offline)
    ));
    assert!(
        started.elapsed() < Duration::from_millis(100),
        "fails at once"
    );
    let mut ids = Vec::new();
    for _ in 0..5 {
        ids.push(queue(&client, incident(41, 60_000)).await);
    }
    assert_eq!(client.queued().await, 5);
    assert!(ids.iter().all(|&id| !on_any_node(&cluster, id)));

    let done = client.go_online().await.unwrap();
    assert_eq!(
        (done.sent, done.answered, done.expired, done.failed),
        (5, 5, 0, 0)
    );
    assert_eq!(client.queued().await, 0);
    wait_until(WAIT, "every queued report on every node", || {
        ids.iter().all(|&id| on_every_node(&cluster, id))
    })
    .await;

    // Online again, the same call answers at once.
    match client.submit_or_queue(incident(41, 60_000)).await.unwrap() {
        Delivery::Answered(done) => assert!(done.committed.is_some()),
        Delivery::Queued(id) => panic!("queued {id} while online"),
    }
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_report_too_old_by_reconnect_is_refused_on_record() {
    let cluster = TestCluster::start(3).await;
    ready(&cluster).await;
    let client = MeshClient::new(42, &cluster.config, EventLog::disabled("client-42"));

    client.go_offline();
    let mut old = incident(42, 1_000);
    old.reported_at_ms = now_ms() - 10_000;
    let id = queue(&client, old).await;

    let done = client.go_online().await.unwrap();
    assert_eq!((done.sent, done.expired, done.failed), (1, 1, 0));
    assert_eq!(client.queued().await, 0);
    wait_until(WAIT, "the refusal on every node", || {
        on_every_node(&cluster, id)
    })
    .await;
    let record = cluster.node(1).ctx.state.with(|s| s.result(&id).cloned());
    assert_eq!(record.unwrap().status_at_commit(), IncidentStatus::Expired);
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_outbox_outlives_the_program() {
    let cluster = TestCluster::start_with(3, |c| {
        let dir = std::env::temp_dir().join(format!("rescuemesh-offline-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join("outbox-client-43.jsonl"));
        c.metrics.dir = Some(dir);
    })
    .await;
    ready(&cluster).await;

    let first = MeshClient::new(43, &cluster.config, EventLog::disabled("client-43"));
    first.go_offline();
    let mut ids = Vec::new();
    for _ in 0..3 {
        ids.push(queue(&first, incident(43, 60_000)).await);
    }
    drop(first);

    // A new program with the same client ID finds the reports and sends them.
    let second = MeshClient::new(43, &cluster.config, EventLog::disabled("client-43"));
    assert_eq!(second.queued().await, 3);
    let done = second.flush_outbox().await.unwrap();
    assert_eq!((done.sent, done.answered), (3, 3));
    wait_until(WAIT, "the old program's reports on every node", || {
        ids.iter().all(|&id| on_every_node(&cluster, id))
    })
    .await;
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_answer_on_its_way_is_lost_while_offline() {
    let cluster = TestCluster::start_with(3, |c| {
        c.timers.client_retry_ms = 300;
        c.timers.client_retry_max_ms = 600;
    })
    .await;
    ready(&cluster).await;
    // Every worker busy, so nothing is planned until the client is offline.
    let mut held = Vec::new();
    for node in &cluster.nodes {
        let threads = node.ctx.workers.load().threads;
        held.push(hold_jobs(node, threads).await);
    }
    let client = std::sync::Arc::new(MeshClient::new(
        44,
        &cluster.config,
        EventLog::disabled("client-44"),
    ));
    let sender = client.clone();
    let report = tokio::spawn(async move { sender.submit_or_queue(incident(44, 60_000)).await });
    wait_until(WAIT, "the report to reach the nodes", || {
        client.udp_counts().tx_requests >= 3
    })
    .await;
    client.go_offline();
    drop(held);

    // The answer that comes meanwhile is dropped, so the report ends in the outbox, same ID.
    let id = match report.await.unwrap().unwrap() {
        Delivery::Queued(id) => id,
        Delivery::Answered(done) => panic!("heard an answer while offline: {done:?}"),
    };
    wait_until(
        WAIT,
        "the cloud to record it while the client is away",
        || on_every_node(&cluster, id),
    )
    .await;
    // Back online, the resend is answered from that record, not planned again.
    let done = client.go_online().await.unwrap();
    assert_eq!((done.sent, done.answered), (1, 1));
    cluster.shutdown().await;
}
