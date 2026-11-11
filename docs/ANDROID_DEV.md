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
| `android/scripts/` | `setup-sdk.sh` (toolchain), `build-rust.sh` (cargo-ndk + UniFFI bindings; Gradle calls it), `emu.sh` (emulator driver), `flows.sh` (adb-driven test flows). |
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

## Release signing and publishing

**The key.** Release APKs are signed with Krunk's Android release key: a PKCS12 keystore outside
the repository, described by a properties file (`storeFile`, `storePassword`, `keyAlias`,
`keyPassword`). Gradle reads `~/.config/krunk/android-release.properties`, or the path in
`KRUNK_ANDROID_SIGNING`. Without that file `assembleRelease` warns and signs with the debug key,
so anyone can build; only the publishing machine makes installable updates.

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
   v3 "true" and the fingerprint above. Smoke-test the `x86_64` APK on the emulator (cold install:
   a debug-signed copy must be uninstalled first; the release build is not debuggable, so
   `run-as` does not work).
4. Publish: `dist/server/publish-android.sh <version> --volume <releases volume>` (details in
   `dist/server/README.md`, "Publish an Android release"). The script refuses APKs that are not
   signed with the release key's certificate.

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
  flow); its tap coordinates assume 1080×2400 at 420 dpi with the toolbar at the bottom.
- The debug app's database can be inspected with
  `adb shell run-as org.krunk.browser sqlite3 files/krunk/browser.db "select count(*) from visits"`.

## Data location

`filesDir/krunk/browser.db` is the core database (same schema as Windows: history, bookmarks,
tiles, favicons, settings in `kv`). The Gecko profile (cookies, site data, caches) lives in
GeckoView's default location under `filesDir`. Backup rules (`res/xml/backup_rules.xml`,
`data_extraction_rules.xml`) include `files/krunk` and exclude the Gecko profile and
`files/krunk/vault*`.
