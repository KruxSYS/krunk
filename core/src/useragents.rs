//! The user-agent presets of Settings → Advanced (Krunk 0.5.0). Version numbers are current
//! as of 6 November 2026 (sources in `SOURCES`); they are strings the user chooses, not
//! anything Krunk relies on, so an older number only makes a site think the browser is older.

use serde::Serialize;

/// Where the versions came from, for the next update of this table.
pub const SOURCES: [&str; 4] = [
    "Chrome 157 stable, 3 Nov 2026: https://chromereleases.googleblog.com/2026/11/stable-channel-update-for-desktop.html",
    "Edge 157 stable, 5 Nov 2026: https://learn.microsoft.com/en-us/microsoft-edge/web-platform/release-notes/157",
    "Firefox 159.0.1, 3 Nov 2026: https://www.firefox.com/en-US/firefox/159.0.1/releasenotes/",
    "Safari 27.0, 17 Sep 2026 (frozen OS tokens): https://webkit.org/blog/18325/webkit-features-for-safari-27-0/",
];

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
        label: "Chrome 157 on Windows",
        user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/157.0.0.0 Safari/537.36",
    },
    Preset {
        id: "chrome-macos",
        label: "Chrome 157 on macOS",
        user_agent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/157.0.0.0 Safari/537.36",
    },
    Preset {
        id: "chrome-linux",
        label: "Chrome 157 on Linux",
        user_agent: "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/157.0.0.0 Safari/537.36",
    },
    Preset {
        id: "chrome-android",
        label: "Chrome 157 on Android (phone)",
        user_agent: "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/157.0.0.0 Mobile Safari/537.36",
    },
    Preset {
        id: "edge-windows",
        label: "Edge 157 on Windows",
        user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/157.0.0.0 Safari/537.36 Edg/157.0.0.0",
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

    const CHROME: &str = "Chrome/157.0.0.0";
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
        assert!(PRESETS.iter().any(|p| p.user_agent.contains("Edg/157.0.0.0")));
        assert!(PRESETS.iter().any(|p| p.user_agent.contains("Android") && p.user_agent.contains("Mobile Safari")));
        assert!(PRESETS.iter().any(|p| p.user_agent.contains("iPhone") && p.user_agent.contains("Mobile/15E148")));
    }

    #[test]
    fn preset_lookup() {
        assert_eq!(preset_of(PRESETS[4].user_agent).map(|p| p.id), Some("edge-windows"));
        assert_eq!(preset_of(""), None);
        assert_eq!(preset_of("Custom/1.0"), None);
        assert_eq!(SOURCES.len(), 4);
    }
}
