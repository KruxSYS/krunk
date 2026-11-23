"""Krunk 0.28.0 harness (VM): edits a profile's settings before start.
python setprof.py <browser.db> show | key=jsonvalue ... (dotted keys, e.g. protection.level="strict")"""
import json
import sqlite3
import sys

db = sqlite3.connect(sys.argv[1])
row = db.execute("select value from kv where key='settings'").fetchone()
s = json.loads(row[0]) if row else {}
if sys.argv[2] == "show":
    keys = sys.argv[3:] or list(s)
    for k in keys:
        print(k, "=", json.dumps(s.get(k))[:300])
    sys.exit()
for kv in sys.argv[2:]:
    k, v = kv.split("=", 1)
    parts = k.split(".")
    o = s
    for p in parts[:-1]:
        o = o.setdefault(p, {})
    # PowerShell 5.1 drops the quotes of `key="text"` on its way to a native program: a value
    # that isn't JSON is taken as a string.
    try:
        o[parts[-1]] = json.loads(v)
    except ValueError:
        o[parts[-1]] = v
db.execute("insert or replace into kv(key,value) values('settings',?)", (json.dumps(s),))
db.commit()
print("ok")
