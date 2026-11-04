# Krunk

Krunk is a lightweight, privacy-minded web browser for Windows 10/11 (x64), written in
Rust. It brings its own engine: **Chromium 156.0.8078.80** through the Chromium Embedded
Framework (**CEF 156.0.15**), bundled in the download. Neither Microsoft Edge nor WebView2 is
used, and nothing has to be installed. It includes:
- tabs and private tabs
- history and bookmarks
- a new-tab page with your most-visited sites
- built-in ad and tracker blocking
- an encrypted password vault
- downloads, find, zoom, print and DevTools
- session restore
- background-tab sleep, to save memory

## Download and requirements
- **The download:** `Krunk-win-x64.zip` (about 183 MB; 420 MB unpacked). Extract it
  anywhere and run `Krunk.exe`. There is no installer. Keep all files of the folder
  together; if one is missing, Krunk says so at startup.
- **Verify:** check `SHA256SUMS` (the zip and the three main binaries):
  - PowerShell: `Get-FileHash Krunk-win-x64.zip`
  - Linux: `sha256sum -c SHA256SUMS`
- **System:** Windows 10 or 11, 64-bit. No runtime is needed.
- **Data location:** everything is stored in `%LOCALAPPDATA%\Krunk`:
  - `browser.db` (history, bookmarks, settings, session)
  - `vault.json` (encrypted passwords)
  - `engine.dat` (the compiled filter engine)
  - `profile\` (the engine's profile: cookies, cache and site data in `profile\default`)
  - `krunk.log`, and `cef.log` (engine warnings and errors only)

  Delete that folder to reset the browser completely.
- **Coming from LightBrowser (Krunk's former name):** on its first start Krunk moves
  `%LOCALAPPDATA%\LightBrowser` to `%LOCALAPPDATA%\Krunk` with everything in it, including
  the engine profile (cookies and logins), the vault and `engine.dat`. It renames the folder
  when it can, and otherwise copies it and then deletes the old one. `krunk.log` records what
  happened. If LightBrowser is still running, Krunk asks you to close it first. If a Krunk
  folder already exists, the LightBrowser folder is left alone. Old `lb://` addresses open the
  matching `krunk://` page, and saved tabs and bookmarks of built-in pages keep working.
- **Coming from the WebView2 version:** settings, history, bookmarks, the session, the vault and
  `engine.dat` are used as they are. Cookies and site data are not migrated: the old
  `profile\EBWebView` folder (WebView2's profile) is left untouched and unused, and you can
  delete it. The same goes for the old `bin\` folder (the WebView2 loader).
- **Unsigned binaries:** the files are not code-signed, so SmartScreen may warn the first time
  `Krunk.exe` runs.

## Usage
Type a URL or search words in the address bar. Search uses DuckDuckGo by default; this can be
changed in Settings. Built-in pages:
- `krunk://settings`
- `krunk://history`
- `krunk://bookmarks`
- `krunk://passwords`
- `krunk://downloads`
- `krunk://about` (shows the CEF and Chromium versions of the running engine)

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
| F12, Ctrl+Shift+I | DevTools (in its own window) |

- **Shield:** the shield in the toolbar shows how many ads and trackers were blocked on the
  page. Click it to allow ads on that site.
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
- **Starting Krunk again** while it runs opens a new tab in the running window.

Command-line options (all optional):

| Option | Effect |
|--------|--------|
| `--update-filters` | Update the filter lists and run a blocking self-test, without opening a window |
| `--mem-report` | Log the memory of the browser's process tree to `memory-report.log` every 10 s |
| `--sleep-test-seconds=N` | Testing only: run the tab sleep/discard timers in seconds instead of minutes |

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
- **Main window:** one top-level Win32 window. Every view is a CEF browser (Alloy style) hosted
  as a child window.
- **Shell:** a view strip at the top showing the tab strip and toolbar. Its height is fixed:
  76 DIP, scaled by DPI.
- **Tabs:** one browser per *live* tab, laid out below the shell. Restored tabs stay unloaded
  until they are activated. Pages that open windows (`window.open`, `target=_blank`) get a new
  tab next to them; `window.open` keeps its opener.
- **Popup:** one extra view in an owned `WS_POPUP` tool window. It shows the main menu, the
  shield panel and the save-password and fill-login prompts.
  - It is created on first use and then hidden and reused, never more than one.
  - It uses the default profile, never a private one.
  - The shell sends the popup's rectangle in CSS pixels. The host converts it to screen pixels
    with the window's DPI (`GetDpiForWindow` + `ClientToScreen`) and clamps it to the monitor
    work area (`core::layout::popup_screen_rect`).
  - **Hide rules:** the popup hides on deactivate, Escape, a click elsewhere (another view gets
    focus), main-window move, resize or minimize, and tab switch or close. A prompt hidden this
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
- **Engine:** Brave's `adblock` engine with five bundled lists (EasyList, EasyPrivacy, and
  uBlock filters, privacy and unbreak; see `assets/filters/ATTRIBUTION.md`). The lists are
  embedded deflated (1.4 MB).
- **Cache:** at startup the compiled engine is loaded from `%LOCALAPPDATA%\Krunk\engine.dat`
  (about 4 ms on the Windows 11 VM). The cache key covers the engine version, which lists are
  enabled, and where each comes from (embedded copy or download) with its content hash. If
  anything differs, the engine is rebuilt on a worker thread.
- **Interception:** every request of every tab, private tabs included (sub-resources, frames,
  workers), passes `CefResourceRequestHandler::OnBeforeResourceLoad` on CEF's IO thread.
  - Blocked requests are cancelled. The shield count is kept in atomics and pushed to the UI
    thread at most about four times a second; the log gets one line per page with the count.
  - The tab's own top-level navigation is never checked.
  - The handler uses only in-memory state shared with the UI thread (`Arc`/atomics); the
    allowlist decision is made once per navigation.
- **Cosmetic filtering:** site-specific hiding rules are injected when a page commits, as a
  constructed stylesheet, which page CSP doesn't block. Generic rules are applied for the class
  names and ids the page reports, capped at 2000.
- **Updates:** at most once every 24 h (checked hourly), a worker thread downloads the lists
  over WinHTTP (HTTPS only, 16 MB cap, 30 s timeout) and validates them: no HTML, a list header,
  at least 50 rules, at least 2 KB. A failed or suspicious download keeps the previous source.
  - The updater uses the browser's HTTP proxy setting; with a SOCKS proxy it skips the download
    rather than bypass the proxy.
  - `Krunk.exe --update-filters` runs the update and a self-test without a window.

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
traffic not caused by pages is the filter-list update.

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
- Threat model (assets, attackers, mitigations, out of scope): `docs/SECURITY.md` (in the
  release download: `SECURITY.md` next to this file).
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
network traffic it starts itself is the daily filter-list update (WinHTTP, see above).

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
- **Never touched:** the active tab, tabs playing audio, tabs with a download in progress, and
  the UI shell and popup. A small tracker in each frame reports playing unmuted media and
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
scripts/build-dist.sh            # -> dist/Krunk-win-x64/, dist/Krunk-win-x64.zip, dist/SHA256SUMS
scripts/build-dist.sh --debug    # same layout with a debug build (testing only)
```

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
   `v8_context_snapshot.bin`, all `locales/`, `licenses/`, the README, SECURITY and third-party
   notices. Then it writes the zip (fixed timestamps and order) and `SHA256SUMS`.

Requirements (versions used for the release):
- Rust stable (1.85 or newer; built with 1.99.1) with `rustup target add x86_64-pc-windows-msvc`
- `cargo install cargo-xwin --version 0.23.1`
- `cargo install export-cef-dir --version 156.3.0` (must match the `cef` crate version)
- LLVM **20.1.8** (`clang-cl`, `lld-link`, `llvm-lib`; the MSVC C++ headers need clang 19 or
  newer), unpacked from the official release tarball. The script expects it in
  `~/.cache/krunk-toolchain/llvm-20.1.8` or in `$LB_LLVM`.
- `cmake`, `ninja`, `curl`, `python3`, `zip`

**Microsoft's licence:** cargo-xwin downloads the MSVC CRT and the Windows SDK (headers and
import libraries) from Microsoft. Building therefore means accepting Microsoft's licence terms
for them; `build-dist.sh` sets `XWIN_ACCEPT_LICENSE=1` for this (and only there). The versions
are pinned so that builds are reproducible: `XWIN_VERSION=17`, CRT `14.44.17.14` (MSVC 14.44.35220),
SDK `10.0.26100`; the download is cached in `~/.cache/krunk-xwin`.

A clean build from a fresh clone took 1 min 27 s on the build machine (CEF's C++ wrapper and
LTO), plus the one-time downloads of CEF (175 MB) and the MSVC CRT/SDK; it has no warnings. The
release profile uses LTO, `opt-level = "s"`, strip and `panic = "abort"`. Two clean builds
from the same checkout path give the same `Krunk.dll` except for 20 bytes: the PE time
stamp and the debug-directory GUID. `Krunk.exe` and the CEF files are always identical.

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
cargo test --workspace                                                   # core: 182 unit tests
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
- **The engine is bundled.** Chromium security fixes reach users only when a new Krunk
  release with a newer CEF is published and installed; there is no auto-update. Releases should
  follow CEF security releases promptly.
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
- **Size:** about 420 MB unpacked (183 MB zipped), against about 6 MB for the WebView2 build. All
  CEF locales and SwiftShader (software rendering for machines without a usable GPU) are kept.
- **Discarding loses back/forward history.** A discarded tab reloads its last URL; its history
  and any unsaved form input are lost. Discard is off by default.
- **uBlock Origin scriptlets (`+js(...)`) are not executed.** Network and cosmetic (CSS) rules
  work; scriptlet-based anti-adblock and annoyance fixes do not.
  - Cosmetic filtering of generic rules is limited to the first 2000 class names and ids a
    page reports.
- **The vault does not detect rollback.** Replacing `vault.json` with an older copy is not
  detected. See `docs/SECURITY.md` for the full threat model and what is out of scope.
- **No tab pinning in the UI.** The memory policy supports pinned tabs, but the tab strip
  cannot pin yet.
- **Platform support:** no Windows on ARM build, no installer, no auto-update and no extensions.
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
