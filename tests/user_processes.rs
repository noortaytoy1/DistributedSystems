//! `scripts/start-users.sh`: real loadgen programs sharing the users (D181, D183). Dry runs check the
//! arithmetic for IDs, shares and start times; real runs check that the signals, the PID file and the
//! totals work, with a real cluster where answers matter.
//!
//! Each test keeps its PID file and logs in its own temporary directory, so they can run together.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use common::TestCluster;

const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/start-users.sh");

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

    fn config(&self) -> PathBuf {
        self.0.join("config.toml")
    }

    fn pid_file(&self, machine: u32, batch: u32) -> PathBuf {
        self.0.join(format!("users-{machine}-{batch}.pid"))
    }

    fn out_file(&self, machine: u32, batch: u32) -> PathBuf {
        self.0.join(format!("users-{machine}-{batch}.out"))
    }

    /// A config for a node nobody listens on, with retries short enough that requests give up fast.
    fn write_closed_config(&self) {
        // Held together so the two ports differ, then closed.
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

    /// The script with only this test's environment. `flags` is split on spaces.
    fn script(&self, flags: &str) -> Command {
        let bin = Path::new(env!("CARGO_BIN_EXE_loadgen")).parent().unwrap();
        let mut command = Command::new("bash");
        command
            .arg(SCRIPT)
            .args(flags.split_whitespace())
            .env("BIN", bin)
            .env("PID_DIR", &self.0)
            .env_remove("PROGRAMS_PER_PC")
            .stdin(Stdio::null());
        command
    }

    /// The script with a config and an output directory, ready for a real start.
    fn start(&self, flags: &str) -> Command {
        let mut command = self.script(flags);
        command
            .arg("--config")
            .arg(self.config())
            .arg("--out")
            .arg(&self.0);
        command
    }

    /// Start in the background, with its output kept for `finish`.
    fn spawn(&self, flags: &str) -> Child {
        self.start(flags)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }

    fn log_files(&self) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(self.logs()) else {
            return Vec::new();
        };
        entries.map(|e| e.unwrap().path()).collect()
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

    fn wait_for_programs(&self, programs: usize) {
        poll_until(Duration::from_secs(30), "the programs to send", || {
            self.programs_that_sent() == programs
        });
    }

    /// True while anything runs with this text on its command line, as far as `pgrep` sees.
    fn runs(&self, text: &str) -> bool {
        Command::new("pgrep")
            .arg("-f")
            .arg(format!("{}{text}", self.config().display()))
            .stdout(Stdio::null())
            .status()
            .unwrap()
            .success()
    }

    fn output_file_text(&self, machine: u32, batch: u32) -> String {
        std::fs::read_to_string(self.out_file(machine, batch)).unwrap()
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
fn finish(mut child: Child) -> Output {
    poll_until(Duration::from_secs(30), "the script to end", || {
        child.try_wait().unwrap().is_some()
    });
    child.wait_with_output().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
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

fn dry_run(scratch: &Scratch, flags: &str) -> Output {
    let flags = format!("--dry-run --config any.toml {flags}");
    scratch.script(&flags).output().unwrap()
}

fn planned(output: &Output) -> Vec<Planned> {
    stdout(output)
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

fn plan(scratch: &Scratch, flags: &str) -> Vec<Planned> {
    let output = dry_run(scratch, flags);
    assert!(output.status.success(), "{flags}: {}", stderr(&output));
    planned(&output)
}

#[test]
fn help_works_from_any_folder() {
    let scripts = Path::new(SCRIPT).parent().unwrap();
    let output = Command::new("bash")
        .args(["start-users.sh", "--help"])
        .current_dir(scripts)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stdout(&output).contains("--machine M"),
        "{}",
        stdout(&output)
    );
}

#[test]
fn the_default_is_ten_programs_sharing_the_users() {
    let scratch = Scratch::new("users-default");
    let programs = plan(&scratch, "--machine 1 --users 1000");
    assert_eq!(programs.len(), 10);
    assert!(programs.iter().all(|p| p.users == 100));
    // Fewer users than programs: never a program with nobody in it.
    let few = plan(&scratch, "--machine 1 --users 3");
    assert_eq!(few.len(), 3);
    assert!(few.iter().all(|p| p.users == 1));
    // 34 users: the total stays exact.
    let uneven = plan(&scratch, "--machine 1 --users 34");
    let shares: Vec<u64> = uneven.iter().map(|p| p.users).collect();
    assert_eq!(shares, [4, 4, 4, 4, 3, 3, 3, 3, 3, 3]);
}

#[test]
fn the_number_of_programs_is_a_setting() {
    let scratch = Scratch::new("users-setting");
    assert_eq!(
        plan(&scratch, "--machine 1 --users 100 --programs 4").len(),
        4
    );
    let from_the_environment = scratch
        .script("--dry-run --config any.toml --machine 1 --users 100")
        .env("PROGRAMS_PER_PC", "5")
        .output()
        .unwrap();
    assert_eq!(planned(&from_the_environment).len(), 5);
}

#[test]
fn the_plan_line_says_how_the_users_are_shared() {
    let scratch = Scratch::new("users-plan-line");
    let even = dry_run(&scratch, "--machine 1 --users 1000");
    assert!(
        stdout(&even).starts_with("plan machine=1 batch=0 users=1000 programs=10 users_each=100 "),
        "{}",
        stdout(&even)
    );
    let uneven = dry_run(&scratch, "--machine 1 --users 34");
    assert!(stdout(&uneven).contains(" users_each=3-4 "));
}

#[test]
fn dry_run_ids_are_unique_and_users_sum_exactly() {
    let scratch = Scratch::new("users-ids");
    for users in [30, 100, 300, 1000, 1500, 3000] {
        for limit in [1, 10, 100, 500, 1000] {
            // Across the three PCs and two batches that may run at one time.
            let mut ids = BTreeSet::new();
            for machine in [1, 2, 3] {
                for batch in [0, 9] {
                    let flags = format!(
                        "--machine {machine} --batch {batch} --users {users} --programs {limit} \
                         --seed-base 3000000"
                    );
                    let programs = plan(&scratch, &flags);
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
    let programs = plan(&scratch, "--machine 2 --batch 9 --users 30");
    let ids: Vec<u64> = programs.iter().map(|p| p.client_id).collect();
    assert_eq!(ids, (29_000..29_010).collect::<Vec<_>>());
    let last = plan(
        &scratch,
        "--machine 9 --batch 9 --users 1000 --programs 1000",
    );
    assert_eq!(last.last().unwrap().client_id, 99_999);
}

#[test]
fn dry_run_gives_every_user_its_own_slot() {
    let scratch = Scratch::new("users-slots");
    let (users, think_ms) = (1000u64, 1000u64);
    for limit in [10u64, 100, 1000] {
        let flags = format!("--machine 1 --users {users} --programs {limit}");
        // Inside loadgen, user j of a program with U users first sends j*T/U after its start.
        let mut slots = BTreeSet::new();
        for p in plan(&scratch, &flags) {
            for j in 0..p.users {
                assert!(slots.insert(p.start_ms + j * think_ms / p.users));
            }
        }
        assert_eq!(slots.len() as u64, users, "at {limit} programs");
        assert_eq!(slots.iter().next_back(), Some(&(think_ms - 1)));
    }
}

#[test]
fn all_at_once_has_no_start_spread_and_steady_and_random_do() {
    let scratch = Scratch::new("users-spread");
    let starts = |pattern: &str| -> Vec<u64> {
        plan(
            &scratch,
            &format!("--machine 1 --users 100 --pattern {pattern}"),
        )
        .iter()
        .map(|p| p.start_ms)
        .collect()
    };
    assert_eq!(starts("all-at-once"), vec![0; 10]);
    let spread: Vec<u64> = (0..10).map(|k| k * 10).collect();
    assert_eq!(starts("steady"), spread);
    assert_eq!(starts("random"), spread);
}

#[test]
fn dry_run_refuses_what_would_collide_or_mean_nothing() {
    let scratch = Scratch::new("users-refuse");
    for flags in [
        "--users 10",                                                // no machine
        "--machine 1",                                               // no users
        "--machine 0 --users 10",                                    // would land on hand-run IDs
        "--machine 10 --users 10",                                   // a sixth digit
        "--machine 1 --users 0",                                     // nobody
        "--machine 1 --users ten",                                   // not a number
        "--machine 1 --users 10 --batch 10",                         // would collide with machine 2
        "--machine 1 --users 10 --programs 1001",                    // would collide with batch 1
        "--machine 1 --users 10 --programs 0",                       // nobody to run them
        "--machine 1 --users 10 --pattern spike",                    // no such pattern
        "--machine 1 --users 10 --pattern random --think-ms 0",      // nothing to randomise
        "--machine 1 --users 10 --pattern all-at-once --think-ms 0", // no beat to send on
        "--machine 1 --users 10 --duration-s 0",                     // no run
        "--machine 1 --users 10 --duration-s 5 --warmup-s 5",        // nothing left to measure
        "--machine 1 --users 10 --no-such-flag 1",                   // a typing mistake
        "--machine 1 --users",                                       // a flag with no value
    ] {
        let output = dry_run(&scratch, flags);
        assert_eq!(output.status.code(), Some(2), "{flags} was not refused");
        assert!(!stderr(&output).is_empty(), "{flags} gave no reason");
    }
}

#[test]
fn the_runner_shares_a_cells_users_exactly() {
    let shares = |users: u64| -> Vec<u64> {
        let script = format!(
            "source scripts/lab-common.sh; read_hosts experiments/hosts.example; \
             for i in 0 1 2; do users_on_machine $i {users}; done"
        );
        let output = Command::new("bash")
            .args(["-c", &script])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        stdout(&output)
            .lines()
            .map(|line| line.parse().unwrap())
            .collect()
    };
    assert_eq!(shares(100), [34, 33, 33]);
    assert_eq!(shares(2), [1, 1, 0]);
    for users in [1, 30, 300, 1000, 1500, 3000] {
        assert_eq!(shares(users).iter().sum::<u64>(), users);
    }
}

#[test]
fn the_hosts_file_refuses_numbers_that_would_share_client_ids() {
    let scratch = Scratch::new("users-hosts");
    let read = |clients: &str| {
        let hosts = scratch.0.join("hosts");
        std::fs::write(&hosts, format!("node 1 10.0.0.11 a\n{clients}")).unwrap();
        let script = format!(
            "source scripts/lab-common.sh; read_hosts {}",
            hosts.display()
        );
        Command::new("bash")
            .args(["-c", &script])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .unwrap()
    };
    assert!(
        read("client 1 10.0.0.21 b\nclient 2 10.0.0.22 c\n")
            .status
            .success()
    );
    let same = read("client 1 10.0.0.21 b\nclient 1 10.0.0.22 c\n");
    assert!(!same.status.success());
    assert!(stderr(&same).contains("same number"), "{}", stderr(&same));
    for number in ["0", "10", "x"] {
        let bad = read(&format!("client {number} 10.0.0.21 b\n"));
        assert!(!bad.status.success(), "client number {number}");
        assert!(stderr(&bad).contains("1 to 9"), "{}", stderr(&bad));
    }
}

#[test]
fn a_real_start_gives_one_log_per_program() {
    let scratch = Scratch::new("users-real");
    scratch.write_closed_config();
    let output = scratch
        .start("--machine 1 --users 5 --programs 5 --think-ms 100 --duration-s 1")
        .output()
        .unwrap();
    let printed = stdout(&output);
    assert!(output.status.success(), "{printed}{}", stderr(&output));
    assert!(
        printed.starts_with("plan machine=1 batch=0 users=5 programs=5"),
        "{printed}"
    );
    assert!(printed.contains("started 5, failed 0"), "{printed}");
    assert!(printed.contains("reported 5 of 5 programs"), "{printed}");

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
    // Each program says when its clock started and how many users it has: what the analysis counts on.
    let started = scratch.events("client_started");
    assert_eq!(started.len(), 5);
    assert!(started.iter().all(|e| e["users"] == 1));
    // Only program 0 of batch 0 samples the machine, and that sample is real on Linux.
    let samples = scratch.events("machine_usage");
    assert!(!samples.is_empty());
    assert!(samples.iter().all(|e| e["src"] == "client-10000"));
    assert!(samples[0]["cpu_total_ticks"].as_u64().unwrap() > 0);
    assert!(samples[0]["mem_available_kb"].as_u64().unwrap() > 0);
    // The PID file goes when the script ends, and every program's output is in one file.
    assert!(!scratch.pid_file(1, 0).exists());
    let out = scratch.output_file_text(1, 0);
    let summaries = out.lines().filter(|l| l.starts_with("client 1")).count();
    assert_eq!(summaries, 5, "{out}");
}

#[test]
fn term_leaves_no_child_no_pid_file_and_a_summary_from_each_program() {
    let scratch = Scratch::new("users-term");
    scratch.write_closed_config();
    let child = scratch.spawn("--machine 1 --users 9 --programs 3 --think-ms 100 --duration-s 120");
    scratch.wait_for_programs(3);
    let recorded = std::fs::read_to_string(scratch.pid_file(1, 0)).unwrap();
    assert_eq!(recorded.trim(), child.id().to_string());

    signal("TERM", child.id());
    let output = finish(child);
    let printed = stdout(&output);
    assert!(output.status.success(), "{printed}{}", stderr(&output));
    assert!(printed.contains("stopped early"), "{printed}");
    assert!(printed.contains("started 3, failed 0"), "{printed}");
    assert!(printed.contains("reported 3 of 3 programs"), "{printed}");
    assert!(!scratch.pid_file(1, 0).exists());
    assert!(!scratch.runs(""));
    let out = scratch.output_file_text(1, 0);
    assert_eq!(out.matches("stopped by a signal").count(), 3, "{out}");
    // Flushed: the logs hold what happened up to the signal.
    assert!(scratch.events("client_sent").len() >= 3);
}

#[test]
fn a_second_start_is_refused_while_the_first_lives_and_stop_ends_it() {
    let scratch = Scratch::new("users-second");
    scratch.write_closed_config();
    let flags = "--machine 2 --users 2 --think-ms 100 --duration-s 120";
    let first = scratch.spawn(flags);
    scratch.wait_for_programs(2);

    let second = scratch.start(flags).output().unwrap();
    assert_eq!(second.status.code(), Some(2));
    assert!(stderr(&second).contains("already running"));

    // Another batch on the same PC is allowed: that is what batches are for.
    let other = scratch
        .start("--machine 2 --batch 1 --users 1 --think-ms 100 --duration-s 1")
        .output()
        .unwrap();
    assert!(other.status.success(), "{}", stderr(&other));

    let stop = scratch.script("--stop --machine 2").output().unwrap();
    assert!(stop.status.success(), "{}", stderr(&stop));
    let first = finish(first);
    assert!(first.status.success());
    assert!(stdout(&first).contains("reported 2 of 2 programs"));
    assert!(!scratch.runs(""));

    // Nothing left to stop is no error.
    let again = scratch.script("--stop --machine 2").output().unwrap();
    assert!(again.status.success());
    assert!(stdout(&again).contains("nothing running"));
}

#[test]
fn a_killed_script_leaves_programs_that_a_start_refuses_and_stop_ends() {
    let scratch = Scratch::new("users-killed");
    scratch.write_closed_config();
    let flags = "--machine 8 --users 2 --think-ms 100 --duration-s 120";
    let first = scratch.spawn(flags);
    scratch.wait_for_programs(2);
    signal("KILL", first.id());
    let first = finish(first);
    assert!(first.status.code().is_none(), "the script should have died");
    assert!(scratch.runs(" --client-id 80000"), "its programs live on");

    // A second program under the same client ID would share its log and its request IDs.
    let second = scratch.start(flags).output().unwrap();
    assert_eq!(second.status.code(), Some(2), "{}", stdout(&second));
    assert!(
        stderr(&second).contains("a killed start left"),
        "{}",
        stderr(&second)
    );

    let stop = scratch.script("--stop --machine 8").output().unwrap();
    assert!(stop.status.success(), "{}", stderr(&stop));
    assert!(stdout(&stop).contains("stopped"), "{}", stdout(&stop));
    assert!(!scratch.runs(""), "nothing may be left");
    // They stopped as programs do: printed, and flushed.
    let out = scratch.output_file_text(8, 0);
    assert_eq!(out.matches("stopped by a signal").count(), 2, "{out}");

    let again = scratch
        .start("--machine 8 --users 1 --think-ms 100 --duration-s 1")
        .output()
        .unwrap();
    assert!(again.status.success(), "{}", stderr(&again));
}

#[test]
fn a_closing_terminal_still_gets_every_program_stopped_and_flushed() {
    let scratch = Scratch::new("users-hup");
    scratch.write_closed_config();
    // Its own process group, with its programs in it, as a terminal's foreground group would be.
    let child = scratch
        .start("--machine 9 --users 3 --programs 3 --think-ms 100 --duration-s 120")
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    scratch.wait_for_programs(3);

    // A hangup reaches the whole group at once: the script and every program.
    let status = Command::new("kill")
        .args(["-HUP", "--", &format!("-{}", child.id())])
        .status()
        .unwrap();
    assert!(status.success());
    let output = finish(child);
    let printed = stdout(&output);
    assert!(output.status.success(), "{printed}{}", stderr(&output));
    assert!(printed.contains("started 3, failed 0"), "{printed}");
    assert!(printed.contains("reported 3 of 3 programs"), "{printed}");
    assert!(!scratch.runs(""));
}

#[test]
fn a_program_whose_reader_has_gone_still_flushes_and_exits_well() {
    let scratch = Scratch::new("users-no-reader");
    scratch.write_closed_config();
    let mut child = Command::new(env!("CARGO_BIN_EXE_loadgen"))
        .arg("--config")
        .arg(scratch.config())
        .args([
            "--client-id",
            "4",
            "--users",
            "1",
            "--think-ms",
            "100",
            "--duration-s",
            "120",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    scratch.wait_for_programs(1);
    signal("TERM", child.id());
    poll_until(Duration::from_secs(10), "the program to end", || {
        child.try_wait().unwrap().is_some()
    });
    assert_eq!(child.wait().unwrap().code(), Some(0));
    assert!(!scratch.events("client_sent").is_empty());
}

#[test]
fn a_stale_pid_file_does_not_block_a_start() {
    let scratch = Scratch::new("users-stale");
    scratch.write_closed_config();
    // A PID that is alive but is not this script, as after a reboot.
    std::fs::write(scratch.pid_file(3, 0), format!("{}\n", std::process::id())).unwrap();
    let output = scratch
        .start("--machine 3 --users 1 --think-ms 100 --duration-s 1")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn a_dead_program_counts_as_failed_and_the_rest_keep_going() {
    let scratch = Scratch::new("users-dead");
    scratch.write_closed_config();
    let child = scratch.spawn("--machine 4 --users 10 --think-ms 100 --duration-s 120");
    scratch.wait_for_programs(10);
    let program = " --client-id 40003";
    let found = Command::new("pgrep")
        .arg("-f")
        .arg(format!("{}{program}", scratch.config().display()))
        .output()
        .unwrap();
    let pid: u32 = stdout(&found).trim().parse().unwrap();
    signal("KILL", pid);
    poll_until(Duration::from_secs(10), "program 3 to go", || {
        !scratch.runs(program)
    });
    assert!(scratch.runs(" --client-id 40004"), "the others keep going");

    signal("TERM", child.id());
    let output = finish(child);
    let printed = stdout(&output);
    // One of ten is exactly 10 percent, which does not stop a series: exit 1, not 2.
    assert_eq!(output.status.code(), Some(1), "{printed}");
    assert!(
        printed.contains("client 40003 exited with status 137"),
        "{printed}"
    );
    assert!(printed.contains("started 10, failed 1"), "{printed}");
    assert!(printed.contains("reported 9 of 10 programs"), "{printed}");
}

#[test]
fn more_than_a_tenth_failing_exits_with_two() {
    let scratch = Scratch::new("users-fail");
    // Starts fine as far as the script can tell, but every program refuses this config.
    std::fs::write(scratch.config(), "no_such_key = 1\n").unwrap();
    let output = scratch
        .start("--machine 5 --users 4 --think-ms 100 --duration-s 1")
        .output()
        .unwrap();
    let printed = stdout(&output);
    assert_eq!(output.status.code(), Some(2), "{printed}");
    assert!(printed.contains("started 4, failed 4"), "{printed}");
    assert!(printed.contains("reported 0 of 4 programs"), "{printed}");
    assert!(!scratch.pid_file(5, 0).exists());
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
        let flags = format!(
            "--machine {machine} --users 8 --programs 2 --think-ms 500 --pattern {pattern} \
             --duration-s 2"
        );
        // Run as a child of this runtime, which also runs the cluster.
        let output = tokio::process::Command::from(scratch.start(&flags))
            .output()
            .await
            .unwrap();
        let printed = stdout(&output);
        assert!(output.status.success(), "{pattern}: {printed}");
        assert!(
            printed.contains("started 2, failed 0"),
            "{pattern}: {printed}"
        );
        assert!(
            printed.contains("reported 2 of 2 programs"),
            "{pattern}: {printed}"
        );
        assert!(printed.contains("gave up 0"), "{pattern}: {printed}");
        assert!(
            !scratch.events("client_done").is_empty(),
            "{pattern}: {printed}"
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

#[test]
fn steady_spreads_the_first_sends_over_the_think_time() {
    let scratch = Scratch::new("users-steady-times");
    scratch.write_closed_config();
    let output = scratch
        .start("--machine 6 --users 20 --programs 2 --think-ms 1000 --duration-s 2")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stdout(&output));
    let times = first_sends_us(&scratch);
    assert!(times.len() >= 20, "{} first sends", times.len());
    // The slots run from 0 to 950 ms. Sleeps only ever run long, so a busy machine cannot make this
    // shorter by much, while a first wave sent together would take a few milliseconds.
    let span_ms = (times[19] - times[0]) / 1000;
    assert!(span_ms >= 500, "the first 20 sends took only {span_ms} ms");
}

#[test]
fn all_at_once_users_send_on_a_multiple_of_the_think_time() {
    let scratch = Scratch::new("users-all-at-once-times");
    scratch.write_closed_config();
    let output = scratch
        .start("--machine 7 --users 6 --programs 2 --think-ms 2000 --pattern all-at-once --duration-s 5")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stdout(&output));
    let times = first_sends_us(&scratch);
    assert!(times.len() >= 6, "{} first sends", times.len());
    // A whole second of slack against the 2 s beat, so a busy machine does not fail this.
    for ts in times {
        let past_the_beat_ms = (ts % 2_000_000) / 1000;
        assert!(
            past_the_beat_ms < 1000,
            "a send {past_the_beat_ms} ms after the beat"
        );
    }
    // Together across the programs, not only each near its beat: on a full beat, all six users of both
    // programs send within a moment, where users spread over the beat would take most of its 2 s.
    let mut by_beat = BTreeMap::<u64, Vec<(u64, String)>>::new();
    for event in scratch.events("client_sent") {
        if event["attempt"] == 0 {
            let ts = event["ts_us"].as_u64().unwrap();
            let program = event["src"].to_string();
            by_beat
                .entry(ts / 2_000_000)
                .or_default()
                .push((ts, program));
        }
    }
    let fullest = by_beat.values().max_by_key(|sends| sends.len()).unwrap();
    assert_eq!(fullest.len(), 6, "{fullest:?}");
    let programs: BTreeSet<_> = fullest.iter().map(|(_, program)| program).collect();
    assert_eq!(programs.len(), 2, "{fullest:?}");
    let first = fullest.iter().map(|(ts, _)| ts).min().unwrap();
    let last = fullest.iter().map(|(ts, _)| ts).max().unwrap();
    let span_ms = (last - first) / 1000;
    assert!(span_ms < 500, "the users of one beat took {span_ms} ms");
}
