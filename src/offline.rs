//! Offline operation on a client (Phase 2, Part K, D400–D406): the reports a user makes while the
//! client is cut off, kept until it is back.
//!
//! - **The switch (D402).** Going offline closes a [`FaultGate`](crate::node::transport::FaultGate)
//!   inside every one of the client's sockets, as the node's fault gate does: nothing leaves and
//!   nothing arrives, so even an answer already on its way is lost, as with a real disconnection.
//!   `MeshClient::go_offline` and `go_online` drive it.
//! - **The outbox (D401).** A report made while offline gets its request ID at once and goes into
//!   the outbox instead of to the cloud. Each entry is one JSON line in
//!   `<metrics.dir>/outbox-client-<id>.jsonl`, so a program that restarts while offline still has
//!   it; with no metrics directory it is kept in memory only. At [`OUTBOX_CAP`] entries a new report
//!   is refused with an error, so a long outage cannot fill the disk (D405).
//! - **Back online (D403, D404).** Every entry is sent, oldest first and [`FLUSH_IN_FLIGHT`] at a
//!   time, with its original ID, so a report the cloud already has is answered from its record and
//!   never planned twice. A report too old by then is not dropped here: the cloud refuses it and
//!   commits the refusal (D143), so the rule is one rule, on record on every node. An entry that
//!   gets no answer stays for the next reconnect.
//!
//! File reads and writes go through `tokio::fs`, so they never block the runtime.
//!
//! Owner: K (offline operation).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use crate::client::Submitted;
use crate::proto::{ClientId, Incident, RequestId};

/// Most reports an outbox holds; past it a new report is refused (D405).
pub const OUTBOX_CAP: usize = 10_000;

/// Queued reports sent at once when the client is back (D404).
pub const FLUSH_IN_FLIGHT: usize = 16;

/// One report made while offline, as kept in the outbox and its file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Queued {
    /// Given when the report was made, and kept for every send, so a resend is harmless.
    pub id: RequestId,
    pub incident: Incident,
    /// When it was queued: unix milliseconds on this client's clock.
    pub queued_at_ms: u64,
}

/// What became of a report handed to `MeshClient::submit_or_queue` (D400).
#[derive(Debug, Clone)]
pub enum Delivery {
    /// The client was online and the cloud answered.
    Answered(Submitted),
    /// The client was offline, so the report waits in the outbox under this ID.
    Queued(RequestId),
}

/// What one flush of the outbox did. `took` is the offline reconciliation delay: from the start of
/// the flush to the last answer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reconciled {
    pub sent: usize,
    /// Answered with a plan, committed or provisional.
    pub answered: usize,
    /// Refused by the cloud as too old (D143). Also on record, so also delivered.
    pub expired: usize,
    /// No answer; still in the outbox.
    pub failed: usize,
    pub took: Duration,
}

/// Why a report could not be queued.
#[derive(Debug, thiserror::Error)]
pub enum OutboxError {
    #[error("the outbox is full ({cap} reports)")]
    Full { cap: usize },
    #[error("cannot write the outbox file {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// The reports a client made while offline, oldest first. Safe to share: every method takes `&self`.
pub struct Outbox {
    path: Option<PathBuf>,
    cap: usize,
    state: tokio::sync::Mutex<State>,
}

#[derive(Default)]
struct State {
    /// False until the file has been read, which happens on first use, inside the runtime.
    loaded: bool,
    entries: Vec<Queued>,
}

impl Outbox {
    /// An outbox kept in `path`, or in memory only for `None`.
    pub fn new(path: Option<PathBuf>) -> Self {
        Self::with_cap(path, OUTBOX_CAP)
    }

    pub fn with_cap(path: Option<PathBuf>, cap: usize) -> Self {
        Self {
            path,
            cap,
            state: tokio::sync::Mutex::new(State::default()),
        }
    }

    /// Where client `client` keeps its outbox: next to its event logs, or nowhere.
    pub fn path_for(dir: Option<&Path>, client: ClientId) -> Option<PathBuf> {
        dir.map(|dir| dir.join(format!("outbox-client-{client}.jsonl")))
    }

    /// Add a report at the end, and to the file before returning.
    pub async fn push(&self, queued: Queued) -> Result<(), OutboxError> {
        let mut state = self.loaded().await;
        if state.entries.len() >= self.cap {
            return Err(OutboxError::Full { cap: self.cap });
        }
        if let Some(path) = &self.path {
            append(path, &queued)
                .await
                .map_err(|source| OutboxError::Write {
                    path: path.clone(),
                    source,
                })?;
        }
        state.entries.push(queued);
        Ok(())
    }

    /// Every waiting report, oldest first.
    pub async fn entries(&self) -> Vec<Queued> {
        self.loaded().await.entries.clone()
    }

    pub async fn len(&self) -> usize {
        self.loaded().await.entries.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }

    /// Take out the reports that reached the cloud, and rewrite the file with the rest.
    pub async fn remove(&self, delivered: &BTreeSet<RequestId>) -> Result<(), OutboxError> {
        let mut state = self.loaded().await;
        state.entries.retain(|q| !delivered.contains(&q.id));
        match &self.path {
            Some(path) => {
                rewrite(path, &state.entries)
                    .await
                    .map_err(|source| OutboxError::Write {
                        path: path.clone(),
                        source,
                    })
            }
            None => Ok(()),
        }
    }

    /// The state, read from the file the first time. A line that does not parse is skipped with a
    /// warning: one torn write must not lose the rest.
    async fn loaded(&self) -> tokio::sync::MutexGuard<'_, State> {
        let mut state = self.state.lock().await;
        if !state.loaded {
            state.loaded = true;
            if let Some(path) = &self.path {
                state.entries = read(path).await;
            }
        }
        state
    }
}

async fn read(path: &Path) -> Vec<Queued> {
    let text = match tokio::fs::read_to_string(path).await {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            tracing::warn!(path = %path.display(), "cannot read the outbox, starting empty: {e}");
            return Vec::new();
        }
    };
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| match serde_json::from_str(line) {
            Ok(queued) => Some(queued),
            Err(e) => {
                tracing::warn!(path = %path.display(), "skipping a damaged outbox line: {e}");
                None
            }
        })
        .collect()
}

fn line(queued: &Queued) -> String {
    let mut line = serde_json::to_string(queued).expect("a queued report always serializes");
    line.push('\n');
    line
}

async fn append(path: &Path, queued: &Queued) -> std::io::Result<()> {
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    file.write_all(line(queued).as_bytes()).await?;
    file.flush().await
}

/// Written to a temporary file and renamed over the old one, so a crash leaves one or the other.
async fn rewrite(path: &Path, entries: &[Queued]) -> std::io::Result<()> {
    let text: String = entries.iter().map(line).collect();
    let tmp = path.with_extension("jsonl.tmp");
    tokio::fs::write(&tmp, text).await?;
    tokio::fs::rename(&tmp, path).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{Capability, Category, Point, UserId};

    fn queued(seq: u64) -> Queued {
        Queued {
            id: RequestId { client: 7, seq },
            incident: Incident {
                owner: UserId { client: 7, user: 0 },
                category: Category::MedicalAssistance,
                zone: 1,
                location: Point { x: 3, y: 4 },
                severity: 3,
                needs: Capability::Medical,
                reported_at_ms: 1_000 + seq,
                ttl_ms: 60_000,
            },
            queued_at_ms: 2_000 + seq,
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rescuemesh-outbox-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join(name);
        let _ = std::fs::remove_file(&path);
        path
    }

    #[tokio::test]
    async fn a_full_outbox_refuses_and_keeps_what_it_has() {
        let outbox = Outbox::with_cap(None, 2);
        outbox.push(queued(1)).await.expect("room");
        outbox.push(queued(2)).await.expect("room");
        assert!(matches!(
            outbox.push(queued(3)).await,
            Err(OutboxError::Full { cap: 2 })
        ));
        assert_eq!(outbox.entries().await, vec![queued(1), queued(2)]);
    }

    #[tokio::test]
    async fn the_file_survives_a_restart_and_keeps_the_order() {
        let path = temp_path("restart.jsonl");
        let first = Outbox::new(Some(path.clone()));
        for seq in 1..=3 {
            first.push(queued(seq)).await.expect("written");
        }
        drop(first);

        let second = Outbox::new(Some(path.clone()));
        assert_eq!(
            second.entries().await,
            vec![queued(1), queued(2), queued(3)]
        );
    }

    #[tokio::test]
    async fn removing_delivered_reports_rewrites_the_file() {
        let path = temp_path("remove.jsonl");
        let outbox = Outbox::new(Some(path.clone()));
        for seq in 1..=3 {
            outbox.push(queued(seq)).await.expect("written");
        }
        let delivered = BTreeSet::from([queued(1).id, queued(3).id]);
        outbox.remove(&delivered).await.expect("rewritten");
        assert_eq!(outbox.entries().await, vec![queued(2)]);

        let reread = Outbox::new(Some(path));
        assert_eq!(reread.entries().await, vec![queued(2)]);
    }

    #[tokio::test]
    async fn a_damaged_line_is_skipped_and_the_rest_kept() {
        let path = temp_path("damaged.jsonl");
        let good = line(&queued(1)) + "{not json\n" + &line(&queued(2));
        std::fs::write(&path, good).expect("written");
        let outbox = Outbox::new(Some(path));
        assert_eq!(outbox.entries().await, vec![queued(1), queued(2)]);
    }

    #[test]
    fn the_file_sits_next_to_the_event_logs() {
        assert_eq!(
            Outbox::path_for(Some(Path::new("runs/a")), 4),
            Some(PathBuf::from("runs/a/outbox-client-4.jsonl"))
        );
        assert_eq!(Outbox::path_for(None, 4), None);
    }
}
