#!/usr/bin/env python3
"""Turn collected runs into the report's numbers and plots.

    python analysis/analyze.py runs [--out analysis/out]

Reads runs/<experiment>/<cell>/<run>/ as scripts/run-experiments.sh writes them. Each metric is
computed per run, then averaged across the cell's runs with a 95% Student's t confidence interval.
Writes summary.csv (long format, one row per cell and metric) and PNG plots to the output directory.

Durations come from `latency_us`-style fields, measured on one machine. The one exception is the
before/during/after split, which compares client timestamps with the victim's: those machines are only
as aligned as NTP makes them, so the phase edges are approximate (AGENTS.md, Measuring).
"""

import argparse
import csv
import json
import math
import re
from collections import defaultdict
from pathlib import Path

import numpy as np
from scipy import stats

USER_HZ = 100  # /proc CPU ticks per second on Ubuntu
ELECTION_AFTER_REJOIN_US = 10_000_000  # FT-6: elections this soon after a rejoin count against it
PERCENTILES = (50, 95, 99)


def read_events(run_dir):
    """Every event in a run, by source name, each source's events in file order. A program killed in the
    middle of a write leaves half a line at the end of its log: that one is dropped and named. A bad line
    anywhere else is real damage, and raises."""
    events = {}
    for path in sorted(Path(run_dir).glob("*.jsonl")):
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
    metrics["retried_share"] = retried / answered if answered else float("nan")
    metrics["no_responder_share"] = no_responder / answered if answered else float("nan")

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
    """Raft elections: new terms seen, and how long each node had no leader, leaving out the times
    it was silenced or catching up itself."""
    terms = set()
    durations_us = []
    for es in node_events(events).values():
        spans = silent_spans(es)
        lost_at = None
        for e in es:
            if e["ev"] != "raft_state" or e["ts_us"] < start_us:
                continue
            terms.add(e["term"])
            if any(a <= e["ts_us"] <= b for a, b in spans):
                lost_at = None
            elif e["leader"] is None and lost_at is None:
                lost_at = e["ts_us"]
            elif e["leader"] is not None and lost_at is not None:
                durations_us.append(e["ts_us"] - lost_at)
                lost_at = None
    return {
        "elections": max(len(terms) - 1, 0),
        "election_duration_mean_ms": float(np.mean(durations_us)) / 1000
        if durations_us
        else float("nan"),
    }


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


def fault_metrics(events, measured_requests):
    logs = node_events(events)
    def values(name, field):
        return [e[field] for es in logs.values() for e in es if e["ev"] == name]

    def mean_or_nan(xs, scale=1.0):
        return float(np.mean(xs)) / scale if xs else float("nan")

    metrics = {
        "detection_mean_ms": mean_or_nan(values("peer_suspected", "silent_ms")),
        "takeovers": len(values("takeover_started", "waited_us")),
        "takeover_wait_mean_ms": mean_or_nan(values("takeover_started", "waited_us"), 1000),
        "recovery_mean_ms": mean_or_nan(values("rejoined", "recovery_us"), 1000),
        "rejoins": len(values("rejoined", "recovery_us")),
        "elections_after_rejoin": elections_after_rejoin(events),
        "degraded_entered": len(values("degraded_entered", "held_responders")),
        "degraded_mean_ms": mean_or_nan(values("degraded_left", "degraded_ms")),
        "provisional_replied": len(values("provisional_replied", "latency_us")),
    }
    for reason in ("lost_first_commit", "responder_taken"):
        metrics[f"conflicts_{reason}"] = sum(1 for r in values("provisional_conflict", "reason") if r == reason)

    silenced = [e["ts_us"] for es in logs.values() for e in es if e["ev"] == "silenced"]
    restored = [e["ts_us"] for es in logs.values() for e in es if e["ev"] == "restored"]
    if not silenced or not restored:
        return metrics
    fault_start, fault_end = min(silenced), max(restored)
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


def message_metrics(events, start_us, completed):
    """Messages sent in the measured time, per answered request. Counters are cumulative."""
    per_kind = defaultdict(int)
    for es in node_events(events).values():
        snapshots = [e for e in es if e["ev"] == "messages" and e["ts_us"] >= start_us]
        if len(snapshots) < 2:
            continue
        first, last = snapshots[0]["sent"], snapshots[-1]["sent"]
        for kind, count in last.items():
            per_kind[kind] += count - first.get(kind, 0)
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


def commit_metrics(events, start_us):
    latencies = [
        e["latency_us"]
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
    metrics = latency_metrics("commit_", latencies)
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
    metrics.update(fault_metrics(events, done))
    metrics.update(message_metrics(events, start, len(done)))
    metrics.update(resource_metrics(events, start))
    metrics.update(commit_metrics(events, start))
    metrics.update(client_program_metrics(events, meta))
    metrics.update(client_script_metrics(run_dir))
    metrics.update(arrival_metrics(events, start))
    metrics.update(client_pc_metrics(events, start))
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
    return summary


def missing_value(name):
    """What a run with no such metric counts as. A link-drop reason that did not happen in a run is 0 there,
    and leaving that run out would average the reason over only the runs that had it."""
    return 0 if name.startswith("client_link_down_") else float("nan")


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


def plot_phases(plt, summary, out):
    cells = [
        (f"{experiment}-{cell}", metrics)
        for (experiment, cell), metrics in sorted(summary.items())
        if "during_p99_ms" in metrics
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


def write_plots(summary, out):
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    plot_e1(plt, summary, out)
    plot_fairness(plt, summary, out)
    plot_phases(plt, summary, out)


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


if __name__ == "__main__":
    main()
