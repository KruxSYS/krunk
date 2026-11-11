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
./gradlew assembleRelease      # minified, debug-signed (release signing is not configured yet)
```

`cargoBuildDebug` / `cargoBuildRelease` run `android/scripts/build-rust.sh <profile>`: a host build
of `krunk_ffi` (bindings + JVM tests), `uniffi-bindgen generate --library`, then `cargo ndk` for
`arm64-v8a` and `x86_64` into `app/build/rust/<variant>/`. Debug libraries are stripped
(`llvm-strip`) because AGP keeps debug variants' symbols; the host library keeps them.

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
- **No KVM in the sandbox.** A local emulator only runs with `-accel off` (software emulation):
  10–30 minutes to boot. Prefer an attached emulator.
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
- The debug app's database can be inspected with
  `adb shell run-as org.krunk.browser sqlite3 files/krunk/browser.db "select count(*) from visits"`.

## Data location

`filesDir/krunk/browser.db` is the core database (same schema as Windows: history, bookmarks,
tiles, favicons, settings in `kv`). The Gecko profile (cookies, site data, caches) lives in
GeckoView's default location under `filesDir`. Backup rules (`res/xml/backup_rules.xml`,
`data_extraction_rules.xml`) include `files/krunk` and exclude the Gecko profile and
`files/krunk/vault*`.
