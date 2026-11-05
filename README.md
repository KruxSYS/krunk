# Krunk

Krunk is a lightweight, privacy-minded web browser for Windows 10/11 (x64), written in
Rust. It brings its own engine: **Chromium 156.0.8078.80** through the Chromium Embedded
Framework (**CEF 156.0.15**), bundled in the download. Neither Microsoft Edge nor WebView2 is
used. It installs per user without administrator rights, or runs portable from a zip. It includes:
- tabs and private tabs, in as many windows as you like (drag tabs between them)
- history and bookmarks
- a new-tab page with your most-visited sites
- built-in ad and tracker blocking
- an encrypted password vault
- downloads, find, zoom, print and DevTools docked next to the page
- session restore
- background-tab sleep, to save memory

## Download and requirements
- **Installer:** `Krunk-Setup-0.3.0.exe` (about 137 MB), from `https://krunk.209.wf`. It installs for the current user
  only, without administrator rights:
  - to `%LOCALAPPDATA%\Programs\Krunk`, with a Start Menu shortcut and, if you tick it, one on
    the desktop;
  - with an entry in Settings → Apps → Installed apps (per user, `HKCU`).
  If Krunk is running, the installer asks you to close it. `Krunk-Setup-0.3.0.exe /S` installs
  silently; it then closes a running Krunk itself and waits up to 30 s for it.
  **Uninstalling** removes the program and the shortcuts. Your data (`%LOCALAPPDATA%\Krunk`) is
  kept unless you tick "Also delete my Krunk data"; a silent uninstall (`/S`) always keeps it.
- **Portable:** `Krunk-win-x64.zip` (about 184 MB; 423 MB unpacked). Extract it anywhere and
  run `Krunk.exe`. Keep all files of the folder together; if one is missing, Krunk says so at
  startup. The portable copy uses the same data folder as the installed one.
- **Verify:** check `SHA256SUMS` (the installer, the zip and the three main binaries):
  - PowerShell: `Get-FileHash Krunk-Setup-0.3.0.exe`
  - Linux: `sha256sum -c SHA256SUMS`
- **System:** Windows 10 or 11, 64-bit. No runtime is needed.
- **Data location:** everything is stored in `%LOCALAPPDATA%\Krunk`:
  - `browser.db` (history, bookmarks, settings, session)
  - `vault.json` (encrypted passwords)
  - `engine.dat` (the compiled filter engine)
  - `profile\` (the engine's profile: cookies, cache and site data in `profile\default`)
  - `krunk.log` (this session; the previous one is `krunk.old.log`), and `cef.log` (engine
    warnings and errors only)
  - `update.json` (when the last update check ran and what it found)

  Delete that folder to reset the browser completely.
- **Coming from LightBrowser (Krunk's former name):** on its first start Krunk moves
  `%LOCALAPPDATA%\LightBrowser` to `%LOCALAPPDATA%\Krunk` with everything in it, including
  the engine profile (cookies and logins), the vault and `engine.dat`. It renames the folder
  when it can, and otherwise copies it and then deletes the old one. `krunk.log` records what
  happened. If LightBrowser is still running, Krunk asks you to close it first. If a Krunk
  folder already exists, the LightBrowser folder is left alone. Old `lb://` addresses open the
  matching `krunk://` page, and saved tabs and bookmarks of built-in pages keep working.
  The old name remains on purpose in only these places:
  - two vault constants that existing vaults depend on: the key-check plaintext
    `"LightBrowser vault key check v1"` and the DPAPI entropy `"LightBrowser vault key wrap v1"`
    (changing them would make saved passwords unreadable);
  - the migration itself: the `LightBrowser` folder name, the `LightBrowserWindow` class check
    and its messages;
  - the `lb://` and `https://lightbrowser.invalid/` redirect for old saved tabs and bookmarks;
  - internal identifiers no user sees: the crate name `lb_core` and message names (`lb.*`,
    `__lbHost`).
- **Coming from Krunk 0.2.x:** installing 0.3.0 over it keeps all your data. The saved
  session (one window's tabs) opens as one window; from then on Krunk saves every window with its
  tabs, position, size and maximized state. The old session fields are still written (for the
  most recently used window), so going back to 0.2.1 opens that window's tabs. The installer
  removes the `README.md` and `SECURITY.md` that 0.2.x put into the program folder: the package
  ships no Markdown files any more (this README and `docs/SECURITY.md` are in the source and
  on the website).
- **Coming from the WebView2 version:** settings, history, bookmarks, the session, the vault and
  `engine.dat` are used as they are. Cookies and site data are not migrated: the old
  `profile\EBWebView` folder (WebView2's profile) is left untouched and unused, and you can
  delete it. The same goes for the old `bin\` folder (the WebView2 loader).
- **Unsigned binaries:** the files (including the installer) are not code-signed, so
  SmartScreen may warn the first time they run. Updates are protected by their own signature
  instead (see "Updates" below).

## Usage
Type a URL or search words in the address bar. Search uses DuckDuckGo by default; this can be
changed in Settings. Built-in pages:
- `krunk://settings`
- `krunk://history`
- `krunk://bookmarks`
- `krunk://passwords`
- `krunk://downloads`
- `krunk://about` (shows the CEF and Chromium versions of the running engine)
- `krunk://storage`: cookies and site data (also in the main menu and Settings → Privacy)
- `krunk://sitedata`: one site's cookies, local and session storage (shield → Cookies and site data)
- `krunk://adblock`: what the ad blocker blocked on a page, the filter lists, and your own filters

Old `lb://` addresses (from LightBrowser) open the matching `krunk://` page.

| Keys | Action |
|------|--------|
| Ctrl+T / Ctrl+Shift+N | New tab / new private tab |
| Ctrl+N | New window |
| Ctrl+W, Ctrl+Shift+T | Close tab (the window closes with its last tab), reopen closed tab (in the current window) |
| Ctrl+Tab / Ctrl+Shift+Tab, Ctrl+1…8, Ctrl+9 | Next / previous tab, tab by position, last tab |
| Ctrl+L, Alt+D, F6 | Address bar |
| F5 / Ctrl+R, Ctrl+F5 / Ctrl+Shift+R | Reload, reload bypassing the cache |
| Alt+Left / Alt+Right, Alt+Home | Back / forward, home page |
| Ctrl+D | Bookmark this page |
| Ctrl+H, Ctrl+Shift+O, Ctrl+J | History, bookmarks, downloads (switches to the page if it is already open) |
| Ctrl+Shift+B | Show or hide the bookmarks bar |
| Ctrl+F, F3 / Shift+F3 (Ctrl+G / Ctrl+Shift+G) | Find, next / previous match |
| Ctrl+= / Ctrl+-, Ctrl+0 | Zoom in / out, reset |
| Ctrl+P | Print |
| F12, Ctrl+Shift+I | DevTools, docked in the tab's window (press again to close) |
| Ctrl+Shift+C, right-click → Inspect | DevTools with the element under the mouse selected |
| Alt+Space | Window menu (move, size, minimize, maximize, close) |

- **Window:** there is no title bar. Drag the window by empty space in the tab row, and
  double-click there to maximize or restore. Hovering the maximize button shows the Windows 11
  snap layouts. Right-click the tab row for the window menu.
- **New tab** (Ctrl+T, "+", the menu) puts the cursor in the address bar, so you can type at once.
- **Windows and tabs:**
  - **Dragging:** drag a tab sideways to reorder it. Drag it down out of the tab row and it
    becomes a new window that follows the mouse; drop it on another window's tab row (a marker
    shows where it will land) to move it there. Esc while dragging puts it back.
  - **A window's only tab** drags the whole window; dropping it on another window's tab row
    merges it into that window and closes the empty one.
  - **No reload:** a moved tab keeps its page as it is: scroll position, typed text, history,
    playing media, DevTools and private mode.
  - **Right-click a tab** for Reload, Duplicate, **Move to new window** (not for a window's only
    tab), **New window**, Close tab, **Close other tabs** and **Close tabs to the right**.
    **Ctrl+N** and the main menu also open a new window. Each closed tab can be reopened with
    Ctrl+Shift+T.
  - **Closing animation:** a closed tab shrinks away. While the mouse stays on the tab row the
    other tabs keep their width, so the next close button is under the mouse; they spread out
    when the mouse leaves. With "reduce motion" set in Windows there is no animation.
  - **Merging:** a tab dropped on another window's tab row lands in that window, which comes to
    the front.
  - **Closing:** closing a window's last tab closes the window, and closing the last window
    quits Krunk.
  - **Restoring:** every window is restored with its tabs, position, size and maximized state.
    A window that would be off screen (a monitor was removed) is moved onto a monitor.
- **DevTools** (F12) opens inside the tab's window, docked right by default:
  - **Resizing and docking:** drag the splitter between page and DevTools to resize it. The
    control in the toolbar (shown while the tab has DevTools) docks it right or at the bottom, or
    closes it. Side and sizes are remembered.
  - **One per tab:** each tab has its own DevTools. It hides when you switch tabs, comes back
    with the tab, moves with it to another window and closes with it. Page full screen hides it;
    F11 keeps it.
  - **Keys:** while DevTools has the focus, Krunk's shortcuts still work (including F12 and
    Ctrl+Shift+I to close it). The exceptions are the keys DevTools uses itself: Ctrl+F, Ctrl+G,
    Ctrl+P, Ctrl+O, Ctrl+[ / Ctrl+], Ctrl+Shift+F, Ctrl+Shift+P, Ctrl+Shift+C and Esc.
  - **Memory:** a tab with DevTools open is never put to sleep or unloaded.
- **Bookmarks bar:**
  - **Showing it:** right-click the bar to show it always, only on the new-tab page, or never
    (Ctrl+Shift+B toggles; also in Settings → Appearance).
  - **Context menu:** right-click a bookmark to open it (here, in a new tab or a new window),
    edit its name and address, or delete it. Right-click a folder to open all, rename or delete
    it, and empty space to add the page or make a folder.
  - **Dragging:** drag bookmarks to reorder them, or onto a folder to put them in it.
- **Shield:** the shield in the toolbar shows how many ads and trackers were blocked on the
  page, and lists the latest blocked addresses. Click it to allow ads on that site, or to open
  **Cookies and site data** for the site.
- **Ad blocking page** (`krunk://adblock`, the shield's "Show all", Settings → Privacy):
  - **Blocked on this page:** every blocked request of the tab it was opened from, grouped by
    host, with its type, the filter that matched and the list it comes from. The list is kept in
    memory only (at most 500 entries per tab) and starts again when the tab navigates.
  - **Filter lists:** every list with its address, number of filters and last update. Built-in
    lists can be turned off and on, and the default choice restored. Add a list by its https
    address: it is downloaded and checked at once, and not added if that fails. Lists you added
    can be removed. "Update now" downloads them all.
  - **My filters:** your own rules in Adblock Plus / uBlock Origin syntax, with a report of lines
    it cannot read. Ctrl+S saves.
  - Every change takes effect without a restart, once the engine is rebuilt (a few seconds).
- **Downloads:**
  - **History:** `krunk://downloads` shows each download's full address, the page it came from,
    and when it started and finished. Right-click a row (or press Shift+F10 or the Menu key) for Open, Show in folder,
    Copy download link, Copy page address, Retry, Pause / Resume, Cancel, Remove from list and
    Delete file.
  - **Several connections:** large files (8 MB or more) from servers that support it are
    downloaded in parts over several connections at once (Settings → Downloads → Connections per
    download, 4 by default, 1 to turn it off). Krunk first asks the server for the first byte
    only; it splits the download only if the answer names the file's size and a validator
    (ETag or Last-Modified), and every part must come from the same version of the file. The
    parts carry the same cookies and referrer as the browser's own request. Anything else
    (and private tabs) uses the engine's normal download. A file that changes on the server
    during the download fails rather than be stitched together.
- **Cookies and site data:**
  - `krunk://storage` lists every site with cookies or stored data, grouped by site
    (registrable domain), with a search box. Expand a site to see each cookie: name, value
    (shortened, "Show" reveals it), domain, path, expiry, Secure, HttpOnly and SameSite.
  - You can delete one cookie, all of a site's cookies, or all cookies. "Clear site data"
    removes an origin's local storage, IndexedDB, Cache Storage, service workers and cookies.
  - The shield's **Cookies and site data** opens `krunk://sitedata` for the current tab's site.
    There you can view, edit, add and delete its local storage and session storage items, and
    edit and delete its cookies.
  - **Hex editor:** the edit dialog has Text and Hex tabs. Hex shows the value's bytes (offset,
    hex and text columns) in an encoding you choose: UTF-8, a binary string (one byte per
    character, common for binary data in local storage), UTF-16LE, Base64 or percent-encoding.
    Values that look binary open on Hex. Type over bytes, press Insert to insert, select with
    Shift or the mouse, copy and paste as hex, Ctrl+Z to undo. Bytes that do not form a valid
    value in the chosen encoding are not saved. A cookie cannot hold control characters or a
    semicolon; use percent-encoding or Base64 for those. In a private tab this page opens as a private tab and only ever touches
    the private profile.
  - Limits (the engine has no API to enumerate storage):
    - The list shows origins you visited (history), origins open in tabs, and every site that
      has cookies. Data of an origin with none of these is not listed (Clear browsing data
      still removes it).
    - Sizes come from the engine's storage quota (IndexedDB, Cache Storage, service workers and
      similar). Local storage is not counted there, so a site with only local storage shows
      0 B.
    - Session storage is shown only for the tab the site-data page was opened from.
    - IndexedDB is shown as a size, not per database.
- **Passwords:**
  - When you sign in to a site, Krunk asks "Save password?". The first save asks you to
    create the vault with a master password.
  - To fill a login, click the key icon in the address bar.
  - Manage logins on `krunk://passwords`. Showing a password asks for the master password again.
- **Private tabs:** they share an in-memory profile that is discarded when the last private tab
  closes. Nothing from them is written to disk, history, the session or the favicon cache.
- **Zoom** is remembered per site, as in Chrome. "Default zoom" in Settings applies to sites you
  have not zoomed.
- **Memory:** background tabs go to sleep after 5 minutes. Optionally they can be unloaded
  completely after a longer time; turn this on in Settings → Performance.
- **Games (pointer lock and full screen):** `requestPointerLock()` works as in Chrome. The cursor is
  hidden and confined to the page, movement keeps coming at the screen edge (smoothly, also
  while mouse buttons and keys are held), and
  `{unadjustedMovement: true}` gives raw mouse input. Esc, Alt+Tab, switching tabs or leaving
  the page release the lock; a "Press Esc to show your cursor" hint appears for 3 s. A page's
  `requestFullscreen()` and **F11** fill the monitor without the tab strip and toolbar; Esc
  leaves page full screen and F11 toggles.
- **Starting Krunk again** while it runs opens a new tab in the most recently used window.

Command-line options (all optional):

| Option | Effect |
|--------|--------|
| `--update-filters` | Update the filter lists and run a blocking self-test, without opening a window |
| `--mem-report` | Log the memory of the browser's process tree to `memory-report.log` every 10 s |
| `--sleep-test-seconds=N` | Testing only: run the tab sleep/discard timers in seconds instead of minutes |
| `--log-blocked` | Diagnostics: log each blocked or replaced request (without its query string) to `krunk.log` |

Chromium switches typed on the command line are ignored: only the browser itself, through its
settings, configures the engine (so e.g. `--no-sandbox` cannot be passed this way).

## How it works

### Architecture

```
core/    platform-independent library (lb_core): storage, history, bookmarks, frecency,
         vault crypto, ad blocking, cookies, URL cleaning, settings and engine switches,
         zoom, tab model, shortcuts, message protocol, origin checks, layout math.
         Tested on Linux.
app/     Krunk.dll: the browser on top of CEF. All unsafe code is in app/src/ffi
         (ffi/cef wraps the engine; the rest wraps Win32).
ui/      the browser's own UI (HTML/CSS/JS), embedded into the DLL at build time.
         ui/dev holds test tooling only (mock host, Playwright tests) and is not embedded.
tools/   pebrand: build tool that brands the exe (version strings, manifest).
scripts/ build, CEF download/verification and Windows VM test scripts.
```

### Engine and processes
- **`Krunk.exe` is CEF's sandbox bootstrap** (`bootstrap.exe`, renamed, with
  Krunk's version strings and manifest). It sets up the Chromium sandbox, loads
  `Krunk.dll` from its own folder and calls the DLL's `RunWinMain`. The same exe + DLL
  pair runs the browser process and every engine subprocess.
- **Sandbox:** always on. Renderers run at Untrusted integrity with restricted tokens; the GPU
  process at Low. `scripts/vm-sandbox.ps1` prints the integrity level and token of every process.
- **Startup:** the DLL first checks that the runtime files exist (`libcef.dll` is delay-loaded,
  so a missing engine gives a message box instead of a silent failure), lets subprocesses run,
  opens the database and settings, and only then starts the engine.
- **Engine profile:** `profile\` is CEF's root cache path and `profile\default` the persistent
  profile. Private tabs use a separate in-memory request context. CEF's own password manager and
  autofill are switched off on both (profile preferences), because the browser has its own vault.
- **Engine version:** the exact CEF build is pinned by the `cef` crate version in `app/Cargo.toml`
  and the archive hashes in `cef.lock`. The About page reads the versions from the loaded
  `libcef.dll` at runtime.

### Windows and views
- **Windows:** any number of top-level Win32 frames. Every view is a CEF browser (Alloy style)
  hosted as a child window of a frame. A hidden app window takes the timers, cross-thread
  messages and system broadcasts (session lock, power), so they don't depend on a frame.
- **Model:** `core::windows::Workspace` holds one tab model per window, unique tab ids across
  windows, one closed-tab stack and the most-recently-used order of the windows. It turns every
  change into operations for the host (create/close a window, create/destroy/move a view,
  quit). "The current window" is the most recently used one: keys and menu commands act on the
  window they were typed in, `window.open` opens in the opener's window, and a second start of
  Krunk or the update prompt use the current window.
- **Shell:** a view strip at the top of each window showing its tab strip and toolbar. Its
  height is fixed: 76 DIP, scaled by that window's DPI.
- **Tabs:** one browser per *live* tab, laid out below the shell. Restored tabs stay unloaded
  until they are activated. Pages that open windows (`window.open`, `target=_blank`) get a new
  tab next to them; `window.open` keeps its opener.
- **Moving a tab between windows** re-parents its browser's child window (`SetParent`, then
  CEF's move/resize notifications). The browser, its renderer, its request context (so a
  private tab stays private) and its history are untouched, so nothing reloads.
- **Dragging tabs:** the shell reorders tabs within its strip with pointer events. When the
  pointer leaves the strip, the shell hands the drag to the host (`tabDragOut`), because a view
  can't see other windows. The host then polls the cursor and the mouse button on a 15 ms
  timer, moves the dragged window, finds the topmost other Krunk window under the cursor
  (z-order) and, over its tab strip, tells its shell where the tab would land
  (`frame::drop_index` over the tab centres the shell reports in its hit map).
- **Session:** the kv entry `session`, version 2: every window's tabs, active tab, restored
  bounds (physical pixels, `rcNormalPosition`) and maximized state, plus which window was in
  front. It also keeps 0.2.x's `tabs`/`active` fields for the most recently used window, and a
  0.2.x session (no `windows`) restores as one window. On restore,
  `core::layout::fit_to_monitors` moves a window whose title band isn't on any monitor.
- **DevTools:** `ShowDevTools` with `set_as_child(frame)`: a child window of the tab's frame,
  placed by the host beside the page (`core::layout::devtools_split`: a 5 DIP splitter, at least
  240 DIP of page and 280/160 DIP of DevTools). The splitter is the frame's own client area
  between the two (cursor, capture and drag in the window procedure). The DevTools browser has
  a keyboard handler (shortcuts reach Krunk except `keys::devtools_keeps`) and no message
  handler, so it has no privileged channel. Only tab views can be inspected: the shell and
  popup have no context menu and DevTools commands act on the active tab.
- **Popup:** one extra view in an owned `WS_POPUP` tool window, re-owned by the window it opens
  for. It shows the main menu, the tab menu, the shield panel and the save-password and
  fill-login prompts.
  - It is created on first use and then hidden and reused, never more than one.
  - It uses the default profile, never a private one.
  - The shell sends the popup's rectangle in CSS pixels. The host converts it to screen pixels
    with the window's DPI (`GetDpiForWindow` + `ClientToScreen`) and clamps it to the monitor
    work area (`core::layout::popup_screen_rect`).
  - **Hide rules:** the popup hides on deactivate, Escape, a click elsewhere (another view gets
    focus), its window's move, resize or minimize, and tab switch or close. A prompt hidden this
    way counts as dismissed, never confirmed.
- **Keyboard shortcuts** are taken before the page sees them (`CefKeyboardHandler::OnPreKeyEvent`),
  in every view.

### Internal origins
| Origin | Serves | Who can send privileged messages |
|--------|--------|----------------------------------|
| `https://shell.krunk.invalid` | `ui/shell/*` (`shell.html`, `popup.html`) + `/shared/*` | the shell view (only `/shell.html`) and the popup view (only `/popup.html`) |
| `https://krunk.invalid` | `ui/pages/*` + `/shared/*` | a tab showing a known built-in page; each page has its own command allow-list |

- A CEF scheme-handler factory answers every request to these origins from embedded bytes. An
  unknown path gets a 404 from the handler, never a network request.
- `.invalid` never resolves. In addition, the engine's host rules map both hosts to "not found"
  and keep them away from a configured proxy, because Chromium preconnects to a navigation's
  host before the handler answers.
- Every response carries a strict CSP (`default-src 'none'`, no inline code, no network,
  `frame-ancestors 'none'`), `X-Frame-Options: DENY` and `Cross-Origin-Resource-Policy: same-origin`.
- Tabs may load internal files only from a built-in page of the same origin; web pages cannot
  embed, frame or fetch them (`core::assets::internal_request_allowed`). The shell origin is
  not reachable from tabs at all.
- Both origins also serve `/favicon?host=<host>`: PNG favicons captured from pages, cached in
  SQLite (never from private tabs).

### Messages between pages and the browser
- In every main frame, before any page script runs, the renderer installs `window.__lbHost`
  (`postMessage`, `addEventListener('message', …)`): a small shim around a native function.
  Messages travel as JSON in CEF process messages.
- The browser decides who sent a message from the **sending frame's committed URL and
  main-frame flag** (as reported by the engine), never from the message:
  `core::protocol::classify` checks the exact origin, the exact document and the main frame.
  Web pages can only send the small `ContentMessage` type (login form found, login submitted,
  fill requested, page class names for cosmetic filtering).
- Replies and vault fills go to the main frame only; a fill is sent only if the tab still shows
  the origin the login belongs to.

### Ad and tracker blocking
- **Engine:** Brave's `adblock` engine with seven bundled lists: EasyList, EasyPrivacy, and the
  uBlock Origin lists (filters, privacy, unbreak, quick fixes and badware risks; see
  `assets/filters/ATTRIBUTION.md`). The lists are embedded deflated.
  - uBO's sub-lists (`!#include`) are inlined, and its `!#if` blocks are resolved for desktop
    Chromium before compiling (`core::preprocess`). Otherwise Firefox-only or mobile-only rules
    would apply too.
  - Rules from the uBO lists may use uBO's trusted scriptlets (`trusted-*`); EasyList's may not,
    as in uBO.
- **Cache:** at startup the compiled engine is loaded from `%LOCALAPPDATA%\Krunk\engine.dat`, and
  the scriptlet engine (below) from `scriptlets.dat`. The cache key covers the engine version,
  which lists are enabled, and where each comes from (embedded copy or download) with its content
  hash. If anything differs, both are rebuilt on a worker thread.
- **Interception:** every request of every tab, private tabs included (sub-resources, frames,
  workers), passes `CefResourceRequestHandler::OnBeforeResourceLoad` on CEF's IO thread.
  - Blocked requests are cancelled. For `$redirect` rules the request is not sent either, and
    a resource handler answers with uBO's stand-in: a no-op script, an empty JSON or VAST
    response, or a 1x1 GIF. Pages that expect the script to exist then keep working.
  - The shield counts both. The count is kept in atomics and pushed to the UI thread at most
    about four times a second; the log gets one line per page with the count.
  - The tab's own top-level navigation is never checked.
  - The handler uses only in-memory state shared with the UI thread (`Arc`/atomics); the
    allowlist decision is made once per navigation.
- **Scriptlets (`##+js(...)`):** uBlock Origin's scriptlets come from uBO release 1.75.4
  (`assets/filters/ubo-resources.json`, pinned by hash). They patch what a page sees before its
  own scripts run, for example by pruning ad fields from JSON and fetch/XHR responses
  (`json-prune*`, `trusted-replace-fetch-response`), fixing constants (`set-constant`) or
  stopping anti-adblock checks. This is how uBO, and now Krunk, keep YouTube ads and the "ad
  blockers" wall away.
  - The browser compiles the lists' scriptlet filters into a small separate engine and hands
    it to every renderer process. It travels with each browser's `extra_info` and is
    re-sent after list updates.
  - In `CefRenderProcessHandler::OnContextCreated`, which fires for every frame before any page
    script, the renderer runs the scripts matching the frame's URL. They run in the page's
    world, wrapped in a private scope as uBO does.
  - Nothing runs on allowlisted sites or with ad blocking off.
- **Cosmetic filtering:** site-specific hiding rules are injected when a page commits, as a
  constructed stylesheet, which page CSP doesn't block. Generic rules are applied for the class
  names and ids the page reports, capped at 2000.
- **Lists you add and your own filters** (`krunk://adblock`): an added list must be an https
  address, at most 16 MB of text with at least one filter (no header needed); it gets no trusted
  scriptlets. The addresses are in the settings, the downloads in `filters\custom-<hash>.txt`,
  and your own filters (at most 200 KB) in the database. Both are part of the cache key. Each
  change rebuilds the engine on the filter worker; a change made while a rebuild runs waits for
  it.
- **What was blocked:** each tab keeps its last 500 blocked requests in memory (address without
  the query, type, page). The filter that matched is looked up only when `krunk://adblock` asks,
  by a throwaway engine built with the filter text on a worker thread; the engine in use keeps
  none.
- **Updates:** at most once every 24 h (checked hourly), a worker thread downloads the lists and
  their sub-lists over WinHTTP (HTTPS only, 16 MB cap, 30 s timeout) and validates them: no HTML,
  a list header, at least 50 rules, at least 2 KB. A failed or suspicious download, including a
  missing sub-list, keeps the previous source.
  - The updater uses the browser's HTTP proxy setting; with a SOCKS proxy it skips the download
    rather than bypass the proxy.
  - `Krunk.exe --update-filters` runs the update and a self-test without a window: blocking
    decisions, plus the scriptlet engine giving a YouTube watch page its scripts.

### Privacy
- **Third-party cookies:** blocked by default (Settings → Privacy):
  - Network cookies: a `CefCookieAccessFilter` asks `core::cookies::is_third_party` for every
    cookie a request would send or store. A cookie is third-party when the request's registrable
    domain (Public Suffix List) differs from the tab's top-level site. This applies at once.
  - Script access (`document.cookie`): Chromium's `--test-third-party-cookie-phaseout` switch
    (needs a restart). The profile preference alone did not block it in CEF 156, so both are used.
- **Tracking parameters** (`utm_*`, `fbclid`, `gclid`, ...) are stripped before navigating.
- **`DNT: 1` and `Sec-GPC: 1`** headers on every request, plus `navigator.doNotTrack` and
  `navigator.globalPrivacyControl` in every frame.
- **HTTPS-only mode** (optional) with a "Continue to HTTP site" interstitial; exceptions last only
  for the session.
- **WebRTC** uses only the default public interface, and local addresses are replaced with
  random mDNS names, as in Chrome, so pages never see your private IP. To answer for those
  names, Chromium's mDNS responder listens on UDP port 5353 (IPv6) once a page creates a WebRTC
  connection. Windows Firewall therefore asks once per program path whether Krunk may use public
  and private networks. **Cancel is safe:** it only blocks inbound mDNS, and WebRTC and the
  hiding of local addresses keep working. The installer can't pre-answer this, because adding a
  firewall rule needs administrator rights. Updates keep the same path
  (`%LOCALAPPDATA%\Programs\Krunk\Krunk.exe`), so your choice carries over to later versions.
- **No background contact with Google:** see "Engine background traffic" below.

### Engine background traffic
Chromium's own services contact Google in the background. A logging-proxy audit of an idle,
fresh profile found secure-time queries, the component updater (and the downloads it triggers),
an "AI mode" eligibility check and Safe Browsing list updates. Krunk switches all of
them off by default, verified with the proxy: a fresh profile idle for five minutes plus
page loads contacted **no `*.google.com`, `*.googleapis.com` or `*.gvt1.com` host**. The only
traffic not caused by pages is the filter-list update and Krunk's update check (below).

The setting **"Download engine security components from Google"** (off by default, restart
required) lets the component updater run. It then fetches the certificate revocation list
(CRLSet) and the Certificate Transparency list from `update.googleapis.com` and
`edgedl.me.gvt1.com`. The updater installs Chrome's standard set of components, because CEF has
no per-component switch. Everything else stays off.

These services run in the browser process, not in a tab, so the ad blocker cannot see them;
they are controlled with engine switches. The WebView2 build got the same lists from Edge's
updater.

Segmentation-platform data (`profile\segmentation_platform`, which records visited URLs outside
history) is deleted at every start. Its switch crashes this engine build.

### Password vault
```
master password --Argon2id (random 16-byte salt, 64 MiB, t=3, p=1)--> KEK      (never stored)
KEK --XChaCha20-Poly1305--> key-check record + wrapped vault key VK (random 32 bytes)
wrapped VK --DPAPI (CurrentUser)--> stored in vault.json
VK  --XChaCha20-Poly1305 (fresh 24-byte nonce, AAD = entry id)--> each login; manifest of ids
```
- **Storage:** passwords live in `vault.json`, separate from the SQLite database. Origins,
  usernames and notes are encrypted too. Every record is authenticated, so tampering,
  swapping or removing entries is detected.
- **Copying the file is not enough:** someone who copies `vault.json` needs the master
  password *and* the same Windows account on the same PC (DPAPI).
- **Changing the master password** re-keys everything (new salt, KEK and VK) and replaces the
  file atomically.
- **Secrets in memory** are held in `Zeroizing` buffers and wiped when the vault locks. They are
  never logged and never stored in SQLite. Messages carrying a password (login submit, fill)
  also pass through the engine's IPC strings, which Krunk cannot wipe.
- Threat model (assets, attackers, mitigations, out of scope): `docs/SECURITY.md` in the
  source (not part of the download, which ships no Markdown files).
- **Unlocking:** off the UI thread; the host enforces an exponential retry delay (1 s → 60 s).
- **Auto-lock:** after inactivity, on Windows lock and on sleep.
- **Login detection** (main frame only) offers "Save password?" in the browser's popup window.
- **Fill:** a key icon in the address bar fills a login only after the user picks it, and only
  on an exact origin match.
- **Copies:** excluded from clipboard history and cleared after 30 s.

### Chromium switches
Built by `core::settings::Settings::browser_switches` and added in
`CefApp::OnBeforeCommandLineProcessing`. Always:
- `--no-pings` disables hyperlink auditing (`<a ping>`).
- `--disable-domain-reliability` disables Chromium's domain reliability telemetry.
- `--disable-background-networking` stops Safe Browsing updates, variations and other
  background downloads.
- `--host-resolver-rules=MAP krunk.invalid ~NOTFOUND, MAP *.krunk.invalid ~NOTFOUND`
  (see "Internal origins").
- `--disable-features=AutofillServerCommunication,MediaRouter,OptimizationHints,InterestFeedContentSuggestions,NetworkTimeServiceQuerying,AimEnabled,AimServerEligibilityEnabled,AimServerRequestOnStartupEnabled`
  turns off autofill crowdsourcing, Cast discovery, optimization hints, feed suggestions,
  secure-time queries and the "AI mode" eligibility request.

Conditional:
- `--disable-component-update`, and `MAP update.googleapis.com ~NOTFOUND` in the host rules,
  unless "Download engine security components" is on.
- `--test-third-party-cookie-phaseout` when "Block third-party cookies" is on (the default).
  Passed once even if also typed into the extra flags.
- `--disable-gpu` when hardware acceleration is off.
- `--proxy-server=<value>` when a proxy is set, with `--proxy-bypass-list` for the internal hosts
  (and the updater host).
- The user's extra flags, validated. `--disable-features` and `--enable-features` lists are
  merged, and our disabled features stay disabled. Security-weakening switches such as
  `--no-sandbox` and `--disable-web-security` are rejected.

CEF is also started with the sandbox on, `command_line_args_disabled` (the exe's own command line
cannot configure the engine) and log level WARNING. Profile preferences on every request context:
no password manager, no autofill, no network prediction (`net.network_prediction_options = 2`),
and WebRTC on the default public interface only.

Krunk runs no telemetry, update service or background process of its own. The only
network traffic it starts itself is the daily filter-list update and the daily update check
(both WinHTTP, see above and "Updates").

### Updates
- **What is sent:** a minute after start and then once a day, Krunk fetches two small files
  from its update server: `<server>/updates/win-x64/latest.json` and `latest.json.sig`.
  The request carries the user agent `Krunk-Update/<version> (win-x64)` and nothing else: no
  ID, no settings and nothing about your browsing. The server sees your IP address, as with any
  download; the current server is served through Cloudflare, which sees it too. Turn it off with Settings → About → "Check for updates automatically"; "Check for
  updates" there still works on demand. The check uses the configured HTTP proxy and is
  skipped with a SOCKS proxy, as the filter update is.
- **What is trusted:** `latest.json` must carry a valid Ed25519 signature made with Krunk's
  release key; the public key is built into Krunk (`assets/keys/update-signing.pub`). The
  installer it names must be on the update server itself, over HTTPS. After downloading,
  its size and SHA-256 must match the signed manifest before it runs. Older or equal versions
  are never offered, so there are no downgrades. Every rejection is written to `krunk.log`
  with its reason.
- **Installing:** a newer version opens a prompt with **Update now**, **Later** and **Release
  notes**. Update now downloads the installer to `%TEMP%\Krunk-update`, checks it, starts it
  silently (`/S /RELAUNCH`) and closes Krunk. The installer waits for Krunk to exit, installs,
  and starts Krunk again, with your tabs restored. A portable copy gets **Download**, which
  opens the website, instead.
- **The server** is set when Krunk is built (`UPDATE_BASE_URL`). Builds use
  **`https://krunk.209.wf`**, the project's website and update server for now (a temporary
  domain while Krunk is tested). Moving to the final domain takes one release from this server
  whose build points at the new one. Krunk 0.2.0 builds pointed at the placeholder
  `https://krunk.example` and can't update themselves; reinstall them from the website. The
  server itself (Docker, Caddy, behind Cloudflare) is in `dist/server/`; see its README.

### Memory
- **Lazy restore:** restored tabs show their saved title and the cached favicon. They get no
  browser (and so no renderer process) until they are first activated.
- **Background sleep:** after `tab_sleep_minutes` (default 5) without activation, a background
  tab's JavaScript heap is garbage-collected (DevTools protocol `HeapProfiler.collectGarbage`).
  The tab is already hidden, so Chromium throttles its timers. A second stage 60 s later
  repeats the collection. Activating the tab shows it again, with no reload. Pages are not
  frozen; see "Known limitations".
- **Discard** (optional, off by default): after `tab_discard_minutes` the tab's browser is
  closed. URL, title and favicon are kept, and the tab reloads when activated. A discarded
  private tab keeps only its URL. The last live private tab is never discarded, so the private
  session (cookies, logins) survives.
- **Never touched:** the active tab of every visible window, tabs playing audio, tabs with a
  download in progress, tabs with DevTools open, and the UI shell and popup. A small tracker in each frame reports playing unmuted media and
  running Web Audio to the browser.
- The rules are pure functions in `core::memory` (`decide`, `exempt`), with unit tests. The
  host evaluates them on a timer.
- Hidden options, not shown in the UI:
  - `--sleep-test-seconds=N` scales the timers for testing: sleep after N s, the second stage
    N s later, discard after 4N s when discard is enabled.
  - `--mem-report` appends the summed working set and private bytes of the browser process
    tree to `%LOCALAPPDATA%\Krunk\memory-report.log` every 10 s. It includes tab and
    sleep counts, and renderer counts taken from each process's `--type=` switch. The tree is our
    process plus every descendant (the engine's subprocesses).

Measured on the Windows 11 VM (CEF 156.0.15 / Chromium 156.0.8078.80) as the sum over the whole
process tree. "10 sites" were Wikipedia, GitHub, Hacker News, MDN, BBC News, The Guardian,
Stack Overflow, Reddit, NYTimes and CNN, plus the new-tab page:

| Scenario | Processes | Renderers | Working set | Private bytes |
|----------|-----------|-----------|-------------|---------------|
| 1 tab (new-tab page) | 7 | 3 | 354 MB | 140 MB |
| 11-tab session restored (lazy) | 7 | 3 | 354 MB | 135 MB |
| 10 sites open, awake | 22 | 18 | 2955–3044 MB | 1731–1802 MB |
| 10 background tabs asleep | 22 | 18 | 2629–2633 MB | 1369–1372 MB |
| 10 background tabs after the second stage | 22 | 18 | 2630–2639 MB | 1368–1372 MB |
| after discard (10 unloaded, CNN active) | 10 | 6 | 722–745 MB | 422–425 MB |

The WebView2 build measured 379 MB idle, 2131–2173 MB / 1165–1291 MB with the 10 sites awake,
1084–1179 MB after its low-memory and suspend stages, and 973–988 MB / 660 MB after discard.

How to read these numbers:
- The working-set sum counts shared DLL pages once per process, so it overstates physical RAM.
- With many sites awake the CEF build uses about 40 % more: Chromium isolates more sites,
  including cross-site ad and video frames, into their own renderer processes (18 against 15).
- Sleep saves about 11 % of the working set and about 21 % of private bytes. WebView2's
  "low memory" target trimmed more (about 45 % of the working set); CEF has no equivalent.
- Discard frees most memory and ends the renderer processes.

To reproduce: enable discard in Settings, start with `--sleep-test-seconds=90 --mem-report`,
open the sites and read `memory-report.log` (steps in `docs/MANUAL_TEST.md`).

## Building from source
The release is cross-compiled on Linux for `x86_64-pc-windows-msvc` with `cargo-xwin`. CEF's
Rust bindings (`cef` crate) build CEF's C++ wrapper library and support only the MSVC target:
`x86_64-pc-windows-gnu` fails with "Unsupported target triplet". One command builds and
packages everything:

```sh
scripts/build-dist.sh            # -> dist/Krunk-win-x64/, the zip, Krunk-Setup-<version>.exe, SHA256SUMS
scripts/build-dist.sh --debug    # same layout with a debug build (testing only)
scripts/build-dist.sh --dev      # dev-updates build (update check accepts http://) -> target/dev-dist/
UPDATE_BASE_URL=https://downloads.example.org scripts/build-dist.sh   # another update server
```
`UPDATE_BASE_URL` must be `https://` (the build fails otherwise, except with `--dev`).
`KRUNK_UPDATE_PUBKEY_FILE` replaces the trusted public key (test keys for dev builds only).

What it does:
1. `scripts/fetch-cef.sh` downloads the CEF archive pinned in `cef.lock` (the "minimal"
   Windows x64 distribution, about 175 MB) into its own cache directory
   (`~/.cache/krunk-cef/downloads`). It checks the SHA-1 published in CEF's build index
   (`cef-builds.spotifycdn.com/index.json`) **and** the SHA-256 pinned in `cef.lock`, and only
   then extracts it with `export-cef-dir`. Nothing from CEF is committed to the repository.
2. `cargo xwin build --release --target x86_64-pc-windows-msvc -p app --lib` builds
   `Krunk.dll` with the static CRT (`.cargo/config.toml`) and `libcef.dll` delay-loaded.
3. It assembles `dist/Krunk-win-x64/`: `Krunk.exe` (the CEF bootstrap, branded by
   `tools/pebrand`), `Krunk.dll`, the CEF DLLs, `.pak` files, `icudtl.dat`,
   `v8_context_snapshot.bin`, all `locales/`, `licenses/` and `THIRD-PARTY-NOTICES.txt`. No
   Markdown: the build fails if any `*.md` file ends up in the folder (README and SECURITY are
   in the source and on the website). Then it writes the zip (fixed timestamps and order) and
   `SHA256SUMS`.
4. `scripts/fetch-nsis.sh` fetches NSIS 3.09 (Ubuntu's `nsis` and `nsis-common` packages,
   SHA-256 pinned in `nsis.lock`, unpacked with `dpkg-deb` into `~/.cache/krunk-toolchain`),
   and `makensis` builds `Krunk-Setup-<version>.exe` from `installer/krunk.nsi`. The installer
   deletes `*.md` from the program folder before copying (0.2.x shipped README.md and
   SECURITY.md); it never touches the data folder.

Requirements (versions used for the release):
- Rust stable (1.87 or newer; built with 1.99.1) with `rustup target add x86_64-pc-windows-msvc`
- `cargo install cargo-xwin --version 0.23.1`
- `cargo install export-cef-dir --version 156.3.0` (must match the `cef` crate version)
- LLVM **20.1.8** (`clang-cl`, `lld-link`, `llvm-lib`; the MSVC C++ headers need clang 19 or
  newer), unpacked from the official release tarball. The script expects it in
  `~/.cache/krunk-toolchain/llvm-20.1.8` or in `$LB_LLVM`.
- `cmake`, `ninja`, `curl`, `python3`, `zip`, `dpkg-deb` (for NSIS)

**Microsoft's licence:** cargo-xwin downloads the MSVC CRT and the Windows SDK (headers and
import libraries) from Microsoft. Building therefore means accepting Microsoft's licence terms
for them; `build-dist.sh` sets `XWIN_ACCEPT_LICENSE=1` for this (and only there). The versions
are pinned so that builds are reproducible: `XWIN_VERSION=17`, CRT `14.44.17.14` (MSVC 14.44.35220),
SDK `10.0.26100`; the download is cached in `~/.cache/krunk-xwin`.

A clean build from a fresh clone took 1 min 27 s on the build machine (CEF's C++ wrapper and
LTO), plus the one-time downloads of CEF (175 MB) and the MSVC CRT/SDK; it has no warnings. The
release profile uses LTO, `opt-level = "s"`, strip and `panic = "abort"`. Two clean builds
from the same checkout path give the same `Krunk.dll` except for a few bytes (12 to 20
measured): the PE time stamp and the debug-directory record (GUID). `Krunk.exe` and the CEF
files are always identical. The zip is byte-identical for the same `Krunk.dll` and
`SOURCE_DATE_EPOCH` (default: the HEAD commit time, which stamps every file). The installer is
not byte-reproducible: NSIS's compressed data differs between runs, while the files it
installs are identical.

**Alternative: native build on Windows** (documented, not used for the release). With Visual
Studio 2022 Build Tools ("Desktop development with C++", MSVC 14.44, Windows SDK 10.0.26100),
Rust, CMake and Ninja: set `CEF_PATH` to the folder produced by `scripts/fetch-cef.sh` (or
run `export-cef-dir` on the verified archive) and run
`cargo build --release --target x86_64-pc-windows-msvc -p app --lib`. Then assemble the folder
as `build-dist.sh` does.

### Updating the engine
Security fixes for the engine reach users only through new Krunk releases (see "Known
limitations"). To move to a newer CEF:
1. Set the new `cef` crate version (e.g. `=157.x.y`) in `app/Cargo.toml` and run
   `cargo update -p cef`. Install the matching `export-cef-dir` version.
2. In `cef.lock`, set `CEF_CRATE`, `CEF_VERSION`, `CHROMIUM_VERSION`, `CEF_ARCHIVE` and
   `CEF_SHA1` from `https://cef-builds.spotifycdn.com/index.json` (windows64, "minimal"), and
   run `scripts/fetch-cef.sh --pin` once to record the archive's SHA-256.
3. `scripts/build-dist.sh`, then run the checks below and `docs/MANUAL_TEST.md`.

### Checks
```sh
cargo test --workspace                                                   # core: 323 unit tests
cargo clippy --workspace --all-targets -- -D warnings                    # Linux
cargo xwin clippy --target x86_64-pc-windows-msvc --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo audit
NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules node ui/dev/test-ui.cjs [shots-dir]   # UI pages with a mock host
```
For `cargo xwin clippy`, export the same `CEF_PATH` and `XWIN_*` variables as `build-dist.sh`.
The app's own unit tests (DPAPI, message escaping) run on Windows:
`cargo xwin test --target x86_64-pc-windows-msvc -p app --no-run`, then run the test exe.

Testing on Windows:
- `scripts/vm.sh` deploys `dist/Krunk-win-x64` to a Windows test machine over SSH and
  drives it (start, stop, screenshots, keys, clicks).
- `scripts/vm-sandbox.ps1` shows the integrity level and token of every Krunk process.
- `scripts/vm-engine-check.ps1` shows which engine DLLs each process loaded and whether any
  `msedgewebview2.exe` was started by Krunk.
- `scripts/vm-mem.ps1` lists the process tree with memory figures.
- `scripts/vm-leak-scan.ps1` searches the data folder for strings that must never be stored,
  such as a test password.
- The Windows checklist is `docs/MANUAL_TEST.md`.

## Known limitations
- **The engine is bundled.** Chromium security fixes reach users only with a new Krunk
  release with a newer CEF. Krunk checks for one daily and offers it, but installs only when you
  choose Update now. Releases should follow CEF security releases promptly.
- **The update server is on a temporary domain** (`https://krunk.209.wf`). When the final
  domain exists, one release from it must point Krunk at the new domain
  (`UPDATE_BASE_URL=https://<domain> scripts/build-dist.sh`). 0.2.0 copies (placeholder server)
  must be reinstalled by hand.
- **No certificate revocation or CT list updates by default.** With "Download engine security
  components" off, the CRLSet and Certificate Transparency list are those built into the
  release. Chromium relaxes CT enforcement about 10 weeks after its build date. Turning the
  setting on restores the updates, at the cost of contact with Google's update servers.
- **No Safe Browsing / SmartScreen.** CEF has no working Safe Browsing, and the WebView2
  build's optional SmartScreen setting is gone. Ad and tracker blocking and third-party cookie
  blocking replace Edge's tracking prevention.
- **No DRM (Widevine) playback.** Netflix, Spotify Web and similar sites that need DRM do not
  play.
- **No page freezing in the background.** `Page.setWebLifecycleState frozen` is not used: in
  CEF 156 a windowed (Alloy-style child window) browser never repaints after it is unfrozen.
  Background tabs rely on Chromium's hidden-page throttling plus garbage collection; discard
  frees the memory.
- **Third-party cookie blocking for scripts uses a Chromium test switch**
  (`--test-third-party-cookie-phaseout`). A future Chromium could rename or drop it. There are no
  per-site exceptions; if an embedded sign-in or widget breaks, turn the setting off (and
  restart). Network cookies are filtered by the browser itself and apply at once.
- **Some settings need a restart:** hardware acceleration, third-party cookie blocking (script
  access), engine security components, proxy and extra flags. The settings page says when.
- **Zoom is per site**, as in Chrome, not per tab as in the WebView2 build.
- **WebSocket connections are not filtered** by the ad blocker: CEF does not pass them to the
  request handler. Theme changes reach pages through DevTools emulation, so a page that has
  DevTools open shows the system theme.
- **HTTP authentication and site permission prompts** (camera, microphone, location,
  notifications) are not shown: CEF's Alloy runtime denies them by default.
- **Size:** about 423 MB unpacked (184 MB zipped), against about 6 MB for the WebView2 build. All
  CEF locales and SwiftShader (software rendering for machines without a usable GPU) are kept.
- **Discarding loses back/forward history.** A discarded tab reloads its last URL; its history
  and any unsaved form input are lost. Discard is off by default.
  - Cosmetic filtering of generic rules is limited to the first 2000 class names and ids a
    page reports.
- **uBO's procedural cosmetic filters** (`:has-text()`, `:upward()`, `:remove()` and similar) are not
  executed: only plain CSS selectors hide elements. Scriptlets written for uBO's isolated world
  run in the page's world, because CEF provides no isolated world for them.
- **YouTube** keeps changing its ad delivery. Krunk carries uBO's YouTube defences (scriptlets
  and Quick Fixes, updated daily) and was verified on 2026-11-04 (no ads, no anti-adblock wall
  on three videos). A YouTube change can still get through until the lists catch up.
- **Pointer lock is provided by Krunk, not the engine.** CEF's Alloy runtime style never
  implements `WebContentsDelegate::RequestPointerLock`; Chromium's default refuses every request
  (`kUnknownError`). So a script in every frame implements the API, and the browser confines and
  hides the cursor and reads raw mouse input (`WM_INPUT`). The page keeps getting the real
  (trusted) mouse events. Their `movementX/Y` and positions come from patched `MouseEvent`
  getters, and while locked a style sheet sends them to the locked element, as Chrome does.
  - Code that inspects those getters can tell the difference.
  - Movement reaches the page about 2–4 ms later than in Chrome.
  - Movement that no real event carries (sub-pixel motion) arrives as a synthetic event
    (`isTrusted` false).
  - The long-term fix is CEF's Chrome runtime style for tabs, which has native pointer lock.
- **The vault does not detect rollback.** Replacing `vault.json` with an older copy is not
  detected. See `docs/SECURITY.md` for the full threat model and what is out of scope.
- **No tab pinning in the UI.** The memory policy supports pinned tabs, but the tab strip
  cannot pin yet.
- **Platform support:** no Windows on ARM build and no extensions.
- **Tab dragging is Krunk's own, not Chrome's.** Under the Alloy runtime the engine has no tab
  strip, so the drag is done by the host: the torn-off window itself is the drag image (there
  is no translucent preview), and a tab can't be dropped into another program.
- **Downloads in parts go around the engine's network stack.** They use WinHTTP with the
  engine's cookies, user agent and referrer, the HTTP proxy setting, and WinHTTP's certificate
  checks. Sites that need HTTP authentication or a client certificate, SOCKS proxies and private
  tabs always get the engine's own single-connection download. A part download paused and
  resumed continues where each part stopped; one interrupted by quitting starts again (its part
  files are removed at the next start).
- **Filters with a port** (`||example.com:8080/…`) never match: Brave's `adblock` engine ignores
  the port. Write `||example.com^*/path` instead.
- **What was blocked** lists requests, not elements hidden by cosmetic filters. When the
  filter text can't be recovered (a list changed since the request), only the list is named.
- **Several monitors:** windows remember their position on any monitor and are moved back on
  screen if that monitor is gone. Dragging across monitors with different scaling was not
  tested (the test machine has one monitor, at up to 125 %).
- **Not tested for 0.3.0 and 0.4.0** (the Windows 11 test VM can't do them):
  - **two monitors:** the VM has one;
  - **display scaling above 125 %:** the VM allows at most 125 %;
  - **a standard (non-admin) user with UAC on:** the VM has UAC off and an admin user.

  The in-app update from 0.2.1 to 0.3.0 was tested against the live server after publishing
  (2026-11-05): 0.2.1 offered 0.3.0, Update now installed it, and Krunk restarted as 0.3.0 with
  the session in one window.
- **DevTools can't be undocked** into its own window; it docks right or at the bottom of its
  tab's window. Theme changes don't reach a page while its DevTools is open (see above).
- **Unsigned:** the binaries are not code-signed (SmartScreen may warn on first start).

## Third-party components
- **Chromium Embedded Framework 156.0.15** and **Chromium 156.0.8078.80**: CEF is BSD-licensed
  (`licenses/CEF-LICENSE.txt` in the download). Chromium and the third-party code it contains are
  listed with their licences in `licenses/Chromium-CREDITS.html`. The binaries are redistributed
  unmodified, except that `Krunk.exe` (CEF's `bootstrap.exe`) carries Krunk's
  version strings and manifest.
- Filter lists:
  - EasyList and EasyPrivacy: GPL-3.0-or-later or CC BY-SA 3.0.
  - uBlock Origin uAssets: GPL-3.0.
  - Attribution and licences are in `assets/filters/` and `THIRD-PARTY-NOTICES.txt`. The lists
    are used unmodified, as data.
- Rust crates: see `Cargo.lock`. The main ones are `cef` / `cef-dll-sys` (Apache-2.0 OR MIT),
  `windows` (MIT/Apache-2.0), `adblock` (MPL-2.0), `psl` (MIT/Apache-2.0), `rusqlite` with
  bundled SQLite (MIT, public domain), and RustCrypto `argon2`/`chacha20poly1305`
  (MIT/Apache-2.0). The build tool `tools/pebrand` uses `editpe` (MIT).
