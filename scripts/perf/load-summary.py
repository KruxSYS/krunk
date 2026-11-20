#!/usr/bin/env python3
"""Krunk 0.26.0 page-load harness: tables from one or more passes (results.jsonl).

  load-summary.py DIR [DIR...]          markdown to stdout (later DIRs' sessions replace earlier ones)
  load-summary.py DIR... --json OUT     also the numbers as JSON (with the noise band, if any)
  load-summary.py DIR... --ab A=B [--band baseline.json]
                                        B against A per scenario and metric: kept only if B's
                                        corpus median is lower by more than the noise band (the
                                        A/A band of these dirs, or the one in --band's JSON)

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
# The same times from navigation start (input → navigationStart taken out): a browser's own
# hand-off delay (e.g. Edge's omnibox) apart from its page loading.
NS = ["fcp_ns", "lcp_ns", "load_ns"]
BF = ["show", "frame"]
SCEN_ORDER = ["cold", "warm", "link", "back", "forward"]


def p90(v):
    s = sorted(v)
    return s[max(0, math.ceil(0.9 * len(s)) - 1)]


GROUP = {"cold": "typed", "warm": "typed", "link": "link", "back": "link", "forward": "link"}


def session_key(r):
    return (r["browser"], r["page"], r.get("run"), GROUP.get(r.get("scenario"), r.get("scenario")))


def load(dirs):
    """Rows of every pass; a session (browser start) in a later pass (a fill pass,
    load-fill.py) replaces the same session's rows from the earlier ones."""
    rows = []
    for d in dirs:
        with open(f"{d}/results.jsonl", encoding="utf-8") as f:
            new = [json.loads(line) for line in f if line.strip()]
        replaced = {session_key(r) for r in new}
        rows = [r for r in rows if session_key(r) not in replaced] + new
    for r in rows:
        if isinstance(r.get("nav"), (int, float)):
            for m in ("fcp", "lcp", "load"):
                if isinstance(r.get(m), (int, float)):
                    r[m + "_ns"] = r[m] - r["nav"]
    return rows


def main():
    args = sys.argv[1:]
    jout = None
    pools = []
    abs_ = []
    band_file = None
    while "--ab" in args:
        i = args.index("--ab")
        a_, b_ = args[i + 1].split("=", 1)
        abs_.append((a_, b_))
        del args[i : i + 2]
    if "--band" in args:
        i = args.index("--band")
        band_file = args[i + 1]
        del args[i : i + 2]
    while "--pool" in args:
        i = args.index("--pool")
        name, members = args[i + 1].split("=", 1)
        pools.append((name, members.split(",")))
        del args[i : i + 2]
    if "--json" in args:
        i = args.index("--json")
        jout = args[i + 1]
        del args[i : i + 2]
    rows = load(args)
    # --pool krunk=krunk-a,krunk-b: the same build twice also counts as one browser with all runs.
    for name, members in pools:
        rows += [dict(r, browser=name) for r in rows if r["browser"] in members]
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
        for m in METRICS + BF + NS:
            v = r.get(m)
            if isinstance(v, (int, float)) and r.get("kind") in ("load", "bfcache"):
                vals[k][m].append(v)
        for why in r.get("bfcacheNotUsed") or []:
            reasons[(r["browser"], r.get("scenario"))][why] += 1

    out = {"pages": pages, "browsers": browsers, "corpus": {}, "perpage": {}, "headline": {}}
    print("# Page-load pass: " + ", ".join(args))
    print()
    # The plan's headline: per page, the median (and p90) of every cold and warm typed load
    # pooled; then the median over the pages.
    typed = [s for s in ("cold", "warm") if s in scen]
    if typed:
        print("## Headline: corpus median (p90) over cold + warm typed loads\n")
        print("Per page: median and p90 of the cold and warm loads together; corpus: the median over the pages. ms from the input.\n")
        hm = ["ttfb", "fcp", "lcp", "dcl", "load", "nav", "fcp_ns", "lcp_ns", "load_ns"]
        print("| browser | " + " | ".join(hm) + " | pages |")
        print("|---|" + "---|" * (len(hm) + 1))
        for b in browsers:
            cells = []
            for m in hm:
                meds, p90s, n = [], [], 0
                for p in pages:
                    v = [x for sc in typed for x in vals[(b, sc, p)][m]]
                    if v:
                        meds.append(statistics.median(v))
                        p90s.append(p90(v))
                        n += 1
                if meds:
                    cm, cp = statistics.median(meds), statistics.median(p90s)
                    out["headline"].setdefault(b, {})[m] = [cm, cp, n]
                    cells.append(f"{cm:.0f} ({cp:.0f})")
                else:
                    cells.append("–")
            print(f"| {b} | " + " | ".join(cells) + f" | {out['headline'].get(b, {}).get('lcp', [0, 0, 0])[2]} |")
        ref = [r for r in ("chrome", "edge") if r in out["headline"]]
        if ref:
            print()
            for b in browsers:
                if b in ref or "lcp" not in out["headline"].get(b, {}):
                    continue
                parts = []
                for r in ref:
                    for m in ("fcp", "lcp", "load"):
                        x, y = out["headline"][b].get(m), out["headline"][r].get(m)
                        if x and y:
                            parts.append(f"{m} {100 * (x[0] - y[0]) / y[0]:+.1f} % vs {r}")
                print(f"- {b}: " + ", ".join(parts))
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
        print("Band = the larger of |corpus median diff %| and the median per-page |diff %|; a fix must beat it.\n")
        print("| scenario | metric | corpus median diff | per-page median |diff| (median, max) | band |")
        print("|---|---|---|---|---|")
        for s in scen:
            for m in ["fcp", "lcp", "load", "ttfb", "nav"] if s not in ("back", "forward") else ["frame", "lcp"]:
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
                    band = max(abs(rel), statistics.median(diffs))
                    out.setdefault("band", {}).setdefault(s, {})[m] = round(band, 2)
                    print(f"| {s} | {m} | {y[0] - x[0]:+.0f} ms ({rel:+.1f} %) | {statistics.median(diffs):.1f} %, {max(diffs):.1f} % | {band:.1f} % |")
    # A/B verdicts: B against A on the corpus median, kept only when faster by more than the band.
    if abs_:
        band = out.get("band")
        if band_file:
            with open(band_file) as f:
                band = json.load(f).get("band", band)
        print("\n## A/B verdicts (corpus median (p90), ms from the input)\n")
        if not band:
            print("_No noise band (no A/A pair here and no --band file): no verdicts._\n")
        print("| A → B | scenario | metric | A | B | change | band | verdict |")
        print("|---|---|---|---|---|---|---|---|")
        for a_, b_ in abs_:
            for s in scen:
                for m in ["fcp", "lcp", "load", "ttfb", "nav"] if s not in ("back", "forward") else ["frame", "lcp"]:
                    x = out["corpus"].get(a_, {}).get(s, {}).get(m)
                    y = out["corpus"].get(b_, {}).get(s, {}).get(m)
                    if not x or not y:
                        continue
                    rel = (y[0] - x[0]) / max(x[0], 1) * 100
                    bnd = (band or {}).get(s, {}).get(m)
                    if bnd is None:
                        verdict = "no band"
                    elif rel < -bnd:
                        verdict = "**beats the band**"
                    elif rel > bnd:
                        verdict = "slower (beyond the band)"
                    else:
                        verdict = "within the band"
                    out.setdefault("ab", []).append({"a": a_, "b": b_, "scenario": s, "metric": m, "a_ms": x[:2], "b_ms": y[:2], "change_pct": round(rel, 2), "band_pct": bnd, "verdict": verdict.strip("*")})
                    bs = f"{bnd:.1f} %" if bnd is not None else "–"
                    print(f"| {a_} → {b_} | {s} | {m} | {x[0]:.0f} ({x[1]:.0f}) | {y[0]:.0f} ({y[1]:.0f}) | {rel:+.1f} % | {bs} | {verdict} |")
    if jout:
        with open(jout, "w") as f:
            json.dump(out, f, indent=1)


if __name__ == "__main__":
    main()
