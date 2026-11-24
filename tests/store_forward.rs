//! Part K: store-and-forward of peer updates between two real client programs (Phase 2, D407–D411).
//! An update that cannot get through is held by its sender and arrives once both ends are back; a
//! newer version replaces an older one while held. The cloud is not needed for P2P.
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::wait_until;
use rescuemesh::client::MeshClient;
use rescuemesh::config::Config;
use rescuemesh::metrics::EventLog;
use rescuemesh::offline::PeerSend;
use rescuemesh::proto::{
    Capability, Category, IncidentSummary, P2pBody, P2pEnvelope, Point, RequestId, UserId,
};
use tokio::sync::mpsc::Receiver;

const WAIT: Duration = Duration::from_secs(10);

/// A config with one node address nobody serves: enough for a program to work out its own address.
/// Retries every 300 ms, so held updates are tried again quickly.
fn alone() -> Config {
    let mut config: Config = toml::from_str(
        "[[cluster.nodes]]\nid = 1\npeer_addr = \"127.0.0.1:1\"\nclient_addr = \"127.0.0.1:2\"\n",
    )
    .unwrap();
    config.p2p.port_min = 0;
    config.p2p.try_ms = 100;
    config.timers.client_retry_ms = 300;
    config.timers.client_retry_max_ms = 600;
    config
}

fn summary(incident: u64) -> P2pBody {
    P2pBody::IncidentSummary(IncidentSummary {
        incident: RequestId {
            client: 51,
            seq: incident,
        },
        category: Category::MedicalAssistance,
        severity: 3,
        zone: 2,
        location: Point { x: 5, y: 9 },
        needs: Capability::Medical,
        reported_at_ms: 0,
    })
}

fn update(from: &MeshClient, to: u64, version: u64, incident: u64) -> P2pEnvelope {
    from.new_update(
        UserId {
            client: 51,
            user: 1,
        },
        Some(UserId {
            client: to,
            user: 1,
        }),
        2,
        version,
        4,
        summary(incident),
    )
}

/// The peer has the update a moment before its ack reaches the sender, which then lets it go.
async fn nothing_held(client: &MeshClient) {
    wait_until(WAIT, "the sender to let the delivered updates go", || {
        client.held_updates() == 0
    })
    .await;
}

async fn next(inbox: &mut Receiver<P2pEnvelope>) -> P2pEnvelope {
    tokio::time::timeout(WAIT, inbox.recv())
        .await
        .expect("an update within the wait")
        .expect("the inbox is open")
}

async fn held(client: &MeshClient, envelope: P2pEnvelope, to: &str) {
    match client.send_peer_or_hold(to, envelope).await.unwrap() {
        PeerSend::Held => {}
        PeerSend::Delivered(sent) => panic!("delivered while it should be held: {sent:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_update_made_offline_reaches_the_peer_on_reconnect() {
    let config = alone();
    let a = MeshClient::new(51, &config, EventLog::disabled("a"));
    let b = MeshClient::new(52, &config, EventLog::disabled("b"));
    let mut inbox = b.take_peer_updates().unwrap();
    let to = b.peer_addr().unwrap().to_string();

    a.go_offline();
    let envelope = update(&a, 52, 1, 1);
    let id = envelope.id;
    held(&a, envelope, &to).await;
    assert_eq!(a.held_updates(), 1);

    a.go_online().await.unwrap();
    assert_eq!(next(&mut inbox).await.id, id);
    nothing_held(&a).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_update_for_an_offline_peer_waits_and_arrives_when_it_returns() {
    let config = alone();
    let a = MeshClient::new(53, &config, EventLog::disabled("a"));
    let b = MeshClient::new(54, &config, EventLog::disabled("b"));
    let mut inbox = b.take_peer_updates().unwrap();
    let to = b.peer_addr().unwrap().to_string();

    b.go_offline();
    let envelope = update(&a, 54, 1, 1);
    let id = envelope.id;
    held(&a, envelope, &to).await;
    // A round while the peer is still away keeps it.
    assert_eq!(a.redeliver_held().await, 0);
    assert_eq!(a.held_updates(), 1);

    // No call from the test: the sender's timer finds the peer back.
    b.go_online().await.unwrap();
    assert_eq!(next(&mut inbox).await.id, id);
    nothing_held(&a).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_the_newest_version_of_a_held_update_arrives() {
    let config = alone();
    let a = MeshClient::new(55, &config, EventLog::disabled("a"));
    let b = MeshClient::new(56, &config, EventLog::disabled("b"));
    let mut inbox = b.take_peer_updates().unwrap();
    let to = b.peer_addr().unwrap().to_string();

    a.go_offline();
    held(&a, update(&a, 56, 1, 7), &to).await;
    let newer = update(&a, 56, 2, 7);
    let newer_id = newer.id;
    held(&a, newer, &to).await;
    let other = update(&a, 56, 1, 8);
    let other_id = other.id;
    held(&a, other, &to).await;
    assert_eq!(a.held_updates(), 2, "version 2 replaced version 1");

    a.go_online().await.unwrap();
    // Held updates go oldest first, so version 1 would have come first had it been kept.
    assert_eq!(next(&mut inbox).await.id, newer_id);
    assert_eq!(next(&mut inbox).await.id, other_id);
    nothing_held(&a).await;
}
