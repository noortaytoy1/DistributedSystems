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
| Election frequency, duration, message overhead | Bidding: `bid_rounds`, `bid_window_mean_ms`, `bid_split_decisions`, `msgs_bid_per_req`. Raft: `elections`, `election_duration_mean_ms`, `msgs_raft_vote_per_req`. Fault election: `fault_elections`, `fault_rounds_abandoned`, `fault_election_mean_ms`, `fault_bids_per_round`. All messages: `msgs_all_per_req` |
| Fault election, its safety checks (D161) | `fault_split_decisions` (must be 0), `fault_elected_not_silenced`, `fault_refused` |
| Load distribution and fairness | `share_node_<n>`, `fairness_jain`; plot `fairness.png` |
| Before, during and after a failure | `before_*`, `during_*`, `after_*`; plot `fault_phases.png`, which leaves out cells with faults on a timer (`ft8-*`, `e5-*`) |
| Failover, recovery, convergence | `detection_mean_ms`, `takeover_wait_mean_ms`, `recovery_mean_ms`, `elections_after_rejoin` (FT-6) |
| CPU, memory, network, queues | `cpu_cores_mean`, `rss_mb_max`, `net_bytes_per_s_mean`, `queued_mean`, `queued_max`, `running_mean` |
| Duplicates and in-flight requests | `gave_up`, `retried_share`, `takeovers` |
| Plans with no responder (D111) | `no_responder_share` |
| Degraded mode (D120) | `degraded_entered`, `degraded_mean_ms`, `provisional_replied`, `conflicts_*` |
| Commit latency, planning cost | `commit_*`, split by path as `commit_local_*` and `commit_forwarded_*` (D26, D166); `service_mean_us` |
| Whether holding the leader role moves work (D166) | `leader_plan_share`, read beside `elections` and `share_node_<n>`. `share_node_<leader>` when one node led all of the measured time. NaN if the leader changed, a node read another leader, or there was a moment with no leader, so a cell can average over fewer runs than it has |

## Conventions

- Latency comes from the `latency_us` fields, which are measured with a monotonic clock on one
  machine. Never subtract `ts_us` values written by different machines. The one exception is deciding
  which phase of a fault a request fell in: the edges are only as good as NTP, so treat them as approximate.
- Report percentiles with confidence intervals across runs, not just one run.
- Fixtures in `test_analyze.py` are synthetic. Real numbers never go into the docs, only into the report
  with the run they came from.
