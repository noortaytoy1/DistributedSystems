//! The presence agent: the client middleware's side of discovery (Phase 2, Part H, D326–D335).
//!
//! A program's users join the presence directory (Part G's, in the cloud) through one call each,
//! and the agent keeps them there: it renews every user's lease at a third of the lease the
//! directory gave (D320), registers a user again when its lease was lost (D329), and lets the
//! application ask who is online by zone or capability (D321). The application never names a node
//! or sees a lease: it calls [`MeshClient::join`](crate::client::MeshClient::join) and friends,
//! which it gets from `src/middleware.rs`.
//!
//! - **Every call goes to all three nodes (D327).** Like a report: the first good answer wins, the
//!   retry schedule and the retry budget are the report's, and a node that fails costs nothing while
//!   another answers. If every node refuses, the caller learns it at once instead of waiting out the
//!   retries.
//! - **One renewal loop per program (D328).** It keeps the joined users and when each is due, and
//!   renews the due ones together, each in its own task, with jitter so a thousand users do not renew
//!   in the same millisecond.
//! - **A lost lease is registered again (D329).** A renewal answered `Unknown`, or no renewal that
//!   worked for a whole lease, means the directory may have dropped the user: the agent registers it
//!   again, with the same incarnation, because the program did not restart.
//! - **Offline (D329, with Part K's gate, D402).** While the client is offline every call fails at
//!   once, so renewals wait; on `go_online` every joined user registers again straight away.
//!
//! Owner: H (client presence and P2P link).

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use futures::StreamExt;
use futures::stream::FuturesUnordered;
use rand::Rng;
use tarpc::client::RpcError;
use tokio::sync::Notify;
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::Instant;

use crate::client::{RetryBudget, jittered, wait_before_retry};
use crate::metrics::{Event, EventLog};
use crate::node::transport::FaultGate;
use crate::proto::{
    Capability, ClientRpcClient, DiscoverReply, MAX_DISCOVER_RECORDS, PresenceQuery,
    PresenceRecord, PresenceReply, PresenceRole, UserId, ZoneId,
};

/// How far a renewal may move from a third of the lease, in percent either way, so users that
/// joined together spread out (D328).
const RENEW_JITTER_PERCENT: u64 = 10;
/// The longest a failed renewal waits before the next try, so a lease is not lost to one slow
/// moment (D329).
const RETRY_RENEW_AFTER: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PresenceError {
    /// Every node refused, with the first node's reason: for example no directory yet (D325).
    #[error("refused by every node: {0}")]
    Rejected(String),
    #[error("no answer after {attempts} attempts")]
    GaveUp { attempts: u32 },
    /// The client is offline (Part K): nothing was sent.
    #[error("the client is offline")]
    Offline,
    /// `leave` or `move_zone` for a user that has not joined.
    #[error("user {0} has not joined")]
    NotJoined(UserId),
    /// This program has no peer endpoint to advertise, so its users cannot join (D336).
    #[error("this program has no peer endpoint")]
    NoPeerEndpoint,
}

/// What a discovery query found, and where it was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovered {
    pub records: Vec<PresenceRecord>,
    /// The log index the answering node read at; another node can be a few entries off (D321).
    pub log_index: u64,
    /// From the first send to the answer, on this client's clock.
    pub latency: Duration,
}

/// What the agent needs from its client to reach the cloud: the client's stubs, its gate, its
/// retry timers and budget. Cheap to clone, so the renewal loop holds its own copy.
#[derive(Clone)]
pub(crate) struct Sender {
    pub(crate) nodes: Vec<ClientRpcClient>,
    pub(crate) gate: FaultGate,
    pub(crate) retry: Duration,
    pub(crate) retry_max: Duration,
    pub(crate) max_retries: u32,
    pub(crate) call_budget: Duration,
    pub(crate) budget: Arc<Mutex<RetryBudget>>,
    pub(crate) retries_skipped: Arc<AtomicU64>,
    pub(crate) log: EventLog,
}

/// What one node's answer means to [`Sender::call_all`].
enum Verdict<T> {
    Done(T),
    Refused(String),
}

impl Sender {
    fn may_send(&self, attempt: u32) -> bool {
        let now = Instant::now();
        let mut budget = lock(&self.budget);
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

    /// Ask every node, retrying like a report (D327), and return the first good answer. A node
    /// that refuses is not asked again; when every node has refused, the call ends with the first
    /// reason at once.
    async fn call_all<R, T, C, F>(
        &self,
        call: C,
        judge: impl Fn(R) -> Verdict<T>,
    ) -> Result<T, PresenceError>
    where
        C: Fn(ClientRpcClient, tarpc::context::Context) -> F,
        F: Future<Output = Result<R, RpcError>>,
    {
        if self.gate.is_silenced() {
            return Err(PresenceError::Offline);
        }
        let deadline = std::time::Instant::now() + self.call_budget;
        let attempts = self.max_retries + 1;
        let mut refused: BTreeMap<usize, String> = BTreeMap::new();
        let mut calls = FuturesUnordered::new();
        for attempt in 0..attempts {
            if self.gate.is_silenced() {
                return Err(PresenceError::Offline);
            }
            if self.may_send(attempt) {
                for (index, node) in self.nodes.iter().enumerate() {
                    if refused.contains_key(&index) {
                        continue;
                    }
                    let mut ctx = tarpc::context::current();
                    ctx.deadline = deadline;
                    let answer = call(node.clone(), ctx);
                    calls.push(async move { (index, answer.await) });
                }
            }
            let wait = tokio::time::sleep(jittered(wait_before_retry(
                self.retry,
                self.retry_max,
                attempt,
            )));
            tokio::pin!(wait);
            loop {
                tokio::select! {
                    biased;
                    Some((index, result)) = calls.next(), if !calls.is_empty() => {
                        let Ok(reply) = result else { continue };
                        match judge(reply) {
                            Verdict::Done(value) => return Ok(value),
                            Verdict::Refused(reason) => {
                                refused.entry(index).or_insert(reason);
                                if refused.len() == self.nodes.len() {
                                    let first = refused.into_values().next().unwrap_or_default();
                                    return Err(PresenceError::Rejected(first));
                                }
                            }
                        }
                    }
                    _ = &mut wait => break,
                }
            }
        }
        Err(PresenceError::GaveUp { attempts })
    }

    async fn register(&self, record: &PresenceRecord) -> Result<Duration, PresenceError> {
        let lease = self
            .call_all(
                |node, ctx| {
                    let record = record.clone();
                    async move { node.register(ctx, record).await }
                },
                registered,
            )
            .await?;
        lease.ok_or(PresenceError::Rejected("unknown".into()))
    }

    /// `Ok(None)` when the directory no longer knows the user.
    async fn renew(&self, record: &PresenceRecord) -> Result<Option<Duration>, PresenceError> {
        self.call_all(
            |node, ctx| {
                let (user, incarnation) = (record.user, record.incarnation);
                async move { node.renew(ctx, user, incarnation).await }
            },
            registered,
        )
        .await
    }
}

/// `Registered` and `Unknown` are answers; `Rejected` is one node's refusal.
fn registered(reply: PresenceReply) -> Verdict<Option<Duration>> {
    match reply {
        PresenceReply::Registered { lease_ms, .. } => {
            Verdict::Done(Some(Duration::from_millis(lease_ms)))
        }
        PresenceReply::Unknown => Verdict::Done(None),
        PresenceReply::Rejected(reason) => Verdict::Refused(reason),
    }
}

/// One joined user, as the renewal loop sees it.
struct Joined {
    record: PresenceRecord,
    lease: Duration,
    /// When the next renewal is due. `None` while one is in flight.
    due: Option<Instant>,
    /// When the directory last confirmed the lease.
    confirmed: Instant,
}

#[derive(Default)]
struct Users {
    joined: BTreeMap<UserId, Joined>,
}

/// The agent: one per client program, held by its `MeshClient`.
pub(crate) struct Presence {
    sender: Sender,
    users: Arc<Mutex<Users>>,
    wake: Arc<Notify>,
    /// The program's start time in unix milliseconds, every user's incarnation (D323).
    incarnation: u64,
    renewals: JoinHandle<()>,
}

impl Presence {
    /// Must run inside a tokio runtime: it starts the renewal loop.
    pub(crate) fn new(sender: Sender, incarnation: u64) -> Self {
        let users = Arc::new(Mutex::new(Users::default()));
        let wake = Arc::new(Notify::new());
        let renewals = tokio::spawn(renew_loop(sender.clone(), users.clone(), wake.clone()));
        Self {
            sender,
            users,
            wake,
            incarnation,
            renewals,
        }
    }

    pub(crate) async fn join(
        &self,
        user: UserId,
        role: PresenceRole,
        zone: ZoneId,
        capabilities: Vec<Capability>,
        peer_addr: String,
    ) -> Result<Duration, PresenceError> {
        let record = PresenceRecord {
            user,
            role,
            zone,
            peer_addr,
            capabilities,
            incarnation: self.incarnation,
        };
        self.register_and_keep(record, false).await
    }

    /// Stop renewing the user, then take it out of the directory.
    pub(crate) async fn leave(&self, user: UserId) -> Result<(), PresenceError> {
        let record = lock(&self.users)
            .joined
            .remove(&user)
            .map(|joined| joined.record)
            .ok_or(PresenceError::NotJoined(user))?;
        self.sender
            .call_all(
                |node, ctx| {
                    let (user, incarnation) = (record.user, record.incarnation);
                    async move { node.deregister(ctx, user, incarnation).await }
                },
                |reply| match reply {
                    PresenceReply::Registered { .. } | PresenceReply::Unknown => Verdict::Done(()),
                    PresenceReply::Rejected(reason) => Verdict::Refused(reason),
                },
            )
            .await?;
        self.sender.log.log(Event::PresenceLeft { user });
        Ok(())
    }

    /// A zone move is a new registration with the new zone (D323).
    pub(crate) async fn move_zone(
        &self,
        user: UserId,
        zone: ZoneId,
    ) -> Result<Duration, PresenceError> {
        let mut record = lock(&self.users)
            .joined
            .get(&user)
            .map(|joined| joined.record.clone())
            .ok_or(PresenceError::NotJoined(user))?;
        record.zone = zone;
        self.register_and_keep(record, true).await
    }

    pub(crate) async fn discover(&self, query: PresenceQuery) -> Result<Discovered, PresenceError> {
        let query = PresenceQuery {
            limit: query.limit.min(MAX_DISCOVER_RECORDS),
            ..query
        };
        let started = Instant::now();
        let (records, log_index) = self
            .sender
            .call_all(
                |node, ctx| {
                    let query = query.clone();
                    async move { node.discover(ctx, query).await }
                },
                |reply| match reply {
                    DiscoverReply::Found { records, log_index } => {
                        Verdict::Done((records, log_index))
                    }
                    DiscoverReply::Rejected(reason) => Verdict::Refused(reason),
                },
            )
            .await?;
        let latency = started.elapsed();
        self.sender.log.log(Event::Discovered {
            zone: query.zone,
            capability: query.capability,
            records: records.len(),
            log_index,
            latency_us: latency.as_micros() as u64,
        });
        Ok(Discovered {
            records,
            log_index,
            latency,
        })
    }

    /// The client is back online (Part K): every joined user registers again now (D329).
    pub(crate) fn back_online(&self) {
        let now = Instant::now();
        for joined in lock(&self.users).joined.values_mut() {
            if joined.due.is_some() {
                joined.due = Some(now);
                // Past its lease, so the loop registers it instead of renewing it.
                joined.confirmed = now.checked_sub(joined.lease).unwrap_or(joined.confirmed);
            }
        }
        self.wake.notify_one();
    }

    async fn register_and_keep(
        &self,
        record: PresenceRecord,
        again: bool,
    ) -> Result<Duration, PresenceError> {
        let started = Instant::now();
        let lease = self.sender.register(&record).await?;
        let user = record.user;
        self.sender.log.log(Event::PresenceRegistered {
            user,
            lease_ms: lease.as_millis() as u64,
            latency_us: started.elapsed().as_micros() as u64,
            again,
        });
        let now = Instant::now();
        lock(&self.users).joined.insert(
            user,
            Joined {
                record,
                lease,
                due: Some(now + renew_after(lease)),
                confirmed: now,
            },
        );
        self.wake.notify_one();
        Ok(lease)
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        self.renewals.abort();
    }
}

/// A third of the lease, moved by up to [`RENEW_JITTER_PERCENT`] either way (D320, D328).
fn renew_after(lease: Duration) -> Duration {
    let percent =
        rand::thread_rng().gen_range(100 - RENEW_JITTER_PERCENT..=100 + RENEW_JITTER_PERCENT);
    (lease / 3).saturating_mul(percent as u32) / 100
}

/// Renew every joined user when it is due, each in its own task (D328). Ends with its `Presence`.
async fn renew_loop(sender: Sender, users: Arc<Mutex<Users>>, wake: Arc<Notify>) {
    let mut running = JoinSet::new();
    loop {
        let now = Instant::now();
        let (due, next) = {
            let mut users = lock(&users);
            let mut due = Vec::new();
            let mut next: Option<Instant> = None;
            let offline = sender.gate.is_silenced();
            for joined in users.joined.values_mut() {
                match joined.due {
                    // Offline: nothing can be sent, so wait for `back_online` to wake the loop.
                    Some(at) if at <= now && offline => {}
                    Some(at) if at <= now => {
                        joined.due = None;
                        due.push((
                            joined.record.clone(),
                            joined.confirmed + joined.lease <= now,
                        ));
                    }
                    Some(at) => next = Some(next.map_or(at, |n| n.min(at))),
                    None => {}
                }
            }
            (due, next)
        };
        for (record, lost) in due {
            running.spawn(renew_one(sender.clone(), users.clone(), record, lost));
        }
        let sleep = async {
            match next {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            _ = sleep => {}
            _ = wake.notified() => {}
            Some(_) = running.join_next(), if !running.is_empty() => {}
        }
    }
}

/// Renew one user, or register it again if its lease was lost (D329), and say when it is due next.
async fn renew_one(sender: Sender, users: Arc<Mutex<Users>>, record: PresenceRecord, lost: bool) {
    let user = record.user;
    let started = Instant::now();
    let outcome = if lost {
        sender.log.log(Event::PresenceLost {
            user,
            reason: "lease ran out",
        });
        sender.register(&record).await.map(|lease| (lease, true))
    } else {
        match sender.renew(&record).await {
            Ok(Some(lease)) => Ok((lease, false)),
            Ok(None) => {
                sender.log.log(Event::PresenceLost {
                    user,
                    reason: "unknown to the directory",
                });
                sender.register(&record).await.map(|lease| (lease, true))
            }
            Err(e) => Err(e),
        }
    };
    let now = Instant::now();
    let mut users = lock(&users);
    // Left, or joined again, while this was in flight: that call has the last word.
    let Some(joined) = users.joined.get_mut(&user) else {
        return;
    };
    if joined.due.is_some() || joined.record != record {
        return;
    }
    match outcome {
        Ok((lease, again)) => {
            let latency_us = started.elapsed().as_micros() as u64;
            if again {
                sender.log.log(Event::PresenceRegistered {
                    user,
                    lease_ms: lease.as_millis() as u64,
                    latency_us,
                    again: true,
                });
            } else {
                sender.log.log(Event::PresenceRenewed { user, latency_us });
            }
            joined.lease = lease;
            joined.confirmed = now;
            joined.due = Some(now + renew_after(lease));
        }
        // Offline: wait for `back_online`, which makes it due at once.
        Err(PresenceError::Offline) => joined.due = Some(now + joined.lease),
        Err(_) => joined.due = Some(now + renew_after(joined.lease).min(RETRY_RENEW_AFTER)),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_renewal_comes_near_a_third_of_the_lease() {
        let lease = Duration::from_millis(9_000);
        for _ in 0..1_000 {
            let after = renew_after(lease);
            assert!(after >= Duration::from_millis(2_700), "{after:?}");
            assert!(after <= Duration::from_millis(3_300), "{after:?}");
        }
    }

    #[test]
    fn registered_and_unknown_are_answers_and_rejected_is_a_refusal() {
        assert!(matches!(
            registered(PresenceReply::Registered {
                lease_ms: 10,
                log_index: 1
            }),
            Verdict::Done(Some(d)) if d == Duration::from_millis(10)
        ));
        assert!(matches!(
            registered(PresenceReply::Unknown),
            Verdict::Done(None)
        ));
        assert!(matches!(
            registered(PresenceReply::Rejected("no".into())),
            Verdict::Refused(r) if r == "no"
        ));
    }
}
