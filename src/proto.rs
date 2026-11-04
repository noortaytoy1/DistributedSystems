//! The wire contract: every message, ID and Raft log entry in RescueMesh.
//!
//! Shared by every role. Change it only with a design-log entry (docs/design-log.md) and a
//! review from every affected owner, and keep docs/protocol.md in sync.

use std::fmt;
use std::str::FromStr;

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

impl FromStr for RequestId {
    type Err = String;

    /// Reads what `Display` writes, so `ctl` can take an ID copied from its own output.
    fn from_str(text: &str) -> Result<Self, String> {
        let number = |part: &str| {
            part.parse()
                .map_err(|e| format!("bad request ID {text:?}: {e}"))
        };
        let (client, seq) = text
            .split_once('-')
            .ok_or_else(|| format!("bad request ID {text:?}: expected <client>-<seq>"))?;
        Ok(RequestId {
            client: number(client)?,
            seq: number(seq)?,
        })
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

/// Payload of a Raft log entry: the commands that waited together share one entry, so one disk
/// save and one replication round cover all of them (D133). Applied in order at the entry's log
/// index. A group holds [`Command`]s and is not one itself, so groups cannot nest.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CommandGroup(pub Vec<Command>);

/// One thing the replicated state can be told to do.
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
    /// Raft log index the result was committed at. Results of one [`CommandGroup`] share it.
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

    /// When the responder is planned to be free again: the time on scene counted from the plan
    /// (D111, D144). `None` if nobody was sent. A plan, not a promise: a backup releases at twice
    /// the service time, a failed release restarts the wait, and a replayed plan's hold starts when
    /// the replay commits. Only the release committing makes a record Resolved.
    pub fn planned_end_ms(&self, ms_per_severity: u64) -> Option<u64> {
        self.plan.responder?;
        let service_ms = u64::from(self.incident.severity).saturating_mul(ms_per_severity);
        Some(self.assigned_at_ms.saturating_add(service_ms))
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

/// One committed incident as an operator or a page reads it: every field the spec lists for an
/// incident, with its status worked out at the moment of reading (D142, D145).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncidentView {
    pub id: IncidentId,
    pub incident: Incident,
    pub status: IncidentStatus,
    pub responder: Option<ResponderId>,
    /// Priority points (D148).
    pub priority: u32,
    pub executor: NodeId,
    /// The report's age when it was planned. Measured on the client's clock, unlike
    /// `assigned_at_ms`: never subtract `reported_at_ms` from `assigned_at_ms` (D144).
    pub age_ms: u64,
    pub assigned_at_ms: u64,
    /// See [`CommittedResult::planned_end_ms`].
    pub expires_at_ms: Option<u64>,
    pub log_index: u64,
}

/// Which incident records to read (D145).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IncidentQuery {
    One(IncidentId),
    /// This client's newest records first, at most `limit` of them and never more than
    /// [`MAX_INCIDENT_ROWS`].
    OfClient {
        client: ClientId,
        limit: u32,
    },
}

/// Rows `ctl incidents --client` asks for unless told otherwise.
pub const DEFAULT_INCIDENT_ROWS: u32 = 10;
/// Most rows one reply carries: building them holds up applying the log (`StateReader::with`).
pub const MAX_INCIDENT_ROWS: u32 = 20;

/// What applying a [`Command`] produced. Raft answers a group with one of these per command, in
/// the group's order.
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

/// The remote interface a node offers clients and `ctl` (D167), through tarpc. No term argument:
/// the term rule is for peer messages. A node serves it twice on `client_addr`: over UDP for clients
/// (`node/datagram.rs`), and over TCP for `ctl`, so a lost silence order cannot vanish (D168).
///
/// `submit` is called on every node. A node that cannot answer never returns, so the caller's call
/// stays open until it cancels or its deadline passes: a node says nothing to mean "not me" (D36).
///
/// The fault gate drops `submit`. All the other methods are control and pass the gate (D23), so a
/// method added here must decide which it is.
#[tarpc::service]
pub trait ClientRpc {
    async fn submit(request: ClientRequest) -> ClientReply;
    async fn status() -> ControlReply;
    /// Drop all traffic in and out of the node for `ms` milliseconds.
    async fn silence(ms: u64) -> ControlReply;
    /// Extra planner work per request on this node, to create load imbalance.
    async fn skew_load(extra_work: u32) -> ControlReply;
    /// Read committed incident records from this node's copy of the state (D145).
    async fn incidents(query: IncidentQuery) -> ControlReply;
}

/// What `ctl` can ask for, one for each control method of [`ClientRpc`].
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
    /// Read committed incident records from this node's copy of the state (D145).
    Incidents(IncidentQuery),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlReply {
    Status(NodeStatus),
    Done,
    Error(String),
    /// Control calls pass the fault gate, so a silenced or returning node still answers. `node` says
    /// so: a reader must not take such a node's rows as current.
    Incidents {
        node: NodeStatus,
        rows: Vec<IncidentView>,
    },
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
    /// Who this node lists in the group: reachable, not able to serve, and this node's opinion
    /// only (D155). Frozen while the node is silenced.
    pub members: Vec<NodeId>,
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
    async fn propose_many(from: NodeId, term: Term, commands: Vec<Command>) -> PeerReply;
    async fn read_index(from: NodeId, term: Term) -> PeerReply;
    async fn fault_bid(from: NodeId, term: Term, bid: FaultBid) -> PeerReply;
    async fn bid(from: NodeId, term: Term, bid: Bid) -> PeerReply;
    async fn bids(from: NodeId, term: Term, bids: Vec<Bid>) -> PeerReply;
    async fn rounds(from: NodeId, term: Term, message: RoundsMsg) -> PeerReply;
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
    /// The commands a follower gathered in the last few milliseconds, as one message (D248).
    ProposeMany(Vec<Command>),
    /// A returning node asks the leader for its commit index, confirmed by a majority (Raft
    /// ReadIndex, D76). Answered with [`PeerReply::ReadIndex`].
    ReadIndex,
    /// This node's numbers for a fault election, sent to the group (D158). Answered with `Ack`.
    /// The envelope's term applies the term rule: a dropped bid only stops the round finishing.
    FaultBid(FaultBid),
    Bid(Bid),
    /// The bids this node made in the last few milliseconds, as one message (D247).
    Bids(Vec<Bid>),
    /// Rotating rounds, the leaderless way to one order (D249).
    Rounds(RoundsMsg),
    Heartbeat,
}

/// One node's commands for one round (D249).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundBatch {
    pub node: NodeId,
    pub round: u64,
    pub commands: Vec<Command>,
    pub control: Option<RoundControl>,
}

/// A change to who takes part, in the one order, so every member makes it at the same place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoundControl {
    Add {
        node: NodeId,
        start_round: u64,
        /// The view of the member that made this entry: the node is not in it (D257).
        view: u64,
    },
}

/// Messages between nodes under rotating rounds (D249).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RoundsMsg {
    /// The sender's stream from just after round `after` through round `through`.
    Stream {
        after: u64,
        through: u64,
        batches: Vec<RoundBatch>,
    },
    /// Go on without `target` as view `view`. The sender holds its stream through `have`.
    Drop {
        view: u64,
        target: NodeId,
        have: u64,
    },
    /// The other half of a drop: its stream ends at `cut`, and these are what the receiver lacks.
    DropCommit {
        view: u64,
        target: NodeId,
        cut: u64,
        batches: Vec<RoundBatch>,
    },
    /// A returning node asks for the applied log, from entry number `next`.
    CatchUp { next: u64 },
    /// A returning node that has caught up asks to be added.
    Join,
    /// How far has this node applied?
    Applied,
}

/// Answers to [`RoundsMsg`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RoundsReply {
    /// This node has saved the sender's stream through round `have`.
    Have {
        have: u64,
    },
    /// The sender is not in this node's view of who takes part, which is `view`.
    NotMember {
        view: u64,
    },
    /// This node is not ready to answer yet.
    NotYet,
    Granted {
        view: u64,
        have: u64,
        batches: Vec<RoundBatch>,
    },
    Refused {
        view: u64,
    },
    Done,
    Entries {
        batches: Vec<RoundBatch>,
        total: u64,
        applied_round: u64,
        view: u64,
        members: Vec<NodeId>,
        /// Members added and not heard from yet, and the round each comes back at (D252).
        starts: Vec<(NodeId, u64)>,
    },
    Applied {
        index: u64,
    },
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
    /// One answer per command of a [`PeerBody::ProposeMany`], in order.
    ProposedMany(Vec<Result<Outcome, ProposeError>>),
    Rounds(RoundsReply),
    /// The leader's commit index (`None`: nothing committed yet), or `NotLeader` with a hint.
    ReadIndex(Result<Option<u64>, ProposeError>),
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

/// One node's numbers in one round of the fault election (D158). The highest `ticket` goes silent,
/// for the `ms` in its own bid; ties go to the lower node ID. A separate election from [`Bid`]: it
/// picks a machine to fail, and it needs a bid from every node before anyone acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FaultBid {
    /// Counts rounds on this node and is lifted by any higher round it hears of. Not saved, and
    /// nothing outside the fault election reads it (D159).
    pub round: u64,
    /// A random number, in `0..1_000_000`.
    pub ticket: u64,
    /// How long this node goes silent if it is elected.
    pub ms: u64,
}

// ---------------------------------------------------------------------------------------------
// Wire encoding
// ---------------------------------------------------------------------------------------------

/// Largest frame accepted on any TCP connection.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// Largest message sent in one UDP datagram (D170): under the 1472 bytes of payload an Ethernet frame
/// carries, so a datagram is never split on the wire, where every fragment would have to arrive.
pub const MAX_DATAGRAM_BYTES: usize = 1_400;

pub type WireError = bincode::Error;

fn wire() -> impl Options {
    bincode::DefaultOptions::new().with_limit(MAX_FRAME_BYTES as u64)
}

/// Encode a message. UDP datagrams and the Raft log file use this; on TCP, tarpc does its own.
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
    fn client_requests_round_trip() {
        let request = ClientRequest {
            id: RequestId { client: 7, seq: 42 },
            attempt: 1,
            age_ms: 2_000,
            incident: incident(),
        };
        let back: ClientRequest = decode(&encode(&request).unwrap()).unwrap();
        assert_eq!(back, request);
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
    fn the_planned_end_is_the_time_on_scene_after_the_plan() {
        // Severity 4 at 200 ms per point.
        assert_eq!(record(Some(9), 0, 1_000).planned_end_ms(200), Some(1_800));
        assert_eq!(record(None, 0, 1_000).planned_end_ms(200), None);
        assert_eq!(
            record(Some(9), 0, u64::MAX).planned_end_ms(200),
            Some(u64::MAX)
        );
    }

    #[test]
    fn expired_replies_round_trip() {
        let reply = ClientReply {
            id: RequestId { client: 1, seq: 2 },
            from: 3,
            body: ReplyBody::Expired(record(None, 60_000, 1_000)),
        };
        let back: ClientReply = decode(&encode(&reply).unwrap()).unwrap();
        assert_eq!(back, reply);
    }

    #[test]
    fn incident_queries_and_rows_round_trip() {
        let request = ControlRequest::Incidents(IncidentQuery::OfClient {
            client: 3,
            limit: 10,
        });
        let back: ControlRequest = decode(&encode(&request).unwrap()).unwrap();
        assert_eq!(back, request);

        let reply = ControlReply::Incidents {
            node: NodeStatus {
                id: 1,
                term: 2,
                leader: Some(1),
                role: "leader".into(),
                last_log_index: Some(9),
                last_applied: Some(9),
                digest: 7,
                results: 1,
                responders: 30,
                silenced: false,
                serving: true,
                degraded: false,
                members: vec![1, 2, 3],
            },
            rows: vec![IncidentView {
                id: RequestId { client: 3, seq: 4 },
                incident: incident(),
                status: IncidentStatus::Assigned,
                responder: Some(16),
                priority: 460,
                executor: 2,
                age_ms: 0,
                assigned_at_ms: 1_000,
                expires_at_ms: Some(1_800),
                log_index: 5,
            }],
        };
        let back: ControlReply = decode(&encode(&reply).unwrap()).unwrap();
        assert_eq!(back, reply);
    }

    /// Every integer at its longest varint, with tarpc's own envelope around it (D170). A report or
    /// a reply over the limit would be dropped by the sender on every send.
    #[test]
    fn the_largest_report_and_reply_fit_one_datagram() {
        use std::time::{Duration, Instant};
        use tarpc::{ClientMessage, Request, Response, context, trace};

        let largest_id = RequestId {
            client: u64::MAX,
            seq: u64::MAX,
        };
        let largest = Incident {
            owner: UserId {
                client: u64::MAX,
                user: u32::MAX,
            },
            category: Category::TechnicalAssistance,
            zone: ZoneId::MAX,
            location: Point {
                x: i32::MIN,
                y: i32::MIN,
            },
            severity: u8::MAX,
            needs: Capability::Rescue,
            reported_at_ms: u64::MAX,
            ttl_ms: u64::MAX,
        };
        let mut ctx = context::current();
        ctx.deadline = Instant::now() + Duration::from_secs(36);
        ctx.trace_context = trace::Context {
            trace_id: trace::TraceId::from(u128::MAX),
            span_id: trace::SpanId::from(u64::MAX),
            sampling_decision: trace::SamplingDecision::Sampled,
        };
        let report: ClientMessage<ClientRpcRequest> = ClientMessage::Request(Request {
            context: ctx,
            id: u64::MAX,
            message: ClientRpcRequest::Submit {
                request: ClientRequest {
                    id: largest_id,
                    attempt: u32::MAX,
                    age_ms: u64::MAX,
                    incident: largest.clone(),
                },
            },
        });
        let cancel: ClientMessage<ClientRpcRequest> = ClientMessage::Cancel {
            trace_context: ctx.trace_context,
            request_id: u64::MAX,
        };
        let plan = Plan {
            responder: Some(u32::MAX),
            cost: u64::MAX,
            priority: u32::MAX,
        };
        let reply = |body| Response {
            request_id: u64::MAX,
            message: Ok(ClientRpcResponse::Submit(ClientReply {
                id: largest_id,
                from: u64::MAX,
                body,
            })),
        };
        let done = reply(ReplyBody::Done(CommittedResult {
            request: largest_id,
            incident: largest,
            executor: u64::MAX,
            term: u64::MAX,
            plan: plan.clone(),
            age_ms: u64::MAX,
            assigned_at_ms: u64::MAX,
            log_index: u64::MAX,
        }));
        let status = Response {
            request_id: u64::MAX,
            message: Ok(ClientRpcResponse::Status(ControlReply::Status(
                NodeStatus {
                    id: u64::MAX,
                    term: u64::MAX,
                    leader: Some(u64::MAX),
                    role: "candidate".into(),
                    last_log_index: Some(u64::MAX),
                    last_applied: Some(u64::MAX),
                    digest: u64::MAX,
                    results: u64::MAX,
                    responders: u64::MAX,
                    silenced: true,
                    serving: true,
                    degraded: true,
                    members: vec![u64::MAX; 3],
                },
            ))),
        };
        let sizes = [
            encode(&report).unwrap().len(),
            encode(&cancel).unwrap().len(),
            encode(&done).unwrap().len(),
            encode(&reply(ReplyBody::Provisional(plan))).unwrap().len(),
            encode(&status).unwrap().len(),
        ];
        for size in sizes {
            assert!(size <= MAX_DATAGRAM_BYTES, "{sizes:?}");
        }
    }

    #[test]
    fn request_ids_parse_back_from_their_display_form() {
        let id = RequestId {
            client: 3,
            seq: 1_762_180_000_000_123,
        };
        assert_eq!(id.to_string().parse(), Ok(id));
        for bad in ["", "3", "3-", "-4", "a-4", "3-b", "3-4-5"] {
            assert!(bad.parse::<RequestId>().is_err(), "{bad:?}");
        }
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
