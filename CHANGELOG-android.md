# Krunk for Android changelog

What changed in each Krunk for Android release, newest first. The Windows browser's list is
`CHANGELOG.md`.

## 0.1.1 — 2026-11-11

A fix release after a full review of the 0.1.0 code, with one changed default. It installs over
0.1.0 and keeps tabs, bookmarks, history and settings.

- **The toolbar is at the top by default.** A new install has the address bar at the top. An
  install that has already saved a setting keeps the position it stored; one that never changed
  any setting moves to the top with this update. Settings › Appearance › Toolbar position
  switches between Top and Bottom.
- **Address bar:** it shows the site you are on (the host, with its end always visible) instead
  of the start of a long address. On phones "not secure" is the red sign alone and the Home
  button appears from tablet width, so the site's name fits. The lock is shown only for the page
  that is really on screen, never for an address that is still loading. The toolbar comes back
  whenever a page loads, so a hidden address bar cannot outlive a navigation or get stuck on a
  page that does not scroll. Bottom-fixed parts of a page (cookie bars, site navigation) are no
  longer covered or left floating while the toolbar hides.
- **Pages can ask:** drop-down lists (`<select>`), alert, confirm and prompt dialogs and HTTP
  sign-in now work (0.1.0 showed none of them). Each dialog names the site that asks, and from
  the second dialog of a page on you can block the rest. Links that open a new window
  (`target=_blank`, `window.open`) open a new tab.
- **Security:**
  - Addresses handed over by other apps and links opened from a page's link menu are limited to
    web addresses and are loaded like any link on a web page.
  - A new window shows no address until it has really loaded something, and a web page cannot
    put a `krunk://` address into the address bar.
  - Krunk asks before a page-started download (file name and site), and file names are cleaned
    of characters that hide or reorder them.
  - Full screen says which site entered it and how to leave.
  - Site icons are fetched with size and time limits and checked before they are decoded.
- **Tabs and restore:** a restore ends on the page you left, with its back and forward list and
  scroll position, also when you left right after a navigation or the app was closed seconds
  after starting. A page whose process crashed, or was closed by Android to free memory, shows
  "This page stopped working" with Reload instead of a dead blank view. "Restore tabs?" keeps
  the session until you answer, also across a restart; after two failed starts in a row Krunk
  asks before restoring. Tabs survive Android closing Krunk in the background whatever the
  start-up setting. "Close all" no longer loads background tabs on the way.
- **Privacy:** private tabs stay out of the Recents thumbnail and screenshots, and one private
  tab is kept alive under memory pressure so private sign-ins are not lost. "Clear browsing
  history" also clears site icons and the recently closed list. Nothing of Krunk goes into
  Android's cloud backup any more (the direct device-to-device transfer still carries your
  data). The release build no longer writes page addresses into the system log. Krunk's own
  requests (site icons, search suggestions) use the browser's user agent, not the device's.
- **Settings:** a custom search engine and a custom new-tab address can be turned on; the
  user-agent setting is applied; "Strip tracking parameters" off survives a restart, and
  tracking parameters are also removed from links other apps hand over and from the link menu.
- **History:** visits are stored under their own page's title, and addresses that only
  redirect are not listed.
- **Smaller fixes:** a database that cannot be opened no longer stops Krunk from starting; find
  in page belongs to its tab and shows its match count also when typed fast; the address editor no longer shows a suggestion from the last
  time; mail and phone links can be copied from the link menu; a cancelled history page load no
  longer ends the list; settings dialogs scroll.
- Known limits, unchanged: downloads are handed to Android without the page's sign-in; file,
  date and colour pickers and the "leave page?" question are not there yet; tracking parameters
  are not removed from links clicked inside a page.
- Internals: page state, load policy, session start-up and prompts are pure, unit-tested code
  (75 JVM and 39 Rust tests for the Android side), session rows are written in one transaction,
  the tab list is no longer rebuilt on every engine callback, the release build refuses a debug
  signature, and the publish script checks version and package of every APK.

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
