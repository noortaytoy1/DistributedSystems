# Analysis

Python tools that turn event logs into the numbers and plots in the report (D64). They are not part of
the system, and need no Rust.

```bash
sudo apt install python3-venv          # Ubuntu packages it apart from Python. Not needed if the next line works
python3 -m venv .venv && . .venv/bin/activate
pip install -r analysis/requirements.txt    # Python 3.10, as on Ubuntu 22.04, or newer: it pins versions for each
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
- `src`: `node-<id>`, `client-<id>`, or `frontend-<id>` for the web page, which the analysis ignores.
- `ev`: the event name. The other fields depend on it.

## Output

`summary.csv` has one row per cell and metric: the mean across runs, the number of runs, and the half-width
of the 95% confidence interval (Student's t). With one run there is no interval.

| Spec asks for | Metrics |
|---------------|---------|
| Latency: mean, median, p95, p99 | `mean_ms`, `p50_ms`, `p95_ms`, `p99_ms`; provisional answers apart, as `provisional_*` |
| Throughput against clients | `throughput_rps`; plot `e1_latency_throughput.png`. Print `answered_with_responder_rps` beside it: `throughput_rps` also counts answers that sent nobody, and those are about as fast as real ones (D229). All of it is closed loop unless the run used `--rate` |
| Election frequency, duration, message overhead | Bidding: `bid_rounds`, `bid_window_mean_ms`, `bid_split_decisions`, `msgs_bid_per_req`. Raft: `elections`, `election_duration_mean_ms` (the vote alone, about 10 ms, and not how long the cloud had no leader, D229), `msgs_raft_vote_per_req`. Fault election: `fault_elections`, `fault_rounds_abandoned`, `fault_election_mean_ms`, `fault_bids_per_round`. All messages: `msgs_all_per_req` |
| Fault election, its safety checks (D161) | `fault_split_decisions` (must be 0), `fault_elected_not_silenced`, `fault_refused` |
| Load distribution and fairness | `share_node_<n>`, `fairness_jain`; plot `fairness.png` |
| Whether holding the leader role moves work (D166) | `leader_plan_share`, read beside `elections` and `share_node_<n>`. `share_node_<leader>` when one node led all of the measured time. NaN if the leader changed, a node read another leader, or there was a moment with no leader, so a cell can average over fewer runs than it has |
| What a follower pays for not leading (D26, D166) | `commit_local_*` and `commit_forwarded_*`, the same figures as `commit_*`, split by path |
| Before, during and after a failure | `before_*`, `during_*`, `after_*`; plot `fault_phases.png`, which leaves out cells with faults on a timer (`ft8-*`, `e5-*`). Unlike `p50_ms` and the rest, these mix provisional answers with committed ones, so in a degraded cell `during_*` reads far better than `before_*`: `analyze.py` prints a note (D229) |
| Failover and recovery | `failover_to_leader_ms_mean` and `failover_to_commit_ms_mean` (FT-2: from when the leader went silent or stepped aside to the next leader's first line, and to the first result after it), with `failovers` and `failovers_without_leader`, which counts losses that no later leader followed and that are in neither mean. `client_gap_mean_ms`, `client_gap_max_ms` (E3's client-visible gap, per program, so it understates one user's wait), `worst_request_ms` (the longest any one request waited, answered or given up on), `detection_mean_ms`, `recovery_mean_ms`, `elections_after_rejoin` (FT-6). A client that never hears again counts its whole silence in the gap, so read it with `gave_up` (D224). Not `election_duration_mean_ms`, which is the vote alone (D229). With no majority left, as in a degraded cell, `failover_to_*` reads about the length of the outage. A power cut logs no `silenced`, so `failover_to_*` and the gap stay empty: the lab guide's T1 reads it from a survivor's log |
| Convergence after a rejoin (D138) | `digest_checkpoints_compared`, `digest_mismatches` (must be 0), `rejoins_digest_confirmed` against `rejoins`, `rejoin_targets_lowered` (D135) |
| CPU, memory, network, queues | `cpu_cores_mean`, `rss_mb_max`, `net_bytes_per_s_mean`, `queued_mean`, `queued_max`, `running_mean`; shed work as `plans_refused`, `plans_refused_node_<n>` (D138) |
| Duplicates and in-flight requests | `gave_up`, `retried_share`, `takeovers`, `takeovers_suspected`, `takeovers_backstop`, `takeover_wait_suspected_mean_ms`, `takeover_wait_backstop_mean_ms`. Not failover figures: a backstop takeover needs no fault, only a busy winner (D138) |
| Plans with no responder (D111) | `no_responder_share`; an expired report counts here too, and so does a report whose plans went stale three times (D220) |
| Reports refused as too old (D147) | `reports_expired`. Should be 0 in every measured cell: `analyze.py` prints a warning otherwise, because the likeliest cause is a stamping bug |
| Incident priority (D148) | `priority_mean`, in priority points. 0 only for a refused report |
| Degraded mode (D120) | `degraded_entered`, `degraded_mean_ms`, `provisional_replied`, `provisional_conflicts` (answers that did not stand on record, which clients are not told of, D122, D229), `conflicts_*` by reason |
| Commit latency, planning cost | `commit_*`, `service_mean_us`. The second comes from bids, so it is NaN in leader-dispatch cells |
| Client model, and whether the client PCs were the limit (D181, D185) | `client_programs`, `client_users` (users the programs say they ran), `users_per_program`, `client_programs_failed`, `client_programs_stopped_early`, `client_scripts_with_no_totals`, `client_link_down` and `client_link_down_<reason>` (0 on UDP, kept for old runs, D172), `client_pc_cpu_busy_p95` (share of the PC's CPU, 0 to 1), `client_pc_mem_available_min_mb` |
| Whether arrivals clump (D182) | `arrival_peak_100ms_share` over the measured time, `arrival_peak_100ms_share_first_s` for the first second of the run, `arrival_peak_100ms_share_after_fault`. Even arrivals read 0.1, and all together 1 |
| The UDP client path (D172): what it carried | `udp_requests_per_req`, `udp_cancels_per_req`, `udp_datagrams_per_req`. Cancels are outside the `msgs_` figures, which count what nodes send |
| The UDP client path: what it lost | `udp_copies_lost_share`: copies of reports sent against copies the nodes handled or their gates turned away, counted from events that cannot miss, over each process's whole log, warm-up included. A node that is off reads none of its copies, so in a run with `kill -9` this is not all network loss. `udp_send_lost`, `udp_queue_full`, `udp_bad`, and `udp_gate_dropped` for reports a silenced node dropped. **Do not quote `udp_request_loss_share` or `udp_cancel_loss_share`.** They read about 1% with nothing lost, because a node's last counters are older than a client's last send, and they would hide a real loss below that (D229). They stay for scripts that read them |
| The UDP receive buffer (D171) | `udp_rcvbuf_got_min_bytes`, `udp_drops_nodes`, `udp_drops_client_pc_max`. `analyze.py` prints a warning for a capped buffer or any drops. Each lost copy can cost a client a 2 s retry |

**Runs left out (D186).** `summarise` names a run on screen and does not average it in if:

- fewer programs sent anything than `run.json` expected, or the programs' users do not add up to the cell's, which
  means less load than the cell says;
- a machine's `users-*.out` says a program failed or a signal stopped programs early, or has no totals at all. A
  program that dies half way has still sent something, so only those lines show it;
- a node's log did not get copied, which would make fairness read as the other nodes' share;
- the cell has a victim and no node logged `silenced`, which means the fault never fired;
- a log has a line that cannot be read, other than half a line at its very end. A killed program leaves that half
  line, so it is dropped and named, and the run still counts unless one of the rules above catches it.

A `run.json` without the newer keys skips the check on program counts. A cell with a run left out has fewer runs
behind its interval, shown in the `runs` column of `summary.csv`, and needs a rerun.

## Conventions

- Latency comes from the `latency_us` fields, which are measured with a monotonic clock on one
  machine. Never subtract `ts_us` values written by different machines. There are two exceptions. One is deciding
  which phase of a fault a request fell in: the edges are only as good as NTP, so treat them as approximate. The other is
  `failover_to_*`, from the old leader's clock to another node's: a few ms of NTP error against hundreds (D229).
- Report percentiles with confidence intervals across runs, not just one run.
- Fixtures in `test_analyze.py` are synthetic. Real numbers never go into the docs, only into the report
  with the run they came from.
