//! The planner's Phase 2 inputs on three real nodes (Phase 2, Part I, D491-D498).
//!
//! A volunteer joins through Part G's directory and a provider offers a resource through Part J's
//! claims; the campus has no responders, so only they can be sent. Each report is planned by a
//! real executor and committed through Raft, so the free list, the hold at apply and the shared
//! unit count are the real ones. Expiry is tested in `phase2.rs`'s unit tests, which set the log
//! clock directly: here it would mean waiting out a lease.
//!
//! Owner: I (with the shared `tests/`).
#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::{TestCluster, wait_until};
use rescuemesh::client::MeshClient;
use rescuemesh::metrics::{EventLog, now_ms};
use rescuemesh::phase2::{Source, is_phase2};
use rescuemesh::proto::{
    Capability, ClaimReply, Incident, Point, PresenceRole, ResourceId, ResourceOffer, UserId,
    ZoneId,
};

const WAIT: Duration = Duration::from_secs(10);
const ZONE: ZoneId = 0;

fn user(client: u64) -> UserId {
    UserId { client, user: 1 }
}

/// Severity 1 and fresh, so the planner never falls back to another capability (D95): a report
/// gets a responder with its own skill, or nobody.
fn incident(owner: u64, needs: Capability) -> Incident {
    Incident {
        owner: user(owner),
        category: needs.categories()[0],
        zone: ZONE,
        location: Point { x: 100, y: 100 },
        severity: 1,
        needs,
        reported_at_ms: now_ms(),
        ttl_ms: 60_000,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_volunteer_and_an_offer_are_sent_and_the_offer_runs_out() {
    let cluster = TestCluster::start_with(3, |c| {
        c.campus.responders = 0;
        // Nobody is released during the test, so a used unit stays used.
        c.campus.service_ms_per_severity = 60_000;
        c.p2p.port_min = 0;
        c.p2p.try_ms = 100;
    })
    .await;
    wait_until(WAIT, "a leader on every node", || {
        cluster
            .nodes
            .iter()
            .all(|n| n.ctx.consensus.status().leader.is_some())
    })
    .await;

    let volunteer = MeshClient::new(61, &cluster.config, EventLog::disabled("planner-test"));
    volunteer
        .join(
            user(61),
            PresenceRole::VolunteerResponder,
            ZONE,
            vec![Capability::Rescue, Capability::Medical],
        )
        .await
        .unwrap();
    let provider = MeshClient::new(62, &cluster.config, EventLog::disabled("planner-test"));
    let offer = ResourceOffer {
        id: ResourceId {
            provider: 62,
            seq: 1,
        },
        provider: user(62),
        capability: Capability::Fire,
        zone: ZONE,
        location: Point { x: 120, y: 80 },
        capacity: 1,
        available_until_ms: u64::MAX,
    };
    let reply = provider.offer_resource(offer.clone()).await.unwrap();
    assert!(matches!(reply, ClaimReply::Offered { .. }), "{reply:?}");
    wait_until(
        WAIT,
        "the volunteer's and the unit's rows on every node",
        || {
            cluster
                .nodes
                .iter()
                .all(|n| n.ctx.state.with(|s| s.phase2().len()) == 2)
        },
    )
    .await;

    let reporter = MeshClient::new(63, &cluster.config, EventLog::disabled("planner-test"));

    // The volunteer's second skill is matched.
    let rescue = reporter
        .submit(incident(63, Capability::Rescue))
        .await
        .unwrap();
    let sent = rescue.committed.expect("committed").plan.responder;
    let sent = sent.expect("the volunteer is sent");
    assert!(is_phase2(sent));
    let source = cluster
        .node(1)
        .ctx
        .state
        .with(|s| s.phase2().source(sent).cloned());
    assert!(
        matches!(source, Some(Source::Volunteer { user: u, .. }) if u == user(61)),
        "{source:?}"
    );

    // The offer's one unit.
    let fire = reporter
        .submit(incident(63, Capability::Fire))
        .await
        .unwrap();
    let unit = fire.committed.expect("committed").plan.responder;
    let unit = unit.expect("the offer's unit is sent");
    assert_eq!(
        cluster
            .node(1)
            .ctx
            .state
            .with(|s| s.phase2().offer_rows(&offer.id).to_vec()),
        vec![unit]
    );

    // Used up: a second fire report gets nobody, and so does a claim on the offer (D494).
    let again = reporter
        .submit(incident(63, Capability::Fire))
        .await
        .unwrap();
    assert_eq!(again.committed.expect("committed").plan.responder, None);
    let claim = provider
        .claim_resource(offer.id, user(64), 1)
        .await
        .unwrap();
    assert!(matches!(claim, ClaimReply::Refused(_)), "{claim:?}");

    // Every node holds both rows for the same requests.
    wait_until(WAIT, "both holds on every node", || {
        cluster.nodes.iter().all(|n| {
            n.ctx
                .state
                .with(|s| s.is_holding(sent, rescue.id) && s.is_holding(unit, fire.id))
        })
    })
    .await;
}
