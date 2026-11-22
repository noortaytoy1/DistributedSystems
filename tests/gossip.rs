//! Part I: zone gossip between real client programs (Phase 2, D351–D369). Each program has a real
//! peer endpoint. Most tests use one stub node for the cloud, which keeps the registered presence
//! records, answers `discover` from them and answers every report with a committed result, so what
//! is checked is the programs' side: a committed report reaches the whole zone, a stale directory
//! does not pull in other zones, a repeated answer is not spread twice, a dead peer does not stop
//! the rest, a subscriber that was offline gets its copies on return (Part K, D415), and with no
//! directory nothing is sent. One test runs the whole path on three real nodes with Part G's
//! directory (D369).
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{TestCluster, wait_until};
use rescuemesh::client::MeshClient;
use rescuemesh::config::{ClusterConfig, Config, NodeAddr};
use rescuemesh::metrics::{EventLog, now_ms};
use rescuemesh::node::datagram::{self, ServerEnd};
use rescuemesh::node::transport::run_calls;
use rescuemesh::proto::{
    Capability, Category, ClientReply, ClientRequest, ClientRpc, ClientRpcRequest,
    ClientRpcResponse, CommittedResult, ControlReply, DiscoverReply, Incident, IncidentQuery,
    P2pBody, P2pEnvelope, Plan, Point, PresenceQuery, PresenceRecord, PresenceReply, PresenceRole,
    ReplyBody, RequestId, UserId, ZoneId,
};
use tarpc::server::{BaseChannel, Channel};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

const WAIT: Duration = Duration::from_secs(10);
const ZONE: ZoneId = 2;
/// The report every `severity = 1` submit is answered as, so a second submit looks like a retry's
/// answer: the same incident, the same commit.
const REPEATED: RequestId = RequestId { client: 51, seq: 1 };

/// The stand-in node's directory and what it has been asked.
#[derive(Default)]
struct Directory {
    records: Mutex<BTreeMap<UserId, PresenceRecord>>,
    discovers: AtomicUsize,
    /// Answer `discover` with `Rejected`, as real nodes do until Part G exists (D325).
    no_directory: AtomicBool,
    /// Ignore the zone filter, as a stale directory might.
    stale: AtomicBool,
}

#[derive(Clone)]
struct Stub(Arc<Directory>);

impl ClientRpc for Stub {
    async fn submit(self, _: tarpc::context::Context, request: ClientRequest) -> ClientReply {
        let (id, log_index) = if request.incident.severity == 1 {
            (REPEATED, 9)
        } else {
            (request.id, 10 + request.id.seq % 1_000)
        };
        ClientReply {
            id: request.id,
            from: 1,
            body: ReplyBody::Done(CommittedResult {
                request: id,
                incident: request.incident.clone(),
                executor: 1,
                term: 1,
                plan: Plan {
                    responder: Some(7),
                    cost: 1,
                    priority: 300,
                },
                age_ms: request.age_ms,
                assigned_at_ms: now_ms(),
                log_index,
            }),
        }
    }
    async fn status(self, _: tarpc::context::Context) -> ControlReply {
        ControlReply::Done
    }
    async fn silence(self, _: tarpc::context::Context, _: u64) -> ControlReply {
        ControlReply::Done
    }
    async fn skew_load(self, _: tarpc::context::Context, _: u32) -> ControlReply {
        ControlReply::Done
    }
    async fn incidents(self, _: tarpc::context::Context, _: IncidentQuery) -> ControlReply {
        ControlReply::Done
    }
    async fn register(self, _: tarpc::context::Context, record: PresenceRecord) -> PresenceReply {
        self.0.records.lock().unwrap().insert(record.user, record);
        PresenceReply::Registered {
            lease_ms: 30_000,
            log_index: 1,
        }
    }
    async fn renew(self, _: tarpc::context::Context, _: UserId, _: u64) -> PresenceReply {
        PresenceReply::Registered {
            lease_ms: 30_000,
            log_index: 1,
        }
    }
    async fn deregister(self, _: tarpc::context::Context, user: UserId, _: u64) -> PresenceReply {
        self.0.records.lock().unwrap().remove(&user);
        PresenceReply::Unknown
    }
    async fn discover(self, _: tarpc::context::Context, query: PresenceQuery) -> DiscoverReply {
        self.0.discovers.fetch_add(1, Ordering::SeqCst);
        if self.0.no_directory.load(Ordering::SeqCst) {
            return DiscoverReply::Rejected("no directory".into());
        }
        let stale = self.0.stale.load(Ordering::SeqCst);
        let records = self
            .0
            .records
            .lock()
            .unwrap()
            .values()
            .filter(|r| stale || query.zone.is_none_or(|z| z == r.zone))
            .take(usize::from(query.limit))
            .cloned()
            .collect();
        DiscoverReply::Found {
            records,
            log_index: 1,
        }
    }

    // Claims (Part J) are not under test here; this stub never answers them.
    async fn offer_resource(
        self,
        _: tarpc::context::Context,
        _: rescuemesh::proto::ResourceOffer,
    ) -> rescuemesh::proto::ClaimReply {
        std::future::pending().await
    }
    async fn claim_resource(
        self,
        _: tarpc::context::Context,
        _: rescuemesh::proto::ResourceId,
        _: UserId,
        _: u32,
        _: bool,
        _: u64,
    ) -> rescuemesh::proto::ClaimReply {
        std::future::pending().await
    }
    async fn release_claim(
        self,
        _: tarpc::context::Context,
        _: rescuemesh::proto::ResourceId,
        _: UserId,
        _: u64,
    ) -> rescuemesh::proto::ClaimReply {
        std::future::pending().await
    }
    async fn resolve_reconciliation(
        self,
        _: tarpc::context::Context,
        _: rescuemesh::proto::ResourceId,
        _: UserId,
        _: u64,
        _: rescuemesh::proto::ReconciliationWinner,
    ) -> rescuemesh::proto::ClaimReply {
        std::future::pending().await
    }
}

/// One stub node and a config pointing at it, with fast retries and any free peer port.
async fn cloud() -> (Arc<Directory>, Config) {
    let dir = Arc::new(Directory::default());
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = socket.local_addr().unwrap().to_string();
    let stub = Stub(dir.clone());
    tokio::spawn(datagram::serve(
        Arc::new(socket),
        Arc::default(),
        datagram::CHANNEL_IDLE,
        |_: &tarpc::ClientMessage<ClientRpcRequest>| true,
        move |end: ServerEnd<ClientRpcRequest, ClientRpcResponse>| {
            run_calls(BaseChannel::with_defaults(end).execute(stub.clone().serve()))
        },
    ));
    let mut config = Config {
        cluster: ClusterConfig {
            nodes: vec![NodeAddr {
                id: 1,
                peer_addr: "127.0.0.1:1".into(),
                client_addr: addr,
            }],
        },
        timers: Default::default(),
        policy: Default::default(),
        workers: Default::default(),
        campus: Default::default(),
        metrics: Default::default(),
        faults: Default::default(),
        storage: Default::default(),
        workload: Default::default(),
        consensus: Default::default(),
        p2p: Default::default(),
        gossip: Default::default(),
        presence: Default::default(),
    };
    config.timers.client_retry_ms = 50;
    config.timers.client_retry_max_ms = 100;
    config.p2p.port_min = 0;
    config.p2p.try_ms = 100;
    (dir, config)
}

fn user(client: u64) -> UserId {
    UserId { client, user: 1 }
}

/// A program with one user joined in `zone`, and the stream of updates gossip hands it.
async fn program(
    client: u64,
    zone: ZoneId,
    config: &Config,
) -> (MeshClient, mpsc::Receiver<P2pEnvelope>) {
    let program = MeshClient::new(client, config, EventLog::disabled("gossip-test"));
    program
        .join(user(client), PresenceRole::Ordinary, zone, Vec::new())
        .await
        .unwrap();
    let updates = program.take_peer_updates().unwrap();
    (program, updates)
}

fn incident(owner: u64, severity: u8) -> Incident {
    Incident {
        owner: user(owner),
        category: Category::LabIncident,
        zone: ZONE,
        location: Point { x: 10, y: 20 },
        severity,
        needs: Capability::Fire,
        reported_at_ms: now_ms(),
        ttl_ms: 60_000,
    }
}

/// Wait until `updates` has handed over `n` updates, and return them.
async fn take(updates: &mut mpsc::Receiver<P2pEnvelope>, n: usize) -> Vec<P2pEnvelope> {
    let mut got = Vec::new();
    while got.len() < n {
        let next = tokio::time::timeout(WAIT, updates.recv())
            .await
            .expect("an update in time")
            .expect("the stream is open");
        got.push(next);
    }
    got
}

fn kinds(updates: &[P2pEnvelope]) -> Vec<&'static str> {
    let mut kinds: Vec<_> = updates
        .iter()
        .map(|u| match u.body {
            P2pBody::IncidentSummary(_) => "summary",
            P2pBody::AssignmentUpdate(_) => "assignment",
            P2pBody::ResourceOffer(_) => "offer",
        })
        .collect();
    kinds.sort_unstable();
    kinds
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_committed_report_reaches_every_program_of_a_five_program_zone() {
    let (_dir, config) = cloud().await;
    let (publisher, _own) = program(51, ZONE, &config).await;
    let mut others = Vec::new();
    for client in 52..=55 {
        others.push(program(client, ZONE, &config).await);
    }

    let answer = publisher.submit(incident(51, 3)).await.unwrap();
    let committed = answer.committed.expect("a committed answer");

    for (_, updates) in &mut others {
        let got = take(updates, 2).await;
        assert_eq!(kinds(&got), ["assignment", "summary"]);
        for u in &got {
            assert_eq!(u.id.origin, 51, "the reporter publishes (D351)");
            assert_eq!(u.from, user(51));
            assert_eq!(u.zone, ZONE);
            assert_eq!(u.max_hops, 7);
            match &u.body {
                P2pBody::IncidentSummary(s) => {
                    assert_eq!(s.incident, committed.request);
                    assert_eq!(u.version, 1);
                }
                P2pBody::AssignmentUpdate(a) => {
                    assert_eq!(a.responder, Some(7));
                    assert_eq!(u.version, committed.log_index, "D355");
                }
                P2pBody::ResourceOffer(_) => panic!("no offers here"),
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stale_directory_does_not_pull_another_zone_into_the_flood() {
    let (dir, config) = cloud().await;
    dir.stale.store(true, Ordering::SeqCst);
    let (publisher, _own) = program(51, ZONE, &config).await;
    let mut inside = Vec::new();
    for client in 52..=54 {
        inside.push(program(client, ZONE, &config).await);
    }
    // Listed for zone 2 by the stale directory, but its only user is in zone 3.
    let (_outside, mut outside_updates) = program(56, 3, &config).await;

    publisher.submit(incident(51, 3)).await.unwrap();
    for (_, updates) in &mut inside {
        assert_eq!(kinds(&take(updates, 2).await), ["assignment", "summary"]);
    }
    // Every zone program has both updates, so nothing is still on its way to program 56.
    assert!(
        outside_updates.try_recv().is_err(),
        "a program not subscribed to the zone hands nothing over (D348)"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_repeated_answer_is_not_spread_again() {
    let (_dir, config) = cloud().await;
    let (publisher, _own) = program(51, ZONE, &config).await;
    let mut others = Vec::new();
    for client in 52..=55 {
        others.push(program(client, ZONE, &config).await);
    }

    // Two submits answered with the same committed result, as a retry's answer is.
    publisher.submit(incident(51, 1)).await.unwrap();
    publisher.submit(incident(51, 1)).await.unwrap();
    // Then a different report. The agent publishes in order, so once this one is everywhere a
    // second copy of the first would have been sent before it.
    publisher.submit(incident(51, 4)).await.unwrap();

    for (_, updates) in &mut others {
        let got = take(updates, 4).await;
        let mut subjects: Vec<_> = got
            .iter()
            .map(|u| match &u.body {
                P2pBody::IncidentSummary(s) => ("summary", s.incident),
                P2pBody::AssignmentUpdate(a) => ("assignment", a.incident),
                P2pBody::ResourceOffer(_) => panic!("no offers here"),
            })
            .collect();
        subjects.sort();
        subjects.dedup();
        assert_eq!(subjects.len(), 4, "four different updates: {got:?}");
        assert!(
            updates.try_recv().is_err(),
            "the repeated answer was not published again (D342, D355)"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_dead_peer_does_not_stop_the_rest_of_the_zone() {
    let (dir, config) = cloud().await;
    // A program nobody serves: a socket bound and never read, listed in the zone.
    let dead = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let dead_user = UserId {
        client: 99,
        user: 1,
    };
    dir.records.lock().unwrap().insert(
        dead_user,
        PresenceRecord {
            user: dead_user,
            role: PresenceRole::Ordinary,
            zone: ZONE,
            peer_addr: dead.local_addr().unwrap().to_string(),
            capabilities: Vec::new(),
            incarnation: 1,
        },
    );
    // Five programs in all, the dead one included: with k = 3 a zone of at most k + 2 is always
    // covered, because each forwarder then sends to every program but itself and the origin. A
    // larger zone may miss a program now and then, which is D345's expected miss, not this test's.
    let (publisher, _own) = program(51, ZONE, &config).await;
    let mut others = Vec::new();
    for client in 52..=54 {
        others.push(program(client, ZONE, &config).await);
    }

    publisher.submit(incident(51, 3)).await.unwrap();
    for (_, updates) in &mut others {
        assert_eq!(kinds(&take(updates, 2).await), ["assignment", "summary"]);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_subscriber_that_was_offline_gets_the_zone_updates_on_return() {
    let (_dir, config) = cloud().await;
    let (publisher, _own) = program(51, ZONE, &config).await;
    let (away, mut updates) = program(52, ZONE, &config).await;

    away.go_offline();
    publisher.submit(incident(51, 3)).await.unwrap();
    // Neither copy gets through, so the publisher holds both for the subscriber (D415).
    wait_until(WAIT, "the publisher to hold both copies", || {
        publisher.held_updates() == 2
    })
    .await;
    assert!(
        updates.try_recv().is_err(),
        "nothing reaches an offline program"
    );

    // Back online, the publisher's next redelivery round sends them (D410).
    away.go_online().await.unwrap();
    assert_eq!(
        kinds(&take(&mut updates, 2).await),
        ["assignment", "summary"]
    );
    wait_until(WAIT, "the publisher to let them go", || {
        publisher.held_updates() == 0
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_no_directory_a_report_is_still_answered_and_nothing_is_sent() {
    let (dir, config) = cloud().await;
    dir.no_directory.store(true, Ordering::SeqCst);
    let publisher = MeshClient::new(51, &config, EventLog::disabled("gossip-test"));

    let answer = publisher.submit(incident(51, 3)).await.unwrap();
    assert!(answer.committed.is_some());
    wait_until(WAIT, "the agent to ask the directory", || {
        dir.discovers.load(Ordering::SeqCst) > 0
    })
    .await;
    // The publish follows the refused discover at once, with an empty zone: no peer datagram.
    // Waiting a little is the only way to see that nothing happens.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(publisher.p2p_counts().tx_datagrams, 0);
}

/// The whole path on three real nodes (D369): users join through Part G's directory, a real report
/// is planned and committed, and the reporter's zone gets its summary and assignment peer to peer.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_real_cloud_report_reaches_its_zone_through_the_real_directory() {
    let cluster = TestCluster::start_with(3, |c| {
        c.p2p.port_min = 0;
        c.p2p.try_ms = 100;
        c.timers.client_retry_ms = 300;
        c.timers.client_retry_max_ms = 600;
    })
    .await;
    wait_until(WAIT, "a leader on every node", || {
        cluster
            .nodes
            .iter()
            .all(|n| n.ctx.consensus.status().leader.is_some())
    })
    .await;
    let (publisher, _own) = program(51, ZONE, &cluster.config).await;
    let mut others = Vec::new();
    for client in 52..=55 {
        others.push(program(client, ZONE, &cluster.config).await);
    }
    // Every node's directory holds all five, so whichever node answers the discover lists them all.
    wait_until(WAIT, "the five users live on every node", || {
        cluster
            .nodes
            .iter()
            .all(|n| n.ctx.state.with(|s| s.directory().live_count()) == 5)
    })
    .await;

    let answer = publisher.submit(incident(51, 3)).await.unwrap();
    let committed = answer.committed.expect("a committed answer");

    for (_, updates) in &mut others {
        let got = take(updates, 2).await;
        assert_eq!(kinds(&got), ["assignment", "summary"]);
        for u in &got {
            assert_eq!(u.id.origin, 51, "the reporter publishes (D351)");
            match &u.body {
                P2pBody::IncidentSummary(s) => assert_eq!(s.incident, committed.request),
                P2pBody::AssignmentUpdate(a) => {
                    assert_eq!(a.incident, committed.request);
                    assert_eq!(
                        a.responder, committed.plan.responder,
                        "the real plan's responder"
                    );
                    assert_eq!(
                        u.version, committed.log_index,
                        "the real commit's index (D355)"
                    );
                }
                P2pBody::ResourceOffer(_) => panic!("no offers here"),
            }
        }
    }
    cluster.shutdown().await;
}
