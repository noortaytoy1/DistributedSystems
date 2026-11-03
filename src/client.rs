//! Client middleware: multicast each request to every node, retry with the same request ID,
//! keep the first reply. The client never names a node.
//!
//! The application makes one `submit` call; this layer copies the request to every replica. One
//! [`MeshClient`] holds one tarpc stub per node, each on its own UDP socket, shared by all simulated
//! users through `Arc` (D53, D167). tarpc matches each reply to its call.
//!
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
//!
//! `ctl`'s commands are the other half of this file: tarpc over TCP, one call, then done (D168).
//!
//! Owner: F (client and scale).

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
use crate::node::transport::{connect_client, datagram_client};
use crate::node::usage;
use crate::proto::{
    ClientId, ClientReply, ClientRequest, ClientRpcClient, CommittedResult, ControlReply,
    ControlRequest, Incident, IncidentStatus, NodeId, Plan, ReplyBody, RequestId,
};

/// Each wait varies by up to this share, in percent, so retries do not arrive together.
const JITTER_PERCENT: u64 = 20;
/// How often a client program logs what its sockets did.
const REPORT_INTERVAL: Duration = Duration::from_secs(1);
/// Status calls a UDP check makes before it says "no answer", so one lost packet cannot read as a
/// closed port.
const UDP_CHECK_TRIES: u32 = 3;

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
        let mut nodes = Vec::new();
        let mut tasks = JoinSet::new();
        for node in &config.cluster.nodes {
            match datagram_client(&node.client_addr, counters.clone(), Some(&log)) {
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
            _tasks: tasks,
        }
    }

    /// What this client's sockets have done so far.
    pub fn udp_counts(&self) -> UdpCounts {
        self.counters.snapshot()
    }

    /// Submit an incident and wait for the first reply. Latency counts from now.
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
        let id = RequestId {
            client: self.id,
            seq: self.next_seq.fetch_add(1, Ordering::Relaxed),
        };
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
            let age_ms = age_at_submit_ms.saturating_add(submitted.elapsed().as_millis() as u64);
            let request = ClientRequest {
                id,
                attempt,
                age_ms,
                incident: incident.clone(),
            };
            self.log.log(Event::ClientSent {
                request: id,
                attempt,
            });
            calls.extend(self.group_send(deadline, request));
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

    /// The group send (D126): the application made one call, and each node gets its own copy from
    /// this layer. Best effort and unordered: the retry covers a lost copy, and Raft orders results.
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
        self.log.log(Event::UdpCounters {
            counts: self.counters.snapshot(),
            udp_drops: None,
        });
    }
}

/// Doubles from `retry` up to `retry_max`.
fn wait_before_retry(retry: Duration, retry_max: Duration, attempt: u32) -> Duration {
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

fn jittered(wait: Duration) -> Duration {
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
