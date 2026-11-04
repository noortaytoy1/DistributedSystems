//! Raft log and state machine storage for openraft (storage API v2).
//!
//! The log and vote live in memory, and with `storage.dir` set they also go to a write-ahead file
//! first (wal.rs), so a node that lost power comes back with its vote and log (D118). The state
//! machine is always in memory: it is rebuilt by replaying the log. Without `storage.dir` a
//! restarted node has forgotten its vote, which Raft forbids; only tests run that way.
//!
//! Snapshots are implemented so openraft can use them, but the node config never triggers one
//! (`SnapshotPolicy::Never`), so the log is never purged and a returning node catches up by log
//! replay.
//!
//! Owner: A (Raft and replication).

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::io::Cursor;
use std::ops::RangeBounds;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockWriteGuard};

use openraft::storage::{LogFlushed, RaftLogStorage, RaftStateMachine};
use openraft::{
    BasicNode, Entry, EntryPayload, LogId, LogState, OptionalSend, RaftLogReader,
    RaftSnapshotBuilder, Snapshot, SnapshotMeta, StorageError, StorageIOError, StoredMembership,
    Vote,
};

use crate::metrics::{Event, EventLog};
use crate::proto::{CommandGroup, NodeId, Outcome};
use crate::raft::TypeConfig;
use crate::raft::wal::{Record, Replayed, Wal};
use crate::state::CloudState;

/// A node logs its state digest each time it applies a log index that is a multiple of this. Every
/// node applies every index, so the logs can be compared at the same index (D138).
pub const CHECKPOINT_EVERY: u64 = 100;

type StorageResult<T> = Result<T, StorageError<NodeId>>;

/// Most commands in one AppendEntries, counted over its log entries (at least one entry goes).
///
/// openraft gives each AppendEntries one `raft_heartbeat_ms` (50 ms) to be sent, saved and answered,
/// and on a timeout sends the same entries again. So a message too big for that is never
/// acknowledged, however often the follower saves it, and a returning node never catches up.
/// `max_payload_entries` (300) full groups would be far too big for that. This many is at most about
/// 504 KB, 4 ms on a 1 Gbit/s wire, and still 2 full groups per round. One group always goes, so a
/// full group of `proposals::GROUP_LIMIT` must itself fit in that time (D234).
pub const MAX_COMMANDS_PER_MESSAGE: usize = 4096;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

// ---------------------------------------------------------------------------------------------
// Shared read access
// ---------------------------------------------------------------------------------------------

/// Read access to the replicated state for the rest of the node. Cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct StateReader {
    inner: Arc<RwLock<CloudState>>,
}

impl StateReader {
    /// Run `f` on the current state. Keep `f` short: applying committed entries waits for it.
    pub fn with<R>(&self, f: impl FnOnce(&CloudState) -> R) -> R {
        f(&self.inner.read().unwrap_or_else(|p| p.into_inner()))
    }

    fn write(&self) -> RwLockWriteGuard<'_, CloudState> {
        self.inner.write().unwrap_or_else(|p| p.into_inner())
    }

    /// Apply one command with no log behind it, for a test that needs a state to read.
    #[cfg(test)]
    pub(crate) fn apply_for_tests(&self, cmd: crate::proto::Command) {
        self.write().apply(1, &cmd);
    }
}

// ---------------------------------------------------------------------------------------------
// Log
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Default)]
struct LogInner {
    vote: Option<Vote<NodeId>>,
    committed: Option<LogId<NodeId>>,
    last_purged: Option<LogId<NodeId>>,
    log: BTreeMap<u64, Entry<TypeConfig>>,
}

/// What the log store has done. Counts are cumulative, for the once-a-second `raft_io` event.
#[derive(Debug, Default)]
pub struct LogStats {
    /// Writes to the log file that waited for the disk.
    pub syncs: AtomicU64,
    /// Time spent in those writes.
    pub sync_us: AtomicU64,
    /// The longest one since this was last read.
    pub max_sync_us: AtomicU64,
    /// Calls to save log entries, and the entries and commands they carried.
    pub saves: AtomicU64,
    pub entries: AtomicU64,
    pub commands: AtomicU64,
}

fn commands_in(entry: &Entry<TypeConfig>) -> usize {
    match &entry.payload {
        EntryPayload::Normal(CommandGroup(commands)) => commands.len(),
        EntryPayload::Blank | EntryPayload::Membership(_) => 0,
    }
}

impl LogStats {
    fn saved(&self, entries: &[Entry<TypeConfig>]) {
        let commands: usize = entries.iter().map(commands_in).sum();
        self.saves.fetch_add(1, Ordering::Relaxed);
        self.entries
            .fetch_add(entries.len() as u64, Ordering::Relaxed);
        self.commands.fetch_add(commands as u64, Ordering::Relaxed);
    }

    fn synced(&self, took: std::time::Duration) {
        let us = took.as_micros() as u64;
        self.syncs.fetch_add(1, Ordering::Relaxed);
        self.sync_us.fetch_add(us, Ordering::Relaxed);
        self.max_sync_us.fetch_max(us, Ordering::Relaxed);
    }
}

/// The Raft log and vote. Clones share the same log, which is how openraft's log reader works.
#[derive(Debug, Clone, Default)]
pub struct LogStore {
    inner: Arc<Mutex<LogInner>>,
    /// The write-ahead file, if this node has `storage.dir` set.
    wal: Option<Arc<Mutex<Wal>>>,
    stats: Arc<LogStats>,
}

impl LogStore {
    /// Open the node's file and load what it holds. Returns true if the node ran here before.
    pub async fn open(path: PathBuf) -> anyhow::Result<(Self, bool)> {
        let (wal, replayed) = tokio::task::spawn_blocking(move || Wal::open(&path)).await??;
        let restarted = replayed.is_restart();
        let Replayed {
            vote,
            committed,
            last_purged,
            log,
        } = replayed;
        let store = Self {
            inner: Arc::new(Mutex::new(LogInner {
                vote,
                committed,
                last_purged,
                log,
            })),
            wal: Some(Arc::new(Mutex::new(wal))),
            stats: Arc::default(),
        };
        Ok((store, restarted))
    }

    pub fn stats(&self) -> Arc<LogStats> {
        self.stats.clone()
    }

    /// Write a record to the file off the async runtime. A no-op without a file. Callers await
    /// this before they change memory, so nothing is reported that a power cut could lose.
    async fn persist(&self, record: Record, sync: bool) -> StorageResult<()> {
        let Some(wal) = self.wal.clone() else {
            return Ok(());
        };
        let stats = self.stats.clone();
        let written = tokio::task::spawn_blocking(move || {
            let started = std::time::Instant::now();
            let written = lock(&wal).write(&record, sync);
            if sync {
                stats.synced(started.elapsed());
            }
            written
        })
        .await
        .map_err(std::io::Error::other)
        .and_then(|r| r);
        written.map_err(|e| StorageIOError::write_logs(&e).into())
    }
}

impl RaftLogReader<TypeConfig> for LogStore {
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + OptionalSend>(
        &mut self,
        range: RB,
    ) -> StorageResult<Vec<Entry<TypeConfig>>> {
        Ok(lock(&self.inner)
            .log
            .range(range)
            .map(|(_, e)| e.clone())
            .collect())
    }

    /// What replication reads for one AppendEntries. See [`MAX_COMMANDS_PER_MESSAGE`].
    async fn limited_get_log_entries(
        &mut self,
        start: u64,
        end: u64,
    ) -> StorageResult<Vec<Entry<TypeConfig>>> {
        let inner = lock(&self.inner);
        let mut commands = 0;
        let mut entries = Vec::new();
        for entry in inner.log.range(start..end).map(|(_, e)| e) {
            commands += commands_in(entry);
            if commands > MAX_COMMANDS_PER_MESSAGE && !entries.is_empty() {
                break;
            }
            entries.push(entry.clone());
        }
        Ok(entries)
    }
}

impl RaftLogStorage<TypeConfig> for LogStore {
    type LogReader = Self;

    async fn get_log_state(&mut self) -> StorageResult<LogState<TypeConfig>> {
        let inner = lock(&self.inner);
        let last = inner
            .log
            .values()
            .next_back()
            .map(|e| e.log_id)
            .or(inner.last_purged);
        Ok(LogState {
            last_purged_log_id: inner.last_purged,
            last_log_id: last,
        })
    }

    async fn get_log_reader(&mut self) -> Self::LogReader {
        self.clone()
    }

    async fn save_vote(&mut self, vote: &Vote<NodeId>) -> StorageResult<()> {
        self.persist(Record::Vote(*vote), true).await?;
        lock(&self.inner).vote = Some(*vote);
        Ok(())
    }

    async fn read_vote(&mut self) -> StorageResult<Option<Vote<NodeId>>> {
        Ok(lock(&self.inner).vote)
    }

    async fn save_committed(&mut self, committed: Option<LogId<NodeId>>) -> StorageResult<()> {
        // Not synced: it only saves replay work, and a stale value is safe.
        self.persist(Record::Committed(committed), false).await?;
        lock(&self.inner).committed = committed;
        Ok(())
    }

    async fn read_committed(&mut self) -> StorageResult<Option<LogId<NodeId>>> {
        Ok(lock(&self.inner).committed)
    }

    async fn append<I>(&mut self, entries: I, callback: LogFlushed<TypeConfig>) -> StorageResult<()>
    where
        I: IntoIterator<Item = Entry<TypeConfig>> + OptionalSend,
        I::IntoIter: OptionalSend,
    {
        let entries: Vec<_> = entries.into_iter().collect();
        self.stats.saved(&entries);
        self.persist(Record::Append(entries.clone()), true).await?;
        {
            let mut inner = lock(&self.inner);
            for entry in entries {
                inner.log.insert(entry.log_id.index, entry);
            }
        }
        // On disk if this node has a file; otherwise memory is the durable medium.
        callback.log_io_completed(Ok(()));
        Ok(())
    }

    /// Delete entries at `log_id.index` and after (they conflict with the leader's log).
    async fn truncate(&mut self, log_id: LogId<NodeId>) -> StorageResult<()> {
        self.persist(Record::Truncate(log_id.index), true).await?;
        lock(&self.inner).log.split_off(&log_id.index);
        Ok(())
    }

    /// Delete entries up to and including `log_id.index` (already in a snapshot).
    async fn purge(&mut self, log_id: LogId<NodeId>) -> StorageResult<()> {
        self.persist(Record::Purge(log_id), true).await?;
        let mut inner = lock(&self.inner);
        inner.last_purged = Some(log_id);
        inner.log = inner.log.split_off(&(log_id.index + 1));
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// State machine
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct StoredSnapshot {
    meta: SnapshotMeta<NodeId, BasicNode>,
    data: Vec<u8>,
}

/// Applies committed entries to [`CloudState`].
#[derive(Debug)]
pub struct StateMachineStore {
    state: StateReader,
    last_applied: Option<LogId<NodeId>>,
    last_membership: StoredMembership<NodeId, BasicNode>,
    current_snapshot: Arc<Mutex<Option<StoredSnapshot>>>,
    snapshot_seq: Arc<AtomicU64>,
    log: EventLog,
}

impl StateMachineStore {
    /// An empty state machine and a reader for the rest of the node.
    pub fn new(log: EventLog) -> (Self, StateReader) {
        let state = StateReader::default();
        let store = Self {
            state: state.clone(),
            last_applied: None,
            last_membership: StoredMembership::default(),
            current_snapshot: Arc::default(),
            snapshot_seq: Arc::default(),
            log,
        };
        (store, state)
    }
}

impl RaftStateMachine<TypeConfig> for StateMachineStore {
    type SnapshotBuilder = SnapshotBuilder;

    async fn applied_state(
        &mut self,
    ) -> StorageResult<(Option<LogId<NodeId>>, StoredMembership<NodeId, BasicNode>)> {
        Ok((self.last_applied, self.last_membership.clone()))
    }

    async fn apply<I>(&mut self, entries: I) -> StorageResult<Vec<Vec<Outcome>>>
    where
        I: IntoIterator<Item = Entry<TypeConfig>> + OptionalSend,
        I::IntoIter: OptionalSend,
    {
        let mut outcomes = Vec::new();
        let mut state = self.state.write();
        for entry in entries {
            self.last_applied = Some(entry.log_id);
            let index = entry.log_id.index;
            let outcome = match entry.payload {
                EntryPayload::Blank => Vec::new(),
                EntryPayload::Normal(group) => state.apply_group(index, &group),
                EntryPayload::Membership(m) => {
                    self.last_membership = StoredMembership::new(Some(entry.log_id), m);
                    Vec::new()
                }
            };
            outcomes.push(outcome);
            // Any entry counts, not only commands: a new leader's blank entry can fall on this
            // index on every node alike, and then the digest is simply the one before it.
            if index > 0 && index % CHECKPOINT_EVERY == 0 {
                self.log.log(Event::StateCheckpoint {
                    log_index: index,
                    digest: state.digest(),
                });
            }
        }
        Ok(outcomes)
    }

    async fn get_snapshot_builder(&mut self) -> Self::SnapshotBuilder {
        // Copy the state now: openraft may build the snapshot while later entries are applied.
        SnapshotBuilder {
            last_applied: self.last_applied,
            membership: self.last_membership.clone(),
            state: self.state.with(Clone::clone),
            current: self.current_snapshot.clone(),
            seq: self.snapshot_seq.clone(),
        }
    }

    async fn begin_receiving_snapshot(&mut self) -> StorageResult<Box<Cursor<Vec<u8>>>> {
        Ok(Box::new(Cursor::new(Vec::new())))
    }

    async fn install_snapshot(
        &mut self,
        meta: &SnapshotMeta<NodeId, BasicNode>,
        snapshot: Box<Cursor<Vec<u8>>>,
    ) -> StorageResult<()> {
        let data = snapshot.into_inner();
        let state: CloudState = bincode::deserialize(&data)
            .map_err(|e| StorageIOError::read_snapshot(Some(meta.signature()), &e))?;
        *self.state.write() = state;
        self.last_applied = meta.last_log_id;
        self.last_membership = meta.last_membership.clone();
        *lock(&self.current_snapshot) = Some(StoredSnapshot {
            meta: meta.clone(),
            data,
        });
        Ok(())
    }

    async fn get_current_snapshot(&mut self) -> StorageResult<Option<Snapshot<TypeConfig>>> {
        Ok(lock(&self.current_snapshot).as_ref().map(|s| Snapshot {
            meta: s.meta.clone(),
            snapshot: Box::new(Cursor::new(s.data.clone())),
        }))
    }
}

/// A point-in-time copy of the state machine that openraft turns into a snapshot.
#[derive(Debug)]
pub struct SnapshotBuilder {
    last_applied: Option<LogId<NodeId>>,
    membership: StoredMembership<NodeId, BasicNode>,
    state: CloudState,
    current: Arc<Mutex<Option<StoredSnapshot>>>,
    seq: Arc<AtomicU64>,
}

impl RaftSnapshotBuilder<TypeConfig> for SnapshotBuilder {
    async fn build_snapshot(&mut self) -> StorageResult<Snapshot<TypeConfig>> {
        let data = bincode::serialize(&self.state)
            .map_err(|e| StorageIOError::write_snapshot(None, &e))?;
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let snapshot_id = match self.last_applied {
            Some(l) => format!("{}-{}-{seq}", l.leader_id, l.index),
            None => format!("empty-{seq}"),
        };
        let meta = SnapshotMeta {
            last_log_id: self.last_applied,
            last_membership: self.membership.clone(),
            snapshot_id,
        };
        *lock(&self.current) = Some(StoredSnapshot {
            meta: meta.clone(),
            data: data.clone(),
        });
        Ok(Snapshot {
            meta,
            snapshot: Box::new(Cursor::new(data)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{Capability, Command, Point, RequestId, Responder};
    use openraft::CommittedLeaderId;
    use openraft::testing::{StoreBuilder, Suite};

    struct Builder;

    impl StoreBuilder<TypeConfig, LogStore, StateMachineStore> for Builder {
        async fn build(&self) -> StorageResult<((), LogStore, StateMachineStore)> {
            let (sm, _) = StateMachineStore::new(EventLog::disabled("test"));
            Ok(((), LogStore::default(), sm))
        }
    }

    /// openraft's own conformance tests for storage implementations.
    #[test]
    fn passes_openraft_storage_suite() {
        Suite::test_all(Builder).unwrap();
    }

    fn entry(index: u64, cmd: Command) -> Entry<TypeConfig> {
        Entry {
            log_id: LogId::new(CommittedLeaderId::new(1, 1), index),
            payload: EntryPayload::Normal(CommandGroup(vec![cmd])),
        }
    }

    fn group_entry(index: u64, commands: usize) -> Entry<TypeConfig> {
        let command = Command::ReleaseResponder {
            responder: 1,
            request: RequestId { client: 1, seq: 1 },
        };
        Entry {
            log_id: LogId::new(CommittedLeaderId::new(1, 1), index),
            payload: EntryPayload::Normal(CommandGroup(vec![command; commands])),
        }
    }

    fn store_with(entries: impl IntoIterator<Item = Entry<TypeConfig>>) -> LogStore {
        let store = LogStore::default();
        lock(&store.inner)
            .log
            .extend(entries.into_iter().map(|e| (e.log_id.index, e)));
        store
    }

    #[tokio::test]
    async fn one_message_stops_at_the_command_cap() {
        let mut store = store_with((1..=40).map(|i| group_entry(i, 256)));
        let sent = store.limited_get_log_entries(1, 41).await.unwrap();
        assert_eq!(sent.len(), MAX_COMMANDS_PER_MESSAGE / 256);
        assert_eq!(sent[0].log_id.index, 1);
        let next = store
            .limited_get_log_entries(sent.len() as u64 + 1, 41)
            .await
            .unwrap();
        assert_eq!(next[0].log_id.index, sent.len() as u64 + 1);
    }

    #[tokio::test]
    async fn small_entries_still_go_in_the_full_range() {
        let mut store = store_with((1..=300).map(|i| group_entry(i, 1)));
        let sent = store.limited_get_log_entries(1, 301).await.unwrap();
        assert_eq!(sent.len(), 300);
    }

    #[tokio::test]
    async fn one_entry_over_the_cap_still_goes() {
        // The contract: a range with something in it must not come back empty.
        let big = MAX_COMMANDS_PER_MESSAGE + 1;
        let mut store = store_with([group_entry(1, big), group_entry(2, 1)]);
        let sent = store.limited_get_log_entries(1, 3).await.unwrap();
        assert_eq!(sent.len(), 1);
    }

    #[tokio::test]
    async fn apply_answers_a_group_with_one_outcome_per_command_and_others_with_none() {
        let (mut sm, _) = StateMachineStore::new(EventLog::disabled("test"));
        let blank = Entry {
            log_id: LogId::new(CommittedLeaderId::new(1, 1), 1),
            payload: EntryPayload::Blank,
        };
        let outcomes = sm.apply([blank, group_entry(2, 3)]).await.unwrap();
        assert_eq!(outcomes[0], Vec::new());
        assert_eq!(outcomes[1], vec![Outcome::Applied; 3]);
    }

    /// The digests `store` logged, by log index.
    fn checkpoints(log: &EventLog, dir: &std::path::Path) -> BTreeMap<u64, u64> {
        log.close();
        let text = std::fs::read_to_string(dir.join("node-1.jsonl")).unwrap();
        text.lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .filter(|e| e["ev"] == "state_checkpoint")
            .map(|e| {
                (
                    e["log_index"].as_u64().unwrap(),
                    e["digest"].as_u64().unwrap(),
                )
            })
            .collect()
    }

    /// `last` entries from index 1: all commands, but a blank one at index 100.
    async fn apply_log(log: EventLog, last: u64, batch: usize) -> u64 {
        let (mut sm, reader) = StateMachineStore::new(log);
        let entries: Vec<_> = (1..=last)
            .map(|index| {
                let release = Command::ReleaseResponder {
                    responder: 1,
                    request: crate::proto::RequestId {
                        client: 1,
                        seq: index,
                    },
                };
                let mut entry = entry(index, release);
                if index == CHECKPOINT_EVERY {
                    entry.payload = EntryPayload::Blank;
                }
                entry
            })
            .collect();
        for chunk in entries.chunks(batch) {
            sm.apply(chunk.to_vec()).await.unwrap();
        }
        reader.with(|s| s.digest())
    }

    #[tokio::test]
    async fn a_digest_is_logged_at_every_checkpoint_index_however_entries_are_batched() {
        let dirs = ["a", "b"].map(|name| {
            let dir = std::env::temp_dir().join(format!(
                "rescuemesh-checkpoints-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            dir
        });
        let last = 2 * CHECKPOINT_EVERY + 50;

        // Two nodes get the same log in different batches, as replication and catch-up give them.
        let one_log = EventLog::to_dir(&dirs[0], "node-1").unwrap();
        let other_log = EventLog::to_dir(&dirs[1], "node-1").unwrap();
        let one = apply_log(one_log.clone(), last, 1).await;
        let other = apply_log(other_log.clone(), last, 77).await;
        assert_eq!(one, other);

        let logged = checkpoints(&one_log, &dirs[0]);
        assert_eq!(
            logged.keys().copied().collect::<Vec<_>>(),
            vec![CHECKPOINT_EVERY, 2 * CHECKPOINT_EVERY],
            "a blank entry on the index still gets a line"
        );
        assert_ne!(logged[&CHECKPOINT_EVERY], logged[&(2 * CHECKPOINT_EVERY)]);
        assert_eq!(logged, checkpoints(&other_log, &dirs[1]));
        for dir in dirs {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[tokio::test]
    async fn snapshot_restores_identical_state() {
        let (mut sm, reader) = StateMachineStore::new(EventLog::disabled("test"));
        let register = Command::RegisterResponders {
            responders: vec![Responder {
                id: 1,
                zone: 0,
                location: Point { x: 3, y: 4 },
                capability: Capability::Fire,
            }],
        };
        sm.apply([entry(1, register)]).await.unwrap();
        let snap = sm
            .get_snapshot_builder()
            .await
            .build_snapshot()
            .await
            .unwrap();
        assert_eq!(snap.meta.last_log_id.map(|l| l.index), Some(1));

        let (mut other, other_reader) = StateMachineStore::new(EventLog::disabled("test"));
        other
            .install_snapshot(&snap.meta, snap.snapshot)
            .await
            .unwrap();
        assert_eq!(reader.with(Clone::clone), other_reader.with(Clone::clone));
        assert_eq!(
            other.applied_state().await.unwrap().0,
            sm.applied_state().await.unwrap().0
        );
        assert!(other.get_current_snapshot().await.unwrap().is_some());
    }
}
