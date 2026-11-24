# Offline operation: the best-effort policy

The spec asks for this to be written down: *"Clients may temporarily go offline. Selected updates should be logged locally and
reconciled upon reconnection according to a clearly documented best-effort policy"*, and *"if one peer is offline, another peer may
retain a best-effort pending update for later delivery. You must define retention limits and what happens when the update becomes
obsolete."* This page is that policy. Every rule names its design-log entry. Part K owns it (Aabed); presence on reconnect is Part H's
(Wahiba).

**Best effort** means: nothing a user did offline is silently changed, nothing is applied twice, and anything let go is logged with its
reason. It does not mean everything made offline will arrive. The limits below say what can be lost and when.

## 1. What "offline" is

A client program goes offline when `MeshClient::go_offline` closes a gate inside every one of its sockets: the ones to the three cloud
nodes and its peer endpoint (D402, D338). Nothing leaves and nothing arrives, not even an answer already on its way, as with a cut
cable. It is the same mechanism as the cloud nodes' fault gate in Phase 1 (D13, D74), one layer out. In experiments, loadgen's
`--offline-at-s N --offline-for-s M` takes a program offline once (D406).

## 2. What a user may do while offline

| Operation | Offline | What the user gets | Entry |
|---|---|---|---|
| Report an incident (`submit_or_queue`) | **Allowed** | `Queued(id)`: the report has its ID and waits in the outbox. It is not planned and has no status until it reaches the cloud | D400, D401 |
| Report an incident (`submit`) | Refused at once | `ClientError::Offline`. Callers that cannot queue are told immediately instead of waiting 36 s | D400 |
| Send a peer update (`send_peer_or_hold`) | **Allowed** | `PeerSend::Held`: held by the sender and delivered later | D407 |
| Send a peer update (`send_peer`) | Refused at once | `PeerError::Offline` | D338 |
| Join, leave, move zone, discover | Refused at once | A presence error. Lease renewals pause | D329 |
| Claim a resource or task | **Not built yet** | Part J (Ahmed). The intended rule: allowed but **speculative**; a claim that loses goes to the visible reconciliation state | plan §6, §7 |
| Change another user's item | Never | No such operation exists, online or offline | plan §7 |

So offline, a user can only **add** things: new reports and new peer updates. Nothing made offline can change or overrule what
someone else did, which is why no offline action needs a conflict rule beyond "first commit wins" (D27).

## 3. What is kept, where, and for how long

| What | Kept by | Where | Limit | When the limit is hit | Entry |
|---|---|---|---|---|---|
| Reports made offline | The client itself | `<metrics.dir>/outbox-client-<id>.jsonl`, one JSON line each, so it survives a restart; memory only without a metrics directory | 10,000 reports | A new report is refused with `OutboxError::Full`; the queued ones are kept | D401, D405 |
| Peer updates that could not be delivered | The sender | Memory | 100 per peer, 10,000 in all, 300 s from when the update was made | The oldest is dropped (`peer_update_dropped`, reason `limit` or `age`) | D407, D408 |

**Why 300 s:** receivers remember the IDs of updates they have taken for 300 s (`p2p::SEEN_FOR`, D461), and so does Part I's gossip
(D344). A held update is never resent after that, so a late resend can always be recognised as a duplicate.

## 4. When an update becomes obsolete

| Case | Rule | Entry |
|---|---|---|
| A newer version of the same subject is made while an older one is held for the same peer | The older one is dropped; only the newest is delivered. The subject is the incident for a summary or an assignment, and the provider and capability for an offer | D409 |
| An older version arrives while a newer one is held | The older one is dropped at once | D409 |
| A resource offer whose `available_until_ms` has passed | Not held, or dropped if it runs out while held | D409 |
| A peer update held for 300 s | Dropped, reason `age` | D408 |
| A report too old by the time it reaches the cloud | **Not** dropped on the client. The cloud refuses it as expired and commits the refusal, so it is on record on all three nodes, under the same rule as any report (D143) | D403 |

## 5. Reconnection, in order

When `go_online` opens the gate:

1. **Presence.** Every user the program had joined registers again straight away, with the same incarnation, because the program did
   not restart (Part H, D329). *The cloud's directory is Part G's and not built yet, so for now every node refuses these calls.*
2. **The outbox.** Every queued report is sent, oldest first, 16 at a time, each **with the ID it was given when it was made**
   (D403, D404). A report the cloud already recorded, for example one sent just before the cut, is answered from that record and never
   planned twice. An answered or expired report leaves the outbox; one with no answer stays for the next reconnect.
3. **Held peer updates.** One redelivery round at once, then every `client_retry_ms` (2 s) on a timer, oldest first per peer. A peer
   that is still away is skipped after one failure per round. Every resend carries the same update ID, so it is at most a `Duplicate`
   at the receiver (D410, D315).
4. **Missed zone updates.** *Not built yet:* the returning peer asks for what it missed in its zones. It needs Part G's directory and
   Part I's gossip wiring.

## 6. Why nothing is applied twice

- Every report has a request ID from the moment it is made, and every peer update an update ID. Neither changes on a resend
  (D113, D315).
- The cloud keeps the **first committed result per request ID**; a later copy gets that result back (D27).
- Peers keep the IDs they took for 300 s and answer a repeat with `Duplicate` (D461).

## 7. What can be lost, stated plainly

- A program that goes away **for good** loses what it holds for peers: they are kept in memory, by the sender only (D407). Reports in
  the outbox survive a restart with the same client ID.
- A peer away for more than **300 s** misses updates held for it.
- Past **10,000** queued reports, new ones are refused, and the user is told.
- A report that waited too long is refused as expired, on record.
- A report sent just before the cut may be planned while the client is away. Its answer is lost on the way, but the resend gets the
  same result (`tests/offline.rs::an_answer_on_its_way_is_lost_while_offline`).

## 8. What is measured

Client events, each with a unix-microsecond timestamp, on the client's own clock (D406, D411):

| Event | Gives |
|---|---|
| `client_offline`, `client_online { offline_ms, queued }` | The outage and what waited |
| `outbox_queued`, `outbox_sent { outcome, waited_ms }` | Each report from queued to answered |
| `outbox_flushed { sent, answered, expired, failed, reconcile_us }` | **The offline reconciliation delay** the spec asks for |
| `peer_update_held`, `peer_update_forwarded { since_made_ms }`, `peer_update_dropped { reason }` | Pending updates kept, delivered and dropped, by reason |

**Tried on one PC** (main at `105d0f6`, `config/local.toml`, 5 users at a 200 ms think time, offline for 5 s): 116
reports queued, all 116 answered after reconnecting, none expired or failed, reconciled in **522 ms**. Every queued report was on all
three nodes with the same `log_index`, its `age_ms` showing the 5 s it waited. The lab figures come from Part L's experiment cells.

## 9. Not built yet

| Piece | Waits on |
|---|---|
| Offline claims and the visible reconciliation state | Part J (Ahmed) |
| Presence on reconnect, end to end | Part G's directory (Omar); the client half is done (D329) |
| Catching up on missed zone updates | Part G and Part I's wiring (Yahia) |
| A second holder for peer updates, so they survive the sender going away | Part G's directory, to name a zone peer (D407) |
| The web page's offline toggle | Part L (Noor) |
