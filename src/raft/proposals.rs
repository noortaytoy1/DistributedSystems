//! The proposal queue: this node's one way into `raft.client_write`.
//!
//! openraft saves each client write to disk inside its core task, and it works through every write
//! already queued before it looks at its timers. N queued writes therefore hold the core for N disk
//! syncs with no heartbeats, and the followers start elections although nothing has failed. So the
//! pile stays here, where the core cannot see it:
//!
//! - **One write at a time.** At most one `client_write` per node is in flight, so the core is free
//!   between saves and heartbeats go out on time.
//! - **Groups.** Whatever arrives while a write is in flight goes out together as the next write,
//!   up to [`GROUP_LIMIT`] commands: one log entry, one disk sync and one replication round for all
//!   of them. Under light load a group is one command and nothing waits, so this adds no delay.
//! - **Deadlines.** A proposal leaves with `ProposeError::Timeout` at its deadline, whether it is
//!   waiting here or its write is in flight. The node sends no reply and the client's retry takes
//!   care of it, as for a full worker pool (D48). The write itself goes on, and the next one starts
//!   only when it ends: under overload the proposal at the head is always about to expire, and a
//!   new write for each of those would pile up inside openraft after all. Only a write that outlasts
//!   the write cap (a leader without a majority) stops being waited for. If it commits after its
//!   proposals have left that is harmless, because every command is idempotent.
//! - **Holding.** A leader stepping aside holds the queue, because every write it sends resets the
//!   followers' election timers (D218). A write in flight finishes and no new one starts. Whatever
//!   waits keeps its deadline, and when the hold ends it is written, or told who leads now.
//!
//! Owner: A (Raft and replication).

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::proto::{Command, CommandGroup, Outcome, ProposeError};

/// Most commands in one log entry.
///
/// High enough not to limit a run on a healthy disk: a round is two saves and some work, about 14 ms
/// at 3000 users on one PC, which allows about 14,000 commands per second against about 6000 from
/// 3000 users (one request each and a release for each, with 3000 responders). On a slow disk it is
/// the limit: with 25 ms added to each save a round took 62 ms, which allows 3200, so proposals wait
/// out their deadline there, and by arithmetic about 15 ms a save is where that starts (D228). Low
/// enough for the wire: even `max_payload_entries` (300) full groups in one AppendEntries would be
/// at most 7.4 MB (123 bytes per command at the longest), well under `MAX_FRAME_BYTES` (16 MiB). The
/// test `a_full_catch_up_message_fits_in_a_frame` holds this to the real encoding, and in practice
/// one message carries far less (`store::MAX_COMMANDS_PER_MESSAGE`).
pub const GROUP_LIMIT: usize = 200;

/// One proposal waiting for its write.
struct Waiting {
    command: Command,
    deadline: Instant,
    reply: oneshot::Sender<Result<Outcome, ProposeError>>,
}

/// What the queue has done. Counts are cumulative, for the once-a-second `raft_io` event.
#[derive(Debug, Default)]
pub struct QueueStats {
    /// Proposals in the queue now, not counting those of the write in flight.
    pub waiting: AtomicUsize,
    /// Writes started.
    pub writes: AtomicU64,
    /// Commands those writes carried.
    pub commands: AtomicU64,
    /// Proposals that left the queue past their deadline, or with their caller gone, unwritten.
    pub expired: AtomicU64,
    /// Writes that outlasted the write cap and stopped being waited for.
    pub write_timeouts: AtomicU64,
    /// Proposals answered, and the time they took from entering the queue to being answered. This
    /// grows with the queue, so it says how much is arriving as well as how fast the log is.
    pub answered: AtomicU64,
    pub answered_us: AtomicU64,
    /// Writes finished, and the time they took from `client_write` to its answer, queueing not
    /// included: one save here, one on a follower and the work around them. Grouping keeps this flat
    /// under load and it does not grow with the queue, so it is what the busy-leader check reads (D228).
    pub writes_done: AtomicU64,
    pub write_us: AtomicU64,
}

/// See the module docs. Dropping it stops the queue's task.
pub struct ProposalQueue {
    tx: mpsc::UnboundedSender<Waiting>,
    stats: Arc<QueueStats>,
    worker: JoinHandle<()>,
    held: watch::Sender<bool>,
}

impl std::fmt::Debug for ProposalQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProposalQueue")
            .field("stats", &self.stats)
            .finish()
    }
}

impl ProposalQueue {
    /// Start the queue. `write` commits one group and returns one outcome per command, in order;
    /// it gets `write_cap` before the queue moves on without it.
    pub fn start<W, F>(limit: usize, write_cap: Duration, write: W) -> Self
    where
        W: Fn(CommandGroup) -> F + Send + 'static,
        F: Future<Output = Result<Vec<Outcome>, ProposeError>> + Send + 'static,
    {
        let (tx, rx) = mpsc::unbounded_channel();
        let stats = Arc::new(QueueStats::default());
        let (held, held_rx) = watch::channel(false);
        let worker = tokio::spawn(run(
            rx,
            limit.max(1),
            write_cap,
            write,
            stats.clone(),
            held_rx,
        ));
        Self {
            tx,
            stats,
            worker,
            held,
        }
    }

    /// Wait until `command` is committed and applied, or until `deadline`.
    ///
    /// The queue is unbounded in length but not in time: nothing stays past its deadline, so its
    /// length is at most what arrives in one `propose_timeout_ms`, and arrivals are limited by the
    /// worker pool and by the responders held (releases).
    pub async fn submit(
        &self,
        command: Command,
        deadline: Instant,
    ) -> Result<Outcome, ProposeError> {
        let started = Instant::now();
        let (reply, answer) = oneshot::channel();
        self.stats.waiting.fetch_add(1, Ordering::Relaxed);
        let waiting = Waiting {
            command,
            deadline,
            reply,
        };
        if self.tx.send(waiting).is_err() {
            self.stats.waiting.fetch_sub(1, Ordering::Relaxed);
            return Err(stopped());
        }
        let result = match tokio::time::timeout_at(deadline, answer).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(stopped()),
            // Dropping `answer` closes the reply channel, which takes this proposal off the queue.
            Err(_) => Err(ProposeError::Timeout),
        };
        self.stats.answered.fetch_add(1, Ordering::Relaxed);
        self.stats
            .answered_us
            .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
        result
    }

    pub fn stats(&self) -> &QueueStats {
        &self.stats
    }

    /// Start no new write until this is called with false (D218). A write in flight finishes.
    /// The hold lives in this object and in no file, so a restarted node starts unheld.
    pub fn hold(&self, held: bool) {
        self.held.send_replace(held);
    }

    /// Stop now. Proposals still waiting get `ProposeError::Fatal`.
    pub fn stop(&self) {
        self.worker.abort();
    }
}

impl Drop for ProposalQueue {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

fn stopped() -> ProposeError {
    ProposeError::Fatal("the proposal queue stopped".into())
}

/// Ends when the queue is dropped.
async fn run<W, F>(
    mut rx: mpsc::UnboundedReceiver<Waiting>,
    limit: usize,
    write_cap: Duration,
    write: W,
    stats: Arc<QueueStats>,
    mut held: watch::Receiver<bool>,
) where
    W: Fn(CommandGroup) -> F,
    F: Future<Output = Result<Vec<Outcome>, ProposeError>>,
{
    let mut taken = Vec::with_capacity(limit);
    while rx.recv_many(&mut taken, limit).await > 0 {
        // The deadlines below are read after this, and each caller leaves by its own deadline.
        let _ = held.wait_for(|held| !*held).await;
        stats.waiting.fetch_sub(taken.len(), Ordering::Relaxed);
        let now = Instant::now();
        let mut commands = Vec::with_capacity(taken.len());
        let mut replies = Vec::with_capacity(taken.len());
        for waiting in taken.drain(..) {
            // Its caller has left or is about to, so it must not cost a save.
            if waiting.deadline <= now || waiting.reply.is_closed() {
                stats.expired.fetch_add(1, Ordering::Relaxed);
                let _ = waiting.reply.send(Err(ProposeError::Timeout));
                continue;
            }
            commands.push(waiting.command);
            replies.push(waiting.reply);
        }
        if commands.is_empty() {
            continue;
        }
        stats.writes.fetch_add(1, Ordering::Relaxed);
        stats
            .commands
            .fetch_add(commands.len() as u64, Ordering::Relaxed);
        let started = Instant::now();
        let written = tokio::time::timeout(write_cap, write(CommandGroup(commands))).await;
        stats
            .write_us
            .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
        stats.writes_done.fetch_add(1, Ordering::Relaxed);
        let result = written.unwrap_or_else(|_| {
            stats.write_timeouts.fetch_add(1, Ordering::Relaxed);
            Err(ProposeError::Timeout)
        });
        answer(replies, result);
    }
}

/// A caller that has left does not mind: `send` just fails.
fn answer(
    replies: Vec<oneshot::Sender<Result<Outcome, ProposeError>>>,
    result: Result<Vec<Outcome>, ProposeError>,
) {
    match result {
        Ok(outcomes) => {
            let mut outcomes = outcomes.into_iter();
            for reply in replies {
                let one = outcomes
                    .next()
                    .ok_or_else(|| ProposeError::Fatal("a group came back short".into()));
                let _ = reply.send(one);
            }
        }
        Err(e) => {
            for reply in replies {
                let _ = reply.send(Err(e.clone()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::campus;
    use crate::config::CampusConfig;
    use crate::proto::{
        self, Capability, Category, Incident, PeerRpcRequest, Plan, Point, RequestId, ResponderId,
        ResultProposal, StaleReason, UserId,
    };
    use crate::raft::TypeConfig;
    use openraft::raft::AppendEntriesRequest;
    use openraft::{CommittedLeaderId, Entry, EntryPayload, LogId, Vote};
    use rand::SeedableRng;
    use rand::rngs::StdRng;
    use std::sync::Mutex;
    use std::time::Duration;
    use tokio::sync::Semaphore;

    const LONG: Duration = Duration::from_secs(30);

    fn command(n: u64) -> Command {
        Command::ReleaseResponder {
            responder: n as ResponderId,
            request: RequestId { client: 1, seq: n },
        }
    }

    /// An outcome that names its command, so a test can tell who got whose answer.
    fn echo(cmd: &Command) -> Outcome {
        let Command::ReleaseResponder { responder, request } = cmd else {
            panic!("tests only use releases");
        };
        Outcome::Stale {
            request: *request,
            responder: *responder,
            reason: StaleReason::UnknownResponder,
        }
    }

    /// Stands in for Raft: each write waits for one permit, and everything is recorded.
    struct FakeRaft {
        permits: Semaphore,
        in_flight: AtomicUsize,
        most_in_flight: AtomicUsize,
        groups: Mutex<Vec<Vec<Command>>>,
    }

    impl Default for FakeRaft {
        fn default() -> Self {
            Self {
                permits: Semaphore::new(0),
                in_flight: AtomicUsize::new(0),
                most_in_flight: AtomicUsize::new(0),
                groups: Mutex::new(Vec::new()),
            }
        }
    }

    impl FakeRaft {
        fn groups(&self) -> Vec<Vec<Command>> {
            self.groups.lock().unwrap().clone()
        }

        fn in_flight(&self) -> usize {
            self.in_flight.load(Ordering::SeqCst)
        }

        async fn write(self: Arc<Self>, group: CommandGroup) -> Result<Vec<Outcome>, ProposeError> {
            let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.most_in_flight.fetch_max(now, Ordering::SeqCst);
            self.groups.lock().unwrap().push(group.0.clone());
            // Counted down even if the queue stops waiting for this write.
            let _left = InFlight(&self.in_flight);
            let permit = self.permits.acquire().await;
            permit.unwrap().forget();
            Ok(group.0.iter().map(echo).collect())
        }
    }

    struct InFlight<'a>(&'a AtomicUsize);

    impl Drop for InFlight<'_> {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }

    fn queue(limit: usize, raft: &Arc<FakeRaft>) -> Arc<ProposalQueue> {
        let raft = raft.clone();
        Arc::new(ProposalQueue::start(limit, LONG, move |group| {
            raft.clone().write(group)
        }))
    }

    fn submit(
        queue: &Arc<ProposalQueue>,
        n: u64,
        wait: Duration,
    ) -> JoinHandle<Result<Outcome, ProposeError>> {
        let queue = queue.clone();
        let deadline = Instant::now() + wait;
        tokio::spawn(async move { queue.submit(command(n), deadline).await })
    }

    /// Poll until `cond` holds; there is no fixed sleep to be too short on a slow machine.
    async fn wait_until(what: &str, cond: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !cond() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn only_one_write_is_in_flight_and_the_next_carries_all_that_waited() {
        let raft = Arc::new(FakeRaft::default());
        let q = queue(100, &raft);
        let first = submit(&q, 0, LONG);
        wait_until("the first write", || raft.in_flight() == 1).await;

        let waiting: Vec<_> = (1..=10).map(|n| submit(&q, n, LONG)).collect();
        wait_until("all ten waiting", || {
            q.stats().waiting.load(Ordering::SeqCst) == 10
        })
        .await;
        assert_eq!(raft.groups().len(), 1, "nothing else may start yet");

        raft.permits.add_permits(2);
        assert_eq!(first.await.unwrap().unwrap(), echo(&command(0)));
        for (n, task) in (1..=10).zip(waiting) {
            assert_eq!(task.await.unwrap().unwrap(), echo(&command(n)));
        }
        assert_eq!(raft.most_in_flight.load(Ordering::SeqCst), 1);
        let groups = raft.groups();
        assert_eq!(groups.len(), 2, "one write for the ten: {groups:?}");
        assert_eq!(groups[1].len(), 10);
        assert_eq!(q.stats().writes.load(Ordering::SeqCst), 2);
        assert_eq!(q.stats().commands.load(Ordering::SeqCst), 11);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_group_stops_at_the_limit_and_order_is_kept() {
        let raft = Arc::new(FakeRaft::default());
        let q = queue(4, &raft);
        let first = submit(&q, 0, LONG);
        wait_until("the first write", || raft.in_flight() == 1).await;
        // In arrival order: each is in the queue before the next is sent.
        let mut waiting = Vec::new();
        for n in 1..=9 {
            waiting.push(submit(&q, n, LONG));
            wait_until("it to wait", || {
                q.stats().waiting.load(Ordering::SeqCst) == n as usize
            })
            .await;
        }
        raft.permits.add_permits(4);
        first.await.unwrap().unwrap();
        for task in waiting {
            task.await.unwrap().unwrap();
        }
        let groups = raft.groups();
        let sizes: Vec<_> = groups.iter().map(Vec::len).collect();
        assert_eq!(sizes, vec![1, 4, 4, 1]);
        let sent: Vec<_> = groups.iter().flatten().cloned().collect();
        assert_eq!(sent, (0..=9).map(command).collect::<Vec<_>>());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_proposal_past_its_deadline_leaves_with_timeout_and_is_never_written() {
        let raft = Arc::new(FakeRaft::default());
        let q = queue(100, &raft);
        let first = submit(&q, 0, LONG);
        wait_until("the first write", || raft.in_flight() == 1).await;

        let started = Instant::now();
        let late = submit(&q, 1, Duration::from_millis(20));
        assert_eq!(late.await.unwrap(), Err(ProposeError::Timeout));
        assert!(started.elapsed() >= Duration::from_millis(20));

        let next = submit(&q, 2, LONG);
        raft.permits.add_permits(2);
        first.await.unwrap().unwrap();
        next.await.unwrap().unwrap();
        let groups = raft.groups();
        assert_eq!(groups, vec![vec![command(0)], vec![command(2)]]);
        assert_eq!(q.stats().expired.load(Ordering::SeqCst), 1);
    }

    /// Under overload the proposal at the head of the queue is always about to expire. Its write
    /// must still hold the queue until it ends, or writes pile up inside openraft.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_caller_that_leaves_does_not_let_a_second_write_start() {
        let raft = Arc::new(FakeRaft::default());
        let q = queue(100, &raft);
        let leaving = submit(&q, 0, Duration::from_millis(20));
        wait_until("the first write", || raft.in_flight() == 1).await;
        assert_eq!(leaving.await.unwrap(), Err(ProposeError::Timeout));

        let next = submit(&q, 1, LONG);
        wait_until("the next to wait", || {
            q.stats().waiting.load(Ordering::SeqCst) == 1
        })
        .await;
        assert_eq!(raft.groups().len(), 1, "the first write is still in flight");
        assert_eq!(q.stats().write_timeouts.load(Ordering::SeqCst), 0);

        raft.permits.add_permits(2);
        assert_eq!(next.await.unwrap().unwrap(), echo(&command(1)));
        assert_eq!(raft.most_in_flight.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_write_that_never_ends_stops_holding_up_the_queue_after_the_cap() {
        let raft = Arc::new(FakeRaft::default());
        let writer = raft.clone();
        let q = Arc::new(ProposalQueue::start(
            100,
            Duration::from_millis(20),
            move |group| writer.clone().write(group),
        ));
        let stuck = submit(&q, 0, LONG);
        assert_eq!(stuck.await.unwrap(), Err(ProposeError::Timeout));
        assert_eq!(q.stats().write_timeouts.load(Ordering::SeqCst), 1);

        let next = submit(&q, 1, LONG);
        wait_until("the next write", || raft.groups().len() == 2).await;
        raft.permits.add_permits(1);
        assert_eq!(next.await.unwrap().unwrap(), echo(&command(1)));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_held_queue_starts_no_write_until_it_is_released() {
        let raft = Arc::new(FakeRaft::default());
        let q = queue(100, &raft);
        q.hold(true);
        let waiting: Vec<_> = (0..5).map(|n| submit(&q, n, LONG)).collect();
        wait_until("all five waiting", || {
            q.stats().waiting.load(Ordering::SeqCst) == 5
        })
        .await;
        assert!(raft.groups().is_empty(), "held: {:?}", raft.groups());

        q.hold(false);
        raft.permits.add_permits(5);
        for (n, task) in (0..5).zip(waiting) {
            assert_eq!(task.await.unwrap().unwrap(), echo(&command(n)));
        }
        assert_eq!(q.stats().waiting.load(Ordering::SeqCst), 0);
        assert_eq!(raft.most_in_flight.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn holding_lets_the_write_in_flight_finish() {
        let raft = Arc::new(FakeRaft::default());
        let q = queue(100, &raft);
        let first = submit(&q, 0, LONG);
        wait_until("the first write", || raft.in_flight() == 1).await;

        q.hold(true);
        let next = submit(&q, 1, LONG);
        raft.permits.add_permits(2);
        assert_eq!(first.await.unwrap().unwrap(), echo(&command(0)));
        wait_until("the next to wait", || {
            q.stats().waiting.load(Ordering::SeqCst) == 1
        })
        .await;
        assert_eq!(raft.groups().len(), 1, "no new write while held");

        q.hold(false);
        assert_eq!(next.await.unwrap().unwrap(), echo(&command(1)));
        assert_eq!(raft.groups().len(), 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_held_proposal_still_leaves_at_its_deadline_and_is_never_written() {
        let raft = Arc::new(FakeRaft::default());
        let q = queue(100, &raft);
        q.hold(true);
        let late = submit(&q, 0, Duration::from_millis(20));
        assert_eq!(late.await.unwrap(), Err(ProposeError::Timeout));

        let next = submit(&q, 1, LONG);
        wait_until("the next to wait", || {
            q.stats().waiting.load(Ordering::SeqCst) == 2
        })
        .await;
        q.hold(false);
        raft.permits.add_permits(1);
        assert_eq!(next.await.unwrap().unwrap(), echo(&command(1)));
        assert_eq!(raft.groups(), vec![vec![command(1)]]);
        assert_eq!(q.stats().expired.load(Ordering::SeqCst), 1);
        assert_eq!(q.stats().waiting.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_answer_counts_the_time_it_took_from_entering_the_queue() {
        let raft = Arc::new(FakeRaft::default());
        let q = queue(100, &raft);
        q.hold(true);
        let first = submit(&q, 0, LONG);
        wait_until("it to wait", || {
            q.stats().waiting.load(Ordering::SeqCst) == 1
        })
        .await;
        assert_eq!(q.stats().answered.load(Ordering::SeqCst), 0);
        let held_for = Instant::now();
        wait_until("some time to pass", || {
            held_for.elapsed() >= Duration::from_millis(30)
        })
        .await;

        q.hold(false);
        raft.permits.add_permits(1);
        first.await.unwrap().unwrap();
        assert_eq!(q.stats().answered.load(Ordering::SeqCst), 1);
        let took = q.stats().answered_us.load(Ordering::SeqCst);
        assert!(took >= 30_000, "{took} us");

        // One that leaves at its deadline took all of that time too: it is the strongest sign.
        q.hold(true);
        let late = submit(&q, 1, Duration::from_millis(20));
        assert_eq!(late.await.unwrap(), Err(ProposeError::Timeout));
        assert_eq!(q.stats().answered.load(Ordering::SeqCst), 2);
        assert!(q.stats().answered_us.load(Ordering::SeqCst) >= took + 20_000);
    }

    /// A saturated log has a long queue and writes of the usual length, on whichever node leads, so
    /// the busy-leader check must be able to read the two apart (D228).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_write_counts_its_own_time_and_not_the_time_its_proposals_queued() {
        let raft = Arc::new(FakeRaft::default());
        let q = queue(100, &raft);
        q.hold(true);
        let first = submit(&q, 0, LONG);
        wait_until("it to wait", || {
            q.stats().waiting.load(Ordering::SeqCst) == 1
        })
        .await;
        let queued_for = Instant::now();
        wait_until("some time in the queue", || {
            queued_for.elapsed() >= Duration::from_millis(60)
        })
        .await;
        assert_eq!(q.stats().writes_done.load(Ordering::SeqCst), 0);

        q.hold(false);
        wait_until("the write", || raft.in_flight() == 1).await;
        let written_for = Instant::now();
        wait_until("some time in the write", || {
            written_for.elapsed() >= Duration::from_millis(20)
        })
        .await;
        assert_eq!(q.stats().writes_done.load(Ordering::SeqCst), 0, "not yet");
        raft.permits.add_permits(1);
        first.await.unwrap().unwrap();

        assert_eq!(q.stats().writes_done.load(Ordering::SeqCst), 1);
        let write_us = q.stats().write_us.load(Ordering::SeqCst);
        let answered_us = q.stats().answered_us.load(Ordering::SeqCst);
        assert!(write_us >= 20_000, "{write_us} us");
        assert!(
            answered_us >= write_us + 60_000,
            "{write_us} us of {answered_us} us"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_failed_write_answers_everyone_in_its_group() {
        let not_leader = ProposeError::NotLeader { leader: Some(2) };
        let expected = not_leader.clone();
        let q = Arc::new(ProposalQueue::start(10, LONG, move |_| {
            let e = not_leader.clone();
            async move { Err(e) }
        }));
        let tasks: Vec<_> = (0..5).map(|n| submit(&q, n, LONG)).collect();
        for task in tasks {
            assert_eq!(task.await.unwrap(), Err(expected.clone()));
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_short_answer_is_an_error_not_a_hang() {
        let q = queue_with_answer(Vec::new());
        assert!(matches!(
            submit(&q, 0, LONG).await.unwrap(),
            Err(ProposeError::Fatal(_))
        ));
    }

    fn queue_with_answer(outcomes: Vec<Outcome>) -> Arc<ProposalQueue> {
        Arc::new(ProposalQueue::start(10, LONG, move |_| {
            let outcomes = outcomes.clone();
            async move { Ok(outcomes) }
        }))
    }

    /// Two commands of the wire's longest shape and of a real load generator's shape.
    fn result(request: RequestId, incident: Incident, cost: u64, term: u64) -> Command {
        Command::CommitResult(ResultProposal {
            request,
            incident,
            executor: 3,
            term,
            plan: Plan {
                responder: Some(300),
                cost,
                priority: 460,
            },
            age_ms: 8,
            assigned_at_ms: 1_792_000_000_000,
        })
    }

    fn longest_command() -> Command {
        let mut command = result(
            RequestId {
                client: u64::MAX,
                seq: u64::MAX,
            },
            Incident {
                owner: UserId {
                    client: u64::MAX,
                    user: u32::MAX,
                },
                category: Category::TechnicalAssistance,
                zone: u16::MAX,
                location: Point {
                    x: i32::MIN,
                    y: i32::MIN,
                },
                severity: 5,
                needs: Capability::Rescue,
                reported_at_ms: u64::MAX,
                ttl_ms: u64::MAX,
            },
            u64::MAX,
            u64::MAX,
        );
        if let Command::CommitResult(p) = &mut command {
            p.executor = u64::MAX;
            p.plan.responder = Some(u32::MAX);
            p.plan.priority = u32::MAX;
            p.age_ms = u64::MAX;
            p.assigned_at_ms = u64::MAX;
        }
        command
    }

    /// What a load generator really sends: a campus incident and a microsecond clock for the seq.
    fn real_command(seq: u64) -> Command {
        let cfg = CampusConfig::default();
        let mut rng = StdRng::seed_from_u64(seq);
        result(
            RequestId {
                client: 3,
                seq: 1_792_000_000_000_000 + seq,
            },
            campus::random_incident(
                &mut rng,
                &cfg,
                UserId {
                    client: 3,
                    user: 12,
                },
                1_792_000_000_000,
            ),
            1_500,
            7,
        )
    }

    /// The message a returning node can be sent: `max_payload_entries` entries of `limit` commands.
    fn catch_up_message(make: impl Fn(u64) -> Command) -> PeerRpcRequest {
        let entries = openraft::Config::default().max_payload_entries;
        let last = LogId::new(CommittedLeaderId::new(u64::MAX, u64::MAX), u64::MAX);
        let entries: Vec<Entry<TypeConfig>> = (0..entries)
            .map(|i| Entry {
                log_id: LogId::new(CommittedLeaderId::new(u64::MAX, u64::MAX), u64::MAX - i),
                payload: EntryPayload::Normal(CommandGroup(
                    (0..GROUP_LIMIT as u64).map(&make).collect(),
                )),
            })
            .collect();
        PeerRpcRequest::AppendEntries {
            from: u64::MAX,
            term: u64::MAX,
            rpc: AppendEntriesRequest {
                vote: Vote::new_committed(u64::MAX, u64::MAX),
                prev_log_id: Some(last),
                entries,
                leader_commit: Some(last),
            },
        }
    }

    /// The arithmetic in [`GROUP_LIMIT`]'s comment, on the real encoding. `encode` also fails on
    /// anything over `MAX_FRAME_BYTES`, and a Raft log file record goes through the same limit.
    #[test]
    fn a_full_catch_up_message_fits_in_a_frame() {
        // tarpc's own wrapping around a request: IDs, deadline and trace context.
        const TARPC_WRAPPING: usize = 256;
        let per_message = |make: fn(u64) -> Command| {
            let bytes = proto::encode(&catch_up_message(make)).unwrap().len();
            assert!(
                bytes + TARPC_WRAPPING < proto::MAX_FRAME_BYTES,
                "{bytes} bytes do not fit in a frame"
            );
            bytes
        };
        let longest = per_message(|_| longest_command());
        let real = per_message(real_command);
        let one_long = proto::encode(&longest_command()).unwrap().len();
        let one_real = proto::encode(&real_command(1)).unwrap().len();
        eprintln!(
            "one command: longest {one_long} bytes, real {one_real} bytes; \
             a full message of 300 x {GROUP_LIMIT}: longest {longest} bytes, real {real} bytes, \
             frame limit {}",
            proto::MAX_FRAME_BYTES
        );
        assert!(longest < proto::MAX_FRAME_BYTES / 2, "keep a wide margin");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_stopped_queue_answers_with_fatal() {
        let raft = Arc::new(FakeRaft::default());
        let q = queue(10, &raft);
        let waiting = submit(&q, 0, LONG);
        wait_until("the write", || raft.in_flight() == 1).await;
        q.stop();
        assert!(matches!(
            waiting.await.unwrap(),
            Err(ProposeError::Fatal(_))
        ));
        wait_until("the worker to end", || q.worker.is_finished()).await;
        assert!(matches!(
            submit(&q, 1, LONG).await.unwrap(),
            Err(ProposeError::Fatal(_))
        ));
    }
}
