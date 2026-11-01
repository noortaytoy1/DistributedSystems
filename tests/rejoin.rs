//! Part D: a node returns from silence, catches up, and only then serves again (D46, D75).
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use common::{TestCluster, wait_until};
use rescuemesh::node::faults;
use rescuemesh::proto::{
    Capability, Command, Incident, NodeId, Outcome, Plan, Point, RequestId, ResultProposal,
};
use rescuemesh::raft::Role;

const WAIT: Duration = Duration::from_secs(10);

fn proposal(seq: u64, executor: NodeId) -> Command {
    Command::CommitResult(ResultProposal {
        request: RequestId { client: 7, seq },
        incident: Incident {
            zone: 0,
            location: Point { x: 5, y: 5 },
            severity: 2,
            needs: Capability::Rescue,
            reported_at_ms: 0,
        },
        executor,
        term: 0,
        plan: Plan {
            responder: None,
            cost: 1,
        },
    })
}

/// The leader every node in `live` agrees on.
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

fn serving(cluster: &TestCluster, id: NodeId) -> bool {
    cluster.node(id).ctx.serving.load(Ordering::SeqCst)
}

fn silenced(cluster: &TestCluster, id: NodeId) -> bool {
    cluster.node(id).ctx.gate.is_silenced()
}

/// Every node has the same applied index and digest.
fn same_state(cluster: &TestCluster) -> bool {
    let views: Vec<_> = (1..=3)
        .map(|id| {
            cluster
                .node(id)
                .ctx
                .state
                .with(|s| (s.last_index(), s.digest()))
        })
        .collect();
    views.iter().all(|v| *v == views[0])
}

/// FT-4: a follower misses commits while silent, returns not serving, catches up, then serves.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_returning_follower_serves_only_after_catching_up() {
    let cluster = TestCluster::start(3).await;
    let leader = agreed_leader(&cluster, &[1, 2, 3]).await;
    let follower = (1..=3).find(|id| *id != leader).unwrap();

    // Driven by hand so all 20 commits land inside the silence, however slow the machine.
    cluster.node(follower).ctx.gate.silence();
    wait_until(WAIT, "silenced follower stops serving", || {
        !serving(&cluster, follower)
    })
    .await;

    for seq in 0..20 {
        let outcome = cluster
            .node(leader)
            .ctx
            .consensus
            .propose(proposal(seq, leader))
            .await
            .unwrap();
        assert!(matches!(outcome, Outcome::Committed(_)));
    }
    // While silent it neither serves nor learns anything.
    assert!(silenced(&cluster, follower));
    assert!(!serving(&cluster, follower));
    assert_eq!(
        cluster.node(follower).ctx.state.with(|s| s.result_count()),
        0
    );

    cluster.node(follower).ctx.gate.restore();
    wait_until(WAIT, "follower serving again", || {
        serving(&cluster, follower)
    })
    .await;
    // Serving means current: it already has everything the leader committed before it returned.
    assert_eq!(
        cluster.node(follower).ctx.state.with(|s| s.result_count()),
        20
    );
    wait_until(WAIT, "identical state on every node", || {
        same_state(&cluster)
    })
    .await;
    cluster.shutdown().await;
}

/// FT-6: a follower silent for far longer than the election timeout comes back without causing
/// a single election, because it may not start one while it is not serving.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_returning_follower_causes_no_election() {
    let cluster = TestCluster::start(3).await;
    let leader = agreed_leader(&cluster, &[1, 2, 3]).await;
    let term = cluster.node(leader).ctx.consensus.status().term;
    let follower = (1..=3).find(|id| *id != leader).unwrap();

    // 1 s is more than three maximum election timeouts (300 ms).
    faults::silence_for(&cluster.node(follower).ctx, Duration::from_secs(1)).unwrap();
    wait_until(WAIT, "follower restored", || !silenced(&cluster, follower)).await;
    wait_until(WAIT, "follower serving again", || {
        serving(&cluster, follower)
    })
    .await;

    for id in 1..=3 {
        let s = cluster.node(id).ctx.consensus.status();
        assert_eq!(s.term, term, "node {id} changed term");
        assert_eq!(s.leader, Some(leader), "node {id} sees another leader");
    }
    cluster.shutdown().await;
}

/// A leader silent for less than an election timeout is usually not replaced, so no new leader
/// sends it AppendEntries. It must still find out it is current and serve again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_leader_back_from_a_short_silence_serves_again() {
    let cluster = TestCluster::start(3).await;
    let leader = agreed_leader(&cluster, &[1, 2, 3]).await;

    faults::silence_for(&cluster.node(leader).ctx, Duration::from_millis(50)).unwrap();
    wait_until(WAIT, "leader restored", || !silenced(&cluster, leader)).await;
    wait_until(WAIT, "old leader serving again", || {
        serving(&cluster, leader)
    })
    .await;
    wait_until(WAIT, "identical state on every node", || {
        same_state(&cluster)
    })
    .await;
    cluster.shutdown().await;
}

/// A leader silent long enough to be replaced returns as a follower and catches up.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_replaced_leader_rejoins_as_a_follower() {
    let cluster = TestCluster::start(3).await;
    let old = agreed_leader(&cluster, &[1, 2, 3]).await;

    // The gate is driven by hand so every commit happens inside the silence, however slow the
    // election is; `silence_for` itself is tested in tests/cluster.rs.
    cluster.node(old).ctx.gate.silence();
    let live: Vec<NodeId> = (1..=3).filter(|id| *id != old).collect();
    let new = agreed_leader(&cluster, &live).await;
    assert_ne!(new, old);
    for seq in 0..10 {
        cluster
            .node(new)
            .ctx
            .consensus
            .propose(proposal(seq, new))
            .await
            .unwrap();
    }
    assert!(!serving(&cluster, old));
    cluster.node(old).ctx.gate.restore();

    wait_until(WAIT, "old leader serving again", || serving(&cluster, old)).await;
    assert_eq!(cluster.node(old).ctx.state.with(|s| s.result_count()), 10);
    assert_eq!(cluster.node(old).ctx.consensus.status().leader, Some(new));
    cluster.shutdown().await;
}
