"""Serves the TV harness pages and logs every request line to stdout (tv.sh reads it)."""
import http.server
import os
import sys

os.chdir(os.path.dirname(os.path.abspath(__file__)))


class H(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        if self.path.startswith("/clicked"):
            self.send_response(204)
            self.end_headers()
        else:
            super().do_GET()

    def log_message(self, fmt, *args):
        sys.stdout.write("REQ %s\n" % self.requestline)
        sys.stdout.flush()


http.server.ThreadingHTTPServer(("0.0.0.0", int(sys.argv[1]) if len(sys.argv) > 1 else 18080), H).serve_forever()
