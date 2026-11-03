//! Fault injection: silence a node for a while, by hand (`ctl silence`) or by a fault election.
//!
//! A silenced node drops all peer and client traffic (`FaultGate` in transport.rs) but keeps its
//! state, like a node cut off by the network. The spec allows failures of up to 20 s.
//!
//! One fault at a time per node: a silence order for a node that is already silenced is
//! rejected, never merged or restarted (D74). Durations of zero or above `MAX_FAULT` are
//! rejected too, so every `silenced`/`restored` pair in the logs is a real, in-spec fault.
//!
//! `faults.enabled = false` in the config (or `node --faults off`) turns all of this off: the node
//! never bids in an election and `silence_for` refuses every order, for no-fault baseline runs
//! (D77). One such node stops every election in the cluster, because none can finish without it.
//!
//! **The fault election (D158, D159).** The three nodes pick the machine that fails, by a random
//! number draw. Nobody orders anybody, the Raft leader has no part in it, and nothing goes through
//! the log. The first node whose random `faults.interval_*` timer runs out calls a round: it draws
//! a ticket and a silence and sends both to the group as `PeerBody::FaultBid`. A node that hears of
//! a round draws and sends its own at once. A node acts only when it holds a bid from every node in
//! the config: the highest ticket is elected (ties to the lower node ID), and the elected node
//! closes its own gate for the silence in its own bid. A node that is off, silenced, catching up or
//! degraded sends no bid, so no round finishes until it is back, and each node gives up on a round
//! after 2 x `timers.peer_rpc_timeout_ms`, logs that it did nothing, and waits again.
//!
//! **Why no second silence starts on this path.** There is no timer in the argument. Say X silences
//! itself for round `a` at time `t`, and Y closes round `b` after `t`, before X is back. Y needs X's
//! bid in `b`, which X sent before `t`. If `b = a`, both hold the same first bids, so Y names X and
//! stays up. If `b > a`, X replaced `a` when it bid in `b`, so X never closed `a`. If `b < a`, X
//! needed Y's bid in `a`, so Y had already left `b` and cannot close it. This holds for one driver
//! per node, one draw per round, and no restart inside a round. It says nothing about `ctl silence`,
//! or a machine that loses power just after sending its bid.
//!
//! Each node decides alone from what it holds, so the round is not agreement: a node that misses a
//! bid does nothing, and the others can still close (D159). `fault_election` lines show it.
//!
//! Owner: C (faults and failover).

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::lock;
use crate::config::{Config, FaultConfig};
use crate::metrics::Event;
use crate::node::NodeContext;
use crate::proto::{FaultBid, NodeId, PeerBody, PeerReply};

/// Longest fault the system must tolerate transparently.
pub const MAX_FAULT: Duration = Duration::from_secs(20);

/// Tickets run from 0 up to this: small enough to read on screen in a demo, large enough that a tie
/// is a few in a million, and a tie has a rule.
const TICKETS: u64 = 1_000_000;

// Odd constants that spread a small round or node number over all 64 bits of a seed.
const ROUND_MIX: u64 = 0x9E37_79B9_7F4A_7C15;
const NODE_MIX: u64 = 0xC2B2_AE3D_27D4_EB4F;
const WAIT_MIX: u64 = 0x1656_67B1_9E37_79F9;

/// Silence this node now and restore it after `duration` (`ctl silence`).
pub fn silence_for(ctx: &Arc<NodeContext>, duration: Duration) -> Result<(), String> {
    if !ctx.config.faults.enabled {
        return Err("fault injection is disabled on this node (faults.enabled = false)".into());
    }
    check_duration(duration)?;
    if !ctx.gate.try_silence() {
        return Err("already silenced; wait for the current fault to end".into());
    }
    let ms = duration.as_millis() as u64;
    ctx.log.log(Event::Silenced { ms });
    tracing::info!(node = ctx.id, ms, "silenced");

    // The task ends after one sleep, so it needs no handle. If the node shuts down first, the
    // runtime drops it with everything else.
    let ctx = ctx.clone();
    tokio::spawn(async move {
        tokio::time::sleep(duration).await;
        ctx.gate.restore();
        ctx.log.log(Event::Restored);
        tracing::info!(node = ctx.id, "restored");
    });
    Ok(())
}

/// A fault must last at least 1 ms and at most `MAX_FAULT`.
fn check_duration(duration: Duration) -> Result<(), String> {
    if duration.is_zero() {
        return Err("silence must last at least 1 ms".into());
    }
    if duration > MAX_FAULT {
        return Err(format!(
            "silence of {} ms is above the {} ms limit",
            duration.as_millis(),
            MAX_FAULT.as_millis()
        ));
    }
    Ok(())
}

/// This node was elected. `silence_for` looks only at the switch, the duration and the gate, so a
/// short `ctl silence` could otherwise stack a fault on an unfinished catch-up (D159).
pub fn on_elected(ctx: &Arc<NodeContext>, duration: Duration) -> Result<(), String> {
    if !ctx.serving.load(Ordering::SeqCst) {
        return Err("recovering: not serving yet".into());
    }
    silence_for(ctx, duration)
}

/// This node's numbers for one round. Pure: the same seed, round and node always give the same
/// numbers, however the round was timed, and a node can never bid two different numbers in a round.
pub fn draw(seed: u64, round: u64, id: NodeId, cfg: &FaultConfig) -> FaultBid {
    let mix = seed ^ round.wrapping_mul(ROUND_MIX) ^ id.wrapping_mul(NODE_MIX);
    let mut rng = StdRng::seed_from_u64(mix);
    FaultBid {
        round,
        ticket: rng.gen_range(0..TICKETS),
        ms: between(&mut rng, cfg.silence()).as_millis() as u64,
    }
}

fn between(rng: &mut impl Rng, (lo, hi): (Duration, Duration)) -> Duration {
    Duration::from_millis(rng.gen_range(lo.as_millis() as u64..=hi.as_millis() as u64))
}

/// Who a complete set of bids elects, and for how long. Highest ticket; ties to the lower node ID;
/// the silence is the winner's own, so all three name the same length with no extra rule.
fn elect(bids: &BTreeMap<NodeId, FaultBid>) -> Option<Elected> {
    bids.iter()
        .max_by_key(|&(id, bid)| (bid.ticket, Reverse(*id)))
        .map(|(id, bid)| Elected {
            victim: *id,
            ms: bid.ms,
        })
}

/// The node a round elected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Elected {
    pub victim: NodeId,
    pub ms: u64,
}

/// How a round ended on this node: what its `fault_election` line says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoundEnd {
    pub round: u64,
    /// The numbers this node held.
    pub tickets: BTreeMap<NodeId, u64>,
    /// `None`: this node never held every number, so it did nothing.
    pub elected: Option<Elected>,
    /// This node's timer started the round.
    pub called: bool,
    /// From this node's own bid to the last bid it needed.
    pub election: Option<Duration>,
}

/// What the driver does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Nothing until this time, or until a bid arrives.
    Sleep(Instant),
    /// The timer ran out while this node may not take part: pick a new wait, so the schedule
    /// neither spins nor stops for good.
    Rearm,
    /// This bid is recorded. Send it to the group, then call [`FaultRounds::send_finished`].
    Send(FaultBid),
    /// A round ended. Log it and pick a new wait; a node that elected itself goes silent.
    Ended(RoundEnd),
}

#[derive(Debug)]
struct OpenRound {
    round: u64,
    opened: Instant,
    called: bool,
    my_bid_at: Option<Instant>,
    /// Stamped as the last bid is recorded, not after the driver's send returns: a winner closing
    /// its gate can hold that send up for a whole peer RPC timeout, which no election took.
    all_bids_at: Option<Instant>,
    send_finished: bool,
    bids: BTreeMap<NodeId, FaultBid>,
}

impl OpenRound {
    fn new(round: u64, opened: Instant, called: bool) -> Self {
        Self {
            round,
            opened,
            called,
            my_bid_at: None,
            all_bids_at: None,
            send_finished: false,
            bids: BTreeMap::new(),
        }
    }

    /// The first bid per node stands, so a node can never change its number after a peer has seen
    /// it, and everyone holding all the bids works out the same winner.
    fn add(&mut self, from: NodeId, bid: FaultBid, nodes: usize, now: Instant) {
        self.bids.entry(from).or_insert(bid);
        if self.bids.len() >= nodes && self.all_bids_at.is_none() {
            self.all_bids_at = Some(now);
        }
    }

    fn end(&self, elected: Option<Elected>) -> RoundEnd {
        let election = match (elected, self.my_bid_at, self.all_bids_at) {
            (Some(_), Some(mine), Some(all)) => Some(all.saturating_duration_since(mine)),
            _ => None,
        };
        RoundEnd {
            round: self.round,
            tickets: self.bids.iter().map(|(id, b)| (*id, b.ticket)).collect(),
            elected,
            called: self.called,
            election,
        }
    }
}

/// One node's rounds of the fault election. Takes the time and the draw as arguments so it is easy
/// to test, and holds nothing replicated: this soft state starts empty after a restart.
#[derive(Debug)]
pub struct FaultRounds {
    me: NodeId,
    /// How many bids close a round: every node in the config, never only the ones this node
    /// believes are alive, or a dead machine would shrink the round to two.
    nodes: usize,
    timeout: Duration,
    /// The highest round this node has closed or given up on.
    left: u64,
    next_call_at: Instant,
    open: Option<OpenRound>,
}

impl FaultRounds {
    pub fn new(me: NodeId, nodes: usize, timeout: Duration, now: Instant) -> Self {
        Self {
            me,
            nodes,
            timeout,
            left: 0,
            next_call_at: now,
            open: None,
        }
    }

    /// The highest round this node has closed or given up on.
    pub fn left(&self) -> u64 {
        self.left
    }

    /// The round in progress, if any.
    pub fn open_round(&self) -> Option<u64> {
        self.open.as_ref().map(|open| open.round)
    }

    /// How many bids this node holds for the round in progress.
    pub fn bids_held(&self) -> usize {
        self.open.as_ref().map_or(0, |open| open.bids.len())
    }

    pub fn set_next_call(&mut self, at: Instant) {
        self.next_call_at = at;
    }

    /// A peer's bid. A round at or below the highest one left, or below the one in progress, is
    /// late and changes nothing, not even the timer. A higher round replaces the one in progress,
    /// which comes back here so the caller can log that it was given up on.
    pub fn add_bid(&mut self, from: NodeId, bid: FaultBid, now: Instant) -> Option<RoundEnd> {
        if bid.round <= self.left {
            return None;
        }
        let replaced = match self.open.take_if(|open| bid.round > open.round) {
            Some(open) => {
                self.left = open.round;
                Some(open.end(None))
            }
            None => None,
        };
        let nodes = self.nodes;
        let open = self
            .open
            .get_or_insert_with(|| OpenRound::new(bid.round, now, false));
        if bid.round == open.round {
            open.add(from, bid, nodes, now);
        }
        replaced
    }

    /// This node's send of `round` has returned. Too late to matter if a higher round took over.
    pub fn send_finished(&mut self, round: u64) {
        if let Some(open) = self.open.as_mut().filter(|open| open.round == round) {
            open.send_finished = true;
        }
    }

    /// Works out what the driver does next. `may_bid` says whether this node may take part now.
    /// `draw` is called only when this node bids, at most once per round.
    pub fn step(
        &mut self,
        now: Instant,
        may_bid: bool,
        draw: impl FnOnce(u64) -> FaultBid,
    ) -> Step {
        let open = match self.open.as_mut() {
            Some(open) => open,
            None if now < self.next_call_at => return Step::Sleep(self.next_call_at),
            None if !may_bid => return Step::Rearm,
            None => self.open.insert(OpenRound::new(self.left + 1, now, true)),
        };
        // Before the deadline: the driver's own send can run past it, and a node holding every
        // number must act on them.
        if open.bids.len() >= self.nodes && open.send_finished {
            let end = open.end(elect(&open.bids));
            return self.leave(end);
        }
        let deadline = open.opened + self.timeout;
        if now >= deadline {
            let end = open.end(None);
            return self.leave(end);
        }
        if open.my_bid_at.is_none() && may_bid {
            let bid = draw(open.round);
            open.my_bid_at = Some(now);
            open.add(self.me, bid, self.nodes, now);
            return Step::Send(bid);
        }
        Step::Sleep(deadline)
    }

    fn leave(&mut self, end: RoundEnd) -> Step {
        self.left = end.round;
        self.open = None;
        Step::Ended(end)
    }
}

/// [`FaultRounds`] and the wake-up that lets a bid reach the driver while it sleeps.
pub struct FaultElection {
    rounds: Mutex<FaultRounds>,
    wake: Notify,
}

impl FaultElection {
    pub fn new(id: NodeId, config: &Config) -> Self {
        // A caller's bid can take one peer timeout to reach a joiner, and the joiner's another to
        // come back. No new key: nothing about this round needs tuning apart from that timeout.
        let timeout = 2 * config.timers.peer_rpc_timeout();
        let nodes = config.cluster.nodes.len();
        Self {
            rounds: Mutex::new(FaultRounds::new(id, nodes, timeout, Instant::now())),
            wake: Notify::new(),
        }
    }

    /// Reads the rounds under the lock, for tests.
    pub fn with_rounds<T>(&self, f: impl FnOnce(&FaultRounds) -> T) -> T {
        f(&lock(&self.rounds))
    }
}

/// A peer's bid arrived; the term rule has already been applied.
pub fn on_bid(ctx: &NodeContext, from: NodeId, bid: FaultBid) {
    if !ctx.config.faults.enabled {
        return;
    }
    let election = &ctx.fault_election;
    let replaced = lock(&election.rounds).add_bid(from, bid, Instant::now());
    if let Some(end) = replaced {
        log_round(ctx, &end);
    }
    // notify_one keeps a wake that lands while the driver is waiting on its own send.
    election.wake.notify_one();
}

/// Start this node's part in the fault election.
pub fn spawn_schedule(ctx: &Arc<NodeContext>) -> Option<JoinHandle<()>> {
    if !ctx.config.faults.enabled {
        return None;
    }
    let seed = ctx.config.faults.seed.unwrap_or_else(rand::random);
    tracing::info!(node = ctx.id, seed, "fault election armed");
    Some(tokio::spawn(run_schedule(ctx.clone(), seed)))
}

/// One task per node, with no leadership check anywhere on the path. Each pass works out one next
/// step from scratch. The sleep is built fresh each time, because an elapsed `Sleep` kept across
/// passes stays ready and would spin.
async fn run_schedule(ctx: Arc<NodeContext>, seed: u64) {
    let election = &ctx.fault_election;
    let faults = &ctx.config.faults;
    // Apart from the draws, so the number of rounds so far does not change a node's tickets.
    let mut waits = StdRng::seed_from_u64(seed ^ ctx.id.wrapping_mul(WAIT_MIX));
    let first_wait = between(&mut waits, faults.interval());
    lock(&election.rounds).set_next_call(Instant::now() + first_wait);
    loop {
        let step = lock(&election.rounds).step(Instant::now(), may_bid(&ctx), |round| {
            draw(seed, round, ctx.id, faults)
        });
        match step {
            Step::Sleep(until) => {
                tokio::select! {
                    () = tokio::time::sleep_until(until) => {}
                    () = election.wake.notified() => {}
                }
            }
            Step::Rearm => {
                let wait = between(&mut waits, faults.interval());
                lock(&election.rounds).set_next_call(Instant::now() + wait);
            }
            Step::Send(bid) => {
                send_bid(&ctx, bid).await;
                lock(&election.rounds).send_finished(bid.round);
            }
            Step::Ended(end) => {
                log_round(&ctx, &end);
                let silence = end
                    .elected
                    .map_or(Duration::ZERO, |e| Duration::from_millis(e.ms));
                let wait = between(&mut waits, faults.interval());
                lock(&election.rounds).set_next_call(Instant::now() + silence + wait);
                if matches!(end.elected, Some(e) if e.victim == ctx.id) {
                    if let Err(reason) = on_elected(&ctx, silence) {
                        tracing::warn!(node = ctx.id, %reason, "elected, but staying up");
                        ctx.log.log(Event::FaultRefused {
                            victim: ctx.id,
                            reason,
                        });
                    }
                }
            }
        }
    }
}

/// `serving` goes false a moment after the gate closes (rejoin.rs), so check the gate too.
fn may_bid(ctx: &NodeContext) -> bool {
    ctx.serving.load(Ordering::SeqCst) && !ctx.gate.is_silenced() && !ctx.degraded.is_active()
}

/// One copy to everyone on this node's list. Awaited before the round can close, because a node
/// that closed its gate first would keep its ticket from the others. No retry: a lost or dropped
/// bid costs one round, and the next timer tries again (D159).
async fn send_bid(ctx: &NodeContext, bid: FaultBid) {
    let timeout = ctx.config.timers.peer_rpc_timeout();
    let answers = ctx.group.send(timeout, || PeerBody::FaultBid(bid)).await;
    for (peer, answer) in answers {
        // Only Ack means the peer holds it: StaleTerm was answered, and dropped.
        if !matches!(answer, Ok(PeerReply::Ack)) {
            tracing::debug!(
                node = ctx.id,
                peer,
                round = bid.round,
                ?answer,
                "fault bid not taken"
            );
        }
    }
}

fn log_round(ctx: &NodeContext, end: &RoundEnd) {
    let victim = end.elected.map(|e| e.victim);
    let ms = end.elected.map(|e| e.ms);
    tracing::info!(
        node = ctx.id,
        round = end.round,
        ?victim,
        ?ms,
        "fault election"
    );
    ctx.log.log(Event::FaultElection {
        round: end.round,
        tickets: end.tickets.clone(),
        victim,
        ms,
        called: end.called,
        election_us: end.election.map(|d| d.as_micros() as u64),
    });
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::node::transport::FaultGate;

    const ROUND_TIMEOUT: Duration = Duration::from_millis(1_000);
    const FIRST_CALL: Duration = Duration::from_millis(30_000);

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn bid(round: u64, ticket: u64) -> FaultBid {
        FaultBid {
            round,
            ticket,
            ms: 1_000 + ticket,
        }
    }

    /// Node 1 of three, with its timer due at `t0 + FIRST_CALL`.
    fn rounds(t0: Instant) -> FaultRounds {
        let mut r = FaultRounds::new(1, 3, ROUND_TIMEOUT, t0);
        r.set_next_call(t0 + FIRST_CALL);
        r
    }

    fn no_draw(round: u64) -> FaultBid {
        panic!("drew for round {round}, which should not happen");
    }

    /// Node 1 takes part in `round` with this ticket, and its send returns at once.
    fn my_bid_sent(r: &mut FaultRounds, now: Instant, round: u64, ticket: u64) {
        let step = r.step(now, true, |n| bid(n, ticket));
        assert_eq!(step, Step::Send(bid(round, ticket)));
        r.send_finished(round);
    }

    fn ended(step: Step) -> RoundEnd {
        match step {
            Step::Ended(end) => end,
            other => panic!("the round did not end: {other:?}"),
        }
    }

    #[test]
    fn durations_from_1_ms_to_the_limit_are_accepted() {
        assert!(check_duration(Duration::from_millis(1)).is_ok());
        assert!(check_duration(Duration::from_secs(5)).is_ok());
        assert!(check_duration(MAX_FAULT).is_ok());
    }

    #[test]
    fn zero_and_over_the_limit_are_rejected() {
        assert!(check_duration(Duration::ZERO).is_err());
        assert!(check_duration(MAX_FAULT + Duration::from_millis(1)).is_err());
    }

    #[test]
    fn the_first_bid_per_node_stands() {
        let t0 = Instant::now();
        let mut r = rounds(t0);
        r.add_bid(2, bid(1, 100), t0);
        r.add_bid(2, bid(1, 999_999), t0);
        assert_eq!(r.bids_held(), 1);
        r.add_bid(3, bid(1, 50), t0);
        my_bid_sent(&mut r, t0, 1, 10);

        let end = ended(r.step(t0, true, no_draw));
        assert_eq!(end.tickets[&2], 100);
        assert_eq!(
            end.elected,
            Some(Elected {
                victim: 2,
                ms: 1_100
            })
        );
    }

    #[test]
    fn a_late_bid_changes_nothing_not_even_the_timer() {
        let t0 = Instant::now();
        let mut r = rounds(t0);
        r.add_bid(2, bid(5, 1), t0);
        let abandoned = t0 + ROUND_TIMEOUT;
        assert!(ended(r.step(abandoned, false, no_draw)).elected.is_none());
        assert_eq!(r.left(), 5);
        let timer = abandoned + FIRST_CALL;
        r.set_next_call(timer);

        // At or below the round left: no round opens, and the timer does not move.
        for round in [5, 4] {
            assert_eq!(r.add_bid(2, bid(round, 1), abandoned), None);
            assert_eq!(r.open_round(), None);
            assert_eq!(r.step(abandoned, true, no_draw), Step::Sleep(timer));
        }

        // Below the round in progress: it does not join it, and the deadline does not move.
        r.add_bid(2, bid(9, 1), abandoned);
        assert_eq!(r.add_bid(3, bid(7, 1), abandoned + ms(5)), None);
        assert_eq!(r.open_round(), Some(9));
        assert_eq!(r.bids_held(), 1);
        assert_eq!(
            r.step(abandoned + ms(5), false, no_draw),
            Step::Sleep(abandoned + ROUND_TIMEOUT)
        );
    }

    #[test]
    fn a_higher_round_replaces_the_open_one_and_hands_it_back() {
        let t0 = Instant::now();
        let mut r = rounds(t0);
        let at = t0 + FIRST_CALL;
        assert_eq!(r.step(at, true, |n| bid(n, 7)), Step::Send(bid(1, 7)));
        assert_eq!(r.open_round(), Some(1));

        let end = r.add_bid(2, bid(4, 9), at + ms(1)).unwrap();
        assert_eq!(end.round, 1);
        assert_eq!(end.elected, None);
        assert_eq!(end.election, None);
        assert!(end.called, "this node's timer started it");
        assert_eq!(end.tickets, BTreeMap::from([(1, 7)]));
        assert_eq!(r.left(), 1);
        assert_eq!(r.open_round(), Some(4));

        // The old round's send coming back must not count for the new one.
        r.send_finished(1);
        r.add_bid(3, bid(4, 5), at + ms(1));
        assert_eq!(
            r.step(at + ms(1), true, |n| bid(n, 8)),
            Step::Send(bid(4, 8))
        );
        assert!(matches!(r.step(at + ms(2), true, no_draw), Step::Sleep(_)));

        // A bid for the round given up on is now late.
        assert_eq!(r.add_bid(3, bid(1, 5), at + ms(2)), None);
        assert_eq!(r.open_round(), Some(4));
    }

    #[test]
    fn a_round_closes_only_with_every_node_and_after_the_own_send() {
        let t0 = Instant::now();
        let mut r = rounds(t0);
        r.add_bid(2, bid(1, 20), t0);
        assert_eq!(r.step(t0, true, |n| bid(n, 10)), Step::Send(bid(1, 10)));
        r.send_finished(1);
        // Two of three: nothing yet, even with this node's own send done.
        assert_eq!(r.step(t0, true, no_draw), Step::Sleep(t0 + ROUND_TIMEOUT));

        // All three, and this node's own send still out: nothing yet.
        let mut r = rounds(t0);
        r.add_bid(2, bid(1, 20), t0);
        r.add_bid(3, bid(1, 30), t0);
        assert_eq!(r.step(t0, true, |n| bid(n, 10)), Step::Send(bid(1, 10)));
        assert_eq!(r.step(t0, true, no_draw), Step::Sleep(t0 + ROUND_TIMEOUT));

        r.send_finished(1);
        let end = ended(r.step(t0, true, no_draw));
        assert_eq!(end.elected.unwrap().victim, 3);
        assert!(!end.called, "a peer's bid opened this round");
        assert_eq!(r.left(), 1);
        assert_eq!(r.open_round(), None);
    }

    #[test]
    fn the_highest_ticket_wins_and_ties_go_to_the_lower_id() {
        let bids = |tickets: [u64; 3]| -> BTreeMap<NodeId, FaultBid> {
            (1..=3)
                .map(|id| (id, bid(1, tickets[id as usize - 1])))
                .collect()
        };
        let winner = |tickets| elect(&bids(tickets)).unwrap().victim;
        assert_eq!(winner([1, 2, 3]), 3);
        assert_eq!(winner([3, 2, 1]), 1);
        assert_eq!(winner([1, 3, 2]), 2);
        assert_eq!(winner([5, 7, 7]), 2);
        assert_eq!(winner([7, 7, 7]), 1);
        assert_eq!(winner([7, 1, 7]), 1);
    }

    #[test]
    fn every_node_can_win_and_the_silence_stays_in_range() {
        let cfg = FaultConfig::default();
        let (lo, hi) = cfg.silence();
        let nodes = [1, 2, 3];
        let mut winners = BTreeSet::new();
        for seed in 0..200 {
            for round in 1..=20 {
                let bids: BTreeMap<NodeId, FaultBid> = nodes
                    .iter()
                    .map(|id| (*id, draw(seed, round, *id, &cfg)))
                    .collect();
                for b in bids.values() {
                    assert_eq!(b.round, round);
                    assert!(b.ticket < TICKETS);
                    let silence = ms(b.ms);
                    assert!((lo..=hi).contains(&silence), "{silence:?}");
                    assert!(silence <= MAX_FAULT);
                }
                winners.insert(elect(&bids).unwrap().victim);
            }
        }
        assert_eq!(winners.into_iter().collect::<Vec<_>>(), nodes);
    }

    #[test]
    fn the_same_seed_round_and_node_give_the_same_numbers() {
        let cfg = FaultConfig::default();
        assert_eq!(draw(42, 3, 2, &cfg), draw(42, 3, 2, &cfg));
        assert_ne!(draw(42, 3, 2, &cfg), draw(43, 3, 2, &cfg));
        assert_ne!(draw(42, 3, 2, &cfg), draw(42, 4, 2, &cfg));
        assert_ne!(draw(42, 3, 2, &cfg), draw(42, 3, 1, &cfg));
    }

    #[test]
    fn a_node_draws_once_per_round() {
        let t0 = Instant::now();
        let mut r = rounds(t0);
        let mut draws = 0;
        let mut pass = |r: &mut FaultRounds, now: Instant| {
            r.step(now, true, |n| {
                draws += 1;
                bid(n, 1)
            })
        };

        r.add_bid(2, bid(1, 9), t0);
        assert_eq!(pass(&mut r, t0), Step::Send(bid(1, 1)));
        r.send_finished(1);
        for tick in 1..5 {
            assert!(matches!(pass(&mut r, t0 + ms(tick)), Step::Sleep(_)));
        }
        assert!(ended(pass(&mut r, t0 + ROUND_TIMEOUT)).elected.is_none());

        // Hearing of that round again does not reopen it, so there is no second number for it.
        r.add_bid(2, bid(1, 9), t0 + ROUND_TIMEOUT);
        r.set_next_call(t0 + 2 * FIRST_CALL);
        assert!(matches!(pass(&mut r, t0 + ROUND_TIMEOUT), Step::Sleep(_)));
        assert_eq!(draws, 1);
    }

    #[test]
    fn an_incomplete_round_is_abandoned_at_its_deadline() {
        let t0 = Instant::now();
        let mut r = rounds(t0);
        r.add_bid(2, bid(1, 20), t0);
        my_bid_sent(&mut r, t0, 1, 10);

        let just_before = t0 + ROUND_TIMEOUT - ms(1);
        assert_eq!(
            r.step(just_before, true, no_draw),
            Step::Sleep(t0 + ROUND_TIMEOUT)
        );
        let end = ended(r.step(t0 + ROUND_TIMEOUT, true, no_draw));
        assert_eq!(end.elected, None);
        assert_eq!(end.election, None);
        assert_eq!(end.tickets, BTreeMap::from([(1, 10), (2, 20)]));
        assert_eq!(r.left(), 1);
        assert_eq!(r.open_round(), None);
    }

    /// The driver's own send can outlast the round. A node that then holds every number acts on
    /// them, or the others' view of the round and its own would differ for no reason.
    #[test]
    fn a_complete_round_closes_even_past_its_deadline() {
        let t0 = Instant::now();
        let mut r = rounds(t0);
        r.add_bid(2, bid(1, 20), t0);
        r.add_bid(3, bid(1, 30), t0);
        my_bid_sent(&mut r, t0, 1, 10);
        let end = ended(r.step(t0 + 2 * ROUND_TIMEOUT, true, no_draw));
        assert_eq!(end.elected.unwrap().victim, 3);
    }

    #[test]
    fn a_timer_that_fires_while_not_eligible_rearms() {
        let t0 = Instant::now();
        let mut r = rounds(t0);
        let due = t0 + FIRST_CALL;
        assert_eq!(r.step(due - ms(1), false, no_draw), Step::Sleep(due));
        assert_eq!(r.step(due, false, no_draw), Step::Rearm);
        assert_eq!(r.open_round(), None);
        assert_eq!(r.left(), 0, "no round was called");

        r.set_next_call(due + FIRST_CALL);
        assert_eq!(r.step(due, false, no_draw), Step::Sleep(due + FIRST_CALL));
    }

    #[test]
    fn a_timer_is_ignored_while_a_round_is_open() {
        let t0 = Instant::now();
        let mut r = rounds(t0);
        let due = t0 + FIRST_CALL;
        r.add_bid(2, bid(8, 20), due - ms(1));

        // The timer is due, but it neither opens another round nor rearms.
        let deadline = due - ms(1) + ROUND_TIMEOUT;
        assert_eq!(r.step(due, false, no_draw), Step::Sleep(deadline));
        assert_eq!(r.open_round(), Some(8));
    }

    #[test]
    fn a_timer_calls_the_round_after_the_last_one_left() {
        let t0 = Instant::now();
        let mut r = rounds(t0);
        r.add_bid(2, bid(5, 1), t0);
        ended(r.step(t0 + ROUND_TIMEOUT, false, no_draw));

        let due = t0 + FIRST_CALL;
        assert_eq!(r.step(due, true, |n| bid(n, 7)), Step::Send(bid(6, 7)));
        r.send_finished(6);
        r.add_bid(2, bid(6, 1), due);
        r.add_bid(3, bid(6, 2), due);
        assert!(ended(r.step(due, true, no_draw)).called);
    }

    #[test]
    fn the_election_time_runs_from_my_bid_to_the_last_bid() {
        let t0 = Instant::now();

        let mut caller = rounds(t0);
        let due = t0 + FIRST_CALL;
        assert_eq!(caller.step(due, true, |n| bid(n, 1)), Step::Send(bid(1, 1)));
        caller.add_bid(2, bid(1, 2), due + ms(2));
        caller.add_bid(3, bid(1, 3), due + ms(5));
        // The send can come back much later, as when the winner's gate drops its answer.
        caller.send_finished(1);
        let end = ended(caller.step(due + ms(500), true, no_draw));
        assert_eq!(end.election, Some(ms(5)));

        // A joiner whose own bid is the last one needed has nothing left to wait for.
        let mut joiner = rounds(t0);
        joiner.add_bid(2, bid(1, 2), t0);
        joiner.add_bid(3, bid(1, 3), t0 + ms(1));
        my_bid_sent(&mut joiner, t0 + ms(3), 1, 1);
        let end = ended(joiner.step(t0 + ms(4), true, no_draw));
        assert_eq!(end.election, Some(Duration::ZERO));
    }

    #[test]
    fn a_silenced_gate_cannot_be_silenced_again_until_restored() {
        let gate = FaultGate::default();
        assert!(gate.try_silence());
        assert!(!gate.try_silence());
        assert!(gate.is_silenced());
        gate.restore();
        assert!(gate.try_silence());
    }
}
