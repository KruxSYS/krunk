#!/usr/bin/env python3
"""How a dragged window follows the mouse (Krunk 0.11.1, scripts/vm-0111-drag.ps1 part 1).

  draglag.py PATH MOVES.csv [--color 00d0a0 | --marker K] [--tol 30]

PATH is a framecap recording (PATH.raw/.csv/.json; the .json has the recording's start QPC);
MOVES.csv the driver's moves (QPC time, screen x, y; the first line names the QPC frequency).
The window's page shows a square of one colour; its centre per frame is the window's place.
0.25.0: --marker K follows a test page's tab marker band instead (vm-www/8094/marker.js; the
band's centre moves with the window just as the square does).
The moves come in two runs: the tear-off (straight down) and the run at constant speed.

Reported:
  tear_off_ms   from the first move of the tear-off to the first frame that shows the square
                somewhere new (the torn window, with its page)
  lag_ms        during the constant-speed run: per frame, how long ago the cursor was where the
                window shows it (the grab offset is taken at rest, after the release)
  gaps_ms       times between frames in which the window moved (stalls show as long gaps)
  step_cv       how even the per-frame steps are, per ms (std/mean of px per ms)
"""
import argparse
import json
import os
import statistics as st
import sys

import numpy as np


def load(path):
    meta = json.load(open(path + ".json"))
    rows = [l.strip().split(",") for l in open(path + ".csv").read().splitlines()[1:] if l.strip()]
    times = [float(r[1]) for r in rows]
    w, h = meta["width"], meta["height"]
    raw = np.fromfile(path + ".raw", dtype=np.uint8)
    frames = raw[: len(times) * w * h * 3].reshape(len(times), h, w, 3)
    return meta, frames, times


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("path")
    ap.add_argument("moves")
    ap.add_argument("--color", default="00d0a0")
    ap.add_argument("--marker", type=int, help="the colour of tab marker K (markers.py)")
    ap.add_argument("--tol", type=int, default=30)
    a = ap.parse_args()
    if a.marker is not None:
        sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
        import markers
        a.color = markers.hexcolor(a.marker)
    meta, frames, times = load(a.path)
    step = meta["step"]
    color = np.array([int(a.color[k : k + 2], 16) for k in (0, 2, 4)])
    pos = []
    for i, fr in enumerate(frames):
        m = np.all(np.abs(fr.astype(int) - color) <= a.tol, axis=2)
        ys, xs = np.nonzero(m)
        pos.append((times[i], xs.mean() * step + meta["rect"][0], ys.mean() * step + meta["rect"][1]) if len(xs) >= 6 else None)
    lines = open(a.moves).read().splitlines()
    qpf = float(lines[0].split("qpf=")[1])
    start, fq = meta["startQpc"], meta["qpf"]
    assert abs(fq - qpf) < 1, "different QPC frequencies"
    moves = []
    for l in lines[1:]:
        q, x, y = l.split(",")
        moves.append(((int(q) - start) / qpf * 1000.0, int(x), int(y)))
    # The two runs: the tear-off moves straight down (dx = 0), the run moves right.
    run = [m for k, m in enumerate(moves) if k > 0 and m[1] != moves[k - 1][1]]
    tear_t0 = moves[0][0]
    first = next(p for p in pos if p)
    torn = next((p for p in pos if p and p[0] >= tear_t0 and (abs(p[1] - first[1]) > 4 or abs(p[2] - first[2]) > 4)), None)
    out = {"recording": a.path, "frames": len(times), "moves": len(moves)}
    out["tear_off_ms"] = round(torn[0] - tear_t0, 1) if torn else None
    # The grab offset at rest: the last frame against the last move.
    last = [p for p in pos if p][-1]
    off = (last[1] - moves[-1][1], last[2] - moves[-1][2])
    t_run0, t_run1 = run[0][0], run[-1][0]
    mt = np.array([m[0] for m in run])
    mx = np.array([m[1] for m in run], dtype=float)
    lags, moving = [], []
    for p in pos:
        if not p or p[0] < t_run0 or p[0] > t_run1 + 50:
            continue
        cx = p[1] - off[0]                      # where the cursor was for this window place
        if cx < mx[0] or cx > mx[-1]:
            continue
        tau = float(np.interp(cx, mx, mt))      # when the cursor was there (x only increases)
        lags.append(p[0] - tau)
        moving.append(p)
    gaps = [b[0] - a_[0] for a_, b in zip(moving, moving[1:])]
    rates = [np.hypot(b[1] - a_[1], b[2] - a_[2]) / (b[0] - a_[0]) for a_, b in zip(moving, moving[1:]) if b[0] > a_[0]]
    q = lambda v, k: round(float(np.percentile(v, k)), 1) if v else None
    out["run_ms"] = round(t_run1 - t_run0, 1)
    out["frames_in_run"] = len(moving)
    out["lag_ms"] = {"median": q(lags, 50), "p95": q(lags, 95), "max": q(lags, 100)}
    out["gaps_ms"] = {"median": q(gaps, 50), "p95": q(gaps, 95), "max": q(gaps, 100)}
    out["step_cv"] = round(st.pstdev(rates) / st.mean(rates), 3) if len(rates) > 2 else None
    out["grab_offset_px"] = [round(off[0], 1), round(off[1], 1)]
    print(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
