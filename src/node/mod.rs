//! Wires a node together: listeners, transport, Raft, assignment, workers and the fault gate.
//!
//! A client request goes: received → already decided? reply from state → assignment picks the
//! executor → executor plans on the worker pool → proposes `CommitResult` through Raft →
//! replies with the committed (winning) result. A node never replies before the commit.
//!
//! Each part's entry point (`heartbeat::spawn`, `faults::spawn_schedule`, `rejoin::spawn`, ...)
//! is called from here, so owners fill in their own file without editing this one.
//!
//! Owner: A (Raft and replication).

pub mod faults;
pub mod heartbeat;
pub mod rejoin;
pub mod transport;
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
use crate::metrics::{Event, EventLog};
use crate::planner::{self, CostKnob};
use crate::proto::{
    ClientFrame, ClientReply, ClientRequest, Command, CommittedResult, ControlReply,
    ControlRequest, NodeId, NodeStatus, Outcome, PeerBody, PeerEnvelope, PeerReply, ProposeError,
    ReplyBody, ResultProposal, ServerFrame,
};
use crate::raft::store::StateReader;
use crate::raft::{Consensus, RaftNode};
use heartbeat::Suspicion;
use transport::{ClientHandler, FaultGate, PeerHandler, PeerSet, ReplyTx};
use workers::{Overloaded, WorkerPool};

/// How often a node logs its message counts and state digest.
const REPORT_INTERVAL: Duration = Duration::from_secs(1);

/// Plans per request before giving up on stale plans; the client's retry starts over.
const MAX_PLAN_ATTEMPTS: u32 = 3;

/// Everything a node's parts share.
pub struct NodeContext {
    pub id: NodeId,
    pub config: Arc<Config>,
    pub consensus: Arc<dyn Consensus>,
    /// Read access to the replicated state.
    pub state: StateReader,
    pub gate: FaultGate,
    pub peers: Arc<PeerSet>,
    pub workers: WorkerPool,
    pub log: EventLog,
    /// Peer liveness from heartbeats (heartbeat.rs).
    pub suspicion: Mutex<Suspicion>,
    /// False while a returning node catches up (rejoin.rs); a node that is not serving does not
    /// bid, plan or reply.
    pub serving: AtomicBool,
    /// Extra planner work per request, set by `ctl skew`.
    pub plan_work: AtomicU32,
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
        }
    }
}

/// A running node.
pub struct NodeHandle {
    pub ctx: Arc<NodeContext>,
    pub peer_addr: SocketAddr,
    pub client_addr: SocketAddr,
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
    let client = TcpListener::bind(&me.client_addr)
        .await
        .with_context(|| format!("binding client address {}", me.client_addr))?;
    start_with(config, id, peer, client).await
}

/// Start a node on listeners that are already bound (tests bind port 0 first).
pub async fn start_with(
    config: Config,
    id: NodeId,
    peer_listener: TcpListener,
    client_listener: TcpListener,
) -> anyhow::Result<NodeHandle> {
    let peer_addr = peer_listener.local_addr()?;
    let client_addr = client_listener.local_addr()?;
    let log = EventLog::open(config.metrics.dir.as_deref(), &format!("node-{id}"))?;
    let gate = FaultGate::default();
    let peers = Arc::new(PeerSet::new(id, &config.cluster, &gate, log.msgs()));
    if config.policy.assignment == AssignmentPolicy::Bidding {
        tracing::warn!("bidding is not implemented yet; falling back to leader dispatch");
    }

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
    let (raft, state) = match started {
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

    let ctx = Arc::new(NodeContext {
        id,
        consensus: raft,
        state,
        gate: gate.clone(),
        peers,
        workers: WorkerPool::new(config.workers.threads, config.workers.queue_capacity),
        log: log.clone(),
        suspicion: Mutex::new(Suspicion::new(
            config.timers.peer_heartbeat(),
            config.timers.takeover_missed_heartbeats,
        )),
        serving: AtomicBool::new(true),
        plan_work: AtomicU32::new(config.workers.plan_work),
        config: Arc::new(config),
    });
    let _ = raft_tx.send(ctx.clone());
    log.log(Event::Started {
        peer_addr: peer_addr.to_string(),
        client_addr: client_addr.to_string(),
    });

    let client_handler: ClientHandler = {
        let ctx = ctx.clone();
        Arc::new(move |frame, reply| on_client_frame(&ctx, frame, reply))
    };
    tasks.push(tokio::spawn(transport::serve_clients(
        client_listener,
        gate,
        log.msgs().clone(),
        client_handler,
    )));
    tasks.push(spawn_campus_seeding(ctx.clone()));
    tasks.push(spawn_reporter(ctx.clone()));
    tasks.extend(heartbeat::spawn(&ctx));
    tasks.extend(faults::spawn_schedule(&ctx));
    tasks.extend(rejoin::spawn(&ctx));

    Ok(NodeHandle {
        ctx,
        peer_addr,
        client_addr,
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
        PeerBody::Heartbeat => {
            lock(&ctx.suspicion).heard_from(from, Instant::now());
            PeerReply::Ack
        }
        // TODO(B): feed the bid into the request's bid round.
        PeerBody::Bid(_) | PeerBody::Raft(_) => PeerReply::Ack,
    }
}

// ---------------------------------------------------------------------------------------------
// Client requests
// ---------------------------------------------------------------------------------------------

fn on_client_frame(ctx: &Arc<NodeContext>, frame: ClientFrame, reply: ReplyTx) {
    match frame {
        ClientFrame::Request(req) => {
            tokio::spawn(handle_request(ctx.clone(), req, reply));
        }
        ClientFrame::Control(cmd) => {
            let answer = on_control(ctx, cmd);
            reply.send(&ServerFrame::Control(answer));
        }
    }
}

async fn handle_request(ctx: Arc<NodeContext>, req: ClientRequest, reply: ReplyTx) {
    let started = Instant::now();
    ctx.log.log(Event::RequestReceived {
        request: req.id,
        attempt: req.attempt,
    });
    if !ctx.serving.load(Ordering::SeqCst) {
        return;
    }
    // A retry of a decided request gets the committed result from whichever node has it.
    if let Some(done) = ctx.state.with(|s| s.result(&req.id).cloned()) {
        send_done(&ctx, &reply, done, started);
        return;
    }
    if decide_executor(&ctx, &req).await != Decision::Execute {
        return;
    }
    match execute(&ctx, &req).await {
        Ok(done) => send_done(&ctx, &reply, done, started),
        // No reply: the client retries with the same ID.
        Err(e) => tracing::debug!(request = %req.id, "not answered: {e}"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Decision {
    Execute,
    Standby,
}

/// Pick the node that plans this request.
async fn decide_executor(ctx: &NodeContext, _req: &ClientRequest) -> Decision {
    match ctx.config.policy.assignment {
        AssignmentPolicy::LeaderDispatch => leader_dispatch(ctx),
        // TODO(B): run a bid round (bidding.rs) and execute only if this node wins. Until then
        // requests are still answered through leader dispatch.
        AssignmentPolicy::Bidding => leader_dispatch(ctx),
    }
}

fn leader_dispatch(ctx: &NodeContext) -> Decision {
    if ctx.consensus.status().leader == Some(ctx.id) {
        Decision::Execute
    } else {
        Decision::Standby
    }
}

#[derive(Debug, thiserror::Error)]
enum ExecuteError {
    #[error(transparent)]
    Overloaded(#[from] Overloaded),
    #[error(transparent)]
    Propose(#[from] ProposeError),
    #[error("plan still stale after {MAX_PLAN_ATTEMPTS} attempts")]
    StillStale,
    #[error("unexpected outcome {0}")]
    Unexpected(&'static str),
}

/// Plan on the worker pool and commit through Raft; plan again if the plan went stale.
async fn execute(ctx: &NodeContext, req: &ClientRequest) -> Result<CommittedResult, ExecuteError> {
    for _ in 0..MAX_PLAN_ATTEMPTS {
        let free = ctx.state.with(|s| s.free_responders());
        let incident = req.incident.clone();
        let knob = CostKnob {
            extra_work: ctx.plan_work.load(Ordering::Relaxed),
        };
        let plan = ctx
            .workers
            .run(move || planner::plan(&incident, &free, knob))
            .await?;
        let proposal = ResultProposal {
            request: req.id,
            incident: req.incident.clone(),
            executor: ctx.id,
            term: ctx.consensus.status().term,
            plan,
        };
        match ctx
            .consensus
            .propose(Command::CommitResult(proposal))
            .await?
        {
            Outcome::Committed(done) | Outcome::Duplicate(done) => return Ok(done),
            Outcome::Stale { .. } => continue,
            other => return Err(ExecuteError::Unexpected(other.kind())),
        }
    }
    Err(ExecuteError::StillStale)
}

fn send_done(ctx: &NodeContext, reply: &ReplyTx, done: CommittedResult, started: Instant) {
    let (request, executor) = (done.request, done.executor);
    let sent = reply.send(&ServerFrame::Reply(ClientReply {
        id: request,
        from: ctx.id,
        body: ReplyBody::Done(done),
    }));
    if sent {
        ctx.log.log(Event::Replied {
            request,
            executor,
            latency_us: started.elapsed().as_micros() as u64,
        });
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
    }
}

// ---------------------------------------------------------------------------------------------
// Background tasks
// ---------------------------------------------------------------------------------------------

/// Once a leader exists, it registers the campus responders through Raft (once per cluster).
fn spawn_campus_seeding(ctx: Arc<NodeContext>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let responders = campus::responders(&ctx.config.campus);
        if responders.is_empty() {
            return;
        }
        let mut tick = tokio::time::interval(Duration::from_millis(200));
        loop {
            tick.tick().await;
            if ctx.state.with(|s| s.responder_count()) > 0 {
                return;
            }
            if ctx.consensus.status().leader == Some(ctx.id) {
                let cmd = Command::RegisterResponders {
                    responders: responders.clone(),
                };
                if let Err(e) = ctx.consensus.propose(cmd).await {
                    tracing::warn!("registering responders failed: {e}");
                }
            }
        }
    })
}

/// Every `REPORT_INTERVAL`, log message counts (E5) and the state digest.
fn spawn_reporter(ctx: Arc<NodeContext>) -> JoinHandle<()> {
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
        }
    })
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}
