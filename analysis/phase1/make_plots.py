#!/usr/bin/env python3
# make_plots.py <runs.json> <out dir>: the report's charts. Colours: the validated reference palette, by entity.
import collections, json, math, os, statistics as st, sys
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
runs = json.load(open(sys.argv[1])); out = sys.argv[2]; os.makedirs(out, exist_ok=True)
C1, C2, C3 = "#2a78d6", "#eb6834", "#1baf7a"
INK, MUTED, GRID, SURFACE = "#0b0b0b", "#52514e", "#e6e5e1", "#fcfcfb"
plt.rcParams.update({"font.size": 11, "axes.edgecolor": MUTED, "axes.labelcolor": MUTED, "xtick.color": MUTED, "ytick.color": MUTED, "text.color": INK, "axes.spines.top": False, "axes.spines.right": False, "axes.grid": True, "grid.color": GRID, "grid.linewidth": 0.8, "figure.facecolor": SURFACE, "axes.facecolor": SURFACE, "lines.linewidth": 2, "savefig.dpi": 160, "savefig.bbox": "tight"})
cells = collections.defaultdict(list)
for r in runs: cells[(r["experiment"], str(r["cell"]))].append(r)
T = {1: 12.706, 2: 4.303, 3: 3.182, 4: 2.776}
def mc(rs, k):
    x = [r[k] for r in rs if isinstance(r.get(k), (int, float))]
    return (st.mean(x), T.get(len(x) - 1, 2.0) * st.stdev(x) / math.sqrt(len(x)) if len(x) > 1 else 0) if x else (None, 0)
S0 = [5000, 10000, 20000, 30000, 33000, 35000, 36000]; S1 = [1000, 5000, 8000, 10000, 12000, 14000]
fig, ax = plt.subplots(figsize=(7.5, 4.2))
ax.plot([0, 36000], [0, 36000], color=MUTED, lw=1, ls="--", label="Answered = offered")
for exp, rates, c, label in (("s0", S0, C1, "plan_work 0"), ("s1", S1, C2, "plan_work 1000")):
    pts = [(q, *mc(cells[(exp, str(q))], "answered_per_s")) for q in rates if (exp, str(q)) in cells]
    ax.errorbar([p[0] for p in pts], [p[1] for p in pts], yerr=[p[2] for p in pts], color=c, marker="o", ms=6, capsize=3, label=label)
    at = pts[len(pts) // 2]
    ax.annotate(label, (at[0], at[1]), textcoords="offset points", xytext=(10, -16) if exp == "s0" else (-10, 12), ha="left" if exp == "s0" else "right", color=INK)
ax.set(title="Answers per second against load (mean, 95% CI, 5 runs each)", xlabel="Offered requests per second", ylabel="Answered per second"); ax.legend(frameon=False, loc="upper left")
fig.savefig(f"{out}/throughput.png"); plt.close(fig)
fig, axes = plt.subplots(1, 2, figsize=(10, 4.2), sharey=True)
for ax, exp, rates, title in ((axes[0], "s0", S0, "plan_work 0"), (axes[1], "s1", S1, "plan_work 1000")):
    for k, c, label in (("p50_ms", C1, "p50"), ("p95_ms", C2, "p95"), ("p99_ms", C3, "p99")):
        pts = [(q, *mc(cells[(exp, str(q))], k)) for q in rates if (exp, str(q)) in cells]
        ax.errorbar([p[0] for p in pts], [p[1] for p in pts], yerr=[min(p[2], p[1] * 0.9) for p in pts], color=c, marker="o", ms=5, capsize=3, label=label)
    ax.set(title=title, xlabel="Offered requests per second", yscale="log"); ax.legend(frameon=False, loc="upper left")
axes[0].set_ylabel("Latency, ms (log scale)")
fig.suptitle("Latency against load (mean, 95% CI, 5 runs each)"); fig.savefig(f"{out}/latency.png"); plt.close(fig)
def windows(rs, field="exec_windows"):
    keys = sorted({int(k) for r in rs for k in r[field]})
    res = {}
    for k in keys:
        vals = {1: [], 2: [], 3: []}
        for r in rs:
            w = r[field].get(str(k))
            if w:
                t = sum(w.values()) or 1
                for n in vals: vals[n].append(w.get(str(n), 0) / t)
        res[k] = {n: st.mean(v) * 100 if v else 0 for n, v in vals.items()}
    return res
rs = cells.get(("e1", "steps"), [])
if rs:
    fig, axes = plt.subplots(2, 1, figsize=(8, 5.4), sharex=True, gridspec_kw={"height_ratios": [1, 1.6]})
    tl = [st.mean(r["timeline"][s] for r in rs if s < len(r["timeline"])) for s in range(0, 245)]
    axes[0].plot(range(len(tl)), tl, color=MUTED); axes[0].set(ylabel="Answers/s", title="Scenario 1: load changes, every node keeps winning (mean of 5 runs)")
    w = windows(rs); keys = [k for k in w if k < 240]
    for n, c in ((1, C1), (2, C2), (3, C3)): axes[1].plot(keys, [w[k][n] for k in keys], color=c, marker="o", ms=4, label=f"Node {n}")
    axes[1].set(xlabel="Seconds into the run", ylabel="Share of work won, %", ylim=(0, 60)); axes[1].legend(frameon=False, ncol=3, loc="upper right")
    fig.savefig(f"{out}/scenario1.png"); plt.close(fig)
rs = cells.get(("e2", "skewfix"), []); q = cells.get(("e2", "queuefix"), []); ctl = cells.get(("e2", "control"), [])
def victim_share(rs):
    keys = sorted({int(k) for r in rs for k in r["exec_windows"]}); res = {}
    for k in keys:
        x = []
        for r in rs:
            w = r["exec_windows"].get(str(k)); v = str(r.get("victim", 1))
            if w and sum(w.values()) > 100: x.append(w.get(v, 0) / sum(w.values()) * 100)
        if x: res[k] = st.mean(x)
    return res
if rs:
    fig, ax = plt.subplots(figsize=(8, 4.2))
    ax.axvspan(60, 120, color=GRID, alpha=0.7, lw=0); ax.annotate("Node slowed", (90, 92), ha="center", color=MUTED)
    for data, c, label in ((rs, C1, "Expected wait (in use)"), (q, C2, "Queue length only")):
        if data:
            s = victim_share(data); ax.plot(list(s), list(s.values()), color=c, marker="o", ms=4, label=label)
    if ctl:
        s = {}
        for k in sorted({int(k) for r in ctl for k in r["exec_windows"]}):
            x = [r["exec_windows"][str(k)].get("1", 0) / sum(r["exec_windows"][str(k)].values()) * 100 for r in ctl if str(k) in r["exec_windows"] and sum(r["exec_windows"][str(k)].values()) > 100]
            if x: s[k] = st.mean(x)
        ax.plot(list(s), list(s.values()), color=C3, marker="o", ms=4, label="Control, nothing slowed (node 1)")
    ax.set(title="Scenario 2: the slowed node's share of the work (mean of 5 runs)", xlabel="Seconds into the run", ylabel="Share won, %", ylim=(0, 100)); ax.legend(frameon=False, loc="upper left")
    fig.savefig(f"{out}/scenario2.png"); plt.close(fig)
fig, axes = plt.subplots(2, 3, figsize=(11, 5.4), sharex=True)
for ax, (e, c, name) in zip(axes.flat, [("f1", "1000", "1,000/s, plan_work 1000"), ("f1", "5000", "5,000/s, plan_work 1000"), ("f1", "8000", "8,000/s, plan_work 1000"), ("f0", "10000", "10,000/s, plan_work 0"), ("f0", "20000", "20,000/s, plan_work 0"), ("f0", "30000", "30,000/s, plan_work 0")]):
    rs = cells.get((e, c), [])
    if not rs: continue
    n = min(len(r["timeline"]) for r in rs)
    tl = [st.mean(r["timeline"][s] for r in rs) for s in range(n)]
    ax.axvspan(45, 65, color=GRID, alpha=0.8, lw=0)
    ax.plot(range(n), tl, color=C1); ax.set(title=name, ylim=(0, max(tl) * 1.25)); ax.title.set_fontsize(10)
for ax in axes[1]: ax.set_xlabel("Seconds into the run")
for ax in axes[:, 0]: ax.set_ylabel("Answers/s")
fig.suptitle("Scenario 3: answers per second through a 20 s fault, shaded (mean of 5 runs)"); fig.savefig(f"{out}/scenario3.png"); plt.close(fig)
pts = [("1,000", "f1", "1000"), ("5,000", "f1", "5000"), ("8,000", "f1", "8000"), ("10,000", "f0", "10000"), ("20,000", "f0", "20000"), ("30,000", "f0", "30000")]
fig, ax = plt.subplots(figsize=(7.5, 4.2)); xs = range(len(pts))
m = [mc(cells[(e, c)], "member_again_ms") for _, e, c in pts if (e, c) in cells]
ax.bar(xs, [v[0] / 1000 for v in m], yerr=[v[1] / 1000 for v in m], color=C1, width=0.5, capsize=3, error_kw={"ecolor": MUTED})
ax.set_xticks(list(xs)); ax.set_xticklabels([p[0] for p in pts]); ax.grid(axis="x", visible=False)
ax.set(title="Scenario 4: time to be a full member again after a 20 s fault (mean, 95% CI)", xlabel="Offered requests per second", ylabel="Seconds after the fault ended")
fig.savefig(f"{out}/scenario4.png"); plt.close(fig)
print("plots in", out)
