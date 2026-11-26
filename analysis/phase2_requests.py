#!/usr/bin/env python3
"""Per-request accounting for Phase 2 runs, which are too big for analyze.py (D448): every report a
program sent must end answered or given up, over the whole run, warm-up included. Reads only the
`client_sent`, `client_done` and `client_gave_up` lines, picked out by grep, so it is quick on large logs.

    python3 analysis/phase2_requests.py <root with experiment/cell/run-N>

Prints one line per run and the total: sent, answered, gave up, unaccounted.
"""
import json
import subprocess
import sys
from pathlib import Path


def requests_of(path):
    lines = subprocess.run(["grep", "-F", '"ev":"client_', str(path)], capture_output=True, text=True).stdout
    sent, done, gave_up = set(), set(), set()
    for line in lines.splitlines():
        try:
            e = json.loads(line)
        except json.JSONDecodeError:
            continue
        key = json.dumps(e.get("request"), sort_keys=True)
        if e["ev"] == "client_sent":
            sent.add(key)
        elif e["ev"] == "client_done":
            done.add(key)
        elif e["ev"] == "client_gave_up":
            gave_up.add(key)
    return sent, done, gave_up


def main():
    totals = [0, 0, 0, 0, 0]
    for meta in sorted(Path(sys.argv[1]).glob("*/*/*/run.json")):
        run = meta.parent
        counts = [0, 0, 0, 0]
        for path in sorted(run.glob("client-*.jsonl")):
            sent, done, gave_up = requests_of(path)
            counts[0] += len(sent)
            counts[1] += len(done)
            counts[2] += len(gave_up)
            counts[3] += len(sent - done - gave_up)
        print(f"{run}: sent {counts[0]}, answered {counts[1]}, gave up {counts[2]}, unaccounted {counts[3]}", flush=True)
        totals = [t + c for t, c in zip(totals, counts + [1])]
    print(f"total over {totals[4]} runs: sent {totals[0]}, answered {totals[1]}, gave up {totals[2]}, unaccounted {totals[3]}")


if __name__ == "__main__":
    main()
