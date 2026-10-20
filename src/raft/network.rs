//! openraft over our transport: Raft RPCs travel as `PeerBody::Raft` frames on the peer port.
//!
//! openraft asks for a new client for every heartbeat round and election, so a [`PeerLink`] is
//! only a handle onto the long-lived, shared [`PeerClient`] connection to that peer.
//!
//! Owner: A (Raft and replication).

use std::sync::Arc;
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

/// Creates [`PeerLink`]s for openraft.
#[derive(Debug, Clone)]
pub struct Network {
    me: NodeId,
    peers: Arc<PeerSet>,
    /// How long openraft waits before retrying an unreachable peer.
    unreachable_backoff: Duration,
}

impl Network {
    pub fn new(me: NodeId, peers: Arc<PeerSet>, unreachable_backoff: Duration) -> Self {
        Self {
            me,
            peers,
            unreachable_backoff,
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
        match self
            .call(term, RaftRpc::AppendEntries(rpc), option.hard_ttl())
            .await?
        {
            PeerReply::AppendEntries(r) => {
                r.map_err(|e| RPCError::RemoteError(RemoteError::new(self.target, e)))
            }
            other => Err(unexpected("AppendEntries", &other)),
        }
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

/// Hand an incoming Raft RPC to openraft and wrap its answer for the wire.
pub async fn serve(raft: &Raft, rpc: RaftRpc) -> PeerReply {
    match rpc {
        RaftRpc::AppendEntries(r) => PeerReply::AppendEntries(raft.append_entries(r).await),
        RaftRpc::Vote(r) => PeerReply::Vote(raft.vote(r).await),
        RaftRpc::InstallSnapshot(r) => PeerReply::InstallSnapshot(raft.install_snapshot(r).await),
    }
}
