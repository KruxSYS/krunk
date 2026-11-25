#!/usr/bin/env python3
"""Krunk's CHANGELOG.md: check it, render the website's /changelog page, or print one version.

  python3 -I dist/server/changelog.py check    CHANGELOG.md
  python3 -I dist/server/changelog.py render   CHANGELOG.md OUT.html [CHANGELOG-android.md]
  python3 -I dist/server/changelog.py section  CHANGELOG.md VERSION   (Markdown body, for NOTES.md)
  python3 -I dist/server/changelog.py date     CHANGELOG.md VERSION   (its YYYY-MM-DD)
  python3 -I dist/server/changelog.py selftest

CHANGELOG.md is a strict Markdown subset:
  "# Title", intro paragraphs, then one "## <semver> — <YYYY-MM-DD>" (em dash) per version,
  newest first. Inside: paragraphs, "###".."######" headings, "-" bullets nested by two spaces
  (continuation lines indented), **bold**, *em*, `code`, and [text](url) links where the URL is
  https:// or site-relative ("/..." or "#..."); any other link stays plain text. All text is
  HTML-escaped. The page has no inline script or style (the site's CSP allows neither).

Standard library only. The web image's build runs check and render (dist/server/Dockerfile);
publish-release.sh uses section and date. Exit codes: 0 ok, 1 invalid or missing, 2 usage.
"""
import datetime
import html
import os
import re
import sys
import tempfile

VERSION_RE = r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-[0-9A-Za-z.-]+)?"
SEMVER = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?$")
HEADING = re.compile(r"^## (\S+) — ([0-9]{4}-[0-9]{2}-[0-9]{2})$")
SUBHEADING = re.compile(r"^(#{3,6}) (\S.*)$")
BULLET = re.compile(r"^( *)- (\S.*)$")
LINK = re.compile(r"\[([^\[\]\n]+)\]\(([^()\s]+)\)")
URL_CHARS = re.compile(r"^[A-Za-z0-9\-._~:/?#@!$&'*+,;=%]+$")
MONTHS = ("January", "February", "March", "April", "May", "June", "July", "August",
          "September", "October", "November", "December")


class Section:
    def __init__(self, version, date, lineno):
        self.version, self.date, self.lineno, self.lines = version, date, lineno, []

    def body(self):
        """The section's Markdown, without the heading and surrounding blank lines."""
        return "\n".join(self.lines).strip("\n")


class Changelog:
    def __init__(self):
        self.title, self.intro, self.sections, self.errors = None, [], [], []
        # An optional "## Unreleased" section above the newest version: what the next release
        # will contain. It is checked like a version's body but never rendered or published.
        self.unreleased = None


def semver_key(v):
    m = SEMVER.match(v)
    major, minor, patch, pre = int(m.group(1)), int(m.group(2)), int(m.group(3)), m.group(4)
    if pre is None:
        return (major, minor, patch, 1, ())
    ids = tuple((0, int(p), "") if p.isdigit() else (1, 0, p) for p in pre.split("."))
    return (major, minor, patch, 0, ids)


def parse(text):
    """Splits the file into title, intro and sections; collects every problem in .errors."""
    doc = Changelog()
    err = doc.errors.append
    lines = text.split("\n")
    if lines and lines[-1] == "":
        lines.pop()
    current = None
    for n, line in enumerate(lines, 1):
        if "\r" in line:
            err(f"line {n}: carriage return (use LF line endings)")
            line = line.replace("\r", "")
        if "\t" in line:
            err(f"line {n}: tab character (indent with spaces)")
        if n == 1:
            if line.startswith("# ") and line[2:].strip():
                doc.title = line[2:].strip()
                continue
            err("line 1: the file must start with a '# Title' line")
        if re.match(r"^#(?!#)", line):
            err(f"line {n}: only line 1 may be a '#' heading")
            continue
        if line == "## Unreleased":
            if doc.sections or doc.unreleased:
                err(f"line {n}: '## Unreleased' may only come once, above the newest version")
                current = None
                continue
            current = doc.unreleased = Section("Unreleased", None, n)
            continue
        if re.match(r"^##(?!#)", line):
            m = HEADING.match(line)
            if not m:
                err(f"line {n}: malformed version heading {line!r} (want '## X.Y.Z — YYYY-MM-DD')")
                current = None
                continue
            version, date = m.group(1), m.group(2)
            current = Section(version, date, n)
            doc.sections.append(current)
            continue
        (current.lines if current else doc.intro).append(line)
    if doc.title is None and not any(e.startswith("line 1") for e in doc.errors):
        err("line 1: the file must start with a '# Title' line")
    return doc


def check(doc):
    """Adds the heading-level problems (versions, dates, order, duplicates, bodies) to .errors."""
    err = doc.errors.append
    if not doc.sections:
        err("no version sections")
    if doc.unreleased is not None:
        where = f"line {doc.unreleased.lineno} (Unreleased)"
        if not doc.unreleased.body().strip():
            err(f"{where}: empty section")
        blocks(doc.unreleased.lines, lambda m: err(f"{where}: {m}"))
    seen = {}
    prev = None
    for s in doc.sections:
        where = f"line {s.lineno} ({s.version})"
        good = True
        if not SEMVER.match(s.version):
            err(f"{where}: not a semver version")
            good = False
        try:
            datetime.date.fromisoformat(s.date)
        except ValueError:
            err(f"{where}: not a valid date: {s.date}")
            good = False
        if s.version in seen:
            err(f"{where}: duplicate of line {seen[s.version]}")
            good = False
        seen.setdefault(s.version, s.lineno)
        if not s.body().strip():
            err(f"{where}: empty section")
        blocks(s.lines, lambda m, w=where: err(f"{w}: {m}"))
        if good and prev is not None:
            if semver_key(s.version) >= semver_key(prev.version):
                err(f"{where}: versions must be strictly descending (after {prev.version})")
            if s.date > prev.date:
                err(f"{where}: dated {s.date}, after the newer {prev.version} ({prev.date})")
        if good:
            prev = s
    return doc.errors


# ---- Blocks -----------------------------------------------------------------------------------

def blocks(lines, problem=lambda m: None):
    """Paragraphs, subheadings and nested bullet lists, as ("p", text), ("h", level, text) and
    ("ul", [item]) with item = {"text": str, "children": [item] | None}."""
    out = []
    i, n = 0, len(lines)
    while i < n:
        line = lines[i]
        if not line.strip():
            i += 1
            continue
        h = SUBHEADING.match(line)
        if h:
            out.append(("h", len(h.group(1)), h.group(2).strip()))
            i += 1
            continue
        b = BULLET.match(line)
        if b:
            if b.group(1):
                problem(f"a list must start unindented: {line.strip()[:40]!r}")
            items, i = parse_list(lines, i, problem)
            out.append(("ul", items))
            continue
        if line.startswith(" "):
            problem(f"indented line outside a list: {line.strip()[:40]!r}")
        if re.match(r"^\s*([*+]|[0-9]+[.)])\s", line):
            problem(f"unsupported list marker (use '- '): {line.strip()[:40]!r}")
        para = []
        while i < n and lines[i].strip() and not SUBHEADING.match(lines[i]) and not BULLET.match(lines[i]):
            para.append(lines[i].strip())
            i += 1
        out.append(("p", "\n".join(para)))
    return out


def parse_list(lines, i, problem):
    base = len(BULLET.match(lines[i]).group(1))
    root = []
    stack = [(base, root)]
    while i < len(lines):
        line = lines[i]
        if not line.strip() or SUBHEADING.match(line):
            break
        b = BULLET.match(line)
        if b:
            ind = len(b.group(1))
            while len(stack) > 1 and ind < stack[-1][0]:
                stack.pop()
            if ind > stack[-1][0]:
                if ind != stack[-1][0] + 2:
                    problem(f"nested bullet indented {ind - stack[-1][0]} spaces, not 2: {line.strip()[:40]!r}")
                parent = stack[-1][1][-1]
                if parent["children"] is None:
                    parent["children"] = []
                stack.append((ind, parent["children"]))
            elif ind < stack[-1][0]:
                problem(f"bullet indented less than the list: {line.strip()[:40]!r}")
            stack[-1][1].append({"text": b.group(2).strip(), "children": None})
        else:
            if not line.startswith(" "):
                problem(f"unindented line inside a list: {line.strip()[:40]!r}")
            item = stack[-1][1][-1]
            item["text"] += "\n" + line.strip()
        i += 1
    return root, i


# ---- Inline -----------------------------------------------------------------------------------

def esc(s):
    """For attribute values."""
    return html.escape(s, quote=True)


def text(s):
    """For element content: quotes and apostrophes stay as they are."""
    return html.escape(s, quote=False)


def safe_url(url):
    if not URL_CHARS.match(url):
        return False
    if url.startswith("https://"):
        return len(url) > 8 and url[8] not in "/?#@"
    return (url.startswith("/") and not url.startswith("//")) or url.startswith("#")


def inline(s, links=True):
    out = []
    i, n = 0, len(s)
    while i < n:
        c = s[i]
        if c == "`":
            j = s.find("`", i + 1)
            if j > i + 1:
                out.append("<code>" + text(s[i + 1:j]) + "</code>")
                i = j + 1
                continue
        elif c == "[" and links:
            m = LINK.match(s, i)
            if m:
                if safe_url(m.group(2)):
                    out.append(f'<a href="{esc(m.group(2))}">{inline(m.group(1), links=False)}</a>')
                else:
                    out.append(text(m.group(0)))
                i = m.end()
                continue
        elif c == "*":
            if s.startswith("**", i):
                j = s.find("**", i + 2)
                if j > i + 2 and not s[i + 2].isspace() and not s[j - 1].isspace():
                    out.append("<strong>" + inline(s[i + 2:j], links) + "</strong>")
                    i = j + 2
                    continue
                out.append("**")
                i += 2
                continue
            j = i + 1
            while True:
                j = s.find("*", j)
                if j < 0 or not s.startswith("**", j):
                    break
                j += 2
            if j > i + 1 and not s[i + 1].isspace() and not s[j - 1].isspace():
                out.append("<em>" + inline(s[i + 1:j], links) + "</em>")
                i = j + 1
                continue
        out.append(text(c))
        i += 1
    return "".join(out)


def render_items(items, depth):
    pad = "  " * depth
    out = [pad + "<ul>"]
    for it in items:
        text = inline(it["text"])
        if it["children"]:
            out.append(f"{pad}  <li>{text}")
            out.extend(render_items(it["children"], depth + 2))
            out.append(f"{pad}  </li>")
        else:
            out.append(f"{pad}  <li>{text}</li>")
    out.append(pad + "</ul>")
    return out


def render_blocks(bs, depth, lead=False):
    pad = "  " * depth
    out = []
    for b in bs:
        if b[0] == "p":
            cls = ' class="lead"' if lead and not out else ""
            out.append(f"{pad}<p{cls}>{inline(b[1])}</p>")
        elif b[0] == "h":
            out.append(f"{pad}<h{b[1]}>{inline(b[2])}</h{b[1]}>")
        else:
            out.extend(render_items(b[1], depth))
    return out


def long_date(iso):
    d = datetime.date.fromisoformat(iso)
    return f"{d.day} {MONTHS[d.month - 1]} {d.year}"


PAGE_HEAD = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Changelog · Krunk</title>
<meta name="description" content="What changed in each Krunk release, newest first.">
<meta name="color-scheme" content="dark">
<meta name="theme-color" content="#07090c">
<meta property="og:site_name" content="Krunk · KruxSYS">
<link rel="icon" type="image/png" href="/krunk-64.png">
<link rel="stylesheet" href="/style.css">
</head>
<body class="site">
<!-- Generated from CHANGELOG.md (and CHANGELOG-android.md) by dist/server/changelog.py; do not edit. -->
<a class="skip" href="#main">Skip to content</a>
<header class="bar site-bar">
  <div class="wrap">
    <a class="brand" href="/"><img src="/krunk.svg" width="30" height="30" alt=""><span class="wordmark">Krunk</span></a>
    <a class="by-kx" href="https://kruxsys.com/projects/krunk/" title="Krunk on kruxsys.com"><span class="by-text">by</span> <img src="/kruxsys.svg" width="18" height="18" alt=""><span class="kx-word">Krux<span class="accent">SYS</span></span></a>
    <nav class="nav" aria-label="Site">
      <a href="/#features">Features</a>
      <a href="/download">Download</a>
      <a href="/changelog" aria-current="page">Changelog</a>
      <a href="/privacy">Privacy</a>
      <a href="/account/">Account</a>
    </nav>
  </div>
</header>
<div class="doc-layout">
"""

PAGE_FOOT = """</div>
<footer class="site-foot">
  <div class="wrap foot-grid">
    <div class="foot-brand">
      <a href="https://kruxsys.com/"><img src="/kruxsys.svg" width="28" height="28" alt=""><span class="kx-word">Krux<span class="accent">SYS</span></span></a>
      <p>Krunk is a KruxSYS product: a fast, private browser for Windows, with a preview for Android. Open source (MIT), no telemetry.</p>
      <p class="foot-note">Bundles Chromium through the Chromium Embedded Framework, and filter lists by EasyList and uBlock Origin; see the notices in the download.</p>
    </div>
    <nav class="foot-col" aria-label="Krunk">
      <h2>Krunk</h2>
      <ul>
        <li><a href="/download">Download</a></li>
        <li><a href="/changelog">Changelog</a></li>
        <li><a href="/privacy">Privacy</a></li>
        <li><a href="/account/">Account</a></li>
        <li><a href="https://github.com/KruxSYS/krunk" rel="noopener">GitHub</a></li>
        <li><a href="mailto:privacy@kruxsys.com">Contact</a></li>
      </ul>
    </nav>
    <nav class="foot-col" aria-label="KruxSYS">
      <h2>KruxSYS</h2>
      <ul>
        <li><a href="https://kruxsys.com/projects/krunk/">Project page</a></li>
        <li><a href="https://kruxsys.com/">kruxsys.com</a></li>
        <li><a href="https://kruxsys.com/projects/screvia/">Screvia</a></li>
        <li><a href="https://kruxsys.com/projects/plexdio/">Plexdio</a></li>
      </ul>
    </nav>
    <p class="foot-meta"><span>MOD/01 · Krunk</span><span>© 2026 KruxSYS</span></p>
  </div>
</footer>
</body>
</html>
"""


def render_page(doc, android=None):
    out = [PAGE_HEAD.rstrip("\n")]
    out.append('<nav class="toc versions" aria-label="Versions">')
    out.append("  <h2>Versions</h2>")
    out.append("  <ol>")
    for s in doc.sections:
        v = esc(s.version)
        out.append(f'    <li><a href="#{v}">{v}</a></li>')
    out.append("  </ol>")
    if android is not None:
        # Krunk for Android has its own version numbers: its anchors are android-<version>.
        out.append('  <h2><a href="#android">Android</a></h2>')
        out.append("  <ol>")
        for s in android.sections:
            v = esc(s.version)
            out.append(f'    <li><a href="#android-{v}">{v}</a></li>')
        out.append("  </ol>")
    out.append("</nav>")
    out.append('<main id="main" class="prose">')
    out.append(f"  <h1>{inline(doc.title or 'Changelog')}</h1>")
    out.extend(render_blocks(blocks(doc.intro), 1, lead=True))
    for s in doc.sections:
        v = esc(s.version)
        out.append("")
        out.append(f'  <section id="{v}">')
        out.append(f'    <h2><a href="#{v}">Krunk {v}</a></h2>')
        out.append(f'    <p class="updated">Released <time datetime="{esc(s.date)}">{text(long_date(s.date))}</time></p>')
        out.extend(render_blocks(blocks(s.lines), 2))
        out.append("  </section>")
    if android is not None:
        out.append("")
        out.append('  <section id="android">')
        # The file's own intro points at files in the repository: not for the website.
        out.append("    <h2>Krunk for Android</h2>")
        out.append("    <p>What changed in each Krunk for Android release, newest first.</p>")
        out.append("  </section>")
        for s in android.sections:
            v = esc(s.version)
            out.append("")
            out.append(f'  <section id="android-{v}">')
            out.append(f'    <h2><a href="#android-{v}">Krunk for Android {v}</a></h2>')
            out.append(f'    <p class="updated">Released <time datetime="{esc(s.date)}">{text(long_date(s.date))}</time></p>')
            out.extend(render_blocks(blocks(s.lines), 2))
            out.append("  </section>")
    out.append("</main>")
    return "\n".join(out) + "\n" + PAGE_FOOT


# ---- Commands ---------------------------------------------------------------------------------

def load(path):
    try:
        with open(path, encoding="utf-8") as f:
            source = f.read()
    except (OSError, UnicodeDecodeError) as e:
        fail(f"cannot read {path}: {e}")
    doc = parse(source)
    check(doc)
    if doc.errors:
        for e in doc.errors:
            print(f"changelog.py: {path}: {e}", file=sys.stderr)
        sys.exit(1)
    return doc


def fail(msg, code=1):
    print(f"changelog.py: {msg}", file=sys.stderr)
    sys.exit(code)


def find(doc, path, version):
    if not re.fullmatch(VERSION_RE, version):
        fail(f"not a semver version: {version}", 2)
    for s in doc.sections:
        if s.version == version:
            return s
    fail(f"no section for version {version} in {path} (add '## {version} — YYYY-MM-DD' first)")


def write_out(data):
    sys.stdout.buffer.write(data.encode("utf-8"))
    sys.stdout.flush()


def main(argv):
    usage = __doc__.split("\n\n")[0]
    if not argv:
        fail("usage:\n" + usage, 2)
    cmd, args = argv[0], argv[1:]
    if cmd == "check" and len(args) == 1:
        doc = load(args[0])
        print(f"{args[0]}: ok, {len(doc.sections)} versions, newest {doc.sections[0].version}")
    elif cmd == "render" and len(args) in (2, 3):
        # An optional third file: Krunk for Android's changelog, shown after the Windows one.
        page = render_page(load(args[0]), load(args[2]) if len(args) == 3 else None)
        out_dir = os.path.dirname(os.path.abspath(args[1]))
        fd, tmp = tempfile.mkstemp(dir=out_dir, prefix=".changelog.")
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as f:
            f.write(page)
        os.chmod(tmp, 0o644)
        os.replace(tmp, args[1])
    elif cmd == "section" and len(args) == 2:
        write_out(find(load(args[0]), args[0], args[1]).body() + "\n")
    elif cmd == "date" and len(args) == 2:
        write_out(find(load(args[0]), args[0], args[1]).date + "\n")
    elif cmd == "selftest" and not args:
        selftest()
    else:
        fail("usage:\n" + usage, 2)


# ---- Self-test --------------------------------------------------------------------------------

def selftest():
    import io

    def expect(cond, what):
        if not cond:
            fail(f"selftest failed: {what}")

    # Escaping: markup in text and code is shown, never interpreted.
    expect(inline('<script>alert("x")</script> & \'q\'') ==
           "&lt;script&gt;alert(\"x\")&lt;/script&gt; &amp; 'q'", "text escaping")
    expect(inline("`<b>` **b<i>** *e*") == "<code>&lt;b&gt;</code> <strong>b&lt;i&gt;</strong> <em>e</em>",
           "code, bold and em escaping")
    expect(inline("a * b, `*.x` and 2*3*4 **") == "a * b, <code>*.x</code> and 2<em>3</em>4 **", "stray asterisks")
    expect(inline("**bold over\na line**") == "<strong>bold over\na line</strong>", "bold across lines")

    # Links: https and site-relative only; everything else is text.
    expect(inline("[site](https://krunk.kruxsys.com/x?a=1&b=2)") ==
           '<a href="https://krunk.kruxsys.com/x?a=1&amp;b=2">site</a>', "https link")
    expect(inline("[c](/changelog#0.12.4)") == '<a href="/changelog#0.12.4">c</a>', "relative link")
    for bad in ("javascript:alert(1)", "JavaScript:alert(1)", "data:text/html,x", "//evil.example",
                "/\\evil.example", "http://plain.example", "https:///x", "mailto:a@b", 'https://a"onclick=x'):
        got = inline(f"[x]({bad})")
        expect("<a" not in got, f"refused link {bad!r}: {got}")
    expect(inline("[**b** `c`](https://e.example)") == '<a href="https://e.example"><strong>b</strong> <code>c</code></a>',
           "link text inline")
    expect(inline("[x](javascript:alert(1))") == "[x](javascript:alert(1))", "refused link shown as text")

    # Nesting: three levels, continuation lines, a list right after a paragraph.
    md = ["Intro line", "**Note:** after:", "- a", "  more a", "  - b", "    - c", "      more c",
          "  - d", "- e", "", "Tail."]
    bs = blocks(md, lambda m: fail(f"selftest failed: unexpected problem {m}"))
    expect([b[0] for b in bs] == ["p", "ul", "p"], f"block kinds {bs}")
    items = bs[1][1]
    expect(len(items) == 2 and items[0]["text"] == "a\nmore a" and items[1]["text"] == "e", f"top items {items}")
    kids = items[0]["children"]
    expect([k["text"] for k in kids] == ["b", "d"] and kids[0]["children"][0]["text"] == "c\nmore c", f"nesting {kids}")
    html_out = "\n".join(render_blocks(bs, 0))
    expect(html_out.count("<ul>") == 3 and html_out.count("</li>") == 5, f"rendered nesting {html_out}")
    probs = []
    blocks(["- a", "    - too deep", "  more"], probs.append)
    expect(len(probs) == 1 and "not 2" in probs[0], f"strict indentation {probs}")
    probs = []
    blocks(["1. one", "* star"], probs.append)
    expect(len(probs) == 1 and "unsupported list marker" in probs[0], f"unsupported markers {probs}")

    # Headings, order, dates, duplicates.
    good = "# T\n\nIntro.\n\n## 0.10.0 — 2026-11-09\n\n- x\n\n## 0.9.0 — 2026-11-09\n\nY.\n\n## 0.9.0-rc.1 — 2026-11-08\n\nZ.\n"
    doc = parse(good)
    expect(not check(doc), f"good file: {doc.errors}")
    expect([s.version for s in doc.sections] == ["0.10.0", "0.9.0", "0.9.0-rc.1"], "sections")
    expect(doc.sections[0].body() == "- x", "section body")
    page = render_page(doc)
    expect('<section id="0.10.0">' in page and '<a href="#0.10.0">Krunk 0.10.0</a>' in page
           and '<time datetime="2026-11-09">9 November 2026</time>' in page and "<script" not in page
           and "style=" not in page and '<p class="lead">Intro.</p>' in page, "page markup")
    # Unreleased: checked, kept out of the sections, the page and section/date lookups.
    doc = parse("# T\n\n## Unreleased\n\n- next\n\n" + good.split("\n\n", 2)[2])
    expect(not check(doc), f"unreleased file: {doc.errors}")
    expect(doc.unreleased.body() == "- next" and doc.sections[0].version == "0.10.0", "unreleased parsed")
    expect("next" not in render_page(doc) and "Unreleased" not in render_page(doc), "unreleased not rendered")
    for bad in ("# T\n## Unreleased\n\n## 0.9.0 — 2026-11-09\nx\n",
                "# T\n## 0.9.0 — 2026-11-09\nx\n## Unreleased\n- y\n",
                "# T\n## Unreleased\n- a\n## Unreleased\n- b\n## 0.9.0 — 2026-11-09\nx\n"):
        d = parse(bad)
        expect(check(d), f"bad unreleased accepted: {bad!r}")
    bad_cases = {
        "ascending": "# T\n## 0.9.0 — 2026-11-09\nx\n## 0.10.0 — 2026-11-09\nx\n",
        "duplicate": "# T\n## 0.9.0 — 2026-11-09\nx\n## 0.9.0 — 2026-11-09\nx\n",
        "bad date": "# T\n## 0.9.0 — 2026-02-30\nx\n",
        "date order": "# T\n## 0.10.0 — 2026-11-08\nx\n## 0.9.0 — 2026-11-09\nx\n",
        "bad semver": "# T\n## 0.09.0 — 2026-11-09\nx\n",
        "hyphen not em dash": "# T\n## 0.9.0 - 2026-11-09\nx\n",
        "empty": "# T\n## 0.9.0 — 2026-11-09\n\n",
        "no title": "## 0.9.0 — 2026-11-09\nx\n",
        "second title": "# T\n## 0.9.0 — 2026-11-09\n# x\n",
        "tab": "# T\n## 0.9.0 — 2026-11-09\n-\tx\n",
    }
    for name, source in bad_cases.items():
        d = parse(source)
        expect(check(d), f"{name} must fail")

    # A missing section: exit 1 with a message naming the version.
    old_err = sys.stderr
    sys.stderr = io.StringIO()
    try:
        find(doc, "CHANGELOG.md", "1.2.3")
        expect(False, "missing section must exit")
    except SystemExit as e:
        msg = sys.stderr.getvalue()
        sys.stderr = old_err
        expect(e.code == 1 and "no section for version 1.2.3" in msg, f"missing section: {e.code} {msg}")
    finally:
        sys.stderr = old_err
    print("changelog.py selftest: ok")


if __name__ == "__main__":
    main(sys.argv[1:])
