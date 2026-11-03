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
`Superseded by Dnn` or `Amended by Dnn`. D192 to D196 are kept for publish-subscribe, Phase 2: its code on `lane-wire` cites them, and cites D188 to D191 too, which went to the review of item 16, so those comments need new numbers when it lands. D218 was kept free for a leader step-aside, and now holds it.

**Status values.**

| Status        | Meaning                                                                          |
|---------------|----------------------------------------------------------------------------------|
| `Implemented` | Decided and in the code on `main`.                                               |
| `Accepted`    | Decided; the code is a template or not written yet. The owner builds it.         |
| `Decided`     | A person chose it, and no code change was needed.                                |
| `Proposed`    | Someone recommends it; the owner or the team has not confirmed it yet.           |
| `Open`        | Not decided. The entry lists the options and a recommendation. Needs a person.   |
| `Amended`     | Still holds, but a later entry changed part of it (named in the entry).          |
| `Superseded`  | Replaced by a later entry (named in the entry).                                  |

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
| D1 | Raft leader election for the cloud coordinator | A | Implemented (via D14), amended by D163 |
| D2 | Load-based bidding election per request | B | Implemented (default, D114) |
| D3  | Leader dispatch kept as a measured baseline                     | B    | Implemented       |
| D4  | Implement Raft ourselves                                        | A    | Superseded by D14 |
| D5  | PreVote in Raft                                                 | A, D | Implemented (gate, D75) |
| D6 | Client sends every request to all three nodes over UDP | F | Amended by D17, D167 |
| D7  | TCP between cloud nodes                                         | C    | Implemented       |
| D8  | Reply only after commit                                         | A    | Implemented       |
| D9  | Request ID plus a replicated table of results                   | A    | Implemented, amended by D27 |
| D10 | Optimistic check at commit instead of locks                     | A, E | Implemented, amended by D28 |
| D11 | Single-owner tasks and channels; planner on a thread pool       | all  | Amended by D47    |
| D12 | The Raft leader runs the fault schedule                         | C    | Superseded by D158 |
| D13 | Faults injected in the network endpoint                         | C    | Implemented       |
| D14 | openraft 0.9.25 behind a `Consensus` trait                      | A    | Implemented       |
| D15 | Rust 2024 on a pinned toolchain, tokio runtime                  | all  | Implemented       |
| D16 | Wire format: length-delimited frames with bincode | all | Implemented, amended by D119, D167, D170 |
| D17 | Clients use TCP on a dedicated client port | C, F | Superseded by D167 |
| D18 | Two ports per node; `ctl` shares the client port | C | Implemented, amended by D168, D175 |
| D19 | One multiplexed connection per peer | C | Superseded by D119 |
| D20 | `TCP_NODELAY` on every connection                               | C    | Implemented       |
| D21 | The term rule does not apply to Raft RPCs                       | all  | Implemented       |
| D22 | Fault gate semantics                                            | C    | Implemented, see D160 |
| D23 | Control frames bypass the fault gate                            | C    | Implemented, see D200 |
| D24 | In-memory Raft storage, no snapshots | A | Superseded by D118 |
| D25 | Static membership; every node calls `initialize`                | A    | Implemented, amended by D137 |
| D26 | Any node proposes; followers forward to the leader | A | Implemented, amended by D133, see D166 |
| D27 | First commit wins; the outcome tells the proposer what happened | A    | Implemented, see D143 |
| D28 | A plan is stale if its responder is held or unknown             | A    | Implemented, see D143, D147 |
| D29 | Three idempotent log commands                                   | A    | Implemented, amended by D133 |
| D30 | Deterministic state machine, integers only                      | A    | Implemented       |
| D31 | State digest: an FNV-1a hash chain over the applied log         | A    | Implemented, amended by D138 |
| D32 | Retry an unreachable peer after two heartbeats                  | A    | Implemented       |
| D33 | No leader, no executor (leader dispatch)                        | A    | Implemented       |
| D34 | Leader dispatch is the default until bidding lands | B | Superseded by D114 |
| D35 | Re-plan a stale plan at most three times | A, E | Implemented, amended by D220 |
| D36 | A node that cannot answer stays quiet | A, F | Implemented, amended by D220 |
| D37 | Any serving node answers a retry from its own state | A | Implemented, see D143, amended by D169 |
| D38 | Responders seeded once through Raft by the leader | A, E | Implemented, amended by D164 |
| D39 | Responders released by the executor, peers as backup | A, E | Implemented |
| D40 | Timer defaults and validation                                   | all  | Implemented, amended by D133 |
| D41 | Peer heartbeats as a failure detector for takeover              | C    | Implemented, amended by D79 |
| D42 | Takeover trigger: suspicion first, backstop max(200 ms, 3 × bid) | B, C | Implemented by D110 |
| D43 | When the bid window starts                                      | B    | Implemented |
| D44 | No `Claim` message; `just_won` in the load score                | B    | Implemented, amended by D85 |
| D45 | Load score formula and tie-break                                | B    | Implemented |
| D46 | Rejoin gate: `serving` flag, current at the leader's commit     | D    | Implemented, amended by D75, D136 |
| D47 | Shared node context instead of single-owner tasks               | A    | Implemented       |
| D48 | Bounded worker pool that sheds load                             | E    | Implemented       |
| D49 | Planner: deterministic, integer cost, CPU knob                  | E    | Implemented, see D148 |
| D50 | Domain model: integer campus, four capabilities, severity 1–5   | all  | Amended by D140, D141, D148 |
| D51 | Request IDs and per-machine client IDs | F | Implemented, amended by D113, D184 |
| D52 | Client retry budget | F | Implemented |
| D53 | One client process, one connection per node, shared by users | F | Implemented, amended by D167, D169, D181 |
| D54 | Load generator: closed loop first, open loop as a mode | F | Implemented, amended by D146, D181, D182, see D173 |
| D55 | JSON Lines event logs written off the async runtime             | all  | Implemented       |
| D56 | Durations on one clock; latency on the client                   | all  | Implemented       |
| D57 | Heartbeat logging: one summary a second, details at debug       | A    | Implemented       |
| D58 | Human logs through `tracing`, measurements through events       | all  | Implemented, amended by D134 |
| D59 | One strict TOML config shared by every process                  | A    | Implemented       |
| D60 | Deployment target: six Ubuntu machines on one LAN               | all  | Implemented       |
| D61 | Peer port serves before Raft starts                             | A    | Implemented       |
| D62 | `ctl` is the operator tool; the experiment runner is bash over SSH | C | Implemented (D130) |
| D63 | Experiment method: repetitions, warm-up, run length             | all  | Implemented, qualified by D162, D187 |
| D64 | Analysis in Python                                              | all  | Implemented       |
| D65 | Testing strategy                                                | all  | Implemented       |
| D66 | Rust coding rules                                               | all  | Implemented       |
| D67 | Team process: parts, owners, pull requests, CI                  | all  | Implemented       |
| D68 | Library boundary: openraft is the only distributed library | all | Implemented, amended by D179, D215 |
| D69 | Phase 2 hooks | all | Accepted, docs only, amended by D163 |
| D70 | Agents ask, recommend and record every decision; "grill with docs" | all | Implemented    |
| D71 | Worker pool mechanism: admission counter plus a fair semaphore  | E    | Implemented       |
| D72 | Worker pool: cancelled and panicking jobs                       | E    | Implemented       |
| D73 | Worker pool load report: queued and running only; no overload event yet | E | Implemented, amended by D138 |
| D74 | Manual faults: one at a time, 1 ms to 20 s, rejected otherwise  | C    | Implemented, see D159 |
| D75 | Rejoin gate: serving off and elections off until current        | D, A | Implemented, amended by D135 |
| D76 | `ReadIndex` peer message: the catch-up target from the leader   | D, A | Implemented, amended by D135 |
| D77 | Fault injection on/off: `faults.enabled`, `--faults` override   | C, A | Implemented, see D158 |
| D78 | Fault schedule: `FaultOrder`, timings, seeding, no overlap      | C, A | Amended by D158, D159, D160 |
| D79 | Peer heartbeat detector: heard-from, pause while silent, events | C    | Implemented, amended by D151, D155 |
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
| D93 | Planner pick rule: exact capability, nearest, cost in metres    | E    | Amended by D101, see D148 |
| D94 | Severe incidents fall back to any capability                    | E    | Amended by D101, see D148 |
| D95 | Incident age in the planner needs a time from the node | A, E | Implemented, see D143, D148 |
| D96 | CPU knob: fixed hash rounds, no built-in base work              | E    | Implemented       |
| D97 | Campus layout: zone grid, even round-robin, seeded positions    | E    | Implemented       |
| D98 | Campus RNG: our own SplitMix64                                  | E    | Implemented       |
| D99 | Random incidents are uniform                                    | E, F | Amended by D102, D141 |
| D100 | Planner and campus tests                                        | E    | Implemented       |
| D101 | Planner zone penalty: +200 m for another zone                   | E    | Implemented       |
| D102 | Unseeded random incidents from OS randomness                    | E, F | Implemented       |
| D103 | Each request's task runs its own round, in `node/bids.rs`        | B, A | Implemented, amended by D108 |
| D104 | Bids go out in the background; no waiting and no retry          | B    | Implemented, amended by D150, D154 |
| D105 | `[policy]` keys for scores, `just_won` and the starting planning time | B, A | Implemented, amended by D108 |
| D106 | Events `bid_sent` and `bid_round_closed`                        | B    | Implemented       |
| D107 | Planning time, CPU reading and hooks for takeover               | B, C | Implemented       |
| D108 | Planning time: 1 µs minimum, and a 1 µs starting value           | B    | Implemented       |
| D109 | Multi-node bidding tests: what they cover and how               | B    | Implemented       |
| D110 | Takeover: stand by after a lost round, plan if the winner is silent | C, B | Implemented       |
| D111 | Responder service time: 200 ms × severity | E, A | Implemented, see D144 |
| D112 | Client connections: ping through the gate, bounded send queues | F | Superseded by D167 for clients |
| D113 | Required client IDs; sequence numbers start at the clock | F | Implemented, amended by D184 |
| D114 | Bidding is the default; the leader only sequences the log | B, A | Implemented, amended by D163 |
| D115 | A different election timeout range per node | A | Implemented |
| D116 | Raft and the leader-run fault schedule stay, despite "no coordinator" | A, C | Superseded by D158 |
| D117 | Richer faults are not built | C | Accepted |
| D118 | Raft log and vote on disk, for physical power-off | A, D | Implemented |
| D119 | Peer calls through tarpc, with typed methods | C, A | Implemented, amended by D150, D167, D175, D176, D213 |
| D120 | Degraded mode: the last server answers without a commit | C, A, F | Implemented, amended by D213 |
| D121 | Degraded trigger: every peer suspected for 2 s | C | Implemented |
| D122 | Leaving degraded mode: replay through Raft, first commit wins | C | Implemented, see D142, D148 |
| D123 | `ReplyBody::Provisional`, and how the client reports it | C, F | Implemented, see D143 |
| D124 | Degraded holds: a local table on the service timer | C, E | Implemented |
| D125 | A returning node with no leader may stand for election after 3 s | D, C | Implemented |
| D126 | Group communication named: `PeerSet::broadcast` and `MeshClient` | C, B, F | Implemented, amended by D152, D157, D158, D210, D213 |
| D127 | FT-6: count elections after each `rejoined` | D, A | Implemented |
| D128 | `resource_usage`: memory, CPU, network and queues once a second | all | Implemented, amended by D185 |
| D129 | Experiment load: `plan_work`, responders and think time | E, F | Implemented, to confirm, amended by D165, D205, D219 |
| D130 | Experiment cells and the runner | all | Implemented, to confirm, amended by D139, D183, D186, D188, D189, D222, D223 |
| D131 | Publish-subscribe: Phase 2, for zones, docs only | all | Accepted, docs only, amended by D209 |
| D132 | The "entries table" is the spec's demo table | all | Implemented, amended by D207 |
| D133 | Proposal queue with grouped saves: disk saves cannot stop the heartbeats | A | Implemented |
| D134 | Default log filter: tarpc's per-call lines are off | all | Implemented |
| D135 | Rejoin: a wait runs out after 1 s, and the node asks the leader again | D, A | Implemented, amended by D225 |
| D136 | Reply gate: `serving` is checked again just before every reply | A, D | Implemented |
| D137 | A node that is already initialized does not call `initialize` | A | Implemented, corrected by D222 |
| D138 | What is measured: refused jobs, digests at fixed indexes, elections, takeovers and the client gap | all | Implemented, amended by D224 |
| D139 | Runner: silence is retried, a leftover program stops the batch, and a run with no fault is set aside | C, F | Implemented, to confirm, amended by D222, D223 |
| D140 | Incident fields: owner, category and TTL; one report, one incident ID | all | Implemented |
| D141 | Category decides the needed capability; `Security` becomes `Technical` | all, E | Implemented |
| D142 | Incident status is worked out on read, not stored | A, F | Implemented |
| D143 | Too old means not planned: the client states the age, the node commits nobody and answers `Expired` | all | Implemented |
| D144 | Assignment times: `assigned_at_ms` in the log, `expires_at_ms` as the planned end | A, C | Implemented |
| D145 | Reading records: `ControlRequest::Incidents` and `ctl incidents` | A, C | Implemented |
| D146 | Report owners under open-loop load | F | Implemented |
| D147 | Measuring refusals: `report_expired`, `reports_expired`, no TTL experiment cell | all | Implemented |
| D148 | Incident priority: an integer in `Plan`, computed by the planner | E, all | Implemented |
| D149 | What was left out of the incident model on purpose | all | Implemented |
| D150 | Peer calls enforce their own deadline, because tarpc times only what it has written | C | Implemented |
| D151 | A suspected peer stays suspected through this node's own silence | C | Implemented |
| D152 | Group communication built: `CloudGroup` with a per-node view, `send` and `probe` | C, B | Implemented, amended by D210 |
| D153 | View rule: a peer leaves after `degraded_after_ms` of unbroken suspicion and returns when heard; voters never change | C | Implemented |
| D154 | Bids and the failure draw follow the view; pings probe every configured peer | C, B | Implemented |
| D155 | `NodeStatus` gains `members`; `ctl status` shows them; the view number stays in `group_view` | C, all | Implemented |
| D156 | Application layer to `node/service.rs`; `send_ordered`, `src/middleware.rs` and `tests/layers.rs` enforce the split | A, C, F | Implemented, required by D211 |
| D157 | Reading of the class note on group communication; D126's question withdrawn | all | Implemented, amended by D210 |
| D158 | The three nodes elect the failed node: `FaultBid` rounds replace the leader's fault schedule | C | Implemented |
| D159 | How a fault-election round runs: who calls it, what closes it, numbering, timers, who may bid | C | Implemented |
| D160 | The fault gate has no exception again: `FaultOrder` and the last word are gone | C | Implemented |
| D161 | `fault_election` event and what is measured | C, all | Implemented |
| D162 | Elected faults in the experiments and the demo: FT-8 first in quick, `ft8-election-rate`, a timed last scene | C, F | Implemented, see D165 |
| D163 | What "no coordinator" means: one elected node orders the log, and no node decides for another | A, B, C | Implemented |
| D164 | Any serving node registers the campus responders | A, E | Implemented |
| D165 | Demo settings: E1's values, a load chosen by rule, node 3 first, fault timers shipped off | F, E, A | Implemented |
| D166 | Measuring the role: `leader_plan_share` in one-leader runs, and commit latency by path | A, B, all | Implemented |
| D167 | Reports travel over UDP through tarpc: `ClientRpc` on a small datagram adapter | C, F, all | Implemented |
| D168 | `ctl` stays on tarpc over TCP, on the same port number; `ctl status` checks UDP; no config key | C, F | Implemented |
| D169 | A call's life: a node that cannot answer stays quiet, the client's cancel ends the call, one deadline per submit | F, C, A | Implemented |
| D170 | The datagram adapter: one message is one datagram, and no error reaches tarpc | C | Implemented |
| D171 | UDP receive buffer, firewall rule and drop counting; new dependency `socket2` | C, all | Implemented; no run has had the buffer raised |
| D172 | What is measured on the UDP path: `udp_counters`, with cancels counted | all | Implemented |
| D173 | Accepted limits on the UDP path | F, C | Implemented |
| D174 | The fault gate on the client port | C | Implemented |
| D175 | Control as methods on `ClientRpc`; the old frames go; sockets back in `node/` | C, F, all | Implemented |
| D176 | RPC over RMI, and tarpc: the comparison with Java RMI, CORBA and Sun RPC/XDR | all | Implemented |
| D177 | Invocation semantics claimed per path | all | Implemented |
| D178 | How we read "For project use UDP", and what UDP really saves | all | Implemented |
| D179 | Library boundary: openraft and tarpc | all | Implemented |
| D180 | What was left out of the client path, and which tests changed | F, C | Implemented |
| D181 | User processes: 10 real programs per client PC share the users; the number is a setting | F | Implemented, amends D53, D54 |
| D182 | Load patterns (steady, random, all-at-once) and spreading the first sends | F | Implemented, amends D54, qualifies D129, amended by D191 |
| D183 | `scripts/start-users.sh`: one way to start and stop users | F | Implemented, amends D130, qualifies D132, amended by D190 |
| D184 | Client IDs and seeds when one PC runs many programs | F | Implemented, amends D51, D113 |
| D185 | `machine_usage`: the client PCs' own CPU and memory | F | Implemented, amends D128, amended by D191 |
| D186 | Runner checks: exact shares, leftovers, old run folders, runs left out | F, all | Implemented, amends D130, qualifies D112, amended by D189, D191 |
| D187 | One control cell, no limit check, and what the analysis adds | F, all | Implemented, qualifies D63, D129 |
| D188 | The runner over ssh: how a command reaches a machine, and a test with nothing shared | all | Implemented, amends D130, amended by D222 |
| D189 | The runner's stop, copy and refuse steps | all | Implemented, amends D130, D186 |
| D190 | What a killed script leaves behind, and signals that lost logs | F | Implemented, amends D183 |
| D191 | What the analysis counts: warm-up, dead programs, half a line, and which runs are left out | F, all | Implemented, amends D182, D185, D186 |
| D197 | A Phase 1 frontend: a web page on a small program on each client PC | F | Implemented |
| D198 | Frontend HTTP: hand-written on tokio, no new crate | F | Implemented |
| D199 | Frontend listen address: a required --listen flag, and no access control | F | Implemented |
| D200 | A second exception to the sockets rule, and status polls as another control sender | F, C | Implemented |
| D201 | Report form and answer card: fields, input checks, provisional banner, no-answer wording | F | Implemented |
| D202 | Load box: real loadgen programs, the split, the patterns, limits and Stop | F | Implemented |
| D203 | Node table: columns, last known, the unreachable rule, no same-state badge | F | Implemented |
| D204 | Zone updates panel: register a zone, its new reports and plans appear live | F | Superseded by D209, Phase 2 |
| D205 | The demo runs E1's plan_work and responders (amends D129) | F, E | Implemented, see D165 |
| D206 | The runner refuses leftover frontend, loadgen and node programs | F, all | Implemented through D139 |
| D207 | Demo script: page first, terminals as cross-check and fallback (amends D132) | all | Accepted |
| D208 | The frontend may land after the data: proof by binary hashes | F | Accepted |
| D209 | Publish-subscribe: first chosen to build small, then settled as Phase 2 by Lecture 9 | all | Implemented in the docs |
| D210 | Group communication: the client middleware's send to all nodes is the group send | F, C, all | Implemented in the docs |
| D211 | The middleware and application boundary, on both the client side and the cloud side | all | Implemented, the cloud side through D156 |
| D212 | Cloud nodes are peers, client and server are roles per transaction, and three system models | all | Implemented in the docs |
| D213 | The questions once left for the instructor are closed | all | Implemented |
| D214 | Test plan corrected: FT-2's target, FT-9, and names nobody had defined | A, C, D | Implemented in the docs |
| D215 | The library boundary names tarpc | all | Implemented in the docs |
| D216 | Noor's decisions of 2026-11-03, and those still being built | all | Accepted, built |
| D217 | The Raft leader does not change with load | A, B | Decided, amended by D218 |
| D218 | A busy leader steps aside, and a normal vote picks another | A, B, C | Implemented, amends D217, amended by D228 |
| D219 | 3000 responders in the top E1 cells | E, F | Implemented, amends D129 |
| D220 | After three stale plans, commit that nobody could be sent | A, E | Implemented, amends D35, D36 |
| D221 | A live log viewer that only reads the event logs | all | Implemented |
| D222 | Remote lines run in the repo folder: `cd` is its own command, and a missing folder exits 125 | C, F | Implemented, amends D130, D139 |
| D223 | A run gets its `run.json` only when it is whole | C, F | Implemented, amends D130, D139 |
| D224 | The client gap counts a client that never hears again | C | Implemented, amends D138 |
| D225 | A Raft that has stopped is not asked in a tight loop | D | Implemented, amends D135 |
| D226 | The split as built: what D156 left to settle | all | Implemented |
| D228 | A busy leader's log is read as one write, not as a queue | A | Implemented, amends D218 |

**Open decisions.** None remain in Phase 1's code: D5, D17, D39, D42, D52, D63 and D95 are settled, each in its
own entry. Any entry marked `Open` in future was a conflict between the Phase 1 architecture document and the
code, or a question nobody has answered. It stays open until someone implements the related code: whoever works
on it settles the decision first. During "grill with docs" the agent finds the open entries the task touches,
asks the person to decide with the options and recommendation listed in the entry, and updates the entry
(status, decided by, decision) in the same PR (AGENTS.md, "Design decisions"). D125, D129 and D130 were decided by
the agent on the instruction not to be asked again, and are marked for the team to confirm. Most of D134 to D208, D214,
D215, D219 and D220 were decided the same way, on Noor's instruction of 2026-11-03 to finish without more questions; each entry
says who decided, and Noor can overrule any of them. No question in this log is waiting for the instructor: D213
closes the ones older entries left. D216 lists Noor's decisions and what is built.

---

## 1. Coordination and consensus

### D1 · Raft leader election for the cloud coordinator

- **Date:** Phase 1 architecture (Oct 2026) · **Part:** A · **Status:** Implemented through openraft (D14), amended by D163 · **Decided by:** team, from the Phase 0 survey
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

- **Date:** 2026-10-20 · **Part:** A · **Status:** Implemented, amended by D137 · **Decided by:** A
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

- **Date:** 2026-10-20 · **Part:** A · **Status:** Implemented, amended by D133; its split is delivered by D166 · **Decided by:** A
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

- **Date:** Phase 1 architecture · **Part:** A · **Status:** Implemented; amended by D27, see D149 ·
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

- **Date:** 2026-10-20 · **Part:** A · **Status:** Implemented, see D143 · **Decided by:** A
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

- **Date:** 2026-10-20 · **Part:** A · **Status:** Implemented, see D143 and D147 · **Decided by:** A
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

- **Date:** 2026-10-20 · **Part:** A · **Status:** Implemented, amended by D133 · **Decided by:** A
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

- **Date:** 2026-10-20 · **Part:** A · **Status:** Implemented, amended by D138 · **Decided by:** A
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

- **Date:** 2026-10-20 · **Part:** A, E · **Status:** Implemented (campus generation: D97), amended by D164
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

- **Date:** Phase 1 architecture · **Part:** F · **Status:** Amended by D17, D167 · **Decided by:** team
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

- **Date:** 2026-10-20 · **Part:** all · **Status:** Implemented, amended by D119, D167 and D170 · **Decided by:** A, C
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

- **Date:** 2026-10-20 (scaffold), settled 2026-11-01 · **Part:** C, F · **Status:** Superseded by D167. TCP was confirmed for clients and peers were amended by D119, then Noor reversed the client part (D167, D216) · **Decided by:** A in the scaffold; omar.leithym
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

- **Date:** 2026-10-20 · **Part:** C · **Status:** Implemented, amended by D168 and D175 · **Decided by:** A
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

- **Date:** Phase 1 architecture · **Part:** C · **Status:** Superseded by D158 · **Decided by:** team
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

- **Date:** 2026-10-20 · **Part:** C · **Status:** Implemented; D78's amendment undone by D160 · **Decided by:** A, C
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

- **Date:** 2026-10-20 · **Part:** C · **Status:** Implemented, see D200 · **Decided by:** A, C
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
  learned, when `serving` goes false) and D136 · **Decided by:** team (Phase 1 §9.6), shaped in the scaffold
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

- **Date:** 2026-11-01 · **Part:** C · **Status:** Implemented, see D159 · **Decided by:** Aabed, on the
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

- **Date:** 2026-11-01 · **Part:** D (with A for the Raft hooks) · **Status:** Implemented, amended by D135 ·
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

- **Date:** 2026-11-01 · **Part:** D, A (contract change) · **Status:** Implemented, amended by D135 ·
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

- **Date:** 2026-11-01 · **Part:** C (config key in A's `config.rs`) · **Status:** Implemented, see D158 ·
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
  **Status:** Amended by D158, D159 and D160 · **Decided by:** Aabed, on the agent's recommendation
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

- **Date:** 2026-11-01 · **Part:** C · **Status:** Implemented, amended by D151 and D155 · **Decided by:** Aabed, on the
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

- **Date:** 2026-10-20 · **Part:** A, E · **Status:** Implemented, amended by D220 · **Decided by:** A
- **Context.** Phase 1 §8.2 said: recompute once from fresh state, then return "retry" to the
  client.
- **Decision.** The executor plans, proposes, and on `Stale` plans again from fresh state, up to
  `MAX_PLAN_ATTEMPTS = 3`. After that it stops without replying (D36); the client's retry starts
  over.
- **Why.** Under contention two attempts are often not enough when several executors chase the same
  few free responders. Three keeps the loop short.
- **Where.** `node/mod.rs::execute`, `MAX_PLAN_ATTEMPTS`.

### D36 · A node that cannot answer stays quiet

- **Date:** 2026-10-20, confirmed 2026-11-01 · **Part:** A, F · **Status:** Implemented, confirmed, amended by D220 · **Decided by:** A; confirmed by omar.leithym, on the agent's recommendation
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

- **Date:** 2026-10-20 · **Part:** A · **Status:** Implemented, see D143, amended by D169 · **Decided by:** A
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

- **Date:** 2026-11-01 · **Part:** B · **Status:** Implemented, amended by D150 and D154 · **Decided by:** Ahmed Abdeen, on the
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

- **Date:** 2026-10-30 · **Part:** E · **Status:** Implemented, amended by D138 · **Decided by:** Yahia Kilany
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

- **Date:** 2026-10-20 · **Part:** E · **Status:** Implemented (open part settled 2026-10-30), see D148
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

- **Date:** 2026-10-20 · **Part:** all · **Status:** Implemented (generation: D97, D99), amended by D140, D141 and D148
- **Decision.** `Point { x, y }` in metres as `i32`, Manhattan distance. `Capability` is one of
  `Medical`, `Fire`, `Security`, `Rescue`. `Incident { zone, location, severity (1..=5), needs,
  reported_at_ms }`, where `reported_at_ms` is the client clock. `Responder { id, zone, location,
  capability }`. `Plan { responder: Option<ResponderId>, cost }`. Defaults: 6 zones, 30
  responders, seed 42.
- **Why.** Integers keep replicated state deterministic (D30). Four capabilities cover the project's
  incident examples (fire, medical, security) plus search and rescue.

### D93 · Planner pick rule: exact capability, nearest, cost in metres

- **Date:** 2026-10-30 · **Part:** E · **Status:** Amended by D101, see D148 · **Decided by:** Yahia Kilany
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

- **Date:** 2026-10-30 · **Part:** E · **Status:** Amended by D101, see D148 · **Decided by:** Yahia Kilany
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

- **Date:** 2026-10-30, settled 2026-11-01 · **Part:** A, E · **Status:** Implemented, see D143 and D148 · **Decided by:** omar.leithym, on the agent's recommendation
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

- **Date:** 2026-10-30 · **Part:** E, F · **Status:** Amended by D102 and D141 · **Decided by:** Yahia Kilany
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

- **Date:** 2026-10-20 · **Part:** F · **Status:** Implemented, amended by D113 and D184 · **Decided by:** A, F
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

- **Date:** 2026-10-20 · **Part:** F · **Status:** Implemented (`client.rs`), retries per D52, amended by D167, D169 and D181 · **Decided by:** team; details by omar.leithym, on the agent's recommendation
- **Decision.** `MeshClient` keeps one connection per node, shared by all simulated users in the
  process via `Arc`. `submit` sends the request to every node, waits `client_retry_ms` for the
  first `Done` with that ID, resends with the same ID, and logs `client_sent`, `client_done`,
  `client_gave_up`.
- **Why.** Thousands of users cannot each hold three TCP connections. Matching by request ID makes
  sharing safe.

### D54 · Load generator: closed loop first, open loop as a mode

- **Date:** 2026-10-20, built 2026-11-01 · **Part:** F · **Status:** Implemented (`bin/loadgen.rs`), amended by D146, D181 and D182, see D173 · **Decided by:** team; open-loop details by omar.leithym, on the agent's recommendation
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

- **Date:** 2026-10-20 · **Part:** all · **Status:** Implemented, amended by D134
- **Decision.** `tracing` to stderr for people (`warn!` unexpected but handled, `info!` rare lifecycle
  events, `debug!` per request). Default filter `info,openraft=warn`, overridden by `RUST_LOG`.
  Anything we measure goes to `metrics::Event`, never only to `tracing`.

### D63 · Experiment method: repetitions, warm-up, run length

- **Date:** 2026-10-27, settled 2026-11-02 · **Part:** all · **Status:** Implemented, qualified by D162 and D187 · **Decided by:**
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

- **Date:** 2026-10-20 · **Part:** all · **Status:** Implemented, amended by D133 · **Decided by:** A, starting from
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

- **Date:** 2026-10-20 · **Part:** all · **Status:** Implemented, amended by D179 and D215
- **Decision.** openraft (D14) is the only library doing distributed-systems work. Everything else
  is ours: state machine, storage, network adapter, transport, fault gate, bidding, planner, client
  middleware, fault injection, rejoin. Libraries handle async I/O (tokio), encoding (serde, bincode,
  serde_json, toml), CLI (clap), errors, randomness and logging. A new dependency needs a
  design-log entry; a new distributed-systems library needs team agreement.
- **Report notes.** The report must separate library code from ours.

### D69 · Phase 2 hooks

- **Date:** Phase 1 architecture, accepted 2026-11-02 · **Part:** all · **Status:** Accepted, docs only, amended by D163 ·
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

- **Date:** 2026-11-01 · **Part:** E, A · **Status:** Implemented, see D144 · **Decided by:** omar.leithym, on the agent's recommendation
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

- **Date:** 2026-11-01 · **Part:** F · **Status:** Superseded by D167 for clients. See also D143, D186 and D200 · **Decided by:** omar.leithym, on the agent's recommendation
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

- **Date:** 2026-11-01 · **Part:** F · **Status:** Implemented, amends D51, amended by D184 · **Decided by:** omar.leithym, on the agent's recommendation
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

- **Date:** 2026-11-01 · **Part:** B, A · **Status:** Implemented, supersedes D34, amended by D163 · **Decided by:** omar.leithym
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

- **Date:** 2026-11-01 · **Part:** A, C · **Status:** Superseded by D158: Raft stays, the fault schedule does not. Its question is closed by D213, and the leader's role is settled by D163 · **Decided by:** omar.leithym
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

- **Date:** 2026-11-01 · **Part:** C, A · **Status:** Implemented, amends D16, D17, D19, amended by D150, D167, D175 and D176, see D158, question closed by D213 · **Decided by:**
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

- **Date:** 2026-11-02 · **Part:** C, A, F · **Status:** Implemented, a deliberate exception to D8, question closed by D213 ·
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

- **Date:** 2026-11-02 · **Part:** C · **Status:** Implemented, see D142 and D148 · **Decided by:** omar.leithym, on the
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

- **Date:** 2026-11-02 · **Part:** C, F · **Status:** Implemented, see D143 · **Decided by:** omar.leithym, on the
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

- **Date:** 2026-11-02 · **Part:** C, B, F · **Status:** Implemented, amended by D152, D157, D158, D210 and D213 · **Decided by:** omar.leithym, on the
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

- **Date:** 2026-11-02 · **Part:** all · **Status:** Implemented, amended by D185 · **Decided by:** omar.leithym, on the
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

- **Date:** 2026-11-02 · **Part:** E, F · **Status:** Implemented, to confirm, amended by D165, D205 and D219, qualified by D182 and D187 · **Decided by:** the agent,
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

- **Date:** 2026-11-02 · **Part:** all · **Status:** Implemented, to confirm, amended by D139, D183, D186, D188, D189, D222 and D223 · **Decided by:** bash over SSH by
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

- **Date:** 2026-11-02 · **Part:** all · **Status:** Accepted, docs only, amended by D209: its question is closed, and Lecture 9 settles Phase 2 · **Decided by:** omar.leithym: "we
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

- **Date:** 2026-11-02 · **Part:** all · **Status:** Implemented, amended by D207, qualified by D183, see D145 · **Decided by:** omar.leithym, on the agent's
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

### D133 · Proposal queue with grouped saves: disk saves cannot stop the heartbeats

- **Date:** 2026-11-03 · **Part:** A · **Status:** Implemented, amends D26, D29 and D40 · **Decided by:** Noor Hany Emam
  (the queue with grouping); the values and small choices below by the agent on its recommendation, on Noor's instruction to
  finish without more questions (2026-11-03)
- **Context.** The pretend network found the leader going silent while it saved a wave of reports, and the followers then voting
  without end, though nothing had failed. openraft waits for one disk flush per log entry inside its core task, and the core can
  work through up to 1000 queued writes before it looks at the tick that sends leader heartbeats (openraft 0.9.25,
  `core/raft_core.rs:726-727`, `:990-1017`, `:1281`; our `raft/store.rs` flushes in `append`). So N proposals waiting in openraft
  hold the core for N flushes. With saves on and 100 users on each of 3 load generators, the cluster moved to a new term 0.78 s
  into the load and went through 225 new terms in the run, and not one report was answered: 600 gave up. `propose_timeout_ms`
  also was no cap at all, because `client_write` was awaited with no limit. Noor's rule, their words:

  > "You should either queue it or block it for 3 seconds... You should leave like 1% to 2% for the node to say I'm alive... The
  > saving to disk and so on, these are like system necessities of the node. This shouldn't be overridden by something like
  > processing requests."

- **Options considered.**
  1. *Our own queue in front of Raft, and everything waiting goes in one save.* One `client_write` in flight per node. A proposal
     that waits over 3 s leaves the queue and the client resends. The core is free between saves, so heartbeats go out on time,
     and the load decides how much one save carries instead of how many saves there are. It changes the log's payload type and
     adds one module.
  2. *The queue with no grouping: one command per write.* Same safety for the heartbeats and no new log type. By arithmetic about
     200 to 400 reports a second, while experiments send up to 3000. Measured it was far worse than that arithmetic: 4.6 answered
     per second per generator at 100 users.
  3. *Saves off in the experiments.* No code. It measures a system that cannot survive power-off, which is the demo's own fault.
  4. *Longer vote timers.* One line. But the wave grows with load, so no fixed timer is long enough, and every failover gets
     slower.
  5. *Block new work for 3 s*, the other half of Noor's rule. Blocking a caller and letting it wait in a queue cost the caller the
     same. The queue also gives one place to group. The 3 s stays, as the wait limit.
- **Decision.** Option 1. `raft/proposals.rs::ProposalQueue` is a node's one way into `raft.client_write`, and grouping is the only
  path: no setting and no environment variable turns it off.
  - **One write at a time.** One task owns the queue and starts a `client_write` only when the last has ended.
  - **Groups.** Whatever waits when a write ends goes out together as the next write, as one log entry of the new type
    `proto::CommandGroup(Vec<Command>)`, up to `proposals::GROUP_LIMIT` = 200 commands. openraft's types become `D = CommandGroup`
    and `R = Vec<Outcome>`, with one outcome per command, in order. Under light load a group is one command and nothing waits.
  - **Deadlines.** Each proposal has a deadline of `timers.propose_timeout_ms` (3 s) from when `propose` or `propose_local` was
    called, and it leaves with `ProposeError::Timeout` at that moment, whether it is still waiting or its write is in flight. The
    node sends no reply and the client's retry takes care of it, as for a full worker pool (D48). A proposal past its deadline
    never costs a save.
  - **The write cap.** A write in flight goes on after its proposals leave, and the next one waits for it: under overload the
    head of the queue is always about to expire, and a new write for each of those would pile up inside openraft after all. Only a
    write that outlasts `propose_timeout_ms` (a leader without a majority) stops being waited for.
  - **Forwarding.** On the leader, forwarded proposals go through the same queue as its own (`propose_local`). A follower that
    knows the leader forwards straight away, without touching its own queue, and a node asked to `propose_local` while it knows
    another leader answers `NotLeader` at once. A forward now waits up to the proposal's deadline instead of
    `peer_rpc_timeout_ms` (500 ms), because the leader's queue may rightly hold it that long and re-sending would only add the same
    command to that queue. A change of leader ends the wait early (`RaftNode.leader`, a watch that changes only when the leader
    does).
  - **Applying.** `CloudState::apply_group` applies a group's commands in order, each through the existing `apply` at the entry's
    log index, so inner commands stay idempotent and the digest is what the same commands one after another at that index give.
    Results of one group share a `log_index`. A blank or membership entry answers with no outcomes.
  - **Catch-up.** `store::MAX_COMMANDS_PER_MESSAGE` = 4096 caps one AppendEntries over its entries (at least one entry always
    goes), through `limited_get_log_entries`. openraft gives each AppendEntries one `raft_heartbeat_ms` to be sent, saved and
    answered, and on a timeout sends the same entries again, so a message too big for that never completes and a returning node
    never catches up.
  - **What is measured.** A new event, `raft_io`, once a second per node: `waiting`, `writes`, `commands`, `expired`,
    `write_timeouts`, `syncs`, `sync_us`, `max_sync_us`, `saves`, `saved_entries`, `saved_commands`, `max_leader_gap_us`.
- **Why.** It follows Noor's rule: the node's own necessities, here saying it is alive, are no longer overridden by processing
  requests. The pile stays where the core cannot see it, and the queue costs nothing at low load. Measured with a prototype of
  this design on one PC, saves on, 3 generators, 60 s measured, 2 runs where shown. Per second is per generator:

  | Users per generator | No queue (today's path) | Queue, no grouping | Queue with grouping |
  |---|---|---|---|
  | 100 | 0.0 answered, 600 gave up, 225 new terms | 4.6 answered, 143 gave up, 0 new terms | 95.2 and 96.4 answered, 0 gave up, 0 new terms |
  | 333 | 0.0 answered, 1995 gave up, 391 new terms | 5.1 answered, 1552 gave up, 0 new terms | 156.9 and 213.9 answered, 8 and 2 gave up, 0 new terms |
  | 1000 | 0.0 answered, 6000 gave up, 517 new terms | 12.3 answered, 4877 gave up, 0 new terms | 128.0 and 129.4 answered, 1014 and 1003 gave up, 0 new terms |

  The two columns on the right show what each half does: the queue alone stops the new terms, and grouping is what brings the
  answers back. At 100 users a save carried 4.7 to 7.1 commands, at 1000 users 41 to 42. Every group run ended with equal digests on all
  three nodes. 200 for `GROUP_LIMIT` is high enough never to limit a run: at one save per round of about 7 ms that is over 28,000
  commands a second, and at 25 ms per round 8000, against about 3500 from 3000 users. It is low enough for the wire: 300 full
  groups in one message would be at most 7.4 MB at 123 bytes per command, under the 16 MiB frame. The prototype ran with 256; it
  never came near that limit, and the limit came down because D141 to D148 made a command longer, from 76 to 123 bytes at the
  longest, and 256 would have left less than half a frame spare.
- **Consequences.** A log entry no longer means one command: `log_index` is shared within a group, and `commit` events can repeat
  one. Counting entries no longer counts commands, so use `saved_commands` or the `commit` events. The digests differ from any
  earlier run, and the Raft log file's format changed, so `data/raft` from an older build must be wiped once. A report that waits
  3 s gets no answer from that node, so under overload the client sees retries instead of elections. The queue has no length limit
  but it has a time limit: it holds at most what arrives in one `propose_timeout_ms`, and arrivals are limited by the worker pool
  and by the responders held. In the one case of a leader without a majority, a write can be abandoned at the cap while it is still
  inside openraft, so at most one more per 3 s can be there, and if one commits later that is harmless, because every command is
  idempotent. Peer calls for forwarded proposals can now be in flight for up to 3 s rather than 500 ms. This fixes the disk half
  only. The CPU half stays D48: lab config threads at most cores minus 1. Grouping also did not make overload free: at 333 and
  1000 users on this one PC, p99 was still 6 to 25 s and there were 20,000 or more takeovers per run. That was measured with every
  program sharing one machine, so it needs its own check on the six PCs.
- **Where.** `src/raft/proposals.rs::{ProposalQueue, GROUP_LIMIT, QueueStats}`, `src/raft/mod.rs::{RaftNode, write_group,
  propose_with_retry, spawn_io_report}` and the `Consensus::propose_local` implementation, `src/raft/store.rs::{LogStats,
  MAX_COMMANDS_PER_MESSAGE, limited_get_log_entries}`, `src/raft/network.rs::HeartbeatStats`, `src/proto.rs::CommandGroup`,
  `src/state.rs::CloudState::apply_group`, `src/metrics.rs::Event::RaftIo`, the comments on `timers.propose_timeout_ms` and
  `timers.peer_rpc_timeout_ms` in `src/config.rs`.
- **How we check it.** Unit tests in `raft/proposals.rs`: `only_one_write_is_in_flight_and_the_next_carries_all_that_waited`,
  `a_group_stops_at_the_limit_and_order_is_kept`, `a_proposal_past_its_deadline_leaves_with_timeout_and_is_never_written`,
  `a_caller_that_leaves_does_not_let_a_second_write_start`, `a_write_that_never_ends_stops_holding_up_the_queue_after_the_cap`,
  `a_failed_write_answers_everyone_in_its_group`, `a_short_answer_is_an_error_not_a_hang`, `a_stopped_queue_answers_with_fatal`,
  and `a_full_catch_up_message_fits_in_a_frame`, which holds the sizes above to the real encoding. In `state.rs`:
  `a_group_gives_the_same_state_and_digest_on_two_copies`, `a_group_is_the_same_as_its_commands_one_after_another_at_one_index`,
  `the_order_inside_a_group_changes_the_digest`, `results_of_one_group_share_its_log_index`, `a_command_in_a_group_is_idempotent`.
  In `store.rs`: `one_message_stops_at_the_command_cap` and its neighbours. On three real nodes, `tests/proposals.rs`:
  `a_burst_shares_log_entries_and_every_proposal_gets_its_own_answer`,
  `a_follower_forwards_straight_away_into_the_leaders_one_queue`, `a_leader_that_cannot_commit_answers_timeout_at_the_deadline`,
  `a_returning_follower_catches_up_over_full_groups`. The table above is from the prototype on commit a98a1c3, before the other
  gaps merged. The final code, after every gap's merge, had one short run on the same PC at the same load: saves on, 100 users on
  each of 3 generators, 30 s measured, a debug build. It gave 95 answered per second per generator, 0 gave up, 0 retried, term 2
  from start to end, equal digests at log index 3795, 6.4 commands per save, and a longest gap between AppendEntries of 44 ms. It
  is one run, not a replacement for the table. **It has not run on the lab machines.** On the six PCs: one `e1-300-bidding` must
  show no new terms, and `raft_io` gives the real disk: `sync_us / syncs` and `saved_commands / saves`.
- **Report notes.** A good story for the design section: a saved disk write looked like a dead leader, and the fix came from
  treating "I am alive" as more important than work. Worth one small figure of answered reports and new terms against the three
  designs, but with lab numbers: the numbers above are rehearsal numbers. Say plainly that Raft still has one leader and that the
  queue changes only how work reaches it.

### D134 · Default log filter: tarpc's per-call lines are off

- **Date:** 2026-11-03 · **Part:** all · **Status:** Implemented, amends D58 · **Decided by:** the agent on its
  recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** D119 brought in tarpc for peer calls. tarpc logs through `tracing` like everything else: several `info`
  lines for every call (`SendRequest`, `ReceiveResponse`, `ReceiveRequest`, `BeginRequest`, `CompleteRequest`,
  `BufferResponse`, `SendResponse`), and an `error` line, `DeadlineExceeded`, for every call past its deadline, from its
  two `in_flight_requests` modules. D58's default filter, `info,openraft=warn`, let all of them through. Each line is one
  write to stderr from an async task, which is against the rule that nothing blocks the async runtime, and at load there
  are thousands a second. Measured on this one PC, with 3 load generators at 100 users each, no disk saves and node output
  going to the Windows disk: with the old filter each generator got 32 reports a second, the cluster reached term 19 and
  the nodes wrote about 72 MB of output in 30 s. With `RUST_LOG=warn` each generator got 100 a second and the cluster
  stayed on term 2. Another rehearsal, `nosync-50`, wrote 225,596 tarpc lines in 63 s. So logging alone was enough to slow
  a node down until it lost its leader. Every load figure taken before this change partly measured the logging.
- **Options considered.**
  1. *`info,openraft=warn,tarpc=warn,tarpc::client::in_flight_requests=off,tarpc::server::in_flight_requests=off` as the
     default in `src/lib.rs`.* The per-call lines go, tarpc's warnings stay (`Connection broken`, and what a mixed build
     prints), and it holds for hand runs too. It is long, and it depends on tarpc's module names, which `Cargo.lock` pins.
  2. *`info,openraft=warn,tarpc=off`.* Shorter, but it also hides the warning that gives away a broken connection or two
     machines on different commits, which is exactly what you want to see in the lab.
  3. *Set `RUST_LOG` in the scripts only.* No code change, but `docs/demo.md`, `docs/runbook.md` and the README all start
     programs by hand, so the demo would run the slow way.
  4. *`warn` for everything.* This is what was measured. It also loses our own rare `info` lines (`raft state`, `serving
     again`, the one-a-second heartbeat summary), which people read during the demo.
  5. *Write logs from a background thread (`tracing-appender`).* This would stop the blocking but not the volume, and it
     adds a dependency a day before the deadline.
- **Decision.** Option 1, as the public constant `rescuemesh::DEFAULT_LOG_FILTER`, used by `init_tracing`. `RUST_LOG`
  still overrides it completely. No script sets `RUST_LOG`.
- **Why.** It removes the cost and keeps every line that is rare enough to be worth reading. The more specific directive
  wins in `EnvFilter`, so `tarpc::client::in_flight_requests=off` beats `tarpc=warn`. The two modules must be off rather
  than `warn` because their `DeadlineExceeded` is at `error` level, and under load it fires for every late call.
- **Consequences.** A late peer call no longer prints a line, but it still shows up as what it causes: `peer_suspected`,
  `takeover_started`, `propose_failed`. Anyone debugging a call sets `RUST_LOG=info,tarpc=info`. Our own `info` lines are
  still written to stderr from async code, but they are few and bounded, so that is accepted. Load figures from before
  this change must not be compared with figures from after it.
- **Where.** `src/lib.rs::{DEFAULT_LOG_FILTER, init_tracing}`.
- **How we check it.** `tests::every_directive_in_the_default_filter_parses`, because `EnvFilter::new` silently drops a
  directive it cannot read. `tests::the_default_filter_drops_tarpcs_call_lines_and_keeps_the_rest` writes one line for each
  case through the real filter. And the same setting as the evidence above, run again on this PC with a release build and the
  node output on the Windows disk: with the new default the first generator got 97.8 reports a second, the cluster stayed on
  term 2 with equal digests, and the three nodes wrote 15,849 bytes in about 37 s with no tarpc line. With `RUST_LOG=warn`,
  right after it, the figures were 98.8 a second and term 2. So the new default costs nothing against `warn`.
- **Report notes.** A small lesson worth one sentence: logging is on the measured path. The numbers above are rehearsal
  numbers, so report only the fact that logging was found and removed, and take any figure from the lab.

### D135 · Rejoin: a wait runs out after 1 s, and the node asks the leader again

- **Date:** 2026-11-03 · **Part:** D, with A for one comment in `raft/mod.rs` · **Status:** Implemented, amends D75 and D76, amended by D225 ·
  **Decided by:** the agent on its recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** A returning node asks the leader for a target index and serves once it has applied that far (D75, D76). D76
  calls the answer the leader's commit index. It is not quite that: openraft's `get_read_log_id` returns the larger of the
  commit index and the leader's first entry of its term, its no-op (`engine/handler/leader_handler/mod.rs:116-120`). For a
  leader just elected, that entry is not committed yet. If that leader is then replaced, the entry can be cut from every
  log. In the rehearsal `silence-leader/leader`, node 3 led term 101 and wrote its no-op at index 4004. It answered node
  2's question with 4004, was replaced, and index 4004 was truncated. The final log ended at 3497, with equal digests on
  all three nodes. Node 2 went on waiting for 4004, and 150 s after the load ended it was still `serving = false`:
  `rejoined: NEVER`. That loses E4's number and would break scene 4 of the demo live.
- **Options considered.**
  1. *The wait lasts 1 s, then the node asks again and keeps the lowest answer.* It needs no new message, and it finishes
     in every case where a leader exists. It adds up to 1 s to a recovery that hit such a target, and sends one more
     `ReadIndex` a second while a node catches up.
  2. *Ask again and always take the newest answer.* Simpler to say. But under load every answer is higher than the last,
     so a node catching up at about the speed of the cluster keeps chasing a moving line, and E4 would measure that chase
     instead of the catch-up.
  3. *The leader answers only with an index it has applied*, the second half of `ensure_linearizable`. Then the answer
     really is committed and never vanishes, so no asking again is needed. But the answer waits for a commit inside a peer
     call with a 500 ms deadline, right when the cluster is unsettled and commits are slowest. It changes the leader's side
     in `raft/mod.rs`, where another job is working, and it never answers a leader that cannot commit.
  4. *Ask again only when the term or leader changes.* Fewest messages and exactly the case that goes wrong, but it adds a
     second trigger to reason about, for one message a second that costs nothing.
  5. *Serve anyway after a time limit.* Rejected: it breaks "a returning node serves nothing until it is current".
- **Decision.** Option 1. `node/rejoin.rs::reach_target` takes its first target from `learn_target` as before: it waits for a
  sign of a current leader and retries every Raft heartbeat. It then waits with `wait_applied(target, ASK_AGAIN_AFTER)`, where
  `ASK_AGAIN_AFTER` is a constant of 1 s. When that runs out it asks once, within `peer_rpc_timeout_ms`, and keeps the lower of
  the old target and the answer. A question that fails changes nothing, and it goes back to waiting on the target it has. A
  lowered target logs the new event `rejoin_target_lowered { from, to }` and a `warn!`. `rejoined.leader_commit` is the target
  the node finally reached. `WAIT_WARNING`, 5 s, now only limits how often it warns. No new config key and no wire change.
- **Why.** Taking the lowest answer is safe. Every answer is at least the commit index at the moment it was given, and the
  commit index only grows, so even the lowest answer covers everything that was committed before the first question. That
  is all D76 ever promised: what was committed when the node asked. It also means a target never goes up because of load.
  1 s is short enough to matter little in E4 and long enough to cost almost nothing. There is one question per wait, and no
  retry loop like `learn_target`'s, because a leader confirms itself with a round of heartbeats for each question, and
  retrying every heartbeat would add about 20 rounds a second to a cluster that is already struggling. A target from a leader
  that is still there is not disturbed: the wait just goes on from where it was.
- **Consequences.** Recovery can take up to 1 s longer after a lost target, and `rejoin_target_lowered` says when that
  happened, so E4 can be read honestly. `read_index` counts rise by about one a second per node catching up, and they count
  in E5. Under churn the leader's confirmation can print openraft's `timeout while confirming leadership` about once a
  question. In a stretch with no leader the questions fail, and the node stays not serving until one answers or its target
  is reached, as before. D125 is untouched: the 3 s election rule runs beside this.
- **Where.** `src/node/rejoin.rs::{ASK_AGAIN_AFTER, catch_up, reach_target, learn_target}`,
  `src/metrics.rs::Event::RejoinTargetLowered`, the comment on `Consensus::read_index` in `src/raft/mod.rs`,
  `src/node/testing.rs` (a scripted `Consensus` for unit tests), `analysis/analyze.py` (`rejoin_targets_lowered`).
- **How we check it.** Unit tests on the scripted `Consensus`, because a real cluster reaches this state only by luck
  under churn: `a_target_that_never_commits_does_not_hold_the_node_back_for_ever` (replays 4004 and 3497),
  `asking_again_does_not_raise_the_target`, `with_no_leader_to_ask_the_old_target_still_counts`,
  `a_leader_with_nothing_committed_needs_no_wait`, `silenced_again_while_waiting_it_stays_not_serving`. With asking again
  taken out, the first three fail by timing out. `tests/rejoin.rs` still passes unchanged. In the trial runs on one machine
  (D139) every returning node finished, but no target was lowered, so this path has only run on the scripted `Consensus` so far.
  Look for `rejoin_target_lowered` in the first real runs under churn.
- **Report notes.** Library-provided: `get_read_log_id` and its majority check. Team-built: asking again and keeping the
  lowest answer. A good small story for the report: a rehearsal found a real liveness bug that no test on an idle cluster
  could, and reading openraft's source explained it. Cite the Raft thesis on ReadIndex, which waits for the leader's
  first commit of its term, for why the no-op matters at all.

### D136 · Reply gate: `serving` is checked again just before every reply

- **Date:** 2026-11-03 · **Part:** A, D · **Status:** Implemented, amends D46 · **Decided by:** the agent on its
  recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** AGENTS.md says a returning node does not reply until it is current. `handle_request` checked `serving` only
  when a request arrived (`node/mod.rs`). A handler can then wait on a commit for a long time. If the node is silenced
  meanwhile and comes back, the gate is open again but the node is still catching up, and when the commit finishes the
  handler replies. In the rehearsal `silence-leader/leader` the victim logged 109 `replied` events between `restored` and
  `rejoined`, every one more than 3 s old, the oldest 46 s. No client took one of those answers first, and each answer was
  committed, so it was safe, but it broke the written rule and it would have needed explaining in scene 4 of the demo.
- **Options considered.**
  1. *Check `serving` in `send_done` and `send_provisional`, the two places every reply goes through.* Two lines at the
     point that matters, and no path can miss it.
  2. *Check after each `await` in `handle_request`.* Many checks, and a new path could forget one.
  3. *Check in `ReplyTx::send`, beside the fault gate.* One place, but the transport would need to know about `serving`,
     and D23's control replies go through the same call. Another job is also changing the client path there.
  4. *Change the written rule to "may reply with committed results while catching up".* It is safe, because the answer is
     committed. But it gives up a simple rule for no gain, since the client's retry gets the answer anyway.
- **Decision.** Option 1, through one small function, `may_reply`. A node that is not serving stays quiet and logs at
  `debug`, as for any other answer it cannot give (D36). `replied` and `provisional_replied` are logged only for replies
  that leave, as before. The check at the top of `handle_request` stays, so a request that arrives while not serving does
  no work.
- **Why.** The rule is about what a client can see from a node, so the check belongs where a reply leaves. Nothing is lost:
  the result is committed, so the client's retry gets it from any serving node (D37), this node included once it is current.
- **Consequences.** A request that was waiting on a commit when its node went silent costs its client one retry, about 2 s,
  if no other node answers first. In the rehearsal no client got its answer from those late replies, so no measurable cost
  is expected. There is still a tiny window between the check and the send, which is the same as any reply on its way.
- **Where.** `src/node/mod.rs::{may_reply, send_done, send_provisional}`, `src/node/transport.rs::ReplyTx::for_tests`.
- **How we check it.** `a_commit_done_while_the_node_is_not_serving_gets_no_reply` and its twin
  `a_serving_node_replies_once_the_commit_is_done`, which hold a handler at the commit on a scripted `Consensus`;
  `a_provisional_answer_also_needs_a_serving_node`. With the check taken out, the first and third fail. In a run, `replied`
  events between `restored` and `rejoined` must be 0. They were 0 in the trial runs on one machine (D139), one of them at 300
  users, but those ran a debug build and are not results.
- **Report notes.** One line under the rejoin section: the gate is enforced at the moment of replying, not only at arrival.

### D137 · A node that is already initialized does not call `initialize`

- **Date:** 2026-11-03 · **Part:** A · **Status:** Implemented, amends D25, corrected by D222 · **Decided by:** the agent on its recommendation,
  on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** D25 has every node call `initialize` at startup and ignore `NotAllowed`. Since D118 a node can restart with
  its log from disk, and then openraft refuses and logs an `ERROR`, `Can not initialize`
  (`engine_impl.rs::check_initialize`). The refusal is harmless, but the line prints on every restart, including the power-off
  scene of the demo, and it teaches people to ignore errors.
- **Options considered.**
  1. *Ask `raft.is_initialized()` first, and call `initialize` only if the answer is no.* It is openraft's own rule, so it
     cannot disagree with `check_initialize`. The call is also a round trip to Raft's core, which keeps something else
     working (see Why).
  2. *Skip it when our `restarted` flag is set.* The first idea, and equal in effect: `Replayed::is_restart` is true in
     exactly the cases `is_initialized` is. But this removes the round trip, and then
     `consensus.status()` right after start can read empty metrics. `tests/durable.rs` caught this: "the log came back from
     the file" failed in 3 of 6 runs.
  3. *Filter the line out with `openraft::engine=off`.* Hides other errors from that module too.
  4. *Leave it and explain it in the runbook.* The line would stay in every demo restart.
- **Decision.** Option 1, in `RaftNode::start`. The `NotAllowed` arm stays on the `initialize` call. `restarted` still
  decides `enable_elect` and what `rejoin::spawn` does.
- **Why.** Nothing changes for a first start: peers' messages wait until the node's context exists (D61), so no vote can
  arrive before `initialize`. Openraft's first metrics are published before its main loop starts (`do_main`), and
  `is_initialized` is answered from inside that loop, so when it returns the metrics are there. `initialize` was giving us
  that by accident, and `Event::Restarted`, the `rejoin_started` event and tests all read them straight away.
- **Consequences.** A restart now prints no `ERROR`, so one that does appear means something. Membership on a restarted node
  comes only from its file or from the leader, which was already true, because openraft refused the call anyway.
- **Where.** `src/raft/mod.rs::RaftNode::start`, `tests/restart_log.rs`.
- **How we check it.** `tests/restart_log.rs::a_restart_logs_no_error`: a one-node cluster with storage, stopped and started
  again, with a global log subscriber catching what the process prints. It failed before the fix with `ERROR
  openraft::engine::engine_impl: Can not initialize`. It is in its own test binary because a process has only one global
  subscriber. `tests/durable.rs` ran 10 times in a row green after the change.
- **Report notes.** None needed.

### D138 · What is measured: refused jobs, digests at fixed indexes, elections, takeovers and the client gap

- **Date:** 2026-11-03 · **Part:** all (metrics are shared), with A for `raft/store.rs` and E for the event at the worker
  pool · **Status:** Implemented, amends D31, settles D73's note, amended by D224 · **Decided by:** the agent on its recommendation, on
  Noor's instruction to finish without more questions (2026-11-03)
- **Context.** Five things stood between the logs and honest numbers.
  1. A job the worker pool refuses left only a `debug` line, the client hears nothing and retries 2 s later, and so shed
     work looked the same as slow work. D73 said this event must exist before E2 runs. It never got added.
  2. The spec asks for convergence to be shown and reported. `state_digest` is logged once a second at whatever index the
     node has reached, so under load two nodes almost never log the same index, and the logs cannot show equal state.
  3. `analyze.py` counted elections as terms seen minus one, assuming the first term seen was already running. That term
     logs nothing inside the window unless something changes, so a window that saw only new terms came out one too low.
  4. `analysis/README.md` listed `takeover_wait_mean_ms` under failover. But a `backstop` takeover needs no fault at all, only
     a busy winner. In the rehearsal `silence-leader/leader`, all 3,694 takeovers were backstops, and not one was on a round
     the victim had won.
  5. E3 promises a client-visible gap (`architecture.md` section 11.2, `experiments/README.md`), and nothing computed it. With
     item 4 corrected, the failover row would have had nothing a client could see.
  Items 1 and 2 cannot be added after the runs, because the logs would not hold them. Items 3 to 5 can, but they cost little.
- **Options considered.**
  - *Refused jobs:*
    1. *A new event `overloaded { request, attempt, queued, running }` at `plan_on_workers`,* the one place all planning
       passes through, including degraded mode.
    2. *A counter in `resource_usage`.* Fewer lines, but it cannot say which request or attempt was refused.
    3. *A `warn!`.* D73 already rejected it: thousands of lines a second to stderr, which D134 has just shown to be costly,
       and it would not be a measurement anyway.
  - *Digests at fixed indexes:*
    1. *Log from `StateMachineStore::apply` at every index that is a multiple of 100,* with any entry counting.
    2. *Log from `CloudState::apply`.* It would put logging in the deterministic core, against the architecture rule.
    3. *Keep a record in `CloudState` and have the reporter log it.* It changes replicated state for a log line, and it
       would miss checkpoints at high rates.
    4. *Only commands count.* Then the index depends on which entries were commands, and a new leader's blank entry on a
       multiple would skip the line. Counting any entry is simpler, and every node still agrees.
    5. *A config key for the interval.* Nothing varies it, and every node must use the same value anyway.
  - *Interval:* every 1000 gives about 4 lines in a whole quick run at 30 users, too few. Every 100 gives one every 2 s or
    so at the lightest load, and at 3000 reports a second a few KB a second, nothing beside the per-request lines. Every 10
    adds lines with no use.
  - *Election count:* count terms above the highest logged before the window, or log a window-start marker from the runner.
    The first needs nothing new.
  - *Takeovers:* split by reason and take them out of the failover row, or filter to `suspected` under the old name. Keeping
    the name would hide the change from anyone reading a table.
- **Decision.**
  - `Event::Overloaded { request, attempt, queued, running }`, with `queued` and `running` read from the pool when it refuses.
  - `Event::StateCheckpoint { log_index, digest }`, from `StateMachineStore::apply`, right after applying each index that is
    a multiple of `raft::store::CHECKPOINT_EVERY` (100), index 0 excepted. `StateMachineStore::new` now takes the `EventLog`.
    `state_digest` stays as it is, so its field names do not move.
  - `analyze.py`: `plans_refused` and `plans_refused_node_<n>` in the measured window. `digest_checkpoints_compared`
    (indexes at least two nodes logged), `digest_mismatches` (indexes with more than one digest, which must be 0),
    `rejoins_digest_confirmed` (rejoins where the first checkpoint the node logged afterwards matches another node's),
    `rejoin_targets_lowered` (D135).
  - `elections` counts terms above the highest logged before the window; a log with nothing before the window treats its first
    term as the running one.
  - `takeover_wait_mean_ms` is gone. New: `takeovers_suspected`, `takeovers_backstop`, `takeover_wait_suspected_mean_ms`,
    `takeover_wait_backstop_mean_ms`, listed apart from failover. `takeovers` stays.
  - New: `client_gap_mean_ms` and `client_gap_max_ms`. For each client, the longest time between two answers on its own clock
    that overlaps the fault. The README's failover row now names these, `detection_mean_ms`, `election_duration_mean_ms` and
    `recovery_mean_ms`.
- **Why.** A digest chains over the whole log before it (D31), so one equal digest shows that everything before it was equal
  too, and one checkpoint after a rejoin proves the whole catch-up. Every node applies every index, so every node reaches
  every checkpoint, and that is what makes logs from different machines comparable with no clock involved. Logging in `apply`
  goes through the event log's channel, and it writes no disk under the state lock (D55). A client gap is a real duration on
  one clock; only choosing which gaps overlap the fault compares clocks, which is the same approximation the before, during
  and after split already makes.
- **Consequences.** A little more log volume. A restarted node logs its checkpoints again as it replays the log, which is
  useful: a different digest at the same index from the same node counts as a mismatch. `ctl status` still shows a digest that
  changes with every command, so under load the column can differ between nodes from one refresh to the next; putting
  checkpoints in `NodeStatus` would be a `proto.rs` change and was not made. `rejoins_digest_confirmed` can be below `rejoins`
  with nothing wrong if a run ends before the next checkpoint, so read it with `digest_mismatches`. The client gap
  includes normal think-time spacing, which is small at the loads used. Per-node CPU and queue figures, the duplicate and stale
  counts and the local against forwarded commit split are still missing from the analysis; all can be added after the data.
- **Where.** `src/metrics.rs`, `src/node/mod.rs::plan_on_workers`, `src/raft/store.rs::{CHECKPOINT_EVERY,
  StateMachineStore}`, `src/raft/mod.rs` (one line), `analysis/analyze.py::{refusal_metrics, convergence_metrics,
  election_metrics, client_gap_metrics, fault_metrics}`, `analysis/README.md`.
- **How we check it.** `a_refused_planning_job_is_logged`, `an_accepted_planning_job_logs_no_refusal`,
  `a_digest_is_logged_at_every_checkpoint_index_however_entries_are_batched` (two stores given one log in different batches,
  with a blank entry on a checkpoint), `tests/rejoin.rs::a_returning_node_logs_the_same_digests_at_the_same_indexes` (real
  nodes, one silenced across a checkpoint). `analysis/test_analyze.py` has new tests for each metric, including a term running
  before the window, which fails on the old count. `test_analyze.py` is not in CI, so run it by hand. In the trial runs on one
  machine (D139, a debug build, so no result), `overloaded` appeared at 300 users and not at 30, 3 to 15 checkpoints were compared
  in each run with 0 mismatches, and every rejoin was confirmed by a digest.
- **Report notes.** This is the convergence metric the spec asks for: `digest_mismatches` and `rejoins_digest_confirmed` against
  `rejoins`, with `recovery_mean_ms` as convergence time. E2 can now state shed work. Call the takeover figures what they are.

### D139 · Runner: silence is retried, a leftover program stops the batch, and a run with no fault is set aside

- **Date:** 2026-11-03 · **Part:** C, F (scripts) · **Status:** Implemented, to confirm, amends D130, amended by D222 and D223 · **Decided by:** the agent on its
  recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** Three ways a batch could lose hours or, worse, give wrong figures with no sign.
  1. `ctl silence` was one call with a 2 s timeout under `set -euo pipefail`. A loaded node can miss 2 s, and one miss ended
     the rest of the batch.
  2. `stop_all` stops only what has a pid file. A leftover load generator keeps sending while its log is wiped. A leftover node
     keeps the ports, so the new node cannot bind, `wait_for_leader` passes on the old one, and the run measures the old
     config. Items 16 and 17 each planned their own check for this, with different patterns; the frontend and the zone feed
     (items 17 and 14) add more programs that could be left running.
  3. Writing the retry showed a third: `ctl status` shows a node that misses its 2 s as `unreachable`. So `victims_of` could
     find no leader or no follower, and the cell ran with no fault, looking like a good run. With no follower, the `others`
     victim's `grep -vx ""` named all three nodes.
- **Options considered.**
  - *The silence:*
    1. *Up to 3 tries, 1 s apart, and "already silenced" counts as done.*
    2. *Leave it.*
    3. *Raise ctl's timeout.* A longer wait for a node that may not answer, and it slows every operator command.
  - *The leftover check:*
    1. *One function, `refuse_leftovers`, with `pgrep -af` on the path, at the start of each run; and again for load programs
       right after the load ends.*
    2. *`pgrep -x` on the names.* It also finds Node.js, including a VS Code server on a lab PC, and could stop an hour-long batch
       for nothing.
    3. *A runbook line only.* Nobody reads it at 03:00.
    4. *Teach the runner to kill them.* It could kill someone's demo on a shared machine.
  - *When a run cannot be trusted:*
    1. *Delete its directory and stop the batch.* `analyze.py` reads every `run.json` it finds, so a bad run must not stay.
       Right when the next run would be spoiled too, as with a leftover program. Wrong for a run that merely never got its
       fault: a trial at 300 users on one machine (debug build, so no number from it counts) had `ctl status` find no leader,
       and the batch ended there, which undoes what the retry is for.
    2. *Set it aside and carry on.* It goes somewhere analysis does not read, with its logs, and the batch goes on.
    3. *Mark it bad where it is and carry on.* It needs a marker every reader must honour.
- **Decision.**
  - `silence_with_retry` in `scripts/lab-common.sh`: up to 3 tries, 1 s apart. A refusal saying `already silenced` counts as
    done, because a lost answer to an order that worked looks exactly like that (D74 refuses a second fault).
  - `refuse_leftovers <programs> [grace_s]` in `scripts/lab-common.sh`, over `on_host`, one `pgrep -af` per machine. The pattern
    is `[t]arget/release/(frontend|zonefeed|loadgen|node)( |$)`, built from `$BIN`. Exit 0 from pgrep means found, so it prints
    the machine and the processes and fails. Exit 1 is clean. Anything else, like ssh's 255, fails as "cannot check".
  - `LOAD_PROGRAMS='frontend|zonefeed|loadgen'` and `ALL_PROGRAMS` add `node`. `run_once` calls `refuse_leftovers
    "$ALL_PROGRAMS" 10` before it wipes anything, and `refuse_leftovers "$LOAD_PROGRAMS"` right after its own load generators end.
    The 10 s lets the last run's nodes finish stopping when `GAP` is short.
  - `abandon_run` in `scripts/run-experiments.sh` deletes the run's directory, prints why and exits non-zero, and the `EXIT`
    trap stops the nodes. It is used when the check after the load trips, and when there is no leader at the start.
  - `set_aside`, in the same file, is used when 3 tries find no victim or 3 tries to silence fail. The run goes on to its end,
    and then its directory, with its logs and a `dropped.txt` giving the reason, moves to `runs/dropped/<cell>-run-<n>/`. That
    is one level too shallow for `analyze.py`, which reads `runs/<experiment>/<cell>/<run>/`. The batch goes on, and at the end
    it lists those runs and exits non-zero.
  - `victims_of others` gives nobody when there is no follower, instead of all three.
- **Why.** Retrying fixes the common case, a slow answer, without hiding a real one. Matching by path is what makes it safe to
  be strict. The first character in brackets stops the pattern from matching the shell that runs pgrep, whose command line
  holds the pattern: over ssh that shell stays alive while pgrep runs. Checking before the wipe keeps a leftover's log for
  looking at. Stopping is right for a leftover, because no figure taken next to one can be trusted, this run's or the next
  one's. A run that missed its fault hurts only itself. At the top of the sweep it is an expected way to fail, so it must not
  cost the rest of a batch, and its logs are evidence of why the cell collapsed.
- **Consequences.** A program started outside `$BIN` is not caught: a `cargo run` debug build, or another copy of the repo. A
  failed `cd` over ssh also exits 1 and reads as clean, but the wipe right after it then fails and stops the run. A silence
  may start some seconds late after failed tries. Phases are computed from `silenced` and `restored` events, so the figures
  stay right. A run set aside still takes its full time. A cell that is always set aside has no data, and the report must
  say that plainly rather than leave the row out. Items 16 and 17 must call this function and not add their own check. Whoever
  adds a program to the client machines adds it to `LOAD_PROGRAMS`.
- **Where.** `scripts/lab-common.sh::{LOAD_PROGRAMS, ALL_PROGRAMS, refuse_leftovers, silence_with_retry}`,
  `scripts/run-experiments.sh::{victims_of, abandon_run, set_aside, run_once}`, `tests/runner_script.rs`,
  `analysis/test_analyze.py::test_a_run_the_runner_set_aside_is_not_read`.
- **How we check it.** `tests/runner_script.rs` runs the real functions in bash, in `cargo test`, Linux only: every program
  under the path is refused until it is gone; nothing running passes (so the check does not find its own shell); a program
  with the same name elsewhere passes; a name that only starts the same passes; the check after the load leaves the run's nodes
  alone; a program stopping within the grace time passes; the same over a stand-in for ssh; a machine that cannot be checked
  stops the run; silence retried 1, 3 and 3-then-fail times; `already silenced` counts as done. `run_once` itself was tried with
  `experiments/hosts.local` in a private network namespace, on real nodes: a pretend `loadgen` left over was refused and nothing
  was wiped; a pretend `frontend` that appeared during a run got that run deleted and stopped the batch; a `ctl` made to find
  nobody at the fault got the run set aside, the next cell ran, and the exit was non-zero with the list; and with `GAP=1` a
  second run started cleanly after the first run's nodes had ended. **It has still not run over real SSH or on the lab
  machines**, as D130 said of the runner.
- **Report notes.** Part of the method section: runs with a leftover program or a missed fault are never counted. If any run
  was set aside, say how many, and which cells, next to the figures.

The next four entries come from the review of D134 to D139, so they sit here although their numbers came later. Every number in them comes from
one PC with a stand-in for ssh. They explain why a choice was made and must never be reported as six-PC results. Nothing in them has run over real SSH.

### D222 · Remote lines run in the repo folder: `cd` is its own command, and a missing folder exits 125

- **Date:** 2026-11-03 · **Part:** C, F (scripts) · **Status:** Implemented, amends D130 and D139 · **Decided by:** the agent on
  its recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** `on_host` in `scripts/lab-common.sh` sent every line to a far machine as `cd DIR && <line>`. The line that starts a
  node ends one job with `&`: `nohup node ... >out 2>&1 & echo $! >runs-config/node-1.pid`. A shell reads the whole thing as
  `{ cd DIR && nohup node ... } &`, and then `echo $! >runs-config/node-1.pid`. So three things go wrong on any machine reached
  by ssh. The background part is a shell of its own, which keeps the ssh session's output open for as long as the node lives,
  so the call never returns. The `echo` runs in the home folder, where `runs-config` does not exist, so there is no pid file. And
  if `~/runs-config` did exist, the pid file would hold that background shell's number and not the node's, and `stop_all` would
  leave the node running. A stand-in for ssh that starts the far shell in the home folder showed it at once:
  `runs-config/node-1.pid: No such file or directory`, then nothing, until a 120 s limit killed the batch. That is the first
  node of the first run. So **the runner could not have started a single node on the six PCs**, and every trial until now used
  `experiments/hosts.local`, where `on_host` runs `bash -c "$*"` with no `cd` and the fault does not exist. D130 said the runner had
  not run over SSH, and D139 repeated it. Its tests stood in for ssh with no change of folder, and only for the leftover check.
  The same form also made a failed `cd` exit 1, which is what `pgrep` says for "no match", so `refuse_leftovers` read a machine
  without the repo as clean. D139 noted that and relied on the wipe failing next. In fact the wipe did not fail:
  `cd DIR && rm -rf ...; mkdir -p ...` runs `mkdir` in the home folder and exits 0, and only `scp` stopped the batch later.
- **Options considered.**
  1. *`cd DIR || exit 125; <line>`.* The rest of the line runs in the folder whatever it holds, and the far shell itself is what
     started the node, so `$!` is the node. A distinct code tells a missing folder from everything `pgrep` says. It costs nothing.
  2. *`cd DIR && { <line>; }`.* Also keeps the background job right, but a line that ends in `&` becomes `& ;`, which is a syntax
     error. A failed `cd` still exits 1.
  3. *`cd DIR || exit 1; <line>`*, the same idea with the usual code. It fixes the node start, but a machine without the repo
     still reads as "no leftovers" in a check that goes by `pgrep`'s exit code.
  4. *Change only the node-start line*, for example `(cd DIR; nohup ... & echo $! >pid)`. It leaves the trap in `on_host` for the
     next line with an `&`, and puts the folder name back into `run_once`.
- **Decision.** Option 1, in `on_host`. Exit 125: `pgrep` uses 0 to 3, `ssh` uses 255, `timeout` 124, and a shell 126 and 127.
  `refuse_leftovers` needs no change: anything but 0 and 1 already prints `cannot check <host> for leftover programs (exit n)` and
  stops. A whole cell now runs in `cargo test`, over a stand-in for ssh that starts in the home folder with a bare environment and
  does not return while anything holds the session's output.
- **Why.** Nothing else in the plan matters if the batch cannot start a node. The bug hides from every test on one machine, so
  the test had to be one that behaves like a far machine, and it has to stay in `cargo test`, because anyone can add another line
  with an `&` to `run_once`.
- **Consequences.** A machine without the repo now stops the batch at the first line sent to it, with the far shell's own `cd`
  error. D139's sentence "a failed `cd` over ssh also exits 1 and reads as clean" is no longer true. `tests/runner_script.rs` takes
  about 5 s more. It needs `git` and `timeout`, which CI and the lab PCs have. **This still has not run over real SSH**: a stand-in
  shows how a shell parses a line, not firewalls, keys or `sshd` limits, so the first run on the lab PCs must still be watched.
- **Where.** `scripts/lab-common.sh::on_host`, `tests/runner_script.rs::{Lab, a_whole_cell_runs_over_ssh_and_stops_everything_it_started,
  a_machine_without_the_repo_folder_cannot_pass_for_a_clean_one}`.
- **How we check it.** With the old `on_host` put back, the whole-cell test hits its 120 s limit and fails, and a failed `cd` that
  exits 1 fails the second test. The same runner was also run by hand over a stand-in for all of `ssh`, `scp` and `rsync`, on six
  folders in a private network namespace: a cell with `victim=leader`, one with `victim=others` with `dash` as the login shell, and
  a skew cell. All exit 0, with nothing in any home folder.
- **Report notes.** A good honest sentence for the method section: the runner's first test against something that behaves like a
  far machine found that it could not start a node, which no test on one machine could have shown.
- **The merge.** D188 found the same bug from the other side. The merged `on_host` keeps this entry's `exit 125`, because `refuse_leftovers`
  goes by `pgrep`'s exit code and reads 1 as clean, and D188's `SSH_OPTIONS`.
- **Corrections to earlier entries.**
  - D137 says a restart prints no `ERROR`. That is too strong. What was tried: on a quiet three-node cluster, `kill -9` and a restart, of a
    follower and of the leader. The restarted node printed no `ERROR`, and `Can not initialize` appeared nowhere. But its peers print
    `ERROR openraft::replication: RPCError err=Unreachable ...` while it is down, 20 to 30 lines in those 3 s here, and under churn the restarted
    node itself printed RPC timeouts at `ERROR`. What D137 changed is that a restart no longer prints `Can not initialize`.
  - D139 says a failed `cd` over ssh exits 1 and reads as clean, and that the wipe after it then stops the run. Neither holds now, and the second
    never did: `cd DIR && rm -rf ...; mkdir -p ...` ran `mkdir` in the home folder and exited 0, and only `scp` stopped the batch later.
  - D139's "the same over a stand-in for ssh" covered only the leftover check. A whole cell now runs over the stand-in. D139's last sentence,
    that the runner has not run over real SSH, still holds.

### D223 · A run gets its `run.json` only when it is whole

- **Date:** 2026-11-03 · **Part:** C, F (scripts), for all figures · **Status:** Implemented, amends D130 and D139. After the merge the old folder moves to `runs/old/` (D186) and is not deleted · **Decided by:**
  the agent on its recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** `analyze.py` counts every `run.json` it finds. D139 deletes a run it cannot trust, but only on the two paths it
  knows about. The runner wrote `run.json` first, before anything else in the run, so every other way out left it behind: a batch
  killed by hand, a machine lost while the logs were copied, `scp` failing, a load generator's exit status under `set -e`. Such a
  folder has `run.json` and no logs, and the analysis reads it as a real run. In a trial here a batch was killed at its first node,
  and `analysis/analyze.py runs` then reported that cell with `throughput_rps` 0.0 and `gave_up` 0, with no sign of a problem. One
  such run among three pulls a cell's mean throughput down by a third. A rerun into an old folder was worse: the old `run.json`
  stayed through the new run, and logs were copied over old ones, keeping any old file the new run did not write.
- **Options considered.**
  1. *Write it as `run.json.partial`, and rename it as the last step of a good run.* A rename is atomic, so no way of dying can leave
     a half-finished run under the name analysis reads, power loss included. Delete the old folder when the new run starts.
  2. *Have `analyze.py` skip a folder with no logs.* It misses a folder with some of its logs.
  3. *Delete the folder from the `EXIT` trap unless the run finished.* It needs state in the trap, and `kill -9` or a power cut leaves
     the folder.
  4. *A `complete` marker file beside `run.json`.* The same effect as option 1, plus a rule every reader must remember.
- **Decision.** Option 1. `run_once` writes `run.json.partial` where it wrote `run.json`, and renames it after the logs are copied
  and only if the cell got its fault. A run that `set_aside` moves to `runs/dropped/` keeps the `.partial` name, so it has two
  reasons not to be read. `rm -rf "$out"` runs after the leftover check and before anything is written.
- **Why.** The file's name is the promise "this run is whole", and the analysis needs no change for it. Removing the old folder at
  the start is what makes a rerun mean the same as a first run.
- **Consequences.** A folder with `run.json.partial` under `runs/` is a run that did not finish, and its logs may be useful, so they
  are kept and nothing reads them. Rerunning a cell loses the older run at the start, not at the end. Item 16 moves an old folder to
  `runs/old/` instead of deleting it (D186 in its log): when the two merge, keep that move and keep the `.partial` name.
- **Where.** `scripts/run-experiments.sh::run_once`,
  `tests/runner_script.rs::a_batch_that_stops_halfway_leaves_no_run_for_the_analysis`.
- **How we check it.** That test makes `scp` fail in a real runner, with an older `run.json` in the folder, and requires a non-zero
  exit and no `run.json`. With `run.json` written first, or with the old folder kept, it fails. The whole-cell test requires
  `run.json` and no `run.json.partial` after a good run.
- **Report notes.** Part of the method: only whole runs are counted, and this is enforced by the file's name, not by a list of
  failure cases.

### D224 · The client gap counts a client that never hears again

- **Date:** 2026-11-03 · **Part:** C (E3), analysis is shared · **Status:** Implemented, amends D138 · **Decided by:** the agent on
  its recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** D138 added `client_gap_mean_ms` and `client_gap_max_ms`, the longest time between two answers that overlaps the fault.
  With a client that gets no answer after the fault there is no second answer, so that silence is not a gap at all. The figure then
  shows the last small gap before it, or nothing, and `confidence_interval` drops a NaN. So the worse the outage, the better the
  figure: a run in which clients never heard again, like the rehearsal's collapsed cells, would leave E3's gap looking short.
- **Options considered.**
  1. *Close the ends with the client's first and last events.* A client keeps sending until it stops, so its last event is about when
     it gave up. A gap is still a time on one clock.
  2. *Close the ends with the run's measured window.* It would mix the node clock or the runner's numbers into a figure that is
     otherwise on the client's clock.
  3. *Report it apart as "clients that never recovered".* Clearer in one way, but one more metric to read, and the gap would still
     look short on its own.
- **Decision.** Option 1, in `client_gap_metrics`. The ends only count if they overlap the fault, so a normal run is unchanged:
  the wait before the first answer ends before the fault, and the wait after the last answer starts after it.
- **Why.** A figure for failover must not improve when failover fails. The value is a lower limit for such a client, because the
  client stopped trying at some point, and that is the honest reading.
- **Consequences.** In a cell where clients give up, `client_gap_max_ms` can be near the length of the run after the fault. Read it
  with `gave_up`. A client whose whole log has no answer counts the whole of its log as one gap.
- **Where.** `analysis/analyze.py::client_gap_metrics`, `analysis/test_analyze.py` (three new tests).
- **How we check it.** `test_a_client_that_never_hears_again_counts_its_whole_silence`,
  `test_a_client_that_never_heard_at_all_counts_the_whole_run`, `test_the_ends_of_a_normal_run_add_no_gap`. D138's first test of the gap
  passes unchanged. `test_analyze.py` is still not in CI, so run it by hand.
- **Report notes.** If a cell has a large gap with `gave_up` above 0, say the clients stopped waiting: the gap is not a time to recover.

### D225 · A Raft that has stopped is not asked in a tight loop

- **Date:** 2026-11-03 · **Part:** D · **Status:** Implemented, amends D135 · **Decided by:** the agent on its recommendation, on Noor's
  instruction to finish without more questions (2026-11-03)
- **Context.** D135's `reach_target` waits up to 1 s, then asks the leader again. Both calls return at once on a Raft that has stopped,
  which it does on a fatal storage error such as a full disk. The loop then has nothing that parks it, and each question makes openraft
  print two `ERROR` lines (`failure receiving rx from RaftCore`, `recv_msg error=Stopped`). So a node catching up when its disk fills
  would burn a CPU and write as fast as it can to that same disk. The old code spun in the same way, with a `warn!` for each round. This
  PC's own disk filled during this work, so the case is not far-fetched.
- **Options considered.**
  1. *Sleep out the rest of the wait.* Two lines. With a live Raft the wait has already run out, so the sleep is over at once.
  2. *End the task when `read_index` returns `Fatal`.* It stops the questions, but the node then stays `serving = false` with nothing
     to say why, and `Fatal` also covers some errors a later call can get past.
  3. *Leave it.* The node is already useless, but the log flood is what would make the disk problem worse.
- **Decision.** Option 1, in `reach_target`.
- **Why.** The smallest change that makes the promise "one question per wait" true in every case.
- **Consequences.** None for a live Raft. A stopped Raft is asked once a second, and it prints those two `ERROR` lines once a second,
  which is how someone finds it.
- **Where.** `src/node/rejoin.rs::reach_target`, `src/node/testing.rs::ScriptedConsensus::stop`.
- **How we check it.** `a_stopped_raft_is_asked_once_per_wait_and_not_in_a_tight_loop`: with the pause taken out, the second question
  comes at once and it fails.
- **Report notes.** None needed.

### Review record for D134 to D139 and D222 to D225: what was attacked and left alone

For the report's honesty, and so nobody repeats the work. All of it is one PC, scripts only, never a result.

- **The rejoin target under leader changes.** Reading openraft 0.9.25: `get_read_log_id` is the larger of the leader's no-op and its commit index,
  taken when the question arrives and sent only after a majority has confirmed the leader. A new leader's no-op is above everything committed, by the
  election rule, so every answer covers what was committed when it was given, and the lowest answer keeps D76's promise. A majority cannot confirm a
  replaced leader, because it must overlap the majority that elected the new one. `noop_log_id` is always set on a leader (`proposer/leader.rs:95`), so
  the `Ok(None)` arm cannot come from real Raft, and it is harmless. On real nodes, with disk saves on and load running, the leader was silenced 8 times
  at 4.5 s apart and 25 times at 1.5 s apart, with a `kill -9` and a restart after each series: every return finished, or was ended by the next silence,
  and at the end all three nodes were serving with equal digests. **`rejoin_target_lowered` did not fire in any of these runs**, so that one path has still
  only run on the scripted `Consensus`.
- **The reply gate.** `send_done` and `send_provisional` are the only two places that send a request's reply. `serving` is written only by `rejoin.rs` and at
  start. No answer that should be sent is lost for good: what the gate holds back is committed, so a client's retry gets it from any serving node, and the
  client sends every retry to all three. In those same runs, `replied` and `provisional_replied` between `restored` and `rejoined` were 0, and in the last
  of them no client gave up.
- **The quiet log filter.** Everything `docs/demo.md` and `docs/runbook.md` tell people to grep is in the `.jsonl` files, and `analysis/analyze.py` reads only
  those. The runbook's symptoms that come from stderr (`Address already in use`, an unknown key, connect failures) come from our own code at levels the filter
  keeps. The heartbeat summary uses the target `heartbeat` at `info`, which the filter keeps. `tarpc`'s only lines still on are `Connection broken`,
  `Requests stream errored out` and `RpcError::Channel`. The last can come once per call in flight when a connection breaks, which is a burst only at that moment.
  In these runs one `tarpc` line appeared: `Requests stream errored out`, on a node whose peer had just been killed with `kill -9`, which is what the filter keeps
  them for.
- **Event field names.** Every field `analyze.py` reads was checked against `src/metrics.rs` by hand, including `reason` on `takeover_started`, `from` and `to`
  on `rejoin_target_lowered`, `log_index` and `digest` on `state_checkpoint`, and `kind` on `commit`. All match. A run through `analyze.py` on real logs from
  a leader, a follower, an `others`, a skew and a no-fault cell raised nothing.
- **Other shell quoting.** `pgrep`'s pattern holds a `$` and a `|`, and it is single-quoted once it reaches the far shell. Its first character in brackets does
  stop it finding its own shell, over the stand-in too. `stop_all`, `ctl` and the load line are safe in both bash and dash.

### D140 · Incident fields: owner, category and TTL; one report, one incident ID

- **Date:** 2026-11-03 · **Part:** all (`proto.rs` is shared) · **Status:** Implemented · **Decided by:** Noor Hany
  Emam for the owner, status and TTL fields (decisions item 5). The form of each field, and the incident ID, decided
  by the agent on its recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** The spec says an incident should contain at least an incident ID, owner or reporter, category, logical
  campus location, severity, timestamp, status and time-to-live. Before this change `Incident` held only `zone`,
  `location`, `severity`, `needs` and `reported_at_ms`, so four of the eight were missing: nothing said who sent a
  report, what kind of event it was, how long it stayed valid or where it stood. The list applies to Phase 1 because
  the spec builds the incident service there, while resource offers and leases are Phase 2. Status has its own entry
  (D142), the meaning of the TTL has its own entry (D143) and category has its own entry (D141). This entry is about
  the fields as data.
- **Options considered.**
  - For the incident ID:
    1. *An alias, `type IncidentId = RequestId`, documented as one report, one incident.* Retries already reuse this
       ID and it already keys the results table. Nothing new can go wrong. It is a reading of the spec, and the report
       must say so.
    2. *A new `id` field inside `Incident`.* It would repeat the request ID in every record, and the two could
       disagree.
    3. *A separate random incident ID.* It needs randomness at the client, adds a second key to keep in step, and
       a retry would have to carry the same one anyway.
  - For the owner:
    1. *`owner: UserId { client: ClientId, user: u32 }`.* A pair that needs no coordination: client IDs are unique
       across machines (D113), and each client numbers its own users.
    2. *`owner: u64` only.* Two load generators would have to agree on number ranges by hand.
    3. *Reuse `RequestId.client`.* That names a process shared by all its users (D53), not a person. The spec's user
       processes would all look like one person.
  - For the TTL:
    1. *`ttl_ms` on every report, with `campus.incident_ttl_ms` as the value a client uses.* A report says how long it
       is valid, as the spec's list reads, and a page or a demo can set it per report.
    2. *Config only, with no field.* Nothing in the record would say what TTL applied, and every node would have to
       trust its own copy of the config when judging an old record.
- **Decision.** `type IncidentId = RequestId`. `UserId { client, user }`, printed `3/17`. `Incident` gains `owner:
  UserId`, `category: Category` and `ttl_ms: u64`; `needs` stays. The eight fields of the spec map like this:

  | Spec's field | Where it lives |
  |--------------|----------------|
  | incident ID | `IncidentId`, an alias of `RequestId` |
  | owner or reporter | `Incident.owner` |
  | category | `Incident.category` |
  | logical campus location | `Incident.zone`, `Incident.location` |
  | severity | `Incident.severity` |
  | timestamp | `Incident.reported_at_ms` |
  | status | `IncidentStatus`, worked out on read (D142), shown in `IncidentView` |
  | time-to-live | `Incident.ttl_ms` |

  Every value is an integer or a plain enum, so the replicated state stays deterministic and hashable. All owners and
  incidents are synthetic, as the spec requires.
- **Why.** The ID alias has the fewest ways to fail: there is one key, and a retry cannot create a second incident. The
  owner pair is unique without any agreement between machines, and Phase 2 needs an owner again for its offers and
  conflict rules, so the name is right for later too. The TTL has to travel with the report because the executor judges
  it, and the record must still say what it was after the config has changed.
- **Consequences.** The wire format, the Raft log file and every digest value change. All six machines need one commit
  and `data/raft` must be wiped once: an old log has good checksums but cannot be decoded, so it is cut at its first
  report with only a warning (`raft/wal.rs`), and a mixed build looks like a dead link. No lab data exists yet, so
  nothing is lost. A log entry grows by about 20 to 25 bytes with D144 and D148, by arithmetic: 2 to 6 for the owner, 1
  for the category, 3 for the TTL, up to 3 for the age, 9 for the time and up to 3 for the priority. At 3000 reports a
  second that is about 75 kB/s more log data on each node. Not measured. "TTL" now has four meanings in this project,
  and the docs must keep them apart: this field, the table trim D9 dropped, the hop limit the spec describes for Phase
  2 dissemination, and the deadline on one RPC in `raft/network.rs`.
- **Where.** `proto.rs::{IncidentId, UserId, Incident}`, `campus.rs::{random_incident, unseeded_incident}`,
  `config.rs::CampusConfig::incident_ttl_ms`, `bin/loadgen.rs`, every test literal.
- **How we check it.** `proto::tests::client_frames_round_trip`, `commands_round_trip`,
  `campus::tests::a_report_carries_its_owner_and_the_configured_ttl`,
  `config::tests::incident_ttl_defaults_to_sixty_seconds_and_zero_is_rejected`.
- **Report notes.** Put the field table above in the Incident Processing Service section. Say that one report is one
  incident, and that status is a reading of the record and not a stored field. Both are our readings of the spec.

### D141 · Category decides the needed capability; `Security` becomes `Technical` (amends D50, D99)

- **Date:** 2026-11-03 · **Part:** all, E · **Status:** Implemented · **Decided by:** the agent on its recommendation,
  on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** `needs` says who can help, not what happened. A blocked road, a power outage or a flood could not be
  expressed, and technical assistance had no capability at all. The spec's application scenario names the events:
  a blocked road, a power outage, a lab incident, localized flooding, a network outage, or a request for medical or
  technical assistance. D50 justified the four capabilities by "the project's incident examples (fire, medical,
  security)", but fire and security are not on the spec's list.
- **Options considered.**
  - For the values:
    1. *Seven: `BlockedRoad`, `PowerOutage`, `LabIncident`, `Flooding`, `NetworkOutage`, `MedicalAssistance`,
       `TechnicalAssistance`.* The spec's own examples, one for one. Medical and technical help need different
       responders, so they stay two values.
    2. *Six, with one `Assistance` value.* It could not say which responder an assistance request needs.
    3. *Reuse `Capability` as the category.* It says who helps, not what happened, which is the gap in the first place.
  - For how category and `needs` relate:
    1. *Keep `needs`, add a fixed table `Category::needs()`, and draw `needs` first, then a category of that
       capability.* D99's uniform capability stays, and so do D129's capacity sums. A web form needs only one dropdown.
    2. *Draw the category first and derive `needs`.* Three of the seven categories need `Technical`, so `Technical`
       would become 3/7 of all load instead of 1/4, and every load figure sized on a quarter would move.
    3. *Drop `needs` and let the planner call `category.needs()`.* A cleaner struct, but it edits the planner's rule and
       every `needs` in the tests for no gain.
  - For the capability names:
    1. *Rename `Security` to `Technical`, in the same position.* Power, network and technical help do not fit
       "Security". The position in `Capability::ALL` stays, so `campus.rs` still gives each responder the same slot.
    2. *Keep all four names.* A power outage would then be answered by "Security", which a reader would ask about.
  - For the planner:
    1. *The planner does not read the category; `needs` stays its input.* The pick rule does not change, so E2 cannot
       move because of this.
    2. *A weight per category.* Nothing in the spec asks for it, and it would be one more thing to justify.
- **Decision.** `Category` as listed. `Category::needs()`: `BlockedRoad` and `Flooding` need `Rescue`; `PowerOutage`,
  `NetworkOutage` and `TechnicalAssistance` need `Technical`; `LabIncident` needs `Fire`; `MedicalAssistance` needs
  `Medical`. `Capability::categories()` is the same table read the other way. `random_incident` draws the zone,
  position, severity and needed capability as before, then, last, a category of that capability.
- **Why.** The first option in each group changes the least that is measured and still answers the spec's list. The
  table is our own reading, because the spec names the events but not who answers them, and it is one small function to
  change if someone reads it differently.
- **Consequences.** Each incident costs one more random draw, so a seed gives different incidents than it did before
  this change, though the shape of the load is the same. The planner never reads the category, so a report whose
  `needs` disagrees with its category is planned by `needs`. A page must set both from one choice, through
  `Category::needs()`.
- **Where.** `proto.rs::{Category, Capability}`, `campus.rs::random_incident`, the rename in `planner.rs` tests.
- **How we check it.** `proto::tests::every_category_maps_to_its_capability`, `the_two_category_tables_agree`,
  `campus::tests::every_category_occurs_and_agrees_with_needs`, `each_capability_stays_about_a_quarter_of_the_load`.
- **Report notes.** One sentence: the categories are the spec's events, and the table from category to capability is
  ours.

### D142 · Incident status is worked out on read, not stored

- **Date:** 2026-11-03 · **Part:** A, F · **Status:** Implemented · **Decided by:** the agent on its recommendation,
  on Noor's instruction to finish without more questions (2026-11-03). That status must exist and be read by something
  is Noor's (decisions item 5, which rejects "a stored label nothing reads")
- **Context.** The spec lists status among an incident's fields and wants incident state consistent among the peers.
  The replicated state already says almost everything: whether a result exists, who it sent, and whether that responder
  is still held. The question is whether status should also be a stored value, and what its values mean.
- **Options considered.**
  - For where status lives:
    1. *Worked out on read: `CommittedResult::status(held, now_ms)` and `CloudState::status_at(id, now_ms)`.* The result
       and the hold already say it, `apply` does not change, and reading cannot move the digest.
    2. *Stored beside each result: `results: BTreeMap<RequestId, IncidentRecord { result, status }>`, changed only by
       log entries.* Every node agrees exactly with no clock involved. It costs a new record type and two transitions
       in the state machine a day before the deadline, and it gives the TTL less to do: an accepted report with nobody
       sent would say `Unassigned` for ever.
    3. *A field inside `CommittedResult`.* A value that changes inside the record would make two replies for one ID
       differ, against D27 and D37.
    4. *Stored status plus a batched log sweep that expires old records.* No document requires it. A release already
       frees every hold within about 2 s when a quorum exists, and without a quorum a sweep cannot commit. It would also
       be the one way a bad clock could reach plans, because swept holds feed `free_responders`. About a new command, an
       outcome, a task and an index to move a label. Dropped, and named as a Phase 2 idea.
  - For the values:
    1. *`Provisional`, `Unassigned`, `Assigned`, `Resolved`, `Expired`.* Each maps to real data today. `Provisional`
       labels a lone node's answer on the client.
    2. *The same without `Provisional`.* A client would have nothing true to show for a lone node's answer.
    3. *Also `Reported`.* It would need a second log entry per request and change E1.
    4. *Also `NeedsReconciliation`.* It is the spec's visible reconciliation state, listed under Phase 2 (D149).
  - For what the words mean:
    1. *`Resolved` means the responder's release committed. `Expired` means nobody was sent, and the report was too old
       on arrival or its deadline has passed.* No new message, D111 already models time on scene, and a responder still
       held past the deadline really is still out there. This is a reading.
    2. *`Resolved` means a new close message from the client.* A new message and a new way to lose it.
    3. *`Expired` means anything not `Resolved` past the deadline.* It would call a responder who is still on scene
       expired.
  - For whether a record that sent nobody turns `Expired` after its deadline:
    1. *Yes, on read, against the reader's clock, drawn as a read rule and not a state change.* One comparison that
       cannot touch a plan, a hold or the digest. Without it the TTL does nothing in any normal run.
    2. *No: `Expired` means only too old on arrival.* No clock at all, but a report with nobody sent then says
       `Unassigned` for ever.
  - For which clock that deadline uses:
    1. *`assigned_at_ms + ttl_ms - age_ms`, saturating: the executor's clock, counting from the report.* One wrong client
       PC cannot change labels on every node.
    2. *`reported_at_ms + ttl_ms`: the client's clock against the reader's.* A client PC a minute slow would make all its
       nobody-sent reports read `Expired` on all three nodes.
  - For how the client sees status:
    1. *`Submitted.status`, from the reply and the record; `ClientReply` does not change.* No clock, lock or fallback on
       the reply path.
    2. *`ClientReply.status`, read from node state.* It would make a whole retry differ (D37), allow contradictory values
       to be built, and need a state read on the reply path. It is the flag on `Done` that D123 rejected.
    3. *Not shown on the client.* Then only `ctl` could ever read it.
- **Decision.** The first choice in each group.

  | The record | Reads |
  |------------|-------|
  | a responder was sent and is still held | `Assigned` |
  | a responder was sent and its release committed | `Resolved` |
  | nobody sent, and `age_ms >= ttl_ms` in the record | `Expired`, on any clock |
  | nobody sent, and the reader's clock is at or past `assigned_at_ms + ttl_ms - age_ms` | `Expired` |
  | nobody sent, otherwise | `Unassigned` |
  | a lone node's answer, on the client only | `Provisional` |

  `IncidentStatus` is in no `Command` or `Outcome`. `CommittedResult::status_at_commit()` is the same rule with no
  clock and no hold, and it is what `Submitted.status` and `client_done.status` carry. Later changes are read with
  `ControlRequest::Incidents` (D145).
- **Why.** This answers what happens at TTL on every node for no log traffic and no new state-machine logic: no new peer
  message, `Command`, `Outcome`, task, timer or dependency. So bidding, Raft, release and takeover run the same code in
  every experiment. The time is an argument, so `state.rs` stays free of clocks.
- **Consequences.** A wrong reader clock changes that node's labels. A wrong executor clock changes the times and labels
  of every nobody-sent record it planned, on all nodes, for good, with equal digests, because `assigned_at_ms` is in the
  log. Neither can change a plan, a hold or who is sent. A clock stepped back can turn `Expired` back to `Unassigned`,
  so the diagram must draw it as a read rule. Nothing happens at the moment of expiry, so nothing can be pushed: a page
  must poll. A retry after the release still gets the same `Done` body and the client still reads `Assigned`; the read
  request shows `Resolved`. After `provisional_conflict` with `responder_taken`, a record reads `Unassigned` although
  the client was told a responder was sent (D122). The report and the demo must say so; the conflict count is the
  evidence.
- **Where.** `proto.rs::{IncidentStatus, CommittedResult::{status, status_at_commit, deadline_ms, too_old_on_arrival}}`,
  `state.rs::{status_at, is_held}`, `client.rs::Submitted`.
- **How we check it.** `proto::tests::status_is_assigned_while_held_and_resolved_after`,
  `nobody_sent_reads_unassigned_then_expired_at_the_deadline`, `too_old_on_arrival_reads_expired_at_any_clock`,
  `the_deadline_counts_from_the_report_and_saturates`, `state::tests::status_at_follows_the_hold_through_release`,
  `reading_status_changes_neither_state_nor_digest`, `tests/incidents.rs`.
- **Report notes.** Say plainly that status is derived and that the step to `Expired` after acceptance uses wall clocks
  and sits outside the digest. Those are the limits of this choice.

### D143 · Too old means not planned: the client states the age, the node commits nobody and answers `ReplyBody::Expired`

- **Date:** 2026-11-03 · **Part:** all · **Status:** Implemented · **Decided by:** Noor Hany Emam (decisions item 5).
  The mechanism below, decided by the agent on its recommendation, on Noor's instruction to finish without more
  questions (2026-11-03)
- **Context.** Noor's rule: a report says, for example, that it is valid for 60 s; a node that gets it later answers
  expired and sends nobody; 60 s is longer than the client's 30 s of retries. A TTL field that only sits in the record
  is what Noor rejected. The mechanism has to fit "Commit, then reply" and must not compare clocks on different machines
  in a way that could silently spoil a run.
- **Options considered.**
  - For the meaning (Noor's):
    1. *Too old is not planned; the node answers expired and sends nobody.* Chosen.
    2. *A stored label nothing reads, with late reports still planned.* Rejected by Noor. A report 90 s old would still
       take a responder.
  - For which age the rule reads:
    1. *`ClientRequest.age_ms`, stated by the client on each send: one wall-clock difference at submit, then `Instant`
       elapsed.* One machine's clock, so a wrong PC clock or a clock step cannot turn a whole run into "nobody sent" with
       no error.
    2. *The same from the wall clock on every send.* A clock step during the retries could fake an age.
    3. *The node's `now - reported_at_ms`, as the planner's fallback does.* A client PC a minute behind would have all its
       reports refused, with very low latency, which can pass for a good run.
    4. *Time since this node received the request.* It restarts on each retry, as D95 found, so it would stay near zero.
  - For where the rule sits:
    1. *At the top of `plan_on_workers`, calling a pure `Incident::is_too_old(age_ms)`.* It covers `execute` and
       `degraded::plan`. It runs after the election, so one node commits. It uses no worker slot and leaves the planning
       time this node bids with as it was.
    2. *The first line of `planner::plan`.* It would take a worker job and be counted as planning time.
    3. *The top of `handle_request`.* All three nodes would act on it, before any election.
    4. *On the client.* The decision belongs on the node, by Noor's wording, and a client that stopped itself would answer
       nothing about the record.
  - For how a refusal is answered:
    1. *Commit `Plan { responder: None, cost: 0, priority: 0 }`, then reply `ReplyBody::Expired(result)`.* Commit then
       reply stands, the node literally answers expired, and the fact is in the log, so every retry gets the same body.
    2. *Commit the same, then reply plain `Done(result)`.* The client would have to work out what happened. Nothing would
       literally answer expired.
    3. *Reply without a commit.* It would be a second exception to "Commit, then reply" beside D120, and each node could
       answer differently.
    4. *Stay quiet.* The client would retry for 30 s and give up on a report the cloud had decided about.
  - For the client at TTL:
    1. *No change to retries: they stay per D52, and a node answers expired.* Every experiment depends on this loop, and
       at 60 s the retries end first.
    2. *The client stops at TTL, with a new `ClientError::Expired`.* A second place holding the same rule.
  - For the value and the key:
    1. *`campus.incident_ttl_ms = 60000`.* Noor's value, above the retries, and a simulated property like D111's key. All
       31 cells stay valid through the default.
    2. *`timers.incident_ttl_ms`.* It is not a timer anything waits on.
    3. *120 000 ms.* No reason to differ from Noor's value.
    4. *A TTL by severity.* More rules, and nothing asks for it.
  - For config checks:
    1. *Reject only 0.* No cell sets the TTL or the client timers, the demo's flag does not go through the check, and the
       analysis warning catches a spoiled run.
    2. *Also reject anything not above the worst-case retries.* A longer rule for a case nothing can reach.
    3. *No check.* A config of 0 would refuse every report in silence.
  - For `ttl_ms = 0` in a report:
    1. *No special case: expired at once.* It gives the demo a one-step refusal. A page must refuse 0.
    2. *0 means no TTL.* It contradicts the spec's list.
  - For who sets the TTL, and how the demo shows a refusal:
    1. *Each report carries it, and `loadgen --ttl-ms` overrides the config; no backdating flag.* `--ttl-ms 0` refuses at
       once, and `--ttl-ms 1500` with all nodes silenced makes a really late retry: one flag and no faked report. The
       runner must never pass it.
    2. *The same plus `loadgen --backdate-ms`.* Backdating by 5 s or more also trips the old-age fallback of D95.
    3. *Config only, lowered in a demo config.* Every client and node would have to share the demo value.
  - For degraded mode:
    1. *The same rule; the body stays `Provisional`; the client status is `Provisional`.* The answer is uncommitted and
       must say so. It cannot fire at a valid TTL, and once replayed the record reads `Expired` from its own fields.
    2. *Answer `Expired` without a commit.* That answer would pose as a decided one.
    3. *No TTL while degraded.* A report could be planned long after it stopped mattering.
  - For D95's age on the planner's fallback:
    1. *Leave it as it is, and write down why one path has two ages.* This gap does not need it, and four test helpers
       stamp `reported_at_ms: 0`, so switching would change what they plan.
    2. *Switch it to `req.age_ms`, amending D95.* One age in one path, more churn the day before the deadline.
  - For a frame stuck in the client's TCP send queue:
    1. *State it as a limit.* It needs a client PC off the network for longer than the TTL, and item 11's UDP client has
       no send queue, so the path ends there.
    2. *Keep a deadline beside each queued frame and discard at dequeue.* About 6 lines that item 11 would delete.
- **Decision.** `Incident::is_too_old(age_ms)` is `age_ms >= ttl_ms`. `ClientRequest` gains `age_ms`, set on each send
  in `MeshClient::submit_from`. The top of `plan_on_workers`: if too old, log `report_expired { request, age_ms, ttl_ms }`
  and return `planner::not_planned()`, with no worker job and no `record_service`. `execute` commits that as usual, and
  `state.rs` holds nothing because the plan has no responder. `ResultProposal` and `CommittedResult` carry `age_ms`, so
  the record itself shows it was too old on arrival. `send_done` sends `Expired(done)` when `done.too_old_on_arrival()`,
  otherwise `Done(done)`, reading no state and no clock. `campus.incident_ttl_ms` defaults to 60000 and `validate` rejects
  0. `loadgen --ttl-ms` overrides the client's copy of it. The loadgen summary gains `expired N` at the end of its first
  line.

  **The trap, and how it is closed.** A send loop that says "`Done` or `Provisional` returns, anything else keeps
  waiting" would ignore `Expired` until the client gives up, so a report the cloud has answered would look lost.
  `client::hand_over` now names every `ReplyBody` with no wildcard, so a new body cannot start out ignored: it is a
  compile error. `client::finished` treats `Done` and `Expired` alike and takes the status from the record.
  `tests/client.rs::an_expired_reply_ends_the_wait_at_once` holds this with retries an hour away. **Whoever rewrites the
  send loop for gap 11 must carry `age_ms` into the new loop and keep this test green.**
- **Why.** Stating the age keeps the decision on one machine's clock. The rule's place means one node commits, the
  refusal goes through Raft like any result, and first commit wins settles every race: a fresh attempt that committed
  first answers `Done` to a late retry, and a refusal that committed first answers `Expired` to a fresh retry, with no
  hold taken. So every reply for one ID stays the same (D27, D37).
- **Consequences.** One new branch sits on the request path. A stamping bug, such as milliseconds read as microseconds,
  would turn every report into "nobody sent" with very low latency, which can pass for a good run. The guards are
  `a_fresh_report_gets_a_responder_and_reads_assigned`, `reports_expired` in the smoke check before the long run, and the
  warning in `analyze.py` (D147). With any TTL above the retries the refusal cannot fire without `--ttl-ms` or a page's
  own TTL: the last send is at most 28.8 s old at +20% jitter (0, 2, 6, 12, 18 and 24 s, times 1.2) against 60 s, and no
  cell sets client timers. The report must not imply it ran under load. A frame waiting in the TCP send queue keeps its
  young stamp, so a client PC off the network for more than the TTL can still get reports planned after it gave up. A
  lone node's refusal is `Provisional(Plan { None, 0, 0 })`, labelled provisional and not counted as expired; it reads
  `Expired` only after the replay. It can only happen with a flag-set TTL. One request path now has two ages on purpose:
  the too-old rule reads the client's stated age, and the planner's fallback reads D95's two-clock age.
- **Where.** `proto.rs::{Incident::is_too_old, ClientRequest::age_ms, ReplyBody::Expired}`,
  `node/mod.rs::{plan_on_workers, send_done}`, `node/degraded.rs::{plan, settle}`,
  `client.rs::{submit_from, finished, hand_over}`, `config.rs::CampusConfig`, `bin/loadgen.rs`, `planner.rs::not_planned`.
- **How we check it.** `tests/incidents.rs::a_too_old_report_commits_nobody_with_responders_free`,
  `a_too_old_report_is_refused_under_leader_dispatch_too`, `a_report_one_millisecond_younger_than_its_ttl_is_planned`,
  `a_zero_ttl_is_refused_through_the_client`, `a_retry_of_a_refused_report_gets_the_same_body`,
  `a_report_planned_while_young_is_not_refused_by_a_late_retry`, `a_retry_that_arrives_too_late_is_refused`.
  `tests/client.rs::each_retry_states_a_larger_age`,
  `an_expired_reply_ends_the_wait_at_once`, `a_fresh_report_gets_a_responder_and_reads_assigned`.
  `tests/degraded.rs::a_lone_node_answers_a_too_old_report_provisionally_and_the_record_reads_expired`.
- **Report notes.** Cite `reports_expired = 0` for the runs. State the limits: the refusal fires only through a flag at a
  valid TTL and never in a measured run, a queued TCP frame can escape it, and no event marks the moment a nobody-sent
  record passes its deadline.

### D144 · Assignment times: `assigned_at_ms` in the log, `expires_at_ms` as the planned end

- **Date:** 2026-11-03 · **Part:** A, C · **Status:** Implemented · **Decided by:** the agent on its recommendation, on
  Noor's instruction to finish without more questions (2026-11-03)
- **Context.** The spec says a response assignment should identify who is assigned to which incident, when the assignment
  was created and when it expires. `Plan` and the responder slot said who, but neither time existed: the end existed only
  as a rule, timed by each node from when it first saw the hold, so it differed between nodes. The state machine has no
  clock, so any time has to come in through the proposal.
- **Options considered.**
  - For the times:
    1. *Store `assigned_at_ms`; compute `expires_at_ms` in the view, and document it as the planned end.* It is the next
       bullet in the same spec list and cannot be added after the data is taken. Nothing enforces the end, so it is a plan
       and not a promise.
    2. *Store both.* The second can always be computed from the first, and storing it says more than is true.
    3. *Leave both to Phase 2 and say so in the report.* The records would have no times at all.
  - For where age and plan time sit:
    1. *`age_ms` and `assigned_at_ms` in `ResultProposal` and `CommittedResult`.* Too old on arrival becomes a fact in the
       log that reads the same on any clock.
    2. *`assigned_at_ms` only.* The record could not show it was refused.
    3. *A `Plan.expired: bool` flag.* It touches about 20 literals, and allows values that contradict each other.
    4. *Times inside `Plan`.* Two plans made a moment apart would differ, which logs false conflicts in `settle`.
  - For a replayed provisional answer:
    1. *The time of the original answer, kept in `Provisional`.* The assignment was created when the client was told.
    2. *The time of the replay.* It would hide how long the record was only provisional.
- **Decision.** The first choice in each group. `execute` stamps `assigned_at_ms` from the node's clock on each proposal.
  `state.rs` copies both fields into the record, with no clock of its own. `Provisional` in `node/degraded.rs` stores
  `age_ms` and `assigned_at_ms`, and `settle` reuses them. `Table::store` now takes a built `Provisional`, because eight
  parameters would fail clippy under `-D warnings`. `CommittedResult::planned_end_ms(ms_per_severity)` is `assigned_at_ms
  + severity × ms_per_severity`, or `None` if nobody was sent. It appears in `IncidentView` as `expires_at_ms`.
- **Why.** It meets the spec's list with one more field in the log. The stamp sits in the proposal, so all nodes hold the
  same value and digests stay equal, whatever their clocks say.
- **Consequences.** `expires_at_ms` is a planned end, not an enforced one: a backup releases at twice that time, a failed
  proposal restarts the wait, and on a replay the hold starts later than the stored time. Docs must say so, and no test
  may assert `Resolved` by that time. `reported_at_ms` is on the client's clock and `assigned_at_ms` on the executor's, so
  they must never be subtracted: that is what `age_ms` is for (AGENTS.md, Measuring).
- **Where.** `proto.rs::{ResultProposal, CommittedResult::planned_end_ms}`, `state.rs::commit_result`,
  `node/mod.rs::execute`, `node/degraded.rs::{Provisional, Table::store, plan, settle}`.
- **How we check it.** `state::tests::a_commit_keeps_the_proposals_age_and_time`,
  `proto::tests::the_planned_end_is_the_time_on_scene_after_the_plan`,
  `node::degraded::tests::a_stored_answer_keeps_its_age_and_time`, `tests/degraded.rs::a_replay_keeps_the_original_stamps`,
  `tests/incidents.rs::rows_for_one_id_match_on_all_three_nodes`.
- **Report notes.** The assignment record of the spec is `responder`, `assigned_at_ms` and `expires_at_ms` in
  `IncidentView`. Say that the end is planned, not enforced.

### D145 · Reading records: `ControlRequest::Incidents` and `ctl incidents`

- **Date:** 2026-11-03 · **Part:** A, C · **Status:** Implemented · **Decided by:** the agent on its recommendation, on
  Noor's instruction to finish without more questions (2026-11-03)
- **Context.** Status only exists when someone reads it (D142), and Noor rejected a label nothing reads. Operators and the
  demo need to see one incident's fields and compare them across nodes, and the web page needs a lookup too.
- **Options considered.**
  - For the request:
    1. *One request: `ControlRequest::Incidents(IncidentQuery)`, with `One(RequestId)` or `OfClient { client, limit }`.*
       A client range costs only the rows returned, and lookup by ID serves the page and the demo.
    2. *`Incident { id }` plus `RecentIncidents { limit }` in reverse key order.* `RequestId` sorts by client first and
       the table is never trimmed, so reverse order shows only the highest client ID that ever reported: clients 1 and 2
       vanish once client 3 has 20 rows.
    3. *The event log only.* It cannot answer "what does the record say now".
    4. *`ctl log`.* D132 rejected it, and this reads results, not the Raft log.
  - For what the reply carries:
    1. *`ControlReply::Incidents { node: NodeStatus, rows }`.* Control frames pass the gate, so a silenced or returning
       node still answers; the reply itself must say so. It reuses a struct, and adds digest and applied index for
       comparing nodes.
    2. *`{ serving, silenced, rows }`.* Less to send, and no digest to compare.
    3. *Rows only, with a second `Status` call.* The two answers could disagree.
    4. *`Error` while silenced or not serving.* The page could not show a silenced node's own view, and it would be one
       more special case.
  - For rows per reply and their order:
    1. *Default 10, cap 20, newest first within one client.* `StateReader::with` holds up applying the log, and sequence
       numbers start at the current time (D113), so the highest is the newest.
    2. *Cap 50.* More time holding up apply, and more than a screen.
    3. *No cap.* One careless call could stall a node under load.
    4. *Sorted by `log_index`.* It needs a scan or a new index.
  - For a lone node's stored answers:
    1. *Not shown: rows are committed records.* The client labels `Provisional`, and `ctl status` shows degraded.
    2. *Shown, through a read accessor in `node/degraded.rs`.* About 8 more lines with no spec line behind them. Add it
       only if the page needs it.
  - For `ctl incidents`:
    1. *`--client C [--limit K]` or `--id C-SEQ`, all three nodes by default, `--node N` narrows, rows print `log_index`.*
       The demo compares one ID across nodes. `ctl` asks one node after another, so under load lists can shift, and a
       brief `Assigned` against `Resolved` is catch-up, which the digest settles.
    2. *One node only.* Comparing nodes would take three commands.
  - For `ctl status`:
    1. *No new columns.* Counting on read would scan on the 1 s client ping, and the table is already wide under `watch`.
    2. *Four count columns at the right end.* Nothing reads them.
- **Decision.** The first choice in each group. `IncidentView { id, incident, status, responder, priority, executor,
  age_ms, assigned_at_ms, expires_at_ms, log_index }`. `expires_at_ms` is an `Option`, `None` if nobody was sent.
  `DEFAULT_INCIDENT_ROWS = 10`, `MAX_INCIDENT_ROWS = 20`; the node applies the cap whatever `limit` says.
  `CloudState::incident_rows(query, now_ms, ms_per_severity)` builds the rows with `results.range` on one client.
  `impl FromStr for RequestId` reads what `Display` writes, so `--id` takes an ID copied from `ctl`'s own output. Each
  node's block prints its role, applied index, digest, serving and silenced, and a note when its rows may be behind.
- **Why.** The first choice in each group keeps a read cheap, truthful about a node that is out of the group, and useful
  for comparing nodes, with no new command, peer message or task.
- **Consequences.** A page must discard rows unless `node.serving && !node.silenced`. If gap 11 moves control to typed
  methods, this becomes an `incidents(query)` method, which must still skip the gate and carry `NodeStatus`. Decisions item
  11 keeps `ctl` on TCP, so a full reply has no datagram limit to meet; it is about 1.4 kB by arithmetic in any case.
- **Where.** `proto.rs::{IncidentView, IncidentQuery, ControlRequest::Incidents, ControlReply::Incidents}`,
  `state.rs::incident_rows`, `node/mod.rs::on_control`, `bin/ctl.rs`.
- **How we check it.** `state::tests::rows_of_one_client_come_newest_first_up_to_the_limit`,
  `a_client_query_never_returns_more_than_the_cap`, `a_higher_client_id_does_not_hide_a_lower_one`,
  `a_row_shows_the_record_and_its_status`, `tests/incidents.rs::rows_for_one_id_match_on_all_three_nodes`,
  `a_client_query_reads_that_clients_newest_records`, `control_rows_say_whether_the_node_is_silenced_or_serving`, the two
  tests in `bin/ctl.rs`.
- **Report notes.** `ctl incidents --id` on three nodes is a good demo of replicated state: the same `log_index` on all
  three.

### D146 · Report owners under open-loop load (amends D54)

- **Date:** 2026-11-03 · **Part:** F · **Status:** Implemented · **Decided by:** the agent on its recommendation, on
  Noor's instruction to finish without more questions (2026-11-03)
- **Context.** Every report now needs an owner. In the closed loop each simulated user is a task, so its index is the
  owner. D54's open loop has no users: `--rate` ignores `--users`.
- **Options considered.**
  1. *`user = sent % --users.max(1)`, so `--users` stops being ignored with `--rate`.* No extra random draw, and a
     population of owners. `.max(1)` avoids a new panic with `--users 0`.
  2. *A random draw from the seeded generator.* It would shift every later draw in a seeded run.
  3. *Always user 0.* The records would say one person sent everything.
- **Decision.** Option 1, in `bin/loadgen.rs::open_loop_owner`. The closed loop uses the user's index.
- **Why.** It is the smallest change that gives open-loop reports real owners. It does not change the rate, and the runner
  never uses open loop.
- **Consequences.** `--users` has a meaning with `--rate` now, and the flag's help says so.
- **Where.** `bin/loadgen.rs`.
- **How we check it.** `bin/loadgen.rs` tests `open_loop_owner_survives_zero_users`,
  `open_loop_reports_take_turns_among_the_users`.
- **Report notes.** None needed.

### D147 · Measuring refusals: `report_expired`, `reports_expired`, and no TTL experiment cell

- **Date:** 2026-11-03 · **Part:** all · **Status:** Implemented · **Decided by:** the agent on its recommendation, on
  Noor's instruction to finish without more questions (2026-11-03)
- **Context.** A refusal should be visible, and a stamping bug must be caught before an hour of runs is spent on it. Field
  names must stay stable once a script reads them, so additions only.
- **Options considered.**
  - For events:
    1. *A new rare `report_expired { request, age_ms, ttl_ms }`; `client_done` gains `status`; `no_responder` stays.*
       Additions only, and analysis already reads `no_responder`.
    2. *The rare event only.* The client side could not be counted.
    3. *A JSONL event on each status change.* Thousands of lines a second.
  - For analysis:
    1. *Add `reports_expired` from `client_done.status`, warn above 0, and add it to the smoke grep in the runbook.* The
       report can show the rule fired 0 times, and a stamping bug is caught by the short check before the long run.
    2. *Add the metric only.* Nobody would look at one number among hundreds.
    3. *No change.* A bad run would look like a very fast one.
  - For an experiment cell:
    1. *None: 31 and 13 cells stay.* The quick hour is fixed, and the spec's measurement list has no TTL item.
    2. *A TTL cell.* It changes D63 and D130.
  - For the clock check:
    1. *Make the runbook's `chronyc tracking` a must before the six-PC test, with no runner code.* A bad clock here moves
       labels and shown times, not plans, and D95's fallback already relies on it.
    2. *A check in `scripts/run-experiments.sh`.* More runner code for something a person can read in a second.
- **Decision.** The first choice in each group. `client_done.status` is `provisional`, `unassigned`, `assigned` or
  `expired`. `client_done.priority` is the plan's priority points. `analyze.py` adds `reports_expired` and `priority_mean`,
  and `expired_warnings` prints a warning to stderr for each cell with `reports_expired` above 0. `no_responder_share`
  still counts an expired report, so it says so beside it.
- **Why.** The rule should fire 0 times in a measured run, and the check that says so has to be loud.
- **Consequences.** `analysis/test_analyze.py` is not in CI, so run it by hand. The runner must never pass `--ttl-ms`.
- **Where.** `metrics.rs::Event::{ReportExpired, ClientDone}`, `client.rs::finished`, `analysis/analyze.py`,
  `analysis/test_analyze.py`, `analysis/README.md`.
- **How we check it.** `analysis/test_analyze.py`: `test_reports_expired`, `test_priority_mean_covers_every_answer`,
  `test_a_cell_with_expired_reports_gets_a_warning`.
- **Report notes.** Show `reports_expired = 0` next to `no_responder_share`.

### D148 · Incident priority: an integer in `Plan`, computed by the planner

- **Date:** 2026-11-03 · **Part:** E, all · **Status:** Implemented · **Decided by:** the agent on its recommendation, on
  Noor's instruction to finish without more questions (2026-11-03); adding it with the incident fields is listed among
  the picks in decisions item 11
- **Context.** The spec has the selected peer compute "an incident priority and a response plan", and asks that the
  computation combine severity, age, location or distance, resource availability and responder capability into a priority
  or assignment decision. It also asks for an algorithm that is deterministic, explainable and tunable. The gap hunt found
  nothing computed, stored or returned a priority: `Plan` held only who goes and how far, and severity and age mattered
  only in the fallback when no matching responder was free (D94, D95). D93 says as much: "Severity and age do not change who
  is nearest".
- **Options considered.**
  1. *A sum of points worked out after the pick and carried in `Plan`; the pick rule stays.* Every factor has a clear
     effect, it costs nothing to compute, and E2 does not move. Nothing acts on the number yet, which the report must
     say.
  2. *Replace the pick with one weighted score over all factors.* It would truly combine them in the assignment, but it
     rewrites D93, D94 and D101, loses "cost is metres", and changes who is sent in every measured run, a day before the
     deadline.
  3. *Order the worker queue by priority.* Urgent reports would be planned first, but the pool's queue is a fair FIFO
     (D48), it would touch the load figures bids are made from, and mild reports could starve under load.
  4. *Rank each incident among all the open ones.* It fits "currently known snapshot" best, but the planner sees one
     incident at a time (D49), and a rank needs a scan over open incidents inside the deterministic core for each plan.
  5. *A field outside `Plan`, on `ResultProposal` and `CommittedResult`.* A lone node answers `Provisional(Plan)`, so that
     client would never see a priority, and the planner would not be the thing that returns it.
  6. *Weights in the config.* It would meet "tunable" more fully, but no experiment reads the number, so nothing would
     tune it, and each weight would be one more key to keep the same on six machines. It can be added later with no wire
     change, because the value travels in the plan.
  7. *Floats.* Forbidden in replicated data.
- **Decision.** Option 1. `Plan.priority: u32`, in **priority points**: higher is more urgent, and one step of severity is
  worth 100 points. It adds up five parts:

  | Factor | Points | Range | Why this scale |
  |--------|--------|-------|----------------|
  | severity | 100 per point of severity | 100 to 500 | the unit everything else is measured against |
  | age | 1 per 50 ms, at most 400 | 0 to 400 | waiting `OLD_AGE_MS` (5 s) counts as one severity step, the same age at which D95 already treats an incident as severe; the cap means the mildest report can wait its way level with a fresh severity 5 and no further |
  | distance | 1 per 10 m to the responder, at most 100; nobody sent counts as 100 | 0 to 100 | help that has farther to come needs attention sooner, and 1000 m is about the widest inside one zone |
  | availability | 100 / (free responders of any capability + 1) | 0 to 100 | it stays near 0 while plenty are free, and rises fast as the campus runs out |
  | capability | 100 if no free responder has the needed capability | 0 or 100 | someone of the wrong capability goes, or nobody does |

  For example, severity 4, 1 s old, with a matching responder 300 m away and 9 free scores 400 + 20 + 30 + 10 + 0 = 460.
  The most it can be at severity 5 is 1200. `planner::not_planned()` gives priority 0 to a report too old to plan (D143),
  because nobody looked at it; anything planned scores at least 100. `Plan::same_assignment` compares only who goes and how
  far, and `node/degraded.rs::settle` uses it for `provisional_conflict`, because two plans made a moment apart can agree
  on who goes and differ in priority. `IncidentView.priority` and `client_done.priority` show it.
- **Why.** It gives the spec's five factors one explainable number, in integers, from inputs the planner already has: the
  severity, the age it is passed, the distance it just worked out, `free.len()`, and whether the capability search found
  anyone. There is no new loop, so planning costs what it did and E2 should not move, though that was not measured. The
  pick rule keeps its tests. Priority is carried in the plan, so it goes through the log and every node holds the same
  value, whatever its own view of the free responders is.
- **Consequences.** The number is an output: it does not change who is sent, and it does not reorder the worker queue, so a
  severity 5 report still waits and can be shed like a severity 1. The report must say so. The age is D95's, on two
  machines' clocks, so a client PC with a wrong clock can move its reports' priority by up to 400 points, though never who
  is sent. Test helpers with `reported_at_ms: 0` get the full 400. The constants need a rebuild to change, so "tunable" is
  met by the load knob and not by these weights. Tests that compared whole plans now compare responder and cost through one
  helper, `picked`, and `age_does_not_change_a_plan_with_a_matching_responder` has to, because priority rises with age on
  purpose. A retried plan keeps the priority of the first commit, since first commit wins.
- **Where.** `proto.rs::Plan`, `planner.rs::{plan, not_planned, Urgency}` and its constants, `node/mod.rs::plan_on_workers`,
  `node/degraded.rs::settle`, `state.rs::incident_rows`, `client.rs::finished`, `metrics.rs`, `analysis/analyze.py`.
- **How we check it.** `planner::tests`: `the_worked_example_from_the_module_docs_is_right`,
  `each_step_of_severity_is_worth_the_same`, `age_raises_priority_up_to_a_cap`,
  `the_mildest_incident_can_wait_its_way_level_with_a_fresh_worst_one_and_no_further`,
  `a_farther_responder_raises_priority_and_nobody_sent_counts_as_farthest`, `fewer_free_responders_raise_priority`,
  `no_free_match_raises_priority`, `the_capability_points_also_apply_when_a_fallback_is_sent`,
  `with_nobody_free_every_factor_but_age_is_at_its_top`, `a_report_not_planned_has_no_priority_and_anything_planned_has_some`,
  `priority_never_changes_who_is_sent`, and the older `the_same_input_always_gives_the_same_plan_in_any_order`.
  `tests/degraded.rs::the_same_responder_with_another_priority_is_no_conflict`,
  `tests/incidents.rs::rows_for_one_id_match_on_all_three_nodes`.
- **Report notes.** Show the table and the worked example. Say what the number does not do: it is computed, returned and
  replicated, but nothing is ordered by it. Name options 2 and 3 as what a later phase could do with it.

### D149 · What was left out of the incident model on purpose

- **Date:** 2026-11-03 · **Part:** all · **Status:** Implemented (nothing to build) · **Decided by:** the agent on its
  recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** A choice not to do something is a decision too (AGENTS.md). Each of these was weighed while designing
  D140 to D148.
- **Options considered.** For each, *leave it out and say so*, or *build it*:
  1. *Deleting expired records.* Deleting at TTL would let a late retry plan again and hide a hold from `node/release.rs`,
     which leaks the responder. Left out: D9 keeps the table.
  2. *A `Reported` status.* It needs two log entries per request and changes E1. Left out.
  3. *Planning a report again before its TTL when a responder comes free.* It needs a second plan for one ID, and first
     commit wins allows only one. Left out.
  4. *Attachments.* Optional in the spec. Left out.
  5. *Owner permissions.* They need authentication, and the spec's owner rules are Phase 2. Left out.
  6. *A visible reconciliation status, `told: Option<Plan>` on `ResultProposal` and `NeedsReconciliation`, about 1 more
     hour.* The spec lists it under Phase 2, and it adds a rule inside first commit wins the day before the deadline. The
     conflict count is the evidence meanwhile. Left out, and named in the report with the false-label limit of D142.
  7. *A deadline beside each frame in the client's send queue.* Item 11's UDP client has no send queue. Left out, and
     stated as a limit in D143.
  8. *A time or scheduling crate.* Existing types cover all of it. No dependency was added.
- **Decision.** All left out. Any of them can come later without undoing this work.
- **Why.** Each one adds behaviour or risk the day before the deadline, and none is needed to meet Noor's rule or the
  spec's Phase 1 list.
- **Consequences.** The report names 2, 3, 4, 5 and 6 as not built, and 6 as Phase 2.
- **Where.** Nothing in the code.
- **How we check it.** Nothing to check.
- **Report notes.** One short list under limits and future work.

### D150 · Peer calls enforce their own deadline, because tarpc times only what it has written

- **Date:** 2026-11-03 · **Part:** C · **Status:** Implemented, amends D119 and D104 · **Decided by:** the agent on its
  recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** `PeerClient::call` handed tarpc a deadline and then waited for tarpc's answer, with no timer of its own.
  Reading tarpc 0.37.0 shows that tarpc starts a call's timer only when it takes the call off its queue and writes it:
  in `src/client.rs`, `poll_write_request` calls `insert_request` only after `poll_next_request` has got past
  `ensure_writeable`. If the socket is full, `ensure_writeable` never finishes, so every later call stays in the queue
  with no timer at all. A machine that lost power sends no close and no ACK, so a socket to it fills up and stays full.
  `PeerSet::broadcast` ends in `join_all`, and the heartbeat loop waits on that. So one such call stops the whole loop:
  no more suspicion of any peer, no degraded mode, no member list, and no log line to say why. The group (D152) sits
  directly on top of this, and so does the failure draw of Noor's item 7, whose round could never close if its own send
  never returned. A silenced node does not trigger it, because it keeps reading its sockets
  (`PeerServer::deliver`), so experiment cells E3 and E4 would never have shown it.
- **Options considered.**
  1. *A timer of our own around the tarpc call, inside `PeerClient::call`.* `timeout_at(deadline, invoke(...))`, with the
     elapsed case mapped to `RpcError::DeadlineExceeded`. Three lines, in the one place every peer call passes. The rule
     that replaces a connection after 1 s of no answers (D119) still fires, because it matches on that same error.
     Dropping the call is safe: tarpc's `ResponseGuard` cancels it.
  2. *An outer timeout in every caller.* Heartbeats, bids, forwarded proposals, the fault driver and the Raft network
     would each need one. Any caller that forgets hangs again, and the next caller to be written has to know about it.
  3. *A socket option such as `TCP_USER_TIMEOUT`.* The kernel would give up on the connection after a while. That does
     not give each call its own deadline, which is 50 ms for a heartbeat and 500 ms for a bid. tokio's `TcpStream` does
     not offer the option, so it needs a new dependency or `unsafe`, which is forbidden. It would also be one more thing
     to tune on the lab machines.
  4. *Leave it, because no rehearsal has shown it.* The one rehearsal cut, at 43 req/s, replaced the connection after
     about 1 s and carried on. Rejected, because the demo's power cut is under load, and this failure is silent.
- **Decision.** Option 1. `PeerClient::call` wraps `invoke` in `tokio::time::timeout_at(deadline, ...)` and maps an elapsed
  timer to `RpcError::DeadlineExceeded`. No new key, message or timer: the deadline is the one the caller already passed.
- **Why.** "Every call has a deadline" (D119) and "each task ends within the timeout" (D104) were true only for calls
  tarpc had managed to write. One small change at the one choke point makes them true for all calls. Using the same
  deadline and the same error means nothing above this line has to change.
- **Consequences.**
  - A call can now return `Timeout` without ever having been written. `Err(Timeout)` already meant "unknown", so no caller
    changes.
  - When our timer fires before tarpc's, tarpc sends a small cancel frame for a call it had written, which its own timer
    would not have sent. It is harmless and is not counted in E5's message counts, only in `net_tx_bytes`.
  - The tarpc task of a replaced, stuck connection stays alive with its queue until the kernel gives up on the socket, by
    my reading about 15 minutes at Linux defaults. It is bounded: at most `CALL_BUFFER` calls on one connection per outage,
    because a connect to a dead machine fails and then backs off (D89). Not changed here.
  - This does not bound `client_write` in `raft/mod.rs`, which is a separate path with no deadline. It is not a peer call,
    so this entry does not touch it.
- **Where.** `node/transport.rs::PeerClient::call`, and two new tests at the bottom of that file.
- **How we check it.**
  - `transport::tests::a_call_to_a_peer_that_never_reads_returns_by_its_deadline`: a listener accepts and never reads, 12
    calls of about 2 MiB each with a 300 ms deadline fill the socket, every call must return `Timeout` within 2 s of its
    deadline, and the connection must then be replaced.
  - `transport::tests::a_broadcast_returns_with_one_stuck_peer_and_reports_the_healthy_one`.
  - Both were seen red on the code as it stood at a98a1c3, with calls still out 10 s after a 300 ms deadline. The messages
    were "a call queued behind a full socket hung past its deadline" and "one stuck peer held up the whole group send".
    Both are green with the fix.
  - Rehearsal, one machine, debug build, no disk, about 440 req/s, private network namespace, with node 1 cut by SIGSTOP plus
    dropped packets on its ports, on builds with and without this fix:

    | Build | Send buffer limit | Result 8 s after the cut |
    |-------|-------------------|--------------------------|
    | without the fix | 64 KiB | no hang: the list changed as normal, because the socket did not fill inside the 1 s before the connection is replaced |
    | without the fix | 8 KiB, staged so that it fills inside that second | **hang**: both survivors logged `peer_suspected` once, and their lists stayed at `1,2,3` |
    | with the fix | 64 KiB | the list went to `2,3` about 2 s after `peer_suspected` |
    | with the fix | 8 KiB | the same |

    So the hang needs the socket to fill in about a second. How likely that is on the lab machines depends on their load and
    send buffer sizes, and neither is known yet.
  - Six-PC check, not done: a real power cut at the demo's load with `ctl status` on screen. Pass means `peer_suspected` on
    both survivors within a second, and `degraded = true` on the last node in scene 6.
- **Report notes.** A good example of a fault only a dead machine shows: silence alone never triggers it. Until the six-PC
  test has run, say plainly that it is proven with a listener that never reads and with a staged small buffer on one machine,
  not with a power cut. Library credit: this is about tarpc's behaviour, so name tarpc in the sentence.

### D151 · A suspected peer stays suspected through this node's own silence

- **Date:** 2026-11-03 · **Part:** C · **Status:** Implemented, amends D79 (item 4) · **Decided by:** the agent on its
  recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** D79 item 4 says that while a node is silenced it judges nobody, and that on restore every peer's clock starts
  fresh. That was right for peers it trusted, because it could not have heard from them. It was wrong for a peer it already
  suspected. Coming back from its own silence tells a node nothing about that peer, yet `Suspicion::reset` gave it a fresh
  clock, so `update` trusted it again straight away and suspected it again 150 to 200 ms later. In demo scene 6 that means
  node 2, back from a silence, lists the powered-off node 1 as alive. With the 2 s hold on the member list (D153), the wrong
  entry would last about 2.2 s, which is long enough to see on screen and long enough to send it bids.
- **Options considered.**
  1. *`Suspicion::reset` skips peers already suspected.* One condition. A suspected peer stays suspected until it is heard.
  2. *Leave it and explain it in the demo.* No code, but a list that says a dead machine is alive right after a fault is the
     thing the list exists to get right.
  3. *Keep suspected peers' clocks, but also throw away the suspicion start time.* The hold would start over on restore, so a
     dead peer would stay listed for another 2 s. No benefit over option 1.
- **Decision.** Option 1. `reset` gives a fresh clock only to peers this node does not suspect. A peer that was suspected
  keeps its old clock and its old start time, so hearing from it is the only thing that ends the suspicion.
- **Why.** The clock measures how long since a peer was heard, and a node's own silence is no evidence for a peer. Peers it
  trusted get the benefit of the doubt, as D79 intended, because the node really could not hear them.
- **Consequences.**
  - `peer_trusted`'s `suspected_ms` can now include this node's own silence: 44,198 ms in the rehearsal below.
  - E3's detection time is not affected, because a restored node no longer logs a second `peer_suspected` for a peer it
    already suspected. That pair was noise.
  - One test pinned the old behaviour and had to change: `reset_starts_every_clock_fresh` became
    `reset_gives_a_peer_nobody_suspects_a_fresh_clock` and `a_suspected_peer_stays_suspected_through_reset`. The reason is in
    the commit message, as AGENTS.md asks.
  - `tests/heartbeat.rs::a_silenced_node_is_suspected_by_both_others_and_trusted_after` is unchanged and still passes,
    because node 2 suspected nobody before its silence.
- **Where.** `node/heartbeat.rs::Suspicion::reset`.
- **How we check it.** `heartbeat::tests::a_suspected_peer_stays_suspected_through_reset`, seen red on a98a1c3 and green
  after. `tests/group.rs::a_peer_suspected_before_a_silence_stays_out_after_restore`, which fails if this fix is taken out.
  Rehearsal, one machine: node 2's `group_view` log went 1,2,3 then 2,3 then 1,2,3 only when node 1 really came back, with a
  12 s silence of its own in between.
- **Report notes.** A small point, but it is what makes scene 6's list true. It shows why a detector's state and a node's own
  fault need separating.

### D152 · Group communication built: CloudGroup with a per-node view, send and probe

- **Date:** 2026-11-03 · **Part:** C, B · **Status:** Implemented, amends D126, amended by D210 · **Decided by:** Noor Hany Emam (item 8 of the
  decisions of 2026-11-03: a named group of nodes with a who's-alive member list that changes when a node dies or returns;
  send once to the group and it reports who got it; bids, pings and the failure draw all use it). The code shape, names and
  return value were decided by the agent on its recommendation, on Noor's instruction to finish without more questions
  (2026-11-03).
- **Context.** D126 gave the sends we already had a name and a table of guarantees. Noor looked at that and rejected it as the
  answer, because a grader may say it is just a loop. The Oct 28 class note says to implement group communication, and the
  Sept 13 note says the application sends once and the middleware gives the replicas their copies, with the two layers kept
  apart. As merged, nothing in the code held a list of members and no send depended on one.
- **Options considered.**
  1. *A cloud group whose view steers bids.* One object in one new file. It knows the members from the config and keeps this
     node's own list. Bids go to the list, pings go to everyone, and each change is numbered and logged. Nothing new goes in
     the Raft log, and no new message goes between nodes. Chosen.
  2. *The same group, with the list only as a report.* `send` still goes to all three whatever the list says. It costs under
     an hour less and leaves the bid path alone, but the copies, the addressees and the order are then identical to
     `PeerSet::broadcast`, so Noor's objection stands: it is a loop with a list beside it. It also keeps about 22,500
     unanswerable bids per survivor per 15 s fault at 1500 req/s (arithmetic).
  3. *Agreed views through the Raft log.* Every node would have the same list. Nothing would read the agreement: heartbeats,
     takeover, degraded mode and bid rounds all stay on local data. The list could not change without a majority, so in the
     class-note case of one machine off and one failed it would still show two members with one alive. It also sends list
     changes back through the leader, against Noor's items 1 and 7, and was estimated at 10 to 13 hours. No source we have
     asks for agreement.
  4. *D126 as it stood, naming only.* Rejected by Noor by name.
  5. *Our own reliable, ordered multicast, or a full view-synchronous group.* Rejected by Noor: Raft already does ordering and
     delivery for results, and bids are worthless late, so retrying them would be wrong.
  6. *A group-communication library.* Another library doing distributed-systems work (AGENTS.md rule 6), to replace code that
     works. D126 already rejected it, and that stands.

  For the code shape: *`node/group.rs` with `NodeContext.group: Arc<CloudGroup>`, a `Mutex<View>` and a pure `next_view`*
  (chosen: a file a grader can open, the same pattern as `BidState` and `Suspicion`); *inside `PeerSet`* (mixes who is alive
  with sockets); *a tokio watch channel updated on each change* (two peers changing in one tick would make two views).

  For the calls: *`send(timeout, body)` and `probe(timeout, body)`, with the group reading the term* (chosen: the term rule
  lives in one place, and pings need 50 ms while bids need 500 ms); *`multicast`* (the slides use that word for the level
  below); *`send(term, timeout, body)`* (every caller would have to get the term right).

  For the return value: *each addressed peer's `Result`, as `broadcast` returns it* (chosen: callers change no logic); *a new
  `Delivery` type* (a new type to learn, carrying the same facts).

  For the level underneath: *keep `PeerSet::broadcast` and add `broadcast_to` with a filter* (chosen: the smallest change, the
  test D126 cites keeps working, and it matches the course picture of multicast as the primitive below group communication);
  *move the code into `group.rs` and delete `broadcast`* (a bigger change for no behaviour).
- **Decision.** Option 1. `CloudGroup { me, all, peers, consensus, log, view: Mutex<View> }`, with `View { number, members }`.

  | Call | Goes to | Guarantee | What covers a lost copy |
  |------|---------|-----------|-------------------------|
  | `CloudGroup::send`: bids | the peers in this node's view | best effort, unordered, no retry; reports who acknowledged in time | a split decision, settled by first commit (D27) |
  | `CloudGroup::probe`: pings | every configured peer | best effort, unordered, no retry; reports who acknowledged in time | the next ping; a real loss becomes a suspicion (D79) |
  | `MeshClient::submit`: from outside the group | all three nodes | best effort, unordered | the client's retry with the same ID (D52) |
  | Committed state: Raft, from openraft | every replica | totally ordered; reaches every replica once committed; an error means unknown | openraft's log replication |

  Each send is two unicasts from a node and three from a client, not IP multicast. How to read an answer: `Ok(Ack)` was
  delivered, and only this counts as delivered. `Ok(StaleTerm)` was answered but dropped by the term rule. `Err(Timeout)` and
  `Err(Closed)` are unknown, because it may still have arrived. `Err(Backoff)`, `Err(Connect)` and `Err(Silenced)` were never
  sent.

  Not added, on purpose: retries or ordering of our own, a new peer message, a group ID, a group library, IP multicast.
  Results still call `Consensus::propose` directly; `send_ordered` is D156 and comes later. The receive side stays plain
  dispatch in `on_peer_message`, and the sender's own copy stays with the caller, as `bids.rs` already does: a `GroupHandler`
  upcall with a loopback would change no behaviour and break AGENTS.md rule 3, "hook in, don't rewire".
- **Why.** Noor's item 8 asks for a group that is more than a name, and rejects building our own guarantees. A list that decides
  where a send goes answers the first, and leaving order to Raft answers the second. The list costs no new log entries,
  commands, tasks, timers or messages, so it cannot affect safety, which rests on Raft and first commit wins.
- **Consequences.**
  - It is not agreed views. Each list is one node's opinion. If a reader wants agreed views, this does not give them, and the
    report says so.
  - The primitive stays public and `tests/heartbeat.rs::a_group_send_reaches_every_peer_and_reports_who_missed_it` stays on it.
  - `NodeContext` gains a field; the group must be built after Raft exists, which `start_with` already allows, so there is no
    cycle. It is an `Arc` because `send_bid` moves it into a task.
- **Where.** `node/group.rs`, `node/transport.rs::PeerSet::{broadcast, broadcast_to}`, `node/mod.rs::{NodeContext, start_with}`,
  `node/bids.rs::send_bid`, `node/heartbeat.rs::spawn`.
- **How we check it.** `tests/group.rs` (see D153 to D155). Each test there was seen to fail against a build broken on purpose
  in the way it guards: pings following the list, `send` ignoring the list, `reset` forgiving a suspected peer, no hold, first
  view not logged, status reporting an empty list. 10 runs in a row, all green, of `tests/group`, `tests/heartbeat`,
  `tests/takeover`, `tests/bidding`, `tests/degraded`, `tests/faults` and the unit tests of the three files. Those runs were
  on a machine with other jobs on it, so they say less than runs on a quiet machine would.
- **Report notes.** The course picture fits the code: `PeerSet::broadcast` is multicast support, and `CloudGroup` sits on top as
  group communication. Credit openraft for order and delivery on the ordered path. Reading of the term: D157.

### D153 · View rule: a peer leaves after degraded_after_ms of unbroken suspicion and returns when heard; Raft's voters never change

- **Date:** 2026-11-03 · **Part:** C · **Status:** Implemented · **Decided by:** the agent on its recommendation, on Noor's
  instruction to finish without more questions (2026-11-03). That the list changes when a node dies or returns is Noor's, from
  item 8.
- **Context.** The list needs a rule for when a peer leaves and comes back, a timer, a start state, and an answer for what a
  silenced node shows. It also has to say how it relates to Raft's membership, which D25 fixed.
- **Options considered.**
  1. *When a peer leaves:* after `degraded_after_ms` of unbroken suspicion, and back as soon as it is heard (chosen); as soon as it
     is suspected, in about 150 to 200 ms (a 150 ms false alarm would cut rival bids from a live node, and it would then win
     every round it sees, and the list would flicker on the demo screen); when it is unreachable or not serving, with `serving`
     added to the heartbeat reply (a wire change on about 240 messages a second, D79).
  2. *Which timer:* reuse `timers.degraded_after_ms` (chosen: one timer to prove on the lab, the same meaning as D121, no cell
     file changes, and in scene 6 the list and degraded mode flip together); a new key `group_leave_after_ms`; a constant in
     `group.rs` (a timer the config cannot reach).
  3. *Members and Raft voters:* fixed from `cluster.nodes`, all three stay voters, and the view sits on top (chosen, D25 stands);
     a join and leave protocol (nothing joins); call `change_membership` when the view changes (one false suspicion would
     shrink the quorum, and a silent node keeps its vote and log, D75).
  4. *View on a silenced node:* frozen, and `ctl` prints `-` (chosen: no extra code, because the heartbeat loop already skips a
     silenced node, and `-` stops a frozen list reading as a live one); frozen, and `ctl` prints it; shrink to itself on silence
     (a node that is cut off cannot judge others, D79).
  5. *Starting view and number:* all members at start, with the number in memory, restarting at 1 and never compared (chosen: it
     matches D79's reset at start, and the number only orders one process's changes); only itself until it hears from peers (a
     healthy start would look like a failure); number stored on disk (nothing needs it).
- **Decision.** The heartbeat loop reads `Suspicion::suspected_for(hold, now)` with `hold = timers.degraded_after()`, under the lock
  it already holds, and after logging the suspicion changes calls `CloudGroup::update(&left)`. `update` uses a pure
  `next_view(current, all, me, left)`, the one place the view changes. A node never removes itself. The number rises only on a
  change, so two peers changing in one tick make one view. The first view is logged from `heartbeat::spawn`, not from
  `CloudGroup::new`, which runs before `started` is logged.
- **Why.** The detector stays underneath, quick and jumpy, as takeover needs. The list is a slower, steadier layer on top: a one-tick
  false alarm changes nothing, and a peer can change the list at most about once per 2 s. Returning at once is safe in the other
  direction, because hearing from a peer is good evidence, and a wrong return costs a few unanswered bids.
- **Consequences.**
  - The list lags. It changes about 2 s after `peer_suspected`, and a fault under about 2.2 s never shows, which is about 6% of a
    uniform 1 to 20 s draw. Say so in the demo.
  - Members means reachable, not able to serve. Between a node's return and `serving = true` it is listed and answers nobody, so
    do not narrate `members` as health. Point at leader, serving and degraded as well.
  - Lists differ between nodes, and a silenced node's list is frozen for up to 20 s.
  - `timers.degraded_after_ms` now has two uses, so changing it moves both. Its comment says so in `config.rs` and in both config
    files. D121 itself is unchanged.
  - A log file is appended, so a restart drops the number back to 1 in the same file. Never compare numbers across a restart or
    across nodes.
- **Where.** `node/group.rs::{View, CloudGroup::update, next_view}`, `node/heartbeat.rs::{Suspicion::suspected_for, spawn}`,
  `config.rs` and `config/*.toml` (comments only).
- **How we check it.** `group::tests` (the number rises only on a change, two peers leaving in one tick make one view, a node never
  removes itself, a peer under the hold stays, a heard peer returns at once). `heartbeat::tests::suspected_for_counts_only_unbroken_suspicion`.
  `tests/group.rs::a_silenced_member_leaves_after_the_hold_and_returns_when_heard`, which also checks that `started` comes before
  the first `group_view` and never asserts a view number. Rehearsal, one machine, in the shape of scene 6: node 3's `group_view`
  log went 1,2,3 then 2,3 then 3 then 2,3 then 1,2,3, and `degraded_entered` came 2.05 s after it suspected node 2.
- **Report notes.** State the limits: each list is one node's opinion, the members are fixed at three, and the view never reaches
  Raft, a `Command` or the state digest.

### D154 · Bids and the failure draw follow the view; pings probe every configured peer

- **Date:** 2026-11-03 · **Part:** C, B · **Status:** Implemented for bids and pings; the failure draw adopts it when it lands;
  amends D104 · **Decided by:** Noor Hany Emam for "bids, pings and the failure draw all use the group" (item 8). The split
  between `send` and `probe`, and what reads the view, were decided by the agent on its recommendation, on Noor's instruction to
  finish without more questions (2026-11-03).
- **Context.** Once a list exists, each send class has to say whether it follows it. D104 said a bid always goes to the two peers.
  A list nobody reads is the report-only group that D152 rejected, but a list that everything follows cannot heal.
- **Options considered.**
  1. *Who each send addresses:* `send` to view members and `probe` to every configured peer (chosen); every configured peer for
     everything (the report-only group, D152 option 2); view members for everything (two live nodes that removed each other would
     never hear each other again, and a 20 s fault would last for ever).
  2. *What reads the view:* `send` and `status` only, while takeover and the degraded trigger keep reading `Suspicion` (chosen); also
     swap the degraded trigger to a "the list is only me" check (it rereads the same suspected set, so it changes nothing and adds a
     dependency on a fault path); also takeover (takeover must act in about 150 ms, not 2 s).
  3. *Client side:* no change, clients are outsiders sending to an open group through `MeshClient` (chosen, a reading of the spec's
     "multicast the request to the three cloud servers"); the client keeps its own view from ping replies (a lone node off a list
     must still get requests, and the client path is being reworked separately).
- **Decision.** `bids.rs::send_bid` calls `ctx.group.send`. `heartbeat.rs` calls `ctx.group.probe`. The failure draw of Noor's item 7
  goes through `send`, under two rules: its round closes on `config.cluster.nodes`, never on the view, and only `Ok(Ack)` counts as
  delivered, because `Ok(StaleTerm)` was answered but dropped. `node/takeover.rs` and `node/degraded.rs` are unchanged.
- **Why.** Pings are how a node finds out a peer is back, so they must reach peers off the list. Bids to a peer that has been silent for
  2 s cannot be answered in a 20 ms window, so sending them is wasted work: a bid is worthless late. Following the list is also the
  one thing that makes it more than a report.
- **Consequences.**
  - Healthy runs do not change, because everyone is on the list.
  - Bid counts fall in fault cells, so `msgs_bid_per_req` drops against any older run, and final data must come after this lands. At
    1500 req/s and a 15 s silence that is about 3,300 unanswerable bids per survivor instead of 22,500. That is arithmetic, 2.2 s
    against 15 s, not a measurement.
  - A live node wrongly suspected for over about 2.2 s gets no rival bids from the peers that removed it, so it wins every round it
    sees and plans the request as well. First commit wins keeps that safe (D27). A node that cannot answer a ping for 2 s is unlikely
    to read a bid inside a 20 ms window anyway. That is from reading: no run at the top load or on the real PCs exists yet.
  - One line reverts it: make `send` address every peer again.
- **Where.** `node/bids.rs::send_bid`, `node/heartbeat.rs::spawn`, `node/group.rs::{send, probe}`.
- **How we check it.** `tests/group.rs::send_reports_who_answered_and_skips_a_member_who_left`;
  `tests/group.rs::probe_still_reaches_a_peer_outside_the_view`, which makes two live nodes remove each other by swapping which one is
  silent, and fails if pings follow the list; `tests/group.rs::bids_reach_a_returned_node_again`, which counts node 1's bid copies
  per round going 2, then 1, then 2, and waits for node 3 to see 3 bids in one round again. For the report: E5's `msgs_bid_per_req`
  in fault cells.
- **Report notes.** E5 can give one number here: wasted bids in fault cells, before and after. Measure it; do not quote the arithmetic
  as a result.

### D155 · NodeStatus gains members; ctl status shows them; the view number stays in group_view

- **Date:** 2026-11-03 · **Part:** C, all (`proto.rs` contract) · **Status:** Implemented, amends D79 (item 6) · **Decided by:** the
  agent on its recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** A list only the code can see cannot be shown in a demo, or checked by the person running it. D79 item 6 said no contract
  change for the suspicion list. This changes that for the member list.
- **Options considered.**
  1. *`NodeStatus`:* `members: Vec<NodeId>` as the last field, with no number (chosen, the same move as D121's `degraded`); also `view:
     u64` (numbers restart with a process and mean nothing across nodes, so different numbers in adjacent rows would read as
     disagreement); no change, with the view only in logs (then it is a grep on three machines).
  2. *`ctl status`:* one new last column, `members`, printed as `1,3` (chosen: nothing read by position moves); one combined column like
     `v2:1,2,3`; a separate `ctl group` command (one more thing to run in a demo).
  3. *Event:* `group_view { view, members }` at start and on each change, with no change to analysis (chosen: `peer_suspected` and
     `peer_trusted` already name the peer, and analysis filters by `ev`); add `left` and `joined` fields (repeats those events); add a
     `view_changes` metric now (the first lab run is counted with one grep, and no rate should be claimed before that).
  4. *Demo:* point at `members` in scenes 3, 4, 5 and 6, plus one grep of `group_view` (chosen: no new rehearsal step); a new scene of
     its own.
- **Decision.** As chosen above. `ctl status` prints `-` for a silenced node, whose list is frozen. The number stays only in `group_view`.
- **Why.** The smallest change that lets someone point at the list. The last field and the last column leave everything else where it is.
- **Consequences.**
  - The wire format of `NodeStatus` changes. A stale binary on any PC fails to decode the longer frame. Every client ping gets one, so
    its links churn with "bad frame", and `ctl` errors. Rebuild all six from one commit with `scripts/lab-build.sh`, as AGENTS.md already
    requires.
  - The table grows from 101 to 109 characters, so widen the demo terminals; `watch` cuts at the terminal width.
  - `scripts/run-experiments.sh` reads columns 1 and 2 only, so it is unaffected. `status-after.txt` gains a column that nothing reads.
  - The stub in `tests/client.rs` answers `Done`, so the longer `NodeStatus` does not reach it, and it is unchanged.
- **Where.** `proto.rs::NodeStatus`, `node/mod.rs::NodeContext::status`, `bin/ctl.rs::{status, members_column}`, `metrics.rs::Event::GroupView`.
- **How we check it.** `tests/group.rs::status_reports_members`, including over a real control frame to a silenced node. `ctl::tests::
  members_print_as_a_list_and_as_a_dash_while_silenced`. Seen in a real `ctl status` table in the rehearsal above. Six-PC check, not done:
  count `group_view` lines in the first `e1-3000` lab run and report the number.
- **Report notes.** Events: `group_view { view, members }`. Do not narrate `members` as health (D153).

### D156 · Application layer moved to node/service.rs; send_ordered, src/middleware.rs and tests/layers.rs enforce the split

- **Date:** 2026-11-03 · **Part:** A, C, F · **Status:** Implemented, required by D211. Its "Where" and "How we check it" below were written as a plan: read `src/node/service.rs`, `src/middleware.rs` and `tests/layers.rs` for the final call sites · **Decided by:** the agent on its recommendation,
  on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** The Sept 13 class note says the application sends once and the middleware makes the copies, and to keep the two layers
  distinct in the implementation. `CloudGroup` gives the middleware side a name, but `node/mod.rs` still mixes both layers: the incident
  service sits beside the wiring, and it calls `consensus.propose` directly.
- **Options considered.**
  1. *`node/service.rs`, `src/middleware.rs`, a layer table and `tests/layers.rs`.* The move takes about 30 to 45 minutes and gives the
     test one real file to guard. Chosen.
  2. *Facade and test only, with the report saying `node/mod.rs` mixes the layers.* Cheaper, but the note says "in implementation".
  3. *A table and file headers only.* Nothing enforces it.
  4. *Physically move files into `src/middleware/`.* About 5 more hours for the same behaviour. It breaks AGENTS.md rule 3, collides with
     all the other work, and leaves old log entries naming old paths.

  For the ordered send: *`group.send_ordered` at the call sites, a plain pass-through to `consensus.propose`* (chosen: the application gets
  one interface, and there is no ordering code of ours, so item 8's rejection holds; `propose_local` stays); *leave `consensus.propose` in
  place and only document it*.

  For the reading of which code is in which layer: *application: users, the incident service in `node/service.rs`, planner, campus, state
  rules; middleware: `MeshClient`, `CloudGroup`, bidding, detector, rejoin, degraded mode, tarpc, openraft* (chosen, because users never
  know who executes, so the election serves the application from below); *count bidding as application*.
- **Decision.** Option 1, in this order, as one commit. Move `on_client_frame`, `handle_request`, `execute`, `plan_on_workers`,
  `send_provisional` and `send_done` out of `node/mod.rs` into `node/service.rs`, with no behaviour change; `decide_executor`,
  `leader_dispatch` and `on_control` stay in `mod.rs`, and `degraded.rs` changes its import. Run `tests/layers.rs` now: it must be red on
  `.consensus`. Then add `send_ordered(cmd)` and `term()` to the group, switch the callers, add `src/middleware.rs` re-exporting
  `MeshClient`, its types and `CloudGroup`, and make `bin/loadgen.rs` import through it. Add a layer table to `lib.rs`.
- **Why.** Only the note's own words, "in implementation", make this worth doing, and a test nobody has seen fail proves nothing. Matching
  `consensus.propose(` as first specified would be blind: rustfmt splits that call across lines, so it would pass on code that breaks the
  rule. The test strips whitespace before matching, and must be shown red before `send_ordered` goes in.
- **Consequences.**
  - `send_ordered` inherits a hang: `client_write` has no deadline. Say so in its doc comment and give new callers their own timeout.
  - The pure move touches the functions the client-path and incident-field work edit, which is why it goes last. Line numbers in older
    entries move.
  - The other work may give `bin/loadgen.rs` a reader in `node/usage.rs`. Re-export it through `src/middleware.rs` or move the parsers to
    `metrics.rs`, or the second change to merge fails the first one's test.
  - If this is dropped, drop the Sept 13 layer claims from the docs and the report as well. The Oct 28 note is answered without it.
- **Where.** Planned: `node/service.rs`, `src/middleware.rs`, `tests/layers.rs`, `node/group.rs::{send_ordered, term}`, `lib.rs`,
  `node/mod.rs`, `node/release.rs`, `node/degraded.rs`, `bin/loadgen.rs`.
- **How we check it.** Planned: `tests/layers.rs::application_files_stay_above_the_group` (whitespace stripped; no `.consensus`, `.peers`,
  `.broadcast(` or `TcpStream` in `node/service.rs`, `planner.rs`, `campus.rs` or `state.rs`), shown red first;
  `tests/layers.rs::loadgen_imports_only_through_the_facade` (`rescuemesh::middleware`, and no `rescuemesh::client`, `rescuemesh::node` or
  `rescuemesh::raft` in `bin/loadgen.rs`).
- **Report notes.** Only claim the layers in the code once this is merged.

### D157 · Reading of the class note on group communication; D126's question withdrawn

- **Date:** 2026-11-03 · **Part:** all · **Status:** Implemented, amends D126, amended by D210 · **Decided by:** the agent on its recommendation, on Noor's
  instruction to finish without more questions (2026-11-03). Noor's standing rule, that the spec and class notes decide and nothing is left
  as a question for the instructor, is what this entry applies.
- **Context.** D126 ends by leaving open whether group communication is expected in Phase 1. Noor's rule is that the spec and the class notes
  decide. AGENTS.md also forbids rewriting an old entry. The sources we have do not define the term, and the chapter that would is not among
  them, so the choice has to be stated as a reading.
- **Options considered.**
  1. *Reading: a named group with members, a view that decides who at least one send goes to, one call per class of send, and stated
     guarantees.* Chosen.
  2. *Any send-to-all with a name.* D126 as it stood. Noor rejected it, and the Oct 28 note says "implement".
  3. *A full view-synchronous group with reliable multicast for every send.* Noor rejected our own guarantees, and Raft already orders results.

  For the old note: *a new entry withdraws it, D126's text stays as history, and the current docs are edited* (chosen: it satisfies both the rule
  and AGENTS.md); *edit D126 directly* (AGENTS.md forbids it).

  For the words used about guarantees: *"who acknowledged in time"; "totally ordered, reaches every replica once committed, an error means unknown";
  "two unicasts from a node, three from a client"; book terms only after checking the book* (chosen: true statements); *D126's table as it was*.
- **Decision.** Option 1, and it is Phase 1 work. It rests on these sources:
  - Class notes, Oct 28: implement group communication. Discovery is the only part that note moves to Phase 2, so this reads as Phase 1.
  - Class notes, Sept 13: the message is sent once at the application layer and the middleware gives the replicas their copies. Here, one call each:
    `submit`, `send`, `probe`.
  - Chapter 2, slide 7, lists multicast under interprocess communication and group communication under indirect communication. Chapter 5 puts indirect
    communication in the middleware above multicast support. The code has that shape: `PeerSet::broadcast` is the primitive and `CloudGroup` sits on top.
  - Project spec: a node goes silent, "rejoin[s] the group", and must become consistent before it resumes. The list shows the first step, and `serving`
    shows the second. That order is a reading.
  - Project spec: the client middleware multicasts to the three servers. This stays, as an outsider sending to an open group. Reading.
  - Class notes, Sept 13: one node off and one failed, then only one server left. Node 3's list ends at 3 alone.
  - Class notes, Oct 28: no coordinator. No node manages the list, and the leader takes no part.

  D126's closing note is withdrawn. Nothing is left to ask.
- **Why.** A stated reading with its sources can be checked, and it can be disagreed with on its merits. An open question can only wait.
- **Consequences.** If a reader wants agreed views, this loses marks on that one line; the report says plainly that each list is one node's opinion. The
  same text goes in the report's decision list and Q&A, and the lines in `docs/architecture.md` section 14 and `docs/demo.md` that leave questions are
  removed by whoever merges the docs.
- **Where.** `docs/design-log.md`, `docs/architecture.md`, `docs/demo.md`, the report.
- **How we check it.** A search for "Dr Amr" in the docs finds only citations of what the notes say and his named failure cases, and no open questions,
  apart from old entries kept as history.
- **Report notes.** Say "our reading", and give the limits. Publish-subscribe has its own entry.

### D158 · The three nodes elect the failed node: FaultBid rounds replace the leader's fault schedule

- **Date:** 2026-11-03 · **Part:** C (contract change in `proto.rs`; hooks in A's `node/mod.rs` and `raft/network.rs`;
  comments in A's `config.rs`) · **Status:** Implemented, supersedes D12 and D116, amends D78 · **Decided by:** Noor Hany
  Emam (item 7 of the 2026-11-03 sitting). One difference from Noor's words, flagged under Decision, was decided by the
  agent on its recommendation, on Noor's instruction to finish without more questions (2026-11-03).
- **Context.** The spec asks that the servers periodically elect one machine to ignore communication for a random time
  of up to 20 s, and the class notes say to mimic failure using election and that the servers are peers with no
  coordinator (Sept 13, Oct 28). Until now one node did all of it: the Raft leader drew a victim alone and ordered it
  silent (D12, D78), and D116 kept that, on the reading that a schedule run by an elected node might fit. A literal reader
  finds no election there. Three other things were wrong with it:
  - It was the clearest coordinator job left on the leader, the job D78 itself said must be justified or changed.
  - With one machine off, the leader's draw picked a live node and silenced it 2 times in 3, so a power cut plus the
    schedule gave two nodes down.
  - Neither the demo nor the quick profile ever ran it, so the report had no data on it.
  D116's closing line left the question with a person outside the team. Noor's rule is that the spec and class notes
  decide, so this entry replaces that line and nothing here waits on anyone's answer.
- **Options considered.**
  1. *All three nodes draw a number over the group, and the highest goes silent (chosen).* A round held for this purpose,
     all three in it, the result is the machine that fails, that machine silences itself, no leader involved, and the
     round can be counted. It needs a new message and a small state machine, and all three logging the same line is usual,
     not guaranteed.
  2. *Votes through the Raft log.* Each node commits a ticket as a `Command`, so the result is agreed. Noor's item 7
     rejected this by name, because it goes back through the leader and adds a saved command. Checked against the code as
     well: log order picks which two tickets compete, every ballot passes through the leader, a node replaying its log
     after a power cut needs a guard so it does not silence itself again, and that guard is racy as sketched. Votes need
     commits, and one rehearsal had no commits at all for over a minute. It would change `Command`, `CloudState`, apply
     and the digest a day before the deadline. Its one gain, an agreed record, is something no document asks for.
  3. *The elected Raft leader silences itself.* Raft's own election would be the election. No round is held for the
     fault, so the spec's sentence stays unmet. Node 3 would almost never fail, because of D115's election ranges. With
     one machine off every round would still leave one node up. A fresh Raft election per fault is not available either:
     openraft 0.9.25 rejects votes during the leader lease, and has no leader-transfer trigger.
  4. *Keep the leader's draw and report an accepted risk.* No code. A literal reader finds no election, it goes against
     Noor's items 1 and 7, the leader keeps its last coordinator job, and the power-cut hazard is managed only by a setting.
  5. *The winner of a real request's bid round goes silent, every Nth request.* It touches the hot path of every cell,
     ties the fault rate to load and not to a timer, and mostly hits node 1 on ties.
- **Decision.** Option 1.
  - Every `faults.interval_*` (30 to 60 s) the three nodes draw: each picks a random ticket and a random silence (1 to
    20 s) and sends both to the other two as `PeerBody::FaultBid { round, ticket, ms }`, through the group of D152.
  - A node acts only when it holds a bid from **every node in `config.cluster.nodes`**, never only the ones on its
    who's-alive list. A node missing any of the 3 numbers does nothing.
  - The highest ticket is elected, ties to the lower node ID. The elected node closes its own gate for the `ms` in its
    own bid. Nobody orders anybody.
  - A node that is off, silenced, catching up or degraded sends no bid, so no new round can finish until it is back. That
    is the whole of the rule that keeps two nodes from going down together.
  - No leader check anywhere on this path. Nothing goes into the log: `state.rs`, `Command`, the digest and the Raft log
    file are unchanged.
  - `ctl silence` stays as it is (D74). `FaultOrder` is removed in both directions (D160).
  - **The one difference from Noor's item 7.** Noor's words read as each node on its own timer. Machines with no shared
    clock would not land in one round that way, so the first timer to run out starts the round, and the others draw at
    once when they hear of it. The caller's ticket is as random as anyone's, so calling a round gives it no power. Noor can
    overrule this: it changes `FaultRounds::step` and nothing else.
- **Why.** It is the only option where the spec's sentence can be seen happening in the running system. D12's reason for
  having no election was that one leader means exactly one victim at a time. The all-numbers rule now gives that with no
  leader, and the argument has no timer in it:

  > Say X silences itself for round `a` at time `t`, and Y closes round `b` after `t`, before X is back. Y needs X's bid in
  > `b`, which X sent before `t`. If `b = a`, both hold the same first bids, so Y names X and stays up. If `b > a`, X replaced
  > `a` when it bid in `b`, so X never closed `a`. If `b < a`, X needed Y's bid in `a`, so Y had already left `b` and cannot
  > close it.

  This holds for one driver per node, one draw per round, and no restart inside a round. It also removes the 2-in-3 chance
  of a second silence with a machine off, and the gate's only exception (D160). Lost bids and stale terms only reduce the
  number of nodes that close a round, so they are safe by construction: a missed fault harms nothing.
- **Consequences.**
  - No Raft agreement on the outcome. Agreement here comes from identical inputs, and a node that misses a bid logs a null
    victim while the others name one. The report must say so, and must not call the round agreement.
  - A winner that misses a bid does not know it won, so that round has zero victims. `fault_elected_not_silenced` counts it.
  - With one machine off, no election can run. The notes' case of one machine off and one failed uses `ctl silence` for the
    second failure (demo scenario 6).
  - One node started with `--faults off` stops all elections in the cluster. It is safe, but new against D77's consequences.
  - The all-numbers rule needs every bid on time, so faults can come less often than configured. A term storm makes this
    worse: a bid from an older term is dropped by the term rule, so the round dies. This fails safe, and
    `fault_rounds_abandoned` shows it.
  - Three limits on "one at a time", to be stated plainly. A `ctl silence` landing inside an open round can overlap an
    elected fault: that is operator-caused, and the demo's `ctl` scenes run with the timers set never to fire. A machine that
    sends its ticket and then loses power before the round closes can give two nodes down, in a window of a few ms on a LAN;
    degraded mode (D120) covers the result. A process restarted inside a round can draw a second number. So say "no second
    silence on this path", never "never two down".
  - D116 was omar.leithym's decision and D78 Aabed's. Noor accepted the reversal with item 7; they should be told.
- **Where.** `node/faults.rs::{FaultRounds, FaultElection, on_bid, on_elected, draw, spawn_schedule, run_schedule}`,
  `proto.rs::{FaultBid, PeerBody::FaultBid, PeerRpc::fault_bid}`, `node/mod.rs::{NodeContext::fault_election,
  on_peer_message}`, `node/transport.rs::invoke`, `config.rs::FaultConfig`.
- **How we check it.** `tests/faults.rs`: `the_schedule_injects_faults_one_victim_at_a_time`,
  `no_two_nodes_name_different_victims`, `no_election_finishes_with_a_node_down`,
  `no_election_finishes_while_a_node_is_silenced`, `the_schedule_resumes_after_two_nodes_were_out`,
  `elected_faults_lose_no_request`. FT-8 under load, and the six-PC power-cut check in the runbook. Broken builds, run by hand,
  are listed in D159.
- **Report notes.** Add to the decisions changed during development. Rewrite decision 13 of the report draft, which says the
  team refused a separate election per fault and calls `FaultOrder` its weakest point. Call this a separate, added election: it
  shares the group send and the idea of a bid with D2, but not its rounds, code or load criterion. Highest number wins is the
  Bully rule, with the number drawn fresh each round, which the spec's "adapted algorithm" allows. State the readings as
  readings: the thing elected is the machine, not a chooser; "the three servers elect" means all three must take part; the
  criterion is a lottery, since the spec names criteria only for load balancing. Limit to state: "ignore communication" is not
  total, because `ctl` frames still pass the gate (D23).

### D159 · How a fault-election round runs: who calls it, what closes it, numbering, timers, who may bid

- **Date:** 2026-11-03 · **Part:** C · **Status:** Implemented, amends D78's timings and D77's consequences ·
  **Decided by:** the agent on its recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** D158 settles what is elected. Everything about how a round actually runs was still open, and each choice is
  one a grader could ask about. This entry records them together because they follow from one rule, and each one lists what was
  rejected.
- **Options considered and the decision, choice by choice.**

  | Question | Chosen | Rejected, and why |
  |----------|--------|-------------------|
  | Who calls a round | Any node whose timer runs out first; the others join at once | Each node on its own timer: no shared clock, so they would not land in one round. A fixed node, or the Raft leader: a coordinator again. |
  | How many bids a node must hold | All nodes in `config.cluster.nodes` | Two of three, or whatever arrived in a window: nodes can split, or leave one node running. The who's-alive list: a dead machine would shrink the round to two. |
  | What wins | The highest random ticket, ties to the lower ID, as Noor's item 7 says | Lowest through `bidding::pick_winner`: the odds are equal, and the case for it comes to three shared lines. A real load score: it would keep silencing the idlest node. Rotation by ID: a rota, not an election. |
  | Ticket range | `0..1_000_000` | Full `u64`: not readable on screen in a live demo. A tie is a few in a million, and has a rule. |
  | Message | New `PeerBody::FaultBid { round, ticket, ms }`, method `fault_bid`, replacing `FaultOrder` | Reuse `PeerBody::Bid` with a reserved request ID: it would raise `just_won` and mix into the bid counts. Keep `FaultOrder` too: it keeps the gate exception. A `Command`: see D158. |
  | Reply | `PeerReply::Ack` | A new reply carrying the receiver's round: nothing reads the content, as with `Bid` (D104). |
  | Whose silence length | The winner's own `ms`, a field in its bid | Derived from the ticket: needs a modulo argument. Drawn by the winner after the election: the others could not log it. The lowest of the three: a node would act on a number it did not draw. |
  | When a round closes | On holding every bid, once this node's own send has finished, with a deadline to give up | A fixed window on every node: then the duration the spec asks for would only report the window. |
  | What code holds a round | A small pure `FaultRounds` in `node/faults.rs`, with time and draws passed in, like `Suspicion` | A second `BidRounds` keyed by a fake request: it closes only at its deadline and brings `just_won`. New code in `bidding.rs`: this leaves Part B's file untouched. |
  | How the driver waits | One wake time, a fresh `sleep_until` each pass, `select!` with `Notify`, woken by `notify_one` from a sync `on_bid` | A pinned `Sleep` reused across passes: an elapsed one stays ready and spins. Polling every Raft heartbeat: 50 ms would swamp the time being measured. A task per bid: more tasks for no gain. |
  | The timer fires while a round is open, or while the node may not bid | Ignored while a round is open; if the node may not bid, `next_call_at = now + a random interval` | Disarm until the next close: it can stop the schedule for good with no log line. Open `left + 1` anyway: it breaks the late rule. |
  | Round numbers | A local counter: highest seen + 1, not saved. The same number merges and a higher one replaces. A round at or below the highest one left, or below the open one, is late and changes nothing, not even the timer | `max(left + 1, unix seconds)`: needs clocks to mean something across machines. Raft term mixed with a counter: ties the path to Raft again. Written to disk: nothing needs it. A lagging node wastes at most one round until a peer's timer lifts it. |
  | Timer values | Keep `interval_min_ms = 30000`, `interval_max_ms = 60000` | Shorter defaults: Noor said 30 to 60 s. The first of three timers calls the round, so the wait averages 37.5 s at 30 to 60 s and 22.5 s at 20 to 30 s, plus the silence. The report states that. |
  | When the wait restarts | After closing: `ms` + a new random interval. After abandoning: a new random interval | A fixed period: D78 already rejected it. A new interval only: rounds would be called during the silence. |
  | Rounds called while a node waits | Join if it may bid, with no skip rule | A skip rule: one rule more, and a round with a node out costs at most 6 calls and one deadline. |
  | Round deadline | Derived: 2 x `peer_rpc_timeout_ms`, 1 s by default | A new key, or a constant: a caller's bid can take one timeout to reach a joiner and the joiner's another to come back, so the timeout already says it. |
  | `interval_min_ms >= silence_max_ms` | Keep the validation rule, with a new reason | Drop it: no longer needed for safety, but it avoids rounds that cannot finish, and it leaves the cells and `rejects_bad_configs` alone. |
  | Who may bid | Serving, gate open, not degraded | Also suspecting no peer: adds no safety. Any node that is up: AGENTS.md says a returning node does not bid. The gate is checked as well as `serving`, because `serving` goes false a moment after the gate closes (`rejoin.rs`). |
  | A winner that is not serving when it closes | Log `fault_refused` and stay up | Silence anyway: `silence_for` looks only at the switch, the duration and the gate, so a short `ctl silence` could stack a fault on an unfinished catch-up, which D78 rejected. |
  | A node with `--faults off` | It does not bid, so no election finishes anywhere | The other two elect between themselves: it breaks the all-numbers rule. |
  | A lost bid, or a `StaleTerm` answer | No retry: the round dies at its deadline and the next timer tries again. Only `Ack` means a peer holds the bid | Resend once: a resend cannot help a term that lags by seconds. Send through Raft: see D158. Same reasoning as D104 and D126. |
  | A winner that misses a bid | Accept it: zero victims that round, and the analysis counts it | A second confirm round: a two-phase problem for a simulated fault. |
  | Order of the winner's own steps | Finish its own send, then close its gate | Close at once: a call fails at once on a closed gate, so its ticket might never leave. |
  | A complete round whose deadline has passed | It closes: the deadline is checked after the all-numbers rule | The other order: the driver's own send can run past the deadline, and a node holding every number would throw them away for no reason. The design did not say which check comes first. |
  | Randomness | Per round and node: `seed ^ round*K1 ^ id*K2`. Waits come from a separate per-node stream | One stream per process: the same round number would then give a different ticket depending on timing. |
  | An operator's way to call a round | None | `ctl elect-fault`: no document asks for it, it adds a contract item and its error states, and the timed demo scene shows the election with nobody typing (D162). |
  | What happens if rounds misbehave on the six PCs | Keep the code; set both interval keys to `100000000` in the config; run `quick` with an explicit cell list that leaves out the `ft8` cells; use `ctl silence` in the demo; report an accepted risk | Revert the commit, or fall back to option 3 of D158: new code at the moment time has run out. |

  **Not done, on purpose:** no Raft, no change to `CloudState`, no agreement guarantee, no saved counter, no retries, no new
  dependency, no new source file, no new config key, no `ctl status` column. D23 stays.
- **Decision.** As the table says. In code: `FaultRounds::{add_bid, step, send_finished, set_next_call}` hold the rules and
  `Step::{Sleep, Rearm, Send, Ended}` is what they tell the driver to do. `FaultRounds::step` records this node's bid inside the
  same lock that decided to make it, so a node can never draw twice for one round, and a higher round landing between the
  decision and the send cannot mix up which round a bid belongs to. The design named this as a separate step. It is one step here
  because that removes the race rather than handling it.
- **Why.** Most of this follows from the all-numbers rule: once a round needs everyone, a silent node blocks it just by saying
  nothing, so there is no need for a skip rule, a suspicion check, a saved counter or a confirm round. Each of those would be one
  more thing to get wrong a day before the deadline.
- **Consequences.**
  - The wait between faults is shorter than the configured range suggests, and longer than it when rounds die. The report states
    both.
  - A node logs one `fault_election` line per round it heard of, even if it was not allowed to bid, so a line with a null victim
    on one node is the soft rule at work. Be ready to explain it at the demo.
  - Six calls per complete round, and fewer once a dead peer has left a node's list (D154).
  - `ctl silence` inside an open round: see D158.
  - Experiment cells that set `seed` repeat the numbers per round, not the timing. A run cannot be replayed, because the seed goes
    to `tracing` and not to the event log. The logs hold every ticket, which is what the replay claim was for.
- **Where.** `node/faults.rs`, `proto.rs::FaultBid`, `config.rs::{FaultConfig, Config::validate}`, `node/group.rs` (the send).
- **How we check it.** Unit tests in `node/faults.rs`: `the_first_bid_per_node_stands`,
  `a_late_bid_changes_nothing_not_even_the_timer`, `a_higher_round_replaces_the_open_one_and_hands_it_back`,
  `a_round_closes_only_with_every_node_and_after_the_own_send`, `the_highest_ticket_wins_and_ties_go_to_the_lower_id`,
  `every_node_can_win_and_the_silence_stays_in_range`, `the_same_seed_round_and_node_give_the_same_numbers`,
  `a_node_draws_once_per_round`, `an_incomplete_round_is_abandoned_at_its_deadline`, `a_complete_round_closes_even_past_its_deadline`,
  `a_timer_that_fires_while_not_eligible_rearms`, `a_timer_is_ignored_while_a_round_is_open`,
  `a_timer_calls_the_round_after_the_last_one_left`, `the_election_time_runs_from_my_bid_to_the_last_bid`.

  In `tests/faults.rs`: `a_fault_bid_from_an_old_term_is_dropped`, `a_node_that_is_not_serving_does_not_bid`,
  `a_silenced_node_calls_no_round_even_while_still_serving`, `a_degraded_node_calls_no_round`,
  `a_winner_that_is_not_serving_refuses`, `the_winner_goes_silent_for_its_own_time`.

  Each rule was broken by hand once to see a test fail. "Two bids are enough" failed `no_election_finishes_with_a_node_down`,
  `no_election_finishes_while_a_node_is_silenced` and `a_node_that_is_not_serving_does_not_bid`. Each of these also failed the named
  test: closing before the own send, a later bid overwriting the first, lowest ticket wins, a late round not ignored, a node not
  serving still bidding, the gate not checked, a degraded node still bidding, a winner that is not serving going silent anyway, the
  winner using another node's silence, and the bid skipping the term rule.
- **Report notes.** The table is the raw material for the "alternatives considered" part of this decision. The driver is a good
  example for the concurrency section: a sync message handler, a single owned task, and a wake-up that cannot be lost.

### D160 · The fault gate has no exception again: `FaultOrder` and the last word are gone

- **Date:** 2026-11-03 · **Part:** C · **Status:** Implemented, undoes D78's amendment of D22 · **Decided by:** the agent on its
  recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** D78 let one reply through a closed gate: the `FaultOrder(Ok)` that a victim sent after obeying, so the leader could
  tell an obeyed order from an unreachable victim. With no order there is nothing to answer. The first design for this gap said to
  remove the whole block in `PeerServer::deliver`. The design for the leader's jobs (gap 10) corrected that: the block holds the
  general check as well, so removing all of it would let replies leave a silenced node, which breaks the fault gate rule.
- **Options considered.**
  1. *Remove only the exception and keep the check (chosen).* Delete the comment and the `last_word` line, turn the condition into
     `if self.gate.is_silenced()`, and keep the two lines after it.
  2. *Remove the whole block, as first written.* Any handler that was running when the gate closed would answer through it.
     Rejected: a silenced node must look like a pulled cable (D22).
  3. *Keep the exception for some other message.* Nothing else needs a last word: a winner closes its own gate after its own send,
     and a peer's unanswered bid just waits out its timeout, which costs nothing.
- **Decision.** Option 1. `PeerBody::FaultOrder`, `PeerReply::FaultOrder`, `PeerRpc::fault_order`, `Event::FaultOrdered` and
  `MsgKind::FaultOrder` are gone. `MsgKind::FaultBid` takes the old one's place, so `MsgKind::ALL` stays 11 long. The serving check
  from `on_order` stays, as `on_elected`, the winner's own check.
- **Why.** "The fault gate drops everything but control" (AGENTS.md) now holds with no exception. The simplest rule is the one a
  reader can check by looking at `deliver`.
- **Consequences.** A winner's closing gate can drop the `Ack` to a peer's bid, so that peer's send waits out `peer_rpc_timeout_ms`,
  and its `fault_election` line comes up to 500 ms late. That is why the third bid's time is stamped as the bid is recorded, not when
  the line is logged (D161). `analysis/analyze.py` needs no change for the message rename: `msgs_fault_order_per_req` becomes
  `msgs_fault_bid_per_req` by itself.
- **Where.** `node/transport.rs::PeerServer::deliver`, `proto.rs`, `metrics.rs`, `raft/network.rs::reply_name`.
- **How we check it.** `node::transport::tests::no_reply_leaves_a_node_whose_gate_closes_mid_call`: the handler closes the gate before
  it returns, and the caller must see a timeout. It fails if the check is removed (seen). `an_obeyed_order_is_acknowledged` is deleted
  with the order. Its stale-term sibling is ported to `FaultBid`, on the same path, as `a_fault_bid_from_an_old_term_is_dropped`. Gap 10's
  design wanted to port it to `Heartbeat`; `FaultBid` is the new message on the same line of code, so the cross-gap review chose that.
- **Report notes.** D78's "last word" bug was offered as a report example. It now has an ending: the exception it needed is gone.

### D161 · `fault_election` event and what is measured: tickets, election time, abandoned rounds, split check

- **Date:** 2026-11-03 · **Part:** C, with analysis for all · **Status:** Implemented · **Decided by:** the agent on its
  recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** The spec asks for election frequency, duration and message overhead. Nothing measured the fault path except
  `fault_ordered`, which no analysis read. The all-numbers rule also makes claims that only logs can check: nobody disagrees, and a
  named node really does go down.
- **Options considered.**
  1. *One line per round on each node (chosen):* `fault_election { round, tickets, victim, ms, called, election_us }`. One grep shows
     the numbers, the victim and the caller, in the shape of `bid_round_closed`.
  2. *Three separate events* (bid sent, bid received, round closed): more lines and more joins for the same facts.
  3. *Also keep `fault_ordered`:* nothing would ever log it.

  For duration: *own bid to last bid recorded, on this node's clock (chosen)*; or from the caller's bid across machines, which AGENTS.md
  forbids; or after the send returns, which can add 500 ms when the winner's gate drops an answer.

  For outputs: *the list below (chosen)*; also a victim share per node, which from a few rounds per run proves nothing; or counts
  only, which misses duration.
- **Decision.**
  - `Event::FaultElection`. `victim`, `ms` and `election_us` are null for a round this node did not finish. `tickets` holds the numbers it
    had. `called` is true if this node's timer started the round. `fault_refused { victim, reason }` stays, and now means an elected
    node stayed up. `fault_ordered` is deleted. `silenced`, `restored` and `rejoined` do not change, so E3 and E4 are unaffected.
  - `election_us` is this node's own bid to the last bid it needed, on this node's `Instant`. The time is stamped in `on_bid`. A joiner whose
    own bid came last reads 0, so a joiner's figure is shorter by construction. The analysis averages only lines with `called` true.
  - `analysis/analyze.py::fault_election_metrics`, grouped by round per run, in the measured time:
    `fault_elections` (rounds where any line names a victim), `fault_rounds_abandoned` (no line does),
    `fault_election_mean_ms` (called lines, nulls filtered), `fault_split_decisions` (rounds naming two victims, must be 0, mirroring
    `bid_split_decisions`), `fault_elected_not_silenced` (the named node's own line is null), `fault_refused`, and
    `fault_bids_per_round` (per request it would be about 0.0007 and say nothing). `fault_refused` was not in the design; it is one line,
    and it answers the same question as `fault_elected_not_silenced` for the other way a named node can stay up.
  - `fault_phases.png` leaves out cells with faults on a timer, because their "during" runs from the first silence to the last restore.
    The design named `ft8`. `e5-faults-on` has the same problem, so it is left out too.
- **Why.** These are the spec's three numbers plus the two checks on the rule. `mean_or_nan` does no null filtering, so without the filter
  the first real log would crash the analysis or skew it.
- **Consequences.** Field names are stable from here: a script reads them. A lagging node can reuse the number of a round the others gave up on,
  which merges two abandoned rounds into one in the count; it cannot reuse a closed round's number, because closing needs its bid. The new
  analysis fields first meet real logs in the FT-8 cell, which is why it runs first (D162). Analysis is not in CI, so run
  `python analysis/test_analyze.py` by hand.
- **Where.** `metrics.rs::{Event::FaultElection, Event::FaultRefused, MsgKind::FaultBid}`, `node/faults.rs::log_round`,
  `analysis/analyze.py::{fault_election_metrics, sent_in_window, plot_phases}`, `analysis/test_analyze.py`, `analysis/README.md`.
- **How we check it.** `analysis/test_analyze.py::test_fault_elections` on a synthetic log: counts, the mean from called lines, null lines skipped, a split
  detected, an elected node that did not go silent, bids per round. `test_a_run_with_no_fault_election_has_counts_of_zero_and_no_mean`.
  `tests/faults.rs::no_two_nodes_name_different_victims` reads real JSONL from a real cluster, and needs one round closed by all three.
  `the_election_time_runs_from_my_bid_to_the_last_bid` covers the stamp.
- **Report notes.** Report `fault_split_decisions` beside `fault_elections`, so a reader sees that 0 is out of a real count. Say the mean is from the caller's
  line on its own clock. Do not claim a victim distribution: the unit test shows every node can win, and that is all a few rounds can show.

### D162 · Elected faults in the experiments and the demo: FT-8 first in quick, `ft8-election-rate`, a timed last scene

- **Date:** 2026-11-03 · **Part:** C, F · **Status:** Implemented for the cells and the demo text. The choice left open about the example config is settled by D165: it ships never-firing values · **Decided by:** the agent on
  its recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** Neither the demo nor the quick profile ever ran the schedule, so the report had nothing on it. A quick run of `ft8-random-faults`
  holds about 2 elections, which is too few to say how long an election takes. The cell list's order also meant a bug here would cost the whole hour
  before anyone saw it.
- **Options considered.**
  1. *Which measured cells:* `ft8-random-faults` first in `quick.list`, plus a new `ft8-election-rate`: 15 cells, about 67.5 minutes (chosen). Only
     `ft8-random-faults`: 14 cells, and about 2 rounds per run. Append FT-8 last: a bug costs the hour. Neither: no data.
  2. *FT-8's seed:* drop `seed = 2` and say plainly a run cannot be replayed (chosen). Keep it: a fixed seed repeats the same tickets per round in all three
     runs. Have the runner write a per-run seed: one more thing to test the day before the deadline.
  3. *E3 and E4 cells, and `ctl silence`:* keep them as they are (chosen). Switching them to elections cannot work, because those cells need a chosen role at a
     chosen moment, which an election cannot promise. The forced-failure row and demo scenario 6 use `ctl silence`.
  4. *Demo:* a timed last scene with nobody typing: schedule on, interval 20000 to 30000, both silence keys at 20000, then `grep fault_election` on each node
     machine (chosen). A typed `elect-fault` step: the spec says periodically, so it should be seen happening by itself. Only FT-8's logs: nothing to watch.
  5. *Silence values:* random 1 to 20 s in the defaults, fixed 20 s only in the timed scene (chosen). This is a reading: the spec says random up to 20 s, the Oct 28 note
     says 20 s, and validation allows the fixed value. Fixed 20 s everywhere: not random. Random in the demo: a 1 s silence shows nothing.
- **Decision.** `experiments/quick.list` starts `ft8-random-faults`, `ft8-election-rate`. `ft8-election-rate` is new: 300 users, think time 1000 ms, interval 1000 to 1500 ms,
  silence 100 to 1000 ms, valid under the config rules, for an estimated 40 rounds per run. Its short silences are for measuring the election, and are not spec-sized
  faults. `ft8-random-faults` drops its seed. `e5-faults-on` keeps its seed, and its comment now says a seed repeats tickets per round number, not the timing. All the other
  cells keep the never-firing interval, and send no bid. `e5-faults-off` now means no node bids. Quick becomes 15 cells and full becomes 32.
- **Why.** First in the list means a bug costs about 4.5 minutes, not an hour. The rate cell gives tens of rounds for the spec's numbers. The other cells stay as they are, so they
  pay nothing: with the timer never firing, this code sends nothing.
- **Consequences.**
  - The run time rises by about 9 minutes in quick, to about 70 minutes.
  - The lab config does not reach the cells, because `write_config` builds from the cell file. To stop elections in a measured run, pass an explicit cell list that leaves out
    the `ft8` cells. To stop them in the demo or by hand, set both interval keys to `100000000` in `config/lab.toml`.
  - The gap for user processes adds its own cells (quick stays 13 there, full 35), so together they are quick 15 and full 36. Whichever merges second fixes
    `tests/experiment_cells.rs`.
  - A load above the sync ceiling starts a term storm, in which bids are dropped by the term rule and FT-8 holds few elections. It fails safe, but the check that
    `e1-300-bidding` shows elections near 0 with nothing else on the machines has not been run; see the build report. If the storm is real on the lab's disks it blocks every cell, and it comes first.
- **Where.** `experiments/quick.list`, `experiments/ft8-random-faults.toml`, `experiments/ft8-election-rate.toml`, `experiments/e5-faults-on.toml`,
  `scripts/run-experiments.sh` (comment), `tests/experiment_cells.rs`.
- **How we check it.** `tests/experiment_cells.rs`: 32 valid cells, 15 quick cells, and `ft8-random-faults` first. The runner and analysis were tried on the two cells on one machine with real
  processes (scripts only, never results). On the six PCs: run the two cells first; rehearse the timed scene; power one node machine off with the schedule on and check the other two log only
  null-victim rounds and no second `silenced`; try the 1 s round deadline on the real network.
- **Report notes.** State the profile with each figure (D63). Say that `ft8-election-rate` measures the election, not the failure model. About 2 elections per run in `ft8-random-faults`, so no distribution
  claim comes from it.

### Evidence so far for D158 to D162: one machine, rehearsal only

Never report these as results from the six PCs. All used a release build in a private network namespace on one machine shared with other jobs (load
average 8 to 34), loopback, the run line's load, and storage on. Each is one run. Warm-up 5 s, measured 70 s.

| What was run | What it showed |
|--------------|----------------|
| `ft8-election-rate`'s timings at 30 users | Over the whole run: 43 rounds, 42 closed by all three, 1 with a null victim, 42 silences with 0 overlapping, wins 13, 14 and 15. In the measured time: `fault_election_mean_ms` 0.5, `fault_bids_per_round` 5.95, `fault_split_decisions` 0, `gave_up` 0, about 23 req/s. |
| The demo's timed scene at 30 users: interval 20 to 30 s, silence 20 s | 2 rounds, both closed by all three, with nobody typing. `gave_up` 0, 0 new Raft terms in the measured time. |
| Power cut with elections on, 30 users, think time 300 ms: node 3 SIGSTOP plus dropped packets on its ports for 16 s | Before: 8 rounds closed. During: nodes 1 and 2 logged 8 rounds with a null victim, and neither went silent. Their member lists went `1,2,3` to `1,2` and back. After: 7 rounds closed again. `gave up 0`. |
| `ft8-random-faults` and `ft8-election-rate` as they are, 300 users | The term storm known from rehearsal: 263 and 150 new Raft terms, 0.03 and 1.8 req/s, 898 and 769 gave up. Rounds mostly died: 2 of 4 and 3 of 47 closed. That is the fail-safe at work: 0 split decisions and no overlap. One round named a node that never knew it had won, and `fault_elected_not_silenced` counted it. |
| `e1-300-bidding`, the same setup, timers never firing, so this code sends nothing | The same storm: 237 new terms, 0.1 req/s, 839 gave up. **So the storm is not this change's doing.** It is the disk-save problem of Noor's item 10, which another lane is on. |
| `e1-30-bidding`, timers never firing, just as other jobs took the machine's load average to 34 | 82 new terms, 3 req/s, where 30 users earlier held a steady leader. Numbers on this machine depend on what else is running, which is one more reason none of these is a result. |
| Every runner log above read line by line, 6 runs | 0 lines breaking the rule (same tickets, same victim, same `ms`, the victim holding the highest ticket), and every `silenced` explained by that node's own line naming itself for that length. |

The runner and `analysis/analyze.py` ran on all of these with no change. `fault_phases.png` was correctly not drawn with only `ft8` cells. One script check only:
the first real use on the six PCs is still to come.


### D163 · What "no coordinator" means here: one elected node orders the log, and no node decides for another

- **Date:** 2026-11-03 · **Part:** A, with B and C · **Status:** Implemented, amends D1 and D114 (wording), D69 (the token), and
  gives D116's closing argument its final form (D116 itself is already superseded by D158) · **Decided by:** Noor Hany Emam
  (item 1 of the 2026-11-03 sitting: keep Raft, shrink the leader). The written reading and the words for the leader's job
  were decided by the agent on its recommendation, on Noor's instruction to finish without more questions (2026-11-03). Noor can
  overrule either.
- **Context.** The spec asks the three cloud nodes to cooperate in decisions, load, failure handling and replication. The
  class notes of Sept 13 and Oct 28 go further: the leader must not act as a coordinator between the nodes, and the cloud nodes
  must be peers. RescueMesh uses Raft, and Raft has a leader, so a reader with those notes open will ask this first.

  Until tonight the leader did three things that a reader could fairly call coordinating: it dispatched requests (gone since
  D114), it picked the node that fails and ordered it silent (gone since D158), and it loaded the responder list (gone with D164).
  D116 kept Raft and that schedule, and argued the point in words. Words alone do not answer the note, because at that time one
  node really did order another silent.

  Two more facts forced an exact statement. First, Noor's item 1 says the leader "only orders results". The code shows that is a
  little too narrow: it orders every command, including releases and the campus list, and it also sends the copies, and answers
  one read for a returning node (`read_index_local`). Second, no document says what "coordinator" means, so the report has to
  state its reading and let a reader judge it.
- **Options considered.**
  1. *Keep Raft. The leader orders the log and decides nothing else, and show it three ways that a grader can check without
     trusting our words: tests that fail if the leader gets a say, one number beside the coordinator baseline, and a live moment
     where the leader goes silent and comes back as an ordinary node (chosen).* It delivers Noor's item 1 and leaves the commit path
     alone. What it cannot remove: every commit still passes through one elected node.
  2. *Replace Raft with a leaderless protocol.* The only option that removes the leader itself. A full rewrite of the most tested
     part of the system a day before the deadline, with no way to reach six PCs in time. Noor's item 1 rejected it by name, as
     did D116's option 3.
  3. *Time-limited turns: the leader role rotates on a timer.* Rejected as fatal. The notes object to what the leader does, and
     this leaves that unchanged. Each turn is a forced election, because openraft 0.9.25 has no leader-transfer call and
     `trigger().elect()` ignores `elect(false)`. The leader steps down on a higher vote whether or not the candidate can win, and
     with one peer down the old leader can win back, so the role may not even move. Each term change also splits bid rounds, inflates
     the election count the spec asks us to report, and does on purpose what D5 and D75 work to prevent.
  4. *Every node starts with the responder list, so no one has to load it.* Rejected as fatal; the reason is in D164.
  5. *Rename the role: "sequencer" in the docs, or in the code and `ctl`.* A new word over the same behaviour looks like relabelling
     to a grader, and `scripts/run-experiments.sh` matches the role string. Rejected.
  6. *Any node loads the list, and the fault schedule stays with the leader but switched off.* The named fallback if the fault draw
     had not landed. It does not meet the notes, since the order message would still exist. Not needed: the draw is on this
     branch (D158), so this was not built.
- **Decision.**
  - **The reading, written as a reading:** no node decides for another, and one elected, replaceable node orders the log. The notes
    speak of what the leader does, and the spec allows an election for a temporary coordination role. So the test is whether one node
    decides for another, not whether any node is ever elected. Our own log already used this test when it rejected a leader sweep
    in D39.
  - **The leader's job is three mechanical duties and no decisions:** it numbers log entries, it sends the copies, and it answers one
    read. Keep the word `leader` in code, events and `ctl`. Drop "coordinator" wherever it names this role. The role's holder is
    whichever node a majority elected, and it changes when that node fails.
  - **One table says who decides what.** It goes in `docs/architecture.md` section 2, the report's section on how the leader is
    chosen, and the demo's answer. This is its text:

    | Question | Who decides | What the leader adds | Library or ours | Decisions | Where |
    |----------|-------------|----------------------|-----------------|-----------|-------|
    | Who plans a report | All serving nodes: the lowest bid | Nothing. A term change can split a round, and the first commit settles it | Ours | D2, D114 | `node/bids.rs::run_round` |
    | Who takes over from a silent winner | Any node that lost the round | Nothing | Ours | D42, D110 | `node/takeover.rs` |
    | Is a peer alive | Each node for itself | Nothing | Ours | D41, D79, D153 | `node/heartbeat.rs`, `node/group.rs` |
    | Does a plan stand | All three state machines: the same rule on the same log | Nothing in the rule. Which of two racing plans reaches it first commits first | Ours | D27, D28 | `state.rs::commit_result` |
    | When a responder is freed | The executor, then any node | Orders the entry | Ours | D39, D111 | `node/release.rs` |
    | Who fails next | All three, by the draw | Nothing. It can be drawn like anyone | Ours | D158, D159 | `node/faults.rs` |
    | Loading the responders | Any serving node. No selection, because it is idempotent | Orders the entry | Ours | D164 | `node/mod.rs::spawn_campus_seeding` |
    | Answering alone | The lone node | Nothing | Ours | D120, D121 | `node/degraded.rs` |
    | A retry of a decided request | Any node, from its own copy | Nothing | Ours | D27 | `node/mod.rs::handle_request` |
    | Is a returning node current | The node's own gate | Answers one read | openraft and ours | D75, D76, D135 | `node/rejoin.rs` |
    | Who orders the log and sends the copies | Whichever node a majority elected. Every node stores, and 2 of 3 commit | These are its duties | openraft | D1, D14, D115 | `raft/` |

  - **What the leader cannot do, listed under the table:** pick a planner; change a plan (`write_local` hands the command to
    `client_write` unread); commit without 2 of 3; silence anyone.
  - **Limits, stated openly under the table:** a cut-off leader can still read `leader` in an old term until it hears otherwise, and
    the others replace it; arrival order at the leader settles a race between two plans; plans come from a node's local state and the
    leader applies first, so a follower's plans may go stale more often (not measured).
  - **No new message, field, config key, timer, `Command` variant, event or dependency** for this entry.
  - **Not built, on purpose:** a test that fails if application code reads `.leader` (`application_code_does_not_read_the_leader`), which is a
    stretch for after the six-PC data, because it guards only later work, and a grep line in the PR does the job until then; a layer diagram beyond
    section 4.3 of the architecture; rotation of the node list per run (D166); switching `TestCluster`'s default policy (it proves nothing about who
    decides and adds triage risk before data).
- **Why.** A grader with the notes open cannot read Rust during a demo. They can check whether one node tells another what to do,
  whether the remaining role moves when its holder fails, and whether the claim has a number beside it. This option gives those
  three. Renaming alone would not count, and neither would an argument, which is why this comes after the jobs have actually moved
  (D158, D164) and not before them.

  Option 2 is the only one that removes the objection in its strictest form, and it cannot be done in the time. That is exactly the
  reason to say the limit out loud rather than leave it for a reader to find.
- **Consequences.**
  - What remains, and the report says so: a strict reader can still call this node a coordinator. Every commit passes through it,
    it drives replication, and arrival order at it settles races. The report states this with its measured cost.
  - It reverses D116's reasoning and D12's reason, "one leader means exactly one victim at a time with no extra election". D158 already
    records that. D116 was omar.leithym's decision and should be heard by him before the PR, as D158 says.
  - The claim sits on a stable leader, which this entry does not provide: D133 does, and the runs must show `elections` near 0. A silence
    at a load where the cluster is already churning tests nothing.
  - The table's rows are claims about code. A PR that gives the leader a say in any row must change the table and this entry, and the
    role tests should fail.
  - Bidding may lose to leader dispatch at load, because bidding adds log entries and a proposer that is also the leader has no
    forward. D114 already says that if E1 shows bidding much worse the team must revisit it and say so. The class notes keep bidding
    as the default, and the number is the price. D166 keeps the baseline in E1 for this reason.
- **Where.** `src/raft/mod.rs` (module comment), `tests/peers.rs`. Docs: the table goes in `docs/architecture.md` section 2.
- **How we check it.** `tests/peers.rs`, none of which assumes which node leads:
  - `a_busy_leader_loses_and_an_idle_leader_wins`: with 200 jobs held on the leader, the first round ranks it behind an idle node on
    score alone, and it plans nothing. With 200 jobs held on both followers, the leader plans.
  - `the_leader_role_moves_and_its_old_holder_is_ordinary`: silence the leader, a new leader in a newer term and a client still served,
    restore, the old holder serves as a follower, and with the others held, it plans.

  The design started from holding twice the worker threads. That is not enough with real planning times: a held node that has never planned scores (8 + 1) x 1000 / 4 = 2250, and an
  idle node with one real 9 ms plan scores the same, which a debug build on a busy machine can reach. So they hold 200 jobs, and the sums are in a comment. A plan whose node "wins" while
  held waits behind the held jobs anyway, so the assertions on who planned hold for any timing.

  Each was broken on purpose once. Run under leader dispatch, both fail (the plan waits behind the held jobs, and the client gives up).
  A leader that always bids 0 fails the first, at its ranking check. A leader that never bids fails the first. A node left out of bids
  after its silence fails the second. A grep line for the PR, until the stretch test exists: `grep -rnE '\.leader\b' src/node` names only the status copy, the baseline's `leader_dispatch`, and
  rejoin asking itself, plus `testing.rs`'s stub once the lanes merge. On the six PCs: start the nodes, and `ctl status` shows equal digests. Silence the leader, then
  power it off, under the chosen load: `gave up` is 0, the role moves, and the returned node follows and wins bids.
- **Report notes.** This is the report's answer to "how is the leader chosen, and is it a coordinator?". Print the table. State the
  reading as a reading, in those words. List option 2 and option 3 as alternatives considered, with their reasons, under the decisions
  changed during development. State the limit and its cost in the same paragraph as the claim. Do not write "well under 1", "1/3" or
  "clients do not notice" before there is six-PC data (D165, D166). Each person's short sheet: A must be able to walk through the table
  row by row; B and C must be able to say why their row has "nothing" in it.

### D164 · Any serving node registers the campus responders

- **Date:** 2026-11-03 · **Part:** A, with E · **Status:** Implemented, amends D38 · **Decided by:** Noor Hany Emam (item 1: loading the
  responder list moves to all nodes). The sub-decisions in the table were decided by the agent on its recommendation, on Noor's
  instruction to finish without more questions (2026-11-03).
- **Context.** D38 had a background task on every node poll every 200 ms, and propose `RegisterResponders` when the node was the leader
  and its state had no responders. It was the last job in the code that looked at who leads for a reason other than the baseline or the
  rejoin read. It needs no chosen node at all, because the command is idempotent (D29): `or_insert_with` keeps a responder that is
  already there, so applying the list twice equals applying it once. The spec wants an election where a responsible node must be
  selected. Here none must be.
- **Options considered.**
  1. *Any serving node proposes it through the log (chosen).* About five lines on the commit path everything else uses. A config that
     differs on one machine stays harmless, because the log decides the list and not each node's file. A few duplicate entries reach
     the log.
  2. *Every node starts with the list in its state, and nothing goes through the log.* Rejected as fatal. Today a differing `[campus]`
     on one machine changes nothing. With a local start list, one node would answer `UnknownResponder` and `Stale` while the others
     commit, so outcomes and the hash chain would differ, and first commit wins would fail across nodes. The config is copied by hand
     (`docs/runbook.md`), and hashing the list would only display the split. It also touches the deterministic core a day before the lab
     for no measured gain, and conflicts with D38's reason, that responders are replicated state.
  3. *Leader only, as in D38.* Leaves a job on the leader that has no reason to be there, against Noor's item 1.
- **Decision.** Option 1, in `node/mod.rs::spawn_campus_seeding`. The leader check is gone. Each tick now does this: if the state holds any
  responder, stop; if this node is serving and not degraded, propose the list. The choices inside it:

  | Question | Chosen | Rejected, and why |
  |----------|--------|-------------------|
  | Guard | Serving and not degraded, the check `release.rs` uses | Serving only: a lone node would fail a proposal every 3 s for as long as its peers are down. Also require a known leader: adds a leader read to the job we are taking away from the leader, and makes it a coin toss whether two nodes propose. No guard: a node back from disk can read an empty state, and it would propose while not serving |
  | Duplicate entries | Accept a few, and tests assert a lower bound only | Stagger by place in the config: gives one node a special first go. A check so only one lands: more machinery than the harm. Up to 3 is not a real bound either, because a forwarded attempt that times out at 500 ms is sent again |
  | Timer | A named constant, `CAMPUS_CHECK_INTERVAL`, 200 ms as before | A config key: nothing tunes it |
  | A failed proposal | Keep `propose_with_retry` and the loop. Log at `debug`, and the `propose_failed` event stays | Give up after N tries: then a node started early would never load it. `warn`: starting before the peers is normal in the lab |
  | New messages, fields, keys, `Command` variants, events, dependencies | None | Everything needed already exists |
  | `tests/replication.rs::first_commit_wins_...` | Start it with `campus.responders = 0`. No assertion changes | Count free responders from the campus: weakens the point of the check. Leave it: it fails some of the time once the change is in |
  | How to test "any node" | Two tests: three nodes with one file, and only one node holding a list | Only the first: it has a tiny race, and it cannot show that the log decides |
  | Shape of `any_node_seeds_the_campus` | Poll flush and file until two nodes' logs hold a `register_responders` commit, poll equal digests, then shut down | Wait for responders, shut down, read logs: a `commit` line is written when the proposal returns, and shutdown aborts it |

  **Two additions to the design, both from checking it against the code.** The design said a restarted node "does not propose while its
  state reads empty", and named no test for it. It is true, but the test first written here, the obvious one, proved nothing. A restarted node gets
  its state back at once from its saved commit index, so it reads a full campus and never reaches the check, and that test passed with the
  guard taken out (seen). The state reads empty only when a power cut lost that index, which is written but not synced. So
  `a_restarted_node_proposes_no_campus_before_it_serves` makes the same situation another way: a node back alone with no campus in its log,
  and a campus in its file. The design also held no test for the degraded half, so `a_lone_node_waits_for_a_peer_to_load_the_campus` covers
  it, and shows the path a node takes when it starts well before its peers, as the demo's start order has it do.
- **Why.** Idempotence means no election is needed, and a mechanism that needs no selection is simpler than any selection. Using the same
  commit path means no special bootstrap path, which was D38's reason and still holds. The guard matches `release.rs`, so a reader learns
  one rule for "a node does background work through the log".
- **Consequences.**
  - Every run gains a few duplicate `RegisterResponders` entries. They change no state, but the digest is a hash chain over the log, so
    digests differ from any earlier run. All nodes still agree. Log indexes shift by the same few. Nothing compares digests across runs.
  - A node started first proposes until it goes degraded, about 2 s by default, and logs a `propose_failed` line per 3 s attempt
    meanwhile. These fall in start-up, before any measured time.
  - The entry is forwarded as well as replicated. With 300 responders it is a few KB, well inside the frame limit, and it happens at start.
  - A test that registers its own responders and then counts them must start with no campus, or it can see the campus land after them. Only one existing test
    counted on the old behaviour: `first_commit_wins_...`. With the change in and its fix taken out, it failed on the sixth of six tries (seen once). The
    design's arithmetic said about one run in three.
  - The old leader-only code stopped once any list landed. So does this. A list a test registers first still stops seeding, as before.
- **Where.** `src/node/mod.rs::{spawn_campus_seeding, CAMPUS_CHECK_INTERVAL}`, `tests/peers.rs`, `tests/durable.rs`, `tests/replication.rs`,
  `tests/common/mod.rs::{TestCluster::start_each, hold_jobs}`, `tests/bidding.rs` (the helper moved out; no test body changed).
- **How we check it.** `tests/peers.rs::any_node_seeds_the_campus` (two nodes' logs hold a commit, then equal digests and every responder
  free) and `the_log_decides_the_campus_whichever_node_proposes_it` (only node 3's file has a campus, and every node ends up with it).
  `tests/durable.rs::a_restarted_node_proposes_no_campus_before_it_serves` and `a_lone_node_waits_for_a_peer_to_load_the_campus`. Each was
  broken on purpose once: the old leader-only code fails both tests in `peers.rs`, no serving check fails the first in `durable.rs`, no
  degraded check fails the second. Rehearsal: see D165.
- **Report notes.** A small, clear example of a choice not to elect: the spec wants an election where a responsible node must be selected,
  and here none must be. Say why a start list on every node was rejected, because it looks simpler and would break first commit wins.

### D165 · Demo settings: E1's planner load and responders, a load chosen by rule on the PCs, node 3 listed and started first, and fault timers shipped off

- **Date:** 2026-11-03 · **Part:** F, E, A · **Status:** Implemented in `config/lab.example.toml`, `docs/demo.md` and `docs/runbook.md`; amends
  D129's last sentence, restates D205's values, and settles the choice D162 left open · **Decided by:** the agent on its
  recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** The demo must show different peers elected as load changes, and a leader that holds no special place. At D129's defaults it shows
  neither. A plan takes under a microsecond, so every idle score ties, ties go to the lower node ID, and node 1 plans most things: in rehearsal
  node 1 made 51% to 74% of plans, with 78% to 83% of rounds tied. Thirty responders leave about half the load with nobody free. Node 1 is
  also usually the leader, because the election ranges favour the first node listed (D115), so the picture says "node 1 is the boss", exactly what the
  notes object to.

  D205 reached the same two values from the frontend's side. This entry adds the rest, and the file carries D205's text, so the two merge without a
  conflict. Separately, after D158 every node has a fault timer, and the example config shipped it firing every 30 to 60 s, so one
  missed hand edit means a surprise silence in the middle of a scene.
- **Options considered and the decision, choice by choice.**

  | Question | Chosen | Rejected, and why |
  |----------|--------|-------------------|
  | Planner load and responders | `plan_work = 1000`, `responders = 300`, the cells' values | D129's defaults: the node-1 picture above. A different tie-break in code: touches the mechanism meant to be equal, and ties fall to 2% to 4% at `plan_work` 1000 anyway (rehearsal) |
  | Demo load | A rule: the highest users per client PC that holds one term for 60 s from load start on the real PCs, starting at 20. Scene 1's raise stays inside it | 30 users per PC plus the 1500 req/s raise of the old script: before the queue, rehearsal thrashed at 30 for 33 s, and 150 users collapsed. A rule survives disks nobody has measured |
  | Who leads first | List 3, 2, 1, and start node 3 first, a second before the others | Leave node 1 first: leader and tie-break winner are the same node on screen. List order alone: `initialize` elects at once, so start order matters more. Only tends to work, so the first thing to cut |
  | Order of `experiments/hosts` | Stays 1, 2, 3. Only the lab config lists 3, 2, 1 | Reordering it moves the skew target and the election ranges of every cell |
  | Fault timers in the shipped file | Never fire: both interval keys at `100000000`, with the 30 to 60 s values and the timed scene's four values in comments | Keep 30 to 60 s and rely on a hand edit: D162 allowed it, and one missed step on one of six machines silences a node live. Change the code's default: Noor's 30 to 60 s stay the default, and every test and cell is written against it |
  | Undoing a skew | `--work 1000`, because skew replaces `plan_work`. `ctl`'s help says so | `--work 0`: makes that node the fastest, the node-1 picture again |
  | Scene scripts | `<leader>` and `<follower>` in place of node 1 | Keep node 1: the leader may be anywhere |
  | The node silenced and powered off | The leader, as the required version | Any node, leader optional: it is the only visible proof that the role is temporary |
  | What the demo promises | No client names a node, no request lost, a stall of about a second, and the measured figure once E3 has one | "Clients do not notice": by the library's rules failover takes about 0.4 to 1.1 s, and p99 will show it |
  | Keeping the baseline out of the demo | A docs rule and a one-line check: `grep assignment config/lab.toml` | A code guard: the default is bidding, and the file sets it |
  | Tie-break and leader CPU in the bid | Leave both, and name the limit | Hash the tie-break, or penalise the leader: a role-specific penalty makes the leader special in the mechanism meant to be equal |
  | Raft timers | Leave D115's ranges | Retune for faster failover: D115 fixed a real split-vote failure |
  | Stable leader | State it as a precondition: check the disk on each node PC first | Assume the PCs are fine: all of the proof sits on it. D133 is the fix, and the runbook's check says whether it holds there |

- **Why.** The demo's claim is about behaviour, so the settings must let the behaviour show. Each value is the cells' own, so the demo and the data run the same
  load per plan. The rule for the load, not a number, because every number tried so far came from one busy machine. Shipping the timers off makes the
  safe state the default state: forgetting does nothing, and turning elections on takes a deliberate edit. The timed scene, and anyone who wants the 30 to 60 s
  schedule, finds the exact lines in the same file.
- **Consequences.**
  - `ctl silence` still works, because `enabled` stays true. With the shipped file, no fault election runs unless someone edits it. **The runbook's
    instruction is reversed:** it no longer asks you to set never-firing values, and instead says how to turn elections on.
  - The list order changes each node's election range: node 3 is fastest and node 1 slowest. This applies to the demo only. The runner never reads the lab
    config, so cells cannot pick it up (`scripts/run-experiments.sh::write_config`).
  - The honest answer to "why node 3?" is "we listed and started it first". It is optics, and the role moves as soon as anything happens.
  - 300 responders means more release entries per request, so the load the log can take may be lower. The rule covers this.
  - The first requests after a term change can still reach a stale row: a silenced leader's own row can read `leader` in the old term. Say so out loud.
  - The disk check in the runbook is new work for the first contact with the PCs: about a minute per node PC.
- **Where.** `config/lab.example.toml`, `src/bin/ctl.rs` (help text). Docs: `docs/demo.md`, `docs/runbook.md`. `config::tests::shipped_configs_are_valid` still
  covers the file, and the never-firing values pass validation.
- **How we check it.** The first rehearsal on the PCs: start order, `ctl status` before any load, the load rule, and scene 3 with the leader as the victim. On one machine
  so far, at 20 users per client: one term from load start, the leader silenced and replaced within a second, no request lost; see "Evidence so far" below, which is rehearsal only.
- **Report notes.** The demo settings are not data, and the runner never reads them, so no figure depends on this entry. Do not present list order as a mechanism.

### D166 · Measuring the role: `leader_plan_share` in one-leader runs, and commit latency by path

- **Date:** 2026-11-03 · **Part:** A, B, with analysis for all · **Status:** Implemented, delivers D26's split · **Decided by:** the agent on its recommendation, on
  Noor's instruction to finish without more questions (2026-11-03)
- **Context.** D163 says holding the role gives a node no say in who plans. A claim like that needs a number beside the coordinator baseline. The spec ties measurements
  to decisions, and wants evidence on load distribution. Also, D26 and `docs/architecture.md` promise commit latency split into local and forwarded, and
  `analysis/analyze.py` never read the `forwarded` field.
- **Options considered.**
  1. *`share_node_<leader>`, only when one node led all of the measured time, else NaN (chosen).* `client_done` already names the winner's executor, so it needs no attribution of
     plans to a role over time and no correction for duplicates.
  2. *Commit events attributed to whoever held the role at that time.* Needs clocks across machines, and needs correcting for `Duplicate`. Under churn it read 0.96 where the first
     read 0.69 (rehearsal), so it would be a number nobody could defend.
  3. *Commit events plus a rule that two shares must agree.* More machinery than option 1, for the same answer on the runs where it means anything.
  4. *Rotate the node list per run, so no node is always the leader.* It blurs `share_node_<n>` across runs, breaks the main output of E2 and E6 (the first node is the skewed
     one), and adds a moving part to a runner not yet run on the real PCs.

  Which runs to headline: *runs with a NaN-free share, at whichever users level holds one leader, beside the baseline (chosen)*; fixed at 300 and 3000 users (300 collapsed in
  rehearsal); pooling all cells (hides the load dependence).

  The baseline: *keep leader dispatch in E1, labelled as the coordinator design measured for comparison (chosen)*; drop it (then nothing shows what peers cost); replace it with a static
  hash (answers a different question).

  Not built: per-node CPU and network in one-leader runs, and a plot. Stretch, only once six-PC data exists.
- **Decision.** In `analysis/analyze.py`:
  - `leader_plan_share` is `share_node_<leader>`, when each node's last `raft_state` before measuring plus all of those inside it name one leader. Any other leader, a `null` leader, or no
    `raft_state` at all gives NaN. A cell then averages over the runs with a value, and `summary.csv`'s `runs` column shows how many. Read it beside `elections`.
  - `commit_metrics` adds `commit_local_*` and `commit_forwarded_*`, from `commit_result` commits only, split on the event's `forwarded` field. Each path with no commits has count 0 and NaN.
  - No new event. Python only, so it can land after the runs, because it reads raw logs.
- **Why.** Under the baseline this number is 1 by construction, so any value well below it says the role carries no extra work, and a value near a third says the role carries no bias at
  all. It needs no trust in a clock, and it is NaN exactly where it would mean nothing.
- **Consequences.**
  - It needs a stable leader: it is NaN whenever the leader changes in the window, so every fault cell reads NaN by design. That includes a silenced leader's own row, which keeps naming itself.
  - It is one machine's view for each row, and the edges of the measured time are only as good as NTP, as with every node event.
  - Do not write "well under 1" or "1/3" in the report until six-PC data exists. Node 3 tends to lead only in the demo's list. In the cells node 1 tends to lead and also wins ties, so the number
    can read above a third for that reason alone. Name that limit beside the figure.
  - If bidding loses to the baseline at load, D114 says to say so. The report states the price.
  - `analysis/test_analyze.py` is not in CI, so run it by hand.
- **Where.** `analysis/analyze.py::{leader_plan_share, commit_metrics, analyse_run}`, `analysis/test_analyze.py::TheLeadersRole`, `analysis/README.md`.
- **How we check it.** `analysis/test_analyze.py::TheLeadersRole`: the share of the node that led throughout; a change before measuring does not count; a change inside gives NaN; a moment with no
  leader gives NaN, even if the same node leads again; two nodes naming different leaders gives NaN; no `raft_state` gives NaN; the split on `forwarded`; a path with no commits. `python3
  analysis/test_analyze.py` passes. On real logs, rehearsal only: see below.
- **Report notes.** Report `leader_plan_share` beside `elections`, and beside the baseline's 1. State the runs it was computed from, and the users level. Report the local and forwarded commit latency:
  that gap is what a follower pays for not leading, and it is the cost of D26 and of peers.

### Evidence so far for D163 to D166: one machine, rehearsal only

Never report these as results from the six PCs. All used real processes and a release build in a private network namespace on one machine, with other jobs running (load average
about 9 to 10 for the first table, and between 9 and 22 for the runs without the queue), loopback, and storage on. Each row is one run. The config is the shipped `config/lab.example.toml` with loopback addresses: node 3
started first, nodes 2 and 1 a second later, then 3 load generators of 20 users each, think time 1000 ms, for 70 s. The figures count every request, with no warm-up.

**With the proposal queue (D133).** A throwaway copy of the dry-run merge of this lane onto `merge-draw`, outside the repo, so this is the code the merge should produce:

| Run | New terms from load start | Answered | Gave up | p50, p95, p99 ms | `leader_plan_share` | Shares of nodes 1, 2, 3 | Commit p50: local, forwarded |
|-----|---------------------------|----------|---------|------------------|---------------------|-------------------------|------------------------------|
| Bidding, nothing silenced | 0 | 4010 | 0 | 44, 66, 73 | 0.321 | 0.355, 0.324, 0.321 | 20.0 ms (1299), 19.7 ms (2749) |
| Bidding, the leader silenced for 20 s at +30 s | 1 | 4007 | 0 | 34, 54, 220; worst 846 | NaN, as designed | 0.381, 0.387, 0.233 | 10.2 ms (1556), 10.5 ms (2621) |
| Leader dispatch, nothing silenced | 0 | 4126 | 0 | 16, 27, 34 | 1.0 | 0, 0, 1.0 | 12.6 ms (4148), none |

What those runs showed, beyond the table:

- **Node 3 led before load in all three runs,** at term 6, with equal digests, and each node's log held one `register_responders` commit. So start order and list order did what D165 says, on this machine.
- **The demo moment, silence run.** Node 3 led and was silenced at +30.01 s. At +30.78 s node 1 had lost its leader, and at +30.79 s it followed node 2 in term 7. Three seconds in, `ctl status` read:

  ```text
  node       role   term leader  ...  serving silenced degraded members
     3     leader      6      3  ...    false     true    false       -
     2     leader      7      2  ...     true    false    false     1,2
     1   follower      7      2  ...     true    false    false     1,2
  ```

  That is the stale row D165 says to explain out loud. Node 3 was restored at +50.01 s and rejoined at +50.07 s as a follower in term 7. After that, 355 of 1080 plans were its own (0.329), and there were no more new terms. `gave up` was 0 and the worst
  latency 846 ms.
- **What peers cost here.** Bidding's p50 was about 28 ms above leader dispatch's in the plain runs, and the bid window alone is 20 ms (my reading of where it goes; not measured apart). This is one load, on one machine.

**Without the queue.** The lane's own build, since D133 is on another branch. With the same settings, bidding had 93 new terms in 70 s, 410 answered, 18 gave up and a p50 of 7.5 s, while leader dispatch had 0 new terms, 2995 answered, 0 gave up and a p50 of 396 ms. So these demo settings need
D133. The demo and the data must run a build that has it, and the check in the runbook stands. It also supports the risk named in D163: a single proposer is easier on the log than three.

**The metric on an older log, read-only.** On the rehearsal log `build/diag/sync-10`, `leader_plan_share` read 0.629, and counting that run's `commit` events by node gave 0.629 too. Shares were 0.629, 0.272, 0.099. The other logs the design cited no longer hold
their node events, so this one could not be repeated there.

**What none of this shows.** Anything about the lab's disks or network, a real power cut, loads above 20 users per client PC, or a load where the queue is working hard. The third check the design asked for, leader dispatch at `e1-300`'s load, has not been run on a build with the queue.


### Reading D167 to D180: who decided, and what the numbers are

Who decided, in every entry below, in the words the log should keep:

- **Noor Hany Emam** decided that clients send to the machines over UDP and that the machines talk to each other
  through group communication over tarpc. Their words, item 9 of the decisions of 2026-11-03: "UDP to send from clients
  to machines, group communication within machines, I think it's fine to use tarpc there."
- Item 11 of the same decisions records how: tarpc over a small datagram adapter, ctl on tarpc over TCP. That pick,
  and everything else below, was **decided by the agent on its recommendation, on Noor's instruction to finish without
  more questions (2026-11-03)**, and Noor can overrule any of it.

**One-PC rehearsal, never six-PC results.** Every number below is rehearsal: one machine, loopback, one kernel, no real switch
or network card. Nothing here has run on the six lab machines, and no number here may be reported as a six-PC result. Rows named
`n-*` and `m-*` are the spike's (`specs/udp-spike/results.jsonl`, kept outside the repo), on 8 CPUs, on the spike's tree. Rows named
`w-*` are from the code on `lane-wire`, on 24 CPUs shared with other jobs, and are listed after D180. They are not comparable across
the two sets.

### D167 · Reports travel over UDP through tarpc: `ClientRpc` on a small datagram adapter

- **Date:** 2026-11-03 · **Part:** C, F, and `proto.rs`, which is shared · **Status:** Implemented. Reverses D17 and its
  "one transport and one framing for everything"; D6's UDP returns; supersedes D112 for clients; amends D119, D53 and
  D18 · **Decided by:** Noor Hany Emam for UDP from clients to machines and tarpc within machines (decisions item 9,
  in their words at the top of this file). The way to do the UDP decided by the agent on its recommendation, on Noor's
  instruction to finish without more questions (2026-11-03), recorded as decisions item 11
- **Context.** Until now nodes called each other through tarpc (D119), but client PCs and `ctl` still used frames and
  reply matching we wrote ourselves, over TCP (D17). Two notes pull on this. Noor's item 9 says UDP from clients to
  machines. The class notes of Oct 4 say "For project use UDP -->don't go this low level, u will use ready made RPC or
  RMI". Hand-written frames meet neither. Item 9 left open how to do the UDP, and a spike measured the ways.
- **Options considered.**
  1. *tarpc over a small datagram adapter, ctl on tarpc over TCP (chosen).* tarpc still does the call IDs, the deadlines,
     the matching and the generated stubs, and we write only the piece that puts one message in one packet. There is no
     connection to look after, so no pings and no reconnects, and a node that loses power costs the client nothing. The
     price is about 420 lines of our own under the RPC, packet loss to explain, and new setup steps on six machines.
  2. *Every remote call through tarpc over TCP.* One mechanism, no new setup steps, and about 1.5 hours cheaper by the
     design's estimate. It is the agent's first recommendation in the design file, and it goes against Noor's item 9, so
     it was not taken. It would also have closed UDP for this deadline, because there would be no time left afterwards.
  3. *Keep the frames and only write the choice down.* No new risk to working code. It meets neither item 9 nor the Oct 4
     note on the path clients really use, so it was rejected as a choice. It stays only as the idea of a fallback, and see
     Consequences: on this branch there is no switch back to it.
  4. *`coap` crate 0.29.* Gives request and reply, but with our own dispatch on resource paths, not generated stubs. It
     is a new distributed library under AGENTS.md rule 6, and one failed `send_to` ends its receive loop (read in
     coap-0.29.0, src/server.rs:262-279). Rejected.
  5. *Our own request and reply over UDP.* A spike ran it and its suites passed, but hand-built request and reply is the
     very level the Oct 4 note rules out. Rejected, and it stays only as a measuring tool in the spike.
  6. *UDP with a config key to pick the transport.* Pays for two client paths and doubles the tests to keep one key.
     Rejected (design file, `dropped_after_attack`).
- **Decision.** `proto::ClientRpc` is a `#[tarpc::service]`: `submit(request: ClientRequest) -> ClientReply`, plus the
  control methods of D175, with no term argument, because the term rule is for peer messages. `MeshClient` holds one stub
  and one unconnected UDP socket per node. Application code still makes one call to `submit`, and the client middleware
  makes one call per node: the group send (D126, Lecture 9 section 2.2). Node to node stays on tarpc over TCP
  (`PeerRpc`), unchanged here. `ClientRequest` keeps its fields, with `attempt`, which keys bid rounds, and `age_ms`,
  which Gap 15 added. `ReplyBody` keeps its bodies. No change to `Command`, `CloudState` or the Raft log.
- **Why.** It fits Noor's sentence exactly, and it fits D6's first reason, that request and reply needs no connection and
  the reply acts as the ack. It also stays a ready-made RPC, so it meets "u will use ready made RPC or RMI". The rows do
  not stand in its way: on the spike's 8 CPUs, tarpc over UDP tied TCP at every shape the experiments use (all at once
  with users waiting, 1000 users: `m-udp` 428.6 reports a second against `m-tcp` 444.0; 3000 users: 435.3 against 442.1).
  On the final code, `w-steady-1000-a` and `-b` gave 975.5 and 978.5 reports a second with a p50 of about 24 ms and term 2.
  The one real technical edge seen is that under true open-loop overload at 3000 a second only TCP lost its leader in the
  rows we have. No cell runs that shape, so this entry does not lean on it.
- **Consequences.** About 450 lines of our own sit under the RPC on the measured path, and a grader can fairly point at
  them as the low level the note warns about (see D178). Packets can be lost, and a lost reply costs a whole wait of 1.6 to
  2.4 s, because a reply has only one copy. At saturation packets drop where TCP would queue, so the report must explain
  2 s retry shapes in the top cells. There are three new setup facts on six machines (D171). And there is no way back on
  this branch except reverting its commits: the old frames are gone, as the design says, because keeping both paths is
  the option rejected above. Gap 14 under this choice would be a one-way datagram beside tarpc, but publish-subscribe is
  out of Phase 1 by decisions item 14, so nothing here builds it.
- **Where.** `src/proto.rs::ClientRpc`, `src/node/datagram.rs`, `src/node/transport.rs` (client port section),
  `src/client.rs`, `src/node/mod.rs` (`start`, `start_with`).
- **How we check it.** Every multi-node suite ran through it: `tests/common/mod.rs::first_reply` now sends over UDP, and
  `TestCluster` and `tests/durable.rs` bind UDP as well as TCP. `tests/client.rs` and `tests/datagram.rs`. Real programs
  in a private network namespace: the `w-*` rows. **Real networks can only be checked in the lab.**
- **Report notes.** This reverses a decision the team confirmed on 2026-11-01 (D17). Put it in the "decisions changed"
  table, with Noor's sentence, D6 and D17 side by side. Credit tarpc (MIT).

### D168 · `ctl` stays on tarpc over TCP, on the same port number; `ctl status` checks UDP; no config key

- **Date:** 2026-11-03 · **Part:** C, F · **Status:** Implemented. Amends D18 (TCP and UDP on `client_addr`) · **Decided
  by:** the agent on its recommendation, on Noor's instruction to finish without more questions (2026-11-03), recorded
  as decisions item 11
- **Context.** With reports on UDP, the operator's commands must go somewhere. A lost `silence` order that vanishes with
  no error would spoil an experiment without anyone knowing. And the node's status table, read over TCP, would look
  healthy while a missing UDP firewall rule makes every report to that node vanish.
- **Options considered.**
  1. *The same trait on both: UDP for reports, TCP for ctl, one port number, and `ctl status` also calls status over UDP
     (chosen).* A lost order cannot vanish, there is no second port number to learn, and a missing rule shows at once.
  2. *ctl over UDP too.* One socket only, as in the spike. A lost silence order or a lost answer just looks like a timeout.
     Rejected: the runner's fault cells depend on the order landing.
  3. *A config key for the client transport.* Rejected in D167.
  4. *A separate port for control.* A third port for the firewall, and D18's reason still holds: control passes the fault
     gate, so a separate port gives no isolation. Rejected.
  5. *The check as a column in the status table.* Neater to read. Rejected: it shares the lines lane-peer edits for its
     `members` column, and scripts read this table by position. A line per node under the table costs nothing.
- **Decision.** `transport::ClientPort` binds both on `client_addr`. With port 0, for tests, TCP picks the number and UDP
  must get the same one, so a number already taken on UDP means trying another, at most 20 times. `ctl` and
  `client::control` keep their signatures and go over TCP. `ctl status` prints its table as before, then one line per
  node: `node 2: udp ok`, or `node 3: udp no answer. Reports to it vanish: is it running, and is the port open? sudo ufw
  allow 8001/udp`. The check, `client::udp_answers`, makes up to 3 status calls within 2 s, so one lost packet cannot read
  as a closed port. A silenced node still says `udp ok`, because control passes the gate, and silenced is not the same as
  a closed port. `scripts/run-experiments.sh` runs the check before each run's load and stops the batch on any `no
  answer`. No config key.
- **Why.** The first hour on six machines is where a missing rule would cost most, and this makes the failure say what it
  is. The batch guard matters because a run with one node deaf to clients looks fine: two nodes still answer everything.
- **Consequences.** Two listeners on one port number, a new thing to explain in the lab, and `sudo ufw allow 8001/udp` as
  well as `8001/tcp`. `ctl status` takes up to 2 s longer when a port does not answer. The check runs from wherever `ctl`
  runs, so it proves that one machine's path. The runner runs it from the first client machine, and the other client
  machines share the same node-side rule.
- **Where.** `transport.rs::ClientPort`, `serve_clients`, `serve_control`, `serve_datagrams`, `client.rs::udp_answers`,
  `bin/ctl.rs::check_udp`, `scripts/run-experiments.sh::require_udp_ports`, `config/lab.example.toml`.
- **How we check it.** `tests/client.rs::ctl_status_says_when_the_udp_port_does_not_answer` runs the real `ctl` against a
  stub with no UDP socket; `ctl_status_says_udp_ok_for_a_silenced_node`; `a_node_answers_control_over_tcp_with_every_method`.
  The runner's guard was not run: it needs the hosts file and SSH of a lab machine.
- **Report notes.** Worth one line in the runbook part of the report: first check on the six machines.

### D169 · A call's life: a node that cannot answer stays quiet, the client's cancel ends the call, one deadline per submit

- **Date:** 2026-11-03 · **Part:** F, C, A · **Status:** Implemented. Keeps D36; amends D37 (fewer identical replies) and
  D53 (tarpc matches replies; one socket per node stands) · **Decided by:** the agent on its recommendation, on Noor's
  instruction to finish without more questions (2026-11-03)
- **Context.** Under RPC every call wants an end. Under bidding, one node answers and the other two have nothing to say,
  because a node that lost the bid does not answer with a result (D36). So what ends those two calls? The spike tried four
  ways. Each row below is steady load at 1000 users unless it says burst. The names need reading, because they do not all
  mean what they seem to:
  - `n-silent`: the node stays quiet, and when the client has its answer it drops the other two calls, so tarpc sends one
    cancel datagram each, which ends the calls on the nodes.
  - `n-keep`: *the client* does not cancel. Its calls run on in tasks until their deadlines, so both ends hold each call
    until then. The node behaves as in `n-silent`. This is the variant the rows call worst.
  - `n-quick`: the node answers an empty `None` as soon as it loses the bid round, which ends the call early.
  - `n-done`: the node answers with the committed result when its work ends and the node has it.

  | Variant | Reports a second | p99 ms | Foreign CPUs, about | Datagrams beyond requests and the one answer |
  |---------|------------------|--------|---------------------|----------------------------------------------|
  | `n-silent-steady`, run 2 | 967.6 | 225.5 | none | 48,468 cancels |
  | `n-done-steady` | 969.5 | 225.5 | none | 48,459 cancels, about 220 extra answers |
  | `n-quick-steady` | 943.5 | 1921.5 | none | 425 cancels, 47,328 empty answers |
  | `n-silent-steady`, run 1 | 695.9 | 7330.2 | 1.4 | 37,601 cancels |
  | `n-keep-steady`, run 1 | 574.1 | 12019.3 | 1.4 | none |
  | `n-keep-steady`, run 2 | 331.3 | 15736.7 | 3.3 | none |
  | `n-silent-burst` | 445.2 | 6889.6 | none | 23,370 cancels |
  | `n-done-burst` | 422.1 | 6930.7 | none | 21,994 cancels |
  | `n-quick-burst`, two runs | 408.4, 393.4 | 6924.1, 6947.8 | under 1 | about 4,400 cancels, about 30,000 empty answers |
  | `n-keep-burst`, two runs | 368.4, 347.2 | 12662.0, 13625.9 | 3.3, 2.5 | none |

  An honest reading of `n-keep`: its rows were also the ones with other jobs on the machine, so 331 to 574 overstates the
  difference. The fair pair is the two runs with the same foreign load, 695.9 against 574.1 with p99 7330 against 12019 ms.
  It is still worst there. Foreign CPUs are worked out from each row's busy share and its own CPU ticks.
- **Options considered, for what ends a call that has nothing to say.**
  1. *`n-silent`: the node stays quiet and the client cancels (chosen).* Keeps D36 to the letter. A later takeover can
     still answer, so one lost answer costs little: p99 stays at 225 ms.
  2. *`n-quick`: an empty answer at once.* Saves no traffic: 47,328 empty answers instead of 48,468 cancels, one datagram
     each. It ends the call before a takeover or a stored answer could use it, which is the p99 of 1921 ms against 225 ms. It
     also changes the type of `submit` to `Option`, and adds a "no" that D36 rejected: a no from one node says nothing about
     the other two.
  3. *`n-done`: answer with the result.* The numbers tie with `n-silent`, because the client's cancel nearly always gets
     there first (about 220 extra answers in 24,510). So it buys nothing, and it breaks D36: a node that lost the bid would answer
     with a result. Rejected on D36, and the data give no reason to reopen D36.
  4. *`n-keep`: never cancel.* The worst in every pair, and it would get worse with the chosen deadline. At 1000 reports a second,
     each node would hold about 24,000 open calls (two thirds of 1000 a second, for 36 s), against about 1,300 with the rows' 2 s
     deadline. Rejected.
- **Options considered, for the deadline.**
  1. *One deadline per submit, 1.2 times the sum of all waits, so 36 s at the defaults (chosen).* Calls from earlier
     attempts stay open until an answer or give-up, so a late answer to attempt 0 still counts, as it did on TCP.
  2. *A deadline per attempt, as in the spike's rows.* A slow commit then becomes another attempt and its answer is thrown
     away. Its one merit is that a lost cancel parks a call for 2 s and not 36 s. Rejected. **No spike row used the chosen
     deadline, so this was the unmeasured part of the design,** which the `w-*` rows now cover: 975.5 and 978.5 reports a
     second with p99 226.6 and 225.5 ms, matching the spike's best `n-silent`.
  3. *tarpc's default of 10 s.* Loses answers after 10 s with no sign. Rejected.
- **Decision.**
  - A node that cannot answer sends nothing: not serving, lost the bid, full, could not commit. `ClientServer::submit`
    waits on a one-slot channel, and if `handle_request` ends without replying, the method never returns, so the call lasts
    until the client's cancel or its deadline.
  - `handle_request` runs in its own task, unchanged, because a cancel aborts the tarpc method but not a task it spawned:
    one node's answer must not cut off another node's bid or commit. `ReplyTx::send` returns false if the call has ended,
    so `replied` is never logged for an answer that went nowhere.
  - `submit_from` holds one `FuturesUnordered` for the whole submit. Each attempt logs `client_sent` and adds one call per
    node, then waits on a biased select between the next call's result and that attempt's jittered timer. A ready answer
    beats the timer. A guard stops a spin once every call has ended, and the timer still decides when the next attempt goes.
  - The deadline is taken from `Instant::now()` inside `submit_from`, never from `started`, which open loop sets to the
    intended send time. It is capped at a year, because tarpc's deadline queue panics past about two.
  - Returning drops the set, and tarpc sends one cancel per call still open. So a first-attempt answer costs 3 requests, 1
    reply and 2 cancels: 6 datagrams against the 4 frames of the old path. On a give-up, up to 18 calls are open, so up to 18
    cancels go out.
  - An answer counts only if it carries this submit's request ID. tarpc matches by call ID alone, and a program restarted
    on an old source port could be handed the old program's reply.
  - `ReplyBody` is matched with no wildcard, so a new body cannot start out ignored, and `Expired` ends the wait (Gap 15's
    trap).
  - D52's retry rule is unchanged: 6 sends, waits of 2, 4, 6, 6, 6, 6 s with ±20% jitter, the same ID and `attempt + 1`.
- **Why.** See the table: `n-silent` is the only variant that keeps D36 and gives away nothing. Traffic stays honest, too:
  the cancels are real datagrams, counted (D172), and "fewer cancels" in `n-quick` is the same traffic under another name.
- **Consequences.** A lost cancel parks a call on a node for up to 36 s (D173). Up to 3 calls per attempt are open at once.
  D37's "up to three identical replies" becomes fewer: the first ends the submit and cancels the others.
- **Where.** `client.rs::submit_from`, `group_send`, `call_budget`, `answer_to`, `transport.rs::ClientServer`, `ReplyTx`,
  `run_calls`, `node/mod.rs` (`on_request`).
- **How we check it.** `tests/client.rs`: `a_late_answer_from_an_earlier_attempt_counts`,
  `a_quiet_node_does_not_hold_up_another`, `a_cancel_does_not_stop_the_nodes_work_and_is_counted`,
  `a_node_that_cannot_answer_stays_quiet_until_the_deadline`, `an_expired_reply_ends_the_wait_at_once`,
  `a_retry_keeps_the_request_id_and_increments_the_attempt`. Unit tests in `client.rs`: `the_call_budget_is_36_s_at_default_timers`,
  `only_a_real_answer_for_this_request_counts`. `tests/datagram.rs::a_cancel_aborts_the_handler_and_a_deadline_needs_no_cancel`.
- **Report notes.** The table above is good material, but only with its foreign-CPU column, and marked as rehearsal on one
  machine. Say in plain words that the worst variant is a client that does not cancel, not a node that stays quiet.

### D170 · The datagram adapter: one message is one datagram, and no error reaches tarpc

- **Date:** 2026-11-03 · **Part:** C · **Status:** Implemented · **Decided by:** the agent on its recommendation, on Noor's
  instruction to finish without more questions (2026-11-03)
- **Context.** tarpc's `serde_transport` needs a byte stream (tarpc src/serde_transport.rs:93-102), but its `Transport`
  trait takes any `Stream` plus `Sink`. So one message can go in one packet with little code. The trap: any error from
  that transport ends the dispatch and every call on it. On UDP errors are normal.
- **Options considered.**
  1. *Our adapter with `UdpFramed` for reading and `try_send_to` for writing, and a reader that splits by source address
     (chosen).* What the spike ran.
  2. *`UdpFramed` for both sides.* Its `Sink` keeps a datagram whose send failed and appends the next message to it, so
     after one failure every later send fails too. Tested: `udpframed_sink_never_recovers_from_a_failed_send`. Rejected.
  3. *Connected sockets on the client.* Simpler reads, but an ICMP "port unreachable" from a dead node comes back as an
     error on the next send, and on a cancel that is terminal, so one dead node would kill the stub. Tested:
     `a_dead_node_kills_a_connected_naive_client_only`. Rejected.
  4. *One tarpc channel per socket on the node, by giving each client program its own port.* There is one port number by
     design. Rejected.
  5. *Several sockets with `SO_REUSEPORT` on the node.* The spike had a switch for it. It spreads the read load, but adds
     moving parts with no row asking for them. Left out.
- **Decision.**
  - One tarpc message is one datagram, encoded with `proto::encode`, at most `MAX_DATAGRAM_BYTES` = 1,400 bytes, so it is
    never split on an Ethernet path, whose payload is 1,472 bytes. Larger is a counted encode error. A test holds the
    largest report and reply under the limit, every integer at its longest varint.
  - **No error ever reaches tarpc.** A bad datagram, an ICMP error or a failed send counts as a lost message. Decoding
    runs inside `catch_unwind`, because tarpc adds a request's deadline to the clock while decoding, and that panics on a
    huge one, which would have ended the reader and with it the node's client port. A request asking for more than a year
    is treated as bad too, because tarpc's deadline queue panics past about two.
  - **Client end.** One unconnected socket per node, and a reply from any other address is counted and dropped, because
    call IDs are small numbers and a stray one must not match a call. It waits for send readiness, so a burst of users
    queues in tarpc and not on the floor.
  - **Node end.** One socket on `client_addr`. `datagram::serve` splits datagrams by source address into one tarpc channel
    per client program, through a queue of 16,384 messages each; a full queue drops and counts, because the reader must
    never wait. A program idle for `CHANNEL_IDLE` = 60 s loses its channel, because UDP has no close. Its sends never wait:
    all channels share one socket, and tokio keeps one waker per socket for writing, so a channel that waited could lose
    its wake-up to another and sit until its next message. A reply goes out now or counts as lost. **This is a change from
    the spike**, whose node end waited on send readiness.
  - **Every task is owned.** `serve` holds its channels in a `JoinSet`, and `transport::run_calls` holds each channel's
    calls in another, so aborting `serve`, as node shutdown does, ends them all and frees the port. The stubs' dispatch and
    the client's reporter are in a `JoinSet` on `MeshClient`, not `.spawn()`. This is also a change from the spike, whose
    channels and calls were spawned free. `handle_request` keeps its own task on purpose (D169), and every wait in it is
    capped.
  - **Removed from the spike:** every `SPIKE_` switch, the injected loss, the per-attempt deadline, the `NonWinner` modes,
    `Losers`, the counters file, `report(...) -> Option`, and the `sizes`, `spike_blast`, `spike_e2e` and `spikeload` programs.
- **Why.** Each rule answers something the spike's tests showed by running it: see `tests/datagram.rs`, where every test
  states a claim about tarpc or tokio-util and checks it, so a version bump that changes one fails there.
- **Consequences.** Out of order and duplicate messages are harmless to tarpc itself: a duplicate request while the first
  is in flight is ignored, and one after it finished runs again, which `Command` idempotence and the results table cover. A
  program restarted on an old source port can have a new call ignored until the old one's deadline (D173).
- **Where.** `node/datagram.rs`, `transport.rs::run_calls`, `proto::MAX_DATAGRAM_BYTES`.
- **How we check it.** `tests/datagram.rs`, all of it, including `a_huge_deadline_is_a_bad_datagram`,
  `a_message_too_large_is_counted_and_the_channel_lives_on`, `an_idle_program_loses_its_channel_and_gets_a_new_one` and
  `aborting_serve_ends_every_channel_and_call_and_frees_the_port`. `proto::tests::the_largest_report_and_reply_fit_one_datagram`.
- **Report notes.** This is the code a grader can point at as "low level". Say what it does not do: it matches nothing,
  times nothing and retries nothing. tarpc does the first two and our middleware's D52 the third.

### D171 · UDP receive buffer, firewall rule and drop counting; new dependency `socket2`

- **Date:** 2026-11-03 · **Part:** C, all · **Status:** Implemented; **no row has run with the buffer raised** ·
  **Decided by:** the agent on its recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** All users' copies land in one socket's receive buffer. When it is full the kernel throws the next one away
  and adds to `UdpRcvbufErrors`. The default buffer is 212,992 bytes, and the kernel charged 896 bytes per packet on
  loopback in the rows, so it holds about 238 packets. The spike's all-at-once rows at that size lost 1.8% (1000 users) and
  5.7% (3000 users) of copies. On the final code, without the sysctl, `w-burst-3000` lost 5,669 copies (4.1%) with the
  kernel's 425,984 bytes, and stayed on term 2.
- **Options considered.**
  1. *Ask for a large buffer with `SO_RCVBUF`, log what the kernel gave, raise `net.core.rmem_max` by hand on all six
     machines, and count drops (chosen).*
  2. *Leave the default.* No setup step, and the rows above show the cost: each lost copy can cost a user a 2 s wait.
     Rejected.
  3. *Several sockets with `SO_REUSEPORT`.* See D170. Rejected for now.
  4. *`SO_RCVBUFFORCE`, which ignores the cap.* Needs a capability the node does not run with. Rejected.
- **Decision.** Every UDP socket, on nodes and clients, asks for `RECEIVE_BUFFER_BYTES` = 8 MiB through `socket2`, before
  the bind. The kernel never refuses: it caps the request at `net.core.rmem_max`, then doubles it. So `report_buffer` logs
  what it really gave, as a `udp_socket` event, and warns if that is under what was asked, naming the fix. `resource_usage`
  gains `udp_drops`, and `udp_counters` carries the same number on client PCs, which log no `resource_usage`. Setup on the
  six machines: `sudo sysctl -w net.core.rmem_max=8388608` on all six, and `sudo ufw allow 8001/udp` on the node machines.
- **Why this number.** The design asked for a value from a rehearsal row's peak queue. No row can give one: the peaks are
  clipped at the buffer itself (213,248 bytes in a burst row), so they show the buffer and not the need. So the number comes
  from arithmetic. One copy from each of 3000 users at once is 2.7 MB at 896 bytes a packet. A network card's driver may
  charge a whole 2 KB buffer per packet (general knowledge, not checked), which makes about 7 MB. 8 MiB covers both.
- **Consequences.** A sysctl that needs sudo on six machines, and one more thing to forget. A missed one does not fail: the
  node runs with a small buffer and says so, on stderr and in `udp_socket`, and `analyze.py` warns about any cell where it
  happened (D172). **The raised buffer itself is untested.** Raising the sysctl changes a system setting on the rehearsal
  machine, so the agent did not do it. The first lab run should check `rcvbuf_got_bytes` and `udp_drops_nodes` before
  anything else. Real network cards may also charge differently from loopback.
- **Where.** `node/datagram.rs::open_socket`, `report_buffer`, `node/usage.rs::read_udp_drops`, `metrics.rs`,
  `Cargo.toml`: `socket2 = "0.6"`, already in the lockfile through tokio, and tokio-util's `net` feature for `UdpFramed`.
- **How we check it.** `usage::tests::udp_drops_come_from_the_udp_lines_by_name`; the `w-burst-3000` row shows both the
  count and the buffer. The lab's first row checks the rest.
- **Report notes.** State the limit plainly: every UDP number in this report came with whatever `rcvbuf_got_bytes` says.

### D172 · What is measured on the UDP path: `udp_counters`, with cancels counted

- **Date:** 2026-11-03 · **Part:** all · **Status:** Implemented · **Decided by:** the agent on its recommendation, on
  Noor's instruction to finish without more questions (2026-11-03)
- **Context.** On TCP, cancels did not exist, and frames per request were 3 plus 1. On this path a first-attempt answer is 3
  requests, 1 reply and 2 cancels. E5's `msgs_*` figures count what nodes send, so a client's cancels would be invisible
  there. A report that said "4 messages per request" would be wrong by a third.
- **Options considered.**
  1. *A once-a-second `udp_counters` event on nodes and clients, all cumulative, with the analysis working out figures per
     answered request (chosen).* Counts every datagram, cheap, and each owner reads the same numbers.
  2. *Report cancels by arithmetic, up to 3 per attempt, as the design's option A does.* Possible on TCP, where nothing else
     can be known. Here the adapter sees every message, so arithmetic would throw away a true count. Rejected.
  3. *A new `MsgKind` for cancels.* `msgs_*` reads what nodes send, and a node never sends a cancel, so it would show 0.
     Rejected.
  4. *A per-call `client_cancel` event.* A log line per cancel on the measured path. Rejected.
- **Decision.** `Event::UdpCounters`, cumulative, once a second and once more as a client stops: `rx_datagrams`, `rx_bytes`,
  `rx_max_bytes`, `rx_bad`, `rx_foreign`, `rx_requests`, `rx_cancels`, `rx_gate_dropped`, `rx_queue_full`,
  `channels_opened`, `tx_datagrams`, `tx_bytes`, `tx_max_bytes`, `tx_requests`, `tx_cancels`, `tx_replies`,
  `tx_would_block`, `tx_encode_errors`, `tx_errors`, and `udp_drops`. `analyze.py` gains `udp_requests_per_req`,
  `udp_cancels_per_req`, `udp_datagrams_per_req`, `udp_request_loss_share`, `udp_cancel_loss_share`, `udp_gate_dropped`,
  `udp_queue_full`, `udp_bad`, `udp_send_lost`, `udp_drops_nodes`, `udp_drops_client_pc_max` and
  `udp_rcvbuf_got_min_bytes`, and prints a warning for a capped buffer or any drops.
- **Kept on purpose.** Every event and `MsgKind` name, and `client_sent`, `client_done`, `client_gave_up`,
  `request_received`, `replied` and `provisional_replied` mean what they did. `client_request` still counts requests a
  node took, past the gate. The `client_link_up` and `client_link_down` variants stay in `Event`, with nothing logging them
  now, because lane-load's analysis reads the names and reads zero for a run with none.
- **Why.** "Keep the cancel traffic honest" was asked for by name. In the rows it comes to exactly 2 per first-attempt
  answer: `w-steady-300` sent 27,000 requests, 18,000 cancels and got 9,000 replies.
- **Consequences.** About 500 bytes a second per process in the log. Client PCs' machine-wide drop count is reported by
  each program on that PC, so for clients only the largest means anything. On one machine, in rehearsal, each node counts
  the drops of all three, so `udp_drops_nodes` reads three times too high there. Loss shares compare two machines' counters
  over slightly different windows, so they are approximate.
- **Where.** `metrics.rs::UdpCounters`, `UdpCounts`, `Event::UdpCounters`, `Event::UdpSocket`, `node/mod.rs::spawn_reporter`,
  `client.rs::report_counters`, `analysis/analyze.py::udp_metrics`, `udp_warnings`, `analysis/README.md`.
- **How we check it.** `metrics::tests::udp_counters_log_as_flat_fields`, `analysis/test_analyze.py` (synthetic),
  `a_cancel_does_not_stop_the_nodes_work_and_is_counted`.
- **Report notes.** E5 must print `udp_datagrams_per_req` next to `msgs_all_per_req` and say the first has the cancels.

### D173 · Accepted limits on the UDP path

- **Date:** 2026-11-03 · **Part:** F, C · **Status:** Implemented · **Decided by:** the agent on its recommendation, on
  Noor's instruction to finish without more questions (2026-11-03)
- **Context.** Each of these was seen, read or worked out, and each has a cost we chose to carry.
- **Options considered, in general.** Fix each now, with more code on the measured path, the day before the six-PC test; or
  bound it, state it and read the numbers. The options for each are below.
- **Decision.** Accept and state:
  1. *No admission limit on the node.* tarpc's own limit counts parked calls too, so it would count quiet nodes' calls. A
     cap on running `handle_request` tasks stays an idea for if a lab row loses its leader.
  2. *No backpressure.* At saturation packets drop where TCP would queue. The report explains the 2 s retry shapes.
  3. *A lost cancel parks a call on the node for up to 36 s.* Arithmetic in the design: 300 reports a second x 2 cancels x
     10% lost x 36 s is about 2,160 parked calls, a few MB. In `w-burst-3000`, the row with drops, 6 cancels were lost of
     114,692. With real 5% loss, about 3,200 calls were parked at a time by arithmetic, and peak memory read the same as with no
     loss (the loss table in the rehearsal rows after D180).
  4. *A program restarted on an old source port can have a new call ignored until the old one's deadline,* and could be
     handed an old reply, which the ID check drops (D169). Rare, and it costs that call one wait. Do not count channels as
     clients.
  5. *Capacity constants.* 20,000 calls in flight and 2,000 buffered per stub, reused from the peer port, against tarpc's 1,000
     and 100. The worst closed-loop stub is users x attempts: 1000 x 6 = 6,000. An open-loop `--rate` above capacity makes new
     calls wait. tarpc's own 100-response buffer stays on the node, against the spike's 4,096, because no channel comes near
     100 replies waiting: the socket write never waits (D170).
  6. *Lost replies have one copy.* A lost reply costs a whole wait of 1.6 to 2.4 s, unlike a lost request, which has two more
     copies.
- **Why.** Each is bounded or rare at this scale, and each fix adds code on the measured path for a case nobody has seen
  hurt.
- **Consequences.** The report must state them. If the lab's first top cell loses its leader or shows many parked calls,
  item 1 is the first thing to revisit.
- **Where.** `transport.rs::MAX_IN_FLIGHT_CALLS`, `CALL_BUFFER`, `stub_settings`, `client.rs::call_budget`.
- **How we check it.** `more_than_a_thousand_calls_can_wait_on_one_stub`, `a_reused_source_port_can_collide_with_an_old_id`,
  `rss_kb` in `resource_usage` during a burst.
- **Report notes.** A short "limits" list, in these words.

### D174 · The fault gate on the client port

- **Date:** 2026-11-03 · **Part:** C · **Status:** Implemented. D22 and D23 stand · **Decided by:** the agent on its
  recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** AGENTS.md: the fault gate drops everything but control. On tarpc, a call that reaches a method is already
  tracked, with a deadline entry and a task, so a silenced node would hold it for up to 36 s doing nothing.
- **Options considered.**
  1. *Check in the reader for `submit`, before tarpc tracks the call, and again where a reply could leave (chosen).*
  2. *Only at the top of the method, as on the peer port.* Simple, but each silenced second parks up to 3000 calls for 36 s.
     Kept as well, for a `submit` over TCP.
  3. *Inside `handle_request`.* Too late: it would log `request_received` for traffic a silenced node must not see.
  For a cancel while silenced: *let it through (chosen),* or drop it with the rest. It changes nothing that can be seen from
  outside: no work and no reply. Dropping it would only park a call that is already open.
  For a status call over UDP while silenced: *answer it (chosen),* or drop it, which would make a silenced node read `udp no
  answer` and look like a firewall problem.
- **Decision.** `serve_datagrams`'s `keep` closure drops `Submit` requests while the gate is closed, and counts them as
  `rx_gate_dropped`. `ClientServer::submit` checks at its top, for TCP, and again after the wait, because the gate can close
  after the reply is handed over. `ReplyTx::send` checks too. Control methods check nothing, on either transport. A method
  added to `ClientRpc` must decide which it is, and the trait's comment says so.
- **Why.** Same model as the peer port: a silenced node looks from outside like a machine that ignores the network.
- **Consequences.** `submit` over TCP also works, through the top-of-method check. Nothing calls it. There are no pings now,
  so "a silenced node keeps its connection" has no meaning here, and its test is gone (D180).
- **Where.** `transport.rs::serve_datagrams`, `ClientServer`, `ReplyTx`.
- **How we check it.** `tests/client.rs::a_silenced_node_drops_submit_and_answers_control`,
  `a_silenced_follower_does_not_disturb_service`, `a_silenced_leader_is_covered_by_a_retry_with_the_same_id`,
  `bidding_serves_a_client_with_a_node_silenced`, `ctl_status_says_udp_ok_for_a_silenced_node`, unit tests on `ReplyTx`.
  Rehearsal: `w-silence-leader-1000`.
- **Report notes.** One line: the gate sits in front of the RPC, not inside it.

### D175 · Control as methods on `ClientRpc`; the old frames go; sockets back in `node/`

- **Date:** 2026-11-03 · **Part:** C, F, all · **Status:** Implemented. Amends D18; D23 stands; `ReplyBody::Rejected` stays
  as D36 says · **Decided by:** the agent on its recommendation, on Noor's instruction to finish without more questions
  (2026-11-03)
- **Context.** `ctl` was plain request and reply with no multicast excuse at all, hand-written over frames. Gap 15 added
  `ControlRequest::Incidents` beside `Status`, `Silence` and `SkewLoad`.
- **Options considered.**
  1. *Typed methods: `status`, `silence`, `skew_load`, `incidents` (chosen).* Visibly a remote interface.
  2. *One `control(ControlRequest)` method.* Smaller, and D119 already rejected the same shape on the peer port as our own
     protocol tunnelled through tarpc.
  3. *Keep the frames behind a switch.* Two paths to test, and a literal grader still finds them.
  For `ReplyBody::Rejected`: *leave it unused (chosen)*, or delete it, which is one more contract change for nothing.
- **Decision.** `ClientFrame` and `ServerFrame` are gone. `ControlRequest` stays as the type `ctl` builds, and
  `client::control` keeps its signature and maps the enum to methods, so `bin/ctl.rs` and `tests/cluster.rs` hardly change.
  On the node, `on_client_frame` splits into a `RequestHandler` and a `ControlHandler`. `transport::connect_client` does the
  connect, `framed()` and stub settings for TCP, and `transport::datagram_client` does the same for UDP, so `client.rs`
  opens no sockets and AGENTS.md's "Sockets live in two places" is true again. D112's exception for `client.rs` ends.
- **Why.** No hand-written request and reply is left on any path.
- **Consequences.** Reply matching and connection handling are tarpc's now, so an old client and a new node fail quietly:
  all six machines must build one commit, which is already the rule (D15).
- **Where.** `proto.rs::ClientRpc`, `client.rs::control`, `transport.rs`.
- **How we check it.** `tests/cluster.rs`, unchanged. `a_node_answers_control_over_tcp_with_every_method`.
- **Report notes.** Goes with D176.

### D176 · RPC over RMI, and tarpc: the comparison with Java RMI, CORBA and Sun RPC/XDR

- **Date:** 2026-11-03 · **Part:** all · **Status:** Implemented. Closes D119's report note · **Decided by:** the agent on its
  recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** Class notes, Oct 4: "technique pick one and document why u picked the one u chose?" Oct 28: "Fe table in
  chapter 2, where we need to pick one system like either JAVA RMI, CORBA; Sun XDR ... check slides". Spec lines 377-381 fix
  Rust and allow libraries that do not hide what is assessed.
- **Options considered.**
  1. *tarpc 0.37, compared with Java RMI, CORBA and Sun RPC/XDR in one table, stated as a reading (chosen).* We read Oct 28
     as "pick one system of that kind": Rust rules out using those as named. The spec fixes the language, so the honest
     words are "nothing we could rely on by the deadline", not "no Rust route exists".
  2. *tonic/gRPC.* D119 rejected its build step and schemas.
  3. *An ONC RPC/XDR crate.* Would name a listed system, but it adds a new codec and a new distributed library for a label.
  4. *RMI-shaped remote objects on top of tarpc.* We offer service procedures over replicated state, with no object identity
     to name. Remote objects would add a registry nobody needs.
- **Decision.** RPC, not RMI. tarpc is the system. The table, from the slides unless marked:

  | | Java RMI | CORBA | Sun RPC | tarpc (our pick) |
  |---|----------|-------|---------|------------------|
  | Style | RMI | RMI | RPC | RPC |
  | Languages | Java only | many | many | Rust only |
  | Interface | remote interface | CORBA IDL | Sun XDR | `#[tarpc::service]` trait |
  | Marshalling | Java serialization | CDR | XDR | serde with bincode |
  | Semantics on its own | at-most-once | at-most-once, with maybe as an option | at-least-once | maybe: it never resends |
  | Transport (general knowledge for the first three: check before citing) | usually TCP | usually TCP | UDP or TCP | UDP for reports through our adapter, TCP for `ctl` and between nodes |

  **Role map** (Chapter 5, lines 235-254). Client stub: the generated `ClientRpcClient` and `PeerRpcClient`. Server stub and
  dispatcher: the generated `serve()`. Service procedure: `impl ClientRpc for ClientServer`, into `handle_request`.
  Communication module: tarpc over our datagram adapter, or over framed TCP. Marshalling: bincode. Interface: the trait.

  **Message map** (Chapter 5, lines 70-81), in two layers. Message type: tarpc's `ClientMessage` and `Response`. tarpc's
  `Request.id`, new per call on one channel, matches replies. Our `RequestId { client, seq }` stays the same across attempts,
  carries the sender and drives the duplicate filter. Method ID: the generated enum variant. Object reference: none.
- **Why RPC and not RMI.** Our service is procedures over replicated state. Nothing has an object identity a client would
  name or hold. Lecture 8 places request and reply under both, "either RPC or RMI, depending on the paradigm you use", and our
  paradigm is procedures. The older argument about addresses and ports is dropped, because a tarpc stub is tied to one address
  too.
- **Consequences.** tarpc gives "maybe" alone, so at-most-once is our work (D177). The lockfile keeps tarpc's OpenTelemetry
  crates (D119).
- **Where.** `proto.rs::ClientRpc`, `PeerRpc`. The report's "Choice of RPC" section.
- **How we check it.** A reader can find each role in the code from the map above.
- **Report notes.** This table and the two maps are that section. Chapter 2's table (ch2/p20.png) has Java RMI and CORBA and no
  RPC row; Sun XDR comes from Chapters 4 and 5, so cite all three.

### D177 · Invocation semantics claimed per path

- **Date:** 2026-11-03 · **Part:** all · **Status:** Implemented · **Decided by:** the agent on its recommendation, on Noor's
  instruction to finish without more questions (2026-11-03)
- **Context.** Chapter 5's table (lines 184-197) gives three rows: maybe, at-least-once and at-most-once, by whether the
  request is retransmitted, duplicates filtered, and the reply re-sent. The answer differs per path.
- **Options considered.**
  1. *Per path, saying whose work each part is (chosen).*
  2. *At-most-once for everything.* False for peer calls, which do not resend.
  3. *Say tarpc gives at-most-once.* False: tarpc gives maybe, and resends nothing (tested in
     `a_lost_request_ends_at_the_deadline_and_the_channel_lives_on`).
- **Decision.** Claim:
  - **Client to cloud: at-most-once for the committed effect, across all replicas, and our own work.** Retransmit request: the
    retry with the same ID (`client.rs`, D52). Duplicate filtering: the results table by request ID, first commit wins (D27).
    Retransmit reply: any serving node answers from state (D37). Planning can still run twice, on a takeover or a split bid,
    with one result committed.
  - **Peer calls: maybe.** `PeerClient::call` does not resend.
  - **Degraded answers: the labelled exception.** A provisional answer can differ from what commits (D122).
- **Why.** It says what the code does, and the examiner's question has a real answer on each path.
- **Consequences.** None in the code. A UDP path does not change the claim: UDP can duplicate and lose, and the filter and the
  retry are exactly what cover those.
- **Where.** `client.rs::submit_from`, `state.rs`, `node/mod.rs::handle_request`.
- **How we check it.** `tests/client.rs::a_silenced_leader_is_covered_by_a_retry_with_the_same_id`,
  `tests/cluster.rs::a_request_is_committed_before_the_reply_and_retries_get_the_same_result`, FT-7.
- **Report notes.** One small table in the report's "Choice of RPC" section.

### D178 · How we read "For project use UDP", and what UDP really saves

- **Date:** 2026-11-03 · **Part:** all · **Status:** Implemented · **Decided by:** the agent on its recommendation, on Noor's
  instruction to finish without more questions (2026-11-03). Noor's item 9 settles the transport itself
- **Context.** Notes line 66 is "Request reply protocol + UDP", and line 67 is "For project use UDP -->don't go this low
  level, u will use ready made RPC or RMI". This can be read two ways: as "use UDP", or as "UDP is the level not to go down to".
- **Options considered.**
  1. *State both readings in the report, say Noor's item 9 chose UDP, and say what the note's second half asks of it (chosen).*
  2. *Claim the note asks for UDP.* It may not, and a reader can see that.
  3. *Say nothing.* The note raises UDP by name, so a grader may well ask.
- **Decision.** Under the reading "use UDP", this design meets it on the client path only: peers and `ctl` stay on TCP. Under
  the reading "do not go this low", `node/datagram.rs` is what the note warns about, and our answer is that it holds no RPC
  logic: call IDs, deadlines and matching stay tarpc's, and retries stay in the middleware. The slides draw RPC above both UDP
  and TCP (Chapter 4 lines 22-26, Chapter 5 lines 13-22), and Chapter 5 lines 23-38 list what UDP saves.
- **What UDP saves here, honestly.**
  - *Acknowledgements:* saved. Requests are followed by replies, and D52's retry does the rest.
  - *Connection setup:* saved, which was once per client program per node (D53), and with it the pings and reconnects of D112.
  - *Flow control:* saved, and it did nothing for us anyway, because our reader never stops reading. The other side of that is
    D173: no backpressure.
  - *What it costs:* a lost reply costs a whole wait, a sysctl, a firewall rule and about 420 lines of ours.
- **Why.** It keeps the claim to what is true. The rows tie, so nothing here is a performance claim.
- **Consequences.** A literal grader gains nothing over a TCP design on the RPC question, because tarpc is the pick in both.
- **Where.** The report's "Choice of RPC" section.
- **How we check it.** Against the notes, by reading.
- **Report notes.** A short paragraph, with the costs in it.

### D179 · Library boundary: openraft and tarpc

- **Date:** 2026-11-03 · **Part:** all · **Status:** Implemented. Amends D68 and AGENTS.md rule 6, wrong since D119 · **Decided
  by:** the agent on its recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** D68 and AGENTS.md rule 6 say openraft is the only library doing distributed-systems work. tarpc has done remote
  calls since D119, and now carries client calls too. `docs/demo.md` already says so, so the documents disagree.
- **Options considered.**
  1. *Name both, and say where the line falls (chosen).*
  2. *Leave them.* They contradict the demo script, and the spec wants library work separated and credited.
- **Decision.** Libraries doing distributed work: openraft (Raft) and tarpc (stubs, call IDs, matching, deadlines, cancels).
  Ours: the datagram adapter, the group send, retries, duplicate filter, bidding, takeover, commit rule, fault gate, fault
  injection, rejoin, degraded mode and the planner. New plain dependencies: `socket2` (a socket option), and tokio-util's `net`
  feature for `UdpFramed` (reading datagrams). Same tarpc features, with none of TLS, compression, OpenTelemetry export, `Retry`
  or `load_balance`. The default log filter is lane-fixes' D134, the same bytes on this branch so that the merge sees one change:
  on this path it matters on every report, not only between nodes.
- **Why.** Spec lines 377-381: libraries may not hide what is assessed, and their work is separated and credited. Election,
  consistency, recovery and the retry rule are all ours.
- **Consequences.** A new distributed library still needs team agreement.
- **Where.** `Cargo.toml`, the AGENTS.md rule, `docs/demo.md`.
- **How we check it.** `Cargo.toml` against the table.
- **Report notes.** Goes in the library table: tarpc 0.37 (MIT), on both paths.

### D180 · What was left out of the client path, and which tests changed

- **Date:** 2026-11-03 · **Part:** F, C · **Status:** Implemented · **Decided by:** the agent on its recommendation, on Noor's
  instruction to finish without more questions (2026-11-03)
- **Context.** The design was written for both TCP and UDP, and many of its details apply to TCP links. A record of what fell
  away keeps a later reader from looking for it, and any test that changed needs a reason (AGENTS.md).
- **Options considered.** For each: *leave out, because UDP has no link (chosen)*, or carry it anyway for symmetry, which is
  code with nothing to do.
- **Decision.**
  - *Gone with the link:* `run_link`, pings, watch channels, reconnects, `Waiters`, `SEND_QUEUE`, half-open detection,
    `client_link_up` and `client_link_down` from the code path (their names stay, D172), and the link-down reasons. A node
    that comes back just answers.
  - *Not built here:* server push. Publish-subscribe is out of Phase 1 by decisions item 14.
  - *Tests removed, with the reason:* `a_connection_that_stops_answering_pings_is_replaced` and
    `a_silenced_node_keeps_its_connection_because_pings_pass_the_gate`: there are no connections and no pings to test.
  - *Tests changed with the same assertions, against real `ClientRpc` stubs:* `gives_up_with_an_error_after_the_retry_budget`,
    `a_retry_keeps_the_request_id_and_increments_the_attempt`, `each_retry_states_a_larger_age`,
    `an_expired_reply_ends_the_wait_at_once`. `reconnects_when_a_node_comes_back_on_the_same_address` becomes
    `a_node_that_comes_back_on_the_same_address_just_answers`, and now also asserts more than one attempt, because the first
    send provably went into nothing.
  - *Tests left alone:* every other real-node test in `tests/client.rs`, and every suite that uses `first_reply`, whose
    signature stays. Its body sends over UDP with no retry, so a test that needs a copy to survive a full buffer should go
    through `MeshClient`.
  - *Changed on purpose from the design:* see "What changed from the design of the client path", after this entry.
- **Why.** Fewer moving parts is the main thing UDP buys.
- **Consequences.** `proto::tests::client_frames_round_trip` becomes `client_requests_round_trip`, since the frames are gone.
- **Where.** `tests/client.rs`, `tests/common/mod.rs`, `tests/durable.rs`, `tests/datagram.rs` (the spike's `udp_spike.rs`,
  carried over, its fixed sleeps replaced by waits on a condition, as AGENTS.md asks).
- **How we check it.** `cargo test`; 10 runs in a row on the timing suites (see the report that came with this branch).
- **Report notes.** None beyond D167.

### What changed from the design of the client path (gap 11, option B)

- The node's sink never waits for send readiness, and neither `poll_ready` nor `poll_flush` registers a waker (D170). The
  spike's did, which could lose a wake-up when several channels share one socket.
- A request's huge deadline is handled: `catch_unwind` around decoding, and a limit in the reader (D170). Not in the design or
  the spike.
- An answer must carry the submit's request ID (D169). Not in the design.
- `ctl status`'s check is a line per node under the table, and retries 3 times within 2 s (D168). The design said "udp ok or no
  answer" and left the form open.
- `scripts/run-experiments.sh` stops the batch on `udp no answer` (D168). Not in the design.
- The buffer size comes from arithmetic, because no row's peak queue can give it (D171).
- `Event::UdpSocket` is new, and the analysis warns on a capped buffer or any drops (D171, D172).
- The runner's `ctl silence` retry and the log filter belong to lane-fixes' D139 and D134. The filter is copied byte for byte so
  the merge sees one change, and the retry is left to that lane.
- The design's `replies_alone_keep_a_link_up` has no meaning without a link, and was not written. `a_call_error_does_not_end_a_submit`
  became `a_dead_node_does_not_end_a_submit`, the UDP form of the same trap.


### Rehearsal rows for D167 to D180: one machine, never six-PC results

Three real `node` programs and 30 real `loadgen` programs, in a private network namespace, pinned to CPUs 0-23 shared with
other jobs, on one machine, on loopback. The `w-*` rows ran on the tree just before commit 4ae4b44, which differs from the final
code only by moving the send loop into `group_send`, a cap on the `ctl` check and one dead error variant. `e1-1000-bidding`'s settings
(`plan_work` 1000, 300 responders), no storage unless named, 30 s with a 10 s warm-up. Think time 1 s. Users start
spread over 1 s by starting the programs 33 ms apart, except `burst`, where all programs start at once. **`burst` here
is not the spike's tick burst:** this branch's `loadgen` has no such pattern yet, and users drift apart after their
first answer. Every row ran with `rcvbuf_got_bytes` 425,984, because `net.core.rmem_max` was left at 212,992.

| Row | Users | Reports a second | p50 ms | p95 ms | p99 ms | Gave up | Retried | Requests sent | Cancels sent | Cancels read | Datagrams dropped by the kernel | Dropped by the gate | Peak MB per node | Last term | Busy % of the 24 CPUs |
|-----|-------|------------------|--------|--------|--------|---------|---------|---------------|--------------|--------------|---------------------------------|---------------------|------------------|-----------|----------------------|
| `w-steady-300` | 300 | 300.0 | 24.51 | 29.58 | 38.31 | 0 | 0 | 27000 | 18000 | 18000 | 0 | 0 | 19 | 2 | 27.3 |
| `w-steady-1000-a` | 990 | 975.5 | 24.47 | 32.44 | 226.58 | 0 | 22 | 88269 | 58884 | 58884 | 0 | 0 | 41 | 2 | 47.4 |
| `w-steady-1000-b` | 990 | 978.5 | 24.34 | 30.7 | 225.5 | 0 | 15 | 88422 | 58984 | 58984 | 0 | 0 | 41 | 2 | 34.9 |
| `w-burst-1000` | 990 | 951.2 | 24.47 | 32.49 | 227.31 | 0 | 24 | 86001 | 57724 | 57724 | 0 | 0 | 46 | 2 | 40.4 |
| `w-steady-3000` | 3000 | 1010.6 | 227.35 | 7375.36 | 17974.67 | 10 | 8625 | 168957 | 134771 | 134771 | 0 | 0 | 79 | 2 | 53.9 |
| `w-burst-3000` | 3000 | 781.6 | 1674.95 | 7026.4 | 12989.94 | 111 | 8210 | 139344 | 114692 | 114686 | 5669 | 0 | 71 | 2 | 61.2 |
| `w-silence-leader-1000` | 990 | 769.5 | 25.8 | 2087.3 | 5950.86 | 0 | 1443 | 80937 | 55676 | 55676 | 0 | 3732 | 52 | 3 | 52.8 |
| `w-kill-follower-1000` | 990 | 907.1 | 24.66 | 227.79 | 2143.99 | 0 | 527 | 85605 | 57648 | 44907 | 0 | 0 | 41 | 2 | 61.0 |
| `w-steady-1000-storage` | 990 | 0.1 | 31086.55 | 31086.55 | 31086.55 | 1251 | 1 | 22551 | 22548 | 22548 | 0 | 0 | 86 | 98 | 22.4 |

Reading them:

- **The chosen deadline holds up.** `w-steady-1000-a` and `-b` match the spike's best `n-silent` (967.6 reports a second, p99
  225.5 ms) with one 36 s deadline per submit, which no spike row used.
- **Every cancel arrived when nothing was dropped.** `w-steady-300`: 27,000 requests, 18,000 cancels, 9,000 replies, so exactly
  3 and 2 per answer.
- **`w-steady-3000` is the cluster's own ceiling,** about 1,000 reports a second here, not the transport: nothing was dropped.
  Its p95 and its 10 give-ups are users waiting for a commit.
- **`w-burst-3000` shows the capped buffer.** 5,669 copies dropped (about 4%), term stays 2, 111 give-ups. This is the row the
  sysctl is for, and it has not been rerun with the sysctl raised (D171).
- **`w-silence-leader-1000`:** 5 s of silence: the gate dropped 3,732 reports, there was one new term, nobody gave up, and all
  digests matched at the end.
- **`w-kill-follower-1000`:** `kill -9` a follower under load. Nothing to detect on the client path, and nobody gave up. p99 2,144 ms
  most likely comes from reports that node had won. Its own log stops, so `Cancels read` is short by what it never read.
- **`w-steady-1000-storage` collapsed** to term 98 and 0.1 reports a second. This is the known disk problem, one sync per log
  entry (Noor's item 10, the stable-leader work, not on this branch), which the design file also saw on TCP at 300 users. It is
  not a result about UDP, and this branch does not fix it. Lower load with storage on is fine: see the power cut below.

Real packet loss, by `iptables -m statistic` in the namespace, on UDP only and so on the client path only, both directions. 1000
users, 45 s, no warm-up in these figures, on the final code:

| UDP loss | Reports a second | p50 ms | p95 ms | p99 ms | Gave up | Retried | Peak MB per node |
|----------|------------------|--------|--------|--------|---------|---------|------------------|
| 0% | 965.6 | 24.3 | 30.7 | 225.7 | 0 | 56 | 51 |
| 1% | 948.0 | 24.6 | 34.6 | 1721.7 | 0 | 494 | 54 |
| 5% | 885.6 | 24.6 | 1620.9 | 2241.9 | 0 | 2047 | 52 |
| 20% | 662.0 | 25.5 | 2268.9 | 5683.0 | 0 | 6540 | 49 |

Nobody gave up at 20%, and all digests matched in every row. This is what D52's retry is for. Retries track the reply loss, as D173
says: at 5% loss, 5.1% of reports retried. **Memory does not show the parked calls that D173 feared:** at 5% loss about 90 cancels
a second are lost, so about 3,200 calls sit parked at any time across the three nodes, by arithmetic, and peak memory is the same as
with no loss. `analyze.py` on the real 5% run gave `udp_requests_per_req` 3.17, `udp_cancels_per_req` 2.13,
`udp_datagrams_per_req` 6.44, `udp_request_loss_share` 5.5%, `udp_cancel_loss_share` 5.4%, and printed its capped-buffer warning.

A real firewall rule, by `iptables` in the namespace, on node 3's UDP port: `ctl status` showed that node's row as healthy and printed
`node 3: udp no answer`. Load then ran on nodes 1 and 2 with 0 retries and 0 give-ups, node 3 read only the 4 status checks, and it
still applied all 480 results through Raft. Removing the rule gave `udp ok`. A silenced node said `udp ok`.

A power cut with storage on, 15 reports a second: `kill -9` node 2, then start it again on its address. 346 answered, 0 give-ups, 0
retried. It logged `restarted` then `rejoined` with `recovery_us` 14,127, and the digests matched at the end.

**None of this is a six-PC result, and none of it may be reported as one.** "This branch" in these rows means `lane-wire` before it
merged: it had neither the proposal queue (D133) nor the load patterns (D182). The merged branch has both.


---

## 16. Real user programs on the client PCs (November 2026)

Rehearsal numbers in this section come from the pretend lab: one WSL kernel, one disk and one clock. They explain why a
choice was made and must never be reported as six-PC results.

### D181 · User processes: 10 real programs on each client PC share the users, and the number is a setting

- **Date:** 2026-11-03 · **Part:** F · **Status:** Implemented, amends D53 and D54 · **Decided by:** Noor Hany Emam
  (the design and the number 10, decisions item 3); the name of the setting and the sharing rule by the agent on its
  recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** The spec says three machines each run "many simulated user processes", that adding them raises the
  load with no change to the application, and that each user process represents a campus participant. It also says
  one frontend may control many background client processes, and asks us to document the client parallelization
  model we adopt. Until now the system ran one `loadgen` program per client PC, with every user a tokio task inside
  it (D53, D54). So the process list on a client PC showed one program however many users it carried, and a grader
  who looks would see that "processes" had only been renamed. The same model also meant 1000 users on a PC shared one
  send queue per node and one runtime, and sent their first requests within a fraction of a millisecond (D182).
- **Options considered.**
  1. *Noor's design: one place to say how many users and which pattern; behind it, 10 real background programs share
     the users, so 1000 users are 10 programs with 100 users each.* Real processes in the process list, each can be
     killed on its own, gentle on the PC, and it is the only count near what has actually been run (30 programs over
     three PCs in the rehearsals). In most cells one process is not one participant, which the report must state.
  2. *1000 real programs, one per user.* Matches "each user process represents a campus participant" word for word.
     Rejected by Noor: 1000 programs and, on TCP, 3000 connections on each client PC, and the client PC chokes first.
     Nothing above 30 programs has ever been run, so the cost is unmeasured either way.
  3. *1 program with many light users, as today.* Cheapest, nothing changes. Rejected by Noor: it stays a gap against
     "processes".
  4. *A setting that starts at 100 programs and moves to 1000 if a 6-minute check passes (the agent's own design
     recommendation).* It would make every cell up to 300 users exact, and every cell if 1000 passes. Not taken: it
     changes the number Noor fixed and could end at what Noor rejected. The code keeps the door open, because the
     number is a setting, but nothing depends on it.
- **Decision.** Option 1.
  - A *program* is one real operating-system `loadgen` process. Its users are tokio tasks that share one `MeshClient`.
  - The number of programs per client PC is a setting whose default is **10**: `--programs` on `scripts/start-users.sh`,
    `PROGRAMS_PER_PC` in the environment, and `programs=` on a cell's run line. No TOML key and no `config.rs` key,
    because nodes never need it and changing it needs no rebuild. `run.json` records the value that made each figure.
  - A PC with N users runs P = min(N, programs) programs. Each carries N/P users, plus one for the first N mod P
    programs, so the total is always exactly N and no program is empty. 1000 users are 10 x 100. 34 users are
    4, 4, 4, 4, 3, 3, 3, 3, 3, 3. 3 users are 3 x 1.
  - A failed program is not restarted (D183).
- **Why.** Noor's words: real background programs behind one box, the load kept large. The agent's reading, for the
  report: "user process" means an operating-system process that holds one or more simulated users. The spec's line
  on the client model lets us choose a model if we document it, and its frontend line describes exactly one frontend
  controlling many background processes. Other values of the setting cost nothing to keep, and only the control cell
  in D187 uses one.
- **Consequences.**
  - One program is one participant only when a PC has 10 users or fewer: of today's cells, the 30-user ones. The report
    must say so plainly and first, not leave it for a reader to find.
  - The number of programs stays at 30 across all of E1 once a cell has 30 users or more, so E1 varies users, not
    processes. "Throughput as clients or processes grow" must be read as users, and the figure should say so.
  - 100 users share one send queue per node and one runtime, so the generator can shape results at the top of the
    sweep. `e7-p1-300` and `machine_usage` (D185, D187) are there to show how much.
  - The web page's box must not read "1000 user processes" when the process list shows 10. Noor's item 3 words the box
    as "how many user processes"; the page should say "Simulated users", with a line below saying how they run. The
    script's `plan` line gives the numbers for that line (`programs=` and `users_each=`). This is the page's decision
    to record, and Noor can overrule it.
  - On TCP, each node holds 30 client connections and answers 30 pings a second in every cell (D112). A transport
    without per-client connections removes both.
- **Where.** `scripts/start-users.sh`, `scripts/run-experiments.sh`, `src/bin/loadgen.rs`, `experiments/e7-p1-300.toml`.
- **How we check it.** `tests/user_processes.rs`: `the_default_is_ten_programs_sharing_the_users`,
  `the_number_of_programs_is_a_setting`, `dry_run_ids_are_unique_and_users_sum_exactly` (levels 30 to 3000 at limits 1
  to 1000). On the lab: `pgrep -c -x loadgen` on a client PC prints 10 during a run.
- **Report notes.** State the model first, as a reading: real programs per PC, tokio tasks inside, one program per
  participant only at the smallest load, 10 programs per PC above it, chosen by Noor. Do not claim the client PC would
  choke at 1000 programs unless `machine_usage` or a real run shows it: that reason has no data behind it yet.

### D182 · Load patterns (steady, random, all-at-once) and spreading the first sends

- **Date:** 2026-11-03 · **Part:** F · **Status:** Implemented, amends D54, qualifies D129, amended by D191 · **Decided by:** Noor Hany
  Emam (the three patterns, decisions item 4); the mechanics, the start spread and the choice not to re-space steady
  after a stall by the agent on its recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** Noor asked for a load pattern option: steady (everyone waits exactly 1 s, what experiments use), random
  (each user waits a random 0 to 2 s), all-at-once (every user sends at the same moment, for spikes). Separately, the
  rehearsal in `normal-load/why.txt` shows how the worst collapse starts: each client's first 100 requests left within
  0.1 ms, all 300 first requests left within 80.4 ms, and the leader then stayed busy long enough for a follower to
  start an election less than a second later. So the very first wave needs fixing whatever else happens. Class notes
  of Sept 9 ask us to document the workloads we test under.
- **Options considered.**
  - *The patterns.*
    1. `--pattern steady|random|all-at-once` on `loadgen`, in the closed loop. Exactly Noor's three.
    2. Steady and random only. Cheaper, but Noor asked for spikes.
    3. All-at-once as "no start spread" only, with no alignment. Users would drift apart after one round, so only the
       first moment would be a spike.
    4. An all-at-once experiment cell. Rejected: all-at-once open loop at 3000 users ended with two nodes unreachable in
       a rehearsal on shared CPUs, and a cell like that could abort a whole series.
  - *Spreading first sends.*
    1. Program k starts k*T/N after the script starts, and user j of U inside it first sends j*T/U later, with T the
       think time and N the users on that PC. Every user on a PC gets its own slot, (k + j*P)*T/N, whatever P is.
    2. k*T/P plus an offset per PC. The offset is a millisecond or so, and SSH start times swamp it.
    3. A `--start-delay-ms` flag on `loadgen`. More Rust for what one `sleep` does in the script, and the wait would
       fall inside the program's own clock.
    4. No spread, as before. 100 users in one program fire within a few milliseconds, which is what the rehearsal
       shows going wrong.
  - *Steady after a stall.*
    1. Wait exactly T after each reply, as Noor's item says and D54 did.
    2. Wait for the user's next slot, phase + n*T. It removes the think time once latency passes T, so overloaded cells
       would offer more load, N/max(R,T) instead of N/(R+T), and it contradicts item 4.
    3. Add a little jitter to each wait. Also contradicts "exactly".
- **Decision.**
  - `loadgen --pattern steady|random|all-at-once`, default steady, closed loop only: naming a pattern with `--rate` is
    refused.
  - *steady:* the first send waits j*T/U, then every wait is exactly T after the reply.
  - *random:* every wait, the first included, is uniform from 0 to 2T, so the mean load matches steady. With T = 1 s
    that is Noor's 0 to 2 s. The draw comes from a second seeded generator, so the incident stream a seed repeats does
    not change.
  - *all-at-once:* every wait runs to the next wall-clock multiple of T, the first included, with no start spread in
    the script. Exactly on a multiple means the next one. The wall clock is what lines up programs and machines, as
    far as NTP agrees.
  - random and all-at-once refuse T = 0, in `loadgen` and in the script: they would have nothing to randomise or to
    line up on.
  - No experiment cell uses random or all-at-once. They are for the demo, the web page and the burst test if someone
    runs it. A cell can name one with `pattern=`.
  - A user's wait never runs past the end of the run, so a long wait cannot make a program outlive its duration.
- **Why.** The spread comes from arithmetic: when users divide evenly among programs, users on one PC never share a
  slot, so changing the number of programs does not change the arrivals, and the setting in D181 stays honest. Steady
  stays exact because that is what Noor decided and because a slot wait would quietly raise the offered load in the
  overload cells.
- **Consequences.**
  - Arrivals change in every cell, which is the point, so all final data must come after this. Warm-up per client in
    `analysis/analyze.py` shifts by up to one think time.
  - The spread does not stop the collapse by itself. It removes the first wave. The later collapse comes from the
    leader's disk saves, which is decision 10 and another change. In the scripts trial (D186: one shared machine, storage
    on, not a result), 300 users with one program per machine had a perfectly even first second and the cluster still
    went through many elections. Clumps clearly hurt, but the spread's own effect has not been measured on the six PCs.
  - Steady can bunch up again after a stall, because every user waits exactly T after its reply. On the dev PC a first
    burst at 1000 users took about 8 s to even out (rehearsal). `arrival_peak_100ms_share_after_fault` shows this
    instead of hiding it.
  - All-at-once really is all at once: at 3000 users it asks the cloud for 3000 plans in one moment, every T. Keep it
    small in the demo.
- **Where.** `src/bin/loadgen.rs` (`Pattern`, `Pacer`, `pause`), `scripts/start-users.sh` (`program_values`).
- **How we check it.** Unit tests in `src/bin/loadgen.rs`: `steady_first_waits_are_even_and_zero_for_one_user`,
  `steady_waits_exactly_the_think_time`, `random_waits_stay_within_twice_the_think_time_and_repeat_for_a_seed`,
  `random_waits_leave_the_incident_stream_unchanged`, `all_at_once_waits_land_on_the_next_multiple_of_the_think_time`,
  `a_pattern_that_needs_a_think_time_refuses_zero`, `a_pattern_cannot_be_named_with_an_open_loop`. In
  `tests/user_processes.rs`: `dry_run_gives_every_user_its_own_slot`, `steady_spreads_the_first_sends_over_the_think_time`,
  `all_at_once_users_send_on_a_multiple_of_the_think_time`, `every_pattern_gets_answers_from_a_real_cluster`. On the
  lab: `arrival_peak_100ms_share_first_s` near 0.1 under steady.
- **Report notes.** Name the three patterns as the workloads, say experiments use steady with a 1 s think time, and say
  why the first sends are spread, citing the rehearsal only as the reason, never as a result.

### D183 · `scripts/start-users.sh`: one way to start and stop users, for the runner, the demo and the web page

- **Date:** 2026-11-03 · **Part:** F · **Status:** Implemented, amends D130, qualifies D132, amended by D190 · **Decided by:** the agent
  on its recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** Something has to start 10 programs on each client PC, hand each its ID and its share of users, spread
  their starts and stop them all. The experiment runner, the demo and the web page all need that. Before this, `loadgen`
  also lost its summary and the end of its log on TERM, so a demo or page stopped early showed nothing behind it.
- **Options considered.**
  1. *A bash script, `scripts/start-users.sh`, called once per PC.* One path for all three uses, no rebuild to fix it,
     and it can be read and run by hand. Bash has sharp edges around signals, which the tests pin down.
  2. *A supervisor inside `loadgen`, in Rust.* Well tested, but a program that starts copies of itself is more code for
     the same job, and every fix needs a rebuild on six PCs.
  3. *A loop inside the runner's SSH string.* No new file, but the demo and the page would each need their own copy.
  4. *One SSH per program, `pkill` to stop.* Many sessions, and `pkill` by name would also kill a second batch, or
     anything else called `loadgen`.
  - For the stop on a program: *TERM prints the summary and flushes* against *no signal handling, sum whatever is on
    disk*. The second leaves a page with no totals.
  - For failures: *no restart, exit 1 if any failed and 2 if more than a tenth failed* against *stop on any failure*
    (one dead program ends an hour's series) and *restart* (hides the failure and skews the load, and `loadgen` only
    fails on config or log errors, which repeat).
  - For threads: *`TOKIO_WORKER_THREADS` = max(1, cores/P) per program* against *the default* (one worker per core in
    each of 10 programs) and *a current-thread runtime* (a Rust change for a small saving).
- **Decision.** Option 1, called as `bash scripts/start-users.sh`.
  - Flags: `--config`, `--machine M` (1 to 9), `--users N`, `--batch B` (0 to 9, default 0), `--programs P` (default 10
    or `PROGRAMS_PER_PC`), `--pattern`, `--think-ms` (default 1000), `--duration-s` (default 60), `--warmup-s` (default
    0), `--seed-base`, `--out DIR` (default `runs/users`), `--dry-run`, `--stop`. `BIN` and `PID_DIR` come from the
    environment. Paths are from the repo root.
  - It stays in the foreground until the programs end. The first line of output is a `plan` line and the last two are
    `started P, failed F` and `reported k of P programs: done, gave up, retried, no responder, provisional`, added up
    from each program's summary. Percentiles come only from `analyze.py`.
  - All programs on a PC and batch write to one file, `users-M-B.out`, emptied at each start, which saves files.
    `loadgen` prints its summary in one write, so blocks from different programs do not mix.
  - **To stop it:** send the script TERM, or Ctrl-C, or run `--stop --machine M --batch B` from anywhere. Either way the
    programs get TERM, print their summaries and flush their logs, and exit 0. A program still stuck 5 s later gets
    KILL and counts as failed. The traps cover TERM, INT and HUP, and EXIT catches a `set -e` abort.
  - It writes its own PID to `$PID_DIR/users-M-B.pid` (default `runs-config/`, which `stop_all` already walks), removes
    it on the way out, and refuses to start if that PID is alive and really is this script. A file left by a reboot
    does not block it.
  - Exit status: 0 all fine, 1 some program failed, 2 more than 10 percent failed or it refused to start.
  - `exec` inside the background subshell keeps each program's PID. `--dry-run` prints the plan and starts nothing.
- **For the web page.** `bash scripts/start-users.sh --config <file> --machine M --batch 9 --users N --pattern P
  --duration-s S`, run as a child; read the `plan` line for "running as P programs x U users each" and the last lines
  for totals; TERM the child or run `--stop --machine M --batch 9` to stop. The page's machine number is its own client
  ID minus 10 (frontends are 11 to 13, D184).
- **Why.** One fan-out to test. The script is independent of the transport: it knows nothing about TCP or UDP. A tenth
  is a judgement: one of 10 programs lost is a small dent worth a warning, and two lost is a run to throw away and a
  sign of something the next run would repeat.
- **Consequences.**
  - A long-lived script over SSH is new. It has run on one machine only; the runner's SSH path is still untested (D130),
    and `on_host` with `local` cannot show SSH problems. First SSH run by hand on the lab: `ssh` returns when the script
    ends, `pgrep` counts match, and TERM from `stop_all` leaves nothing.
  - A program killed while it still waits for its start time prints nothing. That is under one think time at the very
    start of a run, is not counted as a failure, and shows as `reported k of P`.
  - A person who needs a second group of users on a PC must give it another `--batch`.
- **Where.** `scripts/start-users.sh`, `src/bin/loadgen.rs` (`main`, `summary`).
- **How we check it.** `tests/user_processes.rs`: `a_real_start_gives_one_log_per_program`,
  `term_leaves_no_child_no_pid_file_and_a_summary_from_each_program`,
  `a_second_start_is_refused_while_the_first_lives_and_stop_ends_it`, `a_stale_pid_file_does_not_block_a_start`,
  `a_dead_program_counts_as_failed_and_the_rest_keep_going`, `more_than_a_tenth_failing_exits_with_two`,
  `dry_run_refuses_what_would_collide_or_mean_nothing`; `the_summary_keeps_the_wording_the_start_script_adds_up` in
  `src/bin/loadgen.rs`.
- **Report notes.** One sentence is enough: one script starts and stops the background client programs, which is what
  the spec's "single frontend controlling many background client processes" needs behind the page.

### D184 · Client IDs and seeds when one PC runs many programs

- **Date:** 2026-11-03 · **Part:** F · **Status:** Implemented, amends D51 and D113 · **Decided by:** the agent on its
  recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** A request ID is `(client, seq)`, and one client ID also names one log file, so two programs must never
  share an ID (D113). Until now the hosts file's client number 1, 2 or 3 was the program's ID. With 10 programs per PC
  and more than one batch that no longer works, and the web page and zone feed have IDs of their own to protect. The
  old seed, run*100 + client ID, would also repeat across runs once IDs pass 100.
- **Options considered.**
  - *IDs.*
    1. machine*10000 + batch*1000 + k, refusing batch above 9 and k above 999. The number shows PC, batch and program.
    2. machine*1000 + k. Shorter, but no room for batches, so the page's programs would collide with the runner's.
    3. Random IDs. D51 and D113 already rejected them: runs are hard to match to machines, and they can collide.
    4. A new column in the hosts file. More to fill in, for what the machine number already gives.
  - *Seeds.*
    1. run*1000000 + client ID. Every program differs, and no seed repeats across runs while IDs stay below a million.
    2. Keep run*100 + client ID. Repeats across runs.
    3. One seed for all programs. One-user programs would then send identical incidents.
- **Decision.** The ID map, for everything that can talk to the cloud:

  | IDs | Who |
  |-----|-----|
  | 1 to 9 | `loadgen` by hand |
  | 11 to 13 | frontends (the web page), one per client PC |
  | 21 to 23 | zone feed programs |
  | 10000 to 99999 | load programs from `scripts/start-users.sh`: machine*10000 + batch*1000 + k |

  The script refuses a machine outside 1 to 9, a batch above 9 and more than 1000 programs, each of which would break
  the digit pattern (machine 1 batch 10 would equal machine 2 batch 0, and k 1000 would equal batch 1 k 0). The client
  number in the hosts file is now the machine number. Each program's seed is seed base + its ID, and the runner passes
  run*1000000 as the base.
- **Why.** It cannot collide, and a person reading a log name knows at once which PC and which program it came from.
  Five digits stay clear of the small IDs by construction, because the machine digit is never 0. Batch 9 is the
  web page's by convention, and the runner uses batch 0.
- **Consequences.** Logs are now `client-10000.jsonl` to `client-30009.jsonl`. `loadgen` itself still takes any
  `--client-id`, so the map for hand runs is a rule for people, stated in its help text. A hosts file with a client
  numbered above 9 is refused, with a reason, at the first run.
- **Where.** `scripts/start-users.sh` (`program_values`), `scripts/run-experiments.sh`, `scripts/lab-common.sh`,
  `experiments/hosts.example`, `src/bin/loadgen.rs` (help text).
- **How we check it.** `tests/user_processes.rs`: `dry_run_ids_are_unique_and_users_sum_exactly`,
  `the_id_shows_the_machine_the_batch_and_the_program`, `dry_run_refuses_what_would_collide_or_mean_nothing`.
- **Report notes.** None needed beyond the client model; keep the table in the architecture document.

### D185 · `machine_usage`: the client PCs' own CPU and memory

- **Date:** 2026-11-03 · **Part:** F · **Status:** Implemented, amends D128, amended by D191 · **Decided by:** the agent on its
  recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** A saturated client PC reads as cloud latency, and nothing measures the client PCs: `resource_usage`
  (D128) comes from nodes only. The reason for 10 programs ("the client PC chokes first") has no data behind it either.
  The spec asks for CPU and memory where they matter to a design choice. And `tests/layers.rs`, from another item,
  forbids `rescuemesh::node` in `src/bin/loadgen.rs`, so `loadgen` cannot call `src/node/usage.rs`.
- **Options considered.**
  - *How to measure.*
    1. A new event, once a second, from program k=0 of batch 0 on each PC. One log format, no extra process, nothing
       that can hold an SSH session open or outlive the run.
    2. A bash sampler writing `machine-<id>.jsonl`. A second log format, which D128 rejected, and a process that can be
       left behind.
    3. `/proc/loadavg` saved once per run. Cheap, but one number cannot say when the PC was busy.
    4. Nothing, as D128 has it. A client limit would then pass for cloud latency.
  - *Where the parsers live.*
    1. In `src/metrics.rs`, beside the event, public to every program. Simple, and `loadgen` already imports `metrics`.
    2. In `src/node/usage.rs`, with a re-export through `src/middleware.rs`. Depends on another item's file, and pretends
       a client-side reading is a node matter.
    3. In `src/bin/loadgen.rs`. Fine for now, but the event's parsers would sit far from the event.
- **Decision.** Option 1 both times. Event `machine_usage { cpu_busy_ticks, cpu_total_ticks, mem_available_kb }`, each
  optional, from `metrics::machine_usage()`. `/proc/stat`'s first line gives the ticks: total is user, nice, system,
  idle, iowait, irq, softirq and steal; busy is total minus idle and iowait, because a CPU waiting on the disk has nothing
  to do. `/proc/meminfo` gives MemAvailable. `loadgen --sample-machine` turns it on; the script passes it to program 0 of
  batch 0. The reads run in `spawn_blocking`, and the task's handle is owned and aborted at the end.
- **Why.** Same log, same analysis path, and a sampler that dies with its program. Program 0 of batch 0 is the one
  program every experiment on a PC has.
- **Consequences.** Ticks are per machine, so anything else on the PC counts: keep the client PCs quiet. If program 0
  dies, that PC's samples stop, and the run is left out anyway (D186). A PC running only a web page's batch 9 has no
  samples, which is fine, because that is not an experiment.
- **Where.** `src/metrics.rs` (`Event::MachineUsage`, `machine_usage`, `parse_machine_cpu`, `parse_mem_available_kb`),
  `src/bin/loadgen.rs` (`sample_machine`), `analysis/analyze.py` (`client_pc_metrics`).
- **How we check it.** `metrics::tests::machine_cpu_comes_from_the_first_stat_line`,
  `mem_available_comes_from_meminfo`, `machine_usage_keeps_its_field_names`;
  `a_real_start_gives_one_log_per_program` checks real values come out and only from one program;
  `test_machine_usage_gives_cpu_p95_and_memory_min` in `analysis/test_analyze.py`.
- **Report notes.** Report `client_pc_cpu_busy_p95` and `client_pc_mem_available_min_mb` beside E1, as the evidence that
  the client PCs were not the limit, or that they were.

### D186 · Runner checks: exact shares, leftovers, old run folders, and runs left out of the summary

- **Date:** 2026-11-03 · **Part:** F, all · **Status:** Implemented, amends D130, qualifies D112, amended by D189 and D191 · **Decided by:** the
  agent on its recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** Reading the runner against many programs per PC found several ways for a run to be wrong with no sign:
  - `users / machines` rounded 100 users down to 99 per cell, so `e1-100` was not 100 users.
  - A program left over from an earlier run keeps sending while its log is unlinked, adding load nobody counts.
  - Logs are copied by merging, so a rerun with fewer programs keeps the old run's client logs in the folder and the
    throughput adds them up.
  - A program that died simply had no log, and a fault that never fired (`ctl` times out, and no error shows) just had no
    phases. Both were then averaged in with good runs.
- **Options considered.**
  - *Sharing users.* Share out with the remainder, as 34, 33, 33; or round down as before; or shorten the think time to
    make up the difference, which changes D129's one request a second per user.
  - *Leftovers.* One function that looks for `loadgen`, `node`, `frontend` and `zonefeed` by path on every machine, and
    refuses; or `pgrep -x loadgen` on client PCs only, which misses the page and zone feed and catches other jobs called
    `loadgen`; or kill them, which a runner should not do to programs it did not start; or nothing.
  - *Old folders.* Move to `runs/old/`, keeping the data; or delete, which loses it; or nothing.
  - *Bad runs.* Name them on screen and leave them out of the summary; or only warn, so a bad number still reaches the
    report; or stop the series at the first bad program, which costs an hour for one dead program.
  - *A file-limit check on nodes (raise the limit, read it back, abort under 4096).* Needed at 150 or more programs per
    PC over TCP. Dropped: 10 programs per PC is 30 connections per node, far below any default limit.
- **Decision.**
  - The runner gives machine i its share of the cell's users, users/machines plus one for the first remainder machines.
    A machine whose share is 0 runs nothing.
  - `refuse_leftovers` runs before every run and looks on all six machines, by path, for `$BIN/` `loadgen`, `node`,
    `frontend` and `zonefeed`. It allows 10 s for things still stopping, then exits with the list. It never kills.
    The same one function serves items 14 and 17, so neither needs its own guard.
  - An existing run folder moves to `runs/old/<cell>-run-<n>-<time>/`, where `analysis/analyze.py` does not look.
  - `run.json` gains `users`, `programs_per_pc`, `client_programs_expected`, `pattern`, `think_ms` and `victim`.
  - The runner waits on each machine's script. Exit 1 gives a warning and goes on. Exit 2 or more, which includes `ssh`'s
    255 for an unreachable machine, finishes this run's log copy and stops the series.
  - `analysis/analyze.py` leaves a run out if `client_programs` differs from `client_programs_expected`, or if the cell has
    a victim and no node logged `silenced`, and prints which run and why. A `run.json` without these keys skips the checks,
    so older runs still read.
- **Why.** Each of these failures gives a wrong figure with nothing to show for it. Refusing, moving and naming are all
  cheap, and none of them changes a good run. Matching by path is what keeps other programs called `node` from stopping a
  run, as VS Code's would.
- **Consequences.** A series can stop itself, so check on it. A cell whose run is left out has fewer runs behind its
  interval, shown in the `runs` column of `summary.csv`, and needs a rerun. The guard costs six quick SSH calls per run.
- **Where.** `scripts/run-experiments.sh` (`refuse_leftovers`, `run_once`), `scripts/lab-common.sh` (`users_on_machine`),
  `analysis/analyze.py` (`run_problem`, `summarise`).
- **How we check it.** `the_runner_shares_a_cells_users_exactly` in `tests/user_processes.rs`.
  `test_a_run_missing_programs_is_left_out_of_the_summary`, `test_a_victim_with_no_silenced_event_is_left_out`,
  `test_a_run_json_without_the_new_keys_skips_the_checks` in `analysis/test_analyze.py`. The whole runner on one machine
  with `hosts.local`, in a private network namespace, with short runs: see the trial record below. That proves the scripts
  only. **It has still not run over SSH or on the lab machines.**
- **Report notes.** The methods section can say that a run with missing programs or a fault that did not fire is left out
  and rerun, and how many were.

### D187 · One control cell, no limit check, and what the analysis adds

- **Date:** 2026-11-03 · **Part:** F, all · **Status:** Implemented, qualifies D63 and D129 · **Decided by:** Noor Hany
  Emam fixed the number at 10, which ends the limit check; the rest by the agent on its recommendation, on Noor's
  instruction to finish without more questions (2026-11-03)
- **Context.** The agent's design for this item planned cells `e7-cap-100`, `e7-cap-500` and `e7-cap-1000` to settle how
  many programs a PC can carry, and proposed a limit of 100. Noor fixed the number at 10, so nothing is left for those
  cells to settle. But 10 itself has no data behind it: nobody has compared it with anything.
- **Options considered.**
  - *Cells.*
    1. `e7-p1-300` only: 300 users with one program per PC, to compare with `e1-300-bidding` at 10 per PC.
    2. The `e7-cap-*` cells as well. Their question is closed, and with storage on they sit above the rehearsal ceiling,
       where identical runs differ widely, so they would decide nothing.
    3. `e7-p1-3000` and `e7-spike-3000`. Both sit at the retry wall, where noise hides any process effect, and a spike cell
       could end a series.
    4. No cell. Then the report has no way to say what 10 programs changed.
  - *Analysis.*
    1. Add the small metrics the runner's checks and the patterns need.
    2. Also `node_accept_failed` from `node-*.out`, and users and programs on E1's tick labels. Accept failures only arise
       near a file limit, which 30 connections cannot reach, and the text goes away with the TCP client path. The tick
       labels would say 30 programs on almost every point.
- **Decision.** One new cell, `e7-p1-300`, in the full profile only. Quick stays 13 cells, and full goes from 31 to 32. New
  run-line keys: `programs=` and `pattern=`. New metrics: `client_programs`, `users_per_program`, `client_link_down`
  and `client_link_down_<reason>`, `arrival_peak_100ms_share`, `arrival_peak_100ms_share_first_s`,
  `arrival_peak_100ms_share_after_fault`, `client_pc_cpu_busy_p95`, `client_pc_mem_available_min_mb`, `silenced_events`.
  No existing field or metric changes (D55). `arrival_peak_100ms_share` is the share of first sends in the busiest 100 ms
  of their second, over all those seconds: even arrivals give 0.1, and all together give 1.
- **Why.** 300 users sit below the knee where a model's cost can show at all, so if 1 program and 10 programs differ
  anywhere, it should show there. Everything else is a small addition for a question somebody will ask.
- **What we choose not to build.** A local proxy sharing connections between programs, because it is a new component and
  a new hop in every latency figure; an OS thread per user, because it still shows one process; a `proto.rs` change or a
  user number in `RequestId`, because nothing needs it; new dependencies; `--rate` through the script, because no cell uses
  it. `e1-1000-bidding` stays in the full profile only, and it can be run by name after quick to show the knee.
- **Consequences.** If other items add cells, the counts in `tests/experiment_cells.rs`, `scripts/run-experiments.sh` and
  the docs change again. `e7-p1-300` is not in `quick`, so run it by name if time is short. Its own run line says
  `programs=1`, so `PROGRAMS_PER_PC` does not move it.
- **Where.** `experiments/e7-p1-300.toml`, `analysis/analyze.py`, `analysis/README.md`, `tests/experiment_cells.rs`.
- **How we check it.** `tests/experiment_cells.rs`; `analysis/test_analyze.py`: `test_three_client_logs_count_as_three_programs`,
  `test_link_drops_are_counted_by_reason`, `test_arrival_peak_share_is_a_tenth_when_even_and_one_when_all_together`,
  `test_arrivals_are_read_at_the_start_in_the_measured_time_and_after_a_fault`.
- **Report notes.** Compare `e7-p1-300` with `e1-300-bidding` on latency, `client_pc_cpu_busy_p95` and arrivals. State the
  result either way: if they match, the number of programs did not matter at this load.

### Trial record, for D182, D183 and D186 (not for the report)

The whole runner ran on 2026-11-03 on one WSL machine, with `experiments/hosts.local`, in a private network namespace, with
runs of a few seconds, while other builds shared the machine. It tests the scripts. Every number below only shows that
the mechanism does what it says, and none of it is a RescueMesh result.

| Step | What it showed |
|------|----------------|
| `e1-100-bidding` | Plans of 34, 33 and 33 users on 10 programs each. `pgrep` counted 30 programs at +8 s. `run.json` says `users` 100 and `client_programs_expected` 30. 30 client logs, one `machine_usage` stream per machine. |
| The same cell again | The old folder moved to `runs/old/`. |
| A stray `loadgen` from the same build left running | The runner refused before wiping anything, named the program and exited 1. |
| `e3e4-300-leader` | 10 programs x 10 users on each machine, and the leader was silenced at the right moment. |
| `e7-p1-300` | `programs=1` on the run line gave 3 programs in all, with 100 users each. |
| Three patterns at 30 users | `arrival_peak_100ms_share` read 0.163 for steady, 0.207 for random and 1.000 for all-at-once, whose p50 was about three times steady's. `pattern=` on a run line reaches `loadgen`. |
| `PROGRAMS_PER_PC=1001` | All three machines refused with the reason on screen. The runner copied what there was, printed "stopping the series", exited 1 and did not start the next cell. The analysis then named that run and left it out: "0 of 100 client programs sent anything". |
| TERM to the runner in mid-run, 30 programs up | The runner exited, the programs printed "stopped early" and their totals, no program was left and no PID file was left. |
| `client_pc_cpu_busy_p95` | It read 0.756 while other lanes' builds shared the machine: the metric notices a busy client PC. |
| `e7-p1-300` with storage on | First second perfectly even (`arrival_peak_100ms_share_first_s` 0.100), and the cluster still went through 75 elections in the run. This is why D182 does not claim the spread stops the collapse. Decision 10 is the fix for that. |

The first pass was cut short at 19:16 when the Windows disk that holds WSL's virtual disk filled up and WSL's root filesystem
went read-only. WSL was restarted (not by this work), and the steps it lost were run again. Nothing in these results depends
on that, but it is a warning for the data runs: check free space on all six PCs before a long series.

### D188 · The runner over ssh: how a command reaches a machine, and a test with nothing shared

- **Date:** 2026-11-03 · **Part:** all · **Status:** Implemented, amends D130, amended by D222: the merged `on_host` exits 125, not 1 · **Decided by:** the agent on its
  recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** D130 says the runner "has not run over SSH", and D183 repeats it. Reviewing the lane found why that mattered.
  `on_host` sent `cd DistributedSystems && <command>`. The command that starts a node ends in `&`, followed by
  `echo $! >runs-config/node-1.pid`. A shell reads `A && B & C` as "run the whole of `A && B` in the background, then run C",
  so three things went wrong at once on any real machine:
  1. `cd` ran in the background, so the `echo` ran in the home directory. There is no `runs-config` there, so it failed, and
     `set -e` ended the runner.
  2. `$!` was the PID of a background subshell, not of the node, so a later `kill` would have left the node running.
  3. That subshell kept the SSH session's output open, so `ssh` did not return at all: the runner hung.

  The result: the first run on the six PCs would have stopped on its first node, on its first line. No trial could show this,
  because the `local` path in `on_host` has no `cd`. Running it with a stand-in `ssh` reproduced it at once:
  `runs-config/node-1.pid: No such file or directory`, then a hang until the time limit.

  Reading the same path found a second gap: `ssh` has no timer of its own. D186 says a machine that "cannot be reached (ssh's
  255)" stops the series, but a client PC that loses power sends no goodbye, so its `ssh` would wait for hours and the runner
  with it, with no message.
- **Options considered.**
  - *The command line.*
    1. `cd X || exit 1; <command>` (chosen). The `cd` is its own statement, so it cannot be pulled into the background whatever
       the command is, and a missing folder still stops the command.
    2. `cd X && { <command>; }`. Also correct, but it depends on the command's own ending: `{ a &; }` is a syntax error, so a
       command ending in `&` would need another form.
    3. `cd X; <command>`. Simplest, but if the folder is missing the command runs in the home directory, and one of the commands
       is `rm -rf runs/current data/raft`.
    4. A small `scripts/start-node.sh` on each machine, leaving `on_host` as it was. A new file to fix one line, and the trap stays
       for the next command that ends in `&`.
  - *Noticing a dead machine.*
    1. `ConnectTimeout=10`, `ServerAliveInterval=15`, `ServerAliveCountMax=4` on every `ssh`, `scp` and `rsync` (chosen). A dead
       machine is noticed within about a minute, as `ssh`'s exit 255. `sshd` answers these probes itself, so a script that prints
       nothing for minutes is not affected.
    2. A timer in the runner around each `wait`. More bash, and it must guess how long a run may take, which at overload is a
       request's whole 30 s of retries past its end.
    3. Nothing. A lost machine then stops a long series with no message, and nobody finds out until they come back to look.
  - *How to test it.*
    1. A stand-in `ssh`, `scp` and `rsync` on the `PATH`, in `cargo test` (chosen). Each machine is its own directory. The stand-in
       starts the shell in that machine's home directory with a bare environment, using `sh` (dash on Ubuntu) so nothing leans on
       bash, and does not return until everything holding the session's output has closed it. That last part is what shows a hang.
       It runs in CI, every time.
    2. A real `sshd` in a private namespace. Closer to the truth, but `sshd` is not installed on the development machine and it
       needs keys and a config. Nothing installed was changed for this review.
    3. A hand trial on the lab. Needed anyway, but it proves one moment, and the same mistake can come back with the next edit.
- **Decision.** Option 1 each time. `on_host` sends `cd $REMOTE_DIR || exit 1; <command>`, and every `ssh`, `scp` and `rsync` takes
  `SSH_OPTIONS` from `scripts/lab-common.sh`. `lab-build.sh` no longer uses `source`, which dash does not have, and tests that
  `.cargo/env` exists before reading it, because a plain `.` on a missing file ends a dash shell. New test file:
  `tests/runner_over_ssh.rs`.
- **Why.** The runner is the one way the report's data gets made, so it must not depend on anyone being there to notice a hang.
  The test exists because this bug survived every earlier trial, for a reason that would hide the next one as well.
- **Consequences.**
  - This is still a stand-in. It does not show a real `sshd`, real keys, a firewall or a real network. The hand checks in D183
    remain the first thing to do on the lab: `ssh` returns when the script ends, `pgrep` counts match, and TERM leaves nothing.
  - Each machine needs `rsync` (already in the runbook's install line) for the real copy; the stand-in uses `cp`.
  - A network silence of more than a minute between the control machine and a client PC now stops the series. That is the intent.
- **Where.** `scripts/lab-common.sh` (`on_host`, `SSH_OPTIONS`, `copy_to`, `fetch_from`), `scripts/lab-build.sh`,
  `tests/runner_over_ssh.rs`.
- **How we check it.** `a_whole_run_works_on_six_machines_with_nothing_shared` fails within a second with the old `cd X &&` line
  put back. `a_fault_cell_reaches_a_node_through_ctl_on_a_client_machine` covers `ctl` on a client machine. Both use `dash` as the
  remote shell. By hand, in a private namespace, a series of six cells ran through the stand-in with `bash` as the remote shell, and
  the whole runner ran on `hosts.local` with `awk` as `mawk`, which is what a plain Ubuntu has.
- **Report notes.** For the methods section, say the runner was tested with six separate directories behind a stand-in `ssh`,
  and then on the lab machines once that has happened. Do not claim SSH was tested before it has been.

### D189 · The runner's stop, copy and refuse steps

- **Date:** 2026-11-03 · **Part:** all · **Status:** Implemented, amends D130 and D186 · **Decided by:** the agent on its
  recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** Four places where the runner did not do what D186 says, or lost data it had no need to lose.
  1. *Logs were copied 0.5 s after TERM.* A node with work queued takes longer than that to stop. In a trial the leader was still
     running after the runner had finished. A copy taken then has half a line at its end and no `stopped` event.
  2. *A refusal killed what it refused to touch.* D186 says the leftover check "never kills". But the EXIT trap was already set, so
     refusing ran `stop_all`, which sent TERM to every PID file on every machine, and to a demo running from `runs-config/` too.
  3. *One unreachable machine cost everyone's logs.* D186 says exit 2 or more "finishes this run's log copy". But `set -e` ended the
     runner at the first copy that failed, before the other machines' logs were fetched, and before the message that says why.
  4. *Stale PID files.* After a crash and a reboot, a PID file holds a number that may now belong to someone else's process, and
     `stop_all` would send it TERM.
  Also, two client machines given the same number in the hosts file would run programs with the same client IDs, and one's log
  copies would overwrite the other's. The analysis would catch that, but only after a whole series of runs that all get left out.
- **Options considered.**
  - *Copying.*
    1. Wait until nothing of ours runs, up to `STOP_WAIT_S` (10 s), then copy (chosen). Fast when nodes stop quickly, patient
       when they do not, and it reuses the leftover check's own loop.
    2. A longer fixed sleep. Slow in every run, and still too short at the worst moment.
    3. Only make the analysis accept half a line (done as well, in D191). That alone still loses the end of the log.
  - *The refusal.*
    1. Set the EXIT trap only after the first check has passed (chosen). What that check finds was not started by this runner.
    2. Have `stop_all` kill only PIDs this runner recorded. More state to keep across six machines.
    3. Change D186's words to "refuses and stops them". Then running the runner by mistake ends a demo in progress.
  - *An unreachable machine.* Warn and go on copying (chosen), against stopping at the first failure, which loses the rest, and
    retrying, which waits for a machine that has gone.
  - *Stale PID files.* Remove them once the leftover check has passed, when nothing of ours can be running (chosen), against
    checking each PID's command line in `stop_all`, which is more quoting over ssh for the same result.
  - *Client numbers.* Refuse duplicates and anything outside 1 to 9 in `read_hosts`, before anything starts (chosen), against leaving
    it to the script, which sees only its own number, and to the analysis, which sees it hours later.
- **Decision.** As above. `wait_until_stopped` is used before the copy. After the merge with D139, `refuse_leftovers` lives in `scripts/lab-common.sh` with its
  own loop and `grace_s`, and names every machine's leftovers in one refusal. After a refusal
  nothing is touched: the message lists each machine's PIDs and says to kill them, or to use `start-users.sh --stop`. This means a
  runner that died (its machine lost power, say) leaves its nodes running, and the next run refuses until someone stops them.
- **Why.** Each of these is a difference between what a decision says and what the code did. A runner that kills things by accident
  during a demo is worse than one that asks a person to type one line.
- **Consequences.** About six extra SSH calls per run, plus six at the start of a series. The time per run hardly changes against the
  15 s gap. A dead runner needs a person to clear up, which D186's old accidental behaviour hid.
- **Where.** `scripts/run-experiments.sh` (`wait_until_stopped`, `refuse_leftovers`, `run_once`, the main loop),
  `scripts/lab-common.sh` (`read_hosts`).
- **How we check it.** `tests/runner_over_ssh.rs`: every node log in `a_whole_run_works_on_six_machines_with_nothing_shared` must
  contain `stopped`; `a_runner_refuses_what_it_did_not_start_and_leaves_it_alone` checks the program, its PID file and its logs all
  survive; `a_machine_lost_in_a_run_stops_the_series_and_costs_only_its_own_logs`.
  `the_hosts_file_refuses_numbers_that_would_share_client_ids` in `tests/user_processes.rs`. With the old runner put back, all three of
  those runner tests fail. The whole-run test also holds a stale PID file naming an unrelated live process, which must survive.
- **Report notes.** None beyond D186.

### D190 · What a killed script leaves behind, and signals that lost logs

- **Date:** 2026-11-03 · **Part:** F · **Status:** Implemented, amends D183 · **Decided by:** the agent on its recommendation, on
  Noor's instruction to finish without more questions (2026-11-03)
- **Context.** Three holes in "TERM flushes everything and nothing is left behind", each found by trying it.
  1. *A script killed with KILL.* Its traps do not run, so its programs keep going to the end of their time. `--stop` then said
     "nothing running" with two programs still running. A new start with the same machine and batch ran a second program under each
     client ID: both appended to one log, and one log showed 6 `client_link_up` events where one program gives 3. That is against
     D113. The runner's own check catches this for the runner, but not for the demo or the web page.
  2. *A closing terminal.* A hangup sends HUP to the script and to every program together. The programs died of HUP before the script's
     TERM reached them: no summary, up to 200 ms of log lost, and all of them counted as failed.
  3. *A reader that has gone.* `print!` panics if stdout is closed or the disk is full, and the summary was printed before the log was
     closed. A hand run that lost its SSH session lost the end of its log and exited 101.
- **Options considered.**
  - *Finding the programs of a killed script.*
    1. The script appends each program's PID to `users-M-B.programs` beside its PID file, and removes the file when it ends. `--stop`
       and the next start read it, and check each PID's command line (chosen). Exact, and only about programs of this machine and batch.
    2. Search by client ID with `pgrep`. No file, but it finds any program with that ID on the machine, so tests running side by side and
       other jobs would stop each other.
    3. Make programs die with the script. `PR_SET_PDEATHSIG` needs `unsafe`, which is forbidden here, and a pipe that each program watches
       needs a new flag and new Rust for a rare case. They would also lose the chance to flush.
    4. Nothing, and rely on the runner's check. The demo and the page are not covered.
  - *Hangup.*
    1. Programs ignore HUP, set in the subshell before `exec`, and the script answers HUP with TERM as before (chosen). One line.
    2. Handle HUP in `shutdown_signal`. It changes a function that nodes share, for a client-side problem.
    3. Start them with `nohup`. Does the same, and adds its own redirections to think about.
  - *The summary.*
    1. Close the log first, then write the summary with no panic if that fails (chosen).
    2. Keep `print!`. The log is what the report needs, and the summary is only for reading.
- **Decision.** As above. A start refuses, with the PIDs, if programs of its machine and batch are still running from a killed script.
  `--stop` ends them with TERM, so they flush and print as usual, and sends KILL to any still there after 5 s. It reports them. A
  program still waiting for its start time is a copy of the script, and it would go on to start, so it counts too. The script's exit
  status for a refusal stays 2. `--help` now works from any folder.
- **Why.** "Leaves nothing behind" should hold however the script ends, or `--stop` should at least be able to fix it. Duplicate client
  IDs are the failure D113 exists to prevent, and the page would meet it the first time someone killed its child hard.
- **Consequences.**
  - Stopping during the start spread leaves a `sleep` from each program that had not started, for up to one think time. They do nothing,
    hold no SSH session, and match no leftover check.
  - One more small file in `runs-config/`, and only while the script runs or after it is killed.
  - The web page can rely on one more refusal case, with the same exit status and a reason on stderr.
- **Where.** `scripts/start-users.sh` (`leftover_programs`, the `--stop` branch, the start loop), `src/bin/loadgen.rs` (`main`).
- **How we check it.** `tests/user_processes.rs`: `a_killed_script_leaves_programs_that_a_start_refuses_and_stop_ends`,
  `a_closing_terminal_still_gets_every_program_stopped_and_flushed`, `a_program_whose_reader_has_gone_still_flushes_and_exits_well`
  (exits 101 with the old `print!`), `help_works_from_any_folder`. By hand, at 100 users over 5 programs on a real cluster, TERM after 6 s:
  the summaries said 4649 done and the logs held 4650, the extra one being a request that finished as the signal arrived.
- **Report notes.** None needed.

### D191 · What the analysis counts: warm-up, dead programs, half a line, and which runs are left out

- **Date:** 2026-11-03 · **Part:** F, all · **Status:** Implemented, amends D182, D185 and D186 · **Decided by:** the agent on its
  recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** A cross-check found the first of these. After each short run, the totals every program printed were compared with what
  `analyze.py` counted from the logs. They matched for steady and all-at-once, and did not for random: 162 against 141, and 161
  against 135.
  1. *Warm-up counted from a program's first send.* Under `random` a user's first wait is up to two think times, so a program with one
     user started measuring about one think time late, while its measured time stayed the same on paper. On a 60 s run that is about
     1.7 percent of throughput, and more on short runs. D182 had noted a shift of "up to one think time".
  2. *Give-ups counted by when they gave up.* A request gives up about 30 s after it starts, so one started in the warm-up gave up well
     inside the measured time. In one short trial the analysis read 236 gave up where every program's summary said 0. The function's own
     comment says "started after warm-up".
  3. *A program that died half way was not noticed.* The check counted programs that "sent anything", and a program killed half way has
     sent something. So the run was averaged in, with less load than the cell says, while the runner's comment and D186 both said
     such a run is left out.
  4. *Half a line stopped everything.* A killed program can leave half a line at the end of its log, because its buffer is written
     whenever it fills. `json.loads` then raised, and one bad line ended the analysis of every run.
  5. *A link-drop reason missing from a run* was treated as a gap, so a reason seen in one run of five showed that run's count as the mean.
  6. *Nothing checked the users.* The script shares users exactly and tests pin it, but nothing in the logs said how many users ran.
  7. *A node log missing from the copy.* Now that a lost machine no longer stops the copy (D189), a run could lack a node's log. Fairness
     would then read as the other nodes' share, with nothing to show for it.
- **Options considered.**
  - *When a program's clock starts.*
    1. A new event, `client_started { users }`, logged when the clock starts (chosen). It says exactly what loadgen itself uses, and also
       solves item 6.
    2. Use `client_link_up` as the start. It already exists, but it goes away if clients stop holding connections, and it is missing if no
       node answers.
    3. Leave it, because no cell uses `random`. A cell can name it with `pattern=`, and the numbers are wrong whichever cell it is.
  - *Noticing a dead program.*
    1. Read each machine's `users-*.out`, which already names failures, programs stopped by a signal and whether the script reached its
       totals (chosen). Nothing new to write.
    2. Compare each program's last send with the others'. A guess, and it would need a threshold.
    3. Have the runner record exit statuses in `run.json`. It only sees one status per machine, so the text would still be needed.
  - *A bad line.*
    1. Drop half a line at the very end and name it, and leave out a run with a bad line anywhere else (chosen). Only the end of a file
       can be half written by a kill, so anything else is real damage, but it should cost that run and not the whole summary.
    2. Skip any bad line. Hides real damage.
    3. Raise, as before. One bad run costs an hour's series its summary.
- **Decision.**
  - `metrics::Event::ClientStarted { users }`, logged by `loadgen` when its clock starts; `users` is 0 in an open loop.
  - `program_start` in `analysis/analyze.py` uses it for warm-up, per program, and falls back to the first send for logs that do not
    have it.
  - A `client_gave_up` counts if that request's first send came after warm-up, falling back to its own time.
  - New metrics: `client_users`, `client_programs_failed`, `client_programs_stopped_early`, `client_scripts_with_no_totals`, `node_logs`.
    `run.json` gains `nodes`.
  - `run_problem` also leaves out a run where the users do not add up to `run.json`'s, a program failed, a signal stopped programs early,
    a script never printed its totals, or a node's log is missing. `summarise` leaves out a run with a damaged log and carries on.
  - A `client_link_down_<reason>` missing from a run counts as 0 there.
  - Latency, throughput and every other existing metric keep their meaning. For steady, on which every cell runs, `client_started` and the first
    send are within a millisecond, so those figures do not change. `gave_up` does change in cells with give-ups near the start of a run,
    towards the true count.
- **Why.** The rule from D186 is that a wrong figure should not reach the report with nothing to show for it. These are the cases it missed.
- **Consequences.**
  - `client_started` is a new field for scripts to keep stable (D55).
  - A demo-style `kill` of one program during an experiment now costs that run. That is right: it did change the load.
  - A cell with runs left out needs a rerun, as D186 already says.
  - All-at-once and random cells, if anyone adds them, now measure the same way as steady.
- **Where.** `src/metrics.rs`, `src/bin/loadgen.rs`, `analysis/analyze.py` (`read_events`, `program_start`, `client_requests`, `measured_start`,
  `client_program_metrics`, `client_script_metrics`, `run_problem`, `summarise`, `missing_value`), `analysis/README.md`.
- **How we check it.** `analysis/test_analyze.py`: `test_warm_up_counts_from_the_programs_start_and_not_its_first_send`,
  `test_a_request_started_in_the_warm_up_does_not_count_when_it_gives_up_later`, `test_users_that_do_not_add_up_to_the_cell_are_left_out`,
  `test_a_program_that_died_half_way_is_left_out`, `test_programs_a_signal_stopped_early_are_left_out`,
  `test_a_script_that_never_printed_totals_is_left_out`, `test_half_a_last_line_is_dropped_and_the_run_still_reads`,
  `test_a_damaged_log_leaves_only_its_own_run_out`, `test_a_link_drop_reason_counts_as_zero_in_a_run_that_did_not_have_it`,
  `test_a_node_log_that_was_not_copied_is_left_out`. The six that existed at the time were run against the old `analyze.py` and failed there. By hand, after the fix, the programs' printed totals equal the analysis's count in all 12 short runs that finished, across steady, random and all-at-once.
- **Report notes.** The methods section can say that each run's printed totals were checked against the analysis. Say how many runs were
  left out and why, and rerun them.

### Review record for D181 to D191 (not for the report)

What the review tried, on 2026-11-03, on the lane's commits up to `20be41a`. One shared WSL machine, debug builds, short runs: this checks mechanisms only.

| Attack | Result |
|--------|--------|
| Users summing across programs | Correct: 34, 33, 33 and 4,4,4,4,3,3,3,3,3,3. Nothing in the logs checked it: fixed (D191). |
| ID collisions | Correct by construction across machines and batches. Two ways round it: two hosts with one number (fixed, D189) and a restart after a killed script (fixed, D190). |
| Seeds | Correct. No two programs or runs share one, and random's waits do not disturb the incident stream. |
| Leftovers after a stop or a crash | TERM, `--stop` and a stuck program (KILL after 5 s) all correct. A killed script: fixed (D190). A refusal that killed: fixed (D189). |
| Signals flushing logs | TERM correct: summaries 4649, logs 4650. HUP and a gone reader: fixed (D190). |
| Warm-up with a spread start | Correct for steady. Wrong for random, and for gave-up counts: fixed (D191). |
| All-at-once across programs | Correct. 300 users over 10 programs on one PC: first send 1 to 2 ms past the beat, and all sends within 1.4 to 1.9 ms. The test was too weak to show it, so it now checks the span. |
| Six machines over ssh, no shared filesystem | Would have stopped on the first node of the first run: fixed (D188). After the fix, whole series ran through a stand-in `ssh`. Still not a real `sshd`. |
| Bash portability | Correct under bash 5.1 and `mawk`, and with dash as the remote shell after dropping `source`. `shellcheck` is not installed here, so it did not run. File modes and line endings are right. |
| What the analysis reads from many client files | Dead programs, half lines and missing reasons: fixed (D191). Plots ran on real logs from 30 programs and wrote all three figures. |

---

## 17. The web page on the client PC (November 2026)

Publish-subscribe left this section's zone panel without a feed in Phase 1 (D209).

### D197 · A Phase 1 frontend: a web page on a small program on each client PC

- **Date:** 2026-11-03 · **Part:** F · **Status:** Implemented · **Decided by:** Noor Hany Emam (decisions.md item 2)
- **Context.** Until now no person could report anything. `loadgen` sends random incidents and `ctl` only has
  status, silence and skew. The spec says users report incidents and receive plans (spec lines 23-25), and says a
  simple desktop or web frontend is enough (spec line 373). The demo paragraph (spec lines 202-205) describes client
  machines that run one or more frontends plus many simulated user processes. The Phase 1 list (spec lines 216-231)
  does not name a frontend, but the demo paragraph has no phase tag, so we read it as applying to the Phase 1 demo.
  That is a reading, and the report says so.
- **Options considered.**
  1. *A small web page served by a new program on the client PC.* Nothing to install on six machines, no system
     libraries, and it lives on the client PC, so cutting a server's power cannot take it down.
  2. *A desktop window (egui or similar).* Big GUI libraries, it needs a desktop on the lab PCs, it cannot be driven by
     a test, and an optional feature would not be built by `scripts/lab-build.sh` or CI. Nothing extra for marks.
  3. *A terminal menu.* The spec names desktop or web. It also buys nothing over the terminal commands in
     `docs/demo.md`, which are already the fallback.
  4. *An HTTP server inside each node.* A browser pointed at one node is tied to it and dies with it, which is what
     the system must not show, and it changes the measured program.
  5. *The browser straight to the nodes by WebSocket.* The same problem, plus a new protocol on the nodes.
- **Decision.** Option 1. A new binary `src/bin/frontend.rs` with its page in `src/bin/frontend.html`, embedded with
  `include_str!`. Part F owns both. One frontend runs on each client PC and drives only that PC. Three required
  flags: `--listen`, `--config`, `--client-id`. The first line on stdout is `listening on <addr>`.
- **Why.** It is what Noor chose, it needs nothing installed, and it keeps the transparency story honest: a report has
  no node parameter and goes only through `MeshClient`, so the page is the application and `MeshClient` is the
  middleware that makes the copies. `--config` is required, unlike `ctl` and `loadgen`, because a forgotten flag on a
  lab PC would load loopback addresses and show three unreachable rows, a false outage in a demo about failures.
- **Consequences.** A new program to start in the demo, and a page that is one more thing to rehearse. It adds no
  distributed mechanism and no data. It does not prove that wins shift with load: `NodeStatus` has no win or queue
  field, and one card per click cannot show that, so the grep at `docs/demo.md:19` stays the evidence for scenarios 1
  and 2. The page only says who planned each report.
- **Where.** `src/bin/frontend.rs` (`Args`, `main`, `App`), `src/bin/frontend.html`, `tests/frontend.rs`.
  `scripts/lab-build.sh` builds it already, because it uses `--bins`.
- **How we check it.** `cargo test --bin frontend` and `cargo test --test frontend`. The page has been loaded in a real
  browser on the development machine, against three real nodes on one machine, at 1366 x 768: all four panels fit one
  screen, with no console errors. A browser on a lab PC is still to do: nobody has checked for one on those PCs (run
  `which firefox` first).
- **Report notes.** Name it as a reading of the demo paragraph. Spec line 372 says a sophisticated interface is not
  the aim, so the report should spend one sentence on it. The layers slide gives the words: the page is the
  application, `MeshClient` is the middleware.

### D198 · Frontend HTTP: hand-written on tokio, no new crate

- **Date:** 2026-11-03 · **Part:** F · **Status:** Implemented · **Decided by:** the agent on its recommendation, on
  Noor's instruction to finish without more questions (2026-11-03)
- **Context.** A browser speaks HTTP, and `Cargo.lock` has no HTTP crate. Every program in the crate builds from one
  lock file, so a new dependency also changes what the measured programs are built from. AGENTS.md says a dependency
  needs a log entry and that agents add none unless asked.
- **Options considered.**
  1. *Hand-written on `tokio::net`.* About 250 lines with their tests beside them. The price is that a wrong byte means
     a blank page, and no library's testing stands behind it.
  2. *axum.* A grader sees no difference. About 20 new crates (from memory), Cargo.lock changes for `node`, `loadgen`
     and `ctl`, a failed fetch on one lab PC stops all six builds, and it can no longer land after the data.
  3. *tiny_http behind `spawn_blocking`.* The same lock file problem, and threads for a job tokio already does.
- **Decision.** Option 1, kept deliberately small. One request per connection (`Connection: close`). Read until the
  blank line over as many reads as it takes, at most 8 KiB and 5 s. No request bodies: values go in the query string,
  and a request that declares a body gets 400. Replies carry `Content-Length`, `Connection: close`,
  `Cache-Control: no-store` and `X-Content-Type-Options: nosniff`; the page also carries a
  `Content-Security-Policy` that lets it talk only to itself. At most 64 open connections, then 503. Another path gets
  404, another method gets 405 with `Allow`, an unknown or repeated parameter gets 400. API answers are JSON built with
  `serde_json`. No percent-decoding: every value is digits or an enum name, so an encoded value simply fails its check.
- **Why.** The only source changes are new files, so the measured programs and `Cargo.lock` stay exactly as they are.
  The Oct 4 note about ready-made RPC covers the system's remote invocation; we read it as not covering a browser
  talking to a program on its own PC. Option 2 is what to pick if someone reads that note more widely.
- **Consequences.** Our own parser to keep. It is bounded: one kind of client, on one PC, no cloud traffic through it.
  A cross-site POST from another page in the same browser is possible, because there is no token. We accept that on a
  lab PC with synthetic data (D199).
- **Where.** `src/bin/frontend.rs`: `read_head`, `parse_head`, `Response`, `Query`, `serve`, `connection`, `route`,
  `ROUTES`, and the constants `MAX_HEAD_BYTES`, `HEAD_TIMEOUT`, `WRITE_TIMEOUT`, `MAX_CONNECTIONS`.
- **How we check it.** Unit tests `request_parser_handles_split_reads_and_caps` and `bad_request_heads_are_refused`;
  `a_node_parameter_and_bad_values_are_refused` and `the_page_and_info_are_served_as_a_browser_needs` over a real socket.
- **Report notes.** Goes in the "ours, not a library" column, marked as demo plumbing and not an assessed mechanism.

### D199 · Frontend listen address: a required --listen flag, and no access control

- **Date:** 2026-11-03 · **Part:** F · **Status:** Implemented · **Decided by:** the agent on its recommendation, on
  Noor's instruction to finish without more questions (2026-11-03)
- **Context.** AGENTS.md says every address comes from the config file. But the frontend's address is reached only by
  a browser on its own PC, and every node and client reads one shared config, so a `[frontend]` key would need one
  value per PC in a file that must be identical on all six.
- **Options considered.**
  1. *A required `--listen` flag with no default.* No change to `config.rs`, and the address can change on the day
     without a rebuild.
  2. *A `[frontend]` config key.* Edits `config.rs` and both test builders, and so changes measured programs.
  3. *A hard-coded port.* Forbidden, and untestable with port 0.
  For access: *none, on loopback, with load limits*; or a shared token; or a password.
- **Decision.** Option 1, as a stated exception to the rule. The docs show `--listen 127.0.0.1:9001` with a browser on
  that PC; `ssh -L 9001:127.0.0.1:9001 ubuntu@<client PC>` is the fallback, and `0.0.0.0:9001` with
  `sudo ufw allow 9001/tcp` only if neither works. No token and no password.
- **Why.** The data is synthetic (spec lines 36-37), and anyone on that PC can already run `loadgen`. Loopback means no
  new open port. The load box has limits (D202), so a stray request cannot take the cloud down by itself.
- **Consequences.** If someone does use `0.0.0.0`, anyone on the lab network can start 1000 users on that PC.
- **Where.** `Args::listen`; AGENTS.md "No localhost assumptions" gains the exception.
- **How we check it.** The tests start the real program with `--listen 127.0.0.1:0` and read the port from its first line.
- **Report notes.** One sentence under deployment, so a reader who knows the rule sees why this is not a breach of it.

### D200 · A second exception to the sockets rule, and status polls as another control sender

- **Date:** 2026-11-03 · **Part:** F, C · **Status:** Implemented · **Decided by:** the agent on its recommendation,
  on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** AGENTS.md says only `node/` and `raft/network.rs` open sockets. `client.rs` already bends this (D112).
  The frontend's listener is a new socket outside those places. D23 and AGENTS.md also name `ctl` as the sender of
  control frames, and the page's status table sends them too.
- **Options considered.**
  1. *Name `bin/frontend.rs` as a second exception, and note it at D112 and D23.*
  2. *Move the listener under `node/`.* `node/` must not gain client code.
  3. *Leave the rules as written.* A false rule is worse than an amended one.
- **Decision.** Option 1. The port carries no cloud traffic. Status polls use `client::control` with
  `ControlRequest::Status` only. The page has no fault buttons, so `ctl` stays the fault tool (D62).
- **Why.** The rule exists to keep the transport in one place, and this listener is not part of that transport.
- **Consequences.** AGENTS.md, `docs/architecture.md` and D112, D23 need a line each. Three nodes get a few more
  status frames: one poll per open page, about one a second.
- **Where.** `src/bin/frontend.rs` module comment, `status_rows`.
- **How we check it.** `a_lone_nodes_answer_is_labelled_provisional_and_silenced_is_not_unreachable` shows a silenced
  node still answers the poll, so the fault gate holds.
- **Report notes.** None needed beyond the rule's wording.

### D201 · Report form and answer card: fields, input checks, provisional banner, no-answer wording

- **Date:** 2026-11-03 · **Part:** F · **Status:** Implemented · **Decided by:** Noor Hany Emam (decisions.md items 2 and 5 for
  the form and the TTL), the rest by the agent on its recommendation, on Noor's instruction to finish without more
  questions (2026-11-03)
- **Context.** The card is the point of the page: a grader types a report, reads who planned it, cuts that server, and
  sees the next one answered. Nodes check none of a report's values, and severity sets how long a responder is held.
- **Options considered.**
  - *Location:* draw `campus::unseeded_incident` until the zone matches; a new `campus::point_in_zone`; or the person
    types x and y. A new function edits Part E's file, and typed metres mean nothing to a demo audience.
  - *Input checks:* in the frontend only; also in the nodes; or send what was typed. A node check changes the measured path.
  - *The call:* the POST waits for the answer, one report at a time; or it returns a slot and the result comes by poll.
  - *Provisional answers:* an amber banner; a small tag; a "check again" button; or hold the answer until the zone feed.
  - *Giving up:* only `MeshClient`'s D52 budget; or the page re-sends with a new ID. A new ID is a second incident.
- **Decision.** `POST /api/report?zone=&severity=&needs=&who=&valid_for_s=`. Zone must be below `campus.zones`, severity
  1 to 5, capability a known name, who a whole number (default 1), valid-for 1 to 3600 s (default 60). Valid-for 0 is
  refused, because a person never means it. Draw until the zone matches, capped at 256 draws per zone, then set severity
  and needs. One click is one `MeshClient::submit`, and the POST waits for it, at most about 36 s. The button is off
  until the answer, with a running timer. The last 10 cards stay in page memory.
  The JSON is `{ok, request, answered_by, planned_by, responder, responder_zone, responder_capability, cost_m, status,
  priority, provisional, expired, log_index, term, latency_us, attempts, zone, severity, needs, who, valid_for_s,
  not_carried}`, or `{ok:false, error}`. `request`, `log_index` and `term` are null when provisional. The request ID is
  text, because JavaScript rounds a number above 2^53.
  The card shows the responder, or "No responder free right now", who planned it, who answered it, status, priority,
  log index, the proposer's term, latency and attempts. A provisional answer gets an amber banner: "PROVISIONAL: not
  committed, may change when peers return". An expired answer gets a red banner: "EXPIRED: too old when it reached the
  cloud, nobody was sent". A give-up shows "No answer after 6 attempts; it may still be recorded later."
- **Why.** "No responder free" must not read as a failure, since it is a correct committed answer. D122 says clients are
  not told of a later change, so the banner promises nothing more. A give-up only removes the waiter (`src/client.rs`),
  so no answer does not prove nothing was recorded, and the wording says so. The page never resends.
- **Consequences.** A provisional card can go stale with no update unless the person also follows that zone. Nodes still
  trust clients: only this program checks the values. Until the wire lane merges, `who` and `valid_for_s` are checked
  and shown but not carried, the card says so in words (`not_carried`), priority says "not available yet", and
  `Expired` cannot occur.
- **Where.** `src/bin/frontend.rs`: `ReportForm`, `parse_report`, `draw_in_zone`, `build_incident`, `report`,
  `report_json`, and stubs SWITCH-ON 1 to 4. `src/bin/frontend.html`: `renderCard`, `sendReport`.
- **How we check it.** `a_typed_report_returns_a_committed_card` compares what was typed with what the cloud recorded;
  `provisional_json_has_no_index_or_request`, `nobody_free_and_an_expired_answer_are_told_apart`,
  `with_no_node_left_a_report_ends_in_no_answer_and_rows_say_why`. Switching on stubs 1 to 4 adds
  `a_report_that_outlives_its_valid_for_time_comes_back_expired`, which passed against lane-wire: every node silenced for
  longer than a 1 s valid-for time, then the card says expired. In a real browser the amber and red banners were looked at
  with sample answers passed to the page's own `addCard`, not with a live provisional or expired answer.
- **Report notes.** The card's "planned by" is what makes the failure scene work: it tells the grader which server to
  cut. Use it in the demo description.

### D202 · Load box: real loadgen programs, the split, the patterns, limits and Stop

- **Date:** 2026-11-03 · **Part:** F · **Status:** Implemented, see D209: all three patterns run, so its text about `available` flags no longer applies · **Decided by:** Noor Hany Emam (decisions.md items 3 and 4), the rest by the agent on its recommendation,
  on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** The spec asks for many user processes on the client machines, and allows a frontend to control them
  (spec lines 31, 202-205). Noor's own design is one box: "how many user processes", plus a pattern. Behind it the page
  starts 10 real background programs sharing the users.
- **Options considered.**
  - *What runs:* real `loadgen` programs; tokio tasks inside the frontend, which would be a rename; or started by hand.
  - *How many:* 1000 real programs, where the client PC chokes first; one program, which leaves the gap against
    "processes"; or Noor's 10 sharing the users.
  - *Who starts them:* this program directly; or `scripts/start-users.sh` as a child (gap-16's recommendation).
  - *Stop:* SIGKILL only (gap-17's recommendation); or TERM, a short wait, then SIGKILL.
- **Decision.** Each click of Start splits its users over at most 10 programs: 1000 = 10 x 100, 7 = 7 x 1,
  1005 = 5 x 101 + 5 x 100. Another click adds a batch. Each program is a child started directly with
  `tokio::process::Command`: the `loadgen` beside this binary, with `--config <same value> --client-id <frontend id x
  10000 + n> --users --think-ms --duration-s --seed <its ID>`, stdin null, stdout piped and read to the end, stderr
  inherited, `kill_on_drop(true)`, and `TOKIO_WORKER_THREADS` set to the cores over the programs in that click, at least 1
  (gap-16's rule). Patterns are steady (everyone waits exactly T), random (each user waits 0 to 2T), and all-at-once (every
  user sends at the same moment), passed as `loadgen --pattern steady|random|all-at-once`. T is the think time, default
  1000 ms. The IDs are frontend ID x 10000 + n and not x 1000 as gap-17 designed: by the time this was built, five-digit
  IDs belonged to `scripts/start-users.sh` (machine x 10000 + batch x 1000 + program, D184), so 11 x 1000 + 1 = 11001 would
  have been its machine 1, batch 1, program 1, with one ID and one log file between two programs.
  Refused: past 1000 running users, past 1500 offered reports a second (users x 1000 / think_ms, counting what already
  runs), think under 200 ms, duration over 900 s, more than 20 running programs. Defaults: 30 users, 1000 ms, 600 s.
  `GET /api/load` gives rows of `{client_id, pid, users, think_ms, pattern, state, exit_code, summary}`, with states
  running, stopping, finished, stopped and failed. The summary is the program's stdout, shown as it is, and nothing
  parses it. An early exit shows failed with its code and is not restarted. Stop sends TERM by process ID through
  `kill(1)`, waits 2 s, then the child is dropped and killed. On shutdown the frontend does the same for every program,
  then closes its log. The line under the box says what will really run: "Runs as 10 real background programs: 10 x 100
  users."
- **Why.** Direct children give one row per program with its own PID, exit code and printed summary, which is what Noor
  asked to see; the script merges output into one file per PC. TERM first costs 10 lines and needs nothing from main:
  today a `loadgen` dies on TERM with no summary, and once the load lane makes TERM print the summary, Stop shows it
  with no change here. `kill(1)` because unsafe is forbidden and libc is not a direct dependency. Per-click limits alone
  would allow 12,000 users on one PC, and overload can fake degraded mode (`docs/runbook.md:129`).
- **Consequences.** Users inside the 10 programs are still tasks. Spec line 70 ties a participant to a user process, so
  this is a reading, and the line under the box states it openly instead of hiding it. The page does not use gap-16's
  script, so that script is one start path for the runner and the demo, not for the page, and its comment saying
  otherwise must change. The page also does not get the script's start spread between programs: on steady, the first
  user of each of the 10 programs sends at about the same moment. Each program adds 3 status frames a second on TCP,
  which matters only if
  experiments move to many programs (gap-16 settles that). Experiments do not use this box. A frontend SIGKILLed
  itself leaves its programs running until their duration ends, which is why every program has a finite one.
  `docs/demo.md` keeps a short shell loop for the same programs, so many processes do not depend on the page.
- **Where.** `src/bin/frontend.rs`: `Pattern`, `split_users`, `LoadBox`, `check_limits`, `load_start`, `start_program`,
  `program_command`, `watch_program`, `ask_to_end`, `record_end`, `load_stop`, and stub SWITCH-ON 5.
- **How we check it.** `users_split_over_at_most_ten_programs`, `load_limits_are_refused`,
  `the_box_starts_ten_real_programs_and_shows_each_summary`, `stop_ends_the_programs_and_marks_them_stopped`,
  `shutdown_ends_every_program`; in `tests/frontend.rs`, with real `loadgen`:
  `a_loadgen_that_reaches_its_duration_shows_its_summary`, `load_start_runs_real_loadgens_and_stop_ends_them`,
  `sigterm_stops_the_children_and_closes_the_log`. By hand, against three real nodes: 1000 users gave 10 programs of 100,
  `pgrep` counted 10, and each row showed its summary. Against lane-load's `loadgen`, Stop's rows carry the summary too.
- **Report notes.** State the reading first: real programs, tokio tasks inside, the number used. A literal reader should
  hear that from us.

### D203 · Node table: columns, last known, the unreachable rule, no same-state badge

- **Date:** 2026-11-03 · **Part:** F · **Status:** Implemented ·
  **Decided by:** the agent on its recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** The recovery and failure scenes need node state on the same screen as the card. `ctl status` asks nodes
  one after another with a 2 s wait each, so a dead node makes it slow.
- **Options considered.**
  - *Where the data comes from:* one poll per page asking all nodes at once; a background task per node; server-sent
    events; a sequential loop like `ctl`.
  - *When a row says unreachable:* only after the local program reported it across polls spanning 3 s; in the page
    alone; on the first miss.
  - *A same-state badge:* it could light up beside a PROVISIONAL card, because the digest covers only applied commands.
- **Decision.** `GET /api/status` asks every node at once through `client::control`, capped at 800 ms. JSON is
  `{ok, nodes:[{id, reachable:true, role, term, leader, last_log_index, last_applied, digest, results, responders,
  silenced, serving, degraded, members} or {id, reachable:false, error}]}`. The digest is 16 hex characters, as `ctl`
  prints it. The page polls 500 ms after the last answer, with a 2 s fetch timeout, and shows node, role, term, leader,
  log, applied, digest, serving, silenced, degraded, members. A row says unreachable only after `reachable:false`
  across polls spanning 3 s, and until then it keeps its last values, dimmed. A failed or slow `/api/status` shows a
  separate banner: "Local program not answering. Node state unknown." A silenced row marks role, term and leader as
  "(last known)". Under the table: "The leader only orders the log; there is at most one leader per term." No badge.
- **Why.** No stored state, and no polling with no page open. A dead node costs one poll 800 ms, so with a machine off a
  full cycle is about 1.3 s. A slow local program must not read as three dead servers, in a demo about failures. A silenced
  leader still says leader (`src/node/rejoin.rs`), which is why those cells are marked.
- **Consequences.** People compare digests by eye, as with `ctl status`. Results and responders are in the JSON but not
  on screen, to fit one screen.
- **Where.** `src/bin/frontend.rs`: `status`, `status_rows`, `node_json`, stub SWITCH-ON 6. `src/bin/frontend.html`:
  `renderNodeRow`, `pollStatus`.
- **How we check it.** `digest_is_text_and_survives_above_two_to_the_53`,
  `a_lone_nodes_answer_is_labelled_provisional_and_silenced_is_not_unreachable`,
  `with_no_node_left_a_report_ends_in_no_answer_and_rows_say_why`. By hand: SIGSTOP the frontend and see the local banner,
  not three dead servers.
- **Report notes.** None beyond the demo description.

### D204 · Zone updates panel: register a zone, its new reports and plans appear live

- **Date:** 2026-11-03 · **Part:** F · **Status:** Superseded by D209: Phase 2. The page has no zone panel in Phase 1, and the code stays in the history of `lane-frontend` · **Decided by:** Noor Hany Emam (decisions.md item 6), the details by the agent on its
  recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** Noor's item 6: on the web page you register your zone, and new reports and plans for that zone appear
  live. Reporting does not need it; it exists to tell others in a zone. Gap-17's own recommendation was to leave the
  panel out of this change, and item 6 overrides that. Gap-14 gives `subscribe(zones)`, `next()` and `unsubscribe`. It has
  no way to change zones in place, and gives each subscriber its own `MeshClient`.
- **Options considered.**
  - *How it reaches the page:* a bounded list the page polls; server-sent events; a WebSocket. Both of the others need
    more hand-written HTTP for a panel that polling serves well enough.
  - *Adding a zone:* subscribe again with the longer list, then end the old feed; or one feed per zone.
  - *Before the API exists:* an empty panel that pretends; or a panel that says plainly it is not available yet.
- **Decision.** `POST /api/zones/register?zone=` subscribes with the full list on the panel's own `MeshClient` (ID:
  frontend ID x 10000, which no load program uses, since they start at + 1), made on first use. The new feed starts
  before the old one ends, so no update falls between them, and the list drops any commit it already holds by its log
  index. `POST /api/zones/clear` ends the feed. `GET /api/zones?after=<number>` gives
  `{ok, unavailable, zones, note, last_seq, updates}`. One owned task reads `next()` into a list of the last 200. The
  page polls every 500 ms and shows the last 50. A `GaveUp` from `next()` shows "No node is answering; waiting.", and the
  feed resumes by itself. Until gap-14 lands, register answers 501 and the panel says "Zone updates are not available
  yet", and the panel's client is not even made.
- **Why.** Its own client means a burst of updates cannot delay a typed report. Polling needs no new HTTP. A panel that
  says "not available yet" is honest, and a merge agent can switch it on by changing four small functions.
- **Consequences.** Updates reach the screen up to 500 ms after they reach the frontend, so "live" means about half a
  second here. This has never run against a real feed: gap-14 was not on any branch when this was written, so the
  register, next and unsubscribe path is compiled but unproven. If gap-14 is cut (Noor's own rule: cut first if late), the
  panel stays and keeps saying not available.
- **Where.** `src/bin/frontend.rs`: `ZonePanel`, `FeedTask`, `update_json`, `run_zone_feed`, `zones_register`,
  `zones_list`, `zones_clear`, stub SWITCH-ON 7. `src/bin/frontend.html`: `pollZones`, `registerZone`, `clearZones`.
- **How we check it.** `the_zone_list_is_bounded_and_read_after_a_number` and
  `what_other_lanes_build_says_not_available_yet`. After the merge: a test that registers a zone, reports in it and in
  another zone, and sees only the first.
- **Report notes.** Claim item 6 only if the merged build really shows a live update.

### D205 · The demo runs E1's plan_work and responders (amends D129)

- **Date:** 2026-11-03 · **Part:** F, E · **Status:** Implemented in `config/lab.example.toml` and `docs/demo.md`. D165 keeps the same values and text, and adds the load rule, the start order and the fault timers ·
  **Decided by:** the agent on its recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** D129's last sentence keeps the demo on the defaults. At those defaults the page defeats itself. 30
  responders hold about 0.63 s each, so at the 90 reports a second of `docs/demo.md:25` about half get nobody (Erlang-B,
  computed, not run). With `plan_work = 0` an idle score is 0, all nodes tie, and node 1 plans every typed report.
- **Options considered.**
  1. *`plan_work = 1000` and `responders = 300` in `config/lab.example.toml`, the E1 values.*
  2. *Responders 300 only.* Node 1 still wins every idle tie.
  3. *Keep the defaults, and hold load near 30 reports a second while typing.*
- **Decision.** Option 1. `docs/demo.md:49` then says `--work 1000` to put a node back, because skew replaces `plan_work`.
- **Why.** The runner builds each run's config from the hosts file and the cell and never reads `lab.toml`, so data
  cannot change. Under about 390 reports a second the block rate is then near 0, and the first three cards should read
  nodes 1, 2, 3 (read from code, not run).
- **Consequences.** These settings are untried: `plan_work 1000` was timed on a development machine, and 300 responders
  rests on arithmetic. Look at the first rehearsal.
- **Where.** `config/lab.example.toml`, `docs/demo.md`.
- **How we check it.** `config::tests::shipped_configs_are_valid`, and the first rehearsal.
- **Report notes.** None; the demo settings are not data.

### D206 · The runner refuses leftover frontend, loadgen and node programs

- **Date:** 2026-11-03 · **Part:** F, all · **Status:** Implemented through D139 ·
  **Decided by:** the agent on its recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** A page left open with load running, during an experiment, adds load nobody counts. `stop_all` kills only
  programs in pidfiles, so a leftover gives a wrong figure with no sign. A hand-started node is worse: it holds the
  ports, the new node fails to bind, `wait_for_leader` passes on the old one, and the run measures the wrong config.
- **Options considered.**
  1. *At each run's start, refuse on frontend, loadgen and node by path with `pgrep -f`; check frontend and loadgen again
     after the load ends. Exit 0 refuses, 1 is clean, anything else stops as cannot check.*
  2. *Once at the start with `pgrep -x`.* It also matches Node.js, including a VS Code server, and could stop an hour's
     run for nothing.
  3. *A runbook line only.* 4. *Teach the runner to kill them.*
- **Decision.** Option 1: `refuse_leftovers` with one `pgrep -af '[t]arget/release/(frontend|loadgen|node)( |$)'` per host,
  at the top of `run_once`, and again for frontend and loadgen right after the run's own loadgens end. A trip after the
  load deletes that run's directory and stops, because analysis reads every `run.json`.
- **Why.** Matching by path keeps other programs out of it. Reading exit codes properly means an unreachable host stops
  the run instead of passing.
- **Consequences.** A program started outside the build folder is not caught. By the time this was built, lane-load had
  written `refuse_leftovers` in `scripts/run-experiments.sh`: by path, at each run's start, with a short wait, naming
  loadgen, node, frontend and zonefeed. So this lane did not edit the runner, because a second copy would only conflict.
  Two parts of this decision are not in lane-load's version and are left for the merge: the second check right after the
  run's own loadgens end, and reading exit codes, since it runs `pgrep ... || true`. Untried over SSH (D130).
- **Where.** `scripts/run-experiments.sh` on lane-load.
- **How we check it.** `experiments/hosts.local` with a leftover, clean, and an unreachable host.
- **Report notes.** One clause in the method: runs start from a clean machine, and this is checked.

### D207 · Demo script: page first, terminals as cross-check and fallback (amends D132)

- **Date:** 2026-11-03 · **Part:** all · **Status:** Accepted; the text is in `doc-notes/gap-17.md` · **Decided by:** the
  agent on its recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** D132's consequence says the live part uses `ctl status`, event logs and the load generators' output. With a
  page, the audience should see the page first. But the page has never met a real lab network.
- **Options considered.**
  1. *Page first; every `ctl status`, log and `loadgen` command stays as cross-check and fallback.*
  2. *Replace the terminals.* If the page fails in the lab the demo fails.
  3. *Leave `docs/demo.md` as it is.* Then nobody uses the page.
- **Decision.** Option 1. One frontend on each client PC, started after the wipe. In the failure scene, cut the node the
  last card names. Also name location and failure transparency from the slides, plus access as in the Sept 6 note. A short
  bash loop in `docs/demo.md` starts the same 10 programs by hand, so the box is droppable.
- **Why.** If the page fails on the day, the demo still runs exactly as written today.
- **Consequences.** Stop loses a program's printed summary until the load lane merges, so `docs/demo.md:64` and `:107` say
  to let the run reach its duration, or to count with grep on `runs/lab/client-*.jsonl`.
- **Where.** `docs/demo.md`, `docs/runbook.md`.
- **How we check it.** A rehearsal on the six machines, still to do.
- **Report notes.** The demo section can say the page and the terminals show the same state.

### D208 · The frontend may land after the data: proof by binary hashes

- **Date:** 2026-11-03 · **Part:** F · **Status:** Accepted · **Decided by:** the agent on its recommendation, on Noor's
  instruction to finish without more questions (2026-11-03)
- **Context.** Final data must come from the final code. The page is the kind of thing that lands late. Noor's item 12 also
  time-boxes all remaining work.
- **Options considered.**
  1. *In the six-PC commit if ready; otherwise after the experiments, with `sha256sum` of `node`, `loadgen` and `ctl`
     before and after, both in the report. Any library edit the page needs waits until after the report, or the data is
     rerun.*
  2. *Before the experiments only.* 3. *Skip it if late.*
- **Decision.** Option 1. Note that the plain "two new files only" check of the design no longer applies by itself once
  the stubs are switched on: switching them on changes only `src/bin/frontend.rs`, but other lanes change the library, so
  the hashes are compared around the frontend commit alone.
- **Why.** It never holds up the six-PC test or the data, and hashes turn a reading into a fact.
- **Consequences.** If it lands after the experiments, the page has never seen a real power cut, which notes line 83
  asks to see. It then needs its own short rehearsal on one client PC.
- **Where.** The report, and `docs/runbook.md`.
- **How we check it.** `git diff <before> -- src Cargo.toml Cargo.lock` lists only frontend files, and the hashes match.
- **Report notes.** Put both hash lists in the report if this path is taken.

---

## 18. Lectures 8 and 9, and closing what was left open (November 2026)

Noor attached the Lecture 8 and Lecture 9 notes on 2026-11-03 at 19:29. Noor's rule is that recordings and docs decide, and that
nobody is told to go and ask the instructor. These entries apply that rule. Old entries keep their text, and only their status
line changes.

### D209 · Publish-subscribe: first chosen to build small, then settled as Phase 2 by Lecture 9

- **Date:** 2026-11-03 · **Part:** all · **Status:** Implemented, amends D131 and supersedes D204; nothing of it is in Phase 1's
  build · **Decided by:** Noor Hany Emam (items 6 and 14 of the decisions of 2026-11-03). Noor can overrule it
- **Context.** D131 put publish-subscribe in Phase 2 and ended by telling the team to ask the instructor. That question is closed
  here. On 2026-11-03 Noor first chose the other way (item 6): build it small and cut it first if late. On the web page you would
  register your zone, and new reports and plans for that zone would appear live. Noor's point on record: reporting does not need it;
  it exists to tell others in a zone. Leaving it for Phase 2 was the rejected option then. Later that evening the lecture notes
  arrived. Lecture 9 sections 2.1 and 2.3 and its summary place publish-subscribe in Phase 2: it needs a publish step and a discovery
  service, so that clients and peers can come to know of each other. Lecture 8 section 10.3 says the same in more detail, with the
  cloud as the directory of service, and its action items say that both paradigms must appear in the design.
- **Options considered.**
  1. *Phase 2. Show it in the design, keep any code on its own branch, and say why in the report.* Chosen.
  2. *Keep item 6: a small version in the Phase 1 PR.* It reverses what the notes say, it would have no discovery behind it, and it
     adds a client-path change while the deadline is a day off. Item 6's own rule was to cut it first if late, and item 12 time-boxes the
     work.
  3. *Leave D131 as it stood, with its question.* Noor's rule rejects leaving it to someone else's answer.
- **Decision.** Option 1. Reverses item 6.
  - It stays out of the Phase 1 PR. If any code exists it stays on its own branch for Phase 2.
  - `docs/architecture.md` section 13 shows the design as `planned`: publish, directory, subscribe, push.
  - The web page has no zone panel in Phase 1. The panel's code (D204) was taken out and stays in the history of `lane-frontend`,
    commit e47e9d4, for Phase 2. The `/api/zones` routes are gone.
  - D131's question is closed. Nobody needs to ask anything about it.
- **Why.** The lecture answers the question D131 left open, in the course's own words. Noor said Phase 2 first, then chose small, and
  had said to cut it first if late. Group communication is the paradigm the lecture places in Phase 1 (D210).
- **Consequences.** No live zone updates in the Phase 1 demo. If someone asks at the demo: Phase 2, and why, with the design in section
  13. Noor's reason for wanting it, telling others in a zone, is kept here for Phase 2. Decisions the gap-14 work made for its branch
  are recorded when that branch merges.
- **Where.** `docs/architecture.md` sections 2 and 13, `docs/demo.md`, `README.md`, the report.
- **How we check it.** A search for "publish-subscribe" in the docs finds it only as Phase 2 or as this history.
- **Report notes.** State both steps, because the course asks for changed decisions with their alternatives: built small (item 6), then
  Phase 2 (item 14), and why.

### D210 · Group communication: the client middleware's send to all nodes is the group send

- **Date:** 2026-11-03 · **Part:** F, C, all · **Status:** Implemented in the docs, amends D126, D152 and D157 · **Decided by:** Noor Hany
  Emam (item 14 of the decisions of 2026-11-03)
- **Context.** D126 named the sends. D152 built `CloudGroup`, and D157 stated a reading from the notes we had then, treating the client as
  an outsider sending to an open group. Lecture 9 section 2.2 is more direct: "The client middleware uses group communication to send to
  all of the cloud nodes." The application does not know this happens, and the client does not notice it. Lecture 8 section 9 adds that
  the set of members is dynamic, because failures are simulated, and that the middleware, not the application, takes care of it.
- **Options considered.**
  1. *`MeshClient`'s send to all nodes is the main group send; `CloudGroup`'s who's-alive list is the membership.* Chosen: both things the
     lectures ask for exist, and each has a name a reader can open.
  2. *`CloudGroup` alone is the group communication, as D157 reads it, with the client outside.* It leaves out the send the lecture points at.
  3. *Move the membership list into the client.* A client that knows who is alive stops being transparent, and the lecture puts failures on
     the cloud side.
- **Decision.** Option 1. In every doc and in the report:
  - **The group send** is `MeshClient::submit`, in `client.rs`. The application makes one call. The middleware makes the copies, retries with
    the same ID and keeps the first answer. One source, many recipients, driven by the sender.
  - **The membership** is the cloud's who's-alive list, `CloudGroup`'s view, in `node/group.rs`. It changes when a node dies or returns (D153).
  - **Inside the cloud,** `CloudGroup::send` and `probe` use the same idea for bids and pings. Results are ordered by Raft, from openraft (D126,
    D152).
  - `MeshClient` always sends to all the configured nodes. It does not read the view, so a report can reach a node that is coming back, and
    that node stays quiet until it is serving.
- **Why.** It is what the lecture says. D157's outside view of the client is not wrong about how the code runs, but it put the weight on the wrong
  send.
- **Consequences.** "Client middleware" and "group send" must be the words at the demo. Item 14 asks for this to show in code names: today
  `MeshClient`, `submit` and the file header of `client.rs` say it; D156's `src/middleware.rs` adds the name. Limits stay as D157 states them:
  each list is one node's opinion, and each send is three unicasts from a client, not IP multicast.
- **Where.** `client.rs::MeshClient`, `node/group.rs::CloudGroup`, `docs/architecture.md` sections 2, 4.3 and 5.1a, `docs/demo.md`.
- **How we check it.** `tests/client.rs` for the send to all nodes and the retry, `tests/group.rs` for the view. `docs/demo.md` says the same thing.
- **Report notes.** Say "the client middleware's send to all of the cloud nodes is our group send, and the application does not know it happens".

### D211 · The middleware and application boundary, on both the client side and the cloud side

- **Date:** 2026-11-03 · **Part:** all · **Status:** Implemented in the docs and on both sides: the cloud side's file split is D156 · **Decided by:** Noor Hany Emam (item 14 of the decisions of 2026-11-03)
- **Context.** Lecture 9 says the boundary must be visible on both the cloud side and the client side, in the design and in the implementation, and
  that it is asked about repeatedly. Lecture 8 says the same. D156 planned the split as an optional last commit.
- **Options considered.**
  1. *Draw it on both sides in `docs/architecture.md`, and make D156 required.* Chosen.
  2. *Draw it only.* Lecture 9 says implementation as well as design.
  3. *Keep D156 optional.* Cutting it would drop the layer claims from the report, which the lecture says will be asked about.
- **Decision.** Option 1.
  - **Client side.** Application: the simulated user in `bin/loadgen.rs`, and the web page in `bin/frontend.rs` and `bin/frontend.html`. Middleware:
    `client.rs::MeshClient`.
  - **Cloud side.** Application: the incident service (`handle_request`, `execute`, `plan_on_workers`, `send_done`, today in `node/mod.rs`, moving to
    `node/service.rs` under D156), `planner.rs`, `campus.rs`, and the rules in `state.rs`. Middleware: `node/group.rs`, bidding (`bidding.rs`,
    `node/bids.rs`), the detector (`node/heartbeat.rs`), `node/takeover.rs`, `node/rejoin.rs`, `node/degraded.rs`, `node/transport.rs` with tarpc, and
    `raft/` with openraft.
  - D156 is required, no longer optional. Until it merges, the docs say the cloud side's boundary runs through `node/mod.rs`, and say so plainly.
- **Why.** The lecture asks for it by name. Users never know who executes, so the election serves the application from below (D156's reading).
- **Consequences.** D156's note "If this is dropped, drop the Sept 13 layer claims" no longer applies: it must not be dropped. If it misses the deadline, the
  report says the boundary is drawn and the file split is not finished. Cloud nodes talk through their middleware and never call each other directly
  (Lecture 9 section 7.1), which `CloudGroup` and `Consensus` provide.
- **Where.** `docs/architecture.md` section 4.3. D156's files when built: `node/service.rs`, `src/middleware.rs`, `tests/layers.rs`.
- **How we check it.** `tests/layers.rs` when D156 lands. Until then, section 4.3 names each file.
- **Report notes.** Use the diagram in section 4.3. Claim the files' split only after D156 merges.

### D212 · Cloud nodes are peers, client and server are roles per transaction, and three system models

- **Date:** 2026-11-03 · **Part:** all · **Status:** Implemented in the docs · **Decided by:** Noor Hany Emam (item 14 of the decisions of 2026-11-03)
- **Context.** Lecture 9 section 3 says the cloud is not to be seen as client-server. Its nodes are peers that update each other's state, detect failures,
  distribute work and share their load. Client and server are the roles of one transaction, not properties of a node. Lecture 8 section 6 asks for three
  models of one system: physical, architectural and fundamental. The docs called the nodes "server nodes" in places and had no such split.
- **Options considered.**
  1. *Say it in section 2 of the architecture, with a table of transactions and their roles.* Chosen.
  2. *Leave it to the report.* The report draws on these docs, and the words at the demo come from them.
- **Decision.** Option 1. `docs/architecture.md` gets the three models in section 2 and "Peers, and roles per transaction" in section 3.1. "Server" is used only
  for a role in one call, or for the spec's own wording.
- **Why.** The code is already like this: every node sends and receives bids, heartbeats and Raft calls, and any node can answer a client.
- **Consequences.** Client PCs remain clients in Phase 1 because they offer nothing yet. That changes in Phase 2, where each peer both publishes and consumes (Lecture 8 section
  10.3).
- **Where.** `docs/architecture.md` sections 2 and 3.1, `README.md`.
- **How we check it.** Reading the docs, and the demo's questions list.
- **Report notes.** Item 14 also asks for a short sheet per person on what they must be able to explain. That belongs to the report, not this log.

### D213 · The questions once left for the instructor are closed

- **Date:** 2026-11-03 · **Part:** all · **Status:** Implemented, amends D116, D119, D120, D126 and D131 · **Decided by:** Noor Hany Emam (their standing rule: the spec, the notes
  and the recordings decide, and nobody is told to ask Dr Amr). The answers are Noor's items where a number is given
- **Context.** Several places left a question for the instructor. Old entries keep their text under AGENTS.md, so a new entry has to close them.
- **Options considered.**
  1. *Close each in one table here, mark the old entries' status lines, and remove the questions from the live docs.* Chosen.
  2. *Rewrite the old entries.* AGENTS.md forbids it.
- **Decision.** Each question is closed as follows.

  | Where it was left | The question | The answer |
  |-------------------|--------------|------------|
  | D116, section 14 of the architecture | Does Raft with a leader-run fault schedule meet "no coordinator"? | Noor's items 1 and 7. Keep Raft and shrink the leader: it only orders results. Its other jobs move to all nodes, and the failure is chosen by a draw among the nodes. The rewrite to a leaderless method is rejected. See D158 for the draw, and D216 for the rest |
  | D120, section 14 | Is degraded mode's trade-off acceptable? | Yes. The class notes ask for one server left to handle the clients, and the spec's clause on speculative action says to label it and reconcile. The demo states the trade-off first, as its own script says |
  | D126, D131, section 14 | Are group communication and publish-subscribe expected in Phase 1? | Lecture 9. Group communication is Phase 1 (D157, D210). Publish-subscribe is Phase 2 (D209) |
  | D119, section 14 | Does tarpc suit the remote invocation question? | Yes. The Oct 4 note asks for a ready-made RPC, and Noor's item 9 says tarpc between the nodes. Client PCs go over UDP: see D216 |

  The status value `Pending approval`, which no entry ever used, is removed from this file's table, because nothing here waits for the instructor.
- **Why.** Noor's rule. A stated reading with its source can be checked at the demo. An open question can only wait.
- **Consequences.** If a reader disagrees, the report still shows the source and the reasoning. Citations of what the notes say stay, including the demo scenes named after Dr Amr's failure
  cases: those are what he asked to see, not questions.
- **Where.** `docs/architecture.md` section 14, `docs/demo.md`, `README.md`, the status lines of the old entries.
- **How we check it.** A search for "Dr Amr" in the live docs finds only citations and the named scenes. In this log it also finds old entries' text, each closed here.
- **Report notes.** Use the answers above in the report's question list. Do not write any of these as open.

### D214 · Test plan corrected: FT-2's target, FT-9, and names nobody had defined

- **Date:** 2026-11-03 · **Part:** A, C, D · **Status:** Implemented in the docs · **Decided by:** the agent on its recommendation, on Noor's instruction to finish without more questions
  (2026-11-03). Noor can overrule it
- **Context.** `docs/architecture.md` section 11.2 had three kinds of error.
  1. *FT-2 said "failover within 600 ms".* That cannot pass. openraft waits `election_timeout + leader_lease` once a vote is committed (`core/raft_core.rs:1475-1476`), and `leader_lease` is
     `election_timeout_max`, 300 ms (`engine/engine_config.rs:56`). D115 gives nodes 1, 2 and 3 the ranges 150–225, 300–375 and 450–525 ms, on a 75 ms tick. So with node 1 gone, node 2 stands at about 600 to 750 ms
     before any vote. In the one-PC power-cut rehearsal, on the build from before tonight's changes (commit a98a1c3; three 90 s runs and one 300 s run, about 90 reports a second, node 1 leading and cut), the first candidate came at +0.77 to +0.81 s, the new leader at +0.82 to +0.94 s, the first
     answer to a new request at +0.84 to +0.94 s, and the longest time with no answer to anyone was 0.84 to 0.98 s. 150–300 ms also appeared in sections 6.1, 6.8, 8.2 and 9.2.
  2. *FT-9 said "dropped for Phase 1 (D24)",* while section 12 said it is back in scope, D118 superseded D24, and `tests/durable.rs` covers it. Lines for FT-3, FT-4, FT-5 and FT-8 still said they needed parts that are now built.
  3. *LB-1, LB-3 and P-1 were cited but defined nowhere in the repo.*
- **Options considered.**
  - *FT-2's target:*
    1. *0 lost, and a new leader and the first new answer within 1 s, checked on the lab machines in E3.* Chosen: it matches the rule above and the rehearsal, with a little to spare.
    2. *Keep 600 ms and shorten the timers.* The lease and D115's ranges would both have to change, against D115's reason, while the leader is already the thing under repair (D133).
    3. *No number.* The test could then never fail.
  - *The undefined names:* define them from what the log already says each one checks, or delete them. Defining keeps the citations in D2 and D3 meaningful.
- **Decision.** FT-2 passes on 0 lost, with a new leader and the first new answer within 1 s. FT-9 is a live test: under load, a node is cut hard and restarted; it passes on 0 lost and equal digests. The expected times in the architecture now read about 0.45 to 0.85 s. LB-1 is even
  shares under bidding at each load in E1. LB-2 is one overloaded node, as D83 defines it. LB-3 is bidding against leader dispatch, as D3 says. P-1 is E1's latency and throughput sweep. P-2 is the bid window sweep, as D83 defines it.
- **Why.** A target that cannot pass teaches nothing, and D115 itself said E3 measures the real figure.
- **Consequences.** The report states 1 s, says it came from the rule above, and gives the lab figure. The rehearsal figures are one PC only, and the rehearsal did **not** test losing unsynced disk writes, because the one kernel keeps its page cache across the cut. A real power cut on the
  six PCs is still to do.
- **Where.** `docs/architecture.md` sections 6.1, 6.8, 8.2, 9.2 and 11.2, `docs/protocol.md` timers.
- **How we check it.** E3 for FT-2. `tests/durable.rs`, `tests/restart_log.rs` and scene 5 of the demo for FT-9.
- **Report notes.** Give the openraft rule, because it explains the number. It is a fair "found by testing" point.

### D215 · The library boundary names tarpc

- **Date:** 2026-11-03 · **Part:** all · **Status:** Implemented in the docs, amends D68 · **Decided by:** the agent on its recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** D68 and AGENTS.md coordination rule 6 say openraft is the only library doing distributed-systems work. D119 added tarpc for remote calls, so the rule had been false since then, and the report must separate library code from ours.
- **Options considered.**
  1. *Name both in the rule: openraft for Raft, tarpc for remote calls.* Chosen.
  2. *Argue that tarpc is not distributed-systems work.* Remote invocation is exactly what the course calls it.
- **Decision.** Option 1. Anything new of that kind still needs team agreement and a design-log entry.
- **Why.** The rule exists so the report can be honest about what is a library.
- **Consequences.** None for the code.
- **Where.** `AGENTS.md` coordination rule 6, `README.md`, `docs/demo.md`.
- **How we check it.** Reading them. `Cargo.toml` lists only these two of that kind.
- **Report notes.** State both in the library table.

### D216 · Noor's decisions of 2026-11-03, and those still being built

- **Date:** 2026-11-03 · **Part:** all · **Status:** Accepted, and built. Its table says what is on `shared-phase1-gaps` now · **Decided by:** Noor Hany Emam, with the agent's picks in item 11 on Noor's
  instruction to finish. Noor can overrule any pick
- **Context.** Noor settled the remaining questions in one sitting. Their own words on the deadline: finished in an hour, and whatever passes tests by then goes in the PR (item 12). Some builders had not finished when this entry was first written, so it held their record until their own entries landed. They have landed, and the last column below is brought up to date. Nothing has run on the six lab PCs yet, so "built" means in the code and passing its tests.
- **Options considered.** Each item's rejected options are in the table.
- **Decision.**

  | Item | Decided | Rejected | Recorded in | State on this branch |
  |------|---------|----------|-------------|----------------------|
  | 1 | No coordinator: keep Raft and shrink the leader. It only orders results. Picking who fails and loading the responder list move to all nodes | Replacing Raft with a leaderless method: a rewrite one day before the deadline | D158 for who fails; D163–D166 for loading the list and what the leader's role is | built: D158, D163 to D166. D163 states the short form exactly: the leader orders every command, sends the copies and answers one read |
  | 2 | A small web page on the client PC | A desktop window app; a terminal menu | D197 | built |
  | 3, 4 | One box for user processes, 10 real programs sharing the users; steady, random and all-at-once | 1000 real programs; 1 program with light users | D181, D182 | built |
  | 5 | Too old means not planned, 60 s | A stored label nothing reads | D143 | built |
  | 6, 14 | Publish-subscribe: small, then Phase 2 | see D209 | D209 | Phase 2, not on this branch |
  | 7 | Fault election by random draw: every 30 to 60 s each node draws a number and sends it to the other 2. The highest goes silent for 1 to 20 s. A node missing any of the 3 numbers does nothing, so two cannot go down at once. No leader is involved | Saving the result in Raft: it goes back through the leader | D158–D162 | built. `config/lab.example.toml` ships its timers never firing, and its comments say how to turn the draw on (D165) |
  | 8 | A named group with a who's-alive list | Our own ordering and delivery; only naming what exists | D152–D155, D210 | built |
  | 9, 11 | Client PCs to the cloud over UDP, "group communication within machines", tarpc between them. Reverses D17. The way: tarpc over a small datagram adapter. `ctl` stays on tarpc over TCP, so a lost silence order cannot vanish | coap library; a small request-reply of our own | D167–D180 | built: reports go through tarpc over UDP, `ctl` through tarpc over TCP. Run on loopback only: no real switch or network card yet, and no run with the receive buffer raised (D171) |
  | 10 | Queue with grouped saves | see D133 | D133 | built |
  | 11 | Saves stay on in measured cells unless the queue numbers say otherwise. One branch and one PR, a commit per gap, since one person merges | | here | in force |
  | 12 | Everything is time-boxed to about 20:00 on 2026-11-03. What passes tests by then goes in the PR, the rest is left out and listed, publish-subscribe first | | here | applied: publish-subscribe is left out (D209) |
  | 13 | The document carries UML models, decisions with justification and assessment, changed decisions with their alternatives, who did what, all five scenarios, the stress testing, and the measurement analysis | | the report | in progress |
  | 14 | What Lectures 8 and 9 changed | | D209–D212, D156 | built, with the cloud side's file split (D156) |
  | 15 | The Raft leader does not change with load. The bid already moves work by load, for every report | Load-based leader change | D217 | in force, and no code was needed |
  | 16 | A small safe step-aside for a busy leader, kept on its own branch | | D218, only if it goes in | on `lane-handover`, not on this branch. It goes in only if Noor says yes. If the team later says Dr Amr asked for it, D218 amends D217 |

- **Why.** The decisions list says these become log entries, and AGENTS.md says every decision goes in the log. A short record beat a gap while the builders were still working, and the table is now the short list of what is built.
- **Consequences.** Items 1 and 9 landed, so their rows point at their entries, and D167 marks D17 as superseded. D156 landed last.
- **Where.** `docs/architecture.md` sections 2, 5.6, 6.11 and 14.
- **How we check it.** `git grep UdpSocket` and `git grep spawn_campus_seeding` for items 9 and 1, and `ls src/node/service.rs` for D156.
- **Report notes.** The report's changed-decisions list starts from this table.

### D217 · The Raft leader does not change with load

- **Date:** 2026-11-03 · **Part:** A, B · **Status:** Decided, amended by D218 · **Decided by:** Noor Hany Emam
- **Context.** A teammate read the requirement as: if the leader is busy and the other nodes are not, a new election should happen. The spec asks for the cloud peers to elect which peer performs the work, with load among the criteria (spec lines 142-144), and to show different peers elected as load changes (spec line 245). It does not name the Raft leader. There are two elections in this system and they are easy to mix up: the bid for each report, and the Raft vote for the leader.
- **Options considered.**
  1. *Keep it as it is.* The bid picks the worker by load for every report. The Raft leader changes only when the others stop hearing from it.
  2. *Change the leader with load.* A busy leader goes quiet on purpose so another node takes over. Rejected: moving the leader moves the ordering of results, not the planning, because the leader only plans a report if it wins the bid. Each change costs about 0.8 s with no answers in one-PC rehearsals, never six-PC results. openraft 0.9.25 has no hand-over command (`trigger()` offers elect, heartbeat, snapshot and purge_log), and followers refuse votes while the leader's lease holds, so a healthy leader would have to stop its heartbeats. Under heavy load that could repeat and bring back the vote storm that D133 fixed.
- **Decision.** Option 1.
- **Why.** A busy node already loses bids, so work moves away from it without changing the leader. The requirement is met by the bid.
- **Consequences.** Demo scene 1 and the report show wins per node as the evidence that the election reacts to load, not Raft terms. A steady term under load is the healthy result. If a recording or class note says the Raft leader itself must follow load, that overrides this entry.
  **D218 is kept free for that.** Later the same evening Noor heard that the Raft leader itself should change when overloaded. The class notes, the spec and Lectures 8 and 9 do not say so, and the recordings are not on hand. So a small safe step-aside is kept off this branch, on `lane-handover`, and it goes in only if Noor says yes. If the team later says Dr Amr asked for it, D218 amends this entry; until then this entry stands, and no such code is on this branch.
  **One correction to the reasoning above, also from that evening.** Moving the leader does not share planning, but every result passes through the leader, so an overloaded leader machine slows every commit, and moving it would help in that case. The rest of the rejection stands.
- **Where.** `src/bidding.rs`, `src/node/bids.rs`, `src/raft/mod.rs`.
- **How we check it.** Wins per node across load changes and under `ctl skew`; terms staying steady under load.
- **Report notes.** Answer this question directly in the section on how the leader is chosen and how the worker is chosen.

### D218 · A busy leader steps aside, and a normal vote picks another

- **Date:** 2026-11-03 · **Part:** A, with B and C · **Status:** Implemented, amends D217, amended by D228 · **Decided by:** Noor Hany Emam, on a teammate's report that Dr Amr asked for it in
  class. The written notes and Lectures 8 and 9 do not contain it, and the report must say so. Noor's reason, their words: "maybe the raft leader would be too busy to organize". The smaller
  choices inside it (the signal, the numbers, what counts as safe) were picked by the agent on Noor's instruction to take the best choice and not ask, and Noor can overrule any of them.
- **Context.** D217 kept the leader where it is under load, because the bid already moves planning away from a busy node. Later that evening a teammate reported that Dr Amr had said an overloaded
  Raft leader should change through a new election. We searched the class notes, the spec and Lectures 8 and 9, and none of them says this. The recordings are not on hand. Noor decided to build it. The reason holds without
  the report: every result passes through the leader, so an overloaded leader machine slows every commit, and the bid can move planning off it but not the ordering. D217 had this part wrong.
  The hard part is that openraft 0.9.25 has no hand-over command (`trigger()` offers elect, heartbeat, snapshot and purge_log), and followers refuse votes while the leader's lease holds.
- **Options considered: how the leader moves.**
  1. *Go quiet (chosen).* The leader turns its heartbeats off through `runtime_config` and holds new writes in the proposal queue. Nothing then resets the followers' timers, and an ordinary vote picks the new leader. No message, timer or vote rule changes.
  2. *Tell a follower to stand, with `trigger().elect()`.* Rejected: it needs a new message, so a wire change, and the third node still refuses votes while its lease holds, so the leader would have to go quiet anyway.
  3. *Make timers or votes follow load, so the least busy node wins.* Rejected: this changes Raft's own rules, and the ranges in D115 are what stops split votes. Letting the fastest timer win is simple and already tested.
  4. *Shut down the leader's Raft and start it again.* Rejected: it loses writes in flight and costs a catch-up, where going quiet costs about a second and nothing else.
  5. *Take the leader out of the membership and put it back.* Rejected: two log entries for each move, and D25 keeps all three as voters. A crash in the middle would leave two voters.
  6. *Stay with D217.* Rejected by Noor.
- **Options considered: what "overloaded" reads.** One-PC rehearsal numbers are in the table below.
  1. *The bid alone (first draft).* Rejected: it can miss the case that matters. A leader starved of CPU never reached 3 times its peers' bids, at most 2.51 (2.46 in another run), although every commit got about 10 times slower. Bids even out, because a loaded node loses rounds, and a slow log also fills the peers' workers, so their bids rise with it.
  2. *The queue's length.* Rejected: it does not tell the cases apart. `waiting` was at most 148 at high even load and at most 70 with a starved leader, and it drains in bursts, because grouping sends everything at once.
  3. *The machine's CPU, `cpu_permille`.* Rejected: a leader has only its own number unless a new field goes on the wire, and a machine busy with its own planning is what the bid already covers.
  4. *A new wire field with each peer's log wait.* Rejected: a wire change a day before the deadline, and a follower has no queue to compare.
  5. *The log wait, mean per proposal, compared with the peers' bids (chosen).* A proposal's time from entering the queue to being answered. It is one counter, in the same unit as the bid, and it sees a slow CPU and a slow disk alike.
  6. *The log wait over a fixed floor alone.* Rejected: a healthy wait is 3 to 4.3 disk saves on any disk (3.1 at 300 users, 4.3 at 3000), so on a slow lab disk of 25 ms it would be about 80 to 110 ms, over any floor low enough to catch the starved leader, and the leader would move once a minute for nothing. So the wait must pass 40 ms and also 5 of the node's own saves.
  7. *A floor of 100 ms.* Rejected, tried first: a starved leader's mean wait ran from 45 to 230 ms, mostly under 100, so a run of 5 s in a row never came. Longest run: 2 s.
- **Decision.** Option 1 for how, option 5 with the two bars for what.
  - **Who and when.** Only the leader acts. Once a second it reads the last second: each node's mean bid, and its own log wait and disk save. A small pure function decides (`overloaded`, `leader_load`, `Safety::holds`).
  - **Overloaded.** Its load is its mean bid, or its log wait if that is higher and over both bars (40 ms and 5 saves). Under `queue-only` and `cpu-only` the score is not a time, so the bid alone counts. Overloaded means load at least `leader_overload_ratio` times the lowest peer's mean bid, and over a floor (1 ms, 8 jobs or 500 permille), for `leader_overload_for_ms` without a break. Only this second's bids count, so a stale score cannot decide it, and a second with no bid from any node breaks the run.
  - **Safe.** Serving, not degraded, not silenced, all members in the group view, no peer suspected, no silence the failure draw named still going on, every peer bid in that second (a node that is not serving does not bid), and at least `leader_overload_cooldown_ms` since the last term change.
  - **How.** Log `leader_stepped_aside`, heartbeats off, queue held. Bids and pings go on. The old leader sees the higher term, steps down, and turns heartbeats back on at once. If no new leader appears within `QUIET_LIMIT`, 3 s, it turns them back on, carries on, warns why and waits a full cooldown. It logs `leader_step_aside_ended` with `changed`, `re_elected`, `timed_out` or `stopped`.
  - **Keys, in `[policy]`.** `leader_overload_ratio` 3, 0 turns it off, otherwise at least 2. `leader_overload_for_ms` 5000. `leader_overload_cooldown_ms` 60000, at least 3000. Under leader dispatch there are no bids, so it does not run.
  - **Where it is on.** `config/local.toml` and `config/lab.example.toml`. Off in every experiment cell, so measurements stay comparable, except the new `e8-step-aside`. `tests/experiment_cells.rs` holds both to that.
  - **No wire change.** `proto.rs` is untouched: the leader reads the bids it already receives and only stops sending.
- **Why.** It meets the requirement with no change to Raft's safety: all it can cause is an election, and one can happen at any time. The numbers: at even load the leader's bid was at most 1.10 times its lowest peer's, and at high even load at most 1.20, so 3 is clear of both. A short burst does not move it, because it needs 5 s without a break. A move costs about a second with no commits, so a 60 s cooldown limits that to under 2% of the time in the worst case, and it is what stops a storm: every node starts it again on every new term. The peers' bids are what keeps a cluster overloaded everywhere from moving: at 3000 users the log wait was at most 91 ms against peer bids near 50 ms, so the ratio was nowhere near.
- **Measured.** **One-PC rehearsal, never six-PC results.** Three nodes on their own 4 cores each and three load generators on one PC, with other work on the PC, loopback, disk saves on, `plan_work` 1000, think 1000 ms, reports over UDP, one run each, on commit `437ca9f`. 300 users means three sets of 100, with 300 responders.

  | Run (all with disk saves on, one run each) | Leader moved | Gave up | Commit p50 / p99, ms | Answer p50 / p99, ms | Leader's log wait per second, ms: p50 / p90 / max | Its disk save, ms | Its bid over the lowest peer's: p50 / max | Seconds at 3 times or more: bid alone / bid or log wait |
  |-------|--------|--------|--------|--------|--------|--------|--------|--------|
  | (a) Even, 300 users, 60 s, cooldown 5 s so the rule is armed nearly all of it | no, the term stays | 0 | 7.5 / 12.4 | 31.0 / 45.7 | 7.5 / 8.1 / 10.1 | 2.4 | 1.02 / 1.10 | 0 / 0 |
  | (d) Even, 3000 users, 3000 responders (D219), 60 s, cooldown 5 s | no, the term stays | 0 | 19.7 / 39.1 | 91.5 / 2317 | 18.4 / 20.6 / 29.7 | 4.3 | 1.00 / 1.16 | 0 / 0 |
  | (d) The same with 300 responders | no, the term stays | 43, from the small pool (D219) | 19.1 / 34.9 | 369 / 14205 | 17.7 / 19.9 / 21.2 | 4.3 | 1.00 / 1.20 | 0 / 0 |
  | (b) Skew 20000 on the leader, every default, 140 s: before | | | 17.5 / 93.3 | 41.5 / 123.6 | 16.8 / 26.7 / 72.3 | 5.3 | 1.02 / 1.11 | 0 / 2 |
  | (b) Skewed, not moved yet | | | 16.6 / 23.7 | 40.8 / 80.2 | 15.8 / 16.2 / 16.2 | 5.1 | 3.71 / 3.93 | 8 / 8 |
  | (b) After the move | once | 0 | 12.4 / 28.6 | 37.0 / 105.3 | 8.8 / 17.4 / 20.7 | 2.7 | 1.00 / 1.07 | 0 / 0 |
  | (c) 32 busy loops on the leader's 4 cores, cooldown 15 s, 70 s: before | | | 8.3 / 18.6 | 32.3 / 55.5 | 7.9 / 12.1 / 15.4 | 2.6 | 1.02 / 1.47 | 0 / 0 |
  | (c) Starved, not moved yet, 6 s | | | 86.3 / 357.9 | 146 / 722 | 94.0 / 153 / 153 | 10.7 | 1.48 / 2.51 | 0 / 6 |
  | (c) After the move, node 1 still starved | once | 0 | 12.0 / 61.3 | 38.4 / 245 | 11.6 / 33.2 / 45.1 | 3.3 | 0.99 / 1.03 | 0 / 0 |
  | (c) The same with ratio 0, so it never moved | no | 0 | 73.3 / 300.5 | 125 / 648 | 64.5 / 119.7 / 158.2 | 10.0 | 1.56 / 1.98 | 0 / 34 |

  **(a) and (d).** No step-aside and no new term. Digests equal at the end of every run. An earlier run of the 3000-user case, with more other work on the PC, had a log wait of 24.4 / 36.4 / 91.3 ms, and still no change, because the peers' bids were near 50 ms.

  **(b).** The skew came at +70 s, past the 60 s cooldown. The leader went quiet about 8.5 s later, since it takes a second or two for the skewed plans to show in the bid. Node 2 led term 3 after 759 ms. The longest time with no answer anywhere was 771 ms. Nobody gave up, 26 reports were retried, and the digests were equal. The term stayed at 3 for the 60 s left. Wins moved away: node 1 won 6771 of 20057 before the skew, and 16 of 20031 after it. Commit latency hardly changes in this case, because a skew slows planning and not the log: it shows the mechanism, and (c) shows the benefit. The 2 seconds over before the skew are a stall from other work on the PC, and they show why it needs 5 s in a row.

  **(c).** Does commit latency improve after the change? Yes. p50 went from 8.3 ms to 86.3 ms when the leader's cores were starved, and back to 12.0 ms once node 2 led, with node 1 still starved. p99 went from 18.6 to 357.9 and back to 61.3 ms. Left where it was, the leader stayed at 73.3 and 300.5 ms. The leader went quiet about 6 s after the busy loops started. Node 2 led after 876 ms, the longest time with no answer was 733 ms, 4 reports were retried, nobody gave up, and the digests were equal. The bid alone would never have fired here: 0 seconds. A smaller skew does not fire either: at 5000, in a 45 s run, the bid moved the work (node 1 won 341 of 7159 after it, against 1963 of 5889 before), and the leader's bid stayed about 2 times its peers', at most 2.05, so it stayed leader.

  **Why these bars, from the table.** A healthy wait stayed under 40 ms in every second of (a) and of (d) with 3000 responders, and came to 3.1 and 4.3 saves. A starved leader's wait stayed over 40 ms and over 5 saves for the 6 s that decided it. The mildest of those seconds was 44.3 ms with saves of 5.8 ms. That margin is thin, and a dip below it only starts the count over, so a less starved leader stays put. That is the safer way to be wrong.
- **Consequences.**
  - D217's "the leader does not change with load" no longer holds under overload. A steady term under *even* load is still the healthy result, and wins per node are still the evidence that the election follows load.
  - The vote knows nothing of load. The follower whose timer ends first wins, so node 2 takes over from node 1, and node 1 from anyone else (D115). If two machines are busy the leadership can move between them, once per cooldown at most.
  - The log wait cannot tell whose machine is slow: if both followers are starved and the leader is not, the wait rises too. The peers' bids usually rise with it, and the cost is capped at one move per cooldown.
  - The first 60 s after a start or a leader change cannot move it, and neither can a time with no users, because no bids means no reading.
  - A leader killed while quiet leaves nothing behind: going quiet lives only in the process, so after a restart heartbeats are on. A leader silenced while quiet ends its try at once.
  - A step-aside shows once in the followers' `max_leader_gap_us`, for about as long as the move took.
  - `raft_io` gains `answered` and `answered_us`. `analysis/analyze.py` does not read the two new events yet, and `elections` already counts the new term.
- **Where.** `src/node/step_aside.rs`, `src/raft/mod.rs` (`set_quiet`, `log_work`), `src/raft/proposals.rs` (`hold`, the answered counters), `src/node/bids.rs` (`saw_bid`, `take_mean_scores`), `src/node/faults.rs` (`drawn_silence_going_on`), `src/config.rs`, `src/metrics.rs`, `scripts/watch_events.py`, `experiments/e8-step-aside.toml`. It sits in the middleware: `tests/layers.rs` keeps it out of the application.
- **How we check it.** Unit tests of the rule in `node/step_aside.rs`. `tests/step_aside.rs`: `ctl skew` on the leader changes it once; even load changes nothing; ratio 0 is off; the cooldown holds back a second change; a try nobody answers times out and the leader carries on; a silenced peer stops it until that peer is back; a leader silenced while quiet, and one killed while quiet and restarted, both go on with heartbeats on; every node knows of a drawn silence until it is over. `tests/experiment_cells.rs` for the cells and configs. The rehearsal above. It has not run on the six machines.
- **Report notes.** Say plainly that this rests on a teammate's report of what Dr Amr said, that the written notes and Lectures 8 and 9 do not contain it, and that D217 said otherwise first. There are two elections: the bid for each report, and the vote for the leader, and this entry only touches when the second one runs. Show the table above as a one-PC rehearsal. The honest limit is that the new leader is whoever's timer ends first, not the least busy node.

### D219 · 3000 responders in the top E1 cells

- **Date:** 2026-11-03 · **Part:** E, F · **Status:** Implemented, amends D129 · **Decided by:** the agent on its
  recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** **One-PC rehearsal, never six-PC results.** Three nodes and three sets of client programs shared one PC, one disk and loopback, with TCP
  clients, on commit `404167f`, with other work on the PC. Each row is one run of 5 s warm-up and 25 s measured. These numbers show which
  way a change moves the system, not how far it moves on the lab machines.
  D129 gave every E1 cell 300 responders, which by D111's arithmetic allows about 500 assignments a second. It
  expected that at high load "plans increasingly have no responder", and that `no_responder_share` would report it. The local
  run on `404167f` found something worse than that. In every overloaded cell with 300 responders, "answered with a responder"
  topped out at 348 to 415 a second. Plans read the free list and then wait for their commit, so many pick the same few
  responders, one wins and the others go stale (D28). Stale plans per committed result climbed from 0.07 to 0.17 at 300 offered
  to 5 to 9 at 3000 offered. Each loser plans again, and the extra plans fill the worker queue. So offering more got less
  answered: steady went from 716 to 805 answered a second at 1000 offered, down to 350 to 450 at 3000 offered, with p99 at 20 to
  25 s and 310 to 575 give-ups in each run. The top of E1 was measuring a pool chosen too small, not the election or the planner,
  which is what E1 is for.
- **Options considered.**
  1. *3000 responders in `e1-1000-*` and `e1-3000-*`, config only (chosen).* Nothing in the code changes this late. Measured
     at 3000 offered, one run: 1892 answered a second, all with a responder, p50 115 ms, stale per committed 0.96, 1 give-up,
     against 350 answered, p50 2309 ms, 7.03 stale and 557 give-ups. p99 is still 6.5 s and 6531 retries remain, so 3000 offered
     is still past what one PC keeps up with. It does not fix the race itself, only stops the cell from running into it first.
  2. *3000 responders in every E1 cell.* One pool across the whole sweep, so a figure has no change of setting in it. But it
     moves the cells below the knee, which already behave (`e1-300` was healthy on 300 responders), and throws away the point
     where the pool starts to matter.
  3. *Leave it at 300, and report it as what overload looks like.* Honest, and needs no change. But the top two load levels of
     E1 would then all read as the same pool limit, and the scaling figure would say nothing about the mechanism it is for.
- **Decision.** Option 1. `campus.responders = 3000` in `experiments/e1-1000-bidding.toml`, `e1-1000-leader.toml`,
  `e1-3000-bidding.toml` and `e1-3000-leader.toml`. Both policies change, so bidding and leader dispatch still compare at the
  same load on the same pool. `e1-30-*`, `e1-100-*`, `e1-300-*` stay at 300, and every other cell is unchanged.
- **Why.** The pool is an experiment setting, not the system under test, and D129 chose it to be a real limit "at the top".
  The measurement shows it is the first limit by far, ahead of the disk and the CPU. Config only is the smallest change that
  lets the top cells measure something else.
- **Consequences.**
  - **The sweep now changes a setting between 300 and 1000 users.** A figure of E1 must mark this, and no line across that gap
    may be read as the effect of load alone. Comparing bidding with leader dispatch at one load is unaffected.
  - D129's "above 500 requests per second, plans increasingly have no responder" no longer describes `e1-1000` and `e1-3000`.
    The knee from responders is now only reachable in cells that keep 300 and offer more than 500 a second.
  - **Those cells remain, and are left as they are:** `e3e4-1500-*`, `e4-1500-*` and `degraded-1500` still run 1500 users on 300
    responders, so they run in the race this entry describes. The task was the top E1 cells, so they are not changed here. Their
    fault and recovery figures must be read with that in mind, or their pool raised by the same reasoning after a first look at
    lab data.
  - Each plan copies the free list under the state lock, so a pool ten times larger makes that copy ten times longer. The
    1892 a second above includes this cost.
  - The demo's value in `config/lab.example.toml` stays 300 (D205), since the demo runs well below 500 a second.
- **Where.** The four cell files. `tests/experiment_cells.rs::the_top_e1_cells_have_responders_for_their_load` holds the values
  on both sides of the change.
- **How we check it.** On the lab machines, in the summary of each top E1 cell: `no_responder_share` near 0, and throughput well
  above 500 a second. Stale plans per committed result are in the node logs, not in `summary.csv`.
- **Report notes.** State the two pools beside the E1 figure and why they differ. The 350 to 450 against 1892 is a good sentence
  on what a badly chosen setting does to a figure, but only as a one-PC rehearsal number. It was also measured with the code
  before D220 and with TCP clients. The one run with both and UDP is under the table after D220.

### D220 · After three stale plans, commit that nobody could be sent

- **Date:** 2026-11-03 · **Part:** A, E · **Status:** Implemented, amends D35 and D36 · **Decided by:** the agent on its
  recommendation, on Noor's instruction to finish without more questions (2026-11-03)
- **Context.** **One-PC rehearsal, never six-PC results.** Three nodes and three sets of client programs shared one PC, one disk and loopback, with TCP
  clients, on commit `404167f`, with other work on the PC. Each row is one run of 5 s warm-up and 25 s measured. These numbers show which
  way a change moves the system, not how far it moves on the lab machines.
  Under D35 and D36, a node whose plan goes stale three times stops and says nothing, and the client's retry covers
  it. The local run on `404167f` showed what that costs when plans race. At 1000 offered, stale plans per committed result were
  1.97, and each silent loser meant one of two things. The runner-up took over at 200 ms (D110), which started the same race
  again: 6710 backstop takeovers in 25 s, with no fault in the run. Or the client sat through its 2 s retry wait: 2105 retries, p99
  5.9 s. At 3000 offered that became 28081 takeovers and 21000 to 39000 plans refused by a full worker queue, because the retries
  and takeovers were themselves load. Silence had turned losing a race into more racing. D36 did foresee this ("revisit only if
  load tests show retry storms"), and these are those load tests.
- **Options considered.**
  1. *Stay silent, as D35 and D36 say.* No change, and a report that loses three times may yet get a responder on its retry. But
     the wait is the whole cost: p99 5.9 s at 1000 offered, and the retry is what feeds the race.
  2. *Commit a plan with nobody on it after the third stale plan (chosen).* About 20 lines in `execute`. Measured at 1000 offered,
     one run: 878 answered a second, p99 **215 ms** instead of 5916 ms, **0 retries** instead of 2105, 406 a second assigned against
     401, so the same assignment rate. p50 rose from 69 ms to 148 ms. At 3000 offered: 1110 answered against 350, 415 assigned
     against 348, p99 12.8 s against 24.7 s, 16 give-ups against 557.
  3. *Send `ReplyBody::Rejected` and let the client back off.* D36 kept this open. It answers the client as fast, with no entry in
     the log. But a "no" from one node says nothing about the other two, so the client still has to wait for them or retry, and it
     needs a backoff rule, and the protocol and client must change this late.
  4. *Plan more than three times.* More work on plans that mostly go stale, on a worker pool already full. It moves the race, and
     it does not end it.
- **Decision.** Option 2. In `node/mod.rs::execute`, once `MAX_PLAN_ATTEMPTS` plans have come back `Stale`, the node runs
  `planner::plan` on an empty list with the default knob and proposes that through Raft as an ordinary `CommitResult`. It answers
  from what commits: `Committed` or `Duplicate`, the same as any other result. `ExecuteError::StillStale` is gone. `MAX_PLAN_ATTEMPTS`
  stays 3. Nothing is added to `proto.rs` and no config key changes.
- **Why.** The plan names nobody, so `CloudState::commit_result` has no responder to find held and cannot return `Stale`: this
  commit ends the race for this request. It is still a commit, so "commit, then reply" holds, and first commit wins still decides
  between it and any other executor's result. An empty list means no search and `burn(0)`, so it runs on the async task without
  breaking "never block the runtime", and without a worker job it cannot be refused by the very queue that is overflowing. Clients
  already handle a plan with nobody on it. It is what a report gets when the list really is empty, so the client and the status
  rule need no change.
- **Consequences.**
  - **A report can hear "nobody free" while a responder is about to free up.** Before, that report could still get a responder on
    a later retry. This is the trade: an answer now, for a chance later. At the rates measured the assignment rate did not fall (406
    against 401 a second), so the same number of reports get help, and the others hear sooner. The one-PC numbers cannot show
    whether any one report does worse.
  - **That answer is final for that request ID.** First commit wins, so a client's retry gets this same result, and a standby's plan
    with a responder that arrives later is discarded as a duplicate. The report's status stays Unassigned, and nothing assigns it
    later, exactly as for any plan with nobody on it.
  - **`no_responder_share` now counts two things:** the list was empty, and the plans lost the race three times. It rises in overload,
    as D129 expected and the earlier code did not deliver. The two are not told apart in the logs: no new event was added this late.
    The stale count in `CloudState::counters` shows how much racing there was.
  - **Overload now looks different in the figures.** It shows as a higher `no_responder_share` at low latency, where before it showed
    as long latency and give-ups. Comparing a cell with the runs from before this entry would mislead.
  - p50 rises under contention (69 ms to 148 ms in the one run), because a report that once waited elsewhere now commits one more
    entry.
  - D36 stays for its other cases: a node that is not serving, is not the executor, finds its worker pool full or cannot commit
    still sends nothing. Plans refused by a full queue therefore still cost a retry (21000 to 39000 at 3000 offered before this
    entry), which is why 3000 offered still has a 12.8 s p99 and 9735 retries on 300 responders.
  - `degraded::plan` keeps its own three tries and stays quiet after them (`PlanError::StillTaken`). A lone node has no commit to
    wait for and no peers to race, and that path was not in these measurements, so it is left alone.
- **Where.** `src/node/mod.rs` (`execute`, `MAX_PLAN_ATTEMPTS`, `ExecuteError`). Tests: `three_stale_plans_are_answered_with_nobody_sent`
  and `a_plan_that_wins_on_its_last_try_keeps_its_responder` in `node/mod.rs`, on a scripted consensus that can answer `Stale`
  (`ScriptedConsensus::make_stale`, `proposed_plans` in `node/testing.rs`, and `StateReader::apply_for_tests` for a state with free
  responders), and `plan_naming_nobody_commits_when_every_responder_is_held` in `state.rs`, which holds the property the path relies on.
- **How we check it.** The tests above. In a cell past the knee, `retried` near 0 and p99 near the commit time, with
  `no_responder_share` above 0. No real test can force three stale plans in a row, so on real nodes the path has been exercised only
  by the load run behind the numbers above, not by a test.
- **Report notes.** Give the trade in one sentence beside the overload figure. Say this replaced a silent stop, because a reader of
  D35 and D36 would expect silence. The numbers are one-PC rehearsal numbers from one run each.

### The one-PC rehearsal behind D219 and D220

**One-PC rehearsal, never six-PC results.** Steady pattern, one run each, on `404167f`, with TCP clients and disk saves on. Each row changes one
thing against the first sweep. "Answer after 3 stale plans" is D220 and "3000 responders" is D219. Each was measured alone here.

| Users per set | Change | Offered /s | Answered /s | Answered with a responder /s | p50 ms | p99 ms | Gave up | Retried | Stale per committed |
|---|---|---|---|---|---|---|---|---|---|
| 333 | none (first sweep) | 999 | 716 | 401 | 69 | 5916 | 0 | 2105 | 1.97 |
| 333 | saves off | 999 | 916 | 413 | 24 | 2124 | 0 | 572 | 0.37 |
| 333 | 3000 responders | 999 | 902 | 902 | 47 | 1941 | 0 | 250 | 0.22 |
| 333 | answer after 3 stale plans | 999 | 878 | 406 | 148 | 215 | 0 | 0 | 1.92 |
| 1000 | none (first sweep) | 3000 | 350 | 348 | 2309 | 24660 | 557 | 5524 | 7.03 |
| 1000 | 3000 responders | 3000 | 1892 | 1892 | 115 | 6531 | 1 | 6531 | 0.96 |
| 1000 | answer after 3 stale plans | 3000 | 1110 | 415 | 308 | 12753 | 16 | 9735 | 3.21 |

The whole-PC picture from the same rehearsal: healthy at 300 reports a second, starts to struggle at about 500, and stays safe all the way to 3000, with equal
digests and matching results on all three nodes in every cell of the sweep.

**Together, once.** One later run on the merged code had both, and UDP: `e1-3000-bidding`'s settings, 1000 users on each of 3 client sets, 5 s warm-up and 25 s
measured, disk saves on, the receive buffer capped at 425,984 bytes because the sysctl was not raised. 2801 answered a second, p50 56.5 ms, p99 187.8 ms, 0 gave up, 9
retried, no new term, `no_responder_share` 5.3%, 0.48 stale plans per committed result, 145 plans refused. One run on one PC, never a six-PC result. Nothing has measured
them on the six PCs.

### D221 · A live log viewer that only reads the event logs

- **Date:** 2026-11-03 · **Part:** all · **Status:** Implemented · **Decided by:** Noor Hany Emam asked for it ("we don't have visibility into what's happening"); the form is the agent's recommendation
- **Context.** While the system ran on the six machines the team could not see what it was doing. Node output shows leader changes, silences and who is alive, but not what happens to a report. The event logs hold all of it, one JSON object per line, which is hard to read live.
- **Options considered.**
  1. *A small script that reads the event logs and prints plain words.* No rebuild, and it cannot slow a node, because the node does not know it exists.
  2. *More log lines inside the nodes.* Rejected. A rebuild on six machines, and D134 measured what chatty output costs: 32 reports a second against 100 in the same one-PC test, with the node output on a slow disk.
  3. *A panel on the web page.* Rejected for now. More code in a program that must not touch measured paths, and the page only sees its own client PC.
- **Decision.** Option 1: `scripts/watch_events.py`, Python standard library only. By default it prints rare events as they happen (leader changes, the failure draw, silences, suspicions, who's alive, takeovers, catching up) and one summary line per program per second. `--reports` adds a line for every step of every report.
- **Why.** It gives the team and the demo a live view at no cost to what is measured.
- **Consequences.** It reads event names and field names from `src/metrics.rs`, so a renamed event needs a change here too. Each machine has only its own logs, so it shows one machine's view, and a full picture means one terminal per machine. Node logs are flushed in batches, so lines can lag a little. It adds nothing to analysis: measurements still come from `analysis/analyze.py`.
- **Where.** `scripts/watch_events.py`, `README.md`, `docs/runbook.md`.
- **How we check it.** Run on a real three-node cluster on one PC with a silenced leader, in `--once` mode: silence, suspicion, new leader, who's alive and catching up all printed correctly. Live mode was tried on a small hand-made log: old lines are skipped, new lines and new files appear. Not yet run on the six machines. No automated test.
- **Report notes.** Useful in the demo for showing what the system is doing, next to `ctl status`.

### D226 · The split as built: what D156 left to settle

- **Date:** 2026-11-03 · **Part:** A, C, E, F, all · **Status:** Implemented, amends D156 · **Decided by:** the agent on its recommendation, on Noor's
  instruction to finish without more questions (2026-11-03). Noor can overrule any of it.
- **Context.** Lecture 9 asks for the middleware and the application to be kept apart in the implementation as well as the design, on the cloud side and on the
  client side, and says it is asked about repeatedly. D211 made D156 required for that reason. D156 was written before the parallel work merged, so building it on
  the merged code left some things it did not settle, and some that had since changed:
  - `on_client_frame` no longer exists: it became a `RequestHandler` and a `ControlHandler` when the old client frames went (D175), so `handle_request` is the way in.
  - `may_reply`, `ExecuteError` and `MAX_PLAN_ATTEMPTS` are not on D156's list and had to go somewhere.
  - D156 names `bin/loadgen.rs` only, but D211 counts the web page as application too.
  - Neither D156 nor D211 says which layer `node/release.rs` is in.
  - D156 says `send_ordered` inherits a hang. With the proposal queue (D133) and D150, every path through `Consensus::propose` now has a deadline.
  - The cross-cutting note warned that a ban on `rescuemesh::node` would break `bin/loadgen.rs` if it read `node/usage.rs`.
- **What the three programs really use.** Read from the code, because the note above asked for it:

  | Program | From the library | Layer | How the test treats it |
  |---------|------------------|-------|------------------------|
  | `bin/loadgen.rs` | `MeshClient`, `Submitted`, `ClientError` | middleware | only through `rescuemesh::middleware` |
  | `bin/loadgen.rs` | `campus::random_incident` | application | direct |
  | `bin/loadgen.rs` | `config`, `proto` data types, `metrics`, with `metrics::machine_usage` | beside both | direct. D185's parsers are in `metrics.rs`, not `node/usage.rs`, so the trap did not bite |
  | `bin/frontend.rs` | `MeshClient`, `Submitted`, `ClientError`, `control` | middleware | only through `rescuemesh::middleware` |
  | `bin/frontend.rs` | `campus::responders`, `campus::unseeded_incident` | application | direct |
  | `bin/frontend.rs` | tokio's `TcpListener` and `TcpStream`, for its browser (D200) | its own | allowed, but `TcpStream::connect` is not: that could only be going past `MeshClient` |
  | `bin/frontend.rs` | no library at all to start users: it runs `loadgen` as a program (D202) | | nothing to check |
  | `scripts/start-users.sh` | no library at all: it runs `loadgen`, and adds up the first line of its summary | | nothing to check. `loadgen`'s flags and summary did not change |

  A ban on `TcpStream` in the page, the rule every other application file has, would go red on working code, which is why the socket rule differs there.
- **Options considered.**

  Which layer `node/release.rs` is in. It frees responders after their time on scene, reads the replicated state and commits one command.
  1. *Application, and the test guards it.* Chosen. It is a rule about responders. After the switch to `send_ordered` it names nothing below the line, so it costs one import.
  2. *Middleware.* Then someone has to explain at the demo why freeing a responder is communication.
  3. *Leave it out of the table.* The table is what a reader opens, and a missing file reads as a mistake.

  Which programs go through the interface.
  1. *`bin/loadgen.rs` and `bin/frontend.rs`, with `control` on the interface.* Chosen. D211 and Lecture 9 say both are application.
  2. *`bin/loadgen.rs` only, as D156 says.* The page would keep importing `rescuemesh::client`, against D211.
  3. *`bin/ctl.rs` as well.* It is an operator's tool, not application. It would need `udp_answers` on the interface too, and C's review, for no claim anyone makes.

  How the test reads the source.
  1. *A small hand-written reader. Comments go, strings are emptied, whitespace goes, and `#[cfg(test)] mod tests` is cut. Paths are checked against a list of what is
     allowed, and there is a list of names from below.* Chosen. About 140 lines of test code, no dependency.
  2. *D156's plain search with whitespace stripped.* It fails on a comment that names a forbidden call, and passes on `use rescuemesh::{client::MeshClient};`,
     `use rescuemesh as rm;` and `let NodeContext { consensus, .. } = ctx;`.
  3. *A Rust parser, `syn`.* The right tool in general. A new dependency on the night of the lab run is not worth it, and AGENTS.md says not to add one unasked.
  4. *Let the compiler enforce it, with `pub(crate)` modules.* `tests/` uses `rescuemesh::node` and `rescuemesh::client`, so those modules must stay public.

  What the interface hands the service.
  1. *`NodeContext` whole, and the test keeps the service away from the fields below the line.* Chosen. The move stays pure.
  2. *A narrower context type.* The compiler would then enforce it, but every `ctx.` line changes and this stops being a pure move. Better done in Phase 2, if at all.

  The shape of `send_ordered`.
  1. *A plain method that returns the same boxed future.* Chosen. It adds nothing between the caller and `Consensus::propose`, not even a poll, so "no behaviour change" is true by reading it.
  2. *`async fn`.* It reads the same and adds a wrapper future. Harmless, but not nothing.

  The client side's name.
  1. *Keep `MeshClient::group_send`, private, and say what it is.* Chosen. `docs/architecture.md` section 5.1a and the note on D126 already use that name. Its comment and `client.rs`'s header now cite both entries,
     `middleware.rs` and `lib.rs` point at it, and a test keeps it private.
  2. *A new alias such as `ClientMiddleware`.* It adds a second name for one thing, late.

  What reads the term through the group.
  1. *Only code that builds a result: `node/service.rs` and `node/degraded.rs`.* Chosen. `CloudGroup::term` is that one read.
  2. *Every `consensus.status()`.* `rejoin.rs`, `mod.rs` and the rest are middleware and may read Raft. It would only add changes.

  How to try the test on a violation.
  1. *On a copy of `src/`, with the test reading `CARGO_MANIFEST_DIR` at run time.* Chosen. Another agent builds in this same worktree, and a planted violation must not reach their build.
  2. *In the real tree, then undo it.* Simpler, and it could have broken their build for the length of a run.
- **Decision.** As chosen above, and:
  - **Moved, as D156 says:** `handle_request`, `execute`, `plan_on_workers`, `send_provisional`, `send_done`, plus their helpers `may_reply`, `ExecuteError` and `MAX_PLAN_ATTEMPTS`, and the seven unit tests over them.
  - **Stays in `node/mod.rs`, as D156 says:** `decide_executor`, `leader_dispatch`, `Decision`, `on_peer_message`, `on_control`, plus `spawn_campus_seeding` and `spawn_reporter`.
  - **`src/middleware.rs`:** re-exports only. Client side: `MeshClient`, `Submitted`, `ClientError`, `control`. Cloud side: `CloudGroup`, `View`, `NodeContext`, `ReplyTx`, `Overloaded`, `degraded`, and inside the crate `Decision` and `decide_executor`.
  - **Owners:** `node/service.rs` is A's, as D156 planned, because it came out of A's file. `middleware.rs`, `tests/layers.rs` and `lib.rs` are shared. `lib.rs` had no owner line, and gained one.
  - **One commit,** as D156 says, with the order it asks for: the move first, the test seen red, then the rest.
- **Why.** The note says "in implementation", so a drawing is not enough, and a test nobody has seen fail proves nothing. Reading the source is enough here, because the goal is to keep the
  line visible and to stop it wearing away. It does not have to hold against someone working around it on purpose.
- **Consequences.**
  - **The limits, to say out loud.** The test reads text, so it is a check and not a wall. On the cloud side `NodeContext` is shared, and only the test keeps the service from `ctx.consensus`. On the client side the compiler
    does enforce it for the group send, which is private.
  - Two calls come up from below, and both are stated in `node/service.rs`: `node/mod.rs` hands each request to `handle_request`, and `node/degraded.rs` asks `plan_on_workers` for its plans. Middleware calling up is
    normal for the first. The second is there because a lone node still plans, and planning is the application's.
  - Two small application jobs stay in the wiring file, `spawn_campus_seeding` and `on_control`'s read of incident records. D156 left them there, seeding commits through `send_ordered`, and moving them adds no claim.
  - `tracing` lines from the moved code now carry the target `rescuemesh::node::service`. A `RUST_LOG` directive for `rescuemesh::node` still matches them, and no script sets one. Event logs are not affected.
  - D156's "`send_ordered` inherits a hang" no longer holds: `Consensus::propose` ends at `propose_timeout_ms` on every path since D133 and D150, and the doc comment says that instead. Its "give new callers their own timeout" is therefore not needed.
  - Line numbers in older entries that cite `node/mod.rs` moved, as D156 expected.
  - `git log --follow` will not follow the moved code, because it is a piece of a file. `git show --color-moved 8bb244e` shows the move, and `git blame -C -C` finds where each line came from.
  - If someone adds a module the application should use, the test goes red until it is added to the allowed list on purpose. That is the point. The fix for a red test is to go through the interface, not to widen the list.
- **Where.** `src/middleware.rs`, `src/node/service.rs`, `tests/layers.rs`, `src/lib.rs` (the table), `src/node/group.rs::{send_ordered, term}`, `src/node/mod.rs`, `src/node/release.rs`, `src/node/degraded.rs`, `src/client.rs`, `src/bin/loadgen.rs`, `src/bin/frontend.rs`.
- **How we check it.** All on `shared-phase1-gaps`, one PC, in WSL.
  - **The new tests.** `tests/layers.rs`: `application_files_stay_above_the_group`, `what_the_cloud_application_commits_goes_through_the_group`, `loadgen_imports_only_through_the_facade`,
    `frontend_goes_through_the_interface_too`, `the_group_send_stays_behind_submit`, `the_interface_holds_no_code_of_its_own` and `the_check_is_not_blind`.
  - **Seen red first, as D156 asks.** On the moved code, before `send_ordered` went in, the test reported `src/node/service.rs is application code: ["names consensus", "names super::"]`, and 6 of the 7 tests failed.
    Only `the_check_is_not_blind` passed, and it reads no real file. The moved unit tests passed at that point, 7 of 7. Output: `build/split/red-on-the-moved-code.txt`.
  - **Run against deliberate violations.** Each planted on a copy of `src/`, one at a time, with `CARGO_MANIFEST_DIR` pointing at the copy. Output: `build/split/violations.txt`.

    | Planted | Result | Test that failed, and what it said |
    |---------|--------|-----------------------------------|
    | Nothing, a control | green | |
    | `node/service.rs` calls `ctx.consensus.propose`, split across lines as rustfmt writes it | red | `application_files_stay_above_the_group`: names consensus |
    | `node/service.rs` imports from `crate::raft` | red | the same: names `crate::raft::store::StateRe...` |
    | `node/service.rs` imports `crate::node::{...}` instead of the interface | red | the same, and `what_the_cloud_application_commits_goes_through_the_group` |
    | `node/release.rs` calls `ctx.consensus.propose` | red | both of those |
    | `planner.rs` imports `tokio::net::TcpStream` | red | `application_files_stay_above_the_group`: names TcpStream, tokio::net |
    | `bin/loadgen.rs` goes back to `rescuemesh::client` | red | `loadgen_imports_only_through_the_facade` |
    | `bin/loadgen.rs` imports `rescuemesh::node::usage`, the cross-cutting trap | red | the same |
    | `bin/frontend.rs` imports `rescuemesh::{client::{...}}`, nested | red | `frontend_goes_through_the_interface_too` |
    | `bin/frontend.rs` calls `TcpStream::connect` itself | red | the same |
    | `MeshClient::group_send` made `pub` | red | `the_group_send_stays_behind_submit` |
    | `middleware.rs` gains `pub fn shortcut() {}` | red | `the_interface_holds_no_code_of_its_own` |
    | A comment in `node/service.rs` naming the forbidden call | green, no false alarm | |

  - **The move is pure.** `build/split/pure-move.diff` compares the text moved at `0ef783b` with `node/service.rs`, line by line: 452 lines became 454, and the only changed lines are 3 `pub(super)`s, 2 `ctx.group.term()`s, the two
    `.consensus` `.propose(` pairs that became `.group` `.send_ordered(`, and the test module's imports. No test body changed.
  - **Same tests.** `cargo test -- --list` gave 457 names before and 464 after. The difference is exactly the 7 tests of `tests/layers.rs`, and 7 tests whose path changed from `node::tests::` to `node::service::tests::`, with the same names. No test was changed or weakened. Files: `build/split/tests-before.txt`, `tests-after.txt`.
  - **Same events.** `git diff 0ef783b` is empty for `src/metrics.rs`, and the count of every `Event::` name across `src/` is the same before and after (`build/split/events-before.txt`, `events-after.txt`, 42 names).
    Then the same short scene, on real nodes, four times on each build: 20 users on 3 nodes for 16 s, with the leader silenced for 3 s partway through. The sets of event names logged were identical: 25 in node logs, 6 in client logs (`build/split/scene-events.txt`).
    One rare name, `propose_failed`, showed up in only some scenes: 1 of 4 on the old build and 2 of 4 on the new. It is logged in `raft/mod.rs`, which did not change.
  - **Same wire and timers.** `git diff 0ef783b` is empty for `src/proto.rs`, `src/config.rs`, `config/`, `src/state.rs`, `src/planner.rs`, `src/campus.rs`, `src/raft/`, `src/node/transport.rs`, `src/node/bids.rs`, `src/node/heartbeat.rs`, `Cargo.toml` and `Cargo.lock`.
  - **Build and lint.** `cargo fmt --all --check` and `cargo clippy --all-targets -- -D warnings` are clean. The whole of `cargo test` ran 8 times on this code: 7 runs with 464 passed and 0 failed, the last on the commit itself, and one
    earlier run with 462 passed and 2 failed, both flakes from before this change (below). Ten runs in a row each, twice, of `tests/degraded`, `tests/incidents`, `tests/bidding` and `tests/group`: all green. `tests/takeover`: see below.
  - **To see it fail yourself.** Copy `src/` somewhere, plant a violation in the copy, and run the built test with `CARGO_MANIFEST_DIR` pointing at the folder above that `src/`: `CARGO_MANIFEST_DIR=/some/copy "$CARGO_TARGET_DIR"/debug/deps/layers-*` (skip the `.d` file). It reads the source at run time, so the copy does not have to compile.
- **Report notes.** Say: "the application layer has one interface to the middleware, `src/middleware.rs`, on both sides, and a test fails if application code reaches past it. We saw it fail on violations we planted." Give the limits above: a check and not a wall, a shared `NodeContext`,
  degraded mode calling up, two small jobs in the wiring file. Do not say the compiler enforces it, except for the private group send.

---

### D228 · A busy leader's log is read as one write, not as a queue

- **Date:** 2026-11-04 · **Part:** A · **Status:** Implemented, amends D218 · **Decided by:** the agent on its recommendation, on Noor's instruction to take the best choice and not ask. Noor can overrule it.
- **Context.** A review tried to break D218, and did. D218's second signal, the log wait, is a proposal's time from entering the queue to its answer. That is the queue as well as the log, so it grows with what arrives. On a disk with 25 ms added to each save, at 3000 users, every node had the same disk and the same load, and the leader's bid was 1.02 times its lowest peer's. A write took 2 saves, 62 ms, so 16 writes a second of at most 200 commands (`GROUP_LIMIT`) carried about 3200 commands a second against about 6000 asked for. Proposals waited up to their 3 s deadline. The rule read 3 s of log wait against peer bids of a few ms and moved the leader every time the cooldown let it: 4 moves in 50 s with a 5 s cooldown, term 2 to term 6, steady and all at once alike. With the default cooldown that is, by arithmetic, one move every 65 s or so. Moving cannot help, because the next leader has the same disk and the same queue, and the numbers were no better for the moves. It was also close on a healthy disk: all at once at 3000 users with `plan_work` 0 reached 37 ms against the 40 ms bar, and 8 saves against D218's "3 to 4.3 on any disk".
- **Options considered.**
  1. *Read one write's time, from `client_write` to its answer (chosen).* One save here, one on a follower and the work around them. It does not grow with the queue, because a group carries whatever waited. It rises when this machine cannot keep up, which is the case the signal is for.
  2. *Keep the log wait and leave out seconds whose groups are full.* Rejected: all at once inflates the wait with groups of about 60 of 200, so a second with no full group can still read a queue.
  3. *Raise the bars on the log wait.* Rejected: the wait rises to the 3 s deadline on a log at its limit, on any disk, so no bar a starved leader's 45 to 230 ms can pass tells them apart.
  4. *Compare with the peers' log over a new wire field.* Rejected, as in D218: a wire change a day before the deadline, and a follower has no queue.
  5. *Drop the log half and keep the bid alone.* Rejected: a leader starved of CPU never reaches 3 times its peers' bids (D218), and that is the case that slows every commit.
- **Decision.** The load is the mean bid, or the mean time of one write if that is higher and slow. Slow means over 20 ms (`WRITE_FLOOR`) and over 2.5 of this node's own disk saves (`WRITE_SAVES_TENTHS`). A write in flight through a whole reading counts as that whole reading. Everything else in D218 stays: the ratio, the time in a row, the safety conditions, the cooldown, going quiet. `leader_stepped_aside` gains `write_us` and keeps `log_wait_us` to read beside it. `raft_io` gains `writes_done` and `write_us`. The rule no longer reads `answered` or `answered_us`.
- **Why these bars.** A healthy write stays under the floor (at most 16.5 ms here), and past the floor it is about 2 saves on any disk (at most 2.21). A starved leader's write came to 21.8 to 48.0 ms at 2.65 to 5.1 of its own saves, which also grow when it is starved. A floor of 25 ms or a bar of 3 saves let the same run break before 5 s in a row, and one such run never moved at all in 30 s. That margin is thin: a dip below the bars only starts the count over, so a less starved leader stays put, which is the safer way to be wrong.
- **Measured.** **One-PC rehearsal, never six-PC results.** Three nodes on their own 4 cores each, three load generators, loopback, disk saves on, `plan_work` 1000 unless it says otherwise, 3000 responders at 3000 users and 300 at 300, think 1000 ms, other work on the PC, one run each. The cooldown is 5 s, so the rule is armed nearly all the time, except where it says. A slow disk is 25 ms added to every `fdatasync` on all three nodes, with an `LD_PRELOAD` shim.

  | Run | Rule | Leader moved | Mean write, ms: p50 / max | In saves: p50 / max | Log wait, ms: p50 / max | Highest ratio of the bid | Commit p50 / p99, ms |
  |-----|------|--------------|--------|--------|--------|--------|--------|
  | Slow disk, 3000 users, steady, 40 s | D218 | 4 times, term 2 to 6 | | | 2367 / 3001 | | 2466 / 2988 |
  | Slow disk, 3000 users, all at once, 40 s | D218 | 4 times, term 2 to 6 | | | 2250 / 2997 | | 2282 / 2991 |
  | Slow disk, 3000 users, steady, 40 s | this | no | 62.3 / 65.4 | 2.03 / 2.12 | 2396 / 3001 | 1.12 | 2222 / 2988 |
  | Slow disk, 3000 users, steady, 30 s, the final bars | this | no | 62.7 / 67.8 | 2.06 / 2.21 | 2468 / 3001 | 1.07 | 2277 / 2992 |
  | Slow disk, 3000 users, all at once, 40 s | this | no | 64.5 / 69.0 | 2.07 / 2.20 | 2465 / 3002 | 1.25 | 2367 / 2994 |
  | Even, 300 users, 40 s | this | no | 9.2 / 11.6 | 2.16 / 2.26 | 14.3 / 18.2 | 1.09 | 14.2 / 37.7 |
  | Even, 3000 users, 40 s | this | no | 14.2 / 16.5 | 2.71 / 2.97 | 22.3 / 28.1 | 1.11 | 23.2 / 42.8 |
  | All at once, 3000 users, 30 s | this | no | 10.1 / 12.7 | 2.54 / 2.96 | 17.3 / 29.1 | 1.62 | 17.4 / 63.9 |
  | All at once, 3000 users, `plan_work` 0, 30 s | this | no | 8.8 / 11.0 | 2.32 / 3.12 | 20.6 / 38.2 | 2.07 | 27.6 / 103.8 |
  | 1500 users, a follower silenced for 20 s, then for 300 ms every 2 s, six times, 55 s | this | no | 10.0 / 11.8 | 2.22 / 2.81 | 16.5 / 20.0 | 1.43 | 15.9 / 33.0 |
  | 300 users, failure draws every 25 to 30 s, 90 s | this | only by a draw that silenced the leader | 7.7 / 11.3 | 2.15 / 2.28 | 11.6 / 17.2 | 3.87, one second | 11.6 / 190.4 |
  | `ctl skew` 20000 on a follower, 300 users, 30 s | this | no | 8.4 / 10.3 | 2.16 / 2.25 | 13.5 / 15.4 | 1.05 | 13.2 / 23.6 |
  | `ctl skew` 20000 on the leader, 300 users, 35 s | this | once, quiet about 7 s after the skew, a new leader 779 ms later | 9.1 / 11.1 | 2.19 / 2.34 | 13.8 / 22.3 | 4.48 | 14.3 / 428.2 |
  | `ctl skew` 20000 on the leader, 60 users, 35 s, two runs | this | once each, quiet about 11 s and 17 s after the skew, a new leader 714 and 736 ms later | 8.3 / 9.3, 8.8 / 11.5 | 2.21 / 2.34, 2.21 / 2.51 | 11.7 / 17.5, 13.1 / 25.4 | 3.85, 4.51 | 11.0 / 443.1, 12.0 / 419.4 |
  | The demo's order at 60 users, 70 s, cooldown 15 s: skew 20000 on the leader, then put back at +30 s, then the new leader silenced for 5 s, so the first leader leads again | this | once, for the skew. No second time: its bid came back to its peers' | 5.9 / 9.7 | 2.11 / 2.31 | 8.7 / 13.4 | 3.84 | 9.1 / 619.1 |
  | 32 busy loops on the leader's 4 cores at +20 s, 300 users, 50 s, cooldown 15 s, three runs | this | once each, quiet about 6, 8 and 8 s after the loops, a new leader 858, 833 and 732 ms later. Figures here from the first run | 21.8 to 48.0 starved, 8 to 11 after the move | 2.65 to 5.1 starved | 46.7 to 107 starved | 11.75 | 14.5 / 531.1 for the whole run |

  At 3000 users on the slow disk the clients waited about 4.8 s at p50 and 20 s at p95, moved or not. That comes from the log's limit, not from this mechanism, and `GROUP_LIMIT`'s comment, which said a slow disk would never reach it, is corrected. By arithmetic, a disk slower than about 15 ms a save limits the top E1 cells: 6000 commands a second at 200 a write needs a write every 33 ms. Only the 30 ms case was run.
- **Consequences.**
  - A log at its limit no longer moves the leader, and the cluster stays as slow as that disk makes it. `elections` stays 0 on such a run. `raft_io` tells the two apart afterwards: a long wait with a short write is the queue, and a long write is the machine.
  - The write time still cannot tell whose machine is slow. A leader with a fast disk and two followers with disks over 20 ms moves once, onto a slower disk, where its bar is higher and it stays. The cost is capped at one move per cooldown.
  - With all three nodes on one starved machine the write time rises on whichever leads, so on one PC under that much pressure the leader can move once per cooldown for nothing. On six machines a node is starved alone.
  - With `plan_work` 0 the peers' bids are near nothing, so the comparison with them says little and the two bars do the work. `config/local.toml` has that and no disk saves, and its writes took under 1 ms at 3000 users on 4 shared cores.
  - `tests/step_aside.rs` found a second thing on 2 CPUs: a skew reaches a bid only through a job the node plans. With few users rounds do not overlap, so the node with the lowest planning time wins them all and the others' averages stay as they were. A leader won 0 of about 2100 rounds in 40 s, its bid stuck at 10 µs, and the test timed out. So the test now sets the average as well as the skew. At the demo's 60 users the skew did reach the bid, in both runs, 11 s and 17 s after it, later than at 300 users because the leader wins fewer rounds, so the demo script now says about 20 s.
- **Where.** `src/raft/proposals.rs` (`writes_done`, `write_us`), `src/raft/mod.rs` (`LogWork`), `src/node/step_aside.rs` (`LogReading`, `WRITE_FLOOR`, `WRITE_SAVES_TENTHS`), `src/metrics.rs`, `scripts/watch_events.py`, `tests/step_aside.rs`.
- **How we check it.** `step_aside.rs::a_full_queue_with_writes_of_the_usual_length_is_not_a_slow_log` and `a_slow_disk_is_not_a_slow_log_and_a_starved_leader_is` hold the rule to the numbers above. `proposals.rs::a_write_counts_its_own_time_and_not_the_time_its_proposals_queued`. `tests/step_aside.rs::a_skewed_follower_moves_nothing`. The rehearsal above. None of it has run on the six machines, where a real disk replaces the shim.
- **Report notes.** Say that D218's first version passed every run it was tried on and still moved the leader for nothing on a slow disk, what showed it, and how the signal changed. Keep the two ideas apart: a queue says how much is arriving, a write says how fast this machine is.

### Notes on older entries

These add to older entries without changing their text. An entry can have more than one row. Rows that earlier lanes had left inside the index table moved here.

| Entry | Note |
|-------|------|
| D1 | Its title and context say "coordinator". D163 says what that role is: it orders the log and decides nothing else |
| D6 | UDP is back for reports, as first planned, now through tarpc. Three unicasts and first reply wins stand (D167) |
| D9 | `Incident.ttl_ms` is not the table trim this entry dropped. Records are never deleted (D149) |
| D12 | One leader no longer runs the schedule (D158) |
| D16 | `proto::encode` is now for UDP datagrams and the Raft log file; TCP bodies are tarpc's. On UDP a message is one datagram of at most 1,400 bytes, and a bad one is counted, with no connection to close (D167, D170) |
| D17 | Reports no longer use TCP, or one-way frames (D167) |
| D18 | `client_addr` is TCP and UDP on one number. Control is methods on `ClientRpc`, not frames (D168, D175) |
| D22 | The gate has no exception again (D78, D160) |
| D23 | See D200: the frontend's status polls are a second sender of control frames |
| D23 | Stands: control methods skip the gate on both transports (D174) |
| D25, D117, D121 | Stand unchanged by the group work. D121's key gains a second use, recorded in D153, but its rule does not change |
| D25 | "Every node calls `initialize`" now holds only on a first start (D137) |
| D26 | `analysis/analyze.py` now reads `forwarded`, which delivers its split (D166) |
| D27 | Still every reply for one ID carries one result. `Expired` carries the record as `Done` does (D143) |
| D28 | An expired report is still an answer and counts as served, like any plan with nobody on it. It also counts in `no_responder_share` (D143, D147) |
| D31 | Digests are also logged at fixed log indexes (D138) |
| D35 | After three stale plans the executor now commits a plan with nobody on it and replies, instead of stopping (D220). Three stays the number of plans |
| D36 | Stands. A node that stays quiet now holds a call open until the client's cancel or its deadline (D169). "Gives up on stale plans" is no longer one of the silent cases. The others stand, and `Rejected` stays unused (D220) |
| D37 | A retry gets the same body: `Expired` if the record was too old on arrival, otherwise `Done`. The record decides, not the node (D143) |
| D37 | Still any serving node answers a retry from state. "Up to three identical replies" becomes fewer, because the first ends the submit and cancels the others (D169) |
| D38 | Any serving node proposes the list, not only the leader. Registration, the 200 ms poll and the commit path stay (D164) |
| D46 | `handle_request` is no longer the only place that checks `serving` (D75, D136) |
| D49 | The pick stays as settled; `Plan` now also carries a priority (D148) |
| D50 | `Security` is now `Technical`; `Incident` gains `owner`, `category`, `ttl_ms`; `Plan` gains `priority`. Its reason names fire, medical and security as the project's examples, but the spec's list is the one in D141 (D140, D148) |
| D51 | Its "(1, 2, 3)" IDs in the lab are now machine numbers; programs get five-digit IDs (D113, D184) |
| D52 | The rule is unchanged. It now also carries tarpc's deadline, 1.2 times the sum of its waits (D169) |
| D53 | Still one `MeshClient` per program, shared by its users; there are now 10 programs per client PC, not one (D181) |
| D53 | One stub and one UDP socket per node, shared by users. tarpc matches replies (D167, D169) |
| D54 | `--users` is no longer ignored with `--rate`: it sets how many owners the reports are shared among (D146) |
| D54 | "One process with many tasks is the default" becomes 10 processes through the script. `--pattern` and the first-send spread are new. Closed and open loop stay (D181, D182) |
| D54 | An open-loop `--rate` above what the cluster can answer makes new calls wait once 20,000 are open on one stub (D173) |
| D58 | The default filter changed (D134) |
| D63 | Quick is 15 cells (D162). Full is 33: the 31 here, `e7-p1-300` (D187) and `ft8-election-rate` (D162). The count of 36 in D162 assumed cells that were not built |
| D68 | openraft and tarpc (D179, D215) |
| D69 | If claims ever become leases, the token is the commit's log position. The leader issues nothing, because that would be a new duty against the table in D163 |
| D73 | The `overloaded` event it asked for now exists (D138) |
| D74 | "The leader-side schedule must treat 'already silenced' as a skipped round" now reads: an elected node that is already silenced logs `fault_refused` (D159) |
| D75 | The target is asked for again when a wait runs out (D135) |
| D76 | The answer can be an index not committed yet, so it is not always "the commit index" (D135) |
| D77 | One `--faults off` node now stops every election in the cluster (D158) |
| D78 | Gone: `FaultOrder`, the seed-per-term scheme, `fault_ordered`, the last word, the wait-out. Kept: the config keys and their values, the ranges, `fault_refused`, the refusal while recovering, and the validation rule with a new reason (D158, D159, D160) |
| D79 | D151 changes item 4, fresh clocks on restore. D155 changes item 6, "No contract change now" |
| D93, D94 | Still true that severity and age do not change who is nearest; they now also feed the priority (D148) |
| D95 | Its age is unchanged and now also feeds the priority (D148). One request path has two ages on purpose: the too-old rule reads the client's stated age (D143) |
| D99 | One more draw per incident: a category of the drawn capability, last (D141) |
| D104 | D150 makes "each task ends within the timeout" true for a call queued behind a full socket. D154 sends bids to the view, not always to two peers |
| D111 | `expires_at_ms` is `assigned_at_ms` plus this service time: a planned end, not an enforced one (D144) |
| D112 | A frame waiting in the send queue keeps its young stamp, a stated limit of the too-old rule (D143). On TCP each node holds 30 client connections and answers 30 pings a second in every cell (D186). See D200 for the frontend's status polls |
| D112 | No pings, send queues or reconnects on the client path. Its reasoning about half-open TCP still explains D119's rule between nodes (D167) |
| D113 | IDs stay required and unique; the script now gives them out (D184) |
| D114 | "The leader only sequences the log" is now the table in D163: three mechanical duties, and it orders every command, not only results. Its rule about E1 stays |
| D115 | D218 relies on this: the follower with the shortest range takes over from a leader that goes quiet. No range changed |
| D116 | The fault schedule no longer stays with the leader. Raft staying is unchanged (item 1 of Noor's decisions). Its closing line about showing someone the argument is replaced by D158: nothing waits on an answer |
| D116 | The leader's role is settled by D163. Its old text stays, and D213's row for it still stands |
| D119 | In its list of methods, `fault_order` is now `fault_bid` (D158) |
| D119 | A call now enforces its own deadline (D150) |
| D119 | The clause "The client port keeps its own framing" no longer holds, and its report note is closed by D176 (D167, D175) |
| D122 | After `responder_taken` the record reads `Unassigned` although the client was told a responder was sent (D142). The conflict compares who goes, not priority (D148) |
| D123 | `Submitted` grows `id` and `status`. `ReplyBody::Expired` follows this entry's reason against a flag on `Done` |
| D126 | Its table gains a row, not agreed: fault bids through `CloudGroup::send`, best effort and unordered, and nothing covers a lost copy, because the round dies at its deadline and the next timer tries again (D159) |
| D126 | "Name existing sends" is no longer enough, and its closing note is withdrawn. Its rejection of a library and of our own multicast stands (D152, D157) |
| D126 | The client's group send now goes through stubs, `MeshClient::group_send`. Guarantees unchanged (D167) |
| D128 | Its node event stays; client PCs get `machine_usage` (D185) |
| D128 | `resource_usage` gains `udp_drops` (D171) |
| D129 | Same settings; arrivals now spread in every cell, so a user's first request can come up to one think time later (D182, D187) |
| D129 | "The demo keeps the defaults" no longer holds, and the cells do not change for that (D165). `e1-1000-*` and `e1-3000-*` use 3000 responders, and the other cells keep 300 (D219) |
| D130 | The runner gains its guards; its "to confirm" still stands, as it has not run over SSH (D139) |
| D130 | The runner starts users through the script, and has new checks (D183, D186) |
| D130 | The runner could not start a node over ssh, and now writes `run.json` last (D222, D223). It works through a stand-in `ssh` (D188). It has still not run on the lab machines, so "to confirm" stays |
| D132 | `ctl incidents` reads results, not the Raft log, so it does not bring back `ctl log` (D145) |
| D132 | The demo's load lines move to the script (D183) |
| D133 | The proposal queue can now be held, by a leader stepping aside, and it counts how long its answers take (D218). Everything else about it is unchanged |
| D135 | The wait is paced even when Raft has stopped (D225). A later test, `tests/rejoin.rs::a_node_still_rejoins_when_the_leader_goes_away_as_it_returns`, has the leader go silent just as a node returns, on real Raft, and the node still serves nothing until it has all that was committed. It passes without D135 too, so it does not replace the scripted tests: it covers the plain leader change, not a lost index |
| D137 | A restart no longer prints `Can not initialize`, but "no `ERROR`" was too strong: see the corrections in D222 |
| D138 | The client gap closes its ends with the client's first and last events (D224) |
| D139 | A failed `cd` no longer reads as clean, and a run's `run.json` is written last (D222, D223). See the corrections in D222 |
| D143 | A report no longer waits in a client's send queue, because there is none, so the limit D143 stated about a frame keeping a young stamp while off the network goes away: a datagram goes now or never (D167) |
| D162 | The timed scene's values are now comments in `config/lab.example.toml` (D165) |
| D182 | Warm-up no longer shifts with the first send: it counts from `client_started` (D191) |
| D183 | The script also stops what a killed run left, and programs ignore HUP (D190). Its title names the web page, but the page starts `loadgen` itself (D202) |
| D185 | `client_started` joins `machine_usage` as a new client event (D191) |
| D186 | "It never kills" and "finishes this run's log copy" are now true of the code. More reasons to leave a run out (D189, D191) |
| D200 | `client.rs`'s exception to the sockets rule ended with D175, so the frontend's listener is the only one left |
| D205 | D165 keeps the same values and text, and adds the load rule, the start order and the fault timers |
| D216 | Row 16 is settled: Noor said yes, and the step-aside is built (D218) |
| D218 | The log half of the rule now reads one write's time, with its own two bars, and not the log wait. "A slow disk alone does not pass for a busy leader" and "the peers' bids keep a cluster overloaded everywhere from moving" did not hold for a log at its limit (D228) |
