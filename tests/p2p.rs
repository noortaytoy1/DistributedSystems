//! Part H: the P2P link between client programs (Phase 2, D336–D339, D460–D465). Two programs talk directly
//! through their peer endpoints; duplicates, addressing and the offline gate behave as decided; and
//! with a real three-node cloud running, the cloud's client ports carry none of a P2P exchange.
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::{TestCluster, wait_until};
use rescuemesh::client::MeshClient;
use rescuemesh::config::Config;
use rescuemesh::metrics::EventLog;
use rescuemesh::p2p::PeerError;
use rescuemesh::proto::{
    Capability, Category, IncidentSummary, P2pAck, P2pBody, Point, RequestId, UserId,
};

const WAIT: Duration = Duration::from_secs(10);

fn summary(seq: u64) -> P2pBody {
    P2pBody::IncidentSummary(IncidentSummary {
        incident: RequestId { client: 41, seq },
        category: Category::MedicalAssistance,
        severity: 3,
        zone: 2,
        location: Point { x: 5, y: 9 },
        needs: Capability::Medical,
        reported_at_ms: 0,
    })
}

fn user(client: u64) -> UserId {
    UserId { client, user: 1 }
}

/// A config with one node address nobody serves: enough for a program to work out its own address,
/// and the cloud is not needed for P2P.
fn alone() -> Config {
    let mut config: Config = toml::from_str(
        "[[cluster.nodes]]\nid = 1\npeer_addr = \"127.0.0.1:1\"\nclient_addr = \"127.0.0.1:2\"\n",
    )
    .unwrap();
    config.p2p.port_min = 0;
    config.p2p.try_ms = 100;
    config
}

#[tokio::test]
async fn two_programs_exchange_an_update_directly_and_a_repeat_is_a_duplicate() {
    let config = alone();
    let a = MeshClient::new(41, &config, EventLog::disabled("a"));
    let b = MeshClient::new(42, &config, EventLog::disabled("b"));
    let mut inbox = b.take_peer_updates().unwrap();
    assert!(
        b.take_peer_updates().is_none(),
        "only the first call gets the stream"
    );
    let to = b.peer_addr().unwrap().to_string();

    let update = a.new_update(user(41), Some(user(42)), 2, 1, 4, summary(1));
    let first = a.send_peer(&to, update.clone()).await.unwrap();
    assert_eq!(first.ack, P2pAck::Accepted);
    assert_eq!(first.tries, 1);
    let got = tokio::time::timeout(WAIT, inbox.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got, update);

    // The same update again, as after a lost ack: acknowledged, but not handed on twice.
    let again = a.send_peer(&to, update).await.unwrap();
    assert_eq!(again.ack, P2pAck::Duplicate);
    assert!(inbox.try_recv().is_err());

    // A new update gets a new ID.
    let next = a.new_update(user(41), None, 2, 2, 4, summary(2));
    assert_eq!(a.send_peer(&to, next).await.unwrap().ack, P2pAck::Accepted);
}

#[tokio::test]
async fn an_update_for_another_programs_user_is_refused() {
    let config = alone();
    let a = MeshClient::new(43, &config, EventLog::disabled("a"));
    let b = MeshClient::new(44, &config, EventLog::disabled("b"));
    let to = b.peer_addr().unwrap().to_string();
    let wrong = a.new_update(user(43), Some(user(99)), 2, 1, 4, summary(3));
    assert!(matches!(
        a.send_peer(&to, wrong).await.unwrap().ack,
        P2pAck::Refused(_)
    ));
}

#[tokio::test]
async fn offline_sends_nothing_and_an_offline_peer_hears_nothing() {
    let config = alone();
    let a = MeshClient::new(45, &config, EventLog::disabled("a"));
    let b = MeshClient::new(46, &config, EventLog::disabled("b"));
    let to = b.peer_addr().unwrap().to_string();

    a.go_offline();
    let update = a.new_update(user(45), None, 2, 1, 4, summary(4));
    assert_eq!(
        a.send_peer(&to, update.clone()).await,
        Err(PeerError::Offline)
    );
    a.go_online().await.unwrap();

    b.go_offline();
    assert_eq!(
        a.send_peer(&to, update.clone()).await,
        Err(PeerError::GaveUp {
            tries: config.p2p.tries
        })
    );
    b.go_online().await.unwrap();
    assert_eq!(
        a.send_peer(&to, update).await.unwrap().ack,
        P2pAck::Accepted
    );
}

#[tokio::test]
async fn programs_take_the_first_free_port_of_the_range() {
    // A free port, to make a range of one.
    let port = std::net::UdpSocket::bind("0.0.0.0:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut config = alone();
    config.p2p.port_min = port;
    config.p2p.port_max = port;
    let first = MeshClient::new(47, &config, EventLog::disabled("a"));
    assert!(first.peer_addr().unwrap().ends_with(&format!(":{port}")));
    // The range is full: the second program has no endpoint, so its users cannot join.
    let second = MeshClient::new(48, &config, EventLog::disabled("b"));
    assert_eq!(second.peer_addr(), None);
}

/// Demo scenario 6: two programs exchange 100 updates while a real three-node cloud runs, and not
/// one byte reaches the cloud's client ports (D310, D465).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_cloud_carries_none_of_a_p2p_exchange() {
    let cluster = TestCluster::start_with(3, |c| c.p2p.port_min = 0).await;
    wait_until(WAIT, "a leader on every node", || {
        cluster
            .nodes
            .iter()
            .all(|n| n.ctx.consensus.status().leader.is_some())
    })
    .await;
    let a = MeshClient::new(49, &cluster.config, EventLog::disabled("a"));
    let b = MeshClient::new(50, &cluster.config, EventLog::disabled("b"));
    let mut inbox = b.take_peer_updates().unwrap();
    let to = b.peer_addr().unwrap().to_string();

    let cloud_bytes = || -> Vec<u64> {
        cluster
            .nodes
            .iter()
            .map(|n| n.udp_counters.snapshot().rx_bytes)
            .collect()
    };
    let before = cloud_bytes();
    for seq in 0..100 {
        let update = a.new_update(user(49), Some(user(50)), 2, seq, 4, summary(seq));
        assert_eq!(
            a.send_peer(&to, update).await.unwrap().ack,
            P2pAck::Accepted
        );
    }
    for _ in 0..100 {
        tokio::time::timeout(WAIT, inbox.recv())
            .await
            .unwrap()
            .unwrap();
    }
    assert_eq!(
        cloud_bytes(),
        before,
        "the cloud's client ports moved during P2P"
    );
    assert!(a.p2p_counts().tx_bytes > 0);
    assert!(b.p2p_counts().rx_bytes > 0);
}
