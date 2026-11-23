# Krunk for Android: plan

Status: **in progress** (0.5.0 delivered milestone A6: the website APK updates itself on request,
a Play build is ready but not uploaded; round 1 delivered Android 0.1.0 and 0.1.1: toolchain, engine spike, core
browsing, tabs, new-tab page, bookmarks/history, settings; round 2 so far delivered 0.2.0: ad and
tracker blocking with uBlock Origin (milestone A3), a quiet engine, themes, and 0.3.0: the
password vault (milestone A4), with save and fill in pages and optional fingerprint unlock; see
`docs/ANDROID_FEATURES.md` for the parity
matrix and `docs/ANDROID_DEV.md` for the build). Goal: a Krunk for Android phones and tablets with
the same promises as the desktop version:
- ad and tracker blocking that also works on YouTube
- no telemetry, and no network traffic of Krunk's own except filter and update checks
- an encrypted password vault
- private tabs
- working browser games (pointer lock with a mouse, full screen)

## 0. Decisions taken in round 1 (Android 0.1.0)

| Decision | Value |
|---|---|
| Engine | **GeckoView 160.0.20261104181552** (`org.mozilla.geckoview:geckoview-omni`, release channel). Verified in the sandbox emulator: pages render, `ensureBuiltIn` installs a bundled extension, content-script native messaging works. A bump every 4 weeks is a one-line change in `android/gradle/libs.versions.toml` + `android/toolchain.lock`. |
| Package name | `org.krunk.browser` (confirmed by the operator after round 1) |
| Versions | app `0.1.0` / `versionCode 1`, independent of the Windows version (core is 0.15.3) |
| SDK levels | minSdk **26** (also GeckoView's minimum), targetSdk **35**, compileSdk **37.2** (required by GeckoView 160 and Compose 2026.09) |
| ABIs | `arm64-v8a` (devices) and `x86_64` (emulator); per-ABI APKs plus a universal one. No `armeabi-v7a` (assumed). |
| UI | Kotlin + Jetpack Compose, Material 3 with Krunk's own palette (no dynamic colour), toolbar at the top by default since 0.1.1 (bottom as an option; 0.1.0 defaulted to the bottom) |
| Core reuse | `core` via UniFFI 0.32 (`ffi/` crate, proc-macros): storage (same SQLite schema), omnibox/urlclean, search suggestions, settings (same JSON, validated by the core), tabs model, memory policy, frecency, Netscape import/export, user agents, wallpapers |
| Data layout | `filesDir/krunk/browser.db`; Gecko profile at GeckoView's default under `filesDir`; Android-only settings in `kv["android.settings"]`; session in `kv["session"]` (Windows shape) plus `android.gecko_states`, `android.closed_tabs`, `android.clean_exit`; no cloud backup of Krunk's data since 0.1.1 (device-to-device transfer keeps `files/krunk`; the Gecko profile is never included) |
| HTTPS | Gecko's HTTPS-First is on by default (`http://` upgrades automatically); the shared `https_only` setting maps to `allowInsecureConnections = HTTPS_ONLY` |
| Favicons | a bundled WebExtension (`krunk-icons`) reports `<link rel=icon>`; the app fetches one icon per host (user-triggered page loads only) into the shared `favicons` table |
| Play Services | GeckoView depends on `play-services-fido` (passkeys/WebAuthn). **Kept** (operator decision); `-Pkrunk.noGms=true` builds without it for devices without Google Play services (passkeys then unsupported). |

### Decisions taken for Android 0.2.0 (A3, 2026-11-11)

Decided by the development director at the plan review on the operator's behalf; the first and the
last change what users get and should be confirmed by the operator.

| Decision | Value |
|---|---|
| Blocker | **uBlock Origin 1.76.0**, the project's signed Firefox release, built in **unmodified** as a GeckoView extension (`ensureBuiltIn`). Pinned in `android/ubo.lock` (version, file address, SHA-256); the build fails on a hash mismatch and compares every file in the APK with the release. Windows stays on its own pin (`assets/filters/ubo-resources.lock`, 1.75.4) |
| How Krunk drives it | A second built-in extension of Krunk's own, `krunk-guard`. Built-in extensions are privileged, so it may use experiment APIs: a request observer that cancels uBlock Origin's own requests unless the user started an update (the gate), and a bridge that sends uBlock Origin's own messages from one of its own pages (ready, allow list, update, settings). Nothing is written into uBlock Origin's files, pages or storage. Managed storage, external messaging and content scripts in its pages do not exist on Android (spike) |
| Fail closed | The guard is installed and ready before the blocker is installed; without a working guard the blocker is never enabled and blocking is shown as unavailable. `a3.sh gate` checks this at every uBlock Origin or GeckoView bump |
| Engine background traffic | **All off by default** (decision "A+"): Mozilla Remote Settings (`MOZ_REMOTE_SETTINGS_DISABLED`), the connectivity and captive-portal checks, Safe Browsing list updates, the add-on blocklist, the region lookup. Applied through an engine config file. 0.1.0 and 0.1.1 did send these. Cost: no CRLite or intermediate-certificate updates, no Safe Browsing |
| "Update automatically" | One switch in Settings › Privacy, **off by default**: daily filter-list updates and Mozilla Remote Settings together, from the next start. Safe Browsing and the connectivity check stay off in every case |
| Filter-list updates | Only when the user asks (Settings › Privacy) or with the switch above; the lists inside the package work without any network |
| Start | Page loads wait until the blocker filters (measured 2 to 3 s on the emulator, 6 to 7 s on the first start after install or upgrade), at most 10 s; then they load and the shield warns. Krunk's own screens appear at once |
| Per-site allow | Stored in the shared `adblock_allowlist` table and written to uBlock Origin's trusted sites. **No allow switch in private tabs** (the sheet shows the count); "Open once" on a blocked page is allowed there (in memory, 2 minutes) |
| Shield | Inside the address field, with the count; Home sits in the menu's first row on phones |
| Tracking parameters | Request-level rewriting in `krunk-guard` (`webRequest`, top-level GET only), rules from `core::urlclean` |
| Themes | The twelve styles of `core::style` and the accent, resolved in `ffi/src/theme.rs`; shape and active-tab mark are mapped, a gradient runs along the toolbar, private tabs keep one fixed look |
| 0.1.x | No separate fix release: the 0.2.0 changelog opens with the plain statement that 0.1.0 and 0.1.1 contacted Mozilla and Google in the background |

### Decisions taken for Android 0.3.0 (A4, 2026-11-13)

Taken at the plan review of 2026-11-13 as assumptions the operator can overrule; the lock after
leaving Krunk was changed by the operator during the work. The spike that the design rests on ran
on the sandbox emulator before any feature code was written.

| Decision | Value |
|---|---|
| Biometric unlock | **Optional, off by default**, in addition to the vault password, which always works. Switching it on encrypts the vault's unlock key (never the password) under a second Keystore key that works only after a strong biometric match and is invalidated by a new enrolment. A new enrolment, a changed vault password or a missing blob switch it off, and the password dialog says why |
| Import of logins | **None** in 0.3.0 (not part of A4). No export either |
| Lock timer | 1, 5, 15, 30 or 60 minutes without vault use, **default 15**, as on Windows. An **Android-only setting** (`kv` row `android.vault.lock_minutes`), not the shared `vaultAutolockMinutes`: the settings record the Android app gets from the core does not carry that field |
| Lock after leaving Krunk | **Operator's change (2026-11-13):** not at once but after a grace period, so that a visit to an authenticator app does not lock the vault. Choices: at once, 30 seconds, **1 minute (default)**, 5 minutes (`kv` row `android.vault.background_grace_s`). Back within the time: still unlocked; back later: locked before anything of the vault is shown; a best-effort timer also locks while Krunk stays in the background. The vault's windows stay blank in Recents throughout. Cost: the vault is open for up to that time after the user left; "At once" is selectable |
| Private tabs | Nothing is offered by itself and there is never a save offer. The menu item **"Fill from vault"** arms the tab for the page's origin; the next tap on the sign-in field opens the picker. (As built the item is in the menu of every tab with a vault: in a normal tab it brings back a picker the user closed) |
| Forms in frames of another origin | **Not filled.** Filling is tied to the tab's top-level address, and a form whose origin differs from it gets nothing. Stricter than Windows |
| Passwords and the engine | **Withheld until the user picks one.** GeckoView facts from the spike: with `loginAutofillEnabled(false)` the engine never fills by itself but still calls the runtime-wide `Autocomplete.StorageDelegate.onLoginFetch(domain)` on page load and at every field tap, without saying which tab asks; a tap then brings `PromptDelegate.onLoginSelect(session, …)` with the tab known; confirming a self-built `LoginSelectOption` fills the form. Krunk therefore answers the fetch with entries that carry a placeholder instead of the password (one per matching login with the vault open, one stand-in with the vault locked and logins known for the site), and hands the real password of the one picked login to the engine in the tab's own prompt. Three things the spike added: the entries must carry the page's origin including the port; the locked stand-in needs a user name; the engine also asks to save in private tabs, so `signon.privateBrowsingCapture.enabled` is off and Krunk refuses there as well. `onLoginSave(session, …)` also arrives after a fill, so Krunk compares with the vault itself (new, update, unchanged). The engine's password generator and Relay offer are off |
| Keystore wrap | The vault key, already sealed by the password, is wrapped by an AES-256-GCM key in the Android Keystore (`krunk.vault.wrap.v1`, no user authentication, **StrongBox first**, otherwise the TEE-backed key). The key is made **only by "Create vault"**. **What the wrap protects** (corrected by the security review, finding R-1): the saved logins, because a copied file cannot be decrypted on another device even with the right password. **What it does not protect:** the password against guessing. `vault.json` holds a password check record sealed by the password-derived key alone, so someone with a copy of the file can try passwords offline, slowed only by Argon2id (64 MiB, 3 passes per guess); hence the advice to choose a long vault password. **Key loss:** only a key the key store reports as missing or invalidated makes the vault "key lost" ("This vault cannot be opened", shown before a password is asked; the file stays as it is; the only way on is "Delete this vault" behind a typed confirmation). A key store that merely fails to answer is reported as "key store did not answer, try again" (error `KeyUnavailable`), not as key loss. A key or a vault is **never recreated silently** |
| File location | `filesDir/krunk-vault/` (`vault.json`, `state.json`): beside `files/krunk`, not in it, and excluded by name from cloud backup and from device-to-device transfer in both rule files. `BackupRulesTest` holds the rules and the path constant together. Even a copy that travelled could not be decrypted (its key stays behind) |
| Fixtures both ways | A DPAPI or Keystore blob opens only where it was made, so each platform has a pair in `core/tests/fixtures/vault/platform/`: the file as written (Krunk 0.19.0 on the Windows VM; the Android debug build on the emulator with the real Keystore) and an inner twin with only the platform wrap removed on that machine. The `ffi` tests open both twins with the fixture password and find the same logins, and check that the files as written fail cleanly elsewhere. Vaults of formats 2, 3 and 4 keep what the Android app does not show (cards, recovery slot, sync part) |
| Biometric library | The **platform** `BiometricPrompt` and `BiometricManager.canAuthenticate(BIOMETRIC_STRONG)`, offered on **Android 11 and later** (API 30). **No AndroidX Biometric:** its only stable release (1.1.0) needs a `FragmentActivity`, and 1.4.0 is a beta. No new dependency; one new normal permission (`USE_BIOMETRIC`) |

Not run on a device: fingerprint unlock (no fingerprint can be simulated on the sandbox emulator)
and StrongBox (the image has none); both are covered by JVM tests with stand-ins only.

Spike measurements (sandbox emulator, Android 16 x86_64, software GPU, 4 vCPU):
- APK (debug, unstripped Gecko): arm64-v8a 153 MB, x86_64 161 MB, universal 274 MB. `libkrunk_ffi.so`
  (debug, stripped) 8.6 MB per ABI; release is smaller. Most of the size is `libxul.so` (155–167 MB
  uncompressed) and `omni.ja` (15 MB).
- Cold start: first launch after install 12–16 s (profile creation); later cold starts 1.1 s to the
  first frame, GeckoRuntime ready after ~100 ms, `https://example.com` loaded ~6 s after process start.
- `ensureBuiltIn` of a bundled extension: works (installed once, persists across restarts).
- `http://example.com` → `https://example.com/` without any Krunk code (HTTPS-First).

### Decisions taken for Android 0.5.0 (A6, 2026-11-14)

Defaults from §5 stated at the plan review (`.pipeline/plan-android-0.5.0.md`); the operator may
still override them.

| Decision | Value |
|---|---|
| Channels | **Website APK** (`direct` flavour) with a signed in-app update check: published. **Google Play** (`play` flavour): AAB built and listing drafted (`android/play/`, `dist/play/`), **nothing uploaded**. **F-Droid:** later, if there is demand |
| Update manifest | Channel `android-arm64`, `/updates/android-arm64/latest.json` + `.sig`, signed by `publish-android.sh` with the **existing Windows Ed25519 update key** (same scheme; its public half is compiled into the app). Lists every ABI with URL, size and SHA-256; the app takes its own ABI, the universal APK only as a fallback. Notes are inline |
| When the app contacts the server | Only on the user's tap on **Check for updates** (manifest + signature) and on **Download and install** (one APK). No start-up check, timer, WorkManager or push; no "check automatically" option. User-Agent `Krunk-Android/<version>`, nothing else |
| Before the installer | Size and SHA-256 from the signed manifest; package `org.krunk.browser`; exactly the manifest's versionCode; the installed app's signing certificate, which must also be the manifest's. Then `PackageInstaller` (session), the user confirms; "install unknown apps" explained first. Same or lower versionCode = up to date |
| Play signing | **Not decided** (operator, q10 below). The AAB is signed with the existing release key as a local artefact; no upload key was made |
| ABIs | `arm64-v8a`, `x86_64` and universal; **no `armeabi-v7a`** (q3 closed) |
| `UPDATE_BASE_URL` | `https://krunk.kruxsys.com` (q7 closed). Only the `qa` build type (versionCode 6, never published) and debug builds take another address, for the update tests |
| Versions | 0.5.0 = versionCode 7; `0.5.0-qa` = 6 |
| Upgrade from 0.4.0 | 0.4.0 has no update check: users install the 0.5.0 APK over it once (said in the changelog and on the site) |
| QA images | A: the hosted emulator (API 36, x86_64). B: a local software-emulated AVD (API 29, x86_64, no KVM), time-boxed; results in `docs/ANDROID_DEV.md` "QA images (0.5.0)" |

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
| `vault`, `passgen`, `secret` | `vault` as is (0.3.0); `passgen` not used yet | crypto is portable: the same scheme and file format. Desktop wraps the vault key with DPAPI; Android wraps it with an **Android Keystore** AES-256-GCM key (StrongBox when available, otherwise TEE-backed; no user authentication, the password guards the vault). As built: two additive core functions (`Vault::unlock_key`, `Vault::open_with_key`) for biometric unlock, where a second, user-authentication-bound Keystore key encrypts the unlock key; the `ffi` object `KrunkVault` with the foreign trait `VaultKeyWrapper`; a side file `state.json` (unlock rate limiter, the locked-vault hint index, the biometric blob). The file as written opens only on the device that made it; the wrap protects the content of a copied file, not the password against offline guessing (see the 0.3.0 decisions). Not on Android yet: cards, the recovery phrase, the generator |
| `update` | as is | a new channel `android-arm64` in the same signed manifest format (only for APK distribution, see §5) |
| `filterlists`, `preprocess`, `blocker` | not needed at first | uBO does the work; `blocker` stays an option if uBO-as-extension must be replaced |
| `cookies` (third-party policy) | not needed | Gecko's Total Cookie Protection / `cookieBehavior` settings |
| `downloads`, `tabs`, `zoom`, `memory` | partly | the logic is reusable, but Android's lifecycle (process death, `onTrimMemory`) changes the policy; tabs are rewritten for Compose state |
| `protocol`, `assets`, `pages`, `layout`, `frame`, `keys`, `pointer_lock`, `sitedata`, `migrate` | no | desktop shell specifics |

## 3. User interface
- **Recommended: Kotlin + Jetpack Compose** for the browser chrome:
  - toolbar at the top by default, at the bottom as an option (one-handed use), tab switcher grid, menus, settings, history,
    bookmarks and password screens;
  - Material 3, dark theme, edge-to-edge, predictive back.
- **Alternative (rejected): reuse the HTML UI** (`ui/`) in a privileged view. It saves screens,
  but desktop layouts and hover-based UI don't fit touch, and a privileged web UI in the same
  engine as web content repeats the origin-isolation work done on desktop. Some strings and the
  settings model (via `core`) are shared instead.
- **Autofill** (as built in 0.3.0): Krunk's vault offers logins in its own pages through
  GeckoView's `Autocomplete` storage delegate and the login prompts of `PromptDelegate`. There is
  **no silent fill**: the engine's own filling is off, the engine hears placeholders instead of
  passwords, and a login is filled only after the user tapped the field and picked it in Krunk's
  own picker. Being a system-wide Android Autofill service is a later option.

## 4. Sync
Desktop has end-to-end encrypted sync since 0.9.0 (`docs/sync/`). **Android has it since 0.6.0**,
through the same `core::sync` code (UniFFI object `KrunkSync`, `ffi/src/sync/`), so the keys, the
directories, the merge and the conflict rules are the shared code; Kotlin only supplies the HTTPS
transport (`SyncHttp`), the Keystore key and the screen.

As built in 0.6.0 (round 4, plan `.pipeline/plan-android-sync.md`):
- **Sign-in is the device link** (PROTOCOL §6.1, platform `android`): the app shows a code and
  opens the approval page in a tab; the user approves with the account password on the website.
  The keys then come from a device that syncs (6 digits typed on the phone, §6.2), from the sync
  passphrase or recovery key (§6.3; one escrow fetch per attempt, a wrong secret retried
  locally), or, for an account that doesn't sync yet, the phone is the first device (§6.4).
- **What syncs:** what Windows syncs, nothing new: bookmarks, history, settings (the 20
  allowlisted keys), open tabs, passwords and the never-save list (vault unlocked), new tab
  pins, ad-blocker exceptions, device names. Payment cards are not synced on either platform.
  Android-only settings (`kv["android.settings"]`: toolbar position, text size …) stay on the
  phone (decision D2, for the operator to confirm).
- **Storage:** `files/krunk-sync/account.json` and `sync.json` (the core's formats), wrapped by
  the Keystore key `krunk.sync.wrap.v1`; excluded from every backup; a `browser.db` copied by a
  device transfer forgets its sync bookkeeping at the first start.
- **Schedule:** in the foreground only (a minute after start, every 5 minutes, 30 s after a local
  change, Sync now); no background work, no request before sign-in.
- **Sign-out and a device removed elsewhere** delete the tokens, the keys (also the vault's sync
  part) and the bookkeeping; local data stays unless the user also removes what came from sync.
- **Not on Android yet:** approving or removing other devices, Finish securing after a removal,
  the QR claim (`link/pair/claim`): done on Windows or on the website.

## 5. Distribution and updates
| Channel | Self-update | Notes |
|---|---|---|
| Google Play | **not allowed** (Play policy forbids updating outside Play); Play updates the app | Play App Signing; review of the ad-blocking description; GeckoView size fine (AAB per-ABI splits) |
| F-Droid | F-Droid updates it | reproducible-build requirements; Gecko-based browsers are already on F-Droid (e.g. Fennec F-Droid) |
| Direct APK from our server | yes: `/updates/android-arm64/latest.json`, same Ed25519 key scheme as Windows, install via `PackageInstaller` (user confirms; needs "install unknown apps") | the APK signing key and the update key are separate secrets with separate custody |

State (0.1.0): the signed APK is a direct download on the website (`/download/android/…`,
`/download/android.json`); it does not update itself yet.

Proposal:
- **Play first** for reach, with the in-app update check off in that flavour.
- An **APK flavour** with the signed update check for users who avoid Play.
- F-Droid later if there is demand.

## 6. Milestones (one experienced Android/Rust developer)
| # | Milestone | Effort |
|---|---|---|
| A0 ✔ (round 1, without uBO) | Spike: GeckoView sample app with uBO as a built-in extension; check YouTube ads, document-start scriptlets, pointer lock with a USB/Bluetooth mouse, full screen, APK size, cold start on a mid-range phone | 1 week |
| A1 ✔ (round 1) | App skeleton: Compose chrome, tabs (GeckoSession per tab, session restore), address bar with `core::urlclean` via UniFFI, private tabs (separate runtime context) | 3 weeks |
| A2 ✔ (round 1) | Data: history, bookmarks, new-tab top sites (frecency), settings through `core` | 2 weeks |
| A3 ✔ (0.2.0) | Blocking: bundled uBO (pinned version, hash-checked like `ubo-resources.lock`), shield counter and per-site allow via the extension's messaging, uBO list updates on request. Custom lists and protection levels moved to round 3 | 2 weeks |
| A4 ✔ (0.3.0) | Vault: Keystore-wrapped key, unlock (password, optional biometrics), save/fill prompts through GeckoView autocomplete delegates, saved logins in the HTTP sign-in dialog. The recovery phrase, the password generator and payment cards moved to a later round; so did import/export and a system Autofill service | 3 weeks |
| A5 ✔ (0.4.0) | Downloads (Krunk's own, from the engine's response stream; pause/resume where the server allows it), site data/cookies screen, permissions prompts and per-site list, web notifications, full screen polish. Pointer lock is not available on Gecko Android (checked with a mouse on the emulator); multi-connection downloads moved to a later round | 2 weeks |
| A6 ✔ (0.5.0) | Release: signing, Play listing and APK channel with `update` (android-arm64), crash-free QA on 4–5 devices (Android 10–16, phone + tablet, arm64 + x86_64 emulator). **As done:** the website APK (`direct` flavour) has the signed, user-started update check; the `play` flavour is built (AAB) and its listing drafted, **not uploaded** (the Play signing choice is the operator's); QA ran on **emulator images only** (see "QA images" in the 0.5.0 decisions), no real devices; F-Droid deferred | 2–3 weeks |

Total: about **15–16 weeks** to a first public beta, then a GeckoView bump every 4 weeks (about
½–1 day each, more when APIs change).

## 7. Open questions
1. ~~Distribution~~ decided for 0.5.0: the website APK with the in-app update check is the live
   channel; a Play build exists but is not uploaded; F-Droid later, if there is demand. Still the
   operator's: whether and when to go to Play (q10).
2. **Minimum Android version:** recent GeckoView releases no longer support Android 5–7, so
   API 26 (Android 8) is the likely floor anyway; confirm in A0 against the pinned GeckoView.
3. ~~ABIs~~ closed in 0.5.0: `arm64-v8a` for devices, `x86_64` for the emulator, plus a universal
   APK; no `armeabi-v7a`.
4. ~~uBO as a built-in extension~~ closed with 0.2.0: uBlock Origin (GPL-3.0) ships as its own
   signed release file, unmodified, as a separate program next to the MIT app (aggregation, as on
   desktop with its lists); Krunk talks to it only through its messages. Its version and source
   are published: the release is named in `android/ubo.lock`, the app's About screen and
   `android/THIRD-PARTY-NOTICES.md` list it with its licence and the lists inside it, the licence
   text is in the APK, and the build proves the APK's copy equals the release. One note for the
   operator: Peter Lowe's list inside it asks for non-commercial use.
5. **Pointer lock:** checked in 0.4.0: GeckoView 160 on Android reports a lock to the page but
   delivers no mouse movement while locked and offers no pointer-capture API. Touch-only for
   games on Android until the engine has it; is that acceptable, or should it be raised upstream?
6. ~~Shared account/sync~~ answered in 0.6.0: yes, built in round 4 with the shared core
   (§4). Open for the operator: whether the Android-only settings should ever sync (D2).
7. ~~Branding/package name~~ `org.krunk.browser` confirmed; ~~`UPDATE_BASE_URL`~~
   `https://krunk.kruxsys.com` since 0.5.0.
8. ~~Play Services FIDO~~ kept (passkeys); `-Pkrunk.noGms=true` documented for devices without
   Google Play services.
9. **Feature list wording:** confirm/supply the original 17-category list wording so the matrix
   headings match 1:1 (the rows re-key mechanically).
10. ~~Release signing~~ done for 0.1.0 (2026-11-11): the APKs are signed with Krunk's Android
    release key (custody and fingerprint in `docs/ANDROID_DEV.md`) and offered on the website
    as a direct download (`dist/server/publish-android.sh`), without an update check.
    The website channel and its update check are done (0.5.0). **Still open, the operator's
    decision: Play signing.** (a) This key becomes Play's app-signing key: one certificate
    everywhere, users can move between Play and the website APK. (b) A new upload key, and Play
    makes its own app key: the website APK and the Play install then have different certificates
    and cannot update each other (switching means uninstalling, which loses the data, including
    the vault). Until decided, the Play build stays "built, not uploaded" (`android/play/CHECKLIST.md`).
11. **Google-dependent features** (Lens, Discover feed, Google Password Manager, Safe Browsing):
    marked "not planned" in the matrix with alternatives; override if wanted.
12. **Tablet coverage:** round 1 verified the sw600dp layout by resizing the phone emulator; a real
    tablet AVD needs KVM (absent in the sandbox).
13. **Recovery phrase on Android** (open since 0.3.0): the vault format has the recovery slot and
    Windows has the screens, Android has neither the screen to make a phrase nor the one to use
    it. Until then a forgotten vault password cannot be recovered on Android, and the app says
    so. Wanted, and in which round?
14. **Carrying a vault between devices** (open since 0.3.0): the vault file is bound to the
    device's Keystore key, is in no backup or transfer, and there is no import or export, so a
    new phone starts with an empty vault and a Windows vault does not open on Android. The
    intended answer is sync (§4, question 6); an export/import file would be the smaller step.
    Which one, and when?
15. **System Autofill service** (open since 0.3.0): Krunk fills in its own pages only. Offering
    the vault to other apps through Android's Autofill framework is a separate piece of work
    with its own security review. Wanted?
