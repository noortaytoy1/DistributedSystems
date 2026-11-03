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

/// Wall-clock time in unix milliseconds, the unit of the timestamps inside incidents and results.
pub fn now_ms() -> u64 {
    now_us() / 1_000
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
    /// Once a second: the proposal queue and the Raft log file (D133). All cumulative, except
    /// `waiting` (now) and the two largest-so-far figures (since the last event). Commands per
    /// save is `saved_commands / saves`; disk syncs per second comes from `syncs`.
    RaftIo {
        /// Proposals in the queue now.
        waiting: usize,
        /// `client_write` calls the queue started, and the commands they carried.
        writes: u64,
        commands: u64,
        /// Proposals that left the queue unwritten at their deadline.
        expired: u64,
        /// Writes that outlasted the write cap (`propose_timeout_ms`).
        write_timeouts: u64,
        /// Proposals answered, and the time they took from entering the queue: mean log wait over
        /// a second is the change in `answered_us` over the change in `answered` (D218).
        answered: u64,
        answered_us: u64,
        /// Writes finished, and the time they took with no queueing in it: mean write time over a
        /// second is the change in `write_us` over the change in `writes_done` (D228).
        writes_done: u64,
        write_us: u64,
        /// Writes to the log file that waited for the disk, and the time they took.
        syncs: u64,
        sync_us: u64,
        max_sync_us: u64,
        /// Calls to save log entries, and the entries and commands they carried.
        saves: u64,
        saved_entries: u64,
        saved_commands: u64,
        /// Longest gap between two AppendEntries arriving while this node follows: how close to an
        /// election it came.
        max_leader_gap_us: u64,
    },
    /// This Raft leader found itself overloaded and went quiet, so that a normal vote picks a new
    /// leader (D218, D228). All from the last second: `score` is its own mean bid, `write_us` the
    /// mean time one log write took, `log_wait_us` the mean time per proposal with the queue in it,
    /// which the rule does not read, `save_us` its mean disk save, `load` what the rule compared
    /// (the score, or the write time if that is over its bar and higher), and `peer_scores` the
    /// peers' mean bids.
    LeaderSteppedAside {
        term: u64,
        score: u64,
        write_us: u64,
        log_wait_us: u64,
        save_us: u64,
        load: u64,
        peer_scores: BTreeMap<NodeId, u64>,
    },
    /// How that ended. `outcome` is `changed` (another node leads), `re_elected` (this node won the
    /// vote), `timed_out` (no new leader in time, and this node carries on if it still leads) or
    /// `stopped` (this node was silenced, stopped serving or went degraded). `term` is the term it
    /// went quiet in, and `waited_ms` runs from then to here, on this node's clock.
    LeaderStepAsideEnded {
        term: u64,
        outcome: &'static str,
        new_term: u64,
        new_leader: Option<NodeId>,
        waited_ms: u64,
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
    /// queue (D128). `cpu_ticks`, the byte counts and `udp_drops` are cumulative. `udp_drops` is the
    /// machine's `UdpRcvbufErrors`: datagrams the kernel threw away for want of buffer (D171).
    ResourceUsage {
        rss_kb: Option<u64>,
        cpu_ticks: Option<u64>,
        net_rx_bytes: Option<u64>,
        net_tx_bytes: Option<u64>,
        udp_drops: Option<u64>,
        queued: usize,
        running: usize,
    },

    // --- UDP client path (C, F)
    /// A UDP socket opened, and the receive buffer the kernel really gave it (D171).
    UdpSocket {
        local_addr: String,
        rcvbuf_asked_bytes: u64,
        rcvbuf_got_bytes: u64,
    },
    /// Once a second on nodes and clients: what this process's UDP sockets did, cumulative (D172).
    /// `udp_drops` is as in `resource_usage`, for client PCs, which log no `resource_usage`. It is
    /// absent from the last line a client writes as it stops.
    UdpCounters {
        #[serde(flatten)]
        counts: UdpCounts,
        udp_drops: Option<u64>,
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
    /// A report reached the node that would plan it too old, so nobody is planned for it (D143).
    /// Rare: the default TTL outlasts every retry, so a measured run should log none (D147).
    ReportExpired {
        request: RequestId,
        age_ms: u64,
        ttl_ms: u64,
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
    /// One round of the fault election as this node saw it, once per round (D158, D161). `tickets`
    /// are the numbers it held. `victim` and `ms` are null if it never held all of them, and then it
    /// did nothing. `called` is true if this node's timer started the round. `election_us` runs from
    /// this node's own bid to the last bid it needed, on this node's clock, and is null with `victim`.
    FaultElection {
        round: u64,
        tickets: BTreeMap<NodeId, u64>,
        victim: Option<NodeId>,
        ms: Option<u64>,
        called: bool,
        election_us: Option<u64>,
    },
    /// This node was elected and stayed up: it was catching up, or already silenced.
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
    /// This node's list of who is in the group, once at start and once per change (D152, D155).
    /// `view` counts this process's changes only, so it means nothing across nodes, and it starts
    /// again at 1 in the same file after a restart.
    GroupView {
        view: u64,
        members: Vec<NodeId>,
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
    /// A load program's clock starts, and its warm-up with it. Its first send can come up to two
    /// think times later, as its pattern says, so that send cannot stand in for this (D182). `users`
    /// is 0 in an open loop, which has none.
    ClientStarted {
        users: u32,
    },
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
        /// True if the plan sent nobody, because no suitable responder was free (D111) or the
        /// report was too old (D143).
        no_responder: bool,
        /// Where the incident stood when this answer was given (D142): `provisional`, `unassigned`,
        /// `assigned` or `expired`.
        status: &'static str,
        /// The plan's priority points (D148).
        priority: u32,
    },
    ClientGaveUp {
        request: RequestId,
        attempts: u32,
    },
    /// A client connection to `node` was opened (D112). Nothing logs this since reports moved to
    /// UDP, which has no connection (D167). Kept because the analysis reads the name in old runs.
    ClientLinkUp {
        node: NodeId,
    },
    /// A client connection to `node` closed or stopped answering pings (D112). As `client_link_up`.
    ClientLinkDown {
        node: NodeId,
        reason: String,
    },
    /// Once a second, from one program on each client PC: the whole PC's CPU and memory (D185). A
    /// saturated client PC reads as cloud latency, so the analysis must be able to see it. The tick
    /// counts are cumulative.
    MachineUsage {
        cpu_busy_ticks: Option<u64>,
        cpu_total_ticks: Option<u64>,
        mem_available_kb: Option<u64>,
    },
}

/// The whole machine's CPU and memory, as a `machine_usage` event. Blocking file reads: run it off
/// the async runtime. A file that cannot be read leaves its fields out. It lives here, not in
/// `node/usage.rs`, so a load generator never has to reach into the node's code (D185).
pub fn machine_usage() -> Event {
    let file = |path| fs::read_to_string(path).ok();
    let (cpu_busy_ticks, cpu_total_ticks) = file("/proc/stat")
        .as_deref()
        .and_then(parse_machine_cpu)
        .unzip();
    Event::MachineUsage {
        cpu_busy_ticks,
        cpu_total_ticks,
        mem_available_kb: file("/proc/meminfo")
            .as_deref()
            .and_then(parse_mem_available_kb),
    }
}

/// `(busy, total)` from the first line of `/proc/stat`: user, nice, system, idle, iowait, irq,
/// softirq, steal. The guest fields after them are already counted in user and nice. A CPU waiting
/// on the disk has nothing to do, so iowait counts as idle.
fn parse_machine_cpu(stat: &str) -> Option<(u64, u64)> {
    let mut fields = stat.lines().next()?.split_whitespace();
    if fields.next()? != "cpu" {
        return None;
    }
    let ticks: Vec<u64> = fields
        .take(8)
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let total: u64 = ticks.iter().sum();
    let idle = ticks.get(3)?.saturating_add(*ticks.get(4)?);
    Some((total.saturating_sub(idle), total))
}

fn parse_mem_available_kb(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find(|l| l.starts_with("MemAvailable:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
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

    /// False for a log that writes nothing, so a caller can skip work only the log would use.
    pub fn is_enabled(&self) -> bool {
        self.sink.is_some()
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
    /// One node's numbers in a fault election (D158).
    FaultBid,
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
        MsgKind::FaultBid,
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
            MsgKind::FaultBid => "fault_bid",
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
            PeerBody::FaultBid(_) => MsgKind::FaultBid,
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

/// Declares the UDP counters once, as atomics to count with and as plain numbers to log.
macro_rules! udp_counters {
    ($($(#[$doc:meta])* $name:ident,)*) => {
        /// Lock-free counters of what one process's UDP sockets did (D172).
        #[derive(Debug, Default)]
        pub struct UdpCounters {
            $($(#[$doc])* pub $name: AtomicU64,)*
        }

        /// [`UdpCounters`] at one moment. Field names are the analysis contract.
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
        pub struct UdpCounts {
            $($(#[$doc])* pub $name: u64,)*
        }

        impl UdpCounters {
            pub fn snapshot(&self) -> UdpCounts {
                UdpCounts {
                    $($name: self.$name.load(Ordering::Relaxed),)*
                }
            }
        }
    };
}

udp_counters! {
    /// Every datagram read, good or bad.
    rx_datagrams,
    rx_bytes,
    rx_max_bytes,
    /// Datagrams that did not decode, reads that failed, and calls with a deadline tarpc cannot hold.
    rx_bad,
    /// On a client: datagrams from an address other than the node the socket talks to.
    rx_foreign,
    /// On a node: calls and cancels read, before the fault gate.
    rx_requests,
    rx_cancels,
    /// On a node: calls the fault gate dropped.
    rx_gate_dropped,
    /// On a node: messages dropped because their program's queue was full.
    rx_queue_full,
    /// On a node: client programs seen, so one per program unless it stays idle for 60 s.
    channels_opened,
    /// Every datagram the kernel took.
    tx_datagrams,
    tx_bytes,
    tx_max_bytes,
    /// On a client: calls and cancels handed to the socket, including any lost below.
    tx_requests,
    tx_cancels,
    /// On a node: replies handed to the socket, including any lost below.
    tx_replies,
    /// Messages lost because the kernel's send buffer was full.
    tx_would_block,
    /// Messages lost because they would not encode or did not fit one datagram.
    tx_encode_errors,
    /// Messages lost to any other send error, such as a pulled cable.
    tx_errors,
}

impl UdpCounters {
    pub fn received(&self, bytes: usize) {
        self.rx_datagrams.fetch_add(1, Ordering::Relaxed);
        self.rx_bytes.fetch_add(bytes as u64, Ordering::Relaxed);
        self.rx_max_bytes.fetch_max(bytes as u64, Ordering::Relaxed);
    }

    pub fn sent(&self, bytes: usize) {
        self.tx_datagrams.fetch_add(1, Ordering::Relaxed);
        self.tx_bytes.fetch_add(bytes as u64, Ordering::Relaxed);
        self.tx_max_bytes.fetch_max(bytes as u64, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn udp_counters_log_as_flat_fields() {
        let counters = UdpCounters::default();
        counters.received(100);
        counters.received(40);
        counters.sent(70);
        counters.tx_cancels.fetch_add(2, Ordering::Relaxed);
        let line = Line {
            ts_us: 1,
            src: "client-1",
            event: &Event::UdpCounters {
                counts: counters.snapshot(),
                udp_drops: Some(9),
            },
        };
        let json: serde_json::Value = serde_json::to_value(&line).unwrap();
        assert_eq!(json["ev"], "udp_counters");
        assert_eq!(json["rx_datagrams"], 2);
        assert_eq!(json["rx_bytes"], 140);
        assert_eq!(json["rx_max_bytes"], 100);
        assert_eq!(json["tx_datagrams"], 1);
        assert_eq!(json["tx_cancels"], 2);
        assert_eq!(json["udp_drops"], 9);
    }

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

    #[test]
    fn machine_cpu_comes_from_the_first_stat_line() {
        // 100 + 5 + 20 busy, 800 + 50 idle, 3 + 2 + 1 busy, and guest fields that are not added.
        let stat = "cpu  100 5 20 800 50 3 2 1 70 0\ncpu0 1 1 1 1 1 1 1 1 0 0\nintr 12345\n";
        assert_eq!(parse_machine_cpu(stat), Some((131, 981)));
        assert_eq!(parse_machine_cpu("cpu0 1 1 1 1\n"), None);
        assert_eq!(parse_machine_cpu("cpu  1 x 1 1\n"), None);
        assert_eq!(parse_machine_cpu(""), None);
    }

    #[test]
    fn mem_available_comes_from_meminfo() {
        let meminfo =
            "MemTotal:       16000000 kB\nMemFree:  1000 kB\nMemAvailable:    8000000 kB\n";
        assert_eq!(parse_mem_available_kb(meminfo), Some(8_000_000));
        assert_eq!(parse_mem_available_kb("MemTotal: 1 kB\n"), None);
    }

    #[test]
    fn step_aside_events_keep_their_field_names() {
        let aside = Event::LeaderSteppedAside {
            term: 4,
            score: 9_000,
            write_us: 8_000,
            log_wait_us: 12_000,
            save_us: 3_000,
            load: 9_000,
            peer_scores: BTreeMap::from([(2, 1_500), (3, 2_000)]),
        };
        let text = serde_json::to_string(&aside).unwrap();
        assert!(text.contains(r#""ev":"leader_stepped_aside""#), "{text}");
        assert!(text.contains(r#""term":4"#), "{text}");
        assert!(text.contains(r#""score":9000"#), "{text}");
        assert!(text.contains(r#""write_us":8000"#), "{text}");
        assert!(text.contains(r#""log_wait_us":12000"#), "{text}");
        assert!(text.contains(r#""save_us":3000"#), "{text}");
        assert!(text.contains(r#""load":9000"#), "{text}");
        assert!(
            text.contains(r#""peer_scores":{"2":1500,"3":2000}"#),
            "{text}"
        );

        let ended = Event::LeaderStepAsideEnded {
            term: 4,
            outcome: "changed",
            new_term: 5,
            new_leader: Some(2),
            waited_ms: 700,
        };
        let text = serde_json::to_string(&ended).unwrap();
        assert!(text.contains(r#""ev":"leader_step_aside_ended""#), "{text}");
        assert!(text.contains(r#""outcome":"changed""#), "{text}");
        assert!(text.contains(r#""new_term":5"#), "{text}");
        assert!(text.contains(r#""new_leader":2"#), "{text}");
        assert!(text.contains(r#""waited_ms":700"#), "{text}");
    }

    #[test]
    fn machine_usage_keeps_its_field_names() {
        let event = Event::MachineUsage {
            cpu_busy_ticks: Some(1),
            cpu_total_ticks: Some(2),
            mem_available_kb: None,
        };
        let text = serde_json::to_string(&event).unwrap();
        assert!(text.contains(r#""ev":"machine_usage""#), "{text}");
        assert!(text.contains(r#""cpu_busy_ticks":1"#), "{text}");
        assert!(text.contains(r#""cpu_total_ticks":2"#), "{text}");
        assert!(text.contains(r#""mem_available_kb":null"#), "{text}");
    }
}
