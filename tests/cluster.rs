//! Three nodes in one process, driven through the client and control ports.
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::{TestCluster, first_reply, wait_until};
use rescuemesh::client::control;
use rescuemesh::proto::{
    Capability, ClientRequest, ControlReply, ControlRequest, Incident, Point, ReplyBody, RequestId,
};

fn request(seq: u64, attempt: u32) -> ClientRequest {
    ClientRequest {
        id: RequestId { client: 99, seq },
        attempt,
        incident: Incident {
            zone: 0,
            location: Point { x: 10, y: 20 },
            severity: 3,
            needs: Capability::Medical,
            reported_at_ms: 0,
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
