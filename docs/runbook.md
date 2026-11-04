# Six-machine runbook

How to run RescueMesh on the lab machines: three cloud nodes and three client machines on one network.
Nothing here has been run on those machines yet, so expect to adjust it, and fix this page when you do. The runner has run over a
stand-in for ssh in `cargo test` (`tests/runner_script.rs`, `tests/runner_over_ssh.rs`), which shows how a far shell reads each line, and not
firewalls, keys or sshd limits. The demo itself is in [demo.md](demo.md).

## 1. Prepare every machine (once)

All six run Ubuntu. On each one:

```bash
sudo apt install build-essential rsync chrony python3-venv      # C linker, log collection, NTP, the analysis
curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none
source "$HOME/.cargo/env"
git clone https://github.com/noortaytoy1/DistributedSystems.git
```

A fresh clone gets `main`. Build only once the PR you mean to run has merged, or `git switch` to its branch first, and check that
`experiments/e8-step-aside.toml` exists: six PCs on one old commit would still pass the check that all six agree.

On each client machine, check for a browser: `which firefox`. If there is none, use `ssh -L
9001:127.0.0.1:9001 ubuntu@<client machine>` from a laptop on that network. If that is not possible, the demo runs
from the terminals.

- **Toolchain.** `rust-toolchain.toml` picks the compiler on the first build. Do not install another one
  over it: an apt Rust earlier on `PATH` cannot build this repo.
- **Build folder.** Leave `CARGO_TARGET_DIR` unset here. The runner looks in `target/release`, or in `BIN` if you set it.
- **Clock.** `chronyc tracking` **must** show the clock in sync on all six before any test. Each machine's timestamps are
  only roughly aligned with the others', so FT-2 and every latency are measured on one machine, and `analyze.py` says how far apart the clocks were (`node_clock_skew_ms`, D233). A bad clock on a node changes
  the times and labels of incidents it planned, on every node and for good, though never a plan or a hold (D142). A bad
  clock on a client PC changes when the planner's fallback fires and how high a report's priority is.
- **Disk.** Keep the repo on a local disk, and check free space on all six before a long series. Commits sync to
  `storage.dir`, so a slow or shared disk shows up as commit latency. `raft_io` gives the real disk: `sync_us / syncs` and
  `saved_commands / saves` (D133). Never use a shared filesystem: each machine keeps its own logs.

On all six machines, make room for UDP bursts. Without this the kernel quietly caps the receive buffer, and a burst loses reports
(D171):

```bash
sudo sysctl -w net.core.rmem_max=8388608
echo 'net.core.rmem_max=8388608' | sudo tee /etc/sysctl.d/90-rescuemesh.conf     # keeps it after a reboot
```

On the three node machines, open the ports:

```bash
sudo ufw allow 7001/tcp      # peers: reachable from the other two nodes
sudo ufw allow 8001/udp      # reports from clients: reachable from the client machines
sudo ufw allow 8001/tcp      # ctl: reachable from wherever you run it
```

The frontend needs no open port when the browser is on the same machine. Only if neither that nor `ssh -L` works, use
`--listen 0.0.0.0:9001` and `sudo ufw allow 9001/tcp` on that client machine. There is no password (D199).

From the machine that runs the experiments, every machine needs an SSH key with no prompt:

```bash
ssh-copy-id ubuntu@10.0.0.11     # and the other five
ssh -o BatchMode=yes ubuntu@10.0.0.11 true
```

The runner's `ssh` gives up on a machine after 10 s when connecting, and after about a minute of silence during a run, so a machine that loses power
stops the series with a message. The runner expects the repo at `~/DistributedSystems` on every machine, or at `~/$REMOTE_DIR` if you set that. It sends
each line as `cd DistributedSystems || exit 125; <line>`, so a machine without the folder stops the batch at once.

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

The example file is set up for the demo: it lists the nodes 3, 2, 1, runs E1's `plan_work` and responders, and has the fault election's timers off (D165). Keep `experiments/hosts` in order
1, 2, 3: the runner builds each run's config from it and the cell, and never reads `lab.toml`, so none of this reaches the data.

**Rebuild after every `git pull`, on all six machines.** `scripts/lab-build.sh experiments/hosts` does it and checks the commit. By hand, on each
machine: `cargo build --release --bins`. Every machine must run the same commit, because an old client and a new node fail quietly: a mixed build looks like a
dead node (D175).

**Wipe the Raft logs on all three nodes whenever the version changes** (`rm -rf data/raft`, with the nodes stopped). A new field in a log entry changes the
file's format. An old log still has good checksums but does not decode, so a node would cut it with only a warning. Never wipe it during a recovery test (section 5).

## 3. Start by hand

```bash
# on node machine 3 first, then about a second later on 2 and 1, each with its own ID
target/release/node --config config/lab.toml --id 3

# from any machine, before any load: all three should show one leader and equal digests
target/release/ctl --config config/lab.toml status
grep assignment config/lab.toml      # must say bidding

# on client machine 1, 2 and 3, each with its own machine number
bash scripts/start-users.sh --config config/lab.toml --machine 1 --users 20 --duration-s 60

# on client machine 1, 2 and 3: the web page, with IDs 11, 12 and 13. --config has no default on purpose.
target/release/frontend --config config/lab.toml --client-id 11 --listen 127.0.0.1:9001
# then open http://127.0.0.1:9001 in that machine's browser
```

**First check, every time.** From a client machine, run `ctl status`. Under the table there is one line per node. `node 1: udp ok` is what you want.
`node 3: udp no answer` means reports to that node vanish, even though its row above looks healthy, because the table comes over TCP: open `8001/udp` on it.
A silenced node still says `udp ok`. Then look in a node's stderr for `UDP receive buffer capped by the kernel`, which means the sysctl above was missed. The
runner stops a batch by itself on `udp no answer`, but not on a capped buffer: `analyze.py` warns about that afterwards.

Node 3 tends to lead with that order, because the first node listed has the shortest election timeout (D115) and the first node started is already asking for votes. That only tends to happen: read
`ctl status`. A node started well before its peers is degraded until one comes up, and that is normal. It loads the responders once a peer is there (D164).

**The load is a rule, not a number (D165).** Start at 20 users per client PC. Use the highest that holds one term for 60 s from load start, on these PCs, with `ctl status`'s `term` column as the check. A
load above that shows up as a term that keeps rising, and every figure from it is about elections.

The `ctl status` table is 109 characters wide. `watch` cuts at the terminal width, so widen the terminal or `members`
disappears.

Each client machine runs 10 real `loadgen` programs that share its users. Stop early with Ctrl-C, or from anywhere with
`bash scripts/start-users.sh --stop --machine 1`. **Machine numbers must differ**, or client IDs collide. By hand, a single
`loadgen` takes a client ID from 1 to 9: 11 to 13 belong to frontends, 21 to 23 to zone feeds in Phase 2, and five-digit IDs to
this script. A second group of users on one machine needs its own `--batch`.

Node output: `RUST_LOG=info,tarpc=info` brings tarpc's per-call lines back for debugging. Never run an experiment that way: they
slow a node enough to cost it its leader (D134).

To see what a machine is doing in plain words, with no rebuild: `python3 scripts/watch_events.py runs/lab`, and add `--reports` for every step of every report (D221). It only reads that machine's logs, so use one terminal per machine. To read a finished run and stop, use `--from-start --once`: `--once` alone prints nothing.

**A busy leader can step aside.** `config/lab.example.toml` has it on, with a ratio of 3, 5 s in a row and a 60 s cooldown (D218). If a leader moves with no fault, `watch_events.py` on that machine prints `TOO BUSY TO LEAD` and why. Every experiment cell but `e8-step-aside` has it off, whatever `lab.toml` says, because the cells do not read `lab.toml`.

**The fault draw ships off.** `config/lab.example.toml` has `faults.enabled = true` and both interval keys at `100000000`: `ctl silence` works, and no fault election ever runs, so only you cause faults, and forgetting an
edit cannot silence a node in the middle of a scene (D165). To turn the draw on, so that the three nodes elect one of them to go silent about every 30 to 60 s, replace the two interval lines as the comments in the file show,
copy the file to all six machines, and restart the nodes:

```toml
[faults]
enabled = true
interval_min_ms = 30000
interval_max_ms = 60000
```

## 4. Check the settings before measuring

First check the disk on each node machine, because every figure depends on having no new leader during a run unless the cell asks for one (D133, D163):

```bash
mkdir -p data/raft && dd if=/dev/zero of=data/raft/.t bs=512 count=1000 oflag=dsync && rm data/raft/.t
```

`dd` prints how long 1000 synced writes took. Note the number for each node machine in the report. Then the same machines' `raft_io` lines give the real figure under load (D133). Over about 15 ms a save the log reaches its limit at 3000 users, on any leader: commits take seconds with nothing wrong, and that is the disk (D228).

```bash
scripts/run-experiments.sh experiments/hosts quick e1-300-bidding
python analysis/analyze.py runs
grep -E 'service_mean_us|no_responder_share|reports_expired|msgs_all_per_req|udp_rcvbuf_got_min_bytes|udp_drops_nodes|udp_cancels_per_req' analysis/out/summary.csv
```

`udp_rcvbuf_got_min_bytes` should read 16777216: the kernel doubles what it gives. **This is the one number on this page nobody has seen on a machine with the sysctl raised, so check it first.** If `udp_drops_nodes` is above 0, raise the number and say so in the report.

In a cell past the knee, `no_responder_share` above 0 at low latency is a report that lost the race three times, not a pool that was empty. Both count there (D220).

`reports_expired` must be 0, and `analyze.py` warns otherwise. With the 60 s default the refusal cannot fire in a run, so a
nonzero count most likely means an age stamped wrongly, which looks like a very fast run where nobody gets sent (D147).

`service_mean_us` should be near 4000. `plan_work = 1000` was calibrated on a development machine (D129), so
if the lab machines differ much, change it in every cell and say so in the report. It comes from bids, so it reads NaN in a leader-dispatch cell: read it from a bidding cell. Above about
2700, expect the 3000-user cells past the knee, in one-PC rehearsal. That is the knee, not a bug: `experiments/README.md` section 3 has the numbers and the choice to make before the series (D230).

Also confirm timers on the real network: `bid_window_ms`, `peer_heartbeat_ms` and the election timeouts are LAN
defaults (D40) that have only ever run on one machine.

**Does the leader keep its role?** One `e1-300-bidding` must show no new terms: look at `elections` in that first cell, which has the step-aside off. The proposal queue
(D133) is what stops disk saves from costing a leader its role. It has passed its tests and a one-PC rehearsal, and has not been measured on these machines.

One leader-dispatch run at the demo's load gives the number beside `leader_plan_share`. The baseline's share is 1 by construction, so it is the latency that matters there (D166).

## 5. Run the experiments

```bash
scripts/run-experiments.sh experiments/hosts quick      # 15 cells, about 70 minutes
scripts/run-experiments.sh experiments/hosts full       # 34 cells, about 8 hours
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
`run.json` it finds, so a bad run must not stay. A run's `run.json` is written last, under the name `run.json.partial` until the logs are in. A folder with only `run.json.partial`
is a run that did not finish, and the analysis does not read it (D223). A run whose fault could not be injected after 3 tries goes to
`runs/dropped/<cell>-run-<n>/`, with the reason in `dropped.txt`. The batch goes on, and at the end it lists those runs and exits
non-zero. Run those cells again, by name (D139). If a cell keeps landing there, it is above what the machines can hold.

`PROGRAMS_PER_PC` changes the 10 programs per client machine, with no rebuild. Use one value for all final data; `run.json` records
it (D181). A rerun moves the old run folder to `runs/old/` (D186).

**If the machine running the runner dies**, its nodes keep running and the next run refuses, because it will not stop what it did not
start (D189). Clear them on each machine it names: `kill` the PIDs it lists. For a client machine's users, use
`bash scripts/start-users.sh --stop --machine M`.

Between runs the runner wipes `storage.dir` and `metrics.dir` on every machine, so no state carries over.
If you run by hand instead, wipe them yourself:

```bash
rm -rf data/raft runs/lab
```

**Never wipe `storage.dir` during a recovery test:** that is the state a powered-off node comes back from
(D118).

**First run on the lab.** The runner has only run through a stand-in `ssh`. On the first run, check by hand, with one short cell
(`WARMUP=5 MEASURED=20 REPS=1 scripts/run-experiments.sh experiments/hosts quick e1-30-bidding`):

1. `ssh` returns as soon as each node has started. The runner reaches `wait_for_leader` within a few seconds.
2. `pgrep -c -x loadgen` on a client machine prints 10 during the run.
3. Nothing is left afterwards: `pgrep -a -x loadgen` and `pgrep -a -x node` print nothing, and `runs-config/` holds only `run.toml`.
4. Every node's log in the run folder has a `stopped` event, and the folder has 30 `client-*.jsonl` files.
5. Kill a client machine's `start-users.sh` with `kill -9` during a second short run. The runner should say it stopped the series
   (`a client machine ended with status 137`). Its programs run on to the end of their duration, and the runner outlasts them, so afterwards
   nothing is left to refuse: on one PC `pgrep` printed nothing and the next start went ahead. A start refuses only while such programs still run, as when the runner itself died (D189, D230).

## 6. Collect and analyse

The runner copies logs to `runs/<experiment>/<cell>/run-<n>/` on the machine it runs on, next to the config,
the commit and the final `ctl status`. Keep them until the report is done. They never go into git.

```bash
python3 -m venv .venv && . .venv/bin/activate
pip install -r analysis/requirements.txt      # Python 3.10, as on Ubuntu 22.04, or newer (D230)
python analysis/analyze.py runs      # analysis/out/summary.csv and plots
```

`digest_mismatches` must be 0: any other value is a real bug, so keep the logs. `plans_refused` above 0 means a worker pool shed
work in that cell, and `elections` well above 0 means the cell is above what the machines can hold, except where the cell causes them: `ft8-election-rate` read about 5 on one PC, and `degraded-300` about 55 with `election_duration_mean_ms` at about 22 s, as a lone node keeps calling votes it cannot win. A large `client_gap_max_ms` with `gave_up` above 0 means clients stopped waiting,
so it is a lower limit on the outage and not a time to recover (D224). For how long the cloud had no leader, read `failover_gap_ms_max`, on one clock (D233). Never `election_duration_mean_ms`, which is the vote alone, and `failover_to_leader_ms_mean` only beside `node_clock_skew_ms`, as it compares two PCs' clocks. For lost copies read `udp_copies_lost_share`, never `udp_request_loss_share` (D229).

## 7. If something goes wrong

| Symptom | Likely cause |
|---------|--------------|
| `ctl status` says unreachable | the node is down, or `8001/tcp` is closed |
| `ctl status` says `udp no answer` with a healthy row above it | `8001/udp` is closed on that node. Reports to it vanish, and the run still looks fine with two nodes answering |
| A node's stderr says `UDP receive buffer capped by the kernel`, or `analyze.py` warns about a capped UDP buffer | `net.core.rmem_max` was not raised on that machine: `sudo sysctl -w net.core.rmem_max=8388608` |
| Requests retry after about 2 s in bursts, and `udp_drops_nodes` is above 0 | copies lost to a full buffer: raise `net.core.rmem_max` and `RECEIVE_BUFFER_BYTES`, or spread the users' start |
| No leader, and nodes log connect failures | port 7001 is closed between the nodes, or an address is wrong |
| A node logs `Address already in use` | its old process is still running |
| A node refuses to start: unknown key | the config is from another commit; rebuild and copy it again |
| Digests stay different after everything is serving | a real bug: keep the logs, it is what the digest exists to catch. Under load the column can differ from one refresh to the next; compare `state_checkpoint` at the same `log_index` instead (D138) |
| `fault_election` lines with `victim` null, over and over | a round could not finish: one node is off, silenced, catching up or degraded, or has `--faults off`. That is the rule working (D158, D159) |
| `degraded` is true with both peers running | the peers miss heartbeats for 2 s: an overloaded machine or a bad network (D121) |
| A returning node stays `serving = false` | it hears no leader; after 3 s it may stand itself (D125), but it needs one peer for a majority. It asks the leader again every second (D135), so it is not waiting on a lost index. Look for `rejoin: still applying` in its log |
| The runner says `programs from an earlier run are still running` | a `loadgen`, `node`, `frontend` or `zonefeed` from this build is still up on that machine. It stops nothing itself: `kill` the PIDs it lists, or `bash scripts/start-users.sh --stop --machine M`; for a web page's batch, stop the page (D139, D189) |
| The runner says `cannot check <host> for leftover programs` | exit 255 means ssh to that machine failed; exit 125 means the repo folder is not there (D222) |
| The runner seems to hang on starting a node | it should not now (D222), so look at what else holds the ssh session's output open on that machine |
| `warning: still running, so its log copy may be cut short` | a node took over 10 s to stop. `STOP_WAIT_S` allows longer. If the next run refuses, stop it by hand |
| `warning: cannot copy the logs from <machine>` | that machine cannot be reached. The other logs are copied, and its own stay on it until the next run wipes them |
| `experiments/hosts: two client machines have the same number`, or `a client machine's number must be 1 to 9` | each client machine needs its own number from 1 to 9 in the hosts file |
| `start-users: a killed start left programs running` | the script was killed and its programs ran on. `--stop --machine M --batch B` ends them, and they still flush (D190) |
| The runner says `run dropped and batch stopped` | read the reason on that line. Earlier runs are kept. Re-run the cells that are missing |
| The runner ends with `these never got their fault` | the node missed `ctl` for 6 s or more, usually overload. Logs are under `runs/dropped/`. Run those cells again by name |
| `start-users: machine M batch B is already running` | it is, or another start got there first: `--stop`, or another `--batch` |
| The runner stops the series with a status of 2 or more | a client machine refused to start, lost over a tenth of its programs, or cannot be reached (255): read `users-*.out` in the run folder |
| `analyze.py` prints `left out ...` | the line says why: programs missing, failed or stopped early, users that do not add up, a node log not copied, a damaged log, or a fault that never fired. Rerun that run |
| `analyze.py` prints `dropped a half-written last line` | a program or node was killed in mid-write. Harmless by itself; the run is left out only if another check catches it |
| New leaders keep appearing with no fault, and no `leader_stepped_aside` | the planner threads are using every core (D48: at most cores minus 1), or a binary without the proposal queue (D133). Check `max_leader_gap_us` in `raft_io` |
| One new term with no fault, and `leader_stepped_aside` in a node's log | a busy leader stepped aside on purpose (D218). Expected, at most once per cooldown. The line says why: its load, its bid, how long one log write took and its peers' bids. To rule it out, set `leader_overload_ratio = 0` in `[policy]` on all six |
| `leader_step_aside_ended` says `timed_out` | nobody took over within 3 s, so that leader carried on and waits a full cooldown. Check that the followers are serving and can reach each other (D218) |
| `target/release/ctl: No such file or directory`, then `no leader after 30 s` and `run dropped and batch stopped` | `CARGO_TARGET_DIR` is set, so the build is somewhere else. Nothing is wrong with the leader. `unset CARGO_TARGET_DIR` and build, or set `BIN=$CARGO_TARGET_DIR/release` for the runner (D230) |
| `ctl` panics with `failed printing to stdout` | it was piped into something that stopped reading, such as `head`. Harmless, and nothing to do with the cluster. Use `sed -n 1,5p` (D230) |
| `ctl` errors, or reports vanish while every row looks healthy | a stale binary on one machine: an old client and a new node fail quietly (D155, D175). Rebuild all six from one commit |
| `members` shows `-` | that node is silenced; its list is frozen and not worth reading |
| `members` differs between nodes | normal for a moment: each list is one node's opinion. Lasting differences with everyone serving point at a network problem between two nodes |
| Latency is very low, `no responder` is near 100% and `expired` is high | an age stamped wrongly, or `--ttl-ms` passed by mistake: check the loadgen command and the commit (D143) |
| `ctl incidents` shows different `log_index` values for one ID, for longer than a moment | a real bug, the same as digests that stay different. A brief `assigned` against `resolved` is only catch-up |
| The page says "Local program not answering" | the frontend on this PC is stopped or stuck; the nodes may be fine, so check with `ctl status` |
| All three rows on the page say unreachable | the wrong `--config`, or the nodes really are down; `ctl status` tells which |
| The frontend cannot start: `Address already in use` | another frontend is on that port; pick another `--listen` |
| A program row on the page says failed | read the frontend's terminal: its programs' stderr goes there |
