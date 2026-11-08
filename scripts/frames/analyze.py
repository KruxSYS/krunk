#!/usr/bin/env python3
"""Analyse a framecap recording (tools/framecap): PATH.raw + PATH.csv + PATH.json.

  analyze.py flash  PATH [--sheet OUT.png]     tab-switch flashes
  analyze.py track PATH --color RRGGBB [--ease MS]   an object's path (with --ease: its distance
                                               from Krunk's eased flight curve, per frame)
  analyze.py motion PATH [--box X,Y,W,H]       a moving object (download flight)
  analyze.py bar    PATH --box X,Y,W,H --color RRGGBB [--tol N]   a growing progress bar
  analyze.py track  PATH --color RRGGBB [--tol N] [--box X,Y,W,H]  an object of one colour

flash: a frame is *white* when >= 98 % of its pixels are within 12 levels of one colour and that
colour's luminance is >= 245. A run of at most --flash-frames (2) white frames between two
non-white frames is a *flash* (a moment of white between two looks); a longer run is what a tab
really shows (a blank page still loading in the light theme). Uniform frames of other colours
are reported too. Exit code 1 if any flash.

motion: the object's centroid per frame (pixels that differ from the first frame), the step
between frames, and how even those steps are.

track: the centroid of the pixels of one colour (the download glyph's disc) per frame, the
steps between frames, and whether it only moves forward along its path.

bar: the bar's filled width per frame (columns where most pixels match the colour); reports
backward moves and the largest jump.
"""
import argparse
import csv
import json
import sys

import numpy as np


def load(path):
    meta = json.load(open(path + ".json"))
    w, h = meta["width"], meta["height"]
    raw = np.fromfile(path + ".raw", dtype=np.uint8)
    n = raw.size // (w * h * 3)
    frames = raw[: n * w * h * 3].reshape(n, h, w, 3)
    with open(path + ".csv") as f:
        times = [float(r["ms"]) for r in csv.DictReader(f)][:n]
    return meta, frames, np.array(times)


def luminance(rgb):
    r, g, b = (np.asarray(rgb, dtype=float) / 255.0).T
    return 255.0 * (0.2126 * r + 0.7152 * g + 0.0722 * b)


def uniformity(frame, tol=12):
    flat = frame.reshape(-1, 3).astype(int)
    median = np.median(flat, axis=0)
    close = np.all(np.abs(flat - median) <= tol, axis=1)
    return close.mean(), median


def cmd_flash(a):
    meta, frames, times = load(a.path)
    flashes, uniform = [], []
    for i, fr in enumerate(frames):
        frac, med = uniformity(fr)
        if frac >= 0.98:
            lum = float(luminance(med))
            entry = {"frame": i, "ms": round(float(times[i]), 1), "color": "#%02x%02x%02x" % tuple(int(c) for c in med), "luminance": round(lum, 1)}
            uniform.append(entry)
            if lum >= 245:
                flashes.append(entry)
    # Runs of white frames: short ones between other frames are flashes.
    white = [f["frame"] for f in flashes]
    runs, start = [], None
    for i in range(len(frames)):
        w = i in set(white)
        if w and start is None:
            start = i
        if (not w or i == len(frames) - 1) and start is not None:
            end = i if w else i - 1
            runs.append((start, end))
            start = None
    transient = [r for r in runs if r[1] - r[0] + 1 <= a.flash_frames and r[0] > 0 and r[1] < len(frames) - 1]
    gaps = np.diff(times) if len(times) > 1 else np.array([0.0])
    out = {
        "recording": a.path,
        "method": meta["method"],
        "frames": int(len(frames)),
        "seconds": round(float(times[-1] - times[0]) / 1000, 2) if len(times) > 1 else 0,
        "median_frame_gap_ms": round(float(np.median(gaps)), 2),
        "flash_frames": sum(r[1] - r[0] + 1 for r in transient),
        "flashes_at": [{"frames": [r[0], r[1]], "ms": round(float(times[r[0]]), 1)} for r in transient][:50],
        "white_frames": len(flashes),
        "white_runs": [r[1] - r[0] + 1 for r in runs][:100],
        "uniform_frames": len(uniform),
        "white": flashes[:20],
        "uniform": uniform[:50],
    }
    print(json.dumps(out, indent=1))
    if a.sheet and (flashes or uniform):
        from PIL import Image
        picks = [e["frame"] for e in (flashes or uniform)][:12]
        tiles = []
        for i in picks:
            for j in (i - 1, i, i + 1):
                if 0 <= j < len(frames):
                    tiles.append(Image.fromarray(frames[j]))
        tw, th = tiles[0].size
        cols = 3
        sheet = Image.new("RGB", (tw * cols, th * ((len(tiles) + cols - 1) // cols)), "black")
        for k, t in enumerate(tiles):
            sheet.paste(t, ((k % cols) * tw, (k // cols) * th))
        sheet.save(a.sheet)
    return 1 if transient else 0


def box_of(a, frames):
    if a.box:
        x, y, w, h = (int(v) for v in a.box.split(","))
        return frames[:, y : y + h, x : x + w]
    return frames


def cmd_motion(a):
    meta, frames, times = load(a.path)
    sub = box_of(a, frames).astype(int)
    base = sub[0]
    rows = []
    for i in range(1, len(sub)):
        diff = np.any(np.abs(sub[i] - base) > 40, axis=2)
        ys, xs = np.nonzero(diff)
        if len(xs) < 4:
            continue
        rows.append((i, float(times[i]), float(xs.mean()), float(ys.mean()), int(len(xs))))
    steps = []
    for (i0, t0, x0, y0, _), (i1, t1, x1, y1, _) in zip(rows, rows[1:]):
        if i1 == i0 + 1:
            steps.append((np.hypot(x1 - x0, y1 - y0), t1 - t0))
    d = np.array([s[0] for s in steps]) if steps else np.array([0.0])
    dt = np.array([s[1] for s in steps]) if steps else np.array([0.0])
    out = {
        "recording": a.path,
        "frames_with_object": len(rows),
        "frame_gap_ms_median": round(float(np.median(dt)), 2),
        "frame_gap_ms_max": round(float(dt.max()), 2),
        "step_px_median": round(float(np.median(d)), 2),
        "step_px_max": round(float(d.max()), 2),
        "step_px_cv": round(float(d.std() / d.mean()), 3) if d.mean() > 0 else 0,
        "path": [{"frame": r[0], "ms": round(r[1], 1), "x": round(r[2], 1), "y": round(r[3], 1)} for r in rows],
    }
    print(json.dumps(out, indent=1))
    return 0


def cmd_bar(a):
    meta, frames, times = load(a.path)
    sub = box_of(a, frames).astype(int)
    color = np.array([int(a.color[k : k + 2], 16) for k in (0, 2, 4)])
    widths = []
    for fr in sub:
        match = np.all(np.abs(fr - color) <= a.tol, axis=2)
        cols = match.mean(axis=0) >= 0.5
        widths.append(int(cols.sum()))
    w = np.array(widths)
    dw = np.diff(w)
    out = {
        "recording": a.path,
        "frames": len(w),
        "width_first": int(w[0]),
        "width_last": int(w[-1]),
        "backward_moves": int((dw < 0).sum()),
        "largest_jump_px": int(dw.max()) if len(dw) else 0,
        "changing_frames": int((dw != 0).sum()),
        "widths": [{"ms": round(float(t), 1), "w": int(x)} for t, x in zip(times, w)],
    }
    print(json.dumps(out, indent=1))
    return 0


def ease_curve(t):
    """Krunk's download flight easing (app/src/browser/flight.rs `ease`): cubic in-out."""
    t = np.clip(t, 0.0, 1.0)
    return np.where(t < 0.5, 4 * t**3, 1 - (-2 * t + 2) ** 3 / 2)


def ease_fit(rows, duration, step):
    """How far each captured frame is from where the eased flight should be at that frame's time.

    The horizontal position is x0 + dx * ease((t - start) / duration); start, x0 and dx are fitted
    (least squares, start on a 0.5 ms grid before the first frame seen). The residuals, in screen
    pixels, show uneven pacing independently of the frame rate: a frame drawn for the wrong time
    sits off the curve.
    """
    ts = np.array([r[1] for r in rows])
    xs = np.array([r[2] for r in rows]) * step
    best = None
    for start in np.arange(ts[0] - duration, ts[0] + 0.01, 0.5):
        e = ease_curve((ts - start) / duration)
        m = np.vstack([np.ones_like(e), e]).T
        coef, *_ = np.linalg.lstsq(m, xs, rcond=None)
        r = xs - m @ coef
        rms = float(np.sqrt(np.mean(r**2)))
        if best is None or rms < best[0]:
            best = (rms, start, coef, r)
    rms, start, coef, r = best
    return {
        "duration_ms": duration,
        "start_ms": round(float(start), 1),
        "dx_px": round(float(coef[1]), 1),
        "residual_px": [round(float(v), 1) for v in r],
        "max_residual_px": round(float(np.max(np.abs(r))), 1),
        "rms_px": round(rms, 2),
    }


def cmd_track(a):
    meta, frames, times = load(a.path)
    sub = box_of(a, frames).astype(int)
    color = np.array([int(a.color[k : k + 2], 16) for k in (0, 2, 4)])
    rows = []
    for i, fr in enumerate(sub):
        match = np.all(np.abs(fr - color) <= a.tol, axis=2)
        ys, xs = np.nonzero(match)
        if len(xs) >= 6:
            rows.append((i, float(times[i]), float(xs.mean()), float(ys.mean()), int(len(xs))))
    steps, gaps = [], []
    for (i0, t0, x0, y0, _), (i1, t1, x1, y1, _) in zip(rows, rows[1:]):
        steps.append(float(np.hypot(x1 - x0, y1 - y0)))
        gaps.append(t1 - t0)
    path = [(r[2], r[3]) for r in rows]
    # Forward only: each position is no closer to the start than the one before.
    back = 0
    if len(path) > 1:
        d = [np.hypot(x - path[0][0], y - path[0][1]) for x, y in path]
        back = sum(1 for u, v in zip(d, d[1:]) if v + 1.0 < u)
    out = {
        "recording": a.path,
        "frames_with_object": len(rows),
        "duration_ms": round(rows[-1][1] - rows[0][1], 1) if rows else 0,
        "frame_gap_ms": [round(g, 1) for g in gaps],
        "step_px": [round(x, 1) for x in steps],
        "backward_steps": back,
        "path": [{"frame": r[0], "ms": round(r[1], 1), "x": round(r[2], 1), "y": round(r[3], 1), "px": r[4]} for r in rows],
    }
    if a.ease and len(rows) >= 4:
        out["ease_fit"] = ease_fit(rows, a.ease, meta.get("step", 1))
    print(json.dumps(out, indent=1))
    return 0


def main():
    p = argparse.ArgumentParser()
    p.add_argument("mode", choices=["flash", "motion", "bar", "track"])
    p.add_argument("path")
    p.add_argument("--sheet")
    p.add_argument("--box")
    p.add_argument("--color")
    p.add_argument("--tol", type=int, default=24)
    p.add_argument("--flash-frames", type=int, default=2)
    p.add_argument("--ease", type=float, help="track: fit Krunk's flight easing over this many ms")
    a = p.parse_args()
    return {"flash": cmd_flash, "motion": cmd_motion, "bar": cmd_bar, "track": cmd_track}[a.mode](a)


if __name__ == "__main__":
    sys.exit(main())
