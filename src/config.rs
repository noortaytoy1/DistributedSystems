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
    pub client_retry_ms: u64,
    pub client_max_retries: u32,
    /// How long a node keeps trying to commit one proposal (including leader changes).
    pub propose_timeout_ms: u64,
    /// Timeout for one non-Raft peer RPC (bid, heartbeat, forwarded proposal attempt).
    pub peer_rpc_timeout_ms: u64,
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
            client_max_retries: 3,
            propose_timeout_ms: 3000,
            peer_rpc_timeout_ms: 500,
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
    pub fn propose_timeout(&self) -> Duration {
        Duration::from_millis(self.propose_timeout_ms)
    }
    pub fn peer_rpc_timeout(&self) -> Duration {
        Duration::from_millis(self.peer_rpc_timeout_ms)
    }
}

/// How a request is assigned to the node that plans it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AssignmentPolicy {
    /// Per-request load-based bidding election (bidding.rs).
    Bidding,
    /// Baseline: the Raft leader plans every request.
    // TODO(B): make `Bidding` the default once bid rounds work end to end.
    #[default]
    LeaderDispatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    pub assignment: AssignmentPolicy,
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
}

impl Default for CampusConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            zones: 6,
            responders: 30,
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
        let t = &self.timers;
        if t.raft_heartbeat_ms == 0 || t.raft_heartbeat_ms >= t.election_timeout_min_ms {
            return invalid("timers: need 0 < raft_heartbeat_ms < election_timeout_min_ms".into());
        }
        if t.election_timeout_min_ms >= t.election_timeout_max_ms {
            return invalid(
                "timers: need election_timeout_min_ms < election_timeout_max_ms".into(),
            );
        }
        for (name, v) in [
            ("peer_heartbeat_ms", t.peer_heartbeat_ms),
            ("bid_window_ms", t.bid_window_ms),
            ("client_retry_ms", t.client_retry_ms),
            ("propose_timeout_ms", t.propose_timeout_ms),
            ("peer_rpc_timeout_ms", t.peer_rpc_timeout_ms),
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
        assert_eq!(c.policy.assignment, AssignmentPolicy::LeaderDispatch);
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
    }
}
