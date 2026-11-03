# LightBrowser security notes

*Draft. The full vault threat model comes with the vault milestone.*

## Trusted UI vs. web content

### Origin split
The browser's own UI is split across **two separate origins** under the reserved `.invalid` TLD:

| Origin | Documents | Loaded in |
|--------|-----------|-----------|
| `https://shell.lightbrowser.invalid` | `/shell.html` (tab strip, toolbar, bookmarks bar, find bar) and `/popup.html` (menu, shield, bookmark folders, password prompts) | only the shell WebView and the popup WebView |
| `https://lightbrowser.invalid` | built-in pages: new tab, history, bookmarks, downloads, settings, passwords, about | tabs |

Consequences:
- **Tabs can't become the shell.** A tab can never load the shell origin; `allow_navigation`
  cancels it. Messages from tabs are classified on the tab channel, which never yields
  `Sender::Shell` or `Sender::Popup`. So even a compromised built-in page can't use shell-only
  commands such as closing or reordering tabs.
- **Separate browser storage.** Each origin has its own storage, so a built-in page can't read
  or tamper with shell state.
- **Exact matching.** The shell and popup channels accept only their own document path on the
  shell origin. Sub-frames are never trusted. A different scheme, port or subdomain, or a
  look-alike host, is rejected (`core::protocol::classify` tests).
- **Cached favicons.** `/favicon?host=` serves cached favicon bytes on both origins. Only valid
  host names are accepted, and the response is a PNG with `nosniff`.
- The browser chrome (shell), the popup and the built-in pages run on two internal origins under
  the reserved `.invalid` TLD. They are served from bytes embedded in the exe, with a strict CSP.
- The host accepts privileged messages only after `core::protocol::classify` has checked:
  - the channel (which WebView the message arrived on);
  - the exact origin and document;
  - that it came from the top-level frame.
- Each sender has a command allow-list (`Sender::permits`). For example, the new-tab page can't
  reach the vault, and the shell can't list passwords.
- Web pages can only send `ContentMessage`, which is strictly parsed and size-capped.
- Tabs can never load the shell origin. A web page can't navigate a tab to a built-in page on its
  own; only the host can (`core::assets::allow_navigation`).

## Password prompts live outside the page
- The "Save password?" and "Fill saved login?" prompts are drawn in the **popup window**. It is a
  separate top-level window owned by the browser frame, and its document is on the shell origin.
- Page content can't draw over it, style it, read it, or send it input. That rules out
  spoofing the prompt or click-jacking it with overlays or transparent iframes.
- Every prompt carries the `tab_id` and exact origin it was raised for, and the reply echoes both.
  Before filling or saving, the host re-checks that the tab is still the active tab and is still
  on that origin; otherwise the reply is dropped.
- Hiding the popup for any reason (focus loss, Escape, window move or resize, tab switch) counts
  as a dismissal, never a confirmation.

## Scripts injected into web pages
- **Which scripts:** three small scripts from `ui/inject/`, embedded and never served over any
  origin.
  - `gpc.js` and `dnt.js` only define read-only navigator properties.
  - `page-tokens.js` (top frame only) reports the page's class names and ids for generic
    cosmetic filtering.
- **What a page can do with the token message:** it can forge or flood it, but the message is
  strictly parsed (at most 2000 tokens of up to 100 characters each). Its only effect is extra
  hiding CSS on that same page, and the host applies it only if the sender's origin equals the
  tab's current top-level origin.
- **How the CSS is injected:** as a JSON string literal into a fixed script template, so it
  can't break out into code (`assets::style_injection_script`, tested).

## Filter-list updater
- It downloads only from the fixed HTTPS URLs in `core::blocker::list_url`, over WinHTTP with
  TLS 1.2 or higher, a 16 MB cap and a 30 s timeout.
- Content is validated before use. A stored download is used only if its hash matches
  `state.json`; otherwise the embedded list is used.
- It respects the configured HTTP proxy. With a SOCKS proxy it does not download at all, to
  avoid revealing the user's IP outside the proxy.
- No telemetry: the only background requests LightBrowser itself makes are these downloads, at
  most once a day.
