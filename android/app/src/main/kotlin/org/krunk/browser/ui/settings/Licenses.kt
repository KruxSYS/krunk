package org.krunk.browser.ui.settings

/** One third-party component the Android app ships or builds on. */
data class LicenseEntry(val name: String, val license: String, val note: String, val url: String)

/**
 * What Krunk for Android is made of (shown on the About screen). The Rust crates of the shared
 * core are listed in `Cargo.lock`; the main ones are summarised here, as in README's
 * "Third-party components".
 */
val LICENSES: List<LicenseEntry> = listOf(
    LicenseEntry(
        "GeckoView",
        "MPL-2.0",
        "Mozilla's Gecko engine as a library; renders every page.",
        "https://mozilla.github.io/geckoview/",
    ),
    LicenseEntry(
        "Jetpack Compose and AndroidX",
        "Apache-2.0",
        "The user interface toolkit and support libraries (Material 3, Activity, Lifecycle).",
        "https://developer.android.com/jetpack",
    ),
    LicenseEntry(
        "Kotlin",
        "Apache-2.0",
        "Standard library and coroutines.",
        "https://kotlinlang.org/",
    ),
    LicenseEntry(
        "JNA (Java Native Access)",
        "Apache-2.0 or LGPL-2.1",
        "Calls into the shared Rust core.",
        "https://github.com/java-native-access/jna",
    ),
    LicenseEntry(
        "UniFFI",
        "MPL-2.0",
        "Generates the Kotlin bindings of the Rust core.",
        "https://mozilla.github.io/uniffi-rs/",
    ),
    LicenseEntry(
        "rusqlite with bundled SQLite",
        "MIT (SQLite: public domain)",
        "History, bookmarks, settings and tiles in one database, the same schema as Krunk for Windows.",
        "https://github.com/rusqlite/rusqlite",
    ),
    LicenseEntry(
        "Brave adblock-rust",
        "MPL-2.0",
        "The filter engine of the shared core (the Android ad blocker arrives in a later round).",
        "https://github.com/brave/adblock-rust",
    ),
    LicenseEntry(
        "Rust crates of the shared core",
        "MIT or Apache-2.0 unless noted",
        "serde, serde_json, url, psl, RustCrypto argon2 and chacha20poly1305 and the rest of Cargo.lock.",
        "https://krunk.209.wf",
    ),
)
