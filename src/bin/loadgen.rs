//! Simulated users. Each user is a tokio task that reports incidents.
//!
//! ```text
//! cargo run --release --bin loadgen -- --users 20 --duration-s 30
//! ```
//!
//! Owner: F (client and scale). Template: parses its arguments, generates no load yet.

use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use rescuemesh::config::Config;

#[derive(Parser)]
#[command(about = "Generate incident load against a RescueMesh cluster")]
struct Args {
    #[arg(long, default_value = "config/local.toml")]
    config: PathBuf,
    /// Unique per load generator; random if not given.
    #[arg(long)]
    client_id: Option<u64>,
    /// Concurrent simulated users.
    #[arg(long, default_value_t = 10)]
    users: u32,
    #[arg(long, default_value_t = 10)]
    duration_s: u64,
    /// Pause between a reply and the user's next request (closed loop).
    #[arg(long, default_value_t = 0)]
    think_ms: u64,
    #[arg(long, default_value_t = 1)]
    seed: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rescuemesh::init_tracing();
    let args = Args::parse();
    let _config =
        Config::load(&args.config).with_context(|| format!("loading {}", args.config.display()))?;
    // TODO(F): one `MeshClient` per process; `users` tokio tasks that submit
    // `campus::random_incident`s until `duration_s` passes (closed loop with `think_ms`, plus an
    // open-loop mode at a fixed rate); print throughput and p50/p95/p99 latency.
    let _ = (
        args.client_id,
        args.users,
        args.duration_s,
        args.think_ms,
        args.seed,
    );
    anyhow::bail!("loadgen is not implemented yet (TODO(F))")
}
