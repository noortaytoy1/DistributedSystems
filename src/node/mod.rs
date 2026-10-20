//! Wires a node together: listeners, transport, Raft, assignment, workers and the fault gate.
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

use crate::config::Config;
use crate::metrics::{Event, EventLog};
use crate::proto::{
    ClientFrame, ClientReply, ControlReply, ControlRequest, NodeId, NodeStatus, PeerBody,
    PeerEnvelope, PeerReply, ReplyBody, ServerFrame,
};
use heartbeat::Suspicion;
use transport::{ClientHandler, FaultGate, PeerHandler, PeerSet, ReplyTx};
use workers::WorkerPool;

/// Everything a node's parts share.
pub struct NodeContext {
    pub id: NodeId,
    pub config: Arc<Config>,
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
    let ctx = Arc::new(NodeContext {
        id,
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
    log.log(Event::Started {
        peer_addr: peer_addr.to_string(),
        client_addr: client_addr.to_string(),
    });

    let peer_handler: PeerHandler = {
        let ctx = ctx.clone();
        Arc::new(move |env| on_peer_message(ctx.clone(), env).boxed())
    };
    let client_handler: ClientHandler = {
        let ctx = ctx.clone();
        Arc::new(move |frame, reply| on_client_frame(&ctx, frame, reply))
    };
    let mut tasks = vec![
        tokio::spawn(transport::serve_peers(
            peer_listener,
            gate.clone(),
            log.msgs().clone(),
            peer_handler,
        )),
        tokio::spawn(transport::serve_clients(
            client_listener,
            gate,
            log.msgs().clone(),
            client_handler,
        )),
    ];
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

async fn on_peer_message(ctx: Arc<NodeContext>, env: PeerEnvelope) -> PeerReply {
    match env.body {
        PeerBody::Heartbeat => {
            lock(&ctx.suspicion).heard_from(env.from, tokio::time::Instant::now());
            PeerReply::Ack
        }
        // TODO(A): hand Raft RPCs and forwarded proposals to Raft. TODO(B): bids.
        PeerBody::Raft(_) | PeerBody::Propose(_) | PeerBody::Bid(_) => PeerReply::Ack,
    }
}

fn on_client_frame(ctx: &Arc<NodeContext>, frame: ClientFrame, reply: ReplyTx) {
    match frame {
        ClientFrame::Request(req) => {
            ctx.log.log(Event::RequestReceived {
                request: req.id,
                attempt: req.attempt,
            });
            // TODO(A): plan, commit through Raft, then reply. Stub reply for now (M0).
            reply.send(&ServerFrame::Reply(ClientReply {
                id: req.id,
                from: ctx.id,
                body: ReplyBody::Rejected("planning is not wired up yet".into()),
            }));
        }
        ClientFrame::Control(cmd) => {
            let answer = on_control(ctx, cmd);
            reply.send(&ServerFrame::Control(answer));
        }
    }
}

fn on_control(ctx: &Arc<NodeContext>, cmd: ControlRequest) -> ControlReply {
    match cmd {
        ControlRequest::Status => ControlReply::Status(NodeStatus {
            id: ctx.id,
            term: 0,
            leader: None,
            role: "none".into(),
            last_log_index: None,
            last_applied: None,
            digest: 0,
            results: 0,
            responders: 0,
            silenced: ctx.gate.is_silenced(),
            serving: ctx.serving.load(Ordering::SeqCst),
        }),
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

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}
