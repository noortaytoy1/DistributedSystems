# Analysis

Scripts that turn event logs into the numbers and plots in the report.

## Input

Every process writes JSON Lines, one event per line (see [src/metrics.rs](../src/metrics.rs)):

```json
{"ts_us": 1792000000123456, "src": "node-1", "ev": "commit", "kind": "commit_result", "outcome": "committed", "latency_us": 812, "forwarded": false, "log_index": 42, "request": {"client": 7, "seq": 3}}
```

- `ts_us`: unix microseconds on the writing machine's clock.
- `src`: `node-<id>` or `client-<id>`.
- `ev`: the event name. The other fields depend on it.

## Conventions

- Latency comes from the `latency_us` fields, which are measured with a monotonic clock on one
  machine. Never subtract `ts_us` values written by different machines.
- Report p50, p95 and p99 with confidence intervals across runs, not just one run.
- Generated output goes to `analysis/out/`, which is ignored by git.

The analysis language is pending the instructor's approval (Python is proposed for analysis
only).
