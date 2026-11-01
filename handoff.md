# RescueMesh handoff (2026-11-01)

Written for the next session, on any device, so it can continue exactly where this one stopped.
This file lives only on the branch `shared-handoff`, one commit above `part-c-tarpc`. **Never open a
PR for that branch or build on it.**

## 0. Starting on a new device

```bash
git clone https://github.com/noortaytoy1/DistributedSystems.git
cd DistributedSystems
git fetch origin
git switch part-c-tarpc
git show origin/shared-handoff:handoff.md > handoff.md
echo handoff.md >> .git/info/exclude
curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none
export PATH=$HOME/.cargo/bin:$PATH CARGO_TARGET_DIR=$HOME/.cache/rescuemesh-target
cargo test
```

`rust-toolchain.toml` pulls Rust 1.94.1 by itself. `cargo test` should report 178 passed. Then check
that every branch in section 1 exists on origin and has the commit listed there. If one is missing,
the push from the old device did not finish: stop and tell the user.

Set up on a new device, if needed: a git identity (section 2), GitHub push access, and `gh` if wanted.
This device's Claude memory does not travel; everything it held is in this file.

**Deadline: Phase 1 is due Nov 4, 2026, at 8:00 PM.**

## 1. Where things stand

**The next action is to start PR6, `part-c-degraded-mode`, from the tip of `part-c-tarpc`.**
Section 5 gives the steps.

The grilling is finished, apart from the small questions listed under each remaining PR. Every
decision is in section 4. Do not ask those again.

| PR | Branch | State |
|----|--------|-------|
| 1 | `part-a-release-age` | Committed at `6b5df1a`. |
| 2 | `part-f-client` | Committed at `b23dd21`. |
| 3 | `part-f-loadgen` | Committed at `1ab14ad`. |
| 4 | `part-a-durable-storage` | Committed at `a0cca61`. |
| 5 | `part-c-tarpc` | Committed at `a74f18a`. **PR6 branches from here.** |
| 6 | `part-c-degraded-mode` | **Not started: this is next.** Decisions are made. |
| 7 | `shared-group-comm` | Not started. Decisions are made. |
| 8 | `shared-experiments` | Not started. Decisions are made; the logging is still to do. |
| 9 | `shared-docs` | Not started. Decisions are made; some logging is still to do. |

The branches are stacked: each is based on the one above it, and the base of the stack is `main` at
`c8ddbc5`. **The user pushes these branches from the old device, because that session had no GitHub
credentials. No PR exists.**

### Test state at the tip (`part-c-tarpc`, `a74f18a`)

- `cargo fmt --all -- --check`: clean.
- `cargo clippy --all-targets -- -D warnings`: clean.
- `cargo test`: 178 passed, 0 failed.
- 10 runs in a row of each of `cluster`, `bidding`, `faults`, `heartbeat`, `rejoin`, `replication`,
  `takeover` and `durable`: all green, on the tarpc commit before the last docs-only commit.
- `client`: 1 failure in 10, from the false `client_link_down` bug. After that fix (`b23dd21`, rebased
  under tarpc), 15 of 15 green.

### Not yet verified

- PRs 1 to 4 were tested at the time they were built, but **after the rebase only the tip was
  re-run.** The working rules require the checks before each PR, so run them on each branch before
  pushing (section 7).
- `a_silenced_node_keeps_its_connection...` failed before the fix. The one captured failure was
  caused by that bug. The earlier failures had no captured output, so they are presumed to be the
  same cause. The 15 green runs support that, but it is not proven.
- Hand-run on one machine only, and **not experiment results**: a local cluster under `loadgen`, and
  `kill -9` of the leader under load with a restart (no requests lost, digests equal, about 124 ms
  catch-up). That second run used the in-house transport, before tarpc. tarpc has only been through
  the test suites, so repeat the kill test once.
- Nothing has run on the six lab machines. Real power-off, a real LAN and NTP are all untested.

## 2. Environment

On the old device, installed with the user's permission: rustup and Rust 1.94.1, in `~/.cargo` and
`~/.rustup`. There, the apt Rust 1.75 is still first on PATH and **cannot build this repo**, and the shell
config was deliberately left alone. Every Bash call needs:

```bash
export PATH=$HOME/.cargo/bin:$PATH CARGO_TARGET_DIR=$HOME/.cache/rescuemesh-target
```

- **That PATH line applies to the old device;** on a new one, use whatever rustup sets up. Skip
  `CARGO_TARGET_DIR` if you prefer; it is only for WSL and shared disks.
- **Running processes:** none. All local test nodes were stopped, and nothing needs restarting.
- **`gh`:** not installed. The user opens the PRs; give them the text and the order (section 7).
- **Push credentials:** the old device had none (a push failed asking for a username). The repo is
  **public**, so reading needs none. If a push fails on this device, tell the user rather than
  working around it.
- **Git identity:** none is configured, and the global config was deliberately left alone. Commit
  with:

  ```bash
  git -c user.name=omar.leithym -c user.email=omar.leithym@gmail.com commit
  ```

  End every commit message with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`, and every
  PR body with `🤖 Generated with [Claude Code](https://claude.com/claude-code)`.
- **Python:** python3 exists, without numpy, scipy or matplotlib. Use a `.venv` (already git-ignored)
  and a pinned `analysis/requirements.txt`.
- **Scratch files:** the old session's scratchpad is gone. It only held helper scripts: the design-log editing helpers were small Python functions that update an index row, a
  status line or an entry. Rewrite them if useful.
- **Commit script warning:** the old helper committed without checking test results, and that let a
  flaky commit through once. Read the 10-run results before committing.
- **Source documents:** the two PDFs stayed on the old device, because this repo is public and the
  user has not approved publishing them. Ask the user for them if the summary below is not enough.
  They were `Dr Amr comments about Project during class.pdf` and `RescueMesh_Term_Project_v2.pdf`.

### What those documents require, in summary

Dr Amr's class notes:

- Load balancing, access transparency (a user feels nothing when a server fails), fault tolerance.
- Peers with **no coordinator**, said twice. Cloud of 3 physical nodes, 6 machines in all.
- He wants to see one node fail, how transparency holds, and how it recovers; a machine **physically
  turned off**; and one off plus one failed, with the one remaining server handling 3 client nodes.
- Failure detection: typically 5 retries, then an error.
- Mimic failure using election: a node fails for 20 s, recovers, and synchronises with the others.
- Replicas: the application sends a message once and the middleware gives replicas a copy. Keep the
  application and middleware layers distinct.
- Use a ready-made RPC or RMI rather than low-level UDP, pick one and say why, using the chapter 2
  table (Java RMI, CORBA, Sun XDR).
- Implement publish-subscribe and group communication. Discovery belongs to Phase 2.
- Report: explain how a node recovers, every experiment, how long workloads ran, what data was
  collected, alternatives, "what happened", "what we did", "who did what". Credit open source.
- In the demo, show the first 4 entries and the last entry of the "entries table" (see section 4).

Project description (Phase 1 due Nov 4, Phase 2 due Dec 5):

- **Phase 1:** three-node cloud with incident processing; distributed election for work assignment,
  with a justified load metric; concurrent processing, documenting the parallelisation models;
  injected failures of up to 20 s with transparent continuation; rejoin and convergence; stress tests
  with statistically meaningful data, from tens of thousands of operations where feasible.
- **Planner:** combines severity, age, distance, availability and capability; substantial enough to
  create real computational load; deterministic and tunable.
- **In-flight requests** must be defined: detected, retried or completed. Duplicates prevented or
  harmless. Recovery says what is copied, replayed or rebuilt, and how a node knows it is current.
- **Phase 1 demo rows:** normal operation (different peers elected as load changes), load imbalance,
  server failure of the elected server under load, server recovery, scale/stress.
- **Measure:** mean, median, p95, p99; throughput against clients; election frequency, duration and
  message overhead; load distribution and fairness; before, during and after a failure; recovery and
  convergence time; CPU, memory, network, queues.
- **Grading:** documentation of decisions, alternatives and justifications is "of utmost importance".
  The report separates library functions from team-built protocols, and has a contribution section.
- **Phase 2 only:** discovery, direct P2P, zone subscriptions, dissemination, offline operation,
  leases and conflict reconciliation. Speculative actions must be clearly labelled and reconciled.

## 3. Rules from the user's request

- Takeover is **out of scope** and belongs to Abed. It **has since landed** on `main` (PR #13, D42,
  D110), so nothing is waiting on it. Do not change it, leave the stale `TODO(B)` takeover marker in
  `src/bidding.rs`, and do not change the takeover flow in `docs/architecture.md` section 6.7. Tell
  the user the marker is now stale.
- Never push to `main`. Use small PRs and stop at each one, because people merge.
- Before each PR, run `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings` and `cargo test`.
  Run timing-sensitive tests 10 times in a row. Report exactly what ran and what failed.
- A change to another part's file, or to `proto.rs`, needs the review AGENTS.md requires. Say so in
  the PR and list the reviewers.
- Ask before adding a dependency, and log it.
- **Do not fabricate or estimate any results.**
- Do not fill in the owner column in AGENTS.md or write the report sections. List what the team must
  do.
- AGENTS.md's design rule stands: ask about each new design decision one at a time with a
  recommendation, using AskUserQuestion. Log every decision in `docs/design-log.md`, and update
  `docs/architecture.md` and `docs/protocol.md` in the same PR. Use the entry template at the top of
  the design log. New entries go in section 15, "Finishing Phase 1", at the end of the file. **The
  next free ID is D120.** Record the decider as `omar.leithym`, adding "on the agent's recommendation"
  where that applies.

## 4. Every decision the user gave

"(rec)" means the user took the agent's recommendation. **"against rec"** means they chose otherwise,
so do not reopen it. "Logged" means the entry is in the design log on the stack.

### Logged

| Topic | Decision | Entry |
|-------|----------|-------|
| Client retry budget | 5 retries; waits of 2, 4, 6, 6, 6, 6 s; ±20% jitter; error at the end. New key `client_retry_max_ms = 6000`, and `client_max_retries = 5` (rec) | D52 |
| Responder release | Executor releases after the service time; peers release after twice that. Not the leader, because of the "no coordinator" rule (rec) | D39 |
| Service time | 200 ms × severity, key `campus.service_ms_per_severity` (rec) | D111 |
| Incident age | `now − reported_at_ms` passed to `plan()`; 5000 ms or more gets the severe fallback (rec) | D95 |
| Quiet nodes | Confirmed: `Rejected` stays unused (rec) | D36 |
| Runner | Confirmed: bash over SSH, and `ctl` stays the operator tool (rec) | D62 |
| RPC | **tarpc on the peer port, against rec** (the agent recommended keeping the transport and asking Dr Amr). The client port stays as it is | D17, D119 |
| tarpc shape | Typed methods (rec) | D119 |
| Dead nodes | Client pings with `Control(Status)` every 1 s and replaces a connection after 3 s of nothing (rec) | D112 |
| Send queue | 1024 frames per node, dropped when full (rec) | D112 |
| Client tests | The agreed list in `tests/client.rs`, 10 runs (rec) | D112 |
| Client ID | Required; `seq` starts at the current time in microseconds (rec) | D113 |
| Open loop | `--rate R` at evenly spaced intended times, latency from the intended time, `--warmup-s` (rec) | D54 |
| Bidding default | **Bidding is the default now.** User's words: "we want leader for the state replication and maintenance and the bidding to decide who takes the request". Leader dispatch stays as E1's baseline | D114 |
| Coordinator | **Keep Raft and the leader-run fault schedule D12, against rec** (the agent recommended moving the schedule) | D116 |
| Richer faults | Not built; the `TODO(C)` marker stays (rec) | D117 |
| Power-off | Raft log and vote on disk, with sync (rec). Supersedes D24 | D118 |
| Split votes | A different election range per node (rec) | D115 |

Known gaps in what was logged or built:

- The question for D115 quoted node ranges of 150–224, 225–374 and 375–524 ms. The code uses 150–225,
  300–375 and 450–525 ms, because 1 ms of separation could not guarantee different ticks. D115 records
  the real values. **Tell the user about this difference**; it has not been raised yet.
- D112's question said a frame is also dropped while a node is disconnected. In the code, frames wait
  in the queue until it is full, then drop. D112 records this. **Tell the user this too.**
- D52's status is `Accepted`. It should become `Implemented` now that the client is built.
- D118 and D119 were not asked about the 1 s connection-replacement rule in `transport.rs`, or the 3 s
  rule for allowing elections after a restart. They are recorded in those entries as consequences.
  **Point both out to the user in the PR text.**
- The `resource_usage` event and `storage.dir` default were accepted in the final plan confirmation.

### Decided but not logged yet: log these in the named PR

| Topic | Decision | PR |
|-------|----------|----|
| One server left | **Degraded mode, against rec** (the agent recommended a clear error plus old results). The lone node answers without a commit. A deliberate exception to "Commit, then reply" | 6 |
| Degraded trigger | Both peers suspected for 2 s, key `timers.degraded_after_ms = 2000`. Never while silenced or not serving (rec) | 6 |
| Degraded end | Replay each provisional result through Raft as `CommitResult`. First commit wins; a loser logs `provisional_conflict`. If the responder is taken, record the plan with no responder. Clients are not told (rec) | 6 |
| Degraded reply | New `ReplyBody::Provisional(Plan)`. The client accepts it at once if no `Done` came, and returns `provisional: true`. `client_done` gets that field (rec) | 6 |
| Degraded holds | A local table freed by the same service timer (rec) | 6 |
| Group communication | Name it with a small module: `PeerSet::broadcast` used by bids and heartbeats; `MeshClient` is the client-side group send. Docs map the guarantees (rec) | 7 |
| D63 method | `quick` profile: 13 cells, 3 reps, 15 s warm-up + 60 s measured + 15 s restart, about 1 hour. `full` profile: 5 reps, 30 s + 2 min, about 8 hours (rec). The user first asked for 1 or 2 reps; told that gives no usable CI, they chose this. Amend AGENTS.md's "at least 5" and "drop 30 s" through the log, and state the limits in the report | 8 |
| D64 | **Python is approved**, per the user. Use a venv with numpy, scipy and matplotlib. Remove the pending notes | 8 |
| FT-6 | Count elections following each `rejoined` (rec) | 8 |
| `resource_usage` | A once-a-second event for memory and CPU, because the spec asks for them (rec) | 8 |
| D69 | Phase 2 hooks accepted, **docs only**: discovery, P2P | 9 |
| Publish-subscribe | **Phase 2, for zones, docs only.** User's words: "we will use publish sub in phase 2 for the zones, for phase 1 we are not dealing with zones but add it to docs so we can do it later". The spec also lists zone subscriptions under Phase 2 | 9 |
| "Entries table" | It is the spec's **Required Demonstration Scenarios** table: the first 4 rows (normal operation, load imbalance, server failure, server recovery) plus the last (scale/stress). Build a demo script for those 5. No `ctl log` command | 9 |

### The user's working pattern

- Decides quickly; usually takes the recommendation; overrides it on design where Dr Amr's wishes are
  at stake (tarpc, degraded mode), even at higher risk.
- Treats Dr Amr's notes and the spec as requirements. Check new work against both PDFs.
- Wants the honest answer on whether something is acceptable (asked if 1 or 2 reps would do).
- Sometimes answers a question with a file or a free-text reply. Read it as the answer; if a reply
  carries no content, say so rather than guessing.

## 5. PR6: `part-c-degraded-mode` (do this next)

```bash
git switch part-c-tarpc && git switch -c part-c-degraded-mode
```

Suggested design, to fit the decisions above. Ask only if something forces a new design choice:

- **`proto.rs`:** add `ReplyBody::Provisional(Plan)`. Every owner reviews.
- **`config.rs`:** add `timers.degraded_after_ms = 2000`, validated above 0. Update
  `config/local.toml`, the lab config and `protocol.md`. A reviews.
- **New `src/node/degraded.rs` (owner C):**
  - A task enters degraded mode when every peer has been suspected continuously for
    `degraded_after_ms`, this node is serving and it is not silenced. It leaves when any peer is
    trusted again, or when it is silenced. Use `NodeContext::suspicion` and a new flag on the context.
  - A local table holds `RequestId` → provisional plan, so a retry gets the same answer, plus local
    holds on responders, freed after the service time.
- **`node/mod.rs::handle_request`:**
  - Order: not serving → silent. Committed result → reply with it. Degraded → plan on the worker pool
    from free responders minus local holds, store it, and reply `Provisional`.
  - Check degraded **before** `decide_executor`, because Raft cannot commit alone.
  - Needs one hook in `node/mod.rs`, so A reviews.
- **On leaving degraded mode:** propose each stored result as `CommitResult`, retrying until a quorum
  exists. On `Stale`, re-propose with `responder: None`. On `Duplicate` with a different plan, log
  `provisional_conflict`.
- **Events:** `degraded_entered`, `degraded_left`, `provisional_replied`, `provisional_conflict`.
- **`client.rs`:**
  - `hand_over` accepts `Done` and `Provisional`, and `Rejected` stays ignored.
  - `Submitted` currently has `result: CommittedResult`. A provisional answer must **not** pose as a
    committed result, so change the shape, for example `Submitted { from, plan, executor, committed:
    Option<CommittedResult>, provisional, latency, attempts }`, and update `loadgen.rs`
    (`done.result.plan`) and `tests/client.rs`.
  - Add `provisional` to `ClientDone`. Have loadgen print the provisional count.
- **Tests (`tests/degraded.rs`), using `wait_until`:**
  - Silence two nodes: the third answers `Provisional` through `MeshClient`.
  - A retry gets the same plan.
  - Restore the peers: the result is committed and digests match.
  - A single silenced node never triggers degraded mode.
  - A deliberate conflict logs `provisional_conflict`.
- **Docs:**
  - Log the entries with options and reasons. The report must say this breaks "Commit, then reply",
    that in a real partition two sides can answer differently, and that clients are not told.
  - Amend the "Commit, then reply" rule in AGENTS.md to name this exception, and also the glossary.
  - Update `architecture.md` with a new sequence diagram and a new state in section 8.3, and
    `protocol.md`. Add the spec's "speculative action must be clearly labeled and reconciled" as a
    reference.
- **Reviewers:** C, F, A and every owner, for `proto.rs`.

Also repeat the `kill -9` restart test by hand here, on tarpc.

## 6. PRs 7 to 9

### PR7 `shared-group-comm`

- Add `PeerSet::broadcast` in `transport.rs`, used by `node/bids.rs:150` and `node/heartbeat.rs:142`
  instead of their own loops. No message changes.
- Docs map the guarantees: client multicast and bids/heartbeats are best-effort; state is totally
  ordered through Raft. Dr Amr's point is that the application sends once and the middleware copies
  it to replicas, so show `submit` as the application call and `MeshClient` as the middleware.
- Log the decision.
- Reviewers: C, B.

### PR8 `shared-experiments`

Log D63, D64, FT-6 and `resource_usage`.

- **`resource_usage` event:** a once-a-second event from the reporter in `node/mod.rs`, read from
  `/proc`, with memory and CPU. Keep the name stable.
- **`experiments/`:** a config per cell, named `<experiment>-<cell>.toml`. The 13 `quick` cells:
  - E1: 30, 300 and 3,000 users × 2 policies (6 cells).
  - E2/E6: skew × 2 scores (2 cells).
  - E3/E4/FT-6: 2 loads × 2 victims, with silences, sharing runs (4 cells).
  - E5: faults off (1 cell).
- **`full` profile:** 5 reps, 30 s + 2 min, about 31 cells. Define the remaining cells.
- **`scripts/`:** bash over SSH and rsync, from a hosts file:
  - Build the same commit everywhere.
  - For each run: wipe `storage.dir`, start nodes, start loadgens with `--client-id 1`, `2`, `3`, stop,
    rsync logs to `runs/<experiment>/<cell>/<run>/`.
  - Save the config and the git commit hash next to the logs.
- **`analysis/`:** Python with a venv and pinned requirements. It produces p50/p95/p99 and mean with
  95% CIs across runs (Student's t), throughput, and the spec's list: load distribution and fairness,
  election frequency, duration and message overhead, failover and recovery time, before/during/after a
  fault, CPU, memory, network and queues, the share of no-responder plans (D111), and the provisional
  count. Output goes to `analysis/out/`. Load `dataviz` before writing plot code.
- **Tests for the analysis:** feed in a small synthetic log and check the numbers. Keep such fixtures
  clearly synthetic. **Never put numbers in the docs.**

Two points to raise with the user while doing this:

- In a local trial, about 98% of plans at high load had no responder, which is what D111's capacity of
  about 50 assignments per second predicts. So E1 at high load mostly measures that path.
- `workers.plan_work` defaults to 0, so planning costs almost nothing, while the spec wants a
  planner that creates real load and load balancing needs it. Ask what `plan_work` the experiments
  use. This is a **new decision**, and the 200 ms and 30-responder settings may belong in it too.

Reviewers: everyone, because it is shared.

### PR9 `shared-docs`

- Log D69 and publish-subscribe (Phase 2, docs only).
- Fix `docs/architecture.md`:
  - The C4 label still reads "off by default" (line 137), plus any other stale template/built labels.
  - Section 13, Phase 2 hooks: add zone pub-sub, and discovery.
  - Section 12, changes since Phase 1: add the new decisions.
  - Section 14, open-decisions list: it names D5, D17, D39, D42, D52, D63 and D95, nearly all of which
    are now settled.
  - The design log's "Open decisions" note.
  - `experiments/README.md` and `analysis/README.md`.
- Update the README: run commands, loadgen, the bidding default, storage.
- Update `config/lab.example.toml`: new keys, and bidding is already set.
- Add a six-machine runbook (put it in `docs/`): shared config, ports (`ufw allow 7001/tcp` and
  `8001/tcp`), NTP, the pinned toolchain, unique client IDs, log collection, wiping `storage.dir`.
- Add a demo script for the 5 scenarios, plus Dr Amr's cases: a machine physically off, then one off
  and one failed so one server handles the 3 clients.
- Do not fill in owners or write report sections.

## 7. Pushing and the final report

After PR9:

1. On **each** branch, in stack order, run the checks and the 10-run loop, and record the results:

   ```bash
   cargo fmt --all
   cargo clippy --all-targets -- -D warnings
   cargo test
   for t in cluster client bidding faults heartbeat rejoin replication takeover durable; do
     for i in $(seq 10); do cargo test -q --test $t || break; done
   done
   ```

   Add `degraded` from PR6 on. Fix any failure on the branch that owns it and rebase the stack above
   it. Rebase with the same `git -c user.name=... -c user.email=...` identity.
2. Check that `origin/main` has not moved (`git fetch origin`). It moved once during this session. If
   it has, rebase the stack.
3. Push each branch with `git push -u origin <branch>`. Never push `main`. This was authorised in the
   request. Use `--force-with-lease` only on these own branches, if they are re-pushed.
4. If `gh` is still missing, write each PR's text for the user and give compare URLs:
   `https://github.com/noortaytoy1/DistributedSystems/compare/<base>...<branch>?expand=1`. Each base is
   the previous branch in the stack (`main` for PR1); after each merge the next PR is retargeted.
   AGENTS.md requires these sections: **What and why** (with the `TODO(<part>)` markers resolved), **How
   it was tested** (including what could not be tested), **Contract changes**, plus the design-log IDs.
   Use the `mattpocock-skills:pr` skill for the body, and end with the attribution line above.

Reviewers by PR (AGENTS.md: review pair plus the owner of each touched file):

| PR | Reviewers | Depends on |
|----|-----------|------------|
| 1 | A↔D pair, E (planner, new `release.rs`), A | `main` |
| 2 | C↔F pair, A (the election fix in `raft/mod.rs`, D115) | 1 |
| 3 | C↔F pair, A (`config.rs`), B (default) | 2 |
| 4 | A↔D pair, C (`transport.rs`) | 3 |
| 5 | C↔F pair, A (`raft/`), **all owners** (`proto.rs`, new dependency) | 4 |
| 6 | C↔F pair, A, **all owners** (`proto.rs`) | 5 |
| 7 | B, C | 6 |
| 8 | Everyone | 7 |
| 9 | Everyone | 8 |

The final message to the user must state:

- which PRs are open and the order to merge them
- what passed locally
- what can only be checked on the six lab machines
- the exact commands to start testing
- what is still open

Also include the team's to-do list:

- Fill in the owners column.
- Write the report sections, including the contributions section and the chapter 2 RPC comparison.
- Run the experiments.
- Show Dr Amr the "no coordinator" argument (D116) and the degraded-mode trade-off before the demo.
- Clarify what he means by publish-subscribe and group communication in Phase 1, since the notes
  read as Phase 1 while the spec lists zone subscriptions under Phase 2. Same for the tarpc choice.
- Turn on branch protection.
- Get SSH keys across the lab machines.
- Remove the stale `TODO(B)` takeover marker, which is Abed's to decide.
