# RescueMesh

RescueMesh coordinates emergency response on a campus. People report incidents (a fire, a medical
emergency, a security problem) from client devices. A small cloud of three server nodes decides which
responder goes where and makes sure every report gets one answer, even when a server fails.

## What it does

- **Takes incident reports.** A client sends each report to all three nodes at once. It never needs
  to know which node is in charge, and it retries with the same request ID if no answer arrives.
- **Plans the response.** A planner picks a responder for each incident from severity, age,
  distance, availability and capability.
- **Agrees on one answer.** Nodes keep a replicated log with Raft. A result counts only once it is
  committed, and the first committed result for a request wins. Retries and duplicate work can
  never assign two different responders to one report.
- **Spreads the work.** Each request goes to one node, either the least-loaded node (load-based
  bidding) or the Raft leader (baseline mode), set in the config.
- **Survives failures.** A node can disappear for up to 20 seconds. The others elect a new leader if
  needed and keep serving. When the node returns, it catches up from the leader's log before it
  serves again.
- **Measures itself.** Every node and client writes timestamped events (JSON Lines) for latency,
  throughput, elections, failover and recovery analysis.

```
 clients (simulated users)            cloud: three peer nodes
 ┌──────────────────────┐   request  ┌──────────────────────────────────────┐
 │ multicast to all     │ ─────────▶ │ node 1   node 2   node 3             │
 │ retry with same ID   │            │ transport + fault gate               │
 │ first reply wins     │ ◀───────── │ assignment (bidding / leader)        │
 └──────────────────────┘    reply   │ worker pool + planner                │
                                     │ Raft (openraft) + state machine      │
                                     └──────────────────────────────────────┘
```

## Status

Phase 1 is built and passes its tests on one machine. **It has not yet run on the six lab machines**, and the
experiments have not been run there.

- **Built:** Raft replication with the log on disk, load-based bidding (the default), takeover from a silent
  winner, the planner, fault injection, rejoin, degraded mode for one server left, the client middleware, the load
  generator, the experiment runner and the analysis.
- **Phase 2, documented only:** discovery, direct P2P, zone publish-subscribe.

## Requirements

- Rust, installed with [rustup](https://rustup.rs). The repo pins its compiler in
  `rust-toolchain.toml`, and rustup fetches that version on first build.
- Linux (including WSL 2) or macOS. Windows works too, but scripts in `scripts/` need bash.
- On Linux you also need a C linker: `sudo apt install build-essential` on Ubuntu or Debian.

## Build and test

```bash
cargo build --release
cargo test
```

On WSL, the build is much faster if the target directory lives on the Linux filesystem, not the
Windows drive:

```bash
export CARGO_TARGET_DIR="$HOME/.cache/rescuemesh-target"
```

## Run a cluster on one machine

`config/local.toml` describes three nodes on `127.0.0.1`. The quickest way to start them on Linux,
WSL or macOS:

```bash
scripts/run-local.sh          # builds, starts nodes 1-3 in the background, prints status
scripts/stop-local.sh         # stops them
```

Or start each node by hand in its own terminal:

```bash
cargo run --release --bin node -- --config config/local.toml --id 1
cargo run --release --bin node -- --config config/local.toml --id 2
cargo run --release --bin node -- --config config/local.toml --id 3
```

Then inspect the cluster and generate load. `--client-id` is required, and must be unique per load generator:

```bash
cargo run --release --bin ctl -- status
cargo run --release --bin loadgen -- --client-id 1 --users 20 --duration-s 30      # closed loop
cargo run --release --bin loadgen -- --client-id 2 --rate 500 --duration-s 30      # open loop
```

`ctl status` shows each node's role, term, leader, log position, state digest, and whether it is serving,
silenced or degraded. Nodes that have applied the same log show the same digest. `loadgen` prints throughput
and latency percentiles, with counts of retries, requests given up on, plans with no responder and provisional
answers.

Requests are planned by a load-based bidding election among the nodes. The Raft leader orders the log and does
not choose who plans (D114). `policy.assignment = "leader-dispatch"` is the baseline for comparison.

## Inject faults

```bash
cargo run --release --bin ctl -- silence --node 1 --ms 10000   # node 1 drops all traffic for 10 s
cargo run --release --bin ctl -- skew --node 2 --work 5000     # node 2 does extra work per plan
```

A silenced node keeps its state but sends and receives nothing, as if its network cable were
pulled. `ctl` itself still gets through, so you can watch the node recover.

Fault injection can be switched off for no-fault baseline runs: `enabled = false` under
`[faults]` in the config, or `--faults off` when starting a node (D77). `ctl silence` is then
refused.

With fault injection on, the Raft leader also injects faults by itself: every 30 to 60 s it
silences a random node (possibly itself) for 1 to 20 s, one fault at a time. The `[faults]`
section sets the ranges and an optional `seed` to repeat a run (D78).

With both peers gone, the last node keeps answering: after 2 s it enters degraded mode and gives labelled,
uncommitted `Provisional` answers, then commits them through Raft when a peer returns. First commit wins, so a
provisional answer can turn out to differ, and clients are not told (D120).

## Run on several machines

Copy `config/lab.example.toml` to `config/lab.toml`, fill in each machine's address, and make sure
the peer and client ports are open between machines. Use the same file everywhere:

```bash
# on each server machine, with its own ID
cargo run --release --bin node -- --config config/lab.toml --id 1
# on each client machine, with its own client ID
cargo run --release --bin loadgen -- --config config/lab.toml --client-id 1 --users 50
```

The full steps are in [docs/runbook.md](docs/runbook.md), and the demo is in [docs/demo.md](docs/demo.md).

## Run the experiments

```bash
cp experiments/hosts.example experiments/hosts     # set the six machines' addresses
scripts/lab-build.sh experiments/hosts             # the same commit everywhere
scripts/run-experiments.sh experiments/hosts quick # 15 cells, about 70 minutes; `full` is 33, about 8 hours
python analysis/analyze.py runs                    # analysis/out/: summary.csv and plots
```

See [experiments/README.md](experiments/README.md) and [analysis/README.md](analysis/README.md).

## Configuration

Every process reads one TOML file. Unknown keys are rejected, so a typo fails at startup. Sections:

| Section    | What it sets                                                                |
|------------|-----------------------------------------------------------------------------|
| `cluster`  | Node IDs, peer addresses (Raft, bids, heartbeats), client addresses          |
| `timers`   | Raft heartbeat and election timeouts, bid window, client retry, timeouts     |
| `policy`   | `assignment` (`"bidding"` or `"leader-dispatch"`), `score`, `cpu_weight`, `just_won`, `initial_service_us` |
| `workers`  | Planner threads, queue capacity, planner cost knob                           |
| `campus`   | Seed, zones and responders of the synthetic campus                           |
| `metrics`  | Directory for event logs; leave it out to disable                            |
| `storage`  | Directory for each node's Raft log and vote. Leave it out to keep them in memory, which is for tests only: a node then cannot survive losing power (D118) |
| `faults`   | Fault injection on or off, the schedule's ranges and seed                    |

Defaults and validation live in [src/config.rs](src/config.rs).

## Logs and measurements

With `metrics.dir` set, each process appends to `<dir>/node-<id>.jsonl` or
`<dir>/client-<id>.jsonl`, one JSON object per line:

```json
{"ts_us":1792000000123456,"src":"node-1","ev":"commit","kind":"commit_result","outcome":"committed","latency_us":812,"forwarded":false,"log_index":42,"request":{"client":7,"seq":3}}
```

Human-readable logs go to stderr. Set the level with `RUST_LOG`, for example
`RUST_LOG=debug,openraft=info`.

## Repository layout

```
src/
  proto.rs        every message, ID and log entry (the wire contract)
  config.rs       config file format and validation
  state.rs        replicated state machine
  raft/           openraft setup, storage, network adapter
  node/           node wiring, transport and fault gate, heartbeats, workers, rejoin, faults, degraded mode
  bidding.rs      load score and bid selection
  planner.rs      incident planner
  campus.rs       synthetic campus
  client.rs       client middleware
  metrics.rs      event log
  bin/            node, loadgen, ctl
tests/            multi-node scenarios run in one process
config/           cluster configs
scripts/          run a local cluster; build on and run experiments across the lab machines
experiments/      one config per experiment cell, hosts file example
analysis/         Python: numbers and plots from event logs
docs/             architecture (with diagrams), protocol, design log, runbook, demo script
```

## Design documents

- [docs/architecture.md](docs/architecture.md): deployment, components, every flow as a sequence
  diagram, state and lifecycle diagrams, each marked built, template, planned or open.
- [docs/design-log.md](docs/design-log.md): every design decision with its options, reasoning and
  consequences, including the open ones that still need the team.
- [docs/protocol.md](docs/protocol.md): messages, encodings and timers as they are in the code.
- [docs/runbook.md](docs/runbook.md): running on the six lab machines.
- [docs/demo.md](docs/demo.md): the Phase 1 demo, scenario by scenario.

## Contributing

Read [AGENTS.md](AGENTS.md) first. Every change goes through a pull request. When you work with an
AI coding agent, every prompt that may change a file must say **"grill with docs"** (or run
`/grill-with-docs` in Claude Code): the agent asks you about each design decision with its
recommendation, and records the answers in the design log before it writes code.

## Libraries

[openraft](https://github.com/databendlabs/openraft) 0.9.25 provides Raft leader election and log
replication. [tarpc](https://github.com/google/tarpc) 0.37 provides remote calls on the peer port (D119).
Everything else that does distributed work (state machine, storage, network adapter, bidding, recovery, degraded
mode, planner, client middleware, fault injection) is our own code. [AGENTS.md](AGENTS.md) records which
file belongs to which part.
