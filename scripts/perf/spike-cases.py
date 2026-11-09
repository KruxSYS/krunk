#!/usr/bin/env python3
"""Krunk 0.10.0 (P3): the required cases of one vm-0100-spike.ps1 run, from its page reports.

    spike-cases.py RUN_DIR        (holds run.log and spike-<mode>-reports.txt)

Prints one line per case with the facts it rests on and PASS/FAIL. The screenshots (wake: not
blank, DevTools, print) are looked at separately.
"""
import glob
import os
import re
import sys

d = sys.argv[1]
run = open(os.path.join(d, "run.log"), encoding="utf-8", errors="replace").read()
rep_file = glob.glob(os.path.join(d, "spike-*-reports.txt"))[0]
events = []  # (ms, who, text)
for line in open(rep_file, encoding="utf-8-sig", errors="replace"):
    m = re.match(r"(\d+) sp\w+-(\w+) (.*)", line.strip())
    if m:
        events.append((int(m.group(1)), m.group(2), m.group(3)))
marks = [(t, x) for t, w, x in events if w == "harness"]


def mark(text):
    return next(t for t, x in marks if x.startswith(text))


def reports(who, a, b):
    return [(t, x) for t, w, x in events if w == who and a <= t < b and x.startswith("st=")]


def num(x, key):
    m = re.search(rf"{key}=([\d.]+)", x)
    return float(m.group(1)) if m else None


rows = []
front, show2 = mark("tab 1 in front"), mark("show tab 2")
# Background tabs while tab 1 is in front (after a 10-s settle).
for who in ("rtc", "notify", "dl", "busy"):
    r = reports(who, front + 10_000, show2)
    hidden = all("st=hidden" in x for _, x in r)
    rafs = [num(x, "raf") for _, x in r]
    rows.append((f"{who} hidden & throttled", hidden and r and max(rafs) <= 1, f"{len(r)} reports, max raf {max(rafs) if rafs else '-'}"))
# Audio keeps playing while hidden: not paused, position moving.
r = reports("audio", front + 10_000, show2)
ts = [num(x, "t") for _, x in r]
rows.append(("audio plays hidden", r and all("paused=false" in x for _, x in r) and len(set(ts)) > 3, f"positions {ts[:6]}"))
# WebRTC: frames decoded keep pace while hidden.
r = reports("rtc", front + 10_000, show2)
dec = [num(x, "decoded") for _, x in r]
rate = (dec[-1] - dec[0]) / ((r[-1][0] - r[0][0]) / 1000) if len(dec) > 1 else 0
rows.append(("webrtc flows hidden", "connected" in r[-1][1] and rate > 15, f"{rate:.1f} frames/s decoded, {r[-1][1][-60:]}"))
# Notification shown while hidden.
n = [x for _, w, x in events if w == "notify" and x.startswith("notify shown")]
rows.append(("notification from hidden tab", n and "st=hidden" in n[0], n[0] if n else "none"))
# Download complete.
m = re.search(r"download: (\d+) bytes", run)
rows.append(("download completes", m and int(m.group(1)) == 20 * 1024 * 1024, m.group(0) if m else "missing"))
# State kept on wake.
last = [x for _, w, x in events if w == "state" and "value=" in x][-1]
rows.append(("state kept (field, scroll)", 'value="krunk-state-123"' in last and "scroll=1400" in last, last[-45:]))
# Wake: each tab visible with frames within 5 s of being shown.
for n_, who in zip(range(2, 7), ("audio", "rtc", "notify", "dl", "busy")):
    t = mark(f"show tab {n_}")
    r = [x for tt, w, x in events if w == who and t <= tt < t + 7_000 and ("visibility=visible" in x or "st=visible" in x)]
    rows.append((f"tab {n_} ({who}) wakes visible", bool(r), f"{len(r)} visible reports"))
# Window states with tab 1 in front.
mn, rs, cv, uc, mv = mark("minimised"), mark("restored"), mark("covered"), mark("uncovered"), mark("moved to the right")
r = reports("state", mn + 4_000, rs)
rows.append(("minimised: tab 1 hidden", r and all("st=hidden" in x for _, x in r), f"{len(r)} reports"))
r = reports("state", rs + 6_000, cv)
rows.append(("restored: tab 1 visible, full rate", r and all("st=visible" in x and num(x, "raf") > 50 for _, x in r), f"{[num(x, 'raf') for _, x in r]}"))
r = reports("busy", rs + 6_000, cv) + reports("rtc", rs + 6_000, cv)
rows.append(("restored: background still hidden", r and all("st=hidden" in x and num(x, "raf") <= 1 for _, x in r), f"{len(r)} reports"))
r = reports("state", cv + 4_000, uc)
rows.append(("covered: tab 1 not broken", r and all(num(x, "raf") > 50 or "st=hidden" in x for _, x in r), f"{[(x.split()[0], num(x, 'raf')) for _, x in r]}"))
r = reports("state", mv + 3_000, mark("end"))
rows.append(("moved/maximised/back: tab 1 visible", r and all("st=visible" in x and num(x, "raf") > 50 for _, x in r), f"{[num(x, 'raf') for _, x in r]}"))
pd = re.search(r"print dialog (\w+ ?\w*), still open: (\w+)", run)
rows.append(("print dialog opens", pd and pd.group(1) == "seen", pd.group(0) if pd else "no line"))

fails = 0
for name, ok, facts in rows:
    fails += not ok
    print(f"{'PASS' if ok else 'FAIL'}  {name}: {facts}")
print(f"{len(rows) - fails}/{len(rows)} passed")
