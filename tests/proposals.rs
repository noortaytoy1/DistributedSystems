//! The proposal queue on three real nodes: groups form under load and heal a returning node.
#![allow(clippy::unwrap_used)]

mod common;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{TestCluster, wait_until};
use rescuemesh::proto::{
    Capability, Category, Command, Incident, NodeId, Outcome, Plan, Point, ProposeError, RequestId,
    ResultProposal, UserId,
};
use rescuemesh::raft::proposals::GROUP_LIMIT;
use rescuemesh::raft::store;
use rescuemesh::raft::{Consensus, Role};

const WAIT: Duration = Duration::from_secs(120);

/// A result with every field at its longest on the wire, so these tests see the largest messages
/// that real commands can make.
fn big_result(client: u64, seq: u64, executor: NodeId) -> Command {
    Command::CommitResult(ResultProposal {
        request: RequestId {
            client: u64::MAX - client,
            seq: u64::MAX - seq,
        },
        incident: Incident {
            owner: UserId {
                client: u64::MAX,
                user: u32::MAX,
            },
            category: Category::TechnicalAssistance,
            zone: u16::MAX,
            location: Point {
                x: i32::MIN,
                y: i32::MIN,
            },
            severity: 5,
            needs: Capability::Rescue,
            reported_at_ms: u64::MAX,
            ttl_ms: u64::MAX,
        },
        executor,
        term: u64::MAX,
        plan: Plan {
            responder: None,
            cost: u64::MAX,
            priority: u32::MAX,
        },
        age_ms: u64::MAX,
        assigned_at_ms: u64::MAX,
    })
}

async fn agreed_leader(cluster: &TestCluster) -> NodeId {
    let mut leader = None;
    wait_until(WAIT, "one agreed leader", || {
        let views: Vec<_> = cluster
            .nodes
            .iter()
            .map(|n| n.ctx.consensus.status())
            .collect();
        let first = views[0].leader;
        let agreed = first.is_some() && views.iter().all(|s| s.leader == first);
        if agreed && views.iter().filter(|s| s.role == Role::Leader).count() == 1 {
            leader = first;
        }
        leader.is_some()
    })
    .await;
    leader.unwrap()
}

fn consensus(cluster: &TestCluster, id: NodeId) -> Arc<dyn Consensus> {
    cluster.node(id).ctx.consensus.clone()
}

/// Propose `count` commands through `via` at once and return how many reached a commit.
async fn burst(via: Arc<dyn Consensus>, client: u64, count: u64) -> u64 {
    let tasks: Vec<_> = (0..count)
        .map(|seq| {
            let c = via.clone();
            tokio::spawn(async move { c.propose(big_result(client, seq, 1)).await })
        })
        .collect();
    let mut committed = 0;
    for task in tasks {
        if matches!(task.await.unwrap(), Ok(Outcome::Committed(_))) {
            committed += 1;
        }
    }
    committed
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_burst_shares_log_entries_and_every_proposal_gets_its_own_answer() {
    let cluster = TestCluster::start(3).await;
    let leader = agreed_leader(&cluster).await;
    let follower = (1..=3).find(|id| *id != leader).unwrap();
    let before = consensus(&cluster, leader).status().last_log_index.unwrap();

    const PROPOSALS: u64 = 2_000;
    let (from_leader, from_follower) = tokio::join!(
        burst(consensus(&cluster, leader), 1, PROPOSALS / 2),
        burst(consensus(&cluster, follower), 2, PROPOSALS / 2),
    );
    assert_eq!(from_leader + from_follower, PROPOSALS);

    let entries = consensus(&cluster, leader).status().last_log_index.unwrap() - before;
    assert!(
        entries < PROPOSALS,
        "{PROPOSALS} proposals took {entries} log entries"
    );
    wait_until(WAIT, "equal digests", || {
        let views: Vec<_> = cluster
            .nodes
            .iter()
            .map(|n| n.ctx.state.with(|s| (s.result_count(), s.digest())))
            .collect();
        views[0].0 == PROPOSALS as usize && views.iter().all(|v| *v == views[0])
    })
    .await;
    cluster.shutdown().await;
}

/// The newest `raft_io` event this node has logged, once its log is flushed.
fn last_io_report(cluster: &TestCluster, dir: &Path, id: NodeId) -> Option<serde_json::Value> {
    cluster.node(id).ctx.log.flush();
    std::fs::read_to_string(dir.join(format!("node-{id}.jsonl")))
        .ok()?
        .lines()
        .rev()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|e| e["ev"] == "raft_io")
}

fn count(report: &serde_json::Value, field: &str) -> u64 {
    report[field].as_u64().unwrap()
}

/// A follower that knows the leader does not use its own queue: everything goes through the leader's
/// one queue, and so into its one write at a time.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_follower_forwards_straight_away_into_the_leaders_one_queue() {
    let dir = std::env::temp_dir().join(format!("rescuemesh-proposals-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let logs = dir.clone();
    let cluster = TestCluster::start_with(3, move |c| {
        c.metrics.dir = Some(logs);
        // Every node loads the campus at start (D164), through its own queue until it knows a
        // leader. Those writes are not what this test counts.
        c.campus.responders = 0;
    })
    .await;
    let leader = agreed_leader(&cluster).await;
    let follower = (1..=3).find(|id| *id != leader).unwrap();
    let term = consensus(&cluster, leader).status().term;

    let mut before = None;
    wait_until(WAIT, "a first report from the follower", || {
        before = last_io_report(&cluster, &dir, follower);
        before.is_some()
    })
    .await;
    let before = before.unwrap();

    const PROPOSALS: u64 = 300;
    let committed = burst(consensus(&cluster, follower), 3, PROPOSALS).await;
    assert_eq!(committed, PROPOSALS);

    let mut on_the_leader = None;
    wait_until(WAIT, "the leader's report of the burst", || {
        on_the_leader = last_io_report(&cluster, &dir, leader);
        on_the_leader
            .as_ref()
            .is_some_and(|r| count(r, "commands") >= PROPOSALS)
    })
    .await;
    let on_the_leader = on_the_leader.unwrap();
    let after = last_io_report(&cluster, &dir, follower).unwrap();
    // With a new vote in between, on a slow machine, the follower may rightly have had no leader
    // to forward to for a moment. Any node's term shows it, even a vote the others refused.
    if cluster
        .nodes
        .iter()
        .all(|n| n.ctx.consensus.status().term == term)
    {
        assert_eq!(count(&after, "writes"), count(&before, "writes"));
        assert_eq!(count(&after, "commands"), count(&before, "commands"));
    }
    assert!(
        count(&on_the_leader, "writes") < PROPOSALS,
        "{PROPOSALS} proposals took {} writes",
        count(&on_the_leader, "writes")
    );
    assert_eq!(count(&on_the_leader, "expired"), 0);
    assert_eq!(count(&on_the_leader, "write_timeouts"), 0);
    cluster.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_leader_that_cannot_commit_answers_timeout_at_the_deadline() {
    let cluster = TestCluster::start_with(3, |c| c.timers.propose_timeout_ms = 300).await;
    let leader = agreed_leader(&cluster).await;
    for id in (1..=3).filter(|id| *id != leader) {
        cluster.node(id).ctx.gate.silence();
    }
    let c = consensus(&cluster, leader);
    let started = Instant::now();
    let outcome = c.propose(big_result(1, 1, leader)).await;
    let took = started.elapsed();
    assert_eq!(outcome, Err(ProposeError::Timeout));
    assert!(
        took < Duration::from_secs(2),
        "the timeout is a cap, but this took {took:?}"
    );
    cluster.shutdown().await;
}

/// More full groups than one AppendEntries can carry, and more commands than the frame limit would
/// allow in one message if the store did not cap them: the returning node must catch up anyway.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_returning_follower_catches_up_over_full_groups() {
    let cluster = TestCluster::start_with(3, |c| c.timers.propose_timeout_ms = 60_000).await;
    let leader = agreed_leader(&cluster).await;
    let victim = (1..=3).find(|id| *id != leader).unwrap();
    cluster.node(victim).ctx.gate.silence();
    wait_until(WAIT, "the victim to stop serving", || {
        !cluster
            .node(victim)
            .ctx
            .serving
            .load(std::sync::atomic::Ordering::SeqCst)
    })
    .await;

    let entries_before = consensus(&cluster, leader).status().last_log_index.unwrap();
    // Fixed, not a multiple of GROUP_LIMIT: more commands than several messages carry (D234).
    let proposals = 16 * store::MAX_COMMANDS_PER_MESSAGE as u64;
    let committed = burst(consensus(&cluster, leader), 1, proposals).await;
    assert_eq!(committed, proposals);
    let entries = consensus(&cluster, leader).status().last_log_index.unwrap() - entries_before;
    eprintln!("{proposals} proposals made {entries} log entries");
    let full_groups_in_a_message = (store::MAX_COMMANDS_PER_MESSAGE / GROUP_LIMIT) as u64;
    assert!(
        entries > full_groups_in_a_message,
        "needs more entries than one message carries: {entries}"
    );

    cluster.node(victim).ctx.gate.restore();
    let started = Instant::now();
    wait_until(WAIT, "the victim to catch up", || {
        let views: Vec<_> = cluster
            .nodes
            .iter()
            .map(|n| n.ctx.state.with(|s| (s.result_count(), s.digest())))
            .collect();
        views[0].0 == proposals as usize && views.iter().all(|v| *v == views[0])
    })
    .await;
    eprintln!("caught up in {:?}", started.elapsed());
    cluster.shutdown().await;
}
