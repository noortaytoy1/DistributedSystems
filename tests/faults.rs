//! Part C: the fault election on three real nodes (D158, D159).
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use common::{TestCluster, wait_until};
use rescuemesh::client::MeshClient;
use rescuemesh::config::{AssignmentPolicy, Config};
use rescuemesh::metrics::{Event, EventLog};
use rescuemesh::node::faults;
use rescuemesh::node::transport::{FaultGate, PeerClient};
use rescuemesh::proto::{
    Capability, FaultBid, Incident, NodeId, PeerBody, PeerEnvelope, PeerReply, Point,
};
use tokio::task::JoinSet;
use tokio::time::Instant;

/// Only ever reached by a failing test: rounds cost little, but a busy machine can stretch them.
const WAIT: Duration = Duration::from_secs(30);

/// A seed for these tests. Node 2's ticket in round 1 is above 0 (checked where it matters).
const SEED: u64 = 7;

/// Short timings so a test sees several rounds in a few seconds. A round with a node still catching
/// up lasts 2 x `peer_rpc_timeout_ms`, so that is short too. The shortest wait is no shorter than
/// the longest silence, as validation requires.
fn fast_faults(c: &mut Config) {
    c.faults.interval_min_ms = 300;
    c.faults.interval_max_ms = 400;
    c.faults.silence_min_ms = 100;
    c.faults.silence_max_ms = 300;
    c.faults.seed = Some(SEED);
    c.timers.peer_rpc_timeout_ms = 200;
}

/// Rounds about once a second that give up quickly, for tests where none may finish.
fn doomed_rounds(c: &mut Config) {
    c.faults.interval_min_ms = 1_000;
    c.faults.interval_max_ms = 1_100;
    c.faults.silence_min_ms = 100;
    c.faults.silence_max_ms = 1_000;
    c.timers.peer_rpc_timeout_ms = 100;
}

/// No node's timer fires within a test: bids reach a node only as a test sends them.
fn timers_never_fire(c: &mut Config) {
    c.faults.interval_min_ms = 100_000_000;
    c.faults.interval_max_ms = 100_000_000;
    c.faults.seed = Some(SEED);
}

fn silenced_count(cluster: &TestCluster) -> usize {
    cluster
        .nodes
        .iter()
        .filter(|n| n.ctx.gate.is_silenced())
        .count()
}

fn rounds_left(cluster: &TestCluster, id: NodeId) -> u64 {
    cluster
        .node(id)
        .ctx
        .fault_election
        .with_rounds(|r| r.left())
}

/// Watches `faults` gates close, and checks at every look that two are never closed together.
async fn wait_for_faults(cluster: &TestCluster, faults: usize) {
    let deadline = Instant::now() + WAIT;
    let mut seen = 0;
    let mut was_silenced = [false; 3];
    while seen < faults {
        assert!(Instant::now() < deadline, "only {seen} faults in {WAIT:?}");
        assert!(silenced_count(cluster) <= 1, "two nodes silenced at once");
        for id in 1..=3u64 {
            let now = cluster.node(id).ctx.gate.is_silenced();
            let before = &mut was_silenced[(id - 1) as usize];
            if now && !*before {
                seen += 1;
            }
            *before = now;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

fn log_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rescuemesh-faults-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// One node's events of one kind, in the order it logged them. Read after the cluster shut down.
fn events_of(dir: &Path, id: NodeId, kind: &str) -> Vec<serde_json::Value> {
    std::fs::read_to_string(dir.join(format!("node-{id}.jsonl")))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .filter(|event| event["ev"] == kind)
        .collect()
}

/// Silences nodes 1 and 3 and plays their bids in round 1 by hand: ticket 0 and `ms` each. Node 2
/// draws its own and wins, since no ticket beats it. A test client sends these bids, because a
/// silenced node's own client would refuse. Its own send stays out until the peer timeout.
async fn node_2_outbids_silent_peers(cluster: &TestCluster, ms: u64) {
    let mine = faults::draw(SEED, 1, 2, &cluster.config.faults);
    assert!(mine.ticket > 0, "pick another SEED: node 2 must win");

    cluster.node(1).ctx.gate.silence();
    cluster.node(3).ctx.gate.silence();
    let node2 = cluster.node(2);
    let to_node2 = PeerClient::new(
        2,
        node2.peer_addr.to_string(),
        FaultGate::default(),
        EventLog::disabled("test"),
        Duration::from_millis(100),
    );
    let bid = FaultBid {
        round: 1,
        ticket: 0,
        ms,
    };
    for from in [1, 3] {
        loop {
            let envelope = PeerEnvelope {
                from,
                term: node2.ctx.consensus.status().term,
                body: PeerBody::FaultBid(bid),
            };
            let reply = to_node2
                .call(envelope, Duration::from_secs(2))
                .await
                .unwrap();
            match reply {
                PeerReply::Ack => break,
                // The term can move while the leader is among the silent nodes.
                PeerReply::StaleTerm { .. } => {}
                other => panic!("unexpected reply {other:?}"),
            }
        }
    }
}

fn incident() -> Incident {
    Incident {
        zone: 0,
        location: Point { x: 10, y: 20 },
        severity: 2,
        needs: Capability::Medical,
        reported_at_ms: rescuemesh::metrics::now_us() / 1000,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_schedule_injects_faults_one_victim_at_a_time() {
    let cluster = TestCluster::start_with(3, fast_faults).await;
    wait_for_faults(&cluster, 3).await;
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_fault_bid_from_an_old_term_is_dropped() {
    let cluster = TestCluster::start(3).await;
    wait_until(WAIT, "a leader in a term above 0", || {
        (1..=3).all(|id| {
            let s = cluster.node(id).ctx.consensus.status();
            s.leader.is_some() && s.term > 0
        })
    })
    .await;

    let stale = PeerEnvelope {
        from: 1,
        term: 0,
        body: PeerBody::FaultBid(FaultBid {
            round: 1,
            ticket: 999_999,
            ms: 1_000,
        }),
    };
    let peer = cluster.node(1).ctx.peers.get(2).unwrap().clone();
    let reply = peer.call(stale, Duration::from_secs(2)).await.unwrap();
    assert!(matches!(reply, PeerReply::StaleTerm { .. }), "{reply:?}");
    let round = cluster
        .node(2)
        .ctx
        .fault_election
        .with_rounds(|r| r.open_round());
    assert_eq!(round, None, "a dropped bid must not open a round");
    assert!(!cluster.node(2).ctx.gate.is_silenced());
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_scheduled_faults_when_fault_injection_is_disabled() {
    let cluster = TestCluster::start_with(3, |c| {
        fast_faults(c);
        c.faults.enabled = false;
    })
    .await;
    // Several intervals' worth of time with nobody ever silenced.
    let until = Instant::now() + Duration::from_millis(1_500);
    while Instant::now() < until {
        assert_eq!(silenced_count(&cluster), 0);
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    cluster.shutdown().await;
}

/// All three hold the same numbers, so they name one node. The first half of the claim is checked
/// on every round in the logs. The second half keeps the check honest: at least one round has to
/// be closed by all three, or there would be nothing for them to disagree on.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_two_nodes_name_different_victims() {
    let dir = log_dir("split");
    let metrics = dir.clone();
    let cluster = TestCluster::start_with(3, move |c| {
        fast_faults(c);
        c.metrics.dir = Some(metrics);
    })
    .await;
    wait_for_faults(&cluster, 3).await;
    cluster.shutdown().await;

    let mut rounds: BTreeMap<u64, Vec<Option<NodeId>>> = BTreeMap::new();
    for id in 1..=3 {
        for line in events_of(&dir, id, "fault_election") {
            let named = line["victim"].as_u64();
            rounds
                .entry(line["round"].as_u64().unwrap())
                .or_default()
                .push(named);
        }
    }
    for (round, named) in &rounds {
        let distinct: BTreeSet<_> = named.iter().flatten().collect();
        assert!(distinct.len() <= 1, "round {round} named {named:?}");
    }
    assert!(
        rounds
            .values()
            .any(|named| named.len() == 3 && named.iter().all(Option::is_some)),
        "no round was closed by all three: {rounds:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// The all-numbers rule itself: a dead machine must not shrink the round to two.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_election_finishes_with_a_node_down() {
    let dir = log_dir("down");
    let metrics = dir.clone();
    let mut cluster = TestCluster::start_with(3, move |c| {
        doomed_rounds(c);
        c.metrics.dir = Some(metrics);
    })
    .await;
    cluster.nodes.pop().unwrap().shutdown().await;

    let deadline = Instant::now() + WAIT;
    while rounds_left(&cluster, 1) < 3 || rounds_left(&cluster, 2) < 3 {
        assert!(Instant::now() < deadline, "the rounds stopped");
        assert_eq!(
            silenced_count(&cluster),
            0,
            "a round finished without node 3"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    cluster.shutdown().await;

    for id in [1, 2] {
        let lines = events_of(&dir, id, "fault_election");
        assert!(lines.len() >= 3, "{lines:?}");
        for line in lines {
            assert!(line["victim"].is_null(), "{line}");
            assert!(line["ms"].is_null(), "{line}");
            assert!(line["election_us"].is_null(), "{line}");
            assert!(line["tickets"].get("3").is_none(), "{line}");
        }
        assert!(events_of(&dir, id, "silenced").is_empty());
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn no_election_finishes_while_a_node_is_silenced() {
    let dir = log_dir("silent");
    let metrics = dir.clone();
    let cluster = TestCluster::start_with(3, move |c| {
        doomed_rounds(c);
        c.metrics.dir = Some(metrics);
    })
    .await;
    cluster.node(3).ctx.gate.silence();

    let deadline = Instant::now() + WAIT;
    while rounds_left(&cluster, 1) < 3 || rounds_left(&cluster, 2) < 3 {
        assert!(Instant::now() < deadline, "the rounds stopped");
        assert_eq!(silenced_count(&cluster), 1, "a second node went silent");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    cluster.shutdown().await;

    for id in [1, 2] {
        for line in events_of(&dir, id, "fault_election") {
            assert!(line["victim"].is_null(), "{line}");
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Watches a node for long enough that its timer has fired, and checks it called no round.
async fn calls_no_round(cluster: &TestCluster, id: NodeId) {
    let until = Instant::now() + Duration::from_millis(2_500);
    while Instant::now() < until {
        let rounds = cluster
            .node(id)
            .ctx
            .fault_election
            .with_rounds(|r| (r.left(), r.open_round()));
        assert_eq!(rounds, (0, None), "node {id} called a round");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// A returning node does not bid until it is current (AGENTS.md). It still hears the round, and logs
/// that it did nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_node_that_is_not_serving_does_not_bid() {
    let dir = log_dir("catching-up");
    let metrics = dir.clone();
    let cluster = TestCluster::start_with(3, move |c| {
        doomed_rounds(c);
        c.metrics.dir = Some(metrics);
    })
    .await;
    cluster.node(3).ctx.serving.store(false, Ordering::SeqCst);

    let deadline = Instant::now() + WAIT;
    while rounds_left(&cluster, 1) < 3 || rounds_left(&cluster, 2) < 3 {
        assert!(Instant::now() < deadline, "the rounds stopped");
        assert_eq!(
            silenced_count(&cluster),
            0,
            "a round finished without node 3"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    cluster.shutdown().await;

    let mine = events_of(&dir, 3, "fault_election");
    assert!(!mine.is_empty());
    for line in mine {
        assert!(line["victim"].is_null(), "{line}");
        assert!(line["tickets"].get("3").is_none(), "{line}");
        assert_eq!(line["called"], false, "{line}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// `serving` goes false a moment after the gate closes, so the gate is checked on its own too.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_silenced_node_calls_no_round_even_while_still_serving() {
    let cluster = TestCluster::start_with(3, doomed_rounds).await;
    let node3 = &cluster.node(3).ctx;
    node3.gate.silence();
    wait_until(WAIT, "the rejoin gate to notice the silence", || {
        !node3.serving.load(Ordering::SeqCst)
    })
    .await;
    node3.serving.store(true, Ordering::SeqCst);
    calls_no_round(&cluster, 3).await;
    cluster.shutdown().await;
}

/// A lone node answers on its own (D120), and an election cannot finish from there anyway.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_degraded_node_calls_no_round() {
    let cluster = TestCluster::start_with(3, |c| {
        doomed_rounds(c);
        c.timers.degraded_after_ms = 200;
    })
    .await;
    cluster.node(1).ctx.gate.silence();
    cluster.node(2).ctx.gate.silence();
    wait_until(WAIT, "node 3 to enter degraded mode", || {
        cluster.node(3).ctx.degraded.is_active()
    })
    .await;
    calls_no_round(&cluster, 3).await;
    cluster.shutdown().await;
}

/// A node can stop serving between its bid and the end of the round, and then must stay up: a fault
/// stacked on an unfinished catch-up is two failures at once. Here the test holds the round open:
/// nodes 1 and 3 are silent, so node 2's own send stays out until the peer timeout, and the test
/// plays their bids, 0 on both, so node 2's is the highest.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_winner_that_is_not_serving_refuses() {
    let dir = log_dir("refuses");
    let metrics = dir.clone();
    let cluster = TestCluster::start_with(3, move |c| {
        timers_never_fire(c);
        c.timers.peer_rpc_timeout_ms = 1_000;
        c.metrics.dir = Some(metrics);
    })
    .await;
    node_2_outbids_silent_peers(&cluster, 1_000).await;
    let node2 = cluster.node(2);

    wait_until(WAIT, "node 2 to bid, with all three numbers held", || {
        node2.ctx.fault_election.with_rounds(|r| r.bids_held()) == 3
    })
    .await;
    node2.ctx.serving.store(false, Ordering::SeqCst);

    wait_until(WAIT, "node 2 to close the round", || {
        node2.ctx.fault_election.with_rounds(|r| r.left()) == 1
    })
    .await;
    assert!(!node2.ctx.gate.is_silenced(), "it must stay up");
    cluster.shutdown().await;

    let lines = events_of(&dir, 2, "fault_election");
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["victim"], 2, "{lines:?}");
    let refused = events_of(&dir, 2, "fault_refused");
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert_eq!(refused[0]["victim"], 2, "{refused:?}");
    assert!(events_of(&dir, 2, "silenced").is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// A node that wins elects itself out with the silence from its own bid, not anyone else's.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_winner_goes_silent_for_its_own_time() {
    let dir = log_dir("own-time");
    let metrics = dir.clone();
    let cluster = TestCluster::start_with(3, move |c| {
        timers_never_fire(c);
        c.faults.silence_min_ms = 100;
        c.faults.silence_max_ms = 300;
        c.timers.peer_rpc_timeout_ms = 200;
        c.metrics.dir = Some(metrics);
    })
    .await;
    let mine = faults::draw(SEED, 1, 2, &cluster.config.faults);
    node_2_outbids_silent_peers(&cluster, 19_000).await;
    let node2 = cluster.node(2);
    wait_until(WAIT, "node 2 to go silent", || node2.ctx.gate.is_silenced()).await;
    cluster.shutdown().await;

    let silenced = events_of(&dir, 2, "silenced");
    assert_eq!(silenced.len(), 1, "{silenced:?}");
    assert_eq!(silenced[0]["ms"], mine.ms, "{silenced:?}");
    let _ = std::fs::remove_dir_all(dir);
}

/// Two nodes out past the interval stop every round. Once they are back, a later round must finish:
/// neither a timer that fired meanwhile nor the round numbers may stop the schedule for good.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_schedule_resumes_after_two_nodes_were_out() {
    let cluster = TestCluster::start_with(3, fast_faults).await;
    let out = Duration::from_millis(1_000);
    faults::silence_for(&cluster.node(1).ctx, out).unwrap();
    faults::silence_for(&cluster.node(2).ctx, out).unwrap();

    wait_until(WAIT, "both to be back and serving", || {
        cluster
            .nodes
            .iter()
            .all(|n| !n.ctx.gate.is_silenced() && n.ctx.serving.load(Ordering::SeqCst))
    })
    .await;
    wait_for_faults(&cluster, 1).await;
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn elected_faults_lose_no_request() {
    let cluster = TestCluster::start_with(3, |c| {
        fast_faults(c);
        c.policy.assignment = AssignmentPolicy::Bidding;
    })
    .await;
    wait_until(WAIT, "a leader on every node", || {
        cluster
            .nodes
            .iter()
            .all(|n| n.ctx.consensus.status().leader.is_some())
    })
    .await;

    let client = Arc::new(MeshClient::new(7, &cluster.config, EventLog::disabled("c")));
    let stop = Arc::new(AtomicBool::new(false));
    let mut users = JoinSet::new();
    for _ in 0..20 {
        let (client, stop) = (client.clone(), stop.clone());
        users.spawn(async move {
            let mut answered = 0u64;
            while !stop.load(Ordering::SeqCst) {
                client.submit(incident()).await.expect("a request was lost");
                answered += 1;
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            answered
        });
    }

    wait_for_faults(&cluster, 3).await;
    stop.store(true, Ordering::SeqCst);
    let mut answered = 0;
    while let Some(user) = users.join_next().await {
        answered += user.unwrap();
    }
    assert!(answered > 0, "no request ran during the faults");

    // Each finished round took three bids, two copies each.
    let bids = cluster
        .nodes
        .iter()
        .map(|n| match n.ctx.log.msgs().snapshot() {
            Event::Messages { sent, .. } => sent["fault_bid"],
            _ => 0,
        })
        .sum::<u64>();
    assert!(bids >= 18, "{bids} bid copies for three faults");
    cluster.shutdown().await;
}
