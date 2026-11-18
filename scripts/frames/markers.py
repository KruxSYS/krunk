#!/usr/bin/env python3
"""Tab markers (Krunk 0.25.0 harness): the colour of the band a test page paints at its top
(scripts/vm-www/8094/marker.js, `?m=<index>`), so a screen recording shows which tab is on screen.

  markers.py            prints the palette as marker.js's array (to regenerate it)
  markers.py K          prints marker K's colour as RRGGBB (for draglag.py --color)

The palette: every colour of a 5-level grid (30, 85, 140, 195, 250 per channel) that is not a
grey, not near black or white (luminance 24..235), and at least 48 levels (in some channel)
from black, white, Krunk's dark and light theme backgrounds (#1f2023, #f3f4f6) and busy.html's
page colour (#f8fafc). That leaves 116 colours, each 55 levels from any other in at least one
channel; marker i is colour (i * 37) % 116, so neighbouring indices look different.
analyze.py selftest checks that marker.js holds the same list.
"""
import sys

LEVELS = (30, 85, 140, 195, 250)
AVOID = ((0, 0, 0), (255, 255, 255), (0x1F, 0x20, 0x23), (0xF3, 0xF4, 0xF6), (0xF8, 0xFA, 0xFC))
STRIDE = 37


def lum(c):
    return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]


def _palette():
    cands = [
        (r, g, b)
        for r in LEVELS
        for g in LEVELS
        for b in LEVELS
        if max(r, g, b) - min(r, g, b) >= 55
        and 24 <= lum((r, g, b)) <= 235
        and all(max(abs(x - y) for x, y in zip((r, g, b), a)) >= 48 for a in AVOID)
    ]
    return [cands[(i * STRIDE) % len(cands)] for i in range(len(cands))]


PALETTE = _palette()


def color(i):
    """Marker i's (r, g, b)."""
    return PALETTE[i % len(PALETTE)]


def hexcolor(i):
    return "%02x%02x%02x" % color(i)


if __name__ == "__main__":
    if len(sys.argv) > 1:
        print(hexcolor(int(sys.argv[1])))
    else:
        items = ["'#%s'" % hexcolor(i) for i in range(len(PALETTE))]
        print("[\n" + ",\n".join("  " + ", ".join(items[k : k + 8]) for k in range(0, len(items), 8)) + "\n]")
