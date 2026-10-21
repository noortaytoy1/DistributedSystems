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
//! - Once a second each node logs a one-line summary of the Raft heartbeats it sent and received
//!   (`heartbeat` target; `RUST_LOG=heartbeat=debug` shows every single heartbeat).
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
use network::{HeartbeatStats, Network, PeerBeats};
use store::{LogStore, StateMachineStore, StateReader};

/// How often the heartbeat summary is logged.
const HEARTBEAT_SUMMARY_INTERVAL: Duration = Duration::from_secs(1);

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

    /// Hand an incoming Raft RPC from peer `from` to the Raft implementation.
    fn handle_rpc(&self, from: NodeId, rpc: RaftRpc) -> BoxFuture<'_, PeerReply>;

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
    heartbeats: Arc<HeartbeatStats>,
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
        let heartbeats = Arc::new(HeartbeatStats::default());
        let network = Network::new(id, peers.clone(), backoff, heartbeats.clone());
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
            heartbeats,
        });
        Ok((node, state))
    }

    /// Log a one-line heartbeat summary every second until the task is aborted.
    pub fn spawn_heartbeat_summary(self: &Arc<Self>) -> JoinHandle<()> {
        let node = self.clone();
        tokio::spawn(async move {
            let peers: Vec<NodeId> = node.peers.iter().map(|p| p.peer()).collect();
            let mut tick = tokio::time::interval(HEARTBEAT_SUMMARY_INTERVAL);
            tick.tick().await;
            loop {
                tick.tick().await;
                let status = node.status();
                let beats = node.heartbeats.take();
                tracing::info!(target: "heartbeat", "{}", heartbeat_summary(&status, &peers, &beats));
            }
        })
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

    fn handle_rpc(&self, from: NodeId, rpc: RaftRpc) -> BoxFuture<'_, PeerReply> {
        Box::pin(network::serve(&self.raft, &self.heartbeats, from, rpc))
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

/// e.g. `leader, term 5 | to 2: 20 sent, 20 answered | to 3: 20 sent, 0 answered`
/// or `follower, term 5, leader 1 | from 1: 20 received`.
fn heartbeat_summary(
    status: &ConsensusStatus,
    peers: &[NodeId],
    beats: &BTreeMap<NodeId, PeerBeats>,
) -> String {
    let mut line = format!("{}, term {}", status.role.as_str(), status.term);
    match status.leader {
        Some(leader) if leader != status.id => line.push_str(&format!(", leader {leader}")),
        Some(_) => {}
        None => line.push_str(", no leader"),
    }
    let mut any = false;
    for peer in peers {
        let b = beats.get(peer).copied().unwrap_or_default();
        if status.role == Role::Leader {
            line.push_str(&format!(
                " | to {peer}: {} sent, {} answered",
                b.sent, b.answered
            ));
            any = true;
        } else if b.received > 0 || status.leader == Some(*peer) {
            line.push_str(&format!(" | from {peer}: {} received", b.received));
            any = true;
        }
    }
    if !any {
        line.push_str(" | no heartbeats");
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(id: NodeId, role: Role, leader: Option<NodeId>) -> ConsensusStatus {
        ConsensusStatus {
            id,
            term: 5,
            leader,
            role,
            last_log_index: None,
            last_applied: None,
        }
    }

    #[test]
    fn leader_summary_shows_every_follower_even_silent_ones() {
        let beats = BTreeMap::from([(
            2,
            PeerBeats {
                sent: 20,
                answered: 20,
                received: 0,
            },
        )]);
        assert_eq!(
            heartbeat_summary(&status(1, Role::Leader, Some(1)), &[2, 3], &beats),
            "leader, term 5 | to 2: 20 sent, 20 answered | to 3: 0 sent, 0 answered"
        );
    }

    #[test]
    fn follower_summary_shows_its_leader_even_when_silent() {
        let none = BTreeMap::new();
        assert_eq!(
            heartbeat_summary(&status(2, Role::Follower, Some(1)), &[1, 3], &none),
            "follower, term 5, leader 1 | from 1: 0 received"
        );
        assert_eq!(
            heartbeat_summary(&status(2, Role::Candidate, None), &[1, 3], &none),
            "candidate, term 5, no leader | no heartbeats"
        );
    }
}
