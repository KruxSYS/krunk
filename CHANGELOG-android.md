# Krunk for Android changelog

What changed in each Krunk for Android release, newest first. The Windows browser's list is
`CHANGELOG.md`.

## 0.1.0 — 2026-11-11

The first Krunk for Android preview (`android/`, `ffi/`), sharing the Rust core with the Windows
browser (core 0.15.3). It is a separate app with its own version; see the README section "Krunk
for Android (preview)" and `docs/ANDROID_FEATURES.md`. Android 8 or later, `arm64-v8a` phones
and tablets; the APK is signed with Krunk's Android release key and installs from the website
("install unknown apps" must be allowed for the browser you download it with).

- **Engine:** GeckoView 160, pinned; `arm64-v8a` devices and the `x86_64` emulator; Android 8+.
- **Browsing:** address bar with Krunk's URL-or-search rules and DuckDuckGo by default, history,
  bookmark and engine suggestions, automatic HTTPS, HTTPS-only mode, tracking parameters removed,
  back/forward/reload/stop/home, progress, title and favicon, lock / "Not secure", Krunk's error
  page, pull-to-refresh, find in page, request desktop site, pinch zoom and text size, full
  screen, share, link long-press menu (new tab, private tab, copy, share, save image, open image).
- **Tabs:** switcher grid with thumbnails, swipe to close, close all, search; private tabs in
  their own section with their own theme and no history; recently closed; tabs survive restarts
  and process death, with a "Restore tabs" prompt after a crash; background tabs sleep and can be
  discarded (the Windows policy).
- **New tab page:** most visited sites, shortcuts (pin, edit, remove), recently closed, the ten
  built-in wallpapers or a colour, light and dark.
- **Bookmarks and history:** manager with folders and search, Netscape HTML import and export,
  history by day with search, delete and clear.
- **Settings:** search engine and custom engine, homepage, start-up, new-tab page, theme, toolbar
  at the bottom or the top, text size, force zoom, suggestions, HTTPS-only, tab sleep/discard,
  new-tab background, user agent, clear browsing data; About with versions and licences.
- **Shell:** bottom toolbar that hides while scrolling, light/dark/system, edge-to-edge, gesture
  and three-button navigation, predictive back, landscape and tablet layouts, 48 dp targets and
  content descriptions for TalkBack.
- Not in this preview: blocking, vault, downloads manager, permissions, media controls, sync,
  VPN, agent, extensions, updates, release signing (the matrix lists each with its round).
