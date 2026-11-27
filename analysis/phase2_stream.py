#!/usr/bin/env python3
"""Phase 2 runs on the six PCs are too big to hold in memory: at 1,000 users per client PC each program
logs about 1.4 million lines in 75 s, 98% of them one line per P2P copy (D447). This reads each log once,
in parallel, keeps every other line for analyze.py's own functions, and folds the per-copy lines (P2P
sends and receipts, gossip receipts, held updates) into counters as it goes. Every figure keeps the
meaning analyze.py and its README give it; only the P2P, gossip, held-update and P2P-rate figures are
computed here, the same way, from the streamed counters.

    python3 analysis/phase2_stream.py <root with experiment/cell/run-N> [--out DIR] [--jobs N]

Writes <out>/summary.csv (analyze.py's format) and the Phase 2 figures.
"""
import argparse
import json
import math
import sys
from collections import Counter, defaultdict
from multiprocessing import Pool
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
import analyze  # noqa: E402

# One line per P2P copy or held update: counted, never kept.
BULK = {"p2p_sent", "p2p_received", "gossip_received", "gossip_send_failed",
        "peer_update_held", "peer_update_forwarded", "peer_update_dropped"}


def event_name(line):
    i = line.find('"ev":"')
    if i < 0:
        return None
    j = line.find('"', i + 6)
    return line[i + 6:j]


def program_head(path):
    """The first lines of a client log, enough for analyze.program_start."""
    head = []
    with open(path, encoding="utf-8") as f:
        for n, line in enumerate(f):
            ev = event_name(line)
            if ev in ("client_started", "client_sent"):
                head.append(json.loads(line))
            if any(e["ev"] == "client_sent" for e in head) or n > 20000:
                break
    return head


def scan(job):
    """One log: (source, kept events, streamed counters)."""
    path, start_us, end_us = job
    source = Path(path).stem
    kept = []
    p2p = Counter()
    latencies = []
    per_update = defaultdict(lambda: [0, 0, 0, 0, 0])  # sends, copies, endpoint duplicates, forwarded, failed
    outcomes = defaultdict(Counter)
    new = []  # (update, hops, age_ms) of every copy that was new here
    held = Counter()
    with open(path, encoding="utf-8") as f:
        for line in f:
            ev = event_name(line)
            if ev is None:
                continue
            if ev not in BULK:
                try:
                    kept.append(json.loads(line))
                except json.JSONDecodeError:
                    pass  # a cut last line, as analyze.read_events allows
                continue
            try:
                e = json.loads(line)
            except json.JSONDecodeError:
                continue
            ts = e["ts_us"]
            if ts < start_us:
                continue
            if ev == "p2p_sent":
                key = analyze.update_key(e["update"])
                ack = e["ack"]
                p2p["sent"] += 1
                p2p["ack_" + ack] += 1
                if ack != "none":
                    p2p["answered"] += 1
                    p2p["retried"] += e["tries"] > 1
                if ack in ("accepted", "duplicate"):
                    latencies.append(e["latency_us"])
                if ts <= end_us:
                    p2p["sends_in_window"] += 1
                    p2p["tries_in_window"] += e["tries"]
                per_update[key][0] += 1
            elif ev == "p2p_received":
                key = analyze.update_key(e["update"])
                p2p["received"] += 1
                per_update[key][1] += 1
                per_update[key][2] += e.get("outcome") == "duplicate"
            elif ev == "gossip_received":
                key = analyze.update_key(e["update"])
                outcomes[key][e["outcome"]] += 1
                per_update[key][3] += e["forwarded"]
                if e["outcome"] == "new":
                    new.append((key, e["hops"], e["age_ms"]))
            elif ev == "gossip_send_failed":
                per_update[analyze.update_key(e["update"])][4] += 1
            elif ev == "peer_update_held":
                held["held"] += 1
            elif ev == "peer_update_forwarded":
                held["forwarded"] += 1
            elif ev == "peer_update_dropped":
                held["dropped"] += 1
                held["dropped_" + str(e.get("reason"))] += 1
    return source, kept, dict(p2p), latencies, dict(per_update), {k: dict(v) for k, v in outcomes.items()}, new, dict(held)


def streamed_p2p(p2p, latencies, clients):
    sent = p2p.get("sent", 0)
    metrics = {"p2p_sent": sent}
    for ack, name in analyze.P2P_ACKS.items():
        metrics[f"p2p_{name}"] = p2p.get("ack_" + ack, 0)
    metrics["p2p_ack_share"] = analyze.ratio(p2p.get("answered", 0), sent)
    metrics["p2p_retried_share"] = analyze.ratio(p2p.get("retried", 0), p2p.get("answered", 0))
    metrics.update(analyze.latency_metrics("p2p_latency_", latencies))
    metrics["p2p_received"] = p2p.get("received", 0)
    metrics["p2p_endpoints"] = analyze.endpoints(clients)
    return metrics


def streamed_gossip(clients, start_us, per_update, outcomes, new_by_source):
    """analyze.gossip_metrics, from the streamed counters."""
    published = {}
    for es in clients.values():
        for e in es:
            if e["ev"] == "gossip_published" and e["ts_us"] >= start_us:
                published[analyze.update_key(e["update"])] = e
    totals = Counter()
    forwarded = copies = endpoint_duplicates = sends = failed = 0
    for key, (s, c, d, fw, fl) in per_update.items():
        if key not in published:
            continue
        sends += s
        copies += c
        endpoint_duplicates += d
        forwarded += fw
        failed += fl
    for key, counts in outcomes.items():
        if key in published:
            totals.update(counts)
    reached = Counter()
    hops, ages = [], []
    for source, new in new_by_source.items():
        for key, h, age in new:
            if key in published:
                reached[key] += 1
                hops.append(h)
                ages.append(age * 1000)
    updates = len(published)
    metrics = {
        "gossip_published": updates,
        "gossip_published_no_targets": sum(1 for e in published.values() if e["targets"] == 0),
    }
    for outcome in ("new", "duplicate", "obsolete", "not_subscribed"):
        metrics[f"gossip_received_{outcome}"] = totals[outcome]
    duplicates = endpoint_duplicates + totals["duplicate"]
    metrics["gossip_copies_received"] = copies
    metrics["gossip_duplicates"] = duplicates
    metrics["gossip_duplicate_share"] = analyze.ratio(duplicates, copies)
    metrics["gossip_duplicates_per_new"] = analyze.ratio(duplicates, totals["new"])
    chosen = sum(e["targets"] for e in published.values()) + forwarded
    metrics["gossip_copies_per_update"] = analyze.ratio(chosen, updates)
    metrics["gossip_sends_per_update"] = analyze.ratio(sends, updates)
    metrics["gossip_send_failed"] = failed
    reach = [reached[key] for key in published]
    others = analyze.endpoints(clients) - 1
    metrics["gossip_reach_mean"] = float(np.mean(reach)) if reach else float("nan")
    metrics["gossip_coverage_share"] = float(np.mean(reach)) / others if reach and others > 0 else float("nan")
    metrics["gossip_full_coverage_share"] = (
        analyze.ratio(sum(1 for r in reach if r >= others), updates) if others > 0 else float("nan"))
    at_hops = Counter(hops)
    metrics["gossip_hops_mean"] = float(np.mean(hops)) if hops else float("nan")
    metrics["gossip_hops_max"] = max(hops) if hops else float("nan")
    for h in analyze.GOSSIP_HOPS:
        metrics[f"gossip_hops_{h}_share"] = analyze.ratio(at_hops[h], len(hops))
    metrics.update(analyze.latency_metrics("gossip_age_", ages))
    lists = analyze.events_named(clients, ("gossip_zone_refreshed",), start_us)["gossip_zone_refreshed"]
    metrics["gossip_zone_programs_max"] = max((e["programs"] for e in lists), default=0)
    return metrics


def analyse_run(run_dir, pool):
    meta = json.loads((run_dir / "run.json").read_text(encoding="utf-8"))
    warmup_s, measured_s = meta["warmup_s"], meta["measured_s"]
    logs = sorted(p for p in run_dir.glob("*.jsonl")
                  if p.name.startswith(("node-", "client-", "frontend-")))
    starts = [analyze.program_start(program_head(p)) for p in logs if p.name.startswith("client-")]
    starts = [s for s in starts if s is not None]
    start = min(starts) + warmup_s * 1_000_000 if starts else 0
    end = start + measured_s * 1_000_000
    events, p2p, latencies, per_update, outcomes, new_by_source, held = {}, Counter(), [], {}, {}, {}, Counter()
    for source, kept, p, lat, upd, outs, new, h in pool.imap_unordered(scan, [(str(p), start, end) for p in logs]):
        events[source] = kept
        p2p.update(p)
        latencies.extend(lat)
        for key, v in upd.items():
            if key in per_update:
                per_update[key] = [a + b for a, b in zip(per_update[key], v)]
            else:
                per_update[key] = v
        for key, counts in outs.items():
            outcomes.setdefault(key, Counter()).update(counts)
        new_by_source[source] = new
        held.update(h)
    clients = analyze.client_sources(events)
    done, _ = analyze.client_requests(events, warmup_s)
    metrics = analyze.request_metrics(events, warmup_s, measured_s)
    metrics.update(analyze.bid_metrics(events, start))
    metrics.update(analyze.clock_skew_metrics(events))
    metrics.update(analyze.refusal_metrics(events, start))
    metrics.update(analyze.convergence_metrics(events))
    metrics.update(analyze.presence_metrics(events, start))
    metrics.update(streamed_p2p(p2p, latencies, clients))
    metrics.update(streamed_gossip(clients, start, per_update, outcomes, new_by_source))
    offline = analyze.offline_metrics(events, start)
    offline["peer_updates_held"] = held["held"]
    offline["peer_updates_forwarded"] = held["forwarded"]
    offline["peer_updates_dropped"] = held["dropped"]
    for reason in analyze.HELD_DROP_REASONS:
        offline[f"peer_updates_dropped_{reason}"] = held["dropped_" + reason]
    offline["peer_updates_held_at_end"] = float("nan")  # needs every held line in order; not streamed
    metrics.update(offline)
    metrics.update(analyze.claims_metrics(events, start))
    metrics.update(analyze.workload_metrics(events, measured_s))
    cloud = analyze.cloud_bytes_metrics(events, start, end)
    cloud["p2p_sends_per_s"] = p2p["sends_in_window"] / measured_s if measured_s else float("nan")
    cloud["p2p_tries_per_s"] = p2p["tries_in_window"] / measured_s if measured_s else float("nan")
    metrics.update(cloud)
    metrics.update(analyze.message_metrics(events, start, len(done)))
    metrics.update(analyze.resource_metrics(events, start))
    metrics.update(analyze.udp_metrics(events, start, len(done)))
    metrics.update(analyze.commit_metrics(events, start))
    metrics.update(analyze.client_program_metrics(events, meta))
    metrics.update(analyze.client_script_metrics(run_dir))
    metrics.update(analyze.client_pc_metrics(events, start))
    metrics["node_logs"] = len(analyze.node_events(events))
    return meta, metrics


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("runs", type=Path)
    parser.add_argument("--out", type=Path, default=Path("analysis/out-phase2"))
    parser.add_argument("--jobs", type=int, default=12)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    per_cell = defaultdict(list)
    cache = args.out / "runs"
    cache.mkdir(exist_ok=True)
    with Pool(args.jobs, maxtasksperchild=1) as pool:
        for meta_path in sorted(args.runs.glob("*/*/*/run.json")):
            run = meta_path.parent
            # Each run's figures are kept as it finishes, so a stopped analysis resumes where it was.
            saved = cache / f"{run.parent.parent.name}-{run.parent.name}-{run.name}.json"
            if saved.exists():
                meta, metrics = json.loads(saved.read_text(encoding="utf-8"))
            else:
                meta, metrics = analyse_run(run, pool)
                saved.write_text(json.dumps([meta, metrics], default=float), encoding="utf-8")
            problem = analyze.run_problem(meta, metrics)
            if problem:
                print(f"left out {meta_path.parent}: {problem}", file=sys.stderr)
                continue
            per_cell[(meta["experiment"], meta["cell"])].append(metrics)
            print(f"{run}: {metrics.get('count', 0)} answered, "
                  f"{metrics.get('p2p_sent', 0)} P2P sends", flush=True)
    summary = {}
    for cell, runs in per_cell.items():
        names = sorted({name for run in runs for name in run})
        summary[cell] = {name: analyze.confidence_interval(
            [run.get(name, analyze.missing_value(name)) for run in runs]) for name in names}
    analyze.write_csv(summary, args.out / "summary.csv")
    analyze.write_plots(summary, args.out)
    print(f"{len(summary)} cells: wrote {args.out}/summary.csv and plots")


if __name__ == "__main__":
    main()
