#!/usr/bin/env python3
"""Find nodes in a `uiautomator dump` (XML on stdin).

  uix.py find <text|desc> <value> [--contains]   centre "x y" of the first match; exit 1 if none
  uix.py attr <text|desc> <value> <attribute> [--clickable]
                                                 an attribute of the first match (checked, enabled, bounds, ...);
                                                 with --clickable of its nearest clickable ancestor (Compose puts
                                                 a radio button's state on the node around its description)
  uix.py size <text|desc> <value> [--clickable]  "width height" in pixels of the first match; with
                                                 --clickable of its nearest clickable ancestor (the
                                                 touch target of an icon is the button around it)
  uix.py has <text|desc> <value> [--contains]    exit 0 / 1
  uix.py list                                    every non-empty text / content-desc with its bounds

Compose puts a node's text into `text` and its contentDescription into `content-desc`.
"""
import re
import sys
import xml.etree.ElementTree as ET

KEY = {"text": "text", "desc": "content-desc"}


PARENT = {}


def nodes():
    data = sys.stdin.read()
    start = data.find("<?xml")
    if start < 0:
        return []
    try:
        root = ET.fromstring(data[start:])
    except ET.ParseError:
        return []
    for parent in root.iter():
        for child in parent:
            PARENT[child] = parent
    for web in root.iter("node"):
        if web.get("class", "").endswith("WebView") and web in PARENT:
            shift_page(web, PARENT[web])
    return list(root.iter("node"))


def shift_page(web, holder):
    """Shortly after a load the engine reports a page's nodes relative to its view, not the screen.

    The view's holder always has screen bounds: move the page's nodes to its origin.
    """
    got = re.findall(r"\d+", web.get("bounds", ""))
    want = re.findall(r"\d+", holder.get("bounds", ""))
    if len(got) != 4 or len(want) != 4:
        return
    dx, dy = int(want[0]) - int(got[0]), int(want[1]) - int(got[1])
    if dx == 0 and dy == 0:
        return
    for n in web.iter("node"):
        b = [int(v) for v in re.findall(r"\d+", n.get("bounds", ""))]
        if len(b) == 4:
            n.set("bounds", "[%d,%d][%d,%d]" % (b[0] + dx, b[1] + dy, b[2] + dx, b[3] + dy))


def match(kind, value, contains):
    for n in nodes():
        got = n.get(KEY[kind], "")
        if got == value or (contains and value in got):
            return n
    return None


def centre(n):
    x1, y1, x2, y2 = (int(v) for v in re.findall(r"\d+", n.get("bounds", "")))
    return (x1 + x2) // 2, (y1 + y2) // 2


def main():
    cmd = sys.argv[1]
    contains = "--contains" in sys.argv
    clickable = "--clickable" in sys.argv
    args = [a for a in sys.argv[2:] if a not in ("--contains", "--clickable")]
    if cmd == "list":
        for n in nodes():
            if n.get("text") or n.get("content-desc"):
                print("%-28s text=%r desc=%r checked=%s enabled=%s" % (n.get("bounds"), n.get("text"), n.get("content-desc"), n.get("checked"), n.get("enabled")))
        return 0
    n = match(args[0], args[1], contains)
    if n is None:
        return 1
    if cmd == "find":
        print("%d %d" % centre(n))
    elif cmd == "attr":
        # A selected radio button is checkable but no longer clickable.
        while clickable and n.get("clickable") != "true" and n.get("checkable") != "true" and n in PARENT:
            n = PARENT[n]
        print(n.get(args[2], ""))
    elif cmd == "size":
        while clickable and n.get("clickable") != "true" and n in PARENT:
            n = PARENT[n]
        x1, y1, x2, y2 = (int(v) for v in re.findall(r"\d+", n.get("bounds", "")))
        print("%d %d" % (x2 - x1, y2 - y1))
    return 0


sys.exit(main())
