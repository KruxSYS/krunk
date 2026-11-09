"""VM helper (Krunk 0.11.0): bookmarks for the bar tests in a profile Krunk created (Krunk closed).
python seed.py PROFILE_DIR. The bar: News (with a subfolder Deep), Dev, three local icon sites
(never visited: the favicon probe's test), then example.com fillers and, past them, a folder
Later (subfolder Inner) so that "»" holds it."""
import os
import sqlite3
import sys
import time

db = sqlite3.connect(os.path.join(sys.argv[1], "Krunk", "browser.db"))
now = int(time.time() * 1000)
db.execute("DELETE FROM bookmarks WHERE id > 2")
next_id = [100]


def add(parent, title, url=None, pos=0):
    i = next_id[0]
    next_id[0] += 1
    db.execute("INSERT INTO bookmarks (id, parent, is_folder, title, url, position, added) VALUES (?,?,?,?,?,?,?)",
               (i, parent, 0 if url else 1, title, url, pos, now))
    return i


pos = 0
news = add(1, "News", pos=pos); pos += 1
for k, (t, u) in enumerate([("BBC News", "https://www.bbc.co.uk/news"), ("Hacker News", "https://news.ycombinator.com/"),
                            ("The Verge", "https://www.theverge.com/")]):
    add(news, t, u, k)
deep = add(news, "Deep", pos=3)
add(deep, "Rust", "https://www.rust-lang.org/", 0)
add(deep, "Docs.rs", "https://docs.rs/", 1)
dev = add(1, "Dev", pos=pos); pos += 1
for k, (t, u) in enumerate([("GitHub", "https://github.com/"), ("MDN", "https://developer.mozilla.org/"),
                            ("crates.io", "https://crates.io/"), ("Can I use", "https://caniuse.com/")]):
    add(dev, t, u, k)
for t, u in [("Icon site", "http://127.0.0.1:8101/"), ("Link site", "http://localhost:8102/"), ("No icon", "http://127.0.0.2:8103/")]:
    add(1, t, u, pos); pos += 1
for k in range(14):
    add(1, "Example page %02d" % k, "https://example.com/?n=%d" % k, pos); pos += 1
later = add(1, "Later", pos=pos); pos += 1
add(later, "Wikipedia", "https://en.wikipedia.org/", 0)
inner = add(later, "Inner", pos=1)
add(inner, "Words page", "http://localhost:8094/words.html", 0)
add(inner, "IANA", "https://www.iana.org/", 1)
add(1, "Last one", "https://www.iana.org/domains", pos)
db.commit()
print("bookmarks:", db.execute("SELECT count(*) FROM bookmarks").fetchone()[0])
