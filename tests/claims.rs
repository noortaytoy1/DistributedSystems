//! Resource offers and claims against a real cluster (Phase 2, Part J, D370-D380).
//!
//! Covers the demo-8 shape: two clients claim the same resource, one online (refused at once)
//! and one speculative (surfaced as a reconciliation entry instead), then the owner resolves it.
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::{TestCluster, wait_until};
use rescuemesh::client::MeshClient;
use rescuemesh::metrics::EventLog;
use rescuemesh::proto::{
    Capability, ClaimReply, Point, ReconciliationWinner, ResourceId, ResourceOffer, UserId,
};

async fn leader_elected(cluster: &TestCluster) {
    wait_until(Duration::from_secs(10), "a leader", || {
        (1..=3).all(|id| cluster.node(id).ctx.consensus.status().leader.is_some())
    })
    .await;
}

fn offer(provider: u64, seq: u64, capacity: u32, available_until_ms: u64) -> ResourceOffer {
    ResourceOffer {
        id: ResourceId { provider, seq },
        provider: UserId {
            client: provider,
            user: 0,
        },
        capability: Capability::Rescue,
        zone: 1,
        location: Point { x: 0, y: 0 },
        capacity,
        available_until_ms,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_offer_is_claimed_then_released() {
    let cluster = TestCluster::start(3).await;
    leader_elected(&cluster).await;
    let provider = MeshClient::new(101, &cluster.config, EventLog::disabled("c"));
    let claimant = MeshClient::new(102, &cluster.config, EventLog::disabled("c"));

    let o = offer(101, 1, 1, u64::MAX);
    let reply = provider.offer_resource(o.clone()).await.unwrap();
    assert!(matches!(reply, ClaimReply::Offered { .. }));

    let user = UserId {
        client: 102,
        user: 0,
    };
    let ClaimReply::Claimed { token, .. } = claimant.claim_resource(o.id, user, 1).await.unwrap()
    else {
        panic!("expected Claimed");
    };

    let reply = claimant.release_claim(o.id, user, token).await.unwrap();
    assert!(matches!(reply, ClaimReply::Released { .. }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_online_claim_over_capacity_is_refused() {
    let cluster = TestCluster::start(3).await;
    leader_elected(&cluster).await;
    let provider = MeshClient::new(111, &cluster.config, EventLog::disabled("c"));
    let first = MeshClient::new(112, &cluster.config, EventLog::disabled("c"));
    let second = MeshClient::new(113, &cluster.config, EventLog::disabled("c"));

    let o = offer(111, 1, 1, u64::MAX);
    provider.offer_resource(o.clone()).await.unwrap();

    let a = UserId {
        client: 112,
        user: 0,
    };
    let b = UserId {
        client: 113,
        user: 0,
    };
    let claimed = first.claim_resource(o.id, a, 1).await.unwrap();
    assert!(matches!(claimed, ClaimReply::Claimed { .. }));

    let refused = second.claim_resource(o.id, b, 1).await.unwrap();
    assert!(matches!(refused, ClaimReply::Refused(_)), "{refused:?}");
}

/// Demo scenario 8: an online claimant takes a one-capacity resource; a second claimant, whose
/// claim is marked speculative (as it would be after Aabed's offline switch and a delayed
/// reconnect), conflicts instead of being silently refused, and the resource's owner settles it
/// in the challenger's favour.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_speculative_conflict_is_surfaced_and_the_owner_resolves_it_for_the_challenger() {
    let cluster = TestCluster::start(3).await;
    leader_elected(&cluster).await;
    let provider = MeshClient::new(121, &cluster.config, EventLog::disabled("c"));
    let incumbent = MeshClient::new(122, &cluster.config, EventLog::disabled("c"));
    let challenger = MeshClient::new(123, &cluster.config, EventLog::disabled("c"));

    let o = offer(121, 1, 1, u64::MAX);
    provider.offer_resource(o.clone()).await.unwrap();

    let owner = o.provider;
    let incumbent_user = UserId {
        client: 122,
        user: 0,
    };
    let challenger_user = UserId {
        client: 123,
        user: 0,
    };

    let claimed = incumbent
        .claim_resource(o.id, incumbent_user, 1)
        .await
        .unwrap();
    assert!(matches!(claimed, ClaimReply::Claimed { .. }));

    // The challenger went offline before trying, so its claim is queued, then resent as
    // speculative on reconnect (D377).
    challenger.go_offline();
    let delivery = challenger
        .claim_resource_or_queue(o.id, challenger_user, 1)
        .await;
    assert!(matches!(
        delivery,
        rescuemesh::client::ClaimDelivery::Queued
    ));
    assert_eq!(challenger.pending_claims().await, 1);
    challenger.go_online().await.unwrap();
    assert_eq!(challenger.pending_claims().await, 0);

    // The conflict is now a visible reconciliation entry, not a silent refusal, and the
    // incumbent still holds the resource until the owner says otherwise.
    let token = wait_until_reconciled(&cluster, o.id).await;

    let resolved = provider
        .resolve_reconciliation(o.id, owner, token, ReconciliationWinner::Challenger)
        .await
        .unwrap();
    assert!(matches!(resolved, ClaimReply::Resolved { .. }));

    // The challenger holds it now; the incumbent's old claim no longer does.
    wait_until(
        Duration::from_secs(5),
        "the challenger to hold the resource",
        || {
            cluster.node(1).ctx.state.with(|s| {
                let c = s.claims();
                c.reconciling_on(&o.id) == 0 && c.claimed(&o.id) == 1
            })
        },
    )
    .await;
}

/// Waits for the reconciliation entry on `resource` to show up in the applied state, and returns
/// its token by replaying the committed log the test itself drove: simplest here is to read it
/// back out of a fresh speculative claim's own reply, since a second one on an open entry would
/// also answer `Reconciling` with the same token. Instead this polls node 1's state directly.
async fn wait_until_reconciled(cluster: &TestCluster, resource: ResourceId) -> u64 {
    wait_until(Duration::from_secs(5), "a reconciliation entry", || {
        cluster
            .node(1)
            .ctx
            .state
            .with(|s| s.claims().reconciling_on(&resource) > 0)
    })
    .await;
    cluster.node(1).ctx.state.with(|s| {
        s.claims()
            .reconciling_token(&resource)
            .expect("a reconciliation entry")
    })
}
