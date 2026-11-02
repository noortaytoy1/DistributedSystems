//! Drives bid rounds on a running node: bids a load score when a request arrives, sends it to the
//! peers, waits out the window by the clock and reports whether this node won. The rules
//! themselves are in `bidding.rs`, which stays pure.
//!
//! Owner: B (bidding election).

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use tokio::task::JoinHandle;

use super::{NodeContext, cpu, lock};
use crate::bidding::{BidRounds, BidStatus, LoadInputs, ServiceTime, load_score};
use crate::config::{AssignmentPolicy, Config};
use crate::metrics::Event;
use crate::proto::{Bid, ClientRequest, NodeId, PeerBody};

/// How often closed rounds past their grace period are dropped.
const CLEANUP_INTERVAL: Duration = Duration::from_secs(1);

/// One node's soft bidding state. A lock is never held across an `.await`.
pub struct BidState {
    rounds: Mutex<BidRounds>,
    service: Mutex<ServiceTime>,
    cpu_permille: AtomicU32,
}

impl BidState {
    pub fn new(id: NodeId, config: &Config) -> Self {
        Self {
            rounds: Mutex::new(BidRounds::new(id, config.timers.bid_window())),
            service: Mutex::new(ServiceTime::new(config.policy.initial_service_us)),
            cpu_permille: AtomicU32::new(0),
        }
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
    lock(&ctx.bids.rounds).add_bid((bid.request, bid.attempt), from, bid.score, Instant::now());
}

/// Sends the bid to the group without waiting: the window runs by the clock, so a slow peer must
/// not hold it up. A lost bid can only cause a split decision, which the first commit settles.
/// The task ends within the peer RPC timeout, so it needs no owner.
fn send_bid(ctx: &NodeContext, bid: Bid) {
    let term = ctx.consensus.status().term;
    let timeout = ctx.config.timers.peer_rpc_timeout();
    let peers = ctx.peers.clone();
    tokio::spawn(async move {
        let sent = peers.broadcast(term, timeout, || PeerBody::Bid(bid.clone()));
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
