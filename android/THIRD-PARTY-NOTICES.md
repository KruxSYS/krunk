# Krunk for Android: third-party notices

Krunk for Android is MIT-licensed. An APK contains, or is built from, the components below, each
under its own licence. The same list is shown in the app under Settings › About › Open-source
licences (`app/src/main/kotlin/org/krunk/browser/ui/settings/Licenses.kt`); a unit test
(`LicensesTest`) fails when the two differ.

| Component | Licence | What it is | Source |
|---|---|---|---|
| GeckoView | MPL-2.0 | Mozilla's Gecko engine as a library; renders every page | https://mozilla.github.io/geckoview/ |
| Jetpack Compose and AndroidX | Apache-2.0 | User interface toolkit and support libraries | https://developer.android.com/jetpack |
| Kotlin | Apache-2.0 | Standard library and coroutines | https://kotlinlang.org/ |
| JNA (Java Native Access) | Apache-2.0 or LGPL-2.1 | Calls into the shared Rust core | https://github.com/java-native-access/jna |
| UniFFI | MPL-2.0 | Generates the Kotlin bindings of the Rust core | https://mozilla.github.io/uniffi-rs/ |
| rusqlite with bundled SQLite | MIT (SQLite: public domain) | History, bookmarks, settings and tiles | https://github.com/rusqlite/rusqlite |
| Brave adblock-rust | MPL-2.0 | Filter engine of the shared core (not what filters pages on Android) | https://github.com/brave/adblock-rust |
| uBlock Origin | GPL-3.0 | The ad and tracker blocker, by Raymond Hill and contributors | the release named in `ubo.lock` (`UBO_VERSION`, `UBO_XPI`) |
| EasyList and EasyPrivacy | GPL-3.0-or-later or CC BY-SA 3.0 | Filter lists inside uBlock Origin, by the EasyList authors | https://easylist.to/pages/licence.html |
| uBlock filters | GPL-3.0 | uBlock Origin's own lists, by Raymond Hill and the uAssets contributors | https://github.com/uBlockOrigin/uAssets |
| Peter Lowe's ad and tracking server list | McRae General Public License 4.r53 | Ad and tracker hosts, inside uBlock Origin, by Peter Lowe | https://pgl.yoyo.org/adservers/ |
| Online Malicious URL Blocklist (URLhaus) | CC0-1.0 | Addresses that spread malware, inside uBlock Origin; malware-filter project, data by abuse.ch | https://gitlab.com/malware-filter/urlhaus-filter |
| Public Suffix List | MPL-2.0 | Where a site's name ends; inside uBlock Origin and the shared core | https://publicsuffix.org/ |
| SnakeYAML | Apache-2.0 | Part of GeckoView; reads Krunk's engine configuration | https://bitbucket.org/snakeyaml/snakeyaml |
| Rust crates of the shared core | MIT or Apache-2.0 unless noted | serde, serde_json, url, psl, RustCrypto argon2 and chacha20poly1305 and the rest of `Cargo.lock` | `../Cargo.lock` |

## uBlock Origin

uBlock Origin is bundled as the project's own signed release file, unpacked and otherwise
untouched: the build downloads the file named in `ubo.lock`, checks its SHA-256, and fails unless
every file in the APK under `assets/extensions/ublock0/` is byte for byte the file from that
release. Version, file address and digest are in `ubo.lock` and nowhere else; the app reads the
version and the address from it at build time.

- Copyright Raymond Hill and the uBlock Origin contributors. Licence: GPL-3.0; its text is in the
  APK as `assets/extensions/ublock0/LICENSE.txt`.
- Source code of exactly this release: the tag of `UBO_VERSION` at https://github.com/gorhill/uBlock.
- Krunk drives it from a separate extension of its own (`assets/extensions/krunk-guard/`, MIT)
  through uBlock Origin's own messages; nothing is written into its files, pages or storage.
- The filter lists inside it keep their own licences (table above). The package's
  `assets/thirdparties/` folders carry the lists' own licence files and headers, and its fonts
  (`css/fonts/`) and libraries (`lib/`) carry theirs.
- Peter Lowe's list is published under the McRae General Public License; its author asks that it
  be used for non-commercial purposes only (see the README next to the list in the package).
