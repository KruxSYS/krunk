# LightBrowser

LightBrowser is a lightweight, privacy-minded web browser for Windows 10/11 (x64). It is a
single small Rust executable (about 6 MB) around the Microsoft Edge **WebView2** runtime, which
is Chromium and is kept up to date by Windows. It includes:
- tabs and private tabs
- history and bookmarks
- a new-tab page with your most-visited sites
- built-in ad and tracker blocking
- an encrypted password vault
- downloads, find, zoom, print and DevTools
- session restore
- background-tab sleep, to save memory

## Download and requirements
- **The file:** `LightBrowser.exe` is the whole program, with no installer. Verify it against
  `SHA256SUMS`:
  - PowerShell: `Get-FileHash LightBrowser.exe`
  - Linux: `sha256sum -c SHA256SUMS`
- **System:** Windows 10 or 11, 64-bit.
- **Runtime:** the Microsoft Edge WebView2 Runtime (Evergreen). It is preinstalled on Windows 11
  and on current Windows 10. If it is missing, LightBrowser says so at startup and offers to
  open the download page (https://developer.microsoft.com/microsoft-edge/webview2/).
- **Data location:** everything is stored in `%LOCALAPPDATA%\LightBrowser`:
  - `browser.db` (history, bookmarks, settings, session)
  - `vault.json` (encrypted passwords)
  - `engine.dat` (the compiled filter engine)
  - `profile\` (WebView2 cookies, cache and site data)
  - `lightbrowser.log`

  Delete that folder to reset the browser completely.
- **Unsigned binary:** the exe isn't code-signed, so SmartScreen may warn the first time it runs.

## Usage
Type a URL or search words in the address bar. Search uses DuckDuckGo by default; this can be
changed in Settings. Built-in pages:
- `lb://settings`
- `lb://history`
- `lb://bookmarks`
- `lb://passwords`
- `lb://downloads`
- `lb://about`

| Keys | Action |
|------|--------|
| Ctrl+T / Ctrl+Shift+N | New tab / new private tab |
| Ctrl+W, Ctrl+Shift+T | Close tab, reopen closed tab |
| Ctrl+Tab / Ctrl+Shift+Tab, Ctrl+1…8, Ctrl+9 | Next / previous tab, tab by position, last tab |
| Ctrl+L, Alt+D, F6 | Address bar |
| F5 / Ctrl+R, Ctrl+F5 / Ctrl+Shift+R | Reload, reload bypassing the cache |
| Alt+Left / Alt+Right, Alt+Home | Back / forward, home page |
| Ctrl+D | Bookmark this page |
| Ctrl+H, Ctrl+Shift+O, Ctrl+J | History, bookmarks, downloads |
| Ctrl+F, F3 / Shift+F3 (Ctrl+G / Ctrl+Shift+G) | Find, next / previous match |
| Ctrl+= / Ctrl+-, Ctrl+0 | Zoom in / out, reset |
| Ctrl+P | Print |
| F12, Ctrl+Shift+I | DevTools |

- **Shield:** the shield in the toolbar shows how many ads and trackers were blocked on the
  page. Click it to allow ads on that site.
- **Passwords:**
  - When you sign in to a site, LightBrowser asks "Save password?". The first save asks you to
    create the vault with a master password.
  - To fill a login, click the key icon in the address bar.
  - Manage logins on `lb://passwords`. Showing a password asks for the master password again.
- **Private tabs:** they use a separate InPrivate profile. Nothing from them is written to
  history, the session or the favicon cache. Their cookies and site data stay in memory and
  are not written to disk (WebView2 InPrivate).
- **Memory:** background tabs go to sleep after 5 minutes. Optionally they can be unloaded
  completely after a longer time; turn this on in Settings → Performance.

Command-line options (all optional):

| Option | Effect |
|--------|--------|
| `--update-filters` | Update the filter lists and run a blocking self-test, without opening a window |
| `--mem-report` | Log the memory of the browser's process tree to `memory-report.log` every 10 s |
| `--sleep-test-seconds=N` | Testing only: run the tab sleep/suspend/discard timers in seconds instead of minutes |

## How it works

### Architecture

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
  (about 1 ms on Linux; 4–20 ms measured at startup on a Windows 11 VM). The cache key covers the engine
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
  never logged and never stored in SQLite.
- Threat model (assets, attackers, mitigations, out of scope): `docs/SECURITY.md` (in the
  release download: `SECURITY.md` next to this file).
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

Checked at runtime on the Windows 11 VM (WebView2 156.0.4314.58) with
`scripts/vm-mem.ps1 -CommandLine`, which reads the `msedgewebview2.exe` browser process command
line from the process list. With default settings it contains:
```
--user-data-dir="C:\Users\pipe\AppData\Local\LightBrowser\profile\EBWebView" ...
--disable-domain-reliability
--disable-features=AutofillServerCommunication,MediaRouter,OptimizationHints,InterestFeedContentSuggestions,msEdgeShoppingAssistant,msSmartScreenProtection
--no-pings
```
LightBrowser runs no telemetry, update service or background process of its own. The only
network traffic it starts itself is the daily filter-list update (WinHTTP, see above).

### Memory
- **Lazy restore:** restored tabs show their saved title and the cached favicon. They get no
  controller (and so no renderer process) until they are first activated.
- **Background sleep:** after `tab_sleep_minutes` (default 5) without activation, a background
  tab gets `MemoryUsageTargetLevel=Low`; 60 s later it is suspended with `TrySuspend`.
  Activating it calls `Resume` and restores the normal memory level, with no reload.
- **Discard** (optional, off by default): after `tab_discard_minutes` the tab's controller is
  closed. URL, title and favicon are kept, and the tab reloads when activated. A discarded
  private tab keeps only its URL. The last live private tab is never discarded, so the InPrivate
  session (cookies, logins) survives.
- **Never touched:** the active tab, tabs playing audio, tabs with a download in progress, and
  the UI shell and popup.
- The rules are pure functions in `core::memory` (`decide`, `exempt`), with unit tests. The
  host evaluates them on a timer.
- Hidden options, not shown in the UI:
  - `--sleep-test-seconds=N` scales the timers for testing: sleep after N s, suspend N s later,
    discard after 4N s when discard is enabled.
  - `--mem-report` appends the summed working set and private bytes of the browser process
    tree, with tab, renderer and suspended counts, to `%LOCALAPPDATA%\LightBrowser\memory-report.log`
    every 10 s. The tree is our process plus every descendant, which includes the
    `msedgewebview2.exe` processes of our user data folder.

Measured on the Windows 11 VM (WebView2 156.0.4314.58), as the sum over the whole process tree.
"10 sites" were Wikipedia, GitHub, Hacker News, MDN, BBC News, The Guardian, Stack Overflow,
Reddit, NYTimes and CNN, plus the new-tab page:

| Scenario | Processes | Renderers | Working set | Private bytes |
|----------|-----------|-----------|-------------|---------------|
| 1 tab (new-tab page) | 8 | 2 | 379 MB | 140 MB |
| 11-tab session restored (lazy) | 8 | 2 | 379 MB | 137 MB |
| 10 sites open, awake | 23 | 15 | 2131–2173 MB | 1165–1291 MB |
| 10 background tabs at low memory | 23 | 15 | 1179 MB | 1186 MB |
| 10 background tabs suspended | 23 | 15 | 1084–1104 MB | 1179–1186 MB |
| after discard (9 unloaded, CNN active) | 13 | 5 | 973–988 MB | 660 MB |

How to read these numbers:
- The working-set sum counts shared DLL pages once per process, so it overstates physical RAM.
- Low memory and suspend trim the working set by about half, but they do not free committed
  memory.
- Discard frees committed memory too (about −44 %) and ends the renderer processes.

To reproduce: enable discard in Settings, start with `--sleep-test-seconds=90 --mem-report`,
open the sites and read `memory-report.log` (steps in `docs/MANUAL_TEST.md`).

## Building from source
Requirements: Rust stable (1.85 or newer). The workspace has two crates:
- `core`: platform-independent, `#![forbid(unsafe_code)]`
- `app`: the Windows host; all `unsafe` is confined to `app/src/ffi/`

The WebView2 loader DLL is embedded in the exe and checked against its SHA-256 at load time,
so there is nothing to ship next to the exe.

### Linux → Windows cross-compile (GNU toolchain; how the release is built)
```sh
sudo apt install gcc-mingw-w64-x86-64            # linker + windres (for the manifest/version resource)
rustup target add x86_64-pc-windows-gnu
# ~/.cargo/config.toml:
#   [target.x86_64-pc-windows-gnu]
#   linker = "x86_64-w64-mingw32-gcc"
cargo build --release --target x86_64-pc-windows-gnu
# -> target/x86_64-pc-windows-gnu/release/LightBrowser.exe
```
The release profile uses LTO, `opt-level = "s"`, strip and `panic = "abort"`. A clean build
takes about 1 minute on a fast machine. If the machine is short on RAM, use `-j 4`.

### Linux → Windows with the MSVC ABI (optional, cargo-xwin)
```sh
cargo install cargo-xwin
rustup target add x86_64-pc-windows-msvc
cargo xwin build --release --target x86_64-pc-windows-msvc
```
`cargo-xwin` downloads the MSVC CRT and Windows SDK, which means accepting Microsoft's licence.
*This path was not built in this project's pipeline; the GNU build above is the tested one.*

### Native Windows
- **MSVC** (Visual Studio Build Tools with the "Desktop development with C++" workload):
  `cargo build --release`. `embed-resource` uses `rc.exe` from the Windows SDK.
- **GNU:** install MSYS2 with `mingw-w64-x86_64-toolchain`, which provides `gcc` and `windres`,
  run `rustup target add x86_64-pc-windows-gnu`, then
  `cargo build --release --target x86_64-pc-windows-gnu`.

*Neither native path was exercised in this pipeline. The code is not GNU-specific.*

### Checks
```sh
cargo test -p core                                                       # 166 unit tests
cargo clippy --workspace --all-targets -- -D warnings                    # Linux
cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings
cargo fmt --all -- --check
cargo audit
NODE_PATH=/usr/lib/node_modules/@playwright/mcp/node_modules node ui/dev/test-ui.cjs [shots-dir]   # UI pages with a mock host
```
- Testing on Windows: `scripts/vm.sh` deploys `dist/LightBrowser.exe` to a Windows test machine
  over SSH and drives it (start, stop, screenshots, keys, clicks).
- `scripts/vm-mem.ps1` lists the process tree with memory figures and the WebView2 command line.
- `scripts/vm-leak-scan.ps1` searches the data folder for strings that must never be stored,
  such as a test password.
- The Windows checklist is `docs/MANUAL_TEST.md`.

## Known limitations
- **No tab pinning in the UI.** The memory policy supports pinned tabs, but the tab strip
  cannot pin yet.
- **Discarding loses back/forward history.** A discarded tab reloads its last URL; its history
  and any unsaved form input are lost. Discard is off by default.
- **uBlock Origin scriptlets (`+js(...)`) are not executed.** Network and cosmetic (CSS) rules
  work; scriptlet-based anti-adblock and annoyance fixes do not.
  - Cosmetic filtering of generic rules is limited to the first 2000 class names and ids a
    page reports.
- **Some settings need a restart:** hardware acceleration, proxy, extra flags and SmartScreen.
  This is because all tabs share one WebView2 environment. The settings page says when.
- **The WebView2 runtime determines the engine version.** LightBrowser cannot pin or patch
  Chromium; security updates come from Windows/Edge updates.
- **Memory figures:**
  - Sleep and suspend trim the working set but do not release committed memory; only discard
    does.
  - WebView2 shares renderer processes between same-site tabs, so discarding one of two tabs of
    the same site frees less.
- **Vault notes are write-only in the UI.** A login's note is stored encrypted like the
  password, but it is never sent back to the passwords page. Editing a login keeps its note; to
  change it, type a new one.
- **The vault does not detect rollback.** Replacing `vault.json` with an older copy is not
  detected. See `docs/SECURITY.md` for the full threat model and what is out of scope.
- **Platform support:** no Windows on ARM build, no installer, no auto-update and no extensions.
- **Unsigned:** the exe is not code-signed.

## Third-party components
- Microsoft WebView2 loader (`assets/webview2/`, `LICENSE.txt` and `NOTICE.txt`): BSD-style
  licence, redistributable.
- Filter lists:
  - EasyList and EasyPrivacy: GPL-3.0-or-later or CC BY-SA 3.0.
  - uBlock Origin uAssets: GPL-3.0.
  - Attribution and licences are in `assets/filters/`. The lists are used unmodified, as data.
- Rust crates: see `Cargo.lock`. The main ones are `webview2-com`/`windows` (MIT/Apache-2.0),
  `adblock` (MPL-2.0), `rusqlite` with bundled SQLite (MIT, public domain), and RustCrypto
  `argon2`/`chacha20poly1305` (MIT/Apache-2.0).
