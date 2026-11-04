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
    /// Minutes in the background before a tab's browser view is unloaded.
    pub tab_discard_minutes: u32,

    // Privacy
    pub adblock_enabled: bool,
    pub filter_lists: Vec<String>,
    /// Block all third-party cookies (browser-wide, needs a restart).
    pub block_third_party_cookies: bool,
    pub https_only: bool,
    pub send_dnt: bool,
    pub send_gpc: bool,
    pub strip_tracking_params: bool,
    pub clear_on_exit: ClearOnExit,

    // Passwords
    pub vault_autolock_minutes: u32,
    pub offer_save_passwords: bool,

    // Downloads
    /// Empty = the user's Downloads folder.
    pub download_dir: String,
    pub ask_where_to_save: bool,

    // Advanced
    /// Empty = the engine's default user agent.
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
            block_third_party_cookies: true,
            https_only: false,
            send_dnt: true,
            send_gpc: true,
            strip_tracking_params: true,
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
    "--user-agent",
    "--single-process",
    "--disable-gpu-sandbox",
    "--disable-setuid-sandbox",
    "--no-zygote",
    "--remote-debugging-address",
    "--allow-file-access-from-files",
    "--disable-client-side-phishing-detection",
    "--js-flags",
    "--renderer-cmd-prefix",
    "--gpu-launcher",
    "--utility-cmd-prefix",
    "--browser-subprocess-path",
];

/// Features that must not be switched off through the extra flags.
const PROTECTED_FEATURES: &[&str] = &["IsolateOrigins", "SitePerProcess", "StrictOriginIsolation"];

/// Features we always disable: background traffic and services that a
/// lightweight browser with its own vault does not need.
pub const DISABLED_FEATURES: &[&str] =
    &["AutofillServerCommunication", "MediaRouter", "OptimizationHints", "InterestFeedContentSuggestions"];

/// Switches we always pass (documented in the README).
pub const BASE_SWITCHES: &[&str] = &["--no-pings", "--disable-domain-reliability"];

/// Blocks third-party cookies browser-wide. Chromium's third-party-cookie
/// phase-out switch; a future Chromium may rename it.
pub const BLOCK_THIRD_PARTY_COOKIES_SWITCH: &str = "--test-third-party-cookie-phaseout";

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
            if !(t.contains("%s") || t.contains("{searchTerms}")) {
                err("customSearchUrl", "Must contain %s where the search terms go");
            } else if t.len() > 2048 || !is_web_url(&t.replace("{searchTerms}", "x").replace("%s", "x"), true) {
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

    /// Did a field that only takes effect after a restart change compared
    /// with the settings the browser was started with?
    pub fn restart_required(&self, running: &Settings) -> bool {
        self.browser_arguments() != running.browser_arguments()
    }

    /// The Chromium switches for the engine, as one string (shown on the
    /// About page). Changes only take effect after a restart.
    pub fn browser_arguments(&self) -> String {
        self.browser_switches().join(" ")
    }

    /// The Chromium switches for the engine (`--name` or `--name=value`).
    pub fn browser_switches(&self) -> Vec<String> {
        let mut args: Vec<String> = BASE_SWITCHES.iter().map(|s| s.to_string()).collect();
        if !self.hardware_acceleration {
            args.push("--disable-gpu".into());
        }
        if self.block_third_party_cookies {
            args.push(BLOCK_THIRD_PARTY_COOKIES_SWITCH.into());
        }
        if !self.proxy.is_empty() {
            args.push(format!("--proxy-server={}", self.proxy));
        }
        let mut disabled: Vec<String> = DISABLED_FEATURES.iter().map(|s| s.to_string()).collect();
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
        dedup_keep_first(&mut disabled);
        dedup_keep_first(&mut enabled);
        // A feature both enabled and disabled stays disabled.
        enabled.retain(|f| !disabled.contains(f));
        args.push(format!("--disable-features={}", disabled.join(",")));
        if !enabled.is_empty() {
            args.push(format!("--enable-features={}", enabled.join(",")));
        }
        // The same switch twice (e.g. --disable-gpu from the toggle and the
        // extra flags): keep the first.
        let mut seen = std::collections::HashSet::new();
        args.retain(|a| seen.insert(a.split('=').next().unwrap_or(a).to_ascii_lowercase()));
        args
    }
}

/// How the filter-list updater may reach the network, given the proxy setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdaterProxy {
    /// Use the system proxy configuration.
    System,
    /// Use this `host:port` HTTP proxy.
    Named(String),
    /// Don't download: the browser goes through a SOCKS proxy, which the
    /// updater (WinHTTP) cannot use, and bypassing it would leak the user's IP.
    Skip,
}

pub fn updater_proxy(proxy: &str) -> UpdaterProxy {
    let p = proxy.trim();
    if p.is_empty() || p == "direct://" {
        return UpdaterProxy::System;
    }
    match p.split_once("://") {
        Some(("http" | "https", rest)) => UpdaterProxy::Named(rest.trim_end_matches('/').to_string()),
        Some(_) => UpdaterProxy::Skip,
        None => UpdaterProxy::Named(p.to_string()),
    }
}

fn dedup_keep_first(v: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    v.retain(|x| seen.insert(x.clone()));
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
        // Accepted forms: --flag and --flag=value.
        let (name, value) = match flag.split_once('=') {
            Some((n, v)) => (n, Some(v)),
            None => (flag, None),
        };
        let body = name.strip_prefix("--").unwrap_or("");
        let name_ok = body.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
            && body.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        if !name.starts_with("--") || !name_ok {
            return Err(format!("Not a valid flag (use --flag or --flag=value): {flag}"));
        }
        if let Some(v) = value {
            if v.is_empty() {
                return Err(format!("Missing value after '=': {flag}"));
            }
            if v.chars().any(|c| c.is_control() || matches!(c, '"' | '\'' | '`' | ';' | '&' | '|' | '<' | '>')) {
                return Err(format!("Quotes and shell characters are not allowed: {flag}"));
            }
        }
        let lname = name.to_ascii_lowercase();
        if FORBIDDEN_FLAGS.contains(&lname.as_str()) {
            let hint = match lname.as_str() {
                "--proxy-server" => " (use the proxy field)",
                "--user-agent" => " (use the user agent field)",
                _ => " (weakens security)",
            };
            return Err(format!("{lname} is not allowed{hint}"));
        }
        if lname == "--disable-features" {
            if let Some(f) = value.and_then(|v| v.split(',').find(|f| PROTECTED_FEATURES.contains(f))) {
                return Err(format!("Disabling {f} is not allowed (weakens site isolation)"));
            }
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
        s.custom_search_url = "http://s.example/?q=%s".into();
        assert_eq!(fields(&s), vec!["customSearchUrl"]);
        s.custom_search_url = "https://s.example/?q=".into();
        assert_eq!(fields(&s), vec!["customSearchUrl"], "no placeholder");
        s.custom_search_url = "javascript:alert('%s')".into();
        assert_eq!(fields(&s), vec!["customSearchUrl"]);
        s.custom_search_url = "https://s.example/?q=%s".into();
        assert_eq!(s.validate(), Ok(()));
        assert_eq!(s.search_template(), "https://s.example/?q=%s");
        s.custom_search_url = "https://s.example/?q={searchTerms}".into();
        assert_eq!(s.validate(), Ok(()), "OpenSearch placeholder also accepted");
        // Presets need no custom URL.
        for e in [
            SearchEngine::Google,
            SearchEngine::Bing,
            SearchEngine::Brave,
            SearchEngine::Startpage,
            SearchEngine::Ecosia,
        ] {
            let s = Settings { search_engine: e, custom_search_url: "junk".into(), ..Settings::default() };
            assert_eq!(s.validate(), Ok(()), "{e:?}");
            assert!(s.search_template().starts_with("https://"));
        }
    }

    #[test]
    fn startup_and_homepage() {
        let mut s = Settings { startup: Startup::Homepage, homepage: String::new(), ..Settings::default() };
        assert_eq!(fields(&s), vec!["homepage"], "homepage startup needs a URL");
        s.homepage = "ftp://files.example/".into();
        assert_eq!(fields(&s), vec!["homepage"]);
        s.homepage = "https://start.example/".into();
        assert_eq!(s.validate(), Ok(()));
        let s = Settings { startup: Startup::NewTab, homepage: String::new(), ..Settings::default() };
        assert_eq!(s.validate(), Ok(()), "empty homepage is fine when not used");
    }

    #[test]
    fn sleep_and_discard_ranges() {
        let ok = |sleep, enabled, discard| {
            Settings {
                tab_sleep_minutes: sleep,
                tab_discard_enabled: enabled,
                tab_discard_minutes: discard,
                ..Settings::default()
            }
            .validate()
            .is_ok()
        };
        assert!(ok(0, false, 30), "sleep off");
        assert!(ok(5, false, 1), "discard off: its value is not checked against sleep");
        assert!(ok(1440, false, 30));
        assert!(!ok(1441, false, 30));
        assert!(ok(5, true, 6));
        assert!(!ok(5, true, 5), "discard must be longer than sleep");
        assert!(!ok(5, true, 0));
        assert!(!ok(5, true, 1441));
        assert!(ok(0, true, 1), "sleep off: any valid discard time");
    }

    #[test]
    fn autolock_zoom_ua_dir() {
        let bad = |s: Settings| fields(&s);
        assert_eq!(bad(Settings { vault_autolock_minutes: 0, ..Settings::default() }), ["vaultAutolockMinutes"]);
        assert_eq!(bad(Settings { vault_autolock_minutes: 1441, ..Settings::default() }), ["vaultAutolockMinutes"]);
        assert_eq!(bad(Settings { default_zoom: 0.1, ..Settings::default() }), ["defaultZoom"]);
        assert_eq!(bad(Settings { default_zoom: f64::NAN, ..Settings::default() }), ["defaultZoom"]);
        assert_eq!(bad(Settings { user_agent: "Mozilla/5.0 \u{e9}".into(), ..Settings::default() }), ["userAgent"]);
        assert_eq!(bad(Settings { user_agent: "x".repeat(513), ..Settings::default() }), ["userAgent"]);
        assert_eq!(bad(Settings { download_dir: "C:\\dl\n".into(), ..Settings::default() }), ["downloadDir"]);
        assert!(Settings { user_agent: "Mozilla/5.0 (Windows NT 10.0) Custom/1".into(), ..Settings::default() }
            .validate()
            .is_ok());
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
    fn updater_follows_the_browser_proxy() {
        assert_eq!(updater_proxy(""), UpdaterProxy::System);
        assert_eq!(updater_proxy("direct://"), UpdaterProxy::System);
        assert_eq!(updater_proxy("127.0.0.1:8080"), UpdaterProxy::Named("127.0.0.1:8080".into()));
        assert_eq!(updater_proxy("http://proxy.corp:3128"), UpdaterProxy::Named("proxy.corp:3128".into()));
        assert_eq!(updater_proxy("socks5://127.0.0.1:9050"), UpdaterProxy::Skip);
        assert_eq!(updater_proxy("socks4://h:1"), UpdaterProxy::Skip);
    }

    #[test]
    fn dangerous_flags_rejected() {
        for bad in [
            "--no-sandbox",
            "--NO-SANDBOX",
            "--remote-debugging-port=9222",
            "--user-data-dir=C:\\x",
            "--proxy-server=x:1",
            "--user-agent=Foo",
            "--single-process",
            "--disable-gpu-sandbox",
            "--renderer-cmd-prefix=calc.exe",
            "--disable-features=Foo,IsolateOrigins",
            "--disable-features=SitePerProcess",
        ] {
            assert!(validate_flags(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn malformed_flags_rejected() {
        for bad in [
            "-x",
            "--",
            "--=1",
            "-- x",
            "--a=",
            "--a=\"b\"",
            "--a='b'",
            "--a=b;calc",
            "--a=$(x)|y",
            "--é",
            "--a b=",
            "flag",
            "--a=b\u{7}",
        ] {
            assert!(validate_flags(bad).is_err(), "{bad:?}");
        }
        for good in [
            "--force-dark-mode",
            "--enable-features=Foo,Bar",
            "--lang=de-DE",
            "--force-device-scale-factor=1.25",
            "--log-file=C:\\temp\\x.log",
        ] {
            assert!(validate_flags(good).is_ok(), "{good}");
        }
        assert!(validate_flags(&"--x ".repeat(2000)).is_err(), "length cap");
    }

    #[test]
    fn arguments_have_no_duplicates() {
        let s = Settings {
            hardware_acceleration: false,
            extra_flags:
                "--disable-gpu --no-pings --disable-features=MediaRouter,Foo,Foo --enable-features=Foo,Bar,Bar".into(),
            ..Settings::default()
        };
        let a = s.browser_arguments();
        let names: Vec<&str> = a.split(' ').map(|f| f.split('=').next().unwrap()).collect();
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "{a}");
        let disabled = a.split(' ').find(|f| f.starts_with("--disable-features=")).unwrap();
        assert_eq!(disabled.matches("MediaRouter").count(), 1);
        assert_eq!(disabled.matches("Foo").count(), 1);
        let enabled = a.split(' ').find(|f| f.starts_with("--enable-features=")).unwrap();
        assert_eq!(enabled, "--enable-features=Bar", "Foo stays disabled");
    }

    #[test]
    fn restart_detection() {
        let running = Settings::default();
        let live_only = Settings {
            theme: Theme::Dark,
            user_agent: "UA/1".into(),
            default_zoom: 1.5,
            https_only: true,
            adblock_enabled: false,
            ..Settings::default()
        };
        assert!(!live_only.restart_required(&running), "these apply without a restart");
        for changed in [
            Settings { hardware_acceleration: false, ..Settings::default() },
            Settings { proxy: "127.0.0.1:8080".into(), ..Settings::default() },
            Settings { extra_flags: "--force-dark-mode".into(), ..Settings::default() },
            Settings { block_third_party_cookies: false, ..Settings::default() },
        ] {
            assert!(changed.restart_required(&running), "{changed:?}");
        }
    }

    /// The UI tests use `ui/dev/default-settings.json`; it must match the real
    /// defaults. Regenerate with `LB_UPDATE_FIXTURES=1 cargo test -p core`.
    #[test]
    fn ui_default_settings_fixture_is_current() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/dev/default-settings.json");
        let want = serde_json::to_string_pretty(&Settings::default()).unwrap() + "\n";
        if std::env::var_os("LB_UPDATE_FIXTURES").is_some() {
            std::fs::write(path, &want).unwrap();
        }
        let have = std::fs::read_to_string(path).unwrap_or_default();
        assert_eq!(have, want, "ui/dev/default-settings.json is stale; run with LB_UPDATE_FIXTURES=1");
    }

    #[test]
    fn third_party_cookie_switch() {
        let count = |s: &Settings| {
            s.browser_arguments().split_whitespace().filter(|a| *a == BLOCK_THIRD_PARTY_COOKIES_SWITCH).count()
        };
        let on = Settings::default();
        assert!(on.block_third_party_cookies, "on by default");
        assert_eq!(count(&on), 1);
        // Also typed into the extra flags: still passed exactly once.
        let twice =
            Settings { extra_flags: format!("{BLOCK_THIRD_PARTY_COOKIES_SWITCH} --force-dark-mode"), ..on.clone() };
        assert_eq!(twice.validate(), Ok(()));
        assert_eq!(count(&twice), 1);
        assert!(twice.browser_arguments().contains("--force-dark-mode"));
        let off = Settings { block_third_party_cookies: false, ..on.clone() };
        assert_eq!(count(&off), 0);
        assert!(off.restart_required(&on), "toggling it needs a restart");
    }

    #[test]
    fn older_settings_json_gets_defaults() {
        // A record written by an older version: few fields, one removed field.
        let old = r#"{"searchEngine":"google","theme":"dark","adblockAllowlist":["x.example"],"tabSleepMinutes":10}"#;
        let s = Settings::from_json_lenient(old);
        assert_eq!(s.search_engine, SearchEngine::Google);
        assert_eq!(s.theme, Theme::Dark);
        assert_eq!(s.tab_sleep_minutes, 10);
        let d = Settings::default();
        assert_eq!(s.hardware_acceleration, d.hardware_acceleration);
        assert!(s.block_third_party_cookies, "a settings file from before the toggle blocks third-party cookies");
        assert_eq!(s.vault_autolock_minutes, d.vault_autolock_minutes);
        assert_eq!(s.filter_lists, d.filter_lists);
        assert_eq!(s.validate(), Ok(()));
        // Round trip of the full record.
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(Settings::from_json_lenient(&json), s);
        assert!(!json.contains("adblockAllowlist"), "removed fields are dropped on save");
    }

    #[test]
    fn browser_arguments_reflect_settings() {
        let s = Settings::default();
        let a = s.browser_arguments();
        assert!(!a.contains("--disable-gpu"));
        assert!(a.contains("--no-pings"));

        let s = Settings {
            hardware_acceleration: false,
            proxy: "socks5://127.0.0.1:9050".into(),
            extra_flags: "--force-dark-mode --disable-features=Foo --enable-features=Bar".into(),
            ..Settings::default()
        };
        let a = s.browser_arguments();
        assert!(a.contains("--disable-gpu"));
        assert!(a.contains("--proxy-server=socks5://127.0.0.1:9050"));
        assert!(a.contains("--force-dark-mode"));
        assert_eq!(a.matches("--disable-features=").count(), 1);
        assert!(a.contains(",Foo"));
        assert!(a.contains("--enable-features=Bar"));
    }

    #[test]
    fn settings_from_the_webview2_version_still_load() {
        // Written by the WebView2-based release: Edge-only fields are ignored
        // (and dropped on the next save); everything else is kept.
        let old = r#"{"searchEngine":"brave","trackingPrevention":"balanced","smartscreen":true,
            "blockThirdPartyCookies":false,"sendDnt":false,"defaultZoom":1.25,"theme":"light"}"#;
        let s = Settings::from_json_lenient(old);
        assert_eq!(s.search_engine, SearchEngine::Brave);
        assert!(!s.block_third_party_cookies && !s.send_dnt);
        assert_eq!(s.default_zoom, 1.25);
        assert_eq!(s.theme, Theme::Light);
        assert_eq!(s.validate(), Ok(()));
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("trackingPrevention") && !json.contains("smartscreen"), "{json}");
    }

    #[test]
    fn no_edge_specific_switches() {
        // Every setting that adds switches, turned on.
        let s = Settings {
            hardware_acceleration: false,
            proxy: "127.0.0.1:8080".into(),
            extra_flags: "--force-dark-mode --disable-features=Foo".into(),
            ..Settings::default()
        };
        let switches = s.browser_switches();
        for a in &switches {
            assert!(a.starts_with("--"), "{a}");
            assert!(!a.contains("msSmartScreen") && !a.contains("msEdge") && !a.contains("ms-"), "{a}");
            assert_ne!(a.to_ascii_lowercase(), "--no-sandbox");
        }
        assert_eq!(s.browser_arguments(), switches.join(" "));
        let disabled = switches.iter().find(|a| a.starts_with("--disable-features=")).unwrap();
        assert_eq!(disabled, "--disable-features=AutofillServerCommunication,MediaRouter,OptimizationHints,InterestFeedContentSuggestions,Foo");
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
