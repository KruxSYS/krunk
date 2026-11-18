#!/usr/bin/env python3
"""Analyse a framecap recording (tools/framecap): PATH.raw + PATH.csv + PATH.json.

  analyze.py flash  PATH [--sheet OUT.png] [--black-lum L]   tab-switch flashes (white and black)
  analyze.py selftest                          checks the flash rules on made-up frames
  analyze.py track PATH --color RRGGBB [--ease MS]   an object's path (with --ease: its distance
                                               from Krunk's eased flight curve, per frame)
  analyze.py motion PATH [--box X,Y,W,H]       a moving object (download flight)
  analyze.py bar    PATH --box X,Y,W,H --color RRGGBB [--tol N]   a growing progress bar
  analyze.py track  PATH --color RRGGBB [--tol N] [--box X,Y,W,H]  an object of one colour
  analyze.py white  PATH [--ymin Y] [--lum L] [--min N] [--sheet OUT.png]   white areas (resize)
  analyze.py addr   PATH                       the address bar's text and selection per frame
  analyze.py switch PATH EVENTS.csv [--band Y0,Y1] [--tol N] [--out JSON]   input -> the target
                                               tab's marker on screen (0.25.0 harness)

flash: a frame is *white* when >= 98 % of its pixels are within 12 levels of one colour and that
colour's luminance is >= 245. A run of at most --flash-frames (2) white frames between two
non-white frames is a *flash* (a moment of white between two looks); a longer run is what a tab
really shows (a blank page still loading in the light theme). Uniform frames of other colours
are reported too. Exit code 1 if any flash.

flash, black (0.15.0): a uniform frame whose colour has luminance <= --black-lum (16) is *black*.
Krunk never shows black on purpose (the dark theme's page colour #1f2023 is about 32), so every
run of black frames between two non-black frames is a flash, however long; black at the very start
or end (the window not there yet, or gone) is not. Capture the page area only (below the toolbar):
the toolbar is never black and would make the frame non-uniform.

motion: the object's centroid per frame (pixels that differ from the first frame), the step
between frames, and how even those steps are.

track: the centroid of the pixels of one colour (the download glyph's disc) per frame, the
steps between frames, and whether it only moves forward along its path.

white (0.7.2, R6): white areas that appear while a window is resized. A pixel is white when
its luminance is >= --lum (default 235) and its channels differ by at most 12; rows above --ymin
(screen px of the recording) are ignored (a light theme's own white toolbar), and rows from
--ymax on (the taskbar). A frame with at
least --min (default 96) white samples (at the recording's step) is a white frame; the mouse
cursor stays below that. Exit code 1 if any.

addr (0.7.2, R1, R3): a recording of the address bar's text area (light theme). Per frame: the
dark text samples (luminance < 100: typed or page text; the placeholder is lighter) and the
selection samples (the accent blue behind a selected completion). Reported: frames with text,
and for a typing run, frames where the selection vanished after it first appeared.

switch (0.25.0): the test pages paint a band in their tab's marker colour at the very top
(vm-www/8094/marker.js, scripts/frames/markers.py). EVENTS.csv (the VM driver's) starts with
`qpf,<freq>`, then one `qpc,kind,target` row per key or click (QPC of the key/button down; target
= the marker index that should show, -1 = anything other than what was on screen). For each
event: the time from it to the first frame (framecap's present time, on the same QPC time line
via PATH.json's startQpc) whose band shows the target; for -1, whose band differs from the frame
on screen at the event. An event whose target is already on screen is "already" (not timed); one
whose target does not show before the next event (or the recording's end) is "missed". The band
rows are found by themselves (rows that most often show a marker colour) or given with --band
as recording rows (after --step) Y0,Y1 (Y1 exclusive). Printed: per kind count, median, p95 and
max, every event >= 250 ms and every missed one; --out (default PATH.switch.json) gets it all,
per event with what the band showed in between ("none" = no marker: a blank or loading page).

bar: the bar's filled width per frame (columns where most pixels match the colour); reports
backward moves and the largest jump.
"""
import argparse
import csv
import json
import os
import re
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import markers  # noqa: E402


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


def runs_of(marked, n):
    """Runs (first, last) of consecutive frame indices in `marked` among `n` frames."""
    marked = set(marked)
    runs, start = [], None
    for i in range(n):
        w = i in marked
        if w and start is None:
            start = i
        if (not w or i == n - 1) and start is not None:
            runs.append((start, i if w else i - 1))
            start = None
    return runs


def flash_report(frames, times, flash_frames=2, black_lum=16.0):
    """The flash rules (see the module doc): white flashes, black flashes and uniform frames."""
    flashes, blacks, uniform = [], [], []
    for i, fr in enumerate(frames):
        frac, med = uniformity(fr)
        if frac >= 0.98:
            lum = float(luminance(med))
            entry = {"frame": i, "ms": round(float(times[i]), 1), "color": "#%02x%02x%02x" % tuple(int(c) for c in med), "luminance": round(lum, 1)}
            uniform.append(entry)
            if lum >= 245:
                flashes.append(entry)
            elif lum <= black_lum:
                blacks.append(entry)
    n = len(frames)
    # Runs of white frames: short ones between other frames are flashes.
    runs = runs_of([f["frame"] for f in flashes], n)
    transient = [r for r in runs if r[1] - r[0] + 1 <= flash_frames and r[0] > 0 and r[1] < n - 1]
    # Black between two other frames is always a flash (0.15.0).
    black_runs = runs_of([f["frame"] for f in blacks], n)
    black_flash = [r for r in black_runs if r[0] > 0 and r[1] < n - 1]
    return {
        "flash_frames": sum(r[1] - r[0] + 1 for r in transient),
        "flashes_at": [{"frames": [r[0], r[1]], "ms": round(float(times[r[0]]), 1)} for r in transient][:50],
        "white_frames": len(flashes),
        "white_runs": [r[1] - r[0] + 1 for r in runs][:100],
        "black_flash_frames": sum(r[1] - r[0] + 1 for r in black_flash),
        "black_flashes_at": [{"frames": [r[0], r[1]], "ms": round(float(times[r[0]]), 1)} for r in black_flash][:50],
        "black_frames": len(blacks),
        "uniform_frames": len(uniform),
        "white": flashes[:20],
        "black": blacks[:20],
        "uniform": uniform[:50],
        "_bad": bool(transient or black_flash),
        "_picks": [e["frame"] for e in (flashes + blacks or uniform)][:12],
    }


def cmd_flash(a):
    meta, frames, times = load(a.path)
    rep = flash_report(frames, times, a.flash_frames, a.black_lum)
    bad, picks = rep.pop("_bad"), rep.pop("_picks")
    gaps = np.diff(times) if len(times) > 1 else np.array([0.0])
    out = {
        "recording": a.path,
        "method": meta["method"],
        "frames": int(len(frames)),
        "seconds": round(float(times[-1] - times[0]) / 1000, 2) if len(times) > 1 else 0,
        "median_frame_gap_ms": round(float(np.median(gaps)), 2),
        **rep,
    }
    print(json.dumps(out, indent=1))
    if a.sheet and picks:
        from PIL import Image
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
    return 1 if bad else 0


def cmd_selftest(a):
    """The flash rules on made-up 8×8 frames (no recording needed). Exit code 1 on a failure."""
    def frames_of(colors):
        return np.array([np.full((8, 8, 3), c, dtype=np.uint8) for c in colors])

    page, dark_bg, white, black = (90, 120, 200), (0x1F, 0x20, 0x23), (255, 255, 255), (0, 0, 0)
    times = lambda n: np.arange(n, dtype=float) * 16.7  # noqa: E731
    cases = [
        ("black frame between pages", [page, black, page], True, "black_flash_frames", 1),
        ("long black run between pages", [page, black, black, black, black, page], True, "black_flash_frames", 4),
        ("dark theme background is not black", [page, dark_bg, dark_bg, page], False, "black_frames", 0),
        ("black before the window shows", [black, black, page, page], False, "black_flash_frames", 0),
        ("white flash", [page, white, page], True, "flash_frames", 1),
        ("long white (a blank light page) is no flash", [page, white, white, white, page], False, "flash_frames", 0),
        ("near-black (luminance 12) counts", [page, (12, 12, 12), page], True, "black_flash_frames", 1),
    ]
    failed = 0
    for name, colors, bad, key, want in cases:
        r = flash_report(frames_of(colors), times(len(colors)))
        ok = r["_bad"] == bad and r[key] == want
        failed += not ok
        print(("ok   " if ok else "FAIL ") + name + ("" if ok else " (%s=%s, bad=%s)" % (key, r[key], r["_bad"])))

    # switch (0.25.0): frames of 8 rows x 16 columns, the band in rows 1-2, page content below.
    def mframe(m, page=(240, 240, 240), partial=False):
        f = np.full((8, 16, 3), page, dtype=np.uint8)
        f[0] = (0x1F, 0x20, 0x23)               # the toolbar's edge above the page
        if m is not None:
            f[1:3] = markers.color(m)
            if partial:
                f[1:3, :4] = (250, 250, 250)    # a hover card over a quarter of the band
        f[3:] = (np.arange(16)[None, :, None] * 13 + np.arange(5)[:, None, None] * 40) % 256
        return f

    seq = [3] * 10 + [None] * 3 + [5] * 6 + [5] * 4 + [7] * 6 + [None] * 6 + [9] * 3 + [70] * 6
    frs = [mframe(m, partial=(i == 15)) for i, m in enumerate(seq)]
    tms = np.arange(len(frs), dtype=float) * 20.0
    evs = [(105.0, "switch", 5, 1), (370.0, "switch", 5, 2), (395.0, "click", 11, 3),
           (470.0, "open", -1, 4), (590.0, "rapid", 9, 5), (690.0, "link", 70, 6)]
    rep = switch_report(np.array(frs), tms, evs)
    ev = rep["events"]
    checks = [
        ("switch: band rows found", rep["band_rows"] == [1, 3], rep["band_rows"]),
        ("switch: latency to the target marker (blank frames between)", ev[0].get("latency_ms") == 155.0 and ev[0]["between"] == ["none"], ev[0]),
        ("switch: hover card over part of the band still matches", rep["events"][0]["frame"] == 13, ev[0].get("frame")),
        ("switch: target already on screen", ev[1]["status"] == "already", ev[1]["status"]),
        ("switch: wrong tab shown -> missed, with what showed", ev[2]["status"] == "missed" and 7 in ev[2]["between"], ev[2]),
        ("switch: -1 = anything different (a blank page)", ev[3].get("latency_ms") == 110.0, ev[3]),
        ("switch: index past the palette wraps", ev[5]["status"] == "ok", ev[5]),
        ("switch: target not shown before the next event -> missed", ev[4]["status"] == "missed", ev[4]),
        ("switch: per-kind stats", rep["kinds"]["switch"]["timed"] == 1 and rep["kinds"]["click"]["missed"] == 1, rep["kinds"]),
    ]
    for name, ok, detail in checks + check_palette():
        failed += not ok
        print(("ok   " if ok else "FAIL ") + name + ("" if ok else " (%s)" % (detail,)))
    return 1 if failed else 0


# ---------- switch (0.25.0): input -> the target tab's marker on screen ----------

PAL = np.array(markers.PALETTE, dtype=int)


def marker_of(region, tol):
    """The marker index a band region shows (>= 60 % of its pixels within tol of one palette
    colour), else None."""
    flat = region.reshape(-1, 3).astype(int)
    med = np.median(flat, axis=0)
    d = np.abs(PAL - med).max(axis=1)
    k = int(np.argmin(d))
    if d[k] > tol:
        return None
    frac = (np.abs(flat - PAL[k]).max(axis=1) <= tol).mean()
    return k if frac >= 0.6 else None


def find_band(frames, tol):
    """Rows (y0, y1) that most often show a marker colour across the recording."""
    n, h = len(frames), frames.shape[1]
    picks = np.linspace(0, n - 1, min(n, 120)).astype(int) if n else []
    hits = np.zeros(h)
    for i in picks:
        for y in range(h):
            if marker_of(frames[i, y : y + 1], tol) is not None:
                hits[y] += 1
    if not hits.any():
        return None
    rows = np.nonzero(hits >= 0.5 * hits.max())[0]
    # The longest run of consecutive rows among them.
    best, start = (rows[0], rows[0]), rows[0]
    for a, b in zip(rows, rows[1:]):
        if b != a + 1:
            start = b
        if b - start > best[1] - best[0]:
            best = (start, b)
    return int(best[0]), int(best[1]) + 1


def load_events(path):
    """EVENTS.csv: `qpf,<freq>` (or `qpf=<freq>`), then `qpc,kind,target` rows (a header row is
    skipped)."""
    lines = [l.strip() for l in open(path, encoding="utf-8-sig") if l.strip()]
    qpf = float(re.split(r"[,=]", lines[0])[1])
    events = []
    for l in lines[1:]:
        p = [x.strip() for x in l.split(",")]
        if len(p) < 3 or not p[0].lstrip("-").isdigit():
            continue
        events.append((int(p[0]), p[1], int(p[2])))
    events.sort(key=lambda e: e[0])
    return qpf, events


def pct(v, k):
    return round(float(np.percentile(v, k)), 1) if len(v) else None


def switch_report(frames, times, events, band=None, tol=24, slow_ms=250.0):
    """events: (ms on the recording's time line, kind, target, qpc). See the module doc."""
    if band is None:
        band = find_band(frames, tol)
    if band is None:
        return {"error": "no marker band found in the recording", "events": len(events)}
    y0, y1 = band
    K = len(PAL)
    marks = [marker_of(fr[y0:y1], tol) for fr in frames]
    sigs = [fr[y0:y1].reshape(-1, 3).astype(float).mean(axis=0) for fr in frames]
    times = np.asarray(times, dtype=float)
    rows = []
    for n_, (te, kind, target, qpc) in enumerate(events):
        t_next = events[n_ + 1][0] if n_ + 1 < len(events) else float("inf")
        row = {"n": n_, "qpc": qpc, "ms": round(te, 1), "kind": kind, "target": target}
        prior = int(np.searchsorted(times, te, side="right")) - 1
        if prior < 0 or te > times[-1]:
            row["status"] = "outside"
            rows.append(row)
            continue
        before = marks[prior]
        row["before"] = before
        want = None if target < 0 else target % K
        if want is not None and before == want:
            row["status"] = "already"
            rows.append(row)
            continue
        between, hit = [], None
        for j in range(prior + 1, len(frames)):
            if times[j] >= t_next:
                break
            mk = marks[j]
            if want is not None:
                ok = mk == want
            else:
                ok = mk != before or (mk is None and float(np.abs(sigs[j] - sigs[prior]).max()) > 20)
            if ok:
                hit = j
                break
            state = "none" if mk is None else mk
            if (mk != before or mk is None) and (not between or between[-1] != state):
                between.append(state)
        row["between"] = between[:12]
        if hit is None:
            row["status"] = "missed"
        else:
            row["status"] = "ok"
            row["latency_ms"] = round(float(times[hit] - te), 1)
            row["frame"] = hit
            row["seen"] = marks[hit]
        rows.append(row)
    kinds = {}
    for r in rows:
        k = kinds.setdefault(r["kind"], {"count": 0, "timed": 0, "missed": 0, "already": 0, "outside": 0, "_l": []})
        k["count"] += 1
        if r["status"] == "ok":
            k["timed"] += 1
            k["_l"].append(r["latency_ms"])
        elif r["status"] in ("missed", "already", "outside"):
            k[r["status"]] += 1
    for k in kinds.values():
        lat = k.pop("_l")
        k.update({"median_ms": pct(lat, 50), "p95_ms": pct(lat, 95), "max_ms": round(max(lat), 1) if lat else None})
    gaps = np.diff(times) if len(times) > 1 else np.array([0.0])
    return {
        "frames": int(len(frames)),
        "band_rows": [y0, y1],
        "median_frame_gap_ms": round(float(np.median(gaps)), 2),
        "kinds": kinds,
        "slow": [r for r in rows if r.get("latency_ms", 0) >= slow_ms],
        "missed": [r for r in rows if r["status"] == "missed"],
        "events": rows,
    }


def print_switch(rep):
    if "error" in rep:
        print(rep["error"])
        return
    print("band rows %s, %d frames, median frame gap %.1f ms" % (rep["band_rows"], rep["frames"], rep["median_frame_gap_ms"]))
    print("%-12s %5s %5s %6s %7s %8s %8s %8s" % ("kind", "count", "timed", "missed", "already", "median", "p95", "max"))
    for kind, k in sorted(rep["kinds"].items()):
        f = lambda v: "-" if v is None else "%.1f" % v  # noqa: E731
        print("%-12s %5d %5d %6d %7d %8s %8s %8s" % (kind, k["count"], k["timed"], k["missed"], k["already"], f(k["median_ms"]), f(k["p95_ms"]), f(k["max_ms"])))
    for title, key in (("events >= 250 ms", "slow"), ("missed events", "missed")):
        if rep[key]:
            print(title + ":")
            for r in rep[key]:
                print("  #%d %s target=%s at %.0f ms: %s (before=%s, between=%s)" % (
                    r["n"], r["kind"], r["target"], r["ms"], r.get("latency_ms", "missed"), r.get("before"), r.get("between")))


def cmd_switch(a):
    meta, frames, times = load(a.path)
    qpf, evs = load_events(a.events)
    start, fq = meta.get("startQpc", 0), float(meta.get("qpf", qpf))
    if abs(fq - qpf) > 1:
        print("warning: different QPC frequencies (recording %s, events %s)" % (fq, qpf), file=sys.stderr)
    events = [((q - start) / qpf * 1000.0, kind, target, q) for q, kind, target in evs]
    band = tuple(int(v) for v in a.band.split(",")) if a.band else None
    rep = switch_report(frames, times, events, band, a.tol)
    rep = {"recording": a.path, "events_file": a.events, **rep}
    print_switch(rep)
    with open(a.out or a.path + ".switch.json", "w") as f:
        json.dump(rep, f, indent=1)
    return 0


def check_palette():
    """marker.js and markers.py hold the same palette, and it has the properties the analyser
    relies on. Returns a list of (name, ok, detail)."""
    js = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "vm-www", "8094", "marker.js")
    found = [tuple(int(h[k : k + 2], 16) for k in (0, 2, 4)) for h in re.findall(r"'#([0-9a-fA-F]{6})'", open(js).read())]
    pal = [tuple(c) for c in markers.PALETTE]
    out = [("marker.js palette == markers.py palette", found == pal, "%d vs %d colours" % (len(found), len(pal))),
           ("at least 64 colours", len(pal) >= 64, str(len(pal)))]
    close = [(i, j) for i in range(len(pal)) for j in range(i + 1, len(pal)) if max(abs(x - y) for x, y in zip(pal[i], pal[j])) < 40]
    out.append(("every pair >= 40 levels apart in some channel", not close, str(close[:3])))
    lums = [markers.lum(c) for c in pal]
    out.append(("no near-black or near-white", min(lums) > 16 and max(lums) < 245, "%.0f..%.0f" % (min(lums), max(lums))))
    bgs = [(0x1F, 0x20, 0x23), (0xF3, 0xF4, 0xF6)]
    near = [c for c in pal for b in bgs if max(abs(x - y) for x, y in zip(c, b)) < 40]
    out.append(("far from the theme backgrounds", not near, str(near[:3])))
    return out


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


def cmd_white(a):
    meta, frames, times = load(a.path)
    step = meta.get("step", 1)
    y0 = max(0, int((a.ymin - meta["rect"][1]) / step)) if a.ymin is not None else 0
    y1 = int((a.ymax - meta["rect"][1]) / step) if a.ymax is not None else frames.shape[1]
    f = frames[:, y0:y1, :, :].astype(int)
    lum = 0.2126 * f[..., 0] + 0.7152 * f[..., 1] + 0.0722 * f[..., 2]
    spread = f.max(axis=-1) - f.min(axis=-1)
    mask = (lum >= a.lum) & (spread <= 12)
    counts = mask.reshape(len(frames), -1).sum(axis=1)
    bad = []
    for i, n in enumerate(counts):
        if n >= a.min:
            ys, xs = np.nonzero(mask[i])
            bad.append({"frame": i, "ms": round(float(times[i]), 1), "white_samples": int(n),
                        "box_px": [int(xs.min() * step + meta["rect"][0]), int((ys.min() + y0) * step + meta["rect"][1]),
                                   int((xs.max() - xs.min() + 1) * step), int((ys.max() - ys.min() + 1) * step)]})
    gaps = np.diff(times) if len(times) > 1 else np.array([0.0])
    out = {"frames": int(len(frames)), "white_frames": len(bad), "max_white_samples": int(counts.max()) if len(counts) else 0,
           "lum": a.lum, "min": a.min, "ymin": a.ymin, "frame_gap_ms": {"median": round(float(np.median(gaps)), 1), "max": round(float(gaps.max()), 1)},
           "first": bad[:12]}
    if a.sheet and bad:
        from PIL import Image
        tiles = [Image.fromarray(frames[b["frame"]].astype(np.uint8)) for b in bad[:12]]
        w, h = tiles[0].size
        sheet = Image.new("RGB", (w * min(4, len(tiles)), h * ((len(tiles) + 3) // 4)))
        for k, t in enumerate(tiles):
            sheet.paste(t, ((k % 4) * w, (k // 4) * h))
        sheet.save(a.sheet)
    print(json.dumps(out, indent=1))
    return 1 if bad else 0


def cmd_addr(a):
    meta, frames, times = load(a.path)
    f = frames.astype(int)
    lum = 0.2126 * f[..., 0] + 0.7152 * f[..., 1] + 0.0722 * f[..., 2]
    text = (lum < 100).reshape(len(frames), -1).sum(axis=1)
    sel = ((f[..., 2] > 170) & (f[..., 0] < 140) & (f[..., 2] - f[..., 0] > 60)).reshape(len(frames), -1).sum(axis=1)
    rows = [{"frame": i, "ms": round(float(times[i]), 1), "text": int(text[i]), "selection": int(sel[i])} for i in range(len(frames))]
    first_sel = next((r["frame"] for r in rows if r["selection"] >= a.min), None)
    lost = [r for r in rows if first_sel is not None and r["frame"] > first_sel and r["selection"] < a.min]
    out = {"frames": len(rows), "frames_with_text": sum(1 for r in rows if r["text"] >= a.min),
           "first_selection_frame": first_sel, "selection_lost_frames": len(lost), "lost": lost[:10], "series": rows}
    print(json.dumps(out, indent=1))
    return 0


def main():
    p = argparse.ArgumentParser()
    p.add_argument("mode", choices=["flash", "motion", "bar", "track", "white", "addr", "switch", "selftest"])
    p.add_argument("path", nargs="?")
    p.add_argument("events", nargs="?", help="switch: the driver's EVENTS.csv")
    p.add_argument("--band", help="switch: band rows Y0,Y1 of the recording (after --step)")
    p.add_argument("--out", help="switch: the JSON report (default PATH.switch.json)")
    p.add_argument("--sheet")
    p.add_argument("--box")
    p.add_argument("--color")
    p.add_argument("--tol", type=int, default=24)
    p.add_argument("--flash-frames", type=int, default=2)
    p.add_argument("--black-lum", type=float, default=16.0, help="flash: uniform frames this dark are black")
    p.add_argument("--ease", type=float, help="track: fit Krunk's flight easing over this many ms")
    p.add_argument("--ymin", type=float, help="white: ignore rows above this screen y")
    p.add_argument("--ymax", type=float, help="white: ignore rows from this screen y on (the taskbar)")
    p.add_argument("--lum", type=float, default=235.0)
    p.add_argument("--min", type=int, default=96)
    a = p.parse_args()
    if a.mode != "selftest" and not a.path:
        p.error("PATH is required")
    if a.mode == "switch" and not a.events:
        p.error("switch needs PATH and EVENTS.csv")
    return {"selftest": cmd_selftest, "flash": cmd_flash, "motion": cmd_motion, "bar": cmd_bar, "track": cmd_track, "white": cmd_white, "addr": cmd_addr, "switch": cmd_switch}[a.mode](a)


if __name__ == "__main__":
    sys.exit(main())
