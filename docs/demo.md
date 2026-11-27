# Demo script

> **The system has no leader now (D249).** Wherever this guide says "leader", "Raft" or "term", read it this way: all nodes are peers, the `role` column reads `peer`, and the `leader` column reads `-`. For a failure, pick any node. Scene 2b, a busy leader stepping aside, no longer applies: a busy node loses the bidding instead, as in scene 2. With two nodes down, the last one answers `provisional`, and each returns by itself when restarted (D261, D262).

Phase 1 demo on the six machines. It covers the first four rows and the last row of the spec's Required
Demonstration Scenarios table, as Dr Amr asked in class on Oct 28, then his own failure cases. You lead the demo. The
team should rehearse it on the lab machines: **none of this has been run there yet.**

## Adjusting the load, tried on `b6221c5`

Each was tried on one PC (`analysis/results/kills/demo-controls-check.txt`). Use `config/lab.toml` on the six PCs.

| To change | Use |
|---|---|
| A rate you choose, up to 30,000 a second in all | `target/release/loadgen --config config/lab.toml --client-id 21 --rate 800 --duration-s 60 --warmup-s 0`, one per program |
| Users, pause and pattern, from the page, at most 1,000 users and 1,500 reports a second per page | `http://127.0.0.1:9001`, or `/api/load/start?users=60&think_ms=500&duration_s=60&pattern=all-at-once` |
| Users, from a command | `bash scripts/start-users.sh --config config/lab.toml --machine 1 --users 100 --duration-s 60` |
| Slow one node | `target/release/ctl --config config/lab.toml skew --node 2 --work 5000`. Put it back to the `plan_work` in the config, not to 0 |
| Silence one node | `target/release/ctl --config config/lab.toml silence --node 3 --ms 20000` |
| See the nodes | `target/release/ctl --config config/lab.toml status` |

## Before you start

Set up the machines with [runbook.md](runbook.md). `config/lab.example.toml` ships the fault election's timers set never to fire, so only you cause faults, and E1's settings: `plan_work = 1000` and `responders = 300` (D165, D205). To turn
the draw on, see the last scene or runbook section 3. Before every scene that uses `ctl silence`, check `serving = true` on all three. Check `grep assignment config/lab.toml` says `bidding`. Delete old
logs (`rm -rf data/raft runs/lab` on all six), then start node 3, about a second later nodes 2 and 1, and check `ctl status` shows one leader, equal digests and `node N: udp ok` for all three, before any load. Those three
lines come over UDP and the table over TCP, so it shows the two transports side by side. Node 3 tends to lead, and ties still go to node 1, which keeps the two apart on
screen. If you are asked why node 3: because we listed and started it first. Then a frontend on each client machine:
`target/release/frontend --config config/lab.toml --client-id 11 --listen 127.0.0.1:9001` (12 and 13 on the others),
and open `http://127.0.0.1:9001` there.

**Start with the page.** Type a report: zone 2, severity 4, Medical, Send. The card names the responder, who planned it
and who answered. Send two more. Point out that nothing on the page names a node: one click is one `submit`, and the
client middleware makes the copies, which is the group send, and the application does not know it happens (D210). The page
does not prove the election shifts with load; the win count below does.

Start the load from the box on all three client machines: How many user processes 20, Steady, wait 1000 ms, duration
600 s, Start. Use the highest load that held one term for 60 s when you rehearsed on these PCs, starting from 20 users per
client PC. The line under the box says it runs as real background programs, and `pgrep -c -x loadgen` prints 10. Each row
shows its printed summary when it ends.

Keep two terminals visible as a cross-check. If the page fails on the day, they run the whole demo:

```bash
# Terminal 1, the cluster: role, leader, digest, serving, silenced, degraded, members for each node
watch -n 0.5 target/release/ctl --config config/lab.toml status

# Terminal 2, on each node machine: who has won bid rounds so far
grep -c '"i_won":true' runs/lab/node-*.jsonl
```

The table is 109 characters wide. Widen the terminal, because `watch` cuts at its width.

Without the page, the same programs by hand, on each client machine with its own machine number:

```bash
bash scripts/start-users.sh --config config/lab.toml --machine 1 --users 20 --duration-s 600
```

## 1. Normal cloud operation

*Different peers are elected as load conditions change.*

1. Terminal 1 shows one Raft leader and equal digests. Under load the digest column can differ between refreshes; to show equal
   state, compare `state_checkpoint` lines on two nodes at the same `log_index` (D138). Point out that node 3 leads and that wins show on all three: the leader orders the log and
   has no say in who plans (D163), and that the nodes are peers: client and server are the roles of one call (D212). The wins are the evidence that the election follows load. Under even load the leader
   stays put, and a steady term is the healthy result (D217, D218).
2. Run the win count on each node machine twice, a few seconds apart.
3. Raise the load: press Start again on one client machine with more users, or a second batch by hand,
   `bash scripts/start-users.sh --config config/lab.toml --machine 1 --batch 1 --users <N> --duration-s 300`, with N inside the highest
   load found on these PCs. `pgrep` then prints 20. Count again. Kill one program, `kill <pid>`: it prints its summary and leaves, and the
   rest keep going. If a demo goes wrong, `bash scripts/start-users.sh --stop --machine M [--batch B]` stops that machine's users, even if the script itself was killed.

**Expect** the wins to shift between nodes as queues build. At very low load the fastest node can win most
rounds, because the score is expected wait (D45), so show the higher load before drawing conclusions.

**For spikes:** `--pattern all-at-once --users 10`: every user sends on the same beat. Keep it small. `--pattern random`: each user waits 0 to 2 s.

**Show one report on all three nodes.** Take a client's newest record, then compare it across the nodes:

```bash
target/release/ctl --config config/lab.toml incidents --client 1 --limit 1 --node 2
target/release/ctl --config config/lab.toml incidents --id <the ID it printed>
```

**Expect** the same `log_index` and the same fields on all three: a responder, a priority, an owner and a category. A moment
later the same command shows `resolved`, once the responder's release commits. A brief `assigned` on one node against
`resolved` on another is only catch-up.

## 2. Load imbalance

*Raise the load on one server and show the election respond.*

```bash
target/release/ctl --config config/lab.toml skew --node <follower> --work 5000
```

`<follower>` is any row in terminal 1 that is not the leader.

**Expect** that node's win count to grow more slowly than the others', because its plans take longer and its bids
rise. Then put it back with `skew --node <follower> --work 1000`. Do not use `--work 0`: skew replaces `plan_work`, so 0 makes that node the fastest (D165). On the page, the next cards should
move off that node (read from the code, not run).

### 2b. A busy leader steps aside

*Raise the load on the leader itself, and show a normal vote move it (D218).* Keep the users running, because the leader reads its peers' bids. It cannot happen in the first 65 s after a start or a leader change, so if scene 3 has just run, wait a minute.

```bash
target/release/ctl --config config/lab.toml skew --node <leader> --work 20000
```

**Expect** within about 20 s (7 s at 300 users and 11 to 17 s at 60 on one PC, because the skew shows only once that node has won a round with it on, D228), `python3 scripts/watch_events.py runs/lab` on that machine to print `TOO BUSY TO LEAD` and then `STEPPED ASIDE`. Within about a second terminal 1 shows another leader and a term one higher, nobody gives up, and the term stays there for the next 60 s. The wins move off that node as well. **Say this out loud:** the node chose to go quiet, the vote is an ordinary Raft vote, and no timer or vote rule changed. Put it back afterwards with `skew --node <that node> --work 1000`. A skew of 5000, as in scene 2, moves the work but not the leader (measured on one PC, D218).

## 3. Server failure

*The elected server goes nonresponsive under load, and service continues.*

1. Find the leader in terminal 1, and call it `<leader>`. Its role is what this scene is about.
2. Silence it for the longest fault allowed:

   ```bash
   target/release/ctl --config config/lab.toml silence --node <leader> --ms 20000
   ```

**Expect** terminal 1 to show `silenced = true`, and another row reading `leader` in a newer term within about a second (D214). **Say this out loud:** the silenced node's own row may still read
`leader` in the old term. That row is stale. The load generators keep
going. On the others, `peer_suspected` and `takeover_started` appear in the logs. About 2 s after the fault starts, `members` on the
survivors drops the silenced node, and bids stop going to it. Its own row shows `-`. A fault under about 2.2 s never shows in this
column, only in `peer_suspected`. Send another report: the card names another node, after a stall of about a second. At the end
`gave up` should be 0: let the run reach its duration so the rows show their summaries, or read each machine's last line, `reported k of P
programs: done, gave up, retried, no responder, provisional`, or count with `grep -c client_gave_up runs/lab/client-*.jsonl`. A client never
names a node and the page survives the cut, because it lives on the client PC: that is location, failure and access transparency, in the
slides' words and the Sept 6 note's. What we promise: no client names a node, no request is lost, and answers stall for about a
second. We do not say clients do not notice: p99 shows it, and E3 has the measured figure (D165).

## 4. Server recovery

*State is rebuilt before the node takes part again.*

Wait for the 20 s to end, with terminal 1 on screen.

**Expect** `silenced` to go false while `serving` stays false for a moment, then `serving` true with the same
digest as the others, once the node has applied the leader's commit index. `members` on the others lists the node again as soon as it is
heard, before `serving` goes true. Say this out loud: members means reachable, not able to serve. Its role is `follower` now, and its win count rises again. It got no role back and lost no say, because the role never carried a say in who plans (D163). Show how long it took:

```bash
grep '"ev":"rejoined"' runs/lab/node-<leader>.jsonl | tail -1
```

Say what happened: the log lines it missed were copied, then replayed through the state rules; its load
figures and bid rounds were not copied, as each node keeps its own and new traffic updates them; and it knew it was current by asking the leader for a target confirmed by a
majority (D76, D135). Nothing it was working on reaches a client until then, and those clients retry (D136).

## Too old means not planned (Noor's rule, D143)

*A report says how long it stays valid. A node that gets it later answers expired and sends nobody.*

```bash
target/release/loadgen --config config/lab.toml --client-id 9 --users 1 --think-ms 1000 --duration-s 1 --ttl-ms 0
target/release/ctl --config config/lab.toml incidents --client 9
```

**Expect** `expired 1` at the end of the summary's first line, and in `ctl`, on all three nodes, `status=expired`,
`responder=-` and `priority=0`, with `gave up 0`: the report was answered, not lost. `grep report_expired runs/lab/node-*.jsonl`
finds the node that committed it.

For a report that really arrives late, silence all three nodes for 3 s and send one with a 1.5 s TTL:

```bash
for n in 1 2 3; do target/release/ctl --config config/lab.toml silence --node $n --ms 3000; done
target/release/loadgen --config config/lab.toml --client-id 8 --users 1 --think-ms 1000 --duration-s 1 --ttl-ms 1500
```

**Expect** the first send and the first retry to hit closed gates, and the retry at about 6 s to state an age of about 6000 ms
and be refused the same way. **Say** that with the 60 s default this never fires in a measured run, because the client's
retries end first, and the report shows `reports_expired = 0`.

## 5. Dr Amr's case: a machine physically off

1. Under load, **turn off the power of the node machine that leads now**. Not a shutdown, and not `ctl`.
2. **Expect** the same as scenario 3, and `members` on the others drops it about 2 s after that. Clients have nothing to drop: they have no
   connection to it, and its copies vanish while the other two answer (D167). The other nodes replace their connection to it after about 1 s (D119). The check that matters here: `peer_suspected` on
   both survivors within a second, because a stall here is what D150 fixes.
3. Turn it back on and start it again, with the same ID and without wiping anything:

   ```bash
   target/release/node --config config/lab.toml --id <id>
   grep -E '"ev":"(restarted|rejoined)"' runs/lab/node-<id>.jsonl | tail -2
   ```

**Expect** `restarted`, then `rejoined`, then equal digests. The restarted node prints no `Can not initialize` (D137). Its peers print `RPCError err=Unreachable` at `ERROR` while it is down, which is openraft's own line and is expected (D222). Its vote and log came back from disk, and the
rest came from the leader (D118). `ctl status` can show it as leader for a few ms, from its stored vote, before it steps down. A real power cut is the one thing the rehearsal on one PC could not test: there, the one kernel kept its
page cache, so losing unsynced writes was never tried. This scene is that test.

## 6. Dr Amr's case: one off and one failed, one server for 3 clients

**Say first what this trades away** (D120): with one server there is no majority, so the answer cannot be
committed. The last node gives a labelled provisional answer and commits it later, so it can turn out to differ.

1. Under load, turn off one node machine (say node 1).
2. Silence another: `ctl silence --node 2 --ms 20000`.
3. About 2 s later, terminal 1 shows `degraded = true` on node 3. Node 3's `members` now reads `3` alone.
4. The load generators keep receiving answers. At the end they print a `provisional` count. Send a report from the page: the card is
   amber, PROVISIONAL, with no log index and no request ID. Row 3 says degraded.
5. When node 2 returns, `degraded` goes false, and node 3 replays what it answered. Node 3's `members` reads `2,3`. So does node 2's, not
   `1,2,3`: node 2 still suspects the powered-off node 1 (D151). When node 1 returns, every list reads `1,2,3`.

   ```bash
   grep -E '"ev":"(degraded_entered|degraded_left)"' runs/lab/node-3.jsonl
   grep -c '"ev":"provisional_conflict"' runs/lab/node-3.jsonl
   grep '"ev":"group_view"' runs/lab/node-3.jsonl
   ```

**Expect** nodes 2 and 3 to reach equal digests. Any conflicts show how many answers did not stand. Then bring
node 1 back as in scenario 5, and all three digests should match.

**Be ready to say:** after a conflict with reason `responder_taken`, `ctl incidents` reads `unassigned` for that report, although
its client was told a responder was sent. The client is not told (D122). The conflict count is the evidence, and a visible
reconciliation status is Phase 2 (D149).

## 7. Scale and stress

The last row of the table is not a live event, because it takes an hour. Show the results. Stop every frontend and all demo users first: the runner refuses to
start while one runs.

```bash
scripts/run-experiments.sh experiments/hosts quick      # beforehand
python3 analysis/analyze.py runs                        # in its venv: analysis/README.md
ls analysis/out
```

Walk through `e1_latency_throughput.png`, `fairness.png` and `fault_phases.png`, which leaves out the `ft8` and `e5` cells, and `summary.csv` for
recovery and election overhead: `fault_elections`, `fault_election_mean_ms`, `fault_bids_per_round` and `fault_split_decisions`. Show `leader_plan_share` beside `elections`, from the runs where one node led throughout, and the baseline's 1 beside it (D166). Do not call it a third unless the data says so. State the profile, the runs per cell and the limits (D63). If there is time, run one cell
live:

```bash
scripts/run-experiments.sh experiments/hosts quick e1-300-bidding
```

## The servers elect a machine to fail (D158, D162)

*The servers periodically elect one machine to ignore communication.* This one happens by itself. About 3 minutes, nobody typing.

The draw ships off, so this scene turns it on. Stop the nodes. In `config/lab.toml`, under `[faults]`, use the four values in the comment for this scene, and copy the file to all six machines:

```toml
interval_min_ms = 20000
interval_max_ms = 30000
silence_min_ms = 20000
silence_max_ms = 20000
```

Start the nodes, and start the load on all three client machines. A round comes within 30 s. Say nothing and type nothing: the `silenced` column turns true on one node in
Terminal 1, the others keep serving, and it turns false again 20 s later. Then, on each node machine:

```bash
grep '"ev":"fault_election"' runs/lab/node-*.jsonl
```

The numbers are there: each node's line has the same three tickets and names the same victim, the highest. No leader appears anywhere on that path.

If one node's line has `victim: null`, explain it as the rule at work: that node missed a number, so it did nothing, and the others went ahead.

# Phase 2 scenarios

The spec's Required Demonstration Scenarios table, rows 5 to 9, then the whole table in one session. The numbers below are the
spec's, not this page's Phase 1 scenes.

## Spec scenario 5. Peer discovery (Part G, D280–D295)

*Register many simulated users, query by zone and capability on each node, and show that all three directories match.*

**On one machine, with real nodes and the real presence agent:**

```bash
cargo test --release --test discovery -- --nocapture
```

Eight tests, one per hard case of the spec: registration and discovery by zone and capability with the same answer on every
node, a zone move, expiry with no renewals (the same on every node, by the log clock, D281), a restarted program's higher
incarnation (D286), a node silenced while users register and rebuilding the directory from the log when it returns (D287), a user
leaving, the same under rotating rounds, and the measurement events.

**On the lab cluster**, start users that register. Turn on loadgen's Phase 2 lifecycle (D430): add these two lines to
`config/lab.toml` and copy it to all six, as in runbook section 2. The nodes ignore them.

```toml
[workload]
lifecycle = true
```

Then start one program on each client machine, with client IDs 4, 5 and 6:

```bash
target/release/loadgen --config config/lab.toml --client-id 4 --users 20 --think-ms 400 --pattern random --duration-s 600
```

Each user registers, subscribes to its zone, then reports, discovers, sends to peers, offers or claims by role, and moves zone
(D430, D431). When the program ends, its `phase2:` line says `register:ok 20` if all 20 joined. Ask all three nodes side by side:

```bash
target/release/ctl --config config/lab.toml discover --zone 3
target/release/ctl --config config/lab.toml discover --capability medical
```

**On the page (D443).** In **Who is online**, pick zone 3 or a capability, and press **Ask all three nodes**
(`GET /api/directory?zone=3`). The page asks each node on its own, the same way `ctl discover` does, and shows three columns, one
per node. Each column lists that node's records (user, role, zone, capabilities) and the log index it read at, or says
`no answer (silenced or catching up)`. Under the columns is one verdict line, the same as `ctl`'s: `same answer from N nodes`,
`answers differ, read at different log indexes: ask again`, or `DIFFERENT answers at the same log index: a bug`.

**Expect** one block per node, or one column on the page, each with its `log_index`, and a last line
`same answer from 3 nodes`. Under steady renewals two nodes can read a few entries apart; the line then says
`answers differ, read at different log indexes: ask again`, and asking again agrees. Then silence a node
(`ctl silence --node 2 --ms 8000`), ask again (`no answer` from it, the other two agree), and once it is back, ask again: it
holds the same directory, rebuilt from the log. Say how it works: the directory is a second state machine on the same log
(D69), a lease expires by the time carried in the log and not by any node's clock (D281), and a record is live, then late for
5 s of grace, then gone (D285). The numbers are `discover_*` and `presence_convergence_*` in Part L's analysis.

## Spec scenario 6. Direct P2P exchange (Parts H and I, D310–D339, D460–D465, D340–D369, D490)

*After discovery, clients exchange updates directly, with the cloud out of the data path, and a zone update spreads with
duplicates suppressed and forwarding bounded.*

**On one machine, with real client programs, and three real nodes where the cloud matters:**

```bash
cargo test --release --test p2p --test gossip -- --nocapture
```

Five P2P tests: a direct exchange whose repeat is answered as a duplicate, an update for another program's user refused, nothing
sent or heard while offline, ports taken from the range, and the one for this scenario, `the_cloud_carries_none_of_a_p2p_exchange`:
three real nodes run while two programs exchange 100 updates, and no node's client-port `rx_bytes` moves while the programs' peer
counters do (D465). Seven gossip tests: a committed report reaches every program of a five-program zone, a stale directory does
not pull another zone in, a repeated answer is not spread again, a dead peer does not stop the rest, a subscriber that was offline
gets its updates on return (D415), with no directory nothing is sent, and the whole path on three real nodes,
`a_real_cloud_report_reaches_its_zone_through_the_real_directory`: five programs join through Part G's directory, one reports, and
the other four get its summary and assignment peer to peer (D369). The hop limit has its own unit tests in
`cargo test --release --lib gossip` (D346).

**On the lab cluster**, keep lifecycle users running, as in spec scenario 5 or the end-to-end session below. Each user discovers
by zone or capability, then sends straight to a user it found, at that program's peer address: a provider sends its offer,
anyone else its program's last report (D436, D441). Each committed report is also published into its zone by the program that
sent it (D351).

**On the page (D439).** In **Zone updates**, pick zone 2 and press **Follow** (`POST /api/zone/follow?zone=2`). The page joins a
user of its own in zone 2, user 1,000,002, which subscribes its program (D359). Within about 10 s, the time a publisher takes to
refresh its zone list (D358), the panel lists the newest 50 updates it receives: incident summaries, assignments and offers, each
with its sender and hops. `GET /api/zone/updates` also gives each one's version. Then send a zone 2 report from another client
PC's page: its summary and assignment should arrive here. A page never lists its own reports, because gossip never sends an
update back to its origin (D347).

**Expect** hops from 0 to 7, never more: 0 is straight from the sender, and each forward adds one (D316, D346). An offer always
has 0 hops: a provider found this page's user by discovery and sent it directly, which is the direct exchange. An assignment
always came through the zone flood, and its version is its commit's log index (D355). Then, on a client machine, count the zone
copies by hops, and what the peer endpoints answered:

```bash
grep -h '"ev":"gossip_received"' runs/lab/client-*.jsonl | grep -o '"hops":[0-9]*' | sort | uniq -c
grep -h '"ev":"p2p_received"' runs/lab/client-*.jsonl | grep -o '"outcome":"[a-z]*"' | sort | uniq -c
```

**Expect** nothing above `"hops":7`, and `duplicate` beside `accepted`: in D345's simulation a zone update costs about two repeats
per delivery. Each repeat is answered at the endpoint and never handed on (D461).

**Say how it works:** in a P2P exchange the cloud is only the directory. A program learns its peers' addresses from `discover`;
then every peer message goes over UDP from one program's peer endpoint to another's, as one tarpc call, `deliver`, whose reply is
the ack, tried up to 3 times, 500 ms each, under the same update ID (D312, D314, D339). The nodes have no peer endpoint, so
nothing on the panel passed through them (D351, D435). In a zone, a program that gets a new update sends it to 3 programs of the
zone, never to itself or the origin (D345, D347, D354). It forwards only while `hops` is below 7, so a copy crosses at most 8
links (D316, D346). A repeat is dropped by its update ID at the endpoint (D461), and an older version of the same subject by
gossip (D342, D490). Only programs subscribed to the zone forward it (D348). The hop count is the spec's Phase 2 TTL under
another name, kept apart from a report's `ttl_ms` (D346). Coverage of a big zone is not certain: this gossip misses about 5% of
programs at a fan-out of 3, and Part K's held copies cover a program that was away (D345, D415).

**If asked why 7 hops, not the plan's 4:** D345's table, which
`cargo test --release --lib gossip::tests::measure_coverage -- --ignored --nocapture` prints. In a 500-program zone the plan's
limit, `max_hops` 3 or 4 links, reaches 21%, and 7 reaches 93%. In the lab's small zones the extra hops cost nothing, because
copies stop once everyone has the update.

The numbers, in Part L's analysis: P2P latency, `p2p_sent.latency_us`, a round trip on the sender's clock, retries included
(D464); the duplicate rate, repeats per delivered update (D350); forwarding overhead, copies sent per update, from
`gossip_published.targets` and `gossip_received.forwarded`; coverage, the programs that logged `new` per update over the zone's
programs (D365); and the cloud's client-port bytes (`udp_counters`) beside the P2P messages (D435). `gossip_received.age_ms` reads
two PCs' clocks: quote it only beside `client_clock_skew_ms` (D233).

## Spec scenario 7. Offline peer (Part K, D400–D413)

*Disconnect a client, create updates elsewhere, reconnect it, and show the best-effort synchronization.* The policy being shown is
[offline-policy.md](offline-policy.md); keep it open, because the questions come from it.

**Reports made offline.** On one client machine, with the cluster running and terminal 1 on screen:

```bash
target/release/loadgen --config config/lab.toml --client-id 7 --users 5 --think-ms 200 --duration-s 12 --offline-at-s 3 --offline-for-s 5
```

**Expect** the usual summary, then one more line, as it printed on one PC:

```text
offline: queued 116, sent 116, answered 116, expired 0, failed 0, reconciled in 521.8 ms, still queued 0
```

Say what happened: for 5 s the program heard and sent nothing, not even answers on their way. Its users kept reporting, and each
report got its ID at once and went into the outbox, a file on this PC. On return every one was sent with that same ID, so a report the
cloud already had could not be planned twice. Then show one of them on all three nodes:

```bash
grep '"ev":"outbox_queued"' runs/lab/client-7.jsonl | head -1          # take its request, e.g. {"client":7,"seq":...} -> 7-<seq>
target/release/ctl --config config/lab.toml incidents --id 7-<seq>
grep -E '"ev":"(client_online|outbox_flushed)"' runs/lab/client-7.jsonl
```

**Expect** the same `log_index` and fields on all three, and `age_ms` near 5000: the time it waited offline, measured on the client's
clock (D143). `outbox_flushed.reconcile_us` is the offline reconciliation delay. To show the expiry rule, run it again with
`--ttl-ms 3000`: reports made early in the outage come back `expired`, refused by the cloud and on record, not dropped by the client
(D403). On one PC: `offline: queued 103, sent 103, answered 72, expired 31, failed 0, reconciled in 436.3 ms, still queued 0`.

**From the page (D438).** On a client PC's page, in the **Offline switch** panel, press **Go offline**: it takes the page's own
client offline. Send a report: it shows a yellow QUEUED card with its request ID. Press **Come back online**: the page sends the
outbox and prints what happened, as it did on one PC with three local nodes:
`Back: sent 1, answered 1, expired 0, no answer 0, in 68 ms.` That time is the offline reconciliation delay, on the client's
clock. Look the card's ID up on all three nodes with `ctl incidents --id`, as above.

**Peer updates held for an offline peer.** The page switches only its own client, so show this with the test that drives two
real client programs, on one machine:

```bash
cargo test --release --test store_forward -- --nocapture
```

It shows three cases: the sender offline, the peer offline (the update arrives by the 2 s retry, with nobody calling), and a newer
version replacing an older one while held. Then say the limits: 100 per peer, 10,000 in all, 300 s, the same as how long receivers
remember IDs (D408). On the lab, the page shows the receiving end: follow a zone in **Zone updates** before going offline, and
after **Come back online** the zone updates its peers held for it arrive within about 2 s (D415, D410).

**If asked what is lost:** read section 7 of the policy. **If asked about claims made offline:** Part J, and the rule is speculative
with a visible reconciliation state (section 2).

## Spec scenario 8. Conflicting claim (Part J, D370–D381)

*Two clients claim the same resource under an injected delay or disconnection; show the system detecting the conflict and
resolving it, or surfacing it.* It runs on one machine as a test against a real three-node cluster, on the lab from the page's
**Conflicting claim** panel (D444), and at scale inside loadgen's lifecycle (D441, D442).

**On one machine, with real nodes:**

```bash
cargo test --release --test claims -- --nocapture
```

Three cases, one per test: an offer claimed then released; an online claim that loses a capacity race, refused at once; and
the deliberate conflict the spec asks for, `a_speculative_conflict_is_surfaced_and_the_owner_resolves_it_for_the_challenger`.

**Walk through the third test;** the page's panel plays the same steps on the lab. A provider offers a resource of capacity 1. An
incumbent claims it online and gets it. A second claimant (the challenger) goes offline (`MeshClient::go_offline`, Aabed's
switch), tries to claim the same resource while offline (`claim_resource_or_queue`, queued in the claims outbox, a file on the
client, D414), then reconnects (`go_online`). The queued claim is resent marked `speculative`, and because it would now go over
capacity it does not get a silent refusal: it opens a reconciliation entry instead, visible in every node's applied `Claims` state
(`s.claims().reconciling_on(&resource)`). The resource's owner settles it, naming the challenger
(`resolve_reconciliation(..., ReconciliationWinner::Challenger)`): the incumbent's claim is evicted and the challenger holds
the resource under a fresh fencing token.

**On the lab cluster, from the page (D444).** On a client PC's page, in **Conflicting claim**, press **Stage a conflict**
(`POST /api/conflicts/stage`). The page plays the third test by itself, with three of its own users: a provider offers a resource
with 1 unit, an incumbent claims it online, the page goes offline, a challenger claims the same unit (queued in the claims
outbox), the page comes back online, and the queued claim is resent as speculative and opens a reconciliation entry. The panel
lists the conflicts this page staged, newest first: the resource, its owner, the incumbent and the challenger, each step with the
entry's token, and two buttons, **Challenger wins** and **Keep incumbents**, which resolve the entry as the offer's owner
(`POST /api/conflicts/resolve`). The row then shows the answer: resolved at log index N, or the refusal. The panel lists only
entries this page made, because no node offers clients a list of reconciliation entries. The cloud-wide count is in each node's
`claims_state` event (`reconciling`).

**Under load (D441, D442).** With the lifecycle on, providers offer and responders claim and release. A claim on a full offer is
refused at once: `claim:refused` in the program's `phase2:` line, and `claim_conflict` with `kind` `online_refused` on the node.
A claim made offline is resent as speculative when its program is back, and may open an entry. Loadgen leaves those open, so they
show in every node's `claims_state.reconciling`. `workload.claim_delay_ms` is the injected delay that sweeps the conflict rate: a
responder claims on news that much older (`ph2conflict`, spec scenario 9). On each node machine:

```bash
grep '"ev":"claims_state"' runs/lab/node-*.jsonl | tail -1     # offers and reconciling now; claimed, refused, resolved so far
```

**Say how it works:** claims are a third state machine on the shared log (D370), the same pattern as Part G's presence
directory. A claim's fencing token is the log index it was accepted at (D375), confirming the note on D69. First in log
order wins; the difference between the online and the offline case is not the log rule, it is whether the loser already
acted on a belief the log had not confirmed yet (D376). Measured: `claim_conflict`'s `kind` for the conflict count and
type, `reconciliation_resolved.wait_ms` for how long the item sat in reconciliation (D380).

## Spec scenario 9. Scale and stress with Phase 2 on (Part L)

*Large automated runs: latency, throughput, overhead, recovery and convergence.* Phase 2 runs this row again with every user
living the Phase 2 lifecycle (D430, D434); recovery is measured by the Phase 1 series below. As with scene 7, it takes too long
to run live: run it beforehand, show the results, and run one cell live if there is time. Stop every frontend and all demo users
first: the runner refuses to start while one runs.

**On one machine,** check that every cell loads, and try one, never for results:

```bash
cargo test --release --test experiment_cells
WARMUP=5 MEASURED=20 REPS=1 scripts/run-experiments.sh experiments/hosts.local quick ph2users-100
```

**On the lab cluster.** Nine cells in three sweeps, listed in `experiments/phase2.list`. All have the lifecycle on, with
`think_ms=1000` and `pattern=random`.

| Cells | What changes | What it measures |
|---|---|---|
| `ph2users-100`, `ph2users-1000`, `ph2users-5000` | Users per client PC, given as `users_per_pc` (D446): 100, 1,000 or 5,000 on each client PC in the hosts file | Throughput and latency as users grow, with discovery and P2P on; CPU, memory, network and queues on the nodes and the client PCs; discovery, presence convergence and scenario 6's P2P figures |
| `ph2offline-0`, `ph2offline-10`, `ph2offline-30` | `workload.offline_chance_per_min_percent`, at 1,000 users per PC, with 20 s outages | Offline reconciliation delay; peer updates kept, delivered and dropped |
| `ph2conflict-0ms`, `ph2conflict-500ms`, `ph2conflict-2000ms` | `workload.claim_delay_ms`, at 1,000 users per PC | Conflicts per claim, by kind |

```bash
export REMOTE_DIR=${PWD#"$HOME"/}                                                  # the code's folder under ~, the same on all six
scripts/run-experiments.sh experiments/hosts quick $(cat experiments/phase2.list)   # tuning: 15 s warm-up, 60 s measured, 3 runs
scripts/run-experiments.sh experiments/hosts full $(cat experiments/phase2.list)    # the report: 30 s, 120 s, 5 runs
python3 analysis/phase2_stream.py runs --out analysis/out/phase2                # streams the big logs (D447)
grep -E '^ph2(users|offline|conflict),' analysis/out/phase2/summary.csv
```

Before `ph2users-5000`, check the client PCs' free memory: Phase 1 lost programs to memory at heavy load (D434).

**Before you start, on the lab as it was on 2026-11-26.** Nodes 1 and 3 and client PCs 2 and 3 share the Wi-Fi's 10.40.0.0/21. Node 2
and client PC 1 sit on other Wi-Fi subnets that it cannot reach, so the hosts file names them by their wired 10.7.57.x addresses
(`ip -4 addr` on each; client PC 1 then appears as `local`). Check with a ping both ways before a series. One quick series of the nine
cells wrote about 105 GB of logs to client PC 1, so check `df -h` there first.

**Expect** each run in `runs/<experiment>/<cell>/run-<n>/`: `run.json`, `node-<id>.jsonl`, `client-<id>.jsonl`,
`users-<m>-<b>.out`, and each program's two queues, `outbox-client-<id>.jsonl` and `claims-client-<id>.jsonl`, which are not
event logs. What must hold in every run, as in Phase 1: 0 unaccounted and 0 digest mismatches, with `digest_mismatches` and
`directory_digest_mismatches` at 0. Give-ups are a count to report; 0 is the guide's check only near 300 reports a second.

**How to read it:**

- `ph2users`: `throughput_rps` and `p50_ms`, `p95_ms`, `p99_ms` against users per PC. On the nodes, `cpu_cores_mean`,
  `rss_mb_max`, `net_bytes_per_s_mean` and `queued_max`; on the client PCs, `client_pc_cpu_busy_p95` and
  `client_pc_mem_available_min_mb`. Then `discover_client_p99_ms`, `presence_convergence_p99_ms`, and scenario 6's P2P figures,
  with the cloud's bytes beside the P2P messages (D435).
- `ph2offline`: the cell's number is the chance per minute, so the share of time offline is about that times 20 s ÷ 60 s,
  measured from `client_offline` and `client_online` (D432). Reconciliation delay comes from `outbox_flushed.reconcile_us` and
  `claims_flushed.reconcile_us`, each on one clock. What was kept and dropped comes from `peer_update_held`,
  `peer_update_forwarded` and `peer_update_dropped` by `reason`.
- `ph2conflict`: conflicts per claim, not per second (D442): `claim_conflict` by `kind`, `online_refused` or
  `reconciliation_opened`, over the `claim_write` lines whose `kind` is `claim`. Each node's `claims_state` digest must be equal
  at equal `last_applied`.

**Say how it works:** a cell is a config overlay (D63), so the lifecycle and both sweeps are config keys (D430, D432, D442). The
runner shares `users` over the client PCs, 10 programs each (D181). Peers talk on the client PCs only, so the nodes' bytes are all
the cloud carries (D435).

### Phase 1 still passes (scenarios 1 to 4 and 9)

The plan asks to show that Phase 2 broke nothing (section 8). Run the Phase 1 series again on the final build, with the Phase 1
runner, from client PC 1, and set its tables beside the Phase 1 report's (`docs/phase1-report.md`, every table made by
`analysis/phase1/make_tables.py`). The runner writes its own config, with the lifecycle off, so the load is Phase 1's: open-loop
rates, the same cells and the same victim order. The generic runner's Phase 1 cells are no comparison, because the report's
numbers did not come from them.

```bash
LAB_TEST_AUTO=1 bash scripts/phase1/run-experiments.sh series "1 2 3"   # 3 passes; Phase 1's 5 took 6 h 37 min
```

It writes into `runs/exp8h`, beside Phase 1's logs, and moves any run folder of the same name to `runs/old-hand/`: copy Phase 1's
aside first. To make the tables and compare them, commands: see the experiments guide.

**Expect** what held in Phase 1: 0 unaccounted and 0 digest mismatches in every run, and the five verdicts in section 1 of the
Phase 1 report. Beside each figure, say that the Phase 2 build logs two more lines per committed report, the gossip publish
(D367), and that programs ask the directory for their zones' lists at most every 10 s (D358, D366).

## End to end: every scenario in one session (Part L)

*The whole table on the six PCs in one session, with Phase 2's users running throughout (plan section 8).* Not rehearsed yet:
integration day, 2026-12-02, is its first run.

**Before.** Everything in "Before you start", and:

1. The lifecycle on in `config/lab.toml` on all six, as in spec scenario 5. Loadgen then refuses `--rate`, so this session runs on
   the lifecycle's closed loop. The `--rate` row of "Adjusting the load" and the too-old scene need it off again.
2. The peer ports open on the three client PCs: `sudo ufw allow 20000:20999/udp` (runbook section 1).
3. The cluster up, with terminal 1 showing three `peer` rows and equal digests, and a page open on each client PC (IDs 11, 12, 13).

**Start the load** on each client machine, with its own number: an hour at `ph2users-100`'s load, 10 programs of 10 users.

```bash
bash scripts/start-users.sh --config config/lab.toml --machine 1 --users 100 --pattern random --duration-s 3600
```

**Then, in order.** "PC 1" is client machine 1's page.

1. **Normal operation (scene 1).** Send a report from PC 1, and run the win count on each node machine twice. *Point at* the
   card's responder and planner, and wins on all three nodes.
2. **Load imbalance (scene 2).** `ctl skew --node 2 --work 5000`, and a minute later `--work 1000`. *Point at* node 2's wins
   growing more slowly while it is slow.
3. **Server failure (scene 3).** `ctl silence --node 3 --ms 20000`; any node will do, as none leads (D249). *Point at*
   `silenced = true` on its row, and the next card answered by another node after a stall of about a second.
4. **Server recovery (scene 4).** Wait out the 20 s. *Point at* `serving` going true with the same digest, then
   `grep '"ev":"rejoined"' runs/lab/node-3.jsonl | tail -1` on node machine 3.
5. **Peer discovery (spec scenario 5).** On PC 1, **Who is online**, zone 3, **Ask all three nodes**. *Point at* the three
   columns and `same answer from 3 nodes`. Then `ctl silence --node 2 --ms 8000` and ask again: node 2's column says
   `no answer (silenced or catching up)`, and the verdict says `same answer from 2 nodes`. Once it is back, ask again: 3.
6. **Direct P2P exchange (spec scenario 6).** On PC 1, **Zone updates**, zone 2, **Follow**, then send a zone 2 report from PC 2.
   *Point at* the load's updates with their hops, never above 7, the offers sent straight to this page's user, and PC 2's report
   arriving.
7. **Offline peer (spec scenario 7).** On PC 1, **Go offline**, send a report, wait a few seconds, then **Come back online**.
   *Point at* the yellow QUEUED card with its ID, the `Back: sent 1, answered 1, …` line, and the zone 2 updates its peers held
   for it arriving within about 2 s (D415, D410).
8. **Conflicting claim (spec scenario 8).** On PC 1, **Conflicting claim**, **Stage a conflict**, then **Challenger wins**.
   *Point at* each printed step, the entry's token, and the answer, resolved at log index N.
9. **Scale and stress (spec scenario 9).** Stop the load, `bash scripts/start-users.sh --stop --machine 1` on each client
   machine, and show one program's counts by `action:outcome`, `grep -m1 '^phase2:' runs/users/users-1-0.out`. Then show spec
   scenario 9's results: the same users, measured at 100, 1,000 and 5,000 per PC.

If the page fails, each scenario's own section does the same with `ctl`, `loadgen` and the tests.

## Questions to be ready for

Every answer here comes from the spec, the class notes or the recordings, and nothing is left open (D213).

- **"Is the leader a coordinator?"** No node decides for another. One elected, replaceable node orders the log: it numbers log lines, sends the copies and answers one read. `docs/architecture.md` section 2 has a table, row by row, of who decides what.
  Bidding picks who plans (D114), all three draw who fails (D158), any serving node loads the responders (D164), and you saw the leader go silent and come back as an ordinary node. What remains, and we say it: every result passes through one elected node on its way to becoming final, and arrival
  order at it settles a race. Only replacing Raft would remove that, and we rejected that as a rewrite a day before the deadline (D163). `leader_plan_share` is the number beside the baseline (D166). The reading is ours, and the report states it as a reading.
- **"Does the leader change when it is busy?"** Yes, if it stays overloaded (D218, amending D217). Noor decided this on a teammate's report that Dr Amr asked for it in class. The written notes and Lectures 8 and 9 do not say it, and we say so. There are two elections. The bid picks who plans each report, by load, so a busy node loses bids and work moves away from it. The Raft vote picks who orders the log, and every result passes through that node. So a leader whose load stays at least 3 times its lowest peer's bid for 5 s stops its heartbeats and holds its writes, the followers' timers run out, and a normal vote picks a new leader. Raft's safety is not touched, because all it can cause is an election. The cooldown stops a storm: at most once a minute. Scene 2b shows it.
- **"Is this client-server?"** No. The cloud nodes are peers that update each other's state, detect failures, share out work and tell each
  other their load. Client and server are the roles of one call, not of a node (D212, `docs/architecture.md` section 3.1).
- **"Where is the middleware, and where is the application?"** On both sides, drawn in `docs/architecture.md` section 4.3. Client side: the page and
  the simulated users are the application, and `MeshClient` is the middleware. Cloud side: the incident service, the planner and the state
  rules are the application, and the group, the election, the detector, tarpc and openraft are the middleware (D211). `src/middleware.rs` is the line in the code, and
  `tests/layers.rs` fails if application code reaches past it (D156).
- **"What is your group communication?"** The client middleware's send to all of the cloud nodes is the group send, and the application does not
  know it happens. The cloud's who's-alive list, `CloudGroup`'s view, is the membership, and it changes as nodes fail and return. Between the nodes,
  bids follow the list, pings go to everyone, and Raft orders results. Each list is one node's opinion, and we say so (D152–D155, D157, D210).
- **"Where is publish-subscribe?"** Phase 2, as Lecture 9 says: it needs a publish step and a discovery service. It is in the design, in
  `docs/architecture.md` section 13. We first chose to build a small one, then the lecture settled it (D209). Phase 2 built it on the
  directory: zones are the topics, a program subscribes to the zones its users joined (D359), and a reporter publishes each committed
  answer into its zone for peers to spread (D340, D351). The page's **Zone updates** panel follows a zone (D439, spec scenario 6).
  Reporting does not need it; it exists to tell others in a zone.
- **"Why fencing tokens and log order, not vector clocks or a quorum confirmation, for claims?"** The log already gives every node one
  order everyone agrees on, with no second vote: reusing it costs nothing a quorum confirmation would also cost, since a claim still has
  to pass through the same Raft commit to be answered at all (D375, confirming the note on D69). Vector clocks would tell us two claims
  are concurrent, not which one should win, so we would still need a tie-break rule on top, and that rule may as well be the log we
  already have. The limit: this only orders claims the log actually sees. A claim a client made and acted on entirely offline, before
  it ever reaches a node, is invisible to the log until it is replayed, which is exactly why a speculative claim gets a reconciliation
  entry instead of a flat answer, and why the demo's conflict needs a person, not just the log, to settle it (D376, D378).
- **"Why tarpc?"** See D176 for the comparison with Java RMI, CORBA and Sun RPC/XDR, and D177 for the semantics.
- **"Why UDP, when the notes say don't go this low?"** See D178. Noor's decision was UDP from clients to machines. Call IDs, matching and deadlines stay tarpc's, retries stay in the middleware, and the adapter holds no RPC logic.
- **"What ends the call on a node that lost the bid?"** See D169. The node stays quiet, and the client's cancel ends it. A client that never cancels was the worst variant in the rows.
- **"What did you build, and what is a library?"** See D68 and D215: openraft does Raft, tarpc does remote calls, and the
  bidding, group communication, recovery, degraded mode, planner and clients are ours.
- **"What does the TTL do?"** D143: a report too old on arrival is not planned, the node answers expired and sends nobody, and
  a record with nobody sent reads expired after its deadline. Status is worked out on read, not stored (D142).
- **"Where is the incident priority?"** D148: the planner returns it in every plan, from severity, age, distance, availability
  and capability, in integer points. It is computed, returned and replicated. It does not change who goes, and nothing is
  ordered by it yet.
- **"What keeps the leader up under load?"** D133. Saving to disk and saying "I am alive" are the node's necessities, and processing requests must not
  override them. So our own queue sits in front of Raft: one write in flight per node, and everything waiting goes in one save. A report that waits
  over 3 s leaves the queue, and the client resends.
- **"What happens past what it can answer?"** A plan that loses the race for a responder three times commits that nobody could be sent, and the report hears at once (D220). So overload shows as `no_responder_share` rising at low latency. The trade: an answer now, for a chance of a responder later.

- **"What can a user do offline, and what is lost?"** Only add things: new reports, queued in a file with their IDs, and new peer
  updates, held by the sender. Nothing offline can change someone else's item, so first commit wins is the only conflict rule needed.
  Limits: 10,000 queued reports, and 100 held updates per peer for at most 300 s. All of it is in `docs/offline-policy.md` (D400–D413).

**Do not narrate `members` as health.** Lists differ between nodes, and a silenced node's list is frozen. Point at leader, serving and
degraded as well.
