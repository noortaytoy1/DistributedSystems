//! Nodes that lose their process and come back from the Raft log file (D118, FT-9).
#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::{TestCluster, first_reply, wait_until};
use rescuemesh::config::Config;
use rescuemesh::node::{self, NodeHandle};
use rescuemesh::proto::{Capability, ClientRequest, Incident, Point, ReplyBody, RequestId};
use tokio::net::TcpListener;
use tokio::time::Instant;

fn request(seq: u64) -> ClientRequest {
    ClientRequest {
        id: RequestId { client: 77, seq },
        attempt: 0,
        incident: Incident {
            zone: 0,
            location: Point { x: 1, y: 2 },
            severity: 3,
            needs: Capability::Medical,
            reported_at_ms: 0,
        },
    }
}

fn storage_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rescuemesh-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

async fn start_cluster(dir: &Path) -> TestCluster {
    let dir = dir.to_path_buf();
    let cluster = TestCluster::start_with(3, move |c| c.storage.dir = Some(dir)).await;
    leader_everywhere(&cluster.nodes).await;
    cluster
}

async fn leader_everywhere(nodes: &[NodeHandle]) {
    wait_until(Duration::from_secs(15), "a leader on every node", || {
        nodes
            .iter()
            .all(|n| n.ctx.consensus.status().leader.is_some())
    })
    .await;
}

/// Bind a port a stopped node just gave up. Until its aborted tasks finish it may still be taken,
/// and a parallel test's outgoing connection can briefly hold it too.
async fn rebind(addr: &str) -> TcpListener {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match TcpListener::bind(addr).await {
            Ok(listener) => return listener,
            Err(e) if Instant::now() > deadline => panic!("cannot bind {addr} again: {e}"),
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
}

async fn restart(config: &Config, id: u64) -> NodeHandle {
    let me = config.node(id).unwrap();
    let peer = rebind(&me.peer_addr).await;
    let client = rebind(&me.client_addr).await;
    node::start_with(config.clone(), id, peer, client)
        .await
        .unwrap()
}

async fn commit(config: &Config, seq: u64) -> u64 {
    let reply = first_reply(config, request(seq), Duration::from_secs(5))
        .await
        .expect("a reply");
    let ReplyBody::Done(done) = reply.body else {
        panic!("expected Done");
    };
    done.log_index
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_restarted_follower_keeps_its_log_and_catches_up_before_serving() {
    let dir = storage_dir("follower");
    let mut cluster = start_cluster(&dir).await;
    let config = cluster.config.clone();
    commit(&config, 1).await;

    let leader = cluster.nodes[0].ctx.consensus.status().leader.unwrap();
    let term = cluster.nodes[0].ctx.consensus.status().term;
    let victim = (1..=3).find(|id| *id != leader).unwrap();
    let at = cluster
        .nodes
        .iter()
        .position(|n| n.ctx.id == victim)
        .unwrap();
    let before = cluster.nodes.remove(at);
    wait_until(
        Duration::from_secs(5),
        "the victim to apply the commit",
        || before.ctx.state.with(|s| s.result_count()) == 1,
    )
    .await;
    before.shutdown().await;

    // Work the victim misses.
    commit(&config, 2).await;
    let back = restart(&config, victim).await;
    assert!(
        back.ctx.consensus.status().last_log_index.is_some(),
        "the log came back from the file"
    );
    wait_until(Duration::from_secs(10), "serving again", || {
        back.ctx.serving.load(Ordering::SeqCst)
    })
    .await;
    // Serving only once current, so the missed result is already here.
    assert_eq!(back.ctx.state.with(|s| s.result_count()), 2);
    // Its return caused no election.
    assert_eq!(back.ctx.consensus.status().term, term);
    wait_until(Duration::from_secs(5), "equal digests", || {
        let d = back.ctx.state.with(|s| s.digest());
        cluster
            .nodes
            .iter()
            .all(|n| n.ctx.state.with(|s| s.digest()) == d)
    })
    .await;
    back.shutdown().await;
    cluster.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// A node back from disk can read an empty state until it hears from a leader, because a power cut
/// can lose its saved commit index, which is not synced. It must not take that for an empty campus
/// and propose the list while it is not serving (D164). Back alone it never catches up, so a
/// proposal shows in its log as soon as it fails, and a report line is a second, many timeouts.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_restarted_node_proposes_no_campus_before_it_serves() {
    let dir = storage_dir("campus");
    let (storage, metrics) = (dir.join("raft"), dir.join("runs"));
    let log_file = metrics.join("node-3.jsonl");
    // No campus in the log, so the state still reads empty after the restart.
    let cluster = TestCluster::start_with(3, move |c| {
        c.campus.responders = 0;
        c.storage.dir = Some(storage);
        c.metrics.dir = Some(metrics);
    })
    .await;
    leader_everywhere(&cluster.nodes).await;
    let mut config = cluster.config.clone();
    cluster.shutdown().await;

    config.campus.responders = 30;
    config.timers.propose_timeout_ms = 100;
    let alone = restart(&config, 3).await;
    let log = || {
        alone.ctx.log.flush();
        std::fs::read_to_string(&log_file).unwrap()
    };
    let reports = |text: &str| text.matches(r#""ev":"messages""#).count();
    let before = reports(&log());
    wait_until(Duration::from_secs(15), "two more report lines", || {
        reports(&log()) >= before + 2
    })
    .await;
    assert!(!alone.ctx.serving.load(Ordering::SeqCst));
    assert!(!log().contains("register_responders"));

    alone.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

/// A node started before its peers cannot commit. Once it is degraded it stops proposing the campus,
/// so a long wait does not fill its log with failures, and it loads the list when a peer comes up
/// (D164). The first run here only finds three free addresses.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_lone_node_waits_for_a_peer_to_load_the_campus() {
    let dir = storage_dir("lone");
    let metrics = dir.clone();
    let log_file = dir.join("node-3.jsonl");
    let cluster = TestCluster::start_with(3, move |c| {
        c.campus.responders = 0;
        c.metrics.dir = Some(metrics);
        c.timers.degraded_after_ms = 300;
        c.timers.propose_timeout_ms = 100;
    })
    .await;
    let mut config = cluster.config.clone();
    cluster.shutdown().await;

    config.campus.responders = 30;
    let first = restart(&config, 3).await;
    wait_until(Duration::from_secs(15), "the lone node to go degraded", || {
        first.ctx.status().degraded
    })
    .await;
    let log = || {
        first.ctx.log.flush();
        std::fs::read_to_string(&log_file).unwrap()
    };
    let failures = |text: &str| {
        text.lines()
            .filter(|line| line.contains("propose_failed") && line.contains("register_responders"))
            .count()
    };
    let reports = |text: &str| text.matches(r#""ev":"messages""#).count();
    let (failed, before) = (failures(&log()), reports(&log()));
    wait_until(Duration::from_secs(15), "two more report lines", || {
        reports(&log()) >= before + 2
    })
    .await;
    // One may have been under way as the node went degraded.
    assert!(failures(&log()) <= failed + 1, "{}", log());

    let second = restart(&config, 2).await;
    wait_until(Duration::from_secs(15), "the campus on both nodes", || {
        first.ctx.status().responders == 30 && second.ctx.status().responders == 30
    })
    .await;
    first.shutdown().await;
    second.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_whole_cluster_restarts_from_disk_with_every_committed_result() {
    let dir = storage_dir("all");
    let cluster = start_cluster(&dir).await;
    let config = cluster.config.clone();
    commit(&config, 1).await;
    commit(&config, 2).await;
    wait_until(Duration::from_secs(5), "both on every node", || {
        cluster
            .nodes
            .iter()
            .all(|n| n.ctx.state.with(|s| s.result_count()) == 2)
    })
    .await;
    cluster.shutdown().await;

    let mut nodes = Vec::new();
    for id in 1..=3 {
        nodes.push(restart(&config, id).await);
    }
    leader_everywhere(&nodes).await;
    wait_until(
        Duration::from_secs(15),
        "all serving with both results",
        || {
            nodes.iter().all(|n| {
                n.ctx.serving.load(Ordering::SeqCst) && n.ctx.state.with(|s| s.result_count()) == 2
            })
        },
    )
    .await;
    // And it still takes new work.
    commit(&config, 3).await;
    for n in nodes {
        n.shutdown().await;
    }
    let _ = std::fs::remove_dir_all(&dir);
}
