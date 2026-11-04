# RescueMesh

RescueMesh coordinates emergency response on a campus. People report incidents (a blocked road, a power outage, a lab
incident, flooding, a network outage, a request for medical or technical help) from client devices. A small cloud of three
peer nodes decides which responder goes where and makes sure every report gets one answer, even when a node fails.

## What it does

- **Takes incident reports.** The client middleware sends each report to all three nodes at once: that is the group send,
  and the application makes one call and does not know it happens (D210). It never needs to know which node is in charge, and it
  retries with the same request ID if no answer arrives.
- **Plans the response.** A planner picks a responder for each incident and works out its priority, from severity, age,
  distance, availability and capability.
- **Refuses what is too old.** A report says how long it stays valid, 60 s by default. A node that gets it later answers
  expired and sends nobody (D143).
- **Agrees on one answer.** Nodes keep a replicated log with Raft. A result counts only once it is
  committed, and the first committed result for a request wins. Retries and duplicate work can
  never assign two different responders to one report.
- **Spreads the work.** Each request goes to one node, either the least-loaded node (load-based
  bidding) or the Raft leader (baseline mode), set in the config.
- **Survives failures.** A node can disappear for up to 20 seconds. The others elect a new leader if
  needed and keep serving. When the node returns, it catches up from the leader's log before it
  serves again.
- **Keeps the nodes as peers.** The three nodes generate requests and answer them at the same time. Client and server are only
  the roles of one call (D212). The middleware and the application are kept apart on both sides (D211).
- **Measures itself.** Every node and client writes timestamped events (JSON Lines) for latency,
  throughput, elections, failover and recovery analysis.

```
 clients (simulated users)            cloud: three peer nodes
 ┌──────────────────────┐   request  ┌──────────────────────────────────────┐
 │ group send to all    │ ─────────▶ │ node 1   node 2   node 3             │
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
  winner, a proposal queue with grouped disk saves so the leader stays up under load (D133), a busy leader that steps aside so a normal vote can move the leadership (D218), the planner with incident priority,
  TTL and status, fault injection with an election among the nodes for who fails, rejoin, degraded mode for one server left,
  group communication with a who's-alive list, the client middleware, reports over UDP through tarpc with `ctl` on tarpc over TCP (D167, D168),
  any serving node loading the responder list (D164), the load generator with 10 real programs per client
  machine, a web page for each client machine, the middleware and application kept apart on both sides (D156), the experiment runner and the analysis.
- **Phase 2, documented only:** discovery, direct P2P, zone publish-subscribe (D209).

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

Leave it unset on the lab PCs. The experiment runner looks in `target/release`, or in `BIN` if you set it ([experiments/README.md](experiments/README.md) section 3).

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

Then inspect the cluster and generate load. `--client-id` is required: use 1 to 9 by hand.

```bash
cargo run --release --bin ctl -- status
cargo run --release --bin loadgen -- --client-id 1 --users 20 --duration-s 30      # closed loop
cargo run --release --bin loadgen -- --client-id 2 --rate 500 --duration-s 30      # open loop; above what the cluster can answer, new calls wait once 20,000 are open on one stub (D173)
cargo run --release --bin ctl -- incidents --client 1 --limit 5                    # a client's newest records
cargo run --release --bin ctl -- incidents --id 1-1762180000000123                 # one record, on every node
cargo run --release --bin loadgen -- --client-id 3 --users 1 --duration-s 1 --ttl-ms 0   # every report refused
bash scripts/start-users.sh --config config/local.toml --machine 1 --users 100 --duration-s 30   # 10 real programs share the users
bash scripts/start-users.sh --stop --machine 1
```

`ctl status` shows each node's role, term, leader, log position, state digest, whether it is serving, silenced or degraded,
and who it lists as group members. Below its table, one line per node says whether that node's UDP port answers: `node 1: udp ok`. Nodes that have applied the same log show the same digest. `loadgen` prints throughput
and latency percentiles, with counts of retries, requests given up on, plans with no responder, provisional answers and
expired reports. `--pattern steady|random|all-at-once` sets how users wait (D182). `ctl incidents` shows each record's status,
responder, priority, owner, category, times and log index, on every node.

Requests are planned by a load-based bidding election among the nodes. The Raft leader orders the log and decides nothing else (D114, D163). The bid moves work away from a busy node, and a leader that
stays overloaded also steps aside, so that a normal vote picks another (D218). `policy.assignment = "leader-dispatch"` is the baseline for comparison.

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

With fault injection on, the three nodes also elect one of them to fail: about every 30 to 60 s each draws a random number and sends it to the
others, and the highest goes silent for 1 to 20 s. A node missing any of the numbers does nothing, so a node that is out stops the next election, and
no leader is involved (D158). The `[faults]` section sets the ranges and an optional `seed`, which repeats each node's numbers per round but not the
timing. `config/lab.example.toml` ships with both interval keys set so that no timer fires, for the demo, and its comments give the values to turn elections on (D165). The code's default stays 30 to 60 s.

With both peers gone, the last node keeps answering: after 2 s it enters degraded mode and gives labelled,
uncommitted `Provisional` answers, then commits them through Raft when a peer returns. First commit wins, so a
provisional answer can turn out to differ, and clients are not told (D120).

## Run on several machines

Copy `config/lab.example.toml` to `config/lab.toml`, fill in each machine's address, and make sure
the ports are open between machines: peers on TCP, client ports on both UDP (reports) and TCP (`ctl`). A big UDP buffer needs
`sudo sysctl -w net.core.rmem_max=8388608` on all six. Use the same file everywhere, and rebuild on every machine after a `git pull`:

```bash
# on server machine 3 first, then about a second later on 2 and 1, each with its own ID
cargo run --release --bin node -- --config config/lab.toml --id 3
# on each client machine, with its own machine number
bash scripts/start-users.sh --config config/lab.toml --machine 1 --users 20
# the web page on each client machine, then open that address in its browser
cargo run --release --bin frontend -- --config config/lab.toml --client-id 11 --listen 127.0.0.1:9001
```

The full steps are in [docs/runbook.md](docs/runbook.md), and the demo is in [docs/demo.md](docs/demo.md). The example config is set up for the demo; the experiment runner never reads it.

## Run the experiments

**[experiments/README.md](experiments/README.md) is the guide for the lab:** how long to run, which experiment answers each requirement, with SSH or by hand, and how to stress it until it falls.

```bash
cp experiments/hosts.example experiments/hosts     # set the six machines' addresses
scripts/lab-build.sh experiments/hosts             # the same commit everywhere
scripts/run-experiments.sh experiments/hosts quick # 15 cells, about 70 minutes; `full` is 34, about 8 hours
python analysis/analyze.py runs                    # analysis/out/: summary.csv and plots
```

See [experiments/README.md](experiments/README.md) and [analysis/README.md](analysis/README.md).

## Configuration

Every process reads one TOML file. Unknown keys are rejected, so a typo fails at startup. Sections:

| Section    | What it sets                                                                |
|------------|-----------------------------------------------------------------------------|
| `cluster`  | Node IDs, peer addresses (Raft, bids, heartbeats), client addresses          |
| `timers`   | Raft heartbeat and election timeouts, bid window, client retry, timeouts     |
| `policy`   | `assignment` (`"bidding"` or `"leader-dispatch"`), `score`, `cpu_weight`, `just_won`, `initial_service_us`, and when a busy leader steps aside: `leader_overload_ratio` (0 turns it off), `leader_overload_for_ms`, `leader_overload_cooldown_ms` (D218) |
| `workers`  | Planner threads, queue capacity, planner cost knob                           |
| `campus`   | Seed, zones and responders of the synthetic campus, time on scene, how long a report stays valid (`incident_ttl_ms`) |
| `metrics`  | Directory for event logs; leave it out to disable                            |
| `storage`  | Directory for each node's Raft log and vote. Leave it out to keep them in memory, which is for tests only: a node then cannot survive losing power (D118) |
| `faults`   | Fault injection on or off, the fault election's ranges and seed              |

Defaults and validation live in [src/config.rs](src/config.rs).

## Logs and measurements

With `metrics.dir` set, each process appends to `<dir>/node-<id>.jsonl` or
`<dir>/client-<id>.jsonl`, or `<dir>/frontend-<id>.jsonl` for the web page's own reports, which analysis
ignores, one JSON object per line:

```json
{"ts_us":1792000000123456,"src":"node-1","ev":"commit","kind":"commit_result","outcome":"committed","latency_us":812,"forwarded":false,"log_index":42,"request":{"client":7,"seq":3}}
```

Human-readable logs go to stderr. The default filter keeps our own `info` lines and everyone's warnings, and turns off tarpc's
per-call lines, which are costly at load (D134). `RUST_LOG` overrides it, for example `RUST_LOG=debug,openraft=info`.

To watch a machine live in plain words, with no rebuild:

```bash
python3 scripts/watch_events.py runs/lab            # leader changes, draws, silences, who's alive, a summary each second
python3 scripts/watch_events.py runs/lab --reports  # also every step of every report
```

It only reads the logs (D221), and each machine has only its own, so run it where you want to look.

## Repository layout

```
src/
  proto.rs        every message, ID and log entry (the wire contract)
  config.rs       config file format and validation
  state.rs        replicated state machine
  raft/           openraft setup, storage, network adapter
  node/           node wiring, transport and fault gate, UDP adapter, heartbeats, group communication, workers, rejoin, faults, degraded mode
  bidding.rs      load score and bid selection
  planner.rs      incident planner
  campus.rs       synthetic campus
  client.rs       client middleware
  metrics.rs      event log
  bin/            node, loadgen, ctl, frontend (the web page; its HTML is beside it)
tests/            multi-node scenarios run in one process
config/           cluster configs
scripts/          run a local cluster; start a client machine's users; build on and run experiments across the lab machines
experiments/      one config per experiment cell, hosts file example
analysis/         Python: numbers and plots from event logs
docs/             architecture (with diagrams), protocol, design log, runbook, demo script
```

## Design documents

- [docs/architecture.md](docs/architecture.md): deployment, components, every flow as a sequence
  diagram, state and lifecycle diagrams, each marked built, template, planned or open.
- [docs/design-log.md](docs/design-log.md): every design decision with its options, reasoning and
  consequences. None is open; D216 lists Noor's decisions and what is built.
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
replication. [tarpc](https://github.com/google/tarpc) 0.37 provides remote calls, between nodes over TCP and from clients over UDP through a small adapter of ours (D119, D167).
Everything else that does distributed work (state machine, storage, network adapter, datagram adapter, bidding, group communication, recovery,
degraded mode, planner, client middleware, fault injection) is our own code (D179, D215). openraft is under MIT or Apache-2.0 and tarpc under MIT.
AI coding agents helped write the code and these docs. Each design decision says who decided it, in the design log. [AGENTS.md](AGENTS.md) records which
file belongs to which part.
