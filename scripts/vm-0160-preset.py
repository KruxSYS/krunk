"""VM helper (installed as C:\\pipeline\\krunk\\preset0160.py): puts a built-in style preset into
a Krunk profile's settings while Krunk is closed, as choosing it in Settings would.

  python preset0160.py PROFILE_DIR LOOK THEME

LOOK is a preset id from presets0160.json (made by `cargo run -p core --example presets`), or
`glass-purple` (Glass with a purple accent) or `strip-purple` (Classic with a purple tab strip).
THEME is light or dark.
"""
import json
import os
import sqlite3
import sys

profile, look, theme = sys.argv[1], sys.argv[2], sys.argv[3]
presets = json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "presets0160.json"), encoding="utf-8"))
extra = {}
if look == "glass-purple":
    look, extra = "glass", {"accent": "#7c3aed"}
elif look == "strip-purple":
    look, extra = "classic", {"stripColor": "#6d28d9"}
appearance = dict(presets[look])
appearance.update(extra)
db = sqlite3.connect(os.path.join(profile, "Krunk", "browser.db"))
row = db.execute("SELECT value FROM kv WHERE key = 'settings'").fetchone()
settings = json.loads(row[0]) if row else {}
settings["appearance"] = appearance
settings["theme"] = theme
db.execute("INSERT OR REPLACE INTO kv (key, value) VALUES ('settings', ?)", (json.dumps(settings),))
db.commit()
print(look, theme, appearance.get("tabStyle"), appearance.get("gradientFrom"), extra)
