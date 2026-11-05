# RescueMesh, explained simply

Read this first. It explains what the system does, what we tested, what the numbers mean and what is still wrong, with no background needed. The details are in [phase1-report.md](phase1-report.md) and [shared-log.md](shared-log.md).

## 1. What the system does

People on a campus report emergencies: a flood, a blocked road, a lab incident. Three cloud computers work together to choose a responder for each report, and all three keep the same record of every decision. If one of them fails, the other two carry on, and it catches up when it returns.

```text
   Client PC 1        Client PC 2        Client PC 3        many simulated users
        \                  |                  /
         \_________________|_________________/              each report goes to all 3
          |                |                |
       Node 1 <-------> Node 2 <-------> Node 3             the three cloud nodes
                  they talk to each other
```

There is **no boss** among the nodes. The professor ruled that out, so no node is in charge of the others.

## 2. The journey of one report

From a real report at 10,000 a second:

| Time | What happened |
|---:|---|
| 0 ms | Client 3 sends the report to all three nodes |
| 12 to 25 ms | Each node receives it, and tells the others how busy it is: a **bid** |
| 73 ms | The bidding closes. Node 3 is the least busy, so it does the work |
| 73 to 162 ms | Node 3 chooses a responder, and the decision goes into the **shared log**, so all three nodes record it |
| 162 ms | Node 3 replies |
| 187 ms | The client has its answer |
| About 1 s later | The responder has finished, and node 3 records that it is free again |

The client never knows or cares which node did the work.

## 3. The three ideas to understand

**The election: who does the work?** Every report has its own small election. Each node says how long a new job would wait with it, and the lowest wins. If a node is busy, it bids high and others win. No node runs this election. Each one works out the same winner from the bids it holds.

**The shared log: how do all three stay the same?** Imagine three people, each with a notebook that must stay identical, and nobody in charge.

1. Each person writes their work on numbered cards, and posts a copy to the other two.
2. Everyone files the cards in the same order: lowest number first, and for equal numbers, node 1 before 2 before 3.
3. Nobody files card 10 until everyone has moved past 10, because then no lower card can still arrive.

Same cards, same sorting rule, so the same notebook. Nobody decides anything, so nothing needs a leader.

**Failure: what if a node stops?** Everyone would wait for its cards for ever. So after 1 second of silence, the other two agree to carry on without it. When it comes back, it first reads its own notebook from disk, copies what it missed from another node, and asks to rejoin. Its TCP connections are made again by themselves.

**Two or three nodes stop?** The last node can't file cards alone, so it answers on its own, labelled provisional, and those answers go into the notebook once someone returns. If all three stop, each reads its own notebook when it restarts. The one with the most cards goes first, and the others rejoin it.

## 4. What is a library, and what is our code

- **Libraries carry the cards.** `tarpc` sends them between computers, `bincode` packs them into bytes, and `tokio` runs everything at once.
- **The sorting rule is our code,** in `src/rounds.rs`. We looked for a library that does it with no leader. The closest one, `synevi`, stopped working when a node stopped, and its nodes ended up different. All the mature ones have a leader inside.

## 5. The five experiments, in plain words

The class asked for scenarios 1, 2, 3, 4 and 9 of the spec. Every test ran 5 times, with load from all 3 client PCs.

| # | The test | What we did | What happened |
|---|---|---|---|
| 1 | Normal running | Changed the load during a run: 1,000, 5,000, 9,000, then 1,000 reports a second | All three nodes kept winning, about a third each, and their bids rose with the load |
| 2 | One slow server | Made one node 5 times slower for a minute | Its share of the work fell from 33% to 5%, then returned to 33%. Clients noticed nothing |
| 3 | A server fails | Stopped a node for 20 s during load | Answers paused for about 1 second, then carried on at full speed |
| 4 | It comes back | Let it return | It caught up, and all three nodes matched exactly |
| 3+ | Two or three nodes killed | Killed them in every order, at random times, under load | They recovered by themselves, and all three matched. See the report's section 8a |
| 9 | Stress | Raised the load until it broke | See below |

**Across all of it:** 157 runs and about 199 million reports. Every report is accounted for, and the nodes never disagreed in any of 69,715 checks.

## 6. How much load can it take?

It depends on how much work each report costs. `plan_work` is a setting that burns CPU, to stand in for a hard planning job. The spec wants real computing load, so **the first line is the honest headline.**

| Work per report | Held in all 5 runs | Then | What runs out |
|---|---:|---|---|
| About 4 ms of planning | **12,000 a second** | Falls at 14,000 | CPU |
| Almost none | **30,000 a second** on a quiet network | Answers stop rising at about 34,000. Tested to 36,000 | Most likely the shared Wi-Fi |

At midday, on a busy network, 15,000 a second still held fully, but 30,000 did not, on the final build or on last night's. So 30,000 needs a stable network. A wired LAN should go faster, but we did not test it.

## 7. Is something wrong at 30,000?

Mostly no, with one exception.

| Situation at 30,000 a second | Result |
|---|---|
| Nothing fails | **Fine.** 5 of 5 runs, slowest 1% under about 1 s |
| Nothing fails, for 5 minutes | **Fine,** slowest 1% at 562 ms |
| A node fails for 20 s | **At the edge.** In 3 of 5 runs nobody gave up, but in the other 2 some did: 12,233 and 70, of about 4.5 million. It took about 73 s to fully recover |

This is not a bug. At 30,000 there is little spare capacity, so a node that has fallen behind struggles to catch up. At 20,000 and below, a 20 s failure caused no give-ups at all.

## 8. How long can it keep running?

**Not for hours, at high rates.** Memory grows with every report, about 590 bytes on each node, and nothing trims it. The longest run at 20,000 a second lasted about 18 minutes, until a memory guard stopped it: 21.2 million answered, 0 gave up, and the slowest 1% under 1 s. By arithmetic, at 1,000 a second a node would last about 6.5 hours.

## 9. Words you will see

| Word | Meaning |
|---|---|
| Offered | How many reports a second we sent |
| Answered | How many a second came back |
| p50 | Half of the answers were faster than this |
| p99 | 99% of the answers were faster than this |
| Retry | The client heard nothing for 2 s, so it asked again. This is normal, and answers still come |
| Gave up | The client tried 6 times, then stopped. This is the real failure |
| Digest | A fingerprint of a node's whole record. Equal digests mean identical records |
| Mismatch | Two nodes had different fingerprints. This is the worst thing that can happen. We had 0 |
| Held | At least 99% answered, nobody gave up, and p99 under 3 s |
| ± | How sure we are: the 95% confidence interval across the 5 runs |

## 10. Questions the professor may ask

**Where is the leader?** There is none. The order comes from a sorting rule that every node applies by itself.

**So who coordinates when a node fails?** Nobody permanently. Whichever node notices first asks the other, and each node can promise only one change at a time, so they cannot disagree.

**Which library does the shared log?** None does the ordering. `tarpc` carries the log, and the rule is ours. We tried `synevi`, the closest with no leader, and it split when a node stopped (D259).

**How do you avoid doing a report twice?** Every report has a unique ID. If two nodes both plan it, the first to reach the log wins, and the other's work is thrown away. A retry gets the same answer.

**What if two nodes both think they won?** Both do the work, and the first one counts. It costs a little time and is never wrong.

**What if nobody thinks they won?** A node that lost plans it anyway after at least 1.5 s, and the client asks again at 2 s.

**How does a returning node know it is up to date?** The others stop at an agreed point and wait for it. When it has the whole log up to that point, it is current, and all carry on together.

**How do you know the nodes really agree?** Every 100 log entries, each node records a fingerprint of its whole state. We compared them: 0 differences in 69,715 checks.

**Are these numbers real?** Yes. Each answer in three checked runs, 3,749,625 of them, was matched to the reply line in the answering node's own log.

**What are the limits?** Section 8, and a slow patch on a busy network can feed itself, as retries and takeovers add work. Provisional answers can also change later. See phase1-report.md, section 11.

**Why two commands per report?** One says who was sent, and one says that responder is free again. Both go through the log, so all nodes agree who is busy. We still count one request.

## 11. Where everything is

| What | Where |
|---|---|
| The full report | [phase1-report.md](phase1-report.md) |
| The shared log, in full detail | [shared-log.md](shared-log.md) |
| Every design decision, with reasons | [design-log.md](design-log.md) |
| Results and charts | `analysis/results/exp8h/` |
| One report followed through every computer's log | `analysis/results/request-trace-example.txt` |
| The experiment scripts | `scripts/phase1/` |
| All raw logs | `runs/exp8h/` and `runs/exp8h-full/`, on client PC 1 |
