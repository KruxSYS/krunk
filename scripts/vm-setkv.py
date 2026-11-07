"""VM helper (installed as C:\\pipeline\\krunk\\setkv.py): merges a JSON patch into a Krunk
profile's settings (the `settings` row of browser.db's kv table) while Krunk is closed.

  python setkv.py PROFILE_DIR '{"adblockEnabled": false}' [KEY]
  python setkv.py PROFILE_DIR @patch.json [KEY]      (the patch from a file)
"""
import json
import os
import sqlite3
import sys


def merge(dst, patch):
    for k, v in patch.items():
        if isinstance(v, dict) and isinstance(dst.get(k), dict):
            merge(dst[k], v)
        else:
            dst[k] = v
    return dst


profile, arg = sys.argv[1], sys.argv[2]
patch = json.load(open(arg[1:], encoding="utf-8")) if arg.startswith("@") else json.loads(arg)
key = sys.argv[3] if len(sys.argv) > 3 else "settings"
db = sqlite3.connect(os.path.join(profile, "Krunk", "browser.db"))
row = db.execute("SELECT value FROM kv WHERE key = ?", (key,)).fetchone()
value = merge(json.loads(row[0]) if row else {}, patch)
db.execute("INSERT OR REPLACE INTO kv (key, value) VALUES (?, ?)", (key, json.dumps(value)))
db.commit()
print(json.dumps({k: value.get(k) for k in patch}))
