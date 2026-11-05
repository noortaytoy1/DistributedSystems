# RescueMesh Phase 1 report

CSCE 4411, Fall 2026. This report follows the spec's "Report Requirements". Every table of results here is generated from the logs by `analysis/phase1/make_tables.py`, and no result is typed by hand. The design log, [design-log.md](design-log.md), holds every decision in full, D1 to D260. The shared log has its own page, [shared-log.md](shared-log.md).

**Phase 2 is not part of this report.** Discovery, P2P, offline operation and conflicting claims between clients (scenarios 5 to 8) are not built.

## 1. The result

**Scenarios 1, 2, 3, 4 and 9 of the spec's Required Demonstration Scenarios are met, with the limits said in section 11.** These are the five Dr Amr asked for in class on Oct 28 (`docs/demo.md`).

| # | Scenario | The spec asks | Measured | Verdict |
|---|---|---|---|---|
| 1 | Normal cloud operation | Different peers are elected as load changes | All 3 nodes kept winning as load went from 1,000 to 9,000 a second and back. Each won 31% to 35% at every level, while bids rose about 13 times with the load | **Met** |
| 2 | Load imbalance | Load one server, and the election responds | The slowed node's share fell from 33.4% to 4.9%, and came back to 33.3%. The control stayed flat | **Met** |
| 3 | Server failure | The elected server stops under load, and service continues | At six loads from 1,000 to 30,000 a second, the others went on without it in about 1 s, at the offered rate. It sent no answers during the fault. 0 gave up up to 20,000, and 12,303 at 30,000, in 2 of 5 runs | **Met** |
| 4 | Server recovery | State is rebuilt before it takes part again | A full member again 0.8 s to 10.5 s after the fault up to 20,000 a second, 73 s at 30,000, and 6 s and 12 s after a kill. All 45 runs ended with one digest | **Met** |
| 9 | Scale and stress | Large automated runs: latency, throughput, overhead, recovery, convergence | 157 runs, 199 million requests, 0 unaccounted, 0 digest mismatches. Ceilings below | **Met** |

**The whole series:**

| Measure | Value |
|---|---:|
| Runs | 157 |
| Requests sent | 199,481,205 |
| Answered | 199,464,878 |
| Gave up after all retries | 16,327 |
| Unaccounted for | 0 |
| Digest checkpoints compared | 69,715 |
| Digest mismatches | 0 |
| Runs ending with different digests | 0 |

**The ceiling, on the lab's Wi-Fi:**

- **With the planning load (`plan_work` 1000): 12,000 requests a second** held in all 5 runs. At 14,000, all 20 planning threads were full.
- **With no simulated planning (`plan_work` 0): 30,000 a second** held in all 5 runs, and in a 5-minute hold. Above that it depends on the run: 33,000 held in 3 of 5, 35,000 in 3 of 5, and 36,000 in 1 of 5.

Quote the ceiling with its `plan_work`. The spec wants planning substantial enough to create real computing load, so the first figure is the fair one, and the second shows what the protocol and the network can carry.

## 2. Architecture and process model

Six PCs: three cloud nodes, three client PCs. Each has an Intel i7-13700 with 24 threads and 15 GB of memory, running Ubuntu 24.04. All six are on the `AUC-WiFi` network. Node 2 and client PC 1 send over a cable, and receive over the air.

**Who is who, as the logs themselves say it:**

| In the logs | PC name | IP address |
|---|---:|---:|
| `client-1` | cse-p07-2167-21-OptiPlex-Tower-Plus-7010 | 10.7.57.193 |
| `client-2` | cse-p07-2167-21-OptiPlex-Tower-Plus-7010 | 10.7.57.193 |
| `client-3` | cse-p07-2167-21-OptiPlex-Tower-Plus-7010 | 10.7.57.193 |
| `client-4` | cse-p07-2167-24 | 10.40.1.240 |
| `client-5` | cse-p07-2167-24 | 10.40.1.240 |
| `client-6` | cse-p07-2167-24 | 10.40.1.240 |
| `client-7` | cse-p07-2167-22-OptiPlex-Tower-Plus-7010 | 10.40.5.45 |
| `client-8` | cse-p07-2167-22-OptiPlex-Tower-Plus-7010 | 10.40.5.45 |
| `client-9` | cse-p07-2167-22-OptiPlex-Tower-Plus-7010 | 10.40.5.45 |
| `node-1` | cloud node | 10.40.2.29 |
| `node-2` | cloud node | 10.40.32.242 |
| `node-3` | cloud node | 10.40.0.73 |

Client PC 1's address is the one it reaches the cloud from, its cable.

**A cloud node** is one `node` process, with no special role. Its parts, each in [architecture.md](architecture.md) section 4:

| Part | File | Job |
|---|---|---|
| Service | `src/node/service.rs` | Takes a report, bids, plans if it wins, commits, replies |
| Bidding | `src/bidding.rs`, `src/node/bids.rs` | The election for each report |
| Planner | `src/planner.rs`, `src/node/workers.rs` | Picks a responder and a priority |
| Shared log | `src/rounds.rs` | One order for all commands, with no leader |
| State machine | `src/state.rs` | Results and responder holds, the same on every node |
| Failure detector | `src/node/heartbeat.rs` | Suspects a peer that goes quiet |
| Takeover | `src/node/takeover.rs` | Plans a report if its winner went silent |
| Release | `src/node/release.rs` | Frees a responder after its service time |
| Fault injection | `src/node/faults.rs` | The fault election, and `ctl silence` |
| Rejoin | `src/node/rejoin.rs` | Stops serving during a fault, and catches up after |
| Transport | `src/node/transport.rs`, `src/node/datagram.rs`, `src/node/squeeze.rs` | TCP between nodes, UDP from clients |

**Parallelization, a cloud node.** A tokio runtime runs one task per connection, one per call and one per report. Planning is CPU work, so it runs on a pool of plain threads: 20 threads with a queue of 4,096 in the lab. The state is behind a read-write lock, and the shared log's core behind one mutex, never held across an await. Details and diagram: [architecture.md](architecture.md) section 10.

**Parallelization, a client PC.** Each PC ran 3 `loadgen` programs in these experiments, so 9 in all, each with its own client ID. A program at a fixed rate starts a task per report at evenly spaced times, so a slow answer does not delay the next report. Details: [architecture.md](architecture.md) section 10b.

**Adding load changes no logic.** More programs, or a higher rate, are all it takes.

## 3. Protocols and message formats

Full tables: [protocol.md](protocol.md) and [shared-log.md](shared-log.md).

| Step | Message | Transport | From, to |
|---|---|---|---|
| Report | `submit(ClientRequest)` | tarpc over UDP, in `WireMessage` | Client to all 3 nodes |
| Cancel, once answered | `WireMessage::Cancel` | UDP | Client to the other nodes |
| Bid | `PeerBody::Bids(Vec<Bid>)` | tarpc over TCP | Node to peers |
| Log | `PeerBody::Rounds(RoundsMsg)` | tarpc over TCP, its own connection | Node to peers |
| Answer | `ClientReply` | UDP | Node to client |
| Heartbeat | `PeerBody::Heartbeat` | tarpc over TCP | Node to peers |
| Fault election | `PeerBody::FaultBid` | tarpc over TCP | Node to peers |

Messages to one address are combined: UDP for up to 20 ms, bids for up to 10 ms. Combined datagrams and TCP frames are squeezed, in LZ4's block format (D255).

**Timers in these experiments:**

| Timer | Value | Meaning |
|---|---:|---|
| `bid_window_ms` | 60 ms | How long an election stays open |
| `peer_heartbeat_ms` | 50 ms | How often nodes greet each other |
| `takeover_missed_heartbeats` | 20 | So 1 s of silence before a peer is suspected or dropped |
| `round_ms` | 5 ms | How often a node makes a batch for the log |
| `client_retry_ms`, `client_retry_max_ms` | 2 s, 6 s | First wait for a retry, doubling, with ±20% jitter |
| `client_max_retries` | 5 | Then the client reports an error |
| `propose_timeout_ms` | 3 s | How long a command may wait to commit |
| `peer_rpc_timeout_ms` | 500 ms | A small call between nodes |
| `degraded_after_ms` | 2 s | A node alone for this long answers provisionally |
| Backstop | at least 1.5 s | A loser plans the report if the winner has said nothing |

**Duplicates.** Every report has a unique ID: client ID and sequence number. A retry keeps it. The first commit for an ID wins on every node, so running one twice is harmless, and a later attempt gets the same answer. A command already decided is answered without going through the log (D250).

**Requests in flight at a failure.** The client retries after 2 s, to all three nodes. A node that lost the election plans the report once it finds the winner silent, or at the backstop. Whichever commits first is the answer. Section 7 measures this.

## 4. The election

**The survey and first choice are in the Phase 0 deliverable,** due Oct 4, which is not in this repository.

**The election for each report (D2, D114).** Every node sends a bid, its load score, and the lowest wins, with ties to the lower node ID. Nobody coordinates: each node decides from what it holds when its window closes.

```text
score = (queued + running + just_won + 1) x recent_service_us / workers
```

So it is the expected wait in microseconds at this node, from measured numbers: queue, running jobs, rounds it just won and has not started, and how long plans have recently taken.

A node that misses a bid can decide differently, and two nodes may both plan a report, or none until the backstop. This costs work and time, and is never wrong, as the first commit wins.

**The fault election (D158).** The three nodes also pick which of them fails, with a random draw, as the spec asks. It needs a ticket from every node, so a node that is off or catching up stops the draw, and only one fault happens at a time on this path.

**There is no leader election.** Raft was removed from the running system because the class notes rule out a coordinator (D249, section 10).

**What the elections cost:**

| Offered/s | `plan_work` | Election p50 ms | Election p99 ms | Elections seeing all 3 bids | Bid calls per request | Log calls per request | Heartbeats per request | Fairness (Jain) |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 5,000 | 0 | 61.0 ± 0.0 | 62.0 ± 0.0 | 97.7% | 0.089 ± 0.000 | 0.091 ± 0.004 | 0.025 ± 0.000 | 0.999 ± 0.000 |
| 10,000 | 0 | 61.0 ± 0.0 | 62.0 ± 0.1 | 96.2% | 0.044 ± 0.001 | 0.038 ± 0.002 | 0.012 ± 0.000 | 0.997 ± 0.001 |
| 20,000 | 0 | 61.1 ± 0.0 | 62.4 ± 0.1 | 92.3% | 0.022 ± 0.000 | 0.014 ± 0.001 | 0.006 ± 0.000 | 0.999 ± 0.001 |
| 30,000 | 0 | 61.2 ± 0.1 | 65.1 ± 0.5 | 73.7% | 0.014 ± 0.000 | 0.007 ± 0.001 | 0.004 ± 0.000 | 0.998 ± 0.002 |
| 35,000 | 0 | 61.4 ± 0.1 | 72.5 ± 1.2 | 32.5% | 0.009 ± 0.001 | 0.005 ± 0.001 | 0.004 ± 0.000 | 0.900 ± 0.084 |
| 1,000 | 1000 | 61.0 ± 0.1 | 61.9 ± 0.0 | 99.0% | 0.426 ± 0.009 | 0.595 ± 0.020 | 0.126 ± 0.001 | 1.000 ± 0.000 |
| 5,000 | 1000 | 61.0 ± 0.0 | 62.0 ± 0.1 | 98.5% | 0.089 ± 0.001 | 0.089 ± 0.002 | 0.025 ± 0.000 | 0.999 ± 0.000 |
| 10,000 | 1000 | 61.0 ± 0.0 | 62.0 ± 0.1 | 96.6% | 0.044 ± 0.000 | 0.035 ± 0.001 | 0.013 ± 0.000 | 0.998 ± 0.001 |
| 12,000 | 1000 | 61.0 ± 0.0 | 62.2 ± 0.2 | 95.5% | 0.037 ± 0.000 | 0.026 ± 0.001 | 0.010 ± 0.000 | 0.998 ± 0.000 |
| 14,000 | 1000 | 61.0 ± 0.1 | 62.6 ± 0.3 | 88.6% | 0.033 ± 0.001 | 0.024 ± 0.002 | 0.010 ± 0.000 | 1.000 ± 0.000 |

Calls are low because bids and log batches are combined. An election's duration is its window plus scheduling, and that window is a chosen setting, so a smaller one is possible at the cost of more split decisions. **At high load fewer elections see all 3 bids,** which is what limits fairness and first-try success above 30,000 a second.

## 5. Load balancing, with evidence

**Scenario 1, load changing during a run.** 5 runs of 240 s: 1,000 requests a second, then 5,000, then 9,000, then back to 1,000, from all 3 client PCs. `plan_work` 1000.

| Load | Answers/s | Node 1 won | Node 2 won | Node 3 won | Node 1 bid | Node 2 bid | Node 3 bid |
|---|---:|---:|---:|---:|---:|---:|---:|
| 1,000/s | 999 | 33.8% | 33.1% | 33.2% | 4,390 | 4,390 | 4,402 |
| 5,000/s | 4,998 | 34.5% | 31.9% | 33.6% | 24,019 | 24,255 | 23,969 |
| 9,000/s | 8,998 | 34.2% | 31.4% | 34.4% | 55,891 | 56,554 | 55,863 |
| Back to 1,000/s | 999 | 33.7% | 33.1% | 33.1% | 4,410 | 4,427 | 4,452 |

![Scenario 1](../analysis/results/exp8h/scenario1.png)

**What this shows.** Every node kept winning at every load, each between 31.4% and 34.5%. Bids follow the load, from about 4,400 microseconds of expected wait to about 56,000, and back. The PCs are the same model and the load is even, so even shares are the fair result. The winner is chosen again for every report, so this is not nodes taking long turns. Scenario 2 shows what happens when one node really is busier.

**How evenly work was shared over the whole of a run:**

| Offered/s | `plan_work` | Node 1 | Node 2 | Node 3 |
|---|---:|---:|---:|---:|
| 10,000 | 0 | 0.350 ± 0.008 | 0.310 ± 0.004 | 0.339 ± 0.006 |
| 20,000 | 0 | 0.332 ± 0.015 | 0.322 ± 0.006 | 0.346 ± 0.010 |
| 30,000 | 0 | 0.323 ± 0.017 | 0.341 ± 0.009 | 0.336 ± 0.025 |
| 35,000 | 0 | 0.273 ± 0.103 | 0.460 ± 0.057 | 0.266 ± 0.117 |
| 10,000 | 1000 | 0.342 ± 0.002 | 0.315 ± 0.007 | 0.343 ± 0.008 |
| 12,000 | 1000 | 0.340 ± 0.005 | 0.316 ± 0.001 | 0.345 ± 0.004 |

**Scenario 2, one node made slow.** 5 runs of 180 s at 3,000 a second. From 60 s to 120 s, one node's planning work goes from 1000 to 5000, about 4 ms to 20 ms by the calibration in `src/planner.rs`. The slowed node changes between runs.

| Score | Phase | Slowed node won | Each other node won | Slowed node's bid | Others' bid |
|---|---:|---:|---:|---:|---:|
| Expected wait, the policy in use | Before | 33.4% | 33.3% | 13,831 | 13,791 |
| Expected wait, the policy in use | Slowed | 4.9% | 47.6% | 30,393 | 17,883 |
| Expected wait, the policy in use | After | 33.3% | 33.3% | 13,980 | 13,963 |
| Queue length only | Before | 33.5% | 33.3% | 88 | 88 |
| Queue length only | Slowed | 25.2% | 37.4% | 93 | 94 |
| Queue length only | After | 33.6% | 33.2% | 88 | 88 |
| Control: nothing slowed | Before | 34.6% | 32.7% | 13,844 | 13,869 |
| Control: nothing slowed | Slowed | 34.7% | 32.7% | 13,852 | 13,912 |
| Control: nothing slowed | After | 34.9% | 32.5% | 13,976 | 14,043 |

| Score | Runs | Answered/s | Gave up | p50 ms | p99 ms | Fairness (Jain) |
|---|---:|---:|---:|---:|---:|---:|
| Expected wait | 5 | 2,997 ± 0 | 0 | 136 ± 3 | 198 ± 6 | 0.953 ± 0.010 |
| Queue length only | 5 | 2,997 ± 0 | 0 | 139 ± 1 | 220 ± 19 | 0.996 ± 0.004 |
| Control | 5 | 2,997 ± 1 | 0 | 136 ± 2 | 220 ± 36 | 0.999 ± 0.000 |

![Scenario 2](../analysis/results/exp8h/scenario2.png)

**What this shows.** When one node is slowed, its bid rises from 13,831 to 30,393 and its share falls from 33.4% to 4.9%, while each of the others takes 47.6%. When the slowdown ends, its share is back at 33.3%. Service does not suffer: p50 is 136 ms, the same as the control, and nobody gave up. **A score of queue length only reacts far less,** to 25.2%, because its bids hardly change (93 against 94): a slow plan does not show in the queue at this load. This is the measured reason for using expected wait.

## 6. Fault model and recovery protocol

**Faults handled.** A node stops answering for up to 20 s, or its process dies and restarts. One failure at a time. No Byzantine behaviour: all nodes are trusted.

**Detection.** A peer not heard from for 1 s is suspected, by heartbeats for bidding and takeover, and by its log stream for the shared log.

**Failover.** The other two agree to go on without it, with no leader: see [shared-log.md](shared-log.md) section 8. Commits pause until then, since a round needs every member's stream.

**Recovery, and what the spec asks of it:**

| The spec asks | Answer |
|---|---|
| What is copied, replayed or rebuilt? | The whole applied log is sent by a peer, up to 4,000 commands a reply, and replayed through the state machine. So the state is rebuilt, with one code path for a fault and for a restart |
| How does the node know it is current? | It asks to be added. The members stop at the round before its start, and it has the whole log when their log ends and their round is that one. Then all continue together |
| When does it take part again? | Its proposals are refused until it is back as a member |

**One node left.** It cannot commit alone. It answers provisionally (D120), and the answers are reconciled when a peer returns, with conflicts logged (D123).

## 7. Scenario 3: server failure

A node stops answering for 20 s, 45 s into a 150 s run, under load from all 3 client PCs. The victim changes between runs. 5 runs each.

| Offered/s | `plan_work` | Answers/s before | During 20 s fault | After | p99 ms before | During | After | Gave up | Retries |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1,000 | 1000 | 999 ± 1 | 999 ± 0 | 999 ± 0 | 228 ± 54 | 942 ± 14 | 197 ± 7 | 0 | 4 ± 8 |
| 5,000 | 1000 | 5,005 ± 2 | 5,005 ± 5 | 4,999 ± 2 | 236 ± 40 | 1,535 ± 459 | 238 ± 32 | 0 | 1,102 ± 341 |
| 8,000 | 1000 | 8,002 ± 8 | 8,007 ± 12 | 7,992 ± 15 | 241 ± 15 | 1,624 ± 537 | 276 ± 38 | 0 | 2,111 ± 905 |
| 10,000 | 0 | 9,996 ± 10 | 10,002 ± 21 | 10,000 ± 27 | 249 ± 36 | 2,075 ± 73 | 272 ± 50 | 0 | 4,536 ± 797 |
| 20,000 | 0 | 19,992 ± 31 | 20,022 ± 36 | 19,971 ± 32 | 393 ± 212 | 2,381 ± 417 | 448 ± 258 | 0 | 15,880 ± 10,663 |
| 30,000 | 0 | 30,083 ± 108 | 29,992 ± 524 | 29,492 ± 1,357 | 1,252 ± 1,280 | 2,688 ± 266 | 1,258 ± 1,583 | 12,303 | 644,288 ± 1,188,873 |

| Offered/s | `plan_work` | Others dropped it, ms | Longest gap with no answer, ms | Requests in flight at the fault | It won in the last second | Of them, not answered by 0.3 s after the fault | Planned again by the others | Its plan had already committed | Gave up | Answers it sent during the fault | All answers during the fault |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1,000 | 1000 | 1,037 ± 12 | 1,003 ± 9 | 126 ± 6 | 1,676 | 23 | 12 | 11 | 0 | 0 | 89,911 |
| 5,000 | 1000 | 1,043 ± 18 | 1,004 ± 11 | 673 ± 38 | 8,171 | 117 | 42 | 75 | 0 | 0 | 455,790 |
| 8,000 | 1000 | 1,036 ± 10 | 853 ± 354 | 1,273 ± 186 | 13,402 | 297 | 82 | 215 | 0 | 0 | 730,134 |
| 10,000 | 0 | 1,048 ± 14 | 994 ± 13 | 1,521 ± 187 | 16,249 | 405 | 23 | 382 | 0 | 0 | 920,950 |
| 20,000 | 0 | 1,065 ± 23 | 967 ± 54 | 3,620 ± 426 | 33,038 | 669 | 9 | 660 | 0 | 0 | 1,860,804 |
| 30,000 | 0 | 1,054 ± 19 | 714 ± 389 | 7,437 ± 4,680 | 46,420 | 1,772 | 47 | 1,725 | 0 | 0 | 2,822,013 |

![Scenario 3](../analysis/results/exp8h/scenario3.png)

**What this shows.**

- **Failover time.** The others went on without the node in about 1.04 s at every load, from 1,037 ms to 1,065 ms. So it is the detection time, and does not grow with load.
- **During the fault.** Answers fall almost to nothing for about a second, then a burst clears the backlog, and the rate stays at the offered rate for the rest of the fault. That burst is why the "during" column equals the offered rate. p99 rises because reports caught in that second wait for it, or for a retry.
- **Requests in flight.** None was lost: every run has 0 unaccounted. Of the reports the stopped node had won in its last second, most were answered before it stopped. Of those still unanswered 0.3 s later, at 10,000 a second, 23 were planned again by the others, and for 382 its plan had already committed, so a peer gave that answer.
- **Transparency.** Across 6,879,602 answers, from 2 s into each fault to its end, none came from the stopped node. Clients were not told of any failure, and needed no change.
- **The limit.** At 30,000 a second it is at its edge: 2 of 5 runs had reports give up, 12,233 and 70.

## 8. Scenario 4: server recovery

From the same runs, a 5 s fault at 10,000 a second, and a process killed and started 30 s later, with all its state rebuilt.

| Offered/s | `plan_work` | Runs | Caught up to the log, ms | Member again, ms | Answer rate back, ms after the fault | Digest mismatches | Runs ending with one digest | Seconds to one digest, after the load |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 1,000 | 1000 | 5 | 290 ± 73 | 800 ± 75 | 991 ± 1 | 0 | 5 of 5 | 0.8 ± 1.0 |
| 5,000 | 1000 | 5 | 1,247 ± 319 | 1,820 ± 364 | 991 ± 1 | 0 | 5 of 5 | 0.6 ± 0.7 |
| 8,000 | 1000 | 5 | 2,078 ± 557 | 2,848 ± 733 | 991 ± 1 | 0 | 5 of 5 | 0.8 ± 0.6 |
| 10,000 | 0 | 5 | 2,801 ± 637 | 3,632 ± 809 | 991 ± 1 | 0 | 5 of 5 | 1.2 ± 0.6 |
| 20,000 | 0 | 5 | 6,744 ± 1,628 | 10,488 ± 4,118 | 991 ± 1 | 0 | 5 of 5 | 1.2 ± 0.6 |
| 30,000 | 0 | 5 | 14,384 ± 3,246 | 73,320 ± 34,451 | 1,391 ± 1,110 | 0 | 5 of 5 | 1.6 ± 4.4 |
| 10,000, 5 s fault | 0 | 5 | 875 ± 162 | 1,414 ± 199 | 991 ± 1 | 0 | 5 of 5 | 0.8 ± 1.0 |
| 5,000, killed | 1000 | 5 | 6,023 ± 1,328 | 6,000 ± 1,324 | 991 ± 1 | 0 | 5 of 5 | 0.6 ± 1.1 |
| 10,000, killed | 0 | 5 | 11,994 ± 3,219 | 11,958 ± 3,235 | 791 ± 554 | 0 | 5 of 5 | 1.0 ± 0.9 |

![Scenario 4](../analysis/results/exp8h/scenario4.png)

**What this shows.**

- **Recovery takes longer as load rises,** because the node replays what it missed while new commands keep arriving: 0.8 s at 1,000 a second, 3.6 s at 10,000, 10.5 s at 20,000. **At 30,000 it took 73 s ± 34 s,** most likely because little capacity is left on the air to catch up with.
- **A shorter fault is quicker:** 1.4 s after a 5 s fault at 10,000 a second, against 3.6 s after 20 s.
- **A killed process rebuilds everything,** from a peer's log: 6.0 s at 5,000 a second, 12.0 s at 10,000.
- **Convergence.** Every run ended with one digest on all three nodes, within about 2 s of the load ending on average, with 0 mismatches at any checkpoint.
- **The "answer rate back" column** has a resolution of 1 s, so it says only that the rate was back within about a second of the fault.

"Caught up to the log" is the `rejoined` event. "Member again" is when its proposals are accepted again, so that is the end of recovery.

**Faults the nodes elect themselves, on the spec's terms.** A random silence of up to 20 s, every 20 to 30 s, for 5 minutes at 3,000 a second, 5 runs:

| Run | Requests | Gave up | p50 ms | p99 ms | Faults | Faults per node | Mean silence ms | Longest silence ms | Checkpoints | Mismatches |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 944,055 | 0 | 131.1 | 873.3 | 9 | {3: 5, 2: 3, 1: 1} | 13,080 | 17,263 | 991 | 0 |
| 2 | 944,055 | 0 | 131.5 | 807.9 | 9 | {3: 1, 1: 7, 2: 1} | 12,421 | 19,654 | 985 | 0 |
| 3 | 944,055 | 0 | 133.3 | 853.0 | 10 | {3: 4, 1: 3, 2: 3} | 9,834 | 17,717 | 985 | 0 |
| 4 | 944,055 | 0 | 131.7 | 820.0 | 9 | {3: 4, 1: 5} | 12,295 | 19,347 | 971 | 0 |
| 5 | 944,055 | 0 | 134.9 | 863.5 | 10 | {2: 3, 3: 5, 1: 2} | 9,287 | 14,488 | 979 | 0 |

| Across all elected faults | Samples | Median ms | p95 ms | Most ms |
|---|---:|---:|---:|---:|
| Others dropped the silent node | 47 | 1018 | 1031 | 1036 |
| It caught up to the log, from the end of its silence | 47 | 428 | 760 | 1141 |

## 9. Scenario 9: scale and stress

**`plan_work` 0, no simulated planning.** 75 s runs, 60 s measured, 5 runs per rate. A run "held" if it answered at least 99% of the offered rate, nobody gave up, and p99 stayed under 3 s. The ± is a 95% confidence interval across runs, by Student's t.

| Offered/s | Runs that held | Answered/s | Gave up | First try | p50 ms | p95 ms | p99 ms | Cores per node | Rx Mbit/s per node |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 5,000 | 5 of 5 | 5,004 ± 1 | 0 | 100.0% | 137 ± 3 | 180 ± 11 | 227 ± 23 | 0.3 ± 0.0 | 3.4 ± 0.0 |
| 10,000 | 5 of 5 | 9,994 ± 11 | 0 | 100.0% | 146 ± 2 | 206 ± 11 | 268 ± 28 | 0.5 ± 0.0 | 5.9 ± 0.0 |
| 20,000 | 5 of 5 | 20,007 ± 15 | 0 | 100.0% | 171 ± 4 | 317 ± 69 | 451 ± 125 | 0.9 ± 0.0 | 11.1 ± 0.1 |
| 30,000 | 5 of 5 | 30,020 ± 28 | 0 | 99.6% | 218 ± 13 | 427 ± 86 | 631 ± 186 | 1.4 ± 0.0 | 17.2 ± 0.4 |
| 33,000 | 3 of 5 | 32,197 ± 1,271 | 5 | 91.4% | 271 ± 23 | 1,723 ± 1,203 | 3,814 ± 2,785 | 1.5 ± 0.1 | 19.5 ± 0.3 |
| 35,000 | 3 of 5 | 34,215 ± 1,591 | 1,799 | 88.5% | 482 ± 275 | 3,763 ± 5,911 | 6,552 ± 8,183 | 1.6 ± 0.1 | 21.9 ± 1.0 |
| 36,000 | 1 of 5 | 34,070 ± 1,804 | 2,160 | 79.2% | 633 ± 441 | 5,569 ± 4,673 | 10,323 ± 9,411 | 1.6 ± 0.0 | 22.9 ± 2.8 |

**`plan_work` 1000, about 4 ms of planning per report:**

| Offered/s | Runs that held | Answered/s | Gave up | First try | p50 ms | p95 ms | p99 ms | Cores per node | Rx Mbit/s per node |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1,000 | 5 of 5 | 999 ± 0 | 0 | 100.0% | 130 ± 1 | 160 ± 9 | 213 ± 22 | 1.1 ± 0.0 | 1.4 ± 0.0 |
| 5,000 | 5 of 5 | 5,004 ± 1 | 0 | 100.0% | 141 ± 2 | 182 ± 6 | 232 ± 26 | 5.4 ± 0.0 | 3.5 ± 0.0 |
| 8,000 | 5 of 5 | 8,000 ± 2 | 0 | 100.0% | 152 ± 3 | 204 ± 6 | 255 ± 10 | 9.7 ± 0.1 | 5.2 ± 0.1 |
| 10,000 | 5 of 5 | 9,999 ± 6 | 0 | 100.0% | 160 ± 2 | 226 ± 19 | 349 ± 221 | 13.1 ± 0.2 | 6.3 ± 0.0 |
| 12,000 | 5 of 5 | 11,995 ± 5 | 0 | 100.0% | 173 ± 6 | 310 ± 194 | 391 ± 226 | 16.8 ± 0.3 | 7.5 ± 0.0 |
| 14,000 | 0 of 5 | 13,705 ± 142 | 60 | 94.7% | 803 ± 219 | 1,887 ± 1,488 | 3,964 ± 3,887 | 20.2 ± 0.0 | 8.9 ± 0.6 |

![Throughput](../analysis/results/exp8h/throughput.png)

![Latency](../analysis/results/exp8h/latency.png)

**Holding the rate for 5 minutes:**

| Held for 300 s at | Requests | Answered/s | Gave up | First try | p50 ms | p95 ms | p99 ms | Checkpoints | Mismatches | Cores per node | Memory MB, most |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 30,000, `plan_work` 0 | 9,449,055 | 29,984 | 0 | 99.7% | 210.4 | 395.1 | 561.5 | 1,045 | 0 | 1.45 | 5,175 |
| 10,000, `plan_work` 1000 | 3,149,685 | 9,999 | 0 | 99.9% | 158.9 | 216.9 | 275.6 | 1,559 | 0 | 13.28 | 1,831 |

**The same 10,000 a second from different numbers of client PCs:**

| Load from | Answered/s | Gave up | p50 ms | p99 ms | Answers, PC 1 | Answers, PC 2 | Answers, PC 3 |
|---|---:|---:|---:|---:|---:|---:|---:|
| 1 client PC | 10,000 ± 2 | 0 | 120 ± 2 | 155 ± 15 | 601,268 ± 67 | 0 ± 0 | 0 ± 0 |
| 2 client PCs | 10,002 ± 2 | 0 | 130 ± 1 | 180 ± 23 | 300,734 ± 58 | 302,723 ± 249 | 0 ± 0 |
| 3 client PCs | 9,994 ± 11 | 0 | 146 ± 2 | 268 ± 28 | 200,482 ± 32 | 201,716 ± 83 | 201,766 ± 89 |

**What limits it.**

- **With planning load, the CPU.** Cores per node rise with the rate, to about 20 at 14,000 a second, the size of the planning pool.
- **Without it, most likely the shared Wi-Fi.** Above 30,000 a second each node still used under 2 cores, while elections saw fewer bids and retries rose. Fewer and smaller packets moved the ceiling up (D254 to D256), which also points there. We did not measure air time directly.

**What the numbers mean for an answer.** Latency is measured by the client, from the time a report was meant to go, so a queue in the load generator is not hidden (D54). Most of p50 is the 60 ms election window, then the log. A p99 just over 2,000 ms means some answers needed the 2 s retry.

## 10. Design decisions changed during development

Each is in full in [design-log.md](design-log.md).

| From | To | Why | Entry |
|---|---|---|---|
| Write Raft ourselves | `openraft` | Weeks of work before anything could be tested | D4, D14 |
| The Raft leader dispatches reports | Bidding per report | The leader did all the CPU work, and assignment stopped during elections | D34, D114 |
| Raft's leader only orders the log | **Rotating rounds, no leader** | The class notes rule out a coordinator. Raft held 7,000 a second and fell at 10,000 | D163, D249 |
| One TCP connection, Nagle on | Own connection for the log, sent at once | A small message could wait behind others | D237, D251 |
| A plan scans 1 in 16 of all responders | A plan looks at 512 in a row | A scan held the state's lock, which stalled commits | D238, D253 |
| Release scan of all holds every 50 ms | Follow a list of new holds | The same | D253 |
| Replace a connection after 1 s | After 5 s, and connects outlive a call | A slow moment on Wi-Fi became a dropped node | D89, D252 |
| tarpc's full client message | A short form, with no trace context | 26 bytes on each of 5 messages per report | D170, D254 |
| Messages as they are | Squeezed | Messages look alike, and the air is what is short | D255 |
| UDP combined for 10 ms, bids for 5 ms | 20 ms and 10 ms | Fewer packets on the air | D246, D247, D256 |
| Drop after about 300 ms of silence | After 1 s in the lab | Real false drops were measured at 300 ms | D249 |
| Look for a shared-log library | **Keep our own** | None tested works with no leader | D259 |

**Bugs found by the experiments,** each fixed with a test that fails without the fix: two nodes making different drops as the same view, a node returning at an out of date round, and a returning node starting inside its old stream (D252, D257). Each showed as digest mismatches, which is what the checkpoints are for.

## 11. Limits

1. **Two failures close together stop commits.** If a second node fails while the first is still on its way back, the last member cannot drop anyone, and a member that restarts while still listed cannot return. Answers stay correct, but nothing commits, and what brings it back has not been tested. T4's last part showed it, with 1,303 of 9,892 reports giving up, and 0 digest mismatches. Raft would carry on here. **The spec's scenarios fail one node at a time, so they do not reach this.**
2. **`plan_work` is simulated CPU work,** a busy loop that does not change the plan. The real choice of a responder takes microseconds.
3. **T1 and T2 used `kill -9` and 3-minute loads,** by choice of the person running the lab, not real power cuts. A real cut can lose disk writes that `kill -9` cannot.
4. **A restarted node recovers from a peer's log, not its own file,** so losing all three at once loses the state (D249).
5. **Memory grows with the log,** about 5,175 MB per node after 5 minutes at 30,000 a second. Nothing trims it.
6. **Detection is 1 s.** The first choice was about 300 ms, and it gave false drops under load. A failure costs at least this much.
7. **Wi-Fi conditions change,** and the lab's network is shared with others. That is one reason the rates above 30,000 vary from run to run.
8. **Node 2 and client PC 1 send by cable.** The others use the air both ways, so the setup is not symmetric.
9. **Corosync was not tested,** as its programs need approval that was not given before this report (D259).
10. **The shared log's ordering is our own code,** with no library's users behind it. See [shared-log.md](shared-log.md) section 3 for what we tried instead.

## 12. Test plan

- **Unit and multi-node tests:** `cargo test`, 30 groups, all pass at `a7ceb34`. `tests/rounds.rs` holds the shared log's, including each bug above. `tests/layers.rs` holds the layering rules.
- **Experiments:** scripts in `scripts/phase1/`. One command runs them all, and they need nobody present.
- **Fact checks on the logs,** from raw per-request lines, trusting no summary: `analysis/phase1/audit.py` and `analysis/phase1/factcheck.py`. In three runs at 10,000 a second, a plain one, one with a 20 s fault and one with a kill, each of 3,749,625 answers was matched to a reply line in the answering node's own log, with 0 mismatches (`analysis/results/exp8h/factcheck-*.txt`).
- **Conflicts:** only between a lone node's provisional answers and what commits later, as T2 showed. Claims between clients are Phase 2.

## 13. Methodology and raw data

- **Design.** 157 runs. 5 passes through all cells, in the same order each time, so that drift over the night falls across all rates, not on one. The two 5-minute holds ran once. The 5 passes took 6 h 37 min, 5.1 h of it with load running, and the rerun of scenario 2 about 37 min. About 8 hours were asked for, and about 7.2 ran, though the folder is named `exp8h`.
- **Load.** Open loop: 9 programs at a fixed rate, 3 on each client PC, evenly spaced, each with its own seed in each pass. Each report goes to all 3 nodes.
- **Settings.** Commit `a7ceb34` on all six PCs, built on each. 60,000 responders, 20 planning threads, bid window 60 ms, detection 1 s. Score `expected-wait`. CPU energy setting `performance` on the nodes and `balance_performance` on the clients, read at the start in the run's `setup-*.txt`.
- **Measured.** 15 s of warm-up is left out. Rates are in answers finished per second.
- **Statistics.** Means with 95% confidence intervals by Student's t across runs, and percentiles over all requests of a run.
- **The PCs during the runs.** We started no other load on them, and the full analysis ran between and after the series. During the rerun of scenario 2, small files were edited on client PC 1, and a few small logs were read to check on the run. Whether anyone else used the shared Wi-Fi is not known.
- **The totals in section 1 count all 157 runs,** including the 10 described under "Runs left out", whose requests were real.

**The raw data:**

| What | Where |
|---|---|
| Every client's every request, and each run's status and config | `runs/exp8h/<cell>/<rate>/run-<n>/`, on client PC 1 |
| Every node's every event, gzipped, in full | `runs/exp8h-full/node-<id>/<name>/run-<n>/`, and on each node PC under `runs/full/` |
| Per-run and per-cell figures | `analysis/results/exp8h/runs.csv`, `cells.csv` |
| One report followed through every PC's log | `analysis/results/request-trace-example.txt` |
| What each line in the logs means | [shared-log.md](shared-log.md) section 14 |

**Runs left out, and why.** `runs/exp8h/e2/skew` and `e2/queue` were run with a mistake in the script: the command meant to end the slowdown set that node's planning work to 0 and not back to 1000. Before and during were valid, but the node then won nearly everything, as it had really become the fastest. They are kept, and the tables use `e2/skewfix` and `e2/queuefix`, rerun correctly. `runs/phase1` is an earlier partial series, stopped to fact-check it.

## 14. What is a library and what is ours

| Done by a library | Done by us |
|---|---|
| Remote calls: `tarpc` 0.37 | The election: `bidding.rs`, `node/bids.rs` |
| Async, timers, sockets: `tokio` 1 | The shared log's order, drop and return: `rounds.rs` |
| Encoding: `serde`, `bincode` 1.3 | The state machine and digest: `state.rs` |
| Framing: `tokio-util` 0.7 | Failure detection and takeover: `heartbeat.rs`, `takeover.rs` |
| Socket settings: `socket2` 0.6 | Fault injection and rejoin: `faults.rs`, `rejoin.rs` |
| Log lines: `serde_json` | Planner: `planner.rs`. UDP adapter: `datagram.rs`. Squeezing: `squeeze.rs` |
| `openraft` 0.9.25: built in, but off | All experiments and analysis |

**AI.** Much of this code and analysis was written with Claude Code, under the team's direction, as the spec allows. `AGENTS.md` sets how, and each entry in the design log says who decided it.

## 15. Team contributions

**The team must fill this in,** as the spec asks for a separate section on each member's role and concrete contribution. `AGENTS.md` lists the parts, A to F, and their owners' measures.

| Member | Part | Contribution |
|---|---|---|
| | | |
| | | |
| | | |
| | | |
