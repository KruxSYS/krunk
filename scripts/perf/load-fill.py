#!/usr/bin/env python3
"""Krunk 0.26.0 load harness: a fill pass for the sessions of a pass that did not complete.

  load-fill.py PASS_DIR CONFIG.json > fill.json

A session is one browser start: `typed` (cold + warm) or `link` (link, back, forward). Any row of
a session that is not a load or a back/forward-cache restore (timeout, error) or a missing row
puts the whole session in the fill pass, in the original order (run, page, rotated browser).
load-summary.py PASS_DIR FILL_DIR then lets the fill's sessions replace the pass's.
"""
import json
import sys

GROUP = {"cold": "typed", "warm": "typed", "link": "link", "back": "link", "forward": "link"}
EXPECT = {"typed": {"cold", "warm"}, "link": {"link", "back", "forward"}}

pass_dir, conf = sys.argv[1], json.load(open(sys.argv[2]))
seen = {}
for line in open(f"{pass_dir}/results.jsonl", encoding="utf-8"):
    if not line.strip():
        continue
    r = json.loads(line)
    g = GROUP.get(r.get("scenario"), r.get("scenario"))
    key = (r["browser"], r["page"], r["run"], g)
    ok = r.get("kind") in ("load", "bfcache")
    seen.setdefault(key, {})[r.get("scenario")] = ok
cells = []
for run in range(1, conf["runs"] + 1):
    for p in conf["pages"]:
        bs = conf["browsers"]
        order = [bs[(i + run - 1) % len(bs)] for i in range(len(bs))]
        for b in order:
            for g in conf["scenarios"]:
                got = seen.get((b, p["id"], run, g), {})
                if set(got) != EXPECT[g] or not all(got.values()):
                    cells.append({"browser": b, "page": p["id"], "run": run, "scenario": g})
conf = dict(conf)
conf["cells"] = cells
conf.pop("out", None)
print(json.dumps(conf, indent=1))
print(f"{len(cells)} session(s) to fill", file=sys.stderr)
