# AGENTS.md

Working agreement for everyone who changes this repo, people and AI coding tools alike.
Read it before your first change. If something here is wrong or missing, fix it in a PR.

RescueMesh is a three-node cloud that plans incident responses for campus clients. Raft
(openraft) orders every decision. A load-based bidding election picks the node that plans each
request. Nodes survive faults of up to 20 s and catch up when they return. The system is
measured with JSON Lines event logs. [README.md](README.md) covers what it does and how to run it.

## Commands

```bash
cargo build                                   # debug build
cargo test                                    # unit + multi-node tests
cargo fmt --all                               # format (CI checks this)
cargo clippy --all-targets -- -D warnings     # lint (CI fails on any warning)
scripts/run-local.sh / scripts/stop-local.sh  # 3-node cluster on localhost
cargo run --bin ctl -- status                 # inspect a running cluster
```

On WSL, keep build output on the Linux filesystem; building onto `/mnt/c` or `/mnt/f` is slow:
`export CARGO_TARGET_DIR="$HOME/.cache/rescuemesh-target"`.

The code must build and pass tests on Linux, which covers the lab machines and WSL. Windows
builds are a convenience; never use Windows-only APIs or paths.

## Parts and owners

Each part has one owner who builds it, measures it, demos it and writes its report section.
Your review pair reviews your PRs and must be able to explain your part at the demo.

| Part | Mechanism                | Files                                                                | Measures              | Owner |
|------|--------------------------|----------------------------------------------------------------------|-----------------------|-------|
| A    | Raft and replication     | `raft/`, `state.rs`, `node/mod.rs`, `config.rs`                      | E5, commit latency    |       |
| B    | Bidding election         | `bidding.rs`                                                         | E1 policies, E6       |       |
| C    | Faults and failover      | `node/faults.rs`, `node/heartbeat.rs`, `node/transport.rs`, `bin/ctl.rs` | E3                |       |
| D    | Rejoin and recovery      | `node/rejoin.rs`                                                     | E4                    |       |
| E    | Planner and concurrency  | `planner.rs`, `campus.rs`, `node/workers.rs`                         | E2, CPU and queues    |       |
| F    | Client and scale         | `client.rs`, `bin/loadgen.rs`                                        | E1 scaling sweep      |       |
| all  | Shared                   | `proto.rs`, `metrics.rs`, `lib.rs`, `bin/node.rs`, `tests/`, `docs/` | —                     |       |

Review pairs: A↔D, B↔E, C↔F.

Files outside a finished part are templates: signatures, doc comments and placeholder bodies that
compile and never panic. One exception: `node/transport.rs` holds minimal working plumbing
(framing, peer request/reply, client port), written with Part A because Raft cannot run without
it. C owns it from here.

Open work is marked in the code as `TODO(<part>)`. To list one part's work:
`grep -rn "TODO(B)" src`.

## Coordination rules

1. **One owner per file.** Changing someone else's file needs that owner's review on the PR.
2. **`src/proto.rs` is the contract.** Change it only with an entry in
   [docs/design-log.md](docs/design-log.md) and a review from every owner whose code sends or
   receives the changed message. Update [docs/protocol.md](docs/protocol.md) in the same PR.
3. **Hook in, don't rewire.** `node/mod.rs` calls each part's entry point (`heartbeat::spawn`,
   `faults::spawn_schedule`, `rejoin::spawn`, ...). Do your work behind your entry point. If
   you need a new hook in `node/mod.rs`, ask A.
4. **Decisions go in the design log.** If a choice would surprise a teammate, such as a timer
   value, a message, a dependency or a trade-off, add a short entry: what, why, what was
   rejected.
5. **Everything goes through a pull request.** Nobody pushes to `main` directly, not even a
   one-line fix. See [Git workflow](#git-workflow).
6. **Library code is kept apart from ours.** The report must separate them. openraft is the only
   library doing distributed-systems work; anything new of that kind needs team agreement first.

## Git workflow

1. **Branch from an up-to-date `main`.** Name the branch `part-<letter>-<topic>` for your part's
   work (`part-a-heartbeat-log`, `part-b-bidding`), or `shared-<topic>` for shared files and
   docs.
   ```bash
   git switch main && git pull --ff-only
   git switch -c part-b-bid-wiring
   ```
2. **Commit in small steps.** The subject line is imperative and at most 72 characters ("Wire bid
   rounds into decide_executor"). The body says why, not what.
3. **Before you open the PR**, run `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`
   and `cargo test` on Linux or WSL. Then rebase onto the latest `main` so the PR is tested
   against what it will merge into:
   ```bash
   git fetch origin && git rebase origin/main
   git push --force-with-lease origin part-b-bid-wiring   # your own branch only
   ```
4. **Open the PR on GitHub** with:
   - **What and why:** a few sentences, plus the `TODO(<part>)` markers it resolves.
   - **How it was tested:** commands run, and anything you could not test.
   - **Contract changes:** any change to `proto.rs` or config keys, and the matching design-log
     entry.
5. **Reviews.** Your review pair reviews every PR. The owner of every file you touched outside
   your part reviews too. At least one approval is required.
6. **Merge** once CI is green and the PR is approved. Use **Rebase and merge** (or **Squash and
   merge** for messy histories) so `main` stays linear, then delete the branch.
7. **If someone rebased your branch** (for example to bring it up to date with `main`), update your
   copy before adding more work: `git fetch origin && git rebase origin/<branch>`.
8. **Never force-push `main`,** and only force-push your own branches. Use `--force-with-lease`,
   never `--force`.

The repo owner should turn on branch protection for `main` (require a PR, an approval and a green
CI run) so this is enforced, not just agreed.

## Architecture rules

These hold everywhere. A change that breaks one is a bug, even if the tests pass.

- **Deterministic core.** `state.rs`, `bidding.rs` and `planner.rs` never touch the network,
  the clock or randomness. Pass time and inputs in as arguments. In `state.rs`, never iterate a
  `HashMap`: use `BTreeMap`/`BTreeSet`. Every node must reach exactly the same state.
- **Sockets live in two places.** Only `node/` and `raft/network.rs` open sockets.
- **Never block the async runtime.** CPU-heavy work (planning) runs through
  `node/workers.rs`. No `std::thread::sleep`, no blocking file or network I/O in async code,
  and no `std::sync::Mutex` guard held across `.await` (clippy denies `await_holding_lock`).
- **Every peer message carries the sender's Raft term.** Receivers drop non-Raft messages
  from an older term and answer `StaleTerm`. Never drop Raft RPCs this way, because openraft
  must see them to reject a stale leader.
- **Commit, then reply.** A node replies to a client only after the result is committed and
  applied. The first committed result per request ID wins; later ones get the winner back.
- **Every `Command` is idempotent.** A proposal can be retried after a leader change, so
  applying a command twice must equal applying it once.
- **A returning node serves nothing until it is current.** It does not vote for itself, bid
  or reply until its applied index reaches the leader's commit index (`NodeContext::serving`).
- **The fault gate drops everything but control.** A silenced node neither sends nor receives
  peer or client traffic. `ctl` control frames still get through.
- **Integers in replicated data.** No floats in `Command` or `CloudState`. Use integer units
  and name them: `cost`, `latency_us`, `timeout_ms`.

## Rust practices

- **Errors.** Library code returns typed errors (`thiserror`). Binaries use `anyhow` with
  `.context(...)`. No `unwrap()` outside tests (clippy warns, CI fails). Use `expect("why this
  cannot fail")` only for real invariants.
- **No `unsafe`.** It is forbidden crate-wide.
- **Types over comments.** Use `Duration` in APIs and `_ms`/`_us` suffixes in config and
  events. Use newtypes or enums instead of magic numbers and bools.
- **Locks.** Keep critical sections short. Prefer `with(|state| ...)` closures that cannot
  leak a guard. Recover from poisoning with `unwrap_or_else(|p| p.into_inner())` instead of
  panicking.
- **Tasks.** Every `tokio::spawn` is owned. Store its `JoinHandle`, or make sure the task ends
  when its channel closes. Cap waits with `tokio::time::timeout`.
- **Logging.** `tracing` for humans: `warn!` for unexpected but handled, `info!` for rare
  lifecycle events, `debug!` for per-request detail. Measurements go to `metrics::Event`, never
  only to `tracing`.
- **Comments.** Explain why, not what. Every file starts with a `//!` block saying what it does
  and who owns it.
- **Dependencies.** Add one only with a design-log entry. Versions resolve through
  `Cargo.lock`, which is committed. openraft is pinned to `=0.9.25`.
- **Formatting and lints.** `cargo fmt` and `cargo clippy --all-targets -- -D warnings` must be
  clean before you push.

## Testing

- **Unit tests** go at the bottom of the file they test (`#[cfg(test)] mod tests`). The
  deterministic core (`state.rs`, `bidding.rs`, `planner.rs`) needs thorough unit tests.
- **Multi-node tests** go in `tests/`. They start real nodes in one process on `127.0.0.1:0`
  ports. Use the helpers in `tests/common/mod.rs`.
- **No fixed sleeps in tests.** Poll for the condition with a deadline (`wait_until`). Sleeps
  make tests slow on fast machines and flaky on slow ones.
- **Stable means 10 green runs in a row.** For a test that touches timing, run
  `for i in $(seq 10); do cargo test -q --test <name> || break; done` before merging.
- **Fix the code, not the test.** Never weaken an assertion to make a test pass without
  writing down why in the PR.

## Measuring

- Every event is logged with a unix-microsecond timestamp (`metrics::Event`, JSONL). Each owner
  adds and plots their own events. Keep field names stable once a script reads them.
- Client latency is measured on the client clock. Durations are measured with
  `std::time::Instant`, never by subtracting wall-clock timestamps from different machines.
- Run at least 5 repetitions per experiment cell, drop a 30 s warm-up, and keep each run's raw
  logs. Raw data never goes into git: `runs/` and `*.jsonl` are ignored.
- Experiment configs live in `experiments/`, analysis code in `analysis/`.

## Skills this project needs

- **Everyone:** Rust ownership and error handling, async Rust with tokio (tasks, channels,
  `select!`, timeouts), serde, git and PR review, reading JSONL logs.
- **A:** the Raft paper (terms, log matching, commit rules) and openraft's storage and network
  traits.
- **B:** election algorithms (Bully and variants), load metrics, fairness measures.
- **C:** failure detectors, timeouts, TCP behaviour under partition.
- **D:** log catch-up, the term rules, convergence checks.
- **E:** CPU-bound work in async systems, bounded queues and backpressure.
- **F:** load generation (open versus closed loop), percentiles, statistical design of
  experiments.
- **Analysis:** percentiles, confidence intervals, and plotting (Python is pending the
  instructor's approval).

## Notes for AI coding agents

- **Never push to `main`.** Work on a branch named as in [Git workflow](#git-workflow), push the
  branch, and stop at the pull request; people merge.
- **Stay in scope.** Work in the files of the part you were asked about. If the task needs a
  change in another part's file or in `proto.rs`, stop and say so instead of making it.
- **Follow the architecture rules.** If a request conflicts with one, point out the conflict
  instead of working around it.
- **Leave the toolchain alone.** Do not add dependencies, bump versions or change
  `rust-toolchain.toml` unless asked.
- **Check before you finish.** Run `cargo fmt --all`, `cargo clippy --all-targets -- -D
  warnings` and `cargo test`, on Linux or WSL when possible. Report what you ran and what you
  could not run.
- **Keep the code consistent.** Match the surrounding comment density and naming. Keep
  module doc comments up to date.
- **Keep the docs in sync.** When you change messages, timers or config keys, update
  `docs/protocol.md`, `config/local.toml` and the README in the same change.
- **Leave markers alone.** Do not delete `TODO(<part>)` markers you did not resolve, and do
  not create new top-level directories without asking.

## Glossary

- **Term:** Raft's logical epoch. At most one leader per term (`single-term-leader`).
- **Executor:** the node that plans a request: the bidding winner, or the leader in baseline
  mode.
- **First commit wins:** the state machine keeps the first committed result per request ID.
  Later proposals for that ID get `Outcome::Duplicate` with the winner.
- **Stale plan:** a plan whose responder was taken in the meantime. It is not recorded
  (`Outcome::Stale`); the executor plans again.
- **Serving:** a node that is current and may bid, plan and reply.
- **Silenced:** a node whose fault gate is closed. It drops all peer and client traffic.
