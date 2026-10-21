//! openraft over our transport: Raft RPCs travel as `PeerBody::Raft` frames on the peer port.
//!
//! openraft asks for a new client for every heartbeat round and election, so a [`PeerLink`] is
//! only a handle onto the long-lived, shared [`PeerClient`] connection to that peer.
//!
//! Raft heartbeats (append-entries with no entries) are counted per peer in [`HeartbeatStats`]
//! and logged one by one at debug level under the `heartbeat` target.
//!
//! Owner: A (Raft and replication).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use openraft::BasicNode;
use openraft::error::{
    InstallSnapshotError, NetworkError, RPCError, RaftError, RemoteError, Unreachable,
};
use openraft::network::{Backoff, RPCOption, RaftNetwork, RaftNetworkFactory};
use openraft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse,
    VoteRequest, VoteResponse,
};

use crate::node::transport::{PeerClient, PeerSet, TransportError};
use crate::proto::{NodeId, PeerBody, PeerEnvelope, PeerReply, RaftRpc, Term};
use crate::raft::{Raft, TypeConfig};

type RpcResult<T, E = RaftError<NodeId>> = Result<T, RPCError<NodeId, BasicNode, E>>;

#[derive(Debug, thiserror::Error)]
#[error("peer answered a {expected} request with {got}")]
struct UnexpectedReply {
    expected: &'static str,
    got: &'static str,
}

fn reply_name(reply: &PeerReply) -> &'static str {
    match reply {
        PeerReply::Ack => "Ack",
        PeerReply::StaleTerm { .. } => "StaleTerm",
        PeerReply::AppendEntries(_) => "AppendEntries",
        PeerReply::Vote(_) => "Vote",
        PeerReply::InstallSnapshot(_) => "InstallSnapshot",
        PeerReply::Proposed(_) => "Proposed",
    }
}

/// Heartbeat counts for one peer since the last [`HeartbeatStats::take`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PeerBeats {
    /// Heartbeats this node sent to the peer.
    pub sent: u64,
    /// Of those, how many the peer answered.
    pub answered: u64,
    /// Heartbeats this node received from the peer.
    pub received: u64,
}

/// Raft heartbeats per peer, for the live log.
#[derive(Debug, Default)]
pub struct HeartbeatStats {
    peers: Mutex<BTreeMap<NodeId, PeerBeats>>,
}

impl HeartbeatStats {
    fn update(&self, peer: NodeId, f: impl FnOnce(&mut PeerBeats)) {
        let mut peers = self.peers.lock().unwrap_or_else(|p| p.into_inner());
        f(peers.entry(peer).or_default());
    }

    pub fn sent(&self, to: NodeId, answered: bool) {
        self.update(to, |b| {
            b.sent += 1;
            b.answered += u64::from(answered);
        });
    }

    pub fn received(&self, from: NodeId) {
        self.update(from, |b| b.received += 1);
    }

    /// Counts since the previous call; resets them.
    pub fn take(&self) -> BTreeMap<NodeId, PeerBeats> {
        std::mem::take(&mut *self.peers.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

/// Creates [`PeerLink`]s for openraft.
#[derive(Debug, Clone)]
pub struct Network {
    me: NodeId,
    peers: Arc<PeerSet>,
    /// How long openraft waits before retrying an unreachable peer.
    unreachable_backoff: Duration,
    heartbeats: Arc<HeartbeatStats>,
}

impl Network {
    pub fn new(
        me: NodeId,
        peers: Arc<PeerSet>,
        unreachable_backoff: Duration,
        heartbeats: Arc<HeartbeatStats>,
    ) -> Self {
        Self {
            me,
            peers,
            unreachable_backoff,
            heartbeats,
        }
    }
}

impl RaftNetworkFactory<TypeConfig> for Network {
    type Network = PeerLink;

    async fn new_client(&mut self, target: NodeId, _node: &BasicNode) -> PeerLink {
        PeerLink {
            me: self.me,
            target,
            client: self.peers.get(target).cloned(),
            unreachable_backoff: self.unreachable_backoff,
            heartbeats: self.heartbeats.clone(),
        }
    }
}

/// openraft's view of one peer.
#[derive(Debug)]
pub struct PeerLink {
    me: NodeId,
    target: NodeId,
    /// `None` if the target is in the Raft membership but not in our config.
    client: Option<Arc<PeerClient>>,
    unreachable_backoff: Duration,
    heartbeats: Arc<HeartbeatStats>,
}

impl PeerLink {
    async fn call<E: std::error::Error>(
        &self,
        term: Term,
        rpc: RaftRpc,
        timeout: Duration,
    ) -> RpcResult<PeerReply, E> {
        let Some(client) = &self.client else {
            return Err(rpc_error(TransportError::UnknownPeer(self.target)));
        };
        let envelope = PeerEnvelope {
            from: self.me,
            term,
            body: PeerBody::Raft(rpc),
        };
        client.call(envelope, timeout).await.map_err(rpc_error)
    }
}

/// Unreachable makes openraft back off before retrying; other failures retry on the next tick.
fn rpc_error<E: std::error::Error>(e: TransportError) -> RPCError<NodeId, BasicNode, E> {
    match e {
        TransportError::Silenced
        | TransportError::Connect { .. }
        | TransportError::UnknownPeer(_) => RPCError::Unreachable(Unreachable::new(&e)),
        _ => RPCError::Network(NetworkError::new(&e)),
    }
}

fn unexpected<E: std::error::Error>(
    expected: &'static str,
    got: &PeerReply,
) -> RPCError<NodeId, BasicNode, E> {
    RPCError::Network(NetworkError::new(&UnexpectedReply {
        expected,
        got: reply_name(got),
    }))
}

impl RaftNetwork<TypeConfig> for PeerLink {
    async fn append_entries(
        &mut self,
        rpc: AppendEntriesRequest<TypeConfig>,
        option: RPCOption,
    ) -> RpcResult<AppendEntriesResponse<NodeId>> {
        let term = rpc.vote.leader_id().get_term();
        let heartbeat = rpc.entries.is_empty();
        let result = match self
            .call(term, RaftRpc::AppendEntries(rpc), option.hard_ttl())
            .await
        {
            Ok(PeerReply::AppendEntries(r)) => {
                r.map_err(|e| RPCError::RemoteError(RemoteError::new(self.target, e)))
            }
            Ok(other) => Err(unexpected("AppendEntries", &other)),
            Err(e) => Err(e),
        };
        if heartbeat {
            let answered = result.is_ok();
            self.heartbeats.sent(self.target, answered);
            tracing::debug!(target: "heartbeat", to = self.target, term, answered, "heartbeat sent");
        }
        result
    }

    async fn install_snapshot(
        &mut self,
        rpc: InstallSnapshotRequest<TypeConfig>,
        option: RPCOption,
    ) -> RpcResult<InstallSnapshotResponse<NodeId>, RaftError<NodeId, InstallSnapshotError>> {
        let term = rpc.vote.leader_id().get_term();
        match self
            .call(term, RaftRpc::InstallSnapshot(rpc), option.hard_ttl())
            .await?
        {
            PeerReply::InstallSnapshot(r) => {
                r.map_err(|e| RPCError::RemoteError(RemoteError::new(self.target, e)))
            }
            other => Err(unexpected("InstallSnapshot", &other)),
        }
    }

    async fn vote(
        &mut self,
        rpc: VoteRequest<NodeId>,
        option: RPCOption,
    ) -> RpcResult<VoteResponse<NodeId>> {
        let term = rpc.vote.leader_id().get_term();
        match self
            .call(term, RaftRpc::Vote(rpc), option.hard_ttl())
            .await?
        {
            PeerReply::Vote(r) => {
                r.map_err(|e| RPCError::RemoteError(RemoteError::new(self.target, e)))
            }
            other => Err(unexpected("Vote", &other)),
        }
    }

    fn backoff(&self) -> Backoff {
        Backoff::new(std::iter::repeat(self.unreachable_backoff))
    }
}

/// Hand an incoming Raft RPC from `from` to openraft and wrap its answer for the wire.
pub async fn serve(
    raft: &Raft,
    heartbeats: &HeartbeatStats,
    from: NodeId,
    rpc: RaftRpc,
) -> PeerReply {
    match rpc {
        RaftRpc::AppendEntries(r) => {
            if r.entries.is_empty() {
                heartbeats.received(from);
                let term = r.vote.leader_id().get_term();
                tracing::debug!(target: "heartbeat", from, term, "heartbeat received");
            }
            PeerReply::AppendEntries(raft.append_entries(r).await)
        }
        RaftRpc::Vote(r) => PeerReply::Vote(raft.vote(r).await),
        RaftRpc::InstallSnapshot(r) => PeerReply::InstallSnapshot(raft.install_snapshot(r).await),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heartbeat_counts_reset_on_take() {
        let stats = HeartbeatStats::default();
        stats.sent(2, true);
        stats.sent(2, false);
        stats.received(3);
        let beats = stats.take();
        assert_eq!(
            beats[&2],
            PeerBeats {
                sent: 2,
                answered: 1,
                received: 0
            }
        );
        assert_eq!(beats[&3].received, 1);
        assert!(stats.take().is_empty());
    }
}
