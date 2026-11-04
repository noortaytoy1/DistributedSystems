//! Starts one cloud node.
//!
//! ```text
//! cargo run --release --bin node -- --config config/local.toml --id 1
//! cargo run --release --bin node -- --config config/local.toml --id 1 --faults off
//! ```
//!
//! Owner: shared.

use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, ValueEnum};
use rescuemesh::config::Config;

#[derive(Parser)]
#[command(about = "Run one RescueMesh cloud node")]
struct Args {
    /// Cluster config shared by every node and client.
    #[arg(long, default_value = "config/local.toml")]
    config: PathBuf,
    /// This node's ID in the config.
    #[arg(long)]
    id: u64,
    /// Override `faults.enabled` from the config for this run (D77).
    #[arg(long, value_enum)]
    faults: Option<Switch>,
}

#[derive(Clone, Copy, ValueEnum)]
enum Switch {
    On,
    Off,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rescuemesh::init_tracing();
    let args = Args::parse();
    let mut config =
        Config::load(&args.config).with_context(|| format!("loading {}", args.config.display()))?;
    if let Some(switch) = args.faults {
        config.faults.enabled = matches!(switch, Switch::On);
    }
    let node = rescuemesh::node::start(config, args.id).await?;
    tracing::info!(
        "node {} up: peers on {}, clients on {}, fault injection {}",
        args.id,
        node.peer_addr,
        node.client_addr,
        if node.ctx.config.faults.enabled {
            "on"
        } else {
            "off"
        }
    );
    rescuemesh::shutdown_signal().await;
    tracing::info!("shutting down");
    node.shutdown().await;
    Ok(())
}
