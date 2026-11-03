//! Test doubles for the unit tests in `node/`: a scripted [`Consensus`] and a [`NodeContext`] built
//! around it. No sockets and no Raft, so a test can hold a node in a state a real cluster reaches
//! only by luck, such as a catch-up target that never commits.
//!
//! Owner: everyone (test code only).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::future::BoxFuture;
use tokio::sync::{Semaphore, watch};
use tokio::time::Instant;

use super::bids::BidState;
use super::degraded::DegradedState;
use super::faults::FaultElection;
use super::group::CloudGroup;
use super::heartbeat::Suspicion;
use super::transport::{FaultGate, PeerSet};
use super::workers::WorkerPool;
use super::{NodeContext, lock};
use crate::config::Config;
use crate::metrics::EventLog;
use crate::proto::{Command, CommittedResult, NodeId, Outcome, PeerReply, ProposeError, RaftRpc};
use crate::raft::store::StateReader;
use crate::raft::{Consensus, ConsensusStatus, Role};

const THREE_NODES: &str = r#"
[[cluster.nodes]]
id = 1
peer_addr = "a:1"
client_addr = "a:2"
[[cluster.nodes]]
id = 2
peer_addr = "b:1"
client_addr = "b:2"
[[cluster.nodes]]
id = 3
peer_addr = "c:1"
client_addr = "c:2"
"#;

/// A [`Consensus`] whose every answer the test decides.
pub(crate) struct ScriptedConsensus {
    id: NodeId,
    term: AtomicU64,
    leader: Mutex<Option<NodeId>>,
    quiet: AtomicBool,
    answers: Mutex<VecDeque<Result<Option<u64>, ProposeError>>>,
    questions: AtomicUsize,
    applied: watch::Sender<Option<u64>>,
    heard: watch::Sender<Option<u64>>,
    /// One permit for each proposal that has arrived.
    proposed: Semaphore,
    /// One permit for each proposal the test lets commit.
    commits: Semaphore,
}

impl ScriptedConsensus {
    pub(crate) fn new(id: NodeId) -> Arc<Self> {
        Arc::new(Self {
            id,
            term: AtomicU64::new(1),
            leader: Mutex::new(None),
            quiet: AtomicBool::new(false),
            answers: Mutex::new(VecDeque::new()),
            questions: AtomicUsize::new(0),
            applied: watch::channel(None).0,
            heard: watch::channel(None).0,
            proposed: Semaphore::new(0),
            commits: Semaphore::new(0),
        })
    }

    pub(crate) fn set_leader(&self, leader: Option<NodeId>) {
        *lock(&self.leader) = leader;
    }

    pub(crate) fn set_term(&self, term: u64) {
        self.term.store(term, Ordering::SeqCst);
    }

    /// What the last `set_quiet` said.
    pub(crate) fn is_quiet(&self) -> bool {
        self.quiet.load(Ordering::SeqCst)
    }

    /// What `read_index` answers, one per question. The last answer repeats; with none it fails.
    pub(crate) fn script_answers(
        &self,
        answers: impl IntoIterator<Item = Result<Option<u64>, ProposeError>>,
    ) {
        *lock(&self.answers) = answers.into_iter().collect();
    }

    /// How many times `read_index` was called.
    pub(crate) fn questions(&self) -> usize {
        self.questions.load(Ordering::SeqCst)
    }

    pub(crate) fn apply_up_to(&self, index: u64) {
        self.applied.send_replace(Some(index));
    }

    /// As if an AppendEntries from a current leader was accepted.
    pub(crate) fn hear_from_leader(&self) {
        self.heard.send_replace(None);
    }

    /// Resolves when the next proposal has arrived.
    pub(crate) async fn proposal_arrived(&self) {
        self.proposed
            .acquire()
            .await
            .expect("never closed")
            .forget();
    }

    /// Lets one waiting proposal commit.
    pub(crate) fn commit_one(&self) {
        self.commits.add_permits(1);
    }
}

impl Consensus for ScriptedConsensus {
    /// Waits for [`Self::commit_one`], so a test decides what happens while a handler is waiting.
    fn propose(&self, cmd: Command) -> BoxFuture<'_, Result<Outcome, ProposeError>> {
        Box::pin(async move {
            self.proposed.add_permits(1);
            self.commits.acquire().await.expect("never closed").forget();
            Ok(match cmd {
                Command::CommitResult(p) => Outcome::Committed(CommittedResult {
                    request: p.request,
                    incident: p.incident,
                    executor: p.executor,
                    term: p.term,
                    plan: p.plan,
                    age_ms: p.age_ms,
                    assigned_at_ms: p.assigned_at_ms,
                    log_index: 1,
                }),
                _ => Outcome::Applied,
            })
        })
    }

    fn propose_local(&self, cmd: Command) -> BoxFuture<'_, Result<Outcome, ProposeError>> {
        self.propose(cmd)
    }

    fn handle_rpc(&self, _from: NodeId, _rpc: RaftRpc) -> BoxFuture<'_, PeerReply> {
        Box::pin(async { PeerReply::Ack })
    }

    fn status(&self) -> ConsensusStatus {
        let leader = *lock(&self.leader);
        let applied = *self.applied.borrow();
        ConsensusStatus {
            id: self.id,
            term: self.term.load(Ordering::SeqCst),
            leader,
            role: if leader == Some(self.id) {
                Role::Leader
            } else {
                Role::Follower
            },
            last_log_index: applied,
            last_applied: applied,
        }
    }

    fn leader_commit(&self) -> watch::Receiver<Option<u64>> {
        self.heard.subscribe()
    }

    fn read_index(&self) -> BoxFuture<'_, Result<Option<u64>, ProposeError>> {
        Box::pin(async move {
            self.questions.fetch_add(1, Ordering::SeqCst);
            let mut answers = lock(&self.answers);
            if answers.len() > 1 {
                answers.pop_front().expect("checked above")
            } else {
                answers
                    .front()
                    .cloned()
                    .unwrap_or(Err(ProposeError::Timeout))
            }
        })
    }

    fn read_index_local(&self) -> BoxFuture<'_, Result<Option<u64>, ProposeError>> {
        self.read_index()
    }

    fn wait_applied(&self, index: u64, timeout: Duration) -> BoxFuture<'_, bool> {
        Box::pin(async move {
            let mut applied = self.applied.subscribe();
            let reached = applied.wait_for(|a| a.is_some_and(|a| a >= index));
            matches!(tokio::time::timeout(timeout, reached).await, Ok(Ok(_)))
        })
    }

    fn set_elect(&self, _enabled: bool) {}

    fn set_quiet(&self, quiet: bool) {
        self.quiet.store(quiet, Ordering::SeqCst);
    }

    fn shutdown(&self) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

/// A serving node 1 of a three-node cluster on `consensus`. Nothing is started: no listeners, no
/// background tasks.
pub(crate) fn context(
    consensus: Arc<dyn Consensus>,
    log: EventLog,
    tweak: impl FnOnce(&mut Config),
) -> Arc<NodeContext> {
    let mut config = Config::from_toml(THREE_NODES).expect("a valid config");
    tweak(&mut config);
    let id = consensus.status().id;
    let gate = FaultGate::default();
    let peers = Arc::new(PeerSet::new(
        id,
        &config.cluster,
        &gate,
        &log,
        Duration::from_millis(100),
    ));
    let group = Arc::new(CloudGroup::new(
        id,
        &config.cluster,
        peers.clone(),
        consensus.clone(),
        log.clone(),
    ));
    Arc::new(NodeContext {
        id,
        consensus,
        state: StateReader::default(),
        gate,
        peers,
        group,
        workers: WorkerPool::new(config.workers.threads, config.workers.queue_capacity),
        log,
        suspicion: Mutex::new(Suspicion::new(
            config.timers.peer_heartbeat(),
            config.timers.takeover_missed_heartbeats,
        )),
        serving: AtomicBool::new(true),
        bids: BidState::new(id, &config),
        fault_election: FaultElection::new(id, &config),
        plan_work: AtomicU32::new(config.workers.plan_work),
        degraded: DegradedState::default(),
        config: Arc::new(config),
    })
}

/// An empty directory for one test's event log.
pub(crate) fn log_dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rescuemesh-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Every event node 1 has logged in `dir` so far.
pub(crate) fn logged(log: &EventLog, dir: &Path) -> Vec<serde_json::Value> {
    log.flush();
    let text = std::fs::read_to_string(dir.join("node-1.jsonl")).unwrap_or_default();
    text.lines()
        .map(|line| serde_json::from_str(line).expect("one JSON object per line"))
        .collect()
}

/// Polls `cond` until it holds; panics with `what` if that takes too long.
pub(crate) async fn until(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
