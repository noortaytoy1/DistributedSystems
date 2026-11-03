//! Raft through openraft 0.9.25: elects the cloud coordinator and orders every command.
//!
//! Node code talks to Raft only through the [`Consensus`] trait, so the library can be swapped
//! for our own Raft without touching `node/`.
//!
//! - On its first start every node calls `initialize` with the full membership from the config;
//!   openraft treats the repeats as no-ops, so the cluster forms whichever nodes start first.
//! - Any node can propose. A follower forwards its proposal to the leader (`PeerBody::Propose`)
//!   and retries across leader changes until `timers.propose_timeout_ms`, which is a real cap on
//!   the wait. Retrying is safe because every `Command` is idempotent.
//! - On the leader every proposal, its own or forwarded, goes through one queue (proposals.rs): one
//!   `client_write` at a time, carrying everything that waited as one log entry, so that disk saves
//!   cannot stop the heartbeats (D133).
//! - A busy leader can go quiet: no heartbeats, and the queue held. Nothing then resets the
//!   followers' election timers, so they start an ordinary vote (node/step_aside.rs, D218).
//! - Each finished proposal logs a `commit` event with its latency, and every term, leader or role
//!   change logs a `raft_state` event (E5).
//! - Once a second each node logs a one-line summary of the Raft heartbeats it sent and received
//!   (`heartbeat` target; `RUST_LOG=heartbeat=debug` shows every single heartbeat).
//!
//! Owner: A (Raft and replication).

pub mod network;
pub mod proposals;
pub mod store;
pub mod wal;

use std::collections::BTreeMap;
use std::io::Cursor;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use futures::future::BoxFuture;
use openraft::error::{CheckIsLeaderError, ClientWriteError, InitializeError, RaftError};
use openraft::{BasicNode, ServerState, SnapshotPolicy, TokioRuntime};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::config::{Config, Timers};
use crate::metrics::{Event, EventLog};
use crate::node::transport::PeerSet;
use crate::proto::{
    Command, CommandGroup, NodeId, Outcome, PeerBody, PeerEnvelope, PeerReply, ProposeError,
    RaftRpc, Term,
};
use network::{HeartbeatStats, Network, PeerBeats};
use proposals::{GROUP_LIMIT, ProposalQueue};
use store::{LogStats, LogStore, StateMachineStore, StateReader};

/// How often the heartbeat summary is logged.
const HEARTBEAT_SUMMARY_INTERVAL: Duration = Duration::from_secs(1);

/// How often the proposal queue's and the log file's counters are logged.
const IO_REPORT_INTERVAL: Duration = Duration::from_secs(1);

openraft::declare_raft_types!(
    /// openraft type configuration for RescueMesh.
    pub TypeConfig:
        D = CommandGroup,
        R = Vec<Outcome>,
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

    /// Changes on every AppendEntries this node accepts from a current leader, carrying its
    /// `leader_commit`. That value can lag the leader's real commit index, so it is only the sign
    /// that this node has a current leader and term again; the target comes from `read_index`
    /// (D75, D76).
    fn leader_commit(&self) -> watch::Receiver<Option<u64>>;

    /// The leader's commit index, confirmed by a majority (Raft ReadIndex). Asks the leader over
    /// `PeerBody::ReadIndex` when this node is not the leader (D76). Nothing committed before the
    /// call is above the answer, but a leader just elected answers with its first entry of the
    /// term, which is not committed yet and is lost if that leader is replaced: never wait on the
    /// answer for ever (D135).
    fn read_index(&self) -> BoxFuture<'_, Result<Option<u64>, ProposeError>>;

    /// `read_index` only if this node is the leader; used for queries from peers.
    fn read_index_local(&self) -> BoxFuture<'_, Result<Option<u64>, ProposeError>>;

    /// Resolve once this node has applied `index`, or with false after `timeout`.
    fn wait_applied(&self, index: u64, timeout: Duration) -> BoxFuture<'_, bool>;

    /// Allow or forbid this node to start elections. Off while a node is not serving, so a
    /// returning node does not raise its term and depose a healthy leader (D5, D75).
    fn set_elect(&self, enabled: bool);

    /// Go quiet, or stop being quiet: no Raft heartbeats, and no new log writes, which wait in the
    /// proposal queue instead. This changes no election timer and no vote rule: the followers'
    /// timers just run out, as they would for a leader that failed (D218).
    fn set_quiet(&self, quiet: bool);

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
    leader_commit: watch::Sender<Option<u64>>,
    /// Who this node believes leads. Changes only when that does, so waiting on it costs nothing
    /// under load, unlike the metrics channel, which changes with every entry.
    leader: watch::Sender<Option<NodeId>>,
    queue: ProposalQueue,
    log_stats: Arc<LogStats>,
}

impl RaftNode {
    /// Create this node's Raft instance and join the cluster described by `config`.
    pub async fn start(
        id: NodeId,
        config: &Config,
        peers: Arc<PeerSet>,
        log: EventLog,
    ) -> anyhow::Result<(Arc<Self>, StateReader, bool)> {
        let t = &config.timers;
        let (election_min, election_max) = election_range(id, config);
        let (log_store, restarted) = match &config.storage.dir {
            Some(dir) => LogStore::open(dir.join(format!("node-{id}.wal"))).await?,
            None => (LogStore::default(), false),
        };
        let log_stats = log_store.stats();
        let raft_config = openraft::Config {
            cluster_name: "rescuemesh".into(),
            heartbeat_interval: t.raft_heartbeat_ms,
            election_timeout_min: election_min,
            election_timeout_max: election_max,
            snapshot_policy: SnapshotPolicy::Never,
            // A restarted node must not raise its term before it hears from a leader (D118).
            enable_elect: !restarted,
            ..Default::default()
        }
        .validate()?;

        let (state_machine, state) = StateMachineStore::new(log.clone());
        // openraft's default 500 ms backoff would delay catching up a node that just returned.
        let backoff = Duration::from_millis(2 * t.raft_heartbeat_ms);
        let heartbeats = Arc::new(HeartbeatStats::default());
        let network = Network::new(id, peers.clone(), backoff, heartbeats.clone());
        let raft = Raft::new(id, Arc::new(raft_config), network, log_store, state_machine).await?;

        // A restarted node has its membership in its log file, and openraft would refuse to
        // initialize it again and log an ERROR for it (D137). Asking also waits for Raft's first
        // metrics, which the rest of the node reads as soon as this returns.
        if !raft.is_initialized().await? {
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
        }

        let queue = {
            let raft = raft.clone();
            ProposalQueue::start(GROUP_LIMIT, t.propose_timeout(), move |group| {
                let raft = raft.clone();
                async move { write_group(&raft, group).await }
            })
        };
        let node = Arc::new(Self {
            id,
            raft,
            peers,
            timers: t.clone(),
            log,
            heartbeats,
            leader_commit: watch::channel(None).0,
            leader: watch::channel(None).0,
            queue,
            log_stats,
        });
        Ok((node, state, restarted))
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

    /// Log the proposal queue's and the log file's counters every second until the task is aborted.
    pub fn spawn_io_report(self: &Arc<Self>) -> JoinHandle<()> {
        let node = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(IO_REPORT_INTERVAL);
            loop {
                tick.tick().await;
                node.log.log(node.io_report());
            }
        })
    }

    fn io_report(&self) -> Event {
        let queue = self.queue.stats();
        let saved = &self.log_stats;
        Event::RaftIo {
            waiting: queue.waiting.load(Ordering::Relaxed),
            writes: queue.writes.load(Ordering::Relaxed),
            commands: queue.commands.load(Ordering::Relaxed),
            expired: queue.expired.load(Ordering::Relaxed),
            write_timeouts: queue.write_timeouts.load(Ordering::Relaxed),
            syncs: saved.syncs.load(Ordering::Relaxed),
            sync_us: saved.sync_us.load(Ordering::Relaxed),
            max_sync_us: saved.max_sync_us.swap(0, Ordering::Relaxed),
            saves: saved.saves.load(Ordering::Relaxed),
            saved_entries: saved.entries.load(Ordering::Relaxed),
            saved_commands: saved.commands.load(Ordering::Relaxed),
            max_leader_gap_us: self
                .heartbeats
                .take_largest_gap_us(self.status().role == Role::Leader),
        }
    }

    pub fn raft(&self) -> &Raft {
        &self.raft
    }

    /// Log every term, leader or role change until Raft shuts down.
    pub fn spawn_watcher(&self) -> JoinHandle<()> {
        let mut metrics = self.raft.metrics();
        let log = self.log.clone();
        let id = self.id;
        let current_leader = self.leader.clone();
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
                    current_leader.send_if_modified(|known| {
                        let changed = *known != leader;
                        *known = leader;
                        changed
                    });
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
        let mut leader_change = self.leader.subscribe();
        let reply = tokio::select! {
            reply = peer.call(envelope, timeout) => reply,
            // Long before the deadline: a leader that went silent never answers.
            _ = leader_change.changed() => return Err(ProposeError::NotLeader { leader: None }),
        };
        match reply {
            Ok(PeerReply::Proposed(result)) => result,
            // Our term is behind; retry once Raft has caught us up.
            Ok(_) => Err(ProposeError::NotLeader { leader: None }),
            Err(_) => Err(ProposeError::Timeout),
        }
    }

    /// Ask `leader` for its confirmed commit index (`PeerBody::ReadIndex`, D76).
    async fn forward_read_index(&self, leader: NodeId) -> Result<Option<u64>, ProposeError> {
        let Some(peer) = self.peers.get(leader) else {
            return Err(ProposeError::NotLeader { leader: None });
        };
        let envelope = PeerEnvelope {
            from: self.id,
            term: self.status().term,
            body: PeerBody::ReadIndex,
        };
        match peer.call(envelope, self.timers.peer_rpc_timeout()).await {
            Ok(PeerReply::ReadIndex(result)) => result,
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
            // A follower that knows the leader goes straight there, instead of waiting in its own
            // queue to be told so.
            let hint = match self.status().leader {
                Some(leader) if leader != self.id => Some(leader),
                _ => match self.queue.submit(cmd.clone(), deadline).await {
                    Ok(outcome) => return (Ok(outcome), forwarded),
                    Err(ProposeError::NotLeader { leader }) => leader,
                    Err(e @ ProposeError::Fatal(_)) => return (Err(e), forwarded),
                    Err(ProposeError::Timeout) => None,
                },
            };
            if let Some(leader) = hint.filter(|l| *l != self.id) {
                forwarded = true;
                let remaining = deadline.saturating_duration_since(Instant::now());
                // The answer can take as long as the leader's queue does, so this waits up to the
                // deadline: giving up sooner would only add the same command to that queue again.
                // Any failure here is retried: the leader may have changed or be unreachable.
                if let Ok(outcome) = self.forward(leader, cmd.clone(), remaining).await {
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
        Box::pin(async move {
            // The sender's news is old: tell it now, instead of after a wait in the queue.
            if let Some(leader) = self.status().leader.filter(|l| *l != self.id) {
                return Err(ProposeError::NotLeader {
                    leader: Some(leader),
                });
            }
            let deadline = Instant::now() + self.timers.propose_timeout();
            self.queue.submit(cmd, deadline).await
        })
    }

    fn handle_rpc(&self, from: NodeId, rpc: RaftRpc) -> BoxFuture<'_, PeerReply> {
        Box::pin(network::serve(
            &self.raft,
            &self.heartbeats,
            &self.leader_commit,
            from,
            rpc,
        ))
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

    fn leader_commit(&self) -> watch::Receiver<Option<u64>> {
        self.leader_commit.subscribe()
    }

    fn read_index(&self) -> BoxFuture<'_, Result<Option<u64>, ProposeError>> {
        Box::pin(async move {
            match self.read_index_local().await {
                Err(ProposeError::NotLeader {
                    leader: Some(leader),
                }) if leader != self.id => self.forward_read_index(leader).await,
                other => other,
            }
        })
    }

    fn read_index_local(&self) -> BoxFuture<'_, Result<Option<u64>, ProposeError>> {
        Box::pin(async move {
            match self.raft.get_read_log_id().await {
                Ok((read_log_id, _applied)) => Ok(read_log_id.map(|l| l.index)),
                Err(RaftError::APIError(CheckIsLeaderError::ForwardToLeader(f))) => {
                    Err(ProposeError::NotLeader {
                        leader: f.leader_id,
                    })
                }
                Err(RaftError::APIError(CheckIsLeaderError::QuorumNotEnough(_))) => {
                    Err(ProposeError::Timeout)
                }
                Err(RaftError::Fatal(e)) => Err(ProposeError::Fatal(e.to_string())),
            }
        })
    }

    fn wait_applied(&self, index: u64, timeout: Duration) -> BoxFuture<'_, bool> {
        Box::pin(async move {
            self.raft
                .wait(Some(timeout))
                .applied_index_at_least(Some(index), "rejoin catch-up")
                .await
                .is_ok()
        })
    }

    fn set_elect(&self, enabled: bool) {
        self.raft.runtime_config().elect(enabled);
    }

    fn set_quiet(&self, quiet: bool) {
        self.raft.runtime_config().heartbeat(!quiet);
        self.queue.hold(quiet);
    }

    fn shutdown(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            if let Err(e) = self.raft.shutdown().await {
                tracing::warn!("raft shutdown: {e}");
            }
            self.queue.stop();
        })
    }
}

/// One `client_write`, with openraft's errors in our terms.
async fn write_group(raft: &Raft, group: CommandGroup) -> Result<Vec<Outcome>, ProposeError> {
    match raft.client_write(group).await {
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

/// This node's election timeout range. openraft draws one timeout per process and checks it on a
/// tick of 1.5 heartbeats, so two nodes can land on the same effective timeout and split the vote
/// in every term. Ranges two ticks apart, by the node's place in the config, rule that out (D115).
fn election_range(id: NodeId, config: &Config) -> (u64, u64) {
    let t = &config.timers;
    let tick = t.raft_heartbeat_ms * 3 / 2;
    let width = tick.min(t.election_timeout_max_ms - t.election_timeout_min_ms);
    let place = config
        .cluster
        .nodes
        .iter()
        .position(|n| n.id == id)
        .unwrap_or(0) as u64;
    let min = t.election_timeout_min_ms + place * 2 * tick;
    (min, min + width)
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

    #[test]
    fn election_ranges_of_two_nodes_are_at_least_a_tick_apart() {
        let config = crate::config::Config::from_toml(
            r#"
            [[cluster.nodes]]
            id = 1
            peer_addr = "a:1"
            client_addr = "a:2"
            [[cluster.nodes]]
            id = 2
            peer_addr = "b:1"
            client_addr = "b:2"
            [[cluster.nodes]]
            id = 3
            peer_addr = "c:1"
            client_addr = "c:2"
            "#,
        )
        .unwrap();
        assert_eq!(election_range(1, &config), (150, 225));
        assert_eq!(election_range(2, &config), (300, 375));
        assert_eq!(election_range(3, &config), (450, 525));
    }
}
