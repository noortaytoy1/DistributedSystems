//! Nodes that lose their process and come back from the Raft log file (D118, FT-9).
#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::{TestCluster, first_reply, wait_until};
use rescuemesh::config::Config;
use rescuemesh::node::{self, NodeHandle};
use rescuemesh::proto::{
    Capability, Category, ClientRequest, Incident, Point, ReplyBody, RequestId, UserId,
};
use tokio::net::TcpListener;
use tokio::time::Instant;

fn request(seq: u64) -> ClientRequest {
    ClientRequest {
        id: RequestId { client: 77, seq },
        attempt: 0,
        age_ms: 0,
        incident: Incident {
            owner: UserId {
                client: 77,
                user: 0,
            },
            category: Category::MedicalAssistance,
            zone: 0,
            location: Point { x: 1, y: 2 },
            severity: 3,
            needs: Capability::Medical,
            reported_at_ms: 0,
            ttl_ms: 60_000,
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
