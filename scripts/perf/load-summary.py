#!/usr/bin/env python3
"""Krunk 0.26.0 page-load harness: tables from one or more passes (results.jsonl).

  load-summary.py DIR [DIR...]          markdown to stdout
  load-summary.py DIR... --json OUT     also the numbers as JSON

Per browser and scenario: per page median / p90 (nearest rank) of the times from the input
(ttfb, fcp, lcp, dcl, load; bfcache restores: show and frame), and the corpus value: the median of
the per-page medians (and of the per-page p90s). Errors and timeouts are counted, and the
back/forward-cache reasons listed. Browsers named krunk-a / krunk-b etc. are the same build
twice (A/A): their difference is the noise band.
"""
import json
import math
import statistics
import sys
from collections import defaultdict

METRICS = ["nav", "ttfb", "fcp", "lcp", "dcl", "load"]
BF = ["show", "frame"]
SCEN_ORDER = ["cold", "warm", "link", "back", "forward"]


def p90(v):
    s = sorted(v)
    return s[max(0, math.ceil(0.9 * len(s)) - 1)]


def load(dirs):
    rows = []
    for d in dirs:
        with open(f"{d}/results.jsonl", encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if line:
                    rows.append(json.loads(line))
    return rows


def main():
    args = sys.argv[1:]
    jout = None
    if "--json" in args:
        i = args.index("--json")
        jout = args[i + 1]
        del args[i : i + 2]
    rows = load(args)
    browsers = list(dict.fromkeys(r["browser"] for r in rows))
    pages = list(dict.fromkeys(r["page"] for r in rows))
    scen = [s for s in SCEN_ORDER if any(r.get("scenario") == s for r in rows)]
    # (browser, scenario, page) -> metric -> values
    vals = defaultdict(lambda: defaultdict(list))
    kinds = defaultdict(lambda: defaultdict(int))
    reasons = defaultdict(lambda: defaultdict(int))
    for r in rows:
        k = (r["browser"], r.get("scenario"), r["page"])
        kinds[(r["browser"], r.get("scenario"))][r.get("kind", "?")] += 1
        for m in METRICS + BF:
            v = r.get(m)
            if isinstance(v, (int, float)) and r.get("kind") in ("load", "bfcache"):
                vals[k][m].append(v)
        for why in r.get("bfcacheNotUsed") or []:
            reasons[(r["browser"], r.get("scenario"))][why] += 1

    out = {"pages": pages, "browsers": browsers, "corpus": {}, "perpage": {}}
    print("# Page-load pass: " + ", ".join(args))
    print()
    print("Times in ms from the input (median / p90 over the runs). Corpus = median over pages of the per-page median (p90: of the per-page p90).")
    for s in scen:
        ms = BF + ["fcp", "lcp", "load"] if s in ("back", "forward") else METRICS
        print(f"\n## {s}\n")
        print("| browser | " + " | ".join(ms) + " | results |")
        print("|---|" + "---|" * (len(ms) + 1))
        for b in browsers:
            cells = []
            for m in ms:
                meds = [statistics.median(vals[(b, s, p)][m]) for p in pages if vals[(b, s, p)][m]]
                p90s = [p90(vals[(b, s, p)][m]) for p in pages if vals[(b, s, p)][m]]
                if meds:
                    cm, cp = statistics.median(meds), statistics.median(p90s)
                    out["corpus"].setdefault(b, {}).setdefault(s, {})[m] = [cm, cp, len(meds)]
                    cells.append(f"{cm:.0f} / {cp:.0f}" + ("" if len(meds) == len(pages) else f" ({len(meds)}p)"))
                else:
                    cells.append("–")
            kd = ", ".join(f"{k} {n}" for k, n in sorted(kinds[(b, s)].items()))
            print(f"| {b} | " + " | ".join(cells) + f" | {kd} |")
        # Per page: LCP (or frame for back/forward) per browser.
        m = "lcp" if s not in ("back", "forward") else "frame"
        print(f"\nPer page, {m} (median / p90, n):\n")
        print("| page | " + " | ".join(browsers) + " |")
        print("|---|" + "---|" * len(browsers))
        for p in pages:
            cells = []
            for b in browsers:
                v = vals[(b, s, p)][m] or ([] if m != "frame" else vals[(b, s, p)]["lcp"])
                if v:
                    out["perpage"].setdefault(b, {}).setdefault(s, {})[p] = [statistics.median(v), p90(v), len(v)]
                    cells.append(f"{statistics.median(v):.0f} / {p90(v):.0f} ({len(v)})")
                else:
                    cells.append("–")
            print(f"| {p} | " + " | ".join(cells) + " |")
        rs = {b: reasons[(b, s)] for b in browsers if reasons[(b, s)]}
        if rs:
            print("\nBack/forward cache not used:\n")
            for b, rr in rs.items():
                print(f"- {b}: " + ", ".join(f"{k} ×{n}" for k, n in sorted(rr.items(), key=lambda x: -x[1])))

    # A/A noise: same build twice (krunk-a / krunk-b).
    aa = [b for b in browsers if b.endswith("-a")]
    for a in aa:
        b2 = a[:-2] + "-b"
        if b2 not in browsers:
            continue
        print(f"\n## Noise band ({a} vs {b2}, same build)\n")
        print("| scenario | metric | corpus median diff | per-page median |diff| (median, max) |")
        print("|---|---|---|---|")
        for s in scen:
            for m in ["fcp", "lcp", "load"] if s not in ("back", "forward") else ["frame", "lcp"]:
                x = out["corpus"].get(a, {}).get(s, {}).get(m)
                y = out["corpus"].get(b2, {}).get(s, {}).get(m)
                if not x or not y:
                    continue
                diffs = []
                for p in pages:
                    va, vb = vals[(a, s, p)][m], vals[(b2, s, p)][m]
                    if va and vb:
                        ma, mb = statistics.median(va), statistics.median(vb)
                        diffs.append(abs(ma - mb) / max(ma, mb, 1) * 100)
                rel = (y[0] - x[0]) / max(x[0], 1) * 100
                if diffs:
                    print(f"| {s} | {m} | {y[0] - x[0]:+.0f} ms ({rel:+.1f} %) | {statistics.median(diffs):.1f} %, {max(diffs):.1f} % |")
    if jout:
        with open(jout, "w") as f:
            json.dump(out, f, indent=1)


if __name__ == "__main__":
    main()
