//! Part D: a node returns from silence, catches up, and only then serves again (D46, D75).
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::{TestCluster, wait_until};
use rescuemesh::node::faults;
use rescuemesh::proto::{
    Capability, Category, Command, Incident, NodeId, Outcome, Plan, Point, RequestId,
    ResultProposal, UserId,
};
use rescuemesh::raft::Role;
use rescuemesh::raft::store::CHECKPOINT_EVERY;

const WAIT: Duration = Duration::from_secs(10);

fn proposal(seq: u64, executor: NodeId) -> Command {
    Command::CommitResult(ResultProposal {
        request: RequestId { client: 7, seq },
        incident: Incident {
            owner: UserId { client: 7, user: 0 },
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
            responder: None,
            cost: 1,
            priority: 200,
        },
        age_ms: 0,
        assigned_at_ms: 0,
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

/// The digest each node logged at each checkpoint index.
fn checkpoints(dir: &Path, id: NodeId) -> BTreeMap<u64, u64> {
    let text = std::fs::read_to_string(dir.join(format!("node-{id}.jsonl"))).unwrap();
    text.lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .filter(|e| e["ev"] == "state_checkpoint")
        .map(|e| {
            (
                e["log_index"].as_u64().unwrap(),
                e["digest"].as_u64().unwrap(),
            )
        })
        .collect()
}

/// The convergence check the analysis runs (D138): a node that missed part of the log still logs
/// every checkpoint, with the same digests as the nodes that never left.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_returning_node_logs_the_same_digests_at_the_same_indexes() {
    let dir = std::env::temp_dir().join(format!("rescuemesh-checkpoints-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let logs = dir.clone();
    let cluster = TestCluster::start_with(3, move |c| c.metrics.dir = Some(logs)).await;
    let leader = agreed_leader(&cluster, &[1, 2, 3]).await;
    let follower = (1..=3).find(|id| *id != leader).unwrap();

    let consensus = &cluster.node(leader).ctx.consensus;
    // One at a time, because this needs log entries: proposals that wait together share one (D133).
    let commits = |range: std::ops::Range<u64>| async move {
        for seq in range {
            consensus.propose(proposal(seq, leader)).await.unwrap();
        }
    };
    cluster.node(follower).ctx.gate.silence();
    wait_until(WAIT, "silenced follower stops serving", || {
        !serving(&cluster, follower)
    })
    .await;
    // Past a checkpoint while it is away, and past another after it is back.
    commits(0..CHECKPOINT_EVERY + 10).await;
    cluster.node(follower).ctx.gate.restore();
    wait_until(WAIT, "follower serving again", || {
        serving(&cluster, follower)
    })
    .await;
    commits(CHECKPOINT_EVERY + 10..2 * CHECKPOINT_EVERY + 20).await;
    wait_until(WAIT, "identical state on every node", || {
        same_state(&cluster)
    })
    .await;
    cluster.shutdown().await;

    let reference = checkpoints(&dir, leader);
    assert!(
        reference.contains_key(&CHECKPOINT_EVERY)
            && reference.contains_key(&(2 * CHECKPOINT_EVERY)),
        "{reference:?}"
    );
    for id in 1..=3 {
        assert_eq!(checkpoints(&dir, id), reference, "node {id}");
    }
    let _ = std::fs::remove_dir_all(&dir);
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

/// The leader a returning node would ask goes silent just as it returns. It has to finish with
/// whoever leads next, and still serves nothing until it has all that was committed (D135).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_node_still_rejoins_when_the_leader_goes_away_as_it_returns() {
    let cluster = TestCluster::start(3).await;
    let old = agreed_leader(&cluster, &[1, 2, 3]).await;
    let returning = (1..=3).find(|id| *id != old).unwrap();

    cluster.node(returning).ctx.gate.silence();
    wait_until(WAIT, "silenced follower stops serving", || {
        !serving(&cluster, returning)
    })
    .await;
    for seq in 0..20 {
        cluster
            .node(old)
            .ctx
            .consensus
            .propose(proposal(seq, old))
            .await
            .unwrap();
    }

    cluster.node(old).ctx.gate.silence();
    cluster.node(returning).ctx.gate.restore();
    wait_until(WAIT, "follower serving again", || {
        serving(&cluster, returning)
    })
    .await;
    assert_eq!(
        cluster.node(returning).ctx.state.with(|s| s.result_count()),
        20
    );
    assert_ne!(
        cluster.node(returning).ctx.consensus.status().leader,
        Some(old)
    );

    cluster.node(old).ctx.gate.restore();
    wait_until(WAIT, "old leader serving again", || serving(&cluster, old)).await;
    wait_until(WAIT, "identical state on every node", || {
        same_state(&cluster)
    })
    .await;
    cluster.shutdown().await;
}
