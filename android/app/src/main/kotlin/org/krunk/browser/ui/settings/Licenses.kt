package org.krunk.browser.ui.settings

import org.krunk.browser.BuildConfig

/** One third-party component the Android app ships or builds on. */
data class LicenseEntry(val name: String, val license: String, val note: String, val url: String)

/**
 * Where the bundled uBlock Origin comes from, read from `android/ubo.lock` at build time
 * (`BuildConfig.UBO_VERSION`, `BuildConfig.UBO_SOURCE_URL`): the lock is the only place that
 * names the version, so the screen can never list another one than the build contains.
 */
object UboSource {
    private val asset = Regex("""^(https://github\.com/[^/]+/[^/]+)/releases/download/([^/]+)/([^/]+)$""")

    /**
     * The release's page for the release file's address (the page lists the signed file, its
     * digest and the source archives; opening the file itself would start a download). An
     * address of another shape is returned as it is.
     */
    fun releasePage(xpiUrl: String): String =
        asset.matchEntire(xpiUrl)?.let { "${it.groupValues[1]}/releases/tag/${it.groupValues[2]}" } ?: xpiUrl

    /** The release file's name, as on the release page. */
    fun fileName(xpiUrl: String): String = xpiUrl.substringAfterLast('/')
}

/**
 * What Krunk for Android (Krunk by KruxSYS) is made of (shown on the About screen). The Rust crates of the shared
 * core are listed in `Cargo.lock`; the main ones are summarised here, as in README's
 * "Third-party components" and in `android/THIRD-PARTY-NOTICES.md` (a test keeps the two equal).
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
        "The filter engine of the shared core. On Android, pages are filtered by uBlock Origin (below).",
        "https://github.com/brave/adblock-rust",
    ),
    LicenseEntry(
        "uBlock Origin " + BuildConfig.UBO_VERSION,
        "GPL-3.0",
        "The ad and tracker blocker, by Raymond Hill and contributors. Built in unchanged from the signed release " +
            "file " + UboSource.fileName(BuildConfig.UBO_SOURCE_URL) + ", every file checked against it. Its " +
            "licence text and the notices of the fonts and libraries inside it are part of the package.",
        UboSource.releasePage(BuildConfig.UBO_SOURCE_URL),
    ),
    LicenseEntry(
        "EasyList and EasyPrivacy",
        "GPL-3.0-or-later or CC BY-SA 3.0",
        "Filter lists that come inside uBlock Origin, by the EasyList authors.",
        "https://easylist.to/pages/licence.html",
    ),
    LicenseEntry(
        "uBlock filters",
        "GPL-3.0",
        "uBlock Origin's own lists (filters, badware risks, privacy, quick fixes, unbreak), by Raymond Hill and " +
            "the uAssets contributors.",
        "https://github.com/uBlockOrigin/uAssets",
    ),
    LicenseEntry(
        "Peter Lowe's ad and tracking server list",
        "McRae General Public License 4.r53",
        "A list of ad and tracker hosts that comes inside uBlock Origin, by Peter Lowe.",
        "https://pgl.yoyo.org/adservers/",
    ),
    LicenseEntry(
        "Online Malicious URL Blocklist (URLhaus)",
        "CC0-1.0",
        "A list of addresses that spread malware, inside uBlock Origin; from the malware-filter project, " +
            "data by abuse.ch.",
        "https://gitlab.com/malware-filter/urlhaus-filter",
    ),
    LicenseEntry(
        "Public Suffix List",
        "MPL-2.0",
        "Tells where a site's name ends (example.co.uk); inside uBlock Origin and the shared core.",
        "https://publicsuffix.org/",
    ),
    LicenseEntry(
        "SnakeYAML",
        "Apache-2.0",
        "Part of GeckoView; reads the settings that keep the engine from contacting anyone in the background.",
        "https://bitbucket.org/snakeyaml/snakeyaml",
    ),
    LicenseEntry(
        "boringtun",
        "BSD-3-Clause",
        "The WireGuard protocol of the in-browser VPN (protocol state machine only), by Cloudflare; with x25519-dalek (BSD-3-Clause).",
        "https://github.com/cloudflare/boringtun",
    ),
    LicenseEntry(
        "smoltcp",
        "0BSD",
        "The TCP/IP stack inside the VPN tunnel.",
        "https://github.com/smoltcp-rs/smoltcp",
    ),
    LicenseEntry(
        "OpenVPN 3 Core",
        "MPL-2.0",
        "The OpenVPN client library of the in-browser VPN, used under MPL-2.0 through the openvpn-connect crates (MPL-2.0), vendored with the changes listed in vendor/README.md.",
        "https://github.com/OpenVPN/openvpn3",
    ),
    LicenseEntry(
        "Asio",
        "BSL-1.0",
        "Networking library inside OpenVPN 3 Core.",
        "https://think-async.com/Asio/",
    ),
    LicenseEntry(
        "LZ4",
        "BSD-2-Clause",
        "Compression library inside OpenVPN 3 Core (lz4-sys bindings: MIT).",
        "https://github.com/lz4/lz4",
    ),
    LicenseEntry(
        "rustls and ring",
        "Apache-2.0 OR ISC OR MIT (rustls); Apache-2.0 AND ISC (ring)",
        "TLS and cryptography for OpenVPN's control channel.",
        "https://github.com/rustls/rustls",
    ),
    LicenseEntry(
        "webpki-roots",
        "CDLA-Permissive-2.0",
        "Mozilla's root certificates as a Rust crate, linked with rustls.",
        "https://github.com/rustls/webpki-roots",
    ),
    LicenseEntry(
        "Rust crates of the shared core",
        "MIT or Apache-2.0 unless noted",
        "serde, serde_json, url, psl, RustCrypto argon2 and chacha20poly1305 and the rest of Cargo.lock.",
        "https://krunk.kruxsys.com",
    ),
)
