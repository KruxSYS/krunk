# Krunk for Android: feature parity matrix

Cross-reference of a mobile browser's feature set against Windows Krunk 0.15.3 and the Android
preview (0.1.0, round 1). Later rounds work through it in order.

> Categories follow the 17-group structure agreed for round 1; the operator can supply their
> original category wording and we will re-key the rows in round 2 (mapping is mechanical).

**Round-1 verification.** Items marked *done* were built and pass the unit tests; the ones
also marked **(v)** were exercised on the sandbox emulator with a screenshot
(`.pipeline/screenshots/android/`, see the round report for the flow → file list).

**Round 2 (0.2.0).** Rows changed by Android 0.2.0 say so. **(v)** there means: checked on the
sandbox emulator by the scripted device tests of that release (`a3.sh`, 211 checks on the debug
build, the release build checked again before publishing; evidence in
`.pipeline/evidence-android-0.2.0/`). The vault rows stay with their round but were not part of
0.2.0. The column heading "Android 0.1.0" is kept for the table's shape; the cells state the
current release.

**Round 2, second part (0.3.0).** The vault rows of section 11 were built in Android 0.3.0.
**(v)** there means: checked on the sandbox emulator (Android 16 x86_64, no StrongBox) by the
scripted device tests of the vault on the debug build (`android/scripts/a4/a4.sh`; evidence in
`.pipeline/evidence-android-0.3.0/m5/`). "done, not run on a device" means built and covered by
unit tests with stand-ins, but never exercised on an emulator or a phone.

**Round 3, first part (0.4.0).** Rows changed by Android 0.4.0 say so. **(v)** there means:
checked on the sandbox emulator (Android 16 x86_64) on the **release build** by the scripted
device tests of the round (`android/scripts/a5/a5.sh`; evidence in
`.pipeline/evidence-android-0.4.0/m8/`). Real camera and microphone capture, a real location fix
and notification delivery on a phone were not run.

**Round 4 (0.6.0).** Account and sync (§13 and the Sync row of §7). **(v, staging)** there means:
checked end to end against a **staging** copy of the server with Windows Krunk on the Windows VM
and the Android 16 emulator (evidence in `.pipeline/evidence-android-0.6.0/`); the live server
was not used for these tests.

**Columns.** *Windows Krunk*: yes / partial / no, with the README section or CHANGELOG version
(and the `core` module when the logic is shared). *Android 0.1.0*: done / partial / roadmap /
not planned. *Engine*: the GeckoView API that provides it, or the limitation. *Round*: the Android
round that builds it (1 = this one; 2 = blocking + vault; 3 = downloads, permissions, media; 4 =
account/sync; 5 = VPN, agent, release). "Google-service" rows: *not planned — conflicts with
Krunk's no-Google-contact promise; alternative: …*, unless the operator overrides.

## 1. Address bar & search

| Feature | Windows Krunk | Android 0.1.0 | Engine | Round |
|---|---|---|---|---|
| URL vs search decision (scheme, IP, localhost, host:port, public suffix, local suffixes) | yes — Usage › Address bar, 0.15.0; `core::urlclean::input_to_url` | done (v) (same core function via UniFFI) | app-side | 1 |
| Default engine DuckDuckGo; Google, Bing, Brave, Startpage, Ecosia selectable | yes — `core::settings::SearchEngine` | done (v) | app-side | 1 |
| Custom search engine (URL template with `%s`) | yes — `custom_search_url` | done | app-side | 1 |
| Google search integration | selecting Google as the engine only — no Google-specific integration | done (same meaning) | — | 1 |
| History / bookmark / typed-site suggestions, most visited first | yes — 0.5.1; `core::omnibox` | done (v) | app-side | 1 |
| Inline completion of the typed site | yes — 0.6.0 | partial (core computes it; the Android editor shows rows only) | — | 2 |
| Engine search suggestions (setting, never in private tabs, no cookies) | yes — 0.15.0; `core::search_suggest` | done (v) | app-side HTTP | 1 |
| Tracking-parameter stripping (`utm_*`, `fbclid`, `gclid`, …) | yes — Privacy, 0.6.0; `core::urlclean` | done (v) (0.2.0: also removed from links clicked inside a page and from redirects, by request-level rewriting in Krunk's own extension (`krunk-guard`, `webRequest` on top-level GET navigations only; the answer comes from `core::urlclean` through the app, at most three rewrites per navigation, a slow answer lets the request pass). POST requests and sub-resources are never touched; the setting that controls stripping on Windows controls it here. Since 0.1.x: typed addresses and every tab Krunk opens itself) | `WebExtension` `webRequest` + native messaging | 1, 2 |
| Automatic HTTPS upgrade (HTTPS-first) | no (HTTPS-only mode only) | done (v) — Gecko's HTTPS-First is on by default (verified: `http://example.com` loads as https) | Gecko default | 1 |
| HTTPS-only mode | yes — Privacy; `settings.https_only` | done (`allowInsecureConnections = HTTPS_ONLY`) | `GeckoRuntimeSettings` | 1 |
| Copy / paste / edit URL; paste-and-go | yes — 0.6.0 | done (platform text field menu) | — | 1 |
| Site info / certificate popup | yes — 0.5.0; `core::certinfo` | done (v) (0.9.0: the shield sheet's Connection part: issued to/by, validity, SHA-256 (long-press copy), names, folded details (serial, algorithms, key, SHA-1); exception warning + Forget this exception; mixed content; http. The lock opens it there. Leaf certificate only, from the engine; no chain. A page with mixed passive content gets no certificate from GeckoView: Show certificate runs `CertProbe` on request and says so. Emulator-tested) | `SecurityInformation.certificate` / `isException` | 3 |
| Voice search | no | roadmap (Android `RecognizerIntent`, on-device where available) | — | 3 |
| QR code scanning | no (sign-in QR only) | roadmap (needs a camera permission flow; CameraX/ML Kit-free decoder) | — | 3 |
| Search the selection ("Search DuckDuckGo for …") | yes — 0.11.0 | done (v) (0.2.0: "Search DuckDuckGo for …" in the menu over selected text, next to Copy; a new tab with the same privacy; the label and address come from `core`) | `SelectionActionDelegate` | 2 |

## 2. Navigation & page

| Feature | Windows Krunk | Android 0.1.0 | Engine | Round |
|---|---|---|---|---|
| Back, forward, reload, stop | yes — keys table | done (v) | `GeckoSession` | 1 |
| Home (configurable homepage) | partial — Alt+Home, no button since 0.5.0 | done (v) (`settings.homepage`; since 0.2.0 Home is in the first row of the ⋮ menu on phones and a toolbar button from tablet width) | — | 1 |
| Back/forward history menu | yes — 0.8.0; `core::navmenu` | done (v) (0.2.0: Back and Forward moved from the toolbar into the first row of the ⋮ menu, enabled from the page's history; a long press lists the earlier or later pages (`core::navmenu` through `ffi::nav_menu`). The system back gesture is unchanged) | `SessionState` history | 2 |
| Pull-to-refresh | n/a | done (nested scroll from the GeckoView; not exercised on a device) | `NestedScrollingChild` | 1 |
| Progress bar, title, favicon | yes | done (favicons via the bundled `krunk-icons` extension + `/favicon.ico` fallback, stored in the shared `favicons` table) | `ProgressDelegate`, `WebExtension` messaging | 1 |
| Security indicator (lock / "Not secure") | yes — 0.5.0 | done (v) | `onSecurityChange` | 1 |
| Custom error page (Krunk-styled) | partial — crash/hang page, HTTPS-only interstitial | done (v) (`assets/error.html` from `onLoadError`, light/dark) | `NavigationDelegate.onLoadError` | 1 |
| Certificate errors: "Continue anyway", remembered per site + exact certificate | yes — 0.21.0 | done (a native screen over the error page for `ERROR_SECURITY_BAD_CERT`: host, the likely problem, SHA-256; Back to safety, Continue anyway, "Remember for this site" (off by default; else until the app closes; private tabs: never permanent). No Continue for `ERROR_BAD_HSTS_CERT` and `ERROR_SECURITY_SSL`. Settings › Privacy › Certificate exceptions lists and removes them. The engine's `nsICertOverrideService` through the `krunkCerts` experiment API of the bundled `krunk-sitedata` extension. GeckoView 160 passes no certificate to `onLoadError`, so `CertProbe` reads it with one app-side TLS handshake. 0.6.1, emulator-tested) | `CertProbe` + experiment API | 3 |
| Find in page (count, next/previous) | yes — Ctrl+F | done (v) | `SessionFinder` | 1 |
| Request desktop site (per tab) | yes — user-agent presets (`core::useragents`) | done (v) (Gecko desktop mode + viewport; the Windows UA presets are selectable in Settings › User agent. Since 0.2.0 the mode belongs to the tab: it survives a recreated engine session (crash, discard) and, for normal tabs, a restart) | `GeckoSessionSettings.userAgentMode` | 1 |
| Page zoom: pinch | n/a | done (+ "force zoom" setting) | `forceUserScalableEnabled` | 1 |
| Default zoom setting | yes — Usage › Zoom; `core::zoom` | partial — GeckoView has no page-zoom API; `default_zoom` scales text instead | `fontSizeFactor` | 2 |
| Text scaling (system font scale / in-app) | partial — UI font only | done | `automaticFontSizeAdjustment` / `fontSizeFactor` | 1 |
| Full-screen web content | yes — F11, 0.2.0 | done (v) (system bars hidden, back exits. 0.4.0 (v): a page may lock the orientation while it is full screen and gets the old one back on every way out; a lock outside full screen is refused; screen kept on; full screen ends when another tab or screen comes on top) | `ContentDelegate.onFullScreen`, `OrientationController.OrientationDelegate` | 1 |
| Pointer lock (games with a mouse) | yes — `core::pointer_lock` | **not available on Gecko Android** (v) (0.4.0: `requestPointerLock()` resolves and `pointerlockchange` fires, but no mouse movement reaches the page while locked; GeckoView has no pointer-capture API. Krunk adds no hint and fakes nothing) | — (none in GeckoView 160) | 3 |
| Translate | no | roadmap (Firefox Translations is local/offline; needs the models' download flow) | `TranslationsController` | 3 |
| Print / save as PDF | yes — Ctrl+P | roadmap | `GeckoSession.saveAsPdf`, `PrintDelegate` | 3 |
| PDF viewer | yes — 0.11.3 | roadmap (pdf.js is built into Gecko; needs testing) | Gecko pdf.js | 3 |
| View source | yes — 0.11.0 | roadmap | `view-source:` | 3 |
| JavaScript dialogs (alert/confirm/prompt/leave) | yes — 0.12.4 | done (v) (0.1.1: alert, confirm, prompt, `<select>` lists and HTTP sign-in, titled with the asking site, with a way out of dialog loops. 0.2.0: file fields through Android's document picker (one or several files, `accept`; a `capture` field offers the camera app, no permission), date, time, datetime-local, week and month fields, colour fields, "leave this page?", "send the form again?" and `navigator.share` through the share sheet. Every way out answers the page. Not offered: folder upload) | `PromptDelegate` | 1, 2 |
| Reader mode | no | roadmap | Gecko reader view | 4 |
| Developer tools | yes — 0.3.0 | not planned on-device (remote debugging in debug builds only) | `remoteDebuggingEnabled` | — |

## 3. Links & context menu

| Feature | Windows Krunk | Android 0.1.0 | Engine | Round |
|---|---|---|---|---|
| Open link in current tab | yes | done | — | 1 |
| Open in new tab / private tab | yes — 0.15.3 | done (v) (long-press menu) | `onContextMenu` | 1 |
| Copy link, copy link text | yes (address) / no (text) | done / done (title or alt text; the engine exposes no selection) | `ContextElement` | 1 |
| Share link, share page | no | done (Android share sheet) | — | 1 |
| Download link / save image, open image in new tab | yes — 0.12.2 (images) | done (v) (0.4.0: through Krunk's own downloads, with the page's cookies and referrer, `GeckoWebExecutor`) | `ContextElement.srcUri` | 1 |
| `window.open` / `target=_blank` into a new tab | yes | done since 0.1.1 (0.1.0 handed the engine an already open session and the new window failed); the address bar stays empty until the new window has loaded something | `onNewSession` | 1 |
| `mailto:` handler | yes — 0.11.3 | done (v) (0.2.0: `mailto:` and `tel:` links go to Android's mail and phone apps: at once after a tap (within 2 s), otherwise Krunk asks first, naming the address or number. `intent:` and other app links are not handed over) | `onLoadRequest` | 2 |
| Selected address text → open | yes — 0.15.2 | done (v) (0.2.0: "Open example.com/docs" in the same menu when the selection is one web address; never in a password field) | `SelectionActionDelegate` | 2 |

## 4. Tabs

| Feature | Windows Krunk | Android 0.1.0 | Engine | Round |
|---|---|---|---|---|
| Tab model shared with Windows (`core::tabs`) | yes | done (UniFFI `TabModel`; session JSON in `kv["session"]`, same shape) | — | 1 |
| Lazy restore (only the active tab loads) | yes — Memory | done (v) | — | 1 |
| Session restore on start; restore after process death | yes — 0.3.0 | done (v) (+ Gecko `SessionState` per tab for back/forward) | `SessionState` | 1 |
| "Restore tabs" prompt after a crash | partial (always restores) | done | — | 1 |
| Tab switcher grid with thumbnails, close, swipe-to-close, close all, search | tab list ▾ with filter — 0.11.0; previews on hover — 0.15.0 | done (v) | `GeckoView.capturePixels` | 1 |
| Tab counter in the toolbar | n/a | done (v) | — | 1 |
| Private tabs (no history, in-memory data, distinct theme) | yes — Usage › Private tabs | done (v) | `usePrivateMode` | 1 |
| Recently closed (reopen) | partial — Ctrl+Shift+T stack, memory only | done (v) (list, persisted; a documented deviation) | — | 1 |
| Background tab sleep / discard policy (`core::memory`) | yes — 5 min sleep, optional discard | done (`setActive(false)` / session discard; `onTrimMemory`) | `GeckoSession.setActive` | 1 |
| Tab groups (create/rename/colour/move) | no | roadmap | — | 3 |
| Background tab loading control | partial (`link_tabs`) | done (v) (0.2.0: Settings › Tabs › "Tabs opened by links": as the page asks, always in front, always behind; the shared `link_tabs` setting) | — | 2 |
| Pinned tabs | no | roadmap | — | 3 |
| Duplicate, move, close others/right | yes | partial (v) (0.2.0: a long press on a tab card offers Duplicate, Close other tabs and Close tab, also as accessibility actions; "others" are the other tabs of the same kind, never across the private boundary. Moving tabs: round 3) | — | 2 (move: 3) |
| Multiple windows | yes — 0.3.0 | not planned (phones) / roadmap for tablets | — | — |
| Mute tab, task manager | yes — 0.6.0 | roadmap | `MediaSession` | 3 |

## 5. UI & theme

| Feature | Windows Krunk | Android 0.1.0 | Engine | Round |
|---|---|---|---|---|
| Light / dark / follow system (Krunk palette, no Material dynamic colour) | yes | done (v) | `preferredColorScheme` for content | 1 |
| Toolbar at the top by default (since 0.1.1), bottom as an option | n/a | done (v) (Settings › Toolbar position; a position stored by 0.1.0 is kept) | — | 1 |
| Toolbar auto-hide on scroll | n/a | done (v) | nested scroll / `ScrollDelegate` | 1 |
| Edge-to-edge, gesture and 3-button navigation, predictive back | n/a | done | — | 1 |
| Landscape, tablet layout (sw600dp) | n/a | done (v) (wider grid; two-pane settings) | — | 1 |
| Accessibility: 48 dp targets, content descriptions, TalkBack switcher, font scale, haptics | partial on Windows | done (0.2.0: reload/stop and the shield have 48 dp targets inside the address field; switch and check rows are one node announced as on/off; the "Request desktop site" row announces its state) | — | 1 |
| Appearance customisation (accent, styles, presets) | yes — 0.7.0, 0.15.0; `core::style` | done (v) (0.2.0, pulled forward from round 4: Settings › Appearance › Style offers the styles of `core::style` (twenty since Windows 0.20.0) with a drawn preview each, and Accent colour seven named colours or any other. Mapped: accent, toolbar fill (solid, gradient along the toolbar, glass as a tint without blur), shape of the address field and the tab cards, how the active tab card is marked. Not on Android: glow, density, tab radius, UI font, strip and tab text colours, a gradient's exact angle (only its side), and editing a style's single values; a look made by hand on Windows is drawn but shown as "Default". Private tabs keep one fixed look in every style. Contrast (text 4.5:1, icons 3:1) is tested for every style in light, dark and private with 17 accents) | app-side; colours resolved in `ffi/src/theme.rs` | 2 |
| Toolbar customisation | yes — 0.6.0 (toolbar items) | roadmap | — | 3 |
| Swipe between tabs gesture | n/a | roadmap | — | 3 |
| Keyboard shortcuts (hardware keyboard) | yes | roadmap | — | 4 |

## 6. New-tab page

| Feature | Windows Krunk | Android 0.1.0 | Engine | Round |
|---|---|---|---|---|
| Native page with search box | yes (HTML) | done (v) (Compose) | — | 1 |
| Top sites by frecency (`core::frecency`) | yes | done | — | 1 |
| Shortcuts: pin / edit / remove (shared `tiles` table) | yes — 0.6.0, 0.15.0 | done | — | 1 |
| Recently closed row | no | done | — | 1 |
| Built-in wallpapers (10, from `ui/shared/wallpapers`) + colour | yes — 0.7.0 | done | — | 1 |
| Own picture as background, blur | yes — 0.7.0, 0.15.0 | roadmap (Android photo picker) | — | 2 |
| News / Discover feed | no (Chromium feed disabled) | not planned — conflicts with Krunk's no-Google-contact promise; alternative: an optional RSS/Atom feed the user adds, in a later round | — | — |
| Lens / visual search | no | not planned — Google Lens needs Google services; alternative: none planned | — | — |
| Voice shortcut | no | roadmap (with voice search) | — | 3 |
| Layout customisation (columns/rows, sections) | yes — 0.6.0 | partial (columns/rows and sections from settings; no drag reorder yet) | — | 2 |

## 7. Bookmarks

| Feature | Windows Krunk | Android 0.1.0 | Engine | Round |
|---|---|---|---|---|
| Add (star), edit, delete, folders | yes — `core::bookmarks` | done (v: add) (same SQLite tables) | — | 1 |
| Bookmark manager with search | yes — `krunk://bookmarks` | done (v) | — | 1 |
| Netscape HTML import / export | yes — `core::netscape` | done (Android file picker) | — | 1 |
| Import from other browsers | yes — 0.5.0 | roadmap (Android browsers expose no files; HTML export only) | — | — |
| Bookmarks bar | yes — 0.4.0 | not planned on phones (folder "Bookmarks bar" is shown in the manager) | — | — |
| Reading list | no | roadmap | — | 4 |
| Offline saving (save page) | no | roadmap | `saveAsPdf` / WebExtension | 4 |
| Sync | yes — 0.9.0 (beta) | done (v, staging) (0.6.0, bookmarks both ways, merged on the first sync; see §13) | — | 4 |

## 8. History

| Feature | Windows Krunk | Android 0.1.0 | Engine | Round |
|---|---|---|---|---|
| History grouped by day, search | yes — `krunk://history` | done | `HistoryDelegate.onVisited` | 1 |
| Delete entry, delete range, clear all | yes | done (entry, clear all; range via Clear browsing data) | — | 1 |
| Private tabs never write history | yes | done (v) (verified by a `visits` count) | `usePrivateMode` | 1 |
| `:visited` styling from Krunk's history | n/a | roadmap | `HistoryDelegate.getVisited` | 2 |

## 9. Settings & data

| Feature | Windows Krunk | Android 0.1.0 | Engine | Round |
|---|---|---|---|---|
| Settings on the shared `core::settings` fields (validated by the core, same JSON) | yes | done (v) | — | 1 |
| Search engine + custom URL, homepage, startup, new-tab page, theme, text size, suggestions, HTTPS-only, tracking params, tab sleep/discard, user agent | yes | done (0.1.1: custom search engine and custom new-tab address can be turned on, the user-agent setting is applied, "strip tracking parameters" off survives a restart) | — | 1 |
| Toolbar position, force zoom, follow-system font (Android-only, `kv["android.settings"]`) | n/a | done (v: toolbar position) | — | 1 |
| Clear browsing data (history, cookies, cache, site data) | yes | done | `StorageController.clearData` | 1 |
| Clear on exit | yes — `ClearOnExit` | roadmap | — | 2 |
| Cookies and site-data list per site (`krunk://storage`) | yes — 0.2.0 | done (v) (0.4.0: Settings › Privacy › Site data: sites with cookie count and stored size, from a built-in extension (`cookies.getAll`, quota usage) with a fallback list from history and permissions; per-site clearing with a confirmation, permissions optional; a public-suffix site is cleared by its own host only. Cookie values are not shown) | `StorageController.clearDataFromBaseDomain` / `clearDataFromHost`, WebExtension `cookies` | 3 |
| Site permissions defaults and per-site list | yes — Site permissions | done (v) (0.4.0: prompts for camera, microphone, location, notifications, persistent storage, DRM, storage access, local network/device; XR and audible autoplay default deny, muted autoplay default allow; Settings › Privacy › Site permissions with Allow / Deny / Ask per site and kind; the shield sheet opens the page's site. (v) for location, notifications, the list and the change; camera and microphone only through the prompt logic, no real capture on the emulator) | `PermissionDelegate`, `StorageController.setPermission` | 3 |
| Settings search box | yes — 0.5.0 | roadmap | — | 2 |
| About (versions, licences, link) | yes — `krunk://about` | done (0.2.0: lists uBlock Origin with the bundled version and the filter lists inside it with their licences) | — | 1 |
| Data location documented; backup rules (0.1.1: no cloud backup; device-to-device transfer carries the database; Gecko profile never) | n/a | done (README; 0.3.0: the vault directory `files/krunk-vault` is excluded by name from cloud backup and from device-to-device transfer, and `BackupRulesTest` fails if a rule would carry it. A real device-to-device transfer was not run) | — | 1, 2 |
| Reset settings, settings export | yes / no | roadmap | — | 3 |

## 10. Downloads

| Feature | Windows Krunk | Android 0.1.0 | Engine | Round |
|---|---|---|---|---|
| Download page (cards, search, filters, auto-remove) | yes — 0.15.0; `core::downloads` | done (v) (0.4.0: ⋮ › Downloads, Krunk's own: the engine's response stream is saved (cookies, `blob:`, `data:`), the list is `core::downloads` through `ffi`; search; no filter chips or auto-remove yet) | `onExternalResponse`, `GeckoWebExecutor` | 3 |
| Pause / resume / retry, multi-connection | yes — `core::segdl` | done (v) for one connection (0.4.0: Range + If-Range with the saved validator, `core::dlresume`; "cannot be paused" and Restart where the server does not allow it; never a silent restart). Multi-connection: roadmap | — | 3 |
| Open downloaded files, notifications | yes | done (v) (0.4.0: open and share through `content://`, a notification for running, paused, finished and failed downloads, a `dataSync` foreground service in the background) | — | 3 |

## 11. Passwords & autofill

| Feature | Windows Krunk | Android 0.1.0 | Engine | Round |
|---|---|---|---|---|
| Encrypted vault (`core::vault`, Argon2id + XChaCha20) | yes | done (v) (0.3.0: the same scheme and file format through `ffi` `KrunkVault`; create, unlock, wrong password refused, growing wait after wrong attempts, change password. Not on Android: cards, recovery phrase, sync; a vault of format 2 to 4 keeps those parts untouched, by `ffi` test) | app-side | 2 |
| Vault key bound to the device | yes — DPAPI | done (v) for the TEE-backed Keystore key (AES-256-GCM, `krunk.vault.wrap.v1`, made only by "Create vault"); StrongBox first: done, not run on a device (the emulator has none). Key lost (the key store reports the key missing or invalidated): "This vault cannot be opened", no password asked, file kept, delete behind a typed confirmation (v, with a debug hook that removes the key); a key store that does not answer is "try again", not key loss. The wrap protects the logins in a copied file, not the password against offline guessing (Argon2id alone slows that). The file opens on no other device or platform (fixture tests both ways) | Android Keystore | 2 |
| Passwords screen (list, search, view with Show, add, edit, delete, change password, lock now) | yes — `krunk://passwords` | done (v) (menu and Settings › Privacy). Copy user name / password with the sensitive flag and the 30 s clear: done, unit tests only | app-side | 2 |
| Auto-lock | yes — after inactivity, on Windows lock and on sleep | done (v) (after 1/5/15/30/60 minutes without use, default 15; "Lock after leaving Krunk": at once / 30 s / 1 minute (default) / 5 minutes; always locked after the process ends). Both are Android-only settings in `kv` | app-side | 2 |
| Unlock with biometrics | no | done, not run on a device (optional, off, Android 11+; platform `BiometricPrompt`, strong biometrics only; password always works; a new enrolment or a changed password switches it off). On the emulator only: the switch is disabled with "No fingerprint enrolled on this device" (v) | `BiometricPrompt`, Android Keystore | 2 |
| Save / fill prompts, HTTP auth fill | yes — 0.5.0, 0.8.0 | done (v) (0.3.0: save offer after sign-in with new / update / unchanged decided by Krunk, "Never for this site", offers can be switched off; picker on tapping a sign-in field, never a silent fill; the engine gets placeholders until a login is picked; locked vault asks at the tap; no vault: "Create vault" and the login is kept for it. HTTP sign-in dialog: saved logins of the exact origin, "Unlock vault for saved sign-ins", save offer after success; no offer for proxies. Checked on an http test page and one public https form) | `Autocomplete.StorageDelegate` (placeholders), `PromptDelegate.onLoginSelect` / `onLoginSave` / `onAuthPrompt`, `loginAutofillEnabled(false)` | 2 |
| Logins in private tabs | no save offer in private windows | done (v) (nothing offered, never a save offer; "Fill from vault" in the menu arms the tab, then picker and fill) | app-side | 2 |
| Logins on pages without encryption (http) | an http page gets only its own http logins | done (v) (a warning to confirm before a fill and before a save; an https login is never offered to an http page, by `core::vault::match_kind`) | app-side | 2 |
| Sign-in forms in a frame of another site | yes — the frame's own origin's logins | not filled (by decision, stricter than Windows; unit test `LoginPolicyTest`, not checked on the emulator) | — | 2 |
| Vault screens kept out of screenshots and Recents | n/a | done (v) (`FLAG_SECURE` while the vault screen, an unlock dialog, a picker or a save offer is on screen; the scripted tests assert blank screenshots) | `FLAG_SECURE` | 2 |
| Password generator (`core::passgen`) | yes — 0.6.0 | roadmap (not in 0.3.0; the engine's own generator is switched off) | — | 3 |
| Recovery phrase | yes — 0.7.0 | roadmap (not in 0.3.0: a forgotten vault password cannot be recovered on Android, and the create and unlock screens say so) | — | 3 |
| Import of logins from other browsers, export | yes — Settings › Passwords & import | roadmap (none in 0.3.0) | — | 3 |
| Payment cards | yes — 0.6.0 | roadmap | `Autocomplete.CreditCard` | 3 |
| System-wide Autofill service (the vault in other apps) | n/a | roadmap (open question in `docs/ANDROID_PLAN.md` §7) | Android Autofill framework | — |
| Google Password Manager | no | not planned — conflicts with Krunk's no-Google-contact promise; alternative: Krunk's vault, and Android Autofill service integration later | — | — |
| Passkeys / WebAuthn | partial | partial — GeckoView uses Play Services FIDO when present (kept by decision; `-Pkrunk.noGms=true` builds without it for devices without Play services) | GMS FIDO | — |

## 12. Privacy & content blocking

| Feature | Windows Krunk | Android 0.1.0 | Engine | Round |
|---|---|---|---|---|
| Ad and tracker blocking (Brave engine + 7 lists + uBO resources) | yes — How it works › Ad and tracker blocking | done (v) (0.2.0: uBlock Origin 1.76.0, its signed release built in unmodified as a GeckoView extension (`android/ubo.lock`, every file in the APK compared with the release at build time). It filters requests and hides page elements with the lists inside its package, in normal and private tabs, from the first page on: page loads wait for it at start (a few seconds; after 10 s they go ahead and the shield warns). A second extension of Krunk's own (`krunk-guard`) keeps uBlock Origin off the network unless the user starts an update, and drives it through its own messages. Blocking can be switched off in Settings › Privacy. A blocked top-level address shows Krunk's page with "Open once") | `WebExtensionController.ensureBuiltIn`; privileged experiment APIs of the built-in guard extension | 2 |
| Shield counter, per-site allow, custom lists | yes | partial (v) (0.2.0: the shield in the address field shows how many requests were blocked on the page; its sheet has the per-site switch (stored in the shared `adblock_allowlist` table, kept over restarts) and, in private tabs, the count without a switch. Settings › Privacy lists the allowed sites. Custom lists: round 3) | `WebExtension.ActionDelegate`; uBlock Origin's own messages | 2 (custom lists: 3) |
| Tracking protection levels (Standard/Strict/Custom) | yes — 0.6.0; `core::protection` | roadmap (one design over the engine's settings and uBlock Origin's lists, after blocking has shipped) | `ContentBlocking.Settings` | 3 |
| Third-party cookie policy, referrer trimming | yes — `core::cookies` | partial (0.2.0: cookies are "first party, others isolated per site" (Gecko's Total Cookie Protection behaviour), fixed, no setting yet; referrer trimming: roadmap) | `ContentBlocking` | 2 (setting: 3) |
| DNT / GPC headers | yes | roadmap (Gecko prefs) | — | 3 |
| Fingerprinting protection | partial | roadmap (Gecko `privacy.resistFingerprinting` flavours) | — | 3 |
| Custom DNS (DoH/DoT/DoQ) | yes — 0.6.0; `core::dns` | roadmap (Gecko DoH) | `network.trr.*` prefs | 3 |
| Safe Browsing | no (switched off on Windows) | not planned — conflicts with Krunk's no-Google-contact promise; alternative: uBlock Origin's badware and malware lists. **0.1.0 and 0.1.1 did contact Google's Safe Browsing update service in the background although the setting was "off" (fixed in 0.2.0, verified by packet capture).** There is no Safe Browsing warning page in Krunk for Android | `ContentBlocking.SafeBrowsing` | — |
| No telemetry, no background Krunk traffic | yes | done (v) since 0.2.0 — **0.1.0 and 0.1.1 were not quiet:** the engine contacted Mozilla (Remote Settings, a connectivity check) and Google (Safe Browsing list updates) in the background. 0.2.0 switches these off with an engine configuration file and checks it with a packet capture per app UID: no packet in 10 minutes after a fresh install, and none on later starts. The price: without Remote Settings the engine gets no updates of certificate revocation data (CRLite) and of preloaded intermediate certificates, and there is no Safe Browsing. Settings › Privacy › "Update automatically" (off by default) turns the filter-list updates and Mozilla's Remote Settings on together; Safe Browsing and the connectivity check stay off in every case | engine config file (`GeckoRuntimeSettings.configFilePath`), `ContentBlocking.Settings` | 1, 2 |
| Private tabs | yes | done (v) | `usePrivateMode` | 1 |
| Session logs (`krunk://sessions`, opt-in) | yes — 0.15.0 | roadmap | — | 4 |

## 13. Account & sync

| Feature | Windows Krunk | Android 0.1.0 | Engine | Round |
|---|---|---|---|---|
| Krunk account sign-in (device code, QR pairing) | yes — 0.8.0, 0.13.0; `core::account` | done (v, staging) (0.6.0: Settings › Account & sync › Sign in shows the code and opens the approval page in a tab; tokens through `ffi::KrunkSync`, refresh token wrapped by the Keystore key `krunk.sync.wrap.v1`. Not on Android: scanning the account page's QR code (native `link/pair/claim`)) | — | 4 |
| End-to-end encrypted sync of bookmarks, history, settings, tabs, passwords | yes — 0.9.0; `core::sync` | done (v, staging) (0.6.0: the shared `core::sync` engine through UniFFI: the same data types as Windows, incl. new tab pins, ad-blocker exceptions and the never-save list; passwords while the vault is unlocked; keys by approval from a syncing device (6 digits), by passphrase or recovery key, or as the first device; status, per-type switches, Sync now, other devices' tabs; foreground-only schedule. Android-only settings stay on the phone. Not on Android: approving/removing other devices, Finish securing) | — | 4 |

## 14. Extensions

| Feature | Windows Krunk | Android 0.1.0 | Engine | Round |
|---|---|---|---|---|
| Install extensions (CRX / Web Store) | yes — 0.6.0 (own MV3 subset) | not planned as CRX; roadmap: a curated set of Firefox add-ons (WebExtensions) | `WebExtensionController.install` | 4 |
| Bundled built-in extension (Krunk's own) | n/a | done (`krunk-icons`, favicon reporting) | `ensureBuiltIn` | 1 |
| uBlock Origin bundled | n/a | done (v) (0.2.0: see "Ad and tracker blocking"; version, release file and SHA-256 in `android/ubo.lock`, licences in `android/THIRD-PARTY-NOTICES.md` and under Settings › About) | `ensureBuiltIn` | 2 |

## 15. Media

| Feature | Windows Krunk | Android 0.1.0 | Engine | Round |
|---|---|---|---|---|
| Codecs (H.264/AAC, VP8/9, AV1, Opus, …) | yes — own CEF build | partial — Gecko uses the device's codecs (MediaCodec); untested in 0.1.0 | — | 3 |
| Picture-in-Picture | yes — 0.6.0 | roadmap (Android PiP via `MediaSession.Delegate` + activity PiP) | `MediaSession` | 3 |
| Media session / notification controls | yes — 0.11.0 | roadmap | `MediaSession` | 3 |
| Autoplay control | no | done (0.4.0: per site, muted allowed and audible blocked by default; not run on a device) | `PERMISSION_AUTOPLAY_*` | 3 |
| DRM (Widevine) | partial — L3 opt-in | roadmap (Widevine L1/L3 through Gecko's GMP) | — | 3 |

## 16. AI agent, scripts, VPN, updates, default browser

| Feature | Windows Krunk | Android 0.1.0 | Engine | Round |
|---|---|---|---|---|
| AI agent (BYO provider) | yes — 0.12.0 | roadmap | — | 5 |
| Userscripts, macros | yes — 0.12.0 | roadmap (userscripts via a bundled WebExtension) | — | 5 |
| VPN (WireGuard/OpenVPN, Krunk-only) | yes — 0.14.0 | **yes (0.10.0):** Settings › Privacy & security › VPN; WireGuard and OpenVPN in user space inside Krunk's process (no `VpnService`, other apps unaffected), engine routed by a `krunk-guard` channel filter (SOCKS5 with a per-session credential), Krunk's own requests through the same proxy; fail-closed; Everywhere / Private tabs only; address-bar mark and sheet | — | done |
| Update channel (signed manifest) | yes — Updates | **yes (0.5.0, website APK only):** Settings › About › Check for updates; channel `android-arm64`, Ed25519-signed with the Windows update key; only when the user taps it (no start-up, timer or background check); size, SHA-256, package, versionCode and signing certificate checked before the system installer. The Play build has none (Play updates it); F-Droid later, if there is demand | — | done |
| Default browser flow | yes — 0.12.1 | roadmap (`RoleManager.ROLE_BROWSER` request) | — | 3 |
| Custom Tabs for other apps | n/a | roadmap (`CustomTabsService`) | — | 4 |

## 17. Platform integration

| Feature | Windows Krunk | Android 0.1.0 | Engine | Round |
|---|---|---|---|---|
| Open links from other apps (`VIEW` http/https, `SEND` text, web search) | yes — 0.12.1 | done (intent filters) | — | 1 |
| Share sheet (page, link) | no | done | — | 1 |
| Default browser (`APP_BROWSER` category) | yes | partial — selectable in Android settings; in-app prompt is round 3 | — | 3 |
| Backup / device transfer rules | n/a | done | — | 1 |
| Web notifications | yes | done (v) (0.4.0: shown as Android notifications in the "Site notifications" channel, named with the site; the tap returns to the site's tab. The page's own click handler after the tap was not observed on the emulator) | `WebNotificationDelegate` | 3 |
| Home-screen shortcuts / PWA install | no | roadmap (`WebAppManifest`) | — | 4 |
| Widgets (search widget) | n/a | roadmap | — | 4 |
| Protocol handlers (`registerProtocolHandler`) | no | roadmap | — | 4 |
| Installer / portable mode | yes | APK (`direct` flavour, arm64-v8a / x86_64 / universal) installed over the previous version; updates through Android's `PackageInstaller` (the user confirms; "install unknown apps" asked once). A Play bundle (`play` flavour, per-ABI splits) is built but not uploaded; portable mode n/a | — | done (Play upload pending the operator) |
