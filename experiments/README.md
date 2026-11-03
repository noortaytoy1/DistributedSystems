# Experiments

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
| E5 | faults on or off | elections, messages by type, commit latency, by path | A | `e5-faults-off`, `e5-faults-on` |
| FT-8 | elected faults throughout | lost requests, duplicate commits, split decisions (must be 0) | C | `ft8-random-faults` |
| FT-8 | fast elected faults | election frequency, duration and message overhead, for the spec's three numbers; its silences are shorter than the spec's | C | `ft8-election-rate` |
| P-2 | bid window | split decisions | B | `p2-window-<n>ms` |
| D120 | load, with two nodes silent | provisional answers, conflicts, replay | C | `degraded-<users>` |
| E7 | one program per machine against 10 | whether the number of programs changes latency, client machine CPU or arrivals; compare with `e1-300-bidding` | F | `e7-p1-300` |

`e1-1000-*` and `e1-3000-*` use 3000 responders and the lower E1 cells 300, so a line across that gap on a figure is not the effect of load alone (D219).

E5's `msgs_` figures count what nodes send. A client's cancels are in `udp_cancels_per_req` and `udp_datagrams_per_req`: print those beside them (D172).

## Profiles (D63)

| Profile | Cells | Runs per cell | One run | Time in all |
|---------|-------|---------------|---------|-------------|
| `quick` | the 15 in `quick.list` | 3 | 15 s warm-up + 60 s measured + 15 s restart | about 70 minutes |
| `full` | all 33 | 5 | 30 s warm-up + 2 min measured + 15 s restart | about 8 hours |

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
much, change it in every cell, and say so in the report.

**The leader must hold.** Final data must come from a build with the proposal queue (D133) and the group view in: load figures
from before the queue cannot be compared with figures from after it, and `msgs_bid_per_req` falls in fault cells compared with any
older run (D154).

`experiments/hosts` stays in order 1, 2, 3, whatever `config/lab.example.toml` lists: reordering it moves the skew target and each node's election range. Every run also holds a few duplicate `RegisterResponders` entries, because any serving node may propose the list
(D164), so digests differ from any run made before that change. They change no state.

The cells past the knee: a report whose plans go stale three times now commits that nobody could be sent, so overload shows as a higher `no_responder_share` at low latency, where before it showed as long latency and give-ups. Comparing with runs from before D220 would mislead.
`e3e4-1500-*`, `e4-1500-*` and `degraded-1500` still run 1500 users on 300 responders, so read their fault and recovery figures with D219 in mind.

No cell has anything to do with the TTL (D147). The runner never passes `--ttl-ms`, and with the 60 s default a report cannot
be refused in a run, because the client's retries end first. So `reports_expired` must be 0 in every cell, and `analyze.py`
warns otherwise: the likeliest cause is an age stamped wrongly, which looks like a very fast run where nobody gets sent. An
expired report also counts in `no_responder_share`.

`cargo test --test experiment_cells` checks that every cell is a valid config.
