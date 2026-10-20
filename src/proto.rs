//! The wire contract: every message, ID and Raft log entry in RescueMesh.
//!
//! Shared by every role. Change it only with a design-log entry (docs/design-log.md) and a
//! review from every affected owner, and keep docs/protocol.md in sync.

use std::fmt;

use bincode::Options;
use openraft::error::{InstallSnapshotError, RaftError};
use openraft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse,
    VoteRequest, VoteResponse,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::raft::TypeConfig;

// ---------------------------------------------------------------------------------------------
// IDs
// ---------------------------------------------------------------------------------------------

pub type NodeId = u64;
pub type Term = u64;
pub type ClientId = u64;
pub type ResponderId = u32;
pub type ZoneId = u16;

/// Globally unique request ID: the client ID plus a per-client sequence number.
/// Retries reuse the same ID, which is what makes them idempotent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RequestId {
    pub client: ClientId,
    pub seq: u64,
}

impl fmt::Display for RequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.client, self.seq)
    }
}

// ---------------------------------------------------------------------------------------------
// Domain
// ---------------------------------------------------------------------------------------------

/// A point on the synthetic campus, in metres. Integers keep the replicated state deterministic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub fn manhattan(self, other: Point) -> u64 {
        u64::from(self.x.abs_diff(other.x)) + u64::from(self.y.abs_diff(other.y))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Capability {
    Medical,
    Fire,
    Security,
    Rescue,
}

impl Capability {
    pub const ALL: [Capability; 4] = [
        Capability::Medical,
        Capability::Fire,
        Capability::Security,
        Capability::Rescue,
    ];
}

/// An incident reported by a client.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Incident {
    pub zone: ZoneId,
    pub location: Point,
    /// 1 (minor) to 5 (life-threatening).
    pub severity: u8,
    pub needs: Capability,
    /// When the client reported it: unix milliseconds on the client clock.
    pub reported_at_ms: u64,
}

/// A responder that plans can assign to incidents.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Responder {
    pub id: ResponderId,
    pub zone: ZoneId,
    pub location: Point,
    pub capability: Capability,
}

/// The planner's answer for one incident.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Plan {
    /// Responder sent to the incident; `None` means no suitable responder was free.
    pub responder: Option<ResponderId>,
    /// Planner cost of this plan, lower is better. The unit is defined in planner.rs.
    pub cost: u64,
}

// ---------------------------------------------------------------------------------------------
// Raft log
// ---------------------------------------------------------------------------------------------

/// Payload of a Raft log entry.
///
/// Every variant must be idempotent: applying the same command twice must leave the state as
/// if it was applied once, because a proposal can be retried after a leader change.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Command {
    /// Add responders to the shared registry. Responders already known are left untouched.
    RegisterResponders { responders: Vec<Responder> },
    /// Record the result of planning one request. The first commit per request ID wins.
    CommitResult(ResultProposal),
    /// Free a responder held by `request`. A no-op if `request` does not hold it.
    ReleaseResponder {
        responder: ResponderId,
        request: RequestId,
    },
}

impl Command {
    /// Short name for logs and metrics.
    pub fn kind(&self) -> &'static str {
        match self {
            Command::RegisterResponders { .. } => "register_responders",
            Command::CommitResult(_) => "commit_result",
            Command::ReleaseResponder { .. } => "release_responder",
        }
    }

    pub fn request(&self) -> Option<RequestId> {
        match self {
            Command::CommitResult(p) => Some(p.request),
            Command::ReleaseResponder { request, .. } => Some(*request),
            Command::RegisterResponders { .. } => None,
        }
    }
}

/// A result an executor wants to commit for a request.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResultProposal {
    pub request: RequestId,
    pub incident: Incident,
    /// Node that planned the request.
    pub executor: NodeId,
    /// Raft term the executor saw when it proposed.
    pub term: Term,
    pub plan: Plan,
}

/// A result that is part of the replicated state.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CommittedResult {
    pub request: RequestId,
    pub incident: Incident,
    pub executor: NodeId,
    pub term: Term,
    pub plan: Plan,
    /// Raft log index the result was committed at.
    pub log_index: u64,
}

/// What applying a [`Command`] produced. This is the Raft response type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Outcome {
    /// This proposal was the first for its request and is now the result.
    Committed(CommittedResult),
    /// Another result was already committed for this request; that winner is returned and
    /// this proposal was discarded.
    Duplicate(CommittedResult),
    /// The plan's responder could not be taken. Nothing was recorded; re-plan and propose again.
    Stale {
        request: RequestId,
        responder: ResponderId,
        reason: StaleReason,
    },
    /// Applied with nothing to report (registration, release, internal Raft entries).
    Applied,
}

impl Outcome {
    /// Short name for logs and metrics.
    pub fn kind(&self) -> &'static str {
        match self {
            Outcome::Committed(_) => "committed",
            Outcome::Duplicate(_) => "duplicate",
            Outcome::Stale { .. } => "stale",
            Outcome::Applied => "applied",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StaleReason {
    /// The responder is held by another request.
    HeldBy(RequestId),
    /// The responder is not in the registry.
    UnknownResponder,
}

/// Why a proposal did not commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum ProposeError {
    #[error("not the leader (leader hint: {leader:?})")]
    NotLeader { leader: Option<NodeId> },
    #[error("no leader committed the proposal before the deadline")]
    Timeout,
    #[error("consensus stopped: {0}")]
    Fatal(String),
}

// ---------------------------------------------------------------------------------------------
// Client <-> node (client port)
// ---------------------------------------------------------------------------------------------

/// Sent by a client to every node. The client never names a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientRequest {
    pub id: RequestId,
    /// 0 for the first send, then 1, 2, ... for retries with the same ID.
    pub attempt: u32,
    pub incident: Incident,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientReply {
    pub id: RequestId,
    /// Node that sent this reply.
    pub from: NodeId,
    pub body: ReplyBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplyBody {
    /// The committed result for the request. Every reply for one request carries the same result.
    Done(CommittedResult),
    /// The node will not handle the request; the client should keep waiting or retry.
    Rejected(String),
}

/// Frames a client (or ctl) sends to a node's client port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientFrame {
    Request(ClientRequest),
    /// Operator commands from `ctl`. These bypass the fault gate.
    Control(ControlRequest),
}

/// Frames a node sends back on the client port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerFrame {
    Reply(ClientReply),
    Control(ControlReply),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlRequest {
    Status,
    /// Drop all traffic in and out of the node for `ms` milliseconds.
    Silence {
        ms: u64,
    },
    /// Extra planner work per request on this node, to create load imbalance.
    SkewLoad {
        extra_work: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlReply {
    Status(NodeStatus),
    Done,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeStatus {
    pub id: NodeId,
    pub term: Term,
    pub leader: Option<NodeId>,
    pub role: String,
    pub last_log_index: Option<u64>,
    pub last_applied: Option<u64>,
    /// Digest of the replicated state; equal on nodes that applied the same log prefix.
    pub digest: u64,
    pub results: u64,
    pub responders: u64,
    pub silenced: bool,
    /// False while a returning node is catching up.
    pub serving: bool,
}

// ---------------------------------------------------------------------------------------------
// Node <-> node (peer port)
// ---------------------------------------------------------------------------------------------

/// A request frame on a peer connection. Replies carry the same `seq`.
#[derive(Debug, Serialize, Deserialize)]
pub struct PeerFrame {
    pub seq: u64,
    pub envelope: PeerEnvelope,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PeerReplyFrame {
    pub seq: u64,
    pub reply: PeerReply,
}

/// Every peer message carries the sender and its Raft term.
///
/// Receivers drop non-Raft messages whose term is older than their own and answer
/// [`PeerReply::StaleTerm`]. Raft RPCs are never dropped this way: openraft must see them to
/// reject a stale leader correctly.
#[derive(Debug, Serialize, Deserialize)]
pub struct PeerEnvelope {
    pub from: NodeId,
    pub term: Term,
    pub body: PeerBody,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum PeerBody {
    Raft(RaftRpc),
    /// A follower asks the leader to commit a command on its behalf.
    Propose(Command),
    Bid(Bid),
    Heartbeat,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum RaftRpc {
    AppendEntries(AppendEntriesRequest<TypeConfig>),
    Vote(VoteRequest<NodeId>),
    InstallSnapshot(InstallSnapshotRequest<TypeConfig>),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum PeerReply {
    Ack,
    /// The message was dropped because its term is older than the receiver's.
    StaleTerm {
        current: Term,
    },
    AppendEntries(Result<AppendEntriesResponse<NodeId>, RaftError<NodeId>>),
    Vote(Result<VoteResponse<NodeId>, RaftError<NodeId>>),
    InstallSnapshot(
        Result<InstallSnapshotResponse<NodeId>, RaftError<NodeId, InstallSnapshotError>>,
    ),
    Proposed(Result<Outcome, ProposeError>),
}

/// One node's bid to execute a request. Lowest score wins; ties go to the lower node ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bid {
    pub request: RequestId,
    /// Load score in bidding.rs units (lower is less loaded).
    pub score: u64,
}

// ---------------------------------------------------------------------------------------------
// Wire encoding
// ---------------------------------------------------------------------------------------------

/// Largest frame accepted on any connection.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

pub type WireError = bincode::Error;

fn wire() -> impl Options {
    bincode::DefaultOptions::new().with_limit(MAX_FRAME_BYTES as u64)
}

/// Encode a frame body. Every frame on every connection uses this encoding.
pub fn encode<T: Serialize>(msg: &T) -> Result<Vec<u8>, WireError> {
    wire().serialize(msg)
}

pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, WireError> {
    wire().deserialize(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn incident() -> Incident {
        Incident {
            zone: 2,
            location: Point { x: 10, y: -4 },
            severity: 4,
            needs: Capability::Medical,
            reported_at_ms: 1_700_000_000_000,
        }
    }

    #[test]
    fn client_frames_round_trip() {
        let frame = ClientFrame::Request(ClientRequest {
            id: RequestId { client: 7, seq: 42 },
            attempt: 1,
            incident: incident(),
        });
        let back: ClientFrame = decode(&encode(&frame).unwrap()).unwrap();
        assert_eq!(back, frame);
    }

    #[test]
    fn commands_round_trip() {
        let cmd = Command::CommitResult(ResultProposal {
            request: RequestId { client: 1, seq: 2 },
            incident: incident(),
            executor: 3,
            term: 4,
            plan: Plan {
                responder: Some(9),
                cost: 120,
            },
        });
        let back: Command = decode(&encode(&cmd).unwrap()).unwrap();
        assert_eq!(back, cmd);
    }

    #[test]
    fn hostile_length_prefix_is_rejected() {
        // Variant 0 (RegisterResponders) followed by a varint length of u64::MAX: decoding must
        // fail on the missing elements instead of allocating.
        let mut bytes = vec![0x00, 0xfd];
        bytes.extend_from_slice(&[0xff; 8]);
        assert!(decode::<Command>(&bytes).is_err());
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        let mut bytes = encode(&Command::RegisterResponders { responders: vec![] }).unwrap();
        bytes.push(0);
        assert!(decode::<Command>(&bytes).is_err());
    }

    #[test]
    fn manhattan_distance() {
        assert_eq!(Point { x: -3, y: 4 }.manhattan(Point { x: 2, y: -1 }), 10);
    }
}
