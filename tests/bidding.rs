//! Bidding election with real nodes: one result per request, load moves work, a node that is not
//! serving stays out. A silent winner is covered by takeover (tests/takeover.rs).
//!
//! Owner: B (bidding election).

mod common;

use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;

use common::{TestCluster, first_reply, wait_until};
use rescuemesh::config::{AssignmentPolicy, Config};
use rescuemesh::node::NodeHandle;
use rescuemesh::proto::{
    Capability, Category, ClientRequest, CommittedResult, Incident, NodeId, Point, ReplyBody,
    RequestId, UserId,
};

/// Sends per request, the same as a client's first send and five retries (D52).
const MAX_ATTEMPTS: u32 = 6;
const REPLY_TIMEOUT: Duration = Duration::from_secs(10);

fn request(seq: u64, attempt: u32) -> ClientRequest {
    ClientRequest {
        id: RequestId { client: 77, seq },
        attempt,
        incident: Incident {
            owner: UserId {
                client: 77,
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

/// A started cluster with bidding on, and a leader every node agrees on.
async fn bidding_cluster(tweak: impl FnOnce(&mut Config)) -> TestCluster {
    let cluster = TestCluster::start_with(3, |c| {
        c.policy.assignment = AssignmentPolicy::Bidding;
        tweak(c);
    })
    .await;
    wait_for_leader(&cluster, &[1, 2, 3]).await;
    cluster
}

async fn wait_for_leader(cluster: &TestCluster, nodes: &[NodeId]) {
    wait_until(Duration::from_secs(10), "an agreed leader", || {
        let leaders: Vec<_> = nodes
            .iter()
            .map(|&id| cluster.node(id).ctx.consensus.status().leader)
            .collect();
        leaders[0].is_some_and(|l| nodes.contains(&l)) && leaders.iter().all(|l| *l == leaders[0])
    })
    .await;
}

/// Sends the request to every node and returns the committed result from the first reply.
async fn done(cluster: &TestCluster, req: ClientRequest) -> CommittedResult {
    let seq = req.id.seq;
    let reply = first_reply(&cluster.config, req, REPLY_TIMEOUT)
        .await
        .unwrap_or_else(|| panic!("no reply to request {seq}"));
    match reply.body {
        ReplyBody::Done(result) => result,
        other => panic!("request {seq} got {other:?}"),
    }
}

/// Runs `jobs` jobs on the node's worker pool that last until the returned senders are dropped.
async fn hold_jobs(node: &NodeHandle, jobs: usize) -> Vec<mpsc::Sender<()>> {
    let before = node.ctx.workers.load();
    let mut senders = Vec::new();
    for _ in 0..jobs {
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
    wait_until(
        Duration::from_secs(5),
        "the held jobs to reach the pool",
        || {
            let load = node.ctx.workers.load();
            load.queued + load.running == before.queued + before.running + jobs
        },
    )
    .await;
    senders
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_request_gets_one_committed_result() {
    let cluster = bidding_cluster(|_| {}).await;
    let count = 20;

    // Retries as a client does: all 20 want the same responder, and a node that keeps losing it
    // gives up after three plans and leaves the rest to the retry (D35). Without this the test
    // failed about 2 runs in 100.
    let mut replies = Vec::new();
    for seq in 0..count {
        let config = cluster.config.clone();
        replies.push(tokio::spawn(async move {
            for attempt in 0..MAX_ATTEMPTS {
                let wait = REPLY_TIMEOUT / MAX_ATTEMPTS;
                if let Some(reply) = first_reply(&config, request(seq, attempt), wait).await {
                    return Some(reply);
                }
            }
            None
        }));
    }
    let mut results = Vec::new();
    for reply in replies {
        let reply = reply.await.unwrap().expect("every request is answered");
        match reply.body {
            ReplyBody::Done(result) => results.push(result),
            other => panic!("unexpected reply {other:?}"),
        }
    }

    wait_until(
        Duration::from_secs(5),
        "every node to hold every result",
        || {
            (1..=3)
                .all(|id| cluster.node(id).ctx.state.with(|s| s.result_count()) == count as usize)
        },
    )
    .await;
    // The first commit wins, so every node and every reply agree on each request's result.
    for result in &results {
        for id in 1..=3 {
            let held = cluster
                .node(id)
                .ctx
                .state
                .with(|s| s.result(&result.request).cloned());
            assert_eq!(held.as_ref(), Some(result));
        }
    }
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_busy_node_loses_after_planning_very_fast() {
    let cluster = bidding_cluster(|c| {
        c.workers.threads = 2;
        c.workers.queue_capacity = 4;
    })
    .await;

    // All scores tie, so node 1 plans first. A fast plan could once leave node 1 at a score of 0
    // however long its queue got.
    assert_eq!(done(&cluster, request(0, 0)).await.executor, 1);

    let held = hold_jobs(cluster.node(1), 6).await;
    let load = cluster.node(1).ctx.workers.load();
    assert_eq!((load.running, load.queued), (2, 4));

    for seq in 1..=10 {
        let result = done(&cluster, request(seq, 0)).await;
        assert_ne!(result.executor, 1, "the full node planned request {seq}");
    }
    drop(held);
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_node_that_is_not_serving_stays_out() {
    // A slow start, so that the jobs held below raise the scores of nodes 1 and 2, and node 3 would
    // win if it bid.
    let cluster = bidding_cluster(|c| c.policy.initial_service_us = 1000).await;
    cluster.node(3).ctx.serving.store(false, Ordering::SeqCst);
    let held = [
        hold_jobs(cluster.node(1), 1).await,
        hold_jobs(cluster.node(2), 1).await,
    ];

    for seq in 0..5 {
        let req = request(seq, 0);
        let key = (req.id, req.attempt);
        let result = done(&cluster, req).await;
        assert_ne!(result.executor, 3);
        wait_until(Duration::from_secs(5), "the round to close", || {
            (1..=2).all(|id| {
                let bids = cluster
                    .node(id)
                    .ctx
                    .bids
                    .with_rounds(|r| r.ranked_bids(key));
                bids.is_some_and(|bids| bids.iter().all(|&(node, _)| node != 3))
            })
        })
        .await;
    }
    drop(held);
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn leader_dispatch_is_unchanged() {
    let cluster = TestCluster::start(3).await;
    wait_for_leader(&cluster, &[1, 2, 3]).await;
    let leader = cluster
        .node(1)
        .ctx
        .consensus
        .status()
        .leader
        .expect("a leader");

    for seq in 0..5 {
        assert_eq!(done(&cluster, request(seq, 0)).await.executor, leader);
    }
    for id in 1..=3 {
        assert!(cluster.node(id).ctx.bids.with_rounds(|r| r.is_empty()));
    }
    cluster.shutdown().await;
}
