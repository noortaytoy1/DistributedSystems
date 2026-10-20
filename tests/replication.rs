//! Part A: Raft replication on three real nodes in one process.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{TestCluster, wait_until};
use rescuemesh::proto::{
    Capability, Command, Incident, NodeId, Outcome, Plan, Point, RequestId, Responder, ResponderId,
    ResultProposal, StaleReason,
};
use rescuemesh::raft::{Consensus, Role};

const WAIT: Duration = Duration::from_secs(10);

fn proposal(client: u64, seq: u64, executor: NodeId, responder: Option<ResponderId>) -> Command {
    Command::CommitResult(ResultProposal {
        request: RequestId { client, seq },
        incident: Incident {
            zone: 0,
            location: Point { x: 5, y: 5 },
            severity: 2,
            needs: Capability::Rescue,
            reported_at_ms: 0,
        },
        executor,
        term: 0,
        plan: Plan { responder, cost: 1 },
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

/// The leader every live node agrees on.
async fn agreed_leader(cluster: &TestCluster, live: &[NodeId]) -> NodeId {
    let mut leader = None;
    wait_until(WAIT, "one agreed leader", || {
        let views: Vec<_> = live
            .iter()
            .map(|id| cluster.node(*id).ctx.consensus.status())
            .collect();
        let first = views[0].leader;
        let agreed = first.is_some()
            && views
                .iter()
                .all(|s| s.leader == first && s.term == views[0].term);
        let leaders = views.iter().filter(|s| s.role == Role::Leader).count();
        if agreed && leaders == 1 {
            leader = first;
        }
        leader.is_some()
    })
    .await;
    leader.unwrap()
}

/// Wait until `nodes` applied the same commands, then return the shared digest.
async fn converged(cluster: &TestCluster, nodes: &[NodeId], results: usize) -> u64 {
    let mut digest = 0;
    wait_until(WAIT, "identical state on every node", || {
        let views: Vec<_> = nodes
            .iter()
            .map(|id| {
                cluster
                    .node(*id)
                    .ctx
                    .state
                    .with(|s| (s.result_count(), s.last_index(), s.digest()))
            })
            .collect();
        digest = views[0].2;
        views[0].0 == results && views.iter().all(|v| *v == views[0])
    })
    .await;
    digest
}

fn consensus(cluster: &TestCluster, id: NodeId) -> Arc<dyn Consensus> {
    cluster.node(id).ctx.consensus.clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn three_nodes_elect_exactly_one_leader() {
    let cluster = TestCluster::start(3).await;
    let leader = agreed_leader(&cluster, &[1, 2, 3]).await;
    assert!((1..=3).contains(&leader));
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn proposals_from_every_node_replicate_to_identical_state() {
    let cluster = TestCluster::start(3).await;
    agreed_leader(&cluster, &[1, 2, 3]).await;
    consensus(&cluster, 2)
        .propose(register(0..50))
        .await
        .unwrap();

    // 300 results proposed concurrently, a third through each node; followers forward.
    let mut tasks = Vec::new();
    for seq in 0..300u64 {
        let node = seq % 3 + 1;
        let c = consensus(&cluster, node);
        let responder = (seq < 50).then_some(seq as ResponderId);
        tasks.push(tokio::spawn(async move {
            c.propose(proposal(7, seq, node, responder)).await
        }));
    }
    for t in tasks {
        assert!(matches!(t.await.unwrap(), Ok(Outcome::Committed(_))));
    }

    converged(&cluster, &[1, 2, 3], 300).await;
    for id in 1..=3 {
        let held = cluster
            .node(id)
            .ctx
            .state
            .with(|s| s.responder_count() - s.free_responders().len());
        assert_eq!(held, 50, "node {id}");
    }
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn first_commit_wins_when_several_nodes_execute_one_request() {
    let cluster = TestCluster::start(3).await;
    agreed_leader(&cluster, &[1, 2, 3]).await;
    consensus(&cluster, 1)
        .propose(register([1, 2, 3]))
        .await
        .unwrap();

    // Three executors race on one request, each with a different plan.
    let tasks: Vec<_> = (1..=3)
        .map(|node| {
            let c = consensus(&cluster, node);
            tokio::spawn(async move {
                c.propose(proposal(9, 1, node, Some(node as ResponderId)))
                    .await
            })
        })
        .collect();
    let mut outcomes = Vec::new();
    for t in tasks {
        outcomes.push(t.await.unwrap().unwrap());
    }

    let winners: Vec<_> = outcomes
        .iter()
        .filter_map(|o| match o {
            Outcome::Committed(r) => Some(r.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(winners.len(), 1, "exactly one commit: {outcomes:?}");
    let winner = &winners[0];
    for o in &outcomes {
        match o {
            Outcome::Committed(r) | Outcome::Duplicate(r) => assert_eq!(r, winner),
            other => panic!("unexpected {other:?}"),
        }
    }

    converged(&cluster, &[1, 2, 3], 1).await;
    for id in 1..=3 {
        let state = &cluster.node(id).ctx.state;
        assert_eq!(
            state.with(|s| s.result(&winner.request).cloned()).as_ref(),
            Some(winner)
        );
        // Only the winner's responder is held.
        assert_eq!(state.with(|s| s.free_responders().len()), 2);
    }
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn plan_for_a_taken_responder_comes_back_stale() {
    let cluster = TestCluster::start(3).await;
    agreed_leader(&cluster, &[1, 2, 3]).await;
    let c = consensus(&cluster, 3);
    c.propose(register([1])).await.unwrap();
    assert!(matches!(
        c.propose(proposal(1, 1, 3, Some(1))).await.unwrap(),
        Outcome::Committed(_)
    ));
    let stale = c.propose(proposal(1, 2, 3, Some(1))).await.unwrap();
    assert_eq!(
        stale,
        Outcome::Stale {
            request: RequestId { client: 1, seq: 2 },
            responder: 1,
            reason: StaleReason::HeldBy(RequestId { client: 1, seq: 1 }),
        }
    );
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cluster_keeps_committing_when_the_leader_goes_silent_and_it_catches_up_after() {
    let cluster = TestCluster::start(3).await;
    let old = agreed_leader(&cluster, &[1, 2, 3]).await;
    let old_term = cluster.node(old).ctx.consensus.status().term;
    for seq in 0..20 {
        consensus(&cluster, old)
            .propose(proposal(3, seq, old, None))
            .await
            .unwrap();
    }

    cluster.node(old).ctx.gate.silence();
    let live: Vec<NodeId> = (1..=3).filter(|id| *id != old).collect();
    let new = agreed_leader(&cluster, &live).await;
    assert_ne!(new, old);
    assert!(cluster.node(new).ctx.consensus.status().term > old_term);

    // The two live nodes keep committing; proposals through the follower are forwarded.
    for seq in 20..40 {
        let via = live[seq as usize % 2];
        let outcome = consensus(&cluster, via)
            .propose(proposal(3, seq, via, None))
            .await
            .unwrap();
        assert!(matches!(outcome, Outcome::Committed(_)));
    }
    converged(&cluster, &live, 40).await;

    // The silenced node missed all of it, then catches up from the leader's log.
    assert_eq!(cluster.node(old).ctx.state.with(|s| s.result_count()), 20);
    cluster.node(old).ctx.gate.restore();
    converged(&cluster, &[1, 2, 3], 40).await;
    cluster.shutdown().await;
}
