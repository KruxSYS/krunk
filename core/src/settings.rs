//! User settings: defaults, validation and the Chromium arguments derived
//! from them.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchEngine {
    DuckDuckGo,
    Google,
    Bing,
    Brave,
    Startpage,
    Ecosia,
    Custom,
}

impl SearchEngine {
    /// Search URL template; `None` for `Custom` (uses `custom_search_url`).
    pub fn template(self) -> Option<&'static str> {
        Some(match self {
            SearchEngine::DuckDuckGo => "https://duckduckgo.com/?q={searchTerms}",
            SearchEngine::Google => "https://www.google.com/search?q={searchTerms}",
            SearchEngine::Bing => "https://www.bing.com/search?q={searchTerms}",
            SearchEngine::Brave => "https://search.brave.com/search?q={searchTerms}",
            SearchEngine::Startpage => "https://www.startpage.com/do/search?q={searchTerms}",
            SearchEngine::Ecosia => "https://www.ecosia.org/search?q={searchTerms}",
            SearchEngine::Custom => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Startup {
    NewTab,
    RestoreSession,
    Homepage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TrackingPrevention {
    None,
    Basic,
    Balanced,
    Strict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    System,
    Light,
    Dark,
}

/// Identifiers of the bundled filter lists (see `assets/filters`).
pub const FILTER_LISTS: &[&str] = &["easylist", "easyprivacy", "ublock-filters", "ublock-privacy", "ublock-unbreak"];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClearOnExit {
    pub history: bool,
    pub cookies: bool,
    pub cache: bool,
    pub downloads: bool,
}

impl ClearOnExit {
    pub fn any(&self) -> bool {
        self.history || self.cookies || self.cache || self.downloads
    }
}

/// All persisted settings. Unknown JSON fields are ignored and missing ones
/// take their default, so older/newer settings files still load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    // Search & startup
    pub search_engine: SearchEngine,
    pub custom_search_url: String,
    pub startup: Startup,
    pub homepage: String,
    pub show_home_button: bool,
    pub show_bookmarks_bar: bool,

    // Performance
    pub hardware_acceleration: bool,
    /// Minutes in the background before a tab is put to sleep; 0 = never.
    pub tab_sleep_minutes: u32,
    pub tab_discard_enabled: bool,
    /// Minutes in the background before a tab's WebView is unloaded.
    pub tab_discard_minutes: u32,

    // Privacy
    pub adblock_enabled: bool,
    pub filter_lists: Vec<String>,
    pub tracking_prevention: TrackingPrevention,
    pub https_only: bool,
    pub send_dnt: bool,
    pub send_gpc: bool,
    pub strip_tracking_params: bool,
    pub smartscreen: bool,
    pub clear_on_exit: ClearOnExit,

    // Passwords
    pub vault_autolock_minutes: u32,
    pub offer_save_passwords: bool,

    // Downloads
    /// Empty = the user's Downloads folder.
    pub download_dir: String,
    pub ask_where_to_save: bool,

    // Advanced
    /// Empty = the WebView2 default user agent.
    pub user_agent: String,
    /// Empty = system proxy settings. Passed as `--proxy-server`.
    pub proxy: String,
    /// Extra Chromium command-line switches, whitespace separated.
    pub extra_flags: String,

    // Appearance
    pub default_zoom: f64,
    pub theme: Theme,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            search_engine: SearchEngine::DuckDuckGo,
            custom_search_url: String::new(),
            startup: Startup::RestoreSession,
            homepage: "https://duckduckgo.com/".into(),
            show_home_button: true,
            show_bookmarks_bar: true,
            hardware_acceleration: true,
            tab_sleep_minutes: 5,
            tab_discard_enabled: false,
            tab_discard_minutes: 30,
            adblock_enabled: true,
            filter_lists: FILTER_LISTS.iter().map(|s| s.to_string()).collect(),
            tracking_prevention: TrackingPrevention::Strict,
            https_only: false,
            send_dnt: true,
            send_gpc: true,
            strip_tracking_params: true,
            smartscreen: false,
            clear_on_exit: ClearOnExit::default(),
            vault_autolock_minutes: 15,
            offer_save_passwords: true,
            download_dir: String::new(),
            ask_where_to_save: false,
            user_agent: String::new(),
            proxy: String::new(),
            extra_flags: String::new(),
            default_zoom: 1.0,
            theme: Theme::System,
        }
    }
}

/// A validation problem with one settings field (camelCase field name).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldError {
    pub field: &'static str,
    pub message: String,
}

/// Switches that would weaken security or hijack the profile; rejected in
/// the "extra flags" field.
const FORBIDDEN_FLAGS: &[&str] = &[
    "--no-sandbox",
    "--disable-web-security",
    "--disable-site-isolation-trials",
    "--remote-debugging-port",
    "--remote-debugging-pipe",
    "--remote-allow-origins",
    "--user-data-dir",
    "--load-extension",
    "--disable-extensions-except",
    "--allow-running-insecure-content",
    "--ignore-certificate-errors",
    "--ignore-certificate-errors-spki-list",
    "--unsafely-treat-insecure-origin-as-secure",
    "--proxy-server",
];

/// Features we always disable: background traffic and services that a
/// lightweight browser with its own vault does not need.
pub const DISABLED_FEATURES: &[&str] = &[
    "AutofillServerCommunication",
    "MediaRouter",
    "OptimizationHints",
    "InterestFeedContentSuggestions",
    "msEdgeShoppingAssistant",
];

/// Switches we always pass (documented in the README).
pub const BASE_SWITCHES: &[&str] = &["--no-pings", "--disable-domain-reliability"];

pub const MAX_SLEEP_MINUTES: u32 = 24 * 60;
pub const MAX_AUTOLOCK_MINUTES: u32 = 24 * 60;

impl Settings {
    /// The search URL template in effect.
    pub fn search_template(&self) -> &str {
        self.search_engine.template().unwrap_or(&self.custom_search_url)
    }

    /// Checks every field; returns all problems at once so the settings page
    /// can mark each offending input.
    pub fn validate(&self) -> Result<(), Vec<FieldError>> {
        let mut errs = Vec::new();
        let mut err = |field: &'static str, message: &str| errs.push(FieldError { field, message: message.into() });

        if self.search_engine == SearchEngine::Custom {
            let t = &self.custom_search_url;
            if !t.contains("{searchTerms}") {
                err("customSearchUrl", "Must contain {searchTerms}");
            } else if !is_web_url(&t.replace("{searchTerms}", "x"), true) {
                err("customSearchUrl", "Must be an https:// URL");
            }
        }
        if (self.startup == Startup::Homepage || !self.homepage.is_empty()) && !is_web_url(&self.homepage, false) {
            err("homepage", "Must be an http:// or https:// URL");
        }
        if self.tab_sleep_minutes > MAX_SLEEP_MINUTES {
            err("tabSleepMinutes", "Must be between 0 and 1440 minutes");
        }
        if self.tab_discard_enabled {
            if self.tab_discard_minutes == 0 || self.tab_discard_minutes > MAX_SLEEP_MINUTES {
                err("tabDiscardMinutes", "Must be between 1 and 1440 minutes");
            } else if self.tab_sleep_minutes != 0 && self.tab_discard_minutes <= self.tab_sleep_minutes {
                err("tabDiscardMinutes", "Must be longer than the sleep timeout");
            }
        }
        if let Some(bad) = self.filter_lists.iter().find(|l| !FILTER_LISTS.contains(&l.as_str())) {
            err("filterLists", &format!("Unknown filter list: {bad}"));
        }
        if self.vault_autolock_minutes == 0 || self.vault_autolock_minutes > MAX_AUTOLOCK_MINUTES {
            err("vaultAutolockMinutes", "Must be between 1 and 1440 minutes");
        }
        if self.download_dir.len() > 1024 || self.download_dir.chars().any(char::is_control) {
            err("downloadDir", "Invalid folder path");
        }
        if self.user_agent.len() > 512 || self.user_agent.chars().any(|c| c.is_control() || !c.is_ascii()) {
            err("userAgent", "Must be printable ASCII, at most 512 characters");
        }
        if !self.proxy.is_empty() && !is_proxy(&self.proxy) {
            err("proxy", "Use host:port, scheme://host:port (http, https, socks4, socks5) or direct://");
        }
        if let Err(m) = validate_flags(&self.extra_flags) {
            err("extraFlags", &m);
        }
        if !(0.25..=5.0).contains(&self.default_zoom) || !self.default_zoom.is_finite() {
            err("defaultZoom", "Must be between 25% and 500%");
        }
        if errs.is_empty() {
            Ok(())
        } else {
            Err(errs)
        }
    }

    /// Loads settings from JSON, resetting any invalid field to its default
    /// instead of failing (a corrupt settings file must not brick startup).
    pub fn from_json_lenient(json: &str) -> Settings {
        let mut s: Settings = serde_json::from_str(json).unwrap_or_default();
        if let Err(errs) = s.validate() {
            let d = Settings::default();
            for e in errs {
                match e.field {
                    "customSearchUrl" => {
                        s.search_engine = d.search_engine;
                        s.custom_search_url = d.custom_search_url.clone();
                    }
                    "homepage" => s.homepage = d.homepage.clone(),
                    "tabSleepMinutes" => s.tab_sleep_minutes = d.tab_sleep_minutes,
                    "tabDiscardMinutes" => {
                        s.tab_discard_enabled = d.tab_discard_enabled;
                        s.tab_discard_minutes = d.tab_discard_minutes;
                    }
                    "filterLists" => s.filter_lists.retain(|l| FILTER_LISTS.contains(&l.as_str())),
                    "vaultAutolockMinutes" => s.vault_autolock_minutes = d.vault_autolock_minutes,
                    "downloadDir" => s.download_dir.clear(),
                    "userAgent" => s.user_agent.clear(),
                    "proxy" => s.proxy.clear(),
                    "extraFlags" => s.extra_flags.clear(),
                    "defaultZoom" => s.default_zoom = d.default_zoom,
                    _ => {}
                }
            }
            if s.startup == Startup::Homepage && !is_web_url(&s.homepage, false) {
                s.startup = d.startup;
            }
        }
        s
    }

    /// The `AdditionalBrowserArguments` for the WebView2 environment.
    /// Changes only take effect after a restart.
    pub fn browser_arguments(&self) -> String {
        let mut args: Vec<String> = BASE_SWITCHES.iter().map(|s| s.to_string()).collect();
        if !self.hardware_acceleration {
            args.push("--disable-gpu".into());
        }
        if !self.proxy.is_empty() {
            args.push(format!("--proxy-server={}", self.proxy));
        }
        let mut disabled: Vec<String> = DISABLED_FEATURES.iter().map(|s| s.to_string()).collect();
        if !self.smartscreen {
            disabled.push("msSmartScreenProtection".into());
        }
        let mut enabled: Vec<String> = Vec::new();
        for flag in self.extra_flags.split_whitespace() {
            // Chromium only honours the last --disable-features/--enable-features,
            // so merge the user's lists into ours.
            if let Some(list) = flag.strip_prefix("--disable-features=") {
                disabled.extend(list.split(',').filter(|f| !f.is_empty()).map(String::from));
            } else if let Some(list) = flag.strip_prefix("--enable-features=") {
                enabled.extend(list.split(',').filter(|f| !f.is_empty()).map(String::from));
            } else {
                args.push(flag.to_string());
            }
        }
        disabled.dedup();
        args.push(format!("--disable-features={}", disabled.join(",")));
        if !enabled.is_empty() {
            args.push(format!("--enable-features={}", enabled.join(",")));
        }
        args.join(" ")
    }
}

fn is_web_url(s: &str, https_only: bool) -> bool {
    match url::Url::parse(s) {
        Ok(u) => {
            u.host_str().is_some_and(|h| !h.is_empty())
                && if https_only { u.scheme() == "https" } else { matches!(u.scheme(), "http" | "https") }
        }
        Err(_) => false,
    }
}

/// Host name or IP literal, optionally a `*.` wildcard prefix.
pub fn is_hostname(h: &str) -> bool {
    let h = h.strip_prefix("*.").unwrap_or(h);
    if h.is_empty() || h.len() > 253 {
        return false;
    }
    if h.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    h.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    })
}

fn is_proxy(p: &str) -> bool {
    if p == "direct://" {
        return true;
    }
    let rest = match p.split_once("://") {
        Some((scheme, rest)) => {
            if !matches!(scheme, "http" | "https" | "socks4" | "socks5") {
                return false;
            }
            rest
        }
        None => p,
    };
    let Some((host, port)) = rest.rsplit_once(':') else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    port.parse::<u16>().is_ok_and(|n| n != 0) && is_hostname(host)
}

fn validate_flags(flags: &str) -> Result<(), String> {
    if flags.len() > 4096 {
        return Err("Too long".into());
    }
    for flag in flags.split_whitespace() {
        if !flag.starts_with("--") || flag.len() < 3 {
            return Err(format!("Each flag must start with --: {flag}"));
        }
        if flag.chars().any(|c| c.is_control() || c == '"' || c == '\'') {
            return Err(format!("Quotes and control characters are not allowed: {flag}"));
        }
        let name = flag.split('=').next().unwrap_or(flag).to_ascii_lowercase();
        if FORBIDDEN_FLAGS.contains(&name.as_str()) {
            let hint = if name == "--proxy-server" { " (use the proxy field)" } else { " (weakens security)" };
            return Err(format!("{name} is not allowed{hint}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(s: &Settings) -> Vec<&'static str> {
        s.validate().err().unwrap_or_default().into_iter().map(|e| e.field).collect()
    }

    #[test]
    fn defaults_are_valid() {
        let s = Settings::default();
        assert_eq!(s.validate(), Ok(()));
        assert_eq!(s.search_engine, SearchEngine::DuckDuckGo);
        assert!(s.hardware_acceleration && s.adblock_enabled);
        assert_eq!(s.tracking_prevention, TrackingPrevention::Strict);
        assert_eq!(s.tab_sleep_minutes, 5);
    }

    #[test]
    fn rejects_bad_values() {
        let s = Settings {
            search_engine: SearchEngine::Custom,
            custom_search_url: "https://s.example/?q=".into(),
            homepage: "javascript:alert(1)".into(),
            tab_sleep_minutes: 5000,
            vault_autolock_minutes: 0,
            proxy: "not a proxy".into(),
            user_agent: "bad\nua".into(),
            default_zoom: 9.0,
            filter_lists: vec!["nope".into()],
            ..Settings::default()
        };
        let f = fields(&s);
        for want in [
            "customSearchUrl",
            "homepage",
            "tabSleepMinutes",
            "vaultAutolockMinutes",
            "proxy",
            "userAgent",
            "defaultZoom",
            "filterLists",
        ] {
            assert!(f.contains(&want), "missing {want} in {f:?}");
        }
    }

    #[test]
    fn custom_search_requires_https_and_placeholder() {
        let mut s = Settings { search_engine: SearchEngine::Custom, ..Settings::default() };
        s.custom_search_url = "http://s.example/?q={searchTerms}".into();
        assert_eq!(fields(&s), vec!["customSearchUrl"]);
        s.custom_search_url = "https://s.example/?q={searchTerms}".into();
        assert_eq!(s.validate(), Ok(()));
        assert_eq!(s.search_template(), "https://s.example/?q={searchTerms}");
    }

    #[test]
    fn discard_must_follow_sleep() {
        let mut s = Settings {
            tab_discard_enabled: true,
            tab_sleep_minutes: 10,
            tab_discard_minutes: 5,
            ..Settings::default()
        };
        assert_eq!(fields(&s), vec!["tabDiscardMinutes"]);
        s.tab_discard_minutes = 60;
        assert_eq!(s.validate(), Ok(()));
    }

    #[test]
    fn proxy_forms() {
        for ok in ["127.0.0.1:8080", "socks5://proxy.local:1080", "http://[::1]:3128", "direct://"] {
            assert!(is_proxy(ok), "{ok}");
        }
        for bad in ["proxy", "ftp://x:1", "x:0", "x:99999", "a b:80", "http://x:80 --no-sandbox"] {
            assert!(!is_proxy(bad), "{bad}");
        }
    }

    #[test]
    fn dangerous_flags_rejected() {
        for bad in [
            "--no-sandbox",
            "--remote-debugging-port=9222",
            "--user-data-dir=C:\\x",
            "-x",
            "--a=\"b\"",
            "--proxy-server=x:1",
        ] {
            assert!(validate_flags(bad).is_err(), "{bad}");
        }
        assert!(validate_flags("--enable-features=Foo --force-dark-mode").is_ok());
    }

    #[test]
    fn browser_arguments_reflect_settings() {
        let s = Settings::default();
        let a = s.browser_arguments();
        assert!(!a.contains("--disable-gpu"));
        assert!(a.contains("--no-pings"));
        assert!(a.contains("msSmartScreenProtection"));

        let s = Settings {
            hardware_acceleration: false,
            smartscreen: true,
            proxy: "socks5://127.0.0.1:9050".into(),
            extra_flags: "--force-dark-mode --disable-features=Foo --enable-features=Bar".into(),
            ..Settings::default()
        };
        let a = s.browser_arguments();
        assert!(a.contains("--disable-gpu"));
        assert!(a.contains("--proxy-server=socks5://127.0.0.1:9050"));
        assert!(a.contains("--force-dark-mode"));
        assert!(!a.contains("msSmartScreenProtection"));
        assert_eq!(a.matches("--disable-features=").count(), 1);
        assert!(a.contains(",Foo"));
        assert!(a.contains("--enable-features=Bar"));
    }

    #[test]
    fn lenient_load_repairs_fields() {
        let s =
            Settings::from_json_lenient(r#"{"tabSleepMinutes": 99999, "proxy": "bad", "theme": "dark", "unknown": 1}"#);
        assert_eq!(s.tab_sleep_minutes, 5);
        assert_eq!(s.proxy, "");
        assert_eq!(s.theme, Theme::Dark);
        assert_eq!(s.validate(), Ok(()));
        assert_eq!(Settings::from_json_lenient("garbage"), Settings::default());
    }

    #[test]
    fn json_round_trip() {
        let s = Settings { theme: Theme::Light, https_only: true, ..Settings::default() };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(s, back);
    }
}
