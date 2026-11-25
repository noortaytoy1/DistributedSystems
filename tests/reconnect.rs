//! Part K: a client coming back, end to end against three real nodes (Phase 2, D414–D417). Its
//! users are found again in Part G's directory after their leases ran out, and the claims it made
//! offline, kept in the claims outbox, survive a restart and reach the cloud.
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::{TestCluster, wait_until};
use rescuemesh::client::{ClaimDelivery, MeshClient};
use rescuemesh::config::Config;
use rescuemesh::metrics::EventLog;
use rescuemesh::proto::{Capability, Point, PresenceRole, ResourceId, ResourceOffer, UserId};

const WAIT: Duration = Duration::from_secs(20);
const ZONE: u16 = 3;

/// Short leases, so a lease runs out while a test's client is offline (D284, D291).
fn fast(config: &mut Config) {
    config.p2p.port_min = 0;
    config.timers.client_retry_ms = 300;
    config.timers.client_retry_max_ms = 600;
    config.presence.lease_ms = 600;
    config.presence.grace_ms = 300;
    config.presence.tick_ms = 100;
}

async fn ready(cluster: &TestCluster) {
    wait_until(WAIT, "a leader on every node", || {
        cluster
            .nodes
            .iter()
            .all(|n| n.ctx.consensus.status().leader.is_some())
    })
    .await;
}

/// Live records in the directory, the same on every node.
fn live_everywhere(cluster: &TestCluster, live: usize) -> bool {
    cluster
        .nodes
        .iter()
        .all(|n| n.ctx.state.with(|s| s.directory().live_count()) == live)
}

fn user(client: u64) -> UserId {
    UserId { client, user: 1 }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_returning_client_is_found_again_after_its_lease_ran_out() {
    let cluster = TestCluster::start_with(3, fast).await;
    ready(&cluster).await;
    let away = MeshClient::new(61, &cluster.config, EventLog::disabled("away"));
    let looking = MeshClient::new(62, &cluster.config, EventLog::disabled("looking"));
    away.join(
        user(61),
        PresenceRole::VolunteerResponder,
        ZONE,
        vec![Capability::Medical],
    )
    .await
    .unwrap();
    wait_until(WAIT, "the user live on every node", || {
        live_everywhere(&cluster, 1)
    })
    .await;

    // Offline, its renewals stop, and the directory lets the record go on every node.
    away.go_offline();
    wait_until(WAIT, "the lease to run out on every node", || {
        live_everywhere(&cluster, 0)
    })
    .await;
    assert!(
        looking
            .discover_zone(ZONE)
            .await
            .unwrap()
            .records
            .is_empty()
    );

    // Back online, the agent registers it again at once (D329), and it can be found (D416).
    away.go_online().await.unwrap();
    wait_until(WAIT, "the user live again on every node", || {
        live_everywhere(&cluster, 1)
    })
    .await;
    let found = looking.discover_zone(ZONE).await.unwrap();
    assert_eq!(found.records.len(), 1);
    assert_eq!(found.records[0].user, user(61));
    assert_eq!(found.records[0].peer_addr, away.peer_addr().unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_claim_that_gets_no_answer_stays_queued() {
    // One node address nobody serves, and short retries, so every claims call times out quickly.
    let mut config: Config = toml::from_str(
        "[[cluster.nodes]]\nid = 1\npeer_addr = \"127.0.0.1:1\"\nclient_addr = \"127.0.0.1:2\"\n",
    )
    .unwrap();
    fast(&mut config);
    config.timers.client_retry_ms = 50;
    config.timers.client_retry_max_ms = 100;
    config.timers.client_max_retries = 1;
    let client = MeshClient::new(65, &config, EventLog::disabled("claimant"));
    let resource = ResourceId {
        provider: 63,
        seq: 1,
    };

    client.go_offline();
    let queued = client.claim_resource_or_queue(resource, user(65), 1).await;
    assert!(matches!(queued, ClaimDelivery::Queued), "{queued:?}");
    client.go_online().await.unwrap();
    // No node answered, so the claim is kept for the next reconnect instead of lost (D414).
    assert_eq!(client.pending_claims().await, 1);
    let done = client.flush_pending_claims().await.unwrap();
    assert_eq!((done.sent, done.failed), (1, 1));
    assert_eq!(client.pending_claims().await, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn claims_made_offline_survive_a_restart_and_reach_the_cloud() {
    let dir = std::env::temp_dir().join(format!("rescuemesh-reconnect-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let _ = std::fs::remove_file(dir.join("claims-client-64.jsonl"));
    let cluster = TestCluster::start_with(3, |c| {
        fast(c);
        c.metrics.dir = Some(dir.clone());
    })
    .await;
    ready(&cluster).await;
    let provider = MeshClient::new(63, &cluster.config, EventLog::disabled("provider"));
    let offer = ResourceOffer {
        id: ResourceId {
            provider: 63,
            seq: 1,
        },
        provider: user(63),
        capability: Capability::Rescue,
        zone: ZONE,
        location: Point { x: 0, y: 0 },
        capacity: 2,
        available_until_ms: u64::MAX,
    };
    provider.offer_resource(offer.clone()).await.unwrap();

    let first = MeshClient::new(64, &cluster.config, EventLog::disabled("claimant"));
    first.go_offline();
    for _ in 0..2 {
        let queued = first.claim_resource_or_queue(offer.id, user(64), 1).await;
        assert!(matches!(queued, ClaimDelivery::Queued), "{queued:?}");
    }
    drop(first);
    assert!(dir.join("claims-client-64.jsonl").exists());

    // The program restarts with the same client ID: the claims are still there (D414).
    let second = MeshClient::new(64, &cluster.config, EventLog::disabled("claimant"));
    assert_eq!(second.pending_claims().await, 2);
    let done = second.flush_pending_claims().await.unwrap();
    assert_eq!(
        (done.sent, done.claimed, done.reconciling, done.failed),
        (2, 2, 0, 0)
    );
    assert_eq!(second.pending_claims().await, 0);
    let reread = MeshClient::new(64, &cluster.config, EventLog::disabled("claimant"));
    assert_eq!(reread.pending_claims().await, 0, "the file was rewritten");
}
