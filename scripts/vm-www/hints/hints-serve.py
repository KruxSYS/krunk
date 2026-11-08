"""Client-hints test server (Krunk 0.7.0, R12; scripts/vm-070-hints.ps1). Serves on PORT:

  /hints.html   the test page: Accept-CH and Critical-CH for the high-entropy hints; it shows
                navigator.userAgentData in the page, a cross-site iframe (the other host name),
                a dedicated worker, a service worker and shared workers (classic, strict-mode
                classic, module with an import; 0.7.1)
  /hints.html?poll   the same; the page and the iframe report again every 2 s while the value
                changes ("page later", "iframe later": a document open while the setting changed)
  /frame.html   the iframe (its own report)
  /worker.js, /sw.js, /shared.js, /shared-strict.js, /shared-module.js, /dep.js   the workers
  /report?...   results posted by the pages and workers
Every request is logged to LOG with its Sec-CH-UA* headers (or "none").
"""
import os
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8095
LOG = sys.argv[2] if len(sys.argv) > 2 else os.path.join(os.path.dirname(os.path.abspath(__file__)), "hints.log")
lock = threading.Lock()

PAGE = """<!doctype html><meta charset="utf-8"><title>hints ?</title>
<body style="font:15px sans-serif"><h2>Client hints</h2><pre id="o"></pre>
<iframe id="f" width="600" height="80"></iframe>
<script>
window.onerror = (m, src, line) => { fetch('/report?error=' + encodeURIComponent(m + ' @' + line)); };
</script>
<script>
const out = [];
const o = document.getElementById('o');
function say(where, v) { out.push(where + ': ' + v); o.textContent = out.join('\\n'); fetch('/report?' + encodeURIComponent(where) + '=' + encodeURIComponent(v)); document.title = 'hints ' + out.length; }
const d = navigator.userAgentData;
try { say('page', typeof d === 'undefined' ? 'undefined' : JSON.stringify({ brands: d.brands.map(b => b.brand), platform: d.platform })); } catch (e) { say('page', 'error ' + e); }
const other = location.hostname === '127.0.0.1' ? 'localhost' : '127.0.0.1';
const poll = location.search.indexOf('poll') >= 0;
document.getElementById('f').src = 'http://' + other + ':' + location.port + '/frame.html' + (poll ? '?poll' : '');
function now() { const d = navigator.userAgentData; return typeof d === 'undefined' ? 'undefined' : JSON.stringify({ brands: d.brands.map(b => b.brand), platform: d.platform }); }
if (poll) { let last = now(); setInterval(() => { const v = now(); if (v !== last) { last = v; say('page later', v); } }, 2000); }
const w = new Worker('/worker.js');
w.onmessage = (e) => say('worker', e.data);
function shared(name, url, opts) {
  try {
    const s = new SharedWorker(url, opts);
    s.onerror = () => say(name, 'error event (the script did not load or run)');
    s.port.onmessage = (e) => say(name, e.data);
    s.port.start();
  } catch (e) { say(name, 'error ' + e); }
}
shared('shared worker', '/shared.js');
shared('shared strict', '/shared-strict.js');
shared('shared module', '/shared-module.js', { type: 'module' });
shared('shared module plain', '/shared-module-plain.js', { type: 'module' });
shared('shared importScripts', '/shared-import.js');
if (navigator.serviceWorker) {
  navigator.serviceWorker.register('/sw.js').then(() => navigator.serviceWorker.ready).then((r) => {
    navigator.serviceWorker.onmessage = (e) => say('service worker', e.data);
    r.active.postMessage('ask');
  }, (e) => say('service worker', 'error ' + e));
}
</script>"""

FRAME = """<!doctype html><meta charset="utf-8"><body style="font:13px sans-serif"><script>
const d = navigator.userAgentData;
const v = typeof d === 'undefined' ? 'undefined' : JSON.stringify({ brands: d.brands.map(b => b.brand), platform: d.platform });
document.body.textContent = 'iframe (' + location.host + '): ' + v;
fetch('/report?iframe=' + encodeURIComponent(v));
if (location.search.indexOf('poll') >= 0) {
  let last = v;
  setInterval(() => {
    const d = navigator.userAgentData;
    const n = typeof d === 'undefined' ? 'undefined' : JSON.stringify({ brands: d.brands.map(b => b.brand), platform: d.platform });
    if (n !== last) { last = n; fetch('/report?' + encodeURIComponent('iframe later') + '=' + encodeURIComponent(n)); }
  }, 2000);
}
</script>"""

REPORT = "typeof self.navigator.userAgentData === 'undefined' ? 'undefined' : JSON.stringify({ brands: self.navigator.userAgentData.brands.map(b => b.brand), platform: self.navigator.userAgentData.platform })"
WORKER = "postMessage(" + REPORT + ");"
SHARED = "onconnect = (e) => { e.ports[0].postMessage(" + REPORT + "); };"
# 0.7.1: a strict-mode script keeps its directive (Krunk's insertion goes after it) and a module
# worker's import runs first, so the imported module reports what it saw too.
SHARED_STRICT = ("\ufeff/* a comment first */\n'use strict';\n"
                 "onconnect = (e) => { e.ports[0].postMessage((" + REPORT + ") + ' strict=' + (function () { return this === undefined; })()); };")
DEP = "export const seen = (" + REPORT + ");"
SHARED_MODULE = ("import { seen } from '/dep.js';\n"
                 "onconnect = (e) => { e.ports[0].postMessage((" + REPORT + ") + ' import saw ' + seen); };")
SW = ("self.addEventListener('install', () => self.skipWaiting());"
      "self.addEventListener('activate', (e) => e.waitUntil(self.clients.claim()));"
      "self.addEventListener('message', (e) => e.source.postMessage(" + REPORT + "));")


def log(line):
    with lock, open(LOG, "a", encoding="utf-8") as f:
        f.write(time.strftime("%H:%M:%S") + " " + line + "\n")


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def do_GET(self):
        url = urlsplit(self.path)
        hints = sorted("%s=%s" % (k, v) for k, v in self.headers.items() if k.lower().startswith("sec-ch-ua"))
        log("%s %s host=%s hints=[%s]" % (self.command, self.path, self.headers.get("Host", ""), "; ".join(hints) or "none"))
        if url.path == "/report":
            for k, v in parse_qs(url.query).items():
                log("REPORT %s = %s" % (k, v[0]))
            return self.reply(204, b"", "text/plain")
        body, ctype, extra = {
            "/hints.html": (PAGE, "text/html; charset=utf-8", {
                "Accept-CH": "Sec-CH-UA-Platform-Version, Sec-CH-UA-Arch, Sec-CH-UA-Full-Version-List, Sec-CH-UA-Model",
                "Critical-CH": "Sec-CH-UA-Platform-Version"}),
            "/frame.html": (FRAME, "text/html; charset=utf-8", {"Accept-CH": "Sec-CH-UA-Arch"}),
            "/worker.js": (WORKER, "text/javascript", {}),
            "/shared.js": (SHARED, "text/javascript", {}),
            "/shared-strict.js": (SHARED_STRICT, "text/javascript; charset=utf-8", {}),
            "/shared-module.js": (SHARED_MODULE, "text/javascript", {}),
            "/dep.js": (DEP, "text/javascript", {}),
            "/shared-module-plain.js": (SHARED, "text/javascript", {}),
            "/shared-import.js": ("importScripts('/dep-classic.js');\nonconnect = (e) => { e.ports[0].postMessage((" + REPORT + ") + ' import saw ' + seenClassic); };", "text/javascript", {}),
            "/dep-classic.js": ("var seenClassic = (" + REPORT + ");", "text/javascript", {}),
            "/sw.js": (SW, "text/javascript", {}),
        }.get(url.path, (None, None, None))
        if body is None:
            return self.reply(404, b"not found", "text/plain")
        self.reply(200, body.encode("utf-8"), ctype, extra)

    def reply(self, status, body, ctype, extra=None):
        self.send_response(status)
        self.send_header("Content-Type", ctype)
        self.send_header("Cache-Control", "no-store")
        for k, v in (extra or {}).items():
            self.send_header(k, v)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


if __name__ == "__main__":
    ThreadingHTTPServer.daemon_threads = True
    ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
