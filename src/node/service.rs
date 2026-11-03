//! The incident service: the application layer of a cloud node (D156, D211).
//!
//! A client request goes: received → already decided? reply from state → the middleware picks the
//! executor → executor plans on the worker pool → commits `CommitResult` through the group →
//! replies with the committed (winning) result. A node never replies before the commit, with one
//! exception: a node left alone answers provisionally (degraded.rs, D120). It never replies at all
//! while it is not serving, whatever it was waiting on when it stopped (D136). A report too old to
//! plan takes the same path with nobody on the plan, and its reply says `Expired` (D143).
//!
//! **The boundary.** Whatever this file needs from the layer below comes through
//! `src/middleware.rs`. It never names Raft, a peer or a socket: who else is alive, how a winner
//! was chosen and how a result reached the other replicas are the middleware's business. A result
//! leaves by one call, `ctx.group.send_ordered`. `tests/layers.rs` fails if this stops being true.
//!
//! Two calls come up from below: `node/mod.rs` hands each request to `handle_request`, and degraded
//! mode, which has no commit to wait for, asks `plan_on_workers` for its plans.
//!
//! Owner: A (Raft and replication).

use std::sync::Arc;
use std::sync::atomic::Ordering;

use tokio::time::Instant;

use crate::metrics::{Event, now_ms};
use crate::middleware::{Decision, NodeContext, Overloaded, ReplyTx, decide_executor, degraded};
use crate::planner::{self, CostKnob};
use crate::proto::{
    ClientReply, ClientRequest, Command, CommittedResult, Outcome, Plan, ProposeError, ReplyBody,
    RequestId, Responder, ResultProposal,
};

/// Plans per request before the node answers that nobody could be sent (D220).
pub(super) const MAX_PLAN_ATTEMPTS: u32 = 3;

pub(super) async fn handle_request(ctx: Arc<NodeContext>, req: ClientRequest, reply: ReplyTx) {
    let started = Instant::now();
    ctx.log.log(Event::RequestReceived {
        request: req.id,
        attempt: req.attempt,
    });
    if !ctx.serving.load(Ordering::SeqCst) {
        return;
    }
    // A retry of a decided request gets the committed result from whichever node has it.
    if let Some(done) = ctx.state.with(|s| s.result(&req.id).cloned()) {
        send_done(&ctx, &reply, done, started);
        return;
    }
    // Waiting to be replayed: a retry gets the same answer, in or out of degraded mode.
    if let Some(plan) = ctx.degraded.stored(&req.id) {
        send_provisional(&ctx, &reply, req.id, plan, started);
        return;
    }
    // Before any bid or proposal: neither can finish on one node.
    if ctx.degraded.is_active() {
        match degraded::plan(&ctx, &req).await {
            Ok(plan) => send_provisional(&ctx, &reply, req.id, plan, started),
            Err(e) => tracing::debug!(request = %req.id, "not answered: {e}"),
        }
        return;
    }
    if decide_executor(&ctx, &req).await != Decision::Execute {
        return;
    }
    match execute(&ctx, &req).await {
        Ok(done) => send_done(&ctx, &reply, done, started),
        // No reply: the client retries with the same ID.
        Err(e) => tracing::debug!(request = %req.id, "not answered: {e}"),
    }
}

#[derive(Debug, thiserror::Error)]
enum ExecuteError {
    #[error(transparent)]
    Overloaded(#[from] Overloaded),
    #[error(transparent)]
    Propose(#[from] ProposeError),
    #[error("unexpected outcome {0}")]
    Unexpected(&'static str),
}

/// Plan on the worker pool and commit through Raft; plan again if the plan went stale, and after
/// [`MAX_PLAN_ATTEMPTS`] stale plans commit that nobody could be sent (D220).
async fn execute(ctx: &NodeContext, req: &ClientRequest) -> Result<CommittedResult, ExecuteError> {
    for _ in 0..MAX_PLAN_ATTEMPTS {
        let free = ctx.state.with(|s| s.free_responders());
        let plan = plan_on_workers(ctx, req, free).await?;
        let proposal = ResultProposal {
            request: req.id,
            incident: req.incident.clone(),
            executor: ctx.id,
            term: ctx.group.term(),
            plan,
            age_ms: req.age_ms,
            assigned_at_ms: now_ms(),
        };
        match ctx
            .group
            .send_ordered(Command::CommitResult(proposal))
            .await?
        {
            Outcome::Committed(done) | Outcome::Duplicate(done) => return Ok(done),
            Outcome::Stale { .. } => continue,
            other => return Err(ExecuteError::Unexpected(other.kind())),
        }
    }
    // Every plan lost the race for a responder. Say so now: silence would leave the client to its
    // retry, which only starts the race again. A plan with no responder cannot go stale, and with no
    // list to search and no load knob it needs no worker job.
    let age_ms = now_ms().saturating_sub(req.incident.reported_at_ms);
    let plan = planner::plan(&req.incident, &[], age_ms, CostKnob::default());
    let proposal = ResultProposal {
        request: req.id,
        incident: req.incident.clone(),
        executor: ctx.id,
        term: ctx.group.term(),
        plan,
        age_ms: req.age_ms,
        assigned_at_ms: now_ms(),
    };
    match ctx
        .group
        .send_ordered(Command::CommitResult(proposal))
        .await?
    {
        Outcome::Committed(done) | Outcome::Duplicate(done) => Ok(done),
        other => Err(ExecuteError::Unexpected(other.kind())),
    }
}

/// Plan one request from `free` on the worker pool.
pub(super) async fn plan_on_workers(
    ctx: &NodeContext,
    req: &ClientRequest,
    free: Vec<Responder>,
) -> Result<Plan, Overloaded> {
    // Too old means not planned (D143). Here, after the election, so one node commits it, and for
    // `execute` and `degraded::plan` alike. No worker job and no `record_service`, so a refusal
    // leaves the planning time this node bids with as it was.
    if req.incident.is_too_old(req.age_ms) {
        ctx.log.log(Event::ReportExpired {
            request: req.id,
            age_ms: req.age_ms,
            ttl_ms: req.incident.ttl_ms,
        });
        return Ok(planner::not_planned());
    }
    let incident = req.incident.clone();
    // Two machines' clocks: NTP error is negligible against the planner's threshold (D95). The
    // too-old rule above reads the client's stated age instead, because a wrong clock there would
    // turn a whole run into refusals with no error; here it can only change a fallback.
    let age_ms = now_ms().saturating_sub(incident.reported_at_ms);
    let knob = CostKnob {
        extra_work: ctx.plan_work.load(Ordering::Relaxed),
    };
    // Timed inside the job: queue wait is already counted by `queued`.
    let (plan, planning) = ctx
        .workers
        .run(move || {
            let started = std::time::Instant::now();
            (
                planner::plan(&incident, &free, age_ms, knob),
                started.elapsed(),
            )
        })
        .await
        .inspect_err(|_| {
            // The client hears nothing and retries, so only this tells shed work from slow work.
            let pool = ctx.workers.load();
            ctx.log.log(Event::Overloaded {
                request: req.id,
                attempt: req.attempt,
                queued: pool.queued,
                running: pool.running,
            });
        })?;
    ctx.bids.record_service(planning);
    Ok(plan)
}

/// Whether a reply may leave now. A handler can wait on a commit for a long time, and by then the
/// node may have been silenced and be back, still catching up: `serving` when the request arrived
/// says nothing about now (D136). The client retries, as with any node that stays quiet (D36).
fn may_reply(ctx: &NodeContext, request: RequestId) -> bool {
    let serving = ctx.serving.load(Ordering::SeqCst);
    if !serving {
        tracing::debug!(%request, "not answered: not serving");
    }
    serving
}

fn send_provisional(
    ctx: &NodeContext,
    reply: &ReplyTx,
    request: RequestId,
    plan: Plan,
    started: Instant,
) {
    if !may_reply(ctx, request) {
        return;
    }
    let responder = plan.responder;
    let sent = reply.send(ClientReply {
        id: request,
        from: ctx.id,
        body: ReplyBody::Provisional(plan),
    });
    if sent {
        ctx.log.log(Event::ProvisionalReplied {
            request,
            responder,
            latency_us: started.elapsed().as_micros() as u64,
        });
    }
}

/// Reply with a committed result. Which body it gets comes from the record alone, with no state and
/// no clock, so every reply for one ID is the same (D37).
fn send_done(ctx: &NodeContext, reply: &ReplyTx, done: CommittedResult, started: Instant) {
    let (request, executor) = (done.request, done.executor);
    if !may_reply(ctx, request) {
        return;
    }
    let body = if done.too_old_on_arrival() {
        ReplyBody::Expired(done)
    } else {
        ReplyBody::Done(done)
    };
    let sent = reply.send(ClientReply {
        id: request,
        from: ctx.id,
        body,
    });
    if sent {
        ctx.log.log(Event::Replied {
            request,
            executor,
            latency_us: started.elapsed().as_micros() as u64,
        });
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::oneshot;

    use super::*;
    use crate::config::AssignmentPolicy;
    use crate::metrics::EventLog;
    use crate::node::testing::{ScriptedConsensus, context, log_dir, logged, until};
    use crate::proto::{Capability, Category, Incident, Point, UserId};

    fn request() -> ClientRequest {
        ClientRequest {
            id: RequestId { client: 7, seq: 1 },
            attempt: 0,
            age_ms: 0,
            incident: Incident {
                owner: UserId { client: 7, user: 0 },
                category: Category::MedicalAssistance,
                zone: 0,
                location: Point { x: 1, y: 2 },
                severity: 3,
                needs: Capability::Medical,
                reported_at_ms: 0,
                ttl_ms: 60_000,
            },
        }
    }

    /// What has gone back to the call so far. A call takes one answer (D169).
    fn replies(answer: &mut oneshot::Receiver<ClientReply>) -> Vec<ClientReply> {
        answer.try_recv().into_iter().collect()
    }

    /// Runs one request on a leader that plans it itself, up to the moment it waits on the commit;
    /// `during_the_wait` then runs, the commit finishes and the handler ends.
    async fn wait_on_a_commit(
        ctx: &Arc<NodeContext>,
        fake: &ScriptedConsensus,
        during_the_wait: impl FnOnce(),
    ) -> Vec<ClientReply> {
        let (reply, mut frames) = ReplyTx::for_tests(ctx.gate.clone());
        let handler = tokio::spawn(handle_request(ctx.clone(), request(), reply));
        fake.proposal_arrived().await;
        during_the_wait();
        fake.commit_one();
        handler.await.unwrap();
        replies(&mut frames)
    }

    fn planning_leader(dir: &std::path::Path) -> (Arc<ScriptedConsensus>, Arc<NodeContext>) {
        let fake = ScriptedConsensus::new(1);
        fake.set_leader(Some(1));
        let log = EventLog::to_dir(dir, "node-1").unwrap();
        let ctx = context(fake.clone(), log, |c| {
            c.policy.assignment = AssignmentPolicy::LeaderDispatch;
        });
        (fake, ctx)
    }

    #[tokio::test]
    async fn a_serving_node_replies_once_the_commit_is_done() {
        let dir = log_dir("reply-while-serving");
        let (fake, ctx) = planning_leader(&dir);

        let replies = wait_on_a_commit(&ctx, &fake, || {}).await;

        assert!(matches!(replies[..], [ClientReply { from: 1, .. }]));
        let events = logged(&ctx.log, &dir);
        assert!(events.iter().any(|e| e["ev"] == "replied"));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The node was silenced with this request waiting on its commit, and is back, catching up. The
    /// commit is done, but the node has not yet caught up, so it must stay quiet (AGENTS.md).
    #[tokio::test]
    async fn a_commit_done_while_the_node_is_not_serving_gets_no_reply() {
        let dir = log_dir("reply-while-not-serving");
        let (fake, ctx) = planning_leader(&dir);

        let replies = wait_on_a_commit(&ctx, &fake, || {
            ctx.serving.store(false, Ordering::SeqCst);
        })
        .await;

        assert!(replies.is_empty(), "replied while not serving: {replies:?}");
        let events = logged(&ctx.log, &dir);
        assert!(events.iter().any(|e| e["ev"] == "request_received"));
        assert!(!events.iter().any(|e| e["ev"] == "replied"));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Runs one request on a state with free responders, whose first `stale` plans lose the race,
    /// and returns what went back to the client.
    async fn race(
        ctx: &Arc<NodeContext>,
        fake: &ScriptedConsensus,
        stale: usize,
    ) -> Vec<ClientReply> {
        let responders = (1..=2)
            .map(|id| Responder {
                id,
                zone: 0,
                location: Point { x: 0, y: 0 },
                capability: Capability::Medical,
            })
            .collect();
        ctx.state
            .apply_for_tests(Command::RegisterResponders { responders });
        fake.make_stale(stale);
        let (reply, mut answer) = ReplyTx::for_tests(ctx.gate.clone());
        let handler = tokio::spawn(handle_request(ctx.clone(), request(), reply));
        for _ in 0..=stale {
            fake.proposal_arrived().await;
            fake.commit_one();
        }
        handler.await.unwrap();
        replies(&mut answer)
    }

    /// The client hears at once, and not after its retry wait, which would start the race again.
    #[tokio::test]
    async fn three_stale_plans_are_answered_with_nobody_sent() {
        let dir = log_dir("three-stale-plans");
        let (fake, ctx) = planning_leader(&dir);

        let replies = race(&ctx, &fake, MAX_PLAN_ATTEMPTS as usize).await;

        let plans = fake.proposed_plans();
        assert_eq!(plans.len(), 4, "{plans:?}");
        assert!(
            plans[..3].iter().all(|p| p.responder.is_some()),
            "{plans:?}"
        );
        assert_eq!(plans[3].responder, None, "{plans:?}");
        let [
            ClientReply {
                body: ReplyBody::Done(done),
                ..
            },
        ] = &replies[..]
        else {
            panic!("expected one answer, got {replies:?}");
        };
        assert_eq!(done.plan.responder, None);
        let events = logged(&ctx.log, &dir);
        assert!(events.iter().any(|e| e["ev"] == "replied"));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Nobody is only for when all three plans lost: a plan that wins on the last try keeps its
    /// responder, and no fourth proposal follows.
    #[tokio::test]
    async fn a_plan_that_wins_on_its_last_try_keeps_its_responder() {
        let dir = log_dir("stale-then-won");
        let (fake, ctx) = planning_leader(&dir);

        let replies = race(&ctx, &fake, MAX_PLAN_ATTEMPTS as usize - 1).await;

        let plans = fake.proposed_plans();
        assert_eq!(plans.len(), 3, "{plans:?}");
        let [
            ClientReply {
                body: ReplyBody::Done(done),
                ..
            },
        ] = &replies[..]
        else {
            panic!("expected one answer, got {replies:?}");
        };
        assert!(done.plan.responder.is_some(), "{done:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_refused_planning_job_is_logged() {
        let dir = log_dir("overloaded");
        let log = EventLog::to_dir(&dir, "node-1").unwrap();
        let ctx = context(ScriptedConsensus::new(1), log, |c| {
            c.workers.threads = 1;
            c.workers.queue_capacity = 0;
        });
        // Keep the only worker busy until this test lets it go.
        let (let_go, hold) = std::sync::mpsc::channel::<()>();
        let busy = {
            let workers = ctx.workers.clone();
            tokio::spawn(async move { workers.run(move || hold.recv()).await })
        };
        until("the worker to be busy", || ctx.workers.load().running == 1).await;

        let refused = plan_on_workers(&ctx, &request(), Vec::new()).await;

        assert!(refused.is_err());
        let _ = let_go.send(());
        busy.await.unwrap().unwrap().unwrap();
        let events = logged(&ctx.log, &dir);
        let event = events.iter().find(|e| e["ev"] == "overloaded").unwrap();
        assert_eq!(event["request"]["seq"], 1);
        assert_eq!(event["attempt"], 0);
        assert_eq!(event["queued"], 0);
        assert_eq!(event["running"], 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn an_accepted_planning_job_logs_no_refusal() {
        let dir = log_dir("not-overloaded");
        let log = EventLog::to_dir(&dir, "node-1").unwrap();
        let ctx = context(ScriptedConsensus::new(1), log, |_| {});

        assert!(plan_on_workers(&ctx, &request(), Vec::new()).await.is_ok());

        let events = logged(&ctx.log, &dir);
        assert!(!events.iter().any(|e| e["ev"] == "overloaded"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_provisional_answer_also_needs_a_serving_node() {
        let dir = log_dir("provisional-while-not-serving");
        let (_, ctx) = planning_leader(&dir);
        let (reply, mut frames) = ReplyTx::for_tests(ctx.gate.clone());
        let plan = Plan {
            responder: None,
            cost: 1,
            priority: 300,
        };
        let send = || send_provisional(&ctx, &reply, request().id, plan.clone(), Instant::now());

        ctx.serving.store(false, Ordering::SeqCst);
        send();
        assert!(replies(&mut frames).is_empty());
        assert!(
            !logged(&ctx.log, &dir)
                .iter()
                .any(|e| e["ev"] == "provisional_replied")
        );

        ctx.serving.store(true, Ordering::SeqCst);
        send();
        assert_eq!(replies(&mut frames).len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
