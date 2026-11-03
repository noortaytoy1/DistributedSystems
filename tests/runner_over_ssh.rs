//! `scripts/run-experiments.sh` as it runs on the six lab machines: over ssh, with no shared filesystem
//! (D130, D183, D186). `on_host` with the target `local` skips everything that goes wrong only over ssh.
//!
//! A stand-in `ssh`, `scp` and `rsync` come first on the PATH. Each machine is its own directory holding
//! the scripts and binaries, and the control machine has neither binaries nor logs of its own. As a real
//! session does, the stand-in starts the remote shell in the machine's home directory with a bare
//! environment, and does not return until everything holding the session's output has let go of it. The
//! shell is `sh`, which is dash on Ubuntu, so what the runner sends must not lean on bash.
//!
//! This is not a real sshd or a real network: the first run on the lab machines is still the real test.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

use std::fs::{self, File};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const REPO: &str = env!("CARGO_MANIFEST_DIR");

const SSH: &str = r#"#!/usr/bin/env bash
while [[ "${1:-}" == -* ]]; do
  case "$1" in
    -o) shift 2 ;;
    *) shift ;;
  esac
done
host=$1
shift
home=$FAKE_LAB/$host
[[ -d "$home" ]] || { echo "ssh: connect to host $host: no route" >&2; exit 255; }
cd "$home" || exit 255
env -i HOME="$home" PATH="$REAL_PATH" sh -c "$*" </dev/null 2>&1 | cat
exit "${PIPESTATUS[0]}"
"#;

const SCP: &str = r#"#!/usr/bin/env bash
from=${*: -2:1}
to=${*: -1}
exec cp "$from" "$FAKE_LAB/${to%%:*}/${to#*:}"
"#;

const RSYNC: &str = r#"#!/usr/bin/env bash
from=${*: -2:1}
to=${*: -1}
[[ -d "$FAKE_LAB/${from%%:*}" ]] || { echo "rsync: cannot reach ${from%%:*}" >&2; exit 255; }
exec cp -r "$FAKE_LAB/${from%%:*}/${from#*:}." "$to"
"#;

const MACHINES: [&str; 6] = ["pc1", "pc2", "pc3", "pc4", "pc5", "pc6"];

/// Six machines and a control machine, under one temporary directory.
struct Lab {
    root: PathBuf,
    /// The binaries' folder on each machine. Unique, so that `pgrep` sees only this lab's programs.
    bin: String,
}

fn run(command: &mut Command) {
    let status = command.status().unwrap();
    assert!(status.success(), "{command:?}: {status}");
}

/// Free ports on loopback, all different: each is held until all are chosen.
fn free_ports(n: usize) -> Vec<u16> {
    let held: Vec<_> = (0..n)
        .map(|_| std::net::TcpListener::bind("127.0.0.1:0").unwrap())
        .collect();
    held.iter()
        .map(|l| l.local_addr().unwrap().port())
        .collect()
}

impl Lab {
    fn new(name: &str) -> Self {
        let id = format!("{name}-{}", std::process::id());
        let root = std::env::temp_dir().join(format!("rescuemesh-lab-{id}"));
        let _ = fs::remove_dir_all(&root);
        let lab = Self {
            root,
            bin: format!("labbin-{id}"),
        };

        let stubs = lab.root.join("stubs");
        fs::create_dir_all(&stubs).unwrap();
        for (name, text) in [("ssh", SSH), ("scp", SCP), ("rsync", RSYNC)] {
            let path = stubs.join(name);
            fs::write(&path, text).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }

        for machine in MACHINES {
            let repo = lab.machine(machine);
            fs::create_dir_all(repo.join(&lab.bin)).unwrap();
            run(Command::new("cp")
                .args(["-r", &format!("{REPO}/scripts")])
                .arg(&repo));
            for (program, built) in [
                ("node", env!("CARGO_BIN_EXE_node")),
                ("ctl", env!("CARGO_BIN_EXE_ctl")),
                ("loadgen", env!("CARGO_BIN_EXE_loadgen")),
            ] {
                symlink(built, repo.join(&lab.bin).join(program)).unwrap();
            }
        }

        // Scripts, cells and a commit to name; no binaries.
        let control = lab.control();
        fs::create_dir_all(&control).unwrap();
        run(Command::new("cp")
            .args([
                "-r",
                &format!("{REPO}/scripts"),
                &format!("{REPO}/experiments"),
            ])
            .arg(&control));
        let git = |args: &[&str]| {
            run(Command::new("git")
                .args([
                    "-c",
                    "user.name=lab",
                    "-c",
                    "user.email=lab@example.invalid",
                ])
                .args(args)
                .current_dir(&control)
                .stdout(Stdio::null()));
        };
        git(&["init", "-q"]);
        git(&["add", "-A"]);
        git(&["commit", "-qm", "lab"]);

        let ports = free_ports(6);
        let hosts = format!(
            "node 1 127.0.0.1 pc1 {} {}\nnode 2 127.0.0.1 pc2 {} {}\nnode 3 127.0.0.1 pc3 {} {}\n\
             client 1 127.0.0.1 pc4\nclient 2 127.0.0.1 pc5\nclient 3 127.0.0.1 pc6\n",
            ports[0], ports[1], ports[2], ports[3], ports[4], ports[5]
        );
        fs::write(control.join("experiments/hosts"), hosts).unwrap();
        lab
    }

    fn control(&self) -> PathBuf {
        self.root.join("control")
    }

    /// The repo directory on a machine.
    fn machine(&self, name: &str) -> PathBuf {
        self.root.join(name).join("DistributedSystems")
    }

    /// The runner for these cells, with its output going to files: a stand-in ssh left running would
    /// hold a pipe open for ever, and this test must fail, not hang.
    fn runner(&self, cells: &[&str], measured_s: u32, stop_wait_s: u32) -> Child {
        let path = format!(
            "{}:{}",
            self.root.join("stubs").display(),
            std::env::var("PATH").unwrap()
        );
        Command::new("bash")
            .arg("scripts/run-experiments.sh")
            .args(["experiments/hosts", "quick"])
            .args(cells)
            .current_dir(self.control())
            .env("PATH", path)
            .env("REAL_PATH", std::env::var("PATH").unwrap())
            .env("FAKE_LAB", &self.root)
            .env("BIN", &self.bin)
            .env("WARMUP", "1")
            .env("MEASURED", measured_s.to_string())
            .env("GAP", "0")
            .env("REPS", "1")
            .env("STOP_WAIT_S", stop_wait_s.to_string())
            .env_remove("PROGRAMS_PER_PC")
            .env_remove("REMOTE_DIR")
            .stdin(Stdio::null())
            .stdout(File::create(self.root.join("runner.out")).unwrap())
            .stderr(File::create(self.root.join("runner.err")).unwrap())
            .spawn()
            .unwrap()
    }

    fn runner_output(&self) -> String {
        let read = |name| fs::read_to_string(self.root.join(name)).unwrap_or_default();
        format!("{}\n--- stderr\n{}", read("runner.out"), read("runner.err"))
    }

    /// Wait for the runner, with a deadline, and say how it ended.
    fn finish(&self, mut runner: Child) -> Option<i32> {
        poll_until(Duration::from_secs(90), "the runner to end", || {
            runner.try_wait().unwrap().is_some()
        });
        runner.wait().unwrap().code()
    }

    /// What this lab still has running, on any of its machines.
    fn running(&self) -> String {
        let output = Command::new("pgrep")
            .args(["-af", &format!("{}/", self.bin)])
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = Command::new("pkill")
            .args(["-KILL", "-f", &format!("{}/", self.bin)])
            .status();
        if !std::thread::panicking() {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

fn poll_until(timeout: Duration, what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while !cond() {
        assert!(
            Instant::now() < deadline,
            "timed out after {timeout:?} waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// File names in a directory, sorted.
fn names(dir: &Path) -> Vec<String> {
    let mut found: Vec<String> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    found.sort();
    found
}

#[test]
fn a_whole_run_works_on_six_machines_with_nothing_shared() {
    let lab = Lab::new("whole");
    // A PID file left by a crash, whose number now belongs to something else on that machine.
    let mut unrelated = Command::new("sleep").arg("120").spawn().unwrap();
    let config_dir = lab.machine("pc4").join("runs-config");
    fs::create_dir_all(&config_dir).unwrap();
    let stale = format!("{}\n", unrelated.id());
    fs::write(config_dir.join("users-4-4.pid"), stale).unwrap();

    let runner = lab.runner(&["e1-30-bidding"], 2, 10);
    let code = lab.finish(runner);
    let output = lab.runner_output();
    assert_eq!(code, Some(0), "{output}");
    assert!(!output.contains("warning"), "{output}");
    let survived = unrelated.try_wait().unwrap().is_none();
    unrelated.kill().unwrap();
    unrelated.wait().unwrap();
    assert!(
        survived,
        "stop_all killed a process through a stale PID file"
    );

    let out = lab.control().join("runs/e1/30-bidding/run-1");
    let collected = names(&out);
    let client_logs = |prefix: &str| {
        collected
            .iter()
            .filter(|n| n.starts_with(prefix) && n.ends_with(".jsonl"))
            .count()
    };
    assert_eq!(client_logs("client-1"), 10, "{collected:?}");
    assert_eq!(client_logs("client-2"), 10, "{collected:?}");
    assert_eq!(client_logs("client-3"), 10, "{collected:?}");
    for machine in 1..=3 {
        let totals = fs::read_to_string(out.join(format!("users-{machine}-0.out"))).unwrap();
        assert!(totals.contains("started 10, failed 0"), "{totals}");
        assert!(totals.contains("reported 10 of 10 programs"), "{totals}");
    }
    let status = fs::read_to_string(out.join("status-after.txt")).unwrap();
    assert!(status.contains("leader"), "{status}");

    // Each log was copied only after its node had stopped writing it.
    for node in 1..=3 {
        let log = fs::read_to_string(out.join(format!("node-{node}.jsonl"))).unwrap();
        assert!(
            log.contains(r#""ev":"stopped""#),
            "node {node}'s log was copied too soon"
        );
    }

    // Each machine holds only its own logs, and the one config, and no PID file is left anywhere.
    let config = fs::read_to_string(out.join("config.toml")).unwrap();
    for (machine, mine) in [
        ("pc1", "node-1"),
        ("pc2", "node-2"),
        ("pc3", "node-3"),
        ("pc4", "client-1"),
        ("pc5", "client-2"),
        ("pc6", "client-3"),
    ] {
        let repo = lab.machine(machine);
        let logs = names(&repo.join("runs/current"));
        let jsonl: Vec<_> = logs.iter().filter(|n| n.ends_with(".jsonl")).collect();
        assert!(!jsonl.is_empty(), "{machine}: {logs:?}");
        assert!(
            jsonl.iter().all(|n| n.starts_with(mine)),
            "{machine}: {logs:?}"
        );
        assert_eq!(names(&repo.join("runs-config")), ["run.toml"], "{machine}");
        let copy = fs::read_to_string(repo.join("runs-config/run.toml")).unwrap();
        assert_eq!(copy, config, "{machine} has a different config");
        // Nothing wrote in the home directory, which is where a lost `cd` would put it.
        assert_eq!(names(&lab.root.join(machine)), ["DistributedSystems"]);
    }
    let left = lab.running();
    assert!(left.is_empty(), "still running: {left}");
}

#[test]
fn a_fault_cell_reaches_a_node_through_ctl_on_a_client_machine() {
    let lab = Lab::new("fault");
    let cell = "e9-fault";
    // The real cell's run line, with few enough users that this test is about the path, not the load.
    let text =
        fs::read_to_string(lab.control().join("experiments/e3e4-300-follower.toml")).unwrap();
    let (_, rest) = text.split_once('\n').unwrap();
    let cell_text = format!("# run: users=6 think_ms=500 victim=follower silence_ms=500\n{rest}");
    fs::write(
        lab.control().join(format!("experiments/{cell}.toml")),
        cell_text,
    )
    .unwrap();

    let runner = lab.runner(&[cell], 3, 10);
    let code = lab.finish(runner);
    let output = lab.runner_output();
    assert_eq!(code, Some(0), "{output}");
    assert!(output.contains("silenced node"), "{output}");
    let out = lab.control().join("runs/e9/fault/run-1");
    let silenced = [1, 2, 3].iter().any(|node| {
        fs::read_to_string(out.join(format!("node-{node}.jsonl")))
            .unwrap()
            .contains(r#""ev":"silenced""#)
    });
    assert!(silenced, "no node logged the silence\n{output}");
    let left = lab.running();
    assert!(left.is_empty(), "still running: {left}");
}

#[test]
fn a_runner_refuses_what_it_did_not_start_and_leaves_it_alone() {
    let lab = Lab::new("refuse");
    let repo = lab.machine("pc4");
    fs::create_dir_all(repo.join("runs-config")).unwrap();
    fs::create_dir_all(repo.join("runs/current")).unwrap();
    let marker = repo.join("runs/current/from-the-demo");
    fs::write(&marker, "").unwrap();
    fs::write(
        repo.join("demo.toml"),
        "[[cluster.nodes]]\nid = 1\npeer_addr = \"127.0.0.1:9\"\nclient_addr = \"127.0.0.1:10\"\n",
    )
    .unwrap();
    // A demo in progress: a program from the same folder of binaries, with a PID file the runner can see.
    let mut demo = Command::new("bash")
        .args([
            "-c",
            &format!(
                "exec {}/loadgen --config demo.toml --client-id 4 --users 1 --think-ms 100 --duration-s 60",
                lab.bin
            ),
        ])
        .current_dir(&repo)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid_file = repo.join("runs-config/users-1-0.pid");
    fs::write(&pid_file, format!("{}\n", demo.id())).unwrap();
    poll_until(Duration::from_secs(10), "the demo program to start", || {
        lab.running().contains("loadgen")
    });
    assert!(demo.try_wait().unwrap().is_none(), "the demo did not start");

    let runner = lab.runner(&["e1-30-bidding"], 2, 1);
    let code = lab.finish(runner);
    let output = lab.runner_output();
    assert_eq!(code, Some(1), "{output}");
    assert!(output.contains("stop them first"), "{output}");
    assert!(output.contains("pc4: "), "{output}");

    assert!(demo.try_wait().unwrap().is_none(), "the runner killed it");
    assert!(pid_file.exists(), "the runner removed its PID file");
    assert!(marker.exists(), "the runner wiped its logs");
    demo.kill().unwrap();
    demo.wait().unwrap();
}

#[test]
fn a_machine_lost_in_a_run_stops_the_series_and_costs_only_its_own_logs() {
    let lab = Lab::new("lost");
    let runner = lab.runner(&["e1-30-bidding", "e1-30-leader"], 6, 2);
    let last_program = lab.machine("pc6").join("runs/current/client-30009.jsonl");
    poll_until(Duration::from_secs(45), "machine 3's programs", || {
        last_program.exists()
    });

    // The machine goes, and with it the session to it: new connections fail, and the runner's ssh ends.
    fs::rename(lab.root.join("pc6"), lab.root.join("pc6-lost")).unwrap();
    let pattern = format!("{}/stubs/ssh .*pc6 ", lab.root.display());
    let status = Command::new("pkill")
        .args(["-f", &pattern])
        .status()
        .unwrap();
    assert!(status.success(), "no session to pc6 found");

    let code = lab.finish(runner);
    let output = lab.runner_output();
    assert_eq!(code, Some(1), "{output}");
    assert!(output.contains("stopping the series"), "{output}");
    assert!(output.contains("cannot copy the logs from pc6"), "{output}");
    let collected = names(&lab.control().join("runs/e1/30-bidding/run-1"));
    for kept in ["node-1.jsonl", "users-1-0.out", "users-2-0.out"] {
        assert!(collected.iter().any(|n| n == kept), "{collected:?}");
    }
    assert!(!collected.iter().any(|n| n == "users-3-0.out"));
    assert!(
        !lab.control().join("runs/e1/30-leader").exists(),
        "the series went on"
    );
}
