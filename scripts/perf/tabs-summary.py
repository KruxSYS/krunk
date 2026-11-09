#!/usr/bin/env python3
"""Krunk 0.10.0 (P1): one table row per vm-0100-tabs.ps1 run.

    tabs-summary.py RUN_DIR [RUN_DIR ...]

Each RUN_DIR is a vm-batch output folder holding run.log and tabs-<tag>-reports.txt. A row has:
the browser and tab count; CPU % of all cores (total and by process type) over the 30-s window and
the machine's load; the background tabs' median frames/s and timer ticks/s and how many said
'hidden'; the active tab's frames/s and 16-ms timer lateness (median of the 50th and 95th
percentiles); key → frame latency (median, p95) in the page's own clock and from the harness's key
press (input queuing included); tab-switch latency (key → the tab's focus event).
"""
import glob
import os
import re
import statistics
import sys


def med(xs):
    return statistics.median(xs) if xs else float("nan")


def p95(xs):
    if not xs:
        return float("nan")
    s = sorted(xs)
    return s[min(len(s) - 1, int(0.95 * len(s)))]


def row(d):
    run = open(os.path.join(d, "run.log"), encoding="utf-8", errors="replace").read()
    rep_files = glob.glob(os.path.join(d, "tabs-*-reports.txt"))
    reps = open(rep_files[0], encoding="utf-8-sig", errors="replace").read().splitlines() if rep_files else []
    m = re.search(r"\[tabs-(\S+)\] (\w+) (\S+): (\d+) tabs", run)
    tag, browser, build, n = (m.group(1), m.group(2), m.group(3), int(m.group(4))) if m else (os.path.basename(d), "?", "?", 0)
    cpu = re.search(r"RESULT cpu total=([\d.,]+)% of \d+ cores; (.*?); machine=([\d.,]+)%; working set=([\d,]+) MB; processes=(\d+)", run)
    num = lambda s: float(s.replace(",", ""))
    bg_raf, bg_tim, bg_hidden, bg_total = [], [], 0, 0
    fg_raf, late50, late95, keys, key_at = [], [], [], [], []
    focus = {}
    seen = set()
    for line in reps:
        parts = line.split(" ", 2)
        if len(parts) < 3:
            continue
        rest = parts[2] if parts[1].startswith(tag) is False else parts[1] + " " + parts[2]
        mm = re.match(rf"{re.escape(tag)}-(\S+) (.*)", rest)
        if not mm:
            continue
        who, msg = mm.group(1), mm.group(2)
        if who == "fg":
            if (k := re.search(r"key ms=([\d.]+)(?: at=(\d+))?", msg)):
                keys.append(float(k.group(1)))
                if k.group(2):
                    key_at.append(int(k.group(2)))
            elif (r := re.search(r"raf=([\d.]+) late50=([\d.]+) late95=([\d.]+)", msg)):
                fg_raf.append(float(r.group(1))); late50.append(float(r.group(2))); late95.append(float(r.group(3)))
            continue
        if (f := re.search(r"focus at=(\d+)", msg)):
            focus.setdefault(who, []).append(int(f.group(1)))
            continue
        if (r := re.search(r"st=(\w+) raf=([\d.]+) tim=([\d.]+)", msg)):
            if who not in seen:  # the first report includes the page's load
                seen.add(who)
                continue
            bg_total += 1
            bg_hidden += r.group(1) == "hidden"
            bg_raf.append(float(r.group(2))); bg_tim.append(float(r.group(3)))
    sw = []
    sm = re.search(r"switches (.*)", run)
    if sm:
        for item in sm.group(1).split():
            k, t = item.split("@")
            ts = [x for x in focus.get(f"bg{int(k) - 1}", []) if x >= int(t)]
            if ts:
                sw.append(min(ts) - int(t))
    # Key → frame including input queuing: the harness's send times paired in order with the
    # pages' frame times (only when every key arrived).
    kq = []
    km = re.search(r"\] keys (.*)", run)
    if km:
        sent = [int(x) for x in km.group(1).split()]
        if len(sent) == len(key_at):
            kq = [b - a for a, b in zip(sent, sorted(key_at))]
    spm = re.search(r"RESULT speedometer score=(\S+)", run)
    return {
        "speedometer": spm.group(1) if spm else "",
        "run": tag, "browser": f"{browser} {build}", "tabs": n,
        "cpu": num(cpu.group(1)) if cpu else float("nan"), "by_type": cpu.group(2) if cpu else "",
        "machine": num(cpu.group(3)) if cpu else float("nan"), "ws_mb": num(cpu.group(4)) if cpu else float("nan"),
        "procs": int(cpu.group(5)) if cpu else 0,
        "bg_raf": med(bg_raf), "bg_tim": med(bg_tim), "bg_hidden": f"{bg_hidden}/{bg_total}",
        "fg_raf": med(fg_raf), "late50": med(late50), "late95": med(late95),
        "key_med": med(keys), "key_p95": p95(keys), "keyq_med": med(kq), "keyq_p95": p95(kq), "keys": f"{len(keys)}", "switch_med": med(sw), "switches": len(sw),
    }


def main():
    rows = [row(d) for d in sys.argv[1:]]
    cols = ["run", "browser", "tabs", "speedometer", "cpu", "machine", "ws_mb", "procs", "bg_raf", "bg_tim", "bg_hidden",
            "fg_raf", "late50", "late95", "keys", "key_med", "key_p95", "keyq_med", "keyq_p95", "switch_med"]
    print("| " + " | ".join(cols) + " |")
    print("|" + "---|" * len(cols))
    for r in rows:
        cells = [(f"{r[c]:.1f}" if isinstance(r[c], float) else str(r[c])) for c in cols]
        print("| " + " | ".join(cells) + " |")
    print()
    for r in rows:
        print(f"- {r['run']}: {r['by_type']}")


if __name__ == "__main__":
    main()
