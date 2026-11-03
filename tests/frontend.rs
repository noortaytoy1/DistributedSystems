//! The real `frontend` program with real `loadgen` children, against real nodes: what the unit
//! tests in `src/bin/frontend.rs` cannot reach, because `cargo test --bin` builds no other program.
#![allow(clippy::unwrap_used)]

mod common;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use common::TestCluster;
use rescuemesh::config::Config;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tokio::time::Instant;

const PATIENCE: Duration = Duration::from_secs(30);

/// The real program on port 0, reading a config file as it would on a lab PC.
struct Page {
    child: Child,
    addr: String,
    dir: PathBuf,
}

impl Page {
    async fn start(config: &Config, name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("rescuemesh-frontend-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut config = config.clone();
        config.metrics.dir = Some(dir.join("logs"));
        let path = dir.join("config.toml");
        std::fs::write(&path, toml::to_string(&config).unwrap()).unwrap();

        let mut child = Command::new(env!("CARGO_BIN_EXE_frontend"))
            .arg("--config")
            .arg(&path)
            .arg("--client-id")
            .arg("11")
            .arg("--listen")
            .arg("127.0.0.1:0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut line = String::new();
        BufReader::new(stdout).read_line(&mut line).await.unwrap();
        let addr = line
            .trim()
            .strip_prefix("listening on ")
            .unwrap_or_else(|| panic!("first line was {line:?}"))
            .to_string();
        Self { child, addr, dir }
    }

    async fn http(&self, method: &str, target: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(&self.addr).await.unwrap();
        let request =
            format!("{method} {target} HTTP/1.1\r\nHost: test\r\nContent-Length: 0\r\n\r\n");
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).await.unwrap();
        let (head, body) = answer.split_once("\r\n\r\n").unwrap();
        (
            head.split(' ').nth(1).unwrap().parse().unwrap(),
            body.to_string(),
        )
    }

    async fn json(&self, method: &str, target: &str) -> (u16, Value) {
        let (status, body) = self.http(method, target).await;
        (status, serde_json::from_str(&body).unwrap())
    }

    async fn wait_for_rows(&self, what: &str, ok: impl Fn(&[Value]) -> bool) -> Vec<Value> {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let (_, load) = self.json("GET", "/api/load").await;
            let rows = load["rows"].as_array().unwrap();
            if ok(rows) {
                return rows.clone();
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for {what}: {load}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

async fn ready_cluster() -> TestCluster {
    let cluster = TestCluster::start(3).await;
    common::wait_until(PATIENCE, "a leader and responders everywhere", || {
        cluster
            .nodes
            .iter()
            .all(|n| n.ctx.consensus.status().leader.is_some() && n.ctx.status().responders > 0)
    })
    .await;
    cluster
}

/// A reaped process has no entry, and a zombie is as dead as any: tokio reaps a killed child on a
/// best-effort basis.
fn is_dead(pid: u64) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        Ok(stat) => stat
            .rsplit(')')
            .next()
            .is_some_and(|rest| rest.trim_start().starts_with('Z')),
    }
}

fn pids(rows: &[Value]) -> Vec<u64> {
    rows.iter().map(|r| r["pid"].as_u64().unwrap()).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_typed_report_through_the_real_program_returns_a_card_and_the_page_is_served() {
    let cluster = ready_cluster().await;
    let page = Page::start(&cluster.config, "report").await;

    let (status, html) = page.http("GET", "/").await;
    assert_eq!(status, 200);
    assert!(html.contains("<title>RescueMesh client</title>"));

    let (status, card) = page
        .json(
            "POST",
            "/api/report?zone=2&severity=4&needs=Medical&who=7&valid_for_s=60",
        )
        .await;
    assert_eq!(status, 200, "{card}");
    assert_eq!(card["ok"], true);
    assert!(card["request"].as_str().unwrap().starts_with("11-"));
    assert!(card["log_index"].is_u64());
    assert!(card["planned_by"].is_u64());

    let (status, nodes) = page.json("GET", "/api/status").await;
    assert_eq!(status, 200);
    assert_eq!(nodes["nodes"].as_array().unwrap().len(), 3);
    assert!(
        nodes["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|n| n["reachable"] == true)
    );
    drop(page);
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_loadgen_that_reaches_its_duration_shows_its_summary() {
    let cluster = ready_cluster().await;
    let page = Page::start(&cluster.config, "summary").await;

    let (status, answer) = page
        .json(
            "POST",
            "/api/load/start?users=12&think_ms=500&duration_s=1&pattern=steady",
        )
        .await;
    assert_eq!(status, 200, "{answer}");
    assert_eq!(answer["started"], 10);

    let rows = page
        .wait_for_rows("every program to finish", |rows| {
            rows.len() == 10 && rows.iter().all(|r| r["state"] == "finished")
        })
        .await;
    let users: Vec<u64> = rows.iter().map(|r| r["users"].as_u64().unwrap()).collect();
    assert_eq!(users, [2, 2, 1, 1, 1, 1, 1, 1, 1, 1]);
    for (row, n) in rows.iter().zip(1u64..) {
        let id = 11_000 + n;
        assert_eq!(row["client_id"], id);
        assert_eq!(row["exit_code"], 0);
        let summary = row["summary"].as_str().unwrap();
        // loadgen's own words, shown as they are.
        assert!(
            summary.contains(&format!("client {id}: done ")),
            "{summary}"
        );
        assert!(summary.contains("throughput"), "{summary}");
        assert!(summary.contains("latency ms:"), "{summary}");
    }
    drop(page);
    cluster.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn load_start_runs_real_loadgens_and_stop_ends_them() {
    let cluster = ready_cluster().await;
    let page = Page::start(&cluster.config, "stop").await;

    let (status, answer) = page
        .json(
            "POST",
            "/api/load/start?users=10&think_ms=1000&duration_s=60",
        )
        .await;
    assert_eq!(status, 200, "{answer}");
    let (_, load) = page.json("GET", "/api/load").await;
    assert_eq!(load["running_users"], 10);
    assert_eq!(load["running_programs"], 10);
    let running = pids(load["rows"].as_array().unwrap());
    assert_eq!(running.len(), 10);
    for pid in &running {
        // A real program of its own, with the arguments the page promised.
        let cmdline = std::fs::read_to_string(format!("/proc/{pid}/cmdline")).unwrap();
        assert!(cmdline.contains("loadgen"), "{cmdline}");
        assert!(cmdline.contains("--users"), "{cmdline}");
    }

    let (status, answer) = page.json("POST", "/api/load/stop").await;
    assert_eq!(status, 200);
    assert_eq!(answer["stopping"], 10);
    page.wait_for_rows("every program to stop", |rows| {
        rows.iter().all(|r| r["state"] == "stopped")
    })
    .await;
    for pid in running {
        assert!(is_dead(pid), "{pid} is still running");
    }
    drop(page);
    cluster.shutdown().await;
}

/// `kill` by process ID, since unsafe is forbidden and libc is not a direct dependency.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sigterm_stops_the_children_and_closes_the_log() {
    let cluster = ready_cluster().await;
    let mut page = Page::start(&cluster.config, "term").await;

    let (_, card) = page
        .json("POST", "/api/report?zone=1&severity=2&needs=Fire")
        .await;
    assert_eq!(card["ok"], true, "{card}");
    let (status, answer) = page
        .json(
            "POST",
            "/api/load/start?users=3&think_ms=1000&duration_s=60",
        )
        .await;
    assert_eq!(status, 200, "{answer}");
    let (_, load) = page.json("GET", "/api/load").await;
    let running = pids(load["rows"].as_array().unwrap());
    assert_eq!(running.len(), 3);

    let pid = page.child.id().unwrap();
    let term = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .await
        .unwrap();
    assert!(term.success());
    let exit = tokio::time::timeout(PATIENCE, page.child.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(exit.success(), "{exit:?}");
    for child in running {
        assert!(is_dead(child), "{child} outlived the frontend");
    }
    // The log was closed, so what was logged is on disk: the report's two client events.
    let log = std::fs::read_to_string(log_path(&page.dir)).unwrap();
    assert!(log.contains("\"ev\":\"client_sent\""), "{log}");
    assert!(log.contains("\"ev\":\"client_done\""), "{log}");
    drop(page);
    cluster.shutdown().await;
}

fn log_path(dir: &Path) -> PathBuf {
    dir.join("logs").join("frontend-11.jsonl")
}
