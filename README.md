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

Early development.

- **Working:** Raft replication. The three nodes elect a leader, replicate every command and end
  up with identical state; `ctl status` shows matching digests. Results commit before any reply,
  and the first commit per request wins.
- **In progress:** bidding, the planner, fault injection, rejoin gating and the load generator.
  Their files are templates; open work is marked `TODO(<part>)` in the code.

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

Then inspect the cluster and generate load (the load generator is still in progress):

```bash
cargo run --release --bin ctl -- status
cargo run --release --bin loadgen -- --users 20 --duration-s 30
```

`ctl status` shows each node's role, term, leader, log position and state digest. Nodes that have
applied the same log show the same digest.

## Inject faults

These commands are wired up, but the node side is still in progress:

```bash
cargo run --release --bin ctl -- silence --node 1 --ms 10000   # node 1 drops all traffic for 10 s
cargo run --release --bin ctl -- skew --node 2 --work 200000   # node 2 does extra work per plan
```

A silenced node keeps its state but sends and receives nothing, as if its network cable were
pulled. `ctl` itself still gets through, so you can watch the node recover.

## Run on several machines

Copy `config/lab.example.toml` to `config/lab.toml`, fill in each machine's address, and make sure
the peer and client ports are open between machines. Use the same file everywhere:

```bash
# on each server machine, with its own ID
cargo run --release --bin node -- --config config/lab.toml --id 1
# on each client machine, with its own client ID
cargo run --release --bin loadgen -- --config config/lab.toml --client-id 1 --users 50
```

## Configuration

Every process reads one TOML file. Unknown keys are rejected, so a typo fails at startup. Sections:

| Section    | What it sets                                                                |
|------------|-----------------------------------------------------------------------------|
| `cluster`  | Node IDs, peer addresses (Raft, bids, heartbeats), client addresses          |
| `timers`   | Raft heartbeat and election timeouts, bid window, client retry, timeouts     |
| `policy`   | `assignment = "bidding"` or `"leader-dispatch"`                              |
| `workers`  | Planner threads, queue capacity, planner cost knob                           |
| `campus`   | Seed, zones and responders of the synthetic campus                           |
| `metrics`  | Directory for event logs; leave it out to disable                            |

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
  node/           node wiring, transport and fault gate, heartbeats, workers, rejoin, faults
  bidding.rs      load score and bid selection
  planner.rs      incident planner
  campus.rs       synthetic campus
  client.rs       client middleware
  metrics.rs      event log
  bin/            node, loadgen, ctl
tests/            multi-node scenarios run in one process
config/           cluster configs
scripts/          run and stop a local cluster
experiments/      experiment run configs
analysis/         analysis of event logs
docs/             architecture (with diagrams), protocol, design log
```

## Design documents

- [docs/architecture.md](docs/architecture.md): deployment, components, every flow as a sequence
  diagram, state and lifecycle diagrams, each marked built, template, planned or open.
- [docs/design-log.md](docs/design-log.md): every design decision with its options, reasoning and
  consequences, including the open ones that still need the team.
- [docs/protocol.md](docs/protocol.md): messages, encodings and timers as they are in the code.

## Contributing

Read [AGENTS.md](AGENTS.md) first. Every change goes through a pull request. When you work with an
AI coding agent, every prompt that may change a file must say **"grill with docs"** (or run
`/grill-with-docs` in Claude Code): the agent asks you about each design decision with its
recommendation, and records the answers in the design log before it writes code.

## Libraries

[openraft](https://github.com/databendlabs/openraft) 0.9.25 provides Raft leader election and log
replication. Everything else (state machine, storage, network adapter, transport, bidding,
planner, client middleware, fault injection) is our own code. [AGENTS.md](AGENTS.md) records which
file belongs to which part.
