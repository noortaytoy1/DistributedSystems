# quick-check.py <run folder>: the checks that say at once whether a run is good.
import collections, glob, gzip, json, sys
cp = collections.defaultdict(dict); kinds = collections.Counter()
for path in sorted(glob.glob(sys.argv[1] + "/node-*.slim.gz")):
    with gzip.open(path, "rt", errors="replace") as f:
        for line in f:
            try:
                e = json.loads(line)
            except ValueError:
                continue
            ev = e.get("ev")
            if ev == "state_checkpoint":
                cp[e["log_index"]][e["src"]] = e["digest"]
            elif ev in ("silenced", "restored", "rejoined", "restarted", "degraded_entered", "degraded_left", "peer_suspected", "overloaded", "takeover_started", "provisional_replied"):
                kinds[ev] += 1
bad = [k for k, v in cp.items() if len(set(v.values())) > 1]
print(f"checkpoints {len(cp)}, digest mismatches {len(bad)}" + (f", first at {min(bad)}" if bad else ""))
print(dict(kinds))
