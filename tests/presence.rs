//! Part H: the presence agent against stub nodes (Phase 2, D326–D335). Part G's directory is not
//! built yet, so a stub stands in for each node: it counts presence calls and answers as a test
//! tells it to. What is checked is the client side: every node is asked, leases are renewed at a
//! third, a lost lease is registered again, offline pauses renewals, and discovery returns what a
//! node read.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::wait_until;
use rescuemesh::client::MeshClient;
use rescuemesh::config::{ClusterConfig, Config, NodeAddr};
use rescuemesh::metrics::EventLog;
use rescuemesh::node::datagram::{self, ServerEnd};
use rescuemesh::node::transport::run_calls;
use rescuemesh::presence::PresenceError;
use rescuemesh::proto::{
    Capability, ClientReply, ClientRequest, ClientRpc, ClientRpcRequest, ClientRpcResponse,
    ControlReply, DiscoverReply, IncidentQuery, PresenceQuery, PresenceRecord, PresenceReply,
    PresenceRole, UserId,
};
use tarpc::server::{BaseChannel, Channel};
use tokio::net::UdpSocket;
use tokio::time::Instant;

const WAIT: Duration = Duration::from_secs(10);
const LEASE_MS: u64 = 300;

/// What a stub node answers, and what it has been asked.
#[derive(Default)]
struct Directory {
    registers: AtomicUsize,
    renewals: AtomicUsize,
    deregisters: AtomicUsize,
    discovers: AtomicUsize,
    /// 0: answer; 1: refuse everything (no directory yet, D325); 2: answer `Unknown` to the next renewal.
    mode: AtomicU64,
    last: Mutex<Option<PresenceRecord>>,
    last_query: Mutex<Option<PresenceQuery>>,
}

#[derive(Clone)]
struct Stub(Arc<Directory>);

impl Stub {
    fn answer(&self) -> PresenceReply {
        match self.0.mode.load(Ordering::SeqCst) {
            1 => PresenceReply::Rejected("no directory".into()),
            _ => PresenceReply::Registered {
                lease_ms: LEASE_MS,
                log_index: 7,
            },
        }
    }
}

impl ClientRpc for Stub {
    async fn submit(self, _: tarpc::context::Context, _: ClientRequest) -> ClientReply {
        std::future::pending().await
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
        self.0.registers.fetch_add(1, Ordering::SeqCst);
        *self.0.last.lock().unwrap() = Some(record);
        self.answer()
    }
    async fn renew(self, _: tarpc::context::Context, _: UserId, _: u64) -> PresenceReply {
        self.0.renewals.fetch_add(1, Ordering::SeqCst);
        if self
            .0
            .mode
            .compare_exchange(2, 0, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            return PresenceReply::Unknown;
        }
        self.answer()
    }
    async fn deregister(self, _: tarpc::context::Context, _: UserId, _: u64) -> PresenceReply {
        self.0.deregisters.fetch_add(1, Ordering::SeqCst);
        self.answer()
    }
    async fn discover(self, _: tarpc::context::Context, query: PresenceQuery) -> DiscoverReply {
        self.0.discovers.fetch_add(1, Ordering::SeqCst);
        *self.0.last_query.lock().unwrap() = Some(query);
        match self.0.mode.load(Ordering::SeqCst) {
            1 => DiscoverReply::Rejected("no directory".into()),
            _ => DiscoverReply::Found {
                records: self.0.last.lock().unwrap().iter().cloned().collect(),
                log_index: 7,
            },
        }
    }
}

/// Start `n` stub nodes and a config that points at them, with fast retries.
async fn stubs(n: usize) -> (Vec<Arc<Directory>>, Config) {
    let mut dirs = Vec::new();
    let mut nodes = Vec::new();
    for i in 0..n {
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
        dirs.push(dir);
        nodes.push(NodeAddr {
            id: i as u64 + 1,
            peer_addr: format!("127.0.0.1:{}", 1 + i),
            client_addr: addr,
        });
    }
    let mut config = Config {
        cluster: ClusterConfig { nodes },
        timers: Default::default(),
        policy: Default::default(),
        workers: Default::default(),
        campus: Default::default(),
        metrics: Default::default(),
        faults: Default::default(),
        storage: Default::default(),
        consensus: Default::default(),
    };
    config.timers.client_retry_ms = 50;
    config.timers.client_retry_max_ms = 100;
    (dirs, config)
}

const USER: UserId = UserId {
    client: 31,
    user: 4,
};

async fn join(client: &MeshClient) -> Result<Duration, PresenceError> {
    client
        .join(
            USER,
            PresenceRole::VolunteerResponder,
            2,
            vec![Capability::Medical],
            "10.0.0.21:20000".into(),
        )
        .await
}

fn total(dirs: &[Arc<Directory>], count: impl Fn(&Directory) -> usize) -> usize {
    dirs.iter().map(|d| count(d)).sum()
}

#[tokio::test]
async fn joining_registers_on_every_node_and_keeps_renewing() {
    let (dirs, config) = stubs(3).await;
    let client = MeshClient::new(31, &config, EventLog::disabled("c"));
    let lease = join(&client).await.unwrap();
    assert_eq!(lease, Duration::from_millis(LEASE_MS));
    wait_until(WAIT, "the registration on every node", || {
        dirs.iter().all(|d| d.registers.load(Ordering::SeqCst) == 1)
    })
    .await;
    let record = dirs[0].last.lock().unwrap().clone().unwrap();
    assert_eq!(record.user, USER);
    assert_eq!(record.zone, 2);
    assert_eq!(record.peer_addr, "10.0.0.21:20000");
    // Renewals come at about a third of the 300 ms lease, so several within a second.
    let started = Instant::now();
    wait_until(WAIT, "three rounds of renewals", || {
        total(&dirs, |d| d.renewals.load(Ordering::SeqCst)) >= 3 * 3
    })
    .await;
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn every_node_refusing_ends_the_call_at_once() {
    let (dirs, config) = stubs(3).await;
    for d in &dirs {
        d.mode.store(1, Ordering::SeqCst);
    }
    let client = MeshClient::new(32, &config, EventLog::disabled("c"));
    let started = Instant::now();
    assert_eq!(
        join(&client).await,
        Err(PresenceError::Rejected("no directory".into()))
    );
    // Not the whole retry schedule: one answer from each node is enough.
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(matches!(
        client.discover_zone(2).await,
        Err(PresenceError::Rejected(_))
    ));
}

#[tokio::test]
async fn one_dead_node_does_not_stop_a_join() {
    let (dirs, mut config) = stubs(2).await;
    // A third node that never answers.
    let dead = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    config.cluster.nodes.push(NodeAddr {
        id: 3,
        peer_addr: "127.0.0.1:3".into(),
        client_addr: dead.local_addr().unwrap().to_string(),
    });
    let client = MeshClient::new(33, &config, EventLog::disabled("c"));
    assert!(join(&client).await.is_ok());
    assert!(total(&dirs, |d| d.registers.load(Ordering::SeqCst)) >= 1);
}

#[tokio::test]
async fn an_unknown_renewal_registers_the_user_again() {
    let (dirs, config) = stubs(1).await;
    let client = MeshClient::new(34, &config, EventLog::disabled("c"));
    join(&client).await.unwrap();
    dirs[0].mode.store(2, Ordering::SeqCst);
    wait_until(WAIT, "a second registration after Unknown", || {
        dirs[0].registers.load(Ordering::SeqCst) >= 2
    })
    .await;
    // And it keeps renewing after that.
    let renewals = dirs[0].renewals.load(Ordering::SeqCst);
    wait_until(WAIT, "renewals going on", || {
        dirs[0].renewals.load(Ordering::SeqCst) > renewals
    })
    .await;
}

#[tokio::test]
async fn offline_pauses_renewals_and_online_registers_again() {
    let (dirs, config) = stubs(1).await;
    let client = MeshClient::new(35, &config, EventLog::disabled("c"));
    join(&client).await.unwrap();
    wait_until(WAIT, "a renewal", || {
        dirs[0].renewals.load(Ordering::SeqCst) >= 1
    })
    .await;
    client.go_offline();
    // Let anything already in flight land, then nothing more may arrive while offline.
    tokio::time::sleep(Duration::from_millis(150)).await;
    let renewals = dirs[0].renewals.load(Ordering::SeqCst);
    let registers = dirs[0].registers.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(3 * LEASE_MS)).await;
    assert_eq!(dirs[0].renewals.load(Ordering::SeqCst), renewals);
    assert_eq!(dirs[0].registers.load(Ordering::SeqCst), registers);
    client.go_online().await.unwrap();
    wait_until(WAIT, "a registration on return", || {
        dirs[0].registers.load(Ordering::SeqCst) > registers
    })
    .await;
}

#[tokio::test]
async fn leaving_deregisters_and_stops_renewals() {
    let (dirs, config) = stubs(1).await;
    let client = MeshClient::new(36, &config, EventLog::disabled("c"));
    join(&client).await.unwrap();
    client.leave(USER).await.unwrap();
    assert_eq!(dirs[0].deregisters.load(Ordering::SeqCst), 1);
    tokio::time::sleep(Duration::from_millis(150)).await;
    let renewals = dirs[0].renewals.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(2 * LEASE_MS)).await;
    assert_eq!(dirs[0].renewals.load(Ordering::SeqCst), renewals);
    assert_eq!(
        client.leave(USER).await,
        Err(PresenceError::NotJoined(USER))
    );
}

#[tokio::test]
async fn a_zone_move_registers_with_the_new_zone() {
    let (dirs, config) = stubs(1).await;
    let client = MeshClient::new(37, &config, EventLog::disabled("c"));
    join(&client).await.unwrap();
    client.move_zone(USER, 5).await.unwrap();
    assert_eq!(dirs[0].last.lock().unwrap().as_ref().unwrap().zone, 5);
    assert_eq!(dirs[0].registers.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn discovery_returns_what_a_node_read_and_caps_the_limit() {
    let (dirs, config) = stubs(1).await;
    let client = MeshClient::new(38, &config, EventLog::disabled("c"));
    join(&client).await.unwrap();
    let found = client
        .discover_capability(Capability::Medical)
        .await
        .unwrap();
    assert_eq!(found.log_index, 7);
    assert_eq!(found.records.len(), 1);
    assert_eq!(found.records[0].user, USER);
    client
        .discover(PresenceQuery {
            zone: Some(2),
            capability: None,
            limit: 500,
        })
        .await
        .unwrap();
    let asked = dirs[0].last_query.lock().unwrap().clone().unwrap();
    assert_eq!(asked.zone, Some(2));
    assert_eq!(asked.limit, rescuemesh::proto::MAX_DISCOVER_RECORDS);
}
