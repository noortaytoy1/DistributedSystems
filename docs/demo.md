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
  `docs/architecture.md` section 13. We first chose to build a small one, then the lecture settled it (D209). So the page has no zone panel.
  Reporting does not need it; it exists to tell others in a zone.
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

**Do not narrate `members` as health.** Lists differ between nodes, and a silenced node's list is frozen. Point at leader, serving and
degraded as well.
