"""VM helper (Krunk 0.15.0, WP-E1): fills the downloads table of a profile Krunk 0.15 has opened
once (storage V9), with Krunk closed:

    python vm-0150-seed-downloads.py %LOCALAPPDATA%\\Krunk\\browser.db [count=2000]

Rows go after the ones there (new ids), newest a minute ago, the rest spread over the last ~3
weeks (some today and yesterday); every 13th failed ("Network error"), every 29th cancelled, the
rest completed. Files don't exist: Open / Show in folder say "The file no longer exists" (that's
expected). The rows match core/src/downloads.rs `write_row` (json = the DownloadItem, camelCase).
"""
import json
import sqlite3
import sys
import time

NAMES = ["report.pdf", "setup-x64.msi", "photos.zip", "talk.mp4", "song.mp3", "data.csv", "ubuntu-24.04.iso",
         "notes.txt", "logo.png", "app.js", "slides.pptx", "backup.7z", "invoice.pdf", "driver.exe", "podcast.m4a"]
SITES = ["github.com", "releases.example.org", "cdn.example.net", "arxiv.org", "files.example.com", "drive.example.io",
         "download.mozilla.org", "dl.google.com"]


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    path = sys.argv[1]
    count = int(sys.argv[2]) if len(sys.argv) > 2 else 2000
    db = sqlite3.connect(path)
    version = db.execute("PRAGMA user_version").fetchone()[0]
    if version < 9:
        sys.exit(f"{path}: schema version {version}; start Krunk 0.15 once first (it creates the downloads table)")
    first = (db.execute("SELECT max(id) FROM download_items").fetchone()[0] or 0) + 1
    now = int(time.time() * 1000)
    # ~3 weeks back: the gap between two downloads.
    step = (21 * 86400000) // max(count, 1)
    rows = []
    for k in range(count):
        ident = first + count - 1 - k  # newest = highest id, as Krunk numbers them
        base = NAMES[k % len(NAMES)]
        stem, dot, ext = base.rpartition(".")
        name = f"{stem}-{ident}.{ext}" if dot else f"{base}-{ident}"
        site = SITES[(k * 5) % len(SITES)]
        url = f"https://{site}/files/{name}"
        started = now - 60000 - k * step - (k * 7919) % 60000
        size = 20000 + ((k * 104729) % 4000) * 65536
        if k % 29 == 0:
            state, reason, received = "cancelled", None, size // 4
        elif k % 13 == 0:
            state, reason, received = "interrupted", "Network error", size // 3
        else:
            state, reason, received = "completed", None, size
        finished = started + 2000 + (k % 90) * 1000
        item = {
            "id": ident, "url": url, "fileName": name, "path": "C:\\Users\\Public\\Downloads\\krunk-seed\\" + name,
            "state": state, "received": received, "total": size, "started": started, "reason": reason,
            "private": False, "finished": finished, "pageUrl": f"https://{site}/releases", "connections": None,
        }
        rows.append((ident, started, finished, state, f"{name}\n{url}".lower(), json.dumps(item, separators=(",", ":"))))
    with db:
        db.executemany("INSERT INTO download_items(id, started, ended, state, search, json) VALUES (?, ?, ?, ?, ?, ?)", rows)
    total = db.execute("SELECT count(*) FROM download_items").fetchone()[0]
    db.close()
    print(f"seeded {count} downloads (ids {first}..{first + count - 1}); the table has {total}")


if __name__ == "__main__":
    main()
