#!/usr/bin/env python3
# t2_check.py <run folder>: per 10 s, answers and commits, to see whether commits come back by themselves.
import collections, glob, gzip, json, sys
R = sys.argv[1]
ev = []
for p in glob.glob(R + "/*.gz"):
    with gzip.open(p, "rt", errors="replace") as f:
        for l in f:
            try:
                e = json.loads(l)
            except ValueError:
                continue
            if e.get("ev") in ("client_done", "client_gave_up", "silenced", "restored", "rejoined", "restarted", "degraded_entered", "degraded_left", "state_checkpoint"):
                ev.append(e)
t0 = min(e["ts_us"] for e in ev)
for e in sorted(ev, key=lambda x: x["ts_us"]):
    if e["ev"] in ("silenced", "restored", "rejoined", "restarted", "degraded_entered", "degraded_left"):
        print(round((e["ts_us"] - t0) / 1e6, 1), e["src"], e["ev"], {k: v for k, v in e.items() if k in ("ms", "degraded_ms", "provisional", "recovery_us")})
w = collections.defaultdict(collections.Counter)
cp = collections.defaultdict(dict)
for e in ev:
    s = (e["ts_us"] - t0) // 10_000_000 * 10
    if e["ev"] == "client_done":
        w[s]["answers"] += 1
        w[s]["provisional"] += bool(e["provisional"])
    elif e["ev"] == "client_gave_up":
        w[s]["gave_up"] += 1
    elif e["ev"] == "state_checkpoint":
        w[s]["checkpoints_" + e["src"][-1]] += 1
        cp[e["log_index"]][e["src"]] = e["digest"]
print("t answers provisional gave_up | log checkpoints on node 1, 2, 3")
for s in sorted(w):
    c = w[s]
    print(s, c["answers"], c["provisional"], c["gave_up"], "|", c["checkpoints_1"], c["checkpoints_2"], c["checkpoints_3"])
print("checkpoints", len(cp), "digest mismatches", sum(len(set(v.values())) > 1 for v in cp.values()))
print(open(R + "/status-after.txt", errors="replace").read()[:420])
