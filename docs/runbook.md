# Six-machine runbook

How to run RescueMesh on the lab machines: three cloud nodes and three client machines on one network.
Nothing here has been run on those machines yet, so expect to adjust it, and fix this page when you do. The
demo itself is in [demo.md](demo.md).

## 1. Prepare every machine (once)

All six run Ubuntu. On each one:

```bash
sudo apt install build-essential rsync chrony      # C linker, log collection, NTP
curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none
source "$HOME/.cargo/env"
git clone https://github.com/noortaytoy1/DistributedSystems.git
```

On each client machine, check for a browser: `which firefox`. If there is none, use `ssh -L
9001:127.0.0.1:9001 ubuntu@<client machine>` from a laptop on that network. If that is not possible, the demo runs
from the terminals.

- **Toolchain.** `rust-toolchain.toml` picks the compiler on the first build. Do not install another one
  over it: an apt Rust earlier on `PATH` cannot build this repo.
- **Clock.** `chronyc tracking` **must** show the clock in sync on all six before any test. Each machine's timestamps are
  only roughly aligned with the others', so durations are always measured on one machine. A bad clock on a node changes
  the times and labels of incidents it planned, on every node and for good, though never a plan or a hold (D142). A bad
  clock on a client PC changes when the planner's fallback fires and how high a report's priority is.
- **Disk.** Keep the repo on a local disk, and check free space on all six before a long series. Commits sync to
  `storage.dir`, so a slow or shared disk shows up as commit latency. `raft_io` gives the real disk: `sync_us / syncs` and
  `saved_commands / saves` (D133). Never use a shared filesystem: each machine keeps its own logs.

On the three node machines, open the ports:

```bash
sudo ufw allow 7001/tcp      # peers: reachable from the other two nodes
sudo ufw allow 8001/tcp      # clients and ctl: reachable from the client machines
sudo ufw allow 8001/udp      # ONLY with the UDP client path (D216), which has not merged yet. Reports then use UDP, and ctl stays on TCP
```

The UDP line depends on that work: skip it until the build you are running has it. Then `ctl status` says `udp no answer` if the rule is
missing.

The frontend needs no open port when the browser is on the same machine. Only if neither that nor `ssh -L` works, use
`--listen 0.0.0.0:9001` and `sudo ufw allow 9001/tcp` on that client machine. There is no password (D199).

From the machine that runs the experiments, every machine needs an SSH key with no prompt:

```bash
ssh-copy-id ubuntu@10.0.0.11     # and the other five
ssh -o BatchMode=yes ubuntu@10.0.0.11 true
```

## 2. One config, one commit

Fill in the addresses once, and copy the same files everywhere. A different node list on one machine breaks
Raft membership.

```bash
cp config/lab.example.toml config/lab.toml         # set the three nodes' real LAN addresses
cp experiments/hosts.example experiments/hosts     # the same addresses, plus the three clients
scripts/lab-build.sh experiments/hosts             # the same commit on all six, or it stops
for h in ubuntu@10.0.0.{11,12,13,21,22,23}; do scp config/lab.toml "$h:DistributedSystems/config/"; done
```

Both of those files are ignored by git, because they hold real addresses.

**After this upgrade, wipe the Raft logs once on all three nodes** (`rm -rf data/raft`). The log's format changed. An old log
still has good checksums but does not decode, so a node would cut it with only a warning. Every machine must run the same
commit: a mixed build looks like a dead link.

## 3. Start by hand

```bash
# on node machine 1, 2 and 3, each with its own ID
target/release/node --config config/lab.toml --id 1

# from any machine: all three should show one leader and equal digests
target/release/ctl --config config/lab.toml status

# on client machine 1, 2 and 3, each with its own machine number
bash scripts/start-users.sh --config config/lab.toml --machine 1 --users 50 --duration-s 60

# on client machine 1, 2 and 3: the web page, with IDs 11, 12 and 13. --config has no default on purpose.
target/release/frontend --config config/lab.toml --client-id 11 --listen 127.0.0.1:9001
# then open http://127.0.0.1:9001 in that machine's browser
```

The `ctl status` table is 109 characters wide. `watch` cuts at the terminal width, so widen the terminal or `members`
disappears.

Each client machine runs 10 real `loadgen` programs that share its users. Stop early with Ctrl-C, or from anywhere with
`bash scripts/start-users.sh --stop --machine 1`. **Machine numbers must differ**, or client IDs collide. By hand, a single
`loadgen` takes a client ID from 1 to 9: 11 to 13 belong to frontends, 21 to 23 to zone feeds in Phase 2, and five-digit IDs to
this script. A second group of users on one machine needs its own `--batch`.

Node output: `RUST_LOG=info,tarpc=info` brings tarpc's per-call lines back for debugging. Never run an experiment that way: they
slow a node enough to cost it its leader (D134).

`faults.enabled = true` in the lab config means the three nodes also elect one of them to go silent, about every 30 to 60 s. For anything
you want to control yourself, such as the demo, set this in `config/lab.toml` first:

```toml
[faults]
enabled = true                # keeps ctl silence working
interval_min_ms = 100000000   # no node's timer ever fires, so no fault election
interval_max_ms = 100000000
```

## 4. Check the settings before measuring

```bash
scripts/run-experiments.sh experiments/hosts quick e1-300-bidding
python analysis/analyze.py runs
grep -E 'service_mean_us|no_responder_share|reports_expired|msgs_all_per_req' analysis/out/summary.csv
```

`reports_expired` must be 0, and `analyze.py` warns otherwise. With the 60 s default the refusal cannot fire in a run, so a
nonzero count most likely means an age stamped wrongly, which looks like a very fast run where nobody gets sent (D147).

`service_mean_us` should be near 4000. `plan_work = 1000` was calibrated on a development machine (D129), so
if the lab machines differ much, change it in every cell and say so in the report.

Also confirm timers on the real network: `bid_window_ms`, `peer_heartbeat_ms` and the election timeouts are LAN
defaults (D40) that have only ever run on one machine.

**Is the leader stable?** One `e1-300-bidding` must show no new terms: look at `elections` in that first cell. The proposal queue
(D133) is what keeps it so, and it has passed its tests but has not been measured on these machines.

## 5. Run the experiments

```bash
scripts/run-experiments.sh experiments/hosts quick      # 15 cells, about 70 minutes
scripts/run-experiments.sh experiments/hosts full       # 33 cells, about 8 hours
```

`quick` runs `ft8-random-faults` and `ft8-election-rate` first, because they are the first real logs the fault-election analysis sees: a bug
costs minutes, not the hour.

If fault elections misbehave on the real network, keep the code and leave them out: pass an explicit cell list without the `ft8` cells. `lab.toml`
does not reach the cells, because the runner builds each run's config from the cell file. Use `ctl silence` in the demo, and say in the report that
this was an accepted risk.

### Check the fault election on the real network

```bash
# Start everything with fast elections, in config/lab.toml on all six: interval 1000 to 1500 ms, silence 100 to 1000 ms.
# Cut one node machine's power under load. On the other two node machines, for at least 10 s:
grep '"ev":"fault_election"' runs/lab/node-*.jsonl | tail      # victim must be null on every new line
grep -c '"ev":"silenced"' runs/lab/node-*.jsonl                # must not rise
```

Then power it back on, and check a later round closes on all three. Also check the 1 s round deadline on the real network: lines with a null victim
while all three are up mean bids are not getting through in time, or the term is moving (a bid from an older term is dropped).

**Stop every frontend first.** The runner refuses to start a run while a `frontend`, `zonefeed`, `loadgen` or `node` from `$BIN`
(`target/release`) is running on any machine, and prints which, because a leftover gives wrong figures with no sign of it. Stop it by
hand and start again. It does not stop them itself, and a program started from anywhere else is not noticed. A run where a load
program ran beside its own, or with no leader at the start, is deleted and the batch stops: `analysis/analyze.py` reads every
`run.json` it finds, so a bad run must not stay. A run whose fault could not be injected after 3 tries goes to
`runs/dropped/<cell>-run-<n>/`, with the reason in `dropped.txt`. The batch goes on, and at the end it lists those runs and exits
non-zero. Run those cells again, by name (D139). If a cell keeps landing there, it is above what the machines can hold.

`PROGRAMS_PER_PC` changes the 10 programs per client machine, with no rebuild. Use one value for all final data; `run.json` records
it (D181). A rerun moves the old run folder to `runs/old/` (D186).

Between runs the runner wipes `storage.dir` and `metrics.dir` on every machine, so no state carries over.
If you run by hand instead, wipe them yourself:

```bash
rm -rf data/raft runs/lab
```

**Never wipe `storage.dir` during a recovery test:** that is the state a powered-off node comes back from
(D118).

## 6. Collect and analyse

The runner copies logs to `runs/<experiment>/<cell>/run-<n>/` on the machine it runs on, next to the config,
the commit and the final `ctl status`. Keep them until the report is done. They never go into git.

```bash
python3 -m venv .venv && . .venv/bin/activate
pip install -r analysis/requirements.txt
python analysis/analyze.py runs      # analysis/out/summary.csv and plots
```

`digest_mismatches` must be 0: any other value is a real bug, so keep the logs. `plans_refused` above 0 means a worker pool shed
work in that cell, and `elections` well above 0 means the cell is above what the machines can hold.

## 7. If something goes wrong

| Symptom | Likely cause |
|---------|--------------|
| `ctl status` says unreachable | the node is down, or port 8001 is closed |
| No leader, and nodes log connect failures | port 7001 is closed between the nodes, or an address is wrong |
| A node logs `Address already in use` | its old process is still running |
| A node refuses to start: unknown key | the config is from another commit; rebuild and copy it again |
| Digests stay different after everything is serving | a real bug: keep the logs, it is what the digest exists to catch. Under load the column can differ from one refresh to the next; compare `state_checkpoint` at the same `log_index` instead (D138) |
| `fault_election` lines with `victim` null, over and over | a round could not finish: one node is off, silenced, catching up or degraded, or has `--faults off`. That is the rule working (D158, D159) |
| `degraded` is true with both peers running | the peers miss heartbeats for 2 s: an overloaded machine or a bad network (D121) |
| A returning node stays `serving = false` | it hears no leader; after 3 s it may stand itself (D125), but it needs one peer for a majority. It asks the leader again every second (D135), so it is not waiting on a lost index. Look for `rejoin: still applying` in its log |
| The runner says `left over on <host>` | a program from an earlier run or a person's terminal. Stop it by hand and run again (D139) |
| The runner says `cannot check <host>` | ssh to that machine failed |
| The runner says `run dropped and batch stopped` | read the reason on that line. Earlier runs are kept. Re-run the cells that are missing |
| The runner ends with `these never got their fault` | the node missed `ctl` for 6 s or more, usually overload. Logs are under `runs/dropped/`. Run those cells again by name |
| `start-users: machine M batch B is already running` | it is, or another start got there first: `--stop`, or another `--batch` |
| The runner stops the series with a status of 2 or more | a client machine refused to start, lost over a tenth of its programs, or cannot be reached (255): read `users-*.out` in the run folder |
| `analyze.py` prints `left out ...` | a run had missing programs, or its fault never fired: rerun it |
| Elections keep happening with no fault | the planner threads are using every core (D48: at most cores minus 1), or a binary without the proposal queue (D133). Check `max_leader_gap_us` in `raft_io` |
| `ctl` errors, and client links churn with `bad frame` | a stale binary on one machine: `NodeStatus` grew (D155). Rebuild all six from one commit |
| `members` shows `-` | that node is silenced; its list is frozen and not worth reading |
| `members` differs between nodes | normal for a moment: each list is one node's opinion. Lasting differences with everyone serving point at a network problem between two nodes |
| Latency is very low, `no responder` is near 100% and `expired` is high | an age stamped wrongly, or `--ttl-ms` passed by mistake: check the loadgen command and the commit (D143) |
| `ctl incidents` shows different `log_index` values for one ID, for longer than a moment | a real bug, the same as digests that stay different. A brief `assigned` against `resolved` is only catch-up |
| The page says "Local program not answering" | the frontend on this PC is stopped or stuck; the nodes may be fine, so check with `ctl status` |
| All three rows on the page say unreachable | the wrong `--config`, or the nodes really are down; `ctl status` tells which |
| The frontend cannot start: `Address already in use` | another frontend is on that port; pick another `--listen` |
| A program row on the page says failed | read the frontend's terminal: its programs' stderr goes there |
