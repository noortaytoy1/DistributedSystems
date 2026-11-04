//! Wires a node together: listeners, transport, Raft, assignment, workers and the fault gate.
//!
//! A client request comes in here and goes up to the application layer, `service::handle_request`
//! (D156). This file keeps what sits below it: who plans a request (`decide_executor`), peer
//! messages, control frames and the background tasks.
//!
//! Each part's entry point (`heartbeat::spawn`, `faults::spawn_schedule`, `rejoin::spawn`, ...)
//! is called from here, so owners fill in their own file without editing this one.
//!
//! Owner: A (Raft and replication).

pub mod bids;
pub mod cpu;
pub mod datagram;
pub mod degraded;
pub mod faults;
pub mod group;
pub mod heartbeat;
pub mod rejoin;
pub mod release;
pub mod service;
pub mod step_aside;
pub mod takeover;
#[cfg(test)]
pub(crate) mod testing;
pub mod transport;
pub mod usage;
pub mod workers;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use futures::FutureExt;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::campus;
use crate::config::{AssignmentPolicy, Config};
use crate::metrics::{Event, EventLog, UdpCounters, now_ms};
use crate::proto::{
    ClientRequest, Command, ControlReply, ControlRequest, NodeId, NodeStatus, PeerBody,
    PeerEnvelope, PeerReply,
};
use crate::raft::store::StateReader;
use crate::raft::{Consensus, RaftNode};
use bids::BidState;
use degraded::DegradedState;
use faults::FaultElection;
use group::CloudGroup;
use heartbeat::Suspicion;
use transport::{
    ClientPort, ClientServer, ControlHandler, FaultGate, PeerHandler, PeerSet, RequestHandler,
};
use workers::WorkerPool;

/// How often a node logs its message counts and state digest.
const REPORT_INTERVAL: Duration = Duration::from_secs(1);

/// How often a node checks that the campus is registered. Small against starting three machines
/// by hand, and nothing needs to tune it (D164).
const CAMPUS_CHECK_INTERVAL: Duration = Duration::from_millis(200);

/// Everything a node's parts share. Application code holds this too, and `tests/layers.rs` keeps it
/// away from the fields it has no business with, such as `consensus` and `peers`.
pub struct NodeContext {
    pub id: NodeId,
    pub config: Arc<Config>,
    pub consensus: Arc<dyn Consensus>,
    /// Read access to the replicated state.
    pub state: StateReader,
    pub gate: FaultGate,
    pub peers: Arc<PeerSet>,
    /// The cloud nodes as one group: this node's member list, and sends through it (group.rs).
    pub group: Arc<CloudGroup>,
    pub workers: WorkerPool,
    pub log: EventLog,
    /// Peer liveness from heartbeats (heartbeat.rs).
    pub suspicion: Mutex<Suspicion>,
    /// False while a returning node catches up (rejoin.rs); a node that is not serving does not
    /// bid, plan or reply.
    pub serving: AtomicBool,
    /// Bid rounds and the load figures bids are made from (bids.rs).
    pub bids: BidState,
    /// This node's rounds of the fault election (faults.rs).
    pub fault_election: FaultElection,
    /// Extra planner work per request, set by `ctl skew`.
    pub plan_work: AtomicU32,
    /// Whether this node is the last one left, and what it answered meanwhile (degraded.rs).
    pub degraded: DegradedState,
}

impl NodeContext {
    pub fn status(&self) -> NodeStatus {
        let s = self.consensus.status();
        let (digest, results, responders) = self.state.with(|st| {
            (
                st.digest(),
                st.result_count() as u64,
                st.responder_count() as u64,
            )
        });
        NodeStatus {
            id: self.id,
            term: s.term,
            leader: s.leader,
            role: s.role.as_str().into(),
            last_log_index: s.last_log_index,
            last_applied: s.last_applied,
            digest,
            results,
            responders,
            silenced: self.gate.is_silenced(),
            serving: self.serving.load(Ordering::SeqCst),
            degraded: self.degraded.is_active(),
            members: self.group.view().members.into_iter().collect(),
        }
    }
}

/// A running node.
pub struct NodeHandle {
    pub ctx: Arc<NodeContext>,
    pub peer_addr: SocketAddr,
    pub client_addr: SocketAddr,
    /// What this node's UDP socket has done (D172).
    pub udp_counters: Arc<UdpCounters>,
    tasks: Vec<JoinHandle<()>>,
}

impl NodeHandle {
    pub async fn shutdown(self) {
        for task in &self.tasks {
            task.abort();
        }
        self.ctx.consensus.shutdown().await;
        self.ctx.log.log(Event::Stopped);
        let log = self.ctx.log.clone();
        let _ = tokio::task::spawn_blocking(move || log.close()).await;
    }
}

/// Bind this node's addresses from the config and start it.
pub async fn start(config: Config, id: NodeId) -> anyhow::Result<NodeHandle> {
    let me = config
        .node(id)
        .with_context(|| format!("node {id} is not in the config"))?
        .clone();
    let peer = TcpListener::bind(&me.peer_addr)
        .await
        .with_context(|| format!("binding peer address {}", me.peer_addr))?;
    let client = ClientPort::bind(&me.client_addr)
        .await
        .with_context(|| format!("binding client address {}", me.client_addr))?;
    start_with(config, id, peer, client).await
}

/// Start a node on sockets that are already bound (tests bind port 0 first).
pub async fn start_with(
    config: Config,
    id: NodeId,
    peer_listener: TcpListener,
    client_port: ClientPort,
) -> anyhow::Result<NodeHandle> {
    let peer_addr = peer_listener.local_addr()?;
    let client_addr = client_port.local_addr()?;
    let log = EventLog::open(config.metrics.dir.as_deref(), &format!("node-{id}"))?;
    client_port.report_buffer(&log);
    let udp_counters = Arc::new(UdpCounters::default());
    let gate = FaultGate::default();
    // Same cooldown as openraft's retry of an unreachable peer (D32, D90).
    let reconnect_backoff = Duration::from_millis(2 * config.timers.raft_heartbeat_ms);
    let peers = Arc::new(PeerSet::new(
        id,
        &config.cluster,
        &gate,
        &log,
        reconnect_backoff,
    ));

    // The peer port must be serving before Raft starts, or the first elections time out.
    let (raft_tx, raft_rx) = tokio::sync::oneshot::channel::<Arc<NodeContext>>();
    let early_ctx = raft_rx.shared();
    let peer_handler: PeerHandler = Arc::new(move |env| {
        let ctx = early_ctx.clone();
        async move {
            match ctx.await {
                Ok(ctx) => on_peer_message(ctx, env).await,
                // Startup failed; the node is going away.
                Err(_) => PeerReply::Ack,
            }
        }
        .boxed()
    });
    let mut tasks = vec![tokio::spawn(transport::serve_peers(
        peer_listener,
        gate.clone(),
        log.msgs().clone(),
        peer_handler,
    ))];

    let started = RaftNode::start(id, &config, peers.clone(), log.clone()).await;
    let (raft, state, restarted) = match started {
        Ok(ok) => ok,
        Err(e) => {
            for t in &tasks {
                t.abort();
            }
            return Err(e);
        }
    };
    tasks.push(raft.spawn_watcher());
    tasks.push(raft.spawn_heartbeat_summary());
    tasks.push(raft.spawn_io_report());

    let group = Arc::new(CloudGroup::new(
        id,
        &config.cluster,
        peers.clone(),
        raft.clone(),
        log.clone(),
    ));
    let ctx = Arc::new(NodeContext {
        id,
        consensus: raft,
        state,
        gate: gate.clone(),
        peers,
        group,
        workers: WorkerPool::new(config.workers.threads, config.workers.queue_capacity),
        log: log.clone(),
        suspicion: Mutex::new(Suspicion::new(
            config.timers.peer_heartbeat(),
            config.timers.takeover_missed_heartbeats,
        )),
        // A node with a Raft log from an earlier run may be behind (D118).
        serving: AtomicBool::new(!restarted),
        bids: BidState::new(id, &config),
        fault_election: FaultElection::new(id, &config),
        plan_work: AtomicU32::new(config.workers.plan_work),
        degraded: DegradedState::default(),
        config: Arc::new(config),
    });
    let _ = raft_tx.send(ctx.clone());
    log.log(Event::Started {
        peer_addr: peer_addr.to_string(),
        client_addr: client_addr.to_string(),
    });

    let on_request: RequestHandler = {
        let ctx = ctx.clone();
        // In its own task, so a client's cancel cannot cut off a bid or a commit (D169). It owns
        // itself: every wait in it is capped, so it ends without help.
        Arc::new(move |req, reply| {
            tokio::spawn(service::handle_request(ctx.clone(), req, reply));
        })
    };
    let on_control: ControlHandler = {
        let ctx = ctx.clone();
        Arc::new(move |cmd| on_control(&ctx, cmd))
    };
    tasks.push(tokio::spawn(transport::serve_clients(
        client_port,
        ClientServer::new(gate, log.msgs().clone(), on_request, on_control),
        udp_counters.clone(),
    )));
    tasks.push(spawn_campus_seeding(ctx.clone()));
    tasks.push(spawn_reporter(ctx.clone(), udp_counters.clone()));
    tasks.extend(heartbeat::spawn(&ctx));
    tasks.extend(faults::spawn_schedule(&ctx));
    tasks.extend(rejoin::spawn(&ctx, restarted));
    tasks.extend(bids::spawn(&ctx));
    tasks.push(release::spawn(&ctx));
    tasks.extend(degraded::spawn(&ctx));
    tasks.extend(step_aside::spawn(&ctx));

    Ok(NodeHandle {
        ctx,
        peer_addr,
        client_addr,
        udp_counters,
        tasks,
    })
}

// ---------------------------------------------------------------------------------------------
// Peer messages
// ---------------------------------------------------------------------------------------------

async fn on_peer_message(ctx: Arc<NodeContext>, env: PeerEnvelope) -> PeerReply {
    let PeerEnvelope { from, term, body } = env;
    // openraft judges Raft terms itself; it must see stale RPCs to reject them.
    if let PeerBody::Raft(rpc) = body {
        return ctx.consensus.handle_rpc(from, rpc).await;
    }
    let current = ctx.consensus.status().term;
    if term < current {
        return PeerReply::StaleTerm { current };
    }
    match body {
        PeerBody::Propose(cmd) => PeerReply::Proposed(ctx.consensus.propose_local(cmd).await),
        PeerBody::ProposeMany(commands) => {
            let answers = commands
                .into_iter()
                .map(|cmd| ctx.consensus.propose_local(cmd));
            PeerReply::ProposedMany(futures::future::join_all(answers).await)
        }
        PeerBody::ReadIndex => PeerReply::ReadIndex(ctx.consensus.read_index_local().await),
        PeerBody::FaultBid(bid) => {
            faults::on_bid(&ctx, from, bid);
            PeerReply::Ack
        }
        PeerBody::Heartbeat => {
            lock(&ctx.suspicion).heard_from(from, Instant::now());
            PeerReply::Ack
        }
        PeerBody::Bid(bid) => {
            bids::on_bid(&ctx, from, &bid);
            PeerReply::Ack
        }
        PeerBody::Bids(batch) => {
            bids::on_bids(&ctx, from, &batch);
            PeerReply::Ack
        }
        PeerBody::Raft(_) => PeerReply::Ack,
    }
}

// ---------------------------------------------------------------------------------------------
// Who plans a request
// ---------------------------------------------------------------------------------------------

/// What this node does with a request, as the middleware decides it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    Execute,
    Standby,
}

/// Pick the node that plans this request.
pub(crate) async fn decide_executor(ctx: &NodeContext, req: &ClientRequest) -> Decision {
    match ctx.config.policy.assignment {
        AssignmentPolicy::LeaderDispatch => leader_dispatch(ctx),
        AssignmentPolicy::Bidding => {
            // A node that lost the round stands by to take over from a silent winner (D110).
            if bids::run_round(ctx, req).await || takeover::stand_by(ctx, req).await {
                Decision::Execute
            } else {
                Decision::Standby
            }
        }
    }
}

fn leader_dispatch(ctx: &NodeContext) -> Decision {
    if ctx.consensus.status().leader == Some(ctx.id) {
        Decision::Execute
    } else {
        Decision::Standby
    }
}

// ---------------------------------------------------------------------------------------------
// Control
// ---------------------------------------------------------------------------------------------

fn on_control(ctx: &Arc<NodeContext>, cmd: ControlRequest) -> ControlReply {
    match cmd {
        ControlRequest::Status => ControlReply::Status(ctx.status()),
        ControlRequest::Silence { ms } => match faults::silence_for(ctx, Duration::from_millis(ms))
        {
            Ok(()) => ControlReply::Done,
            Err(e) => ControlReply::Error(e),
        },
        ControlRequest::SkewLoad { extra_work } => {
            ctx.plan_work.store(extra_work, Ordering::SeqCst);
            ControlReply::Done
        }
        ControlRequest::Incidents(query) => {
            let ms_per_severity = ctx.config.campus.service_ms_per_severity;
            let now = now_ms();
            let rows = ctx
                .state
                .with(|s| s.incident_rows(&query, now, ms_per_severity));
            ControlReply::Incidents {
                node: ctx.status(),
                rows,
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Background tasks
// ---------------------------------------------------------------------------------------------

/// Any serving node registers the campus responders through the log, until the log holds them
/// (D164). No node has to be picked for this: the command is idempotent, so the few extra entries
/// from nodes proposing together change nothing, and the log decides the list, not one node's file.
/// The check is the one `release.rs` uses. A node back from its log file starts not serving, so it
/// never proposes while its state still reads empty, and a lone node could not commit anyway.
fn spawn_campus_seeding(ctx: Arc<NodeContext>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let responders = campus::responders(&ctx.config.campus);
        if responders.is_empty() {
            return;
        }
        let mut tick = tokio::time::interval(CAMPUS_CHECK_INTERVAL);
        loop {
            tick.tick().await;
            if ctx.state.with(|s| s.responder_count()) > 0 {
                return;
            }
            if ctx.serving.load(Ordering::SeqCst) && !ctx.degraded.is_active() {
                let cmd = Command::RegisterResponders {
                    responders: responders.clone(),
                };
                // Not a warning: a node started before its peers fails here until they come up,
                // which is normal in the lab. `propose_failed` still counts it.
                if let Err(e) = ctx.group.send_ordered(cmd).await {
                    tracing::debug!("registering responders failed: {e}");
                }
            }
        }
    })
}

/// Every `REPORT_INTERVAL`, log message counts (E5), the state digest, resource use (D128) and what
/// the UDP socket did (D172).
fn spawn_reporter(ctx: Arc<NodeContext>, udp_counters: Arc<UdpCounters>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(REPORT_INTERVAL);
        loop {
            tick.tick().await;
            ctx.log.log(ctx.log.msgs().snapshot());
            let (last_applied, digest) = ctx.state.with(|s| (s.last_index(), s.digest()));
            ctx.log.log(Event::StateDigest {
                last_applied,
                digest,
            });
            let Ok(usage) = tokio::task::spawn_blocking(usage::read).await else {
                continue;
            };
            let pool = ctx.workers.load();
            ctx.log.log(Event::ResourceUsage {
                rss_kb: usage.rss_kb,
                cpu_ticks: usage.cpu_ticks,
                net_rx_bytes: usage.net_rx_bytes,
                net_tx_bytes: usage.net_tx_bytes,
                udp_drops: usage.udp_drops,
                queued: pool.queued,
                running: pool.running,
            });
            ctx.log.log(Event::UdpCounters {
                counts: udp_counters.snapshot(),
                udp_drops: usage.udp_drops,
            });
        }
    })
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}
