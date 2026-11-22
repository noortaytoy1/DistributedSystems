# RescueMesh — Phase 2 assignments

**Phase 2: Discovery, P2P Operation, and Offline Reconciliation**
**Due: Friday, December 5, 2026, 8:00 PM** (about two weeks from November 21)
Repo: <https://github.com/noortaytoy1/DistributedSystems> (`main` at `ef89b56`, the Phase 1 final)

Phase 2 is 30% of the course grade: 10% of it is the demo and 20% the design, report and analysis. Like Phase 1, it is
graded mostly on **design decisions, measurements and the ability to explain trade-offs**, not just on a working demo.

---

## 1. The split at a glance

Everyone keeps the kind of work they owned in Phase 1 (Omar and Wahiba's swap is reflected), so nobody starts from zero.
Each person owns **one mechanism** of about the same size, with its tests, measurements, demo scenario and report section.

| Part | Mechanism | Owner | Builds on (Phase 1) | Demo scenario | Measures | Review pair |
|------|-----------|-------|---------------------|---------------|----------|-------------|
| **G** | Discovery and presence directory (cloud side) | **Omar** | Raft, shared log, state machine (A) | 5. Peer discovery | discovery query latency, presence-table convergence | Wahiba |
| **H** | Client presence agent and the P2P link | **Wahiba** | network endpoint, intake, message types, transport | 6. Direct P2P exchange | P2P delivery latency | Omar |
| **I** | Zone publish-subscribe: dissemination with duplicate suppression and bounded forwarding, and the planner using Phase 2 state | **Yahia** | planner, campus zones, state consistency (E) | 6. Direct P2P exchange (zone flood) | duplicate-message rate, forwarding overhead, coverage | Aabed |
| **J** | Resource offers, leases and claims, conflict detection and the reconciliation state | **Ahmed** | bid manager *and claims* (C4), bidding (B) | 8. Conflicting claim | number and type of conflicts, claim latency | Noor |
| **K** | Offline operation: client disconnection, local outbox, store-and-forward, reconnection | **Aabed** | fault gate, fault injector, rejoin gate, takeover (C, D) | 7. Offline peer | offline reconciliation delay, pending updates kept, dropped or obsolete | Yahia |
| **L** | Phase 2 workload, demo page, experiments and report assembly | **Noor** | client, load generator, metrics, experiments (F) | 9. Scale/stress; 1–4 still pass | throughput with P2P, CPU, memory, network | Ahmed |

Review pairs are chosen so each reviewer depends on the part they review, so both people can explain it at the demo.
Directory and client presence share one contract, gossip and store-and-forward share the retention and duplicate rules,
and claims and the load generator meet in the conflict demo.

### Design-log ID blocks (no more collisions)

Phase 1 ended at **D266**. In Phase 1 two people used the same IDs twice and had to renumber, so each person gets a block:

| Block | Who |
|-------|-----|
| D267–D279 | shared (kickoff, the parts table, cross-part decisions). D267 is this plan |
| D280–D309 | Omar (G) |
| D310–D339 | Wahiba (H) |
| D340–D369 | Yahia (I). D192–D196 are also reserved for publish-subscribe (see the note at the top of `docs/design-log.md`), but use your block unless the old `lane-wire` code turns up |
| D370–D399 | Ahmed (J) |
| D400–D429 | Aabed (K) |
| D430–D459 | Noor (L) |

### Branch names

Same rule as Phase 1 (`AGENTS.md`, Git workflow): `part-<letter>-<topic>`, for example `part-g-directory`,
`part-h-presence`, `part-i-gossip`, `part-j-claims`, `part-k-offline`, `part-l-workload`. Use `shared-<topic>` for
shared files and docs.

---

## 2. How we work (same as Phase 1)

We never used GitHub issues or milestones. **The pull request is the unit of work**, and we keep it that way. The full
rules are in `AGENTS.md`; the short version:

1. **Branch from an up-to-date `main`:** `git switch main && git pull --ff-only && git switch -c part-g-directory`.
2. **Every AI prompt that changes a file must say "grill with docs".** The agent then asks you about every design
   decision, one at a time, with options and a recommendation. Then it writes the code and records each decision in
   `docs/design-log.md` (using **your ID block**) and updates `docs/architecture.md` and `docs/protocol.md`.
3. **`src/proto.rs` is the contract.** A message change needs a design-log entry and a review from every owner who sends
   or receives it.
4. **Before opening the PR:** `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (on Linux or
   WSL), then `git fetch origin && git rebase origin/main`.
5. **Open the PR** with: what and why, how it was tested, contract changes, and the design-log IDs. Your review pair
   reviews it; owners of files you touched outside your part review too. At least one approval, green CI, then **Squash
   and merge** and delete the branch.
6. **Small PRs.** Aim for 2 or 3 per person (contract, then mechanism, then measurements). A huge PR on the last night
   is how merges break.

### Start in 10 minutes: paste this into your Claude Code

Open Claude Code in your clone of the repo and paste the following, replacing the part in angle brackets with your own
section's "Prompt" line:

```text
grill with docs: Phase 2, Part <letter> (<your name>). Read AGENTS.md, docs/design-log.md (the
index and sections 13-14 of docs/architecture.md first), docs/protocol.md and docs/phase2-plan.md
section <n>. My design-log IDs are <block>; use only those. Start with the "Decisions to settle
first" in my section: ask me each one with options and your recommendation. Then create the
branch <branch>, build the first PR (the contract), run fmt, clippy and the tests, push the branch
and open the PR with my review pair as reviewer. Do not push to main.
<your section's Prompt line>
```


---

## 3. Omar — Part G: Discovery and presence directory (cloud side)

**Why you:** you built replication, the state machine and the digest. The Phase 1 design already decided that discovery
is *a second state machine on the same shared log* (D69, `docs/architecture.md` section 13), so it lands in code you know.

**What to build**
- A deterministic directory, in a new pure file `src/discovery.rs` (no network, clock or randomness, `BTreeMap` only,
  like `state.rs`). It holds one presence record per user: user ID, role (ordinary, volunteer responder, resource
  provider), logical zone, peer endpoint, advertised capabilities and resources, lease expiry, and an incarnation number.
- New `Command` variants going through the shared log: register, renew lease, deregister, change zone. Added to
  `CloudState`, and included in the digest and in `state_checkpoint` so "same on all three nodes" can be checked.
- Queries answered by any node from its applied state: who is online, who is in zone *z*, and who offers capability
  *c*. The answer is labelled with the index it was read at.
- The spec's hard cases, each with a test:
  - **delayed heartbeats:** a grace period before expiry;
  - **expired registrations:** removed the same way on every node;
  - **duplicate registrations:** the higher incarnation wins;
  - **server failure and state reconstruction:** a rejoining node gets the directory from the log for free; prove it;
  - **a user moving between zones.**

**Decisions to settle first** (grill them; record each in D280–D309)
1. *What clock expires a lease?* The core must be deterministic, so a node cannot read its own clock at apply.
   **Recommendation:** measure leases in shared-log rounds (each round is `round_ms`), so every node expires the same
   entries at the same log position with no clock in the log.
2. *Read consistency for queries.* **Recommendation:** local reads from applied state, labelled with the index (the
   same rule as Phase 1's status reads); the spec asks for "sufficiently consistent".
3. *Lease length and renewal period.* For example a 10 s lease renewed every 3 s; derive them from Phase 1's lab
   timings (1 s detection, the 2 s client retry).

**Files:** `src/discovery.rs` (new), `src/state.rs`, `src/proto.rs` (`Command`, query messages), the cloud handler for
discovery queries (through `node/service.rs` and `CloudGroup`, not past the middleware line), `tests/discovery.rs`.

**Measures:**
- **discovery query latency:** a node-side event plus the client's own timing;
- **presence-table convergence:** time until all three nodes' checkpoints match after a register or expiry, and after a
  rejoin.

**Demo:** scenario 5. Register many simulated users, query by zone and capability on each node, and show that all
three directories match.

**Report section:** "Discovery directory design and consistency model for presence".

**PRs:**
1. `part-g-directory-contract`: the `Command` and query messages plus design-log entries. **This blocks Wahiba, Yahia
   and Ahmed, so aim for Tuesday, November 25.**
2. `part-g-directory`: the state machine, expiry, queries and tests.
3. `part-g-measure`: the events and the convergence analysis.

**Prompt line:** `Build the discovery directory as a second state machine on the shared log, starting with the Command
and query messages.`

---

## 4. Wahiba — Part H: Client presence agent and the P2P link

**Why you:** you own the network endpoint, request intake and message types, and you built the transport's reconnect
back-off. Phase 2's new traffic, client to cloud for presence and client to client for P2P, is exactly that layer.

**What to build**
- A **presence agent in the client middleware** (`MeshClient`):
  - on startup it registers with the directory (Omar's commands), sent to all three nodes like every request;
  - it renews the lease on a timer and re-registers after a gap;
  - it exposes `discover(zone)` and `discover(capability)` to the application through `src/middleware.rs`, so the
    application still never names a node.
- A **peer endpoint on every client program**:
  - each program listens on its own peer address, which its presence record advertises;
  - a small typed peer service (tarpc, like the rest), for example `deliver(update)` returns an ack;
  - one envelope type for every peer message: update ID, origin, zone, kind (incident summary, ack, resource offer,
    assignment update), version and hop count. Yahia and Aabed build on it, so agree it early.
- The **direct exchange flow:** discover peers through the cloud, then talk to them directly. **The cloud carries no
  peer payload.** Count bytes on the cloud's client port during a P2P exchange to prove it.

**Decisions to settle first** (D310–D339)
1. *The sockets rule.* `AGENTS.md` says only `node/` and `raft/network.rs` open sockets. **Recommendation:** add a
   named exception for the client's peer endpoint, implemented in `node/transport.rs` (as `datagram_client` already
   is) and called from `client.rs`, with a design-log entry.
2. *UDP or TCP between peers.* **Recommendation:** UDP with tarpc, as for client-to-cloud. It matches Phase 1's
   measured path, and loss is covered by acks and Aabed's retention.
3. *Peer ports in the lab config.* **Recommendation:** a base port plus the client ID, read from the one config file,
   with the matching `ufw` rule added to `docs/runbook.md`.

**Files:** `src/client.rs`, `src/middleware.rs`, `src/node/transport.rs` (peer endpoint plumbing, owned by C: ask Aabed
to review), `src/proto.rs` (peer envelope, presence calls), `config.rs` and configs, `tests/p2p.rs`.

**Measures:**
- **P2P delivery latency:** sender to receiver, measured on the sender's clock as a round trip, or one way with the
  clock skew stated;
- **ack rate**;
- **cloud bytes during a P2P exchange** (should be zero).

**Demo:** scenario 6. Two or more clients exchange incident and resource updates while the cloud's counters show none of
it passing through.

**Report section:** "Peer protocol and message formats (timeouts, retries, duplicates)".

**PRs:**
1. `part-h-peer-envelope`: the peer envelope and presence calls in `proto.rs`, plus design-log entries. **By Tuesday,
   November 25; Yahia and Aabed need it.**
2. `part-h-presence`: the presence agent and `discover` API.
3. `part-h-p2p`: the peer endpoint and direct exchange, with tests.

**Prompt line:** `Add the client presence agent (register, renew, discover) and a peer endpoint on every client for
direct P2P exchange, starting with the peer envelope in proto.rs.`

---

## 5. Yahia — Part I: Zone publish-subscribe and the planner using Phase 2 state

**Why you:** the campus and its zones are yours, and so is the planner. Zones become publish-subscribe topics (D131,
D209), and the planner has to start using what Phase 2 makes real: offered resources and online responders.

**What to build**
- **Subscriptions:** a peer subscribes to one or more zones; the subscription is part of its presence record (agree
  with Omar).
- **Dissemination**, as a pure, unit-tested core in a new `src/gossip.rs`:
  - **duplicate suppression:** a bounded seen-cache keyed by update ID, with a size and age limit;
  - **bounded forwarding:** a hop limit (the Phase 2 "TTL"; keep it apart from `Incident.ttl_ms`, as
    `docs/architecture.md` section 13 says) and a fan-out of *k* subscribers per hop;
  - **no broadcast storms:** never forward back to the sender, and never forward a seen ID.
- **The cloud publishes once:** when a plan commits, the cloud pushes one incident summary into the zone, and peers
  spread it from there. This is the "push a notice" in section 13's diagram.
- **The planner uses Phase 2 state:** resource availability comes from Ahmed's offers, and responders can be online
  volunteer responders from the directory, alongside the synthetic campus. The planner stays deterministic and gets its
  inputs passed in.

**Decisions to settle first** (D340–D369)
1. *Push or pull for subscribers.* **Recommendation:** push gossip over Wahiba's peer endpoint, from a subscriber list
   taken from the directory.
2. *Fan-out and hop limit.* **Recommendation:** `k = 3` and a hop limit of 4 as starting values, tuned by measurement in
   the report (forwarding overhead against coverage).
3. *Seen-cache bounds.* **Recommendation:** for example 10,000 IDs or 60 s; justify with the measured update rate.
4. *Planner input.* Should Phase 2 resources replace the synthetic responders or sit beside them? **Recommendation:**
   beside them, so all of Phase 1's experiments and scenarios still run unchanged.

**Files:** `src/gossip.rs` (new), `src/planner.rs`, `src/campus.rs`, the client side of forwarding (with Wahiba, in
`client.rs`), `tests/gossip.rs`.

**Measures:**
- **duplicate-message rate:** duplicates received per delivered update;
- **forwarding overhead:** messages sent per update;
- **coverage:** the share of a zone's subscribers reached, and the time to reach them.

**Demo:** scenario 6 (a zone flood that stays bounded), shown on the zone panel Noor revives.

**Report section:** "Bounded dissemination design (publish-subscribe, duplicate suppression, hop limit)".

**PRs:**
1. `part-i-gossip-core`: the pure core and its unit tests. **It can start on day 1; it doesn't need anyone else's
   code.**
2. `part-i-gossip`: wiring to the peer endpoint and the cloud's publish.
3. `part-i-planner-phase2`: the planner using offers and online responders.

**Prompt line:** `Build zone publish-subscribe dissemination with duplicate suppression, a hop limit and bounded
fan-out, starting with a pure gossip core in src/gossip.rs.`

---

## 6. Ahmed — Part J: Resource offers, leases, claims and conflicts

**Why you:** the original component list gave you the bid manager *including claims* (C4: "load score, bidding,
claims"). This is the spec's "nontrivial distributed-consistency problem", and the Phase 1 design already sketched it:
leases with fencing tokens, where the token is the claim's position in the shared log (D69, D163, report section 14).

**What to build**
- **Resource offers:** resource ID, owner, type, quantity or capacity, current availability, logical location, and an
  expiry or lease duration. Offered by provider clients and kept in replicated state through new `Command`s.
- **Claims as time-limited leases:**
  - a claim on a resource or a response task gets a **fencing token**, its log position;
  - a release, or a write, carrying an older token is refused;
  - leases expire on the same log-round clock Omar picks, so every node agrees.
- **Response assignments:** which client or resource is assigned to which incident, when it was created and when it
  expires. The spec requires these fields.
- **Conflict handling:**
  - two clients, delayed or disconnected, both believe they claimed the same resource;
  - when the claims reach the log, the first wins and the other is told so;
  - a claim made **offline** that turns out to conflict moves the item into a visible **reconciliation state** (the
    spec's wording; also listed as a Phase 2 idea in the design log, D149) instead of silently accepting it;
  - document why this choice (fencing tokens plus log order) was made over vector clocks or quorum confirmation, and
    its limits.

**Decisions to settle first** (D370–D399)
1. *Conflict policy.* **Recommendation:** first in log order wins. Online losers get a clear "taken" answer; offline
   speculative claims that lose go to the reconciliation state for the owner to resolve.
2. *Who can resolve a reconciliation state, and how.* **Recommendation:** the resource owner, or expiry of the lease;
   resolved by a new `Command` that names the winner.
3. *Quantities.* Can a resource with capacity 3 be claimed by three clients? **Recommendation:** yes, counted claims,
   with a conflict only when the total goes over the capacity.

**Files:** a new pure `src/claims.rs` (or inside `state.rs`, to be decided with Omar, who owns `state.rs`),
`src/proto.rs`, `tests/claims.rs`, and the client calls (with Wahiba).

**Measures:**
- **number and type of conflicts** (online lost race, offline conflict, expired-lease write refused);
- **claim latency;**
- **time an item spends in the reconciliation state.**

**Demo:** scenario 8. Two clients claim the same resource under an injected delay or disconnection (use Aabed's offline
switch), and the system shows the conflict and resolves it or surfaces it.

**Report section:** "Consistency model for resources, leases and claims; the conflict policy".

**PRs:**
1. `part-j-claims-contract`: offer, claim and release `Command`s plus design-log entries. **By Tuesday, November 25.**
2. `part-j-claims`: leases, fencing, conflicts and the reconciliation state, with tests.
3. `part-j-conflict-demo`: the deliberate-conflict scenario and its events.

**Prompt line:** `Build resource offers and claims as leases with fencing tokens from the shared log, with conflict
detection and a visible reconciliation state, starting with the Command messages.`

---

## 7. Aabed — Part K: Offline operation and reconciliation

**Why you:** in Phase 1 you built the fault gate, the fault injector, the rejoin gate (a node may not take part until it
is current) and takeover. Phase 2's offline client is the same problem one layer out: a peer that disappears, keeps
working, comes back and has to catch up safely.

**What to build**
- **An offline switch for a client:** a client-side gate that drops all of a client's cloud and peer traffic for a
  chosen time, controllable from the load generator and the web page. It reuses the fault gate's idea.
- **A local outbox:** updates made while offline (new reports, resource offers, claims) are logged locally on disk,
  labelled **speculative** (built on Phase 1's provisional answers, D120 and D122), and flushed on reconnection with
  their original IDs, so a retry is harmless (first commit wins).
- **Store-and-forward:** a reachable peer keeps a pending update for an offline peer, with **retention limits**:
  - a maximum count and a maximum age per peer;
  - **obsolete rules:** drop it if the incident is resolved or expired, or a newer version of the same item exists.
- **Reconnection:**
  - re-register presence (with Wahiba), flush the outbox, and collect pending updates from peers and missed zone
    updates from the cloud;
  - conflicting speculative claims go to Ahmed's reconciliation state;
  - what the owner of an incident or resource may do while that owner is offline, and what others may not, follows the
    consistency model, and the policy says so.
- **The written best-effort offline policy:** the spec requires it to be clearly documented.

**Decisions to settle first** (D400–D429)
1. *Which operations are allowed offline.* **Recommendation:** creating reports and offers is allowed (speculative,
   replayed later); claims are allowed but speculative and may lose; nothing that changes someone else's item is
   allowed.
2. *Retention limits.* **Recommendation:** for example 100 updates or 5 minutes per offline peer, and drop on obsolete.
   Justify with measurements.
3. *Who holds pending updates.* **Recommendation:** the sender keeps them, plus one zone peer chosen deterministically,
   so an update is not lost if the sender goes offline too.

**Files:** a new `src/offline.rs` (client side), `src/client.rs` (with Wahiba), the client gate (reusing
`FaultGate` ideas), `bin/ctl.rs` or a loadgen flag for the switch, `tests/offline.rs`.

**Measures:**
- **offline reconciliation delay:** from reconnect to fully caught up;
- **pending updates** kept, delivered, dropped at the limit, and dropped as obsolete;
- **speculative actions** that ended differently on record.

**Demo:** scenario 7. Disconnect a client, create updates elsewhere, reconnect it, and show the synchronization and what
was dropped and why.

**Report section:** "Offline operation and the best-effort reconciliation policy".

**PRs:**
1. `part-k-offline-switch`: the client gate and outbox, with tests. **It can start on day 1.**
2. `part-k-store-forward`: pending updates, retention and the obsolete rules.
3. `part-k-reconnect`: reconnection, replay and the events.

**Prompt line:** `Build offline operation for clients: an offline switch, a local outbox of speculative updates,
store-and-forward with retention limits, and reconciliation on reconnect, starting with the offline switch and outbox.`

---

## 8. Noor — Part L: Phase 2 workload, demo page, experiments and report assembly

**Why you:** you own the client programs, metrics and experiments (K1–K4, C11), and you ran the lab sessions and the
experiment series. Phase 2 is graded on measurements from "tens of thousands of operations", plus an **integrated**
end-to-end demo, and that is your machinery.

**What to build**
- **Simulated users that live the whole Phase 2 lifecycle** in `bin/loadgen.rs`:
  - a role mix of ordinary clients, volunteer responders and resource providers;
  - they register, subscribe to zones, report, discover, exchange P2P, offer and claim, and go offline and come back;
  - all seeded, so runs repeat, and still "more programs means more load, no logic change".
- **The demo page** in `bin/frontend.rs`:
  - the **zone updates panel**, revived from commit `e47e9d4` (D204, D209);
  - a **directory view** showing the same answer from all three nodes side by side, for scenario 5;
  - an **offline toggle** for a client (calls Aabed's switch);
  - a **conflict view** listing items in the reconciliation state, for scenario 8.
- **Experiment cells and analysis:**
  - new cells in `experiments/` and scripts in `analysis/` for every Phase 2 metric in the spec;
  - each owner adds their events, and you make them run at scale on the six PCs;
  - re-run Phase 1's scenarios 1–4 and 9 on the final build to show that Phase 2 broke nothing.
- **The demo script and report assembly:**
  - `docs/demo.md` for scenarios 5–8 plus end to end;
  - the Phase 2 LaTeX report skeleton, the "decisions changed" section, and the **contributions section** (each owner
    writes their own section).

**Decisions to settle first** (D430–D459)
1. *Workload mix.* **Recommendation:** for example 80% ordinary, 15% responders, 5% providers, with an offline chance per
   user per minute. Make it a config key so experiments can sweep it.
2. *Experiment cells.* **Recommendation:**
   - users per PC: 100, 1,000, 5,000 (discovery and gossip scale);
   - offline share: 0%, 10%, 30% (reconciliation);
   - an injected delay for the conflict rate;
   - the `quick` profile for tuning and `full` for the report.
3. *Run P2P traffic on the client PCs only.* **Recommendation:** yes; that is the spec's point, and the cloud
   bytes-carried counter proves it.

**Files:** `src/bin/loadgen.rs`, `src/bin/frontend.rs`, `scripts/`, `experiments/`, `analysis/`, `docs/demo.md`,
`docs/report/`.

**Measures:**
- **throughput and latency** as concurrent clients increase, now with P2P on;
- **CPU, memory, network and queues** on cloud and client PCs;
- the **Phase 1 regression** check.

**Demo:** scenario 9 (scale/stress), and the end-to-end run in which every scenario happens in one session.

**Report section:** "Experimental methodology, raw data, plots and analysis", and the assembly of the whole report.

**PRs:**
1. `part-l-workload`: the user lifecycle in loadgen. **It can start on day 1** with stub calls, and switches to real ones
   as G, H, J and K merge.
2. `part-l-demo-page`: the page panels.
3. `part-l-experiments`: cells, analysis and final runs.
4. `shared-phase2-report`: the report.

**Prompt line:** `Extend loadgen with the Phase 2 user lifecycle (roles, register, subscribe, discover, P2P, offer and
claim, go offline), starting from stubs where other parts are not merged yet.`

---

## 9. Kickoff: do this first (Saturday, November 22)

1. **A one-hour call.** Everyone reads their own section, then agrees on the three contract points everyone depends on:
   - Omar's **directory messages** and the **lease clock**;
   - Wahiba's **peer envelope**;
   - Ahmed's **claim commands**.

   Writing them down in the call saves a week of rework.
2. **Merge `shared-phase2-plan`.** It adds this file as `docs/phase2-plan.md`, adds parts G–L, their owners and the
   review pairs to the parts table in `AGENTS.md`, reserves the ID blocks at the top of `docs/design-log.md`, and
   records all of it as D267. After that, everyone's agent can read the plan from the repo.
3. Everyone starts their **first PR** the same day. The contract PRs (Omar, Wahiba, Ahmed) are small, and they unblock
   the rest.

## 10. Timeline (two weeks)

| When | What | Who |
|------|------|-----|
| Sat Nov 22 | Kickoff call; `shared-phase2-plan` merged | all |
| Tue Nov 25 | **Contract PRs merged**: directory messages and lease clock (G), peer envelope (H), claim commands (J) | Omar, Wahiba, Ahmed |
| Nov 22 – Nov 28 | Work that needs no one else: gossip core (I), offline switch and outbox (K), loadgen lifecycle with stubs (L) | Yahia, Aabed, Noor |
| Nov 25 – Dec 1 | Mechanism PRs, each with tests; review each other's PRs within a day | all |
| **Tue Dec 2** | **Integration day in the lab** on the six PCs: run scenarios 1–9 end to end, fix what breaks | all |
| Dec 2 – Dec 4 | Experiment runs (Noor drives; each owner checks their own numbers); report sections written | all |
| Thu Dec 4 | Report assembled; demo rehearsal | all |
| **Fri Dec 5, 8 PM** | Submit report and code; demo | all |

**Rule of thumb from Phase 1:** anything that depends on timing must be tried on the lab PCs, not only on one machine.
The lab Wi-Fi set every timer last time (report sections 9 and 16).

## 11. What Phase 2 must deliver (spec checklist)

| Spec requirement (Phase 2) | Owner(s) |
|----------------------------|----------|
| Replicated Discovery/Presence Service | Omar (G), Wahiba (H, client side) |
| Delayed heartbeats, expired and duplicate registrations, server failure, reconstruction, zone moves | Omar (G) |
| Direct P2P communication after discovery; the cloud out of the data path | Wahiba (H) |
| Zone subscriptions; dissemination with duplicate suppression and bounded forwarding | Yahia (I) |
| Planner combines severity, age, distance, **resource availability** and capability | Yahia (I), with Ahmed's offers |
| Pending updates for offline peers, with retention limits and obsolescence | Aabed (K) |
| Client disconnection and reconnection; documented best-effort offline policy | Aabed (K) |
| Owner offline: only permitted operations; speculative actions labelled and reconciled | Aabed (K), Ahmed (J) |
| Leases and reservations; at least one deliberate conflict, detected and resolved or surfaced | Ahmed (J) |
| Response assignment with created and expiry times | Ahmed (J) |
| Integrated end to end with the Phase 1 cloud | Noor (L), everyone |
| Measurements: discovery latency, presence convergence, P2P latency, duplicate rate, forwarding overhead, reconciliation delay, conflicts | each owner's events; Noor's cells and analysis |
| Report: directory design, peer protocol, offline policy, consistency choices, analysis, contributions | each owner's section; Noor assembles |

## 12. Definition of done (for every part)

- Merged to `main` through reviewed PRs, with CI green.
- Every decision is in `docs/design-log.md` within your ID block, and `docs/architecture.md` and `docs/protocol.md` are
  updated, with the diagrams marked **built**.
- Unit tests for the pure core and a multi-node test in `tests/`, stable over 10 runs in a row.
- Your events are in `metrics.rs`, and your metric appears in Noor's analysis output.
- Your scenario runs on the six lab PCs and is in `docs/demo.md`.
- Your report section is written, and you can explain the whole system at the demo, not just your part.
