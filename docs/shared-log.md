# The shared log: how the three nodes share one order, with no leader

This page describes the shared log of RescueMesh in full: what it holds, how it moves between the nodes, what is a library and what is our own code, and how we check it. Line numbers are for `src/rounds.rs` at commit `a7ceb34`. The decisions behind it are D249, D251, D252, D257 and D259 in [design-log.md](design-log.md).

## 0. The explanation, for saying it out loud

Three people, A, B and C, each keep a notebook. All three notebooks must end up the same, and nobody is the boss.

1. Each person writes their work on numbered cards, and posts a copy to the other two. The number is one higher than the highest they have seen.
2. Everyone files cards in the same order: lowest number first. For the same number, A before B before C.
3. Nobody files card 10 until all three have moved past 10, since then nobody can still send a lower card.

So the notebooks come out the same with nobody giving an order. It is a sort, so there is nothing for a leader to decide.

- If C goes quiet, A and B would wait for ever. After 1 s they agree to go on without it, and which card was its last.
- When C comes back, it copies the notebook from A or B, and asks to rejoin.

The people are the nodes, the cards are batches of commands, and the notebook is the log.

**The textbook name.** As far as we know this is Lamport's total-order multicast, sent in batches: Lamport clocks give the numbers, and a message is delivered once every process has moved past it. Check this against the lecture notes before saying it. What we add is agreeing to go on without a silent node, and bringing it back. "Rotating rounds" is a poor name, as nothing rotates.

**What to show in the code.** `src/rounds.rs` is 1,340 lines, of which 1,172 are code. Only about 100 hold the idea:

| Part | Code lines | Functions |
|---|---:|---|
| **The rule** | **100** | **`frontier`, `apply_ready`, `apply_batch`** |
| The rest of normal running | 360 | `run_emitter`, `run_sender`, `on_stream`, `run_saver`, `save`, `hold`, `submit`, `on_message` |
| A node goes silent | 193 | `run_detector`, `try_drop`, `on_drop`, `install_drop`, `fenced`, `batches_between` |
| A node comes back | 174 | `run_catch_up`, `on_catch_up`, `on_join`, `start_joining`, `apply_entry_while_joining`, `heard_from` |
| Setup, reporting, the interface to the rest of the node | 235 | `start`, `run_io_report`, the `Consensus` methods |

## 1. The short answer

- **There is no leader, no coordinator and no election in the log.** All three nodes do the same job.
- **Libraries do the sharing.** `tarpc` carries the log between nodes over TCP. `bincode` and `serde` encode it. `tokio` runs the tasks, timers and file writes.
- **The ordering rule is our own code,** in `src/rounds.rs`, about 1,300 lines. We looked for a library to do it, with no leader, and tested the closest one on the six PCs. It failed (section 3).
- **Each node keeps its own copy of the same log.** Nobody owns it. Matching digests on all three nodes are how we check that the copies agree.

In one sentence: each node numbers its own batches of commands by round and sends them to the other two, and every node applies all batches in the same order, sorted by round and then by node ID, once all members have passed that round.

## 2. What is a library and what is ours

The spec asks the report to separate these.

| Job | Done by | Version |
|---|---|---|
| Calling a function on another node, matching replies to calls | `tarpc` library | 0.37 |
| Async tasks, timers, sockets, file writes | `tokio` library | 1 |
| Turning messages into bytes | `serde` and `bincode` libraries | 1 and 1.3 |
| Splitting TCP into frames | `tokio-util` library, `LengthDelimitedCodec` | 0.7 |
| Socket buffer settings | `socket2` library | 0.6 |
| Log lines as JSON | `serde_json` library | |
| **Numbering batches into rounds** | **Ours,** `run_emitter`, line 706 | |
| **The order: round, then node ID** | **Ours,** `apply_ready`, line 306 | |
| **Knowing when a round is safe to apply** | **Ours,** `frontier`, line 290 | |
| **Agreeing to go on without a silent node** | **Ours,** `run_detector`, `try_drop`, `on_drop` | |
| **Bringing a node back** | **Ours,** `run_catch_up`, `on_join`, `on_catch_up` | |
| **The log file on disk** | **Ours,** `run_saver`, line 657 | |
| **Making messages smaller** | **Ours,** `src/node/squeeze.rs`, LZ4's block format, about 100 lines | |
| The state machine and its digest | Ours, `src/state.rs` | |

`openraft` 0.9.25 is still compiled in, behind the same `Consensus` trait, but it runs only with `consensus = "raft"`. The lab's configs set `consensus = "rounds"`.

## 3. Why no library does the ordering

The person running the lab asked for a library in place of our own code, with no leader and no coordinator anywhere. We searched crates.io, GitHub and the web on 2026-11-05.

| Library | Has a leader? | What we found | Result |
|---|---|---|---|
| `synevi` 0.1.0 (Accord) | No | **Compiled and run on the three node PCs.** See below | **Fails when a node stops** |
| `fantoch` (Tempo, Atlas, EPaxos) | No | Read the source. All three contain `todo!("recovery not implemented yet")` | Cannot survive a failure |
| `AlephBFT`, `hbbft` | No | Not tested. They are built for attackers, and as far as we know need 4 nodes to survive one failure | We have 3 |
| `paxakos` 0.13 | Yes, Multi-Paxos | Its README has decorations to track and ensure leadership. GPL-3.0 | Has a leader |
| `openraft` 0.9.25 | Yes | Measured below | Has a leader, and fell |
| Corosync 3.1.7 (Totem) | In normal running no: a token goes round a ring. When the group changes, the lowest node ID builds the new ring (`memb_lowest_in_config`) | **Not run.** Installed without `sudo` on the three node PCs, but running its programs needs approval, not yet given | Not tested |
| Kafka with `rskafka` or `rdkafka` | Yes, inside | Not run. As far as we know each partition has a leader broker, and its metadata runs on KRaft, a Raft quorum | Has a leader |
| OmniPaxos | Yes | Not run. Has its own leader election | Has a leader |
| Zenoh, `libp2p-gossipsub`, `iroh-gossip` | No | Not run. As far as we know they carry messages and give no agreed order | Would only replace `tarpc` |

**The `synevi` test.** Three real node PCs over the lab Wi-Fi, 10 transactions at a time per node, each node keeping a digest of what it applied. The test's source and output are in `analysis/results/library-search/`.

| Test | Batches a second, all nodes | Latency p50 | Digests on the three nodes |
|---|---:|---:|---|
| 2,000-byte batches, 20 s | about 63 | 482 ms | **Same** |
| 16,000-byte batches, 15 s | about 46 | 646 ms | **Same** |
| 64,000-byte batches, 15 s, two runs | about 25 | 1,296 to 1,452 ms | **Different in both runs.** Same count applied |
| 16,000-byte batches, one node stopped for 20 s | **0 during the 20 s** | 649 ms, p99 21,824 ms | **Three different digests.** Same count of 1,772 |

So it stops serving when one node stops, and its copies then differ. Experiments 3 and 4 need the opposite.

**The `openraft` run, for comparison.** At the final code, 10,000 a second, 60,000 responders, one run: 4,306.2 answered a second, 341,568 gave up. Folder `runs/phase1/raft0/10000`.

## 4. Words used

| Word | Meaning |
|---|---|
| Command | One change to the state: `RegisterResponders`, `CommitResult` or `ReleaseResponder` (`src/proto.rs`) |
| Batch | One node's commands for one round: `RoundBatch { node, round, commands, control }` |
| Round | A number that each node's stream counts up. It is not a clock, and nodes need not agree on it at any moment |
| Stream | All the batches of one node, in round order |
| Covered | How far a node's stream is saved here, in `core.covered`. A round with nothing to say has no batch, but it is still covered |
| Acked | How far a peer has saved this node's stream, in `core.acked` |
| Frontier | The highest round safe to apply here |
| View | A number that goes up each time who takes part changes. In the logs it is the field `term`, a name kept from Raft |
| Cut | Where the stream of a dropped node ends |
| Start round | The round a returning node's new stream begins at |

## 5. What the shared log holds, byte by byte

**On the wire between nodes.** One TCP connection per peer, only for the log (D251), with `TCP_NODELAY` on. A frame looks like this:

```text
[4 bytes: length, big endian][1 byte: 0 = as it is, 1 = squeezed][body]
```

The length and the frames come from `LengthDelimitedCodec`. The mark byte and squeezing are ours (`SqueezedFrames`, D255). A frame under 48 bytes is never squeezed, and one that does not get smaller is sent as it is. The body is `bincode` of tarpc's message around our call, `rounds(from, term, RoundsMsg)`. Integers are variable length.

**The messages,** from `src/proto.rs`:

| Message | Sent by | Answer | Meaning |
|---|---|---|---|
| `Stream { after, through, batches }` | every member, to each peer, one call at a time | `Have { have }`, after saving | My batches after round `after`, and that my stream has passed round `through` |
| `Drop { view, target, have }` | a member that finds `target` silent | `Granted { view, have, batches }` or `Refused { view }` | Go on without `target` as view `view`. I hold its stream through `have` |
| `DropCommit { view, target, cut, batches }` | the same member | `Done` | Its stream ends at `cut`, and these are the batches you lack |
| `CatchUp { next }` | a returning node | `Entries { batches, total, applied_round, view, members, starts }` | The applied log, from entry `next` |
| `Join` | a returning node close to the end | `Done` or `NotYet` | Add me |
| `Applied` | a returning node | `Applied { index }` | How far have you applied? |
| | | `NotMember { view }` | You are not in my view, or what you sent is from before you were left out |
| | | `NotYet` | I am not ready, or I have agreed to go on without you |

**On disk.** Each node appends to its own file, `data/raft/node-<id>.rounds`, under the configured storage folder:

```text
[4 bytes: length, big endian][bincode of one RoundBatch] ... repeated
```

`run_saver` (line 657) takes every waiting save, writes them, then calls `sync_data` once for all of them. A batch is answered only after this. A batch of ours is saved before it is sent, and a peer's is saved before we reply `Have`.

**In a client's datagrams.** Clients do not touch the log. Their reports go by UDP to all three nodes (section 6, step 1).

## 6. The life of one report

From a real run at 10,000 a second, from `analysis/results/request-trace-example.txt`. Times are from the client's send.

| Time | Where | What happens | Code |
|---:|---|---|---|
| 0 ms | client 3 | Sends the report to all three nodes by UDP, in a `WireMessage` combined for up to 20 ms and squeezed | `src/client.rs`, `src/node/datagram.rs` |
| 11.8 ms | node 3 | Receives it and sends a bid of 49 to the others | `src/node/service.rs`, `src/node/bids.rs` |
| 12.7 ms | node 2 | Receives it and sends a bid of 135 | |
| 24.5 ms | node 1 | Receives it, then sends a bid of 123 at 26.7 ms | |
| 73.3 ms | node 3 | Its 60 ms window closes. It saw all 3 bids, the lowest score is its own, so it plans | `src/bidding.rs` |
| 73 to 162 ms | node 3 | The command goes through the shared log, steps below | `src/rounds.rs` |
| 162.5 ms | node 3 | Applied at log position 1246, and the reply goes out | `src/node/service.rs` |
| 187.1 ms | client 3 | Gets `from: 3, executor: 3`, status `assigned`, first attempt | `src/client.rs` |
| 1,014.5 ms | node 3 | The service time is up. It proposes `ReleaseResponder` for responder 34548 | `src/node/release.rs` |
| 1,060.4 ms | node 3 | The release has gone through the shared log | |

**Inside the shared log, the steps of that command:**

1. **`propose` puts it in `core.waiting`** (`submit`, line 1185). If more than 60,000 wait, it is refused at once, so an overloaded node says no at the door (D250).
2. **`run_emitter` wakes every `round_ms`, 5 ms** (line 706). It takes up to 8,000 commands. A `CommitResult` already decided here is answered `Duplicate`, and costs the log nothing. It sets `my_round = max(my_round + 1, max_seen)`, so a slow node catches up with the highest round it has seen.
3. **It makes the batch,** puts it in its own `streams` and `pending`, saves it to disk, then raises `covered[me]` to that round. An empty round makes no batch, but still counts as covered.
4. **`run_sender` sends it,** one task per peer (line 781). One call at a time, with whatever is unsent, up to 8,000 commands a call. A slow network gets bigger messages, not more of them.
5. **The peer's `on_stream` takes it** (line 498). It refuses a sender that is not a member, one it has agreed to drop, or a returning node's old message. If `after` is past what it holds, it says how far it has, and the sender goes back. It saves the batches, puts them in `pending`, raises `covered[from]`, and replies `Have { have }`.
6. **The reply raises `acked[peer]`** on the sender, which runs `apply_ready`.
7. **`apply_ready` applies everything at or below the frontier,** in the order (round, node ID) (line 306). `pending` is a `BTreeMap` keyed that way, so the order comes from the key.
8. **`apply_batch` applies each batch** (line 345): `index += 1`, the state machine runs its commands in order, and every 100th index logs a digest. If the batch is this node's, the waiting callers get their answers.

## 7. Why every node gets the same order

**The frontier.** On a node, the frontier is the lowest of `covered[m]` over all members, and also no higher than the highest `acked[p]` over its peers (line 290).

Example, members 1, 2 and 3, on node 1:

| | Round |
|---|---:|
| `covered[1]`, its own | 120 |
| `covered[2]` | 117 |
| `covered[3]` | 118 |
| `acked[2]` | 115 |
| `acked[3]` | 112 |
| Lowest covered | 117 |
| Highest acked | 115 |
| **Frontier** | **115** |

- **Why the lowest covered.** Streams arrive in order, so once a node's stream has passed round 117, no batch of it at or below 117 can still arrive. Every batch up to the frontier is here, so sorting them gives the one order, and no later arrival can change it.
- **Why the highest acked.** Node 1 applies its own batch only after a peer has saved it, so anything a client is told is on two nodes, and survives one failure.
- **The order.** Everyone sorts the same batches by the same key. No node decides anything, so there is nothing to lead.

## 8. A node goes silent

A silent member holds up every round, since the frontier needs its stream. So the other two agree to go on without it.

1. **Noticing.** `run_detector` looks every 25 ms (line 900). A member not heard from for `takeover_missed_heartbeats` x `peer_heartbeat_ms` counts as silent. Lab value: 20 x 50 ms = 1 s.
2. **Asking.** `try_drop` (line 943) promises itself `(view + 1, target)`, stops counting the target's stream, and sends `Drop` to the other member.
3. **Promising.** `on_drop` (line 562) grants if it has not promised this view to a different drop. There is **at most one promise per view,** so there can be only one next view. It stops counting the target's stream, so the end cannot move, and replies with what it holds.
4. **The cut.** The asker takes `cut = max(mine, theirs)`, and sends what the other lacks. Every batch the silent node could have applied is at or below the cut: it applies its own only once a peer has saved it, and both peers stopped counting before they answered.
5. **Installing.** `install_drop` (line 401) removes the target, drops any of its batches past the cut, raises `max_seen` to the cut, and applies what is now safe.

**A promise given to a peer is never taken back** (`core.granted`, D252). A promise only this node knows of can be, when its call fails. Without this rule, two different drops were made as the same view on the lab PCs, and the digests split.

**With one node left,** it cannot drop anyone, so it stops committing. The node's degraded mode answers provisionally (D120). When a peer returns, its rounds carry on.

## 9. A node comes back

1. **It finds out.** Its next stream call gets `NotMember`, and `start_joining` (line 885) clears what it has not applied. A restarted node starts in this state.
2. **It catches up.** `run_catch_up` (line 1012) asks a peer for the applied log with `CatchUp`, up to 4,000 commands a reply, and applies it. Its log is a prefix of theirs, by section 8.
3. **It asks to be added.** Within 200 entries of the end it sends `Join`, at most once a second. A member that already added it just says `Done`.
4. **A member adds it, through the log.** The next batch of that member carries `RoundControl::Add { node, start_round, view }`, with `start_round = round + 100`. So all members add it at one place in the order, and the batches before the start round are not held up.
5. **Every member reads the entry in one view** (D257). A member behind that view stops applying at the entry until it has made the drop. A member past it ignores the entry. So a late entry cannot count on one node and not on another.
6. **The new stream starts past the old one** (D257). A member that makes a drop raises its rounds to at least the cut, so `round + 100` is past the end of the old stream. Without this, a node dropped at round 1329 came back at 1186 on the lab PCs, and those rounds held old and new batches.
7. **Members tell it the start round** (`starts` in `Entries`, D252). It is not read from the log, because a drop is not in the log, so an old entry looks like a current one.
8. **It returns exactly when members have stopped.** Members wait at `start_round - 1` for its stream. When it has the whole log and `applied_round == start_round - 1`, it throws away its own batches from the start round on, sets its rounds there, and starts sending.
9. **It has a limited time for this:** `JOIN_GRACE`, 3 s, plus the silence time before a drop. If it does not send in time, the members drop it again and it asks again.

## 10. Every function

| Line | Function | What it does |
|---:|---|---|
| 179 | `RoundsNode::start` | Opens the log file, makes the state, starts the tasks, one sender per peer |
| 271 | `Inner::save` | Encodes batches and hands them to the saver, and waits until they are on disk |
| 290 | `Inner::frontier` | The highest round safe to apply. Section 7 |
| 306 | `Inner::apply_ready` | Applies pending batches in order. Stops at an entry from a later view |
| 345 | `Inner::apply_batch` | Applies one batch, handles an `Add`, answers callers |
| 401 | `Inner::install_drop` | Leaves a node out from now on |
| 430 | `Inner::batches_between` | One stream's batches between two rounds |
| 450 | `Inner::hold` | Keeps batches in `streams`, and in `pending` if not yet applied |
| 464 | `Inner::on_message` | Sends each incoming message to its handler |
| 498 | `Inner::on_stream` | Takes part of a peer's stream |
| 562 | `Inner::on_drop` | Answers a request to drop a node |
| 606 | `Inner::on_catch_up` | Sends part of the applied log, with who is in and each start round |
| 631 | `Inner::on_join` | Answers a request to be added |
| 657 | `run_saver` | Task: appends to the log file, one sync for all waiting saves |
| 706 | `run_emitter` | Task: every round, makes this node's next batch |
| 781 | `run_sender` | Task, one per peer: one call at a time, with what is unsent |
| 885 | `start_joining` | This node was left out: clears what it has not applied, and fails its callers |
| 900 | `run_detector` | Task: looks for a silent member |
| 943 | `try_drop` | Agrees a drop with the other member |
| 1012 | `run_catch_up` | Task: while left out, applies the log, asks to be added, and comes back |
| 1127 | `apply_entry_while_joining` | Applies one entry of the log while catching up |
| 1140 | `Core::heard_from` | A reply from a peer. A node on its way back does not extend its grace time with one |
| 1147 | `Core::fenced` | Whether this node has agreed to go on without a node |
| 1152 | `run_io_report` | Task: logs the log's counters each second |
| 1185 | `RoundsNode::submit` | Puts a command in the queue and waits for its answer |
| 1208 to 1331 | `impl Consensus` | What the rest of the node sees. It cannot tell this from Raft |

## 11. Every constant

| Name | Value | Why |
|---|---:|---|
| `timers.round_ms` | 5 ms | How often a node makes a batch. Config key |
| `MAX_COMMANDS_PER_CALL` | 8,000 | One message's share of a stream, so a slow peer catches up over several |
| `MAX_COMMANDS_PER_BATCH` | 8,000 | One batch |
| `MAX_WAITING` | 60,000 | More are refused at once (D250) |
| `ADD_MARGIN_ROUNDS` | 100 | How far ahead a returning node's first round is |
| `START_GRACE` | 20 s | How long nodes not heard from yet may take to start |
| `DETECT_EVERY` | 25 ms | How often a node looks for a silent member |
| `CATCH_UP_PAUSE` | 10 ms | Wait between catch-up calls with nothing new |
| `CATCH_UP_COMMANDS` | 4,000 | One catch-up reply, so it crosses a busy network in its call |
| `CATCH_UP_TIMEOUT` | 5 s | One catch-up call |
| `STREAM_TIMEOUT` | 5 s | One stream call. TCP delivers it unless the connection dies, so giving up early only sends data twice |
| `JOIN_WHEN_WITHIN` | 200 entries | A returning node this close asks to be added |
| `JOIN_GRACE` | 3 s | How long a node just added has to send its first message |
| `IO_REPORT_INTERVAL` | 1 s | How often counters are logged |
| Silence before a drop | 1 s in the lab | `takeover_missed_heartbeats` x `peer_heartbeat_ms`. Config keys |

## 12. How we know the copies agree

- **The digest.** After each command, every node computes `digest = FNV(previous digest, index, command, outcome)` (`chain`, `src/state.rs`). One different command, order or outcome changes every digest after it.
- **Checkpoints.** Each node logs `state_checkpoint` with the digest at every 100th index. Comparing them across nodes, at the same index, is what "digest mismatches" counts.
- **`ctl status`** shows log position, digest and result count per node, and the experiment scripts save it at the end of each run.
- **Tests.** `tests/rounds.rs`. Each bug in D252 and D257 has a test that fails on the code before its fix.

## 13. The limits

Said openly, for the report:

1. **Two failures close together stop commits.** If a node fails while another is still on its way back, the last member cannot drop anyone. If the failed one then restarts while still listed, it cannot return either. Answers stay correct, but nothing commits, and what brings it back has not been tested. T4's last part showed this. Raft would carry on here.
2. **A restarted node recovers from a peer's log, not its own file.** So losing all three at once loses the state. The file is written, but not yet read back.
3. **Memory grows with the log.** About 5.2 GB per node after 5 minutes at 30,000 a second. Nothing trims it.
4. **A silent node holds everyone up until it is dropped,** so a failure costs at least the detection time, 1 s in the lab.
5. **A member that promised a drop waits for the asker,** if the asker dies at that moment (D252).
6. **We wrote this ordering ourselves,** so its correctness rests on our tests and these experiments, not on a library's users.

## 14. The other logs: what happened, event by event

The shared log above holds commands. Separately, every program writes **event logs**, for measurement only. They are not shared between nodes and nothing reads them to run the system.

- **Where.** `runs/current/node-<id>.jsonl` on each node PC and `runs/current/client-<id>.jsonl` on each client PC, one JSON object per line.
- **How.** `EventLog` in `src/metrics.rs`: `serde_json` makes the line, a channel passes it to one writer thread, so writing a line does not hold up a request.
- **In the experiments.** Each node's whole log stays on its own PC, gzipped, under `runs/full/<name>/run-<n>/`. To save time on Wi-Fi, PC1 collects every client log, and from each node every line that is not per request, 1 in 50 of those that are, and all of them from 3 s before a fault to 5 s after it.

**A real line:**

```json
{"ts_us":1793825795672324,"src":"client-3","ev":"client_done","request":{"client":3,"seq":1793825791498515},"from":3,"executor":3,"latency_us":188139,"attempts":1,"provisional":false,"no_responder":false,"status":"assigned","priority":412}
```

Under rounds, some names are kept from Raft: `raft_state.term` is the view, `raft_io` holds the log's counters, and the message counter `raft_append` counts `Rounds` messages.

**Every event, generated from `src/metrics.rs`:**

| `ev` | Fields | What it records |
|---|---|---|
| `started` | `peer_addr`, `client_addr` | The program started, with its addresses. |
| `stopped` | none | The program stopped normally. |
| `raft_state` | `term`, `leader`, `role`, `last_applied` | Term, leader or role changed on this node. |
| `commit` | `request`, `kind`, `outcome`, `latency_us`, `forwarded`, `log_index` | A proposal finished committing (E5, commit latency). |
| `propose_failed` | `request`, `kind`, `error`, `latency_us` | A command did not commit in time, with the reason. |
| `raft_io` | `waiting`, `writes`, `commands`, `expired`, `write_timeouts`, `answered`, `answered_us`, `writes_done`, `write_us`, `syncs`, `sync_us`, `max_sync_us`, `saves`, `saved_entries`, `saved_commands`, `max_leader_gap_us` | Once a second: the proposal queue and the Raft log file (D133). All cumulative, except `waiting` (now) and the two largest-so-far figures (since the last event). Commands per save is `saved_commands / saves`; disk syncs per second comes from `syncs`. |
| `leader_stepped_aside` | `term`, `score`, `write_us`, `log_wait_us`, `save_us`, `load`, `peer_scores` | This Raft leader found itself overloaded and went quiet, so that a normal vote picks a new leader (D218, D228). All from the last second: `score` is its own mean bid, `write_us` the mean time one log write took, `log_wait_us` the mean time per proposal with the queue in it, which the rule does not read, `save_us` its mean disk save, `load` what the rule compared (the score, or the write time if that is over its bar and higher), and `peer_scores` the peers' mean bids. |
| `leader_step_aside_ended` | `term`, `outcome`, `new_term`, `new_leader`, `waited_ms` | How that ended. `outcome` is `changed` (another node leads), `re_elected` (this node won the vote), `timed_out` (no new leader in time, and this node carries on if it still leads) or `stopped` (this node was silenced, stopped serving or went degraded). `term` is the term it went quiet in, and `waited_ms` runs from then to here, on this node's clock. |
| `state_digest` | `last_applied`, `digest` | Periodic state fingerprint; equal digests at equal indexes mean identical state. |
| `state_checkpoint` | `log_index`, `digest` | The digest right after applying a log index that is a multiple of `store::CHECKPOINT_EVERY`. Every node applies every index, so these can be compared between nodes under load, which the once-a-second `state_digest` cannot (D138). |
| `messages` | `sent`, `received` | Cumulative message counts by type (E5). |
| `resource_usage` | `rss_kb`, `cpu_ticks`, `net_rx_bytes`, `net_tx_bytes`, `udp_drops`, `queued`, `running` | Once a second: this process's memory and CPU, the machine's network counters and the worker queue (D128). `cpu_ticks`, the byte counts and `udp_drops` are cumulative. `udp_drops` is the machine's `UdpRcvbufErrors`: datagrams the kernel threw away for want of buffer (D171). |
| `udp_socket` | `local_addr`, `rcvbuf_asked_bytes`, `rcvbuf_got_bytes` | A UDP socket opened, and the receive buffer the kernel really gave it (D171). |
| `udp_counters` | `counts`, `udp_drops` | Once a second on nodes and clients: what this process's UDP sockets did, cumulative (D172). `udp_drops` is as in `resource_usage`, for client PCs, which log no `resource_usage`. It is absent from the last line a client writes as it stops. |
| `request_received` | `request`, `attempt` | A node received a report from a client. |
| `replied` | `request`, `executor`, `latency_us` | A node sent a client its answer. `executor` is the node that planned it. |
| `report_expired` | `request`, `age_ms`, `ttl_ms` | A report reached the node that would plan it too old, so nobody is planned for it (D143). Rare: the default TTL outlasts every retry, so a measured run should log none (D147). |
| `bid_sent` | `request`, `attempt`, `score`, `queued`, `running`, `just_won`, `service_us`, `cpu_permille` | This node's bid and what made up its score (E6). |
| `bid_round_closed` | `request`, `attempt`, `winner`, `i_won`, `bids_seen`, `my_score`, `window_us` | This node's view of a closed round. Two nodes with `i_won` true for one request is a split decision (P-2). |
| `silenced` | `ms` | This node stopped answering, for `ms`: a fault. |
| `restored` | none | The fault ended, and this node answers again. |
| `fault_election` | `round`, `tickets`, `victim`, `ms`, `called`, `election_us` | One round of the fault election as this node saw it, once per round (D158, D161). `tickets` are the numbers it held. `victim` and `ms` are null if it never held all of them, and then it did nothing. `called` is true if this node's timer started the round. `election_us` runs from this node's own bid to the last bid it needed, on this node's clock, and is null with `victim`. |
| `fault_refused` | `victim`, `reason` | This node was elected and stayed up: it was catching up, or already silenced. |
| `peer_suspected` | `peer`, `silent_ms` | Nothing heard from `peer` for `silent_ms` (peer heartbeats, D79). `silent_ms` is E3's detection time, on this node's clock. The victim's `silenced` is on another machine's (D233). |
| `peer_trusted` | `peer`, `suspected_ms` | `peer` was heard from again after `suspected_ms` under suspicion. |
| `peer_unreachable` | `peer`, `error` | A connect to `peer` failed; logged once per outage (D92). |
| `peer_reconnected` | `peer`, `down_ms` | A connect to `peer` succeeded after an outage that lasted `down_ms` on this node's clock. |
| `group_view` | `view`, `members` | This node's list of who is in the group, once at start and once per change (D152, D155). `view` counts this process's changes only, so it means nothing across nodes, and it starts again at 1 in the same file after a restart. |
| `takeover_started` | `request`, `attempt`, `winner`, `rank`, `reason`, `waited_us` | This node lost the round and plans the request instead of `winner` (D110). `rank` is its place in the round's ranking (1 = runner-up); `reason` is `suspected` or `backstop`. `waited_us` runs from the round closing to here: E3 takeover time. |
| `degraded_entered` | `held_responders` | Every peer has been suspected for `degraded_after_ms`; this node now answers without a commit (D120, D121). `held_responders` is what the local table took over (D124). |
| `degraded_left` | `reason`, `degraded_ms`, `provisional` | `reason` is `peer_trusted`, `silenced` or `not_serving`. `provisional` answers are waiting to be replayed through Raft. |
| `provisional_replied` | `request`, `responder`, `latency_us` | A lone node answered without a commit. |
| `provisional_conflict` | `request`, `reason`, `responder`, `winner_responder`, `winner_executor` | A provisional answer did not stand (D122). `reason` is `lost_first_commit`, with the winner's plan, or `responder_taken`, and then the result is recorded with no responder. |
| `overloaded` | `request`, `attempt`, `queued`, `running` | The worker pool was full and refused this planning job (D48, D73). `queued` and `running` are the pool at that moment. |
| `release_proposed` | `responder`, `request`, `backup` | This node proposed freeing `responder` after its service time (D39, D111). `backup` is true if the executor should have done it first and has not. |
| `restarted` | `term`, `last_log_index` | The process started with a Raft log from an earlier run (D118, FT-9). |
| `rejoin_started` | `applied` | The fault gate reopened; the node is catching up and does not serve yet (E4). |
| `rejoin_target_lowered` | `from`, `to` | The leader's next answer was below the catch-up target, so the target was an index that was not committed then, and may never be (D135). |
| `rejoined` | `leader_commit`, `applied`, `recovery_us` | The node applied the leader's commit index and serves again. `recovery_us` runs from `rejoin_started` to here, on this node's clock (E4). |
| `client_started` | `users` | A load program's clock starts, and its warm-up with it. Its first send can come up to two think times later, as its pattern says, so that send cannot stand in for this (D182). `users` is 0 in an open loop, which has none. |
| `client_sent` | `request`, `attempt` | A client sent a report. `attempt` 0 is the first try. |
| `client_done` | `request`, `from`, `executor`, `latency_us`, `attempts`, `provisional`, `no_responder`, `status`, `priority` | A client got its answer: which node sent it (`from`), which planned it (`executor`), and how long it took. |
| `client_gave_up` | `request`, `attempts` | A client stopped trying, after all its attempts. |
| `client_link_up` | `node` | A client connection to `node` was opened (D112). Nothing logs this since reports moved to UDP, which has no connection (D167). Kept because the analysis reads the name in old runs. |
| `client_link_down` | `node`, `reason` | A client connection to `node` closed or stopped answering pings (D112). As `client_link_up`. |
| `machine_usage` | `cpu_busy_ticks`, `cpu_total_ticks`, `mem_available_kb` | Once a second, from one program on each client PC: the whole PC's CPU and memory (D185). A saturated client PC reads as cloud latency, so the analysis must be able to see it. The tick counts are cumulative. |
