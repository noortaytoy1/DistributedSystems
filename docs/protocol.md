# Protocol

Messages, encodings and timers. The source of truth is [src/proto.rs](../src/proto.rs) and
[src/config.rs](../src/config.rs); this page explains them. Change both in the same PR, with a
design-log entry (see AGENTS.md). Sequence diagrams for every flow that uses these messages are in
[architecture.md](architecture.md), and the reasons behind each choice are in
[design-log.md](design-log.md).

## Connections and framing

Each node has two port numbers from the config:

| Port               | Who uses it        | Pattern                                               |
|--------------------|--------------------|-------------------------------------------------------|
| `peer_addr`, TCP   | other nodes        | tarpc remote calls (`proto::PeerRpc`), many in flight (D119) |
| `client_addr`, UDP | client middleware  | tarpc remote calls (`proto::ClientRpc`), one UDP packet per message (D167, D170) |
| `client_addr`, TCP | `ctl` and the frontend's status polls | the same trait through tarpc, so a lost silence order cannot vanish (D168) |

On TCP, every frame is a 4-byte big-endian length followed by the body (`tokio_util` `LengthDelimitedCodec`), at most 16 MiB
(`MAX_FRAME_BYTES`): a tarpc message in tarpc's bincode format. Malformed frames close the connection. `TCP_NODELAY` is set on
every connection.

One UDP packet is one tarpc message, encoded with `proto::encode`: bincode with varint integers and no trailing bytes, at
most 1,400 bytes (`MAX_DATAGRAM_BYTES`). A bad UDP packet, an ICMP error or a failed send counts as a lost message: no error ever
reaches tarpc, because it would end every call on that channel (D170). The largest report and answer fit, and a test holds that.

**Reconnecting, between nodes only.** A node connects to a peer lazily on the first call and reuses the connection.
If a connect fails (refused, or not done before the call's deadline), every call to that peer
fails at once with `TransportError::Backoff` until `2 × raft_heartbeat_ms` has passed; then the
next call connects again. The node logs `peer_unreachable` once when the outage starts and
`peer_reconnected { down_ms }` when a connect succeeds (D89–D92). A silent peer does not
trigger this: its listener stays up, so connects succeed and calls time out instead.

## Client protocol

Reports travel through tarpc over UDP (`ClientRpc::submit`). The picture is one attempt:

```
client middleware               every node
  │── submit(report) ────────────▶│  (one tarpc call per node, one UDP packet each)
  │                               │  the winner plans, Raft makes the result final, then the winner answers
  │◀─────── one UDP packet ───────│  (the first answer wins)
  │── cancel ────────────────────▶│  (to each node whose call is still open)
```

This send to all the nodes is the group send, and the application does not know it happens: it makes one `submit` call, and
`MeshClient::group_send` makes the copies. Each user program holds its own `MeshClient` (D210, and [architecture.md](architecture.md) section 5.1a).

- `ClientRequest { id, attempt, age_ms, incident }`. `id = (client, seq)` is unique per user program.
  A retry reuses `id` and increments `attempt`. `age_ms` is how old the report is at this send: one wall-clock
  difference at submit, then a monotonic clock, all on the client. A node judges "too old" from it, so that decision
  never compares two PCs' clocks (D143). Client IDs are given out as in D184: 1 to 9 by hand, 11 to 13 frontends, 21
  to 23 zone feeds (Phase 2), five-digit IDs from `scripts/start-users.sh`, and six-digit IDs for a frontend's programs.
- The client resends with the same ID up to `client_max_retries` times. The wait starts at
  `client_retry_ms` and doubles up to `client_retry_max_ms`, with ±20% random jitter (D52).
- `ClientReply { id, from, body }`, as `submit`'s return. `body` is `Done(CommittedResult)`, `Expired(CommittedResult)`, `Provisional(Plan)` or
  `Rejected(reason)`. Every `Done` or `Expired` answer for one ID carries the same final result, whichever node
  sends it, and the record decides which of the two it is.
- `Expired(CommittedResult)` means the report was too old to plan: `age_ms >= ttl_ms` when it reached the node that would
  plan it, so nobody was sent. It becomes final like any result. **It is an answer: a client must stop waiting when it
  gets one.** A client that ignored it would retry for 30 s and give up on a report the cloud had decided about (D143).
- `Provisional(Plan)` comes only from a node in degraded mode: every peer suspected for
  `degraded_after_ms` (D120, D121). It is not final. The client takes it at once if no `Done` came and
  reports `provisional: true` (D123). A retry gets the same plan. The node replays the result through
  Raft when a peer returns, and the first plan to become final wins, so the record can differ; clients are not told (D122).
- A lone node applies the same too-old rule, and its answer is still `Provisional`, with nobody on the plan. Once replayed,
  the record answers `Expired` to a retry.
- A node that already has a final result for an ID answers a retry straight from its state.

**A call's life (D169).** `MeshClient` holds one stub and one unconnected UDP socket per node, shared by all users in the program.
Each attempt adds one call per node to one set, and every call has the same deadline, 1.2 × the sum of all waits (36 s at the
defaults). Calls from earlier attempts stay open, so a late answer still counts. A node that cannot answer sends nothing: the call
stays open until the client's cancel or its deadline (D36). Returning from `submit` sends one cancel per call still open. A
first-attempt answer is 3 report copies, 1 answer and 2 cancels. Nothing on this path holds a connection, so a node that returns
on its address just answers.

**Channels.** A node splits UDP packets by source address into one tarpc channel per user program. A program with no traffic for 60 s
loses its channel, because UDP has no close. So channels are programs, not clients: do not count them as clients.

**Loss.** A copy lost on the way to one node costs nothing if another node answers. All three lost, or the one answer lost, costs one
wait of about 2 s. A lost cancel parks a call on the node for up to 36 s (D173). `udp_counters` counts all of it (D172).

## Control protocol (`ctl`, and the frontend's status polls)

Calls on the same trait, `ClientRpc`, through tarpc over TCP on the client port (D168, D175). Control methods skip the fault gate
on both transports (D23, D174). `ctl` builds a `ControlRequest`, and `client::control` maps it to a method.

| Method                     | Effect                                             | Reply             |
|----------------------------|----------------------------------------------------|-------------------|
| `status()`                 | none. `ctl status` also calls it over UDP, up to 3 tries in 2 s, and prints `node N: udp ok` or `node N: udp no answer` under its table, so a closed port shows (D168) | `Status(NodeStatus)`, whose last field is `members`, this node's who's-alive list (D155) |
| `incidents(query)`         | none. `One(id)` reads one record; `OfClient { client, limit }` reads that client's newest first, at most 20 | `Incidents { node: NodeStatus, rows: Vec<IncidentView> }`. Control skips the gate, so a silent or returning node answers too: discard its rows unless `node.serving && !node.silenced` (D145) |
| `silence(ms)`              | drop all peer and client traffic for `ms` (1 to 20000) | `Done`, or `Error` if `ms` is out of range, the node is already silent (D74), or fault injection is disabled (D77) |
| `skew_load(extra_work)`    | set this node's planner work per report. It replaces `plan_work`, so to put a node back set it to `plan_work` again (D205) | `Done`            |

## Peer protocol

Nodes call each other through the tarpc service `proto::PeerRpc` (D119). A connection carries many
calls at once, and answers may come back in any order. Each method takes `from` and `term`, the
sender's current Raft term, and has a deadline, which holds even for a call tarpc could not write because the socket is full
(D150). A leader stepping aside sends nothing new: it reads the bids it already receives, and only stops sending (D218). A connection whose calls have gone unanswered for
1 s is replaced. In the code, `PeerEnvelope { from, term, body }` names a call, and each body below
maps to one method.

| Body                         | Sent by                    | Reply                              |
|------------------------------|----------------------------|------------------------------------|
| `Raft(AppendEntries)`        | leader                     | `AppendEntries(Result<..>)`        |
| `Raft(Vote)`                 | candidate                  | `Vote(Result<..>)`                 |
| `Raft(InstallSnapshot)`      | leader, never sent, as snapshots are off (`SnapshotPolicy::Never`) | `InstallSnapshot(Result<..>)`      |
| `Propose(Command)`           | follower, to the leader    | `Proposed(Result<Outcome, ProposeError>)`. One command. The leader puts it in the same queue as its own proposals (D133) |
| `FaultBid(FaultBid { round, ticket, ms })` | every node that may bid, to the group (D158) | `Ack`. Only `Ack` means the peer holds it. A bid from an older term gets `StaleTerm` and is dropped. No retry |
| `ReadIndex`                  | returning node, to the leader | `ReadIndex(Result<Option<u64>, ProposeError>)`: the larger of the leader's final index and its first log line of the term, confirmed by a majority, or `NotLeader { leader }` (D76). Everything final before the question is at or below the answer, but the answer itself may not be final yet, so a waiting node asks again after 1 s and keeps the lowest answer (D135) |
| `Bid(Bid { request, attempt, score })` | every serving node, to the peers on its who's-alive list, not waited for (D104, D154) | `Ack`. Each report and attempt has its own round (D84) |
| `Heartbeat`                  | every node, to every configured peer, on its who's-alive list or not (D154) | `Ack`                              |

**Term rule.** A receiver drops a non-Raft message whose `term` is lower than its own and
answers `StaleTerm { current }`. Raft RPCs are always passed to openraft, which rejects stale
leaders itself and must answer so they step down.

**Fault election.** A round closes on a node only when it holds a bid from every node in `config.cluster.nodes`, whatever its who's-alive list says, and its own
send has finished. The first bid per node stands. A round at or below the highest one this node has left, or below the one in progress, is late and changes
nothing. A higher round replaces the one in progress. Highest ticket wins, ties to the lower node ID, and the winner uses the `ms` in its own bid. `ticket` is
in `0..1_000_000`. `round` is a local counter, not saved. Nothing about a round goes in the log.

**Silence.** A silent node's methods never return, and it fails every outgoing call at once.
The caller sees its deadline pass.

**Peers and roles.** The nodes are peers. Who is the client and who is the server holds only for one call (D212).

## Group communication

The client's send to all the nodes, above, is the group send. `CloudGroup` (`node/group.rs`) is one node's who's-alive list of the cloud nodes as a
group: it holds the membership, and sends between nodes follow it. It adds no message: bids, fault bids and heartbeats are the messages above.

| Call | Addressed to | Used by |
|------|--------------|---------|
| `send(timeout, body)` | the peers on this node's who's-alive list | bids and fault bids |
| `probe(timeout, body)` | every configured peer | peer heartbeats |
| `send_ordered(command)` | every copy, in one order, through Raft | the incident service, responder release, campus loading, degraded replay |

`send` and `probe` read the sender's term themselves and return each addressed peer's answer. `Ok(Ack)` was delivered, and only this counts.
`Ok(StaleTerm)` was answered but dropped by the term rule. `Err(Timeout)` and `Err(Closed)` are unknown. `Err(Backoff)`, `Err(Connect)`
and `Err(Silenced)` were never sent. Neither retries, and neither orders anything. `send_ordered` is one Raft proposal: `Ok` means final and applied on the leader, and `Err` means unknown. A follower gets its `Ok` from the leader, so it can hold the answer before it has applied that log line itself.

The who's-alive list starts with every configured node. A peer leaves after `degraded_after_ms` of unbroken suspicion and returns as soon as it is
heard. A node never removes itself. While a node is silent, its list stays as it was. Each change logs `group_view { view, members }`, and so does the
first list, after `started`. `view` counts this process's changes from 1, so it means nothing across nodes or restarts.

## Raft log file

With `storage.dir` set, each node writes `node-<id>.wal` before it acknowledges anything to openraft
(D118). A record is a 4-byte big-endian length, an 8-byte FNV-1a checksum and a body encoded with
`proto::encode`: `Vote`, `Committed`, `Append`, `Truncate` or `Purge`. Only `Committed` is written without a save.

A new field in `Command` changes this file's format. An old log still has good checksums but no longer decodes, so it is
cut at its first report with only a warning: after such an upgrade, wipe `storage.dir` on all three nodes. D133 changed it too,
when a log line became a group of commands.

## Raft log

The payload of a log line is `CommandGroup`, a list of `Command`s applied in order at that line's log index. Applying it
yields a list of `Outcome`s, one per command, in order, which is the response to `client_write`. A blank or membership log line
yields none. Results of one group share a `log_index` (D133).

| Command                                   | Effect                                                                 | Outcome |
|-------------------------------------------|------------------------------------------------------------------------|---------|
| `RegisterResponders { responders }`       | adds unknown responders; known ones are untouched. Any serving node proposes it at start until its state holds responders, so a log can hold several (D164) | `Applied` |
| `CommitResult(ResultProposal)`            | the first result per report ID wins and holds its responder               | `Committed`, `Duplicate(winner)` or `Stale` |
| `ReleaseResponder { responder, request }` | frees the responder if `request` holds it. Proposed by the node that planned after the service time, by other nodes after twice that (D39) | `Applied` |

`Stale` means the plan's responder is held by another report or unknown. Nothing is recorded,
and the winner plans again with fresh state. After 3 stale plans it makes a plan with nobody on it final, which cannot be stale, and
answers from what becomes final (D220).

Applying any command twice changes nothing, so a proposal retried after a leader change is harmless.

## Incidents, status and priority

An `Incident` holds `owner` (`UserId { client, user }`), `category`, `zone`, `location`, `severity` (1 to 5), `needs`,
`reported_at_ms` and `ttl_ms`. Its ID is the report's ID, a `RequestId` (D140). A `CommittedResult` adds `executor`, `term`, `plan`,
`age_ms`, `assigned_at_ms` and `log_index`.

**Times.** `reported_at_ms` is on the client's clock and `assigned_at_ms` on the clock of the node that planned. **Never subtract one from the
other:** `age_ms` is the report's age, measured on the client. `expires_at_ms` in `IncidentView` is `assigned_at_ms +
severity × service_ms_per_severity`, or nothing if nobody was sent. It is the planned end, not an enforced one: a backup
node releases at twice that time, a failed release restarts the wait, and a replayed plan's hold starts when the replay becomes
final (D144).

**Status** is worked out on read, in no `Command` or `Outcome` (D142):

| The record | Reads |
|------------|-------|
| a responder was sent and is still held | `Assigned` |
| a responder was sent and its release is final | `Resolved` |
| nobody sent, and `age_ms >= ttl_ms` in the record | `Expired`, on any clock |
| nobody sent, and the reader's clock is at or past `assigned_at_ms + ttl_ms - age_ms` | `Expired`. A read rule: no log line, and a clock stepped back turns it back |
| nobody sent, otherwise | `Unassigned` |
| a lone node's answer, on the client only | `Provisional` |

`Submitted.status` in the client middleware is the status when the answer was given. It needs no clock and no state, so a retry after
the release still reads `Assigned`. To see `Resolved`, read `Incidents`.

**Priority.** `Plan.priority` is an integer in priority points, higher is more urgent, worked out by the planner (D148).
It never changes who is sent, and nothing is ordered by it. 0 means the report was too old to plan.

| Factor | Points | Range |
|--------|--------|-------|
| severity | 100 per point of severity | 100 to 500 |
| age, as D95 gives it to the planner | 1 per 50 ms | 0 to 400 |
| distance to the responder | 1 per 10 m; nobody sent counts as the top | 0 to 100 |
| availability | 100 / (free responders of any capability + 1) | 0 to 100 |
| capability | 100 if no free responder has the needed capability | 0 or 100 |

So severity 4, 1 s old, with a matching responder 300 m away and 9 free, scores 400 + 20 + 30 + 10 + 0 = 460.

## A report's life

1. The client middleware calls `submit` on every node, one UDP packet each. Every serving node receives it.
2. If a result is already final for the ID, any node answers with it.
3. Assignment picks one winner: the lowest bid, or the leader under `leader-dispatch`. A node that lost stands by, and plans if the winner is suspected, or past its backstop, with nothing final yet (D42, D110).
4. The winner plans on its worker pool from its own copy of the state, and works out the priority. If the report's
   `age_ms` has reached its `ttl_ms`, it does not plan: the plan is nobody, cost 0, priority 0, with no worker job (D143). A full
   worker pool refuses the job, logs `overloaded`, and nothing is sent to the client (D48, D138).
5. The winner proposes `CommitResult`. A follower that knows the leader forwards it straight away with `Propose`. On the leader it
   goes into the queue in front of Raft: one write in flight, and one write carries a group of up to 2000 commands of what waited, in one log line and one save (D133).
6. After the log line is final and the leader has applied it, the winner answers with the first result to become final: `Expired` if that record was too
   old on arrival, otherwise `Done`. A node that is not serving at that moment stays quiet and the client's retry gets the
   result from a serving node (D136).
   A node in degraded mode skips steps 3 and 5 and answers `Provisional` straight after planning (D120).
7. If the client middleware hears nothing within `client_retry_ms`, it calls again with the same ID, leaving the earlier calls open.
8. The first answer ends the submit, and the client middleware cancels the calls still open. That stops the wait on the node, not its
   work (D169).

## Errors, and what each caller sees

| Error | Raised by | What the caller does |
|-------|-----------|----------------------|
| `ProposeError::NotLeader { leader }` | a node that does not lead, answering `Propose` or `ReadIndex`. Writes held by a leader stepping aside get this when the hold ends, if the leader has changed (D218) | retries at the hinted leader until its deadline (D26) |
| `ProposeError::Timeout` | no leader made the proposal final before the deadline | the node stays quiet and the client's retry takes over (D36) |
| `ProposeError::Fatal(reason)` | consensus stopped | the node stays quiet and logs it |
| `TransportError::Silenced` | an outgoing call on a silent node | never sent. The caller treats it as a failed call |
| `TransportError::Backoff`, `Connect` | a peer that cannot be reached, or is in its cooldown (D89–D92) | never sent. openraft maps these to `Unreachable` (D32) |
| `TransportError::Timeout`, `Closed` | no answer by the call's deadline, or the connection closed | unknown: it may have arrived. Bids and peer heartbeats do not retry. A proposal retries, and that is safe (D27) |
| `TransportError::UnknownPeer`, `Wire` | a node ID not in the config, or a body that does not encode | a bug or a mixed build |
| `Overloaded` | the worker pool, when `threads` are busy and `queue_capacity` jobs wait | logs `overloaded`, no answer, the client's retry takes over (D48, D138) |
| `Outcome::Stale` | the state machine: the plan's responder is held or unknown | the winner plans again, up to 3 plans in all, then makes a plan with nobody on it final and answers (D35, D220) |
| `ClientError::GaveUp { attempts }` | `MeshClient`, when its retries run out | `submit` returns the error and `client_gave_up` is logged |
| `ClientError::Connect`, `Timeout`, `Closed` | `client::control`, for `ctl` and the frontend's polls | the tool says that node is unreachable |
| A malformed TCP frame | the receiver | the connection closes (D16) |
| A bad UDP packet, an ICMP error or a failed send | the UDP adapter (`node/datagram.rs`) | counted and dropped; no error reaches tarpc, and the client's retry covers it (D170) |
| `ReplyBody::Rejected(reason)` | defined, never sent (D36) | none |

## Leader changes

A leader that stays overloaded goes quiet, and a normal vote picks another (D218). `policy.leader_overload_ratio`, in the timers table, sets what overloaded means. No message and no field on the wire changes. Two events say what happened, on that leader's own log:

- `leader_stepped_aside { term, score, write_us, log_wait_us, save_us, load, peer_scores }`: this leader went quiet. `score` is its mean bid in the last second, `write_us` the mean time one log write took, `log_wait_us` the mean time per proposal with the queue in it, which the rule does not read (D228), `save_us` its mean disk save, `load` what the rule compared, and `peer_scores` the peers' mean bids.
- `leader_step_aside_ended { term, outcome, new_term, new_leader, waited_ms }`: `outcome` is `changed`, `re_elected`, `timed_out` or `stopped`, and `term` is the term it went quiet in.

`raft_io` gains `answered` and `answered_us`, both cumulative: the proposals this node's queue answered, and the time they took from entering it. It also gains `writes_done` and `write_us`: the writes finished, and the time they took with no queueing in them, which is what the rule reads (D228).

## Timers

Defaults from `config.rs`, tuned for a LAN:

| Timer                         | Default      | Meaning                                                   |
|-------------------------------|--------------|-----------------------------------------------------------|
| `raft_heartbeat_ms`           | 50           | the leader's heartbeat setting: an idle leader sends one on each 75 ms tick (1.5 times this, in openraft). Also how long one AppendEntries may take |
| `election_timeout_min_ms`     | 150          | where the first node's vote timer starts. A node's place in the config shifts its range: 150–225, 300–375, 450–525 ms (D115) |
| `election_timeout_max_ms`     | 300          | limits the width of each node's range. openraft's lease is not this key: it is the top of each node's own range, 225, 375 or 525 ms. A follower that knows its leader waits its range plus its lease: 375 to 450, 675 to 750 or 975 to 1050 ms, on a 75 ms tick. So the first follower usually stands after about 0.38 to 0.83 s. A vote refused inside a voter's lease, or a candidate behind on the log, adds to that. FT-2's target is 1 s (D214, D231) |
| `peer_heartbeat_ms`           | 50           | how often each node tells the others it is alive         |
| `bid_window_ms`               | 20           | a bid round closes this long after the report reaches this node |
| `takeover_missed_heartbeats`  | 3            | peer heartbeats missed before a node suspects a peer and the next bidder can take over |
| `client_retry_ms`             | 2000         | the client middleware's first wait for an answer; each later wait doubles |
| `client_retry_max_ms`         | 6000         | longest wait between resends (D52)                        |
| `client_max_retries`          | 5            | resends before the client gives up (about 30 s in total)  |
| `propose_timeout_ms`          | 3000         | a proposal waits at most this long, in the queue, in a write in flight or across leader changes, then fails with `Timeout` (D133) |
| `peer_rpc_timeout_ms`         | 500          | one non-Raft peer call (bid, fault bid, read index). A peer heartbeat waits only `peer_heartbeat_ms`, and a forwarded proposal waits up to `propose_timeout_ms` (D133) |
| `degraded_after_ms`           | 2000         | every peer suspected this long, and a serving node answers provisionally (D121); one peer suspected this long leaves this node's who's-alive list (D153) |
| (derived) reconnect cooldown  | 2 × `raft_heartbeat_ms` | after a failed connect, calls to that peer fail fast this long (D90) |
| (derived) client call deadline | 1.2 × the sum of all waits, 36 s | one per submit (D169) |
| `campus.service_ms_per_severity` | 200       | the node that planned frees a responder after this × severity; other nodes after twice that (D111) |
| `campus.incident_ttl_ms`      | 60000        | how long a client says its reports stay valid. A node refuses an older one. Above the client's retries: 30 s, at most 36 s with jitter (D143). `loadgen --ttl-ms` sets it for one run. 0 is refused in the config |
| `faults.interval_min_ms`      | 30000        | each node waits at least this long, from the end of the last round and any silence it elected; the first to finish calls the next round. Must be >= `silence_max_ms`, or rounds cannot finish. `config/lab.example.toml` ships 100000000 here and in `interval_max_ms`, so no timer fires (D165) |
| `faults.interval_max_ms`      | 60000        | ... and at most this long (random) |
| `faults.silence_min_ms`       | 1000         | shortest silence a node draws                              |
| `faults.silence_max_ms`       | 20000        | longest silence a node draws (at most 20000)               |
| (derived) fault round deadline | 2 × `peer_rpc_timeout_ms` | a node gives up on a round after this, logs a null victim and waits again (D159) |
| `policy.leader_overload_ratio` | 3 | a leader is overloaded at this many times its lowest peer's mean bid. 0 turns the step-aside off, otherwise at least 2 (D218) |
| `policy.leader_overload_for_ms` | 5000 | how long a leader must stay overloaded, without a break, before it steps aside (D218) |
| `policy.leader_overload_cooldown_ms` | 60000 | no step-aside until this long after the last term change or the last try; at least 3000. This is what stops a storm of elections (D218) |

Do not confuse `incident_ttl_ms` with the `hard_ttl` openraft gives one RPC. The deadline on one record is
`assigned_at_ms + ttl_ms - age_ms`, and no timer waits on it.

Fixed values in the code, not config keys, because no experiment varies them:

| Constant | Value | Meaning |
|----------|-------|---------|
| takeover backstop (`takeover.rs::BACKSTOP_MIN`, `BACKSTOP_FACTOR`) | max(1000 ms, 3 × the winner's bid) | a node that lost a round plans anyway if nothing is final by then (D42, D110, D235) |
| `rejoin.rs::NO_LEADER_ELECT_AFTER` | 3 s | a returning node that hears no leader this long may stand itself (D125) |
| `rejoin.rs::ASK_AGAIN_AFTER` | 1 s | how long a returning node waits on one target before it asks the leader again (D135) |
| `rejoin.rs::WAIT_WARNING` | 5 s | how often a node still catching up warns |
| `step_aside.rs::CHECK_INTERVAL` | 1 s | how often a node reads the bids of the last second (D218) |
| `step_aside.rs::QUIET_LIMIT` | 3 s | how long a leader stays quiet waiting for a new leader; the same as `propose_timeout_ms` by default (D218) |
| `step_aside.rs::floor` | 1000 µs, 8 jobs or 500 permille, by score kind | a leader at or below this is never overloaded, so tiny numbers on an idle cluster cannot make a large ratio (D218) |
| `step_aside.rs::WRITE_FLOOR`, `WRITE_SAVES_TENTHS` | 20 ms, 2.5 saves | a leader's mean write time counts only above both, so a slow disk cannot pass for a busy leader (D228) |
| `node/service.rs::MAX_PLAN_ATTEMPTS` | 3 | plans of one report before the winner makes a plan with nobody on it final (D35, D220) |
| `node/mod.rs::CAMPUS_CHECK_INTERVAL` | 200 ms | how often a node checks that the campus is registered, and proposes it if it may (D164) |
| `degraded.rs::REPLAY_INTERVAL`, `REPLAY_BATCH` | 100 ms, 64 | how fast provisional answers are replayed (D122) |
| `transport.rs::STALE_CONNECTION` | 1 s | a peer connection whose calls have gone unanswered this long is replaced (D119) |
| `datagram.rs::CHANNEL_IDLE` | 60 s | a user program with no traffic for this long loses its channel on the node (D170) |
| `datagram.rs::RECEIVE_BUFFER_BYTES` | 8 MiB | every UDP socket asks for this; the kernel caps it at `net.core.rmem_max`, then doubles it (D171) |
| `proto::MAX_DATAGRAM_BYTES` | 1,400 bytes | the largest message on UDP, so it is never split on an Ethernet path (D170) |
| `transport.rs::MAX_IN_FLIGHT_CALLS`, `CALL_BUFFER` | 20,000, 2,000 | calls in flight and buffered per stub; an open-loop `--rate` above that makes new calls wait (D173) |
| `proposals.rs::GROUP_LIMIT` | 2000 | most commands in one write, so in one log line and one save. One full group must be sent, saved and answered within `raft_heartbeat_ms` (D133, D228, D234) |
| `store.rs::MAX_COMMANDS_PER_MESSAGE` | 4096 | most commands in one AppendEntries, so a catch-up message can finish inside its time limit (D133) |

## The frontend's local page API

Between a browser and the `frontend` program on the same PC. **Not part of the cluster contract:** no node ever sees
it, and it can change without touching `proto.rs` (D197, D198).

HTTP/1.1, one request per connection, no bodies. Values go in the query string; an unknown or repeated parameter gets
400, another path 404, another method 405. Answers are JSON. Report IDs and digests are text, because JavaScript
rounds a number above 2^53.

| Route | Parameters | Answer |
|-------|------------|--------|
| `GET /` | none | the page |
| `GET /api/info` | none | client_id, zones, severities, capabilities, patterns, defaults, limits |
| `POST /api/report` | `zone`, `severity`, `needs`, `who`, `valid_for_s` | waits for the answer: `{ok, request, answered_by, planned_by, responder, responder_zone, responder_capability, cost_m, status, priority, provisional, expired, log_index, term, latency_us, attempts, zone, severity, needs, who, valid_for_s}`; 504 `{ok:false, gave_up:true, error}` after the D52 budget |
| `GET /api/status` | none | `{ok, nodes:[{id, reachable:true, role, term, leader, last_log_index, last_applied, digest, results, responders, silenced, serving, degraded, members} or {id, reachable:false, error}]}` |
| `POST /api/load/start` | `users`, `think_ms`, `duration_s`, `pattern` | `{ok, started, failed, error}`; 400 past a limit |
| `POST /api/load/stop` | none | `{ok, stopping}` |
| `GET /api/load` | none | `{ok, running_users, running_programs, rows:[{client_id, pid, users, think_ms, pattern, state, exit_code, summary}]}`; state is running, stopping, finished, stopped or failed |

There are no zone routes, because publish-subscribe is Phase 2 (D209).

Constants in `bin/frontend.rs`, not config keys, because no experiment varies them: request head 8 KiB, 5 s to send it,
64 connections, status 800 ms per node, 1000 running users, 1500 offered reports a second, 200 ms least think time,
900 s longest duration, 20 running programs, 10 programs per click, 2 s from TERM to kill.

A program's client ID is the frontend's ID x 10000 + n, from 1. Six digits,
because five-digit IDs belong to `scripts/start-users.sh` (D184).
