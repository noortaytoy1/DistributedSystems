//! `scripts/start-users.sh`: real loadgen programs sharing the users (D181, D183). Dry runs check the
//! arithmetic for IDs, shares and start times; real runs check that the signals, the PID file and the
//! totals work, with a real cluster where answers matter.
//!
//! Each test keeps its PID file and logs in its own temporary directory, so they can run together.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use common::TestCluster;

const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/start-users.sh");

fn bin_dir() -> PathBuf {
    Path::new(env!("CARGO_BIN_EXE_loadgen"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// A temporary directory for one test, removed when the test ends well.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rescuemesh-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn logs(&self) -> PathBuf {
        self.0.join("logs")
    }

    fn pid_file(&self, machine: u32, batch: u32) -> PathBuf {
        self.0.join(format!("users-{machine}-{batch}.pid"))
    }

    fn out_file(&self, machine: u32, batch: u32) -> PathBuf {
        self.0.join(format!("users-{machine}-{batch}.out"))
    }

    fn config(&self) -> PathBuf {
        self.0.join("config.toml")
    }

    /// A config for a node nobody listens on, with retries short enough that requests give up fast.
    fn write_closed_config(&self) {
        // Held together, so the two ports differ, then closed.
        let peer = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let client = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let text = format!(
            "[[cluster.nodes]]\nid = 1\npeer_addr = \"{}\"\nclient_addr = \"{}\"\n\n\
             [timers]\nclient_retry_ms = 20\nclient_retry_max_ms = 40\nclient_max_retries = 1\n\n\
             [metrics]\ndir = \"{}\"\n",
            peer.local_addr().unwrap(),
            client.local_addr().unwrap(),
            self.logs().display()
        );
        std::fs::write(self.config(), text).unwrap();
    }

    /// A config for the given cluster's addresses.
    fn write_cluster_config(&self, cluster: &TestCluster) {
        let mut text = String::new();
        for n in &cluster.config.cluster.nodes {
            text += &format!(
                "[[cluster.nodes]]\nid = {}\npeer_addr = \"{}\"\nclient_addr = \"{}\"\n\n",
                n.id, n.peer_addr, n.client_addr
            );
        }
        text += &format!("[metrics]\ndir = \"{}\"\n", self.logs().display());
        std::fs::write(self.config(), text).unwrap();
    }

    fn script(&self) -> Command {
        let mut command = Command::new("bash");
        command
            .arg(SCRIPT)
            .env("BIN", bin_dir())
            .env("PID_DIR", &self.0)
            .env_remove("PROGRAMS_PER_PC")
            .stdin(Stdio::null());
        command
    }

    /// The script with a config and an output directory, ready for a real start.
    fn start_command(&self, machine: u32, extra: &[&str]) -> Command {
        let mut command = self.script();
        command
            .arg("--config")
            .arg(self.config())
            .arg("--out")
            .arg(&self.0)
            .args(["--machine", &machine.to_string()])
            .args(extra);
        command
    }

    /// Every event of this type in every log, as parsed JSON.
    fn events(&self, kind: &str) -> Vec<serde_json::Value> {
        let mut found = Vec::new();
        for path in self.log_files() {
            for line in std::fs::read_to_string(path).unwrap().lines() {
                let event: serde_json::Value = serde_json::from_str(line).unwrap();
                if event["ev"] == kind {
                    found.push(event);
                }
            }
        }
        found
    }

    fn log_files(&self) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(self.logs()) else {
            return Vec::new();
        };
        entries.map(|e| e.unwrap().path()).collect()
    }

    /// How many programs have logged a `client_sent`. By then each has its signal handler: the
    /// handler goes in before the first user runs.
    fn programs_that_sent(&self) -> usize {
        self.log_files()
            .iter()
            .filter(|path| {
                std::fs::read_to_string(path)
                    .unwrap_or_default()
                    .contains("\"ev\":\"client_sent\"")
            })
            .count()
    }

    /// True while anything still runs with this test's config on its command line.
    fn something_runs(&self) -> bool {
        Command::new("pgrep")
            .arg("-f")
            .arg(self.config())
            .stdout(Stdio::null())
            .status()
            .unwrap()
            .success()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

/// Poll `cond` until it holds; panic with `what` after `timeout`.
fn poll_until(timeout: Duration, what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while !cond() {
        assert!(
            Instant::now() < deadline,
            "timed out after {timeout:?} waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn signal(name: &str, pid: u32) {
    let status = Command::new("kill")
        .args([&format!("-{name}"), &pid.to_string()])
        .status()
        .unwrap();
    assert!(status.success());
}

/// Wait for a spawned script, with a deadline so a hung script fails the test instead of the run.
fn finish(mut child: Child, timeout: Duration) -> Output {
    poll_until(timeout, "the script to end", || {
        child.try_wait().unwrap().is_some()
    });
    child.wait_with_output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// One program line of a dry run.
#[derive(Debug)]
struct Planned {
    client_id: u64,
    users: u64,
    start_ms: u64,
    seed: u64,
}

fn field(line: &str, name: &str) -> u64 {
    let prefix = format!("{name}=");
    line.split_whitespace()
        .find_map(|word| word.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("no {name} in {line}"))
        .parse()
        .unwrap()
}

fn dry_run(scratch: &Scratch, args: &[&str]) -> Output {
    scratch
        .script()
        .args(["--dry-run", "--config", "any.toml"])
        .args(args)
        .output()
        .unwrap()
}

fn plan(scratch: &Scratch, args: &[&str]) -> Vec<Planned> {
    let output = dry_run(scratch, args);
    assert!(output.status.success(), "{}", text(&output.stderr));
    text(&output.stdout)
        .lines()
        .filter(|line| line.starts_with("program "))
        .map(|line| Planned {
            client_id: field(line, "client_id"),
            users: field(line, "users"),
            start_ms: field(line, "start_ms"),
            seed: field(line, "seed"),
        })
        .collect()
}

#[test]
fn the_default_is_ten_programs_sharing_the_users() {
    let scratch = Scratch::new("users-default");
    let programs = plan(&scratch, &["--machine", "1", "--users", "1000"]);
    assert_eq!(programs.len(), 10);
    assert!(programs.iter().all(|p| p.users == 100));
    // Fewer users than programs: never a program with nobody in it.
    let few = plan(&scratch, &["--machine", "1", "--users", "3"]);
    assert_eq!(few.len(), 3);
    assert!(few.iter().all(|p| p.users == 1));
    // 34 users: 4, 4, 4, 4, 3, 3, 3, 3, 3, 3, so the total stays exact.
    let uneven = plan(&scratch, &["--machine", "1", "--users", "34"]);
    let shares: Vec<u64> = uneven.iter().map(|p| p.users).collect();
    assert_eq!(shares, [4, 4, 4, 4, 3, 3, 3, 3, 3, 3]);
}

#[test]
fn the_number_of_programs_is_a_setting() {
    let scratch = Scratch::new("users-setting");
    let flag = plan(
        &scratch,
        &["--machine", "1", "--users", "100", "--programs", "4"],
    );
    assert_eq!(flag.len(), 4);
    let output = scratch
        .script()
        .env("PROGRAMS_PER_PC", "5")
        .args(["--dry-run", "--config", "any.toml"])
        .args(["--machine", "1", "--users", "100"])
        .output()
        .unwrap();
    let programs = text(&output.stdout)
        .lines()
        .filter(|line| line.starts_with("program "))
        .count();
    assert_eq!(programs, 5);
}

#[test]
fn dry_run_ids_are_unique_and_users_sum_exactly() {
    let scratch = Scratch::new("users-ids");
    let mut ids = BTreeSet::new();
    for users in [30, 100, 300, 1000, 1500, 3000] {
        for limit in [1, 10, 100, 500, 1000] {
            // The batch makes each setting's IDs its own only within one PC and batch, so the
            // no-collision check runs per setting across PCs and batches.
            ids.clear();
            for machine in [1, 2, 3] {
                for batch in [0, 9] {
                    let programs = plan(
                        &scratch,
                        &[
                            "--machine",
                            &machine.to_string(),
                            "--batch",
                            &batch.to_string(),
                            "--users",
                            &users.to_string(),
                            "--programs",
                            &limit.to_string(),
                            "--seed-base",
                            "3000000",
                        ],
                    );
                    assert_eq!(programs.len() as u64, users.min(limit));
                    assert_eq!(programs.iter().map(|p| p.users).sum::<u64>(), users);
                    for p in &programs {
                        assert!(ids.insert(p.client_id), "{} is used twice", p.client_id);
                        // Five digits: clear of 1-9 by hand, 11-13 frontends and 21-23 zone feeds.
                        assert!((10_000..100_000).contains(&p.client_id));
                        assert_eq!(p.seed, 3_000_000 + p.client_id);
                    }
                }
            }
        }
    }
}

#[test]
fn the_id_shows_the_machine_the_batch_and_the_program() {
    let scratch = Scratch::new("users-id-digits");
    let programs = plan(
        &scratch,
        &["--machine", "2", "--batch", "9", "--users", "30"],
    );
    let ids: Vec<u64> = programs.iter().map(|p| p.client_id).collect();
    assert_eq!(ids, (29_000..29_010).collect::<Vec<_>>());
}

#[test]
fn dry_run_gives_every_user_its_own_slot() {
    let scratch = Scratch::new("users-slots");
    let (users, think_ms) = (1000u64, 1000u64);
    for limit in [10u64, 100, 1000] {
        let programs = plan(
            &scratch,
            &[
                "--machine",
                "1",
                "--users",
                &users.to_string(),
                "--programs",
                &limit.to_string(),
            ],
        );
        // Inside loadgen, user j of a program with U users first sends j*T/U after its start.
        let mut slots = BTreeSet::new();
        for p in &programs {
            for j in 0..p.users {
                assert!(slots.insert(p.start_ms + j * think_ms / p.users));
            }
        }
        assert_eq!(slots.len() as u64, users, "at {limit} programs");
        assert_eq!(*slots.iter().next_back().unwrap(), think_ms - 1);
    }
}

#[test]
fn all_at_once_has_no_start_spread_and_steady_and_random_do() {
    let scratch = Scratch::new("users-spread");
    let args = |pattern: &'static str| ["--machine", "1", "--users", "100", "--pattern", pattern];
    let starts = |pattern| -> Vec<u64> {
        plan(&scratch, &args(pattern))
            .iter()
            .map(|p| p.start_ms)
            .collect()
    };
    assert_eq!(starts("all-at-once"), vec![0; 10]);
    assert_eq!(
        starts("steady"),
        (0..10).map(|k| k * 10).collect::<Vec<_>>()
    );
    assert_eq!(starts("random"), starts("steady"));
}

#[test]
fn dry_run_refuses_what_would_collide_or_mean_nothing() {
    let scratch = Scratch::new("users-refuse");
    let refused: &[&[&str]] = &[
        &["--users", "10"],                                         // no machine
        &["--machine", "1"],                                        // no users
        &["--machine", "0", "--users", "10"],                       // would land on hand-run IDs
        &["--machine", "10", "--users", "10"],                      // a sixth digit
        &["--machine", "1", "--users", "0"],                        // nobody
        &["--machine", "1", "--users", "ten"],                      // not a number
        &["--machine", "1", "--users", "10", "--batch", "10"],      // would collide with machine 2
        &["--machine", "1", "--users", "10", "--programs", "1001"], // would collide with batch 1
        &["--machine", "1", "--users", "10", "--programs", "0"],
        &["--machine", "1", "--users", "10", "--pattern", "spike"],
        &[
            "--machine",
            "1",
            "--users",
            "10",
            "--pattern",
            "random",
            "--think-ms",
            "0",
        ],
        &[
            "--machine",
            "1",
            "--users",
            "10",
            "--pattern",
            "all-at-once",
            "--think-ms",
            "0",
        ],
        &["--machine", "1", "--users", "10", "--duration-s", "0"],
        &[
            "--machine",
            "1",
            "--users",
            "10",
            "--duration-s",
            "5",
            "--warmup-s",
            "5",
        ],
        &["--machine", "1", "--users", "10", "--no-such-flag", "1"],
    ];
    for args in refused {
        let output = dry_run(&scratch, args);
        assert_eq!(output.status.code(), Some(2), "{args:?} was not refused");
        assert!(!text(&output.stderr).is_empty(), "{args:?} gave no reason");
    }
    // The edges that must still work.
    let last = plan(
        &scratch,
        &[
            "--machine",
            "9",
            "--batch",
            "9",
            "--users",
            "1000",
            "--programs",
            "1000",
        ],
    );
    assert_eq!(last.last().unwrap().client_id, 99_999);
}

#[test]
fn a_real_start_gives_one_log_per_program() {
    let scratch = Scratch::new("users-real");
    scratch.write_closed_config();
    let output = scratch
        .start_command(
            1,
            &[
                "--users",
                "5",
                "--programs",
                "5",
                "--think-ms",
                "100",
                "--duration-s",
                "1",
            ],
        )
        .output()
        .unwrap();
    let stdout = text(&output.stdout);
    assert!(output.status.success(), "{stdout}{}", text(&output.stderr));
    assert!(
        stdout.starts_with("plan machine=1 batch=0 users=5 programs=5"),
        "{stdout}"
    );
    assert!(stdout.contains("started 5, failed 0"), "{stdout}");
    assert!(stdout.contains("reported 5 of 5 programs"), "{stdout}");

    let logs: BTreeSet<String> = scratch
        .log_files()
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    let expected: BTreeSet<String> = (10_000..10_005)
        .map(|id| format!("client-{id}.jsonl"))
        .collect();
    assert_eq!(logs, expected);
    assert_eq!(scratch.programs_that_sent(), 5);
    // Only program 0 of batch 0 samples the machine, and that sample is real on Linux.
    let samples = scratch.events("machine_usage");
    assert!(!samples.is_empty());
    assert!(samples.iter().all(|e| e["src"] == "client-10000"));
    assert!(samples[0]["cpu_total_ticks"].as_u64().unwrap() > 0);
    assert!(samples[0]["mem_available_kb"].as_u64().unwrap() > 0);
    // The PID file goes when the script ends, and every program's output is in one file.
    assert!(!scratch.pid_file(1, 0).exists());
    let out = std::fs::read_to_string(scratch.out_file(1, 0)).unwrap();
    let summaries = out.lines().filter(|l| l.starts_with("client 1")).count();
    assert_eq!(summaries, 5, "{out}");
}

#[test]
fn term_leaves_no_child_no_pid_file_and_a_summary_from_each_program() {
    let scratch = Scratch::new("users-term");
    scratch.write_closed_config();
    let child = scratch
        .start_command(
            1,
            &[
                "--users",
                "9",
                "--programs",
                "3",
                "--think-ms",
                "100",
                "--duration-s",
                "120",
            ],
        )
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    poll_until(Duration::from_secs(30), "every program to send", || {
        scratch.programs_that_sent() == 3
    });
    assert_eq!(
        std::fs::read_to_string(scratch.pid_file(1, 0))
            .unwrap()
            .trim(),
        child.id().to_string()
    );
    signal("TERM", child.id());
    let output = finish(child, Duration::from_secs(30));
    let stdout = text(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(stdout.contains("stopped early"), "{stdout}");
    assert!(stdout.contains("started 3, failed 0"), "{stdout}");
    assert!(stdout.contains("reported 3 of 3 programs"), "{stdout}");
    assert!(!scratch.pid_file(1, 0).exists());
    assert!(!scratch.something_runs());
    let out = std::fs::read_to_string(scratch.out_file(1, 0)).unwrap();
    assert_eq!(out.matches("stopped by a signal").count(), 3, "{out}");
    // Flushed: the logs hold what happened right up to the signal.
    assert!(scratch.events("client_sent").len() >= 3);
}

#[test]
fn a_second_start_is_refused_while_the_first_lives_and_stop_ends_it() {
    let scratch = Scratch::new("users-second");
    scratch.write_closed_config();
    let args = ["--users", "2", "--think-ms", "100", "--duration-s", "120"];
    let first = scratch
        .start_command(2, &args)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    poll_until(Duration::from_secs(30), "the programs to send", || {
        scratch.programs_that_sent() == 2
    });

    let second = scratch.start_command(2, &args).output().unwrap();
    assert_eq!(second.status.code(), Some(2));
    assert!(text(&second.stderr).contains("already running"));

    // Another batch on the same PC is allowed: that is what batches are for.
    let other = scratch
        .start_command(
            2,
            &[
                "--batch",
                "1",
                "--users",
                "1",
                "--think-ms",
                "100",
                "--duration-s",
                "1",
            ],
        )
        .output()
        .unwrap();
    assert!(other.status.success(), "{}", text(&other.stderr));

    let stop = scratch
        .script()
        .args(["--stop", "--machine", "2"])
        .output()
        .unwrap();
    assert!(stop.status.success(), "{}", text(&stop.stderr));
    let first = finish(first, Duration::from_secs(30));
    assert!(first.status.success());
    assert!(text(&first.stdout).contains("reported 2 of 2 programs"));
    assert!(!scratch.something_runs());

    // Nothing left to stop is no error.
    let again = scratch
        .script()
        .args(["--stop", "--machine", "2"])
        .output()
        .unwrap();
    assert!(again.status.success());
    assert!(text(&again.stdout).contains("nothing running"));
}

#[test]
fn a_stale_pid_file_does_not_block_a_start() {
    let scratch = Scratch::new("users-stale");
    scratch.write_closed_config();
    // A PID that is alive but is not this script, as after a reboot.
    std::fs::write(scratch.pid_file(3, 0), format!("{}\n", std::process::id())).unwrap();
    let output = scratch
        .start_command(
            3,
            &["--users", "1", "--think-ms", "100", "--duration-s", "1"],
        )
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output.stderr));
}

#[test]
fn a_dead_program_counts_as_failed_and_the_rest_keep_going() {
    let scratch = Scratch::new("users-dead");
    scratch.write_closed_config();
    let child = scratch
        .start_command(
            4,
            &["--users", "10", "--think-ms", "100", "--duration-s", "120"],
        )
        .stdout(Stdio::piped())
        // bash reports the killed program here, which is expected and only clutters the test output.
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    poll_until(Duration::from_secs(30), "every program to send", || {
        scratch.programs_that_sent() == 10
    });
    let found = Command::new("pgrep")
        .arg("-f")
        .arg(format!("{} --client-id 40003", scratch.config().display()))
        .output()
        .unwrap();
    let pid: u32 = text(&found.stdout).trim().parse().unwrap();
    signal("KILL", pid);
    // The other nine are still there.
    poll_until(Duration::from_secs(10), "program 3 to go", || {
        !Command::new("pgrep")
            .arg("-f")
            .arg(format!("{} --client-id 40003", scratch.config().display()))
            .stdout(Stdio::null())
            .status()
            .unwrap()
            .success()
    });
    assert!(scratch.something_runs());
    signal("TERM", child.id());
    let output = finish(child, Duration::from_secs(30));
    let stdout = text(&output.stdout);
    // One of ten is exactly 10 percent, which does not stop a series: exit 1, not 2.
    assert_eq!(output.status.code(), Some(1), "{stdout}");
    assert!(
        stdout.contains("client 40003 exited with status 137"),
        "{stdout}"
    );
    assert!(stdout.contains("started 10, failed 1"), "{stdout}");
    assert!(stdout.contains("reported 9 of 10 programs"), "{stdout}");
}

#[test]
fn more_than_a_tenth_failing_exits_with_two() {
    let scratch = Scratch::new("users-fail");
    // Starts fine as far as the script can tell, but every program refuses this config.
    std::fs::write(scratch.config(), "no_such_key = 1\n").unwrap();
    let output = scratch
        .start_command(
            5,
            &["--users", "4", "--think-ms", "100", "--duration-s", "1"],
        )
        .output()
        .unwrap();
    let stdout = text(&output.stdout);
    assert_eq!(output.status.code(), Some(2), "{stdout}");
    assert!(stdout.contains("started 4, failed 4"), "{stdout}");
    assert!(stdout.contains("reported 0 of 4 programs"), "{stdout}");
    assert!(!scratch.pid_file(5, 0).exists());
}

/// Run the script as a child of this test's runtime, which also runs the cluster.
async fn start_and_wait(scratch: &Scratch, machine: u32, extra: &[&str]) -> Output {
    let command = scratch.start_command(machine, extra);
    tokio::process::Command::from(command)
        .output()
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_pattern_gets_answers_from_a_real_cluster() {
    let cluster = TestCluster::start(3).await;
    common::wait_until(Duration::from_secs(10), "a leader on every node", || {
        cluster
            .nodes
            .iter()
            .all(|n| n.ctx.consensus.status().leader.is_some())
    })
    .await;

    for (machine, pattern) in [(1, "steady"), (2, "random"), (3, "all-at-once")] {
        let scratch = Scratch::new(&format!("users-cluster-{pattern}"));
        scratch.write_cluster_config(&cluster);
        let output = start_and_wait(
            &scratch,
            machine,
            &[
                "--users",
                "8",
                "--programs",
                "2",
                "--think-ms",
                "500",
                "--pattern",
                pattern,
                "--duration-s",
                "2",
            ],
        )
        .await;
        let stdout = text(&output.stdout);
        assert!(output.status.success(), "{pattern}: {stdout}");
        assert!(
            stdout.contains("started 2, failed 0"),
            "{pattern}: {stdout}"
        );
        assert!(
            stdout.contains("reported 2 of 2 programs"),
            "{pattern}: {stdout}"
        );
        assert!(stdout.contains("gave up 0"), "{pattern}: {stdout}");
        assert!(
            !scratch.events("client_done").is_empty(),
            "{pattern}: {stdout}"
        );
    }
    cluster.shutdown().await;
}

fn first_sends_us(scratch: &Scratch) -> Vec<u64> {
    let mut times: Vec<u64> = scratch
        .events("client_sent")
        .iter()
        .filter(|e| e["attempt"] == 0)
        .map(|e| e["ts_us"].as_u64().unwrap())
        .collect();
    times.sort_unstable();
    times
}

/// Sleeps can only run long, so these bounds hold however busy the machine is: a first wave that
/// went out together would fail the first test, and an all-at-once user off the beat the second.
#[test]
fn steady_spreads_the_first_sends_over_the_think_time() {
    let scratch = Scratch::new("users-steady-times");
    scratch.write_closed_config();
    let output = scratch
        .start_command(
            6,
            &[
                "--users",
                "20",
                "--programs",
                "2",
                "--think-ms",
                "1000",
                "--duration-s",
                "2",
            ],
        )
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output.stdout));
    let times = first_sends_us(&scratch);
    assert!(times.len() >= 20, "{} first sends", times.len());
    // The slots run from 0 to 950 ms, and no user sends a second time before 1000 ms.
    let span_ms = (times[19] - times[0]) / 1000;
    assert!(span_ms >= 900, "the first 20 sends took only {span_ms} ms");
}

#[test]
fn all_at_once_users_send_on_a_multiple_of_the_think_time() {
    let scratch = Scratch::new("users-all-at-once-times");
    scratch.write_closed_config();
    let output = scratch
        .start_command(
            7,
            &[
                "--users",
                "6",
                "--programs",
                "2",
                "--think-ms",
                "2000",
                "--pattern",
                "all-at-once",
                "--duration-s",
                "5",
            ],
        )
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output.stdout));
    let times = first_sends_us(&scratch);
    assert!(times.len() >= 6, "{} first sends", times.len());
    for ts in times {
        let past_the_beat_ms = (ts % 2_000_000) / 1000;
        assert!(
            past_the_beat_ms < 1000,
            "a send {past_the_beat_ms} ms after the beat"
        );
    }
}
