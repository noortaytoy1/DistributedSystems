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
| D2  | Load-based bidding election per request                         | B    | Accepted          |
| D3  | Leader dispatch kept as a measured baseline                     | B    | Implemented       |
| D4  | Implement Raft ourselves                                        | A    | Superseded by D14 |
| D5  | PreVote in Raft                                                 | A, D | Open              |
| D6  | Client sends every request to all three nodes over UDP          | F    | Amended by D17    |
| D7  | TCP between cloud nodes                                         | C    | Implemented       |
| D8  | Reply only after commit                                         | A    | Implemented       |
| D9  | Request ID plus a replicated table of results                   | A    | Implemented, amended by D27 |
| D10 | Optimistic check at commit instead of locks                     | A, E | Implemented, amended by D28 |
| D11 | Single-owner tasks and channels; planner on a thread pool       | all  | Amended by D47    |
| D12 | The Raft leader runs the fault schedule                         | C    | Accepted          |
| D13 | Faults injected in the network endpoint                         | C    | Implemented       |
| D14 | openraft 0.9.25 behind a `Consensus` trait                      | A    | Implemented       |
| D15 | Rust 2024 on a pinned toolchain, tokio runtime                  | all  | Implemented       |
| D16 | Wire format: length-delimited frames with bincode               | all  | Implemented       |
| D17 | Clients use TCP on a dedicated client port                      | C, F | Implemented (confirm) |
| D18 | Two ports per node; `ctl` shares the client port                | C    | Implemented       |
| D19 | One multiplexed connection per peer                             | C    | Implemented       |
| D20 | `TCP_NODELAY` on every connection                               | C    | Implemented       |
| D21 | The term rule does not apply to Raft RPCs                       | all  | Implemented       |
| D22 | Fault gate semantics                                            | C    | Implemented       |
| D23 | Control frames bypass the fault gate                            | C    | Implemented       |
| D24 | In-memory Raft storage, no snapshots                            | A    | Implemented       |
| D25 | Static membership; every node calls `initialize`                | A    | Implemented       |
| D26 | Any node proposes; followers forward to the leader              | A    | Implemented       |
| D27 | First commit wins; the outcome tells the proposer what happened | A    | Implemented       |
| D28 | A plan is stale if its responder is held or unknown             | A    | Implemented       |
| D29 | Three idempotent log commands                                   | A    | Implemented       |
| D30 | Deterministic state machine, integers only                      | A    | Implemented       |
| D31 | State digest: an FNV-1a hash chain over the applied log         | A    | Implemented       |
| D32 | Retry an unreachable peer after two heartbeats                  | A    | Implemented       |
| D33 | No leader, no executor (leader dispatch)                        | A    | Implemented       |
| D34 | Leader dispatch is the default until bidding lands              | B    | Implemented       |
| D35 | Re-plan a stale plan at most three times                        | A, E | Implemented       |
| D36 | A node that cannot answer stays quiet                           | A, F | Implemented (confirm) |
| D37 | Any serving node answers a retry from its own state             | A    | Implemented       |
| D38 | Responders seeded once through Raft by the leader               | A, E | Implemented       |
| D39 | Responders are never released                                   | A, E | Open              |
| D40 | Timer defaults and validation                                   | all  | Implemented       |
| D41 | Peer heartbeats as a failure detector for takeover              | C    | Accepted (confirm) |
| D42 | Takeover trigger                                                | B, C | Open              |
| D43 | When the bid window starts                                      | B    | Open              |
| D44 | No `Claim` message; `just_won` in the load score                | B    | Open              |
| D45 | Load score formula and tie-break                                | B    | Proposed          |
| D46 | Rejoin gate: `serving` flag, current at the leader's commit     | D    | Accepted          |
| D47 | Shared node context instead of single-owner tasks               | A    | Implemented       |
| D48 | Bounded worker pool that sheds load                             | E    | Accepted          |
| D49 | Planner: deterministic, integer cost, CPU knob                  | E    | Implemented       |
| D50 | Domain model: integer campus, four capabilities, severity 1–5   | all  | Implemented       |
| D51 | Request IDs and per-machine client IDs                          | F    | Implemented       |
| D52 | Client retry budget                                             | F    | Open              |
| D53 | One client process, one connection per node, shared by users    | F    | Accepted          |
| D54 | Load generator: closed loop first, open loop as a mode          | F    | Accepted          |
| D55 | JSON Lines event logs written off the async runtime             | all  | Implemented       |
| D56 | Durations on one clock; latency on the client                   | all  | Implemented       |
| D57 | Heartbeat logging: one summary a second, details at debug       | A    | Implemented       |
| D58 | Human logs through `tracing`, measurements through events       | all  | Implemented       |
| D59 | One strict TOML config shared by every process                  | A    | Implemented       |
| D60 | Deployment target: six Ubuntu machines on one LAN               | all  | Implemented       |
| D61 | Peer port serves before Raft starts                             | A    | Implemented       |
| D62 | `ctl` is the operator tool; no separate experiment runner yet   | C    | Implemented (confirm) |
| D63 | Experiment method: repetitions, warm-up, run length             | all  | Open              |
| D64 | Analysis in Python                                              | all  | Pending approval  |
| D65 | Testing strategy                                                | all  | Implemented       |
| D66 | Rust coding rules                                               | all  | Implemented       |
| D67 | Team process: parts, owners, pull requests, CI                  | all  | Implemented       |
| D68 | Library boundary: openraft is the only distributed library      | all  | Implemented       |
| D69 | Phase 2 hooks                                                   | all  | Proposed          |
| D70 | Agents ask, recommend and record every decision; "grill with docs" | all | Implemented    |
| D74 | Planner pick rule: exact capability, nearest, cost in metres    | E    | Implemented       |
| D75 | Severe incidents fall back to any capability                    | E    | Implemented       |
| D76 | Incident age in the planner needs a time from the node          | A, E | Open              |
| D77 | CPU knob: fixed hash rounds, no built-in base work              | E    | Implemented       |
| D78 | Campus layout: zone grid, even round-robin, seeded positions    | E    | Implemented       |
| D79 | Campus RNG: our own SplitMix64                                  | E    | Implemented       |
| D80 | Random incidents are uniform                                    | E, F | Implemented       |
| D81 | Planner and campus tests                                        | E    | Implemented       |

**Open decisions:** D5, D17, D39, D42, D43, D44, D52, D63, D76, plus the open parts of D36, D45,
D48 and D62. Most are conflicts between the Phase 1 architecture document and the code. They stay
open until someone implements the related code: whoever works on it settles the decision first.
During "grill with docs" the agent finds the open entries the task touches, asks the person to
decide with the options and recommendation listed in the entry, and updates the entry (status,
decided by, decision) in the same PR (AGENTS.md, "Design decisions").

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

- **Date:** Phase 1 architecture · **Part:** B · **Status:** Accepted (template: `bidding.rs`,
  `TODO(B)`) · **Decided by:** team
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
- **Report notes.** Open sub-decisions: D42 (takeover), D43 (bid window start), D44 (claims), D45
  (score formula).

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

- **Date:** Phase 1 architecture · **Part:** A, D · **Status:** **Open** · **Decided by:** team
  (Phase 1 document); not implementable as written on openraft
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

- **Date:** 2026-10-20 · **Part:** A · **Status:** Implemented · **Decided by:** A
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

- **Date:** 2026-10-20 · **Part:** A, E · **Status:** Implemented (campus generation: D78)
  · **Decided by:** A
- **Context.** Responders are part of the replicated state, so they must enter it through the log.
- **Decision.** The campus is a pure function of `[campus]` in the config (`seed`, `zones`,
  `responders`), so every node derives the same list. A background task polls every 200 ms; when
  this node is the leader and the state has no responders yet, it proposes `RegisterResponders`.
  Registration is idempotent (D29), so two leaders in a row proposing it is harmless.
- **Why.** No special bootstrap path: seeding uses the same commit path as everything else.
- **Where.** `node/mod.rs::spawn_campus_seeding`, `campus.rs::responders`.

### D39 · Responders are never released

- **Date:** 2026-10-27 · **Part:** A, E · **Status:** **Open** · **Decided by:** nobody yet
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

- **Date:** 2026-10-20 (scaffold) · **Part:** C, F · **Status:** Implemented; **team to confirm**
  because it changes Phase 1's D6 · **Decided by:** A in the scaffold
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
- **Consequences.** Reconnects happen on the next call after a failure; there is no reconnect
  backoff yet (`TODO(C)`).
- **Where.** `transport.rs::PeerClient`, `serve_peers`, `raft/network.rs::PeerLink`.
- **How we check it.** `transport::tests::concurrent_calls_get_their_own_replies`.

### D20 · `TCP_NODELAY` on every connection

- **Date:** 2026-10-20 · **Part:** C · **Status:** Implemented · **Decided by:** A
- **Decision.** Nagle's algorithm is off on every connection.
- **Why.** Small messages dominate; Nagle with delayed ACKs would add up to 40 ms to each, more
  than the whole bid window.
- **Where.** `transport.rs::framed`.

---

## 4. Faults, failure detection and recovery

### D12 · The Raft leader runs the fault schedule

- **Date:** Phase 1 architecture · **Part:** C · **Status:** Accepted (template:
  `faults::spawn_schedule`, `TODO(C)`) · **Decided by:** team
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

- **Date:** 2026-10-20 · **Part:** C · **Status:** Implemented · **Decided by:** A, C
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

- **Date:** 2026-10-20 · **Part:** C · **Status:** Accepted in the template; **team to confirm**
  because it changes Phase 1 §9.3 · **Decided by:** A in the scaffold
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

- **Date:** 2026-10-27 · **Part:** B, C · **Status:** **Open**
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

### D46 · Rejoin gate: `serving` flag, current at the leader's commit

- **Date:** 2026-10-20 · **Part:** D · **Status:** Accepted (template: `rejoin.rs`, `TODO(D)`) ·
  **Decided by:** team (Phase 1 §9.6), shaped in the scaffold
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

- **Date:** 2026-10-20 · **Part:** B · **Status:** Implemented · **Decided by:** A, B
- **Decision.** `policy.assignment` defaults to `leader-dispatch`. Setting `bidding` today logs a
  warning and falls back to leader dispatch.
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

- **Date:** 2026-10-20 · **Part:** A, F · **Status:** Implemented; **team to confirm** ·
  **Decided by:** A
- **Context.** `ReplyBody::Rejected(reason)` exists in the protocol but no node sends it.
- **Decision.** A node that is not serving, is not the executor, finds its worker pool full, cannot
  commit before the propose timeout, or gives up on stale plans sends **nothing**. The client's
  retry covers it.
- **Why.** The client sends to all three nodes; a "no" from one node says nothing about the other
  two. Silence keeps the client logic to "first `Done` wins".
- **Open part.** Whether `Rejected` should be used for "all nodes overloaded" so the client can back
  off. Recommendation: leave it unused until F's load tests show retry storms.

### D37 · Any serving node answers a retry from its own state

- **Date:** 2026-10-20 · **Part:** A · **Status:** Implemented · **Decided by:** A
- **Decision.** When a request arrives and its result is already in the node's local state, the node
  replies at once with that result, whether or not it executed it.
- **Why.** After a failover the client's retry is answered by whichever node is up, without planning
  or committing again (Phase 1 FT-7: one execution, cached reply).
- **Consequences.** A retry can get up to three identical replies; the client keeps the first.
- **Where.** `node/mod.rs::handle_request`.

### D43 · When the bid window starts

- **Date:** 2026-10-27 · **Part:** B · **Status:** **Open**
- **Context.** Phase 1 §6.3: the bid window closes "20 ms after the first bid seen". `bidding.rs`
  and `protocol.md`: "bid round closes `bid_window_ms` after the request arrives".
- **Options considered.**
  1. *From request arrival*: simple and local, but nodes receive the request at slightly different
     times, so their windows are offset.
  2. *From the first bid seen*: windows line up better across nodes; a lost first bid shifts it.
- **Recommendation.** Option 1 for the first version (less state), measure the wrong-winner rate
  in P-2's window sweep (5, 10, 20, 50 ms), switch if it is high.

### D44 · No `Claim` message; `just_won` in the load score

- **Date:** 2026-10-27 · **Part:** B · **Status:** **Open**
- **Context.** Phase 1 had a `Claim` message ("I am working on this") so peers add the request to
  the winner's load at once. `proto.rs` has no `Claim`; instead `LoadInputs::just_won` counts rounds
  a node won whose jobs have not reached its queue yet.
- **Options considered.**
  1. *Claim message*: peers know immediately; 2 more messages per request; a proto change.
  2. *Local `just_won`*: every node computes the same winner from the same bids, so every node can
     add the win to the winner's count itself; no extra message.
- **Recommendation.** Option 2: bidding is deterministic given the bids, so a claim tells peers
  nothing new. Revisit if E6 shows herding (one node winning many rounds in a burst).

### D45 · Load score formula and tie-break

- **Date:** 2026-10-20 · **Part:** B · **Status:** Proposed (in the `bidding.rs` template) ·
  **Decided by:** B to confirm
- **Decision (proposed).** `score = (queued + running + just_won + 1) × recent_service_us ÷ workers
  + w × cpu_permille`, integers only. Lowest wins; ties go to the lower node ID. The ranking is
  also the takeover order.
- **Why.** The first term estimates how long a new job would wait on this node (Eager, Lazowska and
  Zahorjan; queue length is the best simple load index). CPU covers load from outside our process
  (LB-2's busy loop). The `+1` makes an idle node with a slow planner lose to an idle node with a
  fast one.
- **Open part.** The CPU weight `w`, chosen with E6. Phase 1 started with equal weights.

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

- **Date:** 2026-10-20 · **Part:** E · **Status:** Accepted (template uses tokio's blocking pool,
  unbounded; `TODO(E)`) · **Decided by:** team (Phase 1 §10.1), shaped in the scaffold
- **Decision.** At most `workers.threads` (default 4) planning jobs run at once and at most
  `workers.queue_capacity` (default 64) wait. When full, `run` returns `Overloaded` at once, the
  node does not reply (D36), and the client's retry goes elsewhere. Queue length and running jobs
  feed the load score (D45).
- **Why.** CPU work off the runtime keeps 50 ms heartbeats on time when the planner is saturated
  (Phase 1 D11). A bounded queue turns overload into latency and retries instead of memory growth.
- **Open part.** Phase 1 said `W = CPU cores − 1`; the default is a fixed 4. Recommendation: set it
  per machine in the lab config once we know the core count, and record it in every run's config.
- **Where.** `node/workers.rs`, `[workers]` in the config.

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
  - Decision: weighted scoring with straight-line (Manhattan) distance, for now. Rules: D74, D75.
  - Why: the planner sees one incident at a time, so matching would need other open incidents
    (`node/mod.rs`, `state.rs`). Route search was chosen first, then dropped: without obstacles it
    gives the same answer as straight-line distance, and the CPU knob (D77) already gives E2 its
    load.
  - Not used yet: age (D76).

### D50 · Domain model: integer campus, four capabilities, severity 1–5

- **Date:** 2026-10-20 · **Part:** all · **Status:** Implemented (generation: D78, D80)
- **Decision.** `Point { x, y }` in metres as `i32`, Manhattan distance. `Capability` is one of
  `Medical`, `Fire`, `Security`, `Rescue`. `Incident { zone, location, severity (1..=5), needs,
  reported_at_ms }`, where `reported_at_ms` is the client clock. `Responder { id, zone, location,
  capability }`. `Plan { responder: Option<ResponderId>, cost }`. Defaults: 6 zones, 30
  responders, seed 42.
- **Why.** Integers keep replicated state deterministic (D30). Four capabilities cover the project's
  incident examples (fire, medical, security) plus search and rescue.

### D74 · Planner pick rule: exact capability, nearest, cost in metres

- **Date:** 2026-10-30 · **Part:** E · **Status:** Implemented · **Decided by:** Yahia Kilany
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
- **Consequences.** Severity and age do not change who is nearest; see D75 and D76.
- **Where.** `planner.rs::plan`, `planner.rs::nearest`.
- **How we check it.** `planner::tests` (D81).
- **Report notes.** One line per factor from spec p. 3: capability (filter), distance (score),
  availability (only free responders are passed in), severity (D75), age (D76).

### D75 · Severe incidents fall back to any capability

- **Date:** 2026-10-30 · **Part:** E · **Status:** Implemented · **Decided by:** Yahia Kilany
- **Context.** The spec wants severity in the decision. With one incident at a time, severity
  cannot change who is nearest. It can change what happens when no match is free.
- **Options considered.**
  1. Severity 4–5 with no free match: send the nearest free responder of any capability.
     Severity 1–3: no responder.
  2. Severity weights the cost (distance × (6 − severity)): the pick never changes, and cost
     stops being metres.
  3. Severity not used; log it as a gap.
- **Decision.** Option 1, with the same tie-breaks as D74. With nobody to send, the plan is
  `responder: None, cost: 0` (`planner::SEVERE = 4`).
- **Why.** Severity then really changes the outcome, and `cost` keeps one meaning.
- **Consequences.** A severe incident can get the wrong kind of help, on purpose: some help beats
  none.
- **Where.** `planner.rs::plan`, `planner::SEVERE`.
- **How we check it.** `planner::tests::no_match_for_a_severe_incident_sends_the_nearest_of_any_capability`,
  `no_match_for_a_minor_incident_gives_no_responder`.

### D76 · Incident age in the planner needs a time from the node

- **Date:** 2026-10-30 · **Part:** A, E · **Status:** **Open** · **Decided by:** nobody yet
  (Yahia Kilany chose to leave age out for now)
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
  severe one (D75).
- **Where.** `node/mod.rs::execute` (A), `planner.rs::plan` (E).

### D77 · CPU knob: fixed hash rounds, no built-in base work

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

### D78 · Campus layout: zone grid, even round-robin, seeded positions

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
  fallback in D75 to matter.
- **Consequences.** Only positions depend on the seed. `responders = 0` gives an empty campus,
  and seeding then registers nothing.
- **Where.** `campus.rs::responders`, `ZONE_SIZE_M`.
- **How we check it.** `campus::tests` (D81).

### D79 · Campus RNG: our own SplitMix64

- **Date:** 2026-10-30 · **Part:** E · **Status:** Implemented · **Decided by:** Yahia Kilany
- **Options considered.**
  1. Our own SplitMix64 (about 10 lines).
  2. rand's `StdRng`: may change between rand versions; safe only while `Cargo.lock` pins it.
  3. `rand_chacha::ChaCha8Rng`: stable, but a new dependency.
- **Decision.** Option 1, seeded with `campus.seed`.
- **Why.** Its output is fixed by our code, so the campus never changes under us. No new
  dependency.
- **Where.** `campus.rs::SplitMix64`.

### D80 · Random incidents are uniform

- **Date:** 2026-10-30 · **Part:** E, F · **Status:** Implemented · **Decided by:** Yahia Kilany
- **Options considered.**
  1. Everything uniform.
  2. Skewed severity (e.g. 35/25/20/12/8 %).
  3. A hot zone that gets half of all incidents.
- **Decision.** Option 1. Zone, position inside it, severity 1–5 and needed capability are all
  uniform, drawn from the RNG the caller passes in (the load generator seeds it).
- **Why.** Easy to explain. 40 % of incidents are severity 4–5, so the fallback in D75 gets
  used. A hot zone fits better as a later load-generator option.
- **Where.** `campus.rs::random_incident`.

### D81 · Planner and campus tests

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

- **Date:** 2026-10-27 · **Part:** F · **Status:** **Open**
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

### D53 · One client process, one connection per node, shared by users

- **Date:** 2026-10-20 · **Part:** F · **Status:** Accepted (template: `client.rs`, `TODO(F)`)
- **Decision.** `MeshClient` keeps one connection per node, shared by all simulated users in the
  process via `Arc`. `submit` sends the request to every node, waits `client_retry_ms` for the
  first `Done` with that ID, resends with the same ID, and logs `client_sent`, `client_done`,
  `client_gave_up`.
- **Why.** Thousands of users cannot each hold three TCP connections. Matching by request ID makes
  sharing safe.

### D54 · Load generator: closed loop first, open loop as a mode

- **Date:** 2026-10-20 · **Part:** F · **Status:** Accepted (template: `bin/loadgen.rs`)
- **Decision.** `loadgen --users N --duration-s S --think-ms T --seed X --client-id C`. Each user is a
  tokio task in a closed loop: send, wait for the reply or give up, think, repeat. An open-loop mode
  at a fixed rate is planned. Phase 1 used P processes × U users per machine; one process with many
  tasks is the default here.
- **Why.** Closed loop matches "simulated users"; open loop is needed to find saturation without the
  load backing off (coordinated omission). Load is raised by changing `--users`, never the
  application logic.

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

- **Date:** 2026-10-27 · **Part:** all · **Status:** **Open**
- **Context.** Phase 1 §11.4: 5-minute runs after a 30 s warm-up, at least **10** repetitions per
  configuration with different seeds, load levels 30–3,000 users, 95% confidence intervals.
  AGENTS.md and `experiments/README.md`: at least **5** repetitions, drop a 30 s warm-up.
- **Recommendation.** 10 repetitions as submitted, with 5 as the floor if lab time runs out, and say
  which in the report. Keep each run's config and git commit hash next to its logs.

### D64 · Analysis in Python

- **Date:** 2026-10-20 · **Part:** all · **Status:** Pending approval
- **Decision.** Analysis scripts (percentiles, confidence intervals, plots) are written in Python,
  in `analysis/`, output to `analysis/out/` (ignored). They are tools, not part of the system.
- **Why.** Plotting and statistics libraries; nobody has to write them in Rust.

### D62 · `ctl` is the operator tool; no separate experiment runner yet

- **Date:** 2026-10-20 · **Part:** C · **Status:** Implemented; **team to confirm** whether a
  runner is still planned
- **Decision.** `ctl status` shows role, term, leader, log index, applied index, digest, result
  count, `serving` and `silenced` for every node. `ctl silence --node N --ms M` and
  `ctl skew --node N --work W` inject faults and imbalance. The Phase 1 "experiment runner" (start
  everything, set load, collect logs) does not exist yet; `scripts/run-local.sh` covers one machine.
- **Recommendation.** A bash runner in `scripts/` that SSHes to the six machines, starts nodes and
  load generators with a run's config, and copies the logs back.

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

- **Date:** Phase 1 architecture · **Part:** all · **Status:** Proposed
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
