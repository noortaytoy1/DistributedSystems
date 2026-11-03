# Analysis

Python tools that turn event logs into the numbers and plots in the report (D64). They are not part of
the system, and need no Rust.

```bash
python3 -m venv .venv && . .venv/bin/activate
pip install -r analysis/requirements.txt
python analysis/test_analyze.py        # checks the arithmetic on a synthetic log
python analysis/analyze.py runs        # writes analysis/out/ (ignored by git)
```

## Input

`runs/<experiment>/<cell>/<run>/`, as `scripts/run-experiments.sh` writes it: `run.json` and every
process's JSON Lines log, one event per line (see [src/metrics.rs](../src/metrics.rs)):

```json
{"ts_us": 1792000000123456, "src": "node-1", "ev": "commit", "kind": "commit_result", "outcome": "committed", "latency_us": 812, "forwarded": false, "log_index": 42, "request": {"client": 7, "seq": 3}}
```

- `ts_us`: unix microseconds on the writing machine's clock.
- `src`: `node-<id>` or `client-<id>`.
- `ev`: the event name. The other fields depend on it.

## Output

`summary.csv` has one row per cell and metric: the mean across runs, the number of runs, and the half-width
of the 95% confidence interval (Student's t). With one run there is no interval.

| Spec asks for | Metrics |
|---------------|---------|
| Latency: mean, median, p95, p99 | `mean_ms`, `p50_ms`, `p95_ms`, `p99_ms`; provisional answers apart, as `provisional_*` |
| Throughput against clients | `throughput_rps`; plot `e1_latency_throughput.png` |
| Election frequency, duration, message overhead | Bidding: `bid_rounds`, `bid_window_mean_ms`, `bid_split_decisions`, `msgs_bid_per_req`. Raft: `elections`, `election_duration_mean_ms`, `msgs_raft_vote_per_req`. All messages: `msgs_all_per_req` |
| Load distribution and fairness | `share_node_<n>`, `fairness_jain`; plot `fairness.png` |
| Before, during and after a failure | `before_*`, `during_*`, `after_*`; plot `fault_phases.png` |
| Failover, recovery, convergence | `detection_mean_ms`, `takeover_wait_mean_ms`, `recovery_mean_ms`, `elections_after_rejoin` (FT-6) |
| CPU, memory, network, queues | `cpu_cores_mean`, `rss_mb_max`, `net_bytes_per_s_mean`, `queued_mean`, `queued_max`, `running_mean` |
| Duplicates and in-flight requests | `gave_up`, `retried_share`, `takeovers` |
| Plans with no responder (D111) | `no_responder_share` |
| Degraded mode (D120) | `degraded_entered`, `degraded_mean_ms`, `provisional_replied`, `conflicts_*` |
| Commit latency, planning cost | `commit_*`, `service_mean_us` |
| Client model, and whether the client PCs were the limit (D181, D185) | `client_programs`, `users_per_program`, `client_link_down`, `client_link_down_<reason>`, `client_pc_cpu_busy_p95` (share of the PC's CPU, 0 to 1), `client_pc_mem_available_min_mb` |
| Whether arrivals clump (D182) | `arrival_peak_100ms_share` over the measured time, `arrival_peak_100ms_share_first_s` for the first second of the run, `arrival_peak_100ms_share_after_fault`. Even arrivals read 0.1, and all together 1 |

**Runs left out (D186).** `summarise` names a run on screen and does not average it in if:

- fewer programs sent anything than `run.json` expected, which means less load than the cell says;
- a machine's `users-*.out` says a program failed or a signal stopped programs early, or has no totals at all. A
  program that dies half way has still sent something, so only those lines show it;
- the cell has a victim and no node logged `silenced`, which means the fault never fired;
- a log has a line that cannot be read, other than half a line at its very end. A killed program leaves that half
  line, so it is dropped and named, and the run still counts unless one of the rules above catches it.

A `run.json` without the newer keys skips the check on program counts. A cell with a run left out has fewer runs
behind its interval, shown in the `runs` column of `summary.csv`, and needs a rerun.

## Conventions

- Latency comes from the `latency_us` fields, which are measured with a monotonic clock on one
  machine. Never subtract `ts_us` values written by different machines. The one exception is deciding
  which phase of a fault a request fell in: the edges are only as good as NTP, so treat them as approximate.
- Report percentiles with confidence intervals across runs, not just one run.
- Fixtures in `test_analyze.py` are synthetic. Real numbers never go into the docs, only into the report
  with the run they came from.
