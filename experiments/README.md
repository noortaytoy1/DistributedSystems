# How to run the experiments

For the team in the lab, on the six PCs. Read section 1, do section 3, then Plan A (section 4) if you have SSH
or Plan B (section 5) if you do not. Then the tests in section 6. Section 7 says what to keep. The older reference on
cells and keys comes after this guide.

**What has been tried. Nothing here has run on the six PCs yet.** On 2026-11-04 this page's own lines were run on one PC, on
2 CPU threads, at the pushed commit `e4ab477`, with six folders on loopback standing in for six PCs. Expect small problems, and
fix this page when you hit one.

| Part | How far it went |
|------|-----------------|
| Plan A | The runner on `hosts.local`: `e1-30-bidding`, `e3e4-300-leader` and `degraded-300` with short settings, `e8-step-aside` at the plan's own timing. Then over a stand-in for ssh, with client 1 as `local`: `lab-build.sh`, the trial and a fault cell |
| Plan B | Six cells with 300 users and the real 75 s, :00 and :35: `e3e4-300-leader`, `degraded-300`, `e8-step-aside`, `e2e6-skew-expected-wait`, `e7-p1-300`, `e1-300-bidding`. Then `tar`, merge, `analyze.py`: all six in `summary.csv` |
| Section 6 | T5 as written. T1, T2 and T4 with 100 to 120 s of load and `kill -9` standing in for the power cut. T3's lines at 100 and 180 reports a second, flat out and the spike, all short |
| The whole series | Later that day, on one 32-thread PC: 18 cells (`quick`, `degraded-300`, `e1-1000-bidding`, `e8-step-aside`), 3 runs each, through the real runner on `hosts.local`. 54 of 54 runs finished, and `analyze.py` read them all. `quick` took 68.9 minutes, so the runner added 1 to 3 s a run. Also `--rate` up to 6000 a second with 3 programs, and 1000, 1500 and 3000 users |
| Never tried | Real SSH, a real network, a real power cut, `sudo`, `ufw`, `sysctl`, a USB stick, `--rate` with 9 programs or above 6000 a second, and `full` |

**One-PC rehearsal** marks numbers from one PC with one disk (commits `8bb244e` and `e4ab477`). Use them only as a guide to what is
normal. They are never results.

**Since `e4ab477` only the analysis, comments and docs have changed.** Nothing changes the wire format or what a node does (D229, D230, D231), so
logs from that commit on read the same and need no rerun.

## 1. How long

**No document sets a required duration.** The spec, the class notes and Lectures 8 and 9 give no number of minutes.

- The spec (lines 278 to 279) asks for data from tens of thousands of operations where feasible, so that the
  conclusions are statistically meaningful.
- The class notes (Sept 9) ask us to document how long the workload ran.

So the time is ours to choose and to state. Our own Phase 1 design said 10 runs of 5 minutes. D63 replaced that
with two profiles:

| What | Cells | Runs per cell | One run | Arithmetic | With ssh and log copying (estimate, about 20 s a run) |
|------|-------|---------------|---------|------------|------------------------------------------------------|
| `quick` | 15 | 3 | 15 s warm-up + 60 s measured + 15 s restart = 90 s | 15 x 3 x 90 s = 67.5 min | about 1 h 25 min |
| `full` | 34 | 5 | 30 s + 2 min + 15 s = 165 s | 34 x 5 x 165 s = 7 h 48 min | about 8 h 45 min |
| **The plan** (section 4) | 21: `quick` + 6 by name | 3 | 90 s, as `quick` | 21 x 3 x 90 s = **94.5 min** | **about 2 h** |

The 20 s is a guess. On one PC the runner added 1 to 2 s a run, but real ssh and log copying have not been timed.

What each cell type reaches, by arithmetic: users x 1 report a second x measured seconds x runs.
This is a ceiling, because each user also waits for its answer: one-PC rehearsal, with plans of about 2450 us, answered 290 of 300, 965 of 999 and
2849 of 3000 a second. The real count comes from the logs (section 7).

| Cell type | Users | One run of `quick` | `quick`, 3 runs | `full`, 5 runs | Tens of thousands? |
|-----------|-------|--------------------|-----------------|----------------|--------------------|
| `e1-30-*` | 30 | 1,800 | 5,400 | 18,000 | **No, in either profile.** Small on purpose: the idle end. Say so |
| `e1-100-*` (not in the plan) | 100 | 6,000 | 18,000 | 60,000 | **Not in 3 runs** |
| `e1-300-*`, `e2e6-*`, `e3e4-300-*`, `e4-300-*`, `e5-*`, `e7-p1-300`, `e8-step-aside`, `ft8-*`, `p2-*`, `degraded-300` | 300 | 18,000 | 54,000 | 180,000 | Yes |
| `e1-1000-*` | 1000 | 60,000 | 180,000 | 600,000 | Yes |
| `e3e4-1500-*`, `e4-1500-*`, `degraded-1500` | 1500 | 90,000 | 270,000 | 900,000 | Yes, but past what 300 responders can serve (D219) |
| `e1-3000-*` | 3000 | 180,000 | 540,000 | 1,800,000 | Yes |
| **All 21 cells of the plan** | | | up to about 2.7 million | | Yes |
| All 34 cells | | | | up to about 13 million | Yes |

Faults are far fewer than reports. A fault cell has one fault per run, so 3 per cell. `ft8-random-faults` has about 2
draws per run. These are small samples, and the report must say so.

**Recommendation, with the report due 2026-11-04 at 8 PM. Run the plan: `quick` plus 6 cells by name, about 2 hours of machine
time. Do not start `full`.** `full` takes about 8 h 45 min on PCs that have never run this, so one problem at hour 6 costs the
report. After the plan, do section 6, about 1 hour, and rehearse `docs/demo.md` once: its win counts and a page screenshot fill
report slots. If lab time is left, run the other 13 cells by name with the same profile (section 4, 58.5 minutes more).
If time runs short, use the short list in section 7.

**So, in the lab, by estimate:** section 3 about 30 minutes + the plan about 2 hours + section 6 about 1 hour + the analysis up
to 30 minutes = about 4 hours, before the demo rehearsal. In Plan B the 63 runs take a little over 3 hours by hand, so about 5 hours.

**What the report must then state** (D63):

- The profile of every figure: `quick`, 3 runs, 15 s warm-up, 60 s measured. This is below `full` and below the Phase
  1 design's 10 runs of 5 minutes.
- 3 runs give wide intervals: t is 4.30 with 3 runs and 2.78 with 5.
- How long in all: cells, runs and minutes really run, and the lab date.
- The real count of operations per cell and in all. `e1-30-*` is under tens of thousands, and why.
- One fault per run, so 3 per cell, and about 2 draws per run: too few to show a distribution.
- Which of the 34 cells did not run.
- The commit, the PCs' CPU, memory and disk, `plan_work` and the measured `service_mean_us`.
- The two responder pools: 3000 in `e1-1000-*` and `e1-3000-*`, 300 elsewhere. A line across that gap is not load
  alone, and the 1500-user cells run past their pool (D219).
- Anything that differs from this guide: a PC with no sudo, a changed `threads` or `plan_work`, Plan B's start by hand.
- Closed-loop figures as closed-loop. Every user does the same number of requests, so `done` can read the same on every PC and the
  interval on `throughput_rps` can read 0. That is arithmetic, not stability. About 21 ms of a bidding cell's roughly 30 ms is the fixed
  bid window. Near-identical runs say little: the skew cells show the load-awareness, and T3 shows what a closed loop hides.

## 2. Every requirement, and what answers it

T1 to T5 are the tests in section 6. "Spec 282" means line 282 of the project description's text, as in the report.
"Report" means `docs/report/main.tex`. M is its section "Measurement and experimental analysis", S is "All five
scenarios", D is "Decisions with justification and assessment".

| # | Requirement | Source | Answered by | Metric in `analysis/out/summary.csv`, or output to keep | Report |
|---|-------------|--------|-------------|-------------------------------------------------------|--------|
| 1 | Request latency: mean, median, p95, p99 | Spec 282 | `e1-*`, and every cell | `mean_ms`, `p50_ms`, `p95_ms`, `p99_ms`; plot `e1_latency_throughput.png` | M: Latency |
| 2 | Throughput as clients increase | Spec 283, 231 | `e1-30-*`, `e1-300-*`, `e1-1000-*`, `e1-3000-*`; T3 | `throughput_rps` beside `answered_with_responder_rps`, which leaves out answers that sent nobody, `no_responder_share`, `gave_up`, `client_pc_cpu_busy_p95`; same plot | M: Throughput against load; Stress testing: What breaks next |
| 3 | Election frequency, duration, message overhead | Spec 284 | Bidding: any bidding cell. Raft: `e5-faults-off`, `e3e4-*-leader`. Draw: `ft8-*` | Bidding: `bid_rounds`, `bid_window_mean_ms`, `bid_split_decisions`, `msgs_bid_per_req`. Raft: `elections`, `election_duration_mean_ms` (the vote alone), `msgs_raft_vote_per_req`. Draw: `fault_elections`, `fault_rounds_abandoned`, `fault_election_mean_ms`, `fault_bids_per_round`. All: `msgs_all_per_req` | M: Election overhead |
| 4 | Load distribution, fairness, evidence for the policy | Spec 285, 358 | `e1-*-bidding` against `e1-*-leader`; `e2e6-skew-*` | `share_node_1`, `share_node_2`, `share_node_3`, `fairness_jain`; plot `fairness.png` | M: Load distribution |
| 5 | Before, during and after a failure | Spec 286 | `e3e4-*`, `e4-300-silence-20s`, `degraded-300` | `before_p99_ms`, `during_p99_ms`, `after_p99_ms`, and their `_mean_ms`, `_p50_ms`, `_p95_ms`, `_rps`, `_count`; plot `fault_phases.png`. In `degraded-*` these mix provisional answers with committed ones, so say that `during_*` is mostly uncommitted | M: Failure and recovery |
| 6 | Failover time at different loads | Spec 167 | `e3e4-300-*` against `e3e4-1500-*` | `failover_to_leader_ms_mean`, `failover_to_commit_ms_mean`, `failovers_without_leader`; `client_gap_mean_ms`, `client_gap_max_ms`, `worst_request_ms`, `detection_mean_ms`, read with `gave_up`. Not `election_duration_mean_ms`: that is the vote alone, about 10 ms of a wait near 700 ms (D229) | M: Failure and recovery |
| 7 | Recovery and convergence time at different loads | Spec 287, 167, 152; notes Sept 13, Oct 28 | `e3e4-*`, `e4-300-silence-20s` | `recovery_mean_ms`, `rejoins`, `rejoins_digest_confirmed`, `digest_mismatches` (must be 0), `elections_after_rejoin` (target 0) | M: Failure and recovery; How a node recovers |
| 8 | Requests in flight at a failure: retried or completed | Spec 158 | `e3e4-*` | `retried_share`, `gave_up`, `takeovers_suspected`, `takeover_wait_suspected_mean_ms`, `takeovers_backstop` | M: Failure and recovery |
| 9 | Duplicate execution prevented or harmless | Spec 161 | Every cell | `digest_mismatches` (must be 0), `bid_split_decisions`, `takeovers` | D: First commit wins |
| 10 | CPU, memory, network, queues | Spec 295, 224 | `e1-*-bidding` | `cpu_cores_mean`, `rss_mb_max`, `net_bytes_per_s_mean`, `queued_mean`, `queued_max`, `running_mean`, `plans_refused`; client PCs: `client_pc_cpu_busy_p95`, `client_pc_mem_available_min_mb`; saves and disk: `extras.py`. **Nothing covers the client PCs' network** | M: CPU, memory, network and queues |
| 11 | Tens of thousands of operations | Spec 278 | Every cell but `e1-30-*` | `count` + `provisional_count` + `gave_up`, times `runs`: command in section 7 | M: Raw data |
| 12 | Repeated runs, statistically meaningful | Spec 66, 231, 279 | 3 runs per cell | Columns `runs` and `ci95_half_width` | M: Statistics; Limitations |
| 13 | How long, which workloads, how evaluated | Notes Sept 9 | Section 1 | `warmup_s`, `measured_s`, `users`, `profile` in `run.json` | M: What has been run so far; Method |
| 14 | Automated data collection | Spec 278, 307 | Plan A. In Plan B the logs are automatic and the start is by hand: say so | Every log, `run.json`, `summary.csv` | M: Method |
| 15 | Raw data, not only screenshots | Spec 279, 363 | `runs/<experiment>/<cell>/run-<n>/` | The whole folder, kept outside git | M: Raw data |
| 16 | Measurements tied to design decisions | Spec 280 | All of the above | The report's table, with its numbers filled in | M: Each measurement against the decision it tests |
| 17 | **Scenario 1**, normal: different peers elected as load changes | Spec 244; notes Oct 28; Lecture 9 s1.2 | `e1-30-bidding`, `e1-300-bidding`; `docs/demo.md` section 1 | `share_node_1`, `share_node_2`, `share_node_3`; win counts from `grep -c '"i_won":true'`; a screenshot of the page | S: Scenario 1 |
| 18 | **Scenario 2**, load imbalance | Spec 247 | `e2e6-skew-expected-wait`, `e2e6-skew-expected-wait-cpu`; `docs/demo.md` section 2 | `share_node_1`, `fairness_jain`, `p99_ms`; win counts before and after the skew | S: Scenario 2 |
| 19 | **Scenario 3**, server failure under load | Spec 250 | `e3e4-*-leader`; `docs/demo.md` section 3 | `failover_to_leader_ms_mean`, `client_gap_max_ms`, `worst_request_ms`, `during_p99_ms`, `gave_up` | S: Scenario 3 |
| 20 | **Scenario 4**, recovery before taking part again | Spec 254, 152 | `e3e4-*`, `e4-300-silence-20s`; `docs/demo.md` section 4 | `recovery_mean_ms`, `rejoins_digest_confirmed`; the `rejoined` line | S: Scenario 4 |
| 21 | **Scenario 5**, scale and stress: clients bombarding the cloud | Spec 272; Lecture 9 s1.2 | `e1-*`; T3 | Rows 1 to 10; the three plots | S: Scenario 5; Stress testing |
| 22 | Six physical PCs on a real network: 3 cloud, 3 load | Spec 30, 202; Lecture 8 s1.2; notes Sept 13 | Every run in this guide | `config.toml`, `node_logs`, `udp_drops_nodes`, `udp_copies_lost_share` (never `udp_request_loss_share`: it reads about 1% with nothing lost, D229), `retried_share` | M: Method; D: UDP |
| 23 | Hundreds of clients on each client PC | Lecture 8 s1.2; spec 31, 203 | Per PC: `e1-300-*` 100, `e1-1000-*` 333, `e3e4-1500-*` 500, `e1-3000-*` 1000 | `client_users`, `client_programs`, `users_per_program`. `e7-p1-300` shows if the number of programs matters. Limit: users share 10 programs per PC (D181) | D: Many user programs |
| 24 | A frontend controlling many background programs | Spec 203 | `docs/demo.md`, the page's load box. Stop it before the runner | A screenshot; the rows' summaries | D: The web page |
| 25 | Failure by election: the servers elect one to go silent for up to 20 s, then it rejoins | Spec 149; notes Sept 13, Oct 28 | `ft8-random-faults` (silence 1 to 20 s), `ft8-election-rate` (fast); T4 | `fault_elections`, `fault_split_decisions` (must be 0), `fault_elected_not_silenced`, `fault_refused`; the round lines from `extras.py` | D: The failure draw |
| 26 | Injected failures up to 20 s, service continues | Spec 227 | `e4-300-silence-20s`, `degraded-300` | `gave_up`, `client_gap_max_ms`, `worst_request_ms`, `during_p99_ms` | M: Failure and recovery |
| 27 | **Physically turning off a machine** | Notes Oct 28, Sept 13 | T1. **No cell covers it.** The runner cannot, as a lost PC stops it | `restarted`, `rejoined`, `peer_suspected` lines; `extras.py` output; your written times | S: Scenario 3, scene 3b; D: Saving to disk; How a node recovers |
| 28 | **One node off and one failed: one server handles the three client PCs** | Notes Sept 13 | `degraded-300` (two silenced); T2 (one really off) | `degraded_entered`, `degraded_mean_ms`, `provisional_replied`, `provisional_count`, `provisional_p50_ms`, `provisional_conflicts`, `conflicts_lost_first_commit`, `conflicts_responder_taken`; the `provisional` count on each client PC's last line | S: Scenario 3, scene 3c; D: Degraded mode; M: Failure and recovery |
| 29 | **5 retries, then an error** | Notes Sept 13; spec 306 | T5. **No cell covers it**, as in a cell some node always answers | The `client_gave_up` line with `"attempts":6`; `gave_up`, `retried_share` per cell | D: Five retries, then an error |
| 30 | Transparency: the user never names a server, and is sent to a working one | Notes Sept 6; spec 41; Lecture 9 s4.1 | `e3e4-*-leader`; `docs/demo.md` section 3 | `gave_up`, `client_gap_max_ms`, `retried_share`; a page card | S: Scenario 3 |
| 31 | The nodes are peers, with no coordinator | Notes Sept 13, Oct 28; Lecture 9 s3.4 | `e1-300-bidding` against `e1-300-leader`; T4 | `leader_plan_share`, `share_node_1`, `share_node_2`, `share_node_3`; rounds called by a follower | M: Load distribution; D: The failure draw |
| 32 | Group membership changes as nodes fail | Lecture 8 s9.1; Lecture 9 s2.2 | `e3e4-*`; `docs/demo.md` section 3 | `members` in `status-after.txt`; `group_view` events; `msgs_bid_per_req` | D: The group with a who's-alive list |
| 33 | The Raft leader changes when it is overloaded | No written source: a teammate's report of what was said in class. Noor decided yes (D217, D218) | `e8-step-aside`, against `e2e6-skew-expected-wait`; `docs/demo.md` section 2b | `elections`, `failover_to_leader_ms_mean`, `commit_p99_ms`, `p99_ms`; the lines from `grep -h '"ev":"leader_step' runs/e8/step-aside/run-*/node-*.jsonl`. Only runs where node 1 led count | "The whole system in plain words": its note still says this is not in. Rewrite it, and add it to "Changed decisions". State that it has no written source |

**What nothing covers, or only a hand test covers:**

- A real power cut: only T1. It logs no `silenced`, so `failover_to_*`, `client_gap_*`, `before_*`, `during_*` and `after_*` stay empty.
  `extras.py` gives the gap instead. Only this test can lose unsynced disk writes: one-PC rehearsal never could.
- Where the system falls: only T3. The cells stop at 3000 users, which one-PC rehearsal held only with plans of about 2450 us (section 3).
- Stale plans per commit and commands per save: not in `summary.csv`. `extras.py` prints them.
- The client PCs' network: nothing measures it.
- Sharing of the draw between nodes: too few rounds on real timers to say anything.
- A network cut that heals. `ctl silence` is a fair stand-in for a cut, with one exception it cannot show: for about half a second after
  the cable returns, the cut-off node is still degraded, answers in about 4 ms, beats the committed path, and clients take that answer
  as final. On one PC over a virtual cable, twice: 225 to 276 such answers, 125 to 136 of them lost first commit. Say so if the report
  discusses a network cut. A power cut restarts the node, so it should not show this.
- Arrivals after a leader fault. Steady users have no jitter, so they stay bunched for the rest of the run: on one PC
  `arrival_peak_100ms_share_after_fault` read 0.35 against 0.13 before it. Read `after_*` as measured under clumped arrivals.

**Phase 2, left out on purpose.** Four scenarios: peer discovery, direct P2P exchange, offline peer, conflicting claim
(spec table, lines 242 to 274). Three measurements: discovery query latency and presence-table convergence; P2P delivery latency,
duplicate rate and forwarding overhead; offline reconciliation delay and conflicts (spec 288 to 290). Publish-subscribe
(Lecture 9 s2.1). The report says so in "What is left for Phase 2".

## 3. Before you start

About 30 minutes. Do these in order. No PC may sleep during a batch.

**On all six:**

```bash
cd ~/DistributedSystems
git switch main && git pull --ff-only     # once the PR with this guide is merged. Until then: git switch shared-phase1-gaps && git pull --ff-only
ls experiments/e8-step-aside.toml   # must exist. If not, this PC is on a commit from before this guide: all six could still agree on it
git rev-parse HEAD                  # must print the same on all six. Mixed builds look like dead nodes
unset CARGO_TARGET_DIR              # the runner and hand.sh look in target/release. See below
cargo build --release --bins        # after every pull. Several minutes per PC
timedatectl                         # "System clock synchronized: yes". With chrony: chronyc tracking
date +%s                            # run together on all six: within about 1 s of each other
df -T .                             # a local disk. Never nfs: a shared home breaks every run. Clone to a local folder
df -h .                             # free space: see the table below
ping -c 5 <a node PC's address>     # write the time down for the report
lscpu; free -h; lsblk -d -o NAME,ROTA,SIZE                       # write down for the report
pgrep -af '[t]arget/release/(node|loadgen|frontend|zonefeed)( |$)'    # must print nothing. If not: kill <PID>
```

- **`CARGO_TARGET_DIR`.** `AGENTS.md` sets it for WSL. Set on a PC here, the build lands elsewhere, and the runner prints
  `target/release/ctl: No such file or directory`, then `no leader after 30 s` and `run dropped and batch stopped`. Nothing is wrong
  with the leader. `start-users.sh` reads the variable, but the runner always passes its own `BIN`. Unset it on all six and build
  again. To keep it for Plan A, set `BIN=$CARGO_TARGET_DIR/release` for the runner as well. `hand.sh` names `target/release` itself.
- **Never pipe `ctl` into `head`,** or into anything that stops reading early. `ctl` then panics with `failed printing to stdout`,
  which looks like a crash and says nothing about the cluster. Let it finish: `ctl status | sed -n 1,5p`, or send it to a file.

| PC | Free disk | Why |
|----|-----------|-----|
| The PC that collects and analyses | 30 GB | The plan is about 9 GB and section 6 about 10 GB. A report costs about 2.8 KB of logs across all PCs (one-PC rehearsal). A 300-user run is about 60 MB, a 3000-user run about 600 MB. `full` would be about 46 GB |
| Every other PC | 10 GB | Plan B keeps logs on each PC until the end. Node PCs hold most of it: on one PC a 300-user run left about 16 MB per node and 3 MB per client PC |

**On each PC with sudo:**

```bash
sudo sysctl -w net.core.rmem_max=8388608
echo 'net.core.rmem_max=8388608' | sudo tee /etc/sysctl.d/90-rescuemesh.conf     # survives the power cut in T1
```

**On the three node PCs:**

```bash
sudo ufw allow 7001/tcp; sudo ufw allow 8001/udp; sudo ufw allow 8001/tcp
mkdir -p data/raft && dd if=/dev/zero of=data/raft/.t bs=512 count=1000 oflag=dsync && rm data/raft/.t
nproc
```

- `dd` prints how long 1000 synced writes took. Write it down per node PC. One-PC rehearsal synced in 2.5 to 4.7 ms under
  load, so about 3 to 5 s here (idle, that PC's `dd` took 2.3 s). A slower disk still works, because saves are grouped, and shows
  up as commit latency. Past about 15 s here, 15 ms a save, the log reaches its limit at 3000 users, by arithmetic: commits take
  seconds with nothing wrong, and that is the disk (D228). Say so in the report.
- `nproc` must be at least 5. A node has 4 planner threads and needs one core free (D48). With fewer cores, give it
  cores minus 1, in every cell, on each PC that reads the cells (Plan A: the runner's PC. Plan B: all six), and say so
  in the report. For 4 cores: `sed -i 's/^plan_work = 1000$/&\nthreads = 3/' experiments/*.toml`

**A PC with no sudo: make it a client PC, and skip the sysctl and the firewall lines.** A client socket only reads
replies. In one-PC rehearsal, with the same capped buffer, client sockets held at most 1792 bytes and dropped nothing.

- `analyze.py` will warn about a capped buffer in every cell, and `udp_rcvbuf_got_min_bytes` reads about 425984. That is
  expected. Check that `udp_drops_client_pc_max` is 0, and say it in the report.
- It cannot install anything, so check `which cc rsync`. No `cc`: copy `target/release/loadgen` and `target/release/ctl`
  from a PC with the same Ubuntu and the same commit. No `rsync`, or no ssh server: use Plan B.
- It cannot set the clock. If `timedatectl` says no, compare `date +%s` by hand, and say so in the report.
- With more than three such PCs, a node PC must run with the capped buffer. Steady load lost nothing that way in one-PC
  rehearsal, and `udp_drops_nodes` says if it does here. Its firewall must already be open: `ctl status` shows that.

**With the nodes running (start them as in runbook section 3, with `config/lab.toml` from its section 2), from each of the
three client PCs.** The runner makes this check too, from client PC 1 only, and stops on a failure.

```bash
target/release/ctl --config config/lab.toml status
```

You want one `leader`, and under the table `node 1: udp ok`, `node 2: udp ok`, `node 3: udp ok`.
`udp no answer` under a healthy row means `8001/udp` is closed on that node, and every report to it vanishes.
Stop those nodes again before you go on.

**Calibrate `plan_work`, once, after the first 300-user run** (Plan A step 3, or Plan B run 1 of `e1-300-bidding`). Python
setup is at the end of section 5. In Plan B, first collect that one run on one PC with the USB lines in section 5. One PC's
folder alone gives only `left out ... 10 of 30 client programs sent anything` and `no runs found under runs`.

```bash
python3 analysis/analyze.py runs
grep -E '^e1,300-bidding,(service_mean_us|elections|reports_expired|udp_rcvbuf_got_min_bytes|udp_drops_nodes|net_bytes_per_s_mean|no_responder_share),' analysis/out/summary.csv
grep -h '"ev":"udp_socket"' runs/e1/300-bidding/run-1/node-*.jsonl     # each node: "rcvbuf_got_bytes":16777216
```

| Metric | You want | If not |
|--------|----------|--------|
| `service_mean_us` | About 4000: one plan's time, in microseconds (D129). One-PC rehearsal: about 2450. Read it from a bidding cell, as it is NaN in leader cells, which have no bids. Above about 2700, read the note under this table | Our rule of thumb, as no document gives a limit: 2000 to 8000 is fine up to 1000 users, so keep `plan_work = 1000` and state the number. It is not fine at 3000 users: on one PC that cell fell at about 3050 us, so choose as the note under this table says. Outside that range, N = 1000 x 4000 / `service_mean_us`, on each PC that reads the cells: `sed -i 's/^plan_work = 1000$/plan_work = N/' experiments/*.toml`, `sed -i '1s/skew_work=5000/skew_work=5N/' experiments/e2e6-*.toml` and `sed -i '1s/skew_work=20000/skew_work=20N/' experiments/e8-step-aside.toml`, with 5N and 20N worked out. Say so in the report |
| `elections` | 0 | The leader is not holding. Check `nproc` and the disk before going on (D133, D48) |
| `reports_expired` | 0 | Most likely a stamping or clock problem (D147). Do not go on |
| `udp_rcvbuf_got_min_bytes` | 16777216. Nobody has seen this on a raised PC yet | About 425984 means a PC missed the sysctl. Fine only if that PC has no sudo and is a client PC |
| `udp_drops_nodes` | 0 | Buffer too small: check the sysctl on the node PCs |
| `net_bytes_per_s_mean` | Above 0. Nobody has seen it on a real network yet: loopback is left out, so it reads 0 on one PC | 0 means this figure is broken on these PCs. Say so in the report |

**Read `service_mean_us` before the series: it says where the top cells will land.** One-PC rehearsal, `e1-3000-bidding` by the cost of a
plan, one short run each except the first (no new leader in any of them):

| `service_mean_us` | Answered a second | p50, ms | p99, ms | Retried |
|-------------------|-------------------|---------|---------|---------|
| 2498 | 2868 | 41 | 100 | 0 |
| 2728 | 2858 | 43 | 109 | 0 |
| 3054 | 2292 | 96 | 2481 | 6.7% |
| 3365 | 2082 | 155 | 2689 | 11% |
| 3778 | 1873 | 181 | 6191 | 15% |
| 4080 | 1762 | 257 | 6502 | 17% |

The cell held at 2728 us and **fell at about 3050 us. Above about 2700, expect the 3000-user cells past the knee. That is the knee, not a bug:** the
planners run out, and plans wait and retry. The number moves with the PC's speed, and at D129's own target of 4000 that cell has fallen. 1000 users at
4004 us were still fine, p99 56 ms, so the lower cells still compare the policies. **The team must choose `plan_work` before the series,** and say in the
report which it chose:

- Keep `plan_work = 1000` and report the knee. Our recommendation: it needs no edit, and the sweep then shows where latency turns up. E1's two
  policies are both past the knee at 3000, so compare them at 1000 and below.
- Lower it so that the top cell holds: N = 1000 x 2450 / `service_mean_us`, with the `sed` lines in the table above.

## 4. Plan A: with SSH

You need, from one PC: `ssh` with no password to all six (`ssh-copy-id`, then test with `ssh -o BatchMode=yes <target> true`),
`rsync` on all six, and the repo at `~/DistributedSystems` on all six (or set `REMOTE_DIR`). The runner's PC can be client PC 1.

**Step 1. The hosts file.** On the runner's PC only:

```bash
cp experiments/hosts.example experiments/hosts
nano experiments/hosts
```

```text
# role   id  address     ssh target
node     1   10.0.0.11   ubuntu@10.0.0.11
node     2   10.0.0.12   ubuntu@10.0.0.12
node     3   10.0.0.13   ubuntu@10.0.0.13
client   1   10.0.0.21   local
client   2   10.0.0.22   ubuntu@10.0.0.22
client   3   10.0.0.23   ubuntu@10.0.0.23
```

Real addresses. Nodes stay in order 1, 2, 3. Each client has its own number. `local` means "this PC".

**Step 2. One build on all six.** It does one PC after another, and stops if a commit differs. The commit must be on
`origin`, as the far PCs fetch it. If `git fetch` needs a password there, pull and build by hand on each PC instead.

```bash
scripts/lab-build.sh experiments/hosts          # ends: every machine is built from <commit>
```

**Step 3. One-cell trial, then one real cell.** About 3 minutes in all.

```bash
WARMUP=5 MEASURED=20 REPS=1 scripts/run-experiments.sh experiments/hosts quick e1-30-bidding
```

During it, on a client PC, `pgrep -cf '[t]arget/release/loadgen'` must print 10. After it, on the runner's PC,
`runs/e1/30-bidding/run-1` must hold 3 `node-*.jsonl`, 30 `client-*.jsonl` and `run.json`, and
`grep -c '"ev":"stopped"' runs/e1/30-bidding/run-1/node-*.jsonl` must print 1 for each node.

```bash
REPS=1 scripts/run-experiments.sh experiments/hosts quick e1-300-bidding
```

Then calibrate, as in section 3. Later runs of these two cells move these folders to `runs/old/`, so they do not mix.

**Step 4. `quick`: 15 cells, 3 runs each, 67.5 minutes by arithmetic.** Nobody touches the six PCs during a batch.
Stop every frontend first: the runner refuses to start beside one.

```bash
nohup scripts/run-experiments.sh experiments/hosts quick >quick.log 2>&1 &
tail -f quick.log                               # Ctrl-C stops the tail, not the batch. It ends: done: logs are under runs/
```

**Step 5. What `quick` leaves out and the requirements need.** When step 4 has ended. Same profile, by name: 6 cells, 27 minutes.

| Cell | Why it is needed |
|------|------------------|
| `degraded-300` | One server left for the three client PCs (notes, Sept 13). No `quick` cell has two nodes down |
| `e1-1000-bidding`, `e1-1000-leader` | The middle load step (spec 231). `quick` jumps from 300 to 3000, and the point where latency turns up can hide in that gap |
| `e4-300-silence-20s` | The full 20 s (spec 151, 227). The ordered faults in `quick` last 15 s |
| `e7-p1-300` | Does the number of programs change anything (D181)? The report has a slot for it |
| `e8-step-aside` | A busy leader moves by a normal vote (row 33, D218). The only cell with this on. It cannot move in the first 65 s after the leader starts, so with the runner the move comes near the end of the 75 s, and only about 8 s of the run follows it: state that beside any "after" figure. One PC, one run: 68 s after the leader started, a new leader in 805 ms, gave up 0 |

```bash
nohup scripts/run-experiments.sh experiments/hosts quick degraded-300 e1-1000-bidding e1-1000-leader e4-300-silence-20s e7-p1-300 e8-step-aside >extra.log 2>&1 &
```

**Step 6. Check.**

```bash
python3 analysis/analyze.py runs 2>&1 | tee analysis.log
grep 'left out' analysis.log                                     # names any run it did not use, and why. Rerun those
awk -F, '$3=="count" && $4!=3' analysis/out/summary.csv          # cells without 3 runs. Rerun those by name
grep -L '"measured_s":60' runs/*/*/run-*/run.json                # a trial still there. Must print nothing
grep -E ',(digest_mismatches|fault_split_decisions|reports_expired),' analysis/out/summary.csv | awk -F, '$5!=0'    # must print nothing
ls runs/dropped 2>/dev/null                                      # runs that never got their fault. Rerun by name
```

A batch can stop on a real fault. It says why, and [runbook section 7](../docs/runbook.md) has every message. Earlier
runs are kept. Rerun only what is missing, by name. To see what is missing:

```bash
for c in $(cat experiments/quick.list) degraded-300 e1-1000-bidding e1-1000-leader e4-300-silence-20s e7-p1-300 e8-step-aside; do
  n=$(ls runs/${c%%-*}/${c#*-}/run-*/run.json 2>/dev/null | wc -l); [ "$n" = 3 ] || echo "$c has $n of 3"
done
```

**E5 needs no extra cell.** The spec asks for election frequency, duration and overhead (spec 284, row 3), and not for faults on against
off, so `e5-faults-on` stays in the spare list below. It differs from `ft8-random-faults`, which is in `quick`, only in its draw timers (30 to 60 s against
20 to 30 s) and a fixed seed, so `e5-faults-off` against `ft8-random-faults` makes the same comparison. Say so in the report's E5 row (D230).

**If lab time is left: the other 13 cells, by name, 58.5 minutes.** The report's "(full)" rows wait for these. Best first:

```bash
nohup scripts/run-experiments.sh experiments/hosts quick e2e6-skew-queue-only e2e6-skew-cpu-only e4-300-silence-3s e1-100-bidding e1-100-leader e4-1500-silence-3s e4-1500-silence-20s degraded-1500 e5-faults-on p2-window-5ms p2-window-10ms p2-window-20ms p2-window-50ms >more.log 2>&1 &
```

Only if every report slot is filled and the PCs stay free for 8 h 45 min: `full`. It moves the run folders of the cells it
repeats to `runs/old/`, where the analysis does not read them, so first copy `runs/` aside, and state each figure's profile.

## 5. Plan B: no SSH, by hand

One person per PC, or one person walking. A run takes about 3 minutes by hand, so 63 runs take a little over 3 hours.
**Start with the short list in section 7** (39 runs, about 2 hours), then the rest.

**Once.** Write `experiments/hosts` as in Plan A, with `-` as every ssh target, and save the block below as `hand.sh`.
Copy both to all six by USB: `experiments/hosts` and `~/hand.sh`. Then on all six do section 3.

```bash
# hand.sh: the runner's steps, with no SSH. In each terminal: cd ~/DistributedSystems; source ~/hand.sh
WARMUP=15 MEASURED=60 PROFILE=quick
ctl() { target/release/ctl --config runs-config/run.toml "$@"; }

# prepare <cell>: stop if a program or an unsaved log is left, wipe, write the file the runner writes.
prepare() {
  pgrep -af '[t]arget/release/(node|loadgen|frontend|zonefeed)( |$)' && { echo "STOP: kill those PIDs"; return 1; }
  [ -z "$(ls -A runs/current 2>/dev/null)" ] || { echo "STOP: runs/current has logs: keep them or delete them"; return 1; }
  rm -rf runs/current data/raft
  mkdir -p runs/current runs-config
  rm -f runs-config/*.pid runs-config/*.programs
  ( source scripts/lab-common.sh; read_hosts experiments/hosts; nodes_toml
    printf '\n[metrics]\ndir = "runs/current"\n\n[storage]\ndir = "data/raft"\n\n'
    cat "experiments/$1.toml" ) >runs-config/run.toml
  sha256sum runs-config/run.toml | cut -c1-12     # must read the same on all six
}

# keep <cell> <run> [root]: after everything stopped, move this PC's logs into the run's folder.
keep() {
  local out="${3:-runs}/${1%%-*}/${1#*-}/run-$2"
  mkdir -p "$out"; mv runs/current/* "$out"/
  cp runs-config/run.toml "$out"/config.toml; git rev-parse HEAD >"$out"/commit.txt
}

# runjson <cell> <run>: on client PC 1 only, after keep. The keys the runner writes, from the cell's first line.
runjson() {
  ( source scripts/lab-common.sh; read_hosts experiments/hosts
    cell=$1 run=$2
    key() { local v; v=$(sed -n '1s/^# run://p' "experiments/$cell.toml" | tr ' ' '\n' | sed -n "s/^$1=//p"); echo "${v:-$2}"; }
    users=$(key users 30) programs=$(key programs 10) expected=0
    for i in 0 1 2; do s=$(users_on_machine $i "$users"); expected=$((expected + (s < programs ? s : programs))); done
    printf '{"experiment":"%s","cell":"%s","run":%d,"profile":"%s","warmup_s":%d,"measured_s":%d,"commit":"%s","users":%d,"programs_per_pc":%d,"client_programs_expected":%d,"pattern":"steady","think_ms":%d,"victim":"%s","nodes":3}\n' \
      "${cell%%-*}" "${cell#*-}" "$run" "$PROFILE" "$WARMUP" "$MEASURED" "$(git rev-parse HEAD)" "$users" "$programs" "$expected" "$(key think_ms 1000)" "$(key victim none)" \
      >"runs/${cell%%-*}/${cell#*-}/run-$run/run.json" )
}

# handjson <cell> <run> <warmup_s> <measured_s> <users> <programs>: run.json for a test in section 6.
handjson() {
  printf '{"experiment":"%s","cell":"%s","run":%d,"warmup_s":%d,"measured_s":%d,"users":%d,"client_programs_expected":%d,"victim":"none","nodes":3}\n' \
    "${1%%-*}" "${1#*-}" "$2" "$3" "$4" "$5" "$6" >"runs/hand/${1%%-*}/${1#*-}/run-$2/run.json"
}
```

`prepare` writes the same bytes as `write_config` in `scripts/run-experiments.sh`: the node list from `nodes_toml`, then
`metrics.dir = "runs/current"` and `storage.dir = "data/raft"`, then the cell file. It wipes `runs/current` and
`data/raft`, as the runner does between runs. All six use one file, or Raft membership breaks.

**First run: `e1-300-bidding`, run 1. Then collect it on one PC (the USB lines below) and calibrate (section 3).** If its checks
pass and `plan_work` stays as it is, it counts as run 1. If you change `plan_work`, delete that run's folder on all six and run it again.

**Each run, in this order.** The example is `e3e4-300-leader`, run 1.

```bash
# ALL SIX, in each terminal: cd ~/DistributedSystems; source ~/hand.sh
CELL=e3e4-300-leader RUN=1
prepare $CELL                       # the 12 characters it prints must match on all six

# NODE PC N. N is 1, 2 or 3, its number in the hosts file
N=1
nohup target/release/node --config runs-config/run.toml --id $N >runs/current/node-$N.out 2>&1 &
echo $! >runs-config/node-$N.pid

# CLIENT PC 1
ctl status                          # one leader, and three "udp ok". Repeat until it is so
ctl skew --node 1 --work 5000       # only for e2e6-skew-* cells

# CLIENT PC M. M is 1, 2 or 3. U and P from the table below. It waits, and starts when the clock reads :00
M=1 U=100 P=10
sleep $((60 - $(date +%s) % 60)); bash scripts/start-users.sh --config runs-config/run.toml --machine $M \
  --users $U --programs $P --pattern steady --think-ms 1000 --duration-s 75 --warmup-s 15 \
  --seed-base $((RUN * 1000000)) --out runs/current

# CLIENT PC 1, A SECOND TERMINAL, fault cells only. Type this once the load has started, after :00 and before :35.
# Typed before :00, it can fire before the load. It waits, and fires when the clock reads :35 (see the table)
cd ~/DistributedSystems; source ~/hand.sh
sleep $(( (95 - $(date +%s) % 60) % 60 ))
ctl silence --node $(ctl status | awk 'NR>1 && $2=="leader" && !id {id=$1} END {print id}') --ms 15000

# WHEN ALL THREE CLIENT PCs HAVE PRINTED "reported 10 of 10 programs" (1 of 1 for e7-p1-300): CLIENT PC 1
ctl status | tee runs/current/status-after.txt       # equal digests in all three rows, once no row says silenced true

# NODE PC N
kill $(cat runs-config/node-$N.pid)

# ALL SIX, once the nodes have gone (under 1 s on one PC; pgrep -af '[t]arget/release/node' prints nothing)
keep $CELL $RUN

# CLIENT PC 1
runjson $CELL $RUN
```

`--duration-s` includes the warm-up: 15 + 60 = 75. All three client PCs must be waiting before the clock reads :00. If one
misses it, press Ctrl-C on all three. If none had started yet, type the start line again. If any had started, throw the run
away: their logs in `runs/current` would add up with the next try's. After a good run, wait 15 s, and start the next with `prepare`.
To throw a run away: `kill` the nodes, then `rm -rf runs/current` on all six, and start again at `prepare`. To redo a run you have
already kept, first delete its folder on all six, because `keep` merges into a folder that exists.

Everything a run needs is on the first line of its cell file: `head -1 experiments/$CELL.toml`. For a cell not in this table, share
its `users` over the three PCs, the first PCs taking what is left over: 100 is 34, 33, 33. `victim` and `silence_ms` give the fault.

| Cell | `--users` on client PC 1, 2, 3 | `--programs` | Before the load | At :35 on client PC 1 |
|------|--------------------------------|--------------|-----------------|-----------------------|
| `e1-30-*` | 10, 10, 10 | 10 | | |
| `e1-300-*`, `e5-faults-off` | 100, 100, 100 | 10 | | |
| `ft8-*` | 100, 100, 100 | 10 | | Nothing. The nodes draw their own faults |
| `e1-1000-*` | 334, 333, 333 | 10 | | |
| `e1-3000-*` | 1000, 1000, 1000 | 10 | | |
| `e2e6-skew-*` | 100, 100, 100 | 10 | `ctl skew --node 1 --work 5000`, or your `skew_work` | |
| `e3e4-300-leader` | 100, 100, 100 | 10 | | Silence the `leader`, `--ms 15000` |
| `e3e4-300-follower` | 100, 100, 100 | 10 | | Silence a `follower`, `--ms 15000` |
| `e3e4-1500-leader` | 500, 500, 500 | 10 | | Silence the `leader`, `--ms 15000` |
| `e3e4-1500-follower` | 500, 500, 500 | 10 | | Silence a `follower`, `--ms 15000` |
| `e4-300-silence-20s` | 100, 100, 100 | 10 | | Silence a `follower`, `--ms 20000` |
| `degraded-300` | 100, 100, 100 | 10 | | Silence two nodes, `--ms 20000`: command below |
| `e7-p1-300` | 100, 100, 100 | 1 | | |
| `e8-step-aside` | 100, 100, 100 | 10 | `ctl skew --node 1 --work 20000`, or your `skew_work` | Nothing. The leader moves by itself. The wait for :00 uses up part of its 65 s, so it moves sooner than with the runner: 26 s into the load on one PC. That run answered 170 a second, not 281 as with the runner, most likely because node 1's slow plans took the CPU the other nodes shared with it on one PC. Both moved once, gave up 0, equal digests |

:35 is the warm-up plus a third of the measured time: 15 + 20 s. If `ctl silence` prints a timeout, or an error about `--node`,
press the up arrow and run that one line again at once. Never the `sleep` line: it would wait for the next :35, after the load has
ended. `already silenced` means the first one worked (D74). For a follower, write `$2=="follower"` in the command above. For
`degraded-300`, leave one follower alone and silence the other two, the leader among them. Again, type it after :00 and before :35:

```bash
sleep $(( (95 - $(date +%s) % 60) % 60 ))
F=$(ctl status | awk 'NR>1 && $2=="follower" && !id {id=$1} END {print id}')
[ -n "$F" ] && for v in 1 2 3; do [ "$v" = "$F" ] || ctl silence --node $v --ms 20000; done
```

**After the last run: collect by USB.** Logs compress about 10 to 1 (12 to 1 on one PC), so the plan and section 6 come to about
2 GB on the stick, by arithmetic. Each PC's run folders go to the same path, so the files of one run land in one folder.

```bash
# EACH OF THE SIX. Give each PC its own NAME. /media/$USER/USB is wherever your USB stick mounts
tar czf /media/$USER/USB/pc-NAME.tgz runs/*/*/run-* runs/hand/*/*/run-*     # "cannot stat" for a pattern with no runs is fine

# THE PC THAT ANALYSES, in the repo
for f in /media/$USER/USB/pc-*.tgz; do tar xzf "$f"; done
for d in runs/*/*/run-*; do echo "$d $(ls $d/node-*.jsonl | wc -l) $(ls $d/client-*.jsonl | wc -l) $(ls $d/users-*.out | wc -l) $(ls $d/run.json | wc -l)"; done
```

Each line must end `3 30 3 1`, and for `e7-p1-300` `3 3 3 1`.

What `analyze.py` needs in `run.json`:

| Keys | What they do |
|------|-------------|
| `experiment`, `cell`, `warmup_s`, `measured_s` | Required. It fails without them |
| `users`, `client_programs_expected`, `nodes`, `victim` | Checked. A run that does not match them is left out, by name. 0 or no `users` skips that check |
| `run`, `profile`, `commit`, `programs_per_pc`, `pattern`, `think_ms` | For the record |

**Analyse.** Any PC with Python and the logs will do, also a laptop. One 3000-user run needs about 6 GB of free memory: about
15 log lines a report and 1.7 KB a line in memory (one 300-user run peaked at 650 MB on one PC). All the plan's logs come to about
50 million lines. At the 90,000 lines a second measured on one fast PC that is about 10 minutes, so allow 30 on a lab PC. One-PC
rehearsal: 54 runs and 6.1 GB of logs took 6.1 minutes, with a peak of 5.6 GB of memory.

```bash
python3 --version
sudo apt install python3-venv                    # Ubuntu packages it apart from Python. Not needed if the next line works
python3 -m venv .venv && . .venv/bin/activate
pip install -r analysis/requirements.txt         # Python 3.10, as on Ubuntu 22.04, or newer
```

- **Ubuntu 22.04 has Python 3.10**, where numpy 2.3 and scipy 1.16 do not exist, so `requirements.txt` picks numpy 2.2.6 and scipy
  1.15.3 there. `test_analyze.py` has run on those. The versions for Python 3.11 and newer have not been tried on a lab PC.
- No `python3-venv` and no sudo: skip the venv line, and use `pip3 install --user -r analysis/requirements.txt`. Not tried.

```bash
python3 analysis/test_analyze.py                 # checks the arithmetic. Look for OK. Its "left out" lines are its own test data
python3 analysis/analyze.py runs 2>&1 | tee analysis.log
```

Then do the checks in Plan A step 6.

## 6. Tests the runner does not do

By hand, about 1 hour, after the batch. **Use `prepare`, `keep`, `handjson` and the node line from Plan B.** Plan A people, first,
on all six: copy `experiments/hosts` and `hand.sh`, repeat any `sed` you made to the cells in section 3, and `rm -rf runs/current`.
The runner leaves its last run's logs there, already copied, and `prepare` stops on them. Logs go to `runs/hand/`, so they stay
out of the plots for the report. Analyse them apart, after collecting (USB as in Plan B, or `rsync -a <target>:DistributedSystems/runs/hand/ runs/hand/`):

```bash
python3 analysis/analyze.py runs/hand --out analysis/out-hand
```

Save this as `~/extras.py`. It prints what `summary.csv` does not hold. Run it on a run folder once all six PCs' logs are in it.

```python
#!/usr/bin/env python3
# extras.py <run folder>: what summary.csv does not hold. It only reads the logs.
import collections, glob, json, os, sys
count, io, digests, rounds, answers = collections.Counter(), collections.Counter(), {}, {}, {}
for path in sorted(glob.glob(sys.argv[1] + "/*.jsonl")):
    name, last_io, leader = os.path.basename(path), None, None
    for n, line in enumerate(open(path, errors="replace"), 1):
        try:
            e = json.loads(line)
        except ValueError:
            print(f"DAMAGED {name} line {n}")
            continue
        ev = e["ev"]
        if ev == "commit" and e["kind"] == "commit_result":
            count[e["outcome"]] += 1
        elif ev == "raft_io":
            last_io = e
        elif ev == "raft_state":
            leader = e["leader"]
        elif ev == "state_checkpoint":
            digests.setdefault(e["log_index"], set()).add(e["digest"])
        elif ev == "client_sent" and e["attempt"] == 0:
            count["sent"] += 1
        elif ev == "client_done":
            count["answered"] += 1
            answers.setdefault(name[7], []).append(e["ts_us"])
        elif ev == "client_gave_up":
            count["gave_up"] += 1
        elif ev == "provisional_conflict":
            count["conflict " + e["reason"]] += 1
        elif ev == "fault_election":
            rounds.setdefault(e["round"], {})[name] = (e, leader)
        elif ev in ("restarted", "rejoined", "degraded_entered", "degraded_left"):
            print(line.strip())
    if last_io:
        io.update({k: last_io[k] for k in ("saves", "saved_commands", "syncs", "sync_us")})
print(f"sent {count['sent']}, answered {count['answered']}, gave up {count['gave_up']}, "
      f"unaccounted {count['sent'] - count['answered'] - count['gave_up']}")
for group, times in sorted(answers.items()):
    times.sort()
    print(f"client IDs from {group}: longest with no answer to anyone "
          f"{max((b - a for a, b in zip(times, times[1:])), default=0) / 1000:.0f} ms")
print(f"stale per committed {count['stale'] / max(count['committed'], 1):.2f}, "
      f"commands per save {io['saved_commands'] / max(io['saves'], 1):.1f}, "
      f"mean disk sync {io['sync_us'] / max(io['syncs'], 1) / 1000:.1f} ms")
print(f"checkpoints {len(digests)}, digest mismatches {sum(len(d) > 1 for d in digests.values())}")
print({k: v for k, v in count.items() if k.startswith("conflict")} or "no provisional conflicts")
for r, lines in sorted(rounds.items()):
    victims = {e["victim"] for e, _ in lines.values() if e["victim"] is not None}
    ms = {e["ms"] for e, _ in lines.values() if e["ms"] is not None}
    called = [(e, leader) for e, leader in lines.values() if e["called"]]
    who = (f"called by {called[0][0]['src']}, leader then {called[0][1]}, "
           f"took {(called[0][0]['election_us'] or 0) / 1000} ms") if called else "caller's line missing"
    print(f"round {r}: victim {victims or 'none'} for {ms or '-'} ms, {who}, lines {len(lines)}")
for path in glob.glob(sys.argv[1] + "/*.out") + glob.glob(sys.argv[1] + "/prog-*.txt"):
    if "panicked" in open(path, errors="replace").read():
        print("PANIC in", path)
```

The load for T1, T2 and T4, on client PC M, for 5 minutes. It holds that terminal, so type every `ctl` below in a second terminal
on client PC 1, after `cd ~/DistributedSystems; source ~/hand.sh`:

```bash
M=1
sleep $((60 - $(date +%s) % 60)); bash scripts/start-users.sh --config runs-config/run.toml --machine $M \
  --users 100 --duration-s 300 --warmup-s 15 --seed-base 1000000 --out runs/current
```

### T1. Physically turn off a machine during load, and bring it back (8 minutes)

Notes, Oct 28 and Sept 13. Never wipe `data/raft` in this test: that is what the node comes back from (D118).

1. All six: `prepare e1-300-bidding`. Start the nodes. Start the 5-minute load on all three client PCs.
2. At about 60 s, client PC 1: `ctl status`. Note the leader's ID and term, and the clock time.
3. **Cut that node PC's power.** Hold the button or pull the plug. Not a shutdown, and not `ctl`.
4. Client PC 1, a few times: `ctl status`. Another row reads `leader` in a newer term. The dead node reads `unreachable`
   and `udp no answer`, which is right here. `members` drops it after about 2 s. The load keeps going. With a node off, each
   `ctl status` takes a few seconds, because it waits for that node.
5. Wait about 60 s. Power it on. Log in, then start it with the same ID, with `>>` so its old output stays:

   ```bash
   cd ~/DistributedSystems; source ~/hand.sh; N=<its ID>
   nohup target/release/node --config runs-config/run.toml --id $N >>runs/current/node-$N.out 2>&1 &
   echo $! >runs-config/node-$N.pid
   sleep 5                                                    # the log is written a moment later. With no wait, the next line prints nothing
   grep -E '"ev":"(restarted|rejoined)"' runs/current/node-$N.jsonl | tail -2
   grep -c 'Can not initialize' runs/current/node-$N.out      # must be 0 (D137)
   ```

6. When the load ends, client PC 1: `ctl status | tee runs/current/status-after.txt`. Then `kill` the nodes.
7. All six: `keep power-leader 1 runs/hand`. Client PC 1: `handjson power-leader 1 15 285 300 30`.
8. After collecting: `python3 ~/extras.py runs/hand/power/leader/run-1`

**The node's own log loses its last seconds, because nothing syncs it, and it may hold a damaged line where the new
log starts.** This is read from the code, not seen on a real power cut yet. It was tried by cutting 57 bytes off a log by hand
before the restart. `extras.py` then prints `DAMAGED node-2.jsonl line 1915`, and `analyze.py` prints `left out ... a log is
damaged`. The tar on the USB stick is your original, so delete that one line here, with your own number and node, and analyse again:
`sed -i '1915d' runs/hand/power/leader/run-1/node-2.jsonl`

| Write down | Where from | Target, and one-PC rehearsal with `kill -9` as a guide |
|------------|-----------|---------------------------------------------------------|
| Clock time of the cut, and of power on | Your watch | |
| Leader and term before; new leader and term after | `ctl status`. For the time, on one survivor's log, on its own clock: the new term's first `raft_state` line with a `leader`, minus its `peer_suspected` time, plus `silent_ms` | A new leader within 1 s (D214). Rehearsal: 675 ms |
| Longest with no answer to anyone | `extras.py`, one line per client PC | Rehearsal: 686 ms |
| `gave up`, `retried` | Each client PC's last line | Gave up 0 (FT-9) |
| `peer_suspected` on both survivors | `grep '"ev":"peer_suspected"' runs/hand/power/leader/run-1/node-*.jsonl`. Read the lines at the cut's time: the first and last lines are nodes starting and stopping | Within 1 s of the cut. One PC: `silent_ms` about 190 |
| `restarted`, `rejoined`, `recovery_us` | `extras.py` | Rehearsal: catch-up 34 ms |
| Equal digests; digest mismatches | `status-after.txt`; `extras.py` | Equal; 0 |
| `unaccounted` | `extras.py` | 0 |

If time allows, do it again on a follower: `power-follower`. No new leader should be needed.

### T2. One node off and one failed, then replay when a peer returns (8 minutes)

Notes, Sept 13. Say first what this gives up: with one server there is no majority, so an answer cannot be committed. It
is labelled provisional and replayed later, and it may not stand (D120, D122).

1. All six: `prepare e1-300-bidding`. Start the nodes. Start the 5-minute load on all three client PCs.
2. At about 60 s, cut the power of one node PC, say node 1.
3. Client PC 1, at once: `ctl silence --node 2 --ms 20000`
4. About 2 s later, `ctl status` shows `degraded` true on node 3, and its `members` reads `3`. Its role reads `candidate`
   and its term climbs fast: a node alone keeps calling votes it cannot win. That is harmless.
5. After the 20 s, node 2 returns. `degraded` goes false, node 3 replays, and within a few seconds both read `members` `2,3`.
   Expect 3 to 4 s with few answers while a leader settles (one PC).
6. Power on node 1 and start it as in T1. Every `members` reads `1,2,3`.
7. When the load ends: `ctl status | tee runs/current/status-after.txt`. `kill` the nodes.
8. All six: `keep oneleft-300 1 runs/hand`. Client PC 1: `handjson oneleft-300 1 15 285 300 30`.
9. After collecting: `python3 ~/extras.py runs/hand/oneleft/300/run-1`

| Write down | Where from | Target, and one-PC rehearsal as a guide |
|------------|-----------|------------------------------------------|
| `provisional` on each client PC's last line | The client PCs | **Above 0 on all three**: that is one server answering three client PCs |
| `gave up` | The client PCs | 0 |
| Longest with no answer | `extras.py`. It prints the longest of the run, going in or coming back | Rehearsal: about 2.2 s going in (`degraded_after_ms`) and about 3.2 s coming back |
| `degraded_entered`, `degraded_left`: its reason, `degraded_ms`, `provisional` | `extras.py` | |
| Conflicts, by reason | `extras.py` | Rehearsal: 4471 of 5722 did not stand, nearly all `responder_taken`, as 300 responders run out. Expected at this load. Clients are not told (D122) |
| `members` going `3`, then `2,3`, then `1,2,3` | `ctl status` | |
| Equal digests at the end | `status-after.txt` | Equal on all three |

### T3. Stress to the breaking point (33 minutes: 11 runs, about 3 minutes each, fewer if it falls early)

Spec 231; Lecture 9 s1.2. Cells stop at 3000 users in a closed loop, where a slow cloud slows its users. Here `--rate` sends at
a fixed rate whatever the replies do, and latency counts from the intended send time (D54).

**Rising steps.** Three programs on each client PC, 9 in all. Client IDs by hand are 1 to 9 and must be unique across
PCs: client PC 1 uses 1, 2, 3; client PC 2 uses 4, 5, 6; client PC 3 uses 7, 8, 9.

| Offered in all, a second | 1000 | 2000 | 3000 | 4000 | 5000 | 6000 | 8000 | 10000 |
|--------------------------|------|------|------|------|------|------|------|-------|
| `--rate` R per program | 111 | 222 | 333 | 444 | 556 | 667 | 889 | 1111 |

For each step, on a clean cluster:

```bash
# ALL SIX
prepare e1-3000-bidding             # 3000 responders, so the pool is not the first limit (D219)
# NODE PCs: start, as in Plan B. CLIENT PC 1: ctl status

# CLIENT PC M. FIRST is 1, 4 or 7. R from the table. Starts at :00
FIRST=1 R=111
sleep $((60 - $(date +%s) % 60)); for id in $FIRST $((FIRST + 1)) $((FIRST + 2)); do
  target/release/loadgen --config runs-config/run.toml --client-id $id --rate $R --duration-s 75 --warmup-s 15 \
    --seed $id $([ $id = $FIRST ] && echo --sample-machine) >runs/current/prog-$id.txt 2>&1 &
done; wait; cat runs/current/prog-$FIRST.txt
```

A program can run up to about 36 s past its duration, while late reports finish their retries. Wait for all of them. Then:

```bash
# CLIENT PC 1: ctl status | tee runs/current/status-after.txt     NODE PCs: kill the nodes
# ALL SIX
keep stress-rate-1000 1 runs/hand
# CLIENT PC 1. users is 0, because --rate has none. 9 programs
handjson stress-rate-1000 1 15 60 0 9
```

Go up until it has fallen, then one step more. By arithmetic, not measured, planning tops out at 3000 to 5000 plans a
second (4 threads x 3 nodes, at 4 to 2.4 ms a plan) and 3000 responders at about 5000 assignments a second.

One-PC rehearsal, one run per step, 3 programs, as a guide to the shape: at 3000 a second p50 39 ms, at 4500 p50 12.3 s and 16% gave up, so the knee was
between them. A closed loop hides this: 4500 users read p50 185 ms and 0 gave up, because a slow cloud slows its users, and throughput fell as users
rose, 2868, 2559 and 2443 a second at 3000, 4500 and 6000.

**If the client PCs give out first** (`client_pc_cpu_busy_p95` at 0.9 or more), make the cloud slower instead. On client
PC 1, after `ctl status`, each plan then costs about 5 times as much, and a restart undoes it:

```bash
for n in 1 2 3; do ctl skew --node $n --work 5000; done
```

Repeat the steps from 1000 as `stress-skew-rate-1000` and so on. Use the value above, or 5 times your `plan_work`. Never `--work 0`.

**Flat out, once.** A closed loop with no pause, on a clean `e1-3000-bidding` cluster. It shows the most the cloud answers:

```bash
sleep $((60 - $(date +%s) % 60)); for id in $FIRST $((FIRST + 1)) $((FIRST + 2)); do
  target/release/loadgen --config runs-config/run.toml --client-id $id --users 100 --think-ms 0 --duration-s 75 --warmup-s 15 \
    --seed $id $([ $id = $FIRST ] && echo --sample-machine) >runs/current/prog-$id.txt 2>&1 &
done; wait; cat runs/current/prog-$FIRST.txt
```

`keep stress-flat-900 1 runs/hand` and `handjson stress-flat-900 1 15 60 900 9`.

**A spike, twice.** Every user sends on the same beat. `prepare e1-300-bidding`, start the nodes, then on each client PC, with M
its number as before:

```bash
sleep $((60 - $(date +%s) % 60)); bash scripts/start-users.sh --config runs-config/run.toml --machine $M \
  --users 100 --pattern all-at-once --think-ms 1000 --duration-s 75 --warmup-s 15 --out runs/current
```

`keep stress-spike-100 1 runs/hand` and `handjson stress-spike-100 1 15 60 300 30`. Then `prepare e1-1000-bidding`, the same
with `--users 333`, `keep stress-spike-333 1 runs/hand` and `handjson stress-spike-333 1 15 60 999 30`. Does the term change?
Does the raised buffer stop the drops? One-PC rehearsal, with a capped buffer, was fine at 100 per PC and lost 16% of request
copies at 333.

**Read the steps,** after the analysis above:

```bash
grep -E '^stress,[^,]*,(throughput_rps|p50_ms|p99_ms|gave_up|retried_share|elections|no_responder_share|plans_refused|takeovers_backstop|udp_drops_nodes|client_pc_cpu_busy_p95),' analysis/out-hand/summary.csv
```

How to recognise each point. This is our working rule, not from a document. The report's knee is where latency turns sharply upward.

| Reading | Healthy | The knee | It has fallen |
|---------|---------|----------|---------------|
| `throughput_rps` against offered | About equal | About equal | Under 90% of offered, or lower than the step before |
| `p99_ms` | Flat from step to step | 3 times the step before, or more | Seconds, and rising |
| `gave_up` | 0 | 0 | Above 0 |
| `retried_share`, `plans_refused`, `takeovers_backstop` | 0 or close | Rising | Large |
| `elections` | 0 | 0 | Above 0 means overload has reached the leader: write it down, it is a finding |
| `client_pc_cpu_busy_p95` | Under 0.9 | Under 0.9 | 0.9 or more means the client PC is the limit, not the cloud |

Name the limit reached first. Candidates: stale plans, the worker queue, the disk, the planners, UDP drops.

**What counts as a real bug.** Stop, and keep the logs untouched:

- `digest_mismatches` above 0, or digests that stay different after the load stops and all nodes serve.
- `panicked` in any `.out` or `prog-*.txt` file (`extras.py` prints `PANIC`).
- `unaccounted` above 0 in `extras.py`: a report neither answered nor given up on.
- A node that stays `serving` false while a leader exists.
- `reports_expired` above 0, or `fault_split_decisions` above 0.
- `elections` above 0 at a load **below** the knee, with no fault. First check `nproc` and the disk (section 3).
- `ctl incidents --id <id>` showing different `log_index` values for one report, for longer than a moment.

**Not a bug past the knee**, as long as digests stay equal: high latency, retries, `gave_up` (the client was told),
`no_responder_share` rising, `plans_refused`, `takeovers_backstop`. That is what falling looks like. Report it.

### T4. The failure draw on real timers (8 minutes, or 12 with the last part)

Spec 149; notes, Oct 28. `ft8-random-faults` in the plan has only about 2 rounds per run. This gives about 10 in one go.

1. All six: `prepare ft8-random-faults`: a round every 20 to 30 s, silence 1 to 20 s. Start the nodes. Start the 5-minute load.
2. Nobody types a fault. Watch `ctl status`: `silenced` goes true on one node, then false by itself.
3. When the load ends, and no row says `silenced` true (it can last 20 s): `ctl status | tee runs/current/status-after.txt`.
   `kill` the nodes. `keep draw-300 1 runs/hand`, and on client PC 1 `handjson draw-300 1 15 285 300 30`.
4. After collecting: `python3 ~/extras.py runs/hand/draw/300/run-1`. The tickets are in
   `grep '"ev":"fault_election"' runs/hand/draw/300/run-1/node-*.jsonl`. Put that output in the report.

| Write down | You want |
|------------|----------|
| Each round: victim, how long silent, who called it, who led, how long it took | One victim per round, on all 3 lines. One-PC rehearsal: about 0.25 ms, and 0.1 to 25 ms when all six shared 2 CPU threads. Rounds from before the load started count here too |
| Rounds called by a follower, and rounds that elected a follower | At least one of each: no leader on this path |
| A line with `"victim":null` | Allowed: that node missed a number, so it did nothing (D159) |
| `gave up` on each client PC; digests at the end | 0; equal |

**Last part, one node off.** Do "Check the fault election on the real network" in runbook section 5, with
`prepare ft8-election-rate`, the nodes and the 5-minute load as above, and `runs/current` where it says `runs/lab`. Look on the two
node PCs still up. With a node off, no draw may go ahead: on one PC, with `kill -9`, every new line read `"victim":null` within a few
seconds, and the `silenced` count stayed put. Then start the node again as in T1. Three-ticket lines come back. At the end `kill` the
nodes, `keep draw-off 1 runs/hand`, and on client PC 1 `handjson draw-off 1 15 285 300 30`.

### T5. 5 retries, then an error (2 minutes)

Notes, Sept 13. With all three nodes stopped, on client PC 1:

```bash
prepare e1-300-bidding
target/release/loadgen --config runs-config/run.toml --client-id 9 --users 1 --think-ms 1000 --duration-s 1
grep -c '"ev":"client_sent"' runs/current/client-9.jsonl        # 6: the first send and 5 retries
grep '"ev":"client_gave_up"' runs/current/client-9.jsonl        # "attempts":6
```

It takes about 30 s and prints `done 0, gave up 1`. Waits are 2, 4, 6, 6, 6 and 6 s, give or take 20% (D52). On the web page the
same case reads `No answer after 6 attempts`. Keep the two output lines, then `rm -rf runs/current`.

## 7. What to keep, and where it goes in the report

**Keep all of this until the grade is in, in two places, never in git:**

- The whole `runs/` folder: every log, `run.json`, `config.toml`, `commit.txt`, `status-after.txt`, `users-*.out`.
- `analysis/out/`, `analysis/out-hand/`, and `quick.log`, `extra.log`, `analysis.log`.
- Your written notes from section 6. A photo of the paper will do.
- What you wrote down in section 3: the `dd` times, `lscpu`, `free -h`, `lsblk`, `ping`.
- A run with a real bug, untouched.

```bash
tar czf rescuemesh-runs.tgz runs analysis/out analysis/out-hand *.log
```

`analysis/out-hand/` and the `.log` files are not in `.gitignore`, so never `git add -A` here.

Real totals, per cell and in all, for the report:

```bash
awk -F, '$3=="count" || $3=="provisional_count" || $3=="gave_up" {t[$1"-"$2]+=$4*$5; all+=$4*$5} END {for (c in t) printf "%s %.0f\n", c, t[c]; printf "all %.0f\n", all}' analysis/out/summary.csv | sort
```

Each result slot is a `\resultpending` note in `docs/report/main.tex`. It names its metrics and cells, and section 2 gives
the section. Fill those from `analysis/out/summary.csv`, each number with its `ci95_half_width`, and take the plots from
`analysis/out/`. These slots need something else:

| Report section | What goes in | From |
|----------------|--------------|------|
| M: Method; What has been run so far | Lab date, commit, PCs' CPU, memory, disk, network. A row for these runs, with the profile | Section 3 notes |
| M: Raw data | Totals per cell and in all; where the logs are kept | The `awk` line |
| Stress testing: What breaks next | One row per load step, and the limit reached first; stale plans per commit; commands per save; the spike | `analysis/out-hand/summary.csv`, `extras.py` |
| M: CPU, memory, network and queues; D: The queue with grouped saves | Commands per save and mean disk sync at 300, 1000 and 3000 users | `extras.py` on `e1-*-bidding` runs |
| S: Scenarios 1 and 2 | Win counts before and after, and a page screenshot | `docs/demo.md` rehearsal |
| S: Scenario 3, scenes 3b, 3c, 3d | Your notes from T1, T2 and T4 | Section 6 notes |
| D: Five retries, then an error | T5's two lines | T5 |
| D: The group with a who's-alive list | `grep -c '"ev":"group_view"' runs/e1/3000-bidding/run-1/node-*.jsonl` | Logs |
| D: The failure draw | T4's `fault_election` lines | T4 |
| D: Saving to disk; How a node recovers | T1's notes; `du -sh data/raft` on a node PC after the 3000-a-second step in T3, before the next `prepare` | T1, T3 |
| D: Degraded mode | T2's notes, beside `degraded-300` | T2 |
| The whole system in plain words; Changed decisions | The leader's move: its `leader_stepped_aside` and `leader_step_aside_ended` lines and `elections`, beside `e2e6-skew-expected-wait`. The report has no slot for this yet | `e8-step-aside` |
| D: The web page | `which firefox`, then the page through scenes 1 to 4 | `docs/demo.md` rehearsal |
| D: How we measure | One full pass: the commit, the profile, and the `runs` column, with any run left out and why | `analysis.log` |
| Test plan: How we test | `cargo test` on the final commit. Not from these runs | Linux or WSL |
| M: Limitations | Rewrite the "One PC so far" and "Runner" bullets, and add what section 1 says the report must state | This guide |

**If time runs out, do these in this order, and name what you skipped under M: Limitations:**

1. Section 3, the trial and calibration. 40 minutes.
2. The short list: 13 cells, 3 runs each, 58.5 minutes by arithmetic. It touches all five scenarios and rows 1 to 10, at fewer loads.

   ```bash
   nohup scripts/run-experiments.sh experiments/hosts quick ft8-random-faults ft8-election-rate e1-30-bidding e1-300-bidding e1-300-leader e1-1000-bidding e1-3000-bidding e2e6-skew-expected-wait e3e4-300-leader e3e4-300-follower e3e4-1500-leader e5-faults-off degraded-300 >short.log 2>&1 &
   ```

3. T1 and T2, 16 minutes, and T5, 2 minutes. These are asked for by name, and no cell covers them.
4. Analyse. Fill in Method, Raw data, Latency, Throughput, Election overhead, Load distribution, Failure and recovery.
5. The rest of the plan by name, 8 cells, 36 minutes: `e1-30-leader e1-1000-leader e1-3000-leader e2e6-skew-expected-wait-cpu e3e4-1500-follower e4-300-silence-20s e7-p1-300 e8-step-aside`.
6. T3, then T4. Without T3, say that the top of the load was 3000 users and the breaking point was not found.

**Last resort: keep 3 runs and shorten the time.** `WARMUP=15 MEASURED=30` before the same command gives 39 minutes, and 300-user
cells still reach 27,000 operations. Do not go below 3 runs: 1 or 2 give no usable interval (D63). State the time used. The fault
then fires at 25 s, so a 15 s silence leaves 5 s of "after", and `degraded-300`'s 20 s leaves none. The `measured_s` check in step 6
lists every run, which is expected here. In Plan B this needs `MEASURED=30` in `hand.sh`, `--duration-s 45`, and `85` in place of `95` in the fault line, for :25.

---

# Reference: cells, keys and profiles

Run configurations for the measurements in the report. Each cell is one `<experiment>-<cell>.toml` here:
a first line saying how to run it, then the config sections that differ. The runner adds the node list,
`metrics.dir` and `storage.dir` from the hosts file, so all six machines read one identical config.

```text
# run: users=300 think_ms=1000 victim=leader silence_ms=15000
```

| Key | Meaning |
|-----|---------|
| `users` | simulated users in all, shared exactly across the client machines, as 34, 33 and 33 for 100 |
| `programs` | real `loadgen` programs per client machine; default 10, or `PROGRAMS_PER_PC` (D181) |
| `pattern` | `steady` (default), `random` or `all-at-once` (D182); every cell here uses steady |
| `think_ms` | pause between a reply and that user's next request |
| `victim` | `leader`, `follower` or `others` (everyone but one follower, D120); no key means no ordered fault |
| `silence_ms` | how long the victim is silent, starting a third of the way into the measured time |
| `skew_work` | `ctl skew` on the first node: extra planner work per request |

## Cells

| ID | Varies | Measures | Owner | Cells |
|----|--------|----------|-------|-------|
| E1 | users × policy | latency percentiles, throughput, fairness, the leader's share of plans (`leader_plan_share`, D166) | B, F | `e1-<users>-<policy>` |
| E2, E6 | load-score variant, with one node skewed | share of work per node, latency and fairness under skew | E, B | `e2e6-skew-<score>` |
| E3, E4, FT-6 | victim × load | client-side gap (`client_gap_*`), election time, in-flight outcomes, time to pass the rejoin check (`recovery_mean_ms`), equal state (`digest_mismatches`, `rejoins_digest_confirmed`), elections after a rejoin (`elections_after_rejoin`) | C, D | `e3e4-<users>-<victim>`, and `e4-<users>-silence-<n>s` for silence length |
| E5 | faults on or off | elections, messages by type, commit latency, by path. The plan has `e5-faults-off` only: compare it with `ft8-random-faults`, or run `e5-faults-on` if time is left (D230) | A | `e5-faults-off`, `e5-faults-on` |
| FT-8 | elected faults throughout | lost requests, duplicate commits, split decisions (must be 0) | C | `ft8-random-faults` |
| FT-8 | fast elected faults | election frequency, duration and message overhead, for the spec's three numbers; its silences are shorter than the spec's | C | `ft8-election-rate` |
| P-2 | bid window | split decisions | B | `p2-window-<n>ms` |
| D120 | load, with two nodes silent | provisional answers, conflicts, replay | C | `degraded-<users>` |
| E7 | one program per machine against 10 | whether the number of programs changes latency, client machine CPU or arrivals; compare with `e1-300-bidding` | F | `e7-p1-300` |
| E8 | a busy first node with the step-aside on (D218) | whether and when the leader moves (`leader_stepped_aside`, `leader_step_aside_ended`, `elections`), and commit latency either side; compare with `e2e6-skew-expected-wait`. Only meaningful in runs where the first node led | A | `e8-step-aside` |

`e1-1000-*` and `e1-3000-*` use 3000 responders and the lower E1 cells 300, so a line across that gap on a figure is not the effect of load alone (D219).

E5's `msgs_` figures count what nodes send. A client's cancels are in `udp_cancels_per_req` and `udp_datagrams_per_req`: print those beside them (D172).

## Profiles (D63)

| Profile | Cells | Runs per cell | One run | Time in all |
|---------|-------|---------------|---------|-------------|
| `quick` | the 15 in `quick.list` | 3 | 15 s warm-up + 60 s measured + 15 s restart | about 70 minutes |
| `full` | all 34 | 5 | 30 s warm-up + 2 min measured + 15 s restart | about 8 hours |

`quick` starts with the `ft8` cells, so a bug in the fault election costs one cell and not the hour. To leave them out, pass an explicit cell
list; `lab.toml` does not reach the cells.

**`quick` has limits, and the report must state them:** 3 runs give wide confidence intervals, and it is
below the 5 runs and 30 s warm-up of the `full` profile. Say which profile each figure came from.

## Running

```bash
cp experiments/hosts.example experiments/hosts      # then set the real addresses
scripts/lab-build.sh experiments/hosts              # the same commit on all six machines
scripts/run-experiments.sh experiments/hosts quick
python analysis/analyze.py runs
```

Each run lands in `runs/<experiment>/<cell>/run-<n>/`, with every process's log, the config, the git
commit and the final `ctl status`, which has a `members` column in `status-after.txt`. Raw logs stay out of git (`runs/` is
ignored).

The runner refuses to start a run while a `frontend`, `zonefeed`, `loadgen` or `node` from `$BIN` is running on any machine (D139,
D206), so stop every frontend first. It tries `ctl silence` up to 3 times, and a run that never gets its fault goes to `runs/dropped/`,
where the analysis does not read it, and the batch goes on (D139). A rerun moves the old run folder to `runs/old/` (D186). Each run's `run.json` is written last, so only whole runs are counted (D223).
Before the load starts, the runner checks each node's UDP port with `ctl status`, and stops the batch on `udp no answer` (D168). `STOP_WAIT_S` (default 10) is how long the runner lets things stop, before it copies logs and before it
refuses a run (D189).

Each client machine's number in the hosts file is 1 to 9, and becomes the first digit of its programs' client IDs.

`experiments/hosts.local` runs everything on one machine, to try the scripts. Those numbers mean nothing:
all the processes share one CPU and the network is loopback.

Before a real run, check `service_mean_us` in `analysis/out/summary.csv` for one short cell: `plan_work =
1000` was calibrated on a development machine to about 4 ms per plan (D129). If the lab machines differ
much, change it in every cell, and say so in the report. The choice before the series is in section 3.

**No new leader without a cause.** Final data must come from a build with the proposal queue (D133) and the who's-alive list in: load figures
from before the queue cannot be compared with figures from after it, and `msgs_bid_per_req` falls in fault cells compared with any
older run (D154).

A busy leader stepping aside is off in every cell but `e8-step-aside`: `leader_overload_ratio = 0`, because the code's default is on,
and a leader change in the middle of a run would make those runs differ from the ones they are compared with (D218).

`experiments/hosts` stays in order 1, 2, 3, whatever `config/lab.example.toml` lists: reordering it moves the skew target and each node's election range. Every run also holds a few duplicate `RegisterResponders` entries, because any serving node may propose the list
(D164), so digests differ from any run made before that change. They change no state.

The cells past the knee: a report whose plans go stale three times now commits that nobody could be sent, so overload shows as a higher `no_responder_share` at low latency, where before it showed as long latency and give-ups. Comparing with runs from before D220 would mislead.
`e3e4-1500-*`, `e4-1500-*` and `degraded-1500` still run 1500 users on 300 responders, so read their fault and recovery figures with D219 in mind.

No cell has anything to do with the TTL (D147). The runner never passes `--ttl-ms`, and with the 60 s default a report cannot
be refused in a run, because the client's retries end first. So `reports_expired` must be 0 in every cell, and `analyze.py`
warns otherwise: the likeliest cause is an age stamped wrongly, which looks like a very fast run where nobody gets sent. An
expired report also counts in `no_responder_share`.

`cargo test --test experiment_cells` checks that every cell is a valid config.
