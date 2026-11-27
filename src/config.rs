//! Cluster configuration: node addresses, timers and policy switches.
//!
//! Every node, client and `ctl` reads the same TOML file (see `config/local.toml`). Unknown keys
//! are rejected so a typo fails loudly instead of silently using a default.
//!
//! Owner: A (Raft and replication).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::bidding::{JustWonMode, ScoreVariant};

use crate::proto::NodeId;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub cluster: ClusterConfig,
    #[serde(default)]
    pub timers: Timers,
    #[serde(default)]
    pub policy: Policy,
    #[serde(default)]
    pub workers: WorkerConfig,
    #[serde(default)]
    pub campus: CampusConfig,
    #[serde(default)]
    pub metrics: MetricsConfig,
    #[serde(default)]
    pub faults: FaultConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    /// The Phase 2 user lifecycle in `loadgen` (Part L, D430). Off by default, so Phase 1's runs repeat.
    #[serde(default)]
    pub workload: WorkloadConfig,
    /// How the nodes reach one order: rotating rounds, or Raft with its elected leader (D249).
    #[serde(default)]
    pub consensus: ConsensusMode,
    /// Client programs talking to each other directly (Phase 2, Part H).
    #[serde(default)]
    pub p2p: P2pConfig,
    /// Zone publish-subscribe among client programs (Phase 2, Part I).
    #[serde(default)]
    pub gossip: GossipSettings,
    /// The presence directory's leases (Phase 2, Part G, D291).
    #[serde(default)]
    pub presence: PresenceSettings,
}

/// See [`Config::consensus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConsensusMode {
    #[default]
    Raft,
    Rounds,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClusterConfig {
    pub nodes: Vec<NodeAddr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeAddr {
    pub id: NodeId,
    /// Address for Raft, bids and heartbeats, e.g. "10.0.0.11:7001".
    pub peer_addr: String,
    /// Address for clients, over UDP, and for `ctl`, over TCP, e.g. "10.0.0.11:8001" (D168).
    pub client_addr: String,
}

/// Every timer in the system, in milliseconds. Defaults are the LAN starting values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Timers {
    pub raft_heartbeat_ms: u64,
    pub election_timeout_min_ms: u64,
    pub election_timeout_max_ms: u64,
    pub peer_heartbeat_ms: u64,
    pub bid_window_ms: u64,
    pub takeover_missed_heartbeats: u32,
    /// First wait for a reply; each later wait doubles, up to `client_retry_max_ms` (D52).
    pub client_retry_ms: u64,
    pub client_retry_max_ms: u64,
    pub client_max_retries: u32,
    /// How long a node keeps trying to commit one proposal (including leader changes and its wait
    /// in the leader's proposal queue). A proposal never waits longer than this (D133).
    pub propose_timeout_ms: u64,
    /// Timeout for one non-Raft peer RPC (bid, fault bid, read index). A peer heartbeat waits only
    /// `peer_heartbeat_ms`. A forwarded proposal waits up to its `propose_timeout_ms`, because the
    /// leader's queue may hold it that long.
    pub peer_rpc_timeout_ms: u64,
    /// Every peer suspected for this long, and a serving node answers without a commit (D121).
    /// One peer suspected for this long leaves this node's group member list (D153).
    pub degraded_after_ms: u64,
    /// Rotating rounds only: how often a node starts its next round (D249).
    pub round_ms: u64,
}

impl Default for Timers {
    fn default() -> Self {
        Self {
            raft_heartbeat_ms: 50,
            election_timeout_min_ms: 150,
            election_timeout_max_ms: 300,
            peer_heartbeat_ms: 50,
            bid_window_ms: 20,
            takeover_missed_heartbeats: 3,
            client_retry_ms: 2000,
            client_retry_max_ms: 6000,
            client_max_retries: 5,
            propose_timeout_ms: 3000,
            peer_rpc_timeout_ms: 500,
            degraded_after_ms: 2000,
            round_ms: 5,
        }
    }
}

impl Timers {
    pub fn peer_heartbeat(&self) -> Duration {
        Duration::from_millis(self.peer_heartbeat_ms)
    }
    pub fn bid_window(&self) -> Duration {
        Duration::from_millis(self.bid_window_ms)
    }
    pub fn client_retry(&self) -> Duration {
        Duration::from_millis(self.client_retry_ms)
    }
    pub fn client_retry_max(&self) -> Duration {
        Duration::from_millis(self.client_retry_max_ms)
    }
    pub fn propose_timeout(&self) -> Duration {
        Duration::from_millis(self.propose_timeout_ms)
    }
    pub fn peer_rpc_timeout(&self) -> Duration {
        Duration::from_millis(self.peer_rpc_timeout_ms)
    }
    pub fn degraded_after(&self) -> Duration {
        Duration::from_millis(self.degraded_after_ms)
    }
}

/// How a request is assigned to the node that plans it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AssignmentPolicy {
    /// Per-request load-based bidding election (bidding.rs). The Raft leader orders the log; it
    /// does not choose who plans (D114).
    #[default]
    Bidding,
    /// Baseline for E1: the Raft leader plans every request.
    LeaderDispatch,
}

/// Which load score nodes bid with; experiment E6 compares them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScoreName {
    #[default]
    ExpectedWait,
    ExpectedWaitCpu,
    QueueOnly,
    CpuOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    pub assignment: AssignmentPolicy,
    pub score: ScoreName,
    /// Score added per permille of CPU, under `expected-wait-cpu` only.
    pub cpu_weight: u64,
    pub just_won: JustWonMode,
    /// Planning time a node assumes until its first job finishes.
    pub initial_service_us: u64,
    /// A Raft leader whose load stays at least this many times its lowest peer's mean bid goes
    /// quiet, so that a normal vote picks a new leader (D218). Its load is its own mean bid, or how
    /// long one log write takes if that is slow (D228). 0 turns this off. Under bidding only: it
    /// reads the bids.
    pub leader_overload_ratio: u64,
    /// How long that must hold without a break.
    pub leader_overload_for_ms: u64,
    /// No step-aside until this long after the last term change or the last try. This is what stops
    /// a storm of elections.
    pub leader_overload_cooldown_ms: u64,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            assignment: AssignmentPolicy::default(),
            score: ScoreName::default(),
            cpu_weight: 10,
            just_won: JustWonMode::default(),
            initial_service_us: 1,
            leader_overload_ratio: 3,
            leader_overload_for_ms: 5_000,
            leader_overload_cooldown_ms: 60_000,
        }
    }
}

impl Policy {
    pub fn score_variant(&self) -> ScoreVariant {
        match self.score {
            ScoreName::ExpectedWait => ScoreVariant::ExpectedWait,
            ScoreName::ExpectedWaitCpu => ScoreVariant::ExpectedWaitCpu {
                cpu_weight: self.cpu_weight,
            },
            ScoreName::QueueOnly => ScoreVariant::QueueOnly,
            ScoreName::CpuOnly => ScoreVariant::CpuOnly,
        }
    }
    pub fn leader_overload_for(&self) -> Duration {
        Duration::from_millis(self.leader_overload_for_ms)
    }
    pub fn leader_overload_cooldown(&self) -> Duration {
        Duration::from_millis(self.leader_overload_cooldown_ms)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkerConfig {
    /// Planner threads per node.
    pub threads: usize,
    /// Jobs allowed to wait for a thread; more than this and the node sheds load.
    pub queue_capacity: usize,
    /// Planner cost knob: extra CPU work per plan (see planner.rs).
    pub plan_work: u32,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            threads: 4,
            queue_capacity: 64,
            plan_work: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CampusConfig {
    /// Seed for the synthetic campus; every node and client must use the same one.
    pub seed: u64,
    pub zones: u16,
    pub responders: u32,
    /// Simulated time on scene: a responder is held this long per point of severity (D111).
    pub service_ms_per_severity: u64,
    /// How long a client says its reports stay valid (D143). Longer than the client's retries, at
    /// most 36 s by default, so a report is refused only after every resend had its chance.
    pub incident_ttl_ms: u64,
}

impl Default for CampusConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            zones: 6,
            responders: 30,
            service_ms_per_severity: 200,
            incident_ttl_ms: 60_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MetricsConfig {
    /// Directory for JSONL event logs (`node-<id>.jsonl`, `client-<id>.jsonl`).
    /// Unset means events are not written.
    pub dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StorageConfig {
    /// Directory for each node's Raft log file (`node-<id>.wal`). Unset keeps the log in memory
    /// only, so a restarted node has forgotten its vote: tests only, never the lab (D118).
    pub dir: Option<PathBuf>,
}

/// A client program's peer endpoint and how hard it tries to deliver a peer message (Phase 2, Part
/// H, D336–D339).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct P2pConfig {
    /// Each program listens on the first free UDP port from `port_min` to `port_max` and advertises
    /// it in its presence record (D313, D336). `port_min = 0` lets the system pick any port, for
    /// tests on one machine. On the lab's client PCs: `sudo ufw allow 20000:20999/udp`.
    pub port_min: u16,
    pub port_max: u16,
    /// How long one try of a peer message waits for its ack, and how many tries it gets in all,
    /// with the same update ID each time (D339).
    pub try_ms: u64,
    pub tries: u32,
}

impl Default for P2pConfig {
    fn default() -> Self {
        Self {
            port_min: 20_000,
            port_max: 20_999,
            try_ms: 500,
            tries: 3,
        }
    }
}

/// The Phase 2 workload: what `loadgen`'s simulated users do (Part L, D430 to D433). Read only by
/// `loadgen`; nodes ignore it. A cell in `experiments/` overrides it to sweep the mix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WorkloadConfig {
    /// Run the Phase 2 lifecycle (register, subscribe, report, discover, P2P, offer, claim, move,
    /// go offline). False: every user only reports, exactly as in Phase 1.
    pub lifecycle: bool,
    /// Share of users in each role, in percent; the three add up to 100 (D431).
    pub ordinary_percent: u8,
    pub responder_percent: u8,
    pub provider_percent: u8,
    /// Each minute, the chance in percent that a program goes offline (D432). 0: never.
    pub offline_chance_per_min_percent: u8,
    /// How long a program stays offline each time (D432).
    pub offline_for_ms: u64,
    /// How long a responder waits between picking an offer it has heard of and claiming it, so it
    /// claims on older news: the injected delay that sweeps the conflict rate (D442). 0: at once.
    pub claim_delay_ms: u64,
}

impl Default for WorkloadConfig {
    fn default() -> Self {
        Self {
            lifecycle: false,
            ordinary_percent: 80,
            responder_percent: 15,
            provider_percent: 5,
            offline_chance_per_min_percent: 0,
            offline_for_ms: 20_000,
            claim_delay_ms: 0,
        }
    }
}

impl P2pConfig {
    pub fn try_timeout(&self) -> Duration {
        Duration::from_millis(self.try_ms)
    }
}

/// How a client program spreads zone updates (Phase 2, Part I, D364). Every key is optional and
/// defaults to the value `gossip.rs` was measured with (D344, D345).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GossipSettings {
    /// Programs a program sends each new zone update to.
    pub fanout: usize,
    /// The `max_hops` a program puts on what it publishes, and the most it obeys (D346).
    pub hop_cap: u8,
    /// Most entries each seen-cache holds, and how long it keeps one (D344).
    pub seen_cap: usize,
    pub seen_max_age_ms: u64,
    /// How often a zone's list of programs is asked for again, at most (D358).
    pub refresh_ms: u64,
    /// Most gossip sends in flight at once; past it the gossip task waits (D363).
    pub max_in_flight: usize,
}

impl Default for GossipSettings {
    fn default() -> Self {
        Self {
            fanout: crate::gossip::DEFAULT_FANOUT,
            hop_cap: crate::gossip::DEFAULT_HOP_CAP,
            seen_cap: crate::gossip::DEFAULT_SEEN_CAP,
            seen_max_age_ms: crate::gossip::DEFAULT_SEEN_MAX_AGE_MS,
            refresh_ms: crate::gossip::DEFAULT_REFRESH_MS,
            max_in_flight: crate::gossip::DEFAULT_MAX_IN_FLIGHT,
        }
    }
}

impl GossipSettings {
    /// The bounds the pure core takes.
    pub fn core(&self) -> crate::gossip::GossipConfig {
        crate::gossip::GossipConfig {
            fanout: self.fanout,
            hop_cap: self.hop_cap,
            seen_cap: self.seen_cap,
            seen_max_age_ms: self.seen_max_age_ms,
        }
    }

    pub fn refresh(&self) -> Duration {
        Duration::from_millis(self.refresh_ms)
    }
}

/// The presence directory's lease timing (Phase 2, Part G, D284, D291). Every key is optional. The
/// node that proposes a registration or renewal copies `lease_ms` and `grace_ms` into the command, so
/// the log decides a lease, not each node's file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PresenceSettings {
    /// How long a registration or renewal keeps a user discoverable. Clients renew at a third (D320).
    pub lease_ms: u64,
    /// How long after its lease a record is kept, hidden but renewable, before it goes (D285).
    pub grace_ms: u64,
    /// How often the lowest member of a node's view moves the directory's log clock (D281).
    pub tick_ms: u64,
}

impl Default for PresenceSettings {
    fn default() -> Self {
        Self {
            lease_ms: 10_000,
            grace_ms: 5_000,
            tick_ms: 1_000,
        }
    }
}

impl PresenceSettings {
    pub fn tick(&self) -> Duration {
        Duration::from_millis(self.tick_ms)
    }
}

/// Fault injection (faults.rs). `bin/node.rs --faults on|off` overrides `enabled` for one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FaultConfig {
    /// False turns off every injected fault: this node never bids in a fault election, so no
    /// election finishes anywhere, and `ctl silence` is refused (D77, D158).
    pub enabled: bool,
    /// Each node waits a random time in this range, from the end of the last round and any silence
    /// it elected. The first to finish calls the next fault election, so the wait between rounds
    /// averages less than the middle of the range (D159).
    pub interval_min_ms: u64,
    pub interval_max_ms: u64,
    /// Each node draws its silence from this range, at most 20 s. The elected node uses its own.
    pub silence_min_ms: u64,
    pub silence_max_ms: u64,
    /// Seed for this node's numbers; mixed with the round and the node, so one seed repeats the
    /// numbers per round but not the timing. Unset: a random seed per process, logged at start.
    pub seed: Option<u64>,
}

impl Default for FaultConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_min_ms: 30_000,
            interval_max_ms: 60_000,
            silence_min_ms: 1_000,
            silence_max_ms: 20_000,
            seed: None,
        }
    }
}

impl FaultConfig {
    pub fn interval(&self) -> (Duration, Duration) {
        (
            Duration::from_millis(self.interval_min_ms),
            Duration::from_millis(self.interval_max_ms),
        )
    }
    pub fn silence(&self) -> (Duration, Duration) {
        (
            Duration::from_millis(self.silence_min_ms),
            Duration::from_millis(self.silence_max_ms),
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("invalid config: {0}")]
    Invalid(String),
}

impl Config {
    /// Read, parse and validate a config file.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_toml(&text)
    }

    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        let config: Config = toml::from_str(text)?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        let invalid = |msg: String| Err(ConfigError::Invalid(msg));
        let nodes = &self.cluster.nodes;
        if nodes.is_empty() {
            return invalid("cluster.nodes is empty".into());
        }
        let mut ids = BTreeSet::new();
        let mut addrs = BTreeSet::new();
        for n in nodes {
            if n.id == 0 {
                return invalid("node IDs start at 1".into());
            }
            if !ids.insert(n.id) {
                return invalid(format!("duplicate node id {}", n.id));
            }
            for addr in [&n.peer_addr, &n.client_addr] {
                if !addrs.insert(addr.as_str()) {
                    return invalid(format!("address {addr} is used twice"));
                }
            }
        }
        if self.policy.initial_service_us == 0 {
            return invalid("policy: initial_service_us must be above 0".into());
        }
        let p = &self.policy;
        if p.leader_overload_ratio > 0 {
            // At 1 the busiest of three even nodes would count as overloaded.
            if p.leader_overload_ratio < 2 {
                return invalid(
                    "policy: leader_overload_ratio must be 0 (off) or at least 2".into(),
                );
            }
            if p.leader_overload_for_ms == 0 {
                return invalid("policy: leader_overload_for_ms must be > 0".into());
            }
            // A cooldown shorter than one try would let the next try start as this one ends.
            let quiet_ms = crate::node::step_aside::QUIET_LIMIT.as_millis() as u64;
            if p.leader_overload_cooldown_ms < quiet_ms {
                return invalid(format!(
                    "policy: leader_overload_cooldown_ms must be at least {quiet_ms}"
                ));
            }
        }
        let t = &self.timers;
        if t.raft_heartbeat_ms == 0 || t.raft_heartbeat_ms >= t.election_timeout_min_ms {
            return invalid("timers: need 0 < raft_heartbeat_ms < election_timeout_min_ms".into());
        }
        if t.election_timeout_min_ms >= t.election_timeout_max_ms {
            return invalid(
                "timers: need election_timeout_min_ms < election_timeout_max_ms".into(),
            );
        }
        if t.round_ms == 0 {
            return invalid("timers: need round_ms > 0".into());
        }
        if t.client_retry_ms > t.client_retry_max_ms {
            return invalid("timers: need client_retry_ms <= client_retry_max_ms".into());
        }
        if self.campus.service_ms_per_severity == 0 {
            return invalid("campus: service_ms_per_severity must be > 0".into());
        }
        // Every report would be refused at once. `loadgen --ttl-ms 0` is how a demo asks for that.
        if self.campus.incident_ttl_ms == 0 {
            return invalid("campus: incident_ttl_ms must be > 0".into());
        }
        for (name, v) in [
            ("peer_heartbeat_ms", t.peer_heartbeat_ms),
            ("bid_window_ms", t.bid_window_ms),
            ("client_retry_ms", t.client_retry_ms),
            ("client_retry_max_ms", t.client_retry_max_ms),
            ("propose_timeout_ms", t.propose_timeout_ms),
            ("peer_rpc_timeout_ms", t.peer_rpc_timeout_ms),
            ("degraded_after_ms", t.degraded_after_ms),
        ] {
            if v == 0 {
                return invalid(format!("timers: {name} must be > 0"));
            }
        }
        if self.workers.threads == 0 {
            return invalid("workers.threads must be > 0".into());
        }
        if self.campus.zones == 0 {
            return invalid("campus.zones must be > 0".into());
        }
        let g = &self.gossip;
        if let Err(e) = g.core().validate() {
            return invalid(e.to_string());
        }
        if g.refresh_ms == 0 || g.max_in_flight == 0 {
            return invalid("gossip: refresh_ms and max_in_flight must be > 0".into());
        }
        let p = &self.presence;
        if p.lease_ms == 0 || p.tick_ms == 0 || p.tick_ms > p.lease_ms {
            return invalid("presence: need lease_ms > 0 and 0 < tick_ms <= lease_ms".into());
        }
        let f = &self.faults;
        if f.interval_min_ms == 0 || f.interval_min_ms > f.interval_max_ms {
            return invalid("faults: need 0 < interval_min_ms <= interval_max_ms".into());
        }
        let max_fault = crate::node::faults::MAX_FAULT.as_millis() as u64;
        if f.silence_min_ms == 0
            || f.silence_min_ms > f.silence_max_ms
            || f.silence_max_ms > max_fault
        {
            return invalid(format!(
                "faults: need 0 < silence_min_ms <= silence_max_ms <= {max_fault}"
            ));
        }
        // Not for safety: the all-numbers rule keeps faults apart with no timer in the argument
        // (D158). A node that missed the winner's bid waits only this interval, so a shorter one
        // calls rounds that cannot finish while the winner is out.
        if f.interval_min_ms < f.silence_max_ms {
            return invalid("faults: need interval_min_ms >= silence_max_ms".into());
        }
        let w = &self.workload;
        let roles = u16::from(w.ordinary_percent)
            + u16::from(w.responder_percent)
            + u16::from(w.provider_percent);
        if roles != 100 {
            return invalid(format!(
                "workload: ordinary_percent + responder_percent + provider_percent must be 100, \
                 not {roles}"
            ));
        }
        if w.offline_chance_per_min_percent > 100 {
            return invalid("workload: offline_chance_per_min_percent must be 0 to 100".into());
        }
        // Back within the same minute, so one outage ends before the next draw can start another.
        if w.offline_chance_per_min_percent > 0 && !(1..60_000).contains(&w.offline_for_ms) {
            return invalid("workload: need 0 < offline_for_ms < 60000".into());
        }
        // A user waits it inside one action; a minute already reads as a broken run.
        if w.claim_delay_ms >= 60_000 {
            return invalid("workload: need claim_delay_ms < 60000".into());
        }
        Ok(())
    }

    pub fn node(&self, id: NodeId) -> Option<&NodeAddr> {
        self.cluster.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.cluster.nodes.iter().map(|n| n.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gossip_keys_are_optional_and_validated() {
        let node = "[[cluster.nodes]]\nid = 1\npeer_addr = \"a:1\"\nclient_addr = \"a:2\"\n";
        let config = Config::from_toml(node).unwrap();
        assert_eq!(config.gossip, GossipSettings::default());
        assert_eq!(config.gossip.core(), crate::gossip::GossipConfig::default());
        assert_eq!(config.gossip.refresh(), Duration::from_secs(10));
        assert_eq!(config.gossip.max_in_flight, 256);

        let with = |lines: &str| Config::from_toml(&format!("{node}[gossip]\n{lines}\n"));
        let set = with("fanout = 2\nhop_cap = 4\nrefresh_ms = 500").unwrap();
        assert_eq!((set.gossip.fanout, set.gossip.hop_cap), (2, 4));
        assert_eq!(
            set.gossip.seen_cap, 10_000,
            "keys left out keep their default"
        );
        for bad in [
            "fanout = 0",
            "seen_cap = 0",
            "seen_max_age_ms = 0",
            "refresh_ms = 0",
            "max_in_flight = 0",
            "fan_out = 3",
        ] {
            assert!(with(bad).is_err(), "{bad} must be refused");
        }
    }

    #[test]
    fn presence_keys_are_optional_and_validated() {
        let node = "[[cluster.nodes]]
id = 1
peer_addr = \"a:1\"
client_addr = \"a:2\"
";
        let config = Config::from_toml(node).unwrap();
        assert_eq!(config.presence, PresenceSettings::default());
        assert_eq!(config.presence.lease_ms, 10_000);
        assert_eq!(config.presence.tick(), Duration::from_secs(1));

        let with = |lines: &str| {
            Config::from_toml(&format!(
                "{node}[presence]
{lines}
"
            ))
        };
        let set = with(
            "lease_ms = 2000
grace_ms = 0",
        )
        .unwrap();
        assert_eq!((set.presence.lease_ms, set.presence.grace_ms), (2_000, 0));
        assert_eq!(
            set.presence.tick_ms, 1_000,
            "keys left out keep their default"
        );
        for bad in ["lease_ms = 0", "tick_ms = 0", "lease_ms = 500", "lease = 3"] {
            assert!(with(bad).is_err(), "{bad} must be refused");
        }
    }

    #[test]
    fn policy_keys_parse_and_default() {
        let policy: Policy = toml::from_str(
            "score = \"expected-wait-cpu\"\ncpu_weight = 7\njust_won = \"wins-only\"\n",
        )
        .unwrap();
        assert_eq!(
            policy.score_variant(),
            ScoreVariant::ExpectedWaitCpu { cpu_weight: 7 }
        );
        assert_eq!(policy.just_won, JustWonMode::WinsOnly);
        assert_eq!(policy.initial_service_us, 1);
        assert_eq!(
            Policy::default().score_variant(),
            ScoreVariant::ExpectedWait
        );
        assert_eq!(Policy::default().just_won, JustWonMode::WonAndLeading);
    }

    #[test]
    fn a_busy_leader_steps_aside_by_default_and_ratio_0_is_off() {
        let policy = Policy::default();
        assert_eq!(policy.leader_overload_ratio, 3);
        assert_eq!(policy.leader_overload_for(), Duration::from_secs(5));
        assert_eq!(policy.leader_overload_cooldown(), Duration::from_secs(60));

        let node = "[[cluster.nodes]]\nid = 1\npeer_addr = \"a:1\"\nclient_addr = \"a:2\"\n";
        let with = |lines: &str| Config::from_toml(&format!("{node}[policy]\n{lines}\n"));
        let set = with(
            "leader_overload_ratio = 4\nleader_overload_for_ms = 100\n\
             leader_overload_cooldown_ms = 3000",
        )
        .unwrap();
        assert_eq!(set.policy.leader_overload_ratio, 4);
        assert_eq!(set.policy.leader_overload_for_ms, 100);
        assert_eq!(set.policy.leader_overload_cooldown_ms, 3_000);

        // Off, so the other two keys do not matter.
        assert!(
            with(
                "leader_overload_ratio = 0\nleader_overload_for_ms = 0\n\
                 leader_overload_cooldown_ms = 0"
            )
            .is_ok()
        );
        assert!(with("leader_overload_ratio = 1").is_err());
        assert!(with("leader_overload_for_ms = 0").is_err());
        assert!(with("leader_overload_cooldown_ms = 2999").is_err());
    }

    #[test]
    fn unknown_policy_values_are_rejected() {
        assert!(toml::from_str::<Policy>("just_won = \"maybe\"\n").is_err());
        assert!(toml::from_str::<Policy>("score = \"guess\"\n").is_err());
    }

    #[test]
    fn shipped_configs_are_valid() {
        for text in [
            include_str!("../config/local.toml"),
            include_str!("../config/lab.example.toml"),
        ] {
            let c = Config::from_toml(text).unwrap();
            assert_eq!(c.cluster.nodes.len(), 3);
        }
    }

    #[test]
    fn defaults_fill_missing_sections() {
        let c = Config::from_toml(
            r#"
            [[cluster.nodes]]
            id = 1
            peer_addr = "127.0.0.1:7001"
            client_addr = "127.0.0.1:8001"
            "#,
        )
        .unwrap();
        assert_eq!(c.timers, Timers::default());
        assert_eq!(c.policy.assignment, AssignmentPolicy::Bidding);
        assert!(c.faults.enabled);
    }

    #[test]
    fn faults_can_be_switched_off() {
        let c = Config::from_toml(
            r#"
            [[cluster.nodes]]
            id = 1
            peer_addr = "127.0.0.1:7001"
            client_addr = "127.0.0.1:8001"

            [faults]
            enabled = false
            "#,
        )
        .unwrap();
        assert!(!c.faults.enabled);
    }

    #[test]
    fn incident_ttl_defaults_to_sixty_seconds_and_zero_is_rejected() {
        let node = "[[cluster.nodes]]\nid = 1\npeer_addr = \"127.0.0.1:7001\"\nclient_addr = \"127.0.0.1:8001\"\n";
        assert_eq!(
            Config::from_toml(node).unwrap().campus.incident_ttl_ms,
            60_000
        );
        let zero = format!("{node}[campus]\nincident_ttl_ms = 0\n");
        assert!(Config::from_toml(&zero).is_err());
        let short = format!("{node}[campus]\nincident_ttl_ms = 1\n");
        assert_eq!(Config::from_toml(&short).unwrap().campus.incident_ttl_ms, 1);
    }

    #[test]
    fn the_workload_is_phase_1_by_default_and_its_shares_must_add_up() {
        let node = "[[cluster.nodes]]\nid = 1\npeer_addr = \"127.0.0.1:7001\"\nclient_addr = \"127.0.0.1:8001\"\n";
        let w = Config::from_toml(node).unwrap().workload;
        assert!(!w.lifecycle, "Phase 1 runs must repeat unchanged");
        assert_eq!(
            (w.ordinary_percent, w.responder_percent, w.provider_percent),
            (80, 15, 5)
        );
        assert_eq!(w.offline_chance_per_min_percent, 0);
        let with = |lines: &str| Config::from_toml(&format!("{node}[workload]\n{lines}\n"));
        assert!(with("lifecycle = true\nordinary_percent = 70\nresponder_percent = 20\nprovider_percent = 10").is_ok());
        assert!(
            with("ordinary_percent = 90").is_err(),
            "90 + 15 + 5 is not 100"
        );
        assert!(with("offline_chance_per_min_percent = 101").is_err());
        assert!(with("offline_chance_per_min_percent = 10\noffline_for_ms = 0").is_err());
        assert!(with("offline_chance_per_min_percent = 10\noffline_for_ms = 60000").is_err());
        assert!(with("offline_chance_per_min_percent = 10\noffline_for_ms = 59999").is_ok());
        // With no outages the length is never used, so any value is fine.
        assert!(with("offline_for_ms = 0").is_ok());
        assert_eq!(
            w.claim_delay_ms, 0,
            "claims go at once unless a cell injects a delay"
        );
        assert!(with("claim_delay_ms = 2000").is_ok());
        assert!(with("claim_delay_ms = 60000").is_err());
    }

    #[test]
    fn the_default_ttl_outlasts_every_retry_at_full_jitter() {
        let t = Timers::default();
        let mut total_ms = 0;
        let mut wait_ms = t.client_retry_ms;
        for _ in 0..=t.client_max_retries {
            total_ms += wait_ms;
            wait_ms = (wait_ms * 2).min(t.client_retry_max_ms);
        }
        assert_eq!(total_ms, 30_000, "2, 4, 6, 6, 6 and 6 s");
        assert!(CampusConfig::default().incident_ttl_ms > total_ms * 120 / 100);
    }

    #[test]
    fn rejects_bad_configs() {
        let node = |id: u64, port: u16| {
            format!(
                "[[cluster.nodes]]\nid = {id}\npeer_addr = \"127.0.0.1:{port}\"\nclient_addr = \"127.0.0.1:{}\"\n",
                port + 1000
            )
        };
        let dup_id = format!("{}{}", node(1, 7001), node(1, 7002));
        assert!(Config::from_toml(&dup_id).is_err());
        let dup_addr = format!("{}{}", node(1, 7001), node(2, 7001));
        assert!(Config::from_toml(&dup_addr).is_err());
        let typo = format!("{}[timers]\nraft_heartbeat = 10\n", node(1, 7001));
        assert!(Config::from_toml(&typo).is_err());
        let slow_heartbeat = format!("{}[timers]\nraft_heartbeat_ms = 200\n", node(1, 7001));
        assert!(Config::from_toml(&slow_heartbeat).is_err());
        let long_fault = format!("{}[faults]\nsilence_max_ms = 20001\n", node(1, 7001));
        assert!(Config::from_toml(&long_fault).is_err());
        let backwards = format!(
            "{}[faults]\ninterval_min_ms = 500\ninterval_max_ms = 100\n",
            node(1, 7001)
        );
        assert!(Config::from_toml(&backwards).is_err());
        let never_degraded = format!("{}[timers]\ndegraded_after_ms = 0\n", node(1, 7001));
        assert!(Config::from_toml(&never_degraded).is_err());
        let overlapping = format!(
            "{}[faults]\ninterval_min_ms = 5000\nsilence_max_ms = 10000\n",
            node(1, 7001)
        );
        assert!(Config::from_toml(&overlapping).is_err());
    }
}
