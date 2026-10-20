//! RescueMesh: fault-tolerant incident coordination on a three-node Raft cloud.
//!
//! See README.md for what the system does and AGENTS.md for how the code is organised.

pub mod bidding;
pub mod campus;
pub mod client;
pub mod config;
pub mod metrics;
pub mod node;
pub mod planner;
pub mod proto;
pub mod raft;
pub mod state;

/// Log to stderr. `RUST_LOG` overrides the default filter, e.g. `RUST_LOG=debug,openraft=info`.
pub fn init_tracing() {
    use std::io::IsTerminal;
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,openraft=warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .try_init();
}

/// Resolves on Ctrl-C, or on SIGTERM on Linux/macOS (what `kill` and scripts send).
pub async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
