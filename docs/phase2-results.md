# RescueMesh Phase 2 results

The record the Phase 2 report's numbers are typed from ([report/phase2/](report/phase2/README.md)), the way
[phase1-report.md](phase1-report.md) was for Phase 1 (D266, D450). Every table below matches a table of the report,
column for column. **No number is here yet:** every `TBD` is filled from the analysis once the full series has run.

**Where the numbers come from.** `analysis/analyze.py` over `runs/ph2*/`, the Phase 2 cells of
`experiments/phase2.list`, writing `summary.csv` (mean, 95% Student-t half width, runs). The command, and any script
that turns `summary.csv` into these tables, come with `part-l-experiments`. Raw logs stay on the lab PCs under
`runs/<experiment>/<cell>/run-<n>/`; `outbox-client-<id>.jsonl` and `claims-client-<id>.jsonl` there are outboxes,
not event logs (their lines have no `ev`).

**Build.** TBD: the commit built on all six PCs, and the dates and times of the series. Tables without a label are
from it, under the `full` profile (5 runs per cell, 30 s warm-up, 120 s measured). A table from `quick` (3 runs,
15 s, 60 s) says so.

**Cells** (`experiments/phase2.list`; the run line's `users` is the total over the 3 client PCs):

| Experiment | Cell | Users per PC | `users` | `[workload]` | Think time |
|---|---|---:|---:|---|---|
| `ph2users` | `ph2users-100` | 100 | 300 | `lifecycle = true` | `think_ms=1000 pattern=random` |
| `ph2users` | `ph2users-1000` | 1,000 | 3,000 | `lifecycle = true` | the same |
| `ph2users` | `ph2users-5000` | 5,000 | 15,000 | `lifecycle = true` | the same |
| `ph2offline` | `ph2offline-0` | 1,000 | 3,000 | `offline_chance_per_min_percent = 0`, `offline_for_ms = 20000` | TBD (check the list) |
| `ph2offline` | `ph2offline-10` | 1,000 | 3,000 | `offline_chance_per_min_percent = 10`, `offline_for_ms = 20000` | TBD |
| `ph2offline` | `ph2offline-30` | 1,000 | 3,000 | `offline_chance_per_min_percent = 30`, `offline_for_ms = 20000` | TBD |
| `ph2conflict` | `ph2conflict-0ms` | 1,000 | 3,000 | `claim_delay_ms = 0` | TBD |
| `ph2conflict` | `ph2conflict-500ms` | 1,000 | 3,000 | `claim_delay_ms = 500` | TBD |
| `ph2conflict` | `ph2conflict-2000ms` | 1,000 | 3,000 | `claim_delay_ms = 2000` | TBD |

The offline cells are named by the chance of an outage per program per minute (D432), not by the share of time
offline, which is about chance × 20 s / 60 s: about 0%, 3% and 10%.

## 1. The result (report Table `tab:scenarios`)

| Scenario | Measured | Verdict |
|---|---|---|
| 5. Peer discovery | TBD | TBD |
| 6. Direct P2P exchange | TBD | TBD |
| 7. Offline peer | TBD | TBD |
| 8. Conflicting claim | TBD | TBD |
| 9. Scale and stress | TBD | TBD |
| 1 to 4 still pass | TBD | TBD |

## 2. Every run, in total (`tab:totals`)

| | Phase 2 cells | Phase 1 scenarios again |
|---|---:|---:|
| Runs | TBD | TBD |
| Simulated users, most at once | TBD | TBD |
| Requests sent | TBD | TBD |
| Answered | TBD | TBD |
| Gave up after all retries | TBD | TBD |
| Unaccounted for | TBD | TBD |
| Phase 2 actions | TBD | — |
| Digest checkpoints compared | TBD | TBD |
| Digest mismatches | TBD | TBD |
| Directory digest mismatches | TBD | — |
| Claims digest mismatches | TBD | — |

Sources: run.json files kept; `client_users`; `count`, `gave_up`; the sum of each program's `workload_done.actions`;
`digest_checkpoints_compared`, `digest_mismatches`; `directory_digest_mismatches`; `claims_state` digests at equal
`last_applied` (to add). "Unaccounted" needs a per-request check the Phase 2 analysis does not have yet.

**Clock skew per cell** (quoted beside every figure that compares two clocks; no table in the report):

| Cell | `node_clock_skew_ms` | `client_clock_skew_ms` |
|---|---:|---:|
| each of the nine cells | TBD | TBD |

## 3. Scenario 5. Peer discovery (`tab:s5`)

| Users per PC | Discovery, client p50, ms | Discovery, client p99, ms | Discovery, node p50, ms | Register p50, ms | Converged p50, ms | Converged p99, ms | Directory mismatches |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 100 | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| 1,000 | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| 5,000 | TBD | TBD | TBD | TBD | TBD | TBD | TBD |

Sources (`ph2users-*`): `discover_client_p50_ms`, `discover_client_p99_ms`, `discover_node_p50_ms`,
`presence_register_p50_ms`, `presence_convergence_p50_ms`, `presence_convergence_p99_ms`,
`directory_digest_mismatches`. For the text: `presence_writes_seen_by_all`, `presence_writes_seen_by_some`,
`directory_records_max`, `directory_expired`, `presence_renew_p50_ms`.

## 4. Scenario 6. Direct P2P exchange

**The peer link and zone gossip (`tab:s6`):**

| Users per PC | Round trip p50, ms | Round trip p99, ms | Acknowledged | Coverage | Duplicates per delivery | Sends per update | Hops, mean |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 100 | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| 1,000 | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| 5,000 | TBD | TBD | TBD | TBD | TBD | TBD | TBD |

Sources: `p2p_sent.latency_us` and `.ack`; `p2p_received.outcome`; `gossip_published.targets`,
`gossip_received.{outcome, forwarded, hops}`; coverage against the zone's programs (the denominator is not logged;
`gossip_zone_refreshed.programs` is a proxy capped at 16). `gossip_received.age_ms` compares two clocks (D233).

**The cloud's traffic against the peer traffic (`tab:s6cloud`):**

| Users per PC | Reports answered a second | Peer updates sent a second | Cloud bytes in, KB a second | Cloud bytes out, KB a second | Cloud bytes in per call |
|---:|---:|---:|---:|---:|---:|
| 100 | TBD | TBD | TBD | TBD | TBD |
| 1,000 | TBD | TBD | TBD | TBD | TBD |
| 5,000 | TBD | TBD | TBD | TBD | TBD |

Sources: `throughput_rps`; `p2p_sent` per second; the nodes' `udp_counters` `rx_bytes`, `tx_bytes` and
`rx_requests` deltas over the measured window (D435); cross-check against the programs' cloud-socket `tx_bytes`.

## 5. Scenario 7. Offline peer

**Reports made offline (`tab:s7`):**

| Outage chance a minute | Offline share, measured | Outages | Reports queued | Answered on return | Expired | Reconciliation p50, ms | Reconciliation p99, ms |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 0% | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| 10% | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| 30% | TBD | TBD | TBD | TBD | TBD | TBD | TBD |

Sources (`ph2offline-*`): `client_online.offline_ms`; `client_offline` count or `workload_done.outages`;
`outbox_queued`; `outbox_sent.outcome`; `outbox_flushed.reconcile_us` (one clock). Per report: `outbox_sent.waited_ms`.

**Peer updates held, and claims made offline (`tab:s7held`):**

| Outage chance a minute | Updates held | Delivered later | Dropped, too old | Dropped, limit | Dropped, obsolete | Dropped, refused | Claims queued | Reconciling on return |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 0% | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| 10% | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| 30% | TBD | TBD | TBD | TBD | TBD | TBD | TBD | TBD |

Sources: `peer_update_held`, `peer_update_forwarded`, `peer_update_dropped.reason` (`age`, `limit`, `obsolete`,
`refused`), `claim_queued`, `claims_flushed.reconciling`.

## 6. Scenario 8. Conflicting claim (`tab:s8`)

| Claim delay, ms | Claims a second | Granted | Refused online, per 1,000 claims | Reconciling, per 1,000 claims | Claim p50, ms | Claim p99, ms | Claims mismatches |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| 500 | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| 2,000 | TBD | TBD | TBD | TBD | TBD | TBD | TBD |

Sources (`ph2conflict-*`): `claim_write` (kind `claim`, by `outcome`), `claim_conflict.kind` (`online_refused`,
`reconciliation_opened`), `claim_write.latency_us`, `claims_state` digests at equal `last_applied`; for the text,
releases, `claims_state.reconciling` at the end, and the programs' `workload_done` claim outcomes.

**The deliberate conflict** (the page's "Stage a conflict" on the lab PCs):

- the incumbent's grant: TBD
- the queued claim and the reconciliation entry on return, with its token: TBD
- the owner's resolve: resolved at log index TBD
- time the entry was open: TBD (on one clock, the resolving node's `reconciliation_resolved` minus its
  `claim_conflict` `reconciliation_opened`; `reconciliation_resolved.wait_ms` compares two clocks, D378)

## 7. Scenario 9. Scale and stress with P2P

**Throughput and latency (`tab:s9`):**

| Users per PC | Users in all | Reports answered a second | Gave up | p50, ms | p95, ms | p99, ms | Phase 2 actions a second |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 100 | 300 | TBD | TBD | TBD | TBD | TBD | TBD |
| 1,000 | 3,000 | TBD | TBD | TBD | TBD | TBD | TBD |
| 5,000 | 15,000 | TBD | TBD | TBD | TBD | TBD | TBD |

Sources: `throughput_rps`, `gave_up`, `p50_ms`, `p95_ms`, `p99_ms`; `workload_done.actions` summed over
`measured_s`; `client_users`.

**CPU, memory, network and queues (`tab:s9res`):**

| Users per PC | Node cores | Node cores, highest | Node memory, MB | Node network, MB/s | Queued, highest | Client PC CPU busy, p95 | Client PC memory free, lowest, MB |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 100 | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| 1,000 | TBD | TBD | TBD | TBD | TBD | TBD | TBD |
| 5,000 | TBD | TBD | TBD | TBD | TBD | TBD | TBD |

Sources: `cpu_cores_mean`, the highest per node, `rss_mb_max`, `net_bytes_per_s_mean`, `queued_max`,
`client_pc_cpu_busy_p95`, `client_pc_mem_available_min_mb`.

## 8. Phase 1 scenarios on the Phase 2 build (`tab:regression`)

| Scenario | Phase 1 build (phase1-report.md) | Phase 2 build | Same |
|---|---|---|---|
| 1. Normal cloud operation | Each node won 31.4% to 34.5% at every load from 1,000 to 9,000 a second | TBD | TBD |
| 2. Load imbalance | The slowed node's share fell from 33.4% to 4.9% and came back to 33.3% | TBD | TBD |
| 3. Server failure | The others went on without the silent node in about 1.04 s; nobody gave up up to 20,000 a second | TBD | TBD |
| 4. Server recovery | A full member again 0.8 s to 10.5 s after the fault up to 20,000 a second; one digest at the end in all 45 runs | TBD | TBD |
| 9. Scale and stress | Held 12,000 a second with planning work 1000, and 30,000 with planning work 0 | TBD | TBD |

TBD: which runs. The Phase 1 numbers came from `scripts/phase1/run-experiments.sh series` (open loop at set rates),
not from the generic cells; rerun the same steps with `[workload] lifecycle = false`, or say what was run instead.

## 9. Tests (`tab:testfiles`, `tab:labtests`)

| Area | Test file | Tests on main, 2026-11-26 | Tests on the final build |
|---|---|---:|---:|
| Presence directory | `tests/discovery.rs` | 8 | TBD |
| Presence agent | `tests/presence.rs` | 8 | TBD |
| P2P link | `tests/p2p.rs` | 5 | TBD |
| Zone gossip | `tests/gossip.rs` | 7 | TBD |
| Offers and claims | `tests/claims.rs` | 3 | TBD |
| Offline reports | `tests/offline.rs` | 4 | TBD |
| Store-and-forward | `tests/store_forward.rs` | 3 | TBD |
| Reconnection | `tests/reconnect.rs` | 3 | TBD |
| Load programs, web page | `src/bin/loadgen.rs`, `tests/frontend.rs` | on the part-l branches | TBD |

Whole suite on the final build: TBD tests, TBD added in Phase 2, TBD failures.

| Lab check | Result on the final build |
|---|---|
| Same build everywhere, stored state wiped | TBD |
| Peer ports open on the client PCs | TBD |
| Clocks | TBD |
| End-to-end session | TBD |
| Every request accounted for; state, directory and claims digests equal | TBD |
| The cloud out of the peer path | TBD |
| Phase 1 still passes | TBD |
