#!/usr/bin/env python3
"""Krunk 0.26.0: summarises the load trace (KRUNK_PERF_TRACE=1 -> perf.log, `load` lines).

  load-trace-summary.py PERF.LOG [--kind KIND] [--json OUT] [--all]

One `load` line per top-level navigation of a tab (lb_core::perftrace::Load):
  load tab= kind=omnibox|suggest|ntp|open_url|back|forward|reload|host|link end=done|replaced
       qpc0= input= load_url= before_browse= main_request= load_start= commit= painted= load_end=
       loading_done=   (ms after qpc0, the first step seen; `-` = not seen)
       redirects= status= ui_ms= ui=name:count:ms,...
       req= blocked= filter_us= filter_max_us= cookie= cookie_us= cookie_max_us=
Other records (switch, stall, span ...) are skipped; trace-summary.py reads those.

Printed (Markdown): every navigation (with --all; else the last 40) with its steps and the
gaps between them; medians / p95 / max per kind of the steps and the gaps; Krunk's UI-thread
work by name; request filtering and cookie filter cost totals.
"""
import argparse
import json
import statistics
import sys

MARKS = ["input", "load_url", "before_browse", "main_request", "load_start", "commit", "painted",
         "load_end", "loading_done"]
# Gaps between consecutive steps worth reading on their own (whose time it is).
GAPS = [
    ("input", "load_url", "Krunk: input to engine call"),
    ("load_url", "before_browse", "engine: call to navigation start"),
    ("before_browse", "main_request", "engine: start to document request"),
    ("main_request", "load_start", "network + engine: request to load start (commit)"),
    ("load_start", "commit", "engine: start to commit"),
    ("commit", "painted", "renderer: commit to first paint"),
    ("commit", "load_end", "page: commit to load end"),
    ("load_end", "loading_done", "load end to loading done"),
]


def num(v):
    try:
        return float(v)
    except (TypeError, ValueError):
        return None


def parse(path):
    loads = []
    for line in open(path, encoding="utf-8-sig", errors="replace"):
        parts = line.split()
        if not parts or parts[0] != "load":
            continue
        r = {}
        for p in parts[1:]:
            if "=" in p:
                k, v = p.split("=", 1)
                r[k] = v
        ui = {}
        for item in (r.get("ui") or "-").split(","):
            f = item.split(":")
            if len(f) == 3 and num(f[1]) is not None and num(f[2]) is not None:
                ui[f[0]] = (int(num(f[1])), num(f[2]))
        r["_ui"] = ui
        loads.append(r)
    return loads


def stats(v):
    v = sorted(x for x in v if x is not None)
    if not v:
        return None
    p95 = v[min(len(v) - 1, int(round(0.95 * (len(v) - 1))))]
    return {"n": len(v), "median": round(statistics.median(v), 2), "p95": round(p95, 2), "max": round(v[-1], 2),
            "total": round(sum(v), 2)}


def fmt(s):
    return "-" if not s else "%.1f / %.1f / %.1f" % (s["median"], s["p95"], s["max"])


def gap(r, a, b):
    x, y = num(r.get(a)), num(r.get(b))
    return y - x if x is not None and y is not None else None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("perf")
    ap.add_argument("--kind", help="only navigations of this kind (omnibox, link ...)")
    ap.add_argument("--all", action="store_true", help="list every navigation, not the last 40")
    ap.add_argument("--json", help="write the summary as JSON here")
    a = ap.parse_args()
    loads = [r for r in parse(a.perf) if not a.kind or r.get("kind") == a.kind]
    out = {"perf": a.perf, "navigations": len(loads), "kinds": {}, "ui": {}, "filter": {}}
    if not loads:
        print("no load lines in %s (was KRUNK_PERF_TRACE=1 set, with a 0.26.0+ build?)" % a.perf)
        return 1

    shown = loads if a.all else loads[-40:]
    print("### Navigations (ms after the first step)%s\n" % ("" if a.all or len(loads) <= 40 else ", last 40"))
    print("| tab | kind | end | " + " | ".join(MARKS) + " | redirects | status | ui ms | req | blocked | filter ms |")
    print("|---|---|---|" + "---|" * len(MARKS) + "---|---|---|---|---|---|")
    for r in shown:
        f_ms = num(r.get("filter_us"))
        print("| %s | %s | %s | %s | %s | %s | %s | %s | %s | %s |" % (
            r.get("tab", "?"), r.get("kind", "?"), r.get("end", "?"),
            " | ".join(r.get(m, "-") for m in MARKS), r.get("redirects", "-"), r.get("status", "-"),
            r.get("ui_ms", "-"), r.get("req", "-"), r.get("blocked", "-"),
            "-" if f_ms is None else "%.2f" % (f_ms / 1000.0)))

    print("\n### Steps by kind (median / p95 / max ms after the first step)\n")
    kinds = {}
    for r in loads:
        kinds.setdefault(r.get("kind", "?"), []).append(r)
    print("| kind | n | replaced | " + " | ".join(MARKS) + " | ui ms |")
    print("|---|---|---|" + "---|" * len(MARKS) + "---|")
    for k, rs in sorted(kinds.items()):
        st = {m: stats([num(r.get(m)) for r in rs]) for m in MARKS}
        st["ui_ms"] = stats([num(r.get("ui_ms")) for r in rs])
        replaced = sum(1 for r in rs if r.get("end") == "replaced")
        out["kinds"][k] = {"count": len(rs), "replaced": replaced, "steps": st}
        print("| %s | %d | %d | %s | %s |" % (k, len(rs), replaced, " | ".join(fmt(st[m]) for m in MARKS),
                                             fmt(st["ui_ms"])))

    print("\n### Gaps between steps, all kinds (median / p95 / max ms)\n")
    print("| from | to | whose | n | ms |\n|---|---|---|---|---|")
    out["gaps"] = {}
    for x, y, who in GAPS:
        s = stats([gap(r, x, y) for r in loads])
        out["gaps"]["%s-%s" % (x, y)] = s
        print("| %s | %s | %s | %d | %s |" % (x, y, who, s["n"] if s else 0, fmt(s)))

    print("\n### Krunk's UI-thread work on the navigation path\n")
    by = {}
    for r in loads:
        for name, (n, ms) in r["_ui"].items():
            e = by.setdefault(name, {"navs": 0, "calls": 0, "per_nav": []})
            e["navs"] += 1
            e["calls"] += n
            e["per_nav"].append(ms)
    if by:
        print("| name | navigations | calls | ms per navigation (median / p95 / max) | total ms |\n|---|---|---|---|---|")
        for name, e in sorted(by.items(), key=lambda kv: -sum(kv[1]["per_nav"])):
            s = stats(e["per_nav"])
            out["ui"][name] = {"navigations": e["navs"], "calls": e["calls"], **s}
            print("| %s | %d | %d | %s | %.1f |" % (name, e["navs"], e["calls"], fmt(s), s["total"]))
        print("\n_handlers (address, title, favicon, loading, load_start, load_end, load_finished, painted, "
              "before_browse) add up to ui ms; the other names are steps inside them._")
    else:
        print("none")

    print("\n### Request filtering\n")
    req = sum(int(num(r.get("req")) or 0) for r in loads)
    blocked = sum(int(num(r.get("blocked")) or 0) for r in loads)
    f_us = sum(num(r.get("filter_us")) or 0 for r in loads)
    f_max = max((num(r.get("filter_max_us")) or 0 for r in loads), default=0)
    ck = sum(int(num(r.get("cookie")) or 0) for r in loads)
    c_us = sum(num(r.get("cookie_us")) or 0 for r in loads)
    c_max = max((num(r.get("cookie_max_us")) or 0 for r in loads), default=0)
    out["filter"] = {"requests": req, "blocked": blocked, "filter_us": round(f_us), "filter_max_us": f_max,
                     "per_nav_ms": stats([(num(r.get("filter_us")) or 0) / 1000.0 for r in loads]),
                     "cookie_calls": ck, "cookie_us": round(c_us), "cookie_max_us": c_max}
    print("| | count | total ms | µs per call | max µs | ms per navigation (median / p95 / max) |")
    print("|---|---|---|---|---|---|")
    print("| policy (check + started) | %d (%d blocked) | %.1f | %.1f | %.0f | %s |" % (
        req, blocked, f_us / 1000.0, f_us / req if req else 0, f_max, fmt(out["filter"]["per_nav_ms"])))
    print("| cookie filter | %d | %.1f | %.1f | %.0f | %s |" % (
        ck, c_us / 1000.0, c_us / ck if ck else 0, c_max,
        fmt(stats([(num(r.get("cookie_us")) or 0) / 1000.0 for r in loads]))))
    print("\n_IO thread; requests after a navigation's loading ended count for the next one._")

    if a.json:
        with open(a.json, "w") as f:
            json.dump(out, f, indent=1)
    return 0


if __name__ == "__main__":
    sys.exit(main())
