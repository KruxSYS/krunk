//! The user-agent presets of Settings → Advanced (Krunk 0.5.0), and the identity Krunk presents
//! when the setting is "Default": Google Chrome, on Windows and Android. Bot defences (DataDome and
//! the like) rejected the engine's own "Chromium" brand, a brand no ordinary visitor sends (0.27.0).
//! 0.28.0: the version is the **engine's own** (`cef.lock` `CHROMIUM_VERSION`), not the latest
//! Chrome: a page can tell the engine's version from its features (Chrome 158 has APIs a 156
//! engine lacks, and the reverse), and a "158" that behaves like 156 is a contradiction bot checks
//! score, while a Chrome two releases behind is an ordinary visitor. Bump [`CHROME_VERSION`] (and
//! the `chrome_major!` macro) together with the engine; a test keeps them equal to `cef.lock`.

use serde::Serialize;

/// Where the versions came from, for the next update of this table.
pub const SOURCES: [&str; 4] = [
    "Chrome = the engine's Chromium version (cef.lock CHROMIUM_VERSION, 156.0.8078.80); stable releases: https://chromiumdash.appspot.com/releases?platform=Windows",
    "Edge follows the same Chromium major: https://learn.microsoft.com/en-us/microsoft-edge/web-platform/release-notes/",
    "Firefox 159.0.1, 3 Nov 2026: https://www.firefox.com/en-US/firefox/159.0.1/releasenotes/",
    "Safari 27.0, 17 Sep 2026 (frozen OS tokens): https://webkit.org/blog/18325/webkit-features-for-safari-27-0/",
];

/// The engine's Chromium major version, as a literal for `concat!`.
macro_rules! chrome_major {
    () => {
        "156"
    };
}

/// The engine's full Chromium version: the default identity's client hints carry it.
pub const CHROME_VERSION: &str = "156.0.8078.80";
/// The brand real Chrome lists next to "Chromium" in its client hints.
pub const CHROME_BRAND: &str = "Google Chrome";
/// Chrome on 64-bit Windows: the default user agent of Krunk for Windows.
pub const CHROME_WINDOWS: &str = concat!(
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/",
    chrome_major!(),
    ".0.0.0 Safari/537.36"
);
/// Chrome on an Android phone (reduced, as Chrome sends it): Krunk for Android's default.
pub const CHROME_ANDROID: &str = concat!(
    "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/",
    chrome_major!(),
    ".0.0.0 Mobile Safari/537.36"
);

/// The user agent a tab uses for the user-agent setting `setting` ("" = Default): the setting
/// itself, else Chrome. `desktop`: the desktop identity (Windows, or an Android tab asking for
/// the desktop site).
pub fn effective(setting: &str, desktop: bool) -> &str {
    match setting.trim() {
        "" if desktop => CHROME_WINDOWS,
        "" => CHROME_ANDROID,
        s => s,
    }
}

/// The engine switches that make Krunk for Windows present itself as Chrome when the
/// user-agent setting is Default: the user agent's product token, and the brand and version
/// the client hints carry (`navigator.userAgentData`, `Sec-CH-UA*`; Krunk's engine patch
/// chromium-ua-brand.patch). A chosen user agent goes through the engine's `user_agent` setting
/// instead and leaves these out.
pub fn engine_switches(setting: &str) -> Vec<String> {
    if !setting.trim().is_empty() {
        return Vec::new();
    }
    vec![
        concat!("--user-agent-product=Chrome/", chrome_major!(), ".0.0.0").to_string(),
        format!("--krunk-ua-brand={CHROME_BRAND}"),
        format!("--krunk-ua-version={CHROME_VERSION}"),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    pub user_agent: &'static str,
}

/// The presets, in the order the picker shows them.
pub const PRESETS: [Preset; 10] = [
    Preset {
        id: "chrome-windows",
        label: concat!("Chrome ", chrome_major!(), " on Windows"),
        user_agent: CHROME_WINDOWS,
    },
    Preset {
        id: "chrome-macos",
        label: concat!("Chrome ", chrome_major!(), " on macOS"),
        user_agent: concat!(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/",
            chrome_major!(),
            ".0.0.0 Safari/537.36"
        ),
    },
    Preset {
        id: "chrome-linux",
        label: concat!("Chrome ", chrome_major!(), " on Linux"),
        user_agent: concat!(
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/",
            chrome_major!(),
            ".0.0.0 Safari/537.36"
        ),
    },
    Preset {
        id: "chrome-android",
        label: concat!("Chrome ", chrome_major!(), " on Android (phone)"),
        user_agent: CHROME_ANDROID,
    },
    Preset {
        id: "edge-windows",
        label: concat!("Edge ", chrome_major!(), " on Windows"),
        user_agent: concat!(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/",
            chrome_major!(),
            ".0.0.0 Safari/537.36 Edg/",
            chrome_major!(),
            ".0.0.0"
        ),
    },
    Preset {
        id: "firefox-windows",
        label: "Firefox 159 on Windows",
        user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:159.0) Gecko/20100101 Firefox/159.0",
    },
    Preset {
        id: "firefox-macos",
        label: "Firefox 159 on macOS",
        user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:159.0) Gecko/20100101 Firefox/159.0",
    },
    Preset {
        id: "firefox-linux",
        label: "Firefox 159 on Linux",
        user_agent: "Mozilla/5.0 (X11; Linux x86_64; rv:159.0) Gecko/20100101 Firefox/159.0",
    },
    Preset {
        id: "safari-macos",
        label: "Safari 27 on macOS",
        user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/27.0 Safari/605.1.15",
    },
    Preset {
        id: "safari-iphone",
        label: "Safari 27 on iPhone",
        user_agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_7 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/27.0 Mobile/15E148 Safari/604.1",
    },
];

/// The preset a saved user-agent string belongs to (`None`: the default when empty, or a
/// custom string).
pub fn preset_of(user_agent: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.user_agent == user_agent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use std::collections::HashSet;

    const CHROME: &str = concat!("Chrome/", chrome_major!(), ".0.0.0");
    const WEBKIT: &str = "AppleWebKit/537.36 (KHTML, like Gecko)";

    #[test]
    fn presets_are_valid_settings_values_and_unique() {
        let mut seen = HashSet::new();
        for p in PRESETS {
            let s = Settings { user_agent: p.user_agent.to_string(), ..Settings::default() };
            assert!(s.validate().is_ok(), "{}: {:?}", p.id, s.validate());
            assert!(seen.insert(p.id) && seen.insert(p.user_agent), "duplicate: {}", p.id);
            assert!(p.user_agent.starts_with("Mozilla/5.0 ("), "{}", p.id);
            assert!(!p.label.is_empty());
        }
        assert_eq!(PRESETS.iter().filter(|p| p.user_agent.contains(CHROME)).count(), 5, "4 Chrome + Edge");
        assert!(PRESETS.iter().filter(|p| p.id.starts_with("chrome")).all(|p| p.user_agent.contains(WEBKIT)));
        assert_eq!(PRESETS.iter().filter(|p| p.user_agent.contains("Firefox/159.0")).count(), 3);
        assert_eq!(PRESETS.iter().filter(|p| p.user_agent.contains("Version/27.0")).count(), 2);
        assert!(PRESETS.iter().any(|p| p.user_agent.contains(concat!("Edg/", chrome_major!(), ".0.0.0"))));
        assert!(PRESETS.iter().any(|p| p.user_agent.contains("Android") && p.user_agent.contains("Mobile Safari")));
        assert!(PRESETS.iter().any(|p| p.user_agent.contains("iPhone") && p.user_agent.contains("Mobile/15E148")));
    }

    #[test]
    fn default_identity_is_the_engines_chrome() {
        assert!(CHROME_VERSION.starts_with(concat!(chrome_major!(), ".")));
        assert_eq!(CHROME_VERSION.split('.').count(), 4);
        assert_eq!(effective("", true), CHROME_WINDOWS);
        assert_eq!(effective("  ", false), CHROME_ANDROID);
        assert_eq!(effective("Custom/1", false), "Custom/1");
        assert!(CHROME_WINDOWS.contains("Windows NT 10.0; Win64; x64") && CHROME_WINDOWS.contains("Chrome/156.0.0.0"));
        assert!(CHROME_ANDROID.contains("Mobile Safari/537.36"));
        assert_eq!(
            engine_switches(""),
            [
                "--user-agent-product=Chrome/156.0.0.0",
                "--krunk-ua-brand=Google Chrome",
                "--krunk-ua-version=156.0.8078.80"
            ]
        );
        assert!(engine_switches(PRESETS[5].user_agent).is_empty());
    }

    /// The identity must be the engine's version: a page can tell the engine apart by its features.
    #[test]
    fn identity_matches_the_engine() {
        let lock = include_str!("../../cef.lock");
        let engine = lock
            .lines()
            .find_map(|l| l.strip_prefix("CHROMIUM_VERSION="))
            .expect("cef.lock names the Chromium version")
            .trim();
        assert_eq!(CHROME_VERSION, engine, "bump CHROME_VERSION and chrome_major! with the engine (cef.lock)");
    }

    #[test]
    fn preset_lookup() {
        assert_eq!(preset_of(PRESETS[4].user_agent).map(|p| p.id), Some("edge-windows"));
        assert_eq!(preset_of(""), None);
        assert_eq!(preset_of("Custom/1.0"), None);
        assert_eq!(SOURCES.len(), 4);
    }
}
