//! Write-ahead file for the Raft log and vote, so a node survives losing power (D118).
//!
//! One append-only file per node. Each record is a 4-byte big-endian length, an 8-byte FNV-1a
//! checksum, and a body encoded with `proto::encode`. A record that is cut short or fails its
//! checksum can only be the tail of a write interrupted by a crash, and was never acknowledged, so
//! opening the file stops there and truncates it.
//!
//! Every function here blocks: callers run them through `spawn_blocking`.
//!
//! Owner: A (Raft and replication).

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

use openraft::{Entry, LogId, Vote};
use serde::{Deserialize, Serialize};

use crate::proto::{self, NodeId};
use crate::raft::TypeConfig;

const HEADER_BYTES: usize = 12;

/// One change to the stored log or vote.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Record {
    Vote(Vote<NodeId>),
    Committed(Option<LogId<NodeId>>),
    Append(Vec<Entry<TypeConfig>>),
    /// Delete entries at this index and after.
    Truncate(u64),
    /// Delete entries up to and including this one.
    Purge(LogId<NodeId>),
}

/// What a file held when it was opened.
#[derive(Debug, Default)]
pub struct Replayed {
    pub vote: Option<Vote<NodeId>>,
    pub committed: Option<LogId<NodeId>>,
    pub last_purged: Option<LogId<NodeId>>,
    pub log: BTreeMap<u64, Entry<TypeConfig>>,
}

impl Replayed {
    /// True if this node has been part of a cluster before.
    pub fn is_restart(&self) -> bool {
        self.vote.is_some() || !self.log.is_empty() || self.last_purged.is_some()
    }

    fn apply(&mut self, record: Record) {
        match record {
            Record::Vote(vote) => self.vote = Some(vote),
            Record::Committed(committed) => self.committed = committed,
            Record::Append(entries) => {
                for entry in entries {
                    self.log.insert(entry.log_id.index, entry);
                }
            }
            Record::Truncate(index) => {
                self.log.split_off(&index);
            }
            Record::Purge(log_id) => {
                self.last_purged = Some(log_id);
                self.log = self.log.split_off(&(log_id.index + 1));
            }
        }
    }
}

/// The open file. Appends are written whole.
#[derive(Debug)]
pub struct Wal {
    file: File,
}

impl Wal {
    /// Open or create `path`, replay it, and cut off an interrupted last record.
    pub fn open(path: &Path) -> std::io::Result<(Self, Replayed)> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        let mut replayed = Replayed::default();
        let mut good = 0;
        while let Some((record, next)) = read_record(&bytes, good) {
            replayed.apply(record);
            good = next;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        if good < bytes.len() {
            tracing::warn!(
                file = %path.display(),
                cut = bytes.len() - good,
                "dropping an interrupted write at the end of the Raft log file"
            );
            file.set_len(good as u64)?;
        }
        // A new file's name must survive a power cut too.
        if let Some(dir) = path.parent() {
            File::open(dir)?.sync_all()?;
        }
        Ok((Self { file }, replayed))
    }

    /// Write a record. With `sync`, it is on disk when this returns.
    pub fn write(&mut self, record: &Record, sync: bool) -> std::io::Result<()> {
        let body = proto::encode(record).map_err(std::io::Error::other)?;
        let mut bytes = Vec::with_capacity(HEADER_BYTES + body.len());
        bytes.extend_from_slice(&(body.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&checksum(&body).to_be_bytes());
        bytes.extend_from_slice(&body);
        self.file.write_all(&bytes)?;
        if sync {
            self.file.sync_data()?;
        }
        Ok(())
    }
}

/// The record at `at` and where the next one starts; `None` at the end or at a damaged record.
fn read_record(bytes: &[u8], at: usize) -> Option<(Record, usize)> {
    let header = bytes.get(at..at + HEADER_BYTES)?;
    let len = u32::from_be_bytes(header[..4].try_into().ok()?) as usize;
    let sum = u64::from_be_bytes(header[4..].try_into().ok()?);
    let end = at + HEADER_BYTES + len;
    let body = bytes.get(at + HEADER_BYTES..end)?;
    if checksum(body) != sum {
        return None;
    }
    Some((proto::decode(body).ok()?, end))
}

fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use openraft::{CommittedLeaderId, EntryPayload};

    fn entry(index: u64) -> Entry<TypeConfig> {
        Entry {
            log_id: LogId::new(CommittedLeaderId::new(1, 1), index),
            payload: EntryPayload::Blank,
        }
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rescuemesh-wal-{}", std::process::id()));
        let path = dir.join(name);
        let _ = fs::remove_file(&path);
        path
    }

    #[test]
    fn a_fresh_file_is_not_a_restart() {
        let (_, replayed) = Wal::open(&temp("fresh")).unwrap();
        assert!(!replayed.is_restart());
    }

    #[test]
    fn records_replay_in_order() {
        let path = temp("order");
        {
            let (mut wal, _) = Wal::open(&path).unwrap();
            wal.write(&Record::Vote(Vote::new(3, 2)), true).unwrap();
            wal.write(&Record::Append(vec![entry(1), entry(2), entry(3)]), true)
                .unwrap();
            wal.write(&Record::Truncate(3), true).unwrap();
            wal.write(&Record::Append(vec![entry(3)]), true).unwrap();
            wal.write(&Record::Purge(entry(1).log_id), true).unwrap();
            wal.write(&Record::Committed(Some(entry(2).log_id)), false)
                .unwrap();
        }
        let (_, replayed) = Wal::open(&path).unwrap();
        assert!(replayed.is_restart());
        assert_eq!(replayed.vote, Some(Vote::new(3, 2)));
        assert_eq!(replayed.log.keys().copied().collect::<Vec<_>>(), vec![2, 3]);
        assert_eq!(replayed.last_purged, Some(entry(1).log_id));
        assert_eq!(replayed.committed, Some(entry(2).log_id));
    }

    #[test]
    fn an_interrupted_last_record_is_dropped_and_writing_continues() {
        let path = temp("torn");
        {
            let (mut wal, _) = Wal::open(&path).unwrap();
            wal.write(&Record::Append(vec![entry(1)]), true).unwrap();
            wal.write(&Record::Append(vec![entry(2)]), true).unwrap();
        }
        let full = fs::metadata(&path).unwrap().len();
        OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(full - 3)
            .unwrap();
        {
            let (mut wal, replayed) = Wal::open(&path).unwrap();
            assert_eq!(replayed.log.keys().copied().collect::<Vec<_>>(), vec![1]);
            wal.write(&Record::Append(vec![entry(9)]), true).unwrap();
        }
        let (_, replayed) = Wal::open(&path).unwrap();
        assert_eq!(replayed.log.keys().copied().collect::<Vec<_>>(), vec![1, 9]);
    }

    #[test]
    fn a_damaged_record_ends_the_replay() {
        let path = temp("damaged");
        {
            let (mut wal, _) = Wal::open(&path).unwrap();
            wal.write(&Record::Append(vec![entry(1)]), true).unwrap();
        }
        let mut bytes = fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        fs::write(&path, bytes).unwrap();
        let (_, replayed) = Wal::open(&path).unwrap();
        assert!(replayed.log.is_empty());
    }
}
