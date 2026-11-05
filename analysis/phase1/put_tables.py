#!/usr/bin/env python3
# put_tables.py: puts the generated tables between their markers in the report.
import re
p = "docs/phase1-report.md"
t = open(p).read()
for name, path in (("KILLS", "analysis/results/kills/tables.md"), ("STALL", "analysis/results/kills/stall.md")):
    body = open(path).read().strip()
    t, n = re.subn(rf"<!-- {name}:BEGIN -->.*?<!-- {name}:END -->", lambda m: f"<!-- {name}:BEGIN -->\n{body}\n<!-- {name}:END -->", t, flags=re.S)
    assert n == 1, name
open(p, "w").write(t)
print("tables in the report")
