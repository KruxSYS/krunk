"""Serves the Speedometer copy on 127.0.0.1:8096 for the Krunk VM harness and stores the
results a run posts to /result as results/latest.json (errors: /error, results/errors.txt). Test tool, VM only."""
import http.server
import os
import sys

ROOT = os.path.dirname(os.path.abspath(__file__))
os.makedirs(os.path.join(ROOT, "results"), exist_ok=True)


class Handler(http.server.SimpleHTTPRequestHandler):
    # HTTP/1.1 keep-alive: with the default HTTP/1.0 (a connection per file) Chromium 156 (Krunk,
    # Edge 156) intermittently ran TodoMVC-WebComponents without its input field; the live
    # browserbench.org copy and this server with keep-alive do not.
    protocol_version = "HTTP/1.1"
    extensions_map = {**http.server.SimpleHTTPRequestHandler.extensions_map, ".mjs": "text/javascript", ".js": "text/javascript"}

    def __init__(self, *a, **kw):
        super().__init__(*a, directory=ROOT, **kw)

    def do_POST(self):
        names = {"/result": "latest.json", "/error": "errors.txt"}
        if self.path not in names:
            self.send_error(404)
            return
        body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        mode = "ab" if self.path == "/error" else "wb"
        with open(os.path.join(ROOT, "results", names[self.path]), mode) as f:
            f.write(body + (b"\n" if self.path == "/error" else b""))
        self.send_response(204)
        self.end_headers()

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def log_message(self, *a):
        pass


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8096
    http.server.ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
