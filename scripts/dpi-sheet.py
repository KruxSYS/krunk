#!/usr/bin/env python3
"""Contact sheet of a DPI run (scripts/vm-dpi-run.sh): every screenshot of DIR scaled to the
same width, with its name, as DIR/sheet-N.jpg (12 per sheet). For a first look only; defects
are judged on the full-size PNGs."""
import glob
import os
import sys

from PIL import Image, ImageDraw

d = sys.argv[1]
files = sorted(glob.glob(os.path.join(d, "*.png")))
cols, tw = 3, 640
for k in range(0, len(files), 12):
    part = files[k:k + 12]
    thumbs = []
    for f in part:
        im = Image.open(f).convert("RGB")
        im = im.resize((tw, max(1, im.height * tw // im.width)), Image.LANCZOS)
        cell = Image.new("RGB", (tw, im.height + 16), (0, 0, 0))
        cell.paste(im, (0, 0))
        ImageDraw.Draw(cell).text((4, im.height + 2), os.path.basename(f)[:-4], fill=(255, 255, 0))
        thumbs.append(cell)
    th = max(t.height for t in thumbs)
    rows = (len(thumbs) + cols - 1) // cols
    sheet = Image.new("RGB", (cols * tw, rows * th), (40, 40, 40))
    for i, t in enumerate(thumbs):
        sheet.paste(t, ((i % cols) * tw, (i // cols) * th))
    sheet.save(os.path.join(d, "sheet-%d.jpg" % (k // 12 + 1)), quality=80)
