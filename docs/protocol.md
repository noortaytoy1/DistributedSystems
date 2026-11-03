# Protocol

Messages, encodings and timers. The source of truth is [src/proto.rs](../src/proto.rs) and
[src/config.rs](../src/config.rs); this page explains them. Change both in the same PR, with a
design-log entry (see AGENTS.md). Sequence diagrams for every flow that uses these messages are in
[architecture.md](architecture.md), and the reasons behind each choice are in
[design-log.md](design-log.md).

## Connections and framing

Each node listens on two TCP ports from the config. Noor has decided that client PCs move to UDP; that is not on a branch yet, so
this page describes the TCP path that runs today (D216).

| Port          | Who connects       | Pattern                                               |
|---------------|--------------------|-------------------------------------------------------|
| `peer_addr`   | other nodes        | tarpc remote calls (`proto::PeerRpc`), many in flight (D119) |
| `client_addr` | clients and `ctl`  | one-way frames both ways, replies matched by request ID |

Every frame is a 4-byte big-endian length followed by the body (`tokio_util`
`LengthDelimitedCodec`), at most 16 MiB (`MAX_FRAME_BYTES`). Client port bodies are encoded with
`proto::encode`: bincode with varint integers and no trailing bytes. Peer port bodies are tarpc
messages in tarpc's bincode format. Malformed frames close the connection.

`TCP_NODELAY` is set on every connection.

**Reconnecting.** A node connects to a peer lazily on the first call and reuses the connection.
If a connect fails (refused, or not done before the call's deadline), every call to that peer
fails at once with `TransportError::Backoff` until `2 × raft_heartbeat_ms` has passed; then the
next call connects again. The node logs `peer_unreachable` once when the outage starts and
`peer_reconnected { down_ms }` when a connect succeeds (D89–D92). A silenced peer does not
trigger this: its listener stays up, so connects succeed and calls time out instead.

## Client protocol

```
client                          every node
  │── ClientFrame::Request ──────▶│  (same frame to all nodes)
  │                               │  executor plans, commits through Raft, then replies
  │◀─────── ServerFrame::Reply ───│  (first reply wins; later replies for the ID are ignored)
```

This send to all the nodes is the group send, and the application does not know it happens: it makes one `submit` call
(D210, and [architecture.md](architecture.md) section 5.1a).

- `ClientRequest { id, attempt, age_ms, incident }`. `id = (client, seq)` is unique per client process.
  A retry reuses `id` and increments `attempt`. `age_ms` is how old the report is at this send: one wall-clock
  difference at submit, then a monotonic clock, all on the client. A node judges "too old" from it, so that decision
  never compares two machines' clocks (D143). Client IDs are given out as in D184: 1 to 9 by hand, 11 to 13 frontends, 21
  to 23 zone feeds (Phase 2), five-digit IDs from `scripts/start-users.sh`, and six-digit IDs for a frontend's programs.
- The client resends with the same ID up to `client_max_retries` times. The wait starts at
  `client_retry_ms` and doubles up to `client_retry_max_ms`, with ±20% random jitter (D52).
- `ClientReply { id, from, body }`. `body` is `Done(CommittedResult)`, `Expired(CommittedResult)`, `Provisional(Plan)` or
  `Rejected(reason)`. Every `Done` or `Expired` reply for one ID carries the same committed result, whichever node
  sends it, and the record decides which of the two it is.
- `Expired(CommittedResult)` means the report was too old to plan: `age_ms >= ttl_ms` when it reached the node that would
  plan it, so nobody was sent. It is committed like any result. **It is an answer: a client must stop waiting when it
  gets one.** A client that ignored it would retry for 30 s and give up on a report the cloud had decided about (D143).
- `Provisional(Plan)` comes only from a node in degraded mode: every peer suspected for
  `degraded_after_ms` (D120, D121). It is not committed. The client takes it at once if no `Done` came and
  reports `provisional: true` (D123). A retry gets the same plan. The node replays the result through
  Raft when a peer returns, and first commit wins, so the record can differ; clients are not told (D122).
- A lone node applies the same too-old rule, and its answer is still `Provisional`, with nobody on the plan. Once replayed,
  the record answers `Expired` to a retry.
- A node that already has a committed result for an ID answers a retry straight from its state.

**Connections (D112).** A client keeps one connection per node and pings on it with
`Control(Status)` every 1 s. Nothing received for 3 s replaces the connection. A silenced node still
answers, so it keeps its connection.

## Control protocol (`ctl`, and the frontend's status polls)

Sent on the client port as `ClientFrame::Control`, answered with `ServerFrame::Control`. Control
frames bypass the fault gate.

| Request                    | Effect                                             | Reply             |
|----------------------------|----------------------------------------------------|-------------------|
| `Status`                   | none                                               | `Status(NodeStatus)`, whose last field is `members`, this node's group view (D155) |
| `Incidents(IncidentQuery)` | none. `One(id)` reads one record; `OfClient { client, limit }` reads that client's newest first, at most 20 | `Incidents { node: NodeStatus, rows: Vec<IncidentView> }`. Control frames bypass the gate, so a silenced or returning node answers too: discard its rows unless `node.serving && !node.silenced` (D145) |
| `Silence { ms }`           | drop all peer and client traffic for `ms` (1 to 20000) | `Done`, or `Error` if `ms` is out of range, the node is already silenced (D74), or fault injection is disabled (D77) |
| `SkewLoad { extra_work }`  | set this node's planner work per request. It replaces `plan_work`, so to put a node back set it to `plan_work` again (D205) | `Done`            |

## Peer protocol

Nodes call each other through the tarpc service `proto::PeerRpc` (D119). A connection carries many
calls at once, and replies may come back in any order. Each method takes `from` and `term`, the
sender's current Raft term, and has a deadline, which holds even for a call tarpc could not write because the socket is full
(D150). A connection whose calls have gone unanswered for
1 s is replaced. In the code, `PeerEnvelope { from, term, body }` names a call, and each body below
maps to one method.

| Body                         | Sent by                    | Reply                              |
|------------------------------|----------------------------|------------------------------------|
| `Raft(AppendEntries)`        | leader                     | `AppendEntries(Result<..>)`        |
| `Raft(Vote)`                 | candidate                  | `Vote(Result<..>)`                 |
| `Raft(InstallSnapshot)`      | leader                     | `InstallSnapshot(Result<..>)`      |
| `Propose(Command)`           | follower, to the leader    | `Proposed(Result<Outcome, ProposeError>)`. One command. The leader puts it in the same queue as its own proposals (D133) |
| `FaultBid(FaultBid { round, ticket, ms })` | every node that may bid, to the group (D158) | `Ack`. Only `Ack` means the peer holds it. A bid from an older term gets `StaleTerm` and is dropped. No retry |
| `ReadIndex`                  | returning node, to the leader | `ReadIndex(Result<Option<u64>, ProposeError>)`: the larger of the leader's commit index and its first entry of the term, confirmed by a majority, or `NotLeader { leader }` (D76). Everything committed before the question is at or below the answer, but the answer itself may not be committed yet, so a waiting node asks again after 1 s and keeps the lowest answer (D135) |
| `Bid(Bid { request, attempt, score })` | every serving node, to the peers in its view, not waited for (D104, D154) | `Ack`. Each request and attempt has its own round (D84) |
| `Heartbeat`                  | every node, to every configured peer, in its view or not (D154) | `Ack`                              |

**Term rule.** A receiver drops a non-Raft message whose `term` is lower than its own and
answers `StaleTerm { current }`. Raft RPCs are always passed to openraft, which rejects stale
leaders itself and must reply so they step down.

**Fault election.** A round closes on a node only when it holds a bid from every node in `config.cluster.nodes`, whatever its member list says, and its own
send has finished. The first bid per node stands. A round at or below the highest one this node has left, or below the one in progress, is late and changes
nothing. A higher round replaces the one in progress. Highest ticket wins, ties to the lower node ID, and the winner uses the `ms` in its own bid. `ticket` is
in `0..1_000_000`. `round` is a local counter, not saved. Nothing about a round is replicated.

**Silence.** A silenced node's methods never return, and it fails every outgoing call at once.
The caller sees its deadline pass.

**Peers and roles.** The nodes are peers. Who is the client and who is the server holds only for one call (D212).

## Group communication

The client's send to all the nodes, above, is the group send. `CloudGroup` (`node/group.rs`) is one node's view of the cloud nodes as a
group: it holds the membership, and sends between nodes follow it. It adds no message: bids and heartbeats are the messages above.

| Call | Addressed to | Used by |
|------|--------------|---------|
| `send(timeout, body)` | the peers in this node's view | bids |
| `probe(timeout, body)` | every configured peer | heartbeats |

Both read the sender's term themselves and return each addressed peer's answer. `Ok(Ack)` was delivered, and only this counts.
`Ok(StaleTerm)` was answered but dropped by the term rule. `Err(Timeout)` and `Err(Closed)` are unknown. `Err(Backoff)`, `Err(Connect)`
and `Err(Silenced)` were never sent. Neither call retries, and neither orders anything.

The view starts with every configured node. A peer leaves after `degraded_after_ms` of unbroken suspicion and returns as soon as it is
heard. A node never removes itself. While silenced, its view is frozen. Each change logs `group_view { view, members }`, and also the
first view, after `started`. `view` counts this process's changes from 1, so it means nothing across nodes or restarts.

## Raft log file

With `storage.dir` set, each node writes `node-<id>.wal` before it acknowledges anything to openraft
(D118). A record is a 4-byte big-endian length, an 8-byte FNV-1a checksum and a body encoded with
`proto::encode`: `Vote`, `Committed`, `Append`, `Truncate` or `Purge`. Only `Committed` is unsynced.

A new field in `Command` changes this file's format. An old log still has good checksums but no longer decodes, so it is
cut at its first report with only a warning: after such an upgrade, wipe `storage.dir` on all three nodes. D133 changed it too,
when a log entry became a group of commands.

## Raft log

The log entry payload is `CommandGroup`, a list of `Command`s applied in order at the entry's log index. Applying it
yields a list of `Outcome`s, one per command, in order, which is the response to `client_write`. A blank or membership entry
yields none. Results of one group share a `log_index` (D133).

| Command                                   | Effect                                                                 | Outcome |
|-------------------------------------------|------------------------------------------------------------------------|---------|
| `RegisterResponders { responders }`       | adds unknown responders; known ones are untouched                      | `Applied` |
| `CommitResult(ResultProposal)`            | first result per request ID wins and holds its responder               | `Committed`, `Duplicate(winner)` or `Stale` |
| `ReleaseResponder { responder, request }` | frees the responder if `request` holds it. Proposed by the executor after the service time, by other nodes after twice that (D39) | `Applied` |

`Stale` means the plan's responder is held by another request or unknown. Nothing is recorded,
and the executor plans again with fresh state.

Every command is idempotent, so a proposal retried after a leader change is harmless.

## Incidents, status and priority

An `Incident` holds `owner` (`UserId { client, user }`), `category`, `zone`, `location`, `severity` (1 to 5), `needs`,
`reported_at_ms` and `ttl_ms`. Its ID is the request ID (D140). A `CommittedResult` adds `executor`, `term`, `plan`,
`age_ms`, `assigned_at_ms` and `log_index`.

**Times.** `reported_at_ms` is on the client's clock and `assigned_at_ms` on the executor's. **Never subtract one from the
other:** `age_ms` is the report's age, measured on the client. `expires_at_ms` in `IncidentView` is `assigned_at_ms +
severity × service_ms_per_severity`, or nothing if nobody was sent. It is the planned end, not an enforced one: a backup
node releases at twice that time, a failed release restarts the wait, and a replayed plan's hold starts when the replay
commits (D144).

**Status** is worked out on read, in no `Command` or `Outcome` (D142):

| The record | Reads |
|------------|-------|
| a responder was sent and is still held | `Assigned` |
| a responder was sent and its release committed | `Resolved` |
| nobody sent, and `age_ms >= ttl_ms` in the record | `Expired`, on any clock |
| nobody sent, and the reader's clock is at or past `assigned_at_ms + ttl_ms - age_ms` | `Expired`. A read rule: no log entry, and a clock stepped back turns it back |
| nobody sent, otherwise | `Unassigned` |
| a lone node's answer, on the client only | `Provisional` |

`Submitted.status` on the client is the status when the answer was given. It needs no clock and no state, so a retry after
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

## Request lifecycle

1. The client multicasts `Request`. Every serving node receives it.
2. If a result is already committed for the ID, any node replies with it.
3. Assignment picks one executor: the bidding winner, or the leader under `leader-dispatch`.
4. The executor plans on its worker pool from its local copy of the state, and works out the priority. If the report's
   `age_ms` has reached its `ttl_ms`, it does not plan: the plan is nobody, cost 0, priority 0, with no worker job (D143). A full
   worker pool refuses the job, logs `overloaded`, and nothing is sent to the client (D48, D138).
5. The executor proposes `CommitResult`. A follower that knows the leader forwards it straight away with `Propose`. On the leader it
   goes into the proposal queue: one write in flight, and everything waiting goes in one log entry and one save (D133).
6. After the entry is committed and applied, the executor replies with the winning result: `Expired` if that record was too
   old on arrival, otherwise `Done`. A node that is not serving at that moment stays quiet and the client's retry gets the
   result from a serving node (D136).
   A node in degraded mode skips steps 3 and 5 and replies `Provisional` straight after planning (D120).
7. If the client hears nothing within `client_retry_ms`, it resends with the same ID.

## Errors, and what each caller sees

| Error | Raised by | What the caller does |
|-------|-----------|----------------------|
| `ProposeError::NotLeader { leader }` | a node that does not lead, answering `Propose` or `ReadIndex` | retries at the hinted leader until its deadline (D26) |
| `ProposeError::Timeout` | no leader committed the proposal before the deadline | the node stays quiet and the client's retry takes over (D36) |
| `ProposeError::Fatal(reason)` | consensus stopped | the node stays quiet and logs it |
| `TransportError::Silenced` | an outgoing call on a silenced node | never sent. The caller treats it as a failed call |
| `TransportError::Backoff`, `Connect` | a peer that cannot be reached, or is in its cooldown (D89–D92) | never sent. openraft maps these to `Unreachable` (D32) |
| `TransportError::Timeout`, `Closed` | no answer by the call's deadline, or the connection closed | unknown: it may have arrived. Bids and pings do not retry. A proposal retries, and that is safe (D27) |
| `TransportError::UnknownPeer`, `Wire` | a node ID not in the config, or a body that does not encode | a bug or a mixed build |
| `Overloaded` | the worker pool, when `threads` are busy and `queue_capacity` jobs wait | logs `overloaded`, no reply, the client's retry takes over (D48, D138) |
| `Outcome::Stale` | the state machine: the plan's responder is held or unknown | the executor plans again, at most 3 times, then stays quiet (D35, D36) |
| `ClientError::GaveUp { attempts }` | `MeshClient`, when its retries run out | `submit` returns the error and `client_gave_up` is logged |
| `ClientError::Connect`, `Timeout`, `Closed`, `Wire` | `client::control`, for `ctl` and the frontend's polls | the tool says that node is unreachable |
| A malformed frame | the receiver | the connection closes (D16) |
| `ReplyBody::Rejected(reason)` | defined, never sent (D36) | none |

## Timers

Defaults from `config.rs`, tuned for a LAN:

| Timer                         | Default      | Meaning                                                   |
|-------------------------------|--------------|-----------------------------------------------------------|
| `raft_heartbeat_ms`           | 50           | leader → follower heartbeat; also the append-entries timeout |
| `election_timeout_min_ms`     | 150          | base of the election timeout. Each node's range is shifted by its place in the config: 150–225, 300–375, 450–525 ms (D115) |
| `election_timeout_max_ms`     | 300          | limits the width of each node's range. openraft also uses it as a lease: with a committed vote, a follower waits its range plus this before it stands, so a failover takes about 0.45 to 0.85 s, and FT-2's target is 1 s (D214) |
| `peer_heartbeat_ms`           | 50           | node → node liveness heartbeat                            |
| `bid_window_ms`               | 20           | bid round closes this long after the request arrives      |
| `takeover_missed_heartbeats`  | 3            | missed peer heartbeats before the next bidder takes over  |
| `client_retry_ms`             | 2000         | client's first wait for a reply; each later wait doubles  |
| `client_retry_max_ms`         | 6000         | longest wait between resends (D52)                        |
| `client_max_retries`          | 5            | resends before the client gives up (about 30 s in total)  |
| `propose_timeout_ms`          | 3000         | a proposal waits at most this long, in the queue, in a write in flight or across leader changes, then fails with `Timeout` (D133) |
| `peer_rpc_timeout_ms`         | 500          | one non-Raft peer call (bid, heartbeat, read index). A forwarded proposal instead waits up to `propose_timeout_ms` (D133) |
| `degraded_after_ms`           | 2000         | every peer suspected this long, and a serving node answers without a commit (D121); one peer suspected this long leaves this node's group view (D153) |
| (derived) reconnect cooldown  | 2 × `raft_heartbeat_ms` | after a failed connect, calls to that peer fail fast this long (D90) |
| `campus.service_ms_per_severity` | 200       | executor releases a responder after this × severity; other nodes after twice that (D111) |
| `campus.incident_ttl_ms`      | 60000        | how long a client says its reports stay valid. A node refuses an older one. Above the client's retries: 30 s, at most 36 s with jitter (D143). `loadgen --ttl-ms` sets it for one run. 0 is refused in the config |
| `faults.interval_min_ms`      | 30000        | each node waits at least this long, from the end of the last round and any silence it elected; the first to finish calls the next round |
| `faults.interval_max_ms`      | 60000        | ... and at most this long (random; must be >= `silence_max_ms`, or rounds cannot finish) |
| `faults.silence_min_ms`       | 1000         | shortest silence a node draws                              |
| `faults.silence_max_ms`       | 20000        | longest silence a node draws (at most 20000)               |
| (derived) fault round deadline | 2 × `peer_rpc_timeout_ms` | a node gives up on a round after this, logs a null victim and waits again (D159) |

Do not confuse `incident_ttl_ms` with the `hard_ttl` openraft gives one RPC. The deadline on one record is
`assigned_at_ms + ttl_ms - age_ms`, and no timer waits on it.

Fixed values in the code, not config keys, because no experiment varies them:

| Constant | Value | Meaning |
|----------|-------|---------|
| takeover backstop (`takeover.rs::BACKSTOP_MIN`, `BACKSTOP_FACTOR`) | max(200 ms, 3 × the winner's bid) | a node that lost a round plans anyway if nothing commits by then (D42, D110) |
| `rejoin.rs::NO_LEADER_ELECT_AFTER` | 3 s | a returning node that hears no leader this long may stand itself (D125) |
| `rejoin.rs::ASK_AGAIN_AFTER` | 1 s | how long a returning node waits on one target before it asks the leader again (D135) |
| `rejoin.rs::WAIT_WARNING` | 5 s | how often a node still catching up warns |
| `node/mod.rs::MAX_PLAN_ATTEMPTS` | 3 | plans of one request before the executor stops and stays quiet (D35) |
| `degraded.rs::REPLAY_INTERVAL`, `REPLAY_BATCH` | 100 ms, 64 | how fast provisional answers are replayed (D122) |
| `transport.rs::STALE_CONNECTION` | 1 s | a peer connection whose calls have gone unanswered this long is replaced (D119) |
| client ping | every 1 s; 3 s with nothing received replaces the connection | D112 |

## The frontend's local page API

Between a browser and the `frontend` program on the same PC. **Not part of the cluster contract:** no node ever sees
it, and it can change without touching `proto.rs` (D197, D198).

HTTP/1.1, one request per connection, no bodies. Values go in the query string; an unknown or repeated parameter gets
400, another path 404, another method 405. Answers are JSON. Request IDs and digests are text, because JavaScript
rounds a number above 2^53.

| Route | Parameters | Answer |
|-------|------------|--------|
| `GET /` | none | the page |
| `GET /api/info` | none | zones, severities, capabilities, patterns, defaults, limits |
| `POST /api/report` | `zone`, `severity`, `needs`, `who`, `valid_for_s` | waits for the answer: `{ok, request, answered_by, planned_by, responder, responder_zone, responder_capability, cost_m, status, priority, provisional, expired, log_index, term, latency_us, attempts, zone, severity, needs, who, valid_for_s, not_carried}`; 504 `{ok:false, gave_up:true, error}` after the D52 budget |
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
