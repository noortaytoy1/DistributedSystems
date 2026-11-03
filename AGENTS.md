# AGENTS.md

Working agreement for everyone who changes this repo, people and AI coding tools alike.
Read it before your first change. If something here is wrong or missing, fix it in a PR.

RescueMesh is a three-node cloud of peers that plans incident responses for campus clients. Raft
(openraft) orders every committed change. A load-based bidding election picks the node that plans each
request. Nodes survive faults of up to 20 s and catch up when they return. The system is
measured with JSON Lines event logs. [README.md](README.md) covers what it does and how to run it.

The design lives in two documents, and both are part of every change:

- [docs/architecture.md](docs/architecture.md): components, deployment, every flow as a sequence
  diagram, state and lifecycle diagrams, each marked built, template, planned or open.
- [docs/design-log.md](docs/design-log.md): every design decision with its context, the options we
  rejected, the reasoning and the consequences. This is the raw material for the report.

## Grill with docs (mandatory)

**Every prompt you give an AI coding agent in this repo that may change a file must include the
words "grill with docs".** In Claude Code you can run `/grill-with-docs <task>` instead. No
exceptions: features, fixes, refactors, docs, configs, tests, one-liners.

```text
grill with docs: wire bid rounds into decide_executor
/grill-with-docs add peer heartbeats to node/heartbeat.rs
```

What it does: the agent reads the docs, then questions you about every design decision the task
needs, one at a time, each with options and its recommendation. Only after you have decided does it
write code, and it records every decision in the design log and updates the diagrams in the same PR.
The full procedure is in [Design decisions](#design-decisions) below and in
[.claude/skills/grill-with-docs/SKILL.md](.claude/skills/grill-with-docs/SKILL.md).

**Agents: if a prompt asks for a change and does not say "grill with docs", do not write, edit or
delete any file.** Answer in one or two lines asking the person to resend it with "grill with docs".
Read-only work (explaining code, answering questions, running builds and tests) is fine without it.

## Commands

```bash
cargo build                                   # debug build
cargo test                                    # unit + multi-node tests
cargo fmt --all                               # format (CI checks this)
cargo clippy --all-targets -- -D warnings     # lint (CI fails on any warning)
scripts/run-local.sh / scripts/stop-local.sh  # 3-node cluster on localhost
cargo run --bin ctl -- status                 # inspect a running cluster
bash scripts/start-users.sh --config config/local.toml --machine 1 --users 100   # 10 real load programs
```

On WSL, keep build output on the Linux filesystem; building onto `/mnt/c` or `/mnt/f` is slow:
`export CARGO_TARGET_DIR="$HOME/.cache/rescuemesh-target"`.

The code must build and pass tests on Linux, which covers the lab machines and WSL. Windows
builds are a convenience; never use Windows-only APIs or paths.

## Deployment target: six Ubuntu machines

The system must eventually run on **six separate Ubuntu machines on one network**: three cloud
nodes and three client machines running load generators. Running everything on one machine
(`config/local.toml`, the tests) is only for development; the demo and the experiments use the
six machines. Write every change so it works there:

- **No localhost assumptions.** Every address comes from the config file. A node binds the
  `peer_addr` and `client_addr` listed for its ID, so the lab config must use each machine's real
  LAN IP (see `config/lab.example.toml`). Never hard-code `127.0.0.1` or a port outside
  `config.rs` and the configs. The one exception is the frontend's `--listen`, which only a browser on the
  same PC connects to (D199).
- **One config file, copied to all six machines.** Every node and client must read the same file;
  a different node list on one machine breaks Raft membership. `scripts/start-users.sh` gives each load
  program its client ID from the machine number and the batch, so each machine needs its own `--machine`. By
  hand: `loadgen --client-id` from 1 to 9 (D184).
- **Same build everywhere.** Each machine builds from the same commit with the toolchain pinned in
  `rust-toolchain.toml`. Mixed versions can change the wire format or the state digest. Rebuild after every `git pull`, and wipe `data/raft`
  when the version changes (see docs/runbook.md).
- **Nothing shared but the network.** No shared filesystem and no shared clock. Each machine
  writes its own event logs; collect them afterwards. Wall-clock timestamps from different
  machines are only roughly aligned (run NTP); measure durations on one machine.
- **Real networks are slower and less tidy than loopback.** Timers in `config.rs` are LAN
  defaults; anything that relies on timing must be tried on the lab machines, not only in tests
  on one host.
- **Open the ports.** Peer ports (default 7001) must be reachable over TCP between the three node machines, and client ports (default
  8001) from the client machines over UDP and TCP, e.g. `sudo ufw allow 7001/tcp`, `sudo ufw allow 8001/udp` and `sudo ufw allow
  8001/tcp`. `ctl status` says `udp no answer` if a rule is missing. Raise `net.core.rmem_max` on all six (see docs/runbook.md).
- **Ubuntu only.** Scripts are bash, paths are POSIX, and nothing may depend on Windows or macOS
  behaviour.

## Parts and owners

Each part has one owner who builds it, measures it, demos it and writes its report section.
Your review pair reviews your PRs and must be able to explain your part at the demo.

| Part | Mechanism                | Files                                                                | Measures              | Owner |
|------|--------------------------|----------------------------------------------------------------------|-----------------------|-------|
| A    | Raft and replication     | `raft/`, `state.rs`, `node/mod.rs`, `config.rs`                      | E5, commit latency    |       |
| B    | Bidding election         | `bidding.rs`, `node/bids.rs`, `node/cpu.rs`                          | E1 policies, E6       |       |
| C    | Faults and failover      | `node/faults.rs`, `node/heartbeat.rs`, `node/transport.rs`, `node/datagram.rs`, `node/degraded.rs`, `node/group.rs`, `bin/ctl.rs` | E3 |       |
| D    | Rejoin and recovery      | `node/rejoin.rs`                                                     | E4                    |       |
| E    | Planner and concurrency  | `planner.rs`, `campus.rs`, `node/workers.rs`                         | E2, CPU and queues    |       |
| F    | Client and scale         | `client.rs`, `bin/loadgen.rs`, `bin/frontend.rs`, `scripts/start-users.sh` | E1 scaling sweep, E7  |       |
| all  | Shared                   | `proto.rs`, `metrics.rs`, `lib.rs`, `bin/node.rs`, `node/testing.rs` (test doubles for unit tests in `node/`), `tests/`, `docs/` | —                     |       |

Review pairs: A↔D, B↔E, C↔F.

`raft/` includes `raft/proposals.rs`, the proposal queue (D133). Row A also holds `node/service.rs` and `node/step_aside.rs`, and
`src/middleware.rs` is shared (D156, D218).

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
4. **Every decision goes in the design log, in full.** Not only surprising ones: any choice a
   teammate, the instructor or the report could ask "why?" about. Use the template in
   [docs/design-log.md](docs/design-log.md) and update [docs/architecture.md](docs/architecture.md)
   in the same PR. See [Design decisions](#design-decisions).
5. **Everything goes through a pull request.** Nobody pushes to `main` directly, not even a
   one-line fix. See [Git workflow](#git-workflow).
6. **Library code is kept apart from ours.** The report must separate them. openraft (Raft) and tarpc (remote calls)
   are the only libraries doing distributed-systems work (D179, D215); anything new of that kind needs team agreement first.
7. **The docs decide.** The spec, the class notes and the recordings settle a question about what the course wants. Never
   leave a question for the instructor in a doc, and never tell anyone to ask him (D213). State a reading with its source.

## Design decisions

People decide; agents ask, recommend and write it down. This holds for every change, and is what
"grill with docs" runs.

1. **Read first.** Before asking anything, the agent reads this file,
   [docs/design-log.md](docs/design-log.md), [docs/architecture.md](docs/architecture.md),
   [docs/protocol.md](docs/protocol.md) and the code the task touches, so it does not ask about
   what is already decided and can spot conflicts.
2. **Ask about every decision.** The agent never settles a design question on its own. It asks the
   person, one question at a time, with 2–4 concrete options, their trade-offs, and its
   recommendation first with the reason. Decisions include messages and fields, timer values,
   config keys and defaults, replicated data structures, failure handling, retries, what is
   measured and the event field names, dependencies, tests, and choices *not* to do something.
3. **Challenge, don't work around.** If the task conflicts with a logged decision, an
   architecture rule or another part's interface, the agent raises it as a question.
4. **Settle open decisions when you touch them.** Many entries in the design log are `Open`,
   mostly conflicts between the Phase 1 architecture document and the code (see the list under its
   index). They stay open until someone implements the related code. When a task touches one, the
   agent asks the person to settle it first, with the options and recommendation from the entry,
   and updates the entry in the same PR. It never builds on one side of an open conflict silently.
5. **"You choose" is still a decision.** The agent states what it picks and why, and records it as
   decided by the person on the agent's recommendation.
6. **Record everything, verbosely.** Each decision becomes a design-log entry: date, part, status,
   who decided, context, options considered, decision, why, consequences, where in the code, how
   we check it, and notes for the report. Changed decisions get a new entry; the old one is marked
   `Amended` or `Superseded`, never rewritten. Open questions are logged as `Open` with options and
   a recommendation.
7. **Keep the diagrams true.** Every PR that changes a component, message, timer, flow or decision
   updates the matching section and Mermaid diagram in `docs/architecture.md`, including its
   built/template/planned label, and `docs/protocol.md` when messages or timers change.
8. **Cite decisions in the PR.** The PR description lists the design-log IDs it adds or changes.

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
- **Sockets live in two places.** Only `node/` and `raft/network.rs` open sockets, with one named exception:
  `bin/frontend.rs`, which only listens for the browser on its own PC (D200). `client.rs` goes through
  `transport::connect_client` and `transport::datagram_client` (D175).
- **Middleware and application stay apart, on both sides.** On the client side the application makes one `submit` call and
  `MeshClient` does the rest. On the cloud side the incident service uses the group and Raft through their interfaces. It is
  asked about repeatedly, so keep it visible (D211, `docs/architecture.md` section 4.3).
- **Nodes are peers.** Client and server are the roles of one call, never of a node (D212).
- **Pings address everyone.** Never send heartbeats through `CloudGroup::send`: two nodes that removed each other would never
  hear each other again.
- **A call ends by its deadline.** `PeerClient::call` enforces it, because tarpc times only what it has written (D150).
- **Saves and heartbeats come first.** The saving to disk and the "I am alive" of a node must never be crowded out by
  processing requests (Noor's rule, D133).
- **Never block the async runtime.** CPU-heavy work (planning) runs through
  `node/workers.rs`. No `std::thread::sleep`, no blocking file or network I/O in async code,
  and no `std::sync::Mutex` guard held across `.await` (clippy denies `await_holding_lock`).
- **Every peer message carries the sender's Raft term.** Receivers drop non-Raft messages
  from an older term and answer `StaleTerm`. Never drop Raft RPCs this way, because openraft
  must see them to reject a stale leader.
- **Commit, then reply.** A node replies to a client only after the result is committed and
  applied. The first committed result per request ID wins; later ones get the winner back. The one
  exception is degraded mode (D120): a node left alone answers `Provisional`, clearly labelled, and
  replays the result through Raft when a peer returns. A refusal of a report too old to plan is committed
  like any result, so it is no exception (D143).
- **Every `Command` is idempotent.** A proposal can be retried after a leader change, so
  applying a command twice must equal applying it once.
- **A returning node serves nothing until it is current.** It does not bid or reply until its
  applied index reaches the leader's commit index (`NodeContext::serving`). This is checked again just before
  each reply, so a handler that was waiting on a commit stays quiet too (D136). It does not vote for
  itself either, unless it has heard from no leader for 3 s (D125).
- **The fault gate drops everything but control.** A silenced node neither sends nor receives
  peer or client traffic. Control calls, from `ctl` and from status polls, still get through, and any method added to `ClientRpc` must
  decide which it is. On the client port the gate sits in the UDP reader, in front of tarpc (D174).
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
  only to `tracing`. The default filter is `DEFAULT_LOG_FILTER` in `lib.rs`. A library that logs per call must be
  turned off there (D134).
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
- **The runner has tests with nothing shared.** `tests/runner_script.rs` and `tests/runner_over_ssh.rs` run the runner through a stand-in `ssh`, with
  each machine in its own directory. `experiments/hosts.local` cannot show a line that goes wrong only on a far machine, so a change to how the runner reaches a
  machine must keep them green (D188, D222). They are not a real `sshd`, so anything new over ssh also needs a try on the lab.
- **No fixed sleeps in tests.** Poll for the condition with a deadline (`wait_until`). Sleeps
  make tests slow on fast machines and flaky on slow ones.
- **Stable means 10 green runs in a row.** For a test that touches timing, run
  `for i in $(seq 10); do cargo test -q --test <name> || break; done` before merging.
- **Fix the code, not the test.** Never weaken an assertion to make a test pass without
  writing down why in the PR.

## Measuring

- Every event is logged with a unix-microsecond timestamp (`metrics::Event`, JSONL). Each owner
  adds and plots their own events. Keep field names stable once a script reads them. To compare nodes, use
  `state_checkpoint`, logged at fixed log indexes, not `state_digest` (D138).
- Client latency is measured on the client clock. Durations are measured with
  `std::time::Instant`, never by subtracting wall-clock timestamps from different machines.
- Two profiles (D63): `full` is 5 runs per cell with a 30 s warm-up and 2 min measured; `quick` is 3
  runs with 15 s and 60 s, and the report states its limits. Keep each run's raw logs. Raw data never
  goes into git: `runs/` and `*.jsonl` are ignored.
- Experiment configs live in `experiments/`, analysis code in `analysis/`.

## Skills this project needs

- **Everyone:** Rust ownership and error handling, async Rust with tokio (tasks, channels,
  `select!`, timeouts), serde, git and PR review, reading JSONL logs.
- **A:** the Raft paper (terms, log matching, commit rules) and openraft's storage and network
  traits.
- **B:** election algorithms (Bully and variants), load metrics, fairness measures.
- **C:** failure detectors, timeouts, TCP behaviour under partition, UDP sockets and receive buffers.
- **D:** log catch-up, the term rules, convergence checks.
- **E:** CPU-bound work in async systems, bounded queues and backpressure.
- **F:** load generation (open versus closed loop), percentiles, statistical design of
  experiments.
- **Analysis:** percentiles, confidence intervals, and plotting, in Python (D64).

## Notes for AI coding agents

- **No "grill with docs", no writing.** A prompt that would change a file must contain "grill with
  docs" (or be a `/grill-with-docs` run). Otherwise ask for it and change nothing. See
  [Grill with docs](#grill-with-docs-mandatory).
- **Never decide design on your own.** Ask, with options and your recommendation, then record the
  answer in `docs/design-log.md` and update `docs/architecture.md`. See
  [Design decisions](#design-decisions).
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
  `docs/protocol.md`, `config/local.toml` and the README in the same change. When you change a
  component, flow or decision, update `docs/architecture.md` and `docs/design-log.md` too.
- **Leave markers alone.** Do not delete `TODO(<part>)` markers you did not resolve, and do
  not create new top-level directories without asking.

## Glossary

- **Term:** Raft's logical epoch. At most one leader per term (`single-term-leader`).
- **Leader:** the node a majority elected to order the log. It numbers entries, sends the copies and answers one read, and it decides nothing else (D163). The bid moves work by load (D217). A leader that stays overloaded steps aside, and a normal vote picks another (D218).
- **Going quiet:** a leader stopping its heartbeats and holding its writes, so that followers' timers run out and an ordinary vote moves the leadership (D218). No timer or vote rule changes.
- **Executor:** the node that plans a request: the bidding winner, or the leader in baseline
  mode.
- **First commit wins:** the state machine keeps the first committed result per request ID.
  Later proposals for that ID get `Outcome::Duplicate` with the winner.
- **Stale plan:** a plan whose responder was taken in the meantime. It is not recorded
  (`Outcome::Stale`); the executor plans again.
- **Serving:** a node that is current and may bid, plan and reply.
- **Silenced:** a node whose fault gate is closed. It drops all peer and client traffic.
- **Degraded:** a serving node whose peers have all been suspected for `degraded_after_ms`. It answers
  without a commit (D120).
- **Provisional:** an answer given without a commit. It is replayed through Raft later, and first commit
  wins, so it can turn out to differ (D122).
- **Fault election:** every 30 to 60 s the three nodes draw random numbers and send them to each other; the highest goes silent for 1 to 20 s. A node missing
  any number does nothing, so a node that is out stops the next election. No leader, nothing in the log (D158).
- **Group send:** the client middleware's send to all of the cloud nodes. The application makes one call and does not know it
  happens (D210).
- **Stub:** tarpc's generated client for one node. `MeshClient` holds one per node, each on its own UDP socket.
- **Parked call:** a call open on a node with nothing to say. It ends at the client's cancel, or at its deadline if the cancel is lost (D169, D173).
- **Group:** the cloud nodes as one named group (`node/group.rs`). `send` goes to the view and `probe` to every configured peer. Order
  and delivery of results come from Raft, not the group.
- **View:** one node's list of who is in the group, the membership. A peer leaves after `degraded_after_ms` of unbroken suspicion and
  returns when heard. It means reachable, not able to serve, and nodes do not agree on it.
- **Incident status:** where a report stands: `Unassigned`, `Assigned`, `Resolved`, `Expired`, or `Provisional` on the client.
  Worked out on read from the record and the hold, never stored (D142).
- **TTL:** in this project `Incident.ttl_ms`, how long a report stays valid (D143). A report too old on arrival is not planned:
  the node commits nobody and answers `Expired`. Not to be confused with the trim D9 dropped, the hop limit of Phase 2, or an
  RPC's deadline.
- **Priority:** `Plan.priority`, an integer in points, higher is more urgent, from severity, age, distance, availability and
  capability (D148). It does not change who is sent.
- **Program:** one real `loadgen` process on a client PC, holding one or more simulated users as tokio tasks. A client PC runs 10
  by default (D181).
- **Batch:** a group of programs started together through `scripts/start-users.sh` on one client PC, 0 to 9. Experiments use 0 (D184).
