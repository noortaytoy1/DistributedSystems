# Requirements check

Every requirement, what we did about it, and where to see it. Use this to check each one yourself.

The list is section 2 of [experiments/README.md](../experiments/README.md), which gives the spec line, class note or lecture for each. "Spec 282" means line 282 of the project description's text.

**How to read the status:**

- **Done:** measured, with results you can open.
- **Done, final build:** measured on the final build, `b6221c5`, too. Results are in the report's section 8a and in `analysis/results/kills/tables.md`.
- **Done overnight:** measured on `a7ceb34`, and not repeated on the final build, as it was already covered.
- **Partly:** the note says what is missing.

## The two rules from class

| Rule | Status | What we did | Where |
|---|---|---|---|
| At least 10,000 requests a second | Done, final build | Overnight: 12,000 a second with about 3.9 ms of planning per report, and 30,000 with almost none, in 5 of 5 runs each. Final build, midday: all of 10,000 and 15,000 a second answered | Report section 9 |
| No leader, no coordinator | Done | Raft removed. Nodes order the log by a shared sorting rule, and no node is in charge | D249, [shared-log.md](shared-log.md) |

## The five demonstration scenarios

The PDF's Required Demonstration Scenarios, rows 1, 2, 3, 4 and 9. Our guide numbers the last one "Scenario 5".

| # | Requirement | Source | Status | What we did | Where |
|---|---|---|---|---|---|
| 17 | Scenario 1, normal: different peers elected as load changes | Spec 244 | Done | Load changed from 1,000 to 5,000 to 9,000 and back. Each node won 31.4 to 34.5% at each load, and bids rose with load | Report section 5 |
| 18 | Scenario 2, load imbalance | Spec 247 | Done | One node made 5 times slower. Its share went 33.4%, 4.9%, 33.3%. A queue-only score stayed at 25.2%, and the control stayed flat | Report section 5 |
| 19 | Scenario 3, server failure under load | Spec 250 | Done, final build | Others dropped the failed node in 1,037 to 1,065 ms. 0 gave up up to 20,000 a second. At 30,000, 12,303 gave up in total over 5 runs | Report section 7 |
| 20 | Scenario 4, recovery before taking part again | Spec 254, 152 | Done, final build | Member again in 0.8 s at 1,000 a second, 10.5 s at 20,000 and 73 s at 30,000, with identical digests | Report section 8 |
| 21 | Scenario 9, scale and stress | Spec 272 | Done overnight | Raised the load until answers stopped rising, at about 34,000 a second, and about 13,700 with planning. Flat out and the spike on the final build | Report section 9 |

## Fault tolerance

| # | Requirement | Source | Status | What we did | Where |
|---|---|---|---|---|---|
| 25 | Servers elect one to go silent for up to 20 s, then it rejoins | Spec 149 | Done | 47 elected faults, 0 gave up, dropped in a median of 1,018 ms | Report section 7 |
| 26 | Injected failures up to 20 s, service continues | Spec 227 | Done, final build | Every failure run uses 20 s | Report section 7 |
| 27 | Physically turning off a machine | Notes Oct 28, Sept 13 | **Replaced by kills** | The person running the lab decided on 2026-11-05 that killing processes with `kill -9` is enough, so no plug is pulled. A kill stops the process at once, as a power cut would, but it cannot lose disk writes the way a real cut can | Report section 11 |
| 28 | One node off and one failed: one server handles the three client PCs | Notes Sept 13 | Done, final build | **Fixed today (D261, D262).** Before, commits did not come back by themselves after two kills. Now the last node answers, labelled provisional, and all return by themselves | `tests/kills.rs`, T2 and kill runs |
| 29 | 5 retries, then an error | Notes Sept 13, spec 306 | Done, final build | T5: the client gave up with `"attempts":6` | `analysis/results/kills/t5.txt` |
| 7 | Recovery and convergence time at different loads | Spec 287, 167, 152 | Done, final build | See Scenario 4 | Report section 8 |
| 6 | Failover time at different loads | Spec 167 | Done, final build | See Scenario 3 | Report section 7 |
| 5 | Before, during and after a failure | Spec 286 | Done, final build | Latency and throughput for each stretch of each failure run | `cells.csv`, `analysis/results/kills/phases.csv` |
| 8 | Requests in flight at a failure: retried or completed | Spec 158 | Done | Every report is accounted for: 0 unaccounted in all 157 runs | Report section 1 |
| 9 | Duplicate execution prevented or harmless | Spec 161 | Done | First commit wins. 0 digest mismatches in 69,715 checks | Report section 1 |
| 32 | Group membership changes as nodes fail | Lecture 8, Lecture 9 | Done | Members and view are logged at each change | `raft_state`, `group_view` events |

**Not in the list, and asked for in class:** kills in every order. Two and three nodes killed, in each order, under load, at 300, 10,000, 15,000 and 20,000 a second: report section 8a.

## Measurements

| # | Requirement | Source | Status | What we did | Where |
|---|---|---|---|---|---|
| 1 | Request latency: mean, median, p95, p99 | Spec 282 | Done, final build | Per run and per cell, with confidence intervals | `cells.csv`, Report section 9 |
| 2 | Throughput as clients increase | Spec 283, 231 | Done, final build | Against offered load, and against 100, 333 and 1,000 users per PC | Report sections 8a, 9 |
| 3 | Election frequency, duration, message overhead | Spec 284 | Done | Bids, window, rounds seeing all 3 bids | Report section 4 |
| 4 | Load distribution, fairness, evidence for the policy | Spec 285, 358 | Done | Shares per node, against a queue-only score and a control | Report section 5 |
| 10 | CPU, memory, network, queues | Spec 295, 224 | Done, final build | Per node, in the kill and other final runs | `analysis/results/kills/tables.md` |
| 11 | Tens of thousands of operations | Spec 278 | Done | 199,481,205 reports | Report section 1 |
| 12 | Repeated runs, statistically meaningful | Spec 66, 231, 279 | Done | 5 runs per cell overnight, with 95% intervals. Kills: 3 per case at 10,000 a second, 2 at 15,000, 1 at 20,000 and 300 | `cells.csv`, report section 11 |
| 13 | How long, which workloads, how evaluated | Notes Sept 9 | Done | Each run's length and load are in its `run.json`. Longest at 20,000 a second: about 18 min, stopped by a memory guard: see the limits below | Report section 13 |
| 14 | Automated data collection | Spec 278, 307 | Done | One script runs every experiment from client PC 1 over SSH | `scripts/phase1/` |
| 15 | Raw data, not only screenshots | Spec 279, 363 | Done | All raw logs are kept, and nothing under `runs/` is deleted | `runs/exp8h/`, on client PC 1 |
| 16 | Measurements tied to design decisions | Spec 280 | Done | Each result points to the decision it tests | Report, [design-log.md](design-log.md) |

## The setup

| # | Requirement | Source | Status | What we did | Where |
|---|---|---|---|---|---|
| 22 | Six physical PCs: 3 cloud, 3 load | Spec 30, 202 | Done | Every result comes from them. Logs name the client PC that sent and the node that answered | D258, `client_started` |
| 23 | Hundreds of clients on each client PC | Lecture 8, spec 31, 203 | Done, final build | 100 users per PC in T1 and T2, and 333 and 1,000 per PC | Report section 8a |
| 24 | A frontend controlling many background programs | Spec 203 | Done, final build, on one PC | With `curl`, on a local cluster: one report answered and committed, 100 users started in 10 programs, node status with one digest, and stopped. Not tried in a browser or on the six PCs. `docs/demo.md` says how its leader wording reads with no leader | `analysis/results/kills/frontend-check.txt` |
| 30 | Transparency: the user never names a server | Notes Sept 6, spec 41 | Done | Each report goes to all nodes, and the first answer wins. The application makes one call | `src/client.rs` |
| 31 | The nodes are peers, with no coordinator | Notes Sept 13, Oct 28 | Done | Each node won about a third of the work, and any node can order its own commands | D249 |
| 33 | The Raft leader changes when it is overloaded | No written source | **No longer applies** | There is no leader. An overloaded node now loses the bidding instead, as Scenario 2 shows | D249 |

## What you must do yourself

- Push the branch and open the pull request. Pushing needs your GitHub login.
- Fill in the report's section 15, team contributions.
- Add the Phase 0 survey, as it is not in the repo.

## Known limits to say out loud

- **Real power cuts are not tested,** only `kill -9`, which the professor allowed, as Noor reports.
- **Memory grows with the log,** about 590 bytes per report per node, and nothing trims it. At 20,000 a second a node PC runs out in about 18 to 20 minutes, so it cannot run for hours at this rate. At 1,000 a second it would take about 6.5 hours.
- **All three killed can lose a few answers:** 566 of 48,331,376, in 2 of 49 kill runs, when the node with the longest log returned last (report section 11).
- **A provisional answer usually changes:** in T2, about 98% ended with a different outcome, with 300 responders (report section 8a).
- **`plan_work` is simulated CPU work,** a busy loop that stands in for a harder planning job.
- **Final build at high load:** 30,000 a second has not been run on a quiet network. At midday it did not hold, and neither did the overnight build. 20,000 a second varied at midday.
- **A slow patch can feed itself,** through retries and takeovers (report section 11).
- **Corosync was not tested.** The closest library with no leader, `synevi`, gave different digests after a node stopped (D259).
