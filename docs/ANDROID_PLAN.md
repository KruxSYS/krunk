# Krunk for Android: plan

Status: proposal (no code yet). Goal: a Krunk for Android phones and tablets with the same
promises as the desktop version:
- ad and tracker blocking that also works on YouTube
- no telemetry, and no network traffic of Krunk's own except filter and update checks
- an encrypted password vault
- private tabs
- working browser games (pointer lock with a mouse, full screen)

## 1. Engine

| | Android System WebView | GeckoView (Mozilla) | Own Chromium build (fork) |
|---|---|---|---|
| **What it is** | The OS's shared Chromium (`android.webkit.WebView`, AndroidX WebKit) | Firefox's engine as a library (Maven, `org.mozilla.geckoview`) | Chromium's Android browser, patched and built by us |
| **Request blocking** | `shouldInterceptRequest`: sees subresource GETs and can answer them (an empty response blocks). It can't see request bodies, WebSocket, or service-worker traffic in every case. Krunk's `blocker` would run here. | WebExtension `webRequest` (blocking), the full Firefox API | Full: the network stack is ours |
| **Scriptlets at document start** | `WebViewCompat.addDocumentStartJavaScript` (needs `WebViewFeature.DOCUMENT_START_SCRIPT`, present in current WebView). It runs in matching frames, but one script per origin rule, so per-URL scriptlet selection has to happen inside the script. | uBlock Origin itself, as a built-in WebExtension (`WebExtensionController.ensureBuiltIn`): its own scriptlets, redirects, cosmetic filters and YouTube fixes, maintained by uBO | Possible, but we would have to rebuild what desktop Krunk does on CEF |
| **Pointer lock** | Not provided to WebView apps (as in Alloy CEF, the embedder gets no permission hook); a shim like desktop Krunk's, plus Android `View.requestPointerCapture`, would be needed | Implemented in Gecko; with a mouse on Android it still has to be **verified on a device** (Phase 0) | Chrome for Android's implementation |
| **Full screen** | `WebChromeClient.onShowCustomView` | `ContentDelegate.onFullScreen` | native |
| **Cookies / storage UI** | `CookieManager` can't list cookies (only `getCookie(url)`); `WebStorage` is limited and partly deprecated. A `krunk://storage` equivalent is not possible. | `StorageController.clearData*` per host/base domain; listing goes through the WebExtension `cookies` API | full |
| **Private tabs** | One profile per app by default; separate profiles only with recent WebView versions through AndroidX `ProfileStore` (feature-checked at runtime) | `GeckoRuntime` contexts and `usePrivateMode` per session | full |
| **Engine updates** | Google ships them through Play; we can't pin or patch | Mozilla releases every 4 weeks (ESR too). **We** must ship each one in time, as with CEF on Windows | We must merge and build every Chromium release: weeks of CPU time a year and a large build machine |
| **APK size** | ~5 MB (engine is in the OS) | +55–70 MB per ABI | +80–120 MB per ABI |
| **Licence** | Apache-2.0 (AndroidX) | MPL-2.0 (file-level copyleft; fine for an MIT app) | BSD + third parties |

### Recommendation: GeckoView
1. **Blocking that holds up on YouTube.** The point of Krunk's blocking is that it keeps
   working where ad sellers fight back. On GeckoView, uBlock Origin itself runs as a built-in
   extension, kept current by its authors, rather than our port of its pieces (desktop M4
   needed a scriptlet engine, redirect resources and a preprocessor). WebView's per-origin
   document-start scripts and its blind spots in request interception would be a permanent
   second-class version.
2. **Private tabs and site-data control** map onto GeckoView's API directly.
3. **Independence from Google's engine**, matching the desktop promise of no background
   contact with Google.
4. **Cost:** a 4-weekly GeckoView bump. It is mostly a version change in Gradle (the API is
   stable, with deprecations announced), and much less work than maintaining a Chromium fork.

Trade-offs, accepted:
- Pages render with Gecko, not Chromium. A few sites test only in Chrome; a user-agent
  override per site can help.
- The APK is about 70 MB.
- Desktop and Android use different engines, so behaviour bugs may differ.
- Pointer lock with a mouse is unverified; if Gecko lacks it, games stay touch-only on Android
  (stated as a limitation, as on mobile Firefox).

Rejected:
- **WebView:** blocking quality and storage control are too weak for Krunk's promises.
- **Chromium fork:** the build and security-merge burden is too high for a small team.

## 2. Reusing `core/` (Rust) through UniFFI
`core` builds for `aarch64-linux-android` and `x86_64-linux-android` (emulator) with the NDK.
`rusqlite` is bundled, and none of its dependencies are Windows-only. The Kotlin bindings
come from UniFFI (`uniffi` proc-macros on a thin `krunk_ffi` crate that wraps `core`), packed
as an AAR.

| Module | Android | Notes |
|---|---|---|
| `storage`, `history`, `bookmarks`, `frecency`, `netscape` | as is | the same SQLite schema, so a later sync/import shares the format |
| `urlclean` (tracking params, address-bar input) | as is | |
| `settings` | split | shared fields as is; desktop-only ones (Chromium switches, GPU) are hidden on Android |
| `vault`, `passgen`, `secret` | as is | crypto is portable. Desktop wraps the vault key with DPAPI; Android wraps it with an **Android Keystore** AES key (StrongBox when available, optionally user-auth-bound for biometric unlock) |
| `update` | as is | a new channel `android-arm64` in the same signed manifest format (only for APK distribution, see §5) |
| `filterlists`, `preprocess`, `blocker` | not needed at first | uBO does the work; `blocker` stays an option if uBO-as-extension must be replaced |
| `cookies` (third-party policy) | not needed | Gecko's Total Cookie Protection / `cookieBehavior` settings |
| `downloads`, `tabs`, `zoom`, `memory` | partly | the logic is reusable, but Android's lifecycle (process death, `onTrimMemory`) changes the policy; tabs are rewritten for Compose state |
| `protocol`, `assets`, `pages`, `layout`, `frame`, `keys`, `pointer_lock`, `sitedata`, `migrate` | no | desktop shell specifics |

## 3. User interface
- **Recommended: Kotlin + Jetpack Compose** for the browser chrome:
  - toolbar at the bottom (one-handed use), tab switcher grid, menus, settings, history,
    bookmarks and password screens;
  - Material 3, dark theme, edge-to-edge, predictive back.
- **Alternative (rejected): reuse the HTML UI** (`ui/`) in a privileged view. It saves screens,
  but desktop layouts and hover-based UI don't fit touch, and a privileged web UI in the same
  engine as web content repeats the origin-isolation work done on desktop. Some strings and the
  settings model (via `core`) are shared instead.
- **Autofill:** Krunk's vault offers logins in its own pages through the GeckoView
  `Autocomplete` delegate. Being a system-wide Android Autofill service is a later option.

## 4. Sync
Out of scope for this plan. Desktop has no sync either. A later design would reuse the SQLite
formats and end-to-end-encrypt with the vault's key scheme. Manual import/export of bookmarks
(Netscape HTML via `core::netscape`) and of the vault (encrypted file) covers moving devices.

## 5. Distribution and updates
| Channel | Self-update | Notes |
|---|---|---|
| Google Play | **not allowed** (Play policy forbids updating outside Play); Play updates the app | Play App Signing; review of the ad-blocking description; GeckoView size fine (AAB per-ABI splits) |
| F-Droid | F-Droid updates it | reproducible-build requirements; Gecko-based browsers are already on F-Droid (e.g. Fennec F-Droid) |
| Direct APK from our server | yes: `/updates/android-arm64/latest.json`, same Ed25519 key scheme as Windows, install via `PackageInstaller` (user confirms; needs "install unknown apps") | the APK signing key and the update key are separate secrets with separate custody |

Proposal:
- **Play first** for reach, with the in-app update check off in that flavour.
- An **APK flavour** with the signed update check for users who avoid Play.
- F-Droid later if there is demand.

## 6. Milestones (one experienced Android/Rust developer)
| # | Milestone | Effort |
|---|---|---|
| A0 | Spike: GeckoView sample app with uBO as a built-in extension; check YouTube ads, document-start scriptlets, pointer lock with a USB/Bluetooth mouse, full screen, APK size, cold start on a mid-range phone | 1 week |
| A1 | App skeleton: Compose chrome, tabs (GeckoSession per tab, session restore), address bar with `core::urlclean` via UniFFI, private tabs (separate runtime context) | 3 weeks |
| A2 | Data: history, bookmarks, new-tab top sites (frecency), settings through `core` | 2 weeks |
| A3 | Blocking: bundled uBO (pinned version, hash-checked like `ubo-resources.lock`), shield counter and per-site allow via the extension's messaging, uBO list updates | 2 weeks |
| A4 | Vault: Keystore-wrapped key, unlock (password, optional biometrics), save/fill prompts through GeckoView autocomplete delegates | 3 weeks |
| A5 | Downloads, site data/cookies screen, permissions prompts, full screen and pointer lock polish | 2 weeks |
| A6 | Release: signing, Play listing and APK channel with `update` (android-arm64), crash-free QA on 4–5 devices (Android 10–16, phone + tablet, arm64 + x86_64 emulator) | 2–3 weeks |

Total: about **15–16 weeks** to a first public beta, then a GeckoView bump every 4 weeks (about
½–1 day each, more when APIs change).

## 7. Open questions
1. **Distribution:** Play, APK, F-Droid, or a mix (§5)? This decides whether `update` ships on
   Android at all.
2. **Minimum Android version:** recent GeckoView releases no longer support Android 5–7, so
   API 26 (Android 8) is the likely floor anyway; confirm in A0 against the pinned GeckoView.
3. **ABIs:** arm64-v8a only (smaller), or also armeabi-v7a for old devices?
4. **uBO as a built-in extension:** confirm with uBO's licence (GPL-3.0) that bundling it in the
   APK is fine. It is GPL software shipped unmodified next to an MIT app, as on desktop with its
   lists; we must publish its source and version.
5. **Pointer lock:** if A0 shows Gecko on Android lacks it with a mouse, is touch-only acceptable
   for games on Android?
6. **Shared account/sync:** wanted in a later round?
7. **Branding/package name:** `org.krunk.browser`? It needs a domain the project controls,
   which is also needed for `UPDATE_BASE_URL`.
