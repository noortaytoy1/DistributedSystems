//! Degraded mode: the last server left answers without a commit, and commits it once a peer
//! returns (D120-D124).
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::{TestCluster, first_reply, wait_until};
use rescuemesh::client::MeshClient;
use rescuemesh::config::{AssignmentPolicy, Config};
use rescuemesh::metrics::{EventLog, now_ms};
use rescuemesh::proto::{
    Capability, Category, ClientRequest, Command, Incident, IncidentStatus, Plan, Point, ReplyBody,
    RequestId, ResultProposal, UserId,
};

const PATIENCE: Duration = Duration::from_secs(15);

fn incident() -> Incident {
    Incident {
        owner: UserId {
            client: 90,
            user: 0,
        },
        category: Category::MedicalAssistance,
        zone: 0,
        location: Point { x: 10, y: 20 },
        severity: 1,
        needs: Capability::Medical,
        reported_at_ms: now_ms(),
        ttl_ms: 60_000,
    }
}

fn request(seq: u64) -> ClientRequest {
    ClientRequest {
        id: RequestId { client: 90, seq },
        attempt: 0,
        age_ms: 0,
        incident: incident(),
    }
}

fn quick(c: &mut Config) {
    c.policy.assignment = AssignmentPolicy::Bidding;
    c.timers.degraded_after_ms = 300;
    c.timers.client_retry_ms = 300;
    c.timers.client_retry_max_ms = 600;
}

/// A cluster with a leader and its responders on every node.
async fn start(tweak: impl FnOnce(&mut Config)) -> TestCluster {
    let cluster = TestCluster::start_with(3, |c| {
        quick(c);
        tweak(c);
    })
    .await;
    wait_until(PATIENCE, "a leader and responders everywhere", || {
        cluster
            .nodes
            .iter()
            .all(|n| n.ctx.consensus.status().leader.is_some() && n.ctx.status().responders > 0)
    })
    .await;
    cluster
}

fn leader_of(cluster: &TestCluster) -> u64 {
    cluster
        .nodes
        .iter()
        .find_map(|n| n.ctx.consensus.status().leader)
        .unwrap()
}

/// Silence everyone but `lone` and wait until it is degraded.
async fn leave_alone(cluster: &TestCluster, lone: u64) {
    for id in (1..=3).filter(|id| *id != lone) {
        cluster.node(id).ctx.gate.silence();
    }
    let node = cluster.node(lone);
    wait_until(PATIENCE, "the last node to go degraded", || {
        node.ctx.status().degraded
    })
    .await;
}

fn restore_others(cluster: &TestCluster, lone: u64) {
    for id in (1..=3).filter(|id| *id != lone) {
        cluster.node(id).ctx.gate.restore();
    }
}

async fn digests_match(cluster: &TestCluster) {
    wait_until(PATIENCE, "equal digests on every node", || {
        let status: Vec<_> = cluster.nodes.iter().map(|n| n.ctx.status()).collect();
        status.iter().all(|s| {
            s.digest == status[0].digest && s.last_applied == status[0].last_applied && s.serving
        })
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_last_follower_answers_through_the_client_and_commits_on_return() {
    let cluster = start(|_| {}).await;
    let leader = leader_of(&cluster);
    let lone = (1..=3).find(|id| *id != leader).unwrap();
    leave_alone(&cluster, lone).await;

    let client = MeshClient::new(91, &cluster.config, EventLog::disabled("c"));
    let done = client.submit(incident()).await.unwrap();
    assert!(done.provisional);
    assert!(done.committed.is_none());
    assert_eq!((done.from, done.executor), (lone, lone));
    assert!(done.plan.responder.is_some());

    restore_others(&cluster, lone);
    let node = cluster.node(lone);
    wait_until(PATIENCE, "degraded mode to end and the replay", || {
        !node.ctx.status().degraded && node.ctx.degraded.pending() == 0
    })
    .await;
    // Settled means the leader committed it; this follower may not have applied it yet.
    wait_until(PATIENCE, "the result on this node", || {
        node.ctx.status().results == 1
    })
    .await;
    digests_match(&cluster).await;

    // Back to normal: the next answer is committed.
    let next = client.submit(incident()).await.unwrap();
    assert!(!next.provisional);
    assert!(next.committed.is_some());
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_last_leader_answers_too_and_a_retry_gets_the_same_plan() {
    let cluster = start(|c| c.policy.assignment = AssignmentPolicy::LeaderDispatch).await;
    let lone = leader_of(&cluster);
    leave_alone(&cluster, lone).await;

    let ask = |attempt| {
        let mut req = request(1);
        req.attempt = attempt;
        first_reply(&cluster.config, req, Duration::from_secs(5))
    };
    let first = ask(0).await.unwrap();
    let again = ask(1).await.unwrap();
    assert!(matches!(first.body, ReplyBody::Provisional(_)));
    assert_eq!(first.body, again.body);
    assert_eq!(cluster.node(lone).ctx.degraded.pending(), 1);

    restore_others(&cluster, lone);
    digests_match(&cluster).await;
    let node = cluster.node(lone);
    wait_until(PATIENCE, "the result on the record", || {
        node.ctx.state.with(|s| s.result(&request(1).id).is_some())
    })
    .await;
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn each_answer_takes_a_different_responder_while_they_are_held() {
    let cluster = start(|c| c.campus.service_ms_per_severity = 60_000).await;
    let lone = 1;
    leave_alone(&cluster, lone).await;

    let mut taken = std::collections::BTreeSet::new();
    for seq in 0..5 {
        let reply = first_reply(&cluster.config, request(seq), Duration::from_secs(5))
            .await
            .unwrap();
        let ReplyBody::Provisional(Plan { responder, .. }) = reply.body else {
            panic!("expected a provisional answer, got {:?}", reply.body);
        };
        assert!(taken.insert(responder.unwrap()), "a responder given twice");
    }
    restore_others(&cluster, lone);
    digests_match(&cluster).await;
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_silenced_node_never_makes_the_others_degraded() {
    let cluster = start(|_| {}).await;
    cluster.node(3).ctx.gate.silence();
    let others = [cluster.node(1), cluster.node(2)];
    wait_until(PATIENCE, "the silenced node to be suspected", || {
        others
            .iter()
            .all(|n| n.ctx.suspicion.lock().unwrap().is_suspected(3))
    })
    .await;

    // Well past the 300 ms wait, a request is still committed and nobody is degraded.
    let client = MeshClient::new(92, &cluster.config, EventLog::disabled("c"));
    for _ in 0..10 {
        let done = client.submit(incident()).await.unwrap();
        assert!(!done.provisional);
        assert!(others.iter().all(|n| !n.ctx.status().degraded));
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(!cluster.node(3).ctx.status().degraded);
    cluster.node(3).ctx.gate.restore();
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_silenced_node_is_never_degraded_though_it_hears_nobody() {
    let cluster = start(|_| {}).await;
    for id in 1..=3 {
        cluster.node(id).ctx.gate.silence();
    }
    let client = MeshClient::new(93, &cluster.config, EventLog::disabled("c"));
    let attempt = tokio::time::timeout(Duration::from_millis(800), client.submit(incident())).await;
    assert!(
        attempt.is_err(),
        "every node is silenced: nobody may answer"
    );
    assert!(cluster.nodes.iter().all(|n| !n.ctx.status().degraded));
    for id in 1..=3 {
        cluster.node(id).ctx.gate.restore();
    }
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn losing_the_first_commit_logs_a_conflict_and_keeps_the_winner() {
    let dir = std::env::temp_dir().join(format!("rescuemesh-degraded-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let metrics = dir.clone();
    let cluster = start(|c| c.metrics.dir = Some(metrics)).await;
    let lone = 3;
    leave_alone(&cluster, lone).await;
    let reply = first_reply(&cluster.config, request(1), Duration::from_secs(5))
        .await
        .unwrap();
    let ReplyBody::Provisional(provisional) = reply.body else {
        panic!("expected a provisional answer, got {:?}", reply.body);
    };

    // A partition heals: the other side committed its own result for the same request first.
    cluster.node(lone).ctx.gate.silence();
    restore_others(&cluster, lone);
    let winner = Plan {
        responder: None,
        cost: provisional.cost + 1,
        priority: provisional.priority,
    };
    let other = cluster.node(1);
    wait_until(PATIENCE, "a leader on the other side", || {
        let status = other.ctx.consensus.status();
        status.leader.is_some_and(|l| l != lone) && other.ctx.status().serving
    })
    .await;
    let req = request(1);
    let proposal = ResultProposal {
        request: req.id,
        incident: req.incident,
        executor: 1,
        term: other.ctx.consensus.status().term,
        plan: winner.clone(),
        age_ms: req.age_ms,
        assigned_at_ms: now_ms(),
    };
    other
        .ctx
        .consensus
        .propose(Command::CommitResult(proposal))
        .await
        .unwrap();

    cluster.node(lone).ctx.gate.restore();
    let node = cluster.node(lone);
    wait_until(PATIENCE, "the replay to settle", || {
        node.ctx.status().serving && node.ctx.degraded.pending() == 0
    })
    .await;
    let kept = node.ctx.state.with(|s| s.result(&request(1).id).cloned());
    assert_eq!(kept.unwrap().plan, winner);

    wait_until(PATIENCE, "the conflict in the event log", || {
        node.ctx.log.flush();
        std::fs::read_to_string(dir.join(format!("node-{lone}.jsonl")))
            .is_ok_and(|text| text.contains(r#""reason":"lost_first_commit""#))
    })
    .await;
    digests_match(&cluster).await;
    cluster.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_same_responder_with_another_priority_is_no_conflict() {
    let dir = std::env::temp_dir().join(format!("rescuemesh-degraded-p-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let metrics = dir.clone();
    let cluster = start(|c| c.metrics.dir = Some(metrics)).await;
    let lone = 3;
    leave_alone(&cluster, lone).await;
    let reply = first_reply(&cluster.config, request(1), Duration::from_secs(5))
        .await
        .unwrap();
    let ReplyBody::Provisional(provisional) = reply.body else {
        panic!("expected a provisional answer, got {:?}", reply.body);
    };

    // The other side committed the same responder for this request, a moment later or earlier.
    cluster.node(lone).ctx.gate.silence();
    restore_others(&cluster, lone);
    let other = cluster.node(1);
    wait_until(PATIENCE, "a leader on the other side", || {
        let status = other.ctx.consensus.status();
        status.leader.is_some_and(|l| l != lone) && other.ctx.status().serving
    })
    .await;
    let req = request(1);
    let winner = Plan {
        priority: provisional.priority + 7,
        ..provisional.clone()
    };
    let proposal = ResultProposal {
        request: req.id,
        incident: req.incident,
        executor: 1,
        term: other.ctx.consensus.status().term,
        plan: winner.clone(),
        age_ms: req.age_ms,
        assigned_at_ms: now_ms(),
    };
    other
        .ctx
        .consensus
        .propose(Command::CommitResult(proposal))
        .await
        .unwrap();

    cluster.node(lone).ctx.gate.restore();
    let node = cluster.node(lone);
    wait_until(PATIENCE, "the replay to settle", || {
        node.ctx.status().serving && node.ctx.degraded.pending() == 0
    })
    .await;
    assert_eq!(
        node.ctx
            .state
            .with(|s| s.result(&request(1).id).cloned())
            .unwrap()
            .plan,
        winner
    );
    // A conflict is logged before the entry leaves the table, so this cannot be too early.
    node.ctx.log.flush();
    let text = std::fs::read_to_string(dir.join(format!("node-{lone}.jsonl"))).unwrap();
    assert!(!text.contains("provisional_conflict"), "{text}");
    digests_match(&cluster).await;
    cluster.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_replayed_plan_whose_responder_is_taken_is_recorded_without_it() {
    let dir = std::env::temp_dir().join(format!("rescuemesh-degraded-t-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let metrics = dir.clone();
    // Long holds: the first replayed plan still holds the responder when the second is replayed.
    let cluster = start(|c| {
        c.metrics.dir = Some(metrics);
        c.campus.service_ms_per_severity = 150;
    })
    .await;
    let lone = 2;
    leave_alone(&cluster, lone).await;

    // Asked 150 ms apart, the local hold is over, so both answers may take the same responder.
    let (mut seq, mut seen) = (0, std::collections::BTreeSet::new());
    let shared = loop {
        seq += 1;
        assert!(seq < 40, "no responder came up twice");
        let reply = first_reply(&cluster.config, request(seq), Duration::from_secs(5))
            .await
            .unwrap();
        let ReplyBody::Provisional(plan) = reply.body else {
            panic!("expected a provisional answer, got {:?}", reply.body);
        };
        let responder = plan.responder.unwrap();
        if !seen.insert(responder) {
            break responder;
        }
        tokio::time::sleep(Duration::from_millis(160)).await;
    };

    restore_others(&cluster, lone);
    let node = cluster.node(lone);
    wait_until(PATIENCE, "the replay to settle", || {
        node.ctx.status().serving && node.ctx.degraded.pending() == 0
    })
    .await;
    wait_until(PATIENCE, "the conflict in the event log", || {
        node.ctx.log.flush();
        std::fs::read_to_string(dir.join(format!("node-{lone}.jsonl")))
            .is_ok_and(|text| text.contains(r#""reason":"responder_taken""#))
    })
    .await;
    // Settled means the leader committed it; this follower may not have applied it yet.
    let what = format!("every answer on the record, with or without {shared}");
    wait_until(PATIENCE, &what, || {
        node.ctx.state.with(|s| s.result_count()) == seq as usize
    })
    .await;
    digests_match(&cluster).await;
    cluster.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_replay_keeps_the_original_stamps() {
    let cluster = start(|_| {}).await;
    let lone = 2;
    leave_alone(&cluster, lone).await;
    let before = now_ms();
    let mut req = request(1);
    req.age_ms = 1_234;
    let reply = first_reply(&cluster.config, req.clone(), Duration::from_secs(5))
        .await
        .unwrap();
    assert!(matches!(reply.body, ReplyBody::Provisional(_)));
    let answered = now_ms();

    // However long the return takes, the record says when the client was told.
    let started = tokio::time::Instant::now();
    wait_until(PATIENCE, "time to pass before the return", || {
        started.elapsed() > Duration::from_millis(100)
    })
    .await;
    restore_others(&cluster, lone);
    let node = cluster.node(lone);
    wait_until(PATIENCE, "the replayed result on this node", || {
        node.ctx.state.with(|s| s.result(&req.id).is_some())
    })
    .await;
    let result = node.ctx.state.with(|s| s.result(&req.id).cloned()).unwrap();
    assert_eq!(result.age_ms, 1_234);
    assert!(
        (before..=answered).contains(&result.assigned_at_ms),
        "{before} {} {answered}",
        result.assigned_at_ms
    );
    digests_match(&cluster).await;
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lone_node_answers_a_too_old_report_provisionally_and_the_record_reads_expired() {
    let cluster = start(|_| {}).await;
    let lone = 3;
    leave_alone(&cluster, lone).await;
    let mut req = request(1);
    req.age_ms = 1_000;
    req.incident.ttl_ms = 1_000;
    let reply = first_reply(&cluster.config, req.clone(), Duration::from_secs(5))
        .await
        .unwrap();
    // Uncommitted, so it says Provisional; nobody is sent, and nothing is held locally either.
    let ReplyBody::Provisional(plan) = reply.body else {
        panic!("expected a provisional answer, got {:?}", reply.body);
    };
    assert_eq!(plan.responder, None);
    assert_eq!(cluster.node(lone).ctx.degraded.pending(), 1);

    restore_others(&cluster, lone);
    let node = cluster.node(lone);
    wait_until(PATIENCE, "the record to read Expired", || {
        node.ctx.state.with(|s| s.status_at(&req.id, now_ms())) == Some(IncidentStatus::Expired)
    })
    .await;
    digests_match(&cluster).await;

    // The replay committed it, so a retry now gets the committed refusal.
    let retry = {
        let mut retry = req.clone();
        retry.attempt = 1;
        first_reply(&cluster.config, retry, Duration::from_secs(5))
            .await
            .unwrap()
    };
    assert!(
        matches!(retry.body, ReplyBody::Expired(_)),
        "{:?}",
        retry.body
    );
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_cluster_recovers_when_the_longer_logs_were_all_silenced() {
    let cluster = start(|_| {}).await;
    // Node 3 falls behind; the two with the longer log then go silent together as it returns.
    cluster.node(3).ctx.gate.silence();
    let client = MeshClient::new(94, &cluster.config, EventLog::disabled("c"));
    let done = client.submit(incident()).await.unwrap();
    let request = done.committed.unwrap().request;
    cluster.node(1).ctx.gate.silence();
    cluster.node(2).ctx.gate.silence();
    cluster.node(3).ctx.gate.restore();
    tokio::time::sleep(Duration::from_millis(500)).await;
    cluster.node(1).ctx.gate.restore();
    cluster.node(2).ctx.gate.restore();

    // Node 3 cannot win with its shorter log, and nobody can tell the others they are current.
    digests_match(&cluster).await;
    let node = cluster.node(3);
    assert!(node.ctx.state.with(|s| s.result(&request).is_some()));
    let next = client.submit(incident()).await.unwrap();
    assert!(!next.provisional);
    cluster.shutdown().await;
}
