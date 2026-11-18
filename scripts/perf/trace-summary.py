#!/usr/bin/env python3
"""Krunk 0.25.0: summarises the app's switch trace (KRUNK_PERF_TRACE=1 -> perf.log).

  trace-summary.py PERF.LOG [EVENTS.csv] [--switch-json PATH.switch.json] [--json OUT]

perf.log: first line `qpf <freq>`, then one record per line, space-separated key=value after the
record kind:
  switch|open|close qpc_in= tab= path=ready|out_of_sight|not_loaded|minimized act_ms= apply_ms=
                    send_tabs_ms= send_nav_ms= layout_ms= wait_end=painted|frames|timeout|none
                    wait_ms= swap_qpc= capture_ms=
  stall qpc= kind= ms=        a UI-thread message/task that ran > 16 ms
  move  qpc= ms=              one drag move's handling time
Missing keys and unknown records are tolerated.

Printed (Markdown): per record kind and reveal path the count and median/p95/max of every phase,
input -> swap, the wait's end reasons; UI-thread stalls > 16 ms by kind; drag moves. With the
driver's EVENTS.csv (scripts/vm-0250-tabs.ps1) each event is joined to the first switch/open/
close record whose qpc_in follows it (before the next event): delivery (event -> qpc_in) and
event -> swap; with --switch-json (analyze.py switch's output for the same events) also the
visual latency, and for every event >= 250 ms its phases and the stalls inside it.
"""
import argparse
import json
import re
import statistics
import sys

PHASES = ["act_ms", "apply_ms", "send_tabs_ms", "send_nav_ms", "layout_ms", "wait_ms", "capture_ms"]
REVEALS = ("switch", "open", "close")


def num(v):
    try:
        return float(v)
    except (TypeError, ValueError):
        return None


def parse(path):
    qpf, recs = None, []
    for line in open(path, encoding="utf-8-sig", errors="replace"):
        parts = line.split()
        if not parts:
            continue
        if parts[0].startswith("qpf"):
            m = re.match(r"qpf[=\s]*([\d.]+)", line.strip())
            if m:
                qpf = float(m.group(1))
            elif len(parts) > 1:
                qpf = num(parts[1])
            continue
        r = {"_rec": parts[0]}
        for p in parts[1:]:
            if "=" in p:
                k, v = p.split("=", 1)
                r[k] = v
        recs.append(r)
    return qpf, recs


def load_events(path):
    lines = [l.strip() for l in open(path, encoding="utf-8-sig") if l.strip()]
    qpf = float(re.split(r"[,=]", lines[0])[1])
    ev = []
    for l in lines[1:]:
        p = [x.strip() for x in l.split(",")]
        if len(p) >= 3 and p[0].lstrip("-").isdigit():
            ev.append({"qpc": int(p[0]), "kind": p[1], "target": int(p[2])})
    ev.sort(key=lambda e: e["qpc"])
    return qpf, ev


def stats(v):
    v = sorted(x for x in v if x is not None)
    if not v:
        return None
    p95 = v[min(len(v) - 1, int(round(0.95 * (len(v) - 1))))]
    return {"n": len(v), "median": round(statistics.median(v), 1), "p95": round(p95, 1), "max": round(v[-1], 1)}


def fmt(s):
    return "-" if not s else "%.1f / %.1f / %.1f" % (s["median"], s["p95"], s["max"])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("perf")
    ap.add_argument("events", nargs="?")
    ap.add_argument("--switch-json", help="analyze.py switch output for the same events")
    ap.add_argument("--json", help="write the summary as JSON here")
    ap.add_argument("--slow", type=float, default=250.0)
    a = ap.parse_args()
    qpf, recs = parse(a.perf)
    if not qpf:
        qpf = 10_000_000.0
        print("_perf.log has no qpf line: assuming 10 MHz_\n")
    ms = lambda d: d * 1000.0 / qpf  # noqa: E731
    out = {"perf": a.perf, "records": len(recs), "reveals": {}, "stalls": {}, "moves": None}

    for r in recs:
        if r["_rec"] in REVEALS:
            q_in, swap = num(r.get("qpc_in")), num(r.get("swap_qpc"))
            r["in_to_swap_ms"] = ms(swap - q_in) if q_in is not None and swap is not None and swap > 0 else None

    print("### Reveal phases (median / p95 / max ms)\n")
    print("| record | path | n | timeouts | " + " | ".join(p[:-3] for p in PHASES) + " | input→swap | wait ends |")
    print("|---|---|---|---|" + "---|" * len(PHASES) + "---|---|")
    groups = {}
    for r in recs:
        if r["_rec"] in REVEALS:
            groups.setdefault((r["_rec"], r.get("path", "?")), []).append(r)
    for (kind, path), rs in sorted(groups.items()):
        ends = {}
        for r in rs:
            ends[r.get("wait_end", "?")] = ends.get(r.get("wait_end", "?"), 0) + 1
        st = {p: stats([num(r.get(p)) for r in rs]) for p in PHASES}
        st["in_to_swap_ms"] = stats([r["in_to_swap_ms"] for r in rs])
        out["reveals"]["%s/%s" % (kind, path)] = {"count": len(rs), "timeouts": ends.get("timeout", 0), "wait_end": ends, **st}
        print("| %s | %s | %d | %d | %s | %s | %s |" % (kind, path, len(rs), ends.get("timeout", 0),
              " | ".join(fmt(st[p]) for p in PHASES), fmt(st["in_to_swap_ms"]),
              ", ".join("%s %d" % kv for kv in sorted(ends.items()))))

    stalls = [r for r in recs if r["_rec"] == "stall" and (num(r.get("ms")) or 0) > 16]
    print("\n### UI-thread stalls > 16 ms\n")
    if stalls:
        print("| kind | n | median / p95 / max ms | total ms |\n|---|---|---|---|")
        by = {}
        for r in stalls:
            by.setdefault(r.get("kind", "?"), []).append(num(r.get("ms")))
        for k, v in sorted(by.items(), key=lambda kv: -sum(kv[1])):
            s = stats(v)
            out["stalls"][k] = {**s, "total": round(sum(v), 1)}
            print("| %s | %d | %s | %.0f |" % (k, s["n"], fmt(s), sum(v)))
    else:
        print("none")

    moves = [num(r.get("ms")) for r in recs if r["_rec"] == "move"]
    if moves:
        out["moves"] = stats(moves)
        print("\n### Drag moves\n\n%d moves, handling median / p95 / max %s ms" % (len(moves), fmt(out["moves"])))

    if a.events:
        eqpf, evs = load_events(a.events)
        if abs(eqpf - qpf) > 1:
            print("\n_warning: events and trace have different QPC frequencies_")
        vis = {}
        if a.switch_json:
            for e in json.load(open(a.switch_json)).get("events", []):
                vis[e["qpc"]] = e
        reveals = sorted((r for r in recs if r["_rec"] in REVEALS and num(r.get("qpc_in")) is not None), key=lambda r: num(r["qpc_in"]))
        joined = []
        for i, e in enumerate(evs):
            nxt = evs[i + 1]["qpc"] if i + 1 < len(evs) else float("inf")
            rec = next((r for r in reveals if e["qpc"] <= num(r["qpc_in"]) < min(nxt, e["qpc"] + 2 * qpf)), None)
            j = {**e, "n": i}
            v = vis.get(e["qpc"])
            if v:
                j["visual_ms"] = v.get("latency_ms")
                j["status"] = v.get("status")
            if rec:
                j.update({"record": rec["_rec"], "path": rec.get("path"), "wait_end": rec.get("wait_end"),
                          "deliver_ms": round(ms(num(rec["qpc_in"]) - e["qpc"]), 1)})
                if num(rec.get("swap_qpc")):
                    j["swap_ms"] = round(ms(num(rec["swap_qpc"]) - e["qpc"]), 1)
                for p in PHASES:
                    if num(rec.get(p)) is not None:
                        j[p] = num(rec.get(p))
            end = e["qpc"] + (j.get("visual_ms") or j.get("swap_ms") or 500) * qpf / 1000.0
            j["stalls"] = ["%s %.0f" % (s.get("kind", "?"), num(s.get("ms"))) for s in stalls
                           if num(s.get("qpc")) is not None and e["qpc"] <= num(s["qpc"]) <= end]
            joined.append(j)
        out["events"] = joined
        print("\n### Events joined to the trace\n")
        print("| kind | n | with record | deliver ms | event→swap ms | visual ms |\n|---|---|---|---|---|---|")
        kinds = {}
        for j in joined:
            kinds.setdefault(j["kind"], []).append(j)
        for k, js in sorted(kinds.items()):
            print("| %s | %d | %d | %s | %s | %s |" % (k, len(js), sum(1 for j in js if "record" in j),
                  fmt(stats([j.get("deliver_ms") for j in js])), fmt(stats([j.get("swap_ms") for j in js])),
                  fmt(stats([j.get("visual_ms") for j in js]))))
        slow = [j for j in joined if (j.get("visual_ms") or j.get("swap_ms") or 0) >= a.slow]
        if slow:
            print("\n### Events >= %.0f ms\n" % a.slow)
            print("| # | kind | target | visual | swap | deliver | path | wait end | act/apply/tabs/nav/layout/wait/capture | stalls |")
            print("|---|---|---|---|---|---|---|---|---|---|")
            for j in slow:
                ph = "/".join("-" if j.get(p) is None else "%.0f" % j[p] for p in PHASES)
                print("| %d | %s | %d | %s | %s | %s | %s | %s | %s | %s |" % (j["n"], j["kind"], j["target"], j.get("visual_ms", "-"),
                      j.get("swap_ms", "-"), j.get("deliver_ms", "-"), j.get("path", "-"), j.get("wait_end", "-"), ph,
                      ", ".join(j["stalls"][:6]) or "-"))
    if a.json:
        with open(a.json, "w") as f:
            json.dump(out, f, indent=1)
    return 0


if __name__ == "__main__":
    sys.exit(main())
