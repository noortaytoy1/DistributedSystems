//! Starts one cloud node.
//!
//! ```text
//! cargo run --release --bin node -- --config config/local.toml --id 1
//! ```

use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rescuemesh::init_tracing();
    let args = Args::parse();
    let config =
        Config::load(&args.config).with_context(|| format!("loading {}", args.config.display()))?;
    let node = rescuemesh::node::start(config, args.id).await?;
    tracing::info!(
        "node {} up: peers on {}, clients on {}",
        args.id,
        node.peer_addr,
        node.client_addr
    );
    rescuemesh::shutdown_signal().await;
    tracing::info!("shutting down");
    node.shutdown().await;
    Ok(())
}
