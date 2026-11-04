//! Rotating rounds: one order for all commands, with no leader (D249).
//!
//! The class notes ask for peers with no coordinator. Here no node is elected and none orders for
//! another. Each node numbers its own stream of batches by round and sends it to its peers. Every
//! node applies the same merged order: by round, then by node ID. A round is applied once every
//! member's stream has passed it, so all copies of the state machine see the same commands in the
//! same order, and [`CloudState`](crate::state::CloudState) needs no change.
//!
//! - **Streams.** Every `round_ms` a node takes what waited, saves it as its next batch, and its
//!   stream covers that round. A round with nothing to say is not sent: the next message's `through`
//!   covers it. Rounds follow the highest seen, so a slow clock cannot hold the others back.
//! - **One call at a time per peer.** Whatever is unsent goes in the next call. A slow network gets
//!   bigger messages and not more of them.
//! - **Safe to answer.** A node applies its own batch only after a peer has saved it, and a peer's
//!   batch only after saving it itself, so whatever a client was told is on two nodes.
//! - **A node that goes silent.** It holds up every round until the other two drop it, after
//!   `takeover_missed_heartbeats` x `peer_heartbeat_ms`. One asks and the other promises, so two of
//!   three always agree, and one promise per view means there can be only one next view. They agree
//!   where its stream ends, at the higher of what they hold, and share what either lacks.
//! - **One node left.** It cannot drop anyone, so it stops committing, and the node's degraded mode
//!   answers provisionally (D120). When a peer returns, its rounds carry on.
//! - **A node that returns.** It is sent the applied log in order, asks to be added, and a member adds
//!   it through an entry in its stream, at a round far enough ahead that all members agree.
//! - **Limit.** A restarted node recovers from a peer's log and not from its own file, so losing all
//!   three at once loses the state. See D249.
//!
//! Owner: A (Raft and replication).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use futures::future::BoxFuture;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Notify, mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::config::Config;
use crate::metrics::{Event, EventLog};
use crate::node::transport::PeerSet;
use crate::proto::{
    Command, CommandGroup, NodeId, Outcome, PeerBody, PeerEnvelope, PeerReply, ProposeError,
    RaftRpc, RoundBatch, RoundControl, RoundsMsg, RoundsReply,
};
use crate::raft::store::{CHECKPOINT_EVERY, StateReader};
use crate::raft::{Consensus, ConsensusStatus, LogWork, Role};

/// How much of a stream goes in one message, in commands. A slow peer catches up over several.
const MAX_COMMANDS_PER_CALL: usize = 8000;
/// How many commands a node takes into one batch.
const MAX_COMMANDS_PER_BATCH: usize = 8000;
/// Commands waiting to go into a batch. More than this are refused at once, so an overloaded node
/// says no at the door, and a report's retry is what comes back and not a pile.
const MAX_WAITING: usize = 60_000;
/// How far ahead a new member's first round is, in rounds, so all members have applied its entry.
const ADD_MARGIN_ROUNDS: u64 = 100;
/// How long nodes that have not been heard from yet may take to start.
const START_GRACE: Duration = Duration::from_secs(20);
/// How often a node looks for a silent member.
const DETECT_EVERY: Duration = Duration::from_millis(25);
/// How long a returning node waits between catch-up calls with nothing new.
const CATCH_UP_PAUSE: Duration = Duration::from_millis(10);
/// How many log entries one catch-up reply holds at most.
const CATCH_UP_ENTRIES: usize = 400;
const IO_REPORT_INTERVAL: Duration = Duration::from_secs(1);
/// How long a node just added has to send its first message before it counts as silent.
const JOIN_GRACE: Duration = Duration::from_secs(3);

type Answer = oneshot::Sender<Result<Outcome, ProposeError>>;

struct Waiting {
    command: Command,
    deadline: Instant,
    answer: Answer,
}

#[derive(Default)]
struct Stats {
    writes: AtomicU64,
    commands: AtomicU64,
    expired: AtomicU64,
    answered: AtomicU64,
    answered_us: AtomicU64,
    syncs: AtomicU64,
    sync_us: AtomicU64,
    max_sync_us: AtomicU64,
    saved_commands: AtomicU64,
}

/// What a node knows, under one lock. Never held across an `.await`.
struct Core {
    members: BTreeSet<NodeId>,
    view: u64,
    /// Waiting to be added back: applying a peer's log and not emitting.
    joining: bool,
    /// The last round of this node's own stream.
    my_round: u64,
    max_seen: u64,
    waiting: Vec<Waiting>,
    /// Answers for this node's own batches, by round, until applied.
    answers: BTreeMap<u64, Vec<(usize, Answer, Instant)>>,
    /// How far each member's stream is saved here, this node's own included.
    covered: BTreeMap<NodeId, u64>,
    /// How far each peer has saved this node's stream.
    acked: BTreeMap<NodeId, u64>,
    /// What has been sent to each peer, assumed held unless a call fails.
    sent: BTreeMap<NodeId, u64>,
    /// Every batch held, per stream, so a peer that lacks one can be sent it.
    streams: BTreeMap<NodeId, BTreeMap<u64, Arc<RoundBatch>>>,
    /// Saved and not applied.
    pending: BTreeMap<(u64, NodeId), Arc<RoundBatch>>,
    /// Every applied batch in order, so a returning node can be sent them.
    ordered: Vec<Arc<RoundBatch>>,
    applied_round: u64,
    index: u64,
    last_heard: BTreeMap<NodeId, Instant>,
    /// The one next view this node has agreed to: which view, and who it leaves out.
    promise: Option<(u64, NodeId)>,
    /// Views already made: what a slower member needs to catch up on the same drop.
    drops: BTreeMap<u64, (NodeId, u64)>,
    /// An entry to put in this node's next batch.
    control: Option<RoundControl>,
    started: Instant,
    /// Where in `ordered` this node's latest return began: an older `Add` for it does not count.
    join_from: usize,
}

/// The rotating rounds [`Consensus`].
pub struct RoundsNode {
    inner: Arc<Inner>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

struct Inner {
    id: NodeId,
    peers: Arc<PeerSet>,
    log: EventLog,
    state: StateReader,
    core: Mutex<Core>,
    saver: mpsc::UnboundedSender<SaveJob>,
    applied: watch::Sender<Option<u64>>,
    contact: watch::Sender<Option<u64>>,
    wake: Notify,
    stats: Stats,
    round: Duration,
    suspect_after: Duration,
    propose_timeout: Duration,
    rpc_timeout: Duration,
}

struct SaveJob {
    bytes: Vec<u8>,
    done: oneshot::Sender<()>,
}

fn lock(core: &Mutex<Core>) -> MutexGuard<'_, Core> {
    core.lock().unwrap_or_else(|p| p.into_inner())
}

impl RoundsNode {
    /// Starts this node's rounds. `restarted` says it had a file from an earlier run, so it has to
    /// be sent the log before it serves.
    pub async fn start(
        id: NodeId,
        config: &Config,
        peers: Arc<PeerSet>,
        log: EventLog,
    ) -> anyhow::Result<(Arc<Self>, StateReader, bool)> {
        let path = config
            .storage
            .dir
            .as_ref()
            .map(|dir| dir.join(format!("node-{id}.rounds")));
        let restarted = match &path {
            Some(path) => tokio::fs::metadata(path).await.is_ok_and(|m| m.len() > 0),
            None => false,
        };
        let members: BTreeSet<NodeId> = config.cluster.nodes.iter().map(|n| n.id).collect();
        let now = Instant::now();
        let core = Core {
            covered: members.iter().map(|m| (*m, 0)).collect(),
            acked: members.iter().map(|m| (*m, 0)).collect(),
            sent: members.iter().map(|m| (*m, 0)).collect(),
            streams: members.iter().map(|m| (*m, BTreeMap::new())).collect(),
            last_heard: BTreeMap::new(),
            members,
            view: 0,
            joining: restarted,
            my_round: 0,
            max_seen: 0,
            waiting: Vec::new(),
            answers: BTreeMap::new(),
            pending: BTreeMap::new(),
            ordered: Vec::new(),
            applied_round: 0,
            index: 0,
            promise: None,
            drops: BTreeMap::new(),
            control: None,
            started: now,
            join_from: 0,
        };
        let (saver, jobs) = mpsc::unbounded_channel();
        let timers = &config.timers;
        let inner = Arc::new(Inner {
            id,
            peers,
            log,
            state: StateReader::default(),
            core: Mutex::new(core),
            saver,
            applied: watch::channel(None).0,
            contact: watch::channel(None).0,
            wake: Notify::new(),
            stats: Stats::default(),
            round: Duration::from_millis(timers.round_ms),
            suspect_after: timers.peer_heartbeat() * timers.takeover_missed_heartbeats,
            propose_timeout: timers.propose_timeout(),
            rpc_timeout: timers.peer_rpc_timeout(),
        });
        let state = inner.state.clone();
        inner.log_view();
        let mut tasks = vec![
            tokio::spawn(run_saver(inner.clone(), path, jobs)),
            tokio::spawn(run_emitter(inner.clone())),
            tokio::spawn(run_detector(inner.clone())),
            tokio::spawn(run_io_report(inner.clone())),
            tokio::spawn(run_catch_up(inner.clone())),
        ];
        for peer in inner.peers.iter() {
            tasks.push(tokio::spawn(run_sender(inner.clone(), peer.peer())));
        }
        let node = Arc::new(Self {
            inner,
            tasks: Mutex::new(tasks),
        });
        Ok((node, state, restarted))
    }
}

impl Inner {
    fn log_view(&self) {
        let core = lock(&self.core);
        self.log.log(Event::RaftState {
            term: core.view,
            leader: None,
            role: Role::Peer.as_str().to_string(),
            last_applied: (core.index > 0).then_some(core.index),
        });
    }

    async fn save(&self, batches: &[Arc<RoundBatch>]) {
        let commands: usize = batches.iter().map(|b| b.commands.len()).sum();
        self.stats
            .saved_commands
            .fetch_add(commands as u64, Ordering::Relaxed);
        let mut bytes = Vec::new();
        for batch in batches {
            if let Ok(encoded) = crate::proto::encode(batch.as_ref()) {
                bytes.extend_from_slice(&(encoded.len() as u32).to_be_bytes());
                bytes.extend_from_slice(&encoded);
            }
        }
        let (done, saved) = oneshot::channel();
        if self.saver.send(SaveJob { bytes, done }).is_ok() {
            let _ = saved.await;
        }
    }

    /// Applies every round all members have passed, and answers this node's own commands.
    fn apply_ready(&self, core: &mut Core) {
        if core.joining {
            return;
        }
        let mut frontier = u64::MAX;
        let mut own_limit = 0;
        let mut any_peer = false;
        for member in &core.members {
            if *member == self.id {
                frontier = frontier.min(core.covered.get(member).copied().unwrap_or(0));
            } else {
                any_peer = true;
                frontier = frontier.min(core.covered.get(member).copied().unwrap_or(0));
                own_limit = own_limit.max(core.acked.get(member).copied().unwrap_or(0));
            }
        }
        if !any_peer {
            return;
        }
        frontier = frontier.min(own_limit);
        if frontier <= core.applied_round {
            return;
        }
        let mut applied_any = false;
        while let Some((&(round, node), _)) = core.pending.first_key_value() {
            if round > frontier {
                break;
            }
            let Some(batch) = core.pending.remove(&(round, node)) else {
                break;
            };
            self.apply_batch(core, &batch);
            applied_any = true;
        }
        core.applied_round = frontier;
        if applied_any {
            self.applied.send_replace(Some(core.index));
        }
    }

    fn apply_batch(&self, core: &mut Core, batch: &Arc<RoundBatch>) {
        core.index += 1;
        let index = core.index;
        let group = CommandGroup(batch.commands.clone());
        let outcomes = self.state.apply_entry(index, &group, |digest| {
            if index % CHECKPOINT_EVERY == 0 {
                self.log.log(Event::StateCheckpoint {
                    log_index: index,
                    digest,
                });
            }
        });
        if let Some(RoundControl::Add { node, start_round }) = batch.control {
            if !core.members.contains(&node) {
                core.members.insert(node);
                core.view += 1;
                core.promise = None;
                core.covered.insert(node, start_round - 1);
                core.acked.insert(node, start_round - 1);
                core.sent.insert(node, start_round - 1);
                core.streams.entry(node).or_default();
                core.last_heard.insert(node, Instant::now() + JOIN_GRACE);
                self.log.log(Event::RaftState {
                    term: core.view,
                    leader: None,
                    role: Role::Peer.as_str().to_string(),
                    last_applied: Some(index),
                });
            }
        }
        core.ordered.push(batch.clone());
        if batch.node == self.id {
            if let Some(answers) = core.answers.remove(&batch.round) {
                for (position, answer, entered) in answers {
                    if let Some(outcome) = outcomes.get(position) {
                        self.stats.answered.fetch_add(1, Ordering::Relaxed);
                        self.stats
                            .answered_us
                            .fetch_add(entered.elapsed().as_micros() as u64, Ordering::Relaxed);
                        let _ = answer.send(Ok(outcome.clone()));
                    }
                }
            }
        }
    }

    /// Leave `target` out from now on. Its stream ends at `cut`, and both survivors hold all of it.
    fn install_drop(&self, core: &mut Core, view: u64, target: NodeId, cut: u64) {
        if view <= core.view || !core.members.contains(&target) {
            return;
        }
        core.view = view;
        core.members.remove(&target);
        core.promise = None;
        core.drops.insert(view, (target, cut));
        core.pending
            .retain(|(round, node), _| *node != target || *round <= cut);
        core.covered.remove(&target);
        core.acked.remove(&target);
        core.sent.remove(&target);
        self.log.log(Event::RaftState {
            term: core.view,
            leader: None,
            role: Role::Peer.as_str().to_string(),
            last_applied: Some(core.index),
        });
        tracing::info!(node = self.id, view, target, cut, "dropped a silent member");
        self.apply_ready(core);
    }

    /// Batches of `node`'s stream after `after`, up to `through`.
    fn batches_between(
        core: &Core,
        node: NodeId,
        after: u64,
        through: u64,
    ) -> Vec<Arc<RoundBatch>> {
        if after >= through {
            return Vec::new();
        }
        core.streams
            .get(&node)
            .map(|stream| {
                stream
                    .range(after + 1..=through)
                    .map(|(_, b)| b.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn hold(core: &mut Core, batches: &[Arc<RoundBatch>]) {
        for batch in batches {
            core.streams
                .entry(batch.node)
                .or_default()
                .insert(batch.round, batch.clone());
            if batch.round > core.applied_round {
                core.pending
                    .entry((batch.round, batch.node))
                    .or_insert_with(|| batch.clone());
            }
        }
    }

    async fn on_message(&self, from: NodeId, message: RoundsMsg) -> PeerReply {
        PeerReply::Rounds(match message {
            RoundsMsg::Stream {
                after,
                through,
                batches,
            } => self.on_stream(from, after, through, batches).await,
            RoundsMsg::Drop { view, target, have } => self.on_drop(from, view, target, have),
            RoundsMsg::DropCommit {
                view,
                target,
                cut,
                batches,
            } => {
                let batches: Vec<Arc<RoundBatch>> = batches.into_iter().map(Arc::new).collect();
                self.save(&batches).await;
                let mut core = lock(&self.core);
                Self::hold(&mut core, &batches);
                self.install_drop(&mut core, view, target, cut);
                RoundsReply::Done
            }
            RoundsMsg::CatchUp { next } => self.on_catch_up(next),
            RoundsMsg::Join => self.on_join(from),
            RoundsMsg::Applied => {
                let core = lock(&self.core);
                if core.joining {
                    RoundsReply::NotYet
                } else {
                    RoundsReply::Applied { index: core.index }
                }
            }
        })
    }

    async fn on_stream(
        &self,
        from: NodeId,
        after: u64,
        through: u64,
        batches: Vec<RoundBatch>,
    ) -> RoundsReply {
        let batches: Vec<Arc<RoundBatch>> = {
            let mut core = lock(&self.core);
            if core.joining {
                return RoundsReply::NotYet;
            }
            if !core.members.contains(&from) {
                return RoundsReply::NotMember { view: core.view };
            }
            core.last_heard.insert(from, Instant::now());
            core.max_seen = core.max_seen.max(through);
            let have = core.covered.get(&from).copied().unwrap_or(0);
            if after > have {
                return RoundsReply::Have { have };
            }
            batches
                .into_iter()
                .filter(|b| b.round > have && b.round <= through)
                .map(Arc::new)
                .collect()
        };
        if !batches.is_empty() {
            self.save(&batches).await;
        }
        let mut core = lock(&self.core);
        if !core.members.contains(&from) {
            return RoundsReply::NotMember { view: core.view };
        }
        Self::hold(&mut core, &batches);
        let entry = core.covered.entry(from).or_insert(0);
        *entry = (*entry).max(through);
        let have = *entry;
        self.apply_ready(&mut core);
        self.contact.send_replace(Some(core.index));
        drop(core);
        self.wake.notify_waiters();
        RoundsReply::Have { have }
    }

    fn on_drop(&self, from: NodeId, view: u64, target: NodeId, have: u64) -> RoundsReply {
        let mut core = lock(&self.core);
        if core.joining {
            return RoundsReply::NotYet;
        }
        // Already done: the asker was slower. Give it the same result.
        if let Some(&(dropped, cut)) = core.drops.get(&view) {
            if dropped == target {
                return RoundsReply::Granted {
                    view,
                    have: cut,
                    batches: Self::batches_between(&core, target, have, cut)
                        .iter()
                        .map(|b| b.as_ref().clone())
                        .collect(),
                };
            }
            return RoundsReply::Refused { view: core.view };
        }
        if view != core.view + 1
            || target == self.id
            || !core.members.contains(&from)
            || !core.members.contains(&target)
        {
            return RoundsReply::Refused { view: core.view };
        }
        match core.promise {
            Some((promised, whom)) if promised == view && whom != target => {
                return RoundsReply::Refused { view: core.view };
            }
            _ => core.promise = Some((view, target)),
        }
        let mine = core.covered.get(&target).copied().unwrap_or(0);
        RoundsReply::Granted {
            view,
            have: mine,
            batches: Self::batches_between(&core, target, have, mine)
                .iter()
                .map(|b| b.as_ref().clone())
                .collect(),
        }
    }

    fn on_catch_up(&self, next: u64) -> RoundsReply {
        let core = lock(&self.core);
        if core.joining {
            return RoundsReply::NotYet;
        }
        let start = (next as usize).min(core.ordered.len());
        let end = (start + CATCH_UP_ENTRIES).min(core.ordered.len());
        RoundsReply::Entries {
            batches: core.ordered[start..end]
                .iter()
                .map(|b| b.as_ref().clone())
                .collect(),
            total: core.ordered.len() as u64,
            applied_round: core.applied_round,
            view: core.view,
            members: core.members.iter().copied().collect(),
        }
    }

    fn on_join(&self, from: NodeId) -> RoundsReply {
        let mut core = lock(&self.core);
        if core.joining {
            return RoundsReply::NotYet;
        }
        if core.members.contains(&from) {
            // It restarted while still listed, so its stream is gone: the detector drops it.
            let long_ago = Instant::now() - self.suspect_after * 2;
            core.last_heard.insert(from, long_ago);
            return RoundsReply::NotYet;
        }
        if core.control.is_none() && core.members.len() >= 2 {
            let start_round = core.my_round.max(core.max_seen) + ADD_MARGIN_ROUNDS;
            core.control = Some(RoundControl::Add {
                node: from,
                start_round,
            });
        }
        RoundsReply::Done
    }
}

// ---------------------------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------------------------

async fn run_saver(
    inner: Arc<Inner>,
    path: Option<PathBuf>,
    mut jobs: mpsc::UnboundedReceiver<SaveJob>,
) {
    let mut file = match path {
        Some(path) => {
            if let Some(dir) = path.parent() {
                let _ = tokio::fs::create_dir_all(dir).await;
            }
            match tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await
            {
                Ok(file) => Some(file),
                Err(e) => {
                    tracing::warn!("rounds: cannot open the log file: {e}");
                    None
                }
            }
        }
        None => None,
    };
    let mut taken = Vec::new();
    while jobs.recv_many(&mut taken, 512).await > 0 {
        if let Some(file) = file.as_mut() {
            let started = Instant::now();
            let mut failed = false;
            for job in &taken {
                failed |= file.write_all(&job.bytes).await.is_err();
            }
            failed |= file.sync_data().await.is_err();
            if failed {
                tracing::warn!("rounds: a save failed");
            }
            let us = started.elapsed().as_micros() as u64;
            inner.stats.syncs.fetch_add(1, Ordering::Relaxed);
            inner.stats.sync_us.fetch_add(us, Ordering::Relaxed);
            inner.stats.max_sync_us.fetch_max(us, Ordering::Relaxed);
        }
        for job in taken.drain(..) {
            let _ = job.done.send(());
        }
    }
}

/// Every round: take what waited, save it as this node's next batch, and let the senders know.
async fn run_emitter(inner: Arc<Inner>) {
    let mut tick = tokio::time::interval(inner.round);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        let (batch, round) = {
            let mut core = lock(&inner.core);
            if core.joining || !core.members.contains(&inner.id) {
                continue;
            }
            let now = Instant::now();
            let mut commands = Vec::new();
            let mut answers = Vec::new();
            let take = core.waiting.len().min(MAX_COMMANDS_PER_BATCH);
            for waiting in core.waiting.drain(..take) {
                if waiting.deadline <= now || waiting.answer.is_closed() {
                    inner.stats.expired.fetch_add(1, Ordering::Relaxed);
                    let _ = waiting.answer.send(Err(ProposeError::Timeout));
                    continue;
                }
                answers.push((commands.len(), waiting.answer, now));
                commands.push(waiting.command);
            }
            let control = core.control.take();
            core.my_round = (core.my_round + 1).max(core.max_seen);
            let round = core.my_round;
            if commands.is_empty() && control.is_none() {
                (None, round)
            } else {
                inner.stats.writes.fetch_add(1, Ordering::Relaxed);
                inner
                    .stats
                    .commands
                    .fetch_add(commands.len() as u64, Ordering::Relaxed);
                let batch = Arc::new(RoundBatch {
                    node: inner.id,
                    round,
                    commands,
                    control,
                });
                core.answers.insert(round, answers);
                Inner::hold(&mut core, std::slice::from_ref(&batch));
                (Some(batch), round)
            }
        };
        if let Some(batch) = batch {
            inner.save(std::slice::from_ref(&batch)).await;
        }
        let mut core = lock(&inner.core);
        let entry = core.covered.entry(inner.id).or_insert(0);
        *entry = (*entry).max(round);
        inner.apply_ready(&mut core);
        drop(core);
        inner.wake.notify_waiters();
    }
}

/// One call at a time to one peer, holding whatever of this node's stream it has not been sent.
async fn run_sender(inner: Arc<Inner>, peer: NodeId) {
    let Some(client) = inner.peers.get(peer).cloned() else {
        return;
    };
    loop {
        let message = {
            let notified = inner.wake.notified();
            let built = {
                let mut core = lock(&inner.core);
                if core.joining
                    || !core.members.contains(&inner.id)
                    || !core.members.contains(&peer)
                {
                    None
                } else {
                    let after = core.sent.get(&peer).copied().unwrap_or(0);
                    let mine = core.covered.get(&inner.id).copied().unwrap_or(0);
                    if mine <= after {
                        None
                    } else {
                        let mut through = mine;
                        let mut commands = 0;
                        let mut batches = Vec::new();
                        if let Some(stream) = core.streams.get(&inner.id) {
                            for (round, batch) in stream.range(after + 1..=mine) {
                                if commands > 0
                                    && commands + batch.commands.len() > MAX_COMMANDS_PER_CALL
                                {
                                    through = round - 1;
                                    break;
                                }
                                commands += batch.commands.len();
                                batches.push(batch.as_ref().clone());
                            }
                        }
                        core.sent.insert(peer, through);
                        Some(RoundsMsg::Stream {
                            after,
                            through,
                            batches,
                        })
                    }
                }
            };
            match built {
                Some(message) => message,
                None => {
                    let _ = tokio::time::timeout(inner.round, notified).await;
                    continue;
                }
            }
        };
        let term = lock(&inner.core).view;
        let envelope = PeerEnvelope {
            from: inner.id,
            term,
            body: PeerBody::Rounds(message),
        };
        let reply = client.call(envelope, inner.rpc_timeout).await;
        let pause = {
            let mut core = lock(&inner.core);
            match reply {
                Ok(PeerReply::Rounds(RoundsReply::Have { have })) => {
                    core.last_heard.insert(peer, Instant::now());
                    if core.members.contains(&peer) {
                        let acked = core.acked.entry(peer).or_insert(0);
                        *acked = (*acked).max(have);
                        let sent = core.sent.entry(peer).or_insert(0);
                        if have < *sent {
                            *sent = have;
                        }
                    }
                    inner.apply_ready(&mut core);
                    inner.contact.send_replace(Some(core.index));
                    false
                }
                Ok(PeerReply::Rounds(RoundsReply::NotYet)) => {
                    core.last_heard.insert(peer, Instant::now());
                    let acked = core.acked.get(&peer).copied().unwrap_or(0);
                    core.sent.insert(peer, acked);
                    true
                }
                Ok(PeerReply::Rounds(RoundsReply::NotMember { view })) => {
                    // The others went on without this node.
                    if view > core.view && !core.joining {
                        tracing::info!(node = inner.id, view, "left out: catching up to return");
                        start_joining(&inner, &mut core);
                    }
                    true
                }
                _ => {
                    let acked = core.acked.get(&peer).copied().unwrap_or(0);
                    core.sent.insert(peer, acked);
                    true
                }
            }
        };
        if pause {
            tokio::time::sleep(CATCH_UP_PAUSE).await;
        }
    }
}

/// This node was left out. Whatever it was asked and has not applied can no longer be answered here.
fn start_joining(inner: &Inner, core: &mut Core) {
    core.joining = true;
    core.join_from = core.ordered.len();
    core.pending.clear();
    for (_, answers) in std::mem::take(&mut core.answers) {
        for (_, answer, _) in answers {
            let _ = answer.send(Err(ProposeError::Timeout));
        }
    }
    for waiting in core.waiting.drain(..) {
        let _ = waiting.answer.send(Err(ProposeError::Timeout));
    }
    inner.contact.send_replace(None);
}

/// Looks for a member gone silent, and agrees with the other member to go on without it.
async fn run_detector(inner: Arc<Inner>) {
    let mut tick = tokio::time::interval(DETECT_EVERY);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tick.tick().await;
        let found = {
            let core = lock(&inner.core);
            let now = Instant::now();
            if core.joining || core.members.len() != 3 || !core.members.contains(&inner.id) {
                None
            } else {
                let silent = core.members.iter().copied().find(|m| {
                    *m != inner.id
                        && match core.last_heard.get(m) {
                            Some(heard) => {
                                now.saturating_duration_since(*heard) >= inner.suspect_after
                            }
                            None => now.saturating_duration_since(core.started) >= START_GRACE,
                        }
                });
                silent.and_then(|target| {
                    let other = core
                        .members
                        .iter()
                        .copied()
                        .find(|m| *m != inner.id && *m != target)?;
                    Some((target, other, core.view + 1))
                })
            }
        };
        if let Some((target, other, view)) = found {
            try_drop(&inner, target, other, view).await;
        }
    }
}

async fn try_drop(inner: &Inner, target: NodeId, other: NodeId, view: u64) {
    let Some(client) = inner.peers.get(other).cloned() else {
        return;
    };
    let mine = {
        let mut core = lock(&inner.core);
        match core.promise {
            Some((promised, whom)) if promised == view && whom != target => return,
            _ => core.promise = Some((view, target)),
        }
        core.covered.get(&target).copied().unwrap_or(0)
    };
    let envelope = PeerEnvelope {
        from: inner.id,
        term: view - 1,
        body: PeerBody::Rounds(RoundsMsg::Drop {
            view,
            target,
            have: mine,
        }),
    };
    let reply = client.call(envelope, inner.rpc_timeout).await;
    let Ok(PeerReply::Rounds(RoundsReply::Granted {
        have: theirs,
        batches,
        ..
    })) = reply
    else {
        // No partner: with one node left, nothing can be dropped. Free this view's promise.
        let mut core = lock(&inner.core);
        if core.promise == Some((view, target)) {
            core.promise = None;
        }
        return;
    };
    let batches: Vec<Arc<RoundBatch>> = batches.into_iter().map(Arc::new).collect();
    if !batches.is_empty() {
        inner.save(&batches).await;
    }
    let cut = mine.max(theirs);
    let (send, term) = {
        let mut core = lock(&inner.core);
        Inner::hold(&mut core, &batches);
        let send = Inner::batches_between(&core, target, theirs, cut);
        inner.install_drop(&mut core, view, target, cut);
        (send, core.view)
    };
    let commit = PeerEnvelope {
        from: inner.id,
        term,
        body: PeerBody::Rounds(RoundsMsg::DropCommit {
            view,
            target,
            cut,
            batches: send.iter().map(|b| b.as_ref().clone()).collect(),
        }),
    };
    // If this is lost, the other member's own detector asks, and is given the same result.
    let _ = client.call(commit, inner.rpc_timeout).await;
    inner.wake.notify_waiters();
}

/// While left out: apply a member's log in order, ask to be added, and come back at its round.
async fn run_catch_up(inner: Arc<Inner>) {
    let mut asked = Instant::now() - Duration::from_secs(10);
    loop {
        if !lock(&inner.core).joining {
            tokio::time::sleep(inner.round.max(CATCH_UP_PAUSE)).await;
            continue;
        }
        let mut progressed = false;
        for client in inner.peers.iter() {
            let (next, term) = {
                let core = lock(&inner.core);
                (core.ordered.len() as u64, core.view)
            };
            let envelope = PeerEnvelope {
                from: inner.id,
                term,
                body: PeerBody::Rounds(RoundsMsg::CatchUp { next }),
            };
            let reply = client.call(envelope, inner.rpc_timeout).await;
            let Ok(PeerReply::Rounds(RoundsReply::Entries {
                batches,
                total,
                applied_round,
                view,
                members,
            })) = reply
            else {
                continue;
            };
            progressed = !batches.is_empty();
            let done = next + batches.len() as u64 >= total;
            let ask_to_join = {
                let mut core = lock(&inner.core);
                for batch in batches {
                    let batch = Arc::new(batch);
                    inner.apply_entry_while_joining(&mut core, &batch);
                }
                inner
                    .applied
                    .send_replace((core.index > 0).then_some(core.index));
                core.max_seen = core.max_seen.max(applied_round);
                match core.start_round_of(inner.id) {
                    Some(start_round) => {
                        if done && applied_round >= start_round - 1 {
                            // Back in: every member now waits for this node's stream from this round.
                            core.joining = false;
                            core.view = view;
                            core.my_round = start_round - 1;
                            core.applied_round = start_round - 1;
                            core.pending.clear();
                            core.members = members.into_iter().collect();
                            core.covered.clear();
                            core.acked.clear();
                            core.sent.clear();
                            core.last_heard.clear();
                            let everyone: Vec<NodeId> = core.members.iter().copied().collect();
                            for member in everyone {
                                core.covered.insert(member, start_round - 1);
                                core.acked.insert(member, start_round - 1);
                                core.sent.insert(member, start_round - 1);
                                core.last_heard.insert(member, Instant::now() + JOIN_GRACE);
                            }
                            core.promise = None;
                            tracing::info!(node = inner.id, start_round, "back as a member");
                            inner.contact.send_replace(Some(core.index));
                        }
                        false
                    }
                    None if done && asked.elapsed() >= Duration::from_secs(1) => {
                        asked = Instant::now();
                        true
                    }
                    None => false,
                }
            };
            if ask_to_join {
                let term = lock(&inner.core).view;
                let join = PeerEnvelope {
                    from: inner.id,
                    term,
                    body: PeerBody::Rounds(RoundsMsg::Join),
                };
                let _ = client.call(join, inner.rpc_timeout).await;
            }
            break;
        }
        if !progressed {
            tokio::time::sleep(CATCH_UP_PAUSE).await;
        }
    }
}

impl Inner {
    fn apply_entry_while_joining(&self, core: &mut Core, batch: &Arc<RoundBatch>) {
        // A node left out may hold a state from before, so it applies only what is new to it.
        self.apply_batch(core, batch);
        core.streams
            .entry(batch.node)
            .or_default()
            .insert(batch.round, batch.clone());
    }
}

impl Core {
    /// The round this node comes back at, once the log has told it.
    fn start_round_of(&self, me: NodeId) -> Option<u64> {
        self.ordered[self.join_from..]
            .iter()
            .rev()
            .find_map(|b| match b.control {
                Some(RoundControl::Add { node, start_round }) if node == me => Some(start_round),
                _ => None,
            })
    }
}

async fn run_io_report(inner: Arc<Inner>) {
    let mut tick = tokio::time::interval(IO_REPORT_INTERVAL);
    loop {
        tick.tick().await;
        let waiting = lock(&inner.core).waiting.len();
        let stats = &inner.stats;
        let load = |value: &AtomicU64| value.load(Ordering::Relaxed);
        inner.log.log(Event::RaftIo {
            waiting,
            writes: load(&stats.writes),
            commands: load(&stats.commands),
            expired: load(&stats.expired),
            write_timeouts: 0,
            answered: load(&stats.answered),
            answered_us: load(&stats.answered_us),
            writes_done: load(&stats.writes),
            write_us: 0,
            syncs: load(&stats.syncs),
            sync_us: load(&stats.sync_us),
            max_sync_us: stats.max_sync_us.swap(0, Ordering::Relaxed),
            saves: load(&stats.syncs),
            saved_entries: load(&stats.syncs),
            saved_commands: load(&stats.saved_commands),
            max_leader_gap_us: 0,
        });
    }
}

// ---------------------------------------------------------------------------------------------
// What node code sees
// ---------------------------------------------------------------------------------------------

impl RoundsNode {
    async fn submit(&self, command: Command) -> Result<Outcome, ProposeError> {
        let (answer, answered) = oneshot::channel();
        let deadline = Instant::now() + self.inner.propose_timeout;
        {
            let mut core = lock(&self.inner.core);
            if core.joining || core.waiting.len() >= MAX_WAITING {
                self.inner.stats.expired.fetch_add(1, Ordering::Relaxed);
                return Err(ProposeError::Timeout);
            }
            core.waiting.push(Waiting {
                command,
                deadline,
                answer,
            });
        }
        match tokio::time::timeout_at(deadline, answered).await {
            Ok(Ok(result)) => result,
            _ => Err(ProposeError::Timeout),
        }
    }
}

impl Consensus for RoundsNode {
    fn propose(&self, cmd: Command) -> BoxFuture<'_, Result<Outcome, ProposeError>> {
        Box::pin(async move {
            let started = Instant::now();
            let (request, kind) = (cmd.request(), cmd.kind());
            let result = self.submit(cmd).await;
            let latency_us = started.elapsed().as_micros() as u64;
            match &result {
                Ok(outcome) => self.inner.log.log(Event::Commit {
                    request,
                    kind,
                    outcome: outcome.kind(),
                    latency_us,
                    forwarded: false,
                    log_index: match outcome {
                        Outcome::Committed(r) => Some(r.log_index),
                        _ => None,
                    },
                }),
                Err(e) => self.inner.log.log(Event::ProposeFailed {
                    request,
                    kind,
                    error: e.to_string(),
                    latency_us,
                }),
            }
            result
        })
    }

    fn propose_local(&self, cmd: Command) -> BoxFuture<'_, Result<Outcome, ProposeError>> {
        Box::pin(self.submit(cmd))
    }

    fn handle_rpc(&self, _from: NodeId, _rpc: RaftRpc) -> BoxFuture<'_, PeerReply> {
        Box::pin(async { PeerReply::Ack })
    }

    fn handle_rounds(&self, from: NodeId, message: RoundsMsg) -> BoxFuture<'_, PeerReply> {
        Box::pin(async move { self.inner.on_message(from, message).await })
    }

    fn status(&self) -> ConsensusStatus {
        let core = lock(&self.inner.core);
        ConsensusStatus {
            id: self.inner.id,
            term: core.view,
            leader: None,
            role: Role::Peer,
            last_log_index: (core.index > 0).then_some(core.index),
            last_applied: (core.index > 0).then_some(core.index),
        }
    }

    fn leader_commit(&self) -> watch::Receiver<Option<u64>> {
        self.inner.contact.subscribe()
    }

    /// The highest log position a member reports, so a returning node knows what to reach.
    fn read_index(&self) -> BoxFuture<'_, Result<Option<u64>, ProposeError>> {
        Box::pin(async move {
            let (index, joining, term) = {
                let core = lock(&self.inner.core);
                (core.index, core.joining, core.view)
            };
            let mut best = None;
            for client in self.inner.peers.iter() {
                let envelope = PeerEnvelope {
                    from: self.inner.id,
                    term,
                    body: PeerBody::Rounds(RoundsMsg::Applied),
                };
                if let Ok(PeerReply::Rounds(RoundsReply::Applied { index })) =
                    client.call(envelope, self.inner.rpc_timeout).await
                {
                    best = Some(best.map_or(index, |b: u64| b.max(index)));
                }
            }
            match best {
                Some(0) => Ok(None),
                Some(top) => Ok(Some(top)),
                None if joining => Err(ProposeError::Timeout),
                None => Ok((index > 0).then_some(index)),
            }
        })
    }

    fn read_index_local(&self) -> BoxFuture<'_, Result<Option<u64>, ProposeError>> {
        Box::pin(async move {
            let index = lock(&self.inner.core).index;
            Ok((index > 0).then_some(index))
        })
    }

    fn wait_applied(&self, index: u64, timeout: Duration) -> BoxFuture<'_, bool> {
        Box::pin(async move {
            let mut applied = self.inner.applied.subscribe();
            tokio::time::timeout(
                timeout,
                applied.wait_for(|now| now.is_some_and(|now| now >= index)),
            )
            .await
            .is_ok_and(|reached| reached.is_ok())
        })
    }

    fn set_elect(&self, _enabled: bool) {}

    fn set_quiet(&self, _quiet: bool) {}

    fn log_work(&self) -> LogWork {
        let stats = &self.inner.stats;
        let load = |value: &AtomicU64| value.load(Ordering::Relaxed);
        LogWork {
            answered: load(&stats.answered),
            answered_us: load(&stats.answered_us),
            writes: load(&stats.writes),
            writes_done: load(&stats.writes),
            write_us: 0,
            saves: load(&stats.syncs),
            save_us: load(&stats.sync_us),
        }
    }

    fn shutdown(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let tasks = std::mem::take(&mut *self.tasks.lock().unwrap_or_else(|p| p.into_inner()));
            for task in tasks {
                task.abort();
            }
        })
    }
}
