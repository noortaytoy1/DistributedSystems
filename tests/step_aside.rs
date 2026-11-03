//! Part A: a busy leader steps aside on three real nodes, and a normal vote picks another (D218).
//! `ctl skew` makes the leader's planning slow, so its bids rise. The bids need load, so each test
//! that wants the rule to read anything keeps a few users running.
#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use common::{TestCluster, wait_until};
use rescuemesh::client::{MeshClient, control};
use rescuemesh::config::{AssignmentPolicy, Config};
use rescuemesh::metrics::{EventLog, now_ms};
use rescuemesh::node::transport::ClientPort;
use rescuemesh::node::{self, NodeHandle};
use rescuemesh::proto::{
    Capability, Category, ControlReply, ControlRequest, Incident, NodeId, Point, Term, UserId,
};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tokio::time::Instant;

const WAIT: Duration = Duration::from_secs(40);

/// Well over any node's planning at `plan_work` 0, and over the floor in debug and in release: about
/// 20 ms of planning in release, so about 5 ms of expected wait.
const SKEW: u32 = 8_000;

/// How long a plan takes with that skew on, in release: see [`skew`].
const SKEWED_PLAN: Duration = Duration::from_millis(20);

/// With the default timers every follower has started a vote well within this if nothing resets its
/// timer: about 1.1 s at the longest (D115).
const SETTLE: Duration = Duration::from_millis(2_000);

const FOR_MS: u64 = 1_000;

/// One second's check on top of the time in a row, and one more because each node starts its
/// cooldown at its own next check.
const CHECK_SLACK: Duration = Duration::from_secs(2);

fn config(c: &mut Config, dir: &Path, cooldown_ms: u64) {
    c.policy.assignment = AssignmentPolicy::Bidding;
    c.policy.leader_overload_ratio = 3;
    c.policy.leader_overload_for_ms = FOR_MS;
    c.policy.leader_overload_cooldown_ms = cooldown_ms;
    c.metrics.dir = Some(dir.to_path_buf());
    c.faults.enabled = false;
}

fn log_dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rescuemesh-step-aside-{test}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

async fn agreed_leader(cluster: &TestCluster) -> (NodeId, Term) {
    let mut found = None;
    wait_until(WAIT, "a leader every node agrees on", || {
        let views: Vec<_> = cluster
            .nodes
            .iter()
            .map(|n| n.ctx.consensus.status())
            .collect();
        let (leader, term) = (views[0].leader, views[0].term);
        if leader.is_some() && views.iter().all(|s| s.leader == leader && s.term == term) {
            found = leader.map(|l| (l, term));
        }
        found.is_some()
    })
    .await;
    found.unwrap()
}

/// Waits until every node agrees on a leader other than `old`.
async fn new_leader(cluster: &TestCluster, old: NodeId) -> (NodeId, Term) {
    let mut found = None;
    wait_until(WAIT, "a new leader every node agrees on", || {
        let views: Vec<_> = cluster
            .nodes
            .iter()
            .map(|n| n.ctx.consensus.status())
            .collect();
        let (leader, term) = (views[0].leader, views[0].term);
        if leader.is_some_and(|l| l != old)
            && views.iter().all(|s| s.leader == leader && s.term == term)
        {
            found = leader.map(|l| (l, term));
        }
        found.is_some()
    })
    .await;
    found.unwrap()
}

/// Asserts that every node keeps seeing `leader` in `term` for all of `span`.
async fn stays(cluster: &TestCluster, leader: NodeId, term: Term, span: Duration) {
    let until = Instant::now() + span;
    while Instant::now() < until {
        for n in &cluster.nodes {
            let s = n.ctx.consensus.status();
            assert_eq!(
                (s.leader, s.term),
                (Some(leader), term),
                "node {} after {:?} of {span:?}",
                n.ctx.id,
                span - (until - Instant::now()),
            );
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// `ctl skew`, and what it does to the node's bid once the node has planned with it.
///
/// A skew reaches a bid only through a job the node plans. With few users rounds do not overlap, so
/// the node with the lowest planning time wins them all and the others' averages stay as they were.
/// A node whose average was not the lowest may then never plan again: on 2 CPUs a leader won 0 of
/// about 2100 rounds in 40 s, with its bid stuck at 10 µs, and the skew never showed. So the test
/// does not leave that to chance. Under real load rounds overlap and every node wins some.
async fn skew(cluster: &TestCluster, node: NodeId, extra_work: u32) {
    let addr = &cluster.config.node(node).unwrap().client_addr;
    let request = ControlRequest::SkewLoad { extra_work };
    let answer = control(addr, request, Duration::from_secs(5))
        .await
        .unwrap();
    assert!(matches!(answer, ControlReply::Done), "{answer:?}");
    let plan = if extra_work > 0 {
        SKEWED_PLAN
    } else {
        Duration::from_micros(1)
    };
    // Enough for the running average to reach it, at 1/8 of the way each.
    for _ in 0..64 {
        cluster.node(node).ctx.bids.record_service(plan);
    }
}

/// A few users on the real client middleware, each sending a report, waiting for the answer and
/// pausing. Ends when dropped.
struct Load {
    users: JoinSet<()>,
    answered: Arc<AtomicU64>,
    gave_up: Arc<AtomicU64>,
}

impl Load {
    const PAUSE: Duration = Duration::from_millis(50);

    fn start(cluster: &TestCluster, users: u32) -> Self {
        let client = Arc::new(MeshClient::new(
            9,
            &cluster.config,
            EventLog::disabled("load"),
        ));
        let answered = Arc::new(AtomicU64::new(0));
        let gave_up = Arc::new(AtomicU64::new(0));
        let mut set = JoinSet::new();
        for user in 0..users {
            let (client, answered, gave_up) = (client.clone(), answered.clone(), gave_up.clone());
            set.spawn(async move {
                loop {
                    let incident = Incident {
                        owner: UserId { client: 9, user },
                        category: Category::MedicalAssistance,
                        zone: 0,
                        location: Point { x: 10, y: 20 },
                        severity: 1,
                        needs: Capability::Medical,
                        reported_at_ms: now_ms(),
                        ttl_ms: 60_000,
                    };
                    match client.submit(incident).await {
                        Ok(_) => answered.fetch_add(1, Ordering::SeqCst),
                        Err(_) => gave_up.fetch_add(1, Ordering::SeqCst),
                    };
                    tokio::time::sleep(Self::PAUSE).await;
                }
            });
        }
        Self {
            users: set,
            answered,
            gave_up,
        }
    }

    fn answered(&self) -> u64 {
        self.answered.load(Ordering::SeqCst)
    }

    /// Until `more` further answers have arrived, so the bids are flowing.
    async fn wait_for_answers(&self, more: u64) {
        let target = self.answered() + more;
        wait_until(WAIT, "more answers", || self.answered() >= target).await;
    }

    /// Stops the users. Nothing in these tests may lose a report.
    async fn finish(mut self) {
        self.users.shutdown().await;
        assert_eq!(self.gave_up.load(Ordering::SeqCst), 0, "a client gave up");
    }
}

/// Every event of kind `ev` that node `id` has logged so far.
fn events(cluster: &TestCluster, dir: &Path, id: NodeId, ev: &str) -> Vec<Value> {
    cluster.node(id).ctx.log.flush();
    let path = dir.join(format!("node-{id}.jsonl"));
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|e| e["ev"] == ev)
        .collect()
}

fn all_events(cluster: &TestCluster, dir: &Path, ev: &str) -> Vec<Value> {
    (1..=3)
        .flat_map(|id| events(cluster, dir, id, ev))
        .collect()
}

async fn wait_for_equal_state(cluster: &TestCluster) {
    wait_until(WAIT, "every node to hold the same state", || {
        let views: Vec<_> = cluster
            .nodes
            .iter()
            .map(|n| n.ctx.state.with(|s| (s.last_index(), s.digest())))
            .collect();
        views.iter().all(|v| *v == views[0])
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_skewed_leader_steps_aside_and_the_leader_changes_once() {
    const COOLDOWN_MS: u64 = 5_000;
    let dir = log_dir("once");
    let cluster = TestCluster::start_with(3, |c| config(c, &dir, COOLDOWN_MS)).await;
    let (old, old_term) = agreed_leader(&cluster).await;
    let load = Load::start(&cluster, 4);
    load.wait_for_answers(20).await;

    skew(&cluster, old, SKEW).await;
    let (leader, term) = new_leader(&cluster, old).await;
    assert!(term > old_term);

    let aside = events(&cluster, &dir, old, "leader_stepped_aside");
    assert_eq!(aside.len(), 1, "{aside:?}");
    assert_eq!(aside[0]["term"], old_term);
    let score = aside[0]["score"].as_u64().unwrap();
    let peers = aside[0]["peer_scores"].as_object().unwrap();
    assert_eq!(peers.len(), 2, "{peers:?}");
    let lowest = peers.values().map(|v| v.as_u64().unwrap()).min().unwrap();
    assert!(score >= 3 * lowest, "{score} against {lowest}");
    assert!(score > 1_000, "{score} is under the floor");

    wait_until(WAIT, "the old leader to log how it ended", || {
        !events(&cluster, &dir, old, "leader_step_aside_ended").is_empty()
    })
    .await;
    let ended = events(&cluster, &dir, old, "leader_step_aside_ended");
    assert_eq!(ended[0]["outcome"], "changed", "{ended:?}");
    assert_eq!(ended[0]["new_leader"], leader);
    assert_eq!(ended[0]["new_term"], term);
    assert!(ended[0]["waited_ms"].as_u64().unwrap() < 3_000, "{ended:?}");

    // The skew stays on, on what is now a follower, past the time a second change could happen.
    // It does not: the new leader is not the busy one.
    stays(
        &cluster,
        leader,
        term,
        Duration::from_millis(COOLDOWN_MS + FOR_MS) + CHECK_SLACK,
    )
    .await;
    assert_eq!(
        all_events(&cluster, &dir, "leader_stepped_aside").len(),
        1,
        "the leader changed once"
    );
    load.wait_for_answers(20).await;
    load.finish().await;
    wait_for_equal_state(&cluster).await;
    cluster.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn even_load_changes_nothing() {
    const COOLDOWN_MS: u64 = 3_000;
    let dir = log_dir("even");
    let cluster = TestCluster::start_with(3, |c| {
        config(c, &dir, COOLDOWN_MS);
        // Over the floor on every node, so only the comparison between them keeps this quiet.
        c.workers.plan_work = SKEW;
        c.policy.leader_overload_for_ms = 2_000;
    })
    .await;
    let (leader, term) = agreed_leader(&cluster).await;
    let load = Load::start(&cluster, 4);
    load.wait_for_answers(10).await;

    stays(&cluster, leader, term, Duration::from_secs(8)).await;

    assert!(all_events(&cluster, &dir, "leader_stepped_aside").is_empty());
    let bids = events(&cluster, &dir, leader, "bid_sent");
    let high = bids.iter().filter(|b| b["score"].as_u64().unwrap() > 1_000);
    assert!(high.count() > 0, "this test never got over the floor");
    load.finish().await;
    cluster.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}

/// A busy follower is the bid's business. The rule asks whether the leader is busier than its least
/// busy peer, so a peer that is busier proves nothing, and a follower has nothing to step aside from.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_skewed_follower_moves_nothing() {
    const COOLDOWN_MS: u64 = 3_000;
    let dir = log_dir("skewed-follower");
    let cluster = TestCluster::start_with(3, |c| config(c, &dir, COOLDOWN_MS)).await;
    let (leader, term) = agreed_leader(&cluster).await;
    let follower = (1..=3).find(|id| *id != leader).unwrap();
    let load = Load::start(&cluster, 4);
    load.wait_for_answers(20).await;

    skew(&cluster, follower, SKEW).await;
    // Past the cooldown and the time in a row, as in the test where the leader moves.
    let span = Duration::from_millis(COOLDOWN_MS + FOR_MS) + CHECK_SLACK;
    stays(&cluster, leader, term, span).await;

    assert!(all_events(&cluster, &dir, "leader_stepped_aside").is_empty());
    let bids = events(&cluster, &dir, follower, "bid_sent");
    assert!(
        bids.iter().any(|b| b["score"].as_u64().unwrap() > 1_000),
        "the skew never showed, so this tested nothing"
    );
    load.finish().await;
    cluster.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ratio_0_turns_it_off() {
    let dir = log_dir("off");
    let cluster = TestCluster::start_with(3, |c| {
        config(c, &dir, 3_000);
        c.policy.leader_overload_ratio = 0;
    })
    .await;
    let (leader, term) = agreed_leader(&cluster).await;
    let load = Load::start(&cluster, 4);
    load.wait_for_answers(20).await;
    skew(&cluster, leader, SKEW).await;

    // Long past the cooldown and the time in a row.
    stays(&cluster, leader, term, Duration::from_secs(7)).await;

    assert!(all_events(&cluster, &dir, "leader_stepped_aside").is_empty());
    let score = |b: &Value| b["score"].as_u64().unwrap();
    let bids = events(&cluster, &dir, leader, "bid_sent");
    assert!(
        bids.iter().any(|b| score(b) > 1_000),
        "the skew never showed, so this tested nothing"
    );
    load.finish().await;
    cluster.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_cooldown_holds_back_a_second_change() {
    const COOLDOWN: Duration = Duration::from_secs(6);
    let dir = log_dir("cooldown");
    let cluster =
        TestCluster::start_with(3, |c| config(c, &dir, COOLDOWN.as_millis() as u64)).await;
    let (first, _) = agreed_leader(&cluster).await;
    let load = Load::start(&cluster, 4);
    load.wait_for_answers(20).await;

    skew(&cluster, first, SKEW).await;
    let (second, term) = new_leader(&cluster, first).await;
    let changed_at = Instant::now();

    // The new leader is busy at once, and must still wait out the whole cooldown.
    skew(&cluster, second, SKEW).await;
    stays(&cluster, second, term, COOLDOWN).await;
    assert_eq!(all_events(&cluster, &dir, "leader_stepped_aside").len(), 1);

    // And once it has, it moves on: a cooldown stops a storm, not the rule.
    let (third, _) = new_leader(&cluster, second).await;
    assert_ne!(third, second);
    assert!(changed_at.elapsed() >= COOLDOWN);
    assert_eq!(all_events(&cluster, &dir, "leader_stepped_aside").len(), 2);
    load.finish().await;
    wait_for_equal_state(&cluster).await;
    cluster.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}

/// Nobody can take over, because the followers may not stand. The leader must find its own way back.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_step_aside_nobody_answers_times_out_and_the_leader_carries_on() {
    // Longer than this test's own pause with no load, or that pause could hide a second try too soon.
    const COOLDOWN: Duration = Duration::from_secs(9);
    let dir = log_dir("timeout");
    let cluster =
        TestCluster::start_with(3, |c| config(c, &dir, COOLDOWN.as_millis() as u64)).await;
    let (leader, term) = agreed_leader(&cluster).await;
    let followers: Vec<NodeId> = (1..=3).filter(|id| *id != leader).collect();
    for id in &followers {
        cluster.node(*id).ctx.consensus.set_elect(false);
    }
    let load = Load::start(&cluster, 4);
    load.wait_for_answers(20).await;
    skew(&cluster, leader, SKEW).await;

    wait_until(WAIT, "the try to end", || {
        !events(&cluster, &dir, leader, "leader_step_aside_ended").is_empty()
    })
    .await;
    let ended = events(&cluster, &dir, leader, "leader_step_aside_ended");
    assert_eq!(ended[0]["outcome"], "timed_out", "{ended:?}");
    assert_eq!(ended[0]["new_term"], term);
    assert_eq!(ended[0]["new_leader"], leader);
    assert!(ended[0]["waited_ms"].as_u64().unwrap() >= 3_000);

    // The held writes went out, and new ones commit.
    load.wait_for_answers(20).await;
    assert_eq!(all_events(&cluster, &dir, "leader_stepped_aside").len(), 1);

    // With no writes left to hide them, only heartbeats now keep the followers from standing.
    for id in &followers {
        cluster.node(*id).ctx.consensus.set_elect(true);
    }
    load.finish().await;
    stays(&cluster, leader, term, SETTLE).await;

    // A full cooldown later it tries again, and now a follower can answer.
    let load = Load::start(&cluster, 4);
    let (next, _) = new_leader(&cluster, leader).await;
    assert_ne!(next, leader);
    wait_until(WAIT, "the second try to end", || {
        events(&cluster, &dir, leader, "leader_step_aside_ended").len() == 2
    })
    .await;
    let ended = events(&cluster, &dir, leader, "leader_step_aside_ended");
    assert_eq!(ended[1]["outcome"], "changed", "{ended:?}");
    let aside = events(&cluster, &dir, leader, "leader_stepped_aside");
    let time = |event: &Value| Duration::from_micros(event["ts_us"].as_u64().unwrap());
    let apart = time(&aside[1]) - time(&ended[0]);
    assert!(apart >= COOLDOWN, "the next try came after only {apart:?}");
    load.finish().await;
    wait_for_equal_state(&cluster).await;
    cluster.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}

/// A silenced peer leaves a leader with one vote to spare and nobody to spare it for: the rule must
/// wait until the peer is back and bidding again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_silenced_peer_stops_it_until_that_peer_is_back() {
    const COOLDOWN_MS: u64 = 3_000;
    let dir = log_dir("silenced-peer");
    let cluster = TestCluster::start_with(3, |c| config(c, &dir, COOLDOWN_MS)).await;
    let (leader, term) = agreed_leader(&cluster).await;
    let peer = (1..=3).find(|id| *id != leader).unwrap();
    let load = Load::start(&cluster, 4);
    load.wait_for_answers(20).await;
    cluster.node(peer).ctx.gate.silence();
    skew(&cluster, leader, SKEW).await;

    // Well past what the rule needs if nothing stopped it.
    stays_for_the_leader(&cluster, leader, term, Duration::from_secs(6)).await;
    assert!(all_events(&cluster, &dir, "leader_stepped_aside").is_empty());

    cluster.node(peer).ctx.gate.restore();
    let (next, _) = new_leader(&cluster, leader).await;
    assert_ne!(next, leader);
    assert_eq!(all_events(&cluster, &dir, "leader_stepped_aside").len(), 1);
    load.finish().await;
    wait_for_equal_state(&cluster).await;
    cluster.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}

/// With nobody able to take over, the leader stays quiet long enough to be silenced in the middle of
/// it. Its try must end at once, and it must come back keeping its followers' timers from running out.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_leader_silenced_while_quiet_comes_back_with_heartbeats_on() {
    let dir = log_dir("silenced-while-quiet");
    let cluster = TestCluster::start_with(3, |c| config(c, &dir, 3_000)).await;
    let (leader, term) = agreed_leader(&cluster).await;
    let followers: Vec<NodeId> = (1..=3).filter(|id| *id != leader).collect();
    for id in &followers {
        cluster.node(*id).ctx.consensus.set_elect(false);
    }
    let load = Load::start(&cluster, 4);
    load.wait_for_answers(20).await;
    skew(&cluster, leader, SKEW).await;

    wait_until(WAIT, "the leader to go quiet", || {
        !events(&cluster, &dir, leader, "leader_stepped_aside").is_empty()
    })
    .await;
    cluster.node(leader).ctx.gate.silence();
    wait_until(WAIT, "the try to end", || {
        !events(&cluster, &dir, leader, "leader_step_aside_ended").is_empty()
    })
    .await;
    let ended = events(&cluster, &dir, leader, "leader_step_aside_ended");
    assert_eq!(ended[0]["outcome"], "stopped", "{ended:?}");
    assert!(ended[0]["waited_ms"].as_u64().unwrap() < 3_000, "{ended:?}");

    cluster.node(leader).ctx.gate.restore();
    skew(&cluster, leader, 0).await;
    wait_until(WAIT, "the leader to serve again", || {
        cluster.node(leader).ctx.serving.load(Ordering::SeqCst)
    })
    .await;
    // Held writes went out, and nobody had to give up on the way.
    load.wait_for_answers(20).await;
    load.finish().await;

    // With no writes left to hide them, only heartbeats now keep the followers from standing.
    for id in &followers {
        cluster.node(*id).ctx.consensus.set_elect(true);
    }
    stays(&cluster, leader, term, SETTLE).await;
    wait_for_equal_state(&cluster).await;
    cluster.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}

/// Bind a port a stopped node just gave up, which may take a moment.
async fn restarted(config: &Config, id: NodeId) -> NodeHandle {
    let me = config.node(id).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let peer = loop {
        match TcpListener::bind(&me.peer_addr).await {
            Ok(listener) => break listener,
            Err(e) if Instant::now() > deadline => panic!("cannot bind again: {e}"),
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    };
    let client = loop {
        match ClientPort::bind(&me.client_addr).await {
            Ok(port) => break port,
            Err(e) if Instant::now() > deadline => panic!("cannot bind again: {e}"),
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    };
    node::start_with(config.clone(), id, peer, client)
        .await
        .unwrap()
}

/// Killed in the middle of going quiet, with nobody able to take over, so the same node leads again
/// after its restart: openraft resumes from its saved vote, in the same term. Going quiet lives only
/// in the process, so it must lead with heartbeats on.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_leader_killed_while_quiet_leads_with_heartbeats_on_after_a_restart() {
    let dir = log_dir("killed-while-quiet");
    let saved = dir.join("saved");
    let mut cluster = TestCluster::start_with(3, |c| {
        config(c, &dir, 3_000);
        c.storage.dir = Some(saved.clone());
    })
    .await;
    let (leader, term) = agreed_leader(&cluster).await;
    let followers: Vec<NodeId> = (1..=3).filter(|id| *id != leader).collect();
    for id in &followers {
        cluster.node(*id).ctx.consensus.set_elect(false);
    }
    let load = Load::start(&cluster, 4);
    load.wait_for_answers(20).await;
    skew(&cluster, leader, SKEW).await;

    wait_until(WAIT, "the leader to go quiet", || {
        !events(&cluster, &dir, leader, "leader_stepped_aside").is_empty()
    })
    .await;
    let at = (leader - 1) as usize;
    let killed = cluster.nodes.remove(at);
    killed.shutdown().await;
    let config = cluster.config.clone();
    let back = restarted(&config, leader).await;
    cluster.nodes.insert(at, back);

    let mut new_term = term;
    wait_until(WAIT, "every node to follow it again", || {
        let views: Vec<_> = cluster
            .nodes
            .iter()
            .map(|n| n.ctx.consensus.status())
            .collect();
        new_term = views[0].term;
        new_term >= term
            && views
                .iter()
                .all(|s| s.leader == Some(leader) && s.term == new_term)
    })
    .await;
    wait_until(WAIT, "it to serve again", || {
        cluster.node(leader).ctx.serving.load(Ordering::SeqCst)
    })
    .await;
    load.wait_for_answers(20).await;
    // The old process took its counters with it, and the client saw no answer lost for good.
    load.finish().await;

    for id in &followers {
        cluster.node(*id).ctx.consensus.set_elect(true);
    }
    stays(&cluster, leader, new_term, SETTLE).await;
    wait_for_equal_state(&cluster).await;
    cluster.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}

/// Every node that closes a round learns the silence it named, whichever node goes silent.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_node_knows_of_a_drawn_silence_until_it_is_over() {
    let dir = log_dir("drawn-silence");
    let cluster = TestCluster::start_with(3, |c| {
        config(c, &dir, 3_000);
        c.faults.enabled = true;
        c.faults.interval_min_ms = 3_000;
        c.faults.interval_max_ms = 4_000;
        c.faults.silence_min_ms = 2_000;
        c.faults.silence_max_ms = 3_000;
    })
    .await;
    wait_until(WAIT, "a draw that named a node", || {
        cluster
            .nodes
            .iter()
            .all(|n| n.ctx.fault_election.drawn_silence_going_on(Instant::now()))
    })
    .await;
    wait_until(WAIT, "the silence to be over", || {
        cluster
            .nodes
            .iter()
            .any(|n| !n.ctx.fault_election.drawn_silence_going_on(Instant::now()))
    })
    .await;
    cluster.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}

/// Like [`stays`], for the two nodes that can still hear each other: a silenced node's own view of
/// the term stands still.
async fn stays_for_the_leader(cluster: &TestCluster, leader: NodeId, term: Term, span: Duration) {
    let until = Instant::now() + span;
    while Instant::now() < until {
        let s = cluster.node(leader).ctx.consensus.status();
        assert_eq!((s.leader, s.term), (Some(leader), term));
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
