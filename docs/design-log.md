# Design log

Every design decision in RescueMesh, why we took it, what we rejected and what it costs us. This
file is the raw material for the report's design section, so entries are long on purpose: write
down the reasoning while it is fresh, because nobody remembers it two weeks later.

Companion documents:

- [architecture.md](architecture.md): the architecture, with every diagram (deployment,
  components, sequences, state machines). When a decision here changes a diagram there, update
  both in the same PR.
- [protocol.md](protocol.md): messages, encodings and timers as they are in the code.
- [AGENTS.md](../AGENTS.md): who owns what, and the rule that every decision is asked, recommended
  and recorded here (see "Design decisions" there).

## How this log works

**Who decides.** Decisions belong to people. An AI coding agent never settles a design question on
its own: it asks, gives its recommendation with reasons, and writes the answer here once a person
has decided (AGENTS.md, "Design decisions"). A decision that touches several parts needs every
affected owner.

**What counts as a decision.** Anything a teammate, the instructor or the report could ask "why?"
about: a protocol or message, a timer value, a data structure in replicated state, a library, a
failure-handling rule, a measurement method, a default, a trade-off, and also a choice *not* to do
something. When in doubt, log it. Small entries are fine; missing entries are not.

**IDs.** `D1`–`D13` are the decisions of the Phase 1 architecture document (27 Oct 2026), kept
with their original numbers so the report can cite them. `D14` onwards are decisions taken in the
code or since. IDs are never reused: a changed decision gets a new entry, and the old one is marked
`Superseded by Dnn` or `Amended by Dnn`.

**Status values.**

| Status        | Meaning                                                                          |
|---------------|----------------------------------------------------------------------------------|
| `Implemented` | Decided and in the code on `main`.                                               |
| `Accepted`    | Decided; the code is a template or not written yet. The owner builds it.         |
| `Proposed`    | Someone recommends it; the owner or the team has not confirmed it yet.           |
| `Open`        | Not decided. The entry lists the options and a recommendation. Needs a person.   |
| `Amended`     | Still holds, but a later entry changed part of it (named in the entry).          |
| `Superseded`  | Replaced by a later entry (named in the entry).                                  |
| `Pending approval` | Decided by the team, waiting for the instructor.                            |

**Template.** Copy this for a new entry and append it at the end of its section. Keep the index
table below in sync.

```markdown
### Dnn · Short title

- **Date:** YYYY-MM-DD · **Part:** A–F or all · **Status:** Proposed · **Decided by:** names
- **Context.** What problem or question forced a choice. What constraints applied.
- **Options considered.**
  1. Option one: how it works, pros, cons.
  2. Option two: ...
- **Decision.** What we chose, precisely enough to implement (values, names, messages).
- **Why.** The reasoning, with references (papers, course notes, measurements) where we have them.
- **Consequences.** What we give up, what becomes harder, what other parts must now do.
- **Where.** Files, types, config keys, tests.
- **How we check it.** The test or experiment (E1–E6, FT-n, LB-n, P-n) that shows it works.
- **Report notes.** Anything worth a sentence or a figure in the report.
```

---

## Index

| ID  | Decision                                                        | Part | Status            |
|-----|-----------------------------------------------------------------|------|-------------------|
| D1  | Raft leader election for the cloud coordinator                  | A    | Implemented (via D14) |
| D2 | Load-based bidding election per request | B | Implemented (default, D114) |
| D3  | Leader dispatch kept as a measured baseline                     | B    | Implemented       |
| D4  | Implement Raft ourselves                                        | A    | Superseded by D14 |
| D5  | PreVote in Raft                                                 | A, D | Implemented (gate, D75) |
| D6  | Client sends every request to all three nodes over UDP          | F    | Amended by D17    |
| D7  | TCP between cloud nodes                                         | C    | Implemented       |
| D8  | Reply only after commit                                         | A    | Implemented       |
| D9  | Request ID plus a replicated table of results                   | A    | Implemented, amended by D27 |
| D10 | Optimistic check at commit instead of locks                     | A, E | Implemented, amended by D28 |
| D11 | Single-owner tasks and channels; planner on a thread pool       | all  | Amended by D47    |
| D12 | The Raft leader runs the fault schedule                         | C    | Implemented, amended by D78 |
| D13 | Faults injected in the network endpoint                         | C    | Implemented       |
| D14 | openraft 0.9.25 behind a `Consensus` trait                      | A    | Implemented       |
| D15 | Rust 2024 on a pinned toolchain, tokio runtime                  | all  | Implemented       |
| D16 | Wire format: length-delimited frames with bincode | all | Implemented, amended by D119 |
| D17 | Clients use TCP on a dedicated client port | C, F | Implemented, amended by D119 |
| D18 | Two ports per node; `ctl` shares the client port                | C    | Implemented       |
| D19 | One multiplexed connection per peer | C | Superseded by D119 |
| D20 | `TCP_NODELAY` on every connection                               | C    | Implemented       |
| D21 | The term rule does not apply to Raft RPCs                       | all  | Implemented       |
| D22 | Fault gate semantics                                            | C    | Implemented, amended by D78 |
| D23 | Control frames bypass the fault gate                            | C    | Implemented       |
| D24 | In-memory Raft storage, no snapshots | A | Superseded by D118 |
| D25 | Static membership; every node calls `initialize`                | A    | Implemented       |
| D26 | Any node proposes; followers forward to the leader              | A    | Implemented       |
| D27 | First commit wins; the outcome tells the proposer what happened | A    | Implemented       |
| D28 | A plan is stale if its responder is held or unknown             | A    | Implemented       |
| D29 | Three idempotent log commands                                   | A    | Implemented       |
| D30 | Deterministic state machine, integers only                      | A    | Implemented       |
| D31 | State digest: an FNV-1a hash chain over the applied log         | A    | Implemented       |
| D32 | Retry an unreachable peer after two heartbeats                  | A    | Implemented       |
| D33 | No leader, no executor (leader dispatch)                        | A    | Implemented       |
| D34 | Leader dispatch is the default until bidding lands | B | Superseded by D114 |
| D35 | Re-plan a stale plan at most three times                        | A, E | Implemented       |
| D36 | A node that cannot answer stays quiet | A, F | Implemented |
| D37 | Any serving node answers a retry from its own state             | A    | Implemented       |
| D38 | Responders seeded once through Raft by the leader               | A, E | Implemented       |
| D39 | Responders released by the executor, peers as backup | A, E | Implemented |
| D40 | Timer defaults and validation                                   | all  | Implemented       |
| D41 | Peer heartbeats as a failure detector for takeover              | C    | Implemented, amended by D79 |
| D42 | Takeover trigger: suspicion first, backstop max(200 ms, 3 × bid) | B, C | Implemented by D110 |
| D43 | When the bid window starts                                      | B    | Implemented |
| D44 | No `Claim` message; `just_won` in the load score                | B    | Implemented, amended by D85 |
| D45 | Load score formula and tie-break                                | B    | Implemented |
| D46 | Rejoin gate: `serving` flag, current at the leader's commit     | D    | Implemented, amended by D75 |
| D47 | Shared node context instead of single-owner tasks               | A    | Implemented       |
| D48 | Bounded worker pool that sheds load                             | E    | Implemented       |
| D49 | Planner: deterministic, integer cost, CPU knob                  | E    | Implemented       |
| D50 | Domain model: integer campus, four capabilities, severity 1–5   | all  | Implemented       |
| D51 | Request IDs and per-machine client IDs | F | Implemented, amended by D113 |
| D52 | Client retry budget | F | Implemented |
| D53 | One client process, one connection per node, shared by users | F | Implemented |
| D54 | Load generator: closed loop first, open loop as a mode | F | Implemented |
| D55 | JSON Lines event logs written off the async runtime             | all  | Implemented       |
| D56 | Durations on one clock; latency on the client                   | all  | Implemented       |
| D57 | Heartbeat logging: one summary a second, details at debug       | A    | Implemented       |
| D58 | Human logs through `tracing`, measurements through events       | all  | Implemented       |
| D59 | One strict TOML config shared by every process                  | A    | Implemented       |
| D60 | Deployment target: six Ubuntu machines on one LAN               | all  | Implemented       |
| D61 | Peer port serves before Raft starts                             | A    | Implemented       |
| D62 | `ctl` is the operator tool; the experiment runner is bash over SSH | C | Implemented (D130) |
| D63 | Experiment method: repetitions, warm-up, run length             | all  | Implemented       |
| D64 | Analysis in Python                                              | all  | Implemented       |
| D65 | Testing strategy                                                | all  | Implemented       |
| D66 | Rust coding rules                                               | all  | Implemented       |
| D67 | Team process: parts, owners, pull requests, CI                  | all  | Implemented       |
| D68 | Library boundary: openraft is the only distributed library      | all  | Implemented       |
| D69 | Phase 2 hooks                                                   | all  | Accepted, docs only |
| D70 | Agents ask, recommend and record every decision; "grill with docs" | all | Implemented    |
| D71 | Worker pool mechanism: admission counter plus a fair semaphore  | E    | Implemented       |
| D72 | Worker pool: cancelled and panicking jobs                       | E    | Implemented       |
| D73 | Worker pool load report: queued and running only; no overload event yet | E | Implemented |
| D74 | Manual faults: one at a time, 1 ms to 20 s, rejected otherwise  | C    | Implemented       |
| D75 | Rejoin gate: serving off and elections off until current        | D, A | Implemented       |
| D76 | `ReadIndex` peer message: the catch-up target from the leader   | D, A | Implemented       |
| D77 | Fault injection on/off: `faults.enabled`, `--faults` override   | C, A | Implemented       |
| D78 | Fault schedule: `FaultOrder`, timings, seeding, no overlap      | C, A | Implemented       |
| D79 | Peer heartbeat detector: heard-from, pause while silent, events | C    | Implemented       |
| D82 | CPU usage read from /proc/stat                                  | B    | Implemented |
| D83 | Part B tests: variants, order and decision rules                | B    | Accepted          |
| D84 | One bid round per request and attempt; `attempt` added to `Bid` | B, A | Implemented |
| D85 | `just_won` also counts open rounds a node is leading            | B    | Implemented |
| D86 | Planning time: running average with gain 1/8, starts at 1000 µs | B    | Implemented, amended by D108 |
| D87 | The four load score variants                                    | B    | Implemented |
| D88 | Bid rounds as pure state in `bidding.rs`; takeover helper left out | B | Implemented, amended by D103 and D107 |
| D89 | Peer reconnect: fixed cooldown after a failed connect           | C    | Implemented       |
| D90 | Reconnect cooldown = 2 × `raft_heartbeat_ms`, no new config key | C    | Implemented       |
| D91 | `TransportError::Backoff`, treated as Unreachable by openraft    | A, C | Implemented       |
| D92 | Peer outage events: `peer_unreachable`, `peer_reconnected`      | C    | Implemented       |
| D93 | Planner pick rule: exact capability, nearest, cost in metres    | E    | Amended by D101   |
| D94 | Severe incidents fall back to any capability                    | E    | Amended by D101   |
| D95 | Incident age in the planner needs a time from the node | A, E | Implemented |
| D96 | CPU knob: fixed hash rounds, no built-in base work              | E    | Implemented       |
| D97 | Campus layout: zone grid, even round-robin, seeded positions    | E    | Implemented       |
| D98 | Campus RNG: our own SplitMix64                                  | E    | Implemented       |
| D99 | Random incidents are uniform                                    | E, F | Amended by D102    |
| D100 | Planner and campus tests                                        | E    | Implemented       |
| D101 | Planner zone penalty: +200 m for another zone                   | E    | Implemented       |
| D102 | Unseeded random incidents from OS randomness                    | E, F | Implemented       |
| D103 | Each request's task runs its own round, in `node/bids.rs`        | B, A | Implemented, amended by D108 |
| D104 | Bids go out in the background; no waiting and no retry          | B    | Implemented       |
| D105 | `[policy]` keys for scores, `just_won` and the starting planning time | B, A | Implemented, amended by D108 |
| D106 | Events `bid_sent` and `bid_round_closed`                        | B    | Implemented       |
| D107 | Planning time, CPU reading and hooks for takeover               | B, C | Implemented       |
| D108 | Planning time: 1 µs minimum, and a 1 µs starting value           | B    | Implemented       |
| D109 | Multi-node bidding tests: what they cover and how               | B    | Implemented       |
| D110 | Takeover: stand by after a lost round, plan if the winner is silent | C, B | Implemented       |
| D111 | Responder service time: 200 ms × severity | E, A | Implemented |
| D112 | Client connections: ping through the gate, bounded send queues | F | Implemented |
| D113 | Required client IDs; sequence numbers start at the clock | F | Implemented |
| D115 | A different election timeout range per node | A | Implemented |
| D114 | Bidding is the default; the leader only sequences the log | B, A | Implemented |
| D116 | Raft and the leader-run fault schedule stay, despite "no coordinator" | A, C | Implemented |
| D117 | Richer faults are not built | C | Accepted |
| D118 | Raft log and vote on disk, for physical power-off | A, D | Implemented |
| D119 | Peer calls through tarpc, with typed methods | C, A | Implemented |
| D120 | Degraded mode: the last server answers without a commit | C, A, F | Implemented |
| D121 | Degraded trigger: every peer suspected for 2 s | C | Implemented |
| D122 | Leaving degraded mode: replay through Raft, first commit wins | C | Implemented |
| D123 | `ReplyBody::Provisional`, and how the client reports it | C, F | Implemented |
| D124 | Degraded holds: a local table on the service timer | C, E | Implemented |
| D125 | A returning node with no leader may stand for election after 3 s | D, C | Implemented |
| D126 | Group communication named: `PeerSet::broadcast` and `MeshClient` | C, B, F | Implemented |
| D127 | FT-6: count elections after each `rejoined` | D, A | Implemented |
| D128 | `resource_usage`: memory, CPU, network and queues once a second | all | Implemented |
| D129 | Experiment load: `plan_work`, responders and think time | E, F | Implemented, to confirm |
| D130 | Experiment cells and the runner | all | Implemented, to confirm |
| D131 | Publish-subscribe: Phase 2, for zones, docs only | all | Accepted, docs only |
| D132 | The "entries table" is the spec's demo table | all | Implemented |

**Open decisions.** None remain in Phase 1's code: D5, D17, D39, D42, D52, D63 and D95 are settled, each in its
own entry. Any entry marked `Open` in future was a conflict between the Phase 1 architecture document and the
code, or a question nobody has answered. It stays open until someone implements the related code: whoever works
on it settles the decision first. During "grill with docs" the agent finds the open entries the task touches,
asks the person to decide with the options and recommendation listed in the entry, and updates the entry
(status, decided by, decision) in the same PR (AGENTS.md, "Design decisions"). D125, D129 and D130 were decided by
the agent on the instruction not to be asked again, and are marked for the team to confirm.

---

## 1. Coordination and consensus

### D1 · Raft leader election for the cloud coordinator

- **Date:** Phase 1 architecture (Oct 2026) · **Part:** A · **Status:** Implemented through openraft
  (D14) · **Decided by:** team, from the Phase 0 survey
- **Context.** The cloud needs one coordinator that orders every decision, so that every node ends
  up with the same incidents, responders and results. The project allows a node to go silent for
  up to 20 s, and a timeout cannot tell a slow node from a dead one (FLP; Chandra and Toueg).
- **Options considered.**
  1. *Bully* (Garcia-Molina): simple, but its safety depends on timeouts. A slow-but-alive node
     can be declared dead, and two nodes can both believe they lead.
  2. *Chang-Roberts / Hirschberg-Sinclair*: ring elections; need a ring topology and give no
     replication.
  3. *ZooKeeper fast leader election*: a library and protocol of its own, heavy for three nodes.
  4. *Raft*: majority voting with terms; at most one leader per term regardless of timing; log
     replication in the same protocol.
- **Decision.** Raft. Three nodes, so a majority is two and the cluster survives one silent node.
- **Why.** Raft's safety (one leader per term, committed entries never lost) holds without trusting
  timeouts; timeouts only affect liveness. The same protocol gives us the replicated log we need
  for state, so we do not need a second mechanism for replication.
- **Consequences.** A short leaderless gap after a leader fails (one election timeout, 150–300 ms).
  More machinery than Bully.
- **Where.** `src/raft/` (via openraft), `src/state.rs`.
- **How we check it.** `tests/replication.rs::three_nodes_elect_exactly_one_leader`, the leader
  silence test, FT-2, E5 (elections, election time).
- **Report notes.** Cite the Phase 0 survey for the comparison. Point out that our bidding (D2) is
  Bully-like, but it only chooses *who computes*; Raft still decides *what is committed*, so a
  wrong bidding outcome can waste work but never corrupt state.

### D2 · Load-based bidding election per request

- **Date:** Phase 1 architecture · **Part:** B · **Status:** Implemented (off by default, D34) ·
  **Decided by:** team
- **Context.** The project asks for an election that picks which node handles each request, based
  on measured load. The coordinator could assign everything, but then assignment stops during an
  election and the leader is a bottleneck.
- **Options considered.**
  1. *Leader dispatch*: the Raft leader plans everything. Simple, but the leader does all CPU work.
  2. *Load-based ID in Bully*: nodes rank themselves by load in a Bully round; heavy per request.
  3. *Deterministic ranking from heartbeats*: every node computes the winner from the last load it
     heard; no extra messages, but uses stale load (Mitzenmacher, "How useful is old
     information?").
  4. *Per-request bidding* (Contract Net, Smith 1980): every serving node sends a load score for
     the request; lowest wins.
- **Decision.** Per-request bidding. Every serving node broadcasts a `Bid { request, score }`;
  the lowest score wins; ties go to the lower node ID. The bid order is also the takeover order.
- **Why.** Load is measured when the request arrives, not from old heartbeats. Assignment keeps
  working during a Raft election. Every node computes the same winner from the same bids, so no
  extra "you won" message is needed.
- **Consequences.** Up to 6 bid messages per request on three nodes and a bid-window wait
  (`bid_window_ms`, 20 ms) added to latency. Nodes can disagree on the winner if a bid is lost;
  first-commit-wins (D27) makes that harmless, at the cost of duplicate work, which we measure.
- **Where.** `src/bidding.rs`, `PeerBody::Bid`, `node::decide_executor`, `policy.assignment`.
- **How we check it.** E1 (policies), E6 (score variants), LB-1, LB-2, LB-3.
- **Report notes.** Open sub-decision: D42 (takeover). Settled sub-decisions: D43 (bid window
  start), D44 (claims), D45 (score formula).

### D3 · Leader dispatch kept as a measured baseline

- **Date:** Phase 1 architecture · **Part:** B · **Status:** Implemented · **Decided by:** team
- **Context.** We claim bidding is better than letting the leader do everything. A claim needs a
  comparison.
- **Options considered.** Bidding only; bidding plus a baseline.
- **Decision.** Keep `AssignmentPolicy::LeaderDispatch` as a config switch
  (`policy.assignment = "leader-dispatch"`). The leader plans every request; followers stand by.
- **Why.** E1 and LB-3 compare the two on latency percentiles, throughput and fairness. The
  baseline also let the rest of the system work end to end before bidding existed (D34).
- **Consequences.** One more code path, but it is small: it reuses the planner and the commit path.
- **Where.** `config.rs::AssignmentPolicy`, `node/mod.rs::leader_dispatch`.
- **How we check it.** `tests/cluster.rs` checks that the leader is the executor; E1.

### D4 · Implement Raft ourselves

- **Date:** Phase 1 architecture · **Part:** A · **Status:** Superseded by D14 (2026-10-27) ·
  **Decided by:** team (Phase 1 document); superseded by the team
- **Context.** The Phase 1 architecture document (D4) said the team writes Raft itself because the
  course assesses election, replication and recovery, and a library would hide exactly those.
- **Options that were considered.**
  1. *openraft* (D14): working, tested replication at once; our own work is everything around Raft.
  2. *Write Raft ourselves*: matches the course's intent; costs weeks before the 4 Nov deadline
     (feature freeze 31 Oct), and every other part waits on it.
  3. *openraft now, our own Raft later behind the same trait.*
- **Outcome.** The team uses openraft (D14). This entry stays so the report can explain the change
  from the submitted Phase 1 design.
- **Report notes.** Explain the change and separate library code from ours (AGENTS.md coordination
  rule 6, D68): openraft provides election and log replication; storage, the network adapter, the
  state machine, proposal forwarding, transport, fault injection, bidding, rejoin and everything else
  are ours.

### D5 · PreVote in Raft

- **Date:** Phase 1 architecture · **Part:** A, D · **Status:** Implemented as option 2 (2026-11-01,
  D75) · **Decided by:** team (Phase 1 document); option 2 chosen by Aabed on the agent's
  recommendation, because the Nov 4 deadline leaves no time to measure first and fix after
- **Context.** While a node is silent, its election timer keeps firing. In plain Raft it raises its
  term each time; when it comes back, its high term can force the healthy leader to step down and
  cause a needless election. The Phase 1 document chose Raft's PreVote extension (Ongaro thesis,
  §9.6) to stop this.
- **What the code does.** openraft 0.9.25 exposes no separate PreVote RPC: our wire carries only
  `RaftRpc::Vote`, `AppendEntries` and `InstallSnapshot`. We have not yet measured what a returning
  node does to the leader.
- **Options considered.**
  1. *Measure first*: run FT-6 (returning node, count elections caused) once C's fault injection
     works. Act only if returns cause elections.
  2. *Gate in our code*: while a node is not serving (D46), it does not start elections. openraft
     has a runtime switch to stop a node from electing itself (`Raft::runtime_config().elect(false)`
     in 0.9; to be confirmed against the docs).
  3. *Our own Raft with PreVote*: rejected by D14 (we use openraft).
- **Recommendation.** Option 1, with option 2 ready as the fix, owned by D with A reviewing. Record
  the FT-6 numbers here either way.
- **Where.** `src/node/rejoin.rs`, `src/raft/mod.rs`.
- **How we check it.** FT-6, E4: elections whose `raft_state` events follow a `restored` event.
- **Result (2026-11-01).** `tests/rejoin.rs::a_returning_follower_causes_no_election` silences a
  follower for 1 s. With elections switched off it passes: same term, same leader. With the switch
  removed it failed 3 runs out of 3: the returning node's term had grown and forced a new election.
  So option 1's question is answered: returns do cause elections on openraft 0.9 without the gate.

### D14 · openraft 0.9.25 behind a `Consensus` trait

- **Date:** 2026-10-20, confirmed 2026-10-27 · **Part:** A · **Status:** Implemented; supersedes D4
  · **Decided by:** A, confirmed by the team
- **Context.** Every other part depends on a working replicated log. Writing Raft first would block
  the team for weeks. The Phase 1 document had chosen to write Raft ourselves (D4); the team
  replaced that with openraft.
- **Options considered.**
  1. *openraft*: mature, async, tokio-based, pluggable storage and network.
  2. *raft-rs* (TiKV): solid, but synchronous and brings its own protobuf types; more glue.
  3. *Our own Raft* (D4): rejected: weeks of work before anything else can be tested, and every
     other part blocked on it.
- **Decision.** openraft `=0.9.25` (pinned exactly) with features `serde`, `storage-v2` and
  `single-term-leader`. Node code reaches Raft only through the `raft::Consensus` trait:
  `propose`, `propose_local`, `handle_rpc`, `status`, `shutdown`.
- **Why.** `single-term-leader` gives standard Raft terms (at most one leader per term), which the
  term rule (D21) and the "Raft term" in every message rely on. `storage-v2` is the storage API
  we implement. The trait means our own Raft can replace openraft without touching `node/`.
- **Consequences.** The pin means everyone builds the same Raft (wire format and behaviour), and an
  upgrade is a deliberate change. openraft decides its own internal timing (replication batching,
  heartbeat scheduling), which we only configure.
- **Where.** `Cargo.toml`, `src/raft/mod.rs` (`TypeConfig`, `RaftNode`), `src/raft/store.rs`,
  `src/raft/network.rs`.
- **How we check it.** `raft::store::tests::passes_openraft_storage_suite` runs openraft's own
  conformance suite on our storage; `tests/replication.rs`.

### D21 · The term rule does not apply to Raft RPCs

- **Date:** 2026-10-20 · **Part:** all · **Status:** Implemented · **Decided by:** A, C
- **Context.** Every peer message carries the sender's Raft term (Phase 1 §6.2), and messages
  from an older term are dropped so a stale node cannot act on old information.
- **Options considered.** Apply the rule to every message; apply it to non-Raft messages only.
- **Decision.** A receiver drops a non-Raft peer message (`Propose`, `Bid`, `Heartbeat`) whose term
  is lower than its own and answers `PeerReply::StaleTerm { current }`. Raft RPCs always go to
  openraft, whatever their term.
- **Why.** A stale leader learns it is stale from openraft's rejection of its append-entries.
  Dropping it silently would leave it believing it still leads.
- **Where.** `node/mod.rs::on_peer_message`, `proto.rs::PeerEnvelope`.
- **How we check it.** The leader-silence test (`tests/replication.rs`) relies on it: the old
  leader returns, is rejected, and steps down.

### D24 · In-memory Raft storage, no snapshots

- **Date:** 2026-10-20 · **Part:** A · **Status:** Superseded by D118 · **Decided by:** A
- **Context.** The Phase 1 document planned a durable log on disk (component C7) and a snapshot
  every 1,000 entries (§8.3), plus a crash-restart test (FT-9). The faults we must inject are
  network silences: the process keeps running and keeps its memory.
- **Options considered.**
  1. *Disk log and vote* (C7): survives process crashes; costs an fsync per append and a storage
     format to maintain.
  2. *In memory, snapshots on*: catch-up through `InstallSnapshot` after trimming.
  3. *In memory, no snapshots*: the log grows for the length of a run; a returning node always
     catches up by replay.
- **Decision.** Option 3. The log and vote live in memory (`LogStore`), `SnapshotPolicy::Never`
  means the log is never purged. Snapshot building and installing are implemented (openraft's
  suite needs them) but never triggered.
- **Why.** A silenced process keeps its memory, so nothing is lost and Raft's guarantees hold.
  Replay is enough for silences of 20 s. Without fsync, commit latency measures the protocol, not
  the disk.
- **Consequences.** Restarting a node process is **not** a supported fault: a restarted node forgets
  its vote, which Raft forbids (it could vote twice in one term). Fault injection must use the
  fault gate (D13), never a process kill. FT-9 from the Phase 1 plan is dropped for Phase 1. The
  log grows by one entry per request for the length of a run; at a few thousand requests per
  second over five minutes that is about a million entries, fine in memory but worth watching.
- **Where.** `src/raft/store.rs`, `RaftNode::start` (`snapshot_policy`).
- **How we check it.** E4 measures catch-up time by replay; the Phase 1 metric "snapshot used or
  not" is always "not".
- **Report notes.** State clearly that crash-recovery is out of scope and why.

### D25 · Static membership; every node calls `initialize`

- **Date:** 2026-10-20 · **Part:** A · **Status:** Implemented · **Decided by:** A
- **Context.** openraft forms a cluster only after `initialize` is called with the membership.
  Nodes start in any order on six machines.
- **Options considered.** One designated node initializes and adds the others; every node
  initializes with the same membership.
- **Decision.** Every node calls `Raft::initialize` with all nodes from the config at startup and
  ignores `NotAllowed` (already initialized). The membership never changes during a run. Each
  member's `BasicNode` records its `peer_addr`, but the transport dials addresses from the config.
- **Why.** The cluster forms whichever nodes start first. openraft documents that calling
  `initialize` on several nodes with the same membership is safe.
- **Consequences.** Membership changes (adding a fourth node) are not supported. The config must be
  identical on all six machines (D59), or Raft membership breaks.
- **Where.** `RaftNode::start`.

### D26 · Any node proposes; followers forward to the leader

- **Date:** 2026-10-20 · **Part:** A · **Status:** Implemented · **Decided by:** A
- **Context.** With bidding, the executor is often a follower, and it must commit its own result.
  The Phase 1 document had a `ProposePlan` message from the winner to the leader.
- **Options considered.**
  1. *Only the leader proposes*: every executor becomes a client of the leader, with its own retry
     logic in every caller.
  2. *`propose` works on every node*: a follower forwards to the leader and retries internally.
- **Decision.** `Consensus::propose` works everywhere. On a follower, openraft answers
  `ForwardToLeader`; the node sends the command to the leader as `PeerBody::Propose` (this is the
  Phase 1 `ProposePlan`), and the leader commits it with `propose_local` and answers
  `Proposed(outcome)`. Failed attempts retry with backoff starting at 5 ms and doubling to
  100 ms, until `propose_timeout_ms` (3 s). One forward waits at most `peer_rpc_timeout_ms`
  (500 ms).
- **Why.** Retry logic lives in one place. Retrying is safe because every command is idempotent
  (D29): a proposal that did commit before a leader change comes back as `Duplicate` with the same
  result.
- **Consequences.** A forwarded commit costs one extra round trip (follower → leader) on top of
  replication. A proposal can wait up to 3 s across an election before giving up.
- **Where.** `RaftNode::propose_with_retry`, `RaftNode::forward`, `PeerBody::Propose`.
- **How we check it.** `tests/replication.rs::proposals_from_every_node_replicate_to_identical_state`;
  the `commit` event's `forwarded` field splits commit latency by path in E5.

### D32 · Retry an unreachable peer after two heartbeats

- **Date:** 2026-10-20 · **Part:** A · **Status:** Implemented · **Decided by:** A
- **Context.** When a peer is unreachable, openraft waits `backoff()` before trying it again. The
  default is 500 ms.
- **Decision.** `PeerLink::backoff` returns `2 × raft_heartbeat_ms` (100 ms by default).
- **Why.** After a silence ends, the leader resumes replication to the returning node within one
  backoff period. 500 ms would add up to half a second to every recovery time measured in E4.
- **Consequences.** During a 20 s silence the leader retries the silent node 200 times instead of
  40. Each retry fails at once on the silenced side (D22), so the cost is small.
- **Where.** `RaftNode::start` (`backoff`), `raft/network.rs::PeerLink::backoff`.

---

## 2. Replicated state

### D8 · Reply only after commit

- **Date:** Phase 1 architecture · **Part:** A · **Status:** Implemented · **Decided by:** team
- **Context.** A node could answer the client as soon as it has a plan.
- **Options considered.** Reply on plan; reply on commit.
- **Decision.** A node replies only after the result is committed and applied. The reply carries the
  committed result, which may be another node's (D27).
- **Why.** A reply sent before commit could promise a plan that a fault then erases. Raft paper:
  an entry is safe once a majority has it.
- **Consequences.** One replication round trip is added to every request's latency.
- **Where.** `node/mod.rs::handle_request`, `execute`, `send_done`.
- **How we check it.** `tests/cluster.rs::a_request_is_committed_before_the_reply_and_retries_get_the_same_result`
  checks the result is already in the replier's state when the reply arrives.

### D9 · Request ID plus a replicated table of results

- **Date:** Phase 1 architecture · **Part:** A · **Status:** Implemented; amended by D27 ·
  **Decided by:** team
- **Context.** The network can duplicate and reorder messages, clients retry, and two nodes can
  compute the same request. We need at-most-once results that survive failover.
- **Options considered.** Client-side deduplication only; a replicated request table.
- **Decision.** Every request has a unique `RequestId { client, seq }`. The replicated state keeps
  `results: BTreeMap<RequestId, CommittedResult>`; this is the Phase 1 "request table".
- **Why.** Because the table is replicated, any node can recognise a retry and answer it after a
  failover (D37).
- **Amendment.** The Phase 1 document planned to trim the table by TTL. The code never trims it:
  the table grows for the length of a run. Trimming would need a deterministic rule in the log
  (for example a `Command` that drops results older than an index). Not needed for 5-minute runs.
- **Where.** `state.rs::CloudState::results`, `proto.rs::RequestId`.

### D10 · Optimistic check at commit instead of locks

- **Date:** Phase 1 architecture · **Part:** A, E · **Status:** Implemented; amended by D28 ·
  **Decided by:** team
- **Context.** Two executors planning different requests at the same time may both pick the same free
  responder from their local copy of the state.
- **Options considered.** Lock responders across nodes while planning; check at commit time and
  reject plans built on stale data.
- **Decision.** Check at commit. The Phase 1 design recorded per-resource versions in
  `ProposePlan`; the code does the simpler equivalent in D28.
- **Why.** No locks held across nodes, so a silent node never holds a lock. Stale plans are rejected
  instead of overwriting newer assignments (Kleppmann, ch. 7).
- **Consequences.** Under contention some plans are recomputed (D35); we count them (`stale` counter).

### D27 · First commit wins; the outcome tells the proposer what happened

- **Date:** 2026-10-20 · **Part:** A · **Status:** Implemented · **Decided by:** A
- **Context.** Takeovers, retries and wrong bidding winners mean two nodes can propose results for
  one request. The proposer needs to know what happened to its proposal.
- **Decision.** Applying `CommitResult` gives one of:
  - `Committed(result)`: this proposal was first for its request ID and is now the result.
  - `Duplicate(winner)`: a result already exists; the proposal is discarded and the winner is
    returned. The executor replies with the winner, so every reply for one ID carries the same
    result.
  - `Stale { request, responder, reason }`: the plan's responder could not be taken (D28). Nothing
    is recorded.
  - `Applied`: for registration, release and openraft's internal entries.
- **Why.** The first committed result per ID is the only one anybody ever sees, so retries and
  duplicate work can never assign two different responders to one report.
- **Where.** `state.rs::CloudState::commit_result`, `proto.rs::Outcome`.
- **How we check it.** `state::tests::first_commit_wins`,
  `tests/replication.rs::first_commit_wins_when_several_nodes_execute_one_request`. FT-1, FT-7:
  "duplicate commits" must be 0.

### D28 · A plan is stale if its responder is held or unknown

- **Date:** 2026-10-20 · **Part:** A · **Status:** Implemented · **Decided by:** A
- **Context.** The concrete form of D10.
- **Decision.** Each responder slot has `held_by: Option<RequestId>`. A `CommitResult` whose
  responder is held by another request returns `Stale { reason: HeldBy(holder) }`; one whose
  responder is not registered returns `Stale { reason: UnknownResponder }`. A plan with
  `responder: None` (nobody suitable was free) commits without holding anything.
- **Why.** Holding is exactly the conflict that matters, and checking it costs one map lookup. No
  version numbers travel in messages.
- **Consequences.** A request with no free responder still gets an answer ("nobody available"),
  which counts as served. See D39 for why that will happen a lot until responders are released.
- **Where.** `state.rs::commit_result`, `StaleReason`.
- **How we check it.** `state::tests::plan_for_a_taken_responder_is_stale`,
  `tests/replication.rs::plan_for_a_taken_responder_comes_back_stale`.

### D29 · Three idempotent log commands

- **Date:** 2026-10-20 · **Part:** A · **Status:** Implemented · **Decided by:** A
- **Context.** Raft entries carry our commands. A proposal can be retried after a leader change, so
  it may land in the log twice.
- **Decision.** `Command` has three variants, each idempotent:
  - `RegisterResponders { responders }`: adds unknown responders; known ones are left untouched,
    including who holds them.
  - `CommitResult(ResultProposal)`: first commit wins (D27).
  - `ReleaseResponder { responder, request }`: frees the responder only if `request` holds it.
- **Why.** Applying a command twice equals applying it once, which makes D26's retries safe.
- **Where.** `proto.rs::Command`, `state.rs::apply`.
- **How we check it.** `state::tests::release_only_frees_the_holder_and_is_idempotent`,
  `registering_again_keeps_assignments`.

### D30 · Deterministic state machine, integers only

- **Date:** 2026-10-20 · **Part:** A · **Status:** Implemented · **Decided by:** A
- **Context.** Every node applies the same log and must reach exactly the same state, or the digest
  (D31) and "first commit wins" break.
- **Decision.** `state.rs`, `bidding.rs` and `planner.rs` never touch the network, the clock or
  randomness; time and inputs are arguments. Replicated data uses `BTreeMap`/`BTreeSet`, never
  iterates a `HashMap`. No floats in `Command` or `CloudState`: integer units with named suffixes
  (`cost`, `latency_us`, `timeout_ms`, coordinates in metres).
- **Why.** `HashMap` iteration order differs between processes; float results can differ between
  CPUs and compiler versions; clocks differ between machines.
- **Where.** AGENTS.md "Architecture rules"; `state.rs`.
- **How we check it.** `state::tests::same_log_same_digest_different_log_different_digest`; digests
  equal across nodes in every run.

### D31 · State digest: an FNV-1a hash chain over the applied log

- **Date:** 2026-10-20 · **Part:** A · **Status:** Implemented · **Decided by:** A
- **Context.** FT-4 passes when "its state hash equals the leader's". We need a cheap way to show
  that all nodes hold identical state.
- **Options considered.** Hash the whole state on demand (O(state) per check, misses divergence that
  later cancels out); a running hash chain.
- **Decision.** `digest = FNV-1a(previous digest, index, command, outcome)`, updated on every applied
  command. Nodes log it every second (`state_digest` event) and `ctl status` shows it.
- **Why.** Equal digests at equal indexes mean two nodes applied the same log with the same
  results, at O(1) per command. Hashing the outcome too catches a non-deterministic state machine.
  FNV-1a has fixed constants, so every build computes the same value (the toolchain pin, D15,
  keeps `Hash` impls identical).
- **Where.** `state.rs::chain`, `Event::StateDigest`, `NodeStatus::digest`.

### D38 · Responders seeded once through Raft by the leader

- **Date:** 2026-10-20 · **Part:** A, E · **Status:** Implemented (campus generation: D97)
  · **Decided by:** A
- **Context.** Responders are part of the replicated state, so they must enter it through the log.
- **Decision.** The campus is a pure function of `[campus]` in the config (`seed`, `zones`,
  `responders`), so every node derives the same list. A background task polls every 200 ms; when
  this node is the leader and the state has no responders yet, it proposes `RegisterResponders`.
  Registration is idempotent (D29), so two leaders in a row proposing it is harmless.
- **Why.** No special bootstrap path: seeding uses the same commit path as everything else.
- **Where.** `node/mod.rs::spawn_campus_seeding`, `campus.rs::responders`.

### D39 · Responders are never released

- **Date:** 2026-10-27, settled 2026-11-01 · **Part:** A, E · **Status:** Implemented · **Decided by:** omar.leithym, on the agent's recommendation
- **Context.** `ReleaseResponder` exists and is tested, but nothing sends it. With 30 responders
  (the default) the first 30 committed plans hold all of them, and every later request is
  answered with `responder: None`. Load experiments would then measure a planner that has nothing
  to choose from.
- **Options considered.**
  1. *Release after a simulated service time*: the executor (or the leader) proposes
     `ReleaseResponder` a fixed or severity-based time after the commit. Timing is local, but the
     release goes through the log, so state stays deterministic.
  2. *Release when the client confirms*: a new client message; more protocol, more traffic.
  3. *Leases*: a hold expires at a log index or term; needs a deterministic notion of time in the
     log.
  4. *Never release, size the campus to the run*: simplest; unrealistic.
- **Recommendation.** Option 1, proposed by the executor after `service_ms` derived from severity,
  owned by E with A reviewing (it adds a timer to `node/`, A's file). It needs a decision on the
  service-time model and a config key.
- **Where.** `proto.rs::Command::ReleaseResponder`, `node/mod.rs`, `planner.rs`.
- **Code check (2026-11-01).** Confirmed: only `state.rs` and `proto.rs` mentioned `ReleaseResponder`.
- **Decision.** A variant of option 1 that survives a silent executor. Every serving node watches
  the holds in its replicated state. The executor proposes `ReleaseResponder` after the service time
  (D111), every other node after twice that.
- **Why not the alternatives.** The log's first recommendation, executor only, leaks a responder
  for good when the executor goes silent, which E3 and the demo cause. The agent's next suggestion, a
  sweep by the leader, cannot leak but gives the leader a coordinator's job, which Dr Amr's notes
  (Sept 13, Oct 28) rule out.
- **Consequences.** One extra log entry per assigned request, more with backups, so E5's commit and
  message counts include releases. A backup release can overlap the executor's; it is a no-op.
- **Where.** `node/release.rs`, `state.rs::holds`, `metrics.rs::Event::ReleaseProposed`.
- **How we check it.** `node::release::tests`; `tests/cluster.rs`: held responders return to free,
  and a silent executor's responder is released by a peer.

---

## 3. Communication and transport

### D6 · Client sends every request to all three nodes over UDP

- **Date:** Phase 1 architecture · **Part:** F · **Status:** Amended by D17 · **Decided by:** team
- **Context.** Transparency: the client must not know which node does the work or takes over.
- **Decision (Phase 1).** The client sends the same request to all three nodes (three unicasts, not
  IP multicast, which the lab network may block), retries with the same ID, and keeps the first
  reply. Over UDP, because request-reply needs no connection and the reply acts as the ack.
- **What still holds.** Sending to all three nodes and keeping the first reply. **What changed:**
  the transport is TCP (D17).
- **Report notes.** Open question from Phase 1 §13.3: does the lab network allow IP multicast?
  Irrelevant while we use unicast to each node.

### D7 · TCP between cloud nodes

- **Date:** Phase 1 architecture · **Part:** C · **Status:** Implemented · **Decided by:** team
- **Decision.** Nodes talk over TCP: one long-lived, multiplexed connection per peer (D19).
- **Why.** Ordered, reliable delivery for log replication without writing our own retransmission.
  Raft still tolerates loss and duplication when a connection drops and reconnects.
- **Consequences.** Reconnect handling after a silence. A TCP connection can stall for a long time
  on a real network failure; our per-call timeouts (`peer_rpc_timeout_ms`, openraft's TTLs) bound
  every wait.

### D15 · Rust 2024 on a pinned toolchain, tokio runtime

- **Date:** 2026-10-20 · **Part:** all · **Status:** Implemented · **Decided by:** A
- **Decision.** Rust edition 2024, compiler pinned to 1.94.1 in `rust-toolchain.toml` (with rustfmt
  and clippy), tokio as the async runtime. Release builds keep debug symbols (`debug = 1`) for
  profiling experiment runs.
- **Why.** Same compiler everywhere means the same wire format, the same `Hash` output for the
  digest and the same lints in CI. tokio is what openraft runs on.
- **Consequences.** Upgrading the compiler is a team decision, not a local one.
- **Where.** `rust-toolchain.toml`, `Cargo.toml`.

### D16 · Wire format: length-delimited frames with bincode

- **Date:** 2026-10-20 · **Part:** all · **Status:** Implemented · **Decided by:** A, C
- **Decision.** Every frame is a 4-byte big-endian length then a body. Bodies use bincode with
  varint integers, at most 16 MiB (`MAX_FRAME_BYTES`), trailing bytes rejected. A malformed frame
  closes the connection.
- **Why.** Compact and fast for many small messages (heartbeats, bids). Every openraft type has plain
  serde derives with no `skip_serializing_if`, so bincode round-trips them. The size limit stops a
  corrupt length prefix from allocating huge buffers.
- **Rejected.** JSON: larger and slower on the hot path, and it cannot use struct map keys. gRPC:
  code generation and HTTP/2 for no gain on a LAN.
- **Where.** `proto.rs::{encode, decode}`, `transport.rs::framed`.
- **How we check it.** `proto::tests` (round trips, hostile length prefix, trailing bytes).

### D17 · Clients use TCP on a dedicated client port

- **Date:** 2026-10-20 (scaffold), settled 2026-11-01 · **Part:** C, F · **Status:** Implemented; TCP confirmed for clients, peers amended by D119 · **Decided by:** A in the scaffold; omar.leithym
- **Context.** The Phase 1 document chose UDP for clients (D6). The scaffold built TCP.
- **Options considered.**
  1. *UDP* as planned: no connection setup; one datagram per request per node. We would need our
     own handling of datagram size limits and a second encoding path, and replies go to the
     request's source address.
  2. *TCP*: one connection per node per client process, reused by all simulated users; the same
     framing as the peer port; replies come back on the connection.
- **Decision (in code).** TCP, with one-way frames in both directions: `ClientFrame::Request` and
  `ClientFrame::Control` in, `ServerFrame::Reply` and `ServerFrame::Control` out, replies matched
  by request ID.
- **Why.** One transport and one framing for everything; `ctl` reuses it (D18); a client process
  holds three connections however many users it simulates, so connection cost is paid once.
- **Consequences.** A silent node's TCP connection does not tell the client anything; the client
  relies on the other two nodes' replies and on its retry timer, exactly as with UDP.
- **Recommendation.** Keep TCP and record the change from Phase 1 in the report.

### D18 · Two ports per node; `ctl` shares the client port

- **Date:** 2026-10-20 · **Part:** C · **Status:** Implemented · **Decided by:** A
- **Context.** The Phase 1 document proposed three ports: UDP 7000 for clients, TCP 7100 for peers,
  TCP 7200 for the experiment runner.
- **Decision.** Two TCP ports per node, both from the config: `peer_addr` (default port 7001) for
  Raft, forwarded proposals, bids and heartbeats, and `client_addr` (default 8001) for clients and
  `ctl`. Control commands are `ClientFrame::Control` frames on the client port.
- **Why.** Fewer ports to open in the lab firewall. Control frames bypass the fault gate (D23), so a
  separate port gives no extra isolation.
- **Where.** `config.rs::NodeAddr`, `config/*.toml`.

### D19 · One multiplexed connection per peer

- **Date:** 2026-10-20 · **Part:** C · **Status:** Implemented · **Decided by:** A, C
- **Context.** openraft calls `new_client` for every heartbeat round and every election.
- **Options considered.** A connection per request (a TCP handshake every 50 ms); one request at a
  time per connection (head-of-line blocking); one connection with many requests in flight.
- **Decision.** `PeerClient` keeps one TCP connection per peer, connects lazily, reconnects after a
  failure, and matches replies to requests by `seq`. On the server side every request runs in its
  own task, so replies can come back in any order.
- **Why.** A slow request (a forwarded proposal waiting for commit) does not hold up the heartbeats
  behind it.
- **Consequences.** Reconnects happen on the next call after a failure. **Amended by D89:** after a
  failed connect, calls fail fast for a short cooldown instead of reconnecting on every call.
- **Where.** `transport.rs::PeerClient`, `serve_peers`, `raft/network.rs::PeerLink`.
- **How we check it.** `transport::tests::concurrent_calls_get_their_own_replies`.

### D20 · `TCP_NODELAY` on every connection

- **Date:** 2026-10-20 · **Part:** C · **Status:** Implemented · **Decided by:** A
- **Decision.** Nagle's algorithm is off on every connection.
- **Why.** Small messages dominate; Nagle with delayed ACKs would add up to 40 ms to each, more
  than the whole bid window.
- **Where.** `transport.rs::framed`.

### D89 · Peer reconnect: fixed cooldown after a failed connect

- **Date:** 2026-11-01 · **Part:** C · **Status:** Implemented · **Decided by:** Wahiba, on the
  agent's recommendation; reviewers Aabed (owner of `transport.rs`), Noor (C↔F pair), Omar (A, for
  the one-line hooks in `node/mod.rs` and `raft/network.rs`)
- **Context.** D19 left `TODO(C)`: `PeerClient` tried a fresh TCP connect on every call to a peer
  whose connection was gone. Two cases make that costly. (1) The peer process is not running
  (startup, crash, Ctrl-C): every heartbeat, bid and forwarded proposal makes a refused connect and
  logs it. (2) A real network failure on the lab LAN (cable pulled, switch down): the connect
  *hangs*, so every call to that peer waits its full timeout (`peer_rpc_timeout_ms`, 500 ms), and
  other calls queue on the connect lock behind it. openraft's own retries already back off (D32);
  this is about the non-Raft calls. Our fault model does **not** trigger it: `ctl silence` keeps
  the silenced node's listener open, so connects succeed and frames are just discarded (D22). We
  built it anyway because the demo and experiments run on six real machines (D60), where (1) and (2)
  happen, and the project description asks for an explicit retry policy (p. 6).
- **Options considered.**
  1. *Fixed cooldown*: after a failed connect, calls fail at once until a fixed window ends; the first
     call after it tries again. A returning peer is reached within one window.
  2. *Exponential, capped* (e.g. 50 ms doubling to 1 s, reset on success): fewer attempts at a peer
     that stays down, but a node back after a 20 s fault may wait up to the cap before heartbeats and
     bids reach it, which inflates the failover and recovery times we must measure (p. 4).
  3. *Exponential with jitter*: jitter de-synchronises many clients hitting one server; with two
     peers it only adds noise to timing tests and measurements.
  4. *Do nothing* and close the TODO, or *defer* until lab runs show slow calls.
- **Decision.** Option 1. The cooldown starts when a connect fails, either refused/errored or still
  pending when the call's deadline passes. While it runs, `PeerClient::call` returns
  `TransportError::Backoff` (D91) at once, checked before waiting on the connect lock, and again by
  callers that were queued behind the failing connect. Each new failed connect restarts the window;
  the first successful connect ends the outage. Timeouts on an *open* connection (a silenced peer)
  never start a cooldown.
- **Why.** Recovery and failover times are graded measurements, so the backoff must add a small,
  bounded delay: at most one window. It matches openraft's fixed backoff (D32), so all peer traffic to
  a downed node behaves alike. It is the smallest change before the Phase 1 deadline.
- **Consequences.** A peer that stays down gets one connect attempt per window (10 per second at the
  default) instead of one per call. A peer that returns is reconnected up to one window later. A
  silenced peer is unaffected (D22). Calls during the cooldown fail with `Backoff`, not `Timeout`, so
  callers see the failure up to 500 ms sooner.
- **Where.** `node/transport.rs`: `PeerClient::{call, connection, check_backoff, connect_failed,
  connected}`, `Outage`.
- **How we check it.** `transport::tests::refused_connect_backs_off_then_reconnects` (refused →
  `Connect`, next call → `Backoff`, peer started → reconnects after the window).
  `silenced_receiver_times_out_then_recovers` guards the trigger: a timeout on an open connection
  must not start a cooldown. Both run 10× green. The timed-out-connect path is not unit-tested,
  because forcing a hanging connect depends on the machine's network; check it on the lab machines by
  unplugging a node during E3.
- **Report notes.** A sentence in the retry-policy section: three layers of retry (client resend
  D52, openraft backoff D32, transport reconnect cooldown D89), all fixed-interval so recovery time
  stays predictable.

### D90 · Reconnect cooldown = 2 × `raft_heartbeat_ms`, no new config key

- **Date:** 2026-11-01 · **Part:** C · **Status:** Implemented · **Decided by:** Wahiba, on the
  agent's recommendation
- **Context.** D89 needs a window length.
- **Options considered.**
  1. *2 × `raft_heartbeat_ms`* (100 ms by default), the value D32 gives openraft.
  2. *A new key `peer_reconnect_backoff_ms`*: tunable per run, but adds a key to `config.rs` (Part A),
     validation, `protocol.md`, `local.toml` and `lab.example.toml`.
  3. *A constant in `transport.rs`*: simplest, but AGENTS.md wants timers in config, and it drifts if
     the Raft heartbeat is retuned.
- **Decision.** Option 1. `node/mod.rs::start_with` computes it next to the other per-node setup and
  passes it to `PeerSet::new` as a `Duration` (D91).
- **Why.** Raft and non-Raft calls to a downed peer back off alike, so a returning node is reached by
  everything within the same 100 ms. No new config surface before the deadline.
- **Consequences.** Retuning `raft_heartbeat_ms` changes both backoffs together. If E3 shows the
  two need different values, add the key then (new entry amending this one).
- **Where.** `node/mod.rs::start_with`; compare `raft/mod.rs` (`backoff`, D32).
- **How we check it.** By construction; the unit tests pass their own window.

### D91 · `TransportError::Backoff`, treated as Unreachable by openraft

- **Date:** 2026-11-01 · **Part:** A, C · **Status:** Implemented · **Decided by:** Wahiba, on the
  agent's recommendation; Omar (A) to review the `raft/network.rs` and `node/mod.rs` lines
- **Context.** Calls during the cooldown need an error, and `PeerClient` needs the window and an event
  log, which `PeerSet::new` did not take.
- **Options considered.**
  1. *New variant `Backoff { peer, retry_in }`*, added to the Unreachable arm of
     `raft/network.rs::rpc_error`.
  2. *Repeat the last `Connect` error*: no change outside `transport.rs`, but logs would show connect
     attempts that never happened, which misleads E3 analysis.
  3. *New variant without the mapping*: openraft would treat it as a network error and retry every
     tick instead of backing off.
  For wiring: a `Duration` parameter (chosen), the whole `Timers` struct (couples transport to the
  config layout), or a `Timers` helper shared with D32 (more Part A files).
- **Decision.** Option 1, and `PeerSet::new(me, cluster, gate, log: &EventLog, reconnect_backoff:
  Duration)`. The `log` argument replaces the old `msgs` one; message counters come from
  `log.msgs()`.
- **Why.** The error says exactly what happened, and openraft backs off on it like on a refused
  connect. Passing a plain `Duration` keeps `transport.rs` free of config knowledge and lets tests
  pick any window.
- **Consequences.** Callers that match on `TransportError` (heartbeats, bidding) see a new variant;
  treat it like `Connect`.
- **Where.** `node/transport.rs::{TransportError::Backoff, PeerSet::new, PeerClient::new}`,
  `raft/network.rs::rpc_error`, `node/mod.rs::start_with`.
- **How we check it.** `refused_connect_backs_off_then_reconnects` asserts `Backoff { peer: 2, .. }`.

### D92 · Peer outage events: `peer_unreachable`, `peer_reconnected`

- **Date:** 2026-11-01 · **Part:** C · **Status:** Implemented · **Decided by:** Wahiba (chose events
  over the agent's tracing-only recommendation)
- **Context.** The old `TODO(C)` also asked for connection-level metrics if E3 needs them. An
  operator on the lab machines also needs to see why calls fail fast.
- **Options considered.**
  1. *tracing only*, leaving metrics to E3's owner.
  2. *Two events, once per outage* (chosen).
  3. *Also count fast-failed calls* in `peer_reconnected`.
  4. *One `peer_connect` event per attempt* with `ok` and `latency_us`: ~10 lines a second per down
     peer, and analysis must pair them up.
- **Decision.** In `metrics::Event` (faults section): `{"ev":"peer_unreachable","peer":2,"error":"..."}`
  on the first failed connect of an outage, and `{"ev":"peer_reconnected","peer":2,"down_ms":20140}`
  on the connect that ends it. `down_ms` runs from the first failed connect to the successful one,
  measured with `Instant` on the logging node (D56). Matching `tracing` lines: one `warn!` and one
  `info!` per outage. Nothing is logged per fast-failed call.
- **Why.** One pair per outage gives E3 a timeline of when each node saw each peer go and return,
  without flooding the log during a 20 s fault.
- **Consequences.** New field names are now part of the analysis contract (keep them stable). At
  cluster start a node may log a short `peer_unreachable`/`peer_reconnected` pair for peers that
  were not listening yet; analysis should skip outages that start before `started` + a few seconds.
  A silenced peer produces no events (its listener stays up, D22); its fault shows in `silenced` /
  `restored` instead. An outage ends only on the next *call* to that peer, so a node that never
  calls a peer leaves its `peer_unreachable` unmatched. Seen on 2026-11-01: a follower that missed
  the leader at startup never called it again. Until peer heartbeats (D41) make every node call
  every peer every 50 ms, analysis must allow unmatched `peer_unreachable` events.
- **Where.** `metrics.rs::Event::{PeerUnreachable, PeerReconnected}`,
  `transport.rs::PeerClient::{connect_failed, connected}`.
- **How we check it.** Run a local cluster (`metrics.dir` is set in `config/local.toml`), kill one
  node's process and start it again. Done on 2026-11-01: with node 3 down for about 3 s, the leader
  (node 1) logged one `peer_unreachable` and one `peer_reconnected` with `down_ms: 2536`, measured
  from its first failed connect. The follower (node 2) logged nothing for node 3 because it does not
  call node 3 yet (see Consequences).
- **Report notes.** Useful for the lab-network run of E3: per-node view of peer reachability next to
  Raft's `raft_state` events.

---

## 4. Faults, failure detection and recovery

### D12 · The Raft leader runs the fault schedule

- **Date:** Phase 1 architecture · **Part:** C · **Status:** Implemented, amended by D78 (message,
  timings, seeding, overlap rule) · **Decided by:** team
- **Context.** The project requires random failures of up to 20 s, one node at a time.
- **Options considered.** A separate election for each fault round; the leader decides.
- **Decision.** Every 30–60 s (random), the Raft leader picks a victim uniformly from all three nodes,
  itself included, and orders it silent for a random 1–20 s. The order carries the leader's term so
  a stale leader's order is dropped (D21).
- **Why.** One leader means exactly one victim at a time with no extra election. Letting the leader
  pick itself tests coordinator failover too.
- **Consequences.** No faults while there is no leader (a few hundred ms). The order needs a peer
  message: `proto.rs` has no `FaultOrder` yet, so implementing this is a contract change (design-log
  entry, review from every owner, `protocol.md` update).
- **Where.** `node/faults.rs`.
- **How we check it.** FT-8, E3.

### D13 · Faults injected in the network endpoint

- **Date:** Phase 1 architecture · **Part:** C · **Status:** Implemented (gate);
  `faults::silence_for` is `TODO(C)` · **Decided by:** team
- **Options considered.** Kill the process; drop traffic at the network endpoint.
- **Decision.** A `FaultGate` in the transport drops all peer and client traffic while closed. The
  process, its Raft state and its memory keep running.
- **Why.** Matches the project's "ignore communication" rule exactly; one switch drops everything, so
  every protocol above sees real message loss. Process kills are unsupported anyway (D24).
- **Where.** `transport.rs::FaultGate`.
- **How we check it.** `transport::tests::silenced_receiver_times_out_then_recovers`,
  `silenced_sender_fails_fast`; the leader-silence test.

### D22 · Fault gate semantics

- **Date:** 2026-10-20 · **Part:** C · **Status:** Implemented, amended by D78 (the `FaultOrder(Ok)`
  reply leaves a node that has just been silenced) · **Decided by:** A, C
- **Decision.** While silenced, a node:
  - reads and discards every incoming peer and client frame without replying, so callers see a
    timeout, as with a pulled cable;
  - fails every outgoing peer call at once with `TransportError::Silenced`, which openraft sees as
    `Unreachable` and backs off (D32);
  - drops replies that arrive after it was silenced, and drops client replies it tries to send.
- **Why.** From the outside the node is indistinguishable from one whose cable was pulled; from the
  inside it learns nothing new, so its state goes stale exactly as the fault model says.
- **Where.** `transport.rs::{PeerClient::call, peer_connection, client_connection, ReplyTx::send}`.

### D23 · Control frames bypass the fault gate

- **Date:** 2026-10-20 · **Part:** C · **Status:** Implemented · **Decided by:** A, C
- **Decision.** `ctl` commands (`Status`, `Silence`, `SkewLoad`) reach a silenced node and get
  answered.
- **Why.** So we can watch a silenced node during a demo and end a fault by hand. Client and peer
  traffic are still dropped, which is what the fault models.

### D41 · Peer heartbeats as a failure detector for takeover

- **Date:** 2026-10-20 · **Part:** C · **Status:** Implemented, amended by D79 (confirmed 2026-11-01;
  what counts as heard, pausing while silenced, events) · **Decided by:** A in the scaffold
- **Context.** The Phase 1 document said "no separate failure detector: Raft heartbeats do the job".
  But Raft heartbeats only flow from leader to followers, so a follower cannot tell that another
  follower (for example a bidding winner) went silent.
- **Decision.** Every node sends `PeerBody::Heartbeat` to every peer each `peer_heartbeat_ms` (50 ms)
  and suspects a peer after `takeover_missed_heartbeats` (3) intervals with nothing heard.
  `Suspicion` takes the time as an argument so it can be unit-tested.
- **Why.** Takeover needs every node to see every other node.
- **Consequences.** 6 extra messages every 50 ms on three nodes (120 per second), counted as
  `peer_heartbeat` in E5. A timeout-based detector can wrongly suspect a slow node; that only costs
  duplicate work (D27), never two results.
- **Where.** `node/heartbeat.rs`, `NodeContext::suspicion`.
- **How we check it.** E3 (detection time), E5 (message overhead).

### D42 · Takeover trigger

- **Date:** 2026-10-27 · **Part:** B, C · **Status:** Accepted 2026-11-01, implemented by D110 ·
  **Decided by:** Ahmed Abdeen and Aabed, on the agent's recommendation.
- **Context.** Two rules are on paper. Phase 1 §6.3: takeover after `3 × the winner's recent average
  service time, at least 200 ms`, with no committed plan. Code (`bidding.rs` TODO): takeover after
  `takeover_missed_heartbeats` missed peer heartbeats (150 ms by default) with nothing committed.
- **Options considered.**
  1. *Missed heartbeats only*: detects silence fast (150 ms) and does not depend on planning time.
     A slow but alive winner is never taken over, so a slow plan is not duplicated.
  2. *Service-time timer only*: also catches a winner that is alive but stuck; duplicates work when
     planning is slower than usual.
  3. *Both*: take over when the winner is suspected, or when no commit arrives within a generous
     service-time bound.
- **Recommendation.** Option 3, with the heartbeat rule as the main trigger and the service-time
  bound (≥ 200 ms) as a backstop. Measure the wrong-takeover rate in E3.
- **Decision.** Option 3. Main trigger: the winner is suspected (150 ms by default). Backstop:
  no commit within `max(200 ms, 3 × the winner's bid)`. Bids carry only the score, and service
  time leaves out queue wait, so the expected-wait bid stands in for the winner's service time
  (Ahmed's amendment); queue-only and CPU-only scores are not times and get 200 ms. If time runs
  short, the backstop is dropped first.
- **Where.** D110.

### D46 · Rejoin gate: `serving` flag, current at the leader's commit

- **Date:** 2026-10-20 · **Part:** D · **Status:** Implemented, amended by D75 (how the target is
  learned, when `serving` goes false) · **Decided by:** team (Phase 1 §9.6), shaped in the scaffold
- **Context.** A returning node's state is stale. If it bids or replies, it could plan against old
  state or answer with old information.
- **Decision.** `NodeContext::serving` is the switch. When a node returns from a fault it clears
  `serving`, learns the leader's commit index, waits until its applied index reaches it
  (`rejoin::is_current`), then sets `serving` again, logging both transitions. A node that is not
  serving does not start elections, bid, plan or reply. It still accepts log entries and grants
  votes, so the majority stays intact. Raft replication does the copying.
- **Why.** Phase 1 requirement: "the revived node rebuilds current state before it participates".
  Only soft state (load score, bid rounds) is rebuilt locally; everything replicated comes from
  the log.
- **Consequences.** Recovery time (E4) is silence end → `serving` again.
- **Where.** `node/rejoin.rs`, `NodeContext::serving`, `handle_request` (already checks it).
- **How we check it.** FT-4, FT-5, E4; digests equal after rejoin.

### D74 · Manual faults: one at a time, 1 ms to 20 s, rejected otherwise

- **Date:** 2026-11-01 · **Part:** C · **Status:** Implemented · **Decided by:** Aabed, on the
  agent's recommendation
- **Context.** `faults::silence_for` (behind `ctl silence`, and later behind the fault schedule,
  D12) was a stub. Its TODO said overlapping faults must not cut each other short. Three questions
  were open: what a second silence order does while the node is already silenced, what happens to
  durations outside the spec, and how the restore side reaches the rejoin gate (D46).
- **Options considered.**
  1. *Overlap: reject the second order.* One `silenced`/`restored` pair per fault; matches the
     fault model's one victim at a time. The operator must wait for the fault to end.
  2. *Overlap: extend to the later end time.* Never shortens a fault, but one silence then has two
     `silenced` events and one `restored`, so E3 cannot read a fault's length from its event pair.
  3. *Overlap: restart the timer.* Can cut a fault short or stretch it past 20 s. Rejected.
  4. *Over 20 s: clamp to 20 s.* Quietly changes what the operator asked for; an experiment could
     run on a duration nobody chose. Rejected in favour of an error.
  5. *0 ms: accept as a no-op.* Hides typos and logs a meaningless fault. Rejected.
  6. *Restore hook: a `tokio::sync::watch` in `FaultGate` now.* Unused until the rejoin gate is
     built; deferred to that PR so this one stays small.
- **Decision.** `silence_for` rejects `ms = 0` and `ms > 20000` (`MAX_FAULT`) with an error, and
  rejects an order while the gate is closed ("already silenced"). Otherwise it closes the gate,
  logs `silenced { ms }`, and a spawned task sleeps `ms`, reopens the gate and logs `restored`. The
  check-and-close is one atomic step (`FaultGate::try_silence`, a compare-and-swap), so two orders
  arriving together cannot both start a fault. `ctl` prints the error. The restore notification
  for the rejoin gate is added with the rejoin work (D46).
- **Why.** Every `silenced`/`restored` pair in the logs is then exactly one in-spec fault, which
  keeps E3 and E4 analysis simple and honest. The project allows faults of at most 20 s, so a
  longer one is an operator mistake, not a test. The scheduled injector (D12) will only ever have
  one victim at a time anyway, so rejecting overlaps costs nothing there.
- **Consequences.** There is no way to end a fault early from `ctl`; that would need a new control
  message (`proto.rs` change). A node that shuts down while silenced drops the restore task with
  the runtime. The leader-side schedule must treat "already silenced" as a skipped round.
- **Where.** `node/faults.rs::{silence_for, check_duration, MAX_FAULT}`,
  `node/transport.rs::FaultGate::try_silence`, `bin/ctl.rs` (unchanged; prints the error).
- **How we check it.** `faults::tests` (duration limits, atomic re-silence),
  `tests/cluster.rs::ctl_silence_closes_the_gate_rejects_overlaps_and_restores_itself`; E3 and FT-n
  runs use these events.
- **Report notes.** Team-built mechanism (no library). Worth one sentence: faults are injected at
  our own network endpoint and are strictly one at a time, so the measured recovery of each fault
  is not mixed with another.

### D75 · Rejoin gate: serving off and elections off until current

- **Date:** 2026-11-01 · **Part:** D (with A for the Raft hooks) · **Status:** Implemented ·
  **Decided by:** Aabed, on the agent's recommendation; Raft hooks need Part A's review
- **Context.** D46 says a returning node "learns the leader's commit index" and serves only once it
  has applied that far, but openraft 0.9.25 does not expose the leader's commit index to a follower
  (its metrics have `last_applied` and `last_log_index`, no `committed`). D5 (PreVote) was open:
  without PreVote a silent node keeps raising its term and deposes the leader when it returns.
  D46 also left open when `serving` goes false and what happens if no leader is reachable.
- **Options considered.**
  1. *Target: `leader_commit` of the first AppendEntries accepted after the silence*, published by
     `raft/network.rs::serve` on a `tokio::sync::watch` exposed by `Consensus::leader_commit`. No
     wire change, follows the Phase 1 design ("note leaderCommit from the first AppendEntries").
  2. *Target: ask the leader with a new peer message.* A `proto.rs` change reviewed by every owner,
     and the leader may be unknown right after a silence. Rejected.
  3. *Target: poll atomics every 10 ms.* Simpler types, but adds up to 10 ms to every measured
     recovery time. Rejected.
  4. *Which AppendEntries count:* all that openraft accepts (`Success`, `PartialSuccess`,
     `Conflict`), versus `Success` only. `HigherVote` means the sender is a stale leader whose
     commit may be too low, so it never counts. `Success` only would delay the target by a round
     trip, because a returning node's first AppendEntries is usually a `Conflict`.
  5. *Elections while not serving:* switch them off with `runtime_config().elect(false)`
     (`Consensus::set_elect`), versus measure first (D5 option 1).
  6. *When `serving` goes false:* when the silence starts, versus when it ends.
  7. *No leader heard from:* keep waiting and warn every 5 s, versus serve anyway after a timeout.
  8. *The returning node is still the leader* (silence shorter than an election timeout, so nobody
     sends it AppendEntries): confirm leadership with a majority (`get_read_log_id`, which returns
     the commit index), versus treating a leader as current at once.
- **Decision.**
  - When the fault gate closes: `serving = false`, `set_elect(false)`.
  - When it opens: log `rejoin_started { applied }`. Wait for the first AppendEntries openraft
    accepts after the restore (anything but `HigherVote`): it means a current leader exists and
    this node now has its term. Then take the target from `Consensus::read_index` (D76), retried
    every Raft heartbeat with a `peer_rpc_timeout_ms` cap. A node that believes it still leads
    asks itself without waiting for an AppendEntries. Wait with openraft's
    `applied_index_at_least` (no polling).
  - Superseded within this PR: the first version took the target straight from that
    AppendEntries' `leader_commit`. openraft fills that field from the replication task's own
    copy of the commit index (`replication/mod.rs:443`), which can lag, so it is only a lower
    bound. See D76.
  - Then `set_elect(true)`, `serving = true`, log `rejoined { leader_commit, applied,
    recovery_us }`. `recovery_us` runs from the restore to here on this node's `Instant` clock.
  - No target yet, or still applying: keep waiting, `warn!` every 5 s. Silenced again while
    catching up: abandon this catch-up, stay not serving, start over at the next restore.
  - `FaultGate` gains a `watch` channel (`subscribe`) next to its atomic, so the rejoin task wakes
    exactly when a fault starts or ends.
- **Why.** Option 1 is the smallest change that gives an exact target without touching the wire
  contract. Switching elections off is what PreVote would buy us, with one line on openraft, and
  the FT-6 result in D5 shows it is needed. Clearing `serving` at silence start keeps the Phase 1
  states (Normal, Silent, Recovering, Normal) and makes `ctl status` truthful during the demo.
  Waiting forever without a leader is the only safe choice: a node that has heard from no current
  leader cannot know it is current, and AGENTS.md forbids serving before that. Confirming an old
  leader with a majority avoids both a deadlock (nobody sends it AppendEntries) and a stale
  ex-leader answering from old state.
- **Consequences.** `Consensus` gains five methods (`leader_commit`, `read_index`,
  `read_index_local`, `wait_applied`, `set_elect`) and `network::serve` one parameter: Part A
  files, so Part A reviews. With two nodes down the survivor never serves again until one returns: this is the
  Phase 1 two-failure gap made visible, not hidden. Votes and log entries are still accepted while
  not serving, so a silent follower's return never costs the majority.
- **Where.** `node/rejoin.rs::{spawn, catch_up, learn_target, is_current}`,
  `raft/mod.rs::Consensus`, `raft/network.rs::serve`, `node/transport.rs::FaultGate::subscribe`,
  `metrics.rs::Event::{RejoinStarted, Rejoined}`.
- **How we check it.** `rejoin::tests` (is_current); `tests/rejoin.rs`: FT-4
  (`a_returning_follower_serves_only_after_catching_up`), FT-6
  (`a_returning_follower_causes_no_election`, which fails without `set_elect(false)`), a short
  leader silence, and a replaced leader rejoining as follower. E4 reads `rejoin_started` and
  `rejoined` events. FT-4 and the replaced-leader test drive the gate by hand
  (`gate.silence()`/`restore()`) so every commit lands inside the silence: with a timed
  `silence_for` of 1.5 s, electing a leader plus the commits sometimes took longer on a debug
  build and the test raced. 60 runs in a row green after that change.
- **Report notes.** Library-provided: log replay, the AppendEntries consistency check, the
  `elect` switch and the leadership check (openraft). Team-built: the gate itself, the choice of
  target, the election switch timing and the recovery measurement. The FT-6 mutation result (3/3
  elections without the switch, 0 with it) is a good figure for the report.

### D76 · `ReadIndex` peer message: the catch-up target from the leader

- **Date:** 2026-11-01 · **Part:** D, A (contract change) · **Status:** Implemented ·
  **Decided by:** Aabed, on the agent's recommendation; needs Part A's review (`proto.rs`,
  `raft/`, `node/mod.rs`)
- **Context.** The rejoin gate (D75) needs the leader's commit index as a target. The first
  version read it from the `leader_commit` of the first AppendEntries after the silence. openraft
  0.9.25 fills that field from the replication task's own copy of the commit index
  (`replication/mod.rs:443`), which is updated by messages and can lag behind the leader's real
  commit index, so it is only a lower bound. A target that is too low lets a node serve before it
  is current. (While testing, a returning node served with 9 of 10 results. That failure turned
  out to be a test race, see D75, but the lag is real in openraft's code, so the fix stands.)
- **Options considered.**
  1. *Ask the leader (Raft ReadIndex, Ongaro thesis §6.4).* A new peer message; the leader runs
     openraft's `get_read_log_id`, which confirms with a majority that it still leads before it
     answers. Exact and linearizable. Costs a `proto.rs` change.
  2. *Settle window.* Keep the AppendEntries rule but take the maximum `leader_commit` over two
     more heartbeats (~100 ms). No wire change, but a heuristic, and it adds ~100 ms to every
     recovery time.
  3. *Accept and document.* Safety still holds through commit-then-reply and first-commit-wins,
     but it breaks "serves nothing until current" in AGENTS.md. Rejected.
  4. *Message shape:* `ReadIndex` reusing `ProposeError::NotLeader { leader }`, versus a
     `CommitQuery` with its own error enum. Reuse keeps the contract small and makes the retry
     identical to proposal forwarding (D26).
- **Decision.** `PeerBody::ReadIndex` (no fields; the envelope carries sender and term), answered
  with `PeerReply::ReadIndex(Result<Option<u64>, ProposeError>)`: the commit index (`None` if
  nothing is committed yet), `NotLeader { leader }` with a hint, or `Timeout` if the leader could
  not reach a majority. `Consensus::read_index` asks locally first and forwards once to the
  hinted leader; `read_index_local` serves incoming queries. The returning node sends it only
  after an AppendEntries has brought its term up to date, because the leader drops non-Raft
  messages from an older term (D21). Counted as `read_index` in the message counters (E5).
- **Why.** It is the textbook way for a node to learn the commit index safely, it is exact, and it
  is short to explain: "the returning node asks the leader what is committed; the leader checks
  with a majority that it is still the leader, then answers". The confirmation step is what keeps
  a stale ex-leader from giving an old answer.
- **Consequences.** One extra round trip plus one leader heartbeat round per rejoin (a few ms on a
  LAN), included in `recovery_us`. Every owner whose code sends or receives peer messages must
  review: A (`raft/mod.rs`, `raft/network.rs`, `node/mod.rs`), and the metrics owners for the new
  `read_index` counter.
- **Where.** `proto.rs::{PeerBody::ReadIndex, PeerReply::ReadIndex}`,
  `raft/mod.rs::{Consensus::read_index, read_index_local, RaftNode::forward_read_index}`,
  `node/mod.rs::on_peer_message`, `node/rejoin.rs::learn_target`, `metrics.rs::MsgKind::ReadIndex`.
- **How we check it.** `tests/rejoin.rs` (all four tests take their target through it), E5 counts.
- **Report notes.** Library-provided: the majority check (`get_read_log_id`). Team-built: the
  message, its forwarding, and using it as the recovery target. Cite the ReadIndex section of
  Ongaro's thesis.
### D77 · Fault injection on/off: `faults.enabled`, `--faults` override

- **Date:** 2026-11-01 · **Part:** C (config key in A's `config.rs`) · **Status:** Implemented ·
  **Decided by:** Aabed, on the agent's recommendation, at the team's request ("enable or disable
  the fault injection through a flag" before launch)
- **Context.** Experiments need a no-fault baseline, and teammates want to run the cluster
  without any chance of injected faults. Until now nothing could switch fault injection off.
- **Options considered.**
  1. *Where:* a config key only (always the same on all six machines, but editing the file on
     every machine to change it); a node CLI flag only (quick, but three nodes can be started
     differently by mistake); or both, the CLI overriding the config for one run.
  2. *What "off" covers:* the schedule and `ctl silence` both, or the schedule only (manual faults
     kept for demos).
  3. *Default:* on (today's behaviour) or off (safer if forgotten, but every fault run and test
     must turn it on).
- **Decision.** Both: `[faults] enabled = true|false` in the shared TOML (`FaultConfig`, default
  `true`), and `node --faults on|off` overriding it for that process. Off means no faults at all:
  `faults::spawn_schedule` returns without starting, and `faults::silence_for` answers
  `Error("fault injection is disabled ...")`, so `ctl silence` prints that error and the gate stays
  open. The node logs "fault injection on/off" at startup.
- **Why.** The config key keeps the six machines consistent by default (AGENTS.md: one config file
  everywhere); the CLI override covers a quick one-off run without editing files. "Disable fault
  injection" plainly means no faults, which is what a clean baseline needs. Default on keeps every
  existing test, script and demo unchanged.
- **Consequences.** A node started with `--faults off` in a cluster that otherwise has faults on
  refuses orders aimed at it; once the schedule exists (D12), it treats the refusal as a skipped
  round. The setting is per node, not replicated, so it is checked on the victim, never trusted
  from the order.
- **Where.** `config.rs::FaultConfig`, `bin/node.rs` (`--faults`), `node/faults.rs::{silence_for,
  spawn_schedule}`, `config/local.toml`, `config/lab.example.toml`, README.
- **How we check it.** `config::tests::{defaults_fill_missing_sections, faults_can_be_switched_off}`,
  `tests/cluster.rs::ctl_silence_is_refused_when_fault_injection_is_disabled`.
- **Report notes.** Mention it in the experiment method: baseline cells run with
  `faults.enabled = false`.

### D78 · The fault schedule: `FaultOrder`, timings, seeding, one fault at a time

- **Date:** 2026-11-01 · **Part:** C (contract change; config keys in A's `config.rs`) ·
  **Status:** Implemented · **Decided by:** Aabed, on the agent's recommendation
- **Context.** D12 says the Raft leader runs the fault schedule: every 30–60 s it picks a victim
  uniformly from all three nodes, itself included, and orders it silent for 1–20 s, tagging the
  order with its term. It left the message, the timings' home, the randomness, what a recovering
  victim does, and how to keep faults from overlapping undecided.
- **Options considered.**
  1. *Message:* `PeerBody::FaultOrder { ms }` answered by `PeerReply::FaultOrder(Result<(),
     String>)`, versus answering with plain `Ack`. `Ack` hides refusals, so E3 could not tell an
     obeyed order from a refused one.
  2. *Timings:* config keys under `[faults]`, versus constants in `faults.rs`. Constants make a
     test take minutes and cannot be tuned per experiment.
  3. *Randomness:* `StdRng` seeded from `faults.seed` mixed with the leader's term (random seed,
     logged, if unset), versus an unseeded `thread_rng` whose failing runs cannot be repeated.
  4. *Recovering victim:* refuse, versus accept. Accepting stacks a fault on an unfinished recovery
     (two overlapping failures, which the design does not handle) and spoils E4's numbers.
  5. *Interval start:* from the end of the previous silence (and from becoming leader), versus a
     fixed period that lets a 20 s silence run into the next fault.
  6. *Events:* `fault_ordered` and `fault_refused`, versus `fault_ordered` only.
  7. *How the victim's "ok" reaches the leader* (found while testing, see below): let that one
     reply through the closed gate; close the gate a moment after replying (needs a delay, racy);
     or treat a timeout as obeyed (an unreachable victim and an obedient one look the same).
     Chose the first.
- **Decision.**
  - `[faults]` gains `interval_min_ms = 30000`, `interval_max_ms = 60000`, `silence_min_ms =
    1000`, `silence_max_ms = 20000` and an optional `seed`. Validation requires
    `0 < min <= max` for both ranges, `silence_max_ms <= 20000`, and
    **`interval_min_ms >= silence_max_ms`**.
  - Every node runs `faults::spawn_schedule` (unless `faults.enabled = false`, D77); it acts only
    while it is the serving Raft leader. Per term it seeds `StdRng` from `seed XOR term × golden
    ratio constant`, then repeats: wait a random interval (checking leadership every Raft
    heartbeat; losing it ends the term's rounds), draw a victim uniformly from all node IDs and a
    random silence, log `fault_ordered { victim, ms, term }`, deliver the order, and on success
    sleep the silence out before the next wait. A refusal logs `fault_refused { victim, reason }`
    and starts the next wait at once.
  - Delivery: to itself through `faults::on_order`, to a peer as `PeerBody::FaultOrder { ms }` in
    an envelope with the leader's term. A victim with a newer term answers `StaleTerm` (term rule,
    D21). `on_order` refuses with "recovering" while the victim is not serving, and otherwise
    calls `silence_for`, which applies D74 and D77.
  - The `FaultOrder(Ok)` reply is sent even though obeying it has just closed the victim's gate
    (`transport.rs`, amends D22): the victim's last word.
- **Why.** Faults must be random, one at a time, at most 20 s, and must also hit the leader
  (FT-2, FT-8). One leader means one schedule and no extra election. A seed makes a failing run
  repeatable. The overlap rule: a leader that silences itself cannot tell its successor, and the
  successor starts its own wait when it takes over; requiring the shortest wait to be at least the
  longest silence means the successor's first order always comes after the old silence has ended.
  The defaults (30 s against 20 s) meet it with room to spare.
- **Found while testing.** The first version dropped the victim's "ok": the victim closed its
  gate inside the handler and the transport then dropped the reply like any other. Every obeyed
  remote order was logged as `fault_refused ... unreachable` after a 500 ms timeout, and the leader
  skipped waiting the silence out. The 500 ms timeout happened to keep faults apart, which is why
  a first mutation test (removing the wait-out) did not fail. Fixed by option 7; covered by
  `an_obeyed_order_is_acknowledged`.
- **Consequences.** `proto.rs`, `node/mod.rs`, `raft/network.rs`, `metrics.rs` and `config.rs`
  change, so A, the transport owner and the metrics owners review. No faults while there is no
  leader (one election, a few hundred ms). A faster schedule for experiments must keep
  `interval_min_ms >= silence_max_ms`. The doctor asked for peers with no coordinator; here the
  Raft leader picks the victims, which the team should be ready to justify (one schedule means one
  fault at a time without a second election) or change.
- **Where.** `node/faults.rs::{spawn_schedule, run_schedule, draw, order, on_order}`,
  `proto.rs::{PeerBody::FaultOrder, PeerReply::FaultOrder}`, `node/mod.rs::on_peer_message`,
  `node/transport.rs::peer_connection`, `config.rs::FaultConfig`, `metrics.rs::{Event::FaultOrdered,
  Event::FaultRefused, MsgKind::FaultOrder}`, `config/*.toml`.
- **How we check it.** `faults::tests` (ranges, every node drawn, same seed gives the same
  schedule); `config::tests::rejects_bad_configs`; `tests/faults.rs`: faults happen and never two
  nodes at once, an old-term order is dropped, an obeyed order is acknowledged, no faults when
  disabled. Sensitivity check: with the wait-out and the validation rule both removed and
  overlapping timings, the one-at-a-time test failed 3 of 5 runs. FT-8 (long run under load) needs
  the load generator.
- **Report notes.** Team-built entirely (no library). The "last word" bug is a good example for
  the report of why every message needs an explicit reply and of testing the test.

### D79 · Peer heartbeat detector: what counts as heard, pausing while silenced, events

- **Date:** 2026-11-01 · **Part:** C · **Status:** Implemented · **Decided by:** Aabed, on the
  agent's recommendation (also confirms D41)
- **Context.** D41 put a peer-to-peer heartbeat failure detector in the template, waiting for the
  team to confirm it. Building it needed a few more choices: the cadence, what proves a peer is
  alive, what a silenced node's own detector does, what is logged, and what Part B gets to use.
- **Options considered.**
  1. *Build it or rely on Raft only.* Raft heartbeats only go from the leader to followers, so
     takeover of a silent non-leader winner could then only use a service-time timer (D42
     option 2). Built.
  2. *Cadence:* keep D40 (50 ms × 3 = 150 ms), versus 200 ms × 3 = 600 ms. Planning runs on the
     worker pool, off the async runtime, so heartbeats should not starve under load. E3 measures
     false suspicions; the values stay tunable in `[timers]`.
  3. *Heard from:* an incoming heartbeat *or* any answer to ours (`Ack` or `StaleTerm`), versus
     incoming heartbeats only. The term rule drops heartbeats from a node with an old term (a node
     just back from silence), so "incoming only" would keep it suspected longer than it is down.
  4. *Own silence:* pause evaluation while this node is silenced and start every peer's clock fresh
     on restore, versus evaluating anyway (each fault would log two bogus suspicions from the
     silent node's point of view).
  5. *Events:* `peer_suspected` and `peer_trusted` on changes, versus `peer_suspected` only.
  6. *For Part B:* `is_suspected(peer)` and `suspected()` on `ctx.suspicion`, versus also adding the
     list to `ctl status` (a `NodeStatus` change in `proto.rs`). No contract change now.
- **Decision.** Every `peer_heartbeat_ms` each node sends `PeerBody::Heartbeat` to every peer at
  once, each call capped at one interval, and records every peer that answered with anything.
  Incoming heartbeats are still recorded by `node/mod.rs`. `Suspicion::update(now)` then marks a
  peer suspected once nothing was heard for `takeover_missed_heartbeats × peer_heartbeat_ms`, and
  trusted again once it is heard. Suspicions change only in `update`, once per interval, and each
  change logs `peer_suspected { peer, silent_ms }` or `peer_trusted { peer, suspected_ms }`. While
  the node's own gate is closed, the loop skips everything; on restore it resets every peer's clock
  to now. A peer never heard of is never suspected; all peers are reset at start. `is_suspected`
  and `suspected` read the state of the last update.
- **Why.** Takeover needs every node to see every other node, and E3 needs a detection time
  (`silenced` on the victim to `peer_suspected` on the others). Counting replies makes each node
  learn from both directions, so one lost message does not cause a suspicion. A node that is cut
  off cannot judge others, so it does not try.
- **Consequences.** 6 heartbeats and 6 replies per 50 ms on three nodes (about 240 messages a
  second), counted as `peer_heartbeat` in E5. Like every timeout detector it can suspect a slow
  node wrongly; that costs at most duplicate work in takeover (D27), never two results. Takeover
  itself (D42) stays open for B and C. `Suspicion`'s API changed from the template:
  `is_suspected(peer, now)` became `is_suspected(peer)` plus `update(now)`, `reset` and
  `suspected()`; nothing called the old one.
- **Where.** `node/heartbeat.rs::{Suspicion, Change, spawn}`, `metrics.rs::Event::{PeerSuspected,
  PeerTrusted}`.
- **How we check it.** `heartbeat::tests` (unknown peer, threshold, trust, clock never moves back,
  reset); `tests/heartbeat.rs`: a healthy cluster suspects nobody for 1 s; a silenced node is
  suspected by both others after at least 150 ms, suspects nobody itself, and is trusted again
  after restore. 30 runs in a row green. E3 (detection time, false suspicions), E5 (overhead).
- **Report notes.** Team-built; cite failure detectors (Chandra and Toueg): it is eventually
  accurate at best, which is why safety never depends on it.

### D110 · Takeover: stand by after a lost round, then plan if the winner is silent

- **Date:** 2026-11-01 · **Part:** C, B · **Status:** Implemented · **Decided by:** Aabed, on the
  agent's recommendation, with Ahmed Abdeen (Part B) on the trigger (D42).
- **Context.** With bidding, a winner that goes silent after its round closes left the request to
  the client's retry (`client_retry_ms`, 2 s). Part B's hooks (D107) give each closed round's
  ranking and the winner's bid; Part C's detector (D79) says who is silent.
- **Options considered.**
  1. *Where it runs:* a `stand_by` step after a lost round, in the same request task, versus a
     separate task that scans all rounds. Standing by in the request task keeps the client's reply
     channel, so the node that takes over can answer without a retry.
  2. *Stagger:* `peer_heartbeat_ms × (rank − 1)` (the runner-up acts at once, the next one heartbeat
     later), versus Ahmed's `50 ms × rank`. Both keep the same 50 ms gap between ranks; the first
     takes over 50 ms sooner.
  3. *How long to stand by:* until `client_retry_ms`, when the client's retry opens a new round,
     versus until the round is forgotten.
- **Decision.** A node that lost the round reads `ranked_bids`, then every
  `peer_heartbeat_ms / 5` checks: stop if a result is committed or `client_retry_ms` has passed;
  do nothing while not serving; otherwise compute `bidding::executor` over the ranking, counting
  suspected nodes and (after the backstop) the winner as out. If the answer is this node for
  `peer_heartbeat_ms × (rank − 1)`, it logs `takeover_started { request, attempt, winner, rank,
  reason, waited_us }` and plans as an executor would. The backstop is `max(200 ms, 3 × the
  winner's bid)` for the expected-wait scores and 200 ms for queue-only and CPU-only scores (D42).
  The hook is one line in `node/mod.rs::decide_executor`; `bidding.rs` is unchanged.
- **Why.** Takeover moves the request in about 150 ms instead of 2 s, with no message change. The
  ranking is the same on every node that saw the same bids, so usually exactly one node acts.
- **Consequences.** A slow but alive winner past the backstop, or a wrong suspicion, makes two
  nodes plan; the first commit wins and the other gets `Duplicate` (D27), so the cost is work, never
  two results. Under leader dispatch nothing changes. `tests/bidding.rs`' test that a silent
  winner is covered by the client's retry is replaced by `tests/takeover.rs`.
- **Where.** `node/takeover.rs::stand_by`, `node/mod.rs::decide_executor`,
  `metrics.rs::Event::TakeoverStarted`.
- **How we check it.** `takeover::tests` (backstop per score); `tests/takeover.rs`: a winner
  silenced after the bids are in is taken over by the runner-up with no client retry; a winner
  whose only worker is held is taken over by the backstop while still trusted; 20 requests on a
  healthy cluster are all planned by their round's winner. E3 (takeover time = `waited_us`, wrong
  takeovers), FT-1.
- **Report notes.** Team-built. The wrong-takeover rate against the backstop is the E3 trade-off
  to report.

---

## 5. Request handling and assignment

### D33 · No leader, no executor (leader dispatch)

- **Date:** 2026-10-20 · **Part:** A · **Status:** Implemented · **Decided by:** A
- **Decision.** Under leader dispatch, a request that arrives while no leader is known is not planned
  by anyone. The client's retry after `client_retry_ms` covers it.
- **Why.** Any node guessing would risk two executors, and only first-commit-wins would save us.
  Elections take 150–300 ms, far below the 2 s retry.
- **Where.** `node/mod.rs::leader_dispatch`.

### D34 · Leader dispatch is the default until bidding lands

- **Date:** 2026-10-20 · **Part:** B · **Status:** Implemented, amended by D103 · **Decided by:** A, B
- **Decision.** `policy.assignment` defaults to `leader-dispatch`. Setting `bidding` today logs a
  warning and falls back to leader dispatch. *(Amended by D103: setting `bidding` now runs bid rounds.
  The default stays until E1 has run.)*
- **Why.** It works as soon as Raft does, so the other parts can test end to end. Switch the default
  to `bidding` once bid rounds work (`TODO(B)` in `config.rs`).

### D35 · Re-plan a stale plan at most three times

- **Date:** 2026-10-20 · **Part:** A, E · **Status:** Implemented · **Decided by:** A
- **Context.** Phase 1 §8.2 said: recompute once from fresh state, then return "retry" to the
  client.
- **Decision.** The executor plans, proposes, and on `Stale` plans again from fresh state, up to
  `MAX_PLAN_ATTEMPTS = 3`. After that it stops without replying (D36); the client's retry starts
  over.
- **Why.** Under contention two attempts are often not enough when several executors chase the same
  few free responders. Three keeps the loop short.
- **Where.** `node/mod.rs::execute`, `MAX_PLAN_ATTEMPTS`.

### D36 · A node that cannot answer stays quiet

- **Date:** 2026-10-20, confirmed 2026-11-01 · **Part:** A, F · **Status:** Implemented, confirmed · **Decided by:** A; confirmed by omar.leithym, on the agent's recommendation
- **Context.** `ReplyBody::Rejected(reason)` exists in the protocol but no node sends it.
- **Decision.** A node that is not serving, is not the executor, finds its worker pool full, cannot
  commit before the propose timeout, or gives up on stale plans sends **nothing**. The client's
  retry covers it.
- **Why.** The client sends to all three nodes; a "no" from one node says nothing about the other
  two. Silence keeps the client logic to "first `Done` wins".
- **Open part.** Whether `Rejected` should be used for "all nodes overloaded" so the client can back
  off. Recommendation: leave it unused until F's load tests show retry storms.
- **Confirmed (2026-11-01).** `Rejected` stays unused. Revisit only if load tests show retry storms.

### D37 · Any serving node answers a retry from its own state

- **Date:** 2026-10-20 · **Part:** A · **Status:** Implemented · **Decided by:** A
- **Decision.** When a request arrives and its result is already in the node's local state, the node
  replies at once with that result, whether or not it executed it.
- **Why.** After a failover the client's retry is answered by whichever node is up, without planning
  or committing again (Phase 1 FT-7: one execution, cached reply).
- **Consequences.** A retry can get up to three identical replies; the client keeps the first.
- **Where.** `node/mod.rs::handle_request`.

### D43 · When the bid window starts

- **Date:** 2026-10-27, settled 2026-10-30 · **Part:** B · **Status:** Implemented (P-2 confirms or
  overturns it) · **Decided by:** Ahmed Abdeen
- **Context.** Phase 1 §6.3: the bid window closes "20 ms after the first bid seen". `bidding.rs`
  and `protocol.md`: "bid round closes `bid_window_ms` after the request arrives".
- **Options considered.**
  1. *From request arrival*: simple and local, but nodes receive the request at slightly different
     times, so their windows are offset.
  2. *From the first bid seen*: windows line up better across nodes; more state, and a lost first
     bid shifts the window.
- **Decision.** Option 1. Each node opens the round when the client request arrives and closes it
  `bid_window_ms` later, by its own monotonic clock. A bid that arrives before the request opens
  the round early and is kept.
- **Why.** No cross-node timing and less state. The only cost of offset windows is a *split
  decision*: two nodes each think they won because they saw different bids. That is harmless
  because the first commit wins (D27); it only wastes planning work, and we measure how often it
  happens.
- **Consequences.** Split decisions are possible and are counted. The window length is set by
  P-2 (D83).
- **Where.** `bidding.rs::BidRound`, `node/mod.rs` (hooks, A to review).
- **How we check it.** P-2 (D83): split-decision rate per window length. **Rule fixed before the
  test:** if the split-decision rate is above 1 % at every window up to 20 ms, switch to option 2
  and log it as a new entry.

### D44 · No `Claim` message; `just_won` in the load score

- **Date:** 2026-10-27, settled 2026-10-30 · **Part:** B · **Status:** Implemented, amended by D85
  (burst test in D83 confirms or overturns it) · **Decided by:** Ahmed Abdeen
- **Context.** Phase 1 had a `Claim` message ("I am working on this") so peers add the request to
  the winner's load at once. `proto.rs` has no `Claim`.
- **Research.** Sending work to the node that *looked* least loaded makes many requests pile onto
  it before its load updates (Mitzenmacher, "How Useful Is Old Information?", IEEE TPDS 2000;
  Dahlin, "Interpreting Stale Load Information", IEEE TPDS 2000). Something must count work
  that has been won but not yet started.
- **Options considered.**
  1. *Claim message*: peers learn of the win for certain, even if they saw different bids; 2 more
     messages per request; a `proto.rs` change every owner must review.
  2. *Local `just_won`*: a node that wins a round counts the win itself, so its next bids already
     include it; no extra message.
- **Decision.** Option 2. A node increases its own `just_won` when it wins a round and decreases it
  when that job enters the worker pool (or the round's request is decided elsewhere). Only a node's
  own `just_won` goes into its own bid; nodes do not keep a count for their peers.
- **Why.** Bidding is deterministic given the bids, so a claim usually tells peers nothing new.
  It only adds information after a split decision, which D43 keeps rare.
- **Consequences.** After a split decision, two nodes count the same request in their own
  `just_won`; it clears when the job starts or the request commits elsewhere.
- **Where.** `bidding.rs::LoadInputs::just_won`, `BidRound`.
- **How we check it.** Burst test in D83. **Rule fixed before the test:** keep option 2 if turning
  `just_won` on lowers the longest winning streak or the largest queue difference between nodes
  under bursts. Add a `Claim` message (new entry, `proto.rs` change) only if pile-ups remain with
  `just_won` on.

### D45 · Load score formula and tie-break

- **Date:** 2026-10-20, settled 2026-10-30 · **Part:** B · **Status:** Implemented (starting value;
  E6 decides the CPU part) · **Decided by:** Ahmed Abdeen
- **Research.**
  - Indices based on queue length predicted performance better than utilisation indices such as
    CPU %, and a smoothed queue length beat an instantaneous one (Ferrari and Zhou, UC Berkeley
    UCB/CSD-87-353, 1987).
  - Of six single load descriptors, run-queue length was best; combining it with another
    descriptor did not improve on it (Kunz, IEEE TSE 17(7), 1991).
  - Simple policies using little state get most of the benefit (Eager, Lazowska and Zahorjan,
    IEEE TSE 1986).
- **Options considered.** Expected wait without CPU; expected wait plus `w × cpu_permille`;
  queue length only; CPU only.
- **Decision.** Starting formula, integers only:
  `score = (queued + running + just_won + 1) × recent_service_us ÷ workers`, that is, `w = 0`.
  Lowest wins; ties go to the lower node ID. The ranking is also the takeover order.
- **Why.**
  - The score estimates how long a new job would wait on this node: research's best index
    (queue length) turned into time.
  - Outside CPU load (LB-2's busy loop, `ctl skew`) makes planning slower, so
    `recent_service_us`, and with it the score, already rises without a CPU term.
  - The `+1` makes an idle node with a slow planner lose to an idle node with a fast one.
- **Known gap.** A node with no recent jobs keeps an old `recent_service_us`, so outside load on an
  *idle* node is invisible until it plans again. This is the one case a CPU term could fix, and E6
  tests it on purpose.
- **Where.** `bidding.rs::load_score`, `pick_winner`, `ranking`.
- **How we check it.** E6 (D83).
- **Report notes.** Phase 1 started with equal weights for the queue and CPU parts; this entry
  starts with no CPU part and lets E6 decide whether to add one.

### D82 · CPU usage read from /proc/stat

- **Date:** 2026-10-30, read method settled 2026-11-01 · **Part:** B · **Status:** Implemented ·
  **Decided by:** Ahmed Abdeen
- **Context.** E6 compares score variants with and without a CPU part, so the node needs
  `cpu_permille`. The repo has no library for it.
- **Options considered.**
  1. Read `/proc/stat` ourselves: about 20 lines, no new dependency; Linux only.
  2. Add the `sysinfo` crate: less code, any OS; a new dependency needs team agreement.
- **Decision.** Option 1. Sample the `cpu` line every 100 ms and compute busy permille from the
  difference between samples. On a system without `/proc/stat`, report 0 and log one warning.
  Every read runs on a helper thread (`spawn_blocking`), never directly on the async runtime.
- **Why.** The deployment target is Ubuntu only (AGENTS.md), and it adds no dependency. A first
  draft also allowed a short direct read; that was removed because AGENTS.md forbids blocking file
  I/O in async code, and a stalled runtime thread could disturb the 20 ms bid window.
- **Consequences.** The sampler is I/O, so it lives in `node/`, never in `bidding.rs`, which stays
  pure.
- **Where.** A small sampler in `node/` (A to review); the value is passed into
  `bidding::LoadInputs::cpu_permille`.

### D83 · Part B tests: variants, order and decision rules

- **Date:** 2026-10-30, run count settled 2026-11-01 · **Part:** B · **Status:** Accepted ·
  **Decided by:** Ahmed Abdeen
- **Context.** Every Part B choice must be backed by research and by a test whose rule is written
  before the run, so results cannot be picked afterwards.
- **Order.** Each test fixes a value the next one uses; every other setting stays at its current
  value while one is tested.

| Order | Test | Varies | Measures | Rule fixed before the test |
|---|---|---|---|---|
| 1 | P-2 | `bid_window_ms` = 5, 10, 20, 50 (after measuring LAN RTT) | split-decision rate, p99 latency | Smallest window with split decisions under 1 % |
| 2 | E6 | score: expected wait (w = 0); expected wait + CPU (w > 0); queue only; CPU only. Two skew cases: (a) `ctl skew` on one busy node, (b) outside busy loop on one node at low load | p50/p99 latency, share per node, fairness index | Lowest p99 at high load; if 95 % confidence intervals overlap, the variant with fewer parts wins. A CPU part is adopted only if it wins case (b) without losing case (a) |
| 3 | Burst | `just_won` on / off, requests in bursts | longest winning streak, largest queue difference, p99 | Rule in D44 |
| 4 | E1 | users × policy (bidding, leader dispatch) | latency percentiles, throughput, fairness, client gap during a leader failure | Keep bidding if p99 is not worse at normal load and the client gap during a leader failure is smaller |
| 5 | LB-2 | one overloaded node, final settings | share per node | Overloaded node's share falls below one third |

- **Method.** At least 5 runs per cell for every test above for now, 30 s warm-up dropped
  (AGENTS.md). Tuning runs (P-2, E6, burst) run at a medium and a high load level. Final settings
  are measured on the lab machines.
- **Run count.** Five is Part B's choice for its own tests, not a team rule: the team-wide count
  stays open in D63. E1 is shared with F, so F reviews this. If D63 settles on more, E1 and LB-2
  follow it.
- **Recording.** Each result, including losing variants, is added to the matching entry as
  **Result:** with the numbers, and whether the research expectation held. A result that overturns
  a starting value gets a new entry; the old one is marked `Superseded`.

### D84 · One bid round per request and attempt; `attempt` added to `Bid`

- **Date:** 2026-11-01 · **Part:** B, A · **Status:** Implemented · **Decided by:** Ahmed Abdeen, on
  the agent's recommendation
- **Context.** `Bid { request, score }` has no attempt number. When a client retries a request, for
  example because the winner went silent, the nodes need a fresh round in which the silent node does
  not bid. A round keyed only by `RequestId` would mix the bids of the old and new attempts.
  `ClientRequest` already carries `attempt`.
- **Options considered.**
  1. *Add `attempt: u32` to `Bid` and key rounds by `(request, attempt)`*: exact and small; a
     `proto.rs` change with the reviews that needs.
  2. *Key by `RequestId` only*: a bid arriving more than 10 × `bid_window_ms` after the old round
     closed starts a new round. No message change, but it relies on a timing guess, so a slow network
     or heavy load could mix attempts.
- **Decision.** Option 1. `bidding::RoundKey` is `(RequestId, u32)`. A straggler from an old attempt
  stays in that attempt's closed round and does nothing.
- **Why.** It is exact, and the cost is one integer in one message.
- **Consequences.** `proto.rs`, `docs/protocol.md` and the reviews from the owners who send or receive
  `Bid` come with the wiring PR, so the message and the code that sends it change together. Until
  then only `bidding.rs` knows about attempts.
- **Where.** `bidding.rs::RoundKey`, `BidRounds`; `proto.rs::Bid` in the wiring PR.
- **How we check it.** Unit test `attempts_of_one_request_never_mix`; multi-node silenced-winner test.

### D85 · `just_won` also counts open rounds a node is leading

- **Date:** 2026-11-01 · **Part:** B · **Status:** Implemented (burst test in D83 confirms or overturns
  it) · **Decided by:** Ahmed Abdeen, on the agent's recommendation. Amends D44.
- **Context.** A win only exists once a round closes, 20 ms after it opens. In a burst, every round
  opens inside that window, so counting only wins not yet queued (D44) leaves the node looking idle:
  it bids the same low score in all of them and wins them all. This is the herding of Mitzenmacher
  (2000) and Dahlin (2000).
- **Options considered.**
  1. *Wins not yet queued, plus open rounds this node is leading on the bids received so far*: uses
     what the node already knows, no new message. A little more logic. It helps only once the other
     bids have arrived, usually within a millisecond on a LAN, so requests arriving at the same
     instant can still pile up.
  2. *Wins not yet queued only*: simplest, but a burst inside one window can all land on one node.
  3. *Break near-ties with a per-request hash* (rendezvous hashing; Thaler and Ravishankar 1998):
     spreads a burst, but changes the tie-break settled in D45 and needs a definition of "near".
- **Decision.** Option 1. A node is leading a round if it has bid and its (score, ID) is the best so
  far. A leading round stops counting when it closes, and becomes a win if the node still leads. The
  count is read before the node adds its own bid, so a round never counts itself. A win stops counting
  when the job reaches the worker pool or is refused, when the request is decided elsewhere, or when
  the round is forgotten, so the count cannot stay raised by a job nobody reported.
- **Why.** It closes the gap the burst test is about, without a new message.
- **Consequences.** `bidding::JustWonMode` has three settings (off, wins only, wins and leading),
  which the burst test compares. D83 said on/off; the middle setting is added so the test can show
  whether counting leading rounds earns its extra code.
- **Where.** `bidding.rs::BidRounds::just_won`, `JustWonMode`.
- **How we check it.** Unit test `counting_leading_rounds_raises_each_bid_in_a_burst`; burst test in
  D83, under the rule in D44.

### D86 · Planning time: running average with gain 1/8, starts at 1000 µs

- **Date:** 2026-11-01 · **Part:** B · **Status:** Implemented, amended by D108 (starting value) · **Decided by:** Ahmed Abdeen, on the
  agent's recommendation
- **Context.** The score needs `recent_service_us`, and `node/workers.rs` does not provide it (D73).
  Single jobs vary, and one slow job should not make a node look slow from then on. A node that has
  planned nothing has no value yet.
- **Options considered (averaging).**
  1. *Running average*: each job moves the average 1/8 of the way towards it. One stored number,
     whole-number arithmetic.
  2. *Mean of the last N jobs*: easy to explain, but stores N times and needs a choice of N.
  3. *Last job only*: simplest, but one slow job makes the node look slow until its next job.
- **Options considered (starting value).**
  1. *Config key, 1000 µs, replaced by the first real job*: all nodes start equal.
  2. *The same value as a constant*: one less key, but changing it needs a rebuild.
  3. *Zero*: no key, but the score multiplies by planning time, so a node with no finished job scores
     0 however long its queue, and a burst at startup can all land on it.
- **Decision.** Running average with gain 1/8 and option 1 for the start:
  `policy.initial_service_us = 1000`. The first job replaces the starting value. Shifting truncates,
  so the average settles within 7 µs of a steady input.
- **Why.** Research favours a smoothed load index (Ferrari and Zhou 1987). A larger divisor is steadier
  but slower to notice a real slowdown: from 10 ms, one 50 ms job moves the average to 30 ms at 1/2, 15
  ms at 1/8 and 11 ms at 1/32, while getting within 10 % of a lasting 50 ms takes about 4, 18 and 74
  jobs. 1/8 is the gain of TCP's round-trip estimator (Jacobson 1988).
- **Consequences.** 1/8 is a borrowed starting point, not a value measured for this system, and no
  planned test varies it. 1000 µs is a placeholder that stops mattering after one job. The config key
  and the timing of each job come with the wiring PR.
- **Where.** `bidding.rs::ServiceTime`; `config.rs` in the wiring PR.
- **How we check it.** Unit tests on `ServiceTime`.
- **Report notes.** Say "we started from TCP's well-tested value", not "1/8 is best".

### D87 · The four load score variants

- **Date:** 2026-11-01 · **Part:** B · **Status:** Implemented (E6 compares them) · **Decided by:** Ahmed
  Abdeen, on the agent's recommendation
- **Context.** D45 and D83 name four scores for E6, but "queue only" and "CPU only" were not defined
  precisely, and the 21 October draft of `bidding.rs` had a fixed CPU weight of 10.
- **Options considered.**
  1. *Simple versions*: queue only is `queued + running + just_won`; CPU only is `cpu_permille`. Each
     tests one idea. CPU only will often tie, and ties go to the lower node ID, which is part of what
     the test should show.
  2. *Queue only without `just_won`*: the queue length of the papers, but it ignores recent wins, so
     bursts would blur the comparison with the main formula.
- **Decision.** Option 1. `bidding::ScoreVariant` has `ExpectedWait` (D45's formula, the starting
  choice), `ExpectedWaitCpu { cpu_weight }` (that plus `cpu_weight × cpu_permille`), `QueueOnly` and
  `CpuOnly`. The fixed weight is gone: the weight is passed in, and its value is set with the config
  keys in the wiring PR.
- **Why.** E6 compares different ideas, so each variant should be easy to explain and to tell apart.
- **Consequences.** All scores saturate instead of overflowing. Scores from different variants are in
  different units, so every node in a run must use the same variant.
- **Where.** `bidding.rs::ScoreVariant`, `load_score`.
- **How we check it.** Hand-computed unit tests for each variant; E6.

### D88 · Bid rounds as pure state in `bidding.rs`; takeover helper left out

- **Date:** 2026-11-01 · **Part:** B · **Status:** Implemented, amended by D103 and D107 · **Decided by:** Ahmed Abdeen, on the
  agent's recommendation
- **Context.** Branch `part-b-bidding` (21 October) already had the score, the ranking, a `BidRound`
  and about 20 tests, written before D43–D45 were settled. It had no collection of rounds, could not
  keep a bid that arrived before the request, and included an `executor` helper for takeover.
- **Options considered.**
  1. *Reuse that code and change what no longer fits*: faster, and working tests carry over.
  2. *Start from scratch*: matches the decisions exactly, but redoes tests that already work.
- **Decision.** Option 1. `bidding::BidRounds` holds every round and the node's wins, as pure state
  with every time passed in. A round waits for its request, runs its window, then closes. A bid for a
  round whose request never reaches this node stays waiting until it is forgotten, and never starts a
  window. A bid exactly at the deadline is late even if the timer has not closed the round yet, so the
  result does not depend on when the timer task runs. The grace period is an argument; its value comes
  with the wiring PR. The `executor` helper is removed until the takeover decision (D42) is made;
  `ranking` stays, because it is also the takeover order.
- **Why.** Pure state can be tested without a network or a clock (AGENTS.md, deterministic core).
- **Consequences.** Nothing calls this code yet; `decide_executor` still falls back to leader dispatch
  until the wiring PR.
- **Where.** `bidding.rs::BidRounds`.
- **How we check it.** Unit tests at the bottom of `bidding.rs`.

### D103 · Each request's task runs its own round, in `node/bids.rs`

- **Date:** 2026-11-01 · **Part:** B, A · **Status:** Implemented, amended by D108 · **Decided by:** Ahmed Abdeen, on
  the agent's recommendation. Amends D34 and D88.
- **Context.** After `bid_window_ms` a node must close the round and learn who won. The code needs a
  home: `node/mod.rs` is Part A's file, and `bidding.rs` must stay free of the network and the clock.
- **Options considered.**
  1. *Each request's own task waits, then closes its own round; code in a new `node/bids.rs`*: no new
     timer system, since each request already has a task, and only hook lines in `node/mod.rs`.
  2. *One timer task closes all due rounds and tells each waiting request*: matches the `close_due`
     written earlier, but adds a channel per request and a task that could stall every round at once.
  3. *Own task, with the code inside `node/mod.rs`*: no new file, but about 100 lines in Part A's file
     and more conflicts with other parts' PRs.
- **Decision.** Option 1. `bids::run_round` opens the round, reads `just_won`, bids, sends, sleeps until
  the deadline and closes. `BidRounds::close(key, now)` replaces `close_due` and `next_deadline`. Closed
  rounds are kept for `timers.client_retry_ms` (2 s, the reason to keep them at all), with no new config
  key; a task clears them every second. Setting `policy.assignment = "bidding"` now really runs rounds;
  the default stays `leader-dispatch` until E1 has run (D34). A winner that finds a result already
  committed does not plan.
- **Why.** It is the least new machinery, and it keeps one owner per file (AGENTS.md, "Hook in, don't
  rewire").
- **Consequences.**
  - **`wins-only` behaves almost like `off`.** A winner's job goes to the worker pool right after the
    round closes, so a win is counted for only microseconds before `queued` takes over. The count of
    open rounds being led does the real work (D85). The burst test still compares all three settings;
    expect those two to look alike.
  - **Fast planning can keep work on one idle node.** See D86: a node that has planned keeps its real
    average, while one that has not keeps `initial_service_us`. If real planning is faster than the
    starting value and requests arrive one at a time, the first winner keeps winning, because idle
    scores decide and the others never plan. On the template planner, which takes almost no time, node
    1 won all 30 sequential requests in a trial run. Under load `queued` and `running` should spread
    work, but that is untested. **Check this once the real planner is on `main`.**
- **Where.** `node/bids.rs`, `bidding.rs::BidRounds::close`, hooks in `node/mod.rs` (A to review).
- **How we check it.** Multi-node tests in `tests/bidding.rs` (next PR).

### D104 · Bids go out in the background; no waiting and no retry

- **Date:** 2026-11-01 · **Part:** B · **Status:** Implemented · **Decided by:** Ahmed Abdeen, on the
  agent's recommendation
- **Context.** A node must send its bid to two peers without letting a slow peer delay a 20 ms window.
- **Options considered.**
  1. *One short task per peer, not waited for, no retry*: a dead peer delays nothing. A lost bid can
     cause a split decision, which P-2 measures.
  2. *Send to both, then wait for acknowledgements*: fewer tasks, but one dead peer delays the request
     by up to `peer_rpc_timeout_ms` (500 ms).
  3. *Background, with one retry*: a retry nearly always lands after the window has closed, so it does
     nothing useful and adds traffic during faults.
- **Decision.** Option 1, with `PeerClient::call` and `peer_rpc_timeout_ms`. Errors are logged at `debug`
  and otherwise ignored. A silenced node's gate already drops the sends.
- **Why.** The window is decided by the clock, not by acknowledgements. A lost bid costs at most
  duplicate work, because the first commit wins (D27).
- **Consequences.** Each task ends within the timeout, so it is not stored (AGENTS.md, "Tasks").
- **Where.** `node/bids.rs::send_bid`.
- **How we check it.** `bids_seen` in `bid_round_closed`; split-decision rate in P-2.

### D105 · `[policy]` keys for scores, `just_won` and the starting planning time

- **Date:** 2026-11-01 · **Part:** B, A · **Status:** Implemented, amended by D108 (default) · **Decided by:** Ahmed Abdeen, on the
  agent's recommendation
- **Context.** E6 and the burst test must switch behaviour without a rebuild.
- **Options considered (`cpu_weight` default).**
  1. *10*: full CPU adds 10 ms to the score, so the CPU part always has an effect. From the 21 October
     draft; not measured.
  2. *0*: an experiment that forgot to set it would compare the main score with itself without anyone
     noticing.
- **Options considered (keeping closed rounds).** A new key, or reuse `client_retry_ms`. Reused: no test
  plans to vary it.
- **Decision.** `score` (`expected-wait`, `expected-wait-cpu`, `queue-only`, `cpu-only`; default
  `expected-wait`), `cpu_weight` (default 10), `just_won` (`off`, `wins-only`, `won-and-leading`; default
  `won-and-leading`), `initial_service_us` (default 1000; 0 is rejected at startup, see D86).
- **Why.** The defaults are the behaviour already decided. `cpu_weight` cannot affect the main score.
- **Consequences.** **10 is a placeholder.** How much it matters depends on planning time, which is not
  measured yet: with CPU at 50 % it adds 5000 to the score, against an idle score of 250 at 1 ms planning
  (CPU almost decides alone), 10,000 at 40 ms (both matter) and 50,000 at 200 ms (CPU barely matters).
  **Before E6, measure planning time from `bid_sent.service_us` and set the weight to match.** The report
  must not present 10 as a tested value.
- **Where.** `config.rs::Policy`, `config/local.toml`, `config/lab.example.toml`, README.
- **How we check it.** Unit tests in `config.rs`; E6.

### D106 · Events `bid_sent` and `bid_round_closed`

- **Date:** 2026-11-01 · **Part:** B · **Status:** Implemented · **Decided by:** Ahmed Abdeen, on the
  agent's recommendation
- **Context.** P-2, E6 and the burst test read their results from the event log, and field names become
  the analysis contract.
- **Options considered.**
  1. *Both events*: two more lines per node per request; shows what made up each score.
  2. *Only `bid_round_closed`*: half the lines, but no view of the score's parts, and no planning time to
     set `cpu_weight` from.
- **Decision.** Option 1. `bid_sent`: `request`, `attempt`, `score`, `queued`, `running`, `just_won`,
  `service_us`, `cpu_permille`. `bid_round_closed`: `request`, `attempt`, `winner`, `i_won`, `bids_seen`,
  `my_score`, `window_us`. The `takeover` event comes with takeover; the `overloaded` event remains Part
  E's (D73).
- **Why.** Two nodes logging `i_won = true` for one request and attempt is a split decision, so it can be
  counted across machines with no shared clock.
- **Consequences.** `window_us` is measured on one node with `Instant` (D56). It runs a little over
  `bid_window_ms`, because timers wake late: 20.4 to 22.9 ms in a local trial.
- **Where.** `metrics.rs::Event`, `node/bids.rs`.
- **How we check it.** P-2, E6 and burst test analysis.

### D107 · Planning time, CPU reading and hooks for takeover

- **Date:** 2026-11-01 · **Part:** B, C · **Status:** Implemented · **Decided by:** Ahmed Abdeen, on the
  agent's recommendation. Amends D88.
- **Context.** `recent_service_us` and `cpu_permille` need real inputs. Takeover needs the ranking after a
  round closes. Part C has offered to build takeover on Part C's detector; the trigger (D42) is still open.
- **Decision.**
  - Each job times itself inside the worker closure in `node/mod.rs::execute`, from when it starts running
    to when it finishes. Queue wait is left out, because `queued` already counts it.
  - The CPU sampler (`node/cpu.rs`) runs whenever the policy is `bidding`, whichever score is in use, so
    every `bid_sent` shows the reading. It does not run under leader dispatch. Where `/proc/stat` cannot be
    read, the reading stays 0 with one warning and the sampler stops (D82).
  - For takeover, `bidding::executor` returns (removed in D88), and `BidRounds::ranked_bids` gives a closed
    round's bids in ranking order, so the winner's bid is available too. `BidState::with_rounds` reads them
    from `node/`. **This adds no takeover behaviour and does not settle D42.**
- **Why.** Timing inside the job measures planning alone. Keeping these hooks in `bidding.rs` means Part C
  does not need to edit Part B's file.
- **Consequences.** Until takeover is built, a silent winner is covered by the client's retry after
  `client_retry_ms`.
- **Where.** `node/mod.rs::execute`, `node/cpu.rs`, `node/bids.rs`, `bidding.rs`.
- **How we check it.** Unit tests in `node/cpu.rs` and `bidding.rs`.

### D108 · Planning time: 1 µs minimum, and a 1 µs starting value

- **Date:** 2026-11-01 · **Part:** B · **Status:** Implemented · **Decided by:** Ahmed Abdeen, on the
  agent's recommendation. Amends D86, D103 and D105; D86 had chosen a 1000 µs start.
- **Context.** In a three-node trial with the template planner, node 1 won all 30 requests sent one at a
  time. The event logs showed two separate causes.
  1. A plan under 1 µs rounds down to 0 µs. The score multiplies by planning time, so node 1 scored 0
     however long its queue was.
  2. With a minimum of 1 µs alone, a full node still won. With 2 workers and 6 jobs, node 1 scores
     `(6 + 1) × 1 ÷ 2 = 3`, while an untried idle node scores `1 × 1000 ÷ 2 = 500`. The full node's queue
     refuses the job, the client's retry goes the same way, and the untried node never plans, so its 1000
     µs is never replaced. Any plan shorter than about 1 ms gives this, on the real planner too.
- **Options considered (rounding to 0).**
  1. *Count any shorter time as 1 µs*: one line, and the formula is unchanged.
  2. *Change nothing until the real planner is measured*: leaves the bug in place for a demo at
     `plan_work = 0`.
- **Options considered (starting value).**
  1. *1 µs*: an untried node looks fast, so it gets tried and its real time replaces the guess.
  2. *Keep 1000 µs, set 1 in the tests only*: the default settings keep the bug.
  3. *Keep 1000 µs, and a node with a full queue does not bid*: stops the refused-job loop, but a fast node
     still takes all the work until its queue fills.
- **Decision.** `ServiceTime` counts any time under 1 µs as 1 µs, and the default of
  `policy.initial_service_us` becomes 1.
- **Why.** Trying each node once gives every node a real measurement. With 10 ms plans and 4 workers: all
  score 0 and node 1 wins; then node 1 scores 2500, so node 2 wins; then node 3; after that real load and
  real times decide.
- **Consequences.**
  - The first few requests in a burst may go to untried nodes, even if those turn out slower. This should
    last only until each node has planned once.
  - With whole numbers this small, a node with very fast plans scores 0 until it has 3 jobs in progress (at
    4 workers), so the lowest node ID wins until then. At very low load and `plan_work = 0`, node 1 still
    does most of the work. With longer plans this rounding hardly matters.
  - None of this has been measured with the real planner yet (D103's check still stands).
- **Where.** `bidding.rs::ServiceTime`, `config.rs::Policy`, `config/local.toml`, `config/lab.example.toml`.
- **How we check it.** Unit tests in `bidding.rs`; `a_busy_node_loses_after_planning_very_fast` in
  `tests/bidding.rs`.
- **Report notes.** A good example of a measurement overturning a starting value: the trial's logs showed
  the problem, and working through the numbers showed that the first fix was not enough.

### D109 · Multi-node bidding tests: what they cover and how

- **Date:** 2026-11-01 · **Part:** B · **Status:** Implemented · **Decided by:** Ahmed Abdeen, on the
  agent's recommendation
- **Context.** Bidding needs tests with real nodes. The load-shift test in the plan uses `ctl skew`, but the
  template planner on `main` ignores `plan_work`, so skew does nothing until Part E's planner merges.
- **Options considered (making a node busy).**
  1. *Fill its workers and queue with jobs the test holds open*: works today, needs no timing luck, and
     tests the queue part of the score directly. Does not test `ctl skew`.
  2. *Use `ctl skew` and wait for the planner*: closest to the real experiment, but this PR would wait on
     Part E's.
- **Decision.** Option 1. Five tests in `tests/bidding.rs`: one result per request; a busy node loses; a node
  that is not serving stays out; a silent winner is covered by the client's retry; leader dispatch is
  unchanged. The silent-winner test uses a 500 ms window and waits on `BidRounds::bids_seen`, so it can
  silence the winner while the round is open without a fixed sleep (AGENTS.md).
- **Why.** These are the safety properties of bidding, and none depends on the real planner.
- **Consequences.** Three gaps remain. A `ctl skew` version is not written yet (Part E's planner has since
  merged, so it can be). No test checks an even
  spread of work, because with plans this fast the lowest node ID does most of it (D108). The silent-winner
  test covers retry only; takeover is not built, and that PR will need to update it.
  Separately, a cluster sometimes fails to elect a leader at startup, before any bidding runs (all three
  nodes stay candidates while the term rises; 6 of 30 fresh starts on one Mac, with no bidding involved).
  This also fails other test files, and it is Part A's area, so these tests are not yet 10 green in a row
  there.
- **Where.** `tests/bidding.rs`.
- **How we check it.** 10 green runs in a row (AGENTS.md).

---

## 6. Concurrency inside a node

### D11 · Single-owner tasks and channels; planner on a thread pool

- **Date:** Phase 1 architecture · **Part:** all · **Status:** Amended by D47 ·
  **Decided by:** team
- **Decision (Phase 1).** One task owns all Raft state; one task owns all bid rounds; other tasks
  talk to them over channels. CPU-heavy planning runs on a separate pool of worker threads.
- **What still holds.** Planning never runs on the async runtime (D48). **What changed:** see D47.

### D47 · Shared node context instead of single-owner tasks

- **Date:** 2026-10-20 · **Part:** A · **Status:** Implemented · **Decided by:** A
- **Context.** openraft runs its own core task and owns Raft state, so the Phase 1 "Raft task" is
  provided by the library.
- **Decision.** The parts of a node share one `Arc<NodeContext>`: the consensus handle, a
  `StateReader` (a read-write lock reached only through `with(|state| ...)` closures, so no guard
  can leak or be held across `.await`), the fault gate, peers, the worker pool, the event log, and
  atomics for `serving` and `plan_work`. Every client request and every incoming peer request runs
  in its own tokio task.
- **Why.** Short critical sections and closures that cannot leak a guard give the safety Phase 1
  wanted from single ownership, with less plumbing. clippy denies `await_holding_lock`.
- **Consequences.** Applying committed entries waits for readers, so `with` closures must stay
  short (documented on `StateReader::with`).
- **Where.** `node/mod.rs::NodeContext`, `raft/store.rs::StateReader`.

### D48 · Bounded worker pool that sheds load

- **Date:** 2026-10-20 · **Part:** E · **Status:** Implemented (2026-10-30, branch
  `part-e-worker-pool`; how in D71–D73) · **Decided by:** team (Phase 1 §10.1), shaped in the
  scaffold; open part settled by Yahia Kilany on the agent's recommendation (2026-10-30)
- **Decision.** At most `workers.threads` (default 4) planning jobs run at once and at most
  `workers.queue_capacity` (default 64) wait. When full, `run` returns `Overloaded` at once, the
  node does not reply (D36), and the client's retry goes elsewhere. Queue length and running jobs
  feed the load score (D45).
- **Why.** CPU work off the runtime keeps 50 ms heartbeats on time when the planner is saturated
  (Phase 1 D11). A bounded queue turns overload into latency and retries instead of memory growth.
- **Open part (settled 2026-10-30).** Phase 1 said `threads = CPU cores − 1`; the code uses a
  fixed 4.
  - Options: keep 4 and set the real value in the lab config; compute `cores − 1` at startup; or
    let `threads = 0` mean "cores − 1".
  - **Decided:** keep 4 as the default. Set `[workers] threads` in the lab config once we know
    the lab machines' core counts, and keep it in each run's saved config.
  - **Why:** the same number on every machine keeps results comparable. The other two options
    need changes in Part A's files.
  - **Consequence:** whoever writes the lab config checks `nproc` and picks at most `cores − 1`.
- **Where.** `node/workers.rs`, `[workers]` in the config.
- **How we check it.** Unit tests in `node/workers.rs` (D71–D72); E2 and LB-2 under load.

### D71 · Worker pool mechanism: admission counter plus a fair semaphore

- **Date:** 2026-10-30 · **Part:** E · **Status:** Implemented · **Decided by:** Yahia Kilany, on
  the agent's recommendation
- **Context.** D48 sets the limits but not how to enforce them. The template started every job
  at once with no limit.
- **Options considered.**
  1. A counter plus a semaphore, running jobs on tokio's blocking threads. Little code, no new
     dependency. Jobs wait in arrival order.
  2. Our own fixed threads reading from a bounded channel. Dedicated threads, but more code. A
     job already in the channel can't be taken back if its request is cancelled.
- **Decision.** Option 1.
  - A counter tracks the jobs in the pool (waiting plus running). If it is already
    `threads + queue_capacity`, `run` returns `Overloaded` at once.
  - Otherwise the job waits for one of `threads` semaphore permits, then runs on a blocking
    thread while holding it.
  - `queue_capacity = 0` is allowed: up to `threads` jobs run and nothing waits.
- **Why.** It meets D48's limits with the least code, and cancelling a waiting job is free.
- **Consequences.** At most `threads` planner jobs run at once, but on tokio's shared blocking
  threads rather than threads of our own.
- **Where.** `src/node/workers.rs`: `WorkerPool`, `Slot`, `Running`.
- **How we check it.** Tests `limits_hold_and_a_full_pool_sheds_at_once` and
  `zero_queue_capacity_runs_only_the_threads`. Under load: E2, LB-2.
- **Report notes.** This is the node's parallelization model: async tasks for network work, a
  bounded pool for planning, and refusing work when full instead of building a backlog.

### D72 · Worker pool: cancelled and panicking jobs

- **Date:** 2026-10-30 · **Part:** E · **Status:** Implemented · **Decided by:** Yahia Kilany, on
  the agent's recommendation
- **Context.** A request task can disappear while its job is in the pool (for example at
  shutdown). A running job can't be stopped halfway. A planner bug can also make a job crash
  (panic).
- **Options considered.**
  - Cancelled job: (a) a waiting job leaves at once, and a running job keeps its slot until it
    finishes; or (b) free the slot as soon as the task disappears.
  - Panic: (c) free the slot and pass the crash on to the request task; or (d) report it as
    `Overloaded`.
- **Decision.** (a) and (c).
- **Why.** With (a), `load()` always shows the CPU really in use, which bidding needs. With (b),
  more than `threads` jobs could run at once. With (c), bugs stay visible. (d) would hide them and
  inflate E2's overload numbers.
- **Consequences.** A cancelled job that is already running still uses CPU until it ends, and its
  result is thrown away. A crashing planner ends only that request, without a reply, and the
  client retries.
- **Where.** `src/node/workers.rs`: `Slot`, `Running`, `WorkerPool::run`.
- **How we check it.** Tests
  `a_dropped_waiter_leaves_the_queue_but_a_running_job_keeps_its_slot` and
  `a_panicking_job_frees_its_slot_and_panics_in_the_caller`.

### D73 · Worker pool load report: queued and running only; no overload event yet

- **Date:** 2026-10-30 · **Part:** E · **Status:** Implemented · **Decided by:** Yahia Kilany
- **Context.** `load()` always returned zero, so bidding couldn't see a busy node.
- **Options considered.**
  - What `load()` reports: (1) `queued` and `running` only; (2) also a count of refused jobs; or
    (3) also the average planning time.
  - Logging a refused job: (a) keep the existing debug line in `node/mod.rs`; or (b) add a
    `warn!` in the pool.
- **Decision.** (1) and (a). `PoolLoad` keeps its fields: `queued`, `running`, `threads`.
- **Why.** (1) is what bidding needs now. How to average the planning time is Part B's choice
  (D45). (b) would print thousands of lines a second under load.
- **Consequences.**
  - **Before E2 runs**, `metrics.rs` (shared) needs an `overloaded` event, or E2 can't count
    refused jobs from the logs.
  - Part B has to get the average planning time for the load score from somewhere else.
  - `queued` and `running` are read one after the other, so one reading can be off by one job.
- **Where.** `src/node/workers.rs`: `WorkerPool::load`, `PoolLoad`.
- **How we check it.** Every worker-pool test checks the exact `queued` and `running` values.

---

## 7. Planner and domain

### D49 · Planner: deterministic, integer cost, CPU knob

- **Date:** 2026-10-20 · **Part:** E · **Status:** Implemented (open part settled 2026-10-30)
  · **Decided by:** Yahia Kilany (formula and open part)
- **Decision.** `planner::plan(incident, free, knob)` chooses a responder from the free ones using
  severity, age, distance, availability and capability (project spec p. 3), breaks ties
  deterministically, and returns an integer `cost` (unit defined in `planner.rs`).
  `CostKnob::extra_work` burns a fixed amount of CPU per plan; `workers.plan_work` sets it for all
  nodes and `ctl skew --node N --work W` sets it on one node at run time.
- **Why.** Deterministic so any node plans the same thing from the same state. The knob makes
  planning CPU-bound on demand, which E2 and LB-2 need: Phase 1 planned an external busy-loop
  process for LB-2; the knob gives a repeatable imbalance without one.
- **Open part (settled 2026-10-30, Yahia Kilany).** Phase 1 §13.3: weighted scoring or a
  matching problem per request?
  - Options: weighted scoring per request; matching many incidents to many responders (Hungarian
    method); scoring plus route search on a campus map.
  - Decision: weighted scoring with straight-line (Manhattan) distance, for now. Rules: D93, D94.
  - Why: the planner sees one incident at a time, so matching would need other open incidents
    (`node/mod.rs`, `state.rs`). Route search was chosen first, then dropped: without obstacles it
    gives the same answer as straight-line distance, and the CPU knob (D96) already gives E2 its
    load.
  - Not used yet: age (D95).

### D50 · Domain model: integer campus, four capabilities, severity 1–5

- **Date:** 2026-10-20 · **Part:** all · **Status:** Implemented (generation: D97, D99)
- **Decision.** `Point { x, y }` in metres as `i32`, Manhattan distance. `Capability` is one of
  `Medical`, `Fire`, `Security`, `Rescue`. `Incident { zone, location, severity (1..=5), needs,
  reported_at_ms }`, where `reported_at_ms` is the client clock. `Responder { id, zone, location,
  capability }`. `Plan { responder: Option<ResponderId>, cost }`. Defaults: 6 zones, 30
  responders, seed 42.
- **Why.** Integers keep replicated state deterministic (D30). Four capabilities cover the project's
  incident examples (fire, medical, security) plus search and rescue.

### D93 · Planner pick rule: exact capability, nearest, cost in metres

- **Date:** 2026-10-30 · **Part:** E · **Status:** Amended by D101 · **Decided by:** Yahia Kilany
- **Context.** D49 settled on weighted scoring. We need the exact rule and the unit of `cost`.
- **Options considered.**
  1. Exact capability match, nearest wins; ties go to the incident's zone, then the lowest ID.
  2. Same, plus a fixed penalty (e.g. +200 m) for a responder in another zone.
  3. Exact match, ties by lowest ID only (zone ignored).
  4. Soft match: any capability, +1000 m for the wrong one.
- **Decision.** Option 1. `cost` = Manhattan distance in metres from the chosen responder to the
  incident.
- **Why.** The spec treats capability as "required" (p. 3). Distance is the clearest score and is
  easy to explain. The zone tie-break uses the zone field without adding a weight to justify.
  Responder IDs are unique, so the result never depends on the order of the free list.
- **Consequences.** Severity and age do not change who is nearest; see D94 and D95.
- **Where.** `planner.rs::plan`, `planner.rs::nearest`.
- **How we check it.** `planner::tests` (D100).
- **Report notes.** One line per factor from spec p. 3: capability (filter), distance (score),
  availability (only free responders are passed in), severity (D94), age (D95).

### D94 · Severe incidents fall back to any capability

- **Date:** 2026-10-30 · **Part:** E · **Status:** Amended by D101 · **Decided by:** Yahia Kilany
- **Context.** The spec wants severity in the decision. With one incident at a time, severity
  cannot change who is nearest. It can change what happens when no match is free.
- **Options considered.**
  1. Severity 4–5 with no free match: send the nearest free responder of any capability.
     Severity 1–3: no responder.
  2. Severity weights the cost (distance × (6 − severity)): the pick never changes, and cost
     stops being metres.
  3. Severity not used; log it as a gap.
- **Decision.** Option 1, with the same tie-breaks as D93. With nobody to send, the plan is
  `responder: None, cost: 0` (`planner::SEVERE = 4`).
- **Why.** Severity then really changes the outcome, and `cost` keeps one meaning.
- **Consequences.** A severe incident can get the wrong kind of help, on purpose: some help beats
  none.
- **Where.** `planner.rs::plan`, `planner::SEVERE`.
- **How we check it.** `planner::tests::no_match_for_a_severe_incident_sends_the_nearest_of_any_capability`,
  `no_match_for_a_minor_incident_gives_no_responder`.

### D95 · Incident age in the planner needs a time from the node

- **Date:** 2026-10-30, settled 2026-11-01 · **Part:** A, E · **Status:** Implemented · **Decided by:** omar.leithym, on the agent's recommendation
- **Context.** The spec wants age in the decision. The planner has no clock (D30) and gets only
  `(incident, free, knob)`. `incident.reported_at_ms` is on the client clock. The node notes the
  arrival time (`started` in `handle_request`) only to measure latency.
- **Options considered.**
  1. Leave age out now; A adds a time argument to `plan()` in `node/mod.rs` later.
  2. Pause Part E until the signature changes.
  3. A one-line change to `node/mod.rs` in the planner branch, reviewed by A.
  4. Age is not a planner factor: it only orders the queue.
- **Decision so far.** Option 1. Which time to pass is still open:
  - Time since this node received the request (`Instant`, one clock; misses network and retry
    time). Recommended.
  - `now − reported_at_ms` (true age, but mixes two machines' clocks).
- **Recommendation for using it.** An old incident gets the same any-capability fallback as a
  severe one (D94).
- **Where.** `node/mod.rs::execute` (A), `planner.rs::plan` (E).
- **Decision.** The node computes `age_ms = now − reported_at_ms` (saturating at 0) and passes it to
  `plan()`, so the planner stays clock-free. At `OLD_AGE_MS = 5000` or more, an incident gets the same
  any-capability fallback as a severe one (D94).
- **Why.** The log's recommended option, time since this node received the request, restarts on
  every retry and would stay near zero, so the fallback would almost never fire. True age survives
  retries. It mixes two machines' clocks, but NTP error of milliseconds is negligible against 5 s.
  D56 restricts *measured durations*; this is a coarse input to a decision. The plan is committed
  through Raft, so nodes cannot diverge even if their clocks differ. The spec requires age.
- **Consequences.** A client clock wrong by seconds changes when the fallback fires, not safety.
- **Where.** `node/mod.rs::execute`, `planner.rs`. **How we check it.** `planner::tests`.

### D96 · CPU knob: fixed hash rounds, no built-in base work

- **Date:** 2026-10-30 · **Part:** E · **Status:** Implemented · **Decided by:** Yahia Kilany
- **Context.** E2 and LB-2 need planning to cost a known, tunable amount of CPU on one node.
- **Options considered.**
  1. One unit = 1,000 rounds of an integer mixing function (about 1 µs).
  2. One unit = one extra scoring pass over the free responders.
  3. One unit = 1 µs of busy-waiting on the clock.
  - Base work at knob 0: none; a fixed base in `planner.rs`; or a higher config default.
- **Decision.** Option 1, using the SplitMix64 finaliser behind `std::hint::black_box`
  (`planner::ROUNDS_PER_UNIT = 1_000`). No base work: `plan_work = 0` is the regular planning
  only, and anything above 0 adds simulated load. Set it with `[workers] plan_work` or at run time
  with `ctl skew --node N --work W`.
- **Why.** A fixed cost per unit does not change with campus size or with how many responders
  are held, so E2 is easy to calibrate. Option 2's load drifts during a run. Option 3 reads the
  clock. Keeping all simulated load in one setting keeps it visible and tunable (spec p. 8).
- **Consequences.** The knob never changes the plan. Calibrate units to milliseconds on the lab
  machines before E2.
- **Where.** `planner.rs::burn`, `CostKnob`, `config.rs::WorkerConfig::plan_work`.
- **How we check it.** `planner::tests::the_knob_adds_work_but_never_changes_the_plan`.

### D97 · Campus layout: zone grid, even round-robin, seeded positions

- **Date:** 2026-10-30 · **Part:** E · **Status:** Implemented · **Decided by:** Yahia Kilany
- **Context.** Every node must build the same responders from `[campus]` (`seed`, `zones`,
  `responders`) (D38).
- **Options considered.**
  1. Zones as 500 m squares in a near-square grid; responders spread round-robin over zones and
     capabilities; position inside the zone from the seed.
  2. Zones in one row (6 zones: 3000 m × 500 m).
  3. Zone, capability and position all random.
- **Decision.** Option 1. Columns = smallest `c` with `c × c ≥ zones` (6 → 3 × 2), filled row by
  row from (0, 0). Responder `i` (0-based) has ID `i + 1`, zone `i mod zones` and capability
  `i mod 4`.
- **Why.** Every zone and capability is covered evenly (counts differ by at most 1), so no seed
  leaves a zone without, say, a Medical responder. A grid keeps zones close enough for the
  fallback in D94 to matter.
- **Consequences.** Only positions depend on the seed. `responders = 0` gives an empty campus,
  and seeding then registers nothing.
- **Where.** `campus.rs::responders`, `ZONE_SIZE_M`.
- **How we check it.** `campus::tests` (D100).

### D98 · Campus RNG: our own SplitMix64

- **Date:** 2026-10-30 · **Part:** E · **Status:** Implemented · **Decided by:** Yahia Kilany
- **Options considered.**
  1. Our own SplitMix64 (about 10 lines).
  2. rand's `StdRng`: may change between rand versions; safe only while `Cargo.lock` pins it.
  3. `rand_chacha::ChaCha8Rng`: stable, but a new dependency.
- **Decision.** Option 1, seeded with `campus.seed`.
- **Why.** Its output is fixed by our code, so the campus never changes under us. No new
  dependency.
- **Where.** `campus.rs::SplitMix64`.

### D99 · Random incidents are uniform

- **Date:** 2026-10-30 · **Part:** E, F · **Status:** Amended by D102 · **Decided by:** Yahia Kilany
- **Options considered.**
  1. Everything uniform.
  2. Skewed severity (e.g. 35/25/20/12/8 %).
  3. A hot zone that gets half of all incidents.
- **Decision.** Option 1. Zone, position inside it, severity 1–5 and needed capability are all
  uniform, drawn from the RNG the caller passes in (the load generator seeds it).
- **Why.** Easy to explain. 40 % of incidents are severity 4–5, so the fallback in D94 gets
  used. A hot zone fits better as a later load-generator option.
- **Where.** `campus.rs::random_incident`.

### D100 · Planner and campus tests

- **Date:** 2026-10-30 · **Part:** E · **Status:** Implemented · **Decided by:** Yahia Kilany
- **Decision.** Unit tests at the bottom of each file:
  - Planner: capability match; nearest wins with cost in metres; tie → own zone → lowest ID; no
    match at severity 1–3 → none; at 4–5 → nearest of any capability; a match still beats a
    nearer non-match; no free responders; same input → same plan in any order; knob work count.
  - Campus: same seed → same campus; different seed → different positions only; IDs 1..=N;
    the 3 × 2 grid; every responder inside its zone; even spread; empty and single-zone campuses;
    incidents in range and covering every value; same RNG seed → same incidents.
- **Why the knob test counts rounds.** `cost` is metres and the knob never changes the plan, so
  the test checks rounds = knob × 1,000 and an unchanged plan. Timing tests can be flaky.
- **How we check it.** `cargo test --lib planner:: campus::`, 10 runs in a row.

### D101 · Planner zone penalty: +200 m for another zone

- **Date:** 2026-11-01 · **Part:** E · **Status:** Implemented · **Decided by:** Yahia Kilany,
  agreed with the team in the PR review
- **Context.** The PR review chose D93 option 2: prefer responders in the incident's own zone.
- **Options considered.**
  - Penalty size: +200 m; +500 m (one zone width); a config key (needs A's `config.rs`).
  - Severe fallback (D94): same score; distance only.
  - `Plan::cost`: metres only; score including the penalty.
  - Ties: own zone, then lowest ID; lowest ID only.
- **Decision.**
  - Score = Manhattan distance + 200 m for a responder in another zone
    (`planner::ZONE_PENALTY_M = 200`). The lowest score wins.
  - Ties go to the incident's zone, then the lowest ID.
  - The severe fallback uses the same score.
  - `cost` stays the true distance in metres, without the penalty.
- **Why.**
  - 200 m is under half a zone, so a much closer neighbour can still win.
  - One scoring rule for both picks is easier to explain and test.
  - `cost` keeps one meaning in logs and the report.
  - A constant needs no change in A's files.
- **Consequences.**
  - A responder in another zone wins only if it is more than 200 m closer. An exact 200 m gap
    goes to the own zone.
  - Changing the penalty means a code change, not a config change.
- **Where.** `planner.rs::score`, `planner.rs::nearest`, `planner::ZONE_PENALTY_M`.
- **How we check it.** `planner::tests::the_own_zone_wins_unless_another_zone_is_more_than_the_penalty_closer`,
  `cost_is_metres_without_the_penalty`, `the_severe_fallback_uses_the_zone_penalty_too`,
  `a_tie_goes_to_the_incident_zone_then_the_lowest_id`.

### D102 · Unseeded random incidents from OS randomness

- **Date:** 2026-11-01 · **Part:** E, F · **Status:** Implemented · **Decided by:** Yahia Kilany,
  agreed with the team in the PR review
- **Context.** The PR review asked for completely random incidents. D99 already draws every
  field uniformly, but from a seeded RNG, so a run repeats.
- **Options considered.**
  1. Add an unseeded helper; keep the seeded one for tests and repeatable runs.
  2. Replace the seeded one; tests can only check ranges.
  3. Pick the point anywhere on campus and derive the zone (same spread on a full grid).
  4. Keep as is.
- **Decision.** Option 1. `campus::unseeded_incident(cfg, now_ms)` draws from
  `rand::thread_rng()` (OS-seeded). `random_incident(rng, ..)` stays.
- **Why.** Every run sees new incidents, and seeded runs are still there when a result must be
  repeated. `rand` is already a dependency.
- **Consequences.**
  - Unseeded runs cannot be replayed. Keep the raw logs of each run.
  - The load generator (F, `bin/loadgen.rs`) still has to choose which one to call. Not changed
    here.
  - Only clients call it. The deterministic core (`state.rs`, `bidding.rs`, `planner.rs`) never
    does, and responders stay seeded (D97).
- **Where.** `campus.rs::unseeded_incident`.
- **How we check it.** `campus::tests::unseeded_incidents_stay_in_range`, `unseeded_batches_differ`.

---

## 8. Clients and load

### D51 · Request IDs and per-machine client IDs

- **Date:** 2026-10-20 · **Part:** F · **Status:** Implemented · **Decided by:** A, F
- **Decision.** `RequestId { client: u64, seq: u64 }`, unique per client process. A retry reuses the
  ID and increments `attempt` (0 for the first send). Each load generator needs its own
  `--client-id`; when omitted it is random.
- **Why.** Uniqueness across three client machines without coordination. The attempt counter lets
  analysis count retries without guessing.
- **Recommendation.** In the lab, always pass `--client-id` explicitly (1, 2, 3); a random ID makes
  runs harder to match up afterwards.

### D52 · Client retry budget

- **Date:** 2026-10-27, settled 2026-11-01 · **Part:** F · **Status:** Implemented, config in this PR, client in `part-f-client` · **Decided by:** omar.leithym, on the agent's recommendation
- **Context.** Phase 1 §6.3: reply timeout 2 s, up to **5** retries with exponential backoff and
  random jitter, a total budget of about 30 s, "longer than the longest silence (20 s) plus an
  election". The config defaults are `client_retry_ms = 2000` and `client_max_retries = 3`, a fixed
  interval, so the budget is about 8 s.
- **Options considered.**
  1. *Keep 3 × 2 s, fixed*: enough for any single fault, because two nodes always remain and an
     election takes under a second; but the Phase 1 claim no longer holds as written.
  2. *Phase 1 rule*: 5 retries, exponential backoff with jitter; avoids retry storms after a
     failover at 3,000 users (Kleppmann ch. 8).
- **Recommendation.** Option 2 for the client (F), and update `config.rs` defaults (A) and
  `protocol.md` in the same PR. Lost requests (FT-1, FT-8) must be 0 either way.
- **Decision.** Option 2. 5 retries, so 6 sends. The waits are 2, 4, 6, 6, 6, 6 s: the wait doubles
  from `client_retry_ms = 2000` up to the new `client_retry_max_ms = 6000`, each with ±20% random
  jitter. Total about 30 s. When the budget runs out `submit` returns an error.
- **Why.** Exactly Phase 1's budget, longer than a 20 s silence plus an election. Jitter avoids a
  synchronised retry burst after a failover. Dr Amr's class notes (Sept 13) also ask for "5 retries
  then if still no response then send error".
- **Consequences.** A failed request takes about 30 s to report. Takeover's `stand_by` and the bid
  round's grace still use the first wait, `client_retry_ms`, as "when the next round starts", which
  is now a lower bound; a stand-by that ends before a later, longer wait simply leaves that attempt
  to the following retry.
- **Where.** `config.rs::Timers`, `client.rs`. **How we check it.** `tests/client.rs`, FT-1, FT-8.

### D53 · One client process, one connection per node, shared by users

- **Date:** 2026-10-20 · **Part:** F · **Status:** Implemented (`client.rs`), retries per D52, connections per D112 · **Decided by:** team; details by omar.leithym, on the agent's recommendation
- **Decision.** `MeshClient` keeps one connection per node, shared by all simulated users in the
  process via `Arc`. `submit` sends the request to every node, waits `client_retry_ms` for the
  first `Done` with that ID, resends with the same ID, and logs `client_sent`, `client_done`,
  `client_gave_up`.
- **Why.** Thousands of users cannot each hold three TCP connections. Matching by request ID makes
  sharing safe.

### D54 · Load generator: closed loop first, open loop as a mode

- **Date:** 2026-10-20, built 2026-11-01 · **Part:** F · **Status:** Implemented (`bin/loadgen.rs`) · **Decided by:** team; open-loop details by omar.leithym, on the agent's recommendation
- **Decision.** `loadgen --users N --duration-s S --think-ms T --seed X --client-id C`. Each user is a
  tokio task in a closed loop: send, wait for the reply or give up, think, repeat. An open-loop mode
  at a fixed rate is planned. Phase 1 used P processes × U users per machine; one process with many
  tasks is the default here.
- **Why.** Closed loop matches "simulated users"; open loop is needed to find saturation without the
  load backing off (coordinated omission). Load is raised by changing `--users`, never the
  application logic.
- **Open loop as built (2026-11-01).** `--rate R` sends R requests per second at evenly spaced
  intended times, each in its own task, ignoring `--users`, with at most 20,000 in flight. Latency in
  the summary counts from the intended time. Rejected: Poisson arrivals (harder to explain, nothing
  we measure needs them) and latency from the actual send (hides queueing in the generator, the very
  reason for open loop). `--warmup-s` leaves the start out of the printed figures; raw events are
  still logged. The summary also counts retried requests and plans with no responder (D111).

---

## 9. Measurement and observability

### D55 · JSON Lines event logs written off the async runtime

- **Date:** 2026-10-20 · **Part:** all · **Status:** Implemented · **Decided by:** team (M0)
- **Context.** Phase 1 planned CSV files via the `csv` crate.
- **Decision.** One JSON object per line: `{"ts_us": <unix µs>, "src": "node-1", "ev": "<event>",
  ...fields}`, to `<metrics.dir>/node-<id>.jsonl` or `client-<id>.jsonl`. A background OS thread
  writes and flushes every 200 ms, so logging never blocks the runtime. Message counts by type are
  lock-free counters; every node logs `messages` and `state_digest` once a second. Field names are
  the analysis contract: stable once a script reads them.
- **Why.** Events have different fields; JSON Lines handles that without a schema per file and is
  easy to read in Python. No new dependency (serde_json was already there).
- **Where.** `src/metrics.rs`, `node/mod.rs::spawn_reporter`.

### D56 · Durations on one clock; latency on the client

- **Date:** 2026-10-20 · **Part:** all · **Status:** Implemented · **Decided by:** team
- **Decision.** `ts_us` is wall-clock time, for ordering and rough alignment only. Every duration
  (`latency_us`) is measured with a monotonic clock (`Instant`) on one machine. Client latency is
  measured on the client. Never subtract `ts_us` values from different machines; cross-machine
  intervals (Phase 1's takeover time) use NTP-synced clocks and must state the sync error.
- **Why.** No shared clock between six machines; NTP error is often milliseconds, the size of what
  we measure.

### D57 · Heartbeat logging: one summary a second, details at debug

- **Date:** 2026-10-21 · **Part:** A · **Status:** Implemented · **Decided by:** A
- **Decision.** Each node logs one line a second under the `heartbeat` target: the leader shows
  heartbeats sent and answered per follower; a follower shows how many it received from its leader.
  A dead peer shows as 0. `RUST_LOG=heartbeat=debug` logs every heartbeat.
- **Why.** At 20 heartbeats per second per follower, a line per heartbeat drowns the log; a summary
  makes a silent node obvious during a demo.
- **Where.** `raft/mod.rs::heartbeat_summary`, `raft/network.rs::HeartbeatStats`.

### D58 · Human logs through `tracing`, measurements through events

- **Date:** 2026-10-20 · **Part:** all · **Status:** Implemented
- **Decision.** `tracing` to stderr for people (`warn!` unexpected but handled, `info!` rare lifecycle
  events, `debug!` per request). Default filter `info,openraft=warn`, overridden by `RUST_LOG`.
  Anything we measure goes to `metrics::Event`, never only to `tracing`.

### D63 · Experiment method: repetitions, warm-up, run length

- **Date:** 2026-10-27, settled 2026-11-02 · **Part:** all · **Status:** Implemented · **Decided by:**
  omar.leithym, on the agent's recommendation
- **Context.** Phase 1 §11.4: 5-minute runs after a 30 s warm-up, at least **10** repetitions per
  configuration with different seeds, load levels 30–3,000 users, 95% confidence intervals.
  AGENTS.md and `experiments/README.md`: at least **5** repetitions, drop a 30 s warm-up.
- **Recommendation.** 10 repetitions as submitted, with 5 as the floor if lab time runs out, and say
  which in the report. Keep each run's config and git commit hash next to its logs.
- **Part B note (2026-11-01).** Part B runs its own tests 5 times for now (D83). That does not
  settle this entry, which stays open for the team.
- **Settled (2026-11-02).** Two profiles. `quick`: 13 cells, 3 runs, 15 s warm-up + 60 s measured + 15 s
  restart, about 1 hour. `full`: 5 runs, 30 s warm-up + 2 min measured, about 8 hours. The user first asked
  for 1 or 2 runs; told that gives no usable confidence interval, they chose this. It amends AGENTS.md's "at
  least 5" and "drop 30 s" for `quick`. Each run's config and git commit are kept next to its logs.
- **Consequences.** `quick` falls short of the Phase 1 document's 10 runs and 5 minutes, and of `full`.
  **The report must state which profile each figure came from, and these limits.** Cells with few
  requests may also fall short of the spec's "tens of thousands of operations".

### D64 · Analysis in Python

- **Date:** 2026-10-20, approved 2026-11-02 · **Part:** all · **Status:** Implemented · **Decided by:**
  the team; approval reported by omar.leithym
- **Decision.** Analysis scripts (percentiles, confidence intervals, plots) are written in Python, with a
  venv and pinned `analysis/requirements.txt` (numpy, scipy, matplotlib),
  in `analysis/`, output to `analysis/out/` (ignored). They are tools, not part of the system.
- **Why.** Plotting and statistics libraries; nobody has to write them in Rust.

### D62 · `ctl` is the operator tool; no separate experiment runner yet

- **Date:** 2026-10-20, confirmed 2026-11-01 · **Part:** C · **Status:** Implemented (D130) · **Decided by:** omar.leithym, on the agent's recommendation
- **Decision.** `ctl status` shows role, term, leader, log index, applied index, digest, result
  count, `serving` and `silenced` for every node. `ctl silence --node N --ms M` and
  `ctl skew --node N --work W` inject faults and imbalance. The Phase 1 "experiment runner" (start
  everything, set load, collect logs) does not exist yet; `scripts/run-local.sh` covers one machine.
- **Recommendation.** A bash runner in `scripts/` that SSHes to the six machines, starts nodes and
  load generators with a run's config, and copies the logs back.
- **Confirmed (2026-11-01).** `ctl` stays the operator tool. The runner is bash over SSH and rsync, in `scripts/`, driven by a hosts file (`shared-experiments` PR).

---

## 10. Configuration and deployment

### D40 · Timer defaults and validation

- **Date:** 2026-10-20 · **Part:** all · **Status:** Implemented · **Decided by:** A, starting from
  Phase 1 §6.3
- **Decision.** Defaults in `config.rs::Timers` (LAN values):

  | Key                         | Default | Basis                                                     |
  |-----------------------------|---------|-----------------------------------------------------------|
  | `raft_heartbeat_ms`         | 50      | Phase 0 survey for a LAN; also the append-entries timeout |
  | `election_timeout_min_ms`   | 150     | Raft paper's suggested range                              |
  | `election_timeout_max_ms`   | 300     | randomised between min and max                            |
  | `peer_heartbeat_ms`         | 50      | same cadence as Raft (D41)                                |
  | `bid_window_ms`             | 20      | ~10× a LAN round trip; tuned in P-2                       |
  | `takeover_missed_heartbeats`| 3       | 150 ms of silence (D42)                                   |
  | `client_retry_ms`           | 2000    | above p99 plus one election                               |
  | `client_max_retries`        | 3       | see D52                                                   |
  | `propose_timeout_ms`        | 3000    | covers an election plus retries (D26)                     |
  | `peer_rpc_timeout_ms`       | 500     | one non-Raft peer call                                    |

  Validation rejects `raft_heartbeat_ms` ≥ `election_timeout_min_ms`, min ≥ max, and zero values.
- **Why.** The heartbeat must be well below the election timeout or followers start elections against
  a healthy leader.
- **Consequences.** Anything that depends on timing must be tried on the lab machines (AGENTS.md).

### D59 · One strict TOML config shared by every process

- **Date:** 2026-10-20 · **Part:** A · **Status:** Implemented · **Decided by:** A
- **Decision.** One TOML file with sections `cluster`, `timers`, `policy`, `workers`, `campus`,
  `metrics`. Every node, client and `ctl` reads the same file. Unknown keys are rejected
  (`deny_unknown_fields`) so a typo fails at startup. Missing sections use defaults. Validation:
  node IDs start at 1 and are unique, no address used twice. `config/lab.toml` is git-ignored;
  `config/lab.example.toml` is the template.
- **Why.** A different node list on one machine breaks Raft membership (D25). A silently ignored typo
  in a timer would invalidate an experiment.
- **Where.** `config.rs`, `config/`.
- **How we check it.** `config::tests::shipped_configs_are_valid`, `rejects_bad_configs`.

### D60 · Deployment target: six Ubuntu machines on one LAN

- **Date:** 2026-10-21 · **Part:** all · **Status:** Implemented (as rules) · **Decided by:** team
- **Decision.** Three cloud nodes and three client machines, Ubuntu, one network. No localhost
  assumptions (every address from the config), one config file copied to all six machines, the same
  commit and toolchain everywhere, nothing shared but the network (each machine writes its own logs,
  collected afterwards), NTP on, ports 7001 and 8001 open (`ufw allow`). Scripts are bash, paths are
  POSIX. Windows builds are a convenience only.
- **Why.** The demo and experiments run there. Loopback hides latency, reordering and firewall
  problems.
- **Where.** AGENTS.md "Deployment target".

### D61 · Peer port serves before Raft starts

- **Date:** 2026-10-20 · **Part:** A · **Status:** Implemented · **Decided by:** A
- **Decision.** A node starts its peer listener first; peer requests that arrive before the node is
  fully built wait on a shared one-shot future for the node context.
- **Why.** Otherwise the first elections time out against nodes whose listener is not up yet, and
  cluster formation is slow and noisy.
- **Where.** `node/mod.rs::start_with`.

---

## 11. Engineering process

### D65 · Testing strategy

- **Date:** 2026-10-20 · **Part:** all · **Status:** Implemented · **Decided by:** team
- **Decision.** Unit tests at the bottom of each file; the deterministic core (`state.rs`,
  `bidding.rs`, `planner.rs`) is tested thoroughly. Multi-node tests in `tests/` start real nodes in
  one process on `127.0.0.1:0`, using `tests/common`. No fixed sleeps: poll with `wait_until` and a
  deadline. A timing test is stable after 10 green runs in a row. Fix the code, not the test.
  openraft's storage conformance suite runs against our storage.
- **Why.** Real nodes over real sockets catch transport and timing bugs that mocks hide.

### D66 · Rust coding rules

- **Date:** 2026-10-20 · **Part:** all · **Status:** Implemented
- **Decision.** Typed errors with `thiserror` in library code, `anyhow` with context in binaries; no
  `unwrap()` outside tests; `unsafe` forbidden; `Duration` in APIs, `_ms`/`_us` in config and
  events; poisoned locks recovered with `into_inner`; every `tokio::spawn` owned; waits capped with
  timeouts; `cargo fmt` and `clippy -D warnings` clean (CI enforces both).

### D67 · Team process: parts, owners, pull requests, CI

- **Date:** 2026-10-20, PR rule 2026-10-21 · **Part:** all · **Status:** Implemented
- **Decision.** Six parts (A–F) with one owner each and review pairs A↔D, B↔E, C↔F. One owner per
  file. Every change goes through a pull request from a `part-<letter>-<topic>` or `shared-<topic>`
  branch; nobody pushes to `main`. CI on Ubuntu runs fmt, clippy and tests. Linear history (rebase or
  squash merge).
- **Why.** Each member must build, measure, demo and explain their part; reviews make the pair able to
  explain it too.

### D68 · Library boundary: openraft is the only distributed library

- **Date:** 2026-10-20 · **Part:** all · **Status:** Implemented
- **Decision.** openraft (D14) is the only library doing distributed-systems work. Everything else
  is ours: state machine, storage, network adapter, transport, fault gate, bidding, planner, client
  middleware, fault injection, rejoin. Libraries handle async I/O (tokio), encoding (serde, bincode,
  serde_json, toml), CLI (clap), errors, randomness and logging. A new dependency needs a
  design-log entry; a new distributed-systems library needs team agreement.
- **Report notes.** The report must separate library code from ours.

### D69 · Phase 2 hooks

- **Date:** Phase 1 architecture, accepted 2026-11-02 · **Part:** all · **Status:** Accepted, docs only ·
  **Decided by:** omar.leithym, on the agent's recommendation
- **Decision.** The Discovery Service becomes a second state machine on the same Raft log
  (registrations and lease renewals as `Command` variants). Claims could become leases with fencing
  tokens issued by the leader. P2P links live in the client; the cloud leaves the data path after
  discovery.
- **Why.** No second replication protocol; Phase 2 adds commands, not mechanisms.

### D70 · Agents ask, recommend and record every decision; "grill with docs"

- **Date:** 2026-10-27 · **Part:** all · **Status:** Implemented · **Decided by:** Omar Leithy
- **Context.** Most code is written with AI coding agents. Agents tend to settle design questions
  silently, so decisions end up in code nobody can explain at the demo, and the report loses the
  reasoning.
- **Decision.**
  1. Every prompt that may change a file must say **"grill with docs"** (or run `/grill-with-docs`
     in Claude Code). Without it, an agent does read-only work only and asks for the phrase before
     writing anything.
  2. In a grill-with-docs session the agent reads the docs first, then asks the person about every
     design decision the task needs, one at a time, each with the options, trade-offs and its
     recommendation. It never decides on its own.
  3. Every decision goes into this log in the template above, verbosely, in the same PR as the code.
     Diagrams in `architecture.md` and the tables in `protocol.md` are updated in the same PR.
- **Why.** The report needs every decision with its reasoning and rejected options. Every member has
  to explain their part; a decision they were asked about is one they can explain.
- **Consequences.** Slower prompts; more questions per task. Worth it for a graded design.
- **Where.** AGENTS.md "Design decisions", `.claude/skills/grill-with-docs/SKILL.md`.

---

## 15. Finishing Phase 1 (November 2026)

Dr Amr's class notes and the project description were reviewed on 2026-11-01. Several entries below
exist because of them.

### D111 · Responder service time: 200 ms × severity

- **Date:** 2026-11-01 · **Part:** E, A · **Status:** Implemented · **Decided by:** omar.leithym, on the agent's recommendation
- **Context.** Releasing a responder (D39) needs a simulated time on scene. It sets capacity: with
  30 responders, holds averaging S seconds allow about 30/S assignments per second. Beyond that,
  plans correctly return `responder: None`.
- **Options considered.**
  1. *200 ms × severity*: holds of 200–1000 ms, average 600 ms, about 50 assignments per second.
  2. *Fixed 500 ms*: about 60 per second; severity has no effect on the simulation.
  3. *1 s × severity*: average 3 s, about 10 per second; most requests under load get no responder.
- **Decision.** Option 1, with new key `campus.service_ms_per_severity = 200`.
- **Why.** Severe incidents hold responders longer, capacity does not depend on run length, and the
  load experiments still exercise the planner's normal path.
- **Consequences.** It is a labelled simulation parameter, not a measurement. Analysis must report
  the share of `responder: None` plans next to latency, because they are cheaper than real
  assignments.
- **Where.** `config.rs::CampusConfig`, `node/release.rs`.
- **How we check it.** `node::release::tests`, `tests/cluster.rs`.

### D112 · Client connections: ping through the gate, bounded send queues

- **Date:** 2026-11-01 · **Part:** F · **Status:** Implemented · **Decided by:** omar.leithym, on the agent's recommendation
- **Context.** A machine that loses power sends no TCP close, so the socket stays half-open for many
  minutes, and a restarted node would never be reconnected. "No reply" is no signal, because a node
  that is not the executor normally says nothing. Separately, frames for an unreachable node would
  pile up without limit at thousands of users.
- **Options considered.**
  1. *Ping with `Control(Status)`*: no `proto.rs` change; small background traffic.
  2. *Reconnect on every retry*: no background traffic; churns healthy idle connections, and recovers
     only while requests are failing.
  3. *TCP keepalive and `TCP_USER_TIMEOUT`*: no application traffic; needs the `socket2` crate and is
     harder to log.
  For the queue: *bound it and drop when full*, or *block the sender*, which lets one dead node stall
  every user, the very thing multicast is meant to avoid.
- **Decision.** Each connection sends `Control(Status)` every 1 s. Nothing received for 3 s means drop
  it and reconnect (connect cap 1 s, 500 ms pause after a failure). Each node has a queue of 1024
  frames; when full, the frame is dropped for that node only. One socket write is capped at 3 s.
  Events `client_link_up` and `client_link_down`.
- **Why.** Control frames pass the fault gate (D23), so a silenced node keeps its connection while a
  dead one is replaced: exactly the difference we need. The other nodes get the frame and the retry
  timer covers the rest, so `submit` never blocks on one node.
- **Consequences.** Three small messages a second per client process. These limits are constants in
  `client.rs`, not config keys: no experiment varies them. Frames queued during an outage are sent
  on reconnect, which is harmless because requests are idempotent. A `ctl`-style `control` call and
  `MeshClient` open sockets from `client.rs`, the one place outside the "two places" rule; it reuses
  `node::transport::framed`.
- **Where.** `client.rs::run_link`, `serve_link`. **How we check it.** `tests/client.rs`, FT-8.

### D113 · Required client IDs; sequence numbers start at the clock

- **Date:** 2026-11-01 · **Part:** F · **Status:** Implemented, amends D51 · **Decided by:** omar.leithym, on the agent's recommendation
- **Context.** A random ID makes runs hard to match to machines (D51 already advised against it).
  With sequence numbers from 0, a rerun with the same ID against a running cluster reuses request IDs,
  and nodes return the old committed results as though they were new.
- **Options considered.**
  1. *Required, sequence from start time*: both problems gone.
  2. *Random if missing*: convenient locally; two machines could collide by chance.
  3. *Required, sequence from 0*: simple, but the cluster must be wiped before every run.
- **Decision.** Option 1. `loadgen` fails without `--client-id`. `MeshClient` starts `seq` at the
  current time in microseconds and counts up.
- **Consequences.** IDs are large numbers in the logs. Two processes must not share a client ID.
- **Where.** `client.rs::MeshClient::new`, `bin/loadgen.rs`.
- **How we check it.** `tests/client.rs`; the runbook gives each machine its ID.

### D115 · A different election timeout range per node

- **Date:** 2026-11-01 · **Part:** A · **Status:** Implemented · **Decided by:** omar.leithym, on the agent's recommendation
- **Context.** A client test with the leader silenced failed about 1 run in 16: 10 elections in a row
  with no winner, about 305 ms apart. openraft 0.9 draws one election timeout per process
  (`EngineConfig::new`) and checks it on a tick of 1.5 heartbeats (75 ms). In a 150–300 ms range, two
  nodes often get the same number of ticks. If their ticks also line up, each votes for itself in
  every term. Fresh random timeouts per election, Raft's defence (Ongaro and Ousterhout, §5.2), are
  not what this version does. Ticks align less on separate machines, but an alignment lasts for the
  whole outage.
- **Options considered.**
  1. *A range per node, by place in the config.*
  2. *Our own watcher that triggers elections after a fresh random delay*: closest to the paper, but it
     works around openraft's timer, and interacts with the rejoin gate's `elect(false)` (D75).
  3. *Document it and rely on client retries*: the demo could show a long failover.
- **Decision.** Option 1. The range starts at `election_timeout_min_ms + place × 2 ticks`, and is one
  tick wide or the configured width if smaller. Defaults: node 1 150–225 ms, node 2 300–375 ms, node 3
  450–525 ms. No new config key.
- **Why.** Two nodes can no longer fire on the same tick, so one reaches the others first and wins.
  Small, and easy to explain.
- **Consequences.** Failover can take up to about 225 ms longer than the old 300 ms bound when only
  high-ID nodes remain, and lower IDs tend to win elections. `election_timeout_max_ms` now only limits
  the width. E3 measures the real figure.
- **Where.** `raft/mod.rs::election_range`.
- **How we check it.** `raft::tests`; `tests/client.rs::bidding_serves_a_client_with_a_node_silenced`
  over repeated runs; E3.
- **Report notes.** A good "found by testing" story: a library detail broke an assumption from the
  paper, and the repeated-run rule (AGENTS.md) caught it.

### D114 · Bidding is the default; the leader only sequences the log

- **Date:** 2026-11-01 · **Part:** B, A · **Status:** Implemented, supersedes D34 · **Decided by:** omar.leithym
- **Context.** The plan was to flip the default after a comparison run. That run needs the load
  generator, on the lab machines. Meanwhile Dr Amr's notes say twice that cloud nodes are peers with
  no coordinator, and leader dispatch is a coordinator pattern. Takeover (D110) has landed, so bidding
  now handles a silent winner.
- **Options considered.**
  1. *Flip now, keep the baseline in E1.*
  2. *Keep leader dispatch until E1 has run*: original plan, but the demo could show the pattern he
     objected to.
  3. *Change the lab config only*: tests undisturbed, but two different defaults.
- **Decision.** Option 1, in the owner's words: "we want leader for the state replication and
  maintenance and the bidding to decide who takes the request". `Bidding` is the code default and is
  set in both shipped configs. `leader-dispatch` stays as E1's baseline, set explicitly.
- **Why.** Roles are now clean: bidding picks who plans, Raft's leader only orders the log. See also
  D116 on what stays with the leader.
- **Consequences.** The flip came **before** the numbers; if E1 shows bidding much worse, the team
  must revisit it and say so in the report. The test helper `TestCluster` still defaults to leader
  dispatch, because most multi-node tests assert on it; the bidding and takeover tests set bidding.
- **Where.** `config.rs::AssignmentPolicy`, `config/*.toml`, `tests/common/mod.rs`.
- **How we check it.** E1 policy comparison; `tests/bidding.rs`, `tests/takeover.rs`.

### D116 · Raft and the leader-run fault schedule stay, despite "no coordinator"

- **Date:** 2026-11-01 · **Part:** A, C · **Status:** Implemented · **Decided by:** omar.leithym
- **Context.** Dr Amr insists on peers with no coordinator (Sept 13, Oct 28). Raft has a leader. D12
  has that leader run the fault schedule.
- **Options considered.**
  1. *Keep Raft, move the schedule to every node, seeded from the config.* The agent's recommendation.
  2. *Keep Raft and D12*: least change. The spec says servers "elect one machine to ignore
     communication", and his notes say "mimic failure using election", so a schedule run by the
     elected node may fit.
  3. *Replace Raft with a leaderless protocol*: a full rewrite, not possible by Nov 4.
- **Decision.** Option 2.
- **Consequences.** The report must argue the point: any node can propose (D26), bidding assigns
  work (D114), and the leader only sequences the log and picks the fault victim. He may still see this
  as a coordinator, so **someone should show him the argument before the demo.**
- **Where.** `raft/`, `node/faults.rs`.

### D117 · Richer faults are not built

- **Date:** 2026-11-01 · **Part:** C · **Status:** Accepted · **Decided by:** omar.leithym, on the agent's
  recommendation
- **Decision.** No one-way faults, added delay or loss. E1–E6 and the demo use silence, physical
  power-off and one-plus-one failures only. The `TODO(C)` marker stays for anyone who needs it later.
- **Why.** Unused by any experiment, and it adds risk to the gate everything relies on.

### D118 · Raft log and vote on disk, for physical power-off

- **Date:** 2026-11-01 · **Part:** A, D · **Status:** Implemented, supersedes D24 · **Decided by:** omar.leithym, on the agent's recommendation
- **Context.** D24 kept the log and vote in memory because the only faults were network silences. Dr
  Amr's notes (Oct 28) say he wants to see a machine physically turned off. That wipes memory: the node
  forgets its vote, which Raft forbids, and committed entries could be lost if a wiped node helps
  elect a node missing them.
- **Options considered.**
  1. *Log and vote on disk, with a sync.* Phase 1's original plan (C7), and FT-9 comes back.
  2. *Disk without a sync*: latency near today's; a process kill is safe, but a real power cut, the
     exact case he asked for, can lose recent writes.
  3. *Stay in memory, restart behind the rejoin gate*: small, and usually looks fine in a demo, but
     breaks Raft's safety, and the report would have to admit it.
  4. *Keep silences only and ask him to accept that*: fails the stated demo.
- **Decision.** Option 1. `storage.dir` set: each node appends vote and log changes to
  `<dir>/node-<id>.wal` (`raft/wal.rs`), syncing before it acknowledges, off the async runtime. Records
  carry a length and an FNV-1a checksum; a damaged tail is cut off on open. `committed` is written
  unsynced: it only saves replay work. The state machine is rebuilt by replaying the log. A node
  with a file from an earlier run starts with `serving` false and elections off, and catches up
  through the same rejoin path (D46, D75, D76). After 3 s with no leader it allows elections,
  because the whole cluster may have restarted. Event `restarted`. `storage.dir` unset keeps memory only,
  for ordinary tests. No new dependency, and still no snapshots.
- **Why.** Raft's safety rests on a durable vote and log, and this is the smallest thing that gives
  them. Reusing rejoin means one recovery path to explain.
- **Consequences.** A disk sync per append raises commit latency, which now measures the disk as well
  as the protocol, and the report must say so. It depends on each machine's disk, so compare
  like with like. The runner must wipe `storage.dir` between runs, or the state carries over. Log files
  grow for the length of a run. Stopping a server now closes its connections (`JoinSet` in
  `serve_peers` and `serve_clients`), which the restart tests need.
- **Where.** `raft/wal.rs`, `raft/store.rs::LogStore`, `raft/mod.rs::RaftNode::start`,
  `node/rejoin.rs::after_restart`, `config.rs::StorageConfig`.
- **How we check it.** `raft::wal::tests`; `tests/durable.rs` (a restarted follower catches up before
  serving, with no election; the whole cluster restarts with every result). `kill -9` on the leader
  under load was tried by hand on one machine. **Only the lab can test real power-off.**
- **Report notes.** Crash recovery is now in scope. FT-9 returns.

### D119 · Peer calls through tarpc, with typed methods

- **Date:** 2026-11-01 · **Part:** C, A · **Status:** Implemented, amends D16, D17, D19 · **Decided by:**
  omar.leithym; typed methods on the agent's recommendation, tarpc against it
- **Context.** Dr Amr's notes (Sept 13, Oct 4, Oct 28): provide a remote invocation mechanism, do not go
  low-level, use a ready-made RPC or RMI, pick one and say why, with the chapter 2 table (Java RMI,
  CORBA, Sun XDR). Our peer request/reply, matching replies to calls by `seq`, was hand-written.
- **Options considered.**
  1. *Keep our transport, document it, ask him*: no risk to working code, but a grading risk until he
     answers. The agent's recommendation.
  2. *tarpc for peers*: a ready-made Rust RPC library. Meets his wording most directly; a new
     dependency and a rewrite of Part C's working transport days before the deadline.
  3. *tonic/gRPC everywhere*: the largest change, with protobuf schemas and a build step.
  For the shape, once tarpc was chosen: *typed methods* (visibly a remote interface) or *one generic
  `deliver(envelope)`* (smallest, but really our own protocol tunnelled through tarpc).
- **Decision.** tarpc 0.37 on the peer port, as `proto::PeerRpc`: `append_entries`, `vote`,
  `install_snapshot`, `propose`, `read_index`, `fault_order`, `bid`, `heartbeat`, each taking `from` and
  `term`, and each answering a `PeerReply`. It runs over our existing length-delimited framing, using
  bincode. `PeerFrame` and `PeerReplyFrame` are gone. `PeerClient::call(envelope, timeout)` and
  `PeerHandler` keep their signatures, so no caller changed. The client port keeps its own framing:
  its multicast with replies matched by ID is not request/reply (D17).
- **Why.** tarpc is plain Rust with no schema step, and it matches replies and propagates deadlines as
  relative times, so no shared clock is needed (D56). The spec says a framework must not hide what is
  assessed: election, consistency and recovery remain ours.
- **Consequences.** The lockfile grows from about 100 crates to 169, including OpenTelemetry, which
  tarpc uses for tracing. Library credit goes in the report (D68). Silence is kept: a silenced node's
  methods never return, so the caller sees its deadline pass, and the fault order's last word still
  goes out. tarpc drops a call at its deadline, so each handler runs in its own task and a Raft RPC is
  never cut halfway. A new rule: a connection whose calls have gone unanswered for 1 s is replaced,
  because a machine that lost power sends no close (D118). A silenced peer's connect still succeeds.
- **Where.** `proto.rs::PeerRpc`, `node/transport.rs`.
- **How we check it.** Every multi-node suite, unchanged, 10 runs each. **Peer traffic on a real LAN
  can only be checked in the lab.** Experiments cover its cost in E5.
- **Report notes.** Fill in the chapter 2 comparison. tarpc is closest to RPC with an interface in the
  host language (like Java RMI's interfaces, without its object references); bincode is the external
  data representation, in the role of Sun XDR.

### D120 · Degraded mode: the last server answers without a commit

- **Date:** 2026-11-02 · **Part:** C, A, F · **Status:** Implemented, a deliberate exception to D8 ·
  **Decided by:** omar.leithym, against the agent's recommendation
- **Context.** Dr Amr's notes ask to see one machine physically off and another failed, with the one
  remaining server handling all three client nodes. Raft cannot commit on one of three nodes, so until
  now the last node stayed quiet and clients gave up after about 30 s (D52).
- **Options considered.**
  1. *A clear error, plus results already committed*: keeps every safety property, but the remaining
     server handles nothing new. The agent's recommendation.
  2. *Degraded mode*: the lone node plans and answers at once, marks the answer provisional, and commits
     it when a peer returns. Meets the stated demo; gives up "Commit, then reply" for this one case.
  3. *Lower the quorum to one*: breaks Raft's safety in every case, not just this one.
- **Decision.** Option 2. `node/degraded.rs` holds the mode and a local table. In `handle_request` the
  order is: not serving → silent; committed result → `Done`; answer already stored → the same
  `Provisional`; degraded → plan, store, `Provisional`; otherwise the normal path. The degraded check
  comes before `decide_executor`, because neither a bid round nor a proposal can finish alone.
- **Why.** The transparency Dr Amr describes is what the user feels when servers fail, and he asks for
  this case by name. The spec allows speculative actions as long as they are "clearly labeled and
  reconciled"; it lists them under Phase 2, so this is an early, small version: the reply type labels
  the answer and D122 reconciles it.
- **Consequences.**
  - **This breaks "Commit, then reply", and the report must say so.** A provisional answer can later
    differ from what is on record.
  - **In a real partition two sides can answer differently.** The lone node cannot tell "both peers
    are down" from "I am cut off". The other two may commit a different plan for the same request.
  - **Clients are not told when an answer is replaced** (D122).
  - The table is memory only: a lone node that loses power loses answers it has not replayed.
  - No replicated data changes while degraded; the state machine stays deterministic.
- **Where.** `node/degraded.rs`, `node/mod.rs::handle_request`, `proto.rs::ReplyBody::Provisional`.
- **How we check it.** `tests/degraded.rs`, for a lone follower and a lone leader. A two-node silence under
  load was tried by hand on one machine. **A real power-off can only be checked in the lab.**
- **Report notes.** Present this as a trade-off we chose knowingly: availability for the one-server
  case against the consistency Raft gives elsewhere. Show it to Dr Amr before the demo.

### D121 · Degraded trigger: every peer suspected for 2 s

- **Date:** 2026-11-02 · **Part:** C · **Status:** Implemented · **Decided by:** omar.leithym, on the
  agent's recommendation
- **Context.** Entering degraded mode gives up a guarantee, so a brief stall must not trigger it. The
  peer detector (D79) suspects after about 150 ms, which is too eager on its own.
- **Options considered.**
  1. *Every peer suspected without a break for 2 s*: outlasts a leader election and a slow moment,
     and is within the client's first 2 s wait, so usually only one retry is lost.
  2. *As soon as both are suspected*: fastest, but a short network stall gives up the guarantee.
  3. *Use the client's whole retry budget first*: safest, and the clients have given up by then.
- **Decision.** Option 1. New key `timers.degraded_after_ms = 2000`, validated above 0. Never while
  silenced or not serving. It ends as soon as any peer is trusted again, or on silence; the wait then
  starts over. `NodeStatus` gains `degraded`, and `ctl status` shows it, so the demo can show the mode.
- **Why.** A node that is silenced or catching up knows it is the one behind, so it has no business
  answering. Any single peer back means a majority is possible again.
- **Consequences.** A node that hears nobody because its own network is down enters degraded mode too; D120
  covers that. A machine so overloaded that it misses heartbeats for 2 s could enter it wrongly, which
  would show in the logs as `degraded_entered` with peers still running.
- **Where.** `config.rs::Timers::degraded_after_ms`, `node/degraded.rs::Trigger`, `watch`.
- **How we check it.** `degraded::tests`; `tests/degraded.rs` shows one silenced node never triggers it and
  a silenced node never enters it.
- **Report notes.** Events `degraded_entered`, `degraded_left { reason, degraded_ms, provisional }`.

### D122 · Leaving degraded mode: replay through Raft, first commit wins

- **Date:** 2026-11-02 · **Part:** C · **Status:** Implemented · **Decided by:** omar.leithym, on the
  agent's recommendation; event details by the agent, on the instruction not to be asked again
- **Context.** Provisional answers must end up in the replicated state, or a node's table and the log
  disagree for good. The spec asks that speculative actions be reconciled.
- **Options considered.**
  1. *Propose each result as an ordinary `CommitResult`*: reuses first commit wins (D27), so no new
     command and no new rule in the state machine.
  2. *Discard them*: simple, but the record then lacks incidents clients were answered for.
  3. *A new `Reconcile` command with its own merge rule*: more precise, and a new replicated behaviour
     to test days before the deadline.
- **Decision.** Option 1. Once degraded mode is over and the node serves, it proposes up to 64 stored
  results at a time, retrying every 100 ms until a quorum takes them; each result leaves the table only
  when settled. `Duplicate` with a different plan logs `provisional_conflict` with reason
  `lost_first_commit`. `Stale` logs it with reason `responder_taken`, and the result is proposed again with
  no responder. Clients are not told.
- **Why.** Every `Command` is already idempotent, so retries are harmless, and no state-machine change
  means digests still prove convergence.
- **Consequences.** The local hold is on a timer but the replayed hold is on the log, so answers given one
  after another with the same responder collide when replayed quickly: the first holds it, and later
  ones are recorded with no responder though the client was told otherwise. That is what this decision asks
  for, and the conflict count measures it. While waiting, a retry gets the stored plan, not a new one.
- **Where.** `node/degraded.rs::replay`, `settle`; events in `metrics.rs`.
- **How we check it.** `tests/degraded.rs`: equal digests after the return, and both conflict reasons,
  with a winner forced for `lost_first_commit`.
- **Report notes.** Report the conflict count against the provisional count from each run.

### D123 · `ReplyBody::Provisional`, and how the client reports it

- **Date:** 2026-11-02 · **Part:** C, F · **Status:** Implemented · **Decided by:** omar.leithym, on the
  agent's recommendation
- **Context.** A client must be able to tell an uncommitted answer from a committed one, and the
  application must not mistake one for the other.
- **Options considered.**
  1. *A new reply variant carrying only the plan*: no log index or term to fake. The client accepts it
     at once if no `Done` came.
  2. *`Done` with a flag*: small, but every reader of `Done` must remember to check it.
  3. *The client keeps retrying for a `Done`*: then nobody gets an answer, and degraded mode has no effect.
- **Decision.** Option 1. `ReplyBody::Provisional(Plan)`. `client::Submitted` becomes `{ from, executor,
  plan, committed: Option<CommittedResult>, provisional, latency, attempts }`, so a provisional answer cannot
  pose as a committed result. `client_done` gains `provisional`, and `loadgen` prints the count.
- **Why.** Types over comments (AGENTS.md).
- **Consequences.** Everyone who reads `done.result` now reads `done.plan` or `done.committed`. A `Done` that
  arrives after a provisional answer is ignored, as any second reply is.
- **Where.** `proto.rs`, `client.rs::finished`, `hand_over`, `bin/loadgen.rs`.
- **How we check it.** `tests/degraded.rs`.
- **Report notes.** Latency of provisional answers is not comparable with committed ones: plot apart.

### D124 · Degraded holds: a local table on the service timer

- **Date:** 2026-11-02 · **Part:** C, E · **Status:** Implemented · **Decided by:** omar.leithym, on the
  agent's recommendation; taking over existing holds by the agent, on the instruction not to be asked
  again
- **Context.** Two provisional plans must not send the same responder at once, and releases need a
  quorum (D39), so the replicated holds cannot change while a node is alone.
- **Options considered.**
  1. *A local table of holds, freed by the same service time as D111*: same behaviour as normal, no
     replication.
  2. *Hold forever until peers return*: after 30 answers every plan has no responder.
- **Decision.** Option 1. On entering, the table also takes over the replicated holds, each freed after its
  service time from that moment, because they cannot be released through the log now. Free responders are
  all responders minus the table. `release.rs` skips its scan while degraded and catches up afterwards.
- **Why.** Without the takeover, a node that enters degraded mode under load starts with most responders
  held and keeps them held for the whole outage, which would make the mode useless when it matters.
- **Consequences.** The table is cleared on leaving; the log is the only truth after that.
- **Where.** `node/degraded.rs::Table`, `enter`; `state.rs::responders`; `node/release.rs`.
- **How we check it.** `degraded::tests`; `tests/degraded.rs` checks that answers take different responders
  while they are held.

### D125 · A returning node with no leader may stand for election after 3 s

- **Date:** 2026-11-02 · **Part:** D, C · **Status:** Implemented, amends D75 · **Decided by:** the agent, on
  omar.leithym's instruction not to be asked; to be confirmed in the PR review
- **Context.** Found while testing D120 by hand under load. Two nodes silenced together, and at the end
  of the silence no leader ever appeared: the third node's log was shorter, since the old leader had
  uncommitted entries, so its votes were refused, and the two returning nodes had elections off until they
  were current, which only a leader could confirm. The cluster stayed stuck until the clients gave up.
  With one fault at a time this cannot happen, but Dr Amr's case makes two faults at once a requirement.
- **Options considered.**
  1. *Use the restart rule from D118 after a silence as well*: elections back on after 3 s with no
     leader. It adds one rule, and the two paths become the same code.
  2. *Keep elections on during catch-up*: removes the wait, and loses D5's protection, because a
     returning node with a raised term could depose a healthy leader.
  3. *Re-enable when a higher term is seen*: more cases, and it still needs a timeout for the case where
     all three return together.
- **Decision.** Option 1. `rejoin.rs::catch_up_or_elect` serves both paths. The node still serves nothing
  until it is current.
- **Why.** A leader's heartbeats stop a follower standing even with elections on, so under a healthy
  leader this changes nothing, even if the catch-up takes longer than 3 s.
- **Consequences.** The architecture rule that a returning node does not vote for itself until current now
  has this exception. Recovery from a double fault takes at least 3 s, which should show in E4 if it
  covers the case.
- **Where.** `node/rejoin.rs`.
- **How we check it.** `tests/degraded.rs::the_cluster_recovers_when_the_longer_logs_were_all_silenced`, which
  hangs without this.
- **Report notes.** A good example of a bug found by running under load rather than by unit tests.

### D126 · Group communication named: `PeerSet::broadcast` and `MeshClient`

- **Date:** 2026-11-02 · **Part:** C, B, F · **Status:** Implemented · **Decided by:** omar.leithym, on the
  agent's recommendation
- **Context.** Dr Amr's notes ask for group communication, and for the application to send a message once
  while the middleware gives the replicas their copies, with the two layers kept distinct. We already
  did all of this, but in separate loops with no name a reader could point to.
- **Options considered.**
  1. *Name it with a small module and map the guarantees in the docs*: `PeerSet::broadcast`, used by
     bids and heartbeats, and `MeshClient` as the client-side group send. No message changes.
  2. *A group-communication library*: a new dependency doing distributed work (AGENTS.md rule 6), to
     replace code that works.
  3. *A reliable, ordered multicast of our own*: Raft already gives total order where we need it.
- **Decision.** Option 1. The guarantees:

  | Group send | Sender | Guarantee | What covers a lost copy |
  |------------|--------|-----------|-------------------------|
  | `MeshClient::submit` to all three nodes | application, through the client middleware | best effort, unordered | the client's retry with the same ID (D52) |
  | Bids, through `PeerSet::broadcast` | each serving node | best effort, unordered | a split decision, settled by first commit (D27) |
  | Heartbeats, through `PeerSet::broadcast` | every node | best effort, unordered | the next one; a real loss becomes a suspicion (D79) |
  | Committed state, through Raft | the leader | totally ordered, agreed by a majority | openraft's log replication (library) |

- **Why.** The state machine needs total order and already has it. Bids and heartbeats are soft: they
  are worthless late, so retrying them would be wrong.
- **Consequences.** `submit` is the application's one call and `MeshClient` is the middleware; the report
  can show the layers directly. A bid is now one task sending to both peers, not one per peer; it still
  never holds up the bid window.
- **Where.** `node/transport.rs::PeerSet::broadcast`, `node/bids.rs::send_bid`, `node/heartbeat.rs`,
  `client.rs::MeshClient`.
- **How we check it.** `tests/heartbeat.rs::a_group_send_reaches_every_peer_and_reports_who_missed_it`, plus the
  bidding and heartbeat suites unchanged.
- **Report notes.** Publish-subscribe is a separate question (see the entry on it). Ask Dr Amr whether he
  expects either in Phase 1, since his notes read that way while the spec lists zone subscriptions under Phase 2.

### D127 · FT-6: count elections after each `rejoined`

- **Date:** 2026-11-02 · **Part:** D, A · **Status:** Implemented · **Decided by:** omar.leithym, on the
  agent's recommendation
- **Context.** FT-6 asks whether a returning node with a stale term forces an election. openraft 0.9.25
  has no PreVote, so D75's gate is what prevents it, and it needs a measurement.
- **Options considered.**
  1. *Count new Raft terms in the logs within 10 s after each `rejoined`*: no new event, and it works on
     every run that has a fault.
  2. *A dedicated test only*: exists already in `tests/rejoin.rs`, but says nothing under load.
- **Decision.** Option 1, as `elections_after_rejoin` in `analysis/analyze.py`. The target is 0.
- **Consequences.** Timestamps come from several machines, so the 10 s window is approximate. A
  coincidental election in that window counts against the return. Double faults (D125) can raise it
  legitimately, so read it on one-victim cells.
- **Where.** `analysis/analyze.py::elections_after_rejoin`.
- **How we check it.** `analysis/test_analyze.py`.

### D128 · `resource_usage`: memory, CPU, network and queues once a second

- **Date:** 2026-11-02 · **Part:** all · **Status:** Implemented · **Decided by:** omar.leithym, on the
  agent's recommendation; the network and queue fields by the agent, on the instruction not to be asked
- **Context.** The spec lists CPU, memory, network and queue behaviour among the measurements, and
  nothing logged memory or network.
- **Options considered.**
  1. *An event from the node's reporter, read from `/proc`*: no new dependency, and it lands in the same
     log as everything else.
  2. *`sysstat` on each machine*: a second log format, and an install on six machines.
  3. *A crate such as `sysinfo`*: a new dependency for three small files.
- **Decision.** Option 1. Event `resource_usage { rss_kb, cpu_ticks, net_rx_bytes, net_tx_bytes, queued,
  running }`, once a second, from `node/usage.rs`, read off the async runtime. `client_done` also gains
  `no_responder`, because the analysis needs the share of plans that sent nobody (D111).
- **Consequences.** The network counters cover the machine, not the process, and leave out loopback, so
  they read zero on one machine and include any other traffic in the lab. `cpu_ticks` assumes 100 ticks per
  second when converted, which holds on Ubuntu.
- **Where.** `node/usage.rs`, `node/mod.rs::spawn_reporter`, `metrics.rs`.
- **How we check it.** `node::usage::tests`; `analysis/test_analyze.py`.

### D129 · Experiment load: `plan_work`, responders and think time

- **Date:** 2026-11-02 · **Part:** E, F · **Status:** Implemented, to confirm · **Decided by:** the agent,
  on omar.leithym's instruction not to be asked; to be confirmed in the PR review
- **Context.** Two problems would have made the experiments measure the wrong thing. `workers.plan_work`
  defaults to 0, where a plan takes under a microsecond, while the spec wants a planner that creates real
  load, and load balancing has nothing to balance without it. And D111's 30 responders at 600 ms give a
  capacity of about 50 assignments per second, so at high load nearly every plan would have no responder and
  E1 would mostly measure that path.
- **Options considered.**
  1. *In the cells only: `plan_work = 1000`, `campus.responders = 300`, `think_ms = 1000`*: defaults and
     the demo stay as agreed.
  2. *Change the defaults*: every config and test moves, days before the deadline.
  3. *Leave them, and report the limits*: the honest minimum, but E1 and E2 would show little.
- **Decision.** Option 1. **`plan_work = 1000`** took about 4 ms per plan on the development machine
  (calibrated by timing `planner::plan`; this is a setting, not a result). With 4 threads on 3 nodes that is a
  ceiling near 3000 plans per second. **300 responders** give about 500 assignments per second by D111's
  arithmetic. **`think_ms = 1000`** makes each user offer about one request per second, so 30, 300 and 3000
  users run well under, under, and at the planning ceiling. `cpu_weight = 1` in the cells: score is in
  microseconds and CPU in permille, so full CPU weighs about one job's expected wait at this `plan_work`.
  `skew_work = 5000` on one node makes its plans about five times slower.
- **Why.** The sweep then crosses from idle into overload, which is what a scaling figure should show. The
  responders stay a real limit at the top, and `no_responder_share` reports it instead of hiding it.
- **Consequences.** Above 500 requests per second, plans increasingly have no responder. **The lab machines
  may differ**: check `service_mean_us` on one short cell first. A closed loop cannot exceed its users' rate, so
  overload shows as latency, not as lost offered load (D54). The demo keeps the defaults.
- **Where.** `experiments/*.toml`, `planner.rs::ROUNDS_PER_UNIT` (its comment now gives the measured cost).
- **How we check it.** `service_mean_us` and `no_responder_share` in every run's summary.
- **Report notes.** State these settings beside every figure.

### D130 · Experiment cells and the runner

- **Date:** 2026-11-02 · **Part:** all · **Status:** Implemented, to confirm · **Decided by:** bash over SSH by
  omar.leithym (D62); the cell format and the extra `full` cells by the agent, on the instruction not to be asked
- **Context.** The agreed `quick` cells are E1 (30, 300, 3000 users × 2 policies), E2/E6 (skew × 2 scores),
  E3/E4/FT-6 (2 loads × 2 victims), E5 (faults off). `full` was agreed at about 31 cells with the rest to be defined.
- **Options considered.** For the format:
  1. *A TOML overlay plus a `# run:` line*: the file is both the config and the record of how it ran, and
     strict config parsing checks it.
  2. *A table in one script*: shorter, but no single file to point to per cell, against `experiments/README.md`.
  3. *A separate manifest beside each config*: two files per cell to keep in step.
- **Decision.** Option 1. `scripts/run-experiments.sh` and `lab-build.sh` read a hosts file and share
  `lab-common.sh`; the ssh target `local` runs on one machine. The other 18 `full` cells: E1 at 100 and 1000
  users (4); the other two scores (2); silences of 3 s and 20 s at 2 loads (4); faults on (1); FT-8 (1); P-2 at
  bid windows of 5, 10, 20, 50 ms (4); degraded at 2 loads (2).
- **Consequences.** Machines need SSH keys and rsync. A fault starts a third of the way into the measured time.
  `tests/experiment_cells.rs` fails if a cell stops being a valid config or the counts change.
- **Where.** `experiments/`, `scripts/`, `analysis/`.
- **How we check it.** The whole pipeline was tried on one machine with `hosts.local`, short durations and three
  cells, to test the scripts only. **It has not run over SSH or on the lab machines.**

### D131 · Publish-subscribe: Phase 2, for zones, docs only

- **Date:** 2026-11-02 · **Part:** all · **Status:** Accepted, docs only · **Decided by:** omar.leithym: "we
  will use publish sub in phase 2 for the zones, for phase 1 we are not dealing with zones but add it to docs
  so we can do it later"
- **Context.** Dr Amr's Oct 28 notes say "Implement publish-subscribe", and read as Phase 1. The spec lists
  zone subscriptions among peers under Phase 2.
- **Options considered.**
  1. *Phase 2, for zones, documented now*: follows the spec, and adds no code days before the deadline.
  2. *A small publish-subscribe in Phase 1, on the client port*: answers his notes literally, but nothing in
     Phase 1 would subscribe to anything.
- **Decision.** Option 1. Peers will subscribe to incident updates for their zones, with duplicate suppression
  and bounded forwarding as the spec requires. `docs/architecture.md` section 13 holds the hook.
- **Consequences.** A grading risk if he does expect it in Phase 1. **The team should ask him.**
- **Where.** `docs/architecture.md` section 13.
- **Report notes.** Name it as planned, and separate it from group communication, which is built (D126).

### D132 · The "entries table" is the spec's demo table

- **Date:** 2026-11-02 · **Part:** all · **Status:** Implemented · **Decided by:** omar.leithym, on the agent's
  recommendation
- **Context.** Dr Amr's Oct 28 note says to show "4 first entries + last entry" of the "entries table". That
  could mean a table in the project description, or a log of entries in the running system.
- **Options considered.**
  1. *The spec's Required Demonstration Scenarios table*: its first four rows and last row are exactly Phase 1
     (normal operation, load imbalance, server failure, server recovery, scale and stress), and the rows between are
     Phase 2. A demo script covers them.
  2. *The Raft log, through a `ctl log` command*: new code to guess at a meaning that fits the table less well.
- **Decision.** Option 1. `docs/demo.md` walks through those five, plus his two failure cases (a machine
  physically off; one off and one failed). No `ctl log` command.
- **Consequences.** The live part uses `ctl status`, event logs and the load generators' output. Scale and
  stress is shown as prepared results, since it takes an hour. The demo has not been rehearsed on the lab machines.
- **Where.** `docs/demo.md`, `docs/runbook.md`.
- **How we check it.** A rehearsal on the six machines, still to do.
