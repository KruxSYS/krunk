#!/usr/bin/env python3
"""Per-frame mean luminance of a framecap recording, and the number of change bursts (frames
more than 400 ms apart start a new burst: one per tab switch that repaints the content area).

  timeline.py PATH        (PATH.raw + PATH.csv + PATH.json, as analyze.py)
"""
import json
import os
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import analyze  # noqa: E402

meta, frames, ts = analyze.load(sys.argv[1])
lum = [round(float(analyze.luminance(f.reshape(-1, 3).mean(0))), 1) for f in frames]
print(json.dumps({
    "frames": len(frames),
    "change_bursts": 1 + int(np.sum(np.diff(ts) > 400)),
    "timeline_ms_luminance": [[round(float(t)), v] for t, v in zip(ts, lum)],
}, indent=1))
