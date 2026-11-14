# Krunk for Android: developer guide

How to set up the toolchain, build the APK, run it on the emulator, run the tests, and where the
evidence goes. The product description is in `README.md` ("Krunk for Android (preview)"); the
plan and decisions are in `docs/ANDROID_PLAN.md`; the parity matrix is `docs/ANDROID_FEATURES.md`.

## Layout

| Path | What |
|---|---|
| `android/` | Gradle project (Kotlin, Jetpack Compose, Material 3). `app/` is the only module. |
| `android/app/src/main/kotlin/org/krunk/browser/` | `KrunkApp` (process state), `MainActivity`, `core/` (store + settings on the Rust core), `engine/` (GeckoView runtime, session delegates, error page, the icons extension), `tabs/` (tab manager on `core::tabs`, persistence, favicons, thumbnails, memory policy), `vault/` (the password vault: Keystore wrapper, lock controller, biometric unlock; see "Vault"), `ui/` (Compose screens: browser, tabs, new tab, bookmarks, history, settings, theme, `ui/vault/` for the Passwords screen). |
| `android/app/src/main/assets/` | `error.html` (Krunk's error page), `extensions/krunk-icons/` (bundled WebExtension), `wallpapers/` (copied from `ui/shared/wallpapers` at build time, git-ignored). |
| `android/scripts/` | `setup-sdk.sh` (toolchain), `build-rust.sh` (cargo-ndk + UniFFI bindings; Gradle calls it), `update-ubo.sh` (the pinned uBlock Origin release; Gradle calls it), `emu.sh` (emulator driver), `flows.sh` (the 0.1.x adb flows), `a3/` (the device checks since 0.2.0: `a3.sh`, its test server and pages), `a4/` (the vault's device checks, 0.3.0: `a4.sh` and its login page, on the `a3` helpers and server), `a5/` (downloads, permissions, site data, full screen, 0.4.0: `a5.sh`, its own test server and pages). |
| `android/ubo.lock` | The uBlock Origin release built into the app: version, release file, SHA-256. The only place that names it. |
| `android/app/src/main/assets/extensions/` | `krunk-icons/` (site icons), `krunk-guard/` (Krunk's extension that gates and drives the blocker and rewrites tracking parameters), `krunk-sitedata/` (lists the sites with cookies and stored data for Settings → Site data), `ublock0/` (uBlock Origin, unpacked from the pinned release at build time, git-ignored). |
| `android/THIRD-PARTY-NOTICES.md` | Components and licences; kept equal to the About screen by `LicensesTest`. |
| `android/toolchain.lock` | Every pinned version (SDK packages, NDK, Rust tooling, Gradle/AGP/Kotlin/Compose/GeckoView). |
| `ffi/` | `krunk_ffi`: the UniFFI crate over `core` (workspace member; `cargo test -p krunk_ffi`). |
| `dist/android/` | Built APKs + `SHA256SUMS` (git-ignored, attached to releases). |

## Toolchain

```
android/scripts/setup-sdk.sh            # idempotent: SDK platform 37.2, build-tools 37, NDK r30,
                                        # Rust targets, cargo-ndk; writes android/local.properties
android/scripts/setup-sdk.sh --check    # verify only
android/scripts/setup-sdk.sh --with-local-emulator   # + emulator, API 35 default x86_64 image, AVD
```

Requirements: `ANDROID_HOME` with cmdline-tools, JDK 17+ (21 is used), rustup. The versions:
see `android/toolchain.lock`. `compileSdk` is 37.2 because GeckoView 160 and Compose 2026.09
require it; `targetSdk` is 35 and `minSdk` 26 (GeckoView's own minimum).

The UniFFI bindings generator is built from the workspace (`ffi/src/bin/uniffi-bindgen.rs`,
feature `cli`), so it always matches the `uniffi` crate version.

## Build

```
cd android
./gradlew assembleDebug        # debug APKs: app/build/outputs/apk/debug/app-{arm64-v8a,x86_64,universal}-debug.apk
./gradlew assembleRelease      # minified (R8), signed with the release key when it is configured (below)
```

`cargoBuildDebug` / `cargoBuildRelease` run `android/scripts/build-rust.sh <profile>`: a host build
of `krunk_ffi` (bindings + JVM tests), `uniffi-bindgen generate --library`, then `cargo ndk` for
`arm64-v8a` and `x86_64` into `app/build/rust/<variant>/`. Debug libraries are stripped
(`llvm-strip`) because AGP keeps debug variants' symbols. The bindings are always generated from
the **debug** host library: the workspace's release profile strips the UniFFI metadata.

Knobs: `CARGO_BUILD_JOBS` (default 4), `KRUNK_ABIS` (default `arm64-v8a,x86_64`),
`CARGO_TARGET_DIR`, `-Pkrunk.noGms=true` (drops GeckoView's Play Services FIDO dependency).
Gradle is capped at 3 GB heap and 4 workers (`gradle.properties`) because the build host is shared.

Checks:

```
./gradlew lint ktlintCheck testDebugUnitTest       # Android lint, ktlint (pinned CLI), JVM tests
cargo clippy -p krunk_ffi --all-targets -- -D warnings && cargo fmt --check && cargo test -p krunk_ffi
```

The JVM unit tests load the real `libkrunk_ffi.so` (host build) through JNA
(`jna.library.path` is set in `app/build.gradle.kts`), so they exercise the Rust core directly.

## uBlock Origin: the pin and how to move it

Krunk for Android blocks with uBlock Origin itself, built in as a GeckoView extension. It is the
project's signed Firefox release file, unpacked and otherwise untouched.

- **The pin** is `android/ubo.lock`: `UBO_VERSION`, `UBO_ID`, `UBO_XPI` (the release file's
  address) and `UBO_SHA256`. Nothing else in the repository names the version: the build puts it
  into `BuildConfig` (`UBO_VERSION`, `UBO_ID`, `UBO_SOURCE_URL`), the About screen and the unit
  tests read it from there.
- **The build** (`app/build.gradle.kts`): `verifyUbo` runs `android/scripts/update-ubo.sh --check`
  (downloads the file into `~/.cache/krunk/ubo/<version>/` if it is missing and compares its
  SHA-256 with the lock; a mismatch fails the build), `unpackUbo` syncs the archive into
  `app/src/main/assets/extensions/ublock0/` (anything changed or added there by hand is undone),
  and `checkUboInApk<Variant>` compares every file under that folder in each built APK with the
  archive, byte for byte, and fails on any difference or missing file. `androidResources.
  ignoreAssetsPattern` is set so that aapt keeps the `_locales` folders.
- **Unmodified** means exactly that. Configuration goes through uBlock Origin's own messages,
  sent by `krunk-guard`; do not edit, add or remove files in its folder, and do not write into
  its storage.

**Moving to a newer release**

1. In `android/ubo.lock` set `UBO_VERSION` and `UBO_XPI` to the new signed Firefox release
   (`https://github.com/gorhill/uBlock/releases`, the `…firefox.signed.xpi` asset).
2. `android/scripts/update-ubo.sh --pin` downloads it and writes its SHA-256 into the lock.
   Compare that digest with the one GitHub shows for the release asset.
3. Read the release notes and, in the unpacked package, check the things Krunk relies on: the
   page `no-dashboard.html`, the messages `readyToFilter`, `userSettings`, `getWhitelist`,
   `setWhitelist`, `updateNow`, `getLists` (channel `dashboard`) and
   `temporarilyWhitelistDocument` (channel `documentBlocked`), the toolbar title format
   `uBlock Origin (N)` / `(off)`, and that the only remote requests are still list and asset
   downloads (no telemetry; `data_collection_permissions` in its manifest).
4. Build, then run the gate checks on the emulator with the debug build (below):
   `android/scripts/a3/a3.sh gate`. All of them must pass: 0 packets when idle and over cold
   starts, the allow list taken, a user-started update reaching only the lists' hosts, and the
   blocker never enabled without a working guard (`guardlost`, `noguard`).
5. If the lists inside the package changed, update the names and licences in
   `ui/settings/Licenses.kt` and `android/THIRD-PARTY-NOTICES.md` (`LicensesTest` compares them).
6. Commit the lock with the code changes it needed. The same checklist applies to a GeckoView
   bump: the gate and the bridge use engine internals that only built-in extensions may touch.

## The quiet engine

GeckoView on its own contacts Mozilla (Remote Settings, a connectivity check) and Google (Safe
Browsing list updates), also with Safe Browsing set to "off"; 0.1.0 and 0.1.1 shipped like that.
Since 0.2.0 `engine/EngineConfig.kt` writes a config file that the runtime reads at every start
(`GeckoRuntimeSettings.Builder.configFilePath`): the environment variable
`MOZ_REMOTE_SETTINGS_DISABLED=1` (left out when the user turned "Update automatically" on) and the
prefs that switch off the connectivity and captive-portal services, the Safe Browsing providers
and their lookups, the add-on blocklist and the region lookup. `EngineConfigTest` pins the
contents. Two traps: GeckoView parses the file with SnakeYAML, which R8 breaks without the keep
rules in `proguard-rules.pro` (the release build then crashes at start); and a capture that
shows 0 packets proves nothing unless the app is verifiably running in the foreground.

A profile that has synced Remote Settings before (every 0.1.x install; or one on which "Update
automatically" was on) needs one more step: parts of the engine read some collections with
signature verification at start, and with downloaded data in the store that fetches a certificate
chain from `content-signature-2.cdn.mozilla.net` before any extension runs. `GeckoEngine` therefore
removes the store (the `remote-settings` IndexedDB in the profile) before the runtime is created,
once per change: on the first start of a version that has the note file `krunk-engine.updates`
missing, and on the first start after automatic updates were switched off
(`EngineConfig.mustDropRemoteSettingsStore`). The upgrade test (`a3.sh upgrade`, with the last
published APK as `KRUNK_OLD_APK`) captures the first start after the upgrade and fails on any
packet that is not the restored page's.

The gate in `krunk-guard` is the second line: an observer on every request cancels uBlock Origin's
own requests unless an update the user started is running, and cancels requests the engine
itself starts outside a page. Krunk's own downloads go through `GeckoWebExecutor`, which loads
exactly like that (system principal, no tab): `GeckoFetcher` marks them with the header
`X-Krunk-Fetch: 1`, which the gate lets through and removes before the request leaves (since
0.4.0, guard 1.3; without it every link download, resume and restart fails with
`WebRequestError` `ERROR_UNKNOWN` and nothing reaches the network).

**Start order and the engine's late start-up.** Page loads wait until the guard has reported in
and the blocker filters (`engine/blocker/BlockerStart.kt`, at most 10 s). GeckoView starts the
background scripts of already installed extensions "late", when its first window has had an idle
moment; on about one cold start in eight that came only with the first page load, which Krunk was
holding, so the page waited the full 10 s. The guard's privileged start-up hook
(`gate-api.js`, `onStartup`) therefore starts its own background at once with the engine's
`wakeupBackground()`; uBlock Origin's background is woken by the bridge's first message.
`a3.sh gate startlinks` (eight cold starts by a link, with the time each) is the check.

## Device checks (`android/scripts/a3/`)

```
python3 -I android/scripts/a3/server.py android/scripts/a3/www 18080 >> "$KRUNK_A3_ROOT/server.log" &
android/scripts/a3/a3.sh gate            # the blocker's gate and start order (see the script header)
android/scripts/a3/a3.sh ui | links | prompts | themes | small
REL=1 android/scripts/a3/a3.sh gate      # the same against the release APK
android/scripts/a3/a3.sh gate idle10     # the 10-minute idle capture and one page load
KRUNK_OLD_APK=<published x86_64 APK> android/scripts/a3/a3.sh upgrade
android/scripts/a3/a3.sh config          # 360 dp width and rotation; last, they can wedge the emulator
```

- The tests find controls with `uiautomator dump` (`uix.py`), read what reached the network from
  the test server's log, and capture every packet of the app's UID on the device (`iptables …
  -m owner --uid-owner <uid> -j NFLOG` and `tcpdump -i nflog:30`, the emulator has root).
  `KRUNK_TEST_HOST` is the address of the machine running the test server as the device sees it;
  `EV` the folder for logs, captures and screenshots; `KRUNK_A3_ROOT` where `server.log` lives.
- Debug builds answer the broadcast `org.krunk.browser.DEBUG` (`src/debug/…/DebugHooks.kt`:
  `--es cmd status | allow | unallow | update | lists | setting | blocker | guard-disable |
  recreate-session`) in the log under the tag `KrunkGuard`, and read the properties
  `debug.krunk.guard_uri` (another address for the guard: a start without it) and
  `debug.krunk.show_private` (private tabs can be screenshotted). A release build has none of
  this: with `REL=1` the checks that need it are skipped by name.
- `navigator.share` needs a secure context. `adb reverse` ends at the adb server's host, so the
  share test runs a relay on the device (`toybox nc`, `localhost:18081` to the test server).
- Separate test groups with Home, 3 seconds, then `am force-stop`.
- Do not edit `a3.sh` while a run is in progress: bash reads the file as it goes.
- In a capture summary (`pcap_summary.py`) a destination with "syn" opened a new connection; one
  without got only packets of a connection that was already open (keep-alive, closing). After a
  list update the engine keeps the connections for a few minutes.

## Release signing and publishing

**The key.** Release APKs are signed with Krunk's Android release key: a PKCS12 keystore outside
the repository, described by a properties file (`storeFile`, `storePassword`, `keyAlias`,
`keyPassword`). Gradle reads `~/.config/krunk/android-release.properties`, or the path in
`KRUNK_ANDROID_SIGNING`. Without that file `assembleRelease` fails (since 0.1.1), so an APK with
the debug key is never mistaken for a release; `-Pkrunk.allowDebugSignedRelease=true` builds the
minified variant with the debug key on purpose. Only the publishing machine makes installable
updates.

- Certificate: `CN=Krunk, O=Krunk, C=DE`, RSA 4096, valid until 2056-11-03, SHA-256
  `C9:4B:89:15:CD:8B:39:3A:5F:C4:8B:C1:32:A6:DE:02:63:74:E7:63:37:C4:34:1C:59:DD:60:32:83:73:E6:24`.
- Android only installs an update signed with the **same** key. If the keystore or its password
  is lost, every user has to uninstall and reinstall (losing the app's data), so back up
  `android-release.jks` and `android-release.properties` offline. Never commit them, never put
  them in an image or on the server, and never generate a new key in their place.
- It is a different secret from the Windows update-manifest key (`update-signing.key`).
- A first key is made like this (only when none exists):
  `keytool -genkeypair -storetype PKCS12 -keystore android-release.jks -alias krunk -keyalg RSA
  -keysize 4096 -validity 10950 -dname "CN=Krunk, O=Krunk, C=DE"`.

**A release.**
1. Set `versionName` and raise `versionCode` in `android/app/build.gradle.kts`; add the
   `## X.Y.Z — YYYY-MM-DD` section to `CHANGELOG-android.md` (the Windows `CHANGELOG.md` only
   takes Windows versions: the website's build checks its headings).
2. `cd android && ./gradlew assembleRelease`, then copy the three
   `app/build/outputs/apk/release/app-<abi>-release.apk` to
   `dist/android/Krunk-android-<version>-<abi>-release.apk` and refresh `SHA256SUMS`.
3. Check: `$ANDROID_HOME/build-tools/37.0.0/apksigner verify --print-certs -v <apk>` shows v2 and
   v3 "true" and the fingerprint above. Test the `x86_64` APK on the emulator (cold install:
   a debug-signed copy must be uninstalled first; the release build is not debuggable, so
   `run-as` does not work): `REL=1 android/scripts/a3/a3.sh gate`, then `ui`, `links`, and
   `gate idle10` for the 10-minute capture. Test the upgrade from the last published APK
   (`adb install -r`): data kept, blocking on.
4. Check the dependencies: `android/scripts/dep-audit.sh` (the release classpath against the OSV
   database) and `cargo audit` (the crates in `cargo tree -p krunk_ffi`).
5. Publish: `dist/server/publish-android.sh <version> --volume <releases volume>` (on the ai-pipe
   deployment `live-browser_krunk-releases`; details in `dist/server/README.md`, "Publish an
   Android release"). The script refuses APKs that are not signed with the release key's
   certificate, that are not this app in this version, a version that is already published
   (`--force`), and a volume Docker does not know.

## Emulator

```
android/scripts/emu.sh start          # attaches to an emulator already on adb, or boots the local AVD
android/scripts/emu.sh install        # installs the universal debug APK (or a given APK)
android/scripts/emu.sh launch [url]
android/scripts/emu.sh shot <name>    # -> .pipeline/screenshots/android/<name>.png (or $KRUNK_SHOTS)
android/scripts/emu.sh logcat [-d]
android/scripts/emu.sh adb shell ...
android/scripts/flows.sh [flow...]    # the R10 flows (see the script header)
```

In the development sandbox an emulator (Android 16, x86_64, 1080×2400) is provided through a remote
adb server (`ADB_SERVER_SOCKET`); `emu.sh` uses the first online `emulator-*` serial
(`ANDROID_SERIAL` overrides). Other attached devices are ignored.

Known traps:
- **No KVM in the sandbox.** A local emulator only runs with `-accel off` (software emulation).
  It boots in about 6 minutes, but it is far too slow to drive: the app (and system apps) hit
  input-dispatch ANRs within seconds, so no flow completes. Use it only to check that an APK
  installs and starts. A locally started emulator registers with the local adb server, not the
  sandbox's remote one: run every `emu.sh` call with `KRUNK_LOCAL_ADB=1`.
- **The sandbox emulator's graphics stack can wedge** (`allocator-service.ranchu` and
  `surfaceflinger` at 100 % CPU, screencap and input dead). It happened twice in round 1 while
  Krunk was running: once after `adb shell cmd uimode night yes`, once after `adb install -r` over
  the running app and a relaunch, so it is not tied to one command. A guest `adb reboot` does not
  recover it (SurfaceFlinger never registers) and the console port for `adb emu kill` is not
  reachable: only a host-side restart helps. Keep sessions short, `force-stop` the app before
  reinstalling, switch the theme in Settings rather than through `cmd uimode`, and save
  screenshots as you go.
- **Two devices on adb** (an offline TV is attached too): every adb call needs `-s`; `emu.sh`
  does that. Gradle's `connectedAndroidTest` needs `ANDROID_SERIAL=emulator-5554`.
- **Page loads are slow** on the software-GPU emulator (5–15 s); the first start after an install
  initialises the Gecko profile (10–20 s). Wait before screenshots.
- **Gecko child processes run `Application.onCreate` too**: nothing heavy may run there, and
  `onTrimMemory` must not touch lazily created singletons (it checks `isInitialized`).
- **`am start -W` reports "timeout"** on the very first launch after an install; later cold starts
  report ~1.1 s to the first frame.
- Tablet layouts can be checked on the same emulator with `adb shell wm size 1600x2560` and
  `wm density 320` (reset with `wm size reset` / `wm density reset`).
- `android/scripts/flows.sh` drives the round-1 acceptance flows over adb (one screenshot per
  flow); its tap coordinates assume 1080×2400 at 420 dpi with the toolbar at the top (the default;
  `TOOLBAR=bottom` for the other position). A screenshot that does not arrive within 20 s ends
  the run with exit 4 (the emulator's graphics stack has wedged; see above).
- Do configuration changes (rotation, `cmd uimode`, `wm size`) **last** in an emulator session:
  two of three wedges followed one. Leave the app with HOME and wait 3 s before `am force-stop`:
  `onStop` arrives late on this emulator, and a stop in the foreground counts as an unclean exit
  (from the second in a row Krunk shows "Restore tabs?").
- The debug app's database can be inspected with
  `adb shell run-as org.krunk.browser sqlite3 files/krunk/browser.db "select count(*) from visits"`.

## Data location

`filesDir/krunk/browser.db` is the core database (same schema as Windows: history, bookmarks,
tiles, favicons, settings in `kv`). The Gecko profile (cookies, site data, caches) lives in
GeckoView's default location under `filesDir`. Backup rules (`res/xml/backup_rules.xml`,
`data_extraction_rules.xml`, since 0.1.1): cloud backup carries nothing (the database holds
history and the session next to bookmarks and settings, and cannot be split); device-to-device
transfer on Android 12+ carries `files/krunk`; the Gecko profile is in neither.

The password vault (0.3.0) is `filesDir/krunk-vault/` (`vault/VaultFiles.kt`): beside
`files/krunk`, not in it.

- `vault.json` is the vault in the format of the Windows browser (`core::vault`): the KDF
  parameters, the key check, the vault key (sealed by the password's key, then wrapped by the
  Keystore key: `"wrap": "android-keystore"`) and the encrypted entries.
- `state.json` is the `ffi` crate's side file (`ffi/src/vault.rs`, written atomically): the unlock
  rate limiter (failures and the time of the last one), the wrapped key and the tags of the hint
  index (which sites have logins, usable only with the Keystore key, so that a locked vault can
  say "this site has logins"), and the biometric blob (the unlock key encrypted under the
  biometric Keystore key) while fingerprint unlock is on. It holds no password and no entry.
  Losing it costs the hints until the next unlock and fingerprint unlock until it is switched on
  again.
- The settings are not here but in `kv` of `browser.db`: `android.vault.lock_minutes`,
  `android.vault.background_grace_s`, `android.vault.offer_save`; the never-save sites are the
  shared core table.

The directory never leaves the device: it is excluded by name from cloud backup and from
device-to-device transfer in all three rule sets (`backup_rules.xml`; `cloud-backup` and
`device-transfer` in `data_extraction_rules.xml`), and the Keystore key that wraps the vault key
cannot be moved, so the logins in a copy could not be decrypted. `BackupRulesTest` reads the rule
files and fails if an include would carry the directory, if a section no longer names it in an
exclude, or if the path differs from `VaultFiles.DIR`. A real device-to-device transfer was not
run.

## Downloads (0.4.0)

Krunk saves downloads itself (`android/.../downloads/`); Android's `DownloadManager` is not used.

- `DownloadEngine` (plain Kotlin, JVM-tested in `DownloadEngineTest`) is the state machine. It
  saves the engine's own response stream: the `WebResponse` of `onExternalResponse` for a
  download a page started, or a `GeckoWebExecutor` fetch (`GeckoFetcher`; `FETCH_FLAGS_PRIVATE`
  for private tabs) for "Download link", "Save image", resume and restart. The rules for going on
  after a pause and for names come from the core (`ffi/src/downloads.rs`).
- The list is the core's (`DownloadList`, table `download_items`). A finished state is final
  there: an error on a download that can go on is *Paused* with a reason (shown as failed, with
  Retry), one that cannot go on is *Interrupted*, and Restart is a new entry.
- Bytes go to `files/downloads-part/<random>.part` (never backed up). Only a complete,
  length-checked file is published: `MediaStore.Downloads` on Android 10 and later, the app's
  external `Download` folder (shared through the FileProvider) on Android 8 and 9. A finished
  entry's `path` is `<content uri>/<file name>` (`DownloadPaths`).
- `DownloadController` (in `KrunkApp`) holds everything that lives with the process: the waiting
  "Download this file?" questions, the engine, the notifications (`DownloadNotifier`) and the
  start of `DownloadService` (a `dataSync` foreground service while a transfer runs).
- Private tabs: listed only while running, never written to the table, the part file has a
  random name and is swept at the next start; only the saved file remains.
- Debug builds: `adb shell am broadcast -a org.krunk.browser.DEBUG --es cmd downloads` logs the
  list, what runs and the part files (tag `KrunkGuard`); `--es cmd downloads-time-limit` acts as
  if Android's six-hour limit for the service was reached.

**Device checks (`android/scripts/a5/a5.sh`, 0.4.0).** Its own test server (cookie-gated file,
slow/ranged/unranged/cut/unknown-size files with a fixed content hash, `blob:`/`data:`, a
permissions page and a storage page):
```sh
python3 -I android/scripts/a5/server.py android/scripts/a5/www 18080 >> "$KRUNK_A3_ROOT/server.log" &
KRUNK_OLD_APK=<published x86_64 APK> REL=1 android/scripts/a5/a5.sh upgrade
REL=1 android/scripts/a5/a5.sh cookie blob resume noresume open private perms permchange notify sitedata history pointer fullscreen
```
Traps: geolocation and notifications need a secure context, so the permission tests load the
pages through a relay on the device (`http://localhost:18081`, `relay` in the script); the
Download button of the question waits 0.5 s; the server cuts a `cut=` file only once per server
run (the script uses new names); a `uiautomator dump` while the screen is in landscape makes
UiAutomation fix the system rotation at 90° (`user_rotation=1`), which looks like an orientation
lock Krunk did not give back; a restored tab whose address is a download asks for it again.
The debug build answers `--es cmd 'fetch-test <url>'` (each ingredient of a download request
through `GeckoWebExecutor`, logged under `KrunkGuard`).

## Vault

The password vault of 0.3.0. What users get is in `README.md` and `CHANGELOG-android.md`; the
decisions are in `docs/ANDROID_PLAN.md` ("Decisions taken for Android 0.3.0").

**Layers**, from the bottom:

1. `core::vault` (`core/src/vault.rs`): the scheme and the file format, shared with Windows. Two
   functions were added for Android and nothing was changed: `Vault::unlock_key` and
   `Vault::open_with_key` (biometric unlock, so the password itself is never stored).
2. `ffi/src/vault.rs`: the UniFFI object `KrunkVault` (none / locked / unlocked; create, unlock,
   lock, change password, summaries, search, reveal, save, delete, `matches_for`,
   `secret_for_fill`, `for_http_auth`, `has_hint`, the inactivity clock, `erase`), the errors
   `VaultFfiError`, and the foreign trait `VaultKeyWrapper` (`protect` / `unprotect`): the first
   callback interface in `ffi/`, implemented in Kotlin. Lists never carry passwords; a password
   comes back only from `reveal`, `secret_for_fill` and `for_http_auth`. The rate limit is
   enforced here. Do not name a method `destroy`: generated UniFFI objects have their own.
3. `vault/KeystoreKeyWrapper.kt` (the wrap key, behind the interface `WrapKeyStore` so JVM tests
   run every error path with a fake) and `vault/FfiVaultBackend.kt` (`FfiKeyWrapper` hands the
   wrapper to the core; `FfiVaultBackend` implements `VaultBackend` on `KrunkVault`; tests use
   `FakeVaultBackend`).
4. `vault/VaultController.kt`: the lock state (`NONE`, `LOCKED`, `UNLOCKED`, `KEY_LOST`) with an
   injected clock: Lock now, the timer (`tick` every 15 s and on return to the foreground), the
   lock after leaving Krunk (`onBackground` / `onForeground`; the timer in the background is best
   effort, `onForeground` makes up for it before anything is shown). `lockEvents` tells screens
   to drop what they show. `vault/BiometricUnlock.kt` sits beside it; `vault/VaultLockSetting.kt`
   holds the two lock settings.
5. On top, two users: the screens (`ui/vault/*`, which talk to the interface `VaultPort`;
   `AppVaultPort` is the real one, `PreviewVaultPort` a stand-in) and the engine side
   (`engine/logins/LoginsHub`, the runtime's `Autocomplete.StorageDelegate`, through
   `LoginVault` / `vault/AppLoginVault.kt`; `LoginPolicy` and `HttpAuthOffer` are the pure rules;
   `KrunkSession.onLoginSelect` / `onLoginSave` / `onAuthPrompt` raise the prompts drawn in
   `ui/browser/LoginPrompts.kt`).

**What the two keys protect.** `vault.json` holds a password check record that is sealed by the
password's key alone. Someone who has a copy of the file can therefore try passwords against it
on any machine, slowed only by Argon2id (64 MiB, 3 passes for every guess): the Keystore key does
**not** protect the password against guessing. What it protects is the content: the vault key is
wrapped by the device key, so a copied file cannot be decrypted on another device even with the
right password. Do not write anything else in user-facing text; the advice is a long vault
password.

**The engine never holds the passwords.** The engine's own filling is off
(`loginAutofillEnabled(false)` in `GeckoEngine`); `EngineConfig` switches off capture in private
tabs, generated passwords and Relay. `onLoginFetch(domain)` is runtime-wide and does not say which
tab asks, so `LoginsHub` answers it with entries that carry `LoginPolicy.PLACEHOLDER_PASSWORD`:
one per matching login of the pages the open tabs show, or one stand-in (user name
`LOCKED_PLACEHOLDER_USER`) while the vault is locked and the hint list knows the site. Two engine
facts to keep: an entry must carry the page's exact origin **including the port** or the engine
drops it, and the stand-in needs a non-empty user name or the user-name field ignores it. The
real password goes to the engine in `onLoginSelect`, for the one login the user picked, after
`secret_for_fill` checked it against the tab's address again. `onLoginSave` also arrives after a
fill (the engine compared with the placeholder), so Krunk decides new / update / unchanged itself.

**Keystore aliases** (provider `AndroidKeyStore`, AES-256-GCM, blob = version byte `0x01`,
12-byte IV, ciphertext with tag):

| Alias | What | Notes |
|---|---|---|
| `krunk.vault.wrap.v1` | wraps the vault key in `vault.json` | no user authentication; StrongBox first, the TEE-backed key when the device has no StrongBox. Made only by "Create vault" (`prepareForNewVault`); `protect` / `unprotect` never make one |
| `krunk.vault.bio.v1` | encrypts the unlock key for fingerprint unlock (blob in `state.json`) | user authentication required for every use, strong biometrics only, invalidated by a new enrolment; StrongBox first with the same fallback. "On" is stored nowhere: it is on while key and blob both exist |

**Key loss.** Only a wrap key that the key store reports as **missing or invalidated** makes the
state `KEY_LOST` ("This vault cannot be opened": shown before any password is asked, the file
stays as it is, the only way on is "Delete this vault" behind the typed word `DELETE`). A key
store that merely **fails to answer** is not key loss: it is reported as "key store did not
answer, try again" (error `KeyUnavailable`) and the vault stays locked. Neither a key nor a vault
is ever made again silently.

Biometrics use the platform `BiometricPrompt` (no AndroidX Biometric: its stable release needs a
`FragmentActivity`) and are offered from API 30; the manifest has `USE_BIOMETRIC`.

**Secure windows.** Every composable that shows vault content or asks for the vault password
calls `SecureVaultSurface()` (`ui/vault/VaultSecureFlag.kt`, a counter); `KrunkRoot` sets
`FLAG_SECURE` while the count is above zero or the vault screen is open. A new vault dialog must
do the same.

**Debug hooks** (debug builds only, `src/debug/…/DebugHooks.kt`; sent like the others with
`adb shell am broadcast -a org.krunk.browser.DEBUG --es cmd <name>`, answered in the log under
`KrunkGuard`):

- `vault-status`: `vault state=<NONE|LOCKED|UNLOCKED|KEY_LOST> grace=<seconds> minutes=<n>`.
- `vault-drop-key`: deletes the wrap key (the "key lost" case). The device test restarts the app
  afterwards.
- `vault-fixture`: writes `files/krunk-vault/vault-inner.json`, the vault file with only the
  Keystore layer of `vaultKey` removed (still sealed by the password, `"wrap": "none"`), for the
  cross-platform format test. The test deletes the file after pulling it.

**Device checks** (`android/scripts/a4/a4.sh`, on the helpers of `a3/`: same `lib.sh`, same
`uix.py`, same variables `KRUNK_TEST_HOST`, `KRUNK_A3_ROOT`, `EV`, `KRUNK_ADB_TIMEOUT`):

```
python3 -I android/scripts/a3/server.py android/scripts/a4/www 18080 >> "$KRUNK_A3_ROOT/server.log" &
android/scripts/a4/a4.sh create save https locked private auth screen timer grace   # in this order
android/scripts/a4/a4.sh nobio            # no fingerprint enrolled: the switch is disabled, with its reason
android/scripts/a4/a4.sh keylost          # debug build: the wrap key removed, "cannot be opened", delete
android/scripts/a4/a4.sh fixture          # debug build: the fixture pair (below)
android/scripts/a4/a4.sh config           # rotation with the unlock form, dark mode; last
REL=1 android/scripts/a4/a4.sh create     # the release APK; keylost and fixture are skipped by name
KRUNK_OLD_APK=<published x86_64 APK> android/scripts/a4/a4.sh upgrade
```

Each test starts the app itself; `create` installs afresh, the others build on the vault and the
logins the earlier ones left (vault password `fixture-password-1`). The test server gained
`/auth` (Basic, `krunk` / `test`) for the HTTP sign-in test. The script ends with
`RESULT pass=… fail=… skip=…` and a non-zero exit on any failure.

Traps met in the 0.3.0 pass:

- **Screenshots of vault screens are black on purpose** (secure window). `shotblack` asserts
  exactly that; what was on screen is proven by the PASS lines, which read the accessibility
  tree (`uiautomator dump`), not by the picture.
- **A page that keeps changing never lets `uiautomator` see an idle screen**, and a failed dump
  leaves the previous screen's file to be read again. The login page writes its status line
  only when it changes, and `a4.sh` deletes the old dump before every new one.
- The test login page's fields have no names in the accessibility tree: they are tapped by
  position (1080×2400, toolbar at the top). The same holds for the public https form.
- The `https` test uses a public form (`https://the-internet.herokuapp.com/login`): it needs
  the network and that site; the local server is plain http.
- Two "fails" of `save` in the 0.3.0 pass were the script misreading the page's values while the
  form was filled (the screenshot shows it). Look at the screenshot before believing a failed
  value check.
- `adb shell input text` needs `%s` for a space (the fixture password).
- No fingerprint can be simulated on the sandbox emulator: `adb emu` has no console port there.
- Do `config` last, as with `a3.sh`.

**Fixtures** (`core/tests/fixtures/vault/platform/`, README there). A Keystore or DPAPI blob opens
only on the machine that made it, so each platform has the file as written and an inner twin
without the platform's wrap. To make the Android pair again: `a4.sh fixture` on a debug build
(it creates a vault with the password `fixture password`, adds the two logins the README names,
calls `vault-fixture` and pulls both files into `$EV/fixture/`), copy them over
`vault-android-keystore.json` and `vault-android-inner.json`, then `cargo test -p krunk_ffi
platform`. The Windows pair comes from the Windows VM with `strip-dpapi.ps1` in that folder. The
content must stay what the README lists: the test knows the password and the two logins, not
ids or times.

**What could not be tested here.**

- **Fingerprint unlock** on a device: switching it on, unlocking, a new enrolment, the fall-back.
  `BiometricUnlockTest` (fakes with real AES-GCM) is the only cover. On the emulator only the
  disabled switch with nothing enrolled was checked.
- **StrongBox:** the emulator image has none (`pm list features` shows no `strongbox_keystore`)
  and ran the TEE path for real. The StrongBox-first branch and its fallback are covered by
  `KeystoreKeyWrapperTest` and `BiometricUnlockTest` with fakes.
- Copy to the clipboard and its timed clearing: run on the release build (`a4.sh clip`, and
  `m7/rel-extra-steps.sh clipbg` for the clear with Krunk in the background, which pastes into
  Android's Settings search). In the background the timer in the process does not run (Android
  freezes the process within seconds); an inexact alarm (`ClipClearReceiver`) does the clear.
- A real device-to-device transfer and an arm64 device.
