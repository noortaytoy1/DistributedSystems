//! Rotating rounds: one order with no leader, on real nodes in one process (D249).

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{TestCluster, wait_until};
use rescuemesh::config::{Config, ConsensusMode};
use rescuemesh::proto::{
    Capability, Category, Command, Incident, NodeId, Outcome, Plan, Point, RequestId, Responder,
    ResponderId, ResultProposal, UserId,
};
use rescuemesh::raft::{Consensus, Role};

const WAIT: Duration = Duration::from_secs(20);

fn rounds(config: &mut Config) {
    config.consensus = ConsensusMode::Rounds;
    config.faults.enabled = false;
}

fn proposal(seq: u64, executor: NodeId, responder: Option<ResponderId>) -> Command {
    Command::CommitResult(ResultProposal {
        request: RequestId { client: 1, seq },
        incident: Incident {
            owner: UserId { client: 1, user: 0 },
            category: Category::Flooding,
            zone: 0,
            location: Point { x: 5, y: 5 },
            severity: 2,
            needs: Capability::Rescue,
            reported_at_ms: 0,
            ttl_ms: 60_000,
        },
        executor,
        term: 0,
        plan: Plan {
            responder,
            cost: 1,
            priority: 1,
        },
        age_ms: 0,
        assigned_at_ms: 0,
    })
}

fn register(ids: impl IntoIterator<Item = ResponderId>) -> Command {
    Command::RegisterResponders {
        responders: ids
            .into_iter()
            .map(|id| Responder {
                id,
                zone: 0,
                location: Point { x: 0, y: 0 },
                capability: Capability::Rescue,
            })
            .collect(),
    }
}

fn consensus(cluster: &TestCluster, id: NodeId) -> Arc<dyn Consensus> {
    cluster.node(id).ctx.consensus.clone()
}

async fn same_state(cluster: &TestCluster, ids: &[NodeId], results: usize) {
    wait_until(WAIT, "the nodes to hold one state", || {
        let views: Vec<_> = ids
            .iter()
            .map(|id| {
                cluster
                    .node(*id)
                    .ctx
                    .state
                    .with(|s| (s.result_count(), s.digest()))
            })
            .collect();
        views[0].0 >= results && views.iter().all(|v| *v == views[0])
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nobody_leads_and_every_node_commits_to_one_order() {
    let cluster = TestCluster::start_with(3, rounds).await;
    for node in &cluster.nodes {
        let status = node.ctx.consensus.status();
        assert_eq!(status.leader, None);
        assert_eq!(status.role, Role::Peer);
    }
    consensus(&cluster, 1)
        .propose(register(1..=500))
        .await
        .unwrap();
    let mut proposals = Vec::new();
    for seq in 0..300u64 {
        let node = (seq % 3) + 1;
        let c = consensus(&cluster, node);
        proposals.push(tokio::spawn(async move {
            c.propose(proposal(seq, node, Some(seq as u32 + 1))).await
        }));
    }
    for p in proposals {
        assert!(matches!(p.await.unwrap(), Ok(Outcome::Committed(_))));
    }
    same_state(&cluster, &[1, 2, 3], 300).await;
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_first_in_the_order_wins_on_every_node() {
    let cluster = TestCluster::start_with(3, rounds).await;
    consensus(&cluster, 2)
        .propose(register([1, 2]))
        .await
        .unwrap();
    let (a, b) = (consensus(&cluster, 1), consensus(&cluster, 3));
    let (first, second) = tokio::join!(
        a.propose(proposal(7, 1, Some(1))),
        b.propose(proposal(7, 3, Some(2))),
    );
    let winner = |outcome: Outcome| match outcome {
        Outcome::Committed(r) | Outcome::Duplicate(r) => r.executor,
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(winner(first.unwrap()), winner(second.unwrap()));
    same_state(&cluster, &[1, 2, 3], 1).await;
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_others_go_on_without_a_silent_node_and_it_catches_up_after() {
    let cluster = TestCluster::start_with(3, rounds).await;
    let c1 = consensus(&cluster, 1);
    c1.propose(register(1..=500)).await.unwrap();
    c1.propose(proposal(1, 1, Some(1))).await.unwrap();
    same_state(&cluster, &[1, 2, 3], 1).await;

    cluster.node(3).ctx.gate.silence();
    // Blocked until the two agree to go on, then it commits.
    for seq in 2..20u64 {
        c1.propose(proposal(seq, 1, Some(seq as u32)))
            .await
            .unwrap();
    }
    same_state(&cluster, &[1, 2], 19).await;
    assert!(
        cluster.node(1).ctx.consensus.status().term > 0,
        "a new view"
    );

    cluster.node(3).ctx.gate.restore();
    same_state(&cluster, &[1, 2, 3], 19).await;
    wait_until(WAIT, "node 3 to serve again", || {
        cluster
            .node(3)
            .ctx
            .serving
            .load(std::sync::atomic::Ordering::SeqCst)
    })
    .await;

    // Back as a full member: its own proposals commit, and all three agree.
    let c3 = consensus(&cluster, 3);
    let until = tokio::time::Instant::now() + WAIT;
    loop {
        if let Ok(Outcome::Committed(_)) = c3.propose(proposal(50, 3, Some(50))).await {
            break;
        }
        assert!(
            tokio::time::Instant::now() < until,
            "node 3 never came back"
        );
    }
    same_state(&cluster, &[1, 2, 3], 20).await;
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_node_left_alone_stops_committing_until_a_peer_returns() {
    let cluster = TestCluster::start_with(3, |c| {
        rounds(c);
        c.timers.propose_timeout_ms = 500;
    })
    .await;
    let c1 = consensus(&cluster, 1);
    c1.propose(register(1..=10)).await.unwrap();
    cluster.node(2).ctx.gate.silence();
    cluster.node(3).ctx.gate.silence();
    assert!(c1.propose(proposal(1, 1, Some(1))).await.is_err());
    let before = cluster.node(1).ctx.state.with(|s| s.result_count());
    assert_eq!(before, 0, "one node alone must not commit");

    cluster.node(2).ctx.gate.restore();
    cluster.node(3).ctx.gate.restore();
    let until = tokio::time::Instant::now() + WAIT;
    while c1.propose(proposal(2, 1, Some(2))).await.is_err() {
        assert!(tokio::time::Instant::now() < until, "rounds never resumed");
    }
    same_state(&cluster, &[1, 2, 3], 1).await;
    cluster.shutdown().await;
}
