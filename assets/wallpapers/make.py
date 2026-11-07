#!/usr/bin/env python3
"""Builds Krunk's built-in new-tab wallpapers (0.7.0, R2) into ui/shared/wallpapers/.

  python3 assets/wallpapers/make.py

- The photo-like and abstract images were generated for Krunk with the z-image-turbo model
  (prompts and seeds in SOURCES.txt); their PNG originals live in assets/wallpapers/originals/
  (not in git: regenerate them from SOURCES.txt).
- The two gradients are drawn here (numpy), so they need no original.
Every wallpaper becomes a WebP at most 2560 px wide plus a 320 px thumbnail; the whole set must
stay under 4 MB (checked here and by core's asset test).
"""
import json
import os
import sys

import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "..", "ui", "shared", "wallpapers")
BUDGET = 4 * 1024 * 1024

# id, title, look (dark/light: the text colour the new tab page uses on it), default dim (%), source
WALLPAPERS = [
    ("aurora", "Aurora", "dark", 25, "originals/aurora.png"),
    ("nebula", "Nebula", "dark", 15, "originals/nebula.png"),
    ("bokeh", "City lights", "dark", 25, "originals/bokeh.png"),
    ("dusk", "Dusk", "dark", 10, "gradient:dusk"),
    ("forest", "Misty forest", "dark", 35, "originals/forest.png"),
    ("lake", "Alpine lake", "dark", 30, "originals/lake.png"),
    ("dunes", "Dunes", "light", 0, "originals/dunes.png"),
    ("coast", "Coast", "light", 0, "originals/coast.png"),
    ("paper", "Paper waves", "light", 0, "originals/paper.png"),
    ("mint", "Mint", "light", 0, "gradient:mint"),
]


def gradient(kind, w=2048, h=1152):
    y, x = np.mgrid[0:h, 0:w].astype(np.float32)
    u, v = x / w, y / h
    if kind == "dusk":
        stops = [((0.10, 0.05), (32, 27, 77)), ((0.85, 0.20), (102, 45, 110)), ((0.30, 0.95), (12, 38, 74)), ((0.95, 0.90), (196, 92, 82))]
    else:
        stops = [((0.05, 0.10), (220, 245, 236)), ((0.90, 0.15), (204, 232, 248)), ((0.20, 0.90), (236, 248, 214)), ((0.85, 0.85), (196, 236, 222))]
    acc = np.zeros((h, w, 3), np.float32)
    weight = np.zeros((h, w, 1), np.float32)
    for (cx, cy), rgb in stops:
        d = np.sqrt((u - cx) ** 2 + ((v - cy) * h / w) ** 2)[..., None]
        k = np.exp(-((d / 0.42) ** 2)) + 1e-6
        acc += k * np.array(rgb, np.float32)
        weight += k
    img = acc / weight
    # A little grain against banding.
    img += np.random.default_rng(7).normal(0, 1.2, img.shape)
    return Image.fromarray(np.clip(img, 0, 255).astype(np.uint8))


def main():
    os.makedirs(OUT, exist_ok=True)
    meta, total = [], 0
    for wid, title, look, dim, src in WALLPAPERS:
        if src.startswith("gradient:"):
            img = gradient(src.split(":", 1)[1])
        else:
            path = os.path.join(HERE, src)
            if not os.path.exists(path):
                sys.exit(f"missing {path}: regenerate it from SOURCES.txt")
            img = Image.open(path).convert("RGB")
        if img.width > 2560:
            img = img.resize((2560, round(img.height * 2560 / img.width)), Image.LANCZOS)
        full = os.path.join(OUT, f"{wid}.webp")
        img.save(full, "WEBP", quality=85, method=6)
        thumb = img.resize((320, round(img.height * 320 / img.width)), Image.LANCZOS)
        tpath = os.path.join(OUT, f"{wid}-thumb.webp")
        thumb.save(tpath, "WEBP", quality=72, method=6)
        size = os.path.getsize(full) + os.path.getsize(tpath)
        total += size
        meta.append({"id": wid, "title": title, "look": look, "dim": dim})
        print(f"{wid:8s} {img.width}x{img.height} {os.path.getsize(full) // 1024:5d} KB + thumb {os.path.getsize(tpath) // 1024} KB")
    with open(os.path.join(OUT, "wallpapers.json"), "w") as f:
        json.dump(meta, f, indent=1)
        f.write("\n")
    print(f"total {total / 1024 / 1024:.2f} MB (budget 4 MB)")
    if total > BUDGET:
        sys.exit("over the 4 MB budget")


if __name__ == "__main__":
    main()
