//! The cloud nodes are peers: the Raft leader orders the log and decides nothing else (D163). Any
//! node can load the campus (D164), and holding the role gives a node no say in who plans.
//!
//! Nothing here assumes which node leads, or that a leader stays. The role tests use real planning
//! times, because at `plan_work = 0` every idle score ties and they would prove only the tie-break.
#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::{TestCluster, hold_jobs, wait_until};
use rescuemesh::campus;
use rescuemesh::client::{MeshClient, Submitted};
use rescuemesh::config::{AssignmentPolicy, Config};
use rescuemesh::metrics::{EventLog, now_us};
use rescuemesh::proto::{NodeId, UserId};
use rescuemesh::raft::Role;

/// Only ever reached by a failing test.
const WAIT: Duration = Duration::from_secs(30);

/// Jobs held on a node that must look busy.
const HELD: usize = 200;

/// Bidding with real planning times, and nothing silenced unless a test does it.
///
/// The sums, for a score of (jobs + 1) x service_us / threads, with the default 4 threads:
///
/// - Idle, not planned yet: 1 x 1000 / 4 = 250.
/// - Held, not planned yet: 201 x 1000 / 4 = 50250. An idle node that has planned stays under that
///   unless its planning time has reached 201 ms.
/// - Held, planned: at least 201 x 100 / 4 = 5025, because 100 units of work are 100,000 rounds, and
///   a round cannot take under 1 ns. So it always scores above an idle node that has not planned.
///
/// A plan whose node "wins" while held waits behind the held jobs, so it cannot commit either way.
fn with_real_planning(c: &mut Config) {
    c.policy.assignment = AssignmentPolicy::Bidding;
    c.policy.initial_service_us = 1000;
    c.workers.plan_work = 100;
    c.workers.queue_capacity = 2 * HELD;
    // Wide, so a busy machine still gets every bid in before a round closes.
    c.timers.bid_window_ms = 100;
    c.faults.interval_min_ms = 100_000_000;
    c.faults.interval_max_ms = 100_000_000;
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

/// A leader and the campus on every node, and nothing planned yet.
async fn ready_cluster() -> (TestCluster, NodeId) {
    let cluster = TestCluster::start_with(3, with_real_planning).await;
    let leader = agreed_leader(&cluster, &[1, 2, 3]).await;
    wait_until(WAIT, "the campus on every node", || {
        cluster.nodes.iter().all(|n| n.ctx.status().responders > 0)
    })
    .await;
    (cluster, leader)
}

async fn report(client: &MeshClient, cluster: &TestCluster) -> Submitted {
    let owner = UserId { client: 7, user: 0 };
    let incident = campus::unseeded_incident(&cluster.config.campus, owner, now_us() / 1000);
    client.submit(incident).await.unwrap()
}

/// `hold_jobs` counts on the pool not changing under it, and a plan can outlast its reply.
async fn hold_when_idle(cluster: &TestCluster, id: NodeId) -> Vec<std::sync::mpsc::Sender<()>> {
    let node = cluster.node(id);
    wait_until(WAIT, "the worker pool to drain", || {
        let load = node.ctx.workers.load();
        load.queued + load.running == 0
    })
    .await;
    hold_jobs(node, HELD).await
}

fn others(leader: NodeId) -> Vec<NodeId> {
    (1..=3).filter(|id| *id != leader).collect()
}

fn log_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rescuemesh-peers-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// How many nodes have finished proposing the campus so far. A `commit` line is written only when
/// the proposal returns, and shutdown aborts one still running, so this is read on a live cluster.
fn nodes_that_loaded_the_campus(cluster: &TestCluster, dir: &Path) -> usize {
    cluster
        .nodes
        .iter()
        .filter(|node| {
            node.ctx.log.flush();
            std::fs::read_to_string(dir.join(format!("node-{}.jsonl", node.ctx.id))).is_ok_and(
                |text| {
                    text.lines().any(|line| {
                        line.contains(r#""ev":"commit""#)
                            && line.contains(r#""kind":"register_responders""#)
                    })
                },
            )
        })
        .count()
}

/// Every node holds `total` responders, and the same applied index and digest.
async fn same_campus_everywhere(cluster: &TestCluster, total: u64) {
    wait_until(WAIT, "the same state on every node", || {
        let status: Vec<_> = cluster.nodes.iter().map(|n| n.ctx.status()).collect();
        status.iter().all(|s| {
            s.responders == total
                && s.digest == status[0].digest
                && s.last_applied == status[0].last_applied
        })
    })
    .await;
}

/// No node is picked to load the responders. Several propose them, and the extra entries change
/// nothing. A lower bound only: how many land depends on timing and retries.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn any_node_seeds_the_campus() {
    let dir = log_dir("seeds");
    let metrics = dir.clone();
    let cluster = TestCluster::start_with(3, move |c| c.metrics.dir = Some(metrics)).await;

    wait_until(WAIT, "two nodes to have proposed the campus", || {
        nodes_that_loaded_the_campus(&cluster, &dir) >= 2
    })
    .await;

    let total = cluster.config.campus.responders;
    same_campus_everywhere(&cluster, u64::from(total)).await;
    for node in &cluster.nodes {
        let free = node.ctx.state.with(|s| s.free_responders().len());
        assert_eq!(free, total as usize, "node {}", node.ctx.id);
    }
    cluster.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// Only node 3's file has a campus, and node 3 has the longest election timeout (D115), so it is
/// very rarely the node leading. If only a leader could load the list, this would wait for ever.
/// Every node ends up with the list, because the log decides it and not each node's own file.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_log_decides_the_campus_whichever_node_proposes_it() {
    let cluster = TestCluster::start_each(
        3,
        |_| {},
        |id, c| {
            if id != 3 {
                c.campus.responders = 0;
            }
        },
    )
    .await;

    let total = cluster.config.campus.responders;
    same_campus_everywhere(&cluster, u64::from(total)).await;
    cluster.shutdown().await;
}

/// The leader bids like anyone. Under leader dispatch the first half hangs: the leader must plan
/// everything, and its plans wait behind the held jobs.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_busy_leader_loses_and_an_idle_leader_wins() {
    let (cluster, leader) = ready_cluster().await;
    let client = MeshClient::new(71, &cluster.config, EventLog::disabled("c"));

    let held = hold_when_idle(&cluster, leader).await;
    let first = report(&client, &cluster).await;
    assert_ne!(first.executor, leader);
    // Lost on its score, with nobody having planned yet, and not just moved on by a takeover.
    let round = (first.committed.as_ref().unwrap().request, 0);
    let ranked = cluster
        .node(first.executor)
        .ctx
        .bids
        .with_rounds(|r| r.ranked_bids(round));
    let top = ranked.and_then(|bids| bids.first().map(|&(node, _)| node));
    assert_ne!(top, Some(leader), "the busy leader ranked first");
    for _ in 0..5 {
        assert_ne!(report(&client, &cluster).await.executor, leader);
    }
    drop(held);

    let followers = others(leader);
    let held = [
        hold_when_idle(&cluster, followers[0]).await,
        hold_when_idle(&cluster, followers[1]).await,
    ];
    assert_eq!(report(&client, &cluster).await.executor, leader);
    drop(held);
    cluster.shutdown().await;
}

/// The role moves when its holder goes silent, the rest carry on, and the old holder comes back
/// with no role and the same chance as anyone. Silenced by hand, so the checks in the middle land
/// inside the silence however slow the election is.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_leader_role_moves_and_its_old_holder_is_ordinary() {
    let (cluster, old) = ready_cluster().await;
    let old_term = cluster.node(old).ctx.consensus.status().term;
    let live = others(old);
    let client = MeshClient::new(72, &cluster.config, EventLog::disabled("c"));

    cluster.node(old).ctx.gate.silence();
    let new = agreed_leader(&cluster, &live).await;
    assert_ne!(new, old);
    assert!(cluster.node(new).ctx.consensus.status().term > old_term);
    let served = report(&client, &cluster).await;
    assert!(!served.provisional);
    assert_ne!(served.executor, old);

    cluster.node(old).ctx.gate.restore();
    wait_until(WAIT, "the old leader serving, as a follower", || {
        let ctx = &cluster.node(old).ctx;
        let status = ctx.consensus.status();
        ctx.serving.load(Ordering::SeqCst)
            && status.term > old_term
            && status.role == Role::Follower
            && status.leader.is_some_and(|leader| leader != old)
    })
    .await;

    let held = [
        hold_when_idle(&cluster, live[0]).await,
        hold_when_idle(&cluster, live[1]).await,
    ];
    let done = report(&client, &cluster).await;
    assert_eq!(done.executor, old);
    assert!(done.committed.unwrap().term > old_term);
    drop(held);
    cluster.shutdown().await;
}
