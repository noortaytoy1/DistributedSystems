#!/usr/bin/env python3
# factcheck.py <run folder> <name>: every answer a client got, against the reply lines in the nodes' whole logs.
import collections, glob, gzip, json, re, subprocess, sys
run, name = sys.argv[1], sys.argv[2]
rep = run.rstrip("/").split("run-")[-1]
hosts = {1: "10.40.2.29", 2: "10.40.32.242", 3: "10.40.0.73"}
replied = collections.defaultdict(dict)
received = {}
for n, h in hosts.items():
    cmd = f"cd distributedddd/DIstributedSYstems; zcat runs/full/{name}/run-{rep}/node-{n}.jsonl.gz | grep -E '\"ev\":\"(replied|request_received)\"'"
    p = subprocess.Popen(["ssh", "-o", "BatchMode=yes", f"cse1@{h}", cmd], stdout=subprocess.PIPE, text=True)
    got = set(); c = collections.Counter()
    for line in p.stdout:
        e = json.loads(line)
        k = (e["request"]["client"], e["request"]["seq"])
        c[e["ev"]] += 1
        if e["ev"] == "replied":
            assert e["src"] == f"node-{n}"
            replied[k][n] = e
        else:
            got.add(k)
    received[n] = got
    print(f"node {n} ({h}): {c['request_received']} request_received lines, {len(got)} unique requests, {c['replied']} replied lines")
total = wrong_from = wrong_exec = no_reply = 0
pcs = collections.Counter(); answered_by = collections.Counter(); per_pc_node = collections.Counter(); missing_at = collections.Counter()
examples = []
for path in sorted(glob.glob(run + "/client-*.jsonl.gz")):
    prog = int(re.search(r"client-(\d+)", path).group(1)); pc = (prog - 1) // 3 + 1
    with gzip.open(path, "rt", errors="replace") as f:
        for line in f:
            if '"client_done"' not in line:
                continue
            e = json.loads(line)
            assert e["src"] == f"client-{prog}" and e["request"]["client"] == prog
            k = (prog, e["request"]["seq"])
            total += 1; pcs[pc] += 1; answered_by[e["from"]] += 1; per_pc_node[(pc, e["from"])] += 1
            r = replied.get(k, {})
            if e["from"] not in r:
                no_reply += 1
            elif r[e["from"]]["executor"] != e["executor"]:
                wrong_exec += 1
            for n in hosts:
                if k not in received[n]:
                    missing_at[n] += 1
            if total % 150000 == 1:
                examples.append((prog, pc, e["request"]["seq"], e["from"], e["executor"], e["latency_us"]))
print(f"answers checked {total}: client's `from` has no matching reply line in that node's log: {no_reply}; executor differs: {wrong_exec}")
print("reports that did not reach a node:", dict(missing_at) or "none")
print("answers per client PC:", dict(sorted(pcs.items())))
print("answers per cloud node:", dict(sorted(answered_by.items())))
print("client PC x cloud node:", {f"pc{a}-node{b}": v for (a, b), v in sorted(per_pc_node.items())})
print("examples (program, client PC, seq, answered by node, executor, latency us):")
for x in examples: print(" ", x)
