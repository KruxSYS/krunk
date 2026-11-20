#!/usr/bin/env python3
"""Writes a load.mjs config from corpus.json.

  mkconfig.py MODE RUNS BROWSERS SCENARIOS [PAGES] > config.json
    MODE       replay | live | record
    BROWSERS   comma list: krunk, krunk-a, krunk-b, krunk-new, chrome, edge (krunk* names take
               KRUNK_<NAME>_APP / _BASE from the environment, default app260h / lp-base)
    SCENARIOS  comma list: typed, link
    PAGES      comma list of page ids (default: all, or the live set in live mode)
Environment: RTT (default 40), ARCHIVE (default C:\\pipeline\\krunk\\load\\corpus.wprgo),
KRUNK_ENV (JSON object of extra environment for Krunk, e.g. '{"KRUNK_PERF_TRACE":"1"}'),
COLLECT (comma list of files to keep from each session's profile, e.g. 'Krunk\\perf.log,trace.json'),
WAIT_FOCUS=1 (type only once the address bar has the focus: the F-keys fallback pass),
BROWSER_ARGS (JSON list of extra Chrome/Edge switches; {profile} is the session's profile).
"""
import json
import os
import sys

here = os.path.dirname(os.path.abspath(__file__))
corpus = json.load(open(os.path.join(here, "corpus.json")))
mode, runs, browsers, scen = sys.argv[1], int(sys.argv[2]), sys.argv[3].split(","), sys.argv[4].split(",")
pool = corpus["live"] if mode == "live" else corpus["pages"]
if len(sys.argv) > 5:
    want = sys.argv[5].split(",")
    pool = [p for p in corpus["pages"] + corpus["live"] if p["id"] in want]
cfg = {
    "mode": mode,
    "runs": runs,
    "rtt": int(os.environ.get("RTT", "40")),
    "archive": os.environ.get("ARCHIVE", "C:\\pipeline\\krunk\\load\\corpus.wprgo"),
    "pages": [{"id": p["id"], "url": p["url"]} for p in pool],
    "browsers": browsers,
    "scenarios": scen,
    "profiles": "C:\\pipeline\\krunk\\lp",
}
if os.environ.get("WAIT_FOCUS"):
    cfg["waitFocus"] = True
if os.environ.get("COLLECT"):
    cfg["collect"] = os.environ["COLLECT"].split(",")
if os.environ.get("BROWSER_ARGS"):
    cfg["browserArgs"] = json.loads(os.environ["BROWSER_ARGS"])
env = json.loads(os.environ.get("KRUNK_ENV", "{}"))
for b in browsers:
    if b.startswith("krunk"):
        key = b.upper().replace("-", "_")
        cfg[b] = {
            "app": "C:\\pipeline\\krunk\\" + os.environ.get(f"{key}_APP", "app260h"),
            "base": "C:\\pipeline\\krunk\\" + os.environ.get(f"{key}_BASE", "lp-base"),
            "env": env,
        }
print(json.dumps(cfg, indent=1))
