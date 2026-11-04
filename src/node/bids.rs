//! Drives bid rounds on a running node: bids a load score when a request arrives, sends it to the
//! peers, waits out the window by the clock and reports whether this node won. The rules
//! themselves are in `bidding.rs`, which stays pure.
//!
//! Owner: B (bidding election).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::task::JoinHandle;

use super::{NodeContext, cpu, lock};
use crate::bidding::{BidRounds, BidStatus, LoadInputs, ServiceTime, load_score};
use crate::config::{AssignmentPolicy, Config};
use crate::metrics::Event;
use crate::proto::{Bid, ClientRequest, NodeId, PeerBody, RequestId};

/// How long a bid waits for others to go out with it (D247). It uses part of the bid window.
const BID_COMBINE_WAIT: Duration = Duration::from_millis(5);

/// How often closed rounds past their grace period are dropped.
const CLEANUP_INTERVAL: Duration = Duration::from_secs(1);

/// One node's soft bidding state. A lock is never held across an `.await`.
pub struct BidState {
    rounds: Mutex<BidRounds>,
    service: Mutex<ServiceTime>,
    cpu_permille: AtomicU32,
    /// Each node's bids since the last reading, this node's included: their sum and count (D218).
    seen: Mutex<BTreeMap<NodeId, (u64, u64)>>,
    /// Requests this node is planning and committing now. A retry joins them, not repeats them (D243).
    executing: Mutex<BTreeSet<RequestId>>,
    /// Bids waiting to go to the group together (D247).
    outgoing: Arc<Mutex<Vec<Bid>>>,
    in_progress: AtomicU32,
    refused: AtomicU64,
}

/// Reports one node works on at once (D250). At 10,000 a second with about 200 ms each, a node holds
/// about 2,000 when healthy.
pub(super) const MAX_IN_PROGRESS: u32 = 8000;

/// Held while a report is being worked on here.
pub(super) struct InProgress<'a> {
    state: &'a BidState,
}

impl Drop for InProgress<'_> {
    fn drop(&mut self) {
        self.state.in_progress.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Held while this node works on one request. Dropping it ends that.
pub(super) struct Executing<'a> {
    state: &'a BidState,
    request: RequestId,
}

impl Drop for Executing<'_> {
    fn drop(&mut self) {
        lock(&self.state.executing).remove(&self.request);
    }
}

impl BidState {
    pub fn new(id: NodeId, config: &Config) -> Self {
        Self {
            rounds: Mutex::new(BidRounds::new(id, config.timers.bid_window())),
            service: Mutex::new(ServiceTime::new(config.policy.initial_service_us)),
            cpu_permille: AtomicU32::new(0),
            seen: Mutex::new(BTreeMap::new()),
            executing: Mutex::new(BTreeSet::new()),
            outgoing: Arc::default(),
            in_progress: AtomicU32::new(0),
            refused: AtomicU64::new(0),
        }
    }

    /// Counts a report being worked on here, or `None` if there are already [`MAX_IN_PROGRESS`], so
    /// an overloaded node stops adding bids, plans and log commands to what it cannot finish. A
    /// report refused here gets no answer, and the client's retry covers it, as for a full worker
    /// pool (D48, D250).
    pub(super) fn admit(&self) -> Option<InProgress<'_>> {
        let before = self.in_progress.fetch_add(1, Ordering::Relaxed);
        if before >= MAX_IN_PROGRESS {
            self.in_progress.fetch_sub(1, Ordering::Relaxed);
            self.refused.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        Some(InProgress { state: self })
    }

    /// Reports refused at the door so far.
    pub fn refused(&self) -> u64 {
        self.refused.load(Ordering::Relaxed)
    }

    /// `None` if this node is already working on this request: the first attempt's call is still open,
    /// and its answer serves the client.
    pub(super) fn begin_executing(&self, request: RequestId) -> Option<Executing<'_>> {
        // The lock ends first. Dropping an `Executing` takes it again, so one must never be made and
        // dropped while it is held.
        let first = lock(&self.executing).insert(request);
        // `then`, not `then_some`: a refused guard must never exist, as dropping it would end the work's mark.
        first.then(|| Executing {
            state: self,
            request,
        })
    }

    /// Adds one job's planning time to the recent average.
    pub fn record_service(&self, planning: Duration) {
        lock(&self.service).record(planning.as_micros() as u64);
    }

    pub(super) fn set_cpu_permille(&self, permille: u32) {
        self.cpu_permille.store(permille, Ordering::Relaxed);
    }

    /// Reads the rounds under the lock, for takeover and tests.
    pub fn with_rounds<T>(&self, f: impl FnOnce(&BidRounds) -> T) -> T {
        f(&lock(&self.rounds))
    }

    pub(super) fn saw_bid(&self, node: NodeId, score: u64) {
        let mut seen = lock(&self.seen);
        let (sum, count) = seen.entry(node).or_default();
        *sum = sum.saturating_add(score);
        *count += 1;
    }

    /// Each node's mean bid since the last call, and starts the next reading. A node that got no
    /// requests, or does not serve, made no bids, so it is simply not here.
    pub fn take_mean_scores(&self) -> BTreeMap<NodeId, u64> {
        std::mem::take(&mut *lock(&self.seen))
            .into_iter()
            .map(|(node, (sum, count))| (node, sum / count.max(1)))
            .collect()
    }
}

/// Runs this node's part of a round and returns whether it should plan the request. Called once
/// per request arrival; a repeat of an attempt that is already running or closed returns false.
pub async fn run_round(ctx: &NodeContext, req: &ClientRequest) -> bool {
    let key = (req.id, req.attempt);
    let policy = ctx.config.policy;
    let opened = Instant::now();
    let pool = ctx.workers.load();

    // The count is read before this node's own bid goes in, so a round does not count itself.
    let (deadline, just_won) = {
        let mut rounds = lock(&ctx.bids.rounds);
        (rounds.open(key, opened), rounds.just_won(policy.just_won))
    };
    let Some(deadline) = deadline else {
        return false;
    };
    let service_us = lock(&ctx.bids.service).average_us();
    let cpu_permille = ctx.bids.cpu_permille.load(Ordering::Relaxed);
    let inputs = LoadInputs {
        queued: pool.queued,
        running: pool.running,
        just_won,
        recent_service_us: service_us,
        workers: pool.threads,
        cpu_permille,
    };
    let score = load_score(&inputs, policy.score_variant());
    ctx.bids.saw_bid(ctx.id, score);
    let status = lock(&ctx.bids.rounds).add_bid(key, ctx.id, score, opened);
    if status != BidStatus::Accepted {
        return false;
    }
    ctx.log.log(Event::BidSent {
        request: req.id,
        attempt: req.attempt,
        score,
        queued: pool.queued,
        running: pool.running,
        just_won,
        service_us,
        cpu_permille,
    });
    send_bid(
        ctx,
        Bid {
            request: req.id,
            attempt: req.attempt,
            score,
        },
    );

    tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await;

    let now = Instant::now();
    let Some(closed) = lock(&ctx.bids.rounds).close(key, now) else {
        return false;
    };
    ctx.log.log(Event::BidRoundClosed {
        request: req.id,
        attempt: req.attempt,
        winner: closed.ranking.first().copied(),
        i_won: closed.i_won,
        bids_seen: closed.bids_seen,
        my_score: closed.my_score,
        window_us: now.saturating_duration_since(opened).as_micros() as u64,
    });
    if !closed.i_won {
        return false;
    }
    if ctx.state.with(|s| s.result(&req.id).is_some()) {
        lock(&ctx.bids.rounds).request_decided(req.id, now);
        return false;
    }
    // The caller hands the job to the worker pool right after this, where `queued` takes over.
    lock(&ctx.bids.rounds).job_queued(key);
    true
}

/// A peer's bid arrived. It may come before the request; the round then waits for it.
pub fn on_bid(ctx: &NodeContext, from: NodeId, bid: &Bid) {
    ctx.bids.saw_bid(from, bid.score);
    lock(&ctx.bids.rounds).add_bid((bid.request, bid.attempt), from, bid.score, Instant::now());
}

/// Bids a peer sent together: one lock for all of them (D247).
pub fn on_bids(ctx: &NodeContext, from: NodeId, bids: &[Bid]) {
    for bid in bids {
        ctx.bids.saw_bid(from, bid.score);
    }
    let now = Instant::now();
    let mut rounds = lock(&ctx.bids.rounds);
    for bid in bids {
        rounds.add_bid((bid.request, bid.attempt), from, bid.score, now);
    }
}

/// Sends the bid to the group without waiting: the window runs by the clock, so a slow peer must
/// not hold it up. A lost bid can only cause a split decision, which the first commit settles, and
/// a peer this node has listed as gone gets no copy at all (D154). The task ends within the peer
/// RPC timeout, so it needs no owner.
///
/// The first bid to wait starts the task, and bids made meanwhile go with it, so an idle node runs no
/// timer and a busy one sends one message per peer per [`BID_COMBINE_WAIT`] (D247).
fn send_bid(ctx: &NodeContext, bid: Bid) {
    let first = {
        let mut waiting = lock(&ctx.bids.outgoing);
        waiting.push(bid);
        waiting.len() == 1
    };
    if !first {
        return;
    }
    let timeout = ctx.config.timers.peer_rpc_timeout();
    let group = ctx.group.clone();
    let outgoing = ctx.bids.outgoing.clone();
    tokio::spawn(async move {
        tokio::time::sleep(BID_COMBINE_WAIT).await;
        let batch = std::mem::take(&mut *lock(&outgoing));
        let sent = group.send(timeout, || PeerBody::Bids(batch.clone()));
        for (peer, outcome) in sent.await {
            if let Err(e) = outcome {
                tracing::debug!(peer, "bid not delivered: {e}");
            }
        }
    });
}

/// Starts the round cleanup and the CPU sampler. Neither is needed under leader dispatch.
pub fn spawn(ctx: &Arc<NodeContext>) -> Vec<JoinHandle<()>> {
    if ctx.config.policy.assignment != AssignmentPolicy::Bidding {
        return Vec::new();
    }
    vec![spawn_cleanup(ctx.clone()), cpu::spawn(ctx.clone())]
}

/// Closed rounds are kept for one client retry interval, so that late bids do nothing.
fn spawn_cleanup(ctx: Arc<NodeContext>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let grace = ctx.config.timers.client_retry();
        let mut tick = tokio::time::interval(CLEANUP_INTERVAL);
        loop {
            tick.tick().await;
            lock(&ctx.bids.rounds).forget_older_than(Instant::now(), grace);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_retry_during_execution_is_refused_and_a_later_one_is_not() {
        let state = BidState::new(
            1,
            &crate::config::Config::from_toml(crate::node::testing::THREE_NODES).unwrap(),
        );
        let request = RequestId { client: 1, seq: 1 };
        let first = state.begin_executing(request);
        assert!(first.is_some());
        // This must return. Making and dropping a refused guard under the lock would hang here.
        assert!(state.begin_executing(request).is_none());
        // And being refused must not end the first attempt's mark.
        assert!(state.begin_executing(request).is_none());
        drop(first);
        assert!(state.begin_executing(request).is_some());
    }
}
