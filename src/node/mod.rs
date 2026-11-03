//! Wires a node together: listeners, transport, Raft, assignment, workers and the fault gate.
//!
//! A client request goes: received → already decided? reply from state → assignment picks the
//! executor → executor plans on the worker pool → proposes `CommitResult` through Raft →
//! replies with the committed (winning) result. A node never replies before the commit, with one
//! exception: a node left alone answers provisionally (degraded.rs, D120). It never replies at all
//! while it is not serving, whatever it was waiting on when it stopped (D136).
//!
//! Each part's entry point (`heartbeat::spawn`, `faults::spawn_schedule`, `rejoin::spawn`, ...)
//! is called from here, so owners fill in their own file without editing this one.
//!
//! Owner: A (Raft and replication).

pub mod bids;
pub mod cpu;
pub mod degraded;
pub mod faults;
pub mod heartbeat;
pub mod rejoin;
pub mod release;
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
use crate::metrics::{Event, EventLog};
use crate::planner::{self, CostKnob};
use crate::proto::{
    ClientFrame, ClientReply, ClientRequest, Command, CommittedResult, ControlReply,
    ControlRequest, NodeId, NodeStatus, Outcome, PeerBody, PeerEnvelope, PeerReply, Plan,
    ProposeError, ReplyBody, RequestId, Responder, ResultProposal, ServerFrame,
};
use crate::raft::store::StateReader;
use crate::raft::{Consensus, RaftNode};
use bids::BidState;
use degraded::DegradedState;
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
    /// Bid rounds and the load figures bids are made from (bids.rs).
    pub bids: BidState,
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
        // A node with a Raft log from an earlier run may be behind (D118).
        serving: AtomicBool::new(!restarted),
        bids: BidState::new(id, &config),
        plan_work: AtomicU32::new(config.workers.plan_work),
        degraded: DegradedState::default(),
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
    tasks.extend(rejoin::spawn(&ctx, restarted));
    tasks.extend(bids::spawn(&ctx));
    tasks.push(release::spawn(&ctx));
    tasks.extend(degraded::spawn(&ctx));

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
        PeerBody::ReadIndex => PeerReply::ReadIndex(ctx.consensus.read_index_local().await),
        PeerBody::FaultOrder { ms } => {
            PeerReply::FaultOrder(faults::on_order(&ctx, Duration::from_millis(ms)))
        }
        PeerBody::Heartbeat => {
            lock(&ctx.suspicion).heard_from(from, Instant::now());
            PeerReply::Ack
        }
        PeerBody::Bid(bid) => {
            bids::on_bid(&ctx, from, &bid);
            PeerReply::Ack
        }
        PeerBody::Raft(_) => PeerReply::Ack,
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
    // Waiting to be replayed: a retry gets the same answer, in or out of degraded mode.
    if let Some(plan) = ctx.degraded.stored(&req.id) {
        send_provisional(&ctx, &reply, req.id, plan, started);
        return;
    }
    // Before any bid or proposal: neither can finish on one node.
    if ctx.degraded.is_active() {
        match degraded::plan(&ctx, &req).await {
            Ok(plan) => send_provisional(&ctx, &reply, req.id, plan, started),
            Err(e) => tracing::debug!(request = %req.id, "not answered: {e}"),
        }
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
async fn decide_executor(ctx: &NodeContext, req: &ClientRequest) -> Decision {
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
        let plan = plan_on_workers(ctx, req, free).await?;
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

/// Plan one request from `free` on the worker pool.
async fn plan_on_workers(
    ctx: &NodeContext,
    req: &ClientRequest,
    free: Vec<Responder>,
) -> Result<Plan, Overloaded> {
    let incident = req.incident.clone();
    // Two machines' clocks: NTP error is negligible against the planner's threshold (D95).
    let age_ms = (crate::metrics::now_us() / 1_000).saturating_sub(incident.reported_at_ms);
    let knob = CostKnob {
        extra_work: ctx.plan_work.load(Ordering::Relaxed),
    };
    // Timed inside the job: queue wait is already counted by `queued`.
    let (plan, planning) = ctx
        .workers
        .run(move || {
            let started = std::time::Instant::now();
            (
                planner::plan(&incident, &free, age_ms, knob),
                started.elapsed(),
            )
        })
        .await
        .inspect_err(|_| {
            // The client hears nothing and retries, so only this tells shed work from slow work.
            let pool = ctx.workers.load();
            ctx.log.log(Event::Overloaded {
                request: req.id,
                attempt: req.attempt,
                queued: pool.queued,
                running: pool.running,
            });
        })?;
    ctx.bids.record_service(planning);
    Ok(plan)
}

/// Whether a reply may leave now. A handler can wait on a commit for a long time, and by then the
/// node may have been silenced and be back, still catching up: `serving` when the request arrived
/// says nothing about now (D136). The client retries, as with any node that stays quiet (D36).
fn may_reply(ctx: &NodeContext, request: RequestId) -> bool {
    let serving = ctx.serving.load(Ordering::SeqCst);
    if !serving {
        tracing::debug!(%request, "not answered: not serving");
    }
    serving
}

fn send_provisional(
    ctx: &NodeContext,
    reply: &ReplyTx,
    request: RequestId,
    plan: Plan,
    started: Instant,
) {
    if !may_reply(ctx, request) {
        return;
    }
    let responder = plan.responder;
    let sent = reply.send(&ServerFrame::Reply(ClientReply {
        id: request,
        from: ctx.id,
        body: ReplyBody::Provisional(plan),
    }));
    if sent {
        ctx.log.log(Event::ProvisionalReplied {
            request,
            responder,
            latency_us: started.elapsed().as_micros() as u64,
        });
    }
}

fn send_done(ctx: &NodeContext, reply: &ReplyTx, done: CommittedResult, started: Instant) {
    let (request, executor) = (done.request, done.executor);
    if !may_reply(ctx, request) {
        return;
    }
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

/// Every `REPORT_INTERVAL`, log message counts (E5), the state digest and resource use (D128).
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
            let Ok(usage) = tokio::task::spawn_blocking(usage::read).await else {
                continue;
            };
            let pool = ctx.workers.load();
            ctx.log.log(Event::ResourceUsage {
                rss_kb: usage.rss_kb,
                cpu_ticks: usage.cpu_ticks,
                net_rx_bytes: usage.net_rx_bytes,
                net_tx_bytes: usage.net_tx_bytes,
                queued: pool.queued,
                running: pool.running,
            });
        }
    })
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use tokio::sync::mpsc;

    use super::testing::{ScriptedConsensus, context, log_dir, logged, until};
    use super::*;
    use crate::proto::{self, Capability, Incident, Point};

    fn request() -> ClientRequest {
        ClientRequest {
            id: RequestId { client: 7, seq: 1 },
            attempt: 0,
            incident: Incident {
                zone: 0,
                location: Point { x: 1, y: 2 },
                severity: 3,
                needs: Capability::Medical,
                reported_at_ms: 0,
            },
        }
    }

    fn replies(frames: &mut mpsc::UnboundedReceiver<Bytes>) -> Vec<ClientReply> {
        let mut replies = Vec::new();
        while let Ok(bytes) = frames.try_recv() {
            match proto::decode(&bytes).unwrap() {
                ServerFrame::Reply(reply) => replies.push(reply),
                other => panic!("expected a reply, got {other:?}"),
            }
        }
        replies
    }

    /// Runs one request on a leader that plans it itself, up to the moment it waits on the commit;
    /// `during_the_wait` then runs, the commit finishes and the handler ends.
    async fn wait_on_a_commit(
        ctx: &Arc<NodeContext>,
        fake: &ScriptedConsensus,
        during_the_wait: impl FnOnce(),
    ) -> Vec<ClientReply> {
        let (reply, mut frames) = ReplyTx::for_tests(ctx.gate.clone());
        let handler = tokio::spawn(handle_request(ctx.clone(), request(), reply));
        fake.proposal_arrived().await;
        during_the_wait();
        fake.commit_one();
        handler.await.unwrap();
        replies(&mut frames)
    }

    fn planning_leader(dir: &std::path::Path) -> (Arc<ScriptedConsensus>, Arc<NodeContext>) {
        let fake = ScriptedConsensus::new(1);
        fake.set_leader(Some(1));
        let log = EventLog::to_dir(dir, "node-1").unwrap();
        let ctx = context(fake.clone(), log, |c| {
            c.policy.assignment = AssignmentPolicy::LeaderDispatch;
        });
        (fake, ctx)
    }

    #[tokio::test]
    async fn a_serving_node_replies_once_the_commit_is_done() {
        let dir = log_dir("reply-while-serving");
        let (fake, ctx) = planning_leader(&dir);

        let replies = wait_on_a_commit(&ctx, &fake, || {}).await;

        assert!(matches!(replies[..], [ClientReply { from: 1, .. }]));
        let events = logged(&ctx.log, &dir);
        assert!(events.iter().any(|e| e["ev"] == "replied"));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The node was silenced with this request waiting on its commit, and is back, catching up. The
    /// commit is done, but the node has not yet caught up, so it must stay quiet (AGENTS.md).
    #[tokio::test]
    async fn a_commit_done_while_the_node_is_not_serving_gets_no_reply() {
        let dir = log_dir("reply-while-not-serving");
        let (fake, ctx) = planning_leader(&dir);

        let replies = wait_on_a_commit(&ctx, &fake, || {
            ctx.serving.store(false, Ordering::SeqCst);
        })
        .await;

        assert!(replies.is_empty(), "replied while not serving: {replies:?}");
        let events = logged(&ctx.log, &dir);
        assert!(events.iter().any(|e| e["ev"] == "request_received"));
        assert!(!events.iter().any(|e| e["ev"] == "replied"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_refused_planning_job_is_logged() {
        let dir = log_dir("overloaded");
        let log = EventLog::to_dir(&dir, "node-1").unwrap();
        let ctx = context(ScriptedConsensus::new(1), log, |c| {
            c.workers.threads = 1;
            c.workers.queue_capacity = 0;
        });
        // Keep the only worker busy until this test lets it go.
        let (let_go, hold) = std::sync::mpsc::channel::<()>();
        let busy = {
            let workers = ctx.workers.clone();
            tokio::spawn(async move { workers.run(move || hold.recv()).await })
        };
        until("the worker to be busy", || ctx.workers.load().running == 1).await;

        let refused = plan_on_workers(&ctx, &request(), Vec::new()).await;

        assert!(refused.is_err());
        let _ = let_go.send(());
        busy.await.unwrap().unwrap().unwrap();
        let events = logged(&ctx.log, &dir);
        let event = events.iter().find(|e| e["ev"] == "overloaded").unwrap();
        assert_eq!(event["request"]["seq"], 1);
        assert_eq!(event["attempt"], 0);
        assert_eq!(event["queued"], 0);
        assert_eq!(event["running"], 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn an_accepted_planning_job_logs_no_refusal() {
        let dir = log_dir("not-overloaded");
        let log = EventLog::to_dir(&dir, "node-1").unwrap();
        let ctx = context(ScriptedConsensus::new(1), log, |_| {});

        assert!(plan_on_workers(&ctx, &request(), Vec::new()).await.is_ok());

        let events = logged(&ctx.log, &dir);
        assert!(!events.iter().any(|e| e["ev"] == "overloaded"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_provisional_answer_also_needs_a_serving_node() {
        let dir = log_dir("provisional-while-not-serving");
        let (_, ctx) = planning_leader(&dir);
        let (reply, mut frames) = ReplyTx::for_tests(ctx.gate.clone());
        let plan = Plan {
            responder: None,
            cost: 1,
        };
        let send = || send_provisional(&ctx, &reply, request().id, plan.clone(), Instant::now());

        ctx.serving.store(false, Ordering::SeqCst);
        send();
        assert!(replies(&mut frames).is_empty());
        assert!(
            !logged(&ctx.log, &dir)
                .iter()
                .any(|e| e["ev"] == "provisional_replied")
        );

        ctx.serving.store(true, Ordering::SeqCst);
        send();
        assert_eq!(replies(&mut frames).len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
