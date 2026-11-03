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

/// One report is one incident, so an incident goes by the ID of its request (D140). Retries reuse
/// it and it keys the results table; a second ID could only disagree with this one.
pub type IncidentId = RequestId;

/// A simulated campus user. Needs no coordination: client IDs are unique across machines (D113),
/// and each client numbers its own users.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId {
    pub client: ClientId,
    pub user: u32,
}

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.client, self.user)
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
    Technical,
    Rescue,
}

impl Capability {
    pub const ALL: [Capability; 4] = [
        Capability::Medical,
        Capability::Fire,
        Capability::Technical,
        Capability::Rescue,
    ];

    /// The categories this capability answers: [`Category::needs`] read the other way.
    pub fn categories(self) -> &'static [Category] {
        match self {
            Capability::Medical => &[Category::MedicalAssistance],
            Capability::Fire => &[Category::LabIncident],
            Capability::Technical => &[
                Category::PowerOutage,
                Category::NetworkOutage,
                Category::TechnicalAssistance,
            ],
            Capability::Rescue => &[Category::BlockedRoad, Category::Flooding],
        }
    }
}

/// What happened, in the spec's own examples. [`Capability`] says who can help; this says what the
/// report is about (D141).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Category {
    BlockedRoad,
    PowerOutage,
    LabIncident,
    Flooding,
    NetworkOutage,
    MedicalAssistance,
    TechnicalAssistance,
}

impl Category {
    pub const ALL: [Category; 7] = [
        Category::BlockedRoad,
        Category::PowerOutage,
        Category::LabIncident,
        Category::Flooding,
        Category::NetworkOutage,
        Category::MedicalAssistance,
        Category::TechnicalAssistance,
    ];

    /// The capability a report of this category needs. The spec names the categories but not who
    /// answers them, so this table is our own reading (D141).
    pub fn needs(self) -> Capability {
        match self {
            Category::BlockedRoad | Category::Flooding => Capability::Rescue,
            Category::PowerOutage | Category::NetworkOutage | Category::TechnicalAssistance => {
                Capability::Technical
            }
            Category::LabIncident => Capability::Fire,
            Category::MedicalAssistance => Capability::Medical,
        }
    }
}

/// An incident reported by a client. Its ID is the request's ([`IncidentId`]). Its status is worked
/// out on read, not stored: see [`IncidentStatus`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Incident {
    /// Who reported it.
    pub owner: UserId,
    /// What happened. Only people read this; the planner reads `needs`.
    pub category: Category,
    pub zone: ZoneId,
    pub location: Point,
    /// 1 (minor) to 5 (life-threatening).
    pub severity: u8,
    pub needs: Capability,
    /// When the client reported it: unix milliseconds on the client clock.
    pub reported_at_ms: u64,
    /// How long the report stays valid, counted from when it was reported (D143). 0 means it is
    /// already too old, not that it never expires.
    pub ttl_ms: u64,
}

impl Incident {
    /// Whether a report of this age may no longer be planned.
    pub fn is_too_old(&self, age_ms: u64) -> bool {
        age_ms >= self.ttl_ms
    }
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
    /// How urgent the incident is, in priority points, higher is more urgent (D148). The sum is
    /// defined in planner.rs. 0 means nobody looked: the report was too old to plan.
    pub priority: u32,
}

impl Plan {
    /// Whether two plans for one incident send the same responder. Priority is left out because it
    /// depends on the moment of planning: two plans can agree on who goes and differ in it.
    pub fn same_assignment(&self, other: &Plan) -> bool {
        self.responder == other.responder && self.cost == other.cost
    }
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
    /// The report's age as the client stated it in the send this plan answers (D143).
    pub age_ms: u64,
    /// When the executor made the plan: unix milliseconds on its clock (D144). In the proposal
    /// because the state machine has no clock; every node keeps this same value.
    pub assigned_at_ms: u64,
}

/// A result that is part of the replicated state.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CommittedResult {
    pub request: RequestId,
    pub incident: Incident,
    pub executor: NodeId,
    pub term: Term,
    pub plan: Plan,
    /// The report's age as the client stated it in the send this plan answers (D143).
    pub age_ms: u64,
    /// When the executor made the plan: unix milliseconds on its clock (D144).
    pub assigned_at_ms: u64,
    /// Raft log index the result was committed at.
    pub log_index: u64,
}

impl CommittedResult {
    /// The report was already too old when a node took it, so nobody was planned (D143). A fact in
    /// the log, the same on every node whatever its clock says.
    pub fn too_old_on_arrival(&self) -> bool {
        self.incident.is_too_old(self.age_ms)
    }

    /// When a record that sent nobody reads Expired: its TTL, counted from when it was reported on
    /// the executor's clock. Never `reported_at_ms + ttl_ms`, which would let one client PC with a
    /// wrong clock change the labels on every node.
    pub fn deadline_ms(&self) -> u64 {
        self.assigned_at_ms
            .saturating_add(self.incident.ttl_ms.saturating_sub(self.age_ms))
    }

    /// Where this record stands, given whether its responder is still held and the reader's clock
    /// (D142). A responder still held past the deadline really is still out there, so only a record
    /// that sent nobody can expire.
    ///
    /// The step to Expired past the deadline is a rule about reading, not a change in the log: no
    /// entry marks it, and a clock stepped back turns the label back. It cannot change a plan, a
    /// hold or the digest.
    pub fn status(&self, held: bool, now_ms: u64) -> IncidentStatus {
        match self.plan.responder {
            Some(_) if held => IncidentStatus::Assigned,
            Some(_) => IncidentStatus::Resolved,
            None if self.too_old_on_arrival() || now_ms >= self.deadline_ms() => {
                IncidentStatus::Expired
            }
            None => IncidentStatus::Unassigned,
        }
    }

    /// Where this record stood when it committed. A client works this out from the reply alone: no
    /// clock and no state, so a retry that arrives after the release still reads Assigned, and
    /// reading the latest status is a separate question (D142).
    pub fn status_at_commit(&self) -> IncidentStatus {
        match self.plan.responder {
            Some(_) => IncidentStatus::Assigned,
            None if self.too_old_on_arrival() => IncidentStatus::Expired,
            None => IncidentStatus::Unassigned,
        }
    }
}

/// Where an incident stands. Never stored, and in no [`Command`] or [`Outcome`]: every value comes
/// from facts the log already holds, so `apply` does not change and reading a status cannot move the
/// digest (D142).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IncidentStatus {
    /// A lone node's answer, not committed yet (D120). Only the client that got it sees this.
    Provisional,
    /// Committed with nobody sent: no suitable responder was free.
    Unassigned,
    /// A responder was sent and is still held.
    Assigned,
    /// The responder's release committed: its time on scene is over (D111).
    Resolved,
    /// Nobody was sent, and the report was too old on arrival or its deadline has passed.
    Expired,
}

impl IncidentStatus {
    /// Short name for logs and `ctl`. Field values in the event log, so keep them stable.
    pub fn as_str(self) -> &'static str {
        match self {
            IncidentStatus::Provisional => "provisional",
            IncidentStatus::Unassigned => "unassigned",
            IncidentStatus::Assigned => "assigned",
            IncidentStatus::Resolved => "resolved",
            IncidentStatus::Expired => "expired",
        }
    }
}

impl fmt::Display for IncidentStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
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
    /// How old the report is at this send, on the client's own clock. A node judges "too old" from
    /// this, so that decision never compares two machines' clocks (D143).
    pub age_ms: u64,
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
    /// The report was too old to plan, so nobody was sent (D143). Committed like any result, so a
    /// retry gets this same record. This is an answer: a client must stop waiting when it gets one.
    Expired(CommittedResult),
    /// A lone node's plan, not committed: no quorum exists to commit it (D120, D123). The node
    /// commits it once a peer returns, and the first commit wins, so it may turn out to differ.
    Provisional(Plan),
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
    /// True while this node answers without a commit, with every peer gone (D121).
    pub degraded: bool,
}

// ---------------------------------------------------------------------------------------------
// Node <-> node (peer port)
// ---------------------------------------------------------------------------------------------

/// The remote interface one node offers another (D119), through tarpc. Every method carries the
/// sender and its Raft term, as [`PeerEnvelope`] does.
#[tarpc::service]
pub trait PeerRpc {
    async fn append_entries(
        from: NodeId,
        term: Term,
        rpc: AppendEntriesRequest<TypeConfig>,
    ) -> PeerReply;
    async fn vote(from: NodeId, term: Term, rpc: VoteRequest<NodeId>) -> PeerReply;
    async fn install_snapshot(
        from: NodeId,
        term: Term,
        rpc: InstallSnapshotRequest<TypeConfig>,
    ) -> PeerReply;
    async fn propose(from: NodeId, term: Term, command: Command) -> PeerReply;
    async fn read_index(from: NodeId, term: Term) -> PeerReply;
    async fn fault_order(from: NodeId, term: Term, ms: u64) -> PeerReply;
    async fn bid(from: NodeId, term: Term, bid: Bid) -> PeerReply;
    async fn heartbeat(from: NodeId, term: Term) -> PeerReply;
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
    /// A returning node asks the leader for its commit index, confirmed by a majority (Raft
    /// ReadIndex, D76). Answered with [`PeerReply::ReadIndex`].
    ReadIndex,
    /// The leader orders the receiver silent for `ms` (scheduled fault injection, D12, D78). The
    /// envelope's term makes a stale leader's order fail the term rule. Answered with
    /// [`PeerReply::FaultOrder`].
    FaultOrder {
        ms: u64,
    },
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
    /// The leader's commit index (`None`: nothing committed yet), or `NotLeader` with a hint.
    ReadIndex(Result<Option<u64>, ProposeError>),
    /// The victim obeyed, or why it refused (disabled, already silenced, recovering).
    FaultOrder(Result<(), String>),
}

/// One node's bid to execute a request. Lowest score wins; ties go to the lower node ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bid {
    pub request: RequestId,
    /// The client's attempt number; each attempt has its own bid round.
    pub attempt: u32,
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
            owner: UserId { client: 7, user: 3 },
            category: Category::MedicalAssistance,
            zone: 2,
            location: Point { x: 10, y: -4 },
            severity: 4,
            needs: Capability::Medical,
            reported_at_ms: 1_700_000_000_000,
            ttl_ms: 60_000,
        }
    }

    #[test]
    fn every_category_maps_to_its_capability() {
        let expected = [
            (Category::BlockedRoad, Capability::Rescue),
            (Category::Flooding, Capability::Rescue),
            (Category::PowerOutage, Capability::Technical),
            (Category::NetworkOutage, Capability::Technical),
            (Category::TechnicalAssistance, Capability::Technical),
            (Category::LabIncident, Capability::Fire),
            (Category::MedicalAssistance, Capability::Medical),
        ];
        assert_eq!(expected.len(), Category::ALL.len());
        for (category, capability) in expected {
            assert_eq!(category.needs(), capability, "{category:?}");
        }
    }

    #[test]
    fn the_two_category_tables_agree() {
        for category in Category::ALL {
            assert!(category.needs().categories().contains(&category));
        }
        let listed: usize = Capability::ALL.iter().map(|c| c.categories().len()).sum();
        assert_eq!(listed, Category::ALL.len());
    }

    #[test]
    fn a_report_is_too_old_at_its_ttl_and_not_before() {
        assert!(!incident().is_too_old(59_999));
        assert!(incident().is_too_old(60_000));
    }

    #[test]
    fn a_zero_ttl_is_too_old_at_once() {
        let inc = Incident {
            ttl_ms: 0,
            ..incident()
        };
        assert!(inc.is_too_old(0));
    }

    #[test]
    fn client_frames_round_trip() {
        let frame = ClientFrame::Request(ClientRequest {
            id: RequestId { client: 7, seq: 42 },
            attempt: 1,
            age_ms: 2_000,
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
                priority: 460,
            },
            age_ms: 2_000,
            assigned_at_ms: 1_700_000_002_000,
        });
        let back: Command = decode(&encode(&cmd).unwrap()).unwrap();
        assert_eq!(back, cmd);
    }

    /// A record with a 60 s TTL.
    fn record(responder: Option<ResponderId>, age_ms: u64, assigned_at_ms: u64) -> CommittedResult {
        CommittedResult {
            request: RequestId { client: 1, seq: 2 },
            incident: incident(),
            executor: 3,
            term: 4,
            plan: Plan {
                responder,
                cost: 0,
                priority: 0,
            },
            age_ms,
            assigned_at_ms,
            log_index: 5,
        }
    }

    #[test]
    fn plans_that_differ_only_in_priority_send_the_same_responder() {
        let plan = |responder, cost, priority| Plan {
            responder,
            cost,
            priority,
        };
        let first = plan(Some(9), 120, 300);
        assert!(first.same_assignment(&plan(Some(9), 120, 460)));
        assert!(!first.same_assignment(&plan(Some(8), 120, 300)));
        assert!(!first.same_assignment(&plan(None, 120, 300)));
        assert!(!first.same_assignment(&plan(Some(9), 121, 300)));
    }

    #[test]
    fn status_is_assigned_while_held_and_resolved_after() {
        let r = record(Some(9), 0, 1_000);
        assert_eq!(r.status(true, 1_000), IncidentStatus::Assigned);
        // Held past the report's deadline: the responder really is still out there.
        assert_eq!(r.status(true, 1_000_000), IncidentStatus::Assigned);
        assert_eq!(r.status(false, 1_000), IncidentStatus::Resolved);
        assert_eq!(r.status_at_commit(), IncidentStatus::Assigned);
    }

    #[test]
    fn nobody_sent_reads_unassigned_then_expired_at_the_deadline() {
        let r = record(None, 0, 1_000);
        assert_eq!(r.status_at_commit(), IncidentStatus::Unassigned);
        assert_eq!(r.status(false, 60_999), IncidentStatus::Unassigned);
        assert_eq!(r.status(false, 61_000), IncidentStatus::Expired);
        // A reader's clock stepped back turns the label back: a read rule, not a state change.
        assert_eq!(r.status(false, 60_999), IncidentStatus::Unassigned);
    }

    #[test]
    fn too_old_on_arrival_reads_expired_at_any_clock() {
        let r = record(None, 60_000, 1_000);
        assert!(r.too_old_on_arrival());
        assert_eq!(r.status_at_commit(), IncidentStatus::Expired);
        for now_ms in [0, 1_000, u64::MAX] {
            assert_eq!(r.status(false, now_ms), IncidentStatus::Expired);
        }
    }

    #[test]
    fn the_deadline_counts_from_the_report_and_saturates() {
        assert_eq!(record(None, 5_000, 1_000).deadline_ms(), 56_000);
        assert_eq!(record(None, 90_000, 1_000).deadline_ms(), 1_000);
        assert_eq!(record(None, 0, u64::MAX).deadline_ms(), u64::MAX);
    }

    #[test]
    fn expired_replies_round_trip() {
        let frame = ServerFrame::Reply(ClientReply {
            id: RequestId { client: 1, seq: 2 },
            from: 3,
            body: ReplyBody::Expired(record(None, 60_000, 1_000)),
        });
        let back: ServerFrame = decode(&encode(&frame).unwrap()).unwrap();
        assert_eq!(back, frame);
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
