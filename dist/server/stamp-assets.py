#!/usr/bin/env python3
"""Adds a content hash to the site's own asset links: /app.js -> /app.js?v=1a2b3c4d5e.

Why: Cloudflare (in front of the live site) replaces Caddy's "no-cache" on scripts, styles and
images with its own browser cache time of four hours. A visitor then gets the new page with the
old /app.js (the Android download box stayed hidden that way on 2026-11-11). The pages themselves
are never cached, so a link that changes with the file's content always loads the current file.

Usage: stamp-assets.py SITE_DIR      (run when the image is built, see Dockerfile)

Rewrites every *.html below SITE_DIR in place. Only root-relative links (href, src, srcset) to
files that exist below SITE_DIR are touched; a link that already has a query stays as it is.
Fails when a page links to a script or style sheet that does not exist.
"""
import hashlib
import pathlib
import re
import sys

LINK = re.compile(r'\b(href|src|srcset)="(/[^"?#\s]+\.(css|js|svg|png|webp))"')
MUST_EXIST = {"css", "js"}


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    root = pathlib.Path(sys.argv[1]).resolve()
    hashes: dict[str, str] = {}
    missing: list[str] = []
    stamped = 0

    def version(url: str, page: pathlib.Path, ext: str) -> str | None:
        if url not in hashes:
            target = (root / url.lstrip("/")).resolve()
            if root not in target.parents or not target.is_file():
                if ext in MUST_EXIST:
                    missing.append(f"{page.relative_to(root)}: {url}")
                return None
            hashes[url] = hashlib.sha256(target.read_bytes()).hexdigest()[:10]
        return hashes[url]

    for page in sorted(root.rglob("*.html")):
        text = page.read_text(encoding="utf-8")

        def repl(m: re.Match) -> str:
            nonlocal stamped
            v = version(m.group(2), page, m.group(3))
            if v is None:
                return m.group(0)
            stamped += 1
            return f'{m.group(1)}="{m.group(2)}?v={v}"'

        new = LINK.sub(repl, text)
        if new != text:
            page.write_text(new, encoding="utf-8")

    if missing:
        print("stamp-assets: links to files that do not exist:\n  " + "\n  ".join(missing), file=sys.stderr)
        return 1
    print(f"stamp-assets: {stamped} links to {len(hashes)} files stamped")
    return 0


if __name__ == "__main__":
    sys.exit(main())
