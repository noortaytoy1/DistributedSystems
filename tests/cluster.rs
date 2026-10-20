//! Three nodes in one process: client port and control port smoke tests.
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::{TestCluster, first_reply};
use rescuemesh::client::control;
use rescuemesh::proto::{
    Capability, ClientRequest, ControlReply, ControlRequest, Incident, Point, RequestId,
};

fn request(seq: u64) -> ClientRequest {
    ClientRequest {
        id: RequestId { client: 99, seq },
        attempt: 0,
        incident: Incident {
            zone: 0,
            location: Point { x: 10, y: 20 },
            severity: 3,
            needs: Capability::Medical,
            reported_at_ms: 0,
        },
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_request_to_all_nodes_gets_a_reply() {
    let cluster = TestCluster::start(3).await;
    let reply = first_reply(&cluster.config, request(1), Duration::from_secs(5))
        .await
        .expect("no node replied");
    assert_eq!(reply.id, request(1).id);
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_node_answers_status() {
    let cluster = TestCluster::start(3).await;
    for n in &cluster.config.cluster.nodes {
        let reply = control(
            &n.client_addr,
            ControlRequest::Status,
            Duration::from_secs(2),
        )
        .await
        .unwrap();
        let ControlReply::Status(status) = reply else {
            panic!("unexpected reply {reply:?}");
        };
        assert_eq!(status.id, n.id);
        assert!(!status.silenced);
    }
    cluster.shutdown().await;
}
