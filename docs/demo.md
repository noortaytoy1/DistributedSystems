# Demo script

Phase 1 demo on the six machines. It covers the first four rows and the last row of the spec's Required
Demonstration Scenarios table, as Dr Amr asked in class on Oct 28, then his own failure cases. The team should
rehearse it on the lab machines: **none of this has been run there yet.**

## Before you start

Set up the machines with [runbook.md](runbook.md), with the fault schedule off so that only you cause faults.
Delete old logs (`rm -rf data/raft runs/lab` on all six), then start the three nodes.

Keep two terminals visible:

```bash
# Terminal 1, the cluster: role, leader, digest, serving, silenced, degraded for each node
watch -n 0.5 target/release/ctl --config config/lab.toml status

# Terminal 2, on each node machine: who has won bid rounds so far
grep -c '"i_won":true' runs/lab/node-*.jsonl
```

Start the load on all three client machines, each with its own client ID:

```bash
target/release/loadgen --config config/lab.toml --client-id 1 --users 30 --think-ms 1000 --duration-s 600
```

## 1. Normal cloud operation

*Different peers are elected as load conditions change.*

1. Terminal 1 shows one Raft leader and equal digests. Point out that the leader orders the log and does not
   choose who plans (D114).
2. Run the win count on each node machine twice, a few seconds apart.
3. Raise the load: a second load generator with `--client-id 4 --users 300 --think-ms 200`. Count again.

**Expect** the wins to shift between nodes as queues build. At very low load the fastest node can win most
rounds, because the score is expected wait (D45), so show the higher load before drawing conclusions.

## 2. Load imbalance

*Raise the load on one server and show the election respond.*

```bash
target/release/ctl --config config/lab.toml skew --node 1 --work 5000
```

**Expect** node 1's win count to grow more slowly than the others', because its plans take longer and its bids
rise. Then put it back with `skew --node 1 --work 0`.

## 3. Server failure

*The elected server goes nonresponsive under load, and service continues.*

1. Find a node that is winning rounds, from the counts above. For the strongest version pick the one that
   is also the leader.
2. Silence it for the longest fault allowed:

   ```bash
   target/release/ctl --config config/lab.toml silence --node 1 --ms 20000
   ```

**Expect** terminal 1 to show `silenced = true`, and a new leader if it led. The load generators keep going.
On the others, `peer_suspected` and `takeover_started` appear in the logs. At the end `gave up` should be 0.
A client never names a node, which is the access transparency Dr Amr asked about.

## 4. Server recovery

*State is rebuilt before the node takes part again.*

Wait for the 20 s to end, with terminal 1 on screen.

**Expect** `silenced` to go false while `serving` stays false for a moment, then `serving` true with the same
digest as the others, once the node has applied the leader's commit index. Show how long it took:

```bash
grep '"ev":"rejoined"' runs/lab/node-1.jsonl | tail -1
```

Say what happened: the log entries it missed were copied, then replayed through the state machine; its load
figures and bid rounds were rebuilt locally; and it knew it was current by asking the leader for a commit index
confirmed by a majority (D76).

## 5. Dr Amr's case: a machine physically off

1. Under load, **turn off the power of one node machine**. Not a shutdown, and not `ctl`.
2. **Expect** the same as scenario 3. The clients drop the dead connection after about 3 s (D112), and the other
   nodes replace theirs after about 1 s (D119).
3. Turn it back on and start it again, with the same ID and without wiping anything:

   ```bash
   target/release/node --config config/lab.toml --id 1
   grep -E '"ev":"(restarted|rejoined)"' runs/lab/node-1.jsonl | tail -2
   ```

**Expect** `restarted`, then `rejoined`, then equal digests. Its vote and log came back from disk, and the rest
came from the leader (D118).

## 6. Dr Amr's case: one off and one failed, one server for 3 clients

**Tell him first what this trades away** (D120): with one server there is no majority, so the answer cannot be
committed. The last node gives a labelled provisional answer and commits it later, so it can turn out to differ.

1. Under load, turn off one node machine (say node 1).
2. Silence another: `ctl silence --node 2 --ms 20000`.
3. About 2 s later, terminal 1 shows `degraded = true` on node 3.
4. The load generators keep receiving answers. At the end they print a `provisional` count.
5. When node 2 returns, `degraded` goes false, and node 3 replays what it answered:

   ```bash
   grep -E '"ev":"(degraded_entered|degraded_left)"' runs/lab/node-3.jsonl
   grep -c '"ev":"provisional_conflict"' runs/lab/node-3.jsonl
   ```

**Expect** nodes 2 and 3 to reach equal digests. Any conflicts show how many answers did not stand. Then bring
node 1 back as in scenario 5, and all three digests should match.

## 7. Scale and stress

The last row of the table is not a live event, because it takes an hour. Show the results:

```bash
scripts/run-experiments.sh experiments/hosts quick      # beforehand
python analysis/analyze.py runs
ls analysis/out
```

Walk through `e1_latency_throughput.png`, `fairness.png` and `fault_phases.png`, and `summary.csv` for election
overhead and recovery. State the profile, the runs per cell and the limits (D63). If there is time, run one cell
live:

```bash
scripts/run-experiments.sh experiments/hosts quick e1-300-bidding
```

## Questions to be ready for

- **"Is the leader a coordinator?"** See D116. Raft orders the log and runs the fault schedule; it does not decide
  who plans.
- **"Why tarpc?"** See D119 and the chapter 2 comparison the report needs.
- **"Where is publish-subscribe?"** Phase 2, for zones (D131). Group communication is built (D126).
- **"What did you build, and what is a library?"** See D68: openraft does Raft, tarpc does remote calls, and the
  bidding, recovery, degraded mode, planner and clients are ours.
