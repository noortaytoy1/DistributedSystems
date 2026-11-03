//! A restarted node starts quietly: it does not ask openraft for what it already has, and openraft
//! logs an ERROR for every `initialize` it refuses (D137).
//!
//! In a test binary of its own because it installs the process's one global log subscriber:
//! openraft logs from its own task, which a subscriber for one thread would not see.
#![allow(clippy::unwrap_used)]

mod common;

use std::io::Write;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{TestCluster, wait_until};
use rescuemesh::node;
use tokio::net::TcpListener;
use tokio::time::Instant;
use tracing_subscriber::EnvFilter;

/// Collects what the subscriber writes.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl Captured {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

impl Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Bind a port the stopped node just gave up. Until its aborted tasks finish it may still be taken.
async fn rebind(addr: &str) -> TcpListener {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match TcpListener::bind(addr).await {
            Ok(listener) => return listener,
            Err(e) if Instant::now() > deadline => panic!("cannot bind {addr} again: {e}"),
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_restart_logs_no_error() {
    let out = Captured::default();
    let writer = out.clone();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(rescuemesh::DEFAULT_LOG_FILTER))
        .with_writer(move || writer.clone())
        .with_ansi(false)
        .init();

    let dir = std::env::temp_dir().join(format!("rescuemesh-restart-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    // One node, so nothing a peer does can add a line.
    let log_file_dir = dir.clone();
    let cluster = TestCluster::start_with(1, move |c| c.storage.dir = Some(log_file_dir)).await;
    wait_until(Duration::from_secs(10), "a leader", || {
        cluster.node(1).ctx.consensus.status().leader.is_some()
    })
    .await;
    let config = cluster.config.clone();
    cluster.shutdown().await;

    tracing::error!("the capture works");
    assert!(out.text().contains("the capture works"));
    let first_run = out.text().len();

    let me = config.node(1).unwrap();
    let peer = rebind(&me.peer_addr).await;
    let client = rebind(&me.client_addr).await;
    let back = node::start_with(config.clone(), 1, peer, client)
        .await
        .unwrap();

    // Alone it can only serve by leading itself, which needs the membership from its log file.
    wait_until(Duration::from_secs(15), "serving again", || {
        back.ctx.serving.load(Ordering::SeqCst)
    })
    .await;
    back.shutdown().await;
    let _ = std::fs::remove_dir_all(&dir);

    let restart = out.text()[first_run..].to_string();
    assert!(
        !restart.contains("ERROR"),
        "a restart logged an error:\n{restart}"
    );
}
