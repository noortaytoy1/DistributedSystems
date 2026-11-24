//! Client middleware: multicast each request to every node, retry with the same request ID,
//! keep the first reply. The client never names a node.
//!
//! The application makes one `submit` call; this layer copies the request to every replica. One
//! [`MeshClient`] holds one tarpc stub per node, each on its own UDP socket, shared by all simulated
//! users through `Arc` (D53, D167). tarpc matches each reply to its call.
//!
//! - **The group send (D126, D210).** `MeshClient::group_send` is the client middleware's send to
//!   all of the cloud nodes: one source, many recipients, driven by the sender. It is private, so
//!   user code cannot call it and does not know it happens. User code calls `submit`, which it gets
//!   from `src/middleware.rs`, and `tests/layers.rs` holds that line.
//! - **One submit, one set of calls (D169).** Each attempt adds one call per node to the same set.
//!   Calls from earlier attempts stay open, so a late answer still counts. They share one deadline
//!   for the whole submit. Returning drops the set, and tarpc sends one cancel per call still open:
//!   the node stops waiting on it, and its work goes on in a task of its own.
//! - **Retries (D52).** Up to `client_max_retries` resends with the same ID and `attempt + 1`.
//!   The wait starts at `client_retry_ms` and doubles up to `client_retry_max_ms`, ±20% jitter.
//! - **Dead nodes.** Nothing to detect: UDP has no connection to look after. A machine that lost
//!   power just stops answering, and when it returns on its address it answers again.
//! - **Provisional answers (D123).** A lone node's answer counts as a reply, because no commit can
//!   come. `Submitted` keeps it apart from a committed result.
//! - **The report's age (D143).** Each send states how old the report is, measured on this machine,
//!   so a node can refuse one that is too old without comparing two machines' clocks. A refusal is
//!   an answer: the wait ends there, and `Submitted::status` says `Expired`.
//! - **Loss.** A copy lost on the way to one node costs nothing if another node answers. All three
//!   lost, or the one reply lost, costs one wait of about 2 s. `udp_counters` events count what these
//!   sockets did, and the report must explain those waits in its top cells (D172).
//! - **Offline (Phase 2, D400–D405).** `go_offline` closes a gate inside every socket, so the client
//!   hears and sends nothing. `submit` then fails at once with `Offline`, and `submit_or_queue` keeps
//!   the report in the outbox (`offline.rs`) under the ID it was given. `go_online` opens the gate and
//!   sends the outbox with those IDs, so a report the cloud already has is answered from its record.
//!
//! `ctl`'s commands are the other half of this file: tarpc over TCP, one call, then done (D168).
//!
//! Owner: F (client and scale).

use std::collections::BTreeSet;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use futures::StreamExt;
use futures::stream::FuturesUnordered;
use rand::Rng;
use tarpc::client::{NewClient, RpcError};
use tokio::task::JoinSet;
use tokio::time::Instant;

use crate::config::{Config, Timers};
use crate::metrics::{Event, EventLog, UdpCounters, UdpCounts, now_ms, now_us};
use crate::node::datagram::MAX_CALL_DEADLINE;
use crate::node::transport::{FaultGate, connect_client, datagram_client, gated_datagram_client};
use crate::node::usage;
use crate::offline::{Delivery, FLUSH_IN_FLIGHT, Outbox, OutboxError, Queued, Reconciled};
use crate::p2p::{Delivered, PeerError, PeerLink};
use crate::presence::{self, Discovered, Presence, PresenceError};
use crate::proto::{
    Capability, ClientId, ClientReply, ClientRequest, ClientRpcClient, CommittedResult,
    ControlReply, ControlRequest, Incident, IncidentStatus, MAX_DISCOVER_RECORDS, NodeId, P2pBody,
    P2pEnvelope, Plan, PresenceQuery, PresenceRole, ReplyBody, RequestId, UserId, ZoneId,
};

/// Each wait varies by up to this share, in percent, so retries do not arrive together.
const JITTER_PERCENT: u64 = 20;
/// How often a client program logs what its sockets did.
const REPORT_INTERVAL: Duration = Duration::from_secs(1);
/// Status calls a UDP check makes before it says "no answer", so one lost packet cannot read as a
/// closed port.
const UDP_CHECK_TRIES: u32 = 3;

/// Of what a client sends first, the share it may also send as retries (D264).
const RETRY_BUDGET_PERCENT: u64 = 20;
/// Retries a second it may always send, so a light load keeps all its retries through an outage:
/// about 33 reports a second from a program at 300 a second in all, 5 retries each. Over nine
/// programs this is 1,800 a second, well under what the nodes can take.
const RETRY_BUDGET_FLOOR_PER_S: u64 = 200;
/// Retries it can save up, for the burst when a node fails: about 10 s of its share at 20,000 a
/// second over nine programs. Only an outage longer than that reaches the limit.
const RETRY_BUDGET_CAP: u64 = 5000;
/// What it starts with.
const RETRY_BUDGET_START: u64 = 100;

/// What a client may spend on retries, in thousandths of a retry. Without this a long outage is
/// followed by a flood: at 20,000 a second, clients sent 30,000 retries a second over the air the
/// nodes needed to catch up, answers fell to nothing, and they stayed there until the load ended.
#[derive(Debug)]
pub(crate) struct RetryBudget {
    thousandths: u64,
    refilled: Instant,
}

impl RetryBudget {
    fn new(now: Instant) -> Self {
        Self {
            thousandths: RETRY_BUDGET_START * 1000,
            refilled: now,
        }
    }

    fn refill(&mut self, now: Instant) {
        // A floor of N a second is N thousandths a millisecond.
        let ms = now.saturating_duration_since(self.refilled).as_millis() as u64;
        self.thousandths =
            (self.thousandths + ms * RETRY_BUDGET_FLOOR_PER_S).min(RETRY_BUDGET_CAP * 1000);
        self.refilled = now;
    }

    pub(crate) fn first_send(&mut self, now: Instant) {
        self.refill(now);
        self.thousandths =
            (self.thousandths + RETRY_BUDGET_PERCENT * 10).min(RETRY_BUDGET_CAP * 1000);
    }

    pub(crate) fn may_retry(&mut self, now: Instant) -> bool {
        self.refill(now);
        let may = self.thousandths >= 1000;
        if may {
            self.thousandths -= 1000;
        }
        may
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("no reply after {attempts} attempts")]
    GaveUp { attempts: u32 },
    #[error("cannot connect to {addr}: {source}")]
    Connect {
        addr: String,
        source: std::io::Error,
    },
    #[error("timed out")]
    Timeout,
    #[error("connection closed")]
    Closed,
    /// The client is offline: nothing was sent (D400). `submit_or_queue` queues instead.
    #[error("the client is offline")]
    Offline,
    #[error(transparent)]
    Outbox(#[from] OutboxError),
}

/// A request that got a reply.
#[derive(Debug, Clone)]
pub struct Submitted {
    /// The request ID, which is also the incident's.
    pub id: RequestId,
    /// The node that answered first.
    pub from: NodeId,
    /// The node that planned the request.
    pub executor: NodeId,
    pub plan: Plan,
    /// The committed result; `None` for a provisional answer, which has none yet.
    pub committed: Option<CommittedResult>,
    /// True if a lone node answered without a commit (D123).
    pub provisional: bool,
    /// Where the incident stood when this answer was given (D142). To see it change later, for
    /// example to Resolved, read it with `ControlRequest::Incidents`.
    pub status: IncidentStatus,
    /// From first send to first reply, on the client clock.
    pub latency: Duration,
    pub attempts: u32,
}

/// One client process's way to the cloud. Share it between simulated users with `Arc`.
pub struct MeshClient {
    id: ClientId,
    retry: Duration,
    retry_max: Duration,
    max_retries: u32,
    /// How long one submit's calls may stay open.
    call_budget: Duration,
    log: EventLog,
    next_seq: AtomicU64,
    nodes: Vec<ClientRpcClient>,
    counters: Arc<UdpCounters>,
    retry_budget: Arc<std::sync::Mutex<RetryBudget>>,
    retries_skipped: Arc<AtomicU64>,
    /// The presence agent (Part H): joins, renewals and discovery (D326).
    presence: Presence,
    /// The P2P link (Part H): this program's peer endpoint and its way to other programs (D336).
    peer: PeerLink,
    /// Closed while offline; every socket holds a copy (D402).
    gate: FaultGate,
    /// Reports made while offline (D401).
    outbox: Outbox,
    /// When this client last went offline, while it is.
    offline_since: std::sync::Mutex<Option<Instant>>,
    /// Each stub's dispatch and the reporter. Dropped with the client, which aborts them.
    _tasks: JoinSet<()>,
}

impl MeshClient {
    /// Open one socket and stub per node in the config. Must run inside a tokio runtime.
    ///
    /// Cannot fail, as the TCP client it replaced could not. A socket that will not open means that
    /// node's copies are never sent, as if the node were away: the other nodes still answer, and with
    /// none left `submit` gives up.
    pub fn new(id: ClientId, config: &Config, log: EventLog) -> Self {
        let counters = Arc::new(UdpCounters::default());
        let gate = FaultGate::default();
        let mut nodes = Vec::new();
        let mut tasks = JoinSet::new();
        for node in &config.cluster.nodes {
            match gated_datagram_client(
                &node.client_addr,
                counters.clone(),
                Some(&log),
                gate.clone(),
            ) {
                Ok(NewClient { client, dispatch }) => {
                    tasks.spawn(dispatch);
                    nodes.push(client);
                }
                Err(e) => tracing::warn!(
                    node = node.id,
                    addr = %node.client_addr,
                    "cannot open a socket for this node, so it gets no reports: {e}"
                ),
            }
        }
        if log.is_enabled() {
            tasks.spawn(report_counters(counters.clone(), log.clone()));
        }
        let retry_budget = Arc::new(std::sync::Mutex::new(RetryBudget::new(Instant::now())));
        let retries_skipped = Arc::new(AtomicU64::new(0));
        let presence = Presence::new(
            presence::Sender {
                nodes: nodes.clone(),
                gate: gate.clone(),
                retry: config.timers.client_retry(),
                retry_max: config.timers.client_retry_max(),
                max_retries: config.timers.client_max_retries,
                call_budget: call_budget(&config.timers),
                budget: retry_budget.clone(),
                retries_skipped: retries_skipped.clone(),
                log: log.clone(),
            },
            // The program's start time, every user's incarnation (D323).
            now_ms(),
        );
        let peer = PeerLink::new(id, config, gate.clone(), log.clone());
        Self {
            id,
            retry: config.timers.client_retry(),
            retry_max: config.timers.client_retry_max(),
            max_retries: config.timers.client_max_retries,
            call_budget: call_budget(&config.timers),
            log,
            // Starts at the current time, so a rerun with the same client ID can never reuse a
            // request ID the cluster has already answered (D113).
            next_seq: AtomicU64::new(now_us()),
            nodes,
            counters,
            retry_budget,
            retries_skipped,
            presence,
            peer,
            gate,
            outbox: Outbox::new(Outbox::path_for(config.metrics.dir.as_deref(), id)),
            offline_since: std::sync::Mutex::new(None),
            _tasks: tasks,
        }
    }

    // -----------------------------------------------------------------------------------------
    // Presence (Phase 2, Part H): see `presence.rs`
    // -----------------------------------------------------------------------------------------

    /// Put a user in the presence directory and keep it there: the agent renews its lease until
    /// [`leave`](Self::leave) (D326). Returns the lease the directory gave. The record carries this
    /// program's peer endpoint, so other programs can reach the user directly (D336).
    pub async fn join(
        &self,
        user: UserId,
        role: PresenceRole,
        zone: ZoneId,
        capabilities: Vec<Capability>,
    ) -> Result<Duration, PresenceError> {
        let peer_addr = self
            .peer
            .addr()
            .ok_or(PresenceError::NoPeerEndpoint)?
            .to_string();
        self.presence
            .join(user, role, zone, capabilities, peer_addr)
            .await
    }

    /// Stop renewing a user and take it out of the directory.
    pub async fn leave(&self, user: UserId) -> Result<(), PresenceError> {
        self.presence.leave(user).await
    }

    /// Move a joined user to another zone: a new registration with the new zone (D323).
    pub async fn move_zone(&self, user: UserId, zone: ZoneId) -> Result<Duration, PresenceError> {
        self.presence.move_zone(user, zone).await
    }

    /// Who is online, by zone and capability (D321). Asked of every node; the first answer wins.
    pub async fn discover(&self, query: PresenceQuery) -> Result<Discovered, PresenceError> {
        self.presence.discover(query).await
    }

    /// Who is online in a zone.
    pub async fn discover_zone(&self, zone: ZoneId) -> Result<Discovered, PresenceError> {
        self.discover(PresenceQuery {
            zone: Some(zone),
            capability: None,
            limit: MAX_DISCOVER_RECORDS,
        })
        .await
    }

    /// Who is online with a capability.
    pub async fn discover_capability(
        &self,
        capability: Capability,
    ) -> Result<Discovered, PresenceError> {
        self.discover(PresenceQuery {
            zone: None,
            capability: Some(capability),
            limit: MAX_DISCOVER_RECORDS,
        })
        .await
    }

    // -----------------------------------------------------------------------------------------
    // The P2P link (Phase 2, Part H): see `p2p.rs`
    // -----------------------------------------------------------------------------------------

    /// This program's peer endpoint, `ip:port`, as presence records advertise it (D336, D337).
    /// `None` if it could not open.
    pub fn peer_addr(&self) -> Option<&str> {
        self.peer.addr()
    }

    /// The updates other programs delivered to this one, as a stream the application or Part I's
    /// gossip reads (D460). Only the first call gets it.
    pub fn take_peer_updates(&self) -> Option<tokio::sync::mpsc::Receiver<P2pEnvelope>> {
        self.peer.take_updates()
    }

    /// A new peer update from `from`, with a fresh update ID (D339). `to: None` is for a whole zone.
    pub fn new_update(
        &self,
        from: UserId,
        to: Option<UserId>,
        zone: ZoneId,
        version: u64,
        max_hops: u8,
        body: P2pBody,
    ) -> P2pEnvelope {
        self.peer
            .new_update(from, to, zone, version, max_hops, body)
    }

    /// Deliver an update straight to another program's peer endpoint, found with `discover`; the
    /// cloud carries none of it (D310, D339). Forwarding (Part I) sends an existing envelope again
    /// after raising its `hops`.
    pub async fn send_peer(
        &self,
        peer_addr: &str,
        envelope: P2pEnvelope,
    ) -> Result<Delivered, PeerError> {
        self.peer.send_peer(peer_addr, envelope).await
    }

    /// What this program's peer sockets did, sent and received.
    pub fn p2p_counts(&self) -> UdpCounts {
        self.peer.counters().snapshot()
    }

    // -----------------------------------------------------------------------------------------
    // Offline operation (Phase 2, Part K)
    // -----------------------------------------------------------------------------------------

    pub fn is_offline(&self) -> bool {
        self.gate.is_silenced()
    }

    /// Cut this client off: its sockets send and hear nothing until [`go_online`](Self::go_online)
    /// (D402). Going offline twice is the same as once.
    pub fn go_offline(&self) {
        if self.gate.try_silence() {
            *self
                .offline_since
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Instant::now());
            self.log.log(Event::ClientOffline);
        }
    }

    /// Reconnect, then send everything queued while offline and wait for the answers (D403).
    pub async fn go_online(&self) -> Result<Reconciled, ClientError> {
        let since = self
            .offline_since
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        self.gate.restore();
        self.presence.back_online();
        if let Some(since) = since {
            self.log.log(Event::ClientOnline {
                offline_ms: since.elapsed().as_millis() as u64,
                queued: self.outbox.len().await,
            });
        }
        self.flush_outbox().await
    }

    /// Reports waiting in the outbox.
    pub async fn queued(&self) -> usize {
        self.outbox.len().await
    }

    /// Send every queued report with its own ID, oldest first and [`FLUSH_IN_FLIGHT`] at a time
    /// (D404). An answered report leaves the outbox, and so does one the cloud refused as too old,
    /// because that refusal is on record too (D143). One with no answer stays for next time.
    pub async fn flush_outbox(&self) -> Result<Reconciled, ClientError> {
        let entries = self.outbox.entries().await;
        if entries.is_empty() {
            return Ok(Reconciled::default());
        }
        let started = Instant::now();
        let mut done = Reconciled::default();
        let mut delivered = BTreeSet::new();
        let mut sends = futures::stream::iter(entries)
            .map(|queued| async move {
                let outcome = self
                    .send_report(queued.id, queued.incident.clone(), Instant::now())
                    .await;
                (queued, outcome)
            })
            .buffer_unordered(FLUSH_IN_FLIGHT);
        while let Some((queued, outcome)) = sends.next().await {
            done.sent += 1;
            let outcome = match outcome {
                Ok(answer) if answer.status == IncidentStatus::Expired => {
                    done.expired += 1;
                    delivered.insert(queued.id);
                    "expired"
                }
                Ok(_) => {
                    done.answered += 1;
                    delivered.insert(queued.id);
                    "answered"
                }
                Err(_) => {
                    done.failed += 1;
                    "failed"
                }
            };
            self.log.log(Event::OutboxSent {
                request: queued.id,
                outcome,
                waited_ms: now_ms().saturating_sub(queued.queued_at_ms),
            });
        }
        drop(sends);
        done.took = started.elapsed();
        self.outbox.remove(&delivered).await?;
        self.log.log(Event::OutboxFlushed {
            sent: done.sent,
            answered: done.answered,
            expired: done.expired,
            failed: done.failed,
            reconcile_us: done.took.as_micros() as u64,
        });
        Ok(done)
    }

    /// As [`submit`](Self::submit), but while offline the report waits in the outbox instead of
    /// failing (D400). A report cut off while it was out is queued too, under the same ID, so a
    /// copy that got through before the cut is harmless.
    pub async fn submit_or_queue(&self, incident: Incident) -> Result<Delivery, ClientError> {
        self.submit_or_queue_from(incident, Instant::now()).await
    }

    /// As [`submit_or_queue`](Self::submit_or_queue), with latency counted from `started` (D54).
    pub async fn submit_or_queue_from(
        &self,
        incident: Incident,
        started: Instant,
    ) -> Result<Delivery, ClientError> {
        let id = self.next_id();
        if !self.is_offline() {
            match self.send_report(id, incident.clone(), started).await {
                Ok(done) => return Ok(Delivery::Answered(done)),
                Err(ClientError::Offline) => {}
                Err(e) => return Err(e),
            }
        }
        self.outbox
            .push(Queued {
                id,
                incident,
                queued_at_ms: now_ms(),
            })
            .await?;
        self.log.log(Event::OutboxQueued { request: id });
        Ok(Delivery::Queued(id))
    }

    /// What this client's sockets have done so far.
    pub fn udp_counts(&self) -> UdpCounts {
        self.counters.snapshot()
    }

    /// Submit an incident and wait for the first reply. Latency counts from now. This is the one
    /// call user code makes: the copies, the retries and the first answer winning are behind it.
    pub async fn submit(&self, incident: Incident) -> Result<Submitted, ClientError> {
        self.submit_from(incident, Instant::now()).await
    }

    /// As [`submit`](Self::submit), with latency counted from `started`. An open-loop generator
    /// passes the intended send time, so queueing in the generator is not hidden (D54).
    pub async fn submit_from(
        &self,
        incident: Incident,
        started: Instant,
    ) -> Result<Submitted, ClientError> {
        self.send_report(self.next_id(), incident, started).await
    }

    fn next_id(&self) -> RequestId {
        RequestId {
            client: self.id,
            seq: self.next_seq.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// One report under `id`, with its copies and retries. Ends with `Offline` as soon as the client
    /// is found offline before an attempt, as nothing it sends could arrive.
    async fn send_report(
        &self,
        id: RequestId,
        incident: Incident,
        started: Instant,
    ) -> Result<Submitted, ClientError> {
        if self.is_offline() {
            return Err(ClientError::Offline);
        }
        // One wall-clock difference now, then a monotonic clock, so a clock step during the retries
        // cannot fake an age (D143). Against the clock that stamped the report, this starts near 0.
        let age_at_submit_ms = now_ms().saturating_sub(incident.reported_at_ms);
        let submitted = Instant::now();
        // From now and never from `started`, which open loop sets to the intended send time.
        let deadline = submitted.into_std() + self.call_budget;
        let attempts = self.max_retries + 1;
        // Dropping this, by returning, is what sends the cancels.
        let mut calls = FuturesUnordered::new();
        for attempt in 0..attempts {
            if self.is_offline() {
                return Err(ClientError::Offline);
            }
            let age_ms = age_at_submit_ms.saturating_add(submitted.elapsed().as_millis() as u64);
            let request = ClientRequest {
                id,
                attempt,
                age_ms,
                incident: incident.clone(),
            };
            // The time for this attempt still passes if it is not sent, so a report gives up when
            // it always did, and "5 retries, then an error" still holds for a client with few.
            if self.may_send(attempt) {
                self.log.log(Event::ClientSent {
                    request: id,
                    attempt,
                });
                calls.extend(self.group_send(deadline, request));
            }
            let wait = tokio::time::sleep(jittered(self.wait_before_retry(attempt)));
            tokio::pin!(wait);
            loop {
                tokio::select! {
                    // Biased, so an answer that is ready beats the timer. The guard stops a spin
                    // when every call has ended: the timer still decides when the next attempt goes.
                    biased;
                    Some(result) = calls.next(), if !calls.is_empty() => {
                        if let Some(reply) = answer_to(id, result) {
                            return Ok(self.finished(reply, started, attempt + 1));
                        }
                    }
                    _ = &mut wait => break,
                }
            }
        }
        self.log.log(Event::ClientGaveUp {
            request: id,
            attempts,
        });
        Err(ClientError::GaveUp { attempts })
    }

    fn may_send(&self, attempt: u32) -> bool {
        let now = Instant::now();
        let mut budget = self
            .retry_budget
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if attempt == 0 {
            budget.first_send(now);
            return true;
        }
        let may = budget.may_retry(now);
        if !may {
            self.retries_skipped.fetch_add(1, Ordering::Relaxed);
        }
        may
    }

    /// The group send (D126, D210): the client middleware's send to all of the cloud nodes. The
    /// application made one call, and each node gets its own copy from this layer. Best effort and
    /// unordered: the retry covers a lost copy, and Raft orders results. Always every configured
    /// node, never the cloud's who's-alive list, so a report can reach a node that is coming back.
    fn group_send(
        &self,
        deadline: std::time::Instant,
        request: ClientRequest,
    ) -> impl Iterator<Item = impl Future<Output = Result<ClientReply, RpcError>> + '_> + '_ {
        self.nodes.iter().map(move |node| {
            let mut ctx = tarpc::context::current();
            ctx.deadline = deadline;
            node.submit(ctx, request.clone())
        })
    }

    /// Doubles from `client_retry_ms` up to `client_retry_max_ms`.
    fn wait_before_retry(&self, attempt: u32) -> Duration {
        wait_before_retry(self.retry, self.retry_max, attempt)
    }

    fn finished(&self, reply: ClientReply, started: Instant, attempts: u32) -> Submitted {
        let (executor, plan, status, committed) = match reply.body {
            // The record says which of the two it is, so both end the same way.
            ReplyBody::Done(result) | ReplyBody::Expired(result) => {
                let status = result.status_at_commit();
                (result.executor, result.plan.clone(), status, Some(result))
            }
            // A lone node plans for itself.
            ReplyBody::Provisional(plan) => (reply.from, plan, IncidentStatus::Provisional, None),
            ReplyBody::Rejected(_) => unreachable!("answer_to does not hand over Rejected replies"),
        };
        let provisional = committed.is_none();
        let latency = started.elapsed();
        self.log.log(Event::ClientDone {
            request: reply.id,
            from: reply.from,
            executor,
            latency_us: latency.as_micros() as u64,
            attempts,
            provisional,
            no_responder: plan.responder.is_none(),
            status: status.as_str(),
            priority: plan.priority,
        });
        Submitted {
            id: reply.id,
            from: reply.from,
            executor,
            plan,
            committed,
            provisional,
            status,
            latency,
            attempts,
        }
    }
}

impl Drop for MeshClient {
    /// The last line, so the cancels this client's final submits sent are in the log too.
    fn drop(&mut self) {
        self.log.log(Event::ClientRetriesSkipped {
            skipped: self.retries_skipped.load(Ordering::Relaxed),
        });
        self.log.log(Event::UdpCounters {
            counts: self.counters.snapshot(),
            udp_drops: None,
        });
    }
}

/// Doubles from `retry` up to `retry_max`.
pub(crate) fn wait_before_retry(retry: Duration, retry_max: Duration, attempt: u32) -> Duration {
    retry
        .saturating_mul(2u32.saturating_pow(attempt))
        .min(retry_max)
}

/// How long the calls of one submit may stay open: 1.2 times all its waits, so the longest they can
/// come to with jitter, or 36 s at the default timers (D169). tarpc's default of 10 s would quietly
/// lose late answers.
fn call_budget(timers: &Timers) -> Duration {
    let (retry, retry_max) = (timers.client_retry(), timers.client_retry_max());
    let total = (0..=timers.client_max_retries)
        .map(|attempt| wait_before_retry(retry, retry_max, attempt))
        .fold(Duration::ZERO, Duration::saturating_add);
    total
        .saturating_add(total / 5)
        .min(MAX_CALL_DEADLINE - Duration::from_secs(1))
}

/// The answer in a call's result, if it holds one for this request.
///
/// Names every body, with no wildcard: a new one must not start out ignored, or a user would wait
/// for the whole retry budget and give up on a report the cloud had already answered (D143).
///
/// A call that ended without one is no answer: another node's may come, or the retry's. The ID is
/// checked because tarpc matches by call ID alone, and a program restarted on a source port that an
/// old one used can be handed the old program's reply.
fn answer_to(id: RequestId, result: Result<ClientReply, RpcError>) -> Option<ClientReply> {
    let reply = result.ok().filter(|reply| reply.id == id)?;
    match reply.body {
        ReplyBody::Done(_) | ReplyBody::Expired(_) | ReplyBody::Provisional(_) => Some(reply),
        ReplyBody::Rejected(_) => None,
    }
}

pub(crate) fn jittered(wait: Duration) -> Duration {
    let percent = rand::thread_rng().gen_range(100 - JITTER_PERCENT..=100 + JITTER_PERCENT);
    wait.saturating_mul(percent as u32) / 100
}

/// Log what this program's sockets did, once a second, for as long as the client lives.
async fn report_counters(counters: Arc<UdpCounters>, log: EventLog) {
    let mut tick = tokio::time::interval(REPORT_INTERVAL);
    loop {
        tick.tick().await;
        // A blocking read of /proc, as on the nodes, so it stays off the runtime.
        let udp_drops = tokio::task::spawn_blocking(usage::read_udp_drops)
            .await
            .unwrap_or(None);
        log.log(Event::UdpCounters {
            counts: counters.snapshot(),
            udp_drops,
        });
    }
}

/// Make the call that matches a control command.
async fn call_control(
    stub: &ClientRpcClient,
    ctx: tarpc::context::Context,
    request: ControlRequest,
) -> Result<ControlReply, RpcError> {
    match request {
        ControlRequest::Status => stub.status(ctx).await,
        ControlRequest::Silence { ms } => stub.silence(ctx, ms).await,
        ControlRequest::SkewLoad { extra_work } => stub.skew_load(ctx, extra_work).await,
        ControlRequest::Incidents(query) => stub.incidents(ctx, query).await,
    }
}

fn context_for(timeout: Duration) -> tarpc::context::Context {
    let mut ctx = tarpc::context::current();
    ctx.deadline = std::time::Instant::now() + timeout;
    ctx
}

/// Send one control command to one node over TCP and wait for its answer (used by `ctl`).
pub async fn control(
    addr: &str,
    request: ControlRequest,
    timeout: Duration,
) -> Result<ControlReply, ClientError> {
    let exchange = async {
        let NewClient { client, dispatch } =
            connect_client(addr)
                .await
                .map_err(|source| ClientError::Connect {
                    addr: addr.to_string(),
                    source,
                })?;
        tokio::select! {
            reply = call_control(&client, context_for(timeout), request) => {
                reply.map_err(|e| match e {
                    RpcError::DeadlineExceeded => ClientError::Timeout,
                    _ => ClientError::Closed,
                })
            }
            // Ends when the node closes the connection, and then the call cannot end any other way.
            () = dispatch => Err(ClientError::Closed),
        }
    };
    tokio::time::timeout(timeout, exchange)
        .await
        .map_err(|_| ClientError::Timeout)?
}

/// Whether a status call to a node's client port gets an answer over UDP, the way reports travel.
/// Without this check a missing firewall rule looks like a healthy node whose copies vanish (D168).
/// A silenced node answers too: this asks about the port, and control passes the fault gate.
pub async fn udp_answers(addr: &str, timeout: Duration) -> bool {
    let Ok(NewClient { client, dispatch }) = datagram_client(addr, Arc::default(), None) else {
        return false;
    };
    let tries = async {
        for _ in 0..UDP_CHECK_TRIES {
            let ctx = context_for(timeout / UDP_CHECK_TRIES);
            if client.status(ctx).await.is_ok() {
                return true;
            }
        }
        false
    };
    // tarpc's timer starts only once it has written a call, so this wait needs a cap of its own.
    let answered = tokio::time::timeout(timeout, async {
        tokio::select! {
            answered = tries => answered,
            () = dispatch => false,
        }
    });
    answered.await.unwrap_or(false)
}

#[cfg(test)]
mod tests {
    #[test]
    fn retries_stop_at_a_share_of_what_was_sent_first_and_a_small_floor() {
        let start = Instant::now();
        let mut budget = RetryBudget::new(start);
        budget.thousandths = 0;
        for _ in 0..1000 {
            budget.first_send(start);
        }
        // 1000 first sends earn 200 retries, and no more.
        assert_eq!((0..1000).filter(|_| budget.may_retry(start)).count(), 200);
        // With nothing sent, only the floor comes in.
        let later = start + Duration::from_secs(2);
        assert_eq!(
            (0..1000).filter(|_| budget.may_retry(later)).count() as u64,
            2 * RETRY_BUDGET_FLOOR_PER_S
        );
    }

    /// 300 reports a second over nine programs is about 33 each, with up to 5 retries for each
    /// report while every node is down. The nodes can take that many times over, so a budget that
    /// stops it only loses reports: 3,512 gave up at 300 a second with a floor of 10.
    #[test]
    fn a_light_load_keeps_every_retry_however_long_the_outage() {
        let start = Instant::now();
        let mut budget = RetryBudget::new(start);
        budget.thousandths = 0;
        for second in 1..=60u64 {
            let now = start + Duration::from_secs(second);
            for _ in 0..33 {
                budget.first_send(now);
            }
            let allowed = (0..33 * 5).filter(|_| budget.may_retry(now)).count();
            assert_eq!(allowed, 33 * 5, "at second {second}");
        }
    }

    #[test]
    fn a_retry_budget_cannot_save_up_past_its_cap_and_one_report_can_always_retry() {
        let start = Instant::now();
        let mut budget = RetryBudget::new(start);
        assert!(
            (0..5).all(|_| budget.may_retry(start)),
            "5 retries, then an error"
        );
        let much_later = start + Duration::from_secs(100_000);
        let spent = (0..RETRY_BUDGET_CAP * 2)
            .filter(|_| budget.may_retry(much_later))
            .count() as u64;
        assert_eq!(spent, RETRY_BUDGET_CAP);
    }

    use super::*;

    #[test]
    fn jitter_stays_within_twenty_percent() {
        let wait = Duration::from_millis(1000);
        for _ in 0..1000 {
            let j = jittered(wait);
            assert!(j >= Duration::from_millis(800) && j <= Duration::from_millis(1200));
        }
    }

    #[test]
    fn the_call_budget_is_36_s_at_default_timers() {
        assert_eq!(call_budget(&Timers::default()), Duration::from_secs(36));
    }

    #[test]
    fn the_call_budget_stays_within_what_tarpc_can_hold() {
        let timers = Timers {
            client_retry_ms: u64::MAX,
            client_retry_max_ms: u64::MAX,
            ..Timers::default()
        };
        assert!(call_budget(&timers) < MAX_CALL_DEADLINE);
    }

    fn done_for(id: RequestId, body: ReplyBody) -> Result<ClientReply, RpcError> {
        Ok(ClientReply { id, from: 1, body })
    }

    #[test]
    fn only_a_real_answer_for_this_request_counts() {
        let id = RequestId { client: 1, seq: 2 };
        let plan = Plan {
            responder: None,
            cost: 0,
            priority: 0,
        };
        assert!(answer_to(id, done_for(id, ReplyBody::Provisional(plan.clone()))).is_some());
        assert!(answer_to(id, done_for(id, ReplyBody::Rejected("no".into()))).is_none());
        assert!(answer_to(id, Err(RpcError::DeadlineExceeded)).is_none());
        // An old program's reply, handed over because a restarted one reused its source port.
        let other = RequestId { client: 1, seq: 3 };
        assert!(answer_to(id, done_for(other, ReplyBody::Provisional(plan))).is_none());
    }
}
