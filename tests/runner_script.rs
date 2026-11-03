//! The runner's guards, run for real with bash: the leftover check and the retried silence (D139).
//! A pretend lab then runs the whole runner and the real programs over a stand-in for ssh, because
//! `hosts.local` cannot show a line that only goes wrong on a far machine.
//! Linux only, like the scripts themselves.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

use std::fs;
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(10);

/// A path no other process on this machine uses, for one test's pretend build output.
fn unique_bin_dir() -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "rescuemesh-runner-{}-{n}/target/release",
        std::process::id()
    ))
}

/// `script` in bash, as the runner runs: `set -euo pipefail`, lab-common.sh loaded, every machine
/// local.
fn lab_bash(bin: &Path, script: &str) -> Output {
    let common = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/lab-common.sh");
    let script = format!(
        "set -euo pipefail\nsource '{}'\nNODE_SSH=(local)\nCLIENT_SSH=(local)\n{script}",
        common.display()
    );
    Command::new("bash")
        .args(["-c", &script])
        .env("BIN", bin)
        .output()
        .unwrap()
}

/// A process whose command line starts with `program`, as if it ran from that path. No such file is
/// needed: the guard reads command lines. Killed when dropped.
struct Pretend(Child);

impl Pretend {
    fn start(program: &Path, seconds: u32) -> Self {
        let child = Command::new("bash")
            .args([
                "-c",
                &format!("exec -a '{}' sleep {seconds}", program.display()),
            ])
            .spawn()
            .unwrap();
        let pretend = Self(child);
        pretend.wait_until_visible(program);
        pretend
    }

    /// Until bash has replaced itself with the renamed `sleep`.
    fn wait_until_visible(&self, program: &Path) {
        let deadline = Instant::now() + WAIT;
        loop {
            let cmdline = std::fs::read(format!("/proc/{}/cmdline", self.0.id())).unwrap();
            if cmdline.starts_with(program.as_os_str().as_encoded_bytes()) {
                return;
            }
            assert!(Instant::now() < deadline, "the pretend program never ran");
            std::thread::yield_now();
        }
    }
}

impl Drop for Pretend {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_leftover_stops_the_run_until_it_is_gone() {
    let bin = unique_bin_dir();
    for program in ["frontend", "zonefeed", "loadgen", "node"] {
        let leftover = Pretend::start(&bin.join(program), 60);
        let refused = lab_bash(&bin, r#"refuse_leftovers "$ALL_PROGRAMS""#);
        assert!(!refused.status.success(), "{program} was not noticed");
        assert!(
            stderr(&refused).contains(&format!("{program} 60")),
            "{refused:?}"
        );

        drop(leftover);
        let clean = lab_bash(&bin, r#"refuse_leftovers "$ALL_PROGRAMS""#);
        assert!(clean.status.success(), "{clean:?}");
    }
}

#[test]
fn nothing_running_passes_so_the_check_does_not_find_its_own_shell() {
    let clean = lab_bash(&unique_bin_dir(), r#"refuse_leftovers "$ALL_PROGRAMS""#);
    assert!(clean.status.success(), "{clean:?}");
}

#[test]
fn a_program_with_the_same_name_elsewhere_is_not_a_leftover() {
    let bin = unique_bin_dir();
    let _nodejs = Pretend::start(&bin.parent().unwrap().join("nodejs/bin/node"), 60);
    let clean = lab_bash(&bin, r#"refuse_leftovers "$ALL_PROGRAMS""#);
    assert!(clean.status.success(), "{clean:?}");
}

#[test]
fn a_name_that_only_starts_the_same_is_not_a_leftover() {
    let bin = unique_bin_dir();
    let _other = Pretend::start(&bin.join("node-exporter"), 60);
    let clean = lab_bash(&bin, r#"refuse_leftovers "$ALL_PROGRAMS""#);
    assert!(clean.status.success(), "{clean:?}");
}

#[test]
fn the_check_after_the_load_leaves_the_runs_own_nodes_alone() {
    let bin = unique_bin_dir();
    let _node = Pretend::start(&bin.join("node"), 60);
    let clean = lab_bash(&bin, r#"refuse_leftovers "$LOAD_PROGRAMS""#);
    assert!(clean.status.success(), "{clean:?}");
}

#[test]
fn a_program_stopping_within_the_grace_time_is_no_leftover() {
    let bin = unique_bin_dir();
    let _stopping = Pretend::start(&bin.join("node"), 1);
    let clean = lab_bash(&bin, r#"refuse_leftovers "$ALL_PROGRAMS" 30"#);
    assert!(clean.status.success(), "{clean:?}");
}

/// Over ssh the far shell stays alive while pgrep runs, with the whole pattern in its command line.
#[test]
fn the_check_works_the_same_over_ssh() {
    let bin = unique_bin_dir();
    // Stands in for ssh: runs the last argument in a shell, as the far machine would.
    let over_ssh = r#"ssh() { bash -c "${@: -1}"; }
REMOTE_DIR=.
NODE_SSH=(user@pretend)
CLIENT_SSH=(user@pretend)
refuse_leftovers "$ALL_PROGRAMS""#;
    let clean = lab_bash(&bin, over_ssh);
    assert!(clean.status.success(), "{clean:?}");

    let _leftover = Pretend::start(&bin.join("loadgen"), 60);
    let refused = lab_bash(&bin, over_ssh);
    assert!(!refused.status.success());
    assert!(stderr(&refused).contains("user@pretend"), "{refused:?}");
}

/// `cd` failing also exits 1 in `cd X && pgrep`, which would read as "nothing left over".
#[test]
fn a_machine_without_the_repo_folder_cannot_pass_for_a_clean_one() {
    let refused = lab_bash(
        &unique_bin_dir(),
        r#"ssh() { bash -c "${@: -1}"; }
REMOTE_DIR=no-such-folder
NODE_SSH=(user@pretend)
CLIENT_SSH=(user@pretend)
refuse_leftovers "$ALL_PROGRAMS""#,
    );
    assert!(!refused.status.success());
    assert!(stderr(&refused).contains("cannot check"), "{refused:?}");
}

#[test]
fn a_machine_that_cannot_be_checked_stops_the_run() {
    // What ssh exits with when it cannot connect. Reading that as clean would run on a bad machine.
    let refused = lab_bash(
        &unique_bin_dir(),
        r#"on_host() { return 255; }; refuse_leftovers "$ALL_PROGRAMS""#,
    );
    assert!(!refused.status.success());
    assert!(stderr(&refused).contains("cannot check"), "{refused:?}");
}

/// Runs `silence_with_retry` against a `ctl` made of `ctl_body`, which can read `$tries`: this call's
/// number, from 1. Returns the output, whose stdout is how many calls were made.
fn silence_against(ctl_body: &str) -> Output {
    let script = format!(
        r#"calls=$(mktemp)
trap 'wc -l <"$calls"; rm -f "$calls"' EXIT
ctl() {{ echo "$*" >>"$calls"; local tries; tries=$(wc -l <"$calls"); {ctl_body}; }}
silence_with_retry 2 5000"#
    );
    lab_bash(&unique_bin_dir(), &script)
}

fn calls_made(output: &Output) -> u32 {
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.trim().parse().unwrap()
}

#[test]
fn a_silence_that_works_at_once_is_sent_once() {
    let done = silence_against("true");
    assert!(done.status.success(), "{done:?}");
    assert_eq!(calls_made(&done), 1);
}

#[test]
fn a_silence_that_times_out_twice_is_tried_a_third_time() {
    let done = silence_against(r#"[ "$tries" -ge 3 ]"#);
    assert!(done.status.success(), "{done:?}");
    assert_eq!(calls_made(&done), 3);
}

#[test]
fn a_silence_that_never_gets_through_fails_after_three_tries() {
    let failed = silence_against("echo 'no answer within 2 s'; false");
    assert!(!failed.status.success());
    assert_eq!(calls_made(&failed), 3);
}

/// The first order worked but its answer was lost, so the node refuses the next one (D74).
#[test]
fn already_silenced_counts_as_done() {
    let done = silence_against(
        r#"if [ "$tries" -eq 1 ]; then false; else echo 'Error: node 2: already silenced; wait' >&2; false; fi"#,
    );
    assert!(done.status.success(), "{done:?}");
    assert_eq!(calls_made(&done), 2);
}

/// Stands in for ssh as a real session behaves: the far shell starts in the machine's home folder with
/// a bare environment, not in the repo, and the call does not return until everything holding the
/// session's output has closed it.
const FAKE_SSH: &str = r#"#!/usr/bin/env bash
while [[ "${1:-}" == -* ]]; do
  if [[ "$1" == -o ]]; then shift; fi
  shift
done
host=$1
shift
cd "$FAKE_LAB/$host" || exit 255
env -i HOME="$FAKE_LAB/$host" PATH="$PATH" bash -c "$*" </dev/null 2>&1 | cat
exit "${PIPESTATUS[0]}"
"#;

const FAKE_SCP: &str = r#"#!/usr/bin/env bash
[[ -z "${FAKE_SCP_FAILS:-}" ]] || exit 1
to=${*: -1}
cp "${*: -2:1}" "$FAKE_LAB/${to%%:*}/${to#*:}"
"#;

const FAKE_RSYNC: &str = r#"#!/usr/bin/env bash
from=${*: -2:1}
cp -r "$FAKE_LAB/${from%%:*}/${from#*:}." "${*: -1}"
"#;

const RUN_FOLDER: &str = "control/runs/e9/try/run-1";

/// Six machines as folders, nothing shared but loopback, each with the repo at ~/DistributedSystems.
struct Lab {
    root: PathBuf,
    /// Where the programs live on each machine. Unique, so tests cannot see each other's programs.
    bin: String,
}

impl Lab {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let (pid, n) = (std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst));
        let root = std::env::temp_dir().join(format!("rescuemesh-lab-{pid}-{n}"));
        let bin = format!("labbin{pid}x{n}");
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"));

        let control = root.join("control");
        fs::create_dir_all(control.join("experiments")).unwrap();
        copy_folder(&repo.join("scripts"), &control);
        fs::copy(
            repo.join("experiments/quick.list"),
            control.join("experiments/quick.list"),
        )
        .unwrap();
        let cell = fs::read_to_string(repo.join("experiments/e3e4-300-follower.toml")).unwrap();
        let (_, settings) = cell.split_once('\n').unwrap();
        fs::write(
            control.join("experiments/e9-try.toml"),
            format!("# run: users=3 think_ms=200 victim=follower silence_ms=1000\n{settings}"),
        )
        .unwrap();
        git(&control, &["init", "-q"]);
        git(
            &control,
            &[
                "-c",
                "user.email=lab@example.invalid",
                "-c",
                "user.name=lab",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "lab",
            ],
        );

        let stubs = root.join("stubs");
        fs::create_dir_all(&stubs).unwrap();
        for (name, body) in [("ssh", FAKE_SSH), ("scp", FAKE_SCP), ("rsync", FAKE_RSYNC)] {
            fs::write(stubs.join(name), body).unwrap();
            fs::set_permissions(stubs.join(name), fs::Permissions::from_mode(0o755)).unwrap();
        }

        let programs = [
            ("node", env!("CARGO_BIN_EXE_node")),
            ("ctl", env!("CARGO_BIN_EXE_ctl")),
            ("loadgen", env!("CARGO_BIN_EXE_loadgen")),
        ];
        for pc in ["pc1", "pc2", "pc3", "pc4", "pc5", "pc6"] {
            let repo_there = root.join(pc).join("DistributedSystems");
            fs::create_dir_all(repo_there.join(&bin)).unwrap();
            copy_folder(&repo.join("scripts"), &repo_there);
            for (name, path) in programs {
                std::os::unix::fs::symlink(path, repo_there.join(&bin).join(name)).unwrap();
            }
        }

        // Held until all are read, so no two ports come out the same.
        let held: Vec<_> = (0..6)
            .map(|_| TcpListener::bind("127.0.0.1:0").unwrap())
            .collect();
        let port = |i: usize| held[i].local_addr().unwrap().port();
        let hosts = format!(
            "node 1 127.0.0.1 pc1 {} {}\nnode 2 127.0.0.1 pc2 {} {}\nnode 3 127.0.0.1 pc3 {} {}\n\
             client 1 127.0.0.1 pc4\nclient 2 127.0.0.1 pc5\nclient 3 127.0.0.1 pc6\n",
            port(0),
            port(1),
            port(2),
            port(3),
            port(4),
            port(5),
        );
        fs::write(control.join("experiments/hosts"), hosts).unwrap();
        Self { root, bin }
    }

    /// The real runner, on the one cell this lab has, with a short profile.
    fn run_the_cell(&self, more_env: &[(&str, &str)]) -> Output {
        let path = format!(
            "{}:{}",
            self.root.join("stubs").display(),
            std::env::var("PATH").unwrap()
        );
        Command::new("timeout")
            .args(["120", "bash", "scripts/run-experiments.sh"])
            .args(["experiments/hosts", "quick", "e9-try"])
            .current_dir(self.root.join("control"))
            .env("PATH", path)
            .env("FAKE_LAB", &self.root)
            .env("BIN", &self.bin)
            .envs([
                ("WARMUP", "1"),
                ("MEASURED", "3"),
                ("GAP", "0"),
                ("REPS", "1"),
            ])
            .envs(more_env.iter().copied())
            .output()
            .unwrap()
    }

    fn in_run_folder(&self, file: &str) -> PathBuf {
        self.root.join(RUN_FOLDER).join(file)
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        // A failed test may leave programs behind that still write here.
        let _ = lab_bash(
            Path::new(&self.bin),
            r#"pkill -f "[${BIN:0:1}]${BIN:1}/" || true"#,
        );
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn copy_folder(from: &Path, into: &Path) {
    let copied = Command::new("cp").arg("-r").arg(from).arg(into).status();
    assert!(copied.unwrap().success());
}

fn git(folder: &Path, args: &[&str]) {
    let done = Command::new("git")
        .args(args)
        .current_dir(folder)
        .status()
        .unwrap();
    assert!(done.success(), "git {args:?}");
}

/// The line that starts a node ends in `&`, and that hides from any test on this machine what it does
/// on a far one: with `cd X && ...` the pid file went to the home folder and the session never ended,
/// so the batch hung on its first node.
#[test]
fn a_whole_cell_runs_over_ssh_and_stops_everything_it_started() {
    let lab = Lab::new();

    let run = lab.run_the_cell(&[]);

    assert!(run.status.success(), "{run:?}");
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("silenced node"),
        "{run:?}"
    );
    for file in [
        "run.json",
        "status-after.txt",
        "node-1.jsonl",
        "node-2.jsonl",
        "node-3.jsonl",
    ] {
        assert!(lab.in_run_folder(file).exists(), "no {file}: {run:?}");
    }
    let silenced = (1..=3)
        .filter(|n| {
            fs::read_to_string(lab.in_run_folder(&format!("node-{n}.jsonl")))
                .unwrap()
                .contains(r#""ev":"silenced""#)
        })
        .count();
    assert_eq!(silenced, 1, "one node gets the fault: {run:?}");
    let clients = fs::read_dir(lab.root.join(RUN_FOLDER))
        .unwrap()
        .filter(|entry| {
            let name = entry.as_ref().unwrap().file_name();
            let name = name.to_string_lossy();
            name.starts_with("client-") && name.ends_with(".jsonl")
        })
        .count();
    assert!(clients >= 3, "logs of {clients} load programs: {run:?}");

    // Nothing went to the home folder, and every program it started has stopped.
    let home: Vec<_> = fs::read_dir(lab.root.join("pc1"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(home, ["DistributedSystems"]);
    let stopped = lab_bash(
        Path::new(&lab.bin),
        r#"refuse_leftovers "$ALL_PROGRAMS" 10"#,
    );
    assert!(stopped.status.success(), "{stopped:?}");
}
