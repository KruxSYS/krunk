"""VM helper (C:\\pipeline\\krunk\\seed-macro.py): adds the macro "Hold A" with the hotkey
Ctrl+Alt+K to a closed Krunk profile (0.12.0 M5).

  python seed-macro.py PROFILE_DIR
"""
import os
import sqlite3
import sys

db = sqlite3.connect(os.path.join(sys.argv[1], "Krunk", "browser.db"))
src = "await key.down('a');\nawait repeat('forever', () => wait(1000));"
db.execute(
    "INSERT OR REPLACE INTO macros(id, name, source, hotkey, enabled, created_by_ai, created, updated, position)"
    " VALUES ('hold-a', 'Hold A', ?, 'Ctrl+Alt+K', 1, 0, 1, 1, 1)",
    (src,),
)
db.commit()
print(db.execute("SELECT id, hotkey FROM macros").fetchall())

# With --userscript: a script on every web page that marks the title.
if "--userscript" in sys.argv:
    us = "// ==UserScript==\n// @name Title marker\n// @match *://*/*\n// @grant none\n// ==/UserScript==\n" \
         "document.title = 'US RAN ' + document.title;\n"
    db.execute(
        "INSERT OR REPLACE INTO userscripts(id, name, namespace, version, source, enabled, created_by_ai, created, updated, position)"
        " VALUES ('title-marker', 'Title marker', '', '', ?, 1, 0, 1, 1, 1)",
        (us,),
    )
    db.commit()
    print(db.execute("SELECT id FROM userscripts").fetchall())
