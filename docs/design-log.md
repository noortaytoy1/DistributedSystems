# Design log

One entry per decision a teammate might not expect. Newest last. Each entry has the date, the
deciding part, the decision, why we made it, and what we rejected.

---

### 2026-10-20 · A · Raft through openraft 0.9.25, behind a `Consensus` trait

**Decision.** openraft 0.9.25 with the `single-term-leader`, `storage-v2` and `serde` features,
pinned exactly in `Cargo.toml`. Node code uses Raft only through `raft::Consensus`.
**Why.** openraft is a mature Raft implementation. `single-term-leader` gives standard Raft
terms (at most one leader per term), which the other rules rely on. The trait lets us swap in
our own Raft if the library is not approved.
**Rejected.** Writing Raft ourselves up front: weeks of work before anything else can be tested.
**Status.** Pending the instructor's approval.

### 2026-10-20 · all · Wire format: length-delimited TCP frames with bincode

**Decision.** A 4-byte length prefix, then a bincode body (varint integers, 16 MiB limit,
trailing bytes rejected).
**Why.** Compact and fast for the many small messages (heartbeats, bids). Every openraft message
type has plain serde derives with no `skip_serializing_if`, so bincode round-trips them. The size
limit stops a corrupt length prefix from allocating huge buffers.
**Rejected.** JSON: larger and slower on the hot path, and it cannot use struct map keys. gRPC:
code generation and HTTP/2 for no gain on a LAN.

### 2026-10-20 · C · One multiplexed connection per peer

**Decision.** `PeerClient` keeps one TCP connection per peer and matches replies to requests by
`seq`. Requests on the server side run concurrently.
**Why.** openraft calls `new_client` for every heartbeat round and every election, so a
connection per client would mean a TCP handshake every 50 ms. With multiplexing, a slow request
(such as a forwarded proposal waiting for commit) does not block the heartbeats behind it.
**Rejected.** A connection per request, which is slow. One request at a time per connection,
which causes head-of-line blocking.

### 2026-10-20 · all · The term rule does not apply to Raft RPCs

**Decision.** Non-Raft peer messages from an older term are dropped with `StaleTerm`. Raft RPCs
always go to openraft.
**Why.** A stale leader learns it is stale from the rejection openraft sends back. Dropping its
append-entries silently would leave it believing it still leads.

### 2026-10-20 · C · Control frames bypass the fault gate

**Decision.** `ctl` commands reach a silenced node and get answered.
**Why.** So we can watch a silenced node and end a fault by hand. Client and peer traffic are
still dropped, which is what the fault models.

### 2026-10-20 · A · In-memory storage, no snapshots

**Decision.** The Raft log and vote live in memory, and `SnapshotPolicy::Never` means the log is
never purged. A returning node always catches up by log replay.
**Why.** Faults are network silences of up to 20 s. The process and its memory survive, so the
log is never lost and replay is enough. Disk persistence and snapshots are out of Phase 1.
**Consequence.** Restarting a node process is not a supported fault: a restarted node forgets
its vote, which Raft forbids. Fault injection must use the fault gate, never a process kill.

### 2026-10-20 · A · Every node calls `initialize` with the full membership

**Decision.** At startup every node calls `Raft::initialize` with all nodes from the config and
ignores `NotAllowed`.
**Why.** The cluster forms no matter which nodes start first. openraft documents that calling
`initialize` on several nodes with the same membership is safe.

### 2026-10-20 · A · Any node proposes; followers forward to the leader

**Decision.** `Consensus::propose` works on every node. On a follower, openraft answers
`ForwardToLeader`, and the node sends the command to the leader as `PeerBody::Propose`. The
leader commits it and answers with the outcome. Failed attempts are retried with backoff (5 ms
doubling to 100 ms) until `propose_timeout_ms`. A single forward waits at most
`peer_rpc_timeout_ms`.
**Why.** The bidding winner is often a follower and must commit its own result. Retrying is safe
because every command is idempotent: a proposal that did commit before a leader change comes
back as `Duplicate` with the same result.
**Rejected.** Only the leader proposes: that turns every winner into a client of the leader with
its own retry logic, which is the same thing in more places.

### 2026-10-20 · A · State digest is a hash chain over the applied log

**Decision.** `CloudState` keeps `digest = FNV-1a(previous digest, index, command, outcome)`,
updated on every applied command. Nodes log it every second, and `ctl status` shows it.
**Why.** Equal digests at equal indexes show that two nodes applied the same log with the same
results. That is the "state is identical on all nodes" check, at O(1) per command. Hashing the
outcome as well catches a non-deterministic state machine.
**Rejected.** Hashing the whole state on demand: O(state) per check, and it misses divergence
that later cancels out.

### 2026-10-20 · A · Retry unreachable peers after two heartbeats, not 500 ms

**Decision.** `PeerLink::backoff` returns `2 × raft_heartbeat_ms` (100 ms) instead of openraft's
default 500 ms.
**Why.** After a fault ends, the leader resumes replication to the returning node within one
backoff period. 500 ms would add up to half a second to every recovery time measured in E4.

### 2026-10-20 · A · No leader, no executor (leader dispatch)

**Decision.** Under leader dispatch, a request that arrives while no leader is known is not
planned by anyone. The client's retry after `client_retry_ms` covers it.
**Why.** Any node guessing would risk two executors, and only first-commit-wins would save us.
Elections take about 150–300 ms, far below the 2 s retry.

### 2026-10-20 · B · Leader dispatch is the default policy until bidding lands

**Decision.** `policy.assignment` defaults to `leader-dispatch`.
**Why.** It works as soon as Raft does, so the other parts can test end to end. Switch the
default to `bidding` once bid rounds work (see `TODO(B)` in `config.rs`).
