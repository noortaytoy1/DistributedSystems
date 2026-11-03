//! Degraded mode: the last server left answers clients without a commit.
//!
//! Raft cannot commit on one node, so with both peers gone a plain node would stay quiet until the
//! client gave up. Dr Amr's case is one machine off and one failed, with the remaining server
//! handling the clients, so a lone node answers `ReplyBody::Provisional` instead. This is the one
//! exception to "Commit, then reply" (D120): the answer can later turn out to differ, and in a real
//! partition two sides can answer differently.
//!
//! - **Entering (D121).** Every peer suspected without a break for `timers.degraded_after_ms`, this
//!   node serving and not silenced. It leaves as soon as any peer is trusted again, or when it is
//!   silenced.
//! - **Holds (D124).** Releases need a quorum, so the replicated holds cannot change here. On
//!   entering, a local table takes them over, and each one is freed after its service time, as is
//!   each responder a provisional plan takes.
//! - **Retries.** The table keeps each answer until it is settled, so a retry gets the same plan.
//! - **Leaving (D122).** Each stored result goes through Raft as `CommitResult`, retried until a
//!   quorum commits it. First commit wins and a loser logs `provisional_conflict`. If the responder
//!   is taken, the plan is recorded with no responder. Clients are not told.
//!
//! The table is memory only: a lone node that loses power also loses what it has not replayed.
//!
//! Owner: C (faults and failover).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior};

use super::workers::Overloaded;
use super::{MAX_PLAN_ATTEMPTS, NodeContext, lock, plan_on_workers};
use crate::metrics::{Event, now_ms};
use crate::proto::{
    ClientRequest, Command, Incident, Outcome, Plan, RequestId, Responder, ResponderId,
    ResultProposal,
};

/// How often stored results are tried when there are some to replay.
const REPLAY_INTERVAL: Duration = Duration::from_millis(100);
/// Results proposed at once. Raft batches concurrent proposals, so a long outage replays quickly.
const REPLAY_BATCH: usize = 64;

/// One answer given without a commit. It keeps its stamps, so the replay records when the client
/// was told, not when a quorum came back (D144).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Provisional {
    incident: Incident,
    plan: Plan,
    age_ms: u64,
    assigned_at_ms: u64,
}

/// What a finished plan turned into.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Stored {
    /// The answer for this request: this plan, or the one a concurrent attempt stored first.
    Answer(Plan),
    /// The plan's responder was taken while it was planned. Plan again.
    Taken,
}

/// The local table of answers and holds. Takes the time as an argument so it is easy to test.
#[derive(Debug, Default)]
struct Table {
    pending: BTreeMap<RequestId, Provisional>,
    /// When each locally held responder is free again.
    holds: BTreeMap<ResponderId, Instant>,
}

impl Table {
    fn store(
        &mut self,
        request: RequestId,
        answer: Provisional,
        hold: Duration,
        now: Instant,
    ) -> Stored {
        if let Some(existing) = self.pending.get(&request) {
            return Stored::Answer(existing.plan.clone());
        }
        if let Some(responder) = answer.plan.responder {
            if self.holds.get(&responder).is_some_and(|until| *until > now) {
                return Stored::Taken;
            }
            self.holds.insert(responder, now + hold);
        }
        let plan = answer.plan.clone();
        self.pending.insert(request, answer);
        Stored::Answer(plan)
    }

    /// `all` without the responders still held at `now`.
    fn free(&mut self, all: Vec<Responder>, now: Instant) -> Vec<Responder> {
        self.holds.retain(|_, until| *until > now);
        all.into_iter()
            .filter(|r| !self.holds.contains_key(&r.id))
            .collect()
    }
}

/// When to be degraded: the cut-off condition must hold without a break for the whole wait.
#[derive(Debug, Default)]
struct Trigger {
    since: Option<Instant>,
}

impl Trigger {
    fn observe(&mut self, cut_off: bool, wait: Duration, now: Instant) -> bool {
        if !cut_off {
            self.since = None;
            return false;
        }
        let since = *self.since.get_or_insert(now);
        now.saturating_duration_since(since) >= wait
    }
}

/// One node's degraded mode. Nothing here is replicated.
#[derive(Debug, Default)]
pub struct DegradedState {
    active: AtomicBool,
    table: Mutex<Table>,
}

impl DegradedState {
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    /// The answer already given for `request`, while it waits to be replayed.
    pub fn stored(&self, request: &RequestId) -> Option<Plan> {
        lock(&self.table)
            .pending
            .get(request)
            .map(|p| p.plan.clone())
    }

    /// Answers not yet settled through Raft.
    pub fn pending(&self) -> usize {
        lock(&self.table).pending.len()
    }
}

fn service_time(ctx: &NodeContext, incident: &Incident) -> Duration {
    Duration::from_millis(u64::from(incident.severity) * ctx.config.campus.service_ms_per_severity)
}

#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error(transparent)]
    Overloaded(#[from] Overloaded),
    #[error("responders kept being taken after {MAX_PLAN_ATTEMPTS} attempts")]
    StillTaken,
}

/// Plan `req` from this node's own view and store the answer.
pub async fn plan(ctx: &NodeContext, req: &ClientRequest) -> Result<Plan, PlanError> {
    for _ in 0..MAX_PLAN_ATTEMPTS {
        let all = ctx.state.with(|s| s.responders());
        let free = lock(&ctx.degraded.table).free(all, Instant::now());
        let plan = plan_on_workers(ctx, req, free).await?;
        let hold = service_time(ctx, &req.incident);
        let answer = Provisional {
            incident: req.incident.clone(),
            plan,
            age_ms: req.age_ms,
            assigned_at_ms: now_ms(),
        };
        let stored = lock(&ctx.degraded.table).store(req.id, answer, hold, Instant::now());
        if let Stored::Answer(plan) = stored {
            return Ok(plan);
        }
    }
    Err(PlanError::StillTaken)
}

/// Start watching for the node being left alone, and replaying what it answered.
pub fn spawn(ctx: &Arc<NodeContext>) -> Vec<JoinHandle<()>> {
    vec![
        tokio::spawn(watch(ctx.clone())),
        tokio::spawn(replay(ctx.clone())),
    ]
}

async fn watch(ctx: Arc<NodeContext>) {
    let peers = ctx.peers.iter().count();
    let wait = ctx.config.timers.degraded_after();
    let mut trigger = Trigger::default();
    let mut entered = Instant::now();
    let mut tick = tokio::time::interval(ctx.config.timers.peer_heartbeat());
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        let silenced = ctx.gate.is_silenced();
        let serving = ctx.serving.load(Ordering::SeqCst);
        let suspected = lock(&ctx.suspicion).suspected().len();
        let cut_off = peers > 0 && suspected == peers && serving && !silenced;

        let now = Instant::now();
        let active = trigger.observe(cut_off, wait, now);
        if active == ctx.degraded.is_active() {
            continue;
        }
        if active {
            entered = now;
            enter(&ctx, now);
        } else {
            leave(
                &ctx,
                silenced,
                serving,
                now.saturating_duration_since(entered),
            );
        }
    }
}

fn enter(ctx: &NodeContext, now: Instant) {
    let ms_per_severity = ctx.config.campus.service_ms_per_severity;
    let holds: Vec<(ResponderId, u64)> = ctx.state.with(|s| {
        s.holds()
            .into_iter()
            .map(|(id, result)| (id, u64::from(result.incident.severity)))
            .collect()
    });
    {
        let mut table = lock(&ctx.degraded.table);
        for (id, severity) in &holds {
            let hold = Duration::from_millis(severity * ms_per_severity);
            table.holds.insert(*id, now + hold);
        }
    }
    ctx.degraded.active.store(true, Ordering::SeqCst);
    tracing::warn!(node = ctx.id, "degraded: answering without a commit");
    ctx.log.log(Event::DegradedEntered {
        held_responders: holds.len(),
    });
}

fn leave(ctx: &NodeContext, silenced: bool, serving: bool, degraded_for: Duration) {
    ctx.degraded.active.store(false, Ordering::SeqCst);
    lock(&ctx.degraded.table).holds.clear();
    let reason = match (silenced, serving) {
        (true, _) => "silenced",
        (false, false) => "not_serving",
        (false, true) => "peer_trusted",
    };
    let provisional = ctx.degraded.pending();
    tracing::info!(node = ctx.id, reason, provisional, "degraded mode over");
    ctx.log.log(Event::DegradedLeft {
        reason: reason.to_string(),
        degraded_ms: degraded_for.as_millis() as u64,
        provisional,
    });
}

/// Replays stored answers whenever the node is back in the group and may serve.
async fn replay(ctx: Arc<NodeContext>) {
    let mut tick = tokio::time::interval(REPLAY_INTERVAL);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        if ctx.degraded.is_active() || !ctx.serving.load(Ordering::SeqCst) {
            continue;
        }
        let batch: Vec<(RequestId, Provisional)> = lock(&ctx.degraded.table)
            .pending
            .iter()
            .take(REPLAY_BATCH)
            .map(|(id, p)| (*id, p.clone()))
            .collect();
        let settling = batch.into_iter().map(|(id, p)| settle(&ctx, id, p));
        futures::future::join_all(settling).await;
    }
}

/// One try at getting a stored answer committed. It stays in the table if no quorum took it.
async fn settle(ctx: &NodeContext, request: RequestId, provisional: Provisional) {
    let Provisional {
        incident,
        mut plan,
        age_ms,
        assigned_at_ms,
    } = provisional;
    // At most two rounds: a plan with no responder cannot go stale.
    for _ in 0..2 {
        let proposal = ResultProposal {
            request,
            incident: incident.clone(),
            executor: ctx.id,
            term: ctx.consensus.status().term,
            plan: plan.clone(),
            age_ms,
            assigned_at_ms,
        };
        let outcome = match ctx.consensus.propose(Command::CommitResult(proposal)).await {
            Ok(outcome) => outcome,
            Err(e) => {
                tracing::debug!(%request, "provisional result not committed yet: {e}");
                return;
            }
        };
        match outcome {
            Outcome::Committed(_) => break,
            Outcome::Duplicate(winner) => {
                if winner.plan != plan {
                    ctx.log.log(Event::ProvisionalConflict {
                        request,
                        reason: "lost_first_commit".to_string(),
                        responder: plan.responder,
                        winner_responder: winner.plan.responder,
                        winner_executor: Some(winner.executor),
                    });
                }
                break;
            }
            Outcome::Stale { .. } => {
                ctx.log.log(Event::ProvisionalConflict {
                    request,
                    reason: "responder_taken".to_string(),
                    responder: plan.responder,
                    winner_responder: None,
                    winner_executor: None,
                });
                plan.responder = None;
                // A retry while this commits gets the plan that will be on record.
                if let Some(entry) = lock(&ctx.degraded.table).pending.get_mut(&request) {
                    entry.plan = plan.clone();
                }
            }
            Outcome::Applied => {
                tracing::warn!(%request, "unexpected outcome for a provisional result");
                break;
            }
        }
    }
    lock(&ctx.degraded.table).pending.remove(&request);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{Capability, Category, Point, UserId};

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn rid(seq: u64) -> RequestId {
        RequestId { client: 1, seq }
    }

    fn incident() -> Incident {
        Incident {
            owner: UserId { client: 1, user: 0 },
            category: Category::MedicalAssistance,
            zone: 0,
            location: Point { x: 0, y: 0 },
            severity: 1,
            needs: Capability::Medical,
            reported_at_ms: 0,
            ttl_ms: 60_000,
        }
    }

    fn plan_with(responder: Option<ResponderId>) -> Plan {
        Plan { responder, cost: 5 }
    }

    fn responder(id: ResponderId) -> Responder {
        Responder {
            id,
            zone: 0,
            location: Point { x: 0, y: 0 },
            capability: Capability::Medical,
        }
    }

    fn answer(plan: Plan) -> Provisional {
        Provisional {
            incident: incident(),
            plan,
            age_ms: 0,
            assigned_at_ms: 0,
        }
    }

    #[test]
    fn a_stored_answer_keeps_its_age_and_time() {
        let mut table = Table::default();
        let stamped = Provisional {
            age_ms: 2_500,
            assigned_at_ms: 9_000,
            ..answer(plan_with(Some(1)))
        };
        table.store(rid(1), stamped.clone(), ms(200), Instant::now());
        assert_eq!(table.pending.get(&rid(1)), Some(&stamped));
    }

    #[test]
    fn degraded_only_after_the_whole_wait_without_a_break() {
        let t0 = Instant::now();
        let mut trigger = Trigger::default();
        assert!(!trigger.observe(true, ms(2000), t0));
        assert!(!trigger.observe(true, ms(2000), t0 + ms(1999)));
        assert!(trigger.observe(true, ms(2000), t0 + ms(2000)));
    }

    #[test]
    fn one_trusted_peer_ends_it_and_the_wait_starts_over() {
        let t0 = Instant::now();
        let mut trigger = Trigger::default();
        trigger.observe(true, ms(2000), t0);
        assert!(trigger.observe(true, ms(2000), t0 + ms(2000)));
        assert!(!trigger.observe(false, ms(2000), t0 + ms(2050)));
        assert!(!trigger.observe(true, ms(2000), t0 + ms(2100)));
        assert!(!trigger.observe(true, ms(2000), t0 + ms(4099)));
        assert!(trigger.observe(true, ms(2000), t0 + ms(4100)));
    }

    #[test]
    fn a_retry_gets_the_first_answer() {
        let t0 = Instant::now();
        let mut table = Table::default();
        let first = plan_with(Some(1));
        assert_eq!(
            table.store(rid(1), answer(first.clone()), ms(200), t0),
            Stored::Answer(first.clone())
        );
        assert_eq!(
            table.store(rid(1), answer(plan_with(Some(2))), ms(200), t0),
            Stored::Answer(first)
        );
        assert_eq!(table.pending.len(), 1);
        assert!(!table.holds.contains_key(&2));
    }

    #[test]
    fn a_held_responder_is_not_given_twice_until_its_time_is_up() {
        let t0 = Instant::now();
        let mut table = Table::default();
        table.store(rid(1), answer(plan_with(Some(1))), ms(200), t0);
        assert_eq!(
            table.store(rid(2), answer(plan_with(Some(1))), ms(200), t0 + ms(199)),
            Stored::Taken
        );
        let ids = |free: Vec<Responder>| free.iter().map(|r| r.id).collect::<Vec<_>>();
        let all = vec![responder(1), responder(2)];
        assert_eq!(ids(table.free(all.clone(), t0 + ms(199))), vec![2]);
        assert_eq!(ids(table.free(all, t0 + ms(200))), vec![1, 2]);
        assert_eq!(
            table.store(rid(2), answer(plan_with(Some(1))), ms(200), t0 + ms(200)),
            Stored::Answer(plan_with(Some(1)))
        );
    }

    #[test]
    fn a_plan_with_no_responder_holds_nothing() {
        let mut table = Table::default();
        table.store(rid(1), answer(plan_with(None)), ms(200), Instant::now());
        assert!(table.holds.is_empty());
        assert_eq!(table.pending.len(), 1);
    }
}
