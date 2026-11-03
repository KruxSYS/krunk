# LightBrowser

A lightweight Windows 10/11 (x64) browser: a Rust host around the Microsoft Edge WebView2
(Evergreen, Chromium) runtime. *Work in progress; full build and usage instructions arrive with
the packaging milestone.*

## Architecture

```
core/   platform-independent library (lb_core): storage, history, bookmarks, frecency,
        vault crypto, ad blocking, URL cleaning, settings, tab model, shortcuts,
        message protocol, origin checks, layout math. Tested on Linux.
app/    Windows host (LightBrowser.exe). All unsafe code is in app/src/ffi.
ui/     the browser's own UI (HTML/CSS/JS), embedded into the exe at build time.
        ui/dev holds test tooling only (mock host, Playwright tests) and is not embedded.
```

### Windows and WebViews
- **Main window:** one top-level window with one shared `CoreWebView2Environment`.
- **Shell:** a WebView strip at the top showing the tab strip and toolbar. Its height is fixed:
  76 DIP, scaled by DPI.
- **Tabs:** one content controller per *live* tab, laid out below the shell. Restored tabs stay
  unloaded until they are activated. Private tabs use a separate InPrivate profile.
- **Popup:** one extra WebView in an owned `WS_POPUP` tool window. It shows the main menu, the
  shield panel and the save-password and fill-login prompts.
  - It is created on first use and then hidden and reused, never more than one.
  - It uses the shared environment and the default (shell) profile, never a private one.
  - The shell sends the popup's rectangle in CSS pixels. The host converts it to screen pixels
    with the window's DPI (`GetDpiForWindow` + `ClientToScreen`) and clamps it to the monitor
    work area (`core::layout::popup_screen_rect`).
  - Every popup kind has buttons, so the popup is always activated when shown. That keeps it
    keyboard accessible and gives a reliable "click elsewhere" signal (`WM_ACTIVATE`). The window
    is created `WS_EX_NOACTIVATE`, and that flag is cleared by `Window::set_activatable` only
    for kinds that take focus.
  - **Hide rules:** the popup hides on deactivate, Escape, a click elsewhere (another
    controller gets focus), main-window move, resize or minimize, and tab switch or close. A
    prompt hidden this way counts as dismissed, never confirmed.
  - This replaces a transparent full-window overlay: Win32 does not composite sibling child
    windows with transparency.

### Internal origins
| Origin | Serves | Who can send privileged messages |
|--------|--------|----------------------------------|
| `https://shell.lightbrowser.invalid` | `ui/shell/*` (`shell.html`, `popup.html`) + `/shared/*` | the shell WebView (only `/shell.html`) and the popup WebView (only `/popup.html`) |
| `https://lightbrowser.invalid` | `ui/pages/*` + `/shared/*` | a tab showing a known built-in page; each page has its own command allow-list |

- Requests to these origins are answered from embedded bytes through `WebResourceRequested`.
  The `.invalid` TLD never resolves, so they can't leak to the network.
- Every response carries a strict CSP (`default-src 'none'`, no inline code, no network,
  `frame-ancestors 'none'`).
- Both origins also serve `/favicon?host=<host>`: PNG favicons captured from pages, cached in
  SQLite (never from private tabs). The new-tab page and bookmarks use them, so they show
  favicons without any network request.
- Message origins are checked by `core::protocol::classify`: exact origin, exact document,
  top-level frame only. Web pages can only send the small `ContentMessage` type.

### Ad and tracker blocking
- **Engine:** Brave's `adblock` engine with five bundled lists (EasyList, EasyPrivacy, and
  uBlock filters, privacy and unbreak; see `assets/filters/ATTRIBUTION.md`). The lists are
  embedded deflated (1.4 MB).
- **Cache:** at startup the compiled engine is loaded from `%LOCALAPPDATA%\LightBrowser\engine.dat`
  (about 1 ms on Linux, about 7 ms measured under Wine). The cache key covers the engine
  version, which lists are enabled, and where each comes from (embedded copy or download) with
  its content hash. If anything differs, the engine is rebuilt on a worker thread (about 70 ms).
- **Interception:** every content WebView, private tabs included, intercepts all requests
  (`AddWebResourceRequestedFilterWithRequestSourceKinds`: all contexts, documents and workers).
  - Blocked requests get an empty 403.
  - The tab's own top-level navigation and the internal origins are never checked.
  - The handler uses only in-memory state. The allowlist is a `HashSet`, and the per-site
    decision is made once per navigation.
- **Cosmetic filtering:** site-specific hiding rules are injected on `DOMContentLoaded` as a
  constructed stylesheet, which page CSP doesn't block. Generic rules are applied for the
  class names and ids the page reports, capped at 2000.
- **Updates:** at most once every 24 h, a worker thread downloads the lists over WinHTTP
  (HTTPS only, 16 MB cap, 30 s timeout). It validates each download: no HTML, a list header,
  at least 50 rules, at least 2 KB. A failed or suspicious download keeps the previous source.
  - The updater uses the browser's HTTP proxy setting.
  - With a SOCKS proxy it skips the download rather than bypass the proxy.
  - `LightBrowser.exe --update-filters` runs the update and a self-test without a window.
- **Privacy:**
  - Tracking prevention: Strict by default.
  - Tracking parameters (`utm_*`, `fbclid`, `gclid`, ...) are stripped before navigating.
  - `DNT: 1` and `Sec-GPC: 1` headers, plus `navigator.doNotTrack` and
    `navigator.globalPrivacyControl`.
  - Optional HTTPS-only mode with a "Continue to HTTP site" interstitial; exceptions last only
    for the session.

### Password vault
- **Storage:** passwords live in `vault.json`, separate from the SQLite database.
  - Encryption: Argon2id (64 MiB, t=3) → XChaCha20-Poly1305 per entry with fresh nonces.
  - The vault key is additionally wrapped with DPAPI (CurrentUser).
  - Details and threat model: `docs/SECURITY.md`.
- **Unlocking:** off the UI thread; the host enforces an exponential retry delay (1 s → 60 s).
- **Auto-lock:** after inactivity, on Windows lock and on sleep.
- **Login detection** (top frame only) offers "Save password?" in the browser's popup window.
- **Fill:** a key icon in the address bar fills a login only after the user picks it, and only
  on an exact origin match.
- **Copies:** excluded from clipboard history and cleared after 30 s.

### Chromium switches (`AdditionalBrowserArguments`)
Always passed (`core::settings::Settings::browser_arguments`):
- `--no-pings` disables hyperlink auditing (`<a ping>`).
- `--disable-domain-reliability` disables Chromium's domain reliability telemetry.
- `--disable-features=AutofillServerCommunication,MediaRouter,OptimizationHints,InterestFeedContentSuggestions,msEdgeShoppingAssistant`
  turns off autofill crowdsourcing, Cast discovery, optimization-guide downloads, feed
  suggestions and the Edge shopping assistant.
- `msSmartScreenProtection` is added to `--disable-features` unless the user turns SmartScreen
  on.

Conditional:
- `--disable-gpu` when hardware acceleration is off.
- `--proxy-server=<value>` when a proxy is set.
- The user's extra flags, validated. `--disable-features` and `--enable-features` lists are
  merged; security-weakening switches such as `--no-sandbox` and
  `--disable-web-security` are rejected.

The environment also sets extensions off, OS single sign-on off, and tracking prevention on.
Each WebView disables WebView2's general autofill and password autosave, because the browser
has its own vault.

## Development
- Core tests: `cargo test -p core`
- Windows build: `cargo build --release --target x86_64-pc-windows-gnu`
- UI tests (mock host, no exe):
  `NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules node ui/dev/test-ui.cjs [shots-dir]`
