//! Operator tool: query nodes, silence a node, skew load.
//!
//! ```text
//! cargo run --bin ctl -- status
//! cargo run --bin ctl -- silence --node 2 --ms 5000
//! cargo run --bin ctl -- skew --node 1 --work 200000
//! ```
//!
//! Owner: C (faults and failover).

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use rescuemesh::client::control;
use rescuemesh::config::Config;
use rescuemesh::proto::{ControlReply, ControlRequest};

#[derive(Parser)]
#[command(about = "Inspect and inject faults into a RescueMesh cluster")]
struct Args {
    #[arg(long, default_value = "config/local.toml")]
    config: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show term, leader, log position and state digest of every node.
    Status,
    /// Drop all traffic in and out of a node for a while.
    Silence {
        #[arg(long)]
        node: u64,
        #[arg(long, default_value_t = 5000)]
        ms: u64,
    },
    /// Add planner work per request on one node to create load imbalance.
    Skew {
        #[arg(long)]
        node: u64,
        #[arg(long)]
        work: u32,
    },
}

const TIMEOUT: Duration = Duration::from_secs(2);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rescuemesh::init_tracing();
    let args = Args::parse();
    let config =
        Config::load(&args.config).with_context(|| format!("loading {}", args.config.display()))?;

    match args.cmd {
        Cmd::Status => status(&config).await,
        Cmd::Silence { node, ms } => send(&config, node, ControlRequest::Silence { ms }).await,
        Cmd::Skew { node, work } => {
            send(&config, node, ControlRequest::SkewLoad { extra_work: work }).await
        }
    }
}

async fn status(config: &Config) -> anyhow::Result<()> {
    println!(
        "{:>4} {:>10} {:>6} {:>6} {:>8} {:>8} {:>18} {:>8} {:>7} {:>8}",
        "node",
        "role",
        "term",
        "leader",
        "log",
        "applied",
        "digest",
        "results",
        "serving",
        "silenced"
    );
    for n in &config.cluster.nodes {
        match control(&n.client_addr, ControlRequest::Status, TIMEOUT).await {
            Ok(ControlReply::Status(s)) => println!(
                "{:>4} {:>10} {:>6} {:>6} {:>8} {:>8} {:>18} {:>8} {:>7} {:>8}",
                s.id,
                s.role,
                s.term,
                opt(s.leader),
                opt(s.last_log_index),
                opt(s.last_applied),
                format!("{:016x}", s.digest),
                s.results,
                s.serving,
                s.silenced
            ),
            Ok(other) => println!("{:>4} unexpected reply: {other:?}", n.id),
            Err(e) => println!("{:>4} unreachable: {e}", n.id),
        }
    }
    Ok(())
}

async fn send(config: &Config, node: u64, request: ControlRequest) -> anyhow::Result<()> {
    let Some(addr) = config.node(node).map(|n| n.client_addr.clone()) else {
        bail!("node {node} is not in the config");
    };
    match control(&addr, request, TIMEOUT).await? {
        ControlReply::Done => {
            println!("ok");
            Ok(())
        }
        ControlReply::Error(e) => bail!("node {node}: {e}"),
        other => bail!("node {node}: unexpected reply {other:?}"),
    }
}

fn opt<T: ToString>(v: Option<T>) -> String {
    v.map(|v| v.to_string()).unwrap_or_else(|| "-".into())
}
