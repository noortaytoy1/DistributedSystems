#!/usr/bin/env python3
# audit.py <run folder>: recomputes a run from raw per-request lines, trusting no summary.
import collections, glob, gzip, json, re, sys
run = sys.argv[1]
def opener(p): return gzip.open(p, "rt", errors="replace") if p.endswith(".gz") else open(p, errors="replace")
paths = sorted(glob.glob(run + "/client-*.jsonl*"))
print("client log files:", len(paths), [re.search(r"client-(\d+)", p).group(1) for p in paths])
first_sent, done_at, gave = {}, {}, set()
dup_done = 0; attempts = collections.Counter(); lat_field = []; exec_c = collections.Counter(); from_c = collections.Counter()
status = collections.Counter(); no_resp = prov = 0; sent_sec = collections.Counter(); done_sec = collections.Counter(); bad = 0
pc_sent = collections.Counter(); pc_done = collections.Counter()
for p in paths:
    prog = int(re.search(r"client-(\d+)", p).group(1)); pc = (prog - 1) // 3 + 1
    for line in opener(p):
        if '"client_' not in line: continue
        try: e = json.loads(line)
        except ValueError: bad += 1; continue
        ev = e["ev"]
        if ev == "client_sent":
            k = (e["request"]["client"], e["request"]["seq"])
            if e["attempt"] == 0:
                first_sent[k] = e["ts_us"]; sent_sec[e["ts_us"] // 1000000] += 1; pc_sent[pc] += 1
        elif ev == "client_done":
            k = (e["request"]["client"], e["request"]["seq"])
            if k in done_at: dup_done += 1
            done_at[k] = (e["ts_us"], e["latency_us"])
            done_sec[e["ts_us"] // 1000000] += 1; pc_done[pc] += 1
            attempts[e["attempts"]] += 1; exec_c[e["executor"]] += 1; from_c[e["from"]] += 1; status[e["status"]] += 1
            no_resp += bool(e["no_responder"]); prov += bool(e["provisional"])
        elif ev == "client_gave_up":
            gave.add((e["request"]["client"], e["request"]["seq"]))
print(f"unique requests sent {len(first_sent)}, answered {len(done_at)}, gave up {len(gave)}, unaccounted {len(first_sent) - len(done_at) - len(gave)}, answered twice {dup_done}, bad lines {bad}")
print("by client PC, sent:", dict(sorted(pc_sent.items())), "answered:", dict(sorted(pc_done.items())))
print("status:", dict(status), "no responder", no_resp, "provisional", prov)
print("attempts:", dict(sorted(attempts.items())))
n = sum(exec_c.values()) or 1
print("executor share:", {k: round(v / n, 3) for k, v in sorted(exec_c.items())}, "| reply from:", {k: round(v / n, 3) for k, v in sorted(from_c.items())})
ss = sorted(sent_sec); warm = 15
mid = [sent_sec[s] for s in ss[warm:-1]]
dm = [done_sec[s] for s in sorted(done_sec)[warm:warm + 60]]
if mid: print(f"sent per second after warm-up: min {min(mid)} median {sorted(mid)[len(mid)//2]} max {max(mid)} over {len(mid)} s")
if dm: print(f"answered per second, 60 s after warm-up: min {min(dm)} median {sorted(dm)[len(dm)//2]} max {max(dm)}")
start = ss[0] + warm; end = start + 60
xs = sorted(d[0] - first_sent[k] for k, d in done_at.items() if k in first_sent and start * 1000000 <= d[0] < end * 1000000)
fs = sorted(d[1] for k, d in done_at.items() if start * 1000000 <= d[0] < end * 1000000)
p = lambda v, q: v[min(len(v) - 1, int(len(v) * q))] / 1000 if v else None
print(f"measured window: {len(xs)} answers, {len(xs)/60:.1f}/s")
print(f"latency from timestamps (done minus first send): p50 {p(xs,.5):.1f} p95 {p(xs,.95):.1f} p99 {p(xs,.99):.1f} max {xs[-1]/1000:.1f} ms")
print(f"latency field in the log:                        p50 {p(fs,.5):.1f} p95 {p(fs,.95):.1f} p99 {p(fs,.99):.1f} ms")
# Nodes
for path in sorted(glob.glob(run + "/node-*.jsonl*")):
    last = None; seen = collections.Counter(); out = collections.Counter(); kinds = collections.Counter(); win = collections.Counter(); st = None
    for line in opener(path):
        if '"udp_counters"' in line: last = line
        elif '"bid_round_closed"' in line:
            try: e = json.loads(line); seen[e["bids_seen"]] += 1; win[e["i_won"]] += 1
            except ValueError: pass
        elif '"commit"' in line and '"kind"' in line:
            try: e = json.loads(line); out[(e["kind"], e["outcome"])] += 1
            except ValueError: pass
    u = json.loads(last) if last else {}
    print(path.split("/")[-1], "| udp rx_requests", u.get("rx_requests"), "rx_cancels", u.get("rx_cancels"), "tx_replies", u.get("tx_replies"), "rx_bad", u.get("rx_bad"), "rx_queue_full", u.get("rx_queue_full"), "| bids seen (sampled)", dict(seen), "| won", dict(win), "| commits (sampled)", {f"{a}/{b}": c for (a, b), c in out.items()})
print(open(run + "/status-after.txt", errors="replace").read()[:520])
