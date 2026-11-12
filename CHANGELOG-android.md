# Krunk for Android changelog

What changed in each Krunk for Android release, newest first. The Windows browser's list is
`CHANGELOG.md`.

## 0.2.0 — 2026-11-12

Ad and tracker blocking, a quieter engine, a wider address bar and themes. It installs over 0.1.0
and 0.1.1 and keeps tabs, bookmarks, history and settings.

- **Fixed: 0.1.0 and 0.1.1 contacted Mozilla and Google in the background.** Without anyone
  asking, the engine fetched Mozilla's Remote Settings (certificate data, lists) and ran a
  connectivity check, and it asked Google's Safe Browsing service for list updates although Safe
  Browsing was set to "off". Krunk promises no background traffic, so this was a bug. 0.2.0
  switches all three off and checks it with a packet capture: after a fresh install Krunk sends
  nothing until you open a page.
  - What that costs: the engine no longer receives updates of certificate revocation data (CRLite)
    or of preloaded intermediate certificates, and there is no Safe Browsing warning page. A
    certificate revoked after this version of Krunk was built is not known to it.
  - Settings › Privacy › "Update automatically" (off by default) turns the daily filter-list
    updates and Mozilla's Remote Settings on together, from the next start. Safe Browsing and the
    connectivity check stay off in every case.
- **Ad and tracker blocking.** uBlock Origin 1.76.0 is built in, unmodified, as an engine
  extension, with the filter lists that come inside it (EasyList, EasyPrivacy, uBlock Origin's own
  lists, Peter Lowe's list, the malicious-URL list). It blocks requests and hides the leftovers on
  the page, in normal and private tabs.
  - The **shield** in the address bar shows how many requests were blocked on the page. Tap it to
    switch blocking off for that site; the choice is kept. In a private tab the sheet shows the
    count and no switch, so a private visit leaves no entry.
  - An address that is itself on a list shows Krunk's page with "Go back" and "Open once".
  - Filter lists are not updated behind your back: Settings › Privacy › "Update filter lists"
    fetches them when you ask (from the lists' own hosts), or "Update automatically" does it daily.
  - When Krunk starts, pages wait a few seconds until the blocker is ready, so the first page is
    filtered too. If it is not ready after 10 seconds the page loads and the shield says so.
  - Blocking can be switched off as a whole in Settings › Privacy.
- **Tracking parameters** (`utm_…`, `fbclid`, `gclid` and the rest of Krunk's list) are now also
  removed from links you tap inside a page and from redirects, not only from addresses you type.
  Forms that are sent (POST) are never changed.
- **Toolbar.** Back and Forward moved into the ⋮ menu, as its first row together with Home; a
  long press on either lists the pages to go to. The address bar takes the room: about two
  thirds of the toolbar on a small phone. Android's back gesture still goes back in the page's
  history. Reload and stop have a full-size touch target.
- **Themes.** Settings › Appearance › Style offers the twelve styles of the Windows browser, each
  with a small preview, and Accent colour seven colours or any other. A style sets the toolbar's
  fill (plain, gradient, tinted), the corners of the address bar and the tab cards, and how the
  current tab is marked in the switcher. Light, dark and "follow the system" work as before, and
  private tabs keep their own dark look in every style. Text and icons are checked for contrast in
  every style. Without a style nothing changes, except that the current tab's card now has a
  filled title instead of a border.
- **Pages can ask for more:** files (Android's file picker, one or several, by type; a field that
  asks for a photo can use the camera app), dates, times, weeks and months, colours, "leave this
  page?", "send the form again?" and sharing through Android's share sheet. Krunk's own dialogs
  name the site that asks; closing any of them answers the page, so none hangs.
- **Links:** mail and phone links open Android's mail and phone apps (after a tap at once,
  otherwise Krunk asks first). Selected text can be searched, and a selected address opened, from
  the menu over the selection. Settings › Tabs › "Tabs opened by links" decides whether such tabs
  come to the front.
- **Tabs:** a long press on a tab in the switcher offers Duplicate, Close other tabs and Close
  tab. "Request desktop site" now stays with the tab when Android or a crash made Krunk rebuild
  the page, and over a restart.
- **About** lists uBlock Origin and its filter lists with their licences.

Known limits: custom filter lists and protection levels come later; a file you give to a page is
copied inside Krunk's storage for the upload (removed when the tab closes); whole folders cannot
be uploaded; moving tabs in the switcher is not there yet. Each APK is about 7 MB larger.

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
