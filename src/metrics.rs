//! JSONL event log and message counters.
//!
//! Every node and client writes one JSON object per line:
//! `{"ts_us": <unix µs>, "src": "node-1", "ev": "<event>", ...fields}`.
//! Writing happens on a background thread so logging never blocks the async runtime.
//!
//! Owner: everyone (built together at M0). Each owner adds the events they measure to [`Event`];
//! keep field names stable once analysis scripts depend on them.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::proto::{NodeId, PeerBody, RaftRpc, RequestId, ResponderId};

/// Wall-clock time in unix microseconds. Use it for event timestamps only; measure durations
/// with `std::time::Instant`.
pub fn now_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

/// Every event any process logs. Field names are the analysis contract.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "ev", rename_all = "snake_case")]
pub enum Event {
    // --- lifecycle
    Started {
        peer_addr: String,
        client_addr: String,
    },
    Stopped,

    // --- Raft and replication (A)
    /// Term, leader or role changed on this node.
    RaftState {
        term: u64,
        leader: Option<NodeId>,
        role: String,
        last_applied: Option<u64>,
    },
    /// A proposal finished committing (E5, commit latency).
    Commit {
        request: Option<RequestId>,
        kind: &'static str,
        outcome: &'static str,
        latency_us: u64,
        forwarded: bool,
        log_index: Option<u64>,
    },
    ProposeFailed {
        request: Option<RequestId>,
        kind: &'static str,
        error: String,
        latency_us: u64,
    },
    /// Periodic state fingerprint; equal digests at equal indexes mean identical state.
    StateDigest {
        last_applied: u64,
        digest: u64,
    },
    /// The digest right after applying a log index that is a multiple of
    /// `store::CHECKPOINT_EVERY`. Every node applies every index, so these can be compared between
    /// nodes under load, which the once-a-second `state_digest` cannot (D138).
    StateCheckpoint {
        log_index: u64,
        digest: u64,
    },
    /// Cumulative message counts by type (E5).
    Messages {
        sent: BTreeMap<&'static str, u64>,
        received: BTreeMap<&'static str, u64>,
    },

    /// Once a second: this process's memory and CPU, the machine's network counters and the worker
    /// queue (D128). `cpu_ticks` and the byte counts are cumulative.
    ResourceUsage {
        rss_kb: Option<u64>,
        cpu_ticks: Option<u64>,
        net_rx_bytes: Option<u64>,
        net_tx_bytes: Option<u64>,
        queued: usize,
        running: usize,
    },

    // --- request handling (all roles)
    RequestReceived {
        request: RequestId,
        attempt: u32,
    },
    Replied {
        request: RequestId,
        executor: NodeId,
        latency_us: u64,
    },

    // --- bidding (B)
    /// This node's bid and what made up its score (E6).
    BidSent {
        request: RequestId,
        attempt: u32,
        score: u64,
        queued: usize,
        running: usize,
        just_won: usize,
        service_us: u64,
        cpu_permille: u32,
    },
    /// This node's view of a closed round. Two nodes with `i_won` true for one request is a split
    /// decision (P-2).
    BidRoundClosed {
        request: RequestId,
        attempt: u32,
        winner: Option<NodeId>,
        i_won: bool,
        bids_seen: usize,
        my_score: Option<u64>,
        window_us: u64,
    },

    // --- faults (C)
    Silenced {
        ms: u64,
    },
    Restored,
    /// The leader's schedule ordered `victim` silent for `ms` (E3, FT-8).
    FaultOrdered {
        victim: NodeId,
        ms: u64,
        term: u64,
    },
    /// The order was not carried out: the victim refused it, had a newer term, or was unreachable.
    FaultRefused {
        victim: NodeId,
        reason: String,
    },
    /// Nothing heard from `peer` for `silent_ms` (peer heartbeats, D79). E3 detection time is
    /// the victim's `silenced` to this.
    PeerSuspected {
        peer: NodeId,
        silent_ms: u64,
    },
    /// `peer` was heard from again after `suspected_ms` under suspicion.
    PeerTrusted {
        peer: NodeId,
        suspected_ms: u64,
    },
    /// A connect to `peer` failed; logged once per outage (D92).
    PeerUnreachable {
        peer: NodeId,
        error: String,
    },
    /// A connect to `peer` succeeded after an outage that lasted `down_ms` on this node's clock.
    PeerReconnected {
        peer: NodeId,
        down_ms: u64,
    },
    /// This node lost the round and plans the request instead of `winner` (D110). `rank` is its
    /// place in the round's ranking (1 = runner-up); `reason` is `suspected` or `backstop`.
    /// `waited_us` runs from the round closing to here: E3 takeover time.
    TakeoverStarted {
        request: RequestId,
        attempt: u32,
        winner: NodeId,
        rank: usize,
        reason: String,
        waited_us: u64,
    },

    // --- degraded mode (C)
    /// Every peer has been suspected for `degraded_after_ms`; this node now answers without a
    /// commit (D120, D121). `held_responders` is what the local table took over (D124).
    DegradedEntered {
        held_responders: usize,
    },
    /// `reason` is `peer_trusted`, `silenced` or `not_serving`. `provisional` answers are waiting
    /// to be replayed through Raft.
    DegradedLeft {
        reason: String,
        degraded_ms: u64,
        provisional: usize,
    },
    /// A lone node answered without a commit.
    ProvisionalReplied {
        request: RequestId,
        responder: Option<ResponderId>,
        latency_us: u64,
    },
    /// A provisional answer did not stand (D122). `reason` is `lost_first_commit`, with the winner's
    /// plan, or `responder_taken`, and then the result is recorded with no responder.
    ProvisionalConflict {
        request: RequestId,
        reason: String,
        responder: Option<ResponderId>,
        winner_responder: Option<ResponderId>,
        winner_executor: Option<NodeId>,
    },

    // --- planner and workers (E)
    /// The worker pool was full and refused this planning job (D48, D73). `queued` and `running`
    /// are the pool at that moment.
    Overloaded {
        request: RequestId,
        attempt: u32,
        queued: usize,
        running: usize,
    },

    // --- responders (E)
    /// This node proposed freeing `responder` after its service time (D39, D111). `backup` is
    /// true if the executor should have done it first and has not.
    ReleaseProposed {
        responder: ResponderId,
        request: RequestId,
        backup: bool,
    },

    // --- rejoin (D)
    /// The process started with a Raft log from an earlier run (D118, FT-9).
    Restarted {
        term: u64,
        last_log_index: Option<u64>,
    },
    /// The fault gate reopened; the node is catching up and does not serve yet (E4).
    RejoinStarted {
        applied: Option<u64>,
    },
    /// The leader's next answer was below the catch-up target, so the target was an index that was
    /// not committed then, and may never be (D135).
    RejoinTargetLowered {
        from: u64,
        to: u64,
    },
    /// The node applied the leader's commit index and serves again. `recovery_us` runs from
    /// `rejoin_started` to here, on this node's clock (E4).
    Rejoined {
        leader_commit: Option<u64>,
        applied: Option<u64>,
        recovery_us: u64,
    },

    // --- client (F)
    ClientSent {
        request: RequestId,
        attempt: u32,
    },
    ClientDone {
        request: RequestId,
        from: NodeId,
        executor: NodeId,
        latency_us: u64,
        attempts: u32,
        /// True if the answer was a lone node's, not committed (D123).
        provisional: bool,
        /// True if the plan sent nobody, because no suitable responder was free (D111).
        no_responder: bool,
    },
    ClientGaveUp {
        request: RequestId,
        attempts: u32,
    },
    /// A client connection to `node` was opened (D112).
    ClientLinkUp {
        node: NodeId,
    },
    /// A client connection to `node` closed or stopped answering pings (D112).
    ClientLinkDown {
        node: NodeId,
        reason: String,
    },
}

#[derive(Serialize)]
struct Line<'a> {
    ts_us: u64,
    src: &'a str,
    #[serde(flatten)]
    event: &'a Event,
}

enum Msg {
    Line(String),
    Flush(mpsc::Sender<()>),
}

struct Sink {
    tx: Mutex<mpsc::Sender<Msg>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

/// Cheap to clone; all clones write to the same file.
#[derive(Clone)]
pub struct EventLog {
    src: Arc<str>,
    sink: Option<Arc<Sink>>,
    msgs: Arc<MessageCounters>,
}

impl std::fmt::Debug for EventLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventLog")
            .field("src", &self.src)
            .field("enabled", &self.sink.is_some())
            .finish()
    }
}

impl EventLog {
    /// A log that only keeps message counters (tests, or no `metrics.dir`).
    pub fn disabled(src: &str) -> Self {
        Self {
            src: src.into(),
            sink: None,
            msgs: Arc::default(),
        }
    }

    /// Append events to `<dir>/<src>.jsonl`, creating the directory if needed.
    pub fn to_dir(dir: &Path, src: &str) -> std::io::Result<Self> {
        fs::create_dir_all(dir)?;
        let file = File::options()
            .create(true)
            .append(true)
            .open(dir.join(format!("{src}.jsonl")))?;
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name(format!("eventlog-{src}"))
            .spawn(move || write_loop(rx, BufWriter::new(file)))?;
        Ok(Self {
            src: src.into(),
            sink: Some(Arc::new(Sink {
                tx: Mutex::new(tx),
                thread: Mutex::new(Some(thread)),
            })),
            msgs: Arc::default(),
        })
    }

    /// Use `dir` if set, otherwise a disabled log.
    pub fn open(dir: Option<&Path>, src: &str) -> std::io::Result<Self> {
        match dir {
            Some(dir) => Self::to_dir(dir, src),
            None => Ok(Self::disabled(src)),
        }
    }

    pub fn log(&self, event: Event) {
        let Some(sink) = &self.sink else { return };
        let line = Line {
            ts_us: now_us(),
            src: &self.src,
            event: &event,
        };
        match serde_json::to_string(&line) {
            Ok(text) => {
                let tx = sink.tx.lock().unwrap_or_else(|p| p.into_inner());
                let _ = tx.send(Msg::Line(text));
            }
            Err(e) => tracing::warn!("cannot serialize event: {e}"),
        }
    }

    pub fn msgs(&self) -> &Arc<MessageCounters> {
        &self.msgs
    }

    /// Write everything logged so far to disk. Blocks for up to one second; call it at shutdown
    /// from outside the async runtime or via `spawn_blocking`.
    pub fn flush(&self) {
        let Some(sink) = &self.sink else { return };
        let (ack_tx, ack_rx) = mpsc::channel();
        {
            let tx = sink.tx.lock().unwrap_or_else(|p| p.into_inner());
            if tx.send(Msg::Flush(ack_tx)).is_err() {
                return;
            }
        }
        let _ = ack_rx.recv_timeout(Duration::from_secs(1));
    }

    /// Flush and stop the writer thread. Later `log` calls are dropped.
    pub fn close(&self) {
        let Some(sink) = &self.sink else { return };
        self.flush();
        // Swapping in a dead sender ends the writer loop once the old sender is dropped.
        let (dead_tx, _) = mpsc::channel();
        drop(std::mem::replace(
            &mut *sink.tx.lock().unwrap_or_else(|p| p.into_inner()),
            dead_tx,
        ));
        if let Some(handle) = sink.thread.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = handle.join();
        }
    }
}

fn write_loop(rx: mpsc::Receiver<Msg>, mut out: BufWriter<File>) {
    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Msg::Line(line)) => {
                let _ = writeln!(out, "{line}");
            }
            Ok(Msg::Flush(ack)) => {
                let _ = out.flush();
                let _ = ack.send(());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = out.flush();
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = out.flush();
                return;
            }
        }
    }
}

/// Message types counted for the overhead experiment (E5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgKind {
    RaftAppend,
    /// Append-entries with no entries: Raft's own heartbeat.
    RaftHeartbeat,
    RaftVote,
    RaftSnapshot,
    Propose,
    /// A returning node asks the leader for its commit index (D76).
    ReadIndex,
    /// The leader's scheduled fault order (D78).
    FaultOrder,
    Bid,
    PeerHeartbeat,
    ClientRequest,
    ClientReply,
}

impl MsgKind {
    pub const ALL: [MsgKind; 11] = [
        MsgKind::RaftAppend,
        MsgKind::RaftHeartbeat,
        MsgKind::RaftVote,
        MsgKind::RaftSnapshot,
        MsgKind::Propose,
        MsgKind::ReadIndex,
        MsgKind::FaultOrder,
        MsgKind::Bid,
        MsgKind::PeerHeartbeat,
        MsgKind::ClientRequest,
        MsgKind::ClientReply,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            MsgKind::RaftAppend => "raft_append",
            MsgKind::RaftHeartbeat => "raft_heartbeat",
            MsgKind::RaftVote => "raft_vote",
            MsgKind::RaftSnapshot => "raft_snapshot",
            MsgKind::Propose => "propose",
            MsgKind::ReadIndex => "read_index",
            MsgKind::FaultOrder => "fault_order",
            MsgKind::Bid => "bid",
            MsgKind::PeerHeartbeat => "peer_heartbeat",
            MsgKind::ClientRequest => "client_request",
            MsgKind::ClientReply => "client_reply",
        }
    }

    pub fn of(body: &PeerBody) -> MsgKind {
        match body {
            PeerBody::Raft(RaftRpc::AppendEntries(r)) if r.entries.is_empty() => {
                MsgKind::RaftHeartbeat
            }
            PeerBody::Raft(RaftRpc::AppendEntries(_)) => MsgKind::RaftAppend,
            PeerBody::Raft(RaftRpc::Vote(_)) => MsgKind::RaftVote,
            PeerBody::Raft(RaftRpc::InstallSnapshot(_)) => MsgKind::RaftSnapshot,
            PeerBody::Propose(_) => MsgKind::Propose,
            PeerBody::ReadIndex => MsgKind::ReadIndex,
            PeerBody::FaultOrder { .. } => MsgKind::FaultOrder,
            PeerBody::Bid(_) => MsgKind::Bid,
            PeerBody::Heartbeat => MsgKind::PeerHeartbeat,
        }
    }
}

/// Lock-free counters of messages sent and received, by type.
#[derive(Debug, Default)]
pub struct MessageCounters {
    sent: [AtomicU64; MsgKind::ALL.len()],
    received: [AtomicU64; MsgKind::ALL.len()],
}

impl MessageCounters {
    pub fn sent(&self, kind: MsgKind) {
        self.sent[kind as usize].fetch_add(1, Ordering::Relaxed);
    }

    pub fn received(&self, kind: MsgKind) {
        self.received[kind as usize].fetch_add(1, Ordering::Relaxed);
    }

    /// Cumulative counts as a `Messages` event.
    pub fn snapshot(&self) -> Event {
        let read = |counters: &[AtomicU64]| {
            MsgKind::ALL
                .iter()
                .map(|k| (k.as_str(), counters[*k as usize].load(Ordering::Relaxed)))
                .collect()
        };
        Event::Messages {
            sent: read(&self.sent),
            received: read(&self.received),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_one_json_object_per_line() {
        let dir = std::env::temp_dir().join(format!("rescuemesh-metrics-{}", std::process::id()));
        let log = EventLog::to_dir(&dir, "node-9").unwrap();
        log.log(Event::Silenced { ms: 5 });
        log.log(Event::Restored);
        log.close();
        let text = fs::read_to_string(dir.join("node-9.jsonl")).unwrap();
        let lines: Vec<serde_json::Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["ev"], "silenced");
        assert_eq!(lines[0]["ms"], 5);
        assert_eq!(lines[0]["src"], "node-9");
        assert!(lines[1]["ts_us"].as_u64().unwrap() >= lines[0]["ts_us"].as_u64().unwrap());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn counts_messages_by_kind() {
        let counters = MessageCounters::default();
        counters.sent(MsgKind::Bid);
        counters.sent(MsgKind::Bid);
        counters.received(MsgKind::RaftVote);
        let Event::Messages { sent, received } = counters.snapshot() else {
            panic!("wrong event");
        };
        assert_eq!(sent["bid"], 2);
        assert_eq!(received["raft_vote"], 1);
        assert_eq!(received["bid"], 0);
    }
}
