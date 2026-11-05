#!/usr/bin/env python3
"""phase1_analyze.py <runs/phase1> <out dir>: per-run figures and per-cell statistics for scenarios 1, 2, 3, 4 and 9.

Every number comes from the logs: client-*.jsonl.gz (every request), node-*.slim.gz (every event that is not
per request, 1 in 50 of those that are, and all of them around a fault), fault-times.txt and status-after.txt.
"""
import collections, csv, glob, gzip, json, math, multiprocessing, os, re, statistics, sys

T95 = {1: 12.706, 2: 4.303, 3: 3.182, 4: 2.776, 5: 2.571, 6: 2.447, 7: 2.365, 8: 2.306, 9: 2.262}


def lines(path):
    with gzip.open(path, "rt", errors="replace") as f:
        for line in f:
            try:
                yield json.loads(line)
            except ValueError:
                continue


def pct(sorted_values, q):
    if not sorted_values:
        return None
    return sorted_values[min(len(sorted_values) - 1, int(len(sorted_values) * q))]


def jain(counts):
    counts = list(counts)
    if not counts or not sum(counts):
        return None
    return sum(counts) ** 2 / (len(counts) * sum(c * c for c in counts))


def phase_stats(records, a, b):
    """Answers finished between two times: rate and latency."""
    xs = sorted(lat for t, lat in records if a <= t < b)
    secs = max((b - a) / 1e6, 1e-9)
    return {
        "per_s": round(len(xs) / secs, 1),
        "p50_ms": round(pct(xs, 0.5) / 1000, 1) if xs else None,
        "p99_ms": round(pct(xs, 0.99) / 1000, 1) if xs else None,
    }


def analyse(run):
    r = {"path": run}
    meta = json.load(open(f"{run}/run.json"))
    warm, measured = meta["warmup_s"], meta["measured_s"]
    r.update(experiment=meta["experiment"], cell=meta["cell"], run=meta["run"], warmup_s=warm, measured_s=measured)
    times = {}
    fault = None
    for line in open(f"{run}/fault-times.txt", errors="replace"):
        w = line.split()
        if len(w) >= 2 and w[1].isdigit():
            times[w[0]] = int(w[1])
            if w[0] in ("silence", "kill", "skew_on"):
                fault = (w[0], int(w[1]), int(w[3]))
                if w[0] == "silence":
                    fault += (int(w[5]),)
        if line.startswith("converged"):
            r["converged_after_load_s"] = int(line.split()[1])
        if line.startswith("not converged"):
            r["converged_after_load_s"] = None
    t0 = times["start"]
    r["fault"] = fault[0] if fault else ""

    # Clients: every request.
    done, sent, gave_up, retried_attempts = [], 0, 0, 0
    sent_sec = collections.Counter()
    by_prog = collections.Counter()
    by_exec, by_from = collections.Counter(), collections.Counter()
    per_second = collections.Counter()
    exec_window = collections.defaultdict(collections.Counter)
    pc_lat = collections.defaultdict(list)
    client_pcs = {}
    node_pcs = {}
    attempts = collections.Counter()
    no_responder = provisional = 0
    sent_times = {}
    for path in sorted(glob.glob(f"{run}/client-*.jsonl.gz")):
        program = int(re.search(r"client-(\d+)", path).group(1))
        pc = (program - 1) // 3 + 1
        for e in lines(path):
            ev = e.get("ev")
            if ev == "client_started":
                client_pcs[program] = f"{e.get('host', '?')} {e.get('ip', '?')}"
            elif ev == "client_sent":
                if e["attempt"] == 0:
                    sent += 1
                    sent_sec[e["ts_us"] // 1_000_000] += 1
                    if fault:
                        sent_times[(program, e["request"]["seq"])] = e["ts_us"]
                else:
                    retried_attempts += 1
            elif ev == "client_done":
                t = e["ts_us"]
                lat = e["latency_us"]
                done.append((t, lat))
                sec = (t - t0) // 1_000_000
                per_second[sec] += 1
                if t >= t0 + warm * 1_000_000:
                    by_prog[program] += 1
                    by_exec[e["executor"]] += 1
                    by_from[e["from"]] += 1
                    pc_lat[pc].append(lat)
                exec_window[sec // 10][e["executor"]] += 1
                attempts[e["attempts"]] += 1
                no_responder += bool(e.get("no_responder"))
                provisional += bool(e.get("provisional"))
            elif ev == "client_gave_up":
                gave_up += 1
    a, b = t0 + warm * 1_000_000, t0 + (warm + measured) * 1_000_000
    xs = sorted(lat for t, lat in done if a <= t < b)
    sent_in_window = sum(sent_sec[s] for s in sent_sec if a <= s * 1_000_000 < b)
    r.update(
        sent=sent, done=len(done), gave_up=gave_up, unaccounted=sent - len(done) - gave_up,
        retries=retried_attempts, no_responder=no_responder, provisional=provisional,
        measured_answers=len(xs), answered_per_s=round(len(xs) / measured, 1),
        offered_per_s=round(sent_in_window / measured, 1),
        mean_ms=round(sum(xs) / len(xs) / 1000, 1) if xs else None,
        p50_ms=round(pct(xs, 0.5) / 1000, 1) if xs else None,
        p95_ms=round(pct(xs, 0.95) / 1000, 1) if xs else None,
        p99_ms=round(pct(xs, 0.99) / 1000, 1) if xs else None,
        max_ms=round(xs[-1] / 1000, 1) if xs else None,
        first_try_share=round(attempts[1] / max(sum(attempts.values()), 1), 4),
        jain_executor=round(jain(by_exec.values()) or 0, 4),
    )
    for n in (1, 2, 3):
        r[f"exec_share_{n}"] = round(by_exec[n] / max(sum(by_exec.values()), 1), 4)
        r[f"reply_share_{n}"] = round(by_from[n] / max(sum(by_from.values()), 1), 4)
    for pc in (1, 2, 3):
        v = sorted(pc_lat.get(pc, []))
        r[f"pc{pc}_answers"] = len(v)
        r[f"pc{pc}_p50_ms"] = round(pct(v, 0.5) / 1000, 1) if v else None
        r[f"pc{pc}_p99_ms"] = round(pct(v, 0.99) / 1000, 1) if v else None
    r["programs"] = len(by_prog)
    r["timeline"] = [per_second.get(s, 0) for s in range(0, warm + measured + 15)]
    r["exec_windows"] = {str(k * 10): dict(v) for k, v in sorted(exec_window.items())}

    # Nodes.
    cp = collections.defaultdict(dict)
    bids, won = [], collections.Counter()
    seen = collections.Counter()
    scores = collections.defaultdict(lambda: collections.defaultdict(list))
    msgs, usage = {}, collections.defaultdict(list)
    counts = collections.Counter()
    views = collections.defaultdict(list)
    silenced, restored, rejoined = [], [], []
    elections = []
    elected_victim = set()
    for path in sorted(glob.glob(f"{run}/node-*.slim.gz")):
        node = int(re.search(r"node-(\d+)", path).group(1))
        for e in lines(path):
            ev = e.get("ev")
            ts = e.get("ts_us", 0)
            if ev == "state_checkpoint":
                cp[e["log_index"]][node] = e["digest"]
            elif ev == "bid_round_closed":
                if fault and fault[0] in ("silence", "kill") and node != fault[2] and e.get("winner") == fault[2] and fault[1] - 1_000_000 <= ts <= fault[1]:
                    elected_victim.add((e["request"]["client"], e["request"]["seq"]))
                bids.append(e["window_us"])
                seen[e["bids_seen"]] += 1
                won[node] += bool(e["i_won"])
            elif ev == "bid_sent":
                scores[(ts - t0) // 10_000_000][node].append(e["score"])
            elif ev == "started":
                node_pcs[node] = e.get("client_addr", "?").split(":")[0]
            elif ev == "messages":
                msgs[node] = e
            elif ev == "resource_usage":
                usage[node].append(e)
            elif ev == "raft_state":
                views[node].append((ts, e["term"]))
            elif ev == "silenced":
                silenced.append((ts, node, e["ms"]))
            elif ev == "restored":
                restored.append((ts, node))
            elif ev == "rejoined":
                rejoined.append((ts, node, e.get("recovery_us")))
            elif ev == "fault_election":
                elections.append(e)
            elif ev in ("takeover_started", "overloaded", "provisional_replied", "degraded_entered", "peer_suspected"):
                counts[ev] += 1
    r["checkpoints"] = len(cp)
    r["digest_mismatches"] = sum(len(set(v.values())) > 1 for v in cp.values())
    for k in ("takeover_started", "overloaded", "provisional_replied", "degraded_entered", "peer_suspected"):
        r[k] = counts[k]
    bids.sort()
    r["bid_window_p50_ms"] = round(pct(bids, 0.5) / 1000, 1) if bids else None
    r["bid_window_p99_ms"] = round(pct(bids, 0.99) / 1000, 1) if bids else None
    r["bids_saw_all3"] = round(seen[3] / max(sum(seen.values()), 1), 4)
    r["score_windows"] = {str(k * 10): {str(n): round(statistics.mean(v)) for n, v in d.items()} for k, d in sorted(scores.items())}
    cores, rss, rx, tx = [], [], [], []
    for node, us in usage.items():
        if len(us) >= 4:
            u, v = us[len(us) // 4], us[3 * len(us) // 4]
            dt = (v["ts_us"] - u["ts_us"]) / 1e6
            cores.append((v["cpu_ticks"] - u["cpu_ticks"]) / 100 / dt)
            rx.append((v["net_rx_bytes"] - u["net_rx_bytes"]) * 8 / 1e6 / dt)
            tx.append((v["net_tx_bytes"] - u["net_tx_bytes"]) * 8 / 1e6 / dt)
            rss.append(max(x["rss_kb"] for x in us) / 1024)
    r["cpu_cores_per_node"] = round(statistics.mean(cores), 2) if cores else None
    r["rss_mb_max"] = round(max(rss)) if rss else None
    r["rx_mbit_per_node"] = round(statistics.mean(rx), 1) if rx else None
    r["tx_mbit_per_node"] = round(statistics.mean(tx), 1) if tx else None
    r["client_pcs"] = client_pcs
    r["node_pcs"] = node_pcs
    # Election overhead: messages each node sent, per answered request. Bids are combined, so this counts calls.
    answered = max(len(done), 1)
    for kind, name in (("bid", "bid"), ("raft_append", "log_stream"), ("peer_heartbeat", "heartbeat"), ("fault_bid", "fault_bid")):
        total = sum(m.get("sent", {}).get(kind, 0) for m in msgs.values())
        r[f"msgs_{name}"] = total
        r[f"msgs_{name}_per_request"] = round(total / answered, 4)
    status = open(f"{run}/status-after.txt", errors="replace").read()
    digests = {w[6] for w in (l.split() for l in status.splitlines()) if len(w) > 7 and w[0] in "123"}
    results = {w[7] for w in (l.split() for l in status.splitlines()) if len(w) > 7 and w[0] in "123"}
    r["final_same_digest"] = len(digests) == 1
    r["final_results"] = "/".join(sorted(results))

    # A fault: before, during, after, and when the others and the node itself reacted.
    if fault and fault[0] in ("silence", "kill"):
        at, victim = fault[1], fault[2]
        length = fault[3] if fault[0] == "silence" else (times.get("start_again", at) - at) // 1000
        end = at + length * 1000
        for name, u, v in (("before", at - 20_000_000, at), ("during", at, end), ("after", end, end + 20_000_000), ("late", end + 20_000_000, end + 40_000_000)):
            for k, x in phase_stats(done, u, v).items():
                r[f"{name}_{k}"] = x
        r["fault_ms"] = length
        r["victim"] = victim
        changes = [ts for n, vs in views.items() if n != victim for ts, term in vs if ts >= at]
        changes.sort()
        r["others_dropped_it_ms"] = round((changes[0] - at) / 1000) if changes else None
        order = sorted(t for t, _ in done if t >= at - 1_000_000 and t <= at + 60_000_000)
        gaps = [(y - x, x) for x, y in zip(order, order[1:])]
        r["longest_no_answer_ms"] = round(max(gaps)[0] / 1000) if gaps else None
        # Back at the rate: the first second from the fault on where answers reach 90% of the rate before it.
        before = r["before_per_s"] or 0
        back = None
        for s in range((at - t0) // 1_000_000, len(r["timeline"])):
            if r["timeline"][s] >= 0.9 * before:
                back = s
                break
        r["rate_back_after_ms"] = round((t0 + back * 1_000_000 - at) / 1000) if back is not None else None
        # In flight at the fault: asked before it, answered after it.
        in_flight = 0
        won_total = won_done_by_others = won_done_by_victim = won_retried = won_gave_up = 0
        during_victim = during_total = during_from_victim = 0
        open_won_total = open_won_by_others = open_won_by_it = 0
        for path in glob.glob(f"{run}/client-*.jsonl.gz"):
            program = int(re.search(r"client-(\d+)", path).group(1))
            for e in lines(path):
                ev = e.get("ev")
                if ev == "client_done":
                    k = (program, e["request"]["seq"])
                    if e["ts_us"] >= at:
                        st = sent_times.get(k)
                        if st is not None and st < at:
                            in_flight += 1
                    if at + 2_000_000 <= e["ts_us"] < end:
                        during_total += 1
                        during_victim += e["executor"] == victim
                        during_from_victim += e["from"] == victim
                    if k in elected_victim and e["ts_us"] >= at + 300_000:
                        open_won_total += 1
                        if e["executor"] == victim:
                            open_won_by_it += 1
                        else:
                            open_won_by_others += 1
                    if k in elected_victim:
                        won_total += 1
                        if e["executor"] == victim:
                            won_done_by_victim += 1
                        else:
                            won_done_by_others += 1
                        won_retried += e["attempts"] > 1
                elif ev == "client_gave_up" and (program, e["request"]["seq"]) in elected_victim:
                    won_gave_up += 1
        r["in_flight_at_fault"] = in_flight
        r["elected_victim_seen"] = len(elected_victim)
        r["elected_victim_done_by_others"] = won_done_by_others
        r["elected_victim_done_by_it"] = won_done_by_victim
        r["elected_victim_retried"] = won_retried
        r["elected_victim_gave_up"] = won_gave_up
        r["during_exec_by_victim"] = during_victim
        r["during_exec_total"] = during_total
        r["during_answers_from_victim"] = during_from_victim
        r["won_unanswered"] = open_won_total
        r["won_unanswered_by_others"] = open_won_by_others
        r["won_unanswered_committed_by_it"] = open_won_by_it
        vic_out = open(f"{run}/node-{victim}.out", errors="replace").read()
        back_lines = re.findall(r"^(\S+) .*back as a member", vic_out, re.M)
        if back_lines:
            import datetime
            ts = datetime.datetime.fromisoformat(back_lines[-1].replace("Z", "+00:00")).timestamp() * 1e6
            r["member_again_ms"] = round((ts - end) / 1000)
        rj = [x for x in rejoined if x[1] == victim and x[0] >= at]
        if rj:
            r["serving_again_ms"] = round((rj[0][0] - end) / 1000)
    if fault and fault[0] == "skew_on":
        on, victim = fault[1], fault[2]
        off = times.get("skew_off", on + 60_000_000)
        r["victim"] = victim
        r["skew_share_before"] = share_in(exec_window, on - t0 - 40_000_000, on - t0, victim)
        r["skew_share_during"] = share_in(exec_window, on - t0 + 10_000_000, off - t0, victim)
        r["skew_share_after"] = share_in(exec_window, off - t0 + 10_000_000, off - t0 + 50_000_000, victim)
    if elections:
        rounds = collections.defaultdict(list)
        for e in elections:
            rounds[e["round"]].append(e)
        victims = [v[0]["victim"] for v in rounds.values() if v[0]["victim"] is not None]
        r["fault_elections"] = len(rounds)
        r["faults_elected"] = len(victims)
        r["faults_by_node"] = dict(collections.Counter(victims))
        durations = sorted(s[2] for s in silenced)
        r["silence_ms_mean"] = round(statistics.mean(durations)) if durations else None
        r["silence_ms_max"] = durations[-1] if durations else None
        faults = []
        for ts, node, ms in sorted(silenced):
            back = min((x[0] for x in restored if x[1] == node and x[0] >= ts), default=None)
            rj = min((x[0] for x in rejoined if x[1] == node and back and x[0] >= back), default=None)
            others = [t2 for n, vs in views.items() if n != node for t2, term in vs if t2 >= ts]
            faults.append({"node": node, "ms": ms, "others_dropped_it_ms": round((min(others) - ts) / 1000) if others else None,
                           "serving_again_ms": round((rj - back) / 1000) if rj else None})
        r["faults"] = faults
        rec = sorted(x[2] / 1000 for x in rejoined if x[2])
        r["recovery_ms_p50"] = round(pct(rec, 0.5)) if rec else None
        r["recovery_ms_max"] = round(rec[-1]) if rec else None
    return r


def share_in(windows, a_us, b_us, node):
    total = hit = 0
    for k, v in windows.items():
        if a_us <= k * 10_000_000 < b_us:
            total += sum(v.values())
            hit += v.get(node, 0)
    return round(hit / total, 4) if total else None


def mean_ci(values):
    xs = [float(v) for v in values if isinstance(v, (int, float)) and not isinstance(v, bool)]
    if not xs:
        return None, None, 0
    m = statistics.mean(xs)
    if len(xs) < 2:
        return m, None, 1
    return m, T95.get(len(xs) - 1, 2.0) * statistics.stdev(xs) / math.sqrt(len(xs)), len(xs)


def safe(run):
    try:
        return analyse(run)
    except Exception as e:
        import traceback
        return {"path": run, "failed": traceback.format_exc(limit=3)}


def main():
    root, out = sys.argv[1], sys.argv[2]
    os.makedirs(out, exist_ok=True)
    skip = set(sys.argv[3].split(",")) if len(sys.argv) > 3 else {"raft0", "smoke"}
    todo = [os.path.dirname(m) for m in sorted(glob.glob(f"{root}/*/*/run-*/run.json")) if m.split("/")[-4] not in skip]
    runs = []
    with multiprocessing.Pool(int(os.environ.get("ANALYZE_PROCS", "8"))) as pool:
        for result in pool.imap_unordered(safe, todo):
            if "failed" in result:
                print("FAILED", result["path"], result["failed"], flush=True)
            else:
                runs.append(result)
                print("read", result["path"], result["answered_per_s"], flush=True)
    runs.sort(key=lambda r: (r["experiment"], str(r["cell"]), r["run"]))
    scalar = sorted({k for r in runs for k, v in r.items() if not isinstance(v, (list, dict))})
    with open(f"{out}/runs.csv", "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=scalar, extrasaction="ignore")
        w.writeheader()
        w.writerows(runs)
    json.dump(runs, open(f"{out}/runs.json", "w"), default=str)
    cells = collections.defaultdict(list)
    for r in runs:
        cells[f"{r['experiment']}-{r['cell']}"].append(r)
    numeric = [k for k in scalar if any(isinstance(r.get(k), (int, float)) and not isinstance(r.get(k), bool) for r in runs) and k != "run"]
    with open(f"{out}/cells.csv", "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["cell", "runs"] + [c for k in numeric for c in (k, k + "_ci95")])
        for cell, rs in sorted(cells.items()):
            row = [cell, len(rs)]
            for k in numeric:
                m, ci, _ = mean_ci([r.get(k) for r in rs])
                row += [None if m is None else round(m, 3), None if ci is None else round(ci, 3)]
            w.writerow(row)
    print("runs", len(runs), "cells", len(cells), "wrote", out)


if __name__ == "__main__":
    main()
