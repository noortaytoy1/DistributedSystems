# RescueMesh lab results, 2026-11-04

Every number here was read from the files in this folder or from `runs/`. Nothing is estimated unless it says "by arithmetic".

## The short version

**The final results are in [docs/phase1-report.md](../../docs/phase1-report.md):** 157 runs, 199,481,205 requests, 0 unaccounted, 0 digest mismatches. The ceiling is 12,000 requests a second with the planning load, and 30,000 with none, each in all 5 runs. **This page is the record of how we got there, on older code and with fewer repeats, and where it differs, that report is right.**

- **The plan ran in full:** 21 cells, 3 runs each, 15 s warm-up and 60 s measured, on six PCs. 1,372,598 operations. No run was left out.
- **Safety held everywhere it was checked.** In all 21 plan cells and all 31 runs with an `extras.txt` here: 0 digest mismatches, 0 `unaccounted`, 0 panics. `fault_split_decisions` and `reports_expired` were 0 in both summaries.
- **Failover works.** A new leader in about 800 ms at 300 users, and about 840 ms at 1500. Nobody gave up in those cells.
- **At the end of the session, with no leader, the system handles 10,000 requests a second, and its reliable ceiling on Wi-Fi is 30,000 a second with `plan_work` 0, and 12,000 with `plan_work` 1000.** 10,000 was clean in three runs and with a node killed twice. 30,000 was clean in three short runs and a 5-minute hold. On the final code 35,000 was clean and 36,000 fell, one run each. On the build before it, 33,000 was clean, 34,000 began to break and 35,000 fell. See "The ceiling" below. **With `plan_work` 1000, 10,000 was clean and 15,000 fell.** All of the rest of this list and the plan's tables are about the older code at `0397d37`, with Raft.
- **A limit that remains: two failures close together stop commits, and they did not resume for the rest of that run.** Answers stayed correct, with 0 digest mismatches, but 1,303 reports gave up in that test. One failure at a time recovers. See "The ceiling".
- **A real bug was found and fixed on the way:** digest mismatches came back twice on rotating rounds, and T4's last part found the second (D252, D257). Each fix has a test that fails without it.
- **T1 to T4 on rotating rounds ran with 3-minute loads and `kill -9`.** T3's plan steps, the plan's 21 cells and T5 were not repeated on rotating rounds, so their tables below are for Raft.
- **The system does not handle 10,000 requests a second on the code at `0397d37`.** With its plan settings, 600 a second was the highest fixed rate with no fall, in one 75 s run. 800 fell in 1 of 3 short tries and in its 5-minute hold. 1000 fell in all 4 tries.
- **T1 and T2 were simulated with `kill -9` over SSH, not real power cuts.** So the requirement to physically turn off a machine is only partly answered.

## What ran

| Item | Value |
|------|-------|
| Commit | `0397d37` on all six PCs for the plan, T1 to T5 and trials 1 to 3 |
| PCs | 3 node PCs, 3 client PCs |
| Client PC 1 | i7-13700, 24 threads, 15 GB memory, NVMe |
| Network | **The hosts file uses Wi-Fi addresses on all six.** Client PC 1's traffic to node 1 goes out by its cable. Only node 2 and client 1 have a cable plugged in, and node 2 used its Wi-Fi address |
| Pings | Client 1 to node 1: 16 to 99 ms, average 51, 3 packets. Between nodes: averages 6 to 11 ms, highest 19 ms, no loss, 20 packets each |
| Settings | `plan_work` 1000, unchanged. 3000 responders in `e1-1000-*`, `e1-3000-*` and T3, 300 elsewhere |
| Measured `service_mean_us` | About 2750 to 6300 in most bidding cells. About 10,650 in the skew cells and 22,894 in `e8-step-aside` |
| Clock skew | `node_clock_skew_ms` 3.0 to 5.1, `client_clock_skew_ms` 3.1 to 5.9 |

## Differences from the guide

1. **T1 and T2 used `kill -9`, on the request of the person running the lab.** The brief had asked for real power cuts. A real cut can lose unsynced disk writes. `kill -9` cannot show that.
2. **UDP buffers on the client PCs were below the guide's value** for the plan, T1 to T5, and trials 1 and 2: client 1 and client 3 at 212992, client 2 at 4194304. The nodes had 8388608. All were 8388608 for trial 3.
3. **The whole main batch was started without a separate trial cell or calibration here.** Section 3's notes were not part of this session.
4. **T3 was run twice.** The first pass started at 1000 a second and found no clean step, so a second pass added 200, 400, 600 and 800 with `LAB_RATES`, a setting the script's comment reserves for try-outs. The rows are in `stress-summary.txt` in time order, so 1000 appears twice. The first 1000, flat-out and spike rows stay valid.
5. **The second pass's repeat flat-out and spike steps were stopped on purpose,** to save time. The first pass's results remain.
6. **Other programs on the PCs.** Node 3 ran `target/release/rescue_mesh node` from `~/Projects/RescueMesh`, at about 0.1% CPU, on UDP 7003. It was there at 13:11 and gone by 15:57, stopped by something other than this session. VS Code, Firefox and system monitor were open on some PCs at low CPU. This Claude session ran on client PC 1. It also ran low-priority log reads and a few pings during some measured runs, including T2's first minute, and during trials 2 and 3.
7. **What the runner moved.** The first `e1-300-bidding` runs at this commit are in `runs/old/`. Earlier hand steps are in `runs/old-hand/`.

## The plan: 21 cells, 3 runs each

Mean of 3 runs. Gap is the longest with no answer at one client PC, worst run. Full data: `summary-plan.csv`. **3 runs give wide intervals, and these are closed-loop users: a slow cloud slows its users, so read p99 beside gave up and throughput.**

| Cell | Answered/s | p50 ms | p99 ms | Gave up | Retried | Gap ms | Elections |
|------|-----------:|-------:|-------:|--------:|--------:|-------:|----------:|
| `e1-30-bidding` | 28.0 | 60.0 | 138.6 | 0 | 0% | 792 | 0 |
| `e1-30-leader` | 29.0 | 29.4 | 125.9 | 0 | 0% | 588 | 0 |
| `e1-300-bidding` | 268.1 | 95.3 | 349.8 | 0 | 0% | 213 | 0 |
| `e1-300-leader` | 285.0 | 42.4 | 181.7 | 0 | 0% | 227 | 0 |
| `e1-1000-bidding` | 870.1 | 126.5 | 390.9 | 0 | 0.0% | 274 | 0 |
| `e1-1000-leader` | 934.6 | 60.2 | 181.5 | 0 | 0.2% | 168 | 0 |
| `e1-3000-bidding` | **286.8** | **6600** | **26006** | 304.7 | **98.8%** | 551 | 0 |
| `e1-3000-leader` | 1059.7 | 135.9 | 16245 | 92.7 | 41.4% | 256 | 0 |
| `e2e6-skew-expected-wait` | 270.0 | 92.0 | 276.1 | 0 | 0% | 216 | 0 |
| `e2e6-skew-expected-wait-cpu` | 272.5 | 85.7 | 244.3 | 0 | 0% | 165 | 0 |
| `e3e4-300-follower` | 269.3 | 94.8 | 283.1 | 0 | 0% | 206 | 0 |
| `e3e4-300-leader` | 268.3 | 89.8 | 515.6 | 0 | 0.1% | 969 | 1 |
| `e3e4-1500-follower` | 358.5 | 2785 | 9416 | 0 | 70.2% | 644 | 0 |
| `e3e4-1500-leader` | 614.2 | 492.7 | 7062 | 0 | 30.0% | 1445 | 1 |
| `e4-300-silence-20s` | 265.1 | 104.7 | 396.1 | 0 | 0% | 211 | 0 |
| `e5-faults-off` | 267.6 | 97.1 | 336.7 | 0 | 0% | 190 | 0 |
| `e7-p1-300` | 269.3 | 93.7 | 280.0 | 0 | 0% | 200 | 0 |
| `e8-step-aside` | 267.0 | 94.8 | 500.9 | 0 | 0.1% | 927 | 1 |
| `ft8-random-faults` | 266.0 | 92.4 | 661.1 | 0 | 0.1% | 958 | 0.7 |
| `ft8-election-rate` | 229.0 | 104.3 | 2844 | 0 | 6.1% | 3917 | 7.7 |
| `degraded-300` | 240.9 | 98.2 | 5742 | 0 | 4.2% | 3604 | 55 |

**Failover** (`failover_gap_ms`, mean and worst run): `e3e4-300-leader` 801.8 and 831.0. `e3e4-1500-leader` 840.9 and 865.9. `e8-step-aside` 778.2 and 808.2. `ft8-random-faults` 791.5 and 795.2. `ft8-election-rate` 902.3 and 2733.3. `degraded-300` 3419.9 and 3426.0. In `degraded-300` nodes 1 and 3 are silenced, so there is no leader until one returns.

**Leader changes**, read from the logs: node 1 to node 2 in all 3 runs of each `e3e4-*-leader` cell and of `e8-step-aside`. No leader change in any `e3e4-*-follower` or `e4-300-silence-20s` run.

**Load sharing** (node 1, 2, 3, then Jain): `e1-300-bidding` 0.14, 0.14, 0.72, 0.594. `e1-1000-bidding` 0.24, 0.24, 0.52, 0.862. `e1-3000-bidding` 0.43, 0.31, 0.26, 0.954. `e1-*-leader` all on node 1.

**`degraded-300`:** 4966 provisional answers and 3801 provisional conflicts per run. Clients are not told of these. Its `during_*` figures mix provisional and committed answers.

**Operations, in all and per cell (`count` + `provisional_count` + `gave_up`, times runs):** all 1,372,598. `e1-30-bidding` 5,047 and `e1-30-leader` 5,214 are under tens of thousands, on purpose. Every other cell is above 40,000. The largest is `e1-3000-leader` at 191,028. One fault per run, so 3 per cell: too few to show a distribution.

## T1: leader stopped under load, then back. SIMULATED with `kill -9`

Node 1 was stopped at 15:52:29. Folder: `runs/hand/power/leader/run-1`.

| Check | You want | Measured | Result |
|-------|----------|----------|--------|
| New leader | Within 1 s | Node 1, term 3, to node 2, term 4. `failover_gap_ms` 777.6 | Pass |
| Longest with no answer | Rehearsal 686 ms | 863 to 867 ms | Read |
| Gave up | 0 | 0 | Pass |
| Retried | | 8, 10 and 8 on the three client PCs | Read |
| `peer_suspected` on both survivors | Within 1 s | 2 lines on node 2, 3 on node 3, about the stopped node, `silent_ms` 150 to 198. Some may come from startup or shutdown | Read |
| `restarted`, `rejoined` | | Both logged, `recovery_us` 962,524 | Pass. Rehearsal was 34 ms |
| Digests, mismatches | Equal, 0 | Equal, 0 | Pass |
| Unaccounted | 0 | 0 | Pass |

## T2: one node stopped, one silenced, one server left. SIMULATED with `kill -9`

Folder: `runs/hand/oneleft/300/run-1`.

| Check | You want | Measured | Result |
|-------|----------|----------|--------|
| Provisional on each client PC | Above 0 on all three | 1,666, 1,636, 1,673 | Pass |
| Gave up | 0 | 0 | Pass |
| Longest with no answer | Rehearsal about 3.2 s coming back | 3466 to 3553 ms | Read |
| Degraded | | Node 3, 17,799 ms, 4,975 provisional, left with `peer_trusted` | Read |
| Conflicts | Expected at this load | 3,908 `responder_taken`, 8 `lost_first_commit`, of 4,975 | Read |
| Members | `3`, then `2,3`, then `1,2,3` | `3`, then `2,3`, then `1,2,3` | Pass |
| Digests at the end | Equal | Equal, 0 mismatches | Pass |

## T3: stress to the breaking point, plan settings

"Answered" includes provisional answers, so it is an upper limit on real work. Verdict by the script: gave up above 0, or p99 at 1 s or more, is fallen. Fallen steps were moved to `runs/hand-fallen/stress/`, and their rows come from the programs' lines. Worst p50 and p99 are the worst of the 9 programs.

| Step | Offered/s | Answered/s | Gave up | Retried | Worst p50 ms | Worst p99 ms | Verdict |
|------|---------:|----------:|--------:|--------:|------------:|------------:|---------|
| rate-200 | 200 | 198.0 | 0 | 1 | 89.9 | 205.0 | clean |
| rate-400 | 400 | 396.0 | 0 | 0 | 101.5 | 282.1 | clean |
| rate-600 | 600 | 603.0 | 0 | 2 | 114.5 | 328.3 | clean |
| rate-800 | 800 | 801.0 | 0 | 1 | 140.7 | 445.4 | clean |
| rate-800b | 800 | 659.1 | 8,518 | 31,695 | 11,740 | 29,065 | **fallen** |
| rate-800c | 800 | 801.0 | 0 | 1 | 158.1 | 515.0 | clean |
| long-800, 5 minutes | 800 | 758.9 | 11,994 | 21,323 | 154.5 | 24,982 | **fallen** |
| rate-1000, first pass | 1000 | 569.8 | 25,757 | 25,733 | 7,531 | 27,807 | **fallen** |
| rate-1000, second pass | 1000 | 824.1 | 10,491 | 28,195 | 2,872 | 27,916 | **fallen** |
| rate-1000b | 1000 | 756.4 | 14,559 | 29,999 | 6,420 | 27,561 | **fallen** |
| rate-1000c | 1000 | 591.8 | 24,430 | 25,822 | 7,852 | 29,703 | **fallen** |
| rate-2000 | 2000 | 1890.8 | 6,431 | 83,492 | 6,766 | 26,465 | **fallen** |
| rate-3000 | 3000 | 2684.3 | 18,757 | 127,115 | 11,842 | 29,157 | **fallen** |
| rate-4000 | 4000 | 2643.7 | 81,138 | 142,097 | 21,174 | 31,439 | **fallen** |
| rate-5000 | 5000 | 2699.1 | 138,292 | 154,136 | 23,806 | 31,414 | **fallen** |
| rate-6000 | 6000 | 2404.1 | 215,938 | 143,009 | 25,637 | 31,615 | **fallen** |
| rate-8000 | 8000 | 2726.9 | 316,468 | 163,599 | 40,061 | 47,354 | **fallen** |
| rate-10000 | 10000 | 2904.9 | 425,645 | 174,295 | 47,957 | 67,562 | **fallen** |
| flat-900 | no pause | 687.9 | 0 | 9,858 | 948.0 | 3,684 | counted fallen on p99 |

| From `summary-hand.csv` | Answered/s | p50 ms | p99 ms | Gave up | Retried | `elections` | `takeovers_backstop` | `client_pc_cpu_busy_p95` |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| `flat-900` | 687.9 | 903 | 3524 | 0 | 23.9% | 0 | 65,170 | 0.019 |
| `spike-100` | 283.4 | 241.2 | 2296 | 0 | 3.1% | 0 | 10,570 | 0.023 |
| `spike-333` | 629.1 | 510 | 2542 | 0 | 31.4% | 0 | 14,710 | 0.015 |

**Reading it.** There is no gentle knee: below the fall p99 stays under about 0.5 s, and past it the p99 is in tens of seconds. The client PCs were never the limit, with `client_pc_cpu_busy_p95` at about 0.02. It is a tipping point: the same 800 a second both held and fell.

**At the fall, most answers are provisional, not committed.** Client programs' provisional share in the fallen steps at 2000, 5000 and 10000 was 99.4%, 100% and 100%, and 67.8% in the second-pass 1000 step. So those "answered" figures do not show committed work.

## Why it falls: what the logs show

Second by second, at 800 a second, in the step that fell (`rate-800b`), summed over the nodes:

| Second | Received/s | Commits/s | Commit p50 ms | Takeovers/s |
|-------:|----------:|---------:|-------------:|-----------:|
| 5 to 9 | about 2,400 | about 800 | about 85 | about 78 |
| 10 | 2,334 | 521 | 221 | 604 |
| 13 | 3,098 | 212 | 1,155 | 931 |
| 16 | 4,707 | 135 | 2,946 | 1,666 |
| 18 onward | 5,882 and rising | about 0 | at the 3 s limit | 1,750 and rising |

1. A commit takes about 85 ms when healthy. About that time one peer logged 157 ms of silence. Whether a network delay started it is not proven.
2. Past 200 ms, `BACKSTOP_MIN` makes nodes that lost the bid plan the report too. Stale plans and commands rise, and commits slow further. This is a feedback loop.
3. Clients retry after about 2 s, so a node that normally receives about 2,400 requests a second soon receives over 5,000.
4. Elections come after this, as a result. In `rate-1000` the term stayed at 3 for at least 45 s, then rose to about 90.

**A second limit, by arithmetic from the logs.** The leader sends one write at a time, of at most `GROUP_LIMIT` = 200 commands. In trial 2 at 1000 a second: 21.9 writes a second, the queue busy 99.8% of the time, so about 46 ms a write, with about 3.3 ms in each disk sync. 2,666 commands a second, so about 2.67 a report. So the ceiling is about 4,350 commands, or about 1,630 reports a second, whatever the threads.

**Not the limit:** UDP drops on the nodes stayed at 0 during the measured runs, and traffic was at most 16 Mbit/s per node in the 1000 step. **Not measured:** the client PCs' network, and drops on the client PCs. **Not tested:** whether a wired network would change this.

## T4: the failure draw on real timers

| Check | You want | Measured | Result |
|-------|----------|----------|--------|
| Rounds | About 10 | 10 in `draw-300`, 3 lines each, 30 lines, 0 null victims | Pass |
| One victim per round | Yes | Yes | Pass |
| A round called by a follower | At least 1 | Round 1, called by node 3, leader node 1 | Pass |
| A round electing a follower | At least 1 | Round 1, victim node 2, leader node 1 | Pass |
| Draw time | Rehearsal about 0.25 ms | Mean 18.2 ms, 6.7 to 44.0 ms | Read |
| Gave up, digests | 0, equal | 0, equal, `fault_split_decisions` 0 | Pass |
| One node off: no draw goes ahead | `"victim":null` within seconds | Within about 10 s every new line was null, then normal draws after its restart | Pass |

Also read: `draw-300` throughput 263.9 a second, p99 1,086 ms, worst client gap 4,600 ms. `draw-off` 238.1 a second, p99 2,453 ms, worst client gap 2,752 ms, 27 rounds abandoned, 25 elections.

## T5: 5 retries, then an error

**Pass.** `client_sent` 6 (want 6), and `client_gave_up` with `"attempts":6`. See `t5.txt`.

## Trials with other settings, kept apart from the results above

These used changed settings at commit `0397d37`, and are in their own folders. **Never mix them with the plan's figures.**

**Trial 1, `runs/hand-tuned`: INVALID. Ignore these numbers.** 30000 responders is one Raft entry too big for the 50 ms budget, so registration never committed. That was this session's mistake, and it says nothing about threads. Its 24-thread step was stopped early, and SSH to node 1 took over a minute while it ran.

**Trial 2, `runs/hand-tuned2`:** `threads` 20, `responders` 6000, `raft_heartbeat_ms` 100, election 400 to 550 ms, `takeover_missed_heartbeats` 6, `plan_work` 1000. Client buffers below the guide's value. Failover not measured on these settings.

**Trial 3, `runs/hand-tuned3`:** the same, with all client buffers at 8388608.

| Trial, offered/s | Answered/s | Measured answers | Of them provisional | Gave up | Worst p99 ms | Final term | Verdict |
|---|---:|---:|---:|---:|---:|---:|---|
| 2, 1000 | 999.0 | 59,940 | 0 | 0 | 465.9 | 2 | clean |
| 2, 2000 | 1991.1 | 119,464 | 119,397 | 420 | 25,359 | 79 | **fallen** |
| 2, 5000 | 4647.5 | 278,847 | 276,159 | 21,394 | 30,496 | 102 | **fallen** |
| 2, 10000 | 6126.0 | 367,559 | 364,286 | 232,381 | 62,773 | 136 | **fallen** |
| 3, 1000 | 732.9 | 43,978 | 31,146 | 15,962 | 30,045 | 13 | **fallen** |
| 3, 1500 | 1261.8 | 75,710 | 73,694 | 14,473 | 27,364 | 76 | **fallen** |
| 3, 10000 | 6454.2 | 387,251 | 384,971 | 212,689 | 62,262 | 123 | **fallen** |

**Reading it.** These settings did not raise the real limit reliably: 1000 held once and fell once. Bigger client buffers did not fix the fall, on these few runs. One run per step, so this is weak evidence either way.

## Raised limits on a new build

**Commit `4d67039`, local branch `shared-raise-throughput-limits`, not on GitHub and not merged.** It changes `GROUP_LIMIT` from 200 to 2000 (D234) and `BACKSTOP_MIN` from 200 ms to 1000 ms (D235). Formatting, clippy and `cargo test` passed. Folder: `runs/hand-newbuild`. Settings as trial 2, with all client buffers at 8388608. **All six PCs now run this build, so a rerun of the plan at `0397d37` needs a rebuild.** Failover was not measured on it.

| Offered/s | Answered/s | Measured answers | Of them provisional | Gave up | Worst p99 ms | Stale per commit | Final term | Verdict |
|---:|---:|---:|---:|---:|---:|---:|---:|---|
| 1000 | 999.0 | 59,940 | 0 | 0 | 440.8 | 0.21 | 3 | clean |
| 1000, repeat | 999.0 | 59,940 | 0 | 0 | 461.2 | 0.21 | 3 | clean |
| 2000 | 1791.2 | 107,473 | 66,114 | 12,413 | 28,956 | 1.32 | 30 | **fallen** |
| 5000 | 4775.1 | 286,507 | 286,375 | 13,737 | 29,509 | 0.95 | 93 | **fallen** |
| 10000 | 6276.9 | 376,603 | 371,133 | 223,337 | 61,194 | 2.16 | 162 | **fallen** |

**10,000 a second was not reached on this build.** It is one step better than the plan's settings: 1000 held twice, with no provisional answers. 2000 ran for about 30 s at about 2,000 commits a second, then fell. All 5 steps had 0 digest mismatches, 0 `unaccounted` and 0 panics.

**Next limit, read from the 2000 step.** The log carried up to about 8,000 commands a second, past the old ceiling of about 4,350, with writes of 67 to 112 ms. Stale plans ran at 2,048 to 2,584 a second against about 2,000 commits. At about 30 s they reached 3,605 a second, refused plans 1,711 a second, and writes 134 to 181 ms, and then commits fell to about 0. A reading, not proven: a plan picks a responder from what its node knows, and other plans can pick the same one during the roughly 100 ms a commit takes.

**The same build with `plan_work` 100, folder `runs/hand-newbuild-pw100`.** This is a tenth of the simulated planning work, so it is not the same claim as the rows above.

| Offered/s | Answered/s | Measured answers | Of them provisional | Gave up | Worst p99 ms | Stale per commit | Final term | Verdict |
|---:|---:|---:|---:|---:|---:|---:|---:|---|
| 2000 | 1973.3 | 118,396 | 106,894 | 1,486 | 27,035 | 1.12 | 37 | **fallen** |
| 5000 | 4012.9 | 240,772 | 240,380 | 59,470 | 30,095 | 1.11 | 105 | **fallen** |
| 10000 | 5464.1 | 327,849 | 327,805 | 272,091 | 62,448 | 0.71 | 168 | **fallen** |

**Lowering `plan_work` did not help.** At 2000 a second the nodes used only 2.0 to 5.1 CPU cores with almost no plan queue, and it still fell, after about 20 s. So planning CPU is not what stops it at this load. Stale plans stayed at about 1 per commit. Nodes 1 and 2 also counted UDP drops in this step (163,817 and 64,058 during the run), which the earlier steps read did not show. Their timing against the collapse was not read, so this is open. All 3 steps had 0 digest mismatches, 0 `unaccounted` and 0 panics.

**The same build with 60 zones, folder `runs/hand-newbuild-zones60`.** All three steps fell: 2000 (answered 1,931.4 a second, 1.53 stale per commit), 5000 (4,588.9, 3.00), 10000 (5,953.6, 2.62). More zones did not help.

**The same build and settings on one PC over loopback, folder `runs/hand-loopback`. For comparison only, never a six-PC result.** All three nodes and all nine programs shared client PC 1, so there was no network between PCs, and CPU was shared.

| Offered/s | Answered/s | Provisional | Gave up | Worst p99 ms | Stale per commit | Final term | Verdict |
|---:|---:|---:|---:|---:|---:|---:|---|
| 2000 | 1998.0 | 0 | 0 | 45.9 | 0.04 | 2 | clean |
| 3000 | 2997.0 | 0 | 0 | 57.5 | 0.09 | 2 | clean |
| 5000 | 2723.4 | 0 | 136,841 | 30,801 | 0.77 | 2 | **fallen**, plan queues full (mean about 57), the PC shared with all the load |
| 10000 | not collected | | | | | | interrupted by a session restart, and the programs' lines also show it falling |

**The direct comparison, seconds 5 to 25 of each run, same build and settings:**

| | Six PCs, 1000/s | Six PCs, 2000/s | Loopback, 2000/s | Loopback, 3000/s |
|---|---:|---:|---:|---:|
| Bid rounds that saw all 3 bids | 84.8% | **26.0%** | 100% | 100% |
| Rounds a node thought it won, should be 33% | 36.7% | **55.9%** | 33.3% | 33.3% |
| Commit results: committed | 19,982 | 39,801 | 39,951 | 59,914 |
| Commit results: stale | 3,909 | 45,648 | 1,666 | 5,386 |
| Commit results: duplicate | 2,037 | 27,141 | 0 | 0 |
| Commit p50, p99 ms | 78, 181 | 167, 692 | 6.6, 10.6 | 7.0, 11.8 |
| One write, ms | 42.0 | 84.2 | 4.3 | 4.6 |
| One disk sync, ms | 3.2 | 4.2 | 2.2 | 2.3 |

**What this shows.** The difference between these runs is the path between PCs and the separate disks, and loopback also had less CPU per node. A write takes about 10 to 20 times longer between PCs, with disk syncs under 2 times longer. So the time is not in the disk. At 2000 a second on six PCs most bid rounds close before all bids arrive inside the 20 ms `bid_window_ms`, so more than one node believes it won, plans and commits. Most commit results are then waste: 72,789 stale or duplicate against 39,801 committed.

**Not settled.** Why bids and writes are slow between PCs: Wi-Fi delay under this traffic, TCP behaviour, or something else. Idle pings between nodes averaged 3.9 and 6.1 ms, so the delay appears under load. During one 10000 step, SSH to nodes 1 and 2 timed out, and one TCP sample on node 3 read a round trip of about 11 s. The log audit stopped when the session restarted and did not report.

**Reviews of the code and logs, 2026-11-04 evening.** They found a real bug: `campus::responders` tied zone and capability together, so only 12 of the 24 pairs existed at 6 zones (60 of 240 at 60 zones, which is why more zones did not help). **The plan at `0397d37` and everything above this line ran with that bug.** They also found costs that grow under load. Fixes are in D238 to D246 on the local branch `shared-raise-throughput-limits`, not merged and not on GitHub.

**Other measured setup differences:** nodes 1 and 2 used CPU energy setting `balance_performance`, and node 3 used `performance`. Plans took 9.4, 7.7 and 3.2 ms, and node 3 won 61% of rounds, though a single-core speed test came out equal. That may explain node 3's large share in the plan's cells. The cause is supported, not proven. The setting on nodes 1 and 2 was changed by the person running the lab, and resets on restart. It stayed as set for everything below this paragraph that says "performance".

**The build with the fixes so far, folder `runs/hand-fixed`, commit `0790478`.** 12,000 responders, `threads` 20, `plan_work` 1000, `raft_heartbeat_ms` 500, election 2000 to 2400 ms, `bid_window_ms` 60, all client buffers 8388608, CPU setting `performance`, UDP combined within 2 ms. Failover was not measured on these settings, and the longer election timers will slow it.

| Offered/s | Answered/s | Provisional | Nobody sent | Gave up | Retried | Worst p99 ms | Final term | Verdict |
|---:|---:|---:|---:|---:|---:|---:|---:|---|
| 2000 | 1998.0 | 0 | 0 | 0 | 0 | 327.3 | 2 | **clean** |
| 3000 | 2997.0 | 0 | 0 | 0 | 17 | 490.5 | 2 | **clean** |
| 5000 | frozen | | | 300,242 | | | | **deadlock, a bug in that build** |

**The freeze at 5000 was this session's own bug,** in the retry guard of D243, found from a core dump of the frozen leader: all 24 worker threads were waiting on one lock. It is fixed at `d544a35`, with a test. The 2000 and 3000 rows ran with that bug present, but it only fires once retries begin.

## No leader: rotating rounds (D249)

**The person running the lab reports that the class notes and the professor rule out a leader or coordinator.** So Raft was replaced by rotating rounds, behind `consensus = "rounds"`. No node is elected, and all apply one order. Commit `72149ec`, local branch, not merged. Settings: `plan_work` 0, 24,000 responders, `threads` 20, `bid_window_ms` 60, UDP combined within 10 ms, a node dropped after 1 s of silence, CPU setting `performance`, client buffers 8388608.

**`plan_work` 0 means no simulated planning load, so these are not comparable with the plan's `plan_work` 1000.** Logs were cut to key lines at these rates to save copying time, so client-side `unaccounted` was not read, and committed results come from `status-after.txt`.

| Offered/s | Answered/s | Gave up | Provisional | Worst p99 ms | Final results, by node | Digest mismatches | Node drops | Verdict |
|---:|---:|---:|---:|---:|---|---:|---:|---|
| 10000 | 9999.0 | 0 | 0 | 507.5 | 749,925 on all three, same digest | 0 | 0 | **clean** |
| 12000 | 3142.9 | 531,248 | 0 | 70,411 | 411,164 and 661,088 and 661,088 | 0 | 6 | **fallen** |

At 10000 each node used about 1.3 CPU cores, received about 14 Mbit/s and sent about 8. Pings between nodes averaged 22 to 32 ms, against 4 to 6 ms idle. One run per step.

**That result was one run, and its repeat fell, with digest mismatches. See "The ceiling" below.** **Known limit:** a restarted node recovers from a peer's log and not its own file (D249).

## The ceiling: about 33,000 to 35,000 a second with no leader

**Later runs corrected this heading.** Over 5 runs on the final code, 30,000 held in all 5, 33,000 and 35,000 in 3 of 5, and 36,000 in 1 of 5. See [docs/phase1-report.md](../../docs/phase1-report.md).

**Code:** rotating rounds, commits `cdec322` and `eba4d14`, branch `shared-raise-throughput-limits`. **Network:** the same Wi-Fi as everything else here, with node 2 and client 1 sending by cable. Nine load programs, 75 s each, 15 s of it warm-up. Log lines were cut to key lines at these rates, so `unaccounted` was not read, and "digest mismatches" compare fixed-index checkpoints on all three nodes. **`plan_work` 0 unless a row says otherwise, which means no simulated planning load.** Full figures are in `ceiling-summary.txt`.

**What changed to get here,** after the run at `72149ec` that had looked clean fell on its repeat, at 3,347.9 answered a second with 53 digest mismatches:

| Change | Decision | Measured after it |
|---|---|---|
| A promise to drop a node is kept, and a returning node is told its round | D252 | Digest mismatches back to 0, in runs with drops. Three tests fail without it |
| Slow moments no longer replace connections or cut off connects | D252 | 0 false drops in every run below |
| No scan of the whole responder table per request | D253 | 0.5 CPU cores per node at 10,000, against about 1.3 |
| Client datagrams leave out tarpc's trace context | D254 | With the row below: about 262 bytes per request at 10,000, against about 600. **By arithmetic** from the Mbit/s received on all PCs |
| Datagrams and TCP frames are squeezed | D255 | As above |
| Messages wait longer to share packets | D256 | 30,000 went from falling to clean |

**The steps at the builds before the final one, in order of rate. 60,000 responders and 1 s detection unless a row says otherwise. The final code's steps come further down.**

| Offered/s | Answered/s | Gave up | Retried | No responder | Worst p50 ms | Worst p99 ms | Mismatches | Drops | Verdict |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|
| 10000 | 9999.0 | 0 | 252 | 0 | 153.9 | 298.4 | 0 | 0 | **clean** |
| 10000, 24,000 responders, 3 s detection | 9999.0 | 0 | 229 | 0 | 155.7 | 327.2 | 0 | 0 | **clean** |
| 15000, 24,000 responders, 3 s detection | 15003.0 | 0 | 1,832 | 0 | 180.7 | 664.0 | 0 | 0 | **clean** |
| 20000 | 19998.0 | 0 | 13 | 0 | 174.7 | 345.7 | 0 | 0 | **clean** |
| 20000, 24,000 responders, 3 s detection | 19998.0 | 0 | 5,595 | 0 | 200.9 | 666.6 | 0 | 0 | **clean** |
| 25000, 3 s detection | 25002.0 | 0 | 31,578 | 0 | 227.5 | 2,596.9 | 0 | 0 | **clean** |
| 30000 | 29997.0 | 0 | 232,470 | 0 | 290.4 | 2,416.5 | 0 | 0 | **clean, about 13% retried** |
| 30000, 3 s detection | 29997.0 | 0 | 49,257 | 0 | 264.9 | 2,323.0 | 0 | 0 | **clean** |
| 31000 | 30996.1 | 0 | 16,098 | 0 | 222.0 | 1,824.7 | 0 | 0 | **clean** |
| 32000 | 32004.0 | 0 | 26,428 | 0 | 228.9 | 1,922.2 | 0 | 0 | **clean** |
| **33000** | **33003.0** | **0** | **42,808** | **0** | **330.6** | **2,309.5** | **0** | **0** | **the highest clean step at `eba4d14`** |
| 34000 | 34001.2 | 52 | 477,293 | 0 | 527.0 | 12,312.0 | 0 | 0 | **starts to break** |
| 35000, 3 s detection | 34873.3 | 7,662 | 1,353,826 | 0 | 31,605.7 | 47,474.1 | 0 | 0 | **fallen** |
| 40000, 3 s detection | 39882.7 | 6,799 | 1,240,748 | 73 | 26,509.1 | 42,841.4 | 0 | 0 | **fallen** |

**The 5-minute hold, folder `runs/hand-r15`, commit `eba4d14`, 60,000 responders, 1 s detection.** 300 s measured at 30,000 a second: 8,999,100 done, which is 29,997.0 a second, 0 gave up, 51,287 retried, 0 with no responder. Worst p50 234.9 ms, p95 468.8 ms, p99 900.1 ms. 0 digest mismatches of 1,034 checkpoints, 0 drops. Each node used about 1.5 CPU cores and reached about 5.2 GB of memory.

**Read the p99 with care.** A client retries after about 2 s, so a p99 just over 2,000 ms means some answers needed a retry. From 25,000 up, p99 is high though p50 is low. 31,000 to 40,000 are one run each.

**At 35,000 and 40,000 the answered figure is not a real rate.** The nodes kept committing, but half the answers in the worst program took over 26 s.

**With 24,000 responders the limit is the responders.** At 25,000, 231,181 answers said no responder was free, and at 30,000, 496,356. They answered at full rate, but those are not useful answers, so the table uses 60,000.

**What limits it.** At 33,000 each node used about 1.5 CPU cores, and the kernel dropped no datagrams at 25,000 and 30,000. So it is not CPU or sockets. Each packet and byte crosses shared air, and fewer of them moved the ceiling up, so the air is the likely limit. This was not measured directly.

**With `plan_work` 1000, the plan's simulated planning work,** same settings:

| Offered/s | Answered/s | Gave up | Retried | Worst p50 ms | Worst p99 ms | Mismatches | CPU cores per node | Verdict |
|---:|---:|---:|---:|---:|---:|---:|---:|---|
| 10000 | 9999.0 | 0 | 47 | 165.2 | 289.5 | 0 | about 11 to 12 | **clean** |
| 15000 | 14863.7 | 8,356 | 409,012 | 8,462.3 | 27,611.0 | 0 | about 20 | **fallen** |
| 20000 | 19351.9 | 38,765 | 897,877 | 36,877.0 | 53,005.6 | 0 | about 20 | **fallen** |

So with this load the limit is CPU, with all 20 planning threads busy. **This matters for the report:** D129 says the spec wants real planning load, so the ceiling should be quoted with `plan_work` 0 beside it. With `plan_work` 1000 the highest rate shown clean is 10,000.

**A node killed at 10,000 a second,** folder `runs/hand-rf/stress/fail-10000`, 120 s. `kill -9` on node 3 at 22:54:06.6, started again at 22:54:39.2:

| Measure | Result |
|---|---|
| Answered/s, offered 10000 | 9999.0 |
| Gave up, retried | 0, 3,430 |
| Worst p50, p99 | 144.3 ms, 380.6 ms |
| Others dropped node 3 | 1.0 s after the kill: 22:54:06.6 to 22:54:07.6 |
| Node 3 back as a member | 22:54:55.5, 16.3 s after it started again |
| Final state | 1,199,880 results on all three, same digest, 0 mismatches of 377 checkpoints |

**T1, T2 and T4 on rotating rounds, folder `runs/hand-rf`, commit `eba4d14`.** The plan's cells with `plan_work` 1000, 300 users, and **3-minute loads, not the guide's 5 minutes,** to make the deadline. **T1 and T2 simulated with `kill -9`, not real power cuts.**

| Test | Sent | Answered | Gave up | Unaccounted | Digest mismatches | Longest gap to any client group | Notes |
|---|---:|---:|---:|---:|---:|---:|---|
| T1, a node killed and back | 49,326 | 49,326 | 0 | 0 | 0 of 307 | 251 ms | Others dropped it within a second. It rejoined 1.0 s after starting. 49,326 results on all three, same digest |
| T2, one killed, one silenced for 20 s, one left | 48,363 | 48,363 | 0 | 0 | 0 of 306 | 2,454 ms | Node 3 alone answered provisionally for 17.7 s, 4,780 answers. Later conflicts: 4,391 `responder_taken`, 269 `lost_first_commit`, against 3,908 and 8 with Raft. All three ended with one digest |
| T4, failure draw on real timers | 49,132 | 49,132 | 0 | 0 | 0 of 300 | 344 ms | 18 draw lines. Node 3 was still silenced in the final status, as a draw had just gone off. Digests matched |
| T4's last part, a failure about every 2 s and one node killed | 7,813 | 6,572 | 1,241 | 0 | **41 of 50** | 3,131 ms | **A real bug. Logs kept in `runs/hand-rf/draw/off`. Fixed by D257** |

**That bug and its fix.** Node 2 was dropped with its stream ending at round 1329, and came back at round 1186, so those rounds held old and new batches. D257 starts a returning node past the end of its old stream, and has every member read the entry that adds it in one view. `a_returning_node_starts_past_its_old_stream_on_every_node` fails without the fix.

**The same test after the fix, folder `runs/hand-rg/draw/off`, commit shown in `ceiling-summary.txt`:**

| Test | Sent | Answered | Gave up | Unaccounted | Digest mismatches | Longest gap | Final state |
|---|---:|---:|---:|---:|---:|---:|---|
| T4's last part | 9,892 | 8,589 | **1,303** | 0 | **0 of 50** | 3,891 ms | 8,589 results on all three, one digest, **but node 3 not back and no commits after the kill** |

**This is a limit that remains, and it is not small.** Safety held, but the system stopped committing from when node 3 was killed, 21:36:53, to the end of the run. Nearly all the 1,303 that gave up came after that. Node 2 had just been dropped by a draw and was on its way back. With node 3 killed as well, node 1 was left in a view of two, and it cannot drop anyone alone. Node 3 then restarted, still listed, and cannot come back either. **So two failures close together, the second before the first has returned, stop commits. They did not resume for the rest of that run. What brings them back was not tested, and restarting all three would lose the state (D249).** A single failure does not do this, as T1, T2 and the kill above show. Raft would carry on here, since two of three nodes were up.

**On the final code, `c05c7b9`, folder `runs/hand-rg`, 60,000 responders, 1 s detection, `plan_work` 0:**

| Test | Answered/s | Gave up | Retried | Worst p50 ms | Worst p99 ms | Digest mismatches | Final state |
|---|---:|---:|---:|---:|---:|---:|---|
| 10000 | 9999.0 | 0 | 136 | 149.4 | 262.4 | 0 of 266 | |
| 30000 | 29997.0 | 0 | 6,052 | 216.2 | 543.4 | 0 of 248 | |
| 33000 | 33003.0 | 0 | 246,136 | 306.8 | 2,430.7 | 0 of 248 | About 12% retried |
| 34000 | 34002.0 | 0 | 51,104 | 272.3 | 2,175.0 | 0 of 236 | Clean here, but 52 gave up and p99 was 12,312 ms at `eba4d14`, so it is at the edge |
| 35000 | 35001.0 | 0 | 185,463 | 305.3 | 2,453.7 | 0 of 239 | **The highest clean step on the final code, one run.** About 9% retried |
| 36000 | 35866.9 | 7,988 | 1,125,047 | 22,659.1 | 39,524.5 | 0 of 226 | **Fallen** |
| 10000, node 3 killed at 23:40:51.3 | 9999.0 | 0 | 5,150 | 143.4 | 530.5 | 0 of 376 | Dropped 1.0 s later. Started again at 23:41:24.1, back at 23:41:40.1, which is 16.1 s. 1,199,881 results on all three, one digest |

**One request followed through every log** is in `request-trace-example.txt`. In the 25 s run it came from, all three nodes held 249,975 results with one digest. Node 1 executed 89,044, node 2 77,096 and node 3 83,835.

**Limits, said openly:**
- Apart from the hold, these were 75 s runs, on Wi-Fi, with one run for most rates.
- Memory grows with the log, about 5.2 GB per node after 5 minutes at 30,000. Nothing trims it yet, so a much longer run would run out.
- **Two failures close together stop commits,** as above. No wrong answers, but nothing commits, and what brings it back was not tested.
- A restarted node recovers from a peer's log, not its own file, so losing all three at once loses the state (D249).
- If a node promises a drop and the asker then dies, it waits for that node (D252).
- Detection was 3 s in some rows and 1 s in others. The user first picked about 300 ms. 1 s held up to 33,000, and 300 ms was not tried at these rates.

## Odd things

- `e1-3000-bidding` answered 286.8 a second, and `e1-3000-leader` 1059.7, on the same load. Bidding collapses first here.
- `e3e4-1500-*` run past their pool of 300 responders, as the guide warns: `no_responder_share` 0.73 and 0.57.
- Node 3 takes most of the work at low load: 72% at 300 users. Not looked into.
- `ft8-election-rate` has `elections_after_rejoin` about 40 per run, against a target of 0. Failures come very often in this cell, and this was not looked into.
- `draw-300` and `draw-off` ended with node 2 still silenced in `status-after.txt`, despite the 21 s wait. The guide wants no silenced row. Digests matched.
- Final digests differed in `status-after.txt` for rate-2000, 3000, 5000 and 6000, at different log positions, as nodes were stopped right after. Every fixed-index checkpoint matched. It cannot be shown that these would have converged, because the nodes were stopped.
- `/etc/sudoers` on client PC 1 has a syntax error at line 228, shown when `sudo` ran. It did not stop `sudo`. Not touched.

## What could not be run, or was left out

- **A real power cut.** T1 and T2 used `kill -9`.
- `power-follower`, the optional repeat of T1.
- The other 13 cells, and the `full` profile.
- `docs/demo.md`, win counts, and a page screenshot.
- `ctl incidents` and `du -sh data/raft`, which the guide lists for some report slots.
- `leader_stepped_aside` details for `e8-step-aside`, which were not read by hand.
- `cargo test` on `0397d37`.

## Files here

`summary-plan.csv`, `summary-hand.csv`, `stress-summary.txt`, `tuned-stress-summary.txt` (invalid), `tuned2-stress-summary.txt`, `tuned3-stress-summary.txt`, `t5.txt`, `extras-<folder>-<experiment>-<cell>.txt` for each hand run, and the plots as `plan-*.png` and `hand-*.png`.
