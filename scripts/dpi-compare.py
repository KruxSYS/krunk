#!/usr/bin/env python3
"""Compares two runs of scripts/vm-dpi-run.sh for the same display mode, screenshot by
screenshot: "is the layout at 100 % what it was?"

  scripts/dpi-compare.py BEFORE_DIR AFTER_DIR [OUT_DIR]

For every screenshot name both runs have, it counts the pixels that differ and prints the
bounding box of the difference. Left out of the comparison: the taskbar (clock, and the Krunk
button's state) and anything inside MASKS (a blinking caret, the version on krunk://about).
With OUT_DIR, differing screenshots get a side-by-side picture with the difference marked.
A tolerance of 8 per channel ignores the noise of text anti-aliasing between two starts.
"""
import glob
import os
import re
import sys

from PIL import Image, ImageChops, ImageDraw

before, after = sys.argv[1], sys.argv[2]
out = sys.argv[3] if len(sys.argv) > 3 else None
TASKBAR_DIP = 48
# name pattern -> rectangles (x0, y0, x1, y1) in fractions of the screen to ignore
MASKS = {
    r"page-about$": [(0.0, 0.0, 1.0, 1.0)],
}


def names(d):
    m = {}
    for f in glob.glob(os.path.join(d, "*.png")):
        # "<mode>-<percent>-NN-name.png": the name without the mode
        base = os.path.basename(f)[:-4]
        key = re.sub(r"^\d+x\d+-\d+(-rt)?-", "", base)
        m[key] = f
    return m


a, b = names(before), names(after)
scale = 1.0
m = re.search(r"-(\d+)(-rt)?$", os.path.basename(os.path.normpath(before)))
if m:
    scale = int(m.group(1)) / 100.0
same = diff = 0
for key in sorted(set(a) | set(b)):
    if key not in a or key not in b:
        print(f"{key}: only in {'before' if key in a else 'after'}")
        continue
    ia, ib = Image.open(a[key]).convert("RGB"), Image.open(b[key]).convert("RGB")
    if ia.size != ib.size:
        print(f"{key}: sizes differ {ia.size} {ib.size}")
        diff += 1
        continue
    w, h = ia.size
    d = ImageChops.difference(ia, ib).convert("L").point(lambda v: 255 if v > 8 else 0)
    draw = ImageDraw.Draw(d)
    draw.rectangle([0, h - int(TASKBAR_DIP * scale), w, h], fill=0)
    for pat, rects in MASKS.items():
        if re.search(pat, key):
            for x0, y0, x1, y1 in rects:
                draw.rectangle([int(x0 * w), int(y0 * h), int(x1 * w), int(y1 * h)], fill=0)
    box = d.getbbox()
    if box is None:
        same += 1
        continue
    count = sum(1 for v in d.getdata() if v)
    diff += 1
    print(f"{key}: {count} px differ, in {box[0]},{box[1]} - {box[2]},{box[3]}")
    if out:
        os.makedirs(out, exist_ok=True)
        side = Image.new("RGB", (w * 2 + 8, h), (255, 0, 255))
        side.paste(ia, (0, 0))
        mark = ib.copy()
        ImageDraw.Draw(mark).rectangle([box[0] - 2, box[1] - 2, box[2] + 2, box[3] + 2], outline=(255, 0, 0), width=2)
        side.paste(mark, (w + 8, 0))
        side.save(os.path.join(out, key + ".jpg"), quality=85)
print(f"{same} identical, {diff} different")
