//! Part C: peer heartbeats and failure suspicion on three real nodes (D41, D76).
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::{TestCluster, wait_until};
use rescuemesh::proto::NodeId;
use tokio::time::Instant;

const WAIT: Duration = Duration::from_secs(10);

fn suspects(cluster: &TestCluster, observer: NodeId, peer: NodeId) -> bool {
    cluster
        .node(observer)
        .ctx
        .suspicion
        .lock()
        .unwrap()
        .is_suspected(peer)
}

fn anyone_suspected(cluster: &TestCluster) -> bool {
    (1..=3).any(|id| {
        !cluster
            .node(id)
            .ctx
            .suspicion
            .lock()
            .unwrap()
            .suspected()
            .is_empty()
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_healthy_cluster_suspects_nobody() {
    let cluster = TestCluster::start(3).await;
    let until = Instant::now() + Duration::from_secs(1);
    while Instant::now() < until {
        assert!(!anyone_suspected(&cluster), "false suspicion");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_silenced_node_is_suspected_by_both_others_and_trusted_after() {
    let cluster = TestCluster::start(3).await;
    assert!(!anyone_suspected(&cluster));

    cluster.node(2).ctx.gate.silence();
    let silenced_at = Instant::now();
    wait_until(WAIT, "nodes 1 and 3 suspect node 2", || {
        suspects(&cluster, 1, 2) && suspects(&cluster, 3, 2)
    })
    .await;
    // Three missed 50 ms heartbeats, plus up to one interval until the next check.
    assert!(silenced_at.elapsed() >= Duration::from_millis(150));
    // The silenced node is the one cut off: for twice the threshold it suspects nobody, while 1
    // and 3 keep trusting each other.
    let until = Instant::now() + Duration::from_millis(300);
    while Instant::now() < until {
        assert!(!suspects(&cluster, 2, 1) && !suspects(&cluster, 2, 3));
        assert!(!suspects(&cluster, 1, 3) && !suspects(&cluster, 3, 1));
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    cluster.node(2).ctx.gate.restore();
    wait_until(WAIT, "node 2 trusted again", || {
        !suspects(&cluster, 1, 2) && !suspects(&cluster, 3, 2)
    })
    .await;
    // And node 2, fresh clocks after restore, suspects nobody.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!anyone_suspected(&cluster));
    cluster.shutdown().await;
}
