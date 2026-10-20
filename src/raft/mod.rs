//! Raft through openraft 0.9.25: elects the cloud coordinator and orders every command.
//!
//! Node code talks to Raft only through the [`Consensus`] trait, so the library can be swapped
//! for our own Raft without touching `node/`.
//!
//! - Every node calls `initialize` with the full membership from the config; openraft treats the
//!   repeats as no-ops, so the cluster forms whichever nodes start first.
//! - Any node can propose. A follower forwards its proposal to the leader (`PeerBody::Propose`)
//!   and retries across leader changes until `timers.propose_timeout_ms`. Retrying is safe
//!   because every `Command` is idempotent.
//! - Each finished proposal logs a `commit` event with its latency, and every term, leader or role
//!   change logs a `raft_state` event (E5).
//!
//! Owner: A (Raft and replication).

pub mod network;
pub mod store;

use std::collections::BTreeMap;
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use futures::future::BoxFuture;
use openraft::error::{ClientWriteError, InitializeError, RaftError};
use openraft::{BasicNode, ServerState, SnapshotPolicy, TokioRuntime};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::config::{Config, Timers};
use crate::metrics::{Event, EventLog};
use crate::node::transport::PeerSet;
use crate::proto::{
    Command, NodeId, Outcome, PeerBody, PeerEnvelope, PeerReply, ProposeError, RaftRpc, Term,
};
use network::Network;
use store::{LogStore, StateMachineStore, StateReader};

openraft::declare_raft_types!(
    /// openraft type configuration for RescueMesh.
    pub TypeConfig:
        D = Command,
        R = Outcome,
        NodeId = NodeId,
        Node = BasicNode,
        Entry = openraft::Entry<TypeConfig>,
        SnapshotData = Cursor<Vec<u8>>,
        AsyncRuntime = TokioRuntime,
);

pub type Raft = openraft::Raft<TypeConfig>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Leader,
    Follower,
    Candidate,
    Learner,
    Shutdown,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Leader => "leader",
            Role::Follower => "follower",
            Role::Candidate => "candidate",
            Role::Learner => "learner",
            Role::Shutdown => "shutdown",
        }
    }
}

impl From<ServerState> for Role {
    fn from(s: ServerState) -> Self {
        match s {
            ServerState::Leader => Role::Leader,
            ServerState::Follower => Role::Follower,
            ServerState::Candidate => Role::Candidate,
            ServerState::Learner => Role::Learner,
            ServerState::Shutdown => Role::Shutdown,
        }
    }
}

/// This node's view of the cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsensusStatus {
    pub id: NodeId,
    pub term: Term,
    pub leader: Option<NodeId>,
    pub role: Role,
    pub last_log_index: Option<u64>,
    pub last_applied: Option<u64>,
}

/// What node code needs from consensus.
pub trait Consensus: Send + Sync + 'static {
    /// Commit `cmd` and return its outcome once applied. Forwards to the leader and retries
    /// across leader changes until the propose timeout.
    fn propose(&self, cmd: Command) -> BoxFuture<'_, Result<Outcome, ProposeError>>;

    /// Commit `cmd` only if this node is the leader; used for proposals forwarded to us.
    fn propose_local(&self, cmd: Command) -> BoxFuture<'_, Result<Outcome, ProposeError>>;

    /// Hand an incoming Raft RPC to the Raft implementation.
    fn handle_rpc(&self, rpc: RaftRpc) -> BoxFuture<'_, PeerReply>;

    fn status(&self) -> ConsensusStatus;

    fn shutdown(&self) -> BoxFuture<'_, ()>;
}

/// openraft-backed [`Consensus`].
pub struct RaftNode {
    id: NodeId,
    raft: Raft,
    peers: Arc<PeerSet>,
    timers: Timers,
    log: EventLog,
}

impl RaftNode {
    /// Create this node's Raft instance and join the cluster described by `config`.
    pub async fn start(
        id: NodeId,
        config: &Config,
        peers: Arc<PeerSet>,
        log: EventLog,
    ) -> anyhow::Result<(Arc<Self>, StateReader)> {
        let t = &config.timers;
        let raft_config = openraft::Config {
            cluster_name: "rescuemesh".into(),
            heartbeat_interval: t.raft_heartbeat_ms,
            election_timeout_min: t.election_timeout_min_ms,
            election_timeout_max: t.election_timeout_max_ms,
            snapshot_policy: SnapshotPolicy::Never,
            ..Default::default()
        }
        .validate()?;

        let (state_machine, state) = StateMachineStore::new();
        // openraft's default 500 ms backoff would delay catching up a node that just returned.
        let backoff = Duration::from_millis(2 * t.raft_heartbeat_ms);
        let network = Network::new(id, peers.clone(), backoff);
        let raft = Raft::new(
            id,
            Arc::new(raft_config),
            network,
            LogStore::default(),
            state_machine,
        )
        .await?;

        let members: BTreeMap<NodeId, BasicNode> = config
            .cluster
            .nodes
            .iter()
            .map(|n| (n.id, BasicNode::new(&n.peer_addr)))
            .collect();
        match raft.initialize(members).await {
            Ok(()) | Err(RaftError::APIError(InitializeError::NotAllowed(_))) => {}
            Err(e) => return Err(e.into()),
        }

        let node = Arc::new(Self {
            id,
            raft,
            peers,
            timers: t.clone(),
            log,
        });
        Ok((node, state))
    }

    pub fn raft(&self) -> &Raft {
        &self.raft
    }

    /// Log every term, leader or role change until Raft shuts down.
    pub fn spawn_watcher(&self) -> JoinHandle<()> {
        let mut metrics = self.raft.metrics();
        let log = self.log.clone();
        let id = self.id;
        tokio::spawn(async move {
            let mut last = None;
            loop {
                let (term, leader, role, applied) = {
                    let m = metrics.borrow_and_update();
                    (
                        m.current_term,
                        m.current_leader,
                        Role::from(m.state),
                        m.last_applied.map(|l| l.index),
                    )
                };
                if last != Some((term, leader, role)) {
                    last = Some((term, leader, role));
                    tracing::info!(node = id, term, ?leader, role = role.as_str(), "raft state");
                    log.log(Event::RaftState {
                        term,
                        leader,
                        role: role.as_str().into(),
                        last_applied: applied,
                    });
                }
                if metrics.changed().await.is_err() {
                    return;
                }
            }
        })
    }

    async fn write_local(&self, cmd: Command) -> Result<Outcome, ProposeError> {
        match self.raft.client_write(cmd).await {
            Ok(resp) => Ok(resp.data),
            Err(RaftError::APIError(ClientWriteError::ForwardToLeader(f))) => {
                Err(ProposeError::NotLeader {
                    leader: f.leader_id,
                })
            }
            Err(RaftError::APIError(e)) => Err(ProposeError::Fatal(e.to_string())),
            Err(RaftError::Fatal(e)) => Err(ProposeError::Fatal(e.to_string())),
        }
    }

    async fn forward(
        &self,
        leader: NodeId,
        cmd: Command,
        timeout: Duration,
    ) -> Result<Outcome, ProposeError> {
        let Some(peer) = self.peers.get(leader) else {
            return Err(ProposeError::NotLeader { leader: None });
        };
        let envelope = PeerEnvelope {
            from: self.id,
            term: self.status().term,
            body: PeerBody::Propose(cmd),
        };
        match peer.call(envelope, timeout).await {
            Ok(PeerReply::Proposed(result)) => result,
            // Our term is behind; retry once Raft has caught us up.
            Ok(_) => Err(ProposeError::NotLeader { leader: None }),
            Err(_) => Err(ProposeError::Timeout),
        }
    }

    /// Returns the outcome and whether the proposal was forwarded to another node.
    async fn propose_with_retry(&self, cmd: Command) -> (Result<Outcome, ProposeError>, bool) {
        let deadline = Instant::now() + self.timers.propose_timeout();
        let mut backoff = Duration::from_millis(5);
        let mut forwarded = false;
        loop {
            let hint = match self.write_local(cmd.clone()).await {
                Ok(outcome) => return (Ok(outcome), forwarded),
                Err(ProposeError::NotLeader { leader }) => leader,
                Err(e @ ProposeError::Fatal(_)) => return (Err(e), forwarded),
                Err(ProposeError::Timeout) => None,
            };
            if let Some(leader) = hint.filter(|l| *l != self.id) {
                forwarded = true;
                let remaining = deadline.saturating_duration_since(Instant::now());
                let timeout = remaining.min(self.timers.peer_rpc_timeout());
                // Any failure here is retried: the leader may have changed or be unreachable.
                if let Ok(outcome) = self.forward(leader, cmd.clone(), timeout).await {
                    return (Ok(outcome), forwarded);
                }
            }
            if Instant::now() + backoff >= deadline {
                return (Err(ProposeError::Timeout), forwarded);
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_millis(100));
        }
    }
}

impl Consensus for RaftNode {
    fn propose(&self, cmd: Command) -> BoxFuture<'_, Result<Outcome, ProposeError>> {
        Box::pin(async move {
            let started = Instant::now();
            let (request, kind) = (cmd.request(), cmd.kind());
            let (result, forwarded) = self.propose_with_retry(cmd).await;
            let latency_us = started.elapsed().as_micros() as u64;
            match &result {
                Ok(outcome) => self.log.log(Event::Commit {
                    request,
                    kind,
                    outcome: outcome.kind(),
                    latency_us,
                    forwarded,
                    log_index: match outcome {
                        Outcome::Committed(r) => Some(r.log_index),
                        _ => None,
                    },
                }),
                Err(e) => self.log.log(Event::ProposeFailed {
                    request,
                    kind,
                    error: e.to_string(),
                    latency_us,
                }),
            }
            result
        })
    }

    fn propose_local(&self, cmd: Command) -> BoxFuture<'_, Result<Outcome, ProposeError>> {
        Box::pin(self.write_local(cmd))
    }

    fn handle_rpc(&self, rpc: RaftRpc) -> BoxFuture<'_, PeerReply> {
        Box::pin(network::serve(&self.raft, rpc))
    }

    fn status(&self) -> ConsensusStatus {
        let metrics = self.raft.metrics();
        let m = metrics.borrow();
        ConsensusStatus {
            id: self.id,
            term: m.current_term,
            leader: m.current_leader,
            role: m.state.into(),
            last_log_index: m.last_log_index,
            last_applied: m.last_applied.map(|l| l.index),
        }
    }

    fn shutdown(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            if let Err(e) = self.raft.shutdown().await {
                tracing::warn!("raft shutdown: {e}");
            }
        })
    }
}
