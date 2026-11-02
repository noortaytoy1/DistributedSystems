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

use crate::proto::{NodeId, Outcome};
use crate::raft::TypeConfig;
use crate::raft::wal::{Record, Replayed, Wal};
use crate::state::CloudState;

type StorageResult<T> = Result<T, StorageError<NodeId>>;

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

/// The Raft log and vote. Clones share the same log, which is how openraft's log reader works.
#[derive(Debug, Clone, Default)]
pub struct LogStore {
    inner: Arc<Mutex<LogInner>>,
    /// The write-ahead file, if this node has `storage.dir` set.
    wal: Option<Arc<Mutex<Wal>>>,
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
        };
        Ok((store, restarted))
    }

    /// Write a record to the file off the async runtime. A no-op without a file. Callers await
    /// this before they change memory, so nothing is reported that a power cut could lose.
    async fn persist(&self, record: Record, sync: bool) -> StorageResult<()> {
        let Some(wal) = self.wal.clone() else {
            return Ok(());
        };
        let written = tokio::task::spawn_blocking(move || lock(&wal).write(&record, sync))
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
}

impl StateMachineStore {
    /// An empty state machine and a reader for the rest of the node.
    pub fn new() -> (Self, StateReader) {
        let state = StateReader::default();
        let store = Self {
            state: state.clone(),
            last_applied: None,
            last_membership: StoredMembership::default(),
            current_snapshot: Arc::default(),
            snapshot_seq: Arc::default(),
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

    async fn apply<I>(&mut self, entries: I) -> StorageResult<Vec<Outcome>>
    where
        I: IntoIterator<Item = Entry<TypeConfig>> + OptionalSend,
        I::IntoIter: OptionalSend,
    {
        let mut outcomes = Vec::new();
        let mut state = self.state.write();
        for entry in entries {
            self.last_applied = Some(entry.log_id);
            let outcome = match entry.payload {
                EntryPayload::Blank => Outcome::Applied,
                EntryPayload::Normal(cmd) => state.apply(entry.log_id.index, &cmd),
                EntryPayload::Membership(m) => {
                    self.last_membership = StoredMembership::new(Some(entry.log_id), m);
                    Outcome::Applied
                }
            };
            outcomes.push(outcome);
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
    use crate::proto::{Capability, Command, Point, Responder};
    use openraft::CommittedLeaderId;
    use openraft::testing::{StoreBuilder, Suite};

    struct Builder;

    impl StoreBuilder<TypeConfig, LogStore, StateMachineStore> for Builder {
        async fn build(&self) -> StorageResult<((), LogStore, StateMachineStore)> {
            let (sm, _) = StateMachineStore::new();
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
            payload: EntryPayload::Normal(cmd),
        }
    }

    #[tokio::test]
    async fn snapshot_restores_identical_state() {
        let (mut sm, reader) = StateMachineStore::new();
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

        let (mut other, other_reader) = StateMachineStore::new();
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
