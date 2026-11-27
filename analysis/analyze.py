#!/usr/bin/env python3
"""Turn collected runs into the report's numbers and plots.

    python3 analysis/analyze.py runs [--out analysis/out]

Reads runs/<experiment>/<cell>/<run>/ as scripts/run-experiments.sh writes them. Each metric is
computed per run, then averaged across the cell's runs with a 95% Student's t confidence interval.
Writes summary.csv (long format, one row per cell and metric) and PNG plots to the output directory.

Durations come from `latency_us`-style fields, measured on one machine. FT-2 reads failover_gap_ms_* and
client_gap_pc_*, which are on one clock (D233). failover_to_*, before_rps, during_rps, after_rps and
arrival_peak_100ms_share_first_s subtract timestamps from different machines, so they are only as good as those
clocks (AGENTS.md, Measuring). The phases' edges, the other arrival_peak_*, elections_after_rejoin and every filter
on the measured time compare them, to choose lines. node_clock_skew_ms and client_clock_skew_ms say how far apart
the clocks were in that run. Phase 2's gossip_age_* (two client PCs) and reconciliation_wait_* (a client PC and a
node) read two machines' clocks too.
"""

import argparse
import bisect
import csv
import json
import math
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np
from scipy import stats

USER_HZ = 100  # /proc CPU ticks per second on Ubuntu
UDP_RCVBUF_ASKED_BYTES = 8 * 1024 * 1024  # node/datagram.rs: what every UDP socket asks for (D171)
ELECTION_AFTER_REJOIN_US = 10_000_000  # FT-6: elections this soon after a rejoin count against it
LOSS_GRACE_US = 1_000_000  # a term that moves later than this after a quiet spell was not lost to it
LOG_END_GRACE_US = 2_000_000  # a leader lost this close to the end of the logs had no time to be replaced
PERCENTILES = (50, 95, 99)
NODE_CLOCK_SKEW_WARN_MS = 10  # failover_to_* moves by exactly this
CLIENT_CLOCK_SKEW_WARN_MS = 50  # half the width of arrival_peak_*, and one priority point (D148)
# FT-2 holds for every failover, and a mean across runs can hide one. A maximum is its own question (D233).
WORST_RUN_METRICS = ("failover_gap_ms_max", "client_gap_pc_max_ms", "client_gap_max_ms", "worst_request_ms",
                     "reconcile_max_ms")
# Part K's outboxes sit in the metrics directory beside the logs: outbox-client-<id>.jsonl and
# claims-client-<id>.jsonl, one queued report or claim a line, with no "ev" (D401, D414). Not event logs.
NOT_EVENT_LOGS = ("outbox-", "claims-")


def read_events(run_dir):
    """Every event in a run, by source name, each source's events in file order. A program killed in the
    middle of a write leaves half a line at the end of its log: that one is dropped and named. A bad line
    anywhere else is real damage, and raises. Part K's outbox files are not logs, and are not read."""
    events = {}
    for path in sorted(Path(run_dir).glob("*.jsonl")):
        if path.stem.startswith(NOT_EVENT_LOGS):
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        lines = [line for line in text.split("\n") if line.strip()]
        try:
            json.loads(lines[-1] if lines else "null")
        except json.JSONDecodeError:
            print(f"{path}: dropped a half-written last line")
            lines.pop()
        events[path.stem] = [json.loads(line) for line in lines]
    return events


def jain(counts):
    """Jain's fairness index: 1 is perfectly even, 1/n is one node doing everything."""
    total = sum(counts)
    squares = sum(c * c for c in counts)
    if not counts or squares == 0:
        return float("nan")
    return total * total / (len(counts) * squares)


def nearest_rank(sorted_values, percent):
    """The same percentile definition as bin/loadgen.rs."""
    if not sorted_values:
        return float("nan")
    rank = max(math.ceil(len(sorted_values) * percent / 100), 1)
    return sorted_values[rank - 1]


def latency_metrics(prefix, latencies_us):
    values = sorted(latencies_us)
    metrics = {f"{prefix}count": len(values)}
    metrics[f"{prefix}mean_ms"] = float(np.mean(values)) / 1000 if values else float("nan")
    for p in PERCENTILES:
        metrics[f"{prefix}p{p}_ms"] = nearest_rank(values, p) / 1000
    return metrics


def program_start(source_events):
    """When a program's clock started, which is what its warm-up counts from, or None if it never sent
    anything. Its first send can come much later: up to two think times under `random`, which for a program
    with one user would quietly shorten the measured time (D182). A log from before `client_started` has
    only its first send to go on."""
    sent = [e["ts_us"] for e in source_events if e["ev"] == "client_sent"]
    if not sent:
        return None
    started = [e["ts_us"] for e in source_events if e["ev"] == "client_started"]
    return min(started or sent)


def client_requests(events, warmup_s):
    """Each client's measured `client_done` and `client_gave_up` events: started after warm-up,
    judged on that client's own clock."""
    done, gave_up = [], []
    for source, source_events in events.items():
        if not source.startswith("client-"):
            continue
        started = program_start(source_events)
        if started is None:
            continue
        measure_from = started + warmup_s * 1_000_000
        # A request gives up about 30 s after it starts, so one started in the warm-up gives up well inside
        # the measured time. Its first send says when it started, as loadgen's own count has it.
        first_sent = {
            e["request"]["seq"]: e["ts_us"]
            for e in source_events
            if e["ev"] == "client_sent" and e["attempt"] == 0
        }
        for e in source_events:
            if e["ev"] == "client_done" and e["ts_us"] - e["latency_us"] >= measure_from:
                done.append(e)
            elif e["ev"] == "client_gave_up":
                if first_sent.get(e["request"]["seq"], e["ts_us"]) >= measure_from:
                    gave_up.append(e)
    return done, gave_up


def node_events(events):
    return {s: es for s, es in events.items() if s.startswith("node-")}


def measured_start(events, warmup_s):
    """Roughly when measuring began, for node events: machines agree only as far as NTP does."""
    starts = [program_start(es) for s, es in events.items() if s.startswith("client-")]
    starts = [start for start in starts if start is not None]
    return min(starts) + warmup_s * 1_000_000 if starts else 0


def worst_request_ms(events, done, gave_up):
    """The longest any one measured request waited: an answered one's latency, or how long one that gave up
    kept trying. client_gap_* cannot say this, because it is per program and a program holds many users, so
    its other users' answers cut the gap short (D229)."""
    waits = [e["latency_us"] for e in done]
    if gave_up:
        first_sent = {
            (e["request"]["client"], e["request"]["seq"]): e["ts_us"]
            for es in client_sources(events).values()
            for e in es
            if e["ev"] == "client_sent" and e["attempt"] == 0
        }
        for e in gave_up:
            started = first_sent.get((e["request"]["client"], e["request"]["seq"]))
            if started is not None:
                waits.append(e["ts_us"] - started)
    return max(waits) / 1000 if waits else float("nan")


def request_metrics(events, warmup_s, measured_s):
    done, gave_up = client_requests(events, warmup_s)
    committed = [e for e in done if not e.get("provisional")]
    provisional = [e for e in done if e.get("provisional")]
    metrics = latency_metrics("", [e["latency_us"] for e in committed])
    metrics.update(latency_metrics("provisional_", [e["latency_us"] for e in provisional]))
    metrics["throughput_rps"] = len(done) / measured_s if measured_s else float("nan")
    metrics["gave_up"] = len(gave_up)
    answered = len(done)
    retried = sum(1 for e in done if e["attempts"] > 1)
    no_responder = sum(1 for e in done if e.get("no_responder"))
    # An answer that sent nobody counts in throughput_rps and is about as fast as a real one, so a cell
    # that ran out of responders still reads as busy (D229). A provisional answer names a responder, but if
    # that one was taken the record reads nobody, and the client is not told (D122).
    taken = sum(
        1
        for es in node_events(events).values()
        for e in es
        if e["ev"] == "provisional_conflict" and e["reason"] == "responder_taken"
    )
    metrics["answered_with_responder_rps"] = (
        max(answered - no_responder - taken, 0) / measured_s if measured_s else float("nan")
    )
    metrics["worst_request_ms"] = worst_request_ms(events, done, gave_up)
    metrics["retried_share"] = retried / answered if answered else float("nan")
    metrics["no_responder_share"] = no_responder / answered if answered else float("nan")
    # An expired report also counts in no_responder_share. See expired_warnings.
    metrics["reports_expired"] = sum(1 for e in done if e.get("status") == "expired")
    priorities = [e["priority"] for e in done if "priority" in e]
    metrics["priority_mean"] = float(np.mean(priorities)) if priorities else float("nan")

    nodes = sorted(int(s.split("-")[1]) for s in node_events(events))
    by_node = defaultdict(int)
    for e in committed:
        by_node[e["executor"]] += 1
    metrics["fairness_jain"] = jain([by_node[n] for n in nodes])
    for n in nodes:
        metrics[f"share_node_{n}"] = by_node[n] / len(committed) if committed else float("nan")
    return metrics


def bid_metrics(events, start_us):
    rounds = defaultdict(int)
    windows = []
    for es in node_events(events).values():
        for e in es:
            if e["ev"] == "bid_round_closed" and e["ts_us"] >= start_us:
                key = (e["request"]["client"], e["request"]["seq"], e["attempt"])
                rounds[key] += 1 if e["i_won"] else 0
                windows.append(e["window_us"])
    return {
        "bid_rounds": len(rounds),
        "bid_window_mean_ms": float(np.mean(windows)) / 1000 if windows else float("nan"),
        "bid_split_decisions": sum(1 for wins in rounds.values() if wins > 1),
    }


def silent_spans(events):
    spans = []
    start = None
    for e in events:
        if e["ev"] == "silenced":
            start = e["ts_us"]
        elif e["ev"] == "rejoined" and start is not None:
            spans.append((start, e["ts_us"]))
            start = None
    if start is not None:
        spans.append((start, math.inf))
    return spans


def election_metrics(events, start_us):
    """Raft elections: new terms seen, and how long each one's vote took, leaving out the times the node
    was silenced or catching up itself.

    election_duration_mean_ms is only the vote, not how long the cloud had no leader, whatever its name
    says. A node logs no missing leader until it stands for election, so the wait before that is not in it:
    about 10 ms of a failover near 700 ms. failover_metrics has the wait (D229).

    An election is a term above the one already running when the window opened. That term is the
    highest logged before the window; it logs nothing inside the window unless something changes,
    so counting the terms seen and taking one off comes out one too low. A log with nothing before
    the window treats its first term as the running one. Node clocks agree only as far as NTP does,
    so a term starting right at the edge can land on either side."""
    terms = set()
    running = 0
    durations_us = []
    for es in node_events(events).values():
        spans = silent_spans(es)
        lost_at = None
        for e in es:
            if e["ev"] != "raft_state":
                continue
            if e["ts_us"] < start_us:
                running = max(running, e["term"])
                continue
            terms.add(e["term"])
            if any(a <= e["ts_us"] <= b for a, b in spans):
                lost_at = None
            elif e["leader"] is None and lost_at is None:
                lost_at = e["ts_us"]
            elif e["leader"] is not None and lost_at is not None:
                durations_us.append(e["ts_us"] - lost_at)
                lost_at = None
    running = running or min(terms, default=0)
    return {
        "elections": sum(1 for term in terms if term > running),
        "election_duration_mean_ms": float(np.mean(durations_us)) / 1000
        if durations_us
        else float("nan"),
    }


def quiet_spells(es):
    """When this node went quiet while it led: {term: [[from_us, to_us], ...]}, in time order, with spells
    that overlap joined. A silence lasts its `ms`. A step-aside lasts until its end line, or for ever if
    the log has none. One that timed out with the term unchanged drops out: that leader held its writes
    for up to 3 s and carried on, so no leader was lost."""
    watched = ("raft_state", "silenced", "leader_stepped_aside", "leader_step_aside_ended")
    spells = defaultdict(list)
    asides = {}
    led = None
    for e in sorted((e for e in es if e["ev"] in watched), key=lambda e: e["ts_us"]):
        if e["ev"] == "raft_state":
            led = e["term"] if e["role"] == "leader" else None
        elif e["ev"] == "silenced":
            if led is not None:
                spells[led].append([e["ts_us"], e["ts_us"] + e["ms"] * 1000])
        elif e["ev"] == "leader_stepped_aside":
            asides[e["term"]] = [e["ts_us"], math.inf]
            spells[e["term"]].append(asides[e["term"]])
        elif e["term"] in asides:
            aside = asides.pop(e["term"])
            aside[1] = e["ts_us"]
            if e["outcome"] == "timed_out" and e["new_term"] == e["term"]:
                spells[e["term"]].remove(aside)
    joined = {}
    for term, found in spells.items():
        joined[term] = []
        for begin, end in sorted(found):
            if joined[term] and begin <= joined[term][-1][1]:
                joined[term][-1][1] = max(joined[term][-1][1], end)
            else:
                joined[term].append([begin, end])
    return joined


def failover_metrics(events, start_us, measured_s=None):
    """How long the cloud had no leader after losing one (FT-2, D229).

    A leader can go quiet many times in one term, and most short silences cost it nothing. So a term counts
    as lost only if some node logs a higher term before that quiet ended, or up to LOSS_GRACE_US after, and
    the loss starts at the last quiet spell that began before the next leader's first line. A later election
    is another story. The leader's own `silenced` or `leader_stepped_aside` line must fall in the measured
    time. failover_to_leader_ms_mean runs to the first `raft_state` line of a leader in the next term that
    has one, and failover_to_commit_ms_mean to the first result committed after that line, when clients can
    have answers again.

    A loss with no later leader in the log adds to failovers_without_leader and to neither mean, and
    failover_warnings names the cell, so that the worst case cannot read as a good average without anyone
    being told. One within LOG_END_GRACE_US of the logs' end is not judged, as nobody had time to take over.
    A loss with a leader and no commit after it adds to failovers_without_commit and leaves only the second
    mean, so then the two means cover different failovers.

    These subtract `ts_us` written by different machines, so each is off by exactly the difference between
    those two clocks, the same way in every run with the same two PCs, where no confidence interval shows
    it. A new leader's clock behind by more than the failover reads as failovers_without_leader. Never
    judge FT-2 from these: failover_gap_metrics and client_pc_gap_metrics are on one clock (D233)."""
    logs = node_events(events)
    states = [e for es in logs.values() for e in es if e["ev"] == "raft_state"]
    # By term, so that a clock behind the others cannot pair a loss with an election long after it.
    next_leaders = sorted((e["term"], e["ts_us"]) for e in states if e["role"] == "leader")
    commits = sorted(
        e["ts_us"]
        for es in logs.values()
        for e in es
        if e["ev"] == "commit" and e["kind"] == "commit_result" and e["outcome"] == "committed"
    )
    measured_to = start_us + measured_s * 1_000_000 if measured_s and start_us else math.inf
    logs_end = max((e["ts_us"] for es in logs.values() for e in es), default=0)
    to_leader, to_commit = [], []
    failovers = without_leader = without_commit = 0
    for es in logs.values():
        restored = [e["ts_us"] for e in es if e["ev"] == "restored"]
        for term, spells in quiet_spells(es).items():
            if not spells:
                continue
            new_leader = next((ts for t, ts in next_leaders if t > term), None)
            began = [spell for spell in spells if new_leader is None or spell[0] <= new_leader]
            at, until = began[-1] if began else spells[0]
            if at < start_us or at > measured_to:
                continue
            moved = min((e["ts_us"] for e in states if e["term"] > term), default=None)
            cost_the_term = moved is not None and moved <= until + LOSS_GRACE_US
            if began and new_leader is not None:
                if cost_the_term:
                    failovers += 1
                    to_leader.append(new_leader - at)
                    first_commit = bisect.bisect_left(commits, new_leader)
                    if first_commit < len(commits):
                        to_commit.append(commits[first_commit] - at)
                    else:
                        without_commit += 1
            elif at + LOG_END_GRACE_US < logs_end and (cost_the_term or not any(at < r for r in restored)):
                failovers += 1
                without_leader += 1
    return {
        "failovers": failovers,
        "failovers_without_leader": without_leader,
        "failovers_without_commit": without_commit,
        "failover_to_leader_ms_mean": float(np.mean(to_leader)) / 1000 if to_leader else float("nan"),
        "failover_to_commit_ms_mean": float(np.mean(to_commit)) / 1000 if to_commit else float("nan"),
    }


def failover_gap_metrics(events, start_us, measured_s=None):
    """FT-2 on one clock: how long a node that kept following heard from no leader, around each change of
    leader (D233). `max_leader_gap_us` is read with `Instant` on that machine, from the old leader's last
    AppendEntries to the new leader's first, so no clock offset can move the reading.

    The Raft term says which change a line belongs to, and time is only compared within one log, apart from
    the measured time's edges. The node that wins stops counting when it leads, and one coming back from a
    silence waited for its own reasons, so both are left out. It reads long by up to one heartbeat tick, 75 ms,
    plus the first message's way, and short by at most one network hop, so a reading of 1000 or less shows a
    new leader within 1 s. It needs no `silenced` line, so a power cut has it too. A returning old leader's
    message ends the gap, so in `degraded-*`, or after a silence shorter than a failover, it reads only the
    last stretch."""
    measured_to = start_us + measured_s * 1_000_000 if measured_s and start_us else math.inf
    gaps = []
    for es in node_events(events).values():
        spans = silent_spans(es)
        reports = [e for e in es if e["ev"] == "raft_io"]
        report_times = [e["ts_us"] for e in reports]
        followed = None
        for e in es:
            if e["ev"] != "raft_state" or e["leader"] is None:
                continue
            if e["role"] != "follower":
                followed = None
                continue
            at = e["ts_us"]
            changed = followed is not None and e["term"] > followed
            if changed and start_us <= at <= measured_to and not any(a <= at <= b for a, b in spans):
                # The gap is in the first line after the new leader's first message, which came just
                # before this one, so it is in one of the two lines around here.
                i = bisect.bisect_left(report_times, at)
                near = [e.get("max_leader_gap_us", 0) for e in reports[max(i - 1, 0):i + 1]]
                if near:
                    gaps.append(max(near))
            followed = e["term"]
    return {
        "failover_gap_ms_mean": float(np.mean(gaps)) / 1000 if gaps else float("nan"),
        "failover_gap_ms_max": max(gaps) / 1000 if gaps else float("nan"),
    }


def clock_skew_metrics(events):
    """How far apart the PCs' clocks were in this run, from the traffic and with no NTP (D56, D233). A
    client PC sends each report to all three nodes at once. Over thousands of copies a node's quickest arrival
    is its clock minus the client PC's, plus one network hop. So two nodes' quickest arrivals differ by the
    difference between their clocks, to about the difference between two hops: failover_to_* can be off by
    node_clock_skew_ms. client_clock_skew_ms is the largest of the quickest arrivals, either way, which is what
    the phases' edges and the windows compare, and it reads high by up to one hop."""
    sent = {}
    for source, es in client_sources(events).items():
        machine = source[len("client-")]
        for e in es:
            if e["ev"] == "client_sent":
                # A report cut off by an outage goes again from the outbox under its own ID and attempt
                # (D404). The first send is the one to count from: a copy of it that got through before the
                # cut would read as arriving before its later send, by the outage's length.
                key = (e["request"]["client"], e["request"]["seq"], e["attempt"])
                sent.setdefault(key, (machine, e["ts_us"]))
    quickest = defaultdict(dict)
    for source, es in node_events(events).items():
        for e in es:
            if e["ev"] == "request_received":
                found = sent.get((e["request"]["client"], e["request"]["seq"], e["attempt"]))
                if found is not None:
                    by_node = quickest[found[0]]
                    by_node[source] = min(by_node.get(source, math.inf), e["ts_us"] - found[1])
    if not quickest:
        return {}
    return {
        "node_clock_skew_ms": max(max(n.values()) - min(n.values()) for n in quickest.values()) / 1000,
        "client_clock_skew_ms": max(abs(v) for n in quickest.values() for v in n.values()) / 1000,
    }


def leader_plan_share(events, start_us, metrics):
    """Share of plans made by the node leading, or NaN unless one node led all of the measured time
    (D166). A term with no leader counts as a change, and so does a node still reading an old leader.

    Each node's last `raft_state` before the start says who it followed going in, and any inside say
    who it followed after. `client_done` already names the winning executor, so the share needs no
    attribution of plans to a role over time, and no correction for duplicate commits.
    """
    leaders = set()
    for es in node_events(events).values():
        states = [e for e in es if e["ev"] == "raft_state"]
        before = [e for e in states if e["ts_us"] < start_us]
        leaders.update(e["leader"] for e in before[-1:])
        leaders.update(e["leader"] for e in states if e["ts_us"] >= start_us)
    if len(leaders) != 1 or None in leaders:
        return float("nan")
    return metrics.get(f"share_node_{leaders.pop()}", float("nan"))


def elections_after_rejoin(events):
    """FT-6: new Raft terms within 10 s after each `rejoined`. A returning node with a stale term
    should cause none (D5, D127)."""
    raft = sorted(
        (e["ts_us"], e["term"])
        for es in node_events(events).values()
        for e in es
        if e["ev"] == "raft_state"
    )
    total = 0
    for es in node_events(events).values():
        for e in es:
            if e["ev"] != "rejoined":
                continue
            at = e["ts_us"]
            before = max((term for ts, term in raft if ts <= at), default=0)
            later = {t for ts, t in raft if at < ts <= at + ELECTION_AFTER_REJOIN_US and t > before}
            total += len(later)
    return total


def client_gap_metrics(events, fault_start, fault_end):
    """E3's client-visible gap: for each client, the longest time between two answers, on its own
    clock, that overlaps the fault. This is what failover looks like from outside. Only choosing
    which gaps overlap compares clocks, so the size of a gap is as exact as its client's clock.

    The client's first and last events close the two ends. Without them a client that never hears
    again after the fault has no gap at all, and the worst run of a cell drops out of its mean.

    This is per program, and a program holds many users, so one user's answer ends the gap for all of
    them. It understates what the unluckiest user saw: worst_request_ms says that (D229)."""
    longest = []
    for source, es in events.items():
        if not source.startswith("client-") or not es:
            continue
        ends = [e["ts_us"] for e in es]
        answered = sorted([e["ts_us"] for e in es if e["ev"] == "client_done"] + [min(ends), max(ends)])
        gaps = [b - a for a, b in zip(answered, answered[1:]) if a < fault_end and b > fault_start]
        if gaps:
            longest.append(max(gaps))
    return {
        "client_gap_mean_ms": float(np.mean(longest)) / 1000 if longest else float("nan"),
        "client_gap_max_ms": max(longest) / 1000 if longest else float("nan"),
    }


def client_pc_gap_metrics(events, warmup_s, measured_s):
    """FT-2's first new answer on one clock: the longest a client PC went with no answer to any program it
    runs, in the measured time (D233). Its programs share its clock, and the first digit of a client ID is
    the machine (D184), so nothing here compares two machines, not even to choose a gap. It reads long by
    about one normal wait between answers, which is 10 ms at 100 reports a second, and a PC that never
    hears again counts its whole silence. A power cut and a step-aside have it too."""
    answers, starts = defaultdict(list), defaultdict(list)
    for source, es in client_sources(events).items():
        started = program_start(es)
        if started is not None:
            machine = source[len("client-")]
            starts[machine].append(started)
            answers[machine] += [e["ts_us"] for e in es if e["ev"] == "client_done"]
    longest = []
    for machine, times in answers.items():
        measure_from = min(starts[machine]) + warmup_s * 1_000_000
        measure_to = measure_from + measured_s * 1_000_000
        inside = sorted(t for t in times if measure_from <= t <= measure_to)
        edges = [measure_from] + inside + [measure_to]
        longest.append(max(b - a for a, b in zip(edges, edges[1:])))
    return {
        "client_gap_pc_mean_ms": float(np.mean(longest)) / 1000 if longest else float("nan"),
        "client_gap_pc_max_ms": max(longest) / 1000 if longest else float("nan"),
    }


def fault_metrics(events, measured_requests):
    logs = node_events(events)
    def values(name, field):
        return [e[field] for es in logs.values() for e in es if e["ev"] == name]

    def mean_or_nan(xs, scale=1.0):
        return float(np.mean(xs)) / scale if xs else float("nan")

    metrics = {
        "detection_mean_ms": mean_or_nan(values("peer_suspected", "silent_ms")),
        "takeovers": len(values("takeover_started", "waited_us")),
        "recovery_mean_ms": mean_or_nan(values("rejoined", "recovery_us"), 1000),
        "rejoins": len(values("rejoined", "recovery_us")),
        "elections_after_rejoin": elections_after_rejoin(events),
        "degraded_entered": len(values("degraded_entered", "held_responders")),
        "degraded_mean_ms": mean_or_nan(values("degraded_left", "degraded_ms")),
        "provisional_replied": len(values("provisional_replied", "latency_us")),
    }
    for reason in ("lost_first_commit", "responder_taken"):
        metrics[f"conflicts_{reason}"] = sum(1 for r in values("provisional_conflict", "reason") if r == reason)
    # Named to sort beside provisional_count and provisional_replied in summary.csv: each is an answer that
    # did not stand on record, and the client is not told (D122, D229).
    metrics["provisional_conflicts"] = len(values("provisional_conflict", "reason"))

    # Split by reason and kept away from the failover figures: a `backstop` takeover fires when a
    # busy winner runs out of time, with no fault anywhere, and under overload that is nearly all
    # of them. Only `suspected` has anything to do with a node that went away, and even that is
    # one request's wait, not how long clients saw nothing.
    takeovers = [e for es in logs.values() for e in es if e["ev"] == "takeover_started"]
    for reason in ("suspected", "backstop"):
        waits = [e["waited_us"] for e in takeovers if e["reason"] == reason]
        metrics[f"takeovers_{reason}"] = len(waits)
        metrics[f"takeover_wait_{reason}_mean_ms"] = mean_or_nan(waits, 1000)

    silenced = [e["ts_us"] for es in logs.values() for e in es if e["ev"] == "silenced"]
    restored = [e["ts_us"] for es in logs.values() for e in es if e["ev"] == "restored"]
    if not silenced or not restored:
        return metrics
    fault_start, fault_end = min(silenced), max(restored)
    metrics.update(client_gap_metrics(events, fault_start, fault_end))
    # Provisional answers as well as committed ones, unlike the main latency figures. In a degraded cell
    # during_* is mostly uncommitted answers, so it reads far better than before_*: see provisional_notes.
    sent_at = [e["ts_us"] - e["latency_us"] for e in measured_requests]
    if not sent_at:
        return metrics
    edges = {
        "before": (min(sent_at), fault_start),
        "during": (fault_start, fault_end),
        "after": (fault_end, max(sent_at)),
    }
    for phase, (lo, hi) in edges.items():
        inside = [e["latency_us"] for e, at in zip(measured_requests, sent_at) if lo <= at < hi]
        metrics.update(latency_metrics(f"{phase}_", inside))
        seconds = (hi - lo) / 1_000_000
        metrics[f"{phase}_rps"] = len(inside) / seconds if seconds > 0 else float("nan")
    return metrics


def refusal_metrics(events, start_us):
    """Planning jobs a full worker pool refused (D73, D138). The client hears nothing and retries,
    so nothing else tells shed work from slow work."""
    metrics = {"plans_refused": 0}
    for source, es in node_events(events).items():
        refused = sum(1 for e in es if e["ev"] == "overloaded" and e["ts_us"] >= start_us)
        metrics["plans_refused"] += refused
        metrics[f"plans_refused_node_{source.split('-')[1]}"] = refused
    return metrics


def convergence_metrics(events):
    """Digests at fixed log indexes (D138). A digest chains over the whole log before it, so one
    equal digest shows that everything before it was equal too. A rejoin is confirmed by the first
    checkpoint its node logs after `rejoined`, if another node logged that index with the same
    digest. Over the whole run, warm-up included: state from any time counts."""
    logs = node_events(events)
    digests = defaultdict(set)
    nodes = defaultdict(set)
    for source, es in logs.items():
        for e in es:
            if e["ev"] == "state_checkpoint":
                digests[e["log_index"]].add(e["digest"])
                nodes[e["log_index"]].add(source)
    confirmed = 0
    for source, es in logs.items():
        for at, e in enumerate(es):
            if e["ev"] != "rejoined":
                continue
            index = next((c["log_index"] for c in es[at + 1:] if c["ev"] == "state_checkpoint"), None)
            if index is not None and nodes[index] - {source} and len(digests[index]) == 1:
                confirmed += 1
    return {
        "digest_checkpoints_compared": sum(1 for seen_by in nodes.values() if len(seen_by) > 1),
        "digest_mismatches": sum(1 for found in digests.values() if len(found) > 1),
        "rejoins_digest_confirmed": confirmed,
        "rejoin_targets_lowered": sum(
            1 for es in logs.values() for e in es if e["ev"] == "rejoin_target_lowered"
        ),
    }


PRESENCE_GROUP_US = 1_000_000  # directory_seen events of one write arrive within this of each other (D293)


def presence_metrics(events, start_us):
    """Part G's directory (D292 to D295). Discovery latency on the client (`discovered`, D334) and on the
    node (`directory_query`); write latency at the proposer (`directory_write`); presence-table convergence:
    each node's wait from a register or deregister arriving to it showing in its own directory
    (`directory_seen`), on that node's clock, and per write the slowest of the nodes. A write's events are
    grouped by user and kind, the first within PRESENCE_GROUP_US. Convergence compares no two clocks; only
    the grouping reads timestamps from different machines, at a one-second scale."""
    nodes = node_events(events)
    measured = lambda e: e["ts_us"] >= start_us  # noqa: E731
    client_discover = [
        e["latency_us"]
        for s, es in events.items()
        if s.startswith("client-")
        for e in es
        if e["ev"] == "discovered" and measured(e)
    ]
    node_query = [e["latency_us"] for es in nodes.values() for e in es if e["ev"] == "directory_query" and measured(e)]
    writes = [e for es in nodes.values() for e in es if e["ev"] == "directory_write" and measured(e)]
    seen = sorted(
        (e for es in nodes.values() for e in es if e["ev"] == "directory_seen" and measured(e)),
        key=lambda e: e["ts_us"],
    )
    metrics = latency_metrics("discover_client_", client_discover)
    metrics.update(latency_metrics("discover_node_", node_query))
    for kind in ("register", "renew", "deregister"):
        metrics.update(latency_metrics(f"presence_{kind}_", [e["latency_us"] for e in writes if e["kind"] == kind]))
    metrics.update(latency_metrics("presence_seen_", [e["wait_us"] for e in seen]))
    groups = []
    open_groups = {}
    for e in seen:
        key = (e["user"]["client"], e["user"]["user"], e["kind"])
        group = open_groups.get(key)
        if group is None or e["ts_us"] - group["first"] > PRESENCE_GROUP_US:
            group = {"first": e["ts_us"], "waits": []}
            open_groups[key] = group
            groups.append(group)
        group["waits"].append(e["wait_us"])
    full = [max(g["waits"]) for g in groups if len(g["waits"]) == len(nodes)]
    metrics.update(latency_metrics("presence_convergence_", full))
    metrics["presence_writes_seen_by_all"] = len(full)
    metrics["presence_writes_seen_by_some"] = len(groups) - len(full)
    outcomes = defaultdict(int)
    for e in writes:
        outcomes[e["outcome"]] += 1
    for name in ("presence_registered", "presence_renewed", "presence_superseded", "presence_unknown",
                 "presence_deregistered"):
        metrics[f"{name}_count"] = outcomes[name]
    states = [e for es in nodes.values() for e in es if e["ev"] == "directory_state"]
    by_index = defaultdict(set)
    for e in states:
        by_index[e["last_applied"]].add(e["digest"])
    metrics["directory_states_compared"] = sum(1 for d in by_index.values() if d)
    metrics["directory_digest_mismatches"] = sum(1 for d in by_index.values() if len(d) > 1)
    last = {s: [e for e in es if e["ev"] == "directory_state"] for s, es in nodes.items()}
    last = [es[-1] for es in last.values() if es]
    metrics["directory_records_max"] = max((e["records"] for e in states), default=0)
    metrics["directory_expired"] = max((e["expired"] for e in last), default=0)
    metrics["directory_revived"] = max((e["revived"] for e in last), default=0)
    return metrics


# ---------------------------------------------------------------------------------------------
# Phase 2: the P2P link (H), zone gossip (I), offline operation (K), claims (J), the workload (L) and
# the cloud's bytes (D435). Each function gives the same keys for every run, so a run without the
# events reads 0 or NaN, and joins lines from different logs by ID, never by time (D447).
# ---------------------------------------------------------------------------------------------


def ratio(part, whole):
    return part / whole if whole else float("nan")


def update_key(update):
    return update["origin"], update["seq"]


def events_named(logs, names, start_us):
    """{ev: [events]} for each name, from `start_us` on, in one pass over `logs`."""
    found = {name: [] for name in names}
    for es in logs.values():
        for e in es:
            if e["ev"] in found and e["ts_us"] >= start_us:
                found[e["ev"]].append(e)
    return found


def endpoints(clients):
    """Programs whose peer endpoint opened (`p2p_listening`), so that peers could reach them."""
    return sum(1 for es in clients.values() if any(e["ev"] == "p2p_listening" for e in es))


# p2p_sent.ack, by the name its count goes under. "none" is no ack after every try (D339).
P2P_ACKS = {"accepted": "delivered", "duplicate": "duplicate", "refused": "refused", "none": "failed"}


def p2p_metrics(events, start_us):
    """Part H's P2P link (D336, D339, D464). Every `send_peer` call logs one `p2p_sent` on the sender: a
    user's direct send, a gossip copy, or a held update tried again. Its `latency_us` runs from the first
    try to the ack, retries included, on the sender's clock, so no two clocks meet. The delivery latency
    counts the sends whose peer has the update afterwards, acked `accepted` or `duplicate`. `p2p_ack_share`
    is the plan's ack rate: the sends that got any answer."""
    clients = client_sources(events)
    found = events_named(clients, ("p2p_sent", "p2p_received"), start_us)
    sent = found["p2p_sent"]
    acks = Counter(e["ack"] for e in sent)
    metrics = {"p2p_sent": len(sent)}
    for ack, name in P2P_ACKS.items():
        metrics[f"p2p_{name}"] = acks[ack]
    answered = [e for e in sent if e["ack"] != "none"]
    metrics["p2p_ack_share"] = ratio(len(answered), len(sent))
    metrics["p2p_retried_share"] = ratio(sum(1 for e in answered if e["tries"] > 1), len(answered))
    delivered = [e["latency_us"] for e in sent if e["ack"] in ("accepted", "duplicate")]
    metrics.update(latency_metrics("p2p_latency_", delivered))
    metrics["p2p_received"] = len(found["p2p_received"])
    metrics["p2p_endpoints"] = endpoints(clients)
    return metrics


GOSSIP_HOPS = range(8)  # a copy carries 0 to [gossip] hop_cap hops, 7 by default (D345, D346)
GOSSIP_LINES = ("gossip_received", "p2p_received", "p2p_sent", "gossip_send_failed")


def gossip_metrics(events, start_us):
    """Part I's zone gossip (D340 to D368). It follows the zone updates published in the measured time,
    from each origin's `gossip_published`, into every program's log, joined by update ID. A user's direct
    send to a peer passes the gossip core too, and logs a `gossip_received`, but nobody publishes it, so
    the join leaves it out.

    Duplicates. Part H's endpoint answers an update ID it has seen with `duplicate` before gossip sees the
    copy (D353). So a duplicate is a `p2p_received` with `duplicate`, or the rare `gossip_received` with
    `duplicate`. `gossip_duplicate_share` is duplicates over every copy that reached an endpoint, and
    `gossip_duplicates_per_new` is D350's rate, duplicates per delivery.

    Forwarding overhead. `gossip_copies_per_update` is what the cores chose to send: the origin's
    `targets` plus every `forwarded`, per published update (D350). `gossip_sends_per_update` counts the
    `p2p_sent` lines instead, so it adds store-and-forward's later tries (D415).

    Coverage. Per update, the programs that got it as new (`gossip_reach_mean`), and that over every other
    program with a peer endpoint (`gossip_coverage_share`). No event says which programs subscribe to a
    zone. A program subscribes to the zones of its users, so with users in every zone it is all of them.

    `gossip_age_*` is `age_ms`, the receiving PC's clock minus the origin PC's (D233). `gossip_hops_*` says
    how far an update went, with no clock."""
    clients = client_sources(events)
    published = {}
    for es in clients.values():
        for e in es:
            if e["ev"] == "gossip_published" and e["ts_us"] >= start_us:
                published[update_key(e["update"])] = e
    outcomes = Counter()
    reached = defaultdict(set)
    hops, ages = [], []
    forwarded = copies = endpoint_duplicates = sends = failed = 0
    for source, es in clients.items():
        for e in es:
            if e["ev"] not in GOSSIP_LINES or update_key(e["update"]) not in published:
                continue
            if e["ev"] == "gossip_received":
                outcomes[e["outcome"]] += 1
                forwarded += e["forwarded"]
                if e["outcome"] == "new":
                    reached[update_key(e["update"])].add(source)
                    hops.append(e["hops"])
                    ages.append(e["age_ms"] * 1000)
            elif e["ev"] == "p2p_received":
                copies += 1
                endpoint_duplicates += e["outcome"] == "duplicate"
            elif e["ev"] == "p2p_sent":
                sends += 1
            else:
                failed += 1
    updates = len(published)
    metrics = {
        "gossip_published": updates,
        "gossip_published_no_targets": sum(1 for e in published.values() if e["targets"] == 0),
    }
    for outcome in ("new", "duplicate", "obsolete", "not_subscribed"):
        metrics[f"gossip_received_{outcome}"] = outcomes[outcome]
    duplicates = endpoint_duplicates + outcomes["duplicate"]
    metrics["gossip_copies_received"] = copies
    metrics["gossip_duplicates"] = duplicates
    metrics["gossip_duplicate_share"] = ratio(duplicates, copies)
    metrics["gossip_duplicates_per_new"] = ratio(duplicates, outcomes["new"])
    chosen = sum(e["targets"] for e in published.values()) + forwarded
    metrics["gossip_copies_per_update"] = ratio(chosen, updates)
    metrics["gossip_sends_per_update"] = ratio(sends, updates)
    metrics["gossip_send_failed"] = failed
    reach = [len(reached[update]) for update in published]
    others = endpoints(clients) - 1
    metrics["gossip_reach_mean"] = float(np.mean(reach)) if reach else float("nan")
    metrics["gossip_coverage_share"] = (
        float(np.mean(reach)) / others if reach and others > 0 else float("nan")
    )
    metrics["gossip_full_coverage_share"] = (
        ratio(sum(1 for r in reach if r >= others), updates) if others > 0 else float("nan")
    )
    at_hops = Counter(hops)
    metrics["gossip_hops_mean"] = float(np.mean(hops)) if hops else float("nan")
    metrics["gossip_hops_max"] = max(hops) if hops else float("nan")
    for h in GOSSIP_HOPS:
        metrics[f"gossip_hops_{h}_share"] = ratio(at_hops[h], len(hops))
    metrics.update(latency_metrics("gossip_age_", ages))
    lists = events_named(clients, ("gossip_zone_refreshed",), start_us)["gossip_zone_refreshed"]
    metrics["gossip_zone_programs_max"] = max((e["programs"] for e in lists), default=0)
    return metrics


HELD_DROP_REASONS = ("age", "limit", "obsolete", "refused")  # offline::Dropped (D408, D409)
OFFLINE_LINES = ("client_online", "outbox_queued", "outbox_sent", "outbox_flushed", "claim_queued",
                 "claims_flushed", "peer_update_held", "peer_update_forwarded", "peer_update_dropped")


def offline_metrics(events, start_us):
    """Part K's offline operation (D400 to D417), each figure on one program's own clock.

    `offline_*` is how long each outage lasted (`client_online.offline_ms`). The reconciliation delay has
    three figures. `reconcile_reports_*` is `outbox_flushed.reconcile_us`, from the start of the flush to
    its last answer. `reconcile_claims_*` is `claims_flushed.reconcile_us`, the queued claims sent after the
    reports (D414). `reconcile_*` is per reconnect: from its `client_online` to the end of the last flush it
    ran, in that program's log. A reconnect with nothing queued flushes nothing, and is not in it. The held
    peer updates go out after both flushes, and no event marks the end of that, so they are not in it.

    `peer_updates_*` count store-and-forward (D407 to D411): held, sent later, or let go by reason.
    `peer_updates_held_at_end` is what each program still held when its log ended."""
    clients = client_sources(events)
    found = events_named(clients, OFFLINE_LINES, start_us)
    metrics = latency_metrics("offline_", [e["offline_ms"] * 1000 for e in found["client_online"]])
    metrics.update(latency_metrics("reconcile_reports_", [e["reconcile_us"] for e in found["outbox_flushed"]]))
    metrics.update(latency_metrics("reconcile_claims_", [e["reconcile_us"] for e in found["claims_flushed"]]))
    whole, held_at_end = [], 0
    for es in clients.values():
        opened = finished = None
        holding = defaultdict(int)
        for e in es:
            ev = e["ev"]
            if ev in ("client_offline", "client_online"):
                if opened is not None and finished is not None:
                    whole.append(finished - opened)
                opened = e["ts_us"] if ev == "client_online" and e["ts_us"] >= start_us else None
                finished = None
            elif ev in ("outbox_flushed", "claims_flushed"):
                finished = e["ts_us"] if opened is not None else None
            elif ev == "peer_update_held":
                holding[(e["to_addr"], update_key(e["update"]))] += 1
            elif ev in ("peer_update_forwarded", "peer_update_dropped"):
                key = (e["to_addr"], update_key(e["update"]))
                # One let go at once was never held, so it takes nothing off.
                if holding.get(key):
                    holding[key] -= 1
        if opened is not None and finished is not None:
            whole.append(finished - opened)
        held_at_end += sum(holding.values())
    metrics.update(latency_metrics("reconcile_", whole))
    metrics["reconcile_max_ms"] = max(whole) / 1000 if whole else float("nan")
    metrics["outbox_queued"] = len(found["outbox_queued"])
    sent = Counter(e["outcome"] for e in found["outbox_sent"])
    for outcome in ("answered", "expired", "failed"):
        metrics[f"outbox_{outcome}"] = sent[outcome]
    waits = [e["waited_ms"] * 1000 for e in found["outbox_sent"] if e["outcome"] == "answered"]
    metrics.update(latency_metrics("outbox_wait_", waits))
    metrics["claims_outbox_queued"] = len(found["claim_queued"])
    for field in ("sent", "claimed", "reconciling", "other", "failed"):
        metrics[f"claims_outbox_{field}"] = sum(e[field] for e in found["claims_flushed"])
    drops = Counter(e["reason"] for e in found["peer_update_dropped"])
    metrics["peer_updates_held"] = len(found["peer_update_held"])
    metrics["peer_updates_forwarded"] = len(found["peer_update_forwarded"])
    metrics["peer_updates_dropped"] = len(found["peer_update_dropped"])
    for reason in HELD_DROP_REASONS:
        metrics[f"peer_updates_dropped_{reason}"] = drops[reason]
    metrics["peer_updates_held_at_end"] = held_at_end
    return metrics


# claim_write.outcome for a claim, by the name its count goes under (proto.rs ClaimOutcome::kind).
CLAIM_OUTCOMES = {"resource_claimed": "claimed", "resource_claim_refused": "refused",
                  "resource_claim_reconciling": "reconciling", "resource_unknown": "unknown"}


def resource_key(resource):
    return resource["provider"], resource["seq"]


def user_key(user):
    return user["client"], user["user"]


def claims_metrics(events, start_us):
    """Part J's offers and claims (D370 to D381), from the node that proposed each write. A conflict is a
    claim that did not simply get in: refused online, or a reconciliation entry opened by a claim resent
    from the claims outbox (D376). `claim_conflicts_per_claim` is the two kinds together over every claim
    write, so a sweep that makes responders claim less often still compares (D442). `claim_unknown` is a claim on an offer the
    cloud does not hold or that has run out: the spec's expired-lease refusal, which no event tells apart.

    Latency is the proposer's, from the call arriving to the commit, on its clock. `reconciliation_time_*`
    runs from an entry's `reconciliation_opened` to its `reconciliation_resolved` on the same node, one
    clock. `reconciliation_wait_*` is the event's `wait_ms`: the resolving node's clock minus the
    challenger PC's first try, two clocks (D378, D380). loadgen resolves no entry (D441), so in its runs
    both are empty and the entries show in `claims_reconciling_at_end`.

    `claims_digest_mismatches` compares the once-a-second `claims_state` digests at equal `last_applied`,
    over the whole log, as the directory's are."""
    nodes = node_events(events)
    found = events_named(nodes, ("claim_write", "claim_conflict", "resource_offered", "reconciliation_resolved"),
                         start_us)
    claims = [e for e in found["claim_write"] if e["kind"] == "claim"]
    metrics = latency_metrics("claim_", [e["latency_us"] for e in claims])
    outcomes = Counter(e["outcome"] for e in claims)
    for outcome, name in CLAIM_OUTCOMES.items():
        metrics[f"claim_{name}"] = outcomes[outcome]
    metrics["claim_speculative"] = sum(1 for e in claims if e["speculative"])
    metrics["claim_conflicts_per_claim"] = ratio(metrics["claim_refused"] + metrics["claim_reconciling"],
                                                 len(claims))
    kinds = Counter(e["kind"] for e in found["claim_conflict"])
    for kind in ("online_refused", "reconciliation_opened"):
        metrics[f"claim_conflicts_{kind}"] = kinds[kind]
    released = Counter(e["outcome"] for e in found["claim_write"] if e["kind"] == "release")
    metrics["release_released"] = released["resource_claim_released"]
    metrics["release_stale"] = released["resource_claim_stale_release"]
    metrics.update(latency_metrics("offer_", [e["latency_us"] for e in found["resource_offered"]]))
    resolved = found["reconciliation_resolved"]
    metrics.update(latency_metrics("reconciliation_wait_", [e["wait_ms"] * 1000 for e in resolved]))
    times = []
    for es in nodes.values():
        opened = {}
        for e in es:
            if e["ev"] == "claim_conflict" and e["kind"] == "reconciliation_opened":
                opened[(resource_key(e["resource"]), user_key(e["claimant"]))] = e["ts_us"]
            elif e["ev"] == "reconciliation_resolved" and e["ts_us"] >= start_us:
                at = opened.pop((resource_key(e["resource"]), user_key(e["challenger"])), None)
                if at is not None:
                    times.append(e["ts_us"] - at)
    metrics.update(latency_metrics("reconciliation_time_", times))
    digests, seen_by, last = defaultdict(set), defaultdict(set), []
    for source, es in nodes.items():
        states = [e for e in es if e["ev"] == "claims_state"]
        for e in states:
            digests[e["last_applied"]].add(e["digest"])
            seen_by[e["last_applied"]].add(source)
        last += states[-1:]
    metrics["claims_states_compared"] = sum(1 for nodes_at in seen_by.values() if len(nodes_at) > 1)
    metrics["claims_digest_mismatches"] = sum(1 for found_at in digests.values() if len(found_at) > 1)
    metrics["claims_reconciling_at_end"] = max((e["reconciling"] for e in last), default=0)
    return metrics


# The outcomes kept apart, as `workload_<action>_<outcome>`; any other is in `workload_other` (D436, D441).
WORKLOAD_ACTIONS = (
    "report:answered", "report:queued", "report:gave_up", "report:failed",
    "register:ok", "subscribe:with_join", "move_zone:ok", "move_zone:offline",
    "discover:ok", "discover:offline", "discover:gave_up",
    "peer_send:delivered", "peer_send:held", "peer_send:no_peer", "peer_send:nothing_to_send",
    "peer_send:gave_up",
    "offer:offered", "offer:renewed", "offer:offline", "offer:timeout",
    "claim:claimed", "claim:refused", "claim:reconciling", "claim:queued", "claim:no_offer",
    "release:released", "release:refused", "release:offline", "release:timeout",
)
PEER_RECEIVED_KINDS = ("incident_summary", "assignment_update", "resource_offer")
WORKLOAD_SETTINGS = ("claim_delay_ms", "offline_chance_per_min_percent", "offline_for_ms")


def workload_metrics(events, measured_s):
    """Part L's simulated users (D430 to D442), from each program's `workload_done`: what its users did,
    counted by `action:outcome`, summed over programs. A program counts only actions started after its own
    warm-up. A user joins and subscribes once, at its start, so after a warm-up `workload_register_ok` is
    about 0. `peer_received:*` counts updates that reached the program, so it is not an action.

    `workload_claim_refused_share` is the clients' view of conflicts: online claims refused, over online
    claims granted or refused. A claim queued offline is answered at reconnect and is not in it;
    `claim_conflicts_per_claim` has those. `workload_started` gives the roles and the cell's settings."""
    clients = client_sources(events)
    done = [e for es in clients.values() for e in es if e["ev"] == "workload_done"]
    started = [e for es in clients.values() for e in es if e["ev"] == "workload_started"]
    actions = Counter()
    for e in done:
        actions.update(e["actions"])
    metrics = {f"workload_{key.replace(':', '_')}": actions[key] for key in WORKLOAD_ACTIONS}
    for kind in PEER_RECEIVED_KINDS:
        metrics[f"workload_peer_received_{kind}"] = actions[f"peer_received:{kind}"]
    by_users = sum(n for key, n in actions.items() if not key.startswith("peer_received:"))
    metrics["workload_actions"] = by_users
    metrics["workload_actions_per_s"] = by_users / measured_s if done and measured_s else float("nan")
    metrics["workload_other"] = by_users - sum(actions[key] for key in WORKLOAD_ACTIONS)
    refused = actions["claim:refused"]
    metrics["workload_claim_refused_share"] = ratio(refused, actions["claim:claimed"] + refused)
    metrics["workload_programs"] = len(done)
    metrics["workload_outages"] = sum(e["outages"] for e in done)
    for role in ("ordinary", "responders", "providers"):
        metrics[f"workload_{role}"] = sum(e[role] for e in started)
    for setting in WORKLOAD_SETTINGS:
        metrics[f"workload_{setting}"] = max((e[setting] for e in started), default=float("nan"))
    return metrics


def cloud_bytes_metrics(events, start_us, end_us):
    """D435: P2P traffic runs between the client PCs only, and the cloud's bytes prove it. Over the
    measured time, each process's rate from its own cumulative counters, on its own clock, summed:

    - `cloud_udp_bytes_per_s`: the three nodes' client ports (node `udp_counters`), which carry every
      client call to the cloud: reports, presence, discovery and claims;
    - `cloud_net_bytes_per_s`: the node PCs' network cards (`resource_usage`), Raft included. The loopback
      is not counted, so with every process on one PC it shows none of their traffic, only the PC's other
      traffic;
    - `client_cloud_socket_bytes_per_s`: the programs' sockets to the cloud (client `udp_counters`). Their
      peer sockets are apart and logged nowhere;
    - `p2p_sends_per_s` and `p2p_tries_per_s`: the P2P sends on the clients (`p2p_sent`), and the tries
      in them. P2P bytes are not logged.

    `cloud_udp_bytes_over_client` near 1 says the cloud carried what the programs' cloud sockets sent and
    nothing more. Only the window's edges compare clocks, which moves each rate by up to a second's worth
    over the measured time."""

    def rate(samples, fields):
        inside = [e for e in samples
                  if start_us <= e["ts_us"] <= end_us and all(e.get(f) is not None for f in fields)]
        if len(inside) < 2:
            return None
        # A process started again counts from 0, so a step down ends one life and adds nothing.
        moved = sum(max(sum(b[f] - a[f] for f in fields), 0) for a, b in zip(inside, inside[1:]))
        seconds = (inside[-1]["ts_us"] - inside[0]["ts_us"]) / 1_000_000
        return moved / seconds if seconds > 0 else None

    def total(logs, ev, fields):
        rates = [rate([e for e in es if e["ev"] == ev], fields) for es in logs.values()]
        rates = [r for r in rates if r is not None]
        return sum(rates) if rates else float("nan")

    nodes, clients = node_events(events), client_sources(events)
    cloud = total(nodes, "udp_counters", ("rx_bytes", "tx_bytes"))
    client = total(clients, "udp_counters", ("rx_bytes", "tx_bytes"))
    seconds = (end_us - start_us) / 1_000_000
    sends = [e for es in clients.values() for e in es if e["ev"] == "p2p_sent" and start_us <= e["ts_us"] <= end_us]
    return {
        "cloud_udp_bytes_per_s": cloud,
        "cloud_net_bytes_per_s": total(nodes, "resource_usage", ("net_rx_bytes", "net_tx_bytes")),
        "client_cloud_socket_bytes_per_s": client,
        "cloud_udp_bytes_over_client": cloud / client if client > 0 else float("nan"),
        "p2p_sends_per_s": len(sends) / seconds if seconds > 0 else float("nan"),
        "p2p_tries_per_s": sum(e["tries"] for e in sends) / seconds if seconds > 0 else float("nan"),
    }


def sent_in_window(events, start_us):
    """Messages sent in the measured time, by kind. Counters are cumulative."""
    per_kind = defaultdict(int)
    for es in node_events(events).values():
        snapshots = [e for e in es if e["ev"] == "messages" and e["ts_us"] >= start_us]
        if len(snapshots) < 2:
            continue
        first, last = snapshots[0]["sent"], snapshots[-1]["sent"]
        for kind, count in last.items():
            per_kind[kind] += count - first.get(kind, 0)
    return per_kind


def fault_election_metrics(events, start_us):
    """The fault election (D158, D161), grouped by round from each node's `fault_election` lines.

    A node that never held every number logs a null victim. Null lines are skipped in the mean, and a
    round where no line names a victim counts as abandoned. A lagging node can reuse the number of a
    round the others gave up on, which only merges two abandoned rounds.
    """
    rounds = defaultdict(list)
    durations_us = []
    for source, es in node_events(events).items():
        node = int(source.split("-")[1])
        for e in es:
            if e["ev"] != "fault_election" or e["ts_us"] < start_us:
                continue
            rounds[e["round"]].append((node, e["victim"]))
            # A joiner's figure is shorter by construction, so only the caller's says how long the
            # election took (D161).
            if e["called"] and e["election_us"] is not None:
                durations_us.append(e["election_us"])

    elections = abandoned = splits = not_silenced = 0
    for lines in rounds.values():
        named = {victim for _, victim in lines if victim is not None}
        if not named:
            abandoned += 1
            continue
        elections += 1
        if len(named) > 1:
            splits += 1
        if any(node in named and victim is None for node, victim in lines):
            not_silenced += 1

    bids = sent_in_window(events, start_us).get("fault_bid", 0)
    refused = sum(
        1
        for es in node_events(events).values()
        for e in es
        if e["ev"] == "fault_refused" and e["ts_us"] >= start_us
    )
    return {
        "fault_elections": elections,
        "fault_rounds_abandoned": abandoned,
        "fault_election_mean_ms": float(np.mean(durations_us)) / 1000 if durations_us else float("nan"),
        # Must be 0. Same numbers, same winner, so a split means the rule broke (bid_split_decisions
        # is the same check for the load election, where a split is allowed and the commit settles it).
        "fault_split_decisions": splits,
        # The named node's own line is null: it missed a bid, so it never knew it had won.
        "fault_elected_not_silenced": not_silenced,
        "fault_refused": refused,
        # Per round, because per request it would be a tiny number that says nothing.
        "fault_bids_per_round": bids / len(rounds) if rounds else float("nan"),
    }


def message_metrics(events, start_us, completed):
    """Messages sent in the measured time, per answered request."""
    per_kind = sent_in_window(events, start_us)
    if not completed:
        return {}
    metrics = {f"msgs_{kind}_per_req": n / completed for kind, n in per_kind.items()}
    metrics["msgs_all_per_req"] = sum(per_kind.values()) / completed
    return metrics


def resource_metrics(events, start_us):
    cpu, rss, net, queued, running = [], [], [], [], []
    for es in node_events(events).values():
        samples = [e for e in es if e["ev"] == "resource_usage" and e["ts_us"] >= start_us]
        for before, after in zip(samples, samples[1:]):
            seconds = (after["ts_us"] - before["ts_us"]) / 1_000_000
            if seconds <= 0:
                continue
            if after.get("cpu_ticks") is not None and before.get("cpu_ticks") is not None:
                cpu.append((after["cpu_ticks"] - before["cpu_ticks"]) / USER_HZ / seconds)
            if after.get("net_tx_bytes") is not None and before.get("net_tx_bytes") is not None:
                moved = (
                    after["net_tx_bytes"] - before["net_tx_bytes"]
                    + after["net_rx_bytes"] - before["net_rx_bytes"]
                )
                net.append(moved / seconds)
        for s in samples:
            if s.get("rss_kb") is not None:
                rss.append(s["rss_kb"])
            queued.append(s["queued"])
            running.append(s["running"])

    def mean(xs):
        return float(np.mean(xs)) if xs else float("nan")

    return {
        "cpu_cores_mean": mean(cpu),
        "rss_mb_max": max(rss) / 1024 if rss else float("nan"),
        "net_bytes_per_s_mean": mean(net),
        "queued_mean": mean(queued),
        "queued_max": max(queued) if queued else float("nan"),
        "running_mean": mean(running),
    }


def whole_log_counts(samples, fields):
    """What a process's cumulative counters add up to over its whole log. A node killed and restarted
    counts from 0 again in the same file, so a counter that falls ends one life and starts the next."""
    totals = defaultdict(int)
    previous = {}
    for sample in samples:
        for field in fields:
            value = sample.get(field, 0)
            if value < previous.get(field, 0):
                totals[field] += previous[field]
            previous[field] = value
    for field, value in previous.items():
        totals[field] += value
    return totals


def udp_metrics(events, start_us, completed):
    """What the UDP client path carried and lost (D172). The counters are cumulative per process.

    What it carried is per answered request, so it is over the measured time: each counter's last value
    minus its first. Cancels are outside the msgs_ figures, which count what nodes send, so this is where
    they show.

    What it lost is over each process's whole log, warm-up included. Each process samples on its own
    second, so over the measured time one side's count starts up to a second later than the other's, and
    that read as 1 to 4 percent lost in a run where the kernel dropped nothing.

    udp_request_loss_share and udp_cancel_loss_share still read false, about 1 percent with nothing lost,
    and would hide a real loss below that. A node writes no counters as it stops, so its last line can be a
    second older than the client's last send. udp_copy_metrics has the real figure (D229)."""
    totals = {"client": defaultdict(int), "node": defaultdict(int)}
    whole_log = {"client": defaultdict(int), "node": defaultdict(int)}
    drops = {"client": [], "node": []}
    rcvbufs = []
    for source, es in events.items():
        side = "client" if source.startswith("client-") else "node"
        rcvbufs += [e["rcvbuf_got_bytes"] for e in es if e["ev"] == "udp_socket"]
        counts = whole_log_counts(
            (e for e in es if e["ev"] == "udp_counters"),
            ("tx_requests", "tx_cancels", "rx_requests", "rx_cancels"),
        )
        for field, value in counts.items():
            whole_log[side][field] += value
        samples = [e for e in es if e["ev"] == "udp_counters" and e["ts_us"] >= start_us]
        if len(samples) < 2:
            continue
        first, last = samples[0], samples[-1]
        for field, value in last.items():
            if field not in ("ts_us", "src", "ev", "udp_drops") and not field.endswith("_max_bytes"):
                totals[side][field] += value - first.get(field, 0)
        known = [e["udp_drops"] for e in samples if e.get("udp_drops") is not None]
        if len(known) >= 2:
            drops[side].append(known[-1] - known[0])
    metrics = {}
    if rcvbufs:
        metrics["udp_rcvbuf_got_min_bytes"] = min(rcvbufs)
    client, node = totals["client"], totals["node"]
    if not client and not node:
        return metrics
    if completed:
        metrics["udp_requests_per_req"] = client["tx_requests"] / completed
        metrics["udp_cancels_per_req"] = client["tx_cancels"] / completed
        metrics["udp_datagrams_per_req"] = (client["tx_datagrams"] + node["tx_datagrams"]) / completed
    sent, read = whole_log["client"], whole_log["node"]
    if sent["tx_requests"]:
        metrics["udp_request_loss_share"] = max(1 - read["rx_requests"] / sent["tx_requests"], 0.0)
    if sent["tx_cancels"]:
        metrics["udp_cancel_loss_share"] = max(1 - read["rx_cancels"] / sent["tx_cancels"], 0.0)
    metrics["udp_gate_dropped"] = node["rx_gate_dropped"]
    metrics["udp_queue_full"] = node["rx_queue_full"]
    metrics["udp_bad"] = client["rx_bad"] + node["rx_bad"]
    metrics["udp_send_lost"] = sum(
        side["tx_would_block"] + side["tx_encode_errors"] + side["tx_errors"] for side in (client, node)
    )
    # Machine-wide counters. One node runs per machine, so nodes add up; on one machine, in rehearsal,
    # each node counts all three. Many client programs share a PC, so for them only the largest means
    # anything.
    metrics["udp_drops_nodes"] = sum(drops["node"])
    metrics["udp_drops_client_pc_max"] = max(drops["client"], default=0)
    return metrics


def udp_copy_metrics(events):
    """The share of report copies that never reached a node's code, from events that cannot miss (D229).
    Each `client_sent` is one copy to every node. Each copy a node handled is a `request_received`, or was
    turned away by the fault gate, which counts requests only. Over the whole log, as a copy can be on its
    way at any edge. A node that is off reads nothing, so in a run with `kill -9` not all of this is the
    network. A gate's count can be a second old when the run ends, which only matters if a silence
    outlasts the clients."""
    logs = node_events(events)
    sent = sum(1 for es in client_sources(events).values() for e in es if e["ev"] == "client_sent")
    if not sent or not logs:
        return {}
    handled = 0
    for es in logs.values():
        handled += sum(1 for e in es if e["ev"] == "request_received")
        counters = (e for e in es if e["ev"] == "udp_counters")
        handled += whole_log_counts(counters, ("rx_gate_dropped",))["rx_gate_dropped"]
    return {"udp_copies_lost_share": max(1 - handled / (sent * len(logs)), 0.0)}


def commit_metrics(events, start_us):
    commits = [
        e
        for es in node_events(events).values()
        for e in es
        if e["ev"] == "commit" and e["ts_us"] >= start_us and e["kind"] == "commit_result"
    ]
    service = [
        e["service_us"]
        for es in node_events(events).values()
        for e in es
        if e["ev"] == "bid_sent" and e["ts_us"] >= start_us
    ]
    metrics = latency_metrics("commit_", [e["latency_us"] for e in commits])
    # What a node pays for not leading: `forwarded` means this proposal went through a peer (D26, D166).
    for path, forwarded in (("local", False), ("forwarded", True)):
        latencies = [e["latency_us"] for e in commits if e["forwarded"] == forwarded]
        metrics.update(latency_metrics(f"commit_{path}_", latencies))
    metrics["service_mean_us"] = float(np.mean(service)) if service else float("nan")
    return metrics


def client_sources(events):
    return {s: es for s, es in events.items() if s.startswith("client-")}


def client_program_metrics(events, meta):
    """How the load was shaped (D181, D186): real programs that sent anything, users per program, and
    client connections dropped, by reason. Link events exist only while clients hold connections."""
    programs = sum(
        1 for es in client_sources(events).values() if any(e["ev"] == "client_sent" for e in es)
    )
    metrics = {"client_programs": programs}
    if meta.get("users") and programs:
        metrics["users_per_program"] = meta["users"] / programs
    users = [
        e["users"]
        for es in client_sources(events).values()
        for e in es
        if e["ev"] == "client_started"
    ]
    if users:
        metrics["client_users"] = sum(users)
    drops = [
        e["reason"]
        for es in client_sources(events).values()
        for e in es
        if e["ev"] == "client_link_down"
    ]
    metrics["client_link_down"] = len(drops)
    for reason in set(drops):
        metrics[f"client_link_down_{reason.replace(' ', '_')}"] = drops.count(reason)
    return metrics


def client_script_metrics(run_dir):
    """What each machine's start-users.sh printed about its programs (D183, D186). A program that dies
    half way has still sent something, so only these lines show it: programs that failed, programs a
    signal stopped early, and scripts with no totals at all, because they were killed themselves."""
    failed = stopped_early = no_totals = 0
    for path in Path(run_dir).glob("users-*.out"):
        text = path.read_text(encoding="utf-8", errors="replace")
        totals = re.findall(r"started \d+, failed (\d+)", text)
        failed += sum(int(n) for n in totals)
        no_totals += 0 if totals else 1
        stopped_early += text.count("stopped by a signal")
    return {
        "client_programs_failed": failed,
        "client_programs_stopped_early": stopped_early,
        "client_scripts_with_no_totals": no_totals,
    }


def arrival_peak_share(times_us):
    """The share of arrivals in the busiest 100 ms of their second, over all the seconds given. Even
    arrivals give a tenth, and a whole second's users sending together give 1. Client machines agree
    only as far as NTP does, which is good enough at this width. Seconds with few arrivals read high."""
    seconds = defaultdict(lambda: defaultdict(int))
    for ts in times_us:
        seconds[ts // 1_000_000][ts // 100_000] += 1
    if not seconds:
        return float("nan")
    peaks = sum(max(buckets.values()) for buckets in seconds.values())
    return peaks / len(times_us)


def arrival_metrics(events, start_us):
    """Do first sends clump (D182)? At the very start, over the measured time, and after a fault, when
    waiting users can come back together."""
    sends = sorted(
        e["ts_us"]
        for es in client_sources(events).values()
        for e in es
        if e["ev"] == "client_sent" and e["attempt"] == 0
    )
    if not sends:
        return {}
    metrics = {
        "arrival_peak_100ms_share_first_s": arrival_peak_share(
            [ts - sends[0] for ts in sends if ts < sends[0] + 1_000_000]
        ),
        "arrival_peak_100ms_share": arrival_peak_share([ts for ts in sends if ts >= start_us]),
    }
    restored = [e["ts_us"] for es in node_events(events).values() for e in es if e["ev"] == "restored"]
    if restored:
        fault_end = max(restored)
        metrics["arrival_peak_100ms_share_after_fault"] = arrival_peak_share(
            [ts for ts in sends if ts >= fault_end]
        )
    return metrics


def client_pc_metrics(events, start_us):
    """The client PCs' own CPU and memory (D185): a saturated client PC reads as cloud latency."""
    busy, available = [], []
    for es in client_sources(events).values():
        samples = [e for e in es if e["ev"] == "machine_usage" and e["ts_us"] >= start_us]
        for before, after in zip(samples, samples[1:]):
            if None in (before["cpu_total_ticks"], after["cpu_total_ticks"]):
                continue
            total = after["cpu_total_ticks"] - before["cpu_total_ticks"]
            if total > 0:
                busy.append((after["cpu_busy_ticks"] - before["cpu_busy_ticks"]) / total)
        available += [e["mem_available_kb"] for e in samples if e["mem_available_kb"] is not None]
    metrics = {}
    if busy:
        metrics["client_pc_cpu_busy_p95"] = nearest_rank(sorted(busy), 95)
    if available:
        metrics["client_pc_mem_available_min_mb"] = min(available) / 1024
    return metrics


def analyse_run(run_dir):
    meta = json.loads((Path(run_dir) / "run.json").read_text(encoding="utf-8"))
    warmup_s, measured_s = meta["warmup_s"], meta["measured_s"]
    events = read_events(run_dir)
    start = measured_start(events, warmup_s)
    done, _ = client_requests(events, warmup_s)
    metrics = request_metrics(events, warmup_s, measured_s)
    metrics.update(bid_metrics(events, start))
    metrics.update(election_metrics(events, start))
    metrics.update(failover_metrics(events, start, measured_s))
    metrics.update(failover_gap_metrics(events, start, measured_s))
    metrics.update(client_pc_gap_metrics(events, warmup_s, measured_s))
    metrics.update(clock_skew_metrics(events))
    metrics["leader_plan_share"] = leader_plan_share(events, start, metrics)
    metrics.update(fault_metrics(events, done))
    metrics.update(refusal_metrics(events, start))
    metrics.update(convergence_metrics(events))
    metrics.update(presence_metrics(events, start))
    metrics.update(p2p_metrics(events, start))
    metrics.update(gossip_metrics(events, start))
    metrics.update(offline_metrics(events, start))
    metrics.update(claims_metrics(events, start))
    metrics.update(workload_metrics(events, measured_s))
    metrics.update(cloud_bytes_metrics(events, start, start + measured_s * 1_000_000))
    metrics.update(fault_election_metrics(events, start))
    metrics.update(message_metrics(events, start, len(done)))
    metrics.update(resource_metrics(events, start))
    metrics.update(udp_metrics(events, start, len(done)))
    metrics.update(udp_copy_metrics(events))
    metrics.update(commit_metrics(events, start))
    metrics.update(client_program_metrics(events, meta))
    metrics.update(client_script_metrics(run_dir))
    metrics.update(arrival_metrics(events, start))
    metrics.update(client_pc_metrics(events, start))
    metrics["node_logs"] = len(node_events(events))
    metrics["silenced_events"] = sum(
        1 for es in node_events(events).values() for e in es if e["ev"] == "silenced"
    )
    return meta, metrics


def run_problem(meta, metrics):
    """Why this run must not be averaged in, or None (D186). Missing or dead programs mean less load than
    the cell says, and a fault that never fired would pass for a fault with no effect. A run.json from
    before the runner wrote these keys skips the checks."""
    expected = meta.get("client_programs_expected")
    if expected is not None and metrics["client_programs"] != expected:
        return f"{metrics['client_programs']} of {expected} client programs sent anything"
    if meta.get("users") and metrics.get("client_users", meta["users"]) != meta["users"]:
        return f"{metrics['client_users']} users ran, and the cell says {meta['users']}"
    if metrics["client_programs_failed"]:
        return f"{metrics['client_programs_failed']} client programs failed; see users-*.out"
    if metrics["client_programs_stopped_early"]:
        return f"a signal stopped {metrics['client_programs_stopped_early']} client programs early"
    if metrics["client_scripts_with_no_totals"]:
        return f"{metrics['client_scripts_with_no_totals']} start-users.sh runs never printed their totals"
    if meta.get("nodes") and metrics["node_logs"] != meta["nodes"]:
        return f"{metrics['node_logs']} of {meta['nodes']} node logs were copied"
    victim = meta.get("victim", "none")
    if victim != "none" and metrics["silenced_events"] == 0:
        return f"the cell silences its {victim}, and no node logged a silence"
    return None


def confidence_interval(values):
    """Mean and 95% half-width across runs. With one run there is no interval."""
    xs = np.array([v for v in values if not math.isnan(v)], dtype=float)
    if len(xs) == 0:
        return float("nan"), float("nan"), 0
    if len(xs) == 1:
        return float(xs[0]), float("nan"), 1
    half = stats.t.ppf(0.975, len(xs) - 1) * xs.std(ddof=1) / math.sqrt(len(xs))
    return float(xs.mean()), float(half), len(xs)


def summarise(root):
    """{(experiment, cell): {metric: (mean, half_width, n)}} over every run under `root`."""
    per_cell = defaultdict(list)
    for meta_path in sorted(Path(root).glob("*/*/*/run.json")):
        try:
            meta, metrics = analyse_run(meta_path.parent)
        except json.JSONDecodeError as error:
            # One run with a damaged log must not cost the summary of every other run.
            print(f"left out {meta_path.parent}: a log is damaged ({error})")
            continue
        problem = run_problem(meta, metrics)
        if problem:
            print(f"left out {meta_path.parent}: {problem}")
            continue
        per_cell[(meta["experiment"], meta["cell"])].append(metrics)
    summary = {}
    for key, runs in per_cell.items():
        names = sorted({name for run in runs for name in run})
        summary[key] = {
            name: confidence_interval([run.get(name, missing_value(name)) for run in runs])
            for name in names
        }
        for name in WORST_RUN_METRICS:
            seen = [run[name] for run in runs if name in run and not math.isnan(run[name])]
            if seen:
                summary[key][f"{name}_worst_run"] = (max(seen), float("nan"), len(seen))
    return summary


def expired_warnings(summary):
    """Cells where a report was refused as too old. The default TTL outlasts every client retry and
    the runner never passes --ttl-ms, so a measured run should have none (D147). Any here most likely
    means an age stamped wrongly, which looks like a very fast run where nobody gets sent."""
    warnings = []
    for (experiment, cell), metrics in sorted(summary.items()):
        mean = metrics.get("reports_expired", (float("nan"),))[0]
        if mean > 0:
            warnings.append(
                f"warning: {experiment}/{cell}: reports_expired is {mean:g} per run, expected 0; "
                "check how the client stamps ages before trusting this cell"
            )
    return warnings


def missing_value(name):
    """What a run with no such metric counts as. A link-drop reason that did not happen in a run is 0 there,
    and leaving that run out would average the reason over only the runs that had it."""
    return 0 if name.startswith("client_link_down_") else float("nan")


def udp_warnings(summary):
    """Cells whose UDP path ran short (D171). Lost copies cost a client about 2 s, so a cell with drops
    has retry shapes the report must explain, and a capped buffer means a machine missed the sysctl."""
    warnings = []
    for (experiment, cell), metrics in sorted(summary.items()):
        got = metrics.get("udp_rcvbuf_got_min_bytes", (float("nan"),))[0]
        if got < UDP_RCVBUF_ASKED_BYTES:
            warnings.append(
                f"warning: {experiment}/{cell}: a UDP receive buffer got {got:.0f} bytes of "
                f"{UDP_RCVBUF_ASKED_BYTES} asked; net.core.rmem_max was not raised on some machine"
            )
        for name in ("udp_drops_nodes", "udp_drops_client_pc_max"):
            drops = metrics.get(name, (float("nan"),))[0]
            if drops > 0:
                warnings.append(
                    f"warning: {experiment}/{cell}: {name} is {drops:g} per run; "
                    "each lost copy can cost a client a 2 s retry"
                )
    return warnings


def failover_warnings(summary):
    """Cells where a leader was lost and the logs show no next one (D229). Those losses are in neither
    failover_to_* mean, so the mean alone reads better than the cell was."""
    warnings = []
    for (experiment, cell), metrics in sorted(summary.items()):
        lost = metrics.get("failovers_without_leader", (float("nan"),))[0]
        if lost > 0:
            warnings.append(
                f"warning: {experiment}/{cell}: failovers_without_leader is {lost:g} per run, and those losses are "
                "in no failover_to_* mean; read the raft_state lines in that cell's logs and node_clock_skew_ms, "
                "as a new leader's clock behind by more than the failover reads this way too"
            )
    return warnings


def clock_warnings(summary):
    """Cells whose clocks were far enough apart to move the figures that compare them (D56, D233)."""
    warnings = []
    for (experiment, cell), metrics in sorted(summary.items()):
        nodes = metrics.get("node_clock_skew_ms", (float("nan"),))[0]
        clients = metrics.get("client_clock_skew_ms", (float("nan"),))[0]
        if nodes > NODE_CLOCK_SKEW_WARN_MS and metrics.get("failovers", (0,))[0] > 0:
            warnings.append(
                f"warning: {experiment}/{cell}: the node PCs' clocks were about {nodes:.0f} ms apart, so "
                "failover_to_* can be off by that much; FT-2 reads failover_gap_ms_* and client_gap_pc_*"
            )
        if clients > CLIENT_CLOCK_SKEW_WARN_MS:
            warnings.append(
                f"warning: {experiment}/{cell}: a client PC's clock was about {clients:.0f} ms from a node's, "
                "so before_*, during_*, after_*, arrival_peak_*, priority_mean, reconciliation_wait_* and "
                "gossip_age_* (two client PCs, up to twice that) are off; say so"
            )
    return warnings


def provisional_notes(summary):
    """What to say beside a degraded cell's latency (D122, D229). A provisional answer needs no commit, so
    before_*, during_* and after_* read far better during an outage, and most clients told a responder was
    coming may not have that on record later. They are not told."""
    notes = []
    for (experiment, cell), metrics in sorted(summary.items()):
        replied = metrics.get("provisional_replied", (float("nan"),))[0]
        if replied > 0:
            conflicts = metrics.get("provisional_conflicts", (float("nan"),))[0]
            notes.append(
                f"note: {experiment}/{cell}: {replied:g} provisional answers and {conflicts:g} "
                "provisional_conflicts per run, which clients are not told of; before_*, during_* and "
                "after_* here mix provisional answers with committed ones"
            )
    return notes


def write_csv(summary, path):
    with open(path, "w", newline="", encoding="utf-8") as out:
        writer = csv.writer(out)
        writer.writerow(["experiment", "cell", "metric", "runs", "mean", "ci95_half_width"])
        for (experiment, cell), metrics in sorted(summary.items()):
            for name, (mean, half, n) in sorted(metrics.items()):
                writer.writerow([experiment, cell, name, n, mean, half])


# ---------------------------------------------------------------------------------------------
# Plots. Static figures for the report; summary.csv is the table view behind each.
# ---------------------------------------------------------------------------------------------

SERIES = ["#2a78d6", "#eb6834", "#1baf7a"]
INK, MUTED, GRID = "#0b0b0b", "#52514e", "#e5e4e0"


def style(ax, title, xlabel, ylabel):
    ax.set_title(title, loc="left", fontsize=11, color=INK)
    ax.set_xlabel(xlabel, color=MUTED, fontsize=9)
    ax.set_ylabel(ylabel, color=MUTED, fontsize=9)
    ax.grid(axis="y", color=GRID, linewidth=0.8)
    ax.set_axisbelow(True)
    ax.tick_params(colors=MUTED, labelsize=8, length=0)
    for side in ("top", "right", "left"):
        ax.spines[side].set_visible(False)
    ax.spines["bottom"].set_color(GRID)


def e1_series(summary, metric):
    """{policy: [(users, mean, half)]} from cells named <users>-<policy>."""
    series = defaultdict(list)
    for (experiment, cell), metrics in summary.items():
        if experiment != "e1" or metric not in metrics:
            continue
        users, policy = cell.split("-", 1)
        mean, half, _ = metrics[metric]
        series[policy].append((int(users), mean, 0.0 if math.isnan(half) else half))
    return {policy: sorted(points) for policy, points in sorted(series.items())}


def plot_e1(plt, summary, out):
    panels = [("p50_ms", "p50"), ("p95_ms", "p95"), ("p99_ms", "p99"), ("throughput_rps", "Throughput")]
    if not e1_series(summary, "p50_ms"):
        return
    fig, axes = plt.subplots(1, 4, figsize=(13, 3.2))
    for ax, (metric, title) in zip(axes, panels):
        for color, (policy, points) in zip(SERIES, e1_series(summary, metric).items()):
            xs, ys, errs = zip(*points)
            ax.errorbar(xs, ys, yerr=errs, color=color, linewidth=2, elinewidth=1, marker="o",
                        markersize=5, capsize=2, label=policy)
        ax.set_xscale("log")
        users = sorted({x for points in e1_series(summary, metric).values() for x, _, _ in points})
        ax.set_xticks(users, [str(u) for u in users])
        ax.minorticks_off()
        style(ax, title, "users", "req/s" if metric == "throughput_rps" else "ms")
    axes[0].legend(frameon=False, fontsize=8)
    fig.tight_layout()
    fig.savefig(out / "e1_latency_throughput.png", dpi=160)
    plt.close(fig)


def plot_fairness(plt, summary, out):
    rows = [
        (f"{experiment}-{cell}", metrics["fairness_jain"])
        for (experiment, cell), metrics in sorted(summary.items())
        if "fairness_jain" in metrics and not math.isnan(metrics["fairness_jain"][0])
    ]
    if not rows:
        return
    fig, ax = plt.subplots(figsize=(7, max(2.0, 0.28 * len(rows))))
    names = [name for name, _ in rows]
    means = [m[0] for _, m in rows]
    errs = [0.0 if math.isnan(m[1]) else m[1] for _, m in rows]
    ax.barh(names, means, xerr=errs, color=SERIES[0], height=0.6, error_kw={"ecolor": MUTED, "capsize": 2})
    ax.set_xlim(0, 1.05)
    ax.invert_yaxis()
    style(ax, "Load fairness (Jain's index, 1 is even)", "index", "")
    ax.grid(axis="x", color=GRID, linewidth=0.8)
    ax.grid(axis="y", visible=False)
    fig.tight_layout()
    fig.savefig(out / "fairness.png", dpi=160)
    plt.close(fig)


# Faults on a timer, one after another. Their "during" runs from the first silence to the last restore,
# which is most of the run, so a before-and-after picture says nothing about any one fault.
FAULTS_ON_A_TIMER = {"ft8", "e5"}


def plot_phases(plt, summary, out):
    cells = [
        (f"{experiment}-{cell}", metrics)
        for (experiment, cell), metrics in sorted(summary.items())
        if "during_p99_ms" in metrics and experiment not in FAULTS_ON_A_TIMER
    ]
    if not cells:
        return
    fig, ax = plt.subplots(figsize=(max(6, 1.2 * len(cells)), 3.4))
    width = 0.26
    for i, phase in enumerate(("before", "during", "after")):
        xs = [x + (i - 1) * (width + 0.02) for x in range(len(cells))]
        ys = [m[f"{phase}_p99_ms"][0] for _, m in cells]
        errs = [0.0 if math.isnan(m[f"{phase}_p99_ms"][1]) else m[f"{phase}_p99_ms"][1] for _, m in cells]
        ax.bar(xs, ys, width=width, yerr=errs, color=SERIES[i], label=phase,
               error_kw={"ecolor": MUTED, "capsize": 2})
    ax.set_xticks(range(len(cells)))
    ax.set_xticklabels([name for name, _ in cells], rotation=30, ha="right")
    style(ax, "p99 latency before, during and after a fault", "", "ms")
    ax.legend(frameon=False, fontsize=8)
    fig.tight_layout()
    fig.savefig(out / "fault_phases.png", dpi=160)
    plt.close(fig)


def sweep_series(summary, experiment, metric):
    """[(x, mean, half)] over one Phase 2 sweep's cells, by x: the last number in the cell's name, so "100",
    "10" and "500ms" give users per PC, the chance offline and the claim delay (D434)."""
    points = []
    for (name, cell), metrics in summary.items():
        numbers = re.findall(r"\d+", cell)
        if name != experiment or not numbers or metric not in metrics:
            continue
        mean, half, _ = metrics[metric]
        if not math.isnan(mean):
            points.append((int(numbers[-1]), mean, 0.0 if math.isnan(half) else half))
    return sorted(points)


# The Phase 2 sweeps (D434, D447): experiment, file, x axis, log x, and panels of (title, unit, series).
PHASE2_SWEEPS = (
    ("ph2users", "ph2users_scale.png", "users per client PC", True, (
        ("Report latency", "ms", (("p50_ms", "p50"), ("p95_ms", "p95"), ("p99_ms", "p99"))),
        ("Throughput", "req/s", (("throughput_rps", "answered"),)),
        ("Discovery latency p95", "ms", (("discover_client_p95_ms", "client"),)),
        ("P2P latency p95", "ms", (("p2p_latency_p95_ms", "sender"),)),
    )),
    ("ph2offline", "ph2offline_reconciliation.png", "chance offline per minute (%)", False, (
        ("Reconciliation delay", "ms", (("reconcile_mean_ms", "mean"), ("reconcile_max_ms", "max"))),
        ("Peer updates held and dropped", "per run", (("peer_updates_held", "held"),
                                                       ("peer_updates_dropped", "dropped"))),
    )),
    ("ph2conflict", "ph2conflict_claims.png", "responder's wait before claiming (ms)", False, (
        ("Conflicts per claim", "share", (("claim_conflicts_per_claim", "node"),)),
        ("Claim latency", "ms", (("claim_p50_ms", "p50"), ("claim_p95_ms", "p95"))),
    )),
)


def plot_sweep(plt, summary, out, experiment, name, xlabel, log_x, panels):
    """One Phase 2 sweep, one panel per figure, with no second y axis. Every panel has every cell on its x
    axis, so a figure a cell lacks shows as a gap, as reconciliation does with no outages. No runs, no file."""
    xs = sorted({x for _, _, series in panels for metric, _ in series
                 for x, _, _ in sweep_series(summary, experiment, metric)})
    if not xs:
        return
    if log_x:
        margin = 10 ** (0.08 * (math.log10(xs[-1] / xs[0]) or 1))
        limits = (xs[0] / margin, xs[-1] * margin)
    else:
        margin = 0.08 * ((xs[-1] - xs[0]) or 1)
        limits = (xs[0] - margin, xs[-1] + margin)
    fig, axes = plt.subplots(1, len(panels), figsize=(3.3 * len(panels), 3.2), squeeze=False)
    for ax, (title, unit, series) in zip(axes[0], panels):
        drawn = 0
        for color, (metric, label) in zip(SERIES, series):
            points = sweep_series(summary, experiment, metric)
            if not points:
                continue
            x, y, err = zip(*points)
            drawn += 1
            ax.errorbar(x, y, yerr=err, color=color, linewidth=2, elinewidth=1, marker="o", markersize=5,
                        capsize=2, label=label)
        if log_x:
            ax.set_xscale("log")
        ax.set_xticks(xs, [str(x) for x in xs])
        ax.set_xlim(*limits)
        ax.minorticks_off()
        ax.set_ylim(bottom=0)
        style(ax, title, xlabel, unit)
        if drawn > 1:
            ax.legend(frameon=False, fontsize=8)
    fig.tight_layout()
    fig.savefig(out / name, dpi=160)
    plt.close(fig)


def write_plots(summary, out):
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    plot_e1(plt, summary, out)
    plot_fairness(plt, summary, out)
    plot_phases(plt, summary, out)
    for experiment, name, xlabel, log_x, panels in PHASE2_SWEEPS:
        plot_sweep(plt, summary, out, experiment, name, xlabel, log_x, panels)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("runs", type=Path)
    parser.add_argument("--out", type=Path, default=Path("analysis/out"))
    args = parser.parse_args()
    summary = summarise(args.runs)
    if not summary:
        raise SystemExit(f"no runs found under {args.runs}")
    args.out.mkdir(parents=True, exist_ok=True)
    write_csv(summary, args.out / "summary.csv")
    write_plots(summary, args.out)
    print(f"{len(summary)} cells: wrote {args.out}/summary.csv and plots")
    for note in provisional_notes(summary):
        print(note)
    warnings = expired_warnings(summary) + udp_warnings(summary) + failover_warnings(summary)
    for warning in warnings + clock_warnings(summary):
        print(warning, file=sys.stderr)


if __name__ == "__main__":
    main()
