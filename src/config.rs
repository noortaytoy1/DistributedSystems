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
    /// Address clients and `ctl` connect to, e.g. "10.0.0.11:8001".
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
    /// How long a node keeps trying to commit one proposal (including leader changes).
    pub propose_timeout_ms: u64,
    /// Timeout for one non-Raft peer RPC (bid, heartbeat, forwarded proposal attempt).
    pub peer_rpc_timeout_ms: u64,
    /// Every peer suspected for this long, and a serving node answers without a commit (D121).
    /// One peer suspected for this long leaves this node's group member list (D153).
    pub degraded_after_ms: u64,
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
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            assignment: AssignmentPolicy::default(),
            score: ScoreName::default(),
            cpu_weight: 10,
            just_won: JustWonMode::default(),
            initial_service_us: 1,
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
}

impl Default for CampusConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            zones: 6,
            responders: 30,
            service_ms_per_severity: 200,
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
        let t = &self.timers;
        if t.raft_heartbeat_ms == 0 || t.raft_heartbeat_ms >= t.election_timeout_min_ms {
            return invalid("timers: need 0 < raft_heartbeat_ms < election_timeout_min_ms".into());
        }
        if t.election_timeout_min_ms >= t.election_timeout_max_ms {
            return invalid(
                "timers: need election_timeout_min_ms < election_timeout_max_ms".into(),
            );
        }
        if t.client_retry_ms > t.client_retry_max_ms {
            return invalid("timers: need client_retry_ms <= client_retry_max_ms".into());
        }
        if self.campus.service_ms_per_severity == 0 {
            return invalid("campus: service_ms_per_severity must be > 0".into());
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
