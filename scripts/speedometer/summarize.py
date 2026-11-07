#!/usr/bin/env python3
"""Summarises Speedometer runs (scripts/speedometer/matrix.sh output) as Markdown tables.

  summarize.py DIR [DIR ...] [--ref CH] [--operator FILE.json]

Runs are grouped by their tag without the trailing run number (A1, A2, A3 -> A). Prints one
table of scores per group (mean, min, max, runs, ratio to the reference group) and one table of
per-suite means (ms) for every group, plus the operator's own export if given.
"""
import argparse
import glob
import json
import os
import re
import statistics as st


def load_dir(d):
    runs = {}
    status = {}
    rt = os.path.join(d, "runs.txt")
    if os.path.exists(rt):
        for line in open(rt):
            f = line.split()
            if len(f) >= 6:
                status[f[0]] = (f[1], f[2], f[3], f[4] if f[4].startswith("off=") else "", line.strip())
    for p in sorted(glob.glob(os.path.join(d, "*.json"))):
        tag = os.path.basename(p)[:-5]
        try:
            runs[tag] = json.load(open(p))
        except ValueError:
            pass
    return runs, status


def suites(d):
    return [k for k in d if "/" not in k and not k.startswith("Iteration") and k not in ("Geomean", "Score")]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("dirs", nargs="+")
    ap.add_argument("--ref", default="CH")
    ap.add_argument("--operator")
    a = ap.parse_args()
    groups = {}
    labels = {}
    for d in a.dirs:
        runs, status = load_dir(d)
        for tag, data in runs.items():
            g = re.sub(r"\d+$", "", tag)
            groups.setdefault(g, []).append(data)
            if tag in status:
                b, prof, bg, off, _ = status[tag]
                labels[g] = f"{b} {prof} {bg} {off}".strip()
    ref = groups.get(a.ref)
    ref_mean = st.mean(d["Score"]["mean"] for d in ref) if ref else None
    print("| Group | What | Score (mean) | min – max | runs | vs " + a.ref + " |")
    print("|---|---|---|---|---|---|")
    for g in sorted(groups):
        s = [d["Score"]["mean"] for d in groups[g]]
        ratio = f"{st.mean(s) / ref_mean * 100 - 100:+.1f} %" if ref_mean else ""
        print(f"| {g} | {labels.get(g, '')} | {st.mean(s):.2f} | {min(s):.2f} – {max(s):.2f} | {len(s)} | {ratio} |")
    print()
    cols = sorted(groups)
    names = suites(next(iter(groups.values()))[0])
    op = json.load(open(a.operator)) if a.operator else None
    head = "| Suite (ms) | " + " | ".join(cols) + (" | operator |" if op else " |")
    print(head)
    print("|---" * (len(cols) + 1 + (1 if op else 0)) + "|")
    for n in names:
        row = [f"{st.mean(d[n]['mean'] for d in groups[c]):.1f}" for c in cols]
        if op:
            row.append(f"{op[n]['mean']:.1f}" if n in op else "")
        print(f"| {n} | " + " | ".join(row) + " |")


if __name__ == "__main__":
    main()
