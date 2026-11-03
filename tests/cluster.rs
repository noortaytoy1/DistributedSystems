//! Three nodes in one process, driven through the client and control ports.
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::{TestCluster, first_reply, wait_until};
use rescuemesh::client::control;
use rescuemesh::proto::{
    Capability, Category, ClientRequest, ControlReply, ControlRequest, Incident, Point, ReplyBody,
    RequestId, UserId,
};

fn request(seq: u64, attempt: u32) -> ClientRequest {
    ClientRequest {
        id: RequestId { client: 99, seq },
        attempt,
        age_ms: 0,
        incident: Incident {
            owner: UserId {
                client: 99,
                user: 0,
            },
            category: Category::MedicalAssistance,
            zone: 0,
            location: Point { x: 10, y: 20 },
            severity: 3,
            needs: Capability::Medical,
            reported_at_ms: 0,
            ttl_ms: 60_000,
        },
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_request_is_committed_before_the_reply_and_retries_get_the_same_result() {
    let cluster = TestCluster::start(3).await;
    // Under leader dispatch nobody plans before a leader exists (the client's retry covers that).
    wait_until(Duration::from_secs(10), "a leader", || {
        (1..=3).all(|id| cluster.node(id).ctx.consensus.status().leader.is_some())
    })
    .await;

    let reply = first_reply(&cluster.config, request(1, 0), Duration::from_secs(10))
        .await
        .expect("no node replied");
    let ReplyBody::Done(result) = reply.body else {
        panic!("unexpected reply {reply:?}");
    };
    assert_eq!(result.request, request(1, 0).id);
    // Leader dispatch: the leader planned it and was the one to reply.
    let leader = cluster.node(1).ctx.consensus.status().leader;
    assert_eq!(Some(result.executor), leader);
    assert_eq!(reply.from, result.executor);
    // Committed before the reply, so it is already in the replier's state.
    let replier = &cluster.node(reply.from).ctx.state;
    assert_eq!(
        replier.with(|s| s.result(&result.request).cloned()),
        Some(result.clone())
    );

    // A retry with the same ID gets the same result and commits nothing new.
    let retry = first_reply(&cluster.config, request(1, 1), Duration::from_secs(10))
        .await
        .expect("no reply to the retry");
    assert_eq!(retry.body, ReplyBody::Done(result));
    wait_until(Duration::from_secs(5), "result on every node", || {
        (1..=3).all(|id| cluster.node(id).ctx.state.with(|s| s.result_count()) == 1)
    })
    .await;
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_node_answers_status() {
    let cluster = TestCluster::start(3).await;
    for n in &cluster.config.cluster.nodes {
        let reply = control(
            &n.client_addr,
            ControlRequest::Status,
            Duration::from_secs(2),
        )
        .await
        .unwrap();
        let ControlReply::Status(status) = reply else {
            panic!("unexpected reply {reply:?}");
        };
        assert_eq!(status.id, n.id);
        assert!(!status.silenced);
    }
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ctl_silence_closes_the_gate_rejects_overlaps_and_restores_itself() {
    let cluster = TestCluster::start(3).await;
    let addr = cluster.config.node(2).unwrap().client_addr.clone();
    let silence = |ms| {
        control(
            &addr,
            ControlRequest::Silence { ms },
            Duration::from_secs(2),
        )
    };

    // Out-of-range durations never close the gate.
    assert!(matches!(silence(0).await.unwrap(), ControlReply::Error(_)));
    assert!(matches!(
        silence(20_001).await.unwrap(),
        ControlReply::Error(_)
    ));
    assert!(!cluster.node(2).ctx.gate.is_silenced());

    assert_eq!(silence(1_000).await.unwrap(), ControlReply::Done);
    assert!(cluster.node(2).ctx.gate.is_silenced());
    // Control still reaches a silenced node (D23), and a second fault is refused (D74).
    assert!(matches!(
        silence(5_000).await.unwrap(),
        ControlReply::Error(_)
    ));

    wait_until(Duration::from_secs(5), "node 2 restored", || {
        !cluster.node(2).ctx.gate.is_silenced()
    })
    .await;
    // Once restored it can be silenced again.
    assert_eq!(silence(100).await.unwrap(), ControlReply::Done);
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ctl_silence_is_refused_when_fault_injection_is_disabled() {
    let cluster = TestCluster::start_with(3, |c| c.faults.enabled = false).await;
    let addr = cluster.config.node(2).unwrap().client_addr.clone();
    let reply = control(
        &addr,
        ControlRequest::Silence { ms: 1_000 },
        Duration::from_secs(2),
    )
    .await
    .unwrap();
    assert!(matches!(reply, ControlReply::Error(_)));
    assert!(!cluster.node(2).ctx.gate.is_silenced());
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_held_responder_is_released_by_its_executor_after_the_service_time() {
    let cluster = TestCluster::start_with(3, |c| c.campus.service_ms_per_severity = 20).await;
    let total = cluster.config.campus.responders as usize;
    let node = cluster.node(1);
    wait_until(Duration::from_secs(5), "responders and a leader", || {
        node.ctx.state.with(|s| s.responder_count()) == total
            && node.ctx.consensus.status().leader.is_some()
    })
    .await;
    for seq in 0..10 {
        let reply = first_reply(&cluster.config, request(seq, 0), Duration::from_secs(5)).await;
        assert!(reply.is_some(), "request {seq} got no reply");
    }
    wait_until(Duration::from_secs(5), "every responder free again", || {
        cluster.nodes.iter().all(|n| {
            n.ctx
                .state
                .with(|s| s.free_responders().len() == total && s.counters().released > 0)
        })
    })
    .await;
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_silent_executors_responder_is_released_by_a_peer() {
    // Long enough that the executor is silenced before its own release is due.
    let cluster = TestCluster::start_with(3, |c| c.campus.service_ms_per_severity = 500).await;
    let total = cluster.config.campus.responders as usize;
    wait_until(Duration::from_secs(5), "a leader and responders", || {
        cluster.nodes.iter().all(|n| {
            n.ctx.state.with(|s| s.responder_count()) == total
                && n.ctx.consensus.status().leader.is_some()
        })
    })
    .await;
    let reply = first_reply(&cluster.config, request(1, 0), Duration::from_secs(5))
        .await
        .expect("a reply");
    let ReplyBody::Done(done) = reply.body else {
        panic!("expected Done");
    };
    let executor = cluster.node(done.executor);
    executor.ctx.gate.silence();
    let others: Vec<_> = cluster
        .nodes
        .iter()
        .filter(|n| n.ctx.id != done.executor)
        .collect();
    wait_until(Duration::from_secs(10), "a peer releases the hold", || {
        others.iter().all(|n| {
            n.ctx
                .state
                .with(|s| s.counters().released == 1 && s.free_responders().len() == total)
        })
    })
    .await;
    executor.ctx.gate.restore();
    cluster.shutdown().await;
}
