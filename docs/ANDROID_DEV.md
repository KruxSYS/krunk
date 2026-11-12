# Krunk for Android: developer guide

How to set up the toolchain, build the APK, run it on the emulator, run the tests, and where the
evidence goes. The product description is in `README.md` ("Krunk for Android (preview)"); the
plan and decisions are in `docs/ANDROID_PLAN.md`; the parity matrix is `docs/ANDROID_FEATURES.md`.

## Layout

| Path | What |
|---|---|
| `android/` | Gradle project (Kotlin, Jetpack Compose, Material 3). `app/` is the only module. |
| `android/app/src/main/kotlin/org/krunk/browser/` | `KrunkApp` (process state), `MainActivity`, `core/` (store + settings on the Rust core), `engine/` (GeckoView runtime, session delegates, error page, the icons extension), `tabs/` (tab manager on `core::tabs`, persistence, favicons, thumbnails, memory policy), `ui/` (Compose screens: browser, tabs, new tab, bookmarks, history, settings, theme). |
| `android/app/src/main/assets/` | `error.html` (Krunk's error page), `extensions/krunk-icons/` (bundled WebExtension), `wallpapers/` (copied from `ui/shared/wallpapers` at build time, git-ignored). |
| `android/scripts/` | `setup-sdk.sh` (toolchain), `build-rust.sh` (cargo-ndk + UniFFI bindings; Gradle calls it), `update-ubo.sh` (the pinned uBlock Origin release; Gradle calls it), `emu.sh` (emulator driver), `flows.sh` (the 0.1.x adb flows), `a3/` (the device checks since 0.2.0: `a3.sh`, its test server and pages). |
| `android/ubo.lock` | The uBlock Origin release built into the app: version, release file, SHA-256. The only place that names it. |
| `android/app/src/main/assets/extensions/` | `krunk-icons/` (site icons), `krunk-guard/` (Krunk's extension that gates and drives the blocker and rewrites tracking parameters), `ublock0/` (uBlock Origin, unpacked from the pinned release at build time, git-ignored). |
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

The gate in `krunk-guard` is the second line: an observer on every request cancels uBlock Origin's
own requests unless an update the user started is running, and cancels requests the engine
itself starts outside a page.

## Device checks (`android/scripts/a3/`)

```
python3 -I android/scripts/a3/server.py android/scripts/a3/www 18080 >> "$KRUNK_A3_ROOT/server.log" &
android/scripts/a3/a3.sh gate            # the blocker's gate and start order (see the script header)
android/scripts/a3/a3.sh ui | links | prompts | themes | small
REL=1 android/scripts/a3/a3.sh gate      # the same against the release APK
android/scripts/a3/a3.sh gate idle10     # the 10-minute idle capture and one page load
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
transfer on Android 12+ carries `files/krunk`; the Gecko profile is in neither. A later vault
must get its own exclude in the device-transfer rules.
