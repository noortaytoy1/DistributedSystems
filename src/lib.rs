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

/// The log filter when `RUST_LOG` is not set (D134). tarpc writes several `info` lines for every
/// call, and its `in_flight_requests` modules an `error` for every call past its deadline. Under
/// load that is thousands of writes to stderr a second, each one from async code, which slows the
/// node enough to cost it its leader. tarpc's other warnings stay: they are what shows a broken
/// connection or a mixed build.
pub const DEFAULT_LOG_FILTER: &str = "info,openraft=warn,tarpc=warn,\
     tarpc::client::in_flight_requests=off,tarpc::server::in_flight_requests=off";

/// Log to stderr. `RUST_LOG` overrides [`DEFAULT_LOG_FILTER`], e.g. `RUST_LOG=debug,openraft=info`.
pub fn init_tracing() {
    use std::io::IsTerminal;
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(DEFAULT_LOG_FILTER));
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

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use tracing_subscriber::EnvFilter;

    use super::DEFAULT_LOG_FILTER;

    /// Collects what a subscriber writes.
    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn every_directive_in_the_default_filter_parses() {
        // `EnvFilter::new`, which `init_tracing` uses, silently drops one it cannot read.
        EnvFilter::try_new(DEFAULT_LOG_FILTER).unwrap();
    }

    #[test]
    fn the_default_filter_drops_tarpcs_call_lines_and_keeps_the_rest() {
        let out = Captured::default();
        let writer = out.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new(DEFAULT_LOG_FILTER))
            .with_writer(move || writer.clone())
            .with_ansi(false)
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "rescuemesh::node", "kept: our own line");
            tracing::info!(target: "heartbeat", "kept: the heartbeat summary");
            tracing::warn!(target: "openraft::core", "kept: a Raft warning");
            tracing::warn!(target: "tarpc::client", "kept: Connection broken");
            tracing::info!(target: "openraft::core", "dropped: Raft detail");
            tracing::info!(target: "tarpc::client", "dropped: SendRequest");
            tracing::info!(target: "tarpc::server", "dropped: ReceiveRequest");
            tracing::error!(target: "tarpc::client::in_flight_requests", "dropped: DeadlineExceeded");
            tracing::error!(target: "tarpc::server::in_flight_requests", "dropped: DeadlineExceeded");
        });
        let text = String::from_utf8(out.0.lock().unwrap().clone()).unwrap();
        assert_eq!(text.matches("kept:").count(), 4, "{text}");
        assert!(!text.contains("dropped:"), "{text}");
    }
}
