//! The runner's guards, run for real with bash: the leftover check and the retried silence (D139).
//! Linux only, like the scripts themselves.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

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
