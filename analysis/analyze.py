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
import sys
from collections import defaultdict
from pathlib import Path

import numpy as np
from scipy import stats

USER_HZ = 100  # /proc CPU ticks per second on Ubuntu
UDP_RCVBUF_ASKED_BYTES = 8 * 1024 * 1024  # node/datagram.rs: what every UDP socket asks for (D171)
ELECTION_AFTER_REJOIN_US = 10_000_000  # FT-6: elections this soon after a rejoin count against it
PERCENTILES = (50, 95, 99)


def read_events(run_dir):
    """Every event in a run, by source name, each source's events in file order."""
    events = {}
    for path in sorted(Path(run_dir).glob("*.jsonl")):
        with open(path, encoding="utf-8") as lines:
            events[path.stem] = [json.loads(line) for line in lines if line.strip()]
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


def client_requests(events, warmup_s):
    """Each client's measured `client_done` and `client_gave_up` events: started after warm-up,
    judged on that client's own clock."""
    done, gave_up = [], []
    for source, source_events in events.items():
        if not source.startswith("client-"):
            continue
        sent = [e["ts_us"] for e in source_events if e["ev"] == "client_sent"]
        if not sent:
            continue
        measure_from = min(sent) + warmup_s * 1_000_000
        for e in source_events:
            if e["ev"] == "client_done" and e["ts_us"] - e["latency_us"] >= measure_from:
                done.append(e)
            elif e["ev"] == "client_gave_up" and e["ts_us"] >= measure_from:
                gave_up.append(e)
    return done, gave_up


def node_events(events):
    return {s: es for s, es in events.items() if s.startswith("node-")}


def measured_start(events, warmup_s):
    """Roughly when measuring began, for node events: machines agree only as far as NTP does."""
    sent = [
        e["ts_us"]
        for s, es in events.items()
        if s.startswith("client-")
        for e in es
        if e["ev"] == "client_sent"
    ]
    return min(sent) + warmup_s * 1_000_000 if sent else 0


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


def udp_metrics(events, start_us, completed):
    """What the UDP client path carried and lost in the measured time (D172). The counters are
    cumulative per process, so each is its last value minus its first.

    Cancels are outside the msgs_ figures, which count what nodes send, so this is where they show.
    Each process's samples start at its own clock's reading of `start_us`, so the two sides of a loss
    share cover slightly different windows. Treat it as approximate."""
    totals = {"client": defaultdict(int), "node": defaultdict(int)}
    drops = {"client": [], "node": []}
    rcvbufs = []
    for source, es in events.items():
        side = "client" if source.startswith("client-") else "node"
        rcvbufs += [e["rcvbuf_got_bytes"] for e in es if e["ev"] == "udp_socket"]
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
    if client["tx_requests"]:
        arrived = (node["rx_requests"] / client["tx_requests"])
        metrics["udp_request_loss_share"] = max(1 - arrived, 0.0)
    if client["tx_cancels"]:
        arrived = node["rx_cancels"] / client["tx_cancels"]
        metrics["udp_cancel_loss_share"] = max(1 - arrived, 0.0)
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
    metrics.update(udp_metrics(events, start, len(done)))
    metrics.update(commit_metrics(events, start))
    return meta, metrics


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
        meta, metrics = analyse_run(meta_path.parent)
        per_cell[(meta["experiment"], meta["cell"])].append(metrics)
    summary = {}
    for key, runs in per_cell.items():
        names = sorted({name for run in runs for name in run})
        summary[key] = {
            name: confidence_interval([run.get(name, float("nan")) for run in runs])
            for name in names
        }
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
    for warning in expired_warnings(summary) + udp_warnings(summary):
        print(warning, file=sys.stderr)


if __name__ == "__main__":
    main()
