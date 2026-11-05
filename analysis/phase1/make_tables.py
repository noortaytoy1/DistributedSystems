#!/usr/bin/env python3
# make_tables.py <runs.json> <out.md>: the report's tables, read from the analysis, so no number is typed by hand.
import collections, json, math, statistics as st, sys
T = {1: 12.706, 2: 4.303, 3: 3.182, 4: 2.776, 5: 2.571, 6: 2.447, 7: 2.365, 8: 2.306, 9: 2.262}
runs = json.load(open(sys.argv[1]))
cells = collections.defaultdict(list)
for r in runs:
    cells[(r["experiment"], str(r["cell"]))].append(r)
def vals(rs, k): return [r[k] for r in rs if isinstance(r.get(k), (int, float)) and not isinstance(r.get(k), bool)]
def ci(rs, k, nd=0):
    x = vals(rs, k)
    if not x: return "n/a"
    a = st.mean(x)
    if len(x) < 2: return f"{a:,.{nd}f}"
    return f"{a:,.{nd}f} ± {T.get(len(x)-1, 2.0)*st.stdev(x)/math.sqrt(len(x)):,.{nd}f}"
def total(rs, k): return f"{sum(vals(rs, k)):,}"
def held(r, target):
    return r["answered_per_s"] >= 0.99 * target and r["gave_up"] == 0 and (r["p99_ms"] or 1e9) < 3000
out = []
def table(head, rows):
    out.append("| " + " | ".join(head) + " |")
    out.append("|" + "|".join("---" if i == 0 else "---:" for i in range(len(head))) + "|")
    for row in rows:
        out.append("| " + " | ".join(str(c) for c in row) + " |")
    out.append("")
def H(s): out.append(s); out.append("")
tot = lambda k: sum(r.get(k) or 0 for r in runs)
H("<!-- TOTALS -->")
table(["Measure", "Value"], [
    ["Runs", f"{len(runs):,}"], ["Requests sent", f"{tot('sent'):,}"], ["Answered", f"{tot('done'):,}"],
    ["Gave up after all retries", f"{tot('gave_up'):,}"], ["Unaccounted for", f"{tot('unaccounted'):,}"],
    ["Digest checkpoints compared", f"{tot('checkpoints'):,}"], ["Digest mismatches", f"{tot('digest_mismatches'):,}"],
    ["Runs ending with different digests", f"{sum(not r['final_same_digest'] for r in runs):,}"]])
def stress(exp, rates):
    rows = []
    for rate in rates:
        rs = cells.get((exp, str(rate)))
        if not rs: continue
        rows.append([f"{rate:,}", f"{sum(held(r, rate) for r in rs)} of {len(rs)}", ci(rs, "answered_per_s"), total(rs, "gave_up"), f"{st.mean(vals(rs, 'first_try_share'))*100:.1f}%", ci(rs, "p50_ms"), ci(rs, "p95_ms"), ci(rs, "p99_ms"), ci(rs, "cpu_cores_per_node", 1), ci(rs, "rx_mbit_per_node", 1)])
    table(["Offered/s", "Runs that held", "Answered/s", "Gave up", "First try", "p50 ms", "p95 ms", "p99 ms", "Cores per node", "Rx Mbit/s per node"], rows)
H("<!-- S0 -->"); stress("s0", [5000, 10000, 20000, 30000, 33000, 35000, 36000])
H("<!-- S1 -->"); stress("s1", [1000, 5000, 8000, 10000, 12000, 14000])
H("<!-- ELECTION -->")
rows = []
for exp, rate in [("s0", 5000), ("s0", 10000), ("s0", 20000), ("s0", 30000), ("s0", 35000), ("s1", 1000), ("s1", 5000), ("s1", 10000), ("s1", 12000), ("s1", 14000)]:
    rs = cells.get((exp, str(rate)))
    if not rs: continue
    rows.append([f"{rate:,}", "0" if exp == "s0" else "1000", ci(rs, "bid_window_p50_ms", 1), ci(rs, "bid_window_p99_ms", 1), f"{st.mean(vals(rs, 'bids_saw_all3'))*100:.1f}%", ci(rs, "msgs_bid_per_request", 3), ci(rs, "msgs_log_stream_per_request", 3), ci(rs, "msgs_heartbeat_per_request", 3), ci(rs, "jain_executor", 3)])
table(["Offered/s", "`plan_work`", "Election p50 ms", "Election p99 ms", "Elections seeing all 3 bids", "Bid calls per request", "Log calls per request", "Heartbeats per request", "Fairness (Jain)"], rows)
H("<!-- SHARES -->")
rows = []
for exp, rate in [("s0", 10000), ("s0", 20000), ("s0", 30000), ("s0", 35000), ("s1", 10000), ("s1", 12000)]:
    rs = cells.get((exp, str(rate)))
    if not rs: continue
    rows.append([f"{rate:,}", "0" if exp == "s0" else "1000"] + [ci(rs, f"exec_share_{n}", 3) for n in (1, 2, 3)])
table(["Offered/s", "`plan_work`", "Node 1", "Node 2", "Node 3"], rows)
H("<!-- PCS -->")
rows = []
for label, key in [("1 client PC", ("pcs", "1")), ("2 client PCs", ("pcs", "2")), ("3 client PCs", ("s0", "10000"))]:
    rs = cells.get(key)
    if not rs: continue
    rows.append([label, ci(rs, "answered_per_s"), total(rs, "gave_up"), ci(rs, "p50_ms"), ci(rs, "p99_ms")] + [ci(rs, f"pc{p}_answers") for p in (1, 2, 3)])
table(["Load from", "Answered/s", "Gave up", "p50 ms", "p99 ms", "Answers, PC 1", "Answers, PC 2", "Answers, PC 3"], rows)
H("<!-- HOLDS -->")
rows = []
for key, label, nd in [(("h0", "30000"), "30,000, `plan_work` 0", 0), (("h1", "10000"), "10,000, `plan_work` 1000", 0)]:
    rs = cells.get(key)
    if not rs: continue
    r = rs[0]
    rows.append([label, f"{r['sent']:,}", f"{r['answered_per_s']:,.0f}", r["gave_up"], f"{r['first_try_share']*100:.1f}%", r["p50_ms"], r["p95_ms"], r["p99_ms"], f"{r['checkpoints']:,}", r["digest_mismatches"], r["cpu_cores_per_node"], f"{r['rss_mb_max']:,}"])
table(["Held for 300 s at", "Requests", "Answered/s", "Gave up", "First try", "p50 ms", "p95 ms", "p99 ms", "Checkpoints", "Mismatches", "Cores per node", "Memory MB, most"], rows)
H("<!-- E1 -->")
stages = [("1,000/s", 20, 60), ("5,000/s", 70, 120), ("9,000/s", 130, 180), ("Back to 1,000/s", 200, 240)]
rows = []
rs = cells.get(("e1", "steps"), [])
for name, a, b in stages:
    share = collections.defaultdict(list); score = collections.defaultdict(list); rate = []
    for r in rs:
        c = collections.Counter(); s = collections.defaultdict(list)
        for k, v in r["exec_windows"].items():
            if a <= int(k) < b:
                c.update({int(n): x for n, x in v.items()})
        for k, v in r["score_windows"].items():
            if a <= int(k) < b:
                for n, x in v.items(): s[int(n)].append(x)
        t = sum(c.values()) or 1
        rate.append(t / (b - a))
        for n in (1, 2, 3):
            share[n].append(c[n] / t)
            if s[n]: score[n].append(st.mean(s[n]))
    rows.append([name, f"{st.mean(rate):,.0f}"] + [f"{st.mean(share[n])*100:.1f}%" for n in (1, 2, 3)] + [f"{st.mean(score[n]):,.0f}" if score[n] else "n/a" for n in (1, 2, 3)])
table(["Load", "Answers/s", "Node 1 won", "Node 2 won", "Node 3 won", "Node 1 bid", "Node 2 bid", "Node 3 bid"], rows)
H("<!-- E2 -->")
def phase_table(cell):
    res = {}
    rs = cells.get(("e2", cell), [])
    for name, a, b in (("Before", 20, 60), ("Slowed", 70, 120), ("After", 130, 180)):
        sv, so, bv, bo = [], [], [], []
        for r in rs:
            v = r.get("victim", 1)
            c = collections.Counter()
            for k, w in r["exec_windows"].items():
                if a <= int(k) < b: c.update({int(n): x for n, x in w.items()})
            t = sum(c.values()) or 1
            sv.append(c[v] / t)
            so.append(st.mean(c[n] / t for n in (1, 2, 3) if n != v))
            x = [w[str(v)] for k, w in r["score_windows"].items() if a <= int(k) < b and str(v) in w]
            y = [w[str(n)] for k, w in r["score_windows"].items() if a <= int(k) < b for n in (1, 2, 3) if n != v and str(n) in w]
            if x: bv.append(st.mean(x))
            if y: bo.append(st.mean(y))
        res[name] = (st.mean(sv), st.mean(so), st.mean(bv) if bv else 0, st.mean(bo) if bo else 0) if rs else None
    return res
rows = []
for label, cell in (("Expected wait, the policy in use", "skewfix"), ("Queue length only", "queuefix"), ("Control: nothing slowed", "control")):
    res = phase_table(cell)
    for name, v in res.items():
        if v is None: continue
        rows.append([label, name, f"{v[0]*100:.1f}%", f"{v[1]*100:.1f}%", f"{v[2]:,.0f}", f"{v[3]:,.0f}"])
table(["Score", "Phase", "Slowed node won", "Each other node won", "Slowed node's bid", "Others' bid"], rows)
rows = []
for label, cell in (("Expected wait", "skewfix"), ("Queue length only", "queuefix"), ("Control", "control")):
    rs = cells.get(("e2", cell))
    if rs: rows.append([label, len(rs), ci(rs, "answered_per_s"), total(rs, "gave_up"), ci(rs, "p50_ms"), ci(rs, "p99_ms"), ci(rs, "jain_executor", 3)])
table(["Score", "Runs", "Answered/s", "Gave up", "p50 ms", "p99 ms", "Fairness (Jain)"], rows)
H("<!-- E3 -->")
order = [("f1", "1000", "1,000", 1000), ("f1", "5000", "5,000", 1000), ("f1", "8000", "8,000", 1000), ("f0", "10000", "10,000", 0), ("f0", "20000", "20,000", 0), ("f0", "30000", "30,000", 0)]
rows = []
for e, c, name, pw in order:
    rs = cells.get((e, c))
    if not rs: continue
    rows.append([name, pw, ci(rs, "before_per_s"), ci(rs, "during_per_s"), ci(rs, "after_per_s"), ci(rs, "before_p99_ms"), ci(rs, "during_p99_ms"), ci(rs, "after_p99_ms"), total(rs, "gave_up"), ci(rs, "retries")])
table(["Offered/s", "`plan_work`", "Answers/s before", "During 20 s fault", "After", "p99 ms before", "During", "After", "Gave up", "Retries"], rows)
rows = []
for e, c, name, pw in order:
    rs = cells.get((e, c))
    if not rs: continue
    rows.append([name, pw, ci(rs, "others_dropped_it_ms"), ci(rs, "longest_no_answer_ms"), ci(rs, "in_flight_at_fault"), total(rs, "elected_victim_seen"), total(rs, "won_unanswered"), total(rs, "won_unanswered_by_others"), total(rs, "won_unanswered_committed_by_it"), total(rs, "elected_victim_gave_up"), total(rs, "during_answers_from_victim"), total(rs, "during_exec_total")])
table(["Offered/s", "`plan_work`", "Others dropped it, ms", "Longest gap with no answer, ms", "Requests in flight at the fault", "It won in the last second", "Of them, not answered by 0.3 s after the fault", "Planned again by the others", "Its plan had already committed", "Gave up", "Answers it sent during the fault", "All answers during the fault"], rows)
H("<!-- E4 -->")
order2 = order + [("f0s", "10000", "10,000, 5 s fault", 0), ("k1", "5000", "5,000, killed", 1000), ("k0", "10000", "10,000, killed", 0)]
rows = []
for e, c, name, pw in order2:
    rs = cells.get((e, c))
    if not rs: continue
    rows.append([name, pw, len(rs), ci(rs, "serving_again_ms"), ci(rs, "member_again_ms"), ci(rs, "rate_back_after_ms"), total(rs, "digest_mismatches"), f"{sum(r['final_same_digest'] for r in rs)} of {len(rs)}", ci(rs, "converged_after_load_s", 1)])
table(["Offered/s", "`plan_work`", "Runs", "Caught up to the log, ms", "Member again, ms", "Answer rate back, ms after the fault", "Digest mismatches", "Runs ending with one digest", "Seconds to one digest, after the load"], rows)
H("<!-- R1 -->")
rows = []
faults = []
for r in cells.get(("r1", "3000"), []):
    faults += r.get("faults", [])
    rows.append([r["run"], f"{r['sent']:,}", r["gave_up"], r["p50_ms"], r["p99_ms"], r.get("faults_elected"), json.dumps(r.get("faults_by_node")).replace('"', ""), f"{r.get('silence_ms_mean', 0):,}", f"{r.get('silence_ms_max', 0):,}", f"{r.get('checkpoints', 0):,}", r["digest_mismatches"]])
table(["Run", "Requests", "Gave up", "p50 ms", "p99 ms", "Faults", "Faults per node", "Mean silence ms", "Longest silence ms", "Checkpoints", "Mismatches"], rows)
d = sorted(f["others_dropped_it_ms"] for f in faults if f.get("others_dropped_it_ms") is not None)
s = sorted(f["serving_again_ms"] for f in faults if f.get("serving_again_ms") is not None)
p = lambda v, q: v[min(len(v) - 1, int(len(v) * q))] if v else "n/a"
table(["Across all elected faults", "Samples", "Median ms", "p95 ms", "Most ms"], [
    ["Others dropped the silent node", len(d), p(d, .5), p(d, .95), d[-1] if d else "n/a"],
    ["It caught up to the log, from the end of its silence", len(s), p(s, .5), p(s, .95), s[-1] if s else "n/a"]])
H("<!-- WHO -->")
r0 = cells[("s0", "10000")][0]
rows = [[f"`client-{k}`", v.split()[0], v.split()[1]] for k, v in sorted(r0["client_pcs"].items(), key=lambda kv: int(kv[0]))]
rows += [[f"`node-{k}`", "cloud node", v] for k, v in sorted(r0["node_pcs"].items())]
table(["In the logs", "PC name", "IP address"], rows)
open(sys.argv[2], "w").write("\n".join(out))
print("wrote", sys.argv[2], len(out), "lines")
