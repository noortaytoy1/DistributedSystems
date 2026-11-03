//! Incident records on real nodes: what a report reads as over its life, and how a report too old
//! to plan is refused (D142, D143).
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::{TestCluster, first_reply, wait_until};
use rescuemesh::client::MeshClient;
use rescuemesh::config::{AssignmentPolicy, Config};
use rescuemesh::metrics::{EventLog, now_ms};
use rescuemesh::proto::{
    Capability, Category, ClientRequest, CommittedResult, Incident, IncidentStatus, Point,
    ReplyBody, RequestId, UserId,
};

const PATIENCE: Duration = Duration::from_secs(15);

fn incident(severity: u8, ttl_ms: u64) -> Incident {
    Incident {
        owner: UserId {
            client: 70,
            user: 1,
        },
        category: Category::MedicalAssistance,
        zone: 0,
        location: Point { x: 10, y: 20 },
        severity,
        needs: Capability::Medical,
        reported_at_ms: now_ms(),
        ttl_ms,
    }
}

/// A raw request, so a test can state any age.
fn request(seq: u64, attempt: u32, age_ms: u64, ttl_ms: u64) -> ClientRequest {
    ClientRequest {
        id: RequestId { client: 70, seq },
        attempt,
        age_ms,
        incident: incident(2, ttl_ms),
    }
}

/// Bidding on, as in a real run, and a leader on every node. Waits for `responders` on each node.
async fn start(responders: u64, tweak: impl FnOnce(&mut Config)) -> TestCluster {
    let cluster = TestCluster::start_with(3, |c| {
        c.policy.assignment = AssignmentPolicy::Bidding;
        tweak(c);
    })
    .await;
    wait_until(PATIENCE, "a leader and the campus on every node", || {
        cluster.nodes.iter().all(|n| {
            n.ctx.consensus.status().leader.is_some() && n.ctx.status().responders == responders
        })
    })
    .await;
    cluster
}

fn free_everywhere(cluster: &TestCluster, total: usize) -> bool {
    cluster
        .nodes
        .iter()
        .all(|n| n.ctx.state.with(|s| s.free_responders().len()) == total)
}

fn status_on(cluster: &TestCluster, node: u64, id: &RequestId) -> Option<IncidentStatus> {
    let ctx = &cluster.node(node).ctx;
    ctx.state.with(|s| s.status_at(id, now_ms()))
}

fn expired_record(body: &ReplyBody) -> &CommittedResult {
    match body {
        ReplyBody::Expired(result) => result,
        other => panic!("expected Expired, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_report_reads_assigned_then_resolved_on_all_three_nodes() {
    // Severity 2 × 1500 ms: long enough to read Assigned everywhere before the release commits.
    let cluster = start(30, |c| c.campus.service_ms_per_severity = 1_500).await;
    let client = MeshClient::new(71, &cluster.config, EventLog::disabled("c"));
    let done = client.submit(incident(2, 60_000)).await.unwrap();
    assert_eq!(done.status, IncidentStatus::Assigned);
    assert!(done.plan.responder.is_some());

    wait_until(PATIENCE, "Assigned on all three nodes", || {
        (1..=3).all(|n| status_on(&cluster, n, &done.id) == Some(IncidentStatus::Assigned))
    })
    .await;
    wait_until(PATIENCE, "Resolved on all three nodes", || {
        (1..=3).all(|n| status_on(&cluster, n, &done.id) == Some(IncidentStatus::Resolved))
    })
    .await;
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_too_old_report_commits_nobody_with_responders_free() {
    let dir = std::env::temp_dir().join(format!("rescuemesh-expired-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let metrics = dir.clone();
    let cluster = start(30, |c| c.metrics.dir = Some(metrics)).await;

    let reply = first_reply(&cluster.config, request(1, 0, 1_000, 1_000), PATIENCE)
        .await
        .expect("a refusal is still an answer");
    let record = expired_record(&reply.body);
    assert_eq!(record.plan.responder, None);
    assert_eq!(record.plan.cost, 0);
    assert_eq!(record.age_ms, 1_000);
    assert_eq!(record.status_at_commit(), IncidentStatus::Expired);

    // Committed like any result, on every node, and nobody was taken.
    wait_until(PATIENCE, "the record on all three nodes", || {
        (1..=3).all(|n| status_on(&cluster, n, &record.request) == Some(IncidentStatus::Expired))
    })
    .await;
    assert!(free_everywhere(&cluster, 30));

    let executor = record.executor;
    let log = dir.join(format!("node-{executor}.jsonl"));
    let node = cluster.node(executor);
    wait_until(PATIENCE, "report_expired in the executor's log", || {
        node.ctx.log.flush();
        std::fs::read_to_string(&log).is_ok_and(|text| {
            text.contains(r#""ev":"report_expired""#) && text.contains(r#""ttl_ms":1000"#)
        })
    })
    .await;
    cluster.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_report_one_millisecond_younger_than_its_ttl_is_planned() {
    let cluster = start(30, |_| {}).await;
    let reply = first_reply(&cluster.config, request(1, 0, 999, 1_000), PATIENCE)
        .await
        .expect("a reply");
    let ReplyBody::Done(result) = reply.body else {
        panic!("expected Done, got {:?}", reply.body);
    };
    assert!(result.plan.responder.is_some());
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_zero_ttl_is_refused_through_the_client_without_a_retry() {
    // Retries far away, so only the refusal can end this submit, however busy the machine is.
    let cluster = start(30, |c| {
        c.timers.client_retry_ms = 60_000;
        c.timers.client_retry_max_ms = 60_000;
    })
    .await;
    let client = MeshClient::new(72, &cluster.config, EventLog::disabled("c"));
    let done = client.submit(incident(5, 0)).await.unwrap();
    assert_eq!(done.status, IncidentStatus::Expired);
    assert_eq!(done.attempts, 1);
    assert_eq!(done.plan.responder, None);
    assert!(!done.provisional);
    assert!(free_everywhere(&cluster, 30));
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_retry_of_a_refused_report_gets_the_same_body() {
    let cluster = start(30, |_| {}).await;
    let first = first_reply(&cluster.config, request(1, 0, 1_000, 1_000), PATIENCE)
        .await
        .expect("a reply");
    expired_record(&first.body);
    // Even a retry stating a young age: the first commit stands.
    let retry = first_reply(&cluster.config, request(1, 1, 0, 1_000), PATIENCE)
        .await
        .expect("a reply to the retry");
    assert_eq!(retry.body, first.body);
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_report_planned_while_young_is_not_refused_by_a_late_retry() {
    let cluster = start(30, |_| {}).await;
    let first = first_reply(&cluster.config, request(1, 0, 0, 1_000), PATIENCE)
        .await
        .expect("a reply");
    assert!(matches!(first.body, ReplyBody::Done(_)));
    let late = first_reply(&cluster.config, request(1, 1, 5_000, 1_000), PATIENCE)
        .await
        .expect("a reply to the retry");
    assert_eq!(late.body, first.body);
    assert!(status_on(&cluster, late.from, &late.id).is_some());
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_retry_that_arrives_too_late_is_refused() {
    let cluster = start(30, |c| {
        c.timers.client_retry_ms = 300;
        c.timers.client_retry_max_ms = 600;
        c.timers.client_max_retries = 50;
    })
    .await;
    for node in &cluster.nodes {
        node.ctx.gate.silence();
    }
    let client = MeshClient::new(73, &cluster.config, EventLog::disabled("c"));
    let ttl_ms = 900;
    let report = incident(2, ttl_ms);
    let submit = tokio::spawn(async move {
        let done = client.submit(report).await;
        (done, client)
    });

    // Nobody hears the sends while the report is young.
    let started = tokio::time::Instant::now();
    wait_until(PATIENCE, "the report to outlive its TTL", || {
        started.elapsed() > Duration::from_millis(ttl_ms + 100)
    })
    .await;
    for node in &cluster.nodes {
        node.ctx.gate.restore();
    }

    let (done, _client) = submit.await.unwrap();
    let done = done.unwrap();
    assert_eq!(done.status, IncidentStatus::Expired);
    assert!(done.attempts > 1, "the first send was not heard");
    let result = done.committed.unwrap();
    assert!(result.age_ms >= ttl_ms, "stated age {} ms", result.age_ms);
    assert!(free_everywhere(&cluster, 30));
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nobody_free_reads_unassigned_then_expired_with_no_log_entry() {
    let cluster = start(0, |c| c.campus.responders = 0).await;
    let client = MeshClient::new(74, &cluster.config, EventLog::disabled("c"));
    let ttl_ms = 1_000;
    let done = client.submit(incident(2, ttl_ms)).await.unwrap();
    assert_eq!(done.status, IncidentStatus::Unassigned);
    let result = done.committed.as_ref().unwrap();
    assert_eq!(result.plan.responder, None);
    assert!(!result.too_old_on_arrival());

    wait_until(PATIENCE, "the record on all three nodes", || {
        (1..=3).all(|n| status_on(&cluster, n, &done.id).is_some())
    })
    .await;
    // At explicit times, so a slow machine cannot change what these read.
    for node in &cluster.nodes {
        node.ctx.state.with(|s| {
            let at = |now_ms| s.status_at(&done.id, now_ms);
            assert_eq!(
                at(result.assigned_at_ms),
                Some(IncidentStatus::Unassigned),
                "node {}",
                node.ctx.id
            );
            assert_eq!(
                at(result.deadline_ms()),
                Some(IncidentStatus::Expired),
                "node {}",
                node.ctx.id
            );
        });
    }
    let positions = |cluster: &TestCluster| -> Vec<(u64, u64)> {
        cluster
            .nodes
            .iter()
            .map(|n| n.ctx.state.with(|s| (s.last_index(), s.digest())))
            .collect()
    };
    let before = positions(&cluster);
    wait_until(PATIENCE, "Expired on all three nodes", || {
        (1..=3).all(|n| status_on(&cluster, n, &done.id) == Some(IncidentStatus::Expired))
    })
    .await;
    // Past the deadline by the clock alone: nothing went through the log.
    assert_eq!(positions(&cluster), before);
    cluster.shutdown().await;
}
