//! Part C: takeover on three real nodes with bidding on (D42, D110). A silent winner is taken over
//! by the runner-up without the client retrying, a busy winner by the backstop, and a healthy
//! cluster never takes over.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::mpsc;
use std::time::Duration;

use common::{TestCluster, first_reply, wait_until};
use rescuemesh::config::{AssignmentPolicy, Config};
use rescuemesh::node::NodeHandle;
use rescuemesh::proto::{
    Capability, ClientRequest, CommittedResult, Incident, NodeId, Point, ReplyBody, RequestId,
};

const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

fn request(seq: u64) -> ClientRequest {
    ClientRequest {
        id: RequestId { client: 88, seq },
        attempt: 0,
        incident: Incident {
            zone: 0,
            location: Point { x: 10, y: 20 },
            severity: 3,
            needs: Capability::Medical,
            reported_at_ms: 0,
        },
    }
}

/// Bidding on, and a leader every node agrees on.
async fn bidding_cluster(tweak: impl FnOnce(&mut Config)) -> TestCluster {
    let cluster = TestCluster::start_with(3, |c| {
        c.policy.assignment = AssignmentPolicy::Bidding;
        tweak(c);
    })
    .await;
    wait_until(Duration::from_secs(10), "an agreed leader", || {
        let leaders: Vec<_> = (1..=3)
            .map(|id| cluster.node(id).ctx.consensus.status().leader)
            .collect();
        leaders[0].is_some() && leaders.iter().all(|l| *l == leaders[0])
    })
    .await;
    cluster
}

/// Sends `req` once to every node (no client retry) and returns the committed result.
fn spawn_request(
    cluster: &TestCluster,
    req: ClientRequest,
) -> tokio::task::JoinHandle<CommittedResult> {
    let config = cluster.config.clone();
    tokio::spawn(async move {
        let reply = first_reply(&config, req, REPLY_TIMEOUT)
            .await
            .expect("answered without a client retry");
        match reply.body {
            ReplyBody::Done(result) => result,
            other => panic!("unexpected reply {other:?}"),
        }
    })
}

/// Waits until every node holds all three bids, so the round's ranking is fixed.
async fn wait_for_bids(cluster: &TestCluster, req: &ClientRequest) {
    let key = (req.id, req.attempt);
    wait_until(
        Duration::from_secs(5),
        "every node to hold every bid",
        || (1..=3).all(|id| cluster.node(id).ctx.bids.with_rounds(|r| r.bids_seen(key)) == 3),
    )
    .await;
}

fn winner(cluster: &TestCluster, observer: NodeId, req: &ClientRequest) -> Option<NodeId> {
    let key = (req.id, req.attempt);
    cluster
        .node(observer)
        .ctx
        .bids
        .with_rounds(|r| r.ranked_bids(key))
        .and_then(|bids| bids.first().map(|&(node, _)| node))
}

/// Fills every worker thread of `node` until the returned senders are dropped.
async fn hold_workers(node: &NodeHandle) -> Vec<mpsc::Sender<()>> {
    let threads = node.ctx.workers.load().threads;
    let mut senders = Vec::new();
    for _ in 0..threads {
        let (tx, rx) = mpsc::channel::<()>();
        let pool = node.ctx.workers.clone();
        tokio::spawn(async move {
            let _ = pool
                .run(move || {
                    let _ = rx.recv();
                })
                .await;
        });
        senders.push(tx);
    }
    wait_until(Duration::from_secs(5), "every worker to be held", || {
        node.ctx.workers.load().running == threads
    })
    .await;
    senders
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_silent_winner_is_taken_over_by_the_runner_up() {
    // A long window, so the winner can be silenced while its round is still open.
    let cluster = bidding_cluster(|c| c.timers.bid_window_ms = 500).await;
    let req = request(1);
    let reply = spawn_request(&cluster, req.clone());
    wait_for_bids(&cluster, &req).await;
    // Every score ties, so node 1 wins and node 2 is the runner-up.
    cluster.node(1).ctx.gate.silence();

    let result = reply.await.unwrap();
    assert_eq!(result.executor, 2, "the runner-up takes over");
    for id in 2..=3 {
        assert_eq!(winner(&cluster, id, &req), Some(1));
    }
    cluster.node(1).ctx.gate.restore();
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_busy_winner_is_taken_over_by_the_backstop() {
    let cluster = bidding_cluster(|c| {
        c.timers.bid_window_ms = 500;
        c.workers.threads = 1;
    })
    .await;
    let req = request(2);
    let reply = spawn_request(&cluster, req.clone());
    wait_for_bids(&cluster, &req).await;
    // The bids are in, so node 1 still wins; its plan then waits behind a held worker while it
    // keeps answering heartbeats.
    let held = hold_workers(cluster.node(1)).await;

    let result = reply.await.unwrap();
    assert_eq!(result.executor, 2, "the backstop moves the request on");
    assert!(
        !cluster
            .node(2)
            .ctx
            .suspicion
            .lock()
            .unwrap()
            .is_suspected(1)
    );
    drop(held);
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_healthy_cluster_never_takes_over() {
    let cluster = bidding_cluster(|_| {}).await;
    for seq in 10..30 {
        let req = request(seq);
        let result = spawn_request(&cluster, req.clone()).await.unwrap();
        let loser = (1..=3).find(|&id| id != result.executor).unwrap();
        // The loser's window runs on its own clock and can close after the reply.
        wait_until(Duration::from_secs(5), "the loser's round to close", || {
            winner(&cluster, loser, &req).is_some()
        })
        .await;
        assert_eq!(
            winner(&cluster, loser, &req),
            Some(result.executor),
            "request {seq} was planned by a node that did not win"
        );
    }
    cluster.shutdown().await;
}
