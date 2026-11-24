//! Part G: the presence directory on real nodes, with Part H's real presence agent (D289–D292).
//!
//! Each of the spec's hard cases is tested in `discovery.rs`'s unit tests on one copy of the
//! directory. Here they run through the log on three nodes, and every test ends by checking that
//! the three copies hold the same directory: the claim demo scenario 5 makes.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use common::{TestCluster, wait_until};
use rescuemesh::config::Config;
use rescuemesh::discovery::RecordState;
use rescuemesh::metrics::EventLog;
use rescuemesh::middleware::MeshClient;
use rescuemesh::proto::{
    Capability, Command, MAX_DISCOVER_RECORDS, Outcome, PresenceLease, PresenceOutcome,
    PresenceQuery, PresenceRecord, PresenceRole, UserId, ZoneId,
};

const WAIT: Duration = Duration::from_secs(20);

fn fast(config: &mut Config) {
    config.p2p.port_min = 0;
    config.timers.client_retry_ms = 300;
    config.timers.client_retry_max_ms = 600;
    config.timers.client_max_retries = 20;
}

/// The three nodes' directories: what each holds, and its live count.
fn directories(cluster: &TestCluster) -> Vec<(u64, usize)> {
    cluster
        .nodes
        .iter()
        .map(|n| {
            n.ctx
                .state
                .with(|s| (s.directory().content_digest(), s.directory().live_count()))
        })
        .collect()
}

/// Every node holds the same directory, with `live` records found by `discover`.
async fn converged(cluster: &TestCluster, live: usize) {
    wait_until(WAIT, "the same directory on every node", || {
        let d = directories(cluster);
        d.iter().all(|x| *x == d[0]) && d[0].1 == live
    })
    .await;
}

async fn leader_everywhere(cluster: &TestCluster) {
    wait_until(WAIT, "a leader on every node", || {
        cluster
            .nodes
            .iter()
            .all(|n| n.ctx.consensus.status().leader.is_some())
    })
    .await;
}

fn user(client: u64, n: u32) -> UserId {
    UserId { client, user: n }
}

/// Demo scenario 5: users of two programs register through the real agent, and discovery by zone
/// and by capability gives the same answer on every node.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn registered_users_are_found_by_zone_and_capability_on_every_node() {
    let cluster = TestCluster::start_with(3, fast).await;
    leader_everywhere(&cluster).await;
    let a = MeshClient::new(61, &cluster.config, EventLog::disabled("a"));
    let b = MeshClient::new(62, &cluster.config, EventLog::disabled("b"));
    for n in 0..6 {
        let (role, caps) = match n % 3 {
            0 => (PresenceRole::Ordinary, vec![]),
            1 => (PresenceRole::VolunteerResponder, vec![Capability::Medical]),
            _ => (PresenceRole::ResourceProvider, vec![Capability::Fire]),
        };
        let zone = (n % 2) as ZoneId;
        a.join(user(61, n), role, zone, caps.clone()).await.unwrap();
        b.join(user(62, n), role, zone, caps).await.unwrap();
    }
    converged(&cluster, 12).await;

    let zone_1 = a.discover_zone(1).await.unwrap();
    assert_eq!(zone_1.records.len(), 6);
    assert!(zone_1.records.iter().all(|r| r.zone == 1));
    // One record per program first (D288, D357).
    assert_ne!(zone_1.records[0].user.client, zone_1.records[1].user.client);
    let medics = b.discover_capability(Capability::Medical).await.unwrap();
    assert_eq!(medics.records.len(), 4);
    assert!(
        medics
            .records
            .iter()
            .all(|r| r.capabilities == vec![Capability::Medical])
    );
    // The peer address advertised is the program's own endpoint (D336).
    let mine = zone_1.records.iter().find(|r| r.user.client == 61).unwrap();
    assert_eq!(mine.peer_addr, a.peer_addr().unwrap());

    // Every node gives the same answer at the same index.
    let query = PresenceQuery {
        zone: Some(0),
        capability: None,
        limit: MAX_DISCOVER_RECORDS,
    };
    let answers: Vec<_> = cluster
        .nodes
        .iter()
        .map(|n| n.ctx.state.with(|s| s.directory().discover(&query)))
        .collect();
    assert!(answers.iter().all(|x| *x == answers[0]), "{answers:?}");
    assert_eq!(answers[0].len(), 6);
    cluster.shutdown().await;
}

/// The spec's "a user moving between zones", through the agent's `move_zone`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_user_that_moves_zone_is_found_in_the_new_zone_only() {
    let cluster = TestCluster::start_with(3, fast).await;
    leader_everywhere(&cluster).await;
    let a = MeshClient::new(63, &cluster.config, EventLog::disabled("a"));
    let u = user(63, 0);
    a.join(u, PresenceRole::Ordinary, 2, vec![]).await.unwrap();
    a.move_zone(u, 5).await.unwrap();
    converged(&cluster, 1).await;
    assert!(a.discover_zone(2).await.unwrap().records.is_empty());
    assert_eq!(a.discover_zone(5).await.unwrap().records[0].user, u);
    for node in &cluster.nodes {
        assert_eq!(
            node.ctx.state.with(|s| s.directory().counters().moved),
            1,
            "node {}",
            node.ctx.id
        );
    }
    cluster.shutdown().await;
}

/// The spec's "expired registrations" and "delayed heartbeats": nobody renews, and every node drops
/// the record at the same log position, after the lease and its grace. Ticks alone move the clock.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_record_nobody_renews_expires_the_same_way_on_every_node() {
    let cluster = TestCluster::start_with(3, |c| {
        c.presence.lease_ms = 600;
        c.presence.grace_ms = 400;
        c.presence.tick_ms = 100;
    })
    .await;
    leader_everywhere(&cluster).await;
    let u = user(64, 0);
    let record = PresenceRecord {
        user: u,
        role: PresenceRole::VolunteerResponder,
        zone: 1,
        peer_addr: "127.0.0.1:1".into(),
        capabilities: vec![Capability::Rescue],
        incarnation: 1,
    };
    let lease = PresenceLease {
        at_ms: rescuemesh::metrics::now_ms(),
        lease_ms: 600,
        grace_ms: 400,
    };
    let outcome = cluster
        .node(1)
        .ctx
        .consensus
        .propose(Command::PresenceRegister { record, lease })
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        Outcome::Presence(PresenceOutcome::Registered { .. })
    ));
    let state = |id: u64| {
        cluster
            .node(id)
            .ctx
            .state
            .with(|s| s.directory().state_of(&u))
    };
    wait_until(WAIT, "the record late on every node", || {
        (1..=3).all(|id| state(id) != RecordState::Live)
    })
    .await;
    wait_until(WAIT, "the record gone on every node", || {
        (1..=3).all(|id| state(id) == RecordState::Gone)
    })
    .await;
    converged(&cluster, 0).await;
    for node in &cluster.nodes {
        assert_eq!(node.ctx.state.with(|s| s.directory().counters().expired), 1);
    }
    cluster.shutdown().await;
}

/// The spec's "duplicate registrations": a restarted program (higher incarnation) replaces the old
/// record, and the old program's late registration is refused, on every node.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_restarted_program_wins_over_its_old_registration() {
    let cluster = TestCluster::start_with(3, fast).await;
    leader_everywhere(&cluster).await;
    let u = user(65, 0);
    let register = |incarnation: u64, peer: &str| Command::PresenceRegister {
        record: PresenceRecord {
            user: u,
            role: PresenceRole::Ordinary,
            zone: 0,
            peer_addr: peer.into(),
            capabilities: vec![],
            incarnation,
        },
        lease: PresenceLease {
            at_ms: rescuemesh::metrics::now_ms(),
            lease_ms: 60_000,
            grace_ms: 0,
        },
    };
    let ctx = &cluster.node(2).ctx;
    ctx.consensus
        .propose(register(200, "127.0.0.1:2"))
        .await
        .unwrap();
    let refused = ctx
        .consensus
        .propose(register(100, "127.0.0.1:1"))
        .await
        .unwrap();
    assert!(matches!(
        refused,
        Outcome::Presence(PresenceOutcome::Superseded { held: 200, .. })
    ));
    converged(&cluster, 1).await;
    for node in &cluster.nodes {
        let held = node
            .ctx
            .state
            .with(|s| s.directory().entry(&u).unwrap().record.peer_addr.clone());
        assert_eq!(held, "127.0.0.1:2");
    }
    cluster.shutdown().await;
}

/// The spec's "server failure and state reconstruction": a node silenced while users register comes
/// back, catches up from the log and holds the same directory, with no step of its own. Users whose
/// proposer was the silenced node still get in, through another node once the view drops it (D287).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_node_back_from_silence_rebuilds_the_directory_from_the_log() {
    let cluster = TestCluster::start_with(3, fast).await;
    leader_everywhere(&cluster).await;
    let a = MeshClient::new(66, &cluster.config, EventLog::disabled("a"));
    a.join(user(66, 100), PresenceRole::Ordinary, 0, vec![])
        .await
        .unwrap();

    let out = cluster.node(3).ctx.clone();
    out.gate.silence();
    wait_until(WAIT, "node 3 stops serving", || {
        !out.serving.load(Ordering::SeqCst)
    })
    .await;
    // Twelve users: about a third of them hash to node 3 while it is still in the others' views.
    for n in 0..12 {
        a.join(
            user(66, n),
            PresenceRole::VolunteerResponder,
            3,
            vec![Capability::Technical],
        )
        .await
        .unwrap();
    }
    assert_eq!(
        out.state.with(|s| s.directory().len()),
        1,
        "it learnt nothing while out"
    );

    out.gate.restore();
    wait_until(WAIT, "node 3 serving again", || {
        out.serving.load(Ordering::SeqCst)
    })
    .await;
    converged(&cluster, 13).await;
    let found = out.state.with(|s| {
        s.directory().discover(&PresenceQuery {
            zone: Some(3),
            capability: Some(Capability::Technical),
            limit: MAX_DISCOVER_RECORDS,
        })
    });
    assert_eq!(found.len(), 12);
    cluster.shutdown().await;
}

/// A program that leaves takes its user out of every node's directory.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_user_that_leaves_is_gone_from_every_node() {
    let cluster = TestCluster::start_with(3, fast).await;
    leader_everywhere(&cluster).await;
    let a = MeshClient::new(67, &cluster.config, EventLog::disabled("a"));
    let u = user(67, 0);
    a.join(u, PresenceRole::Ordinary, 0, vec![]).await.unwrap();
    converged(&cluster, 1).await;
    a.leave(u).await.unwrap();
    converged(&cluster, 0).await;
    for node in &cluster.nodes {
        assert_eq!(
            node.ctx.state.with(|s| s.directory().state_of(&u)),
            RecordState::Gone
        );
    }
    cluster.shutdown().await;
}

/// The lab runs rotating rounds (D249): the directory works the same with no leader, and expires
/// through the ticks of the lowest member of the view.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_directory_works_the_same_under_rotating_rounds() {
    let cluster = TestCluster::start_with(3, |c| {
        fast(c);
        c.consensus = rescuemesh::config::ConsensusMode::Rounds;
        c.faults.enabled = false;
        c.presence.lease_ms = 800;
        c.presence.grace_ms = 200;
        c.presence.tick_ms = 100;
    })
    .await;
    let a = MeshClient::new(68, &cluster.config, EventLog::disabled("a"));
    for n in 0..5 {
        a.join(user(68, n), PresenceRole::Ordinary, 4, vec![])
            .await
            .unwrap();
    }
    converged(&cluster, 5).await;
    assert_eq!(a.discover_zone(4).await.unwrap().records.len(), 5);
    // The agent renews at a third of the lease, so the users outlive several leases of log time.
    let clock =
        |cluster: &TestCluster| cluster.node(1).ctx.state.with(|s| s.directory().clock_ms());
    let start = clock(&cluster);
    wait_until(WAIT, "three leases of log time", || {
        clock(&cluster) >= start + 2_400
    })
    .await;
    converged(&cluster, 5).await;
    // Dropping the program stops its renewals: every node expires all five.
    drop(a);
    converged(&cluster, 0).await;
    cluster.shutdown().await;
}
