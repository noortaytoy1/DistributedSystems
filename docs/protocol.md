# Protocol

Messages, encodings and timers. The source of truth is [src/proto.rs](../src/proto.rs) and
[src/config.rs](../src/config.rs); this page explains them. Change both in the same PR, with a
design-log entry (see AGENTS.md). Sequence diagrams for every flow that uses these messages are in
[architecture.md](architecture.md), and the reasons behind each choice are in
[design-log.md](design-log.md).

## Connections and framing

Each node listens on two TCP ports from the config:

| Port          | Who connects       | Pattern                                               |
|---------------|--------------------|-------------------------------------------------------|
| `peer_addr`   | other nodes        | request/reply, many in flight, matched by `seq`       |
| `client_addr` | clients and `ctl`  | one-way frames both ways, replies matched by request ID |

Every frame is a 4-byte big-endian length followed by the body (`tokio_util`
`LengthDelimitedCodec`). Bodies are encoded with `proto::encode`: bincode with varint integers,
no trailing bytes, at most 16 MiB (`MAX_FRAME_BYTES`). Malformed frames close the connection.

`TCP_NODELAY` is set on every connection.

## Client protocol

```
client                          every node
  │── ClientFrame::Request ──────▶│  (same frame to all nodes)
  │                               │  executor plans, commits through Raft, then replies
  │◀─────── ServerFrame::Reply ───│  (first reply wins; later replies for the ID are ignored)
```

- `ClientRequest { id, attempt, incident }`. `id = (client, seq)` is unique per client process.
  A retry reuses `id` and increments `attempt`.
- The client retries after `client_retry_ms` with the same ID, up to `client_max_retries` times.
- `ClientReply { id, from, body }`. `body` is `Done(CommittedResult)` or `Rejected(reason)`.
  Every `Done` reply for one ID carries the same committed result, whichever node sends it.
- A node that already has a committed result for an ID answers a retry straight from its state.

## Control protocol (`ctl`)

Sent on the client port as `ClientFrame::Control`, answered with `ServerFrame::Control`. Control
frames bypass the fault gate.

| Request                    | Effect                                             | Reply             |
|----------------------------|----------------------------------------------------|-------------------|
| `Status`                   | none                                               | `Status(NodeStatus)` |
| `Silence { ms }`           | drop all peer and client traffic for `ms`          | `Done`            |
| `SkewLoad { extra_work }`  | add planner work per request on this node          | `Done`            |

## Peer protocol

Every request is a `PeerFrame { seq, envelope }`. The reply is a `PeerReplyFrame` with the same
`seq`. A connection carries many requests at once, and replies may come back in any order.

`PeerEnvelope { from, term, body }`: `term` is the sender's current Raft term.

| Body                         | Sent by                    | Reply                              |
|------------------------------|----------------------------|------------------------------------|
| `Raft(AppendEntries)`        | leader                     | `AppendEntries(Result<..>)`        |
| `Raft(Vote)`                 | candidate                  | `Vote(Result<..>)`                 |
| `Raft(InstallSnapshot)`      | leader                     | `InstallSnapshot(Result<..>)`      |
| `Propose(Command)`           | follower, to the leader    | `Proposed(Result<Outcome, ProposeError>)` |
| `Bid(Bid)`                   | every serving node         | `Ack`                              |
| `Heartbeat`                  | every node, every peer     | `Ack`                              |

**Term rule.** A receiver drops a non-Raft message whose `term` is lower than its own and
answers `StaleTerm { current }`. Raft RPCs are always passed to openraft, which rejects stale
leaders itself and must reply so they step down.

**Silence.** A silenced node reads and discards every incoming frame without replying, and
fails every outgoing call at once. The caller sees a timeout.

## Raft log

The log entry payload is `Command`. Applying it yields an `Outcome`, which is the response
to `client_write`.

| Command                                   | Effect                                                                 | Outcome |
|-------------------------------------------|------------------------------------------------------------------------|---------|
| `RegisterResponders { responders }`       | adds unknown responders; known ones are untouched                      | `Applied` |
| `CommitResult(ResultProposal)`            | first result per request ID wins and holds its responder               | `Committed`, `Duplicate(winner)` or `Stale` |
| `ReleaseResponder { responder, request }` | frees the responder if `request` holds it                              | `Applied` |

`Stale` means the plan's responder is held by another request or unknown. Nothing is recorded,
and the executor plans again with fresh state.

Every command is idempotent, so a proposal retried after a leader change is harmless.

## Request lifecycle

1. The client multicasts `Request`. Every serving node receives it.
2. If a result is already committed for the ID, any node replies with it.
3. Assignment picks one executor: the bidding winner, or the leader under `leader-dispatch`.
4. The executor plans on its worker pool from its local copy of the state.
5. The executor proposes `CommitResult`. A follower forwards it to the leader with `Propose`.
6. After the entry is committed and applied, the executor replies with the winning result.
7. If the client hears nothing within `client_retry_ms`, it resends with the same ID.

## Timers

Defaults from `config.rs`, tuned for a LAN:

| Timer                         | Default      | Meaning                                                   |
|-------------------------------|--------------|-----------------------------------------------------------|
| `raft_heartbeat_ms`           | 50           | leader → follower heartbeat; also the append-entries timeout |
| `election_timeout_min_ms`     | 150          | follower waits at least this long before an election     |
| `election_timeout_max_ms`     | 300          | ... and at most this long (randomised)                    |
| `peer_heartbeat_ms`           | 50           | node → node liveness heartbeat                            |
| `bid_window_ms`               | 20           | bid round closes this long after the request arrives      |
| `takeover_missed_heartbeats`  | 3            | missed peer heartbeats before the next bidder takes over  |
| `client_retry_ms`             | 2000         | client resends after this long without a reply            |
| `client_max_retries`          | 3            | resends before the client gives up                        |
| `propose_timeout_ms`          | 3000         | a node stops trying to commit one proposal after this     |
| `peer_rpc_timeout_ms`         | 500          | one non-Raft peer call (bid, heartbeat, forwarded proposal) |
