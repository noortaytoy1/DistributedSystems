#!/usr/bin/env python3
# kills_analyze.py [run folders]: every measure of the final runs (D261): kills per run, step and stage, and the
# other runs per run. Writes analysis/results/kills/{runs,steps,phases,others}.csv. Never run it while a test is running.
import collections, csv, datetime, glob, gzip, json, multiprocessing, os, re, statistics, sys

OUT = "analysis/results/kills"
ANSI = re.compile(r"\x1b\[[0-9;]*m")
TOLD = {
    "rebuilt": "rebuilt from the log file",
    "taken_back": "taking a node back in, alone",
    "member_again": "back as a member",
    "went_first": "all were gone: going first",
    "alone": "on the way back with nobody reachable",
    "dropped": "dropped a silent member",
    "started_again": "what this node holds is not the log",
}


def lines(path):
    with gzip.open(path, "rt", errors="replace") as f:
        for line in f:
            try:
                yield json.loads(line)
            except ValueError:
                pass


def pct(sorted_values, q):
    if not sorted_values:
        return None
    return sorted_values[min(len(sorted_values) - 1, int(q * len(sorted_values)))] / 1000


def key(request):
    return (request["client"] << 53) + request["seq"]


def read_out(path):
    """What a node told its .out file, with the time of each line."""
    found = []
    if not os.path.exists(path):
        return found
    for line in open(path, errors="replace"):
        text = ANSI.sub("", line)
        for name, words in TOLD.items():
            if words in text:
                try:
                    stamp = datetime.datetime.fromisoformat(text.split()[0].replace("Z", "+00:00"))
                except ValueError:
                    continue
                found.append((int(stamp.timestamp() * 1e6), name, text.strip()[:200]))
    return found


def commit_of(run):
    return open(f"{run}/commit.txt").read().strip()[:7] if os.path.exists(f"{run}/commit.txt") else ""


def node_measures(run, r, conflicts_by=None):
    """CPU, memory, network, queues and digests, from every node's log, into r."""
    checkpoints = collections.defaultdict(lambda: collections.defaultdict(set))
    reasons = collections.Counter()
    replied = 0
    for path in sorted(glob.glob(f"{run}/node-*.jsonl.slim.gz")):
        node = int(re.search(r"node-(\d+)", path).group(1))
        spans, queued, running = [], 0, 0
        for e in lines(path):
            ev = e.get("ev")
            if ev == "state_checkpoint":
                checkpoints[e["log_index"]][node].add(e["digest"])
            elif ev == "resource_usage":
                spans.append((e["ts_us"], e["cpu_ticks"], e["net_rx_bytes"], e["net_tx_bytes"], e["rss_kb"], e.get("udp_drops", 0)))
                queued, running = max(queued, e.get("queued", 0)), max(running, e.get("running", 0))
            elif ev == "provisional_conflict":
                reasons[e.get("reason", "?")] += 1
            elif ev == "provisional_replied":
                replied += 1
        busy, bytes_per_s = [], []
        for a, b in zip(spans, spans[1:]):
            seconds = (b[0] - a[0]) / 1e6
            if seconds > 0 and b[1] >= a[1]:
                busy.append((b[1] - a[1]) / 100 / seconds)
                bytes_per_s.append((b[2] - a[2] + b[3] - a[3]) / seconds)
        r[f"cores_mean_{node}"] = round(statistics.mean(busy), 2) if busy else ""
        r[f"cores_max_{node}"] = round(max(busy), 1) if busy else ""
        r[f"net_mb_per_s_mean_{node}"] = round(statistics.mean(bytes_per_s) / 1e6, 1) if bytes_per_s else ""
        r[f"max_rss_mb_{node}"] = max((s[4] for s in spans), default=0) // 1024
        r[f"queued_max_{node}"] = queued
        r[f"running_max_{node}"] = running
        r[f"udp_drops_{node}"] = sum(max(0, b[5] - a[5]) for a, b in zip(spans, spans[1:]))
    mismatched = [i for i, by in checkpoints.items() if len(set().union(*by.values())) > 1]
    r.update(checkpoints=len(checkpoints), digest_mismatches=len(mismatched), first_mismatch=min(mismatched) if mismatched else "",
             provisional_replied_lines=replied, conflicts_total=sum(reasons.values()),
             conflicts_responder_taken=reasons["responder_taken"], conflicts_lost_first_commit=reasons["lost_first_commit"])


def final_status(run):
    status = open(f"{run}/status-after.txt", errors="replace").read() if os.path.exists(f"{run}/status-after.txt") else ""
    rows = [line.split() for line in status.splitlines() if re.match(r"\s*[123]\s+peer", line)]
    return dict(final_digests_differ=len({row[6] for row in rows}) > 1 if rows else "", final_results=rows[0][7] if rows else "",
                final_applied_differ=max(int(row[5]) for row in rows) - min(int(row[5]) for row in rows) if rows else "")


def analyse_other(run):
    """A run with no kill plan: speed, users, flat out, spike, T1, T2, T4."""
    parts = run.rstrip("/").split("/")
    meta = json.load(open(f"{run}/run.json"))
    r = dict(experiment=parts[-3], cell=parts[-2], run=int(parts[-1].split("-")[1]), commit=commit_of(run), warmup_s=meta["warmup_s"], measured_s=meta["measured_s"], users=meta.get("users", 0))
    sent, done, gave = set(), set(), set()
    provisional = retries = first_try = skipped = no_responder = 0
    lat, started = [], None
    for path in sorted(glob.glob(f"{run}/client-*.jsonl.gz")):
        for e in lines(path):
            ev = e.get("ev")
            if ev == "client_started":
                started = min(started or e["ts_us"], e["ts_us"])
            elif ev == "client_sent":
                if e["attempt"] == 0:
                    sent.add(key(e["request"]))
                else:
                    retries += 1
            elif ev == "client_done":
                done.add(key(e["request"]))
                lat.append(e["latency_us"])
                provisional += bool(e.get("provisional"))
                no_responder += bool(e.get("no_responder"))
                first_try += e["attempts"] == 1
            elif ev == "client_gave_up":
                gave.add(key(e["request"]))
            elif ev == "client_retries_skipped":
                skipped += e["skipped"]
    lat.sort()
    r.update(sent=len(sent), answered=len(done), provisional=provisional, no_responder=no_responder, gave_up=len(gave), unaccounted=len(sent - done - gave),
             retries=retries, retries_skipped=skipped, first_try_pct=round(100 * first_try / max(1, len(lat)), 2),
             answered_per_s=round(len(done) / max(1, meta["measured_s"] + meta["warmup_s"])),
             mean_ms=round(statistics.mean(lat) / 1000, 1) if lat else "", p50_ms=pct(lat, 0.5), p95_ms=pct(lat, 0.95), p99_ms=pct(lat, 0.99), max_ms=lat[-1] / 1000 if lat else "")
    node_measures(run, r)
    r.update(final_status(run))
    return r


def analyse(run):
    parts = run.rstrip("/").split("/")
    case, rate, rep = parts[-3].removeprefix("kill"), int(parts[-2]), int(parts[-1].split("-")[1])
    meta = json.load(open(f"{run}/run.json"))
    warm = meta["warmup_s"]
    steps, t0, converged = [], None, ""
    for line in open(f"{run}/fault-times.txt", errors="replace"):
        w = line.split()
        if len(w) >= 2 and w[0] == "start":
            t0 = int(w[1])
        elif len(w) >= 4 and w[0] in ("kill", "back"):
            steps.append((int(w[1]), w[0], [int(n) for n in w[3].split(",")]))
        elif "converged" in line:
            converged = line.strip()
    steps.sort()
    config = open(f"{run}/config.toml", errors="replace").read()
    ips = {i + 1: a for i, a in enumerate(re.findall(r'peer_addr = "([\d.]+):', config))}

    sent, gave_up = set(), set()
    done, committed, provisional = set(), set(), set()
    retries = attempts_one = 0
    answers = []  # (time, latency, provisional, from)
    gave_times = []
    for path in sorted(glob.glob(f"{run}/client-*.jsonl.gz")):
        for e in lines(path):
            ev = e.get("ev")
            if ev == "client_sent":
                if e["attempt"] == 0:
                    sent.add(key(e["request"]))
                else:
                    retries += 1
            elif ev == "client_done":
                k = key(e["request"])
                done.add(k)
                (provisional if e.get("provisional") else committed).add(k)
                answers.append((e["ts_us"], e["latency_us"], bool(e.get("provisional")), e["from"]))
                attempts_one += e["attempts"] == 1
            elif ev == "client_gave_up":
                gave_up.add(key(e["request"]))
                gave_times.append(e["ts_us"])
    answers.sort()
    both = len(provisional & committed)
    latencies = sorted(a[1] for a in answers)
    r = dict(case=case, rate=rate, run=rep, commit=commit_of(run), steps=" ".join(f"{(t - t0) // 1000000}:{w}:{','.join(map(str, n))}" for t, w, n in steps))
    r.update(sent=len(sent), answered=len(done), committed=len(committed), provisional=len(provisional),
             both=both, gave_up=len(gave_up), unaccounted=len(sent - done - gave_up),
             retries=retries, first_try_pct=round(100 * attempts_one / max(1, len(answers)), 2),
             p50_ms=pct(latencies, 0.5), p95_ms=pct(latencies, 0.95), p99_ms=pct(latencies, 0.99),
             max_ms=latencies[-1] / 1000 if latencies else None)
    last_answer = answers[-1][0] if answers else t0
    r["answered_per_s_whole_run"] = round(len(answers) / max(1, (last_answer - t0) / 1e6))

    # The longest time with no answer of any kind, and with no committed answer, after the warm-up.
    def longest_gap(items):
        best, at, prev = 0, None, t0 + warm * 1000000
        for t in items:
            if t - prev > best:
                best, at = t - prev, prev
            prev = t
        return round(best / 1e6, 2), None if at is None else round((at - t0) / 1e6, 1)

    r["longest_no_answer_s"], r["longest_no_answer_from_s"] = longest_gap([a[0] for a in answers if a[0] >= t0 + warm * 1000000])
    r["longest_no_commit_s"], r["longest_no_commit_from_s"] = longest_gap([a[0] for a in answers if not a[2] and a[0] >= t0 + warm * 1000000])

    # Nodes.
    node_measures(run, r)
    r.update(final_status(run))
    r["retries_skipped"] = sum(e.get("skipped", 0) for path in glob.glob(f"{run}/client-*.jsonl.gz") for e in lines(path) if e.get("ev") == "client_retries_skipped")
    told, rss, usage, digests = {}, {}, {}, {}
    events = collections.defaultdict(list)
    for path in sorted(glob.glob(f"{run}/node-*.jsonl.slim.gz")):
        node = int(re.search(r"node-(\d+)", path).group(1))
        told[node] = read_out(f"{run}/node-{node}.out")
        rss[node] = 0
        usage[node] = []
        digests[node] = []
        for e in lines(path):
            ev, ts = e.get("ev"), e.get("ts_us", 0)
            if ev == "state_digest":
                digests[node].append((ts, e["last_applied"] or 0))
            elif ev in ("restarted", "rejoined", "rejoin_started", "peer_reconnected", "peer_unreachable", "degraded_entered", "degraded_left", "raft_state"):
                events[node].append((ts, ev, e))
    words = collections.Counter(name for node in told for _, name, _ in told[node])
    for name in TOLD:
        r[f"told_{name}"] = words[name]

    r["converged"] = converged
    for node in sorted(rss):
        path = f"{run}/applied-ids-{node}.txt.gz"
        if not os.path.exists(path):
            r[f"file_reports_{node}"] = ""
            continue
        applied = set()
        with gzip.open(path, "rt") as f:
            for line in f:
                c, s = line.split()
                applied.add((int(c) << 53) + int(s))
        r[f"file_reports_{node}"] = len(applied)
        r[f"answered_committed_not_in_file_{node}"] = len(committed - applied)
        r[f"answered_provisional_not_in_file_{node}"] = len(provisional - applied)
        stats = open(f"{run}/applied-{node}.txt", errors="replace").read().strip()
        r[f"file_stats_{node}"] = stats
        r[f"file_order_{node}"] = (re.search(r"order=(\w+)", stats) or [None, ""])[1]
        del applied

    # Each step: what clients and nodes saw after it.
    step_rows = []
    down = set()
    for at, (t, what, who) in enumerate(steps):
        down = (down | set(who)) if what == "kill" else (down - set(who))
        row = dict(case=case, rate=rate, run=rep, step=at + 1, at_s=round((t - t0) / 1e6, 1), what=what, nodes=",".join(map(str, who)), nodes_up=3 - len(down))
        after = [a for a in answers if a[0] >= t]
        row["first_answer_s"] = round((after[0][0] - t) / 1e6, 2) if after else None
        first_commit = next((a for a in after if not a[2]), None)
        row["first_commit_s"] = round((first_commit[0] - t) / 1e6, 2) if first_commit else None
        window = [a for a in after if a[0] < t + 5000000]
        row["answers_per_s_next_5s"] = round(len(window) / 5)
        # From 1 s on: the time is noted just before the kill goes over SSH, and answers already on their way still land.
        row["dead_answering"] = sum(1 for a in after if a[3] in down and a[0] >= t + 1000000 and (at + 1 == len(steps) or a[0] < steps[at + 1][0]))
        for node in who:
            if what != "back":
                continue
            for ts, ev, e in events[node]:
                if ts >= t and ev == "rejoined":
                    row[f"serving_again_s_{node}"] = round((ts - t) / 1e6, 2)
                    break
            for ts, name, _ in told.get(node, []):
                if ts >= t and name == "rebuilt":
                    row[f"rebuilt_s_{node}"] = round((ts - t) / 1e6, 2)
                    break
            for ts, name, _ in told.get(node, []):
                if ts >= t and name in ("member_again", "went_first"):
                    row[f"member_again_s_{node}"] = round((ts - t) / 1e6, 2)
                    break
            # TCP: when each node up first shows a connection to this one again, at 1 s resolution.
            for other in sorted(set(ips) - {node}):
                path = f"{run}/tcp-peers-{other}.txt"
                if not os.path.exists(path):
                    continue
                for line in open(path, errors="replace"):
                    w = line.split()
                    if w and int(w[0]) * 1000000 >= t and any(x.startswith(ips[node] + "=") for x in w[1:]):
                        row[f"tcp_{other}_to_{node}_s"] = int(w[0]) - t // 1000000
                        break
            for other in sorted(set(ips) - {node}):
                for ts, ev, e in events[other]:
                    if ts >= t and ev == "peer_reconnected" and e.get("peer") == node:
                        row[f"reconnect_{other}_to_{node}_s"] = round((ts - t) / 1e6, 2)
                        break
        step_rows.append(row)

    # Phases: between steps, what clients got.
    phase_rows = []
    edges = [t0 + warm * 1000000] + [s[0] for s in steps] + [answers[-1][0] + 1 if answers else t0]
    down = set()
    gave_times.sort()
    for i in range(len(edges) - 1):
        if i > 0:
            t, what, who = steps[i - 1]
            down = (down | set(who)) if what == "kill" else (down - set(who))
        a, b = edges[i], edges[i + 1]
        part = [x for x in answers if a <= x[0] < b]
        lat = sorted(x[1] for x in part)
        seconds = max(0.001, (b - a) / 1e6)
        phase_rows.append(dict(case=case, rate=rate, run=rep, phase=i + 1, from_s=round((a - t0) / 1e6, 1), to_s=round((b - t0) / 1e6, 1),
                               nodes_up=3 - len(down), answers_per_s=round(len(part) / seconds), committed_per_s=round(sum(not x[2] for x in part) / seconds),
                               provisional_per_s=round(sum(x[2] for x in part) / seconds), gave_up=sum(a <= t < b for t in gave_times),
                               p50_ms=pct(lat, 0.5), p99_ms=pct(lat, 0.99), max_ms=lat[-1] / 1000 if lat else None,
                               by_node=" ".join(f"{n}:{sum(x[3] == n for x in part)}" for n in (1, 2, 3))))
    return r, step_rows, phase_rows


def write(path, rows):
    names = []
    for row in rows:
        for name in row:
            if name not in names:
                names.append(name)
    with open(path, "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=names)
        w.writeheader()
        w.writerows(rows)


def one(run):
    try:
        return analyse(run) if "/kill" in run else analyse_other(run)
    except Exception as e:  # A broken run is reported, and never skipped silently.
        return ("broken", run, repr(e))


def main():
    runs = sys.argv[1:]
    if not runs:
        runs = sorted(glob.glob("runs/exp8h/kill*/*/run-*"))
        for name in ("final0", "final1", "users", "stress", "power", "oneleft", "draw"):
            runs += sorted(glob.glob(f"runs/exp8h/{name}/*/run-*"))
    os.makedirs(OUT, exist_ok=True)
    all_runs, all_steps, all_phases, others = [], [], [], []
    with multiprocessing.Pool(6, maxtasksperchild=1) as pool:
        for result in pool.imap(one, runs):
            if isinstance(result, tuple) and result[0] == "broken":
                print(f"{result[1]}: could not read: {result[2]}")
            elif isinstance(result, tuple):
                r, s, p = result
                all_runs.append(r); all_steps += s; all_phases += p
                print(f"{r['case']:5} {r['rate']:6} run {r['run']} {r['commit']}: sent {r['sent']} answered {r['answered']} (provisional {r['provisional']}) gave up {r['gave_up']} "
                      f"unaccounted {r['unaccounted']} mismatches {r['digest_mismatches']} of {r['checkpoints']} p99 {r['p99_ms']} no-answer {r['longest_no_answer_s']}s no-commit {r['longest_no_commit_s']}s")
            else:
                others.append(result)
                print(f"{result['experiment']} {result['cell']} run {result['run']} {result['commit']}: sent {result['sent']} answered {result['answered']} gave up {result['gave_up']} unaccounted {result['unaccounted']} mismatches {result['digest_mismatches']}")
    write(f"{OUT}/runs.csv", all_runs); write(f"{OUT}/steps.csv", all_steps); write(f"{OUT}/phases.csv", all_phases); write(f"{OUT}/others.csv", others)


if __name__ == "__main__":
    main()
