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

- **Toolchain.** `rust-toolchain.toml` picks the compiler on the first build. Do not install another one
  over it: an apt Rust earlier on `PATH` cannot build this repo.
- **Clock.** `chronyc tracking` should show the clock in sync. Each machine's timestamps are only roughly
  aligned with the others', so durations are always measured on one machine.
- **Disk.** Keep the repo on a local disk. Commits sync to `storage.dir`, so a slow or shared disk shows up as
  commit latency. Never use a shared filesystem: each machine keeps its own logs.

On the three node machines, open the ports:

```bash
sudo ufw allow 7001/tcp      # peers: reachable from the other two nodes
sudo ufw allow 8001/tcp      # clients and ctl: reachable from the client machines
```

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

## 3. Start by hand

```bash
# on node machine 1, 2 and 3, each with its own ID
target/release/node --config config/lab.toml --id 1

# from any machine: all three should show one leader and equal digests
target/release/ctl --config config/lab.toml status

# on client machine 1, 2 and 3, each with its own client ID
target/release/loadgen --config config/lab.toml --client-id 1 --users 50 --think-ms 1000 --duration-s 60
```

**Client IDs must be unique across machines**, or request IDs collide.

`faults.enabled = true` in the lab config means the leader also silences a random node every 30 to 60 s. For
anything you want to control yourself, such as the demo, set this in `config/lab.toml` first:

```toml
[faults]
enabled = true                # keeps ctl silence working
interval_min_ms = 100000000   # the schedule never fires
interval_max_ms = 100000000
```

## 4. Check the settings before measuring

```bash
scripts/run-experiments.sh experiments/hosts quick e1-300-bidding
python analysis/analyze.py runs
grep -E 'service_mean_us|no_responder_share|msgs_all_per_req' analysis/out/summary.csv
```

`service_mean_us` should be near 4000. `plan_work = 1000` was calibrated on a development machine (D129), so
if the lab machines differ much, change it in every cell and say so in the report.

Also confirm timers on the real network: `bid_window_ms`, `peer_heartbeat_ms` and the election timeouts are LAN
defaults (D40) that have only ever run on loopback.

## 5. Run the experiments

```bash
scripts/run-experiments.sh experiments/hosts quick      # 13 cells, about 1 hour
scripts/run-experiments.sh experiments/hosts full       # 31 cells, about 8 hours
```

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

## 7. If something goes wrong

| Symptom | Likely cause |
|---------|--------------|
| `ctl status` says unreachable | the node is down, or port 8001 is closed |
| No leader, and nodes log connect failures | port 7001 is closed between the nodes, or an address is wrong |
| A node logs `Address already in use` | its old process is still running |
| A node refuses to start: unknown key | the config is from another commit; rebuild and copy it again |
| Digests stay different after everything is serving | a real bug: keep the logs, it is what the digest exists to catch |
| `degraded` is true with both peers running | the peers miss heartbeats for 2 s: an overloaded machine or a bad network (D121) |
| A returning node stays `serving = false` | it hears no leader; after 3 s it may stand itself (D125), but it needs one peer for a majority |
