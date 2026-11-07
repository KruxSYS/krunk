//! User settings: defaults, validation and the Chromium arguments derived
//! from them.

use serde::{Deserialize, Serialize};

/// A filter list the user added by its address (krunk://adblock).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomList {
    pub url: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
}

fn enabled_by_default() -> bool {
    true
}

/// How many lists the user may add.
pub const MAX_CUSTOM_LISTS: usize = 20;

/// Where the bookmarks bar shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BookmarksBar {
    #[default]
    Always,
    /// Only on the new-tab page.
    NewTab,
    Never,
}

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
    /// The engine's name as shown in the address bar's placeholder; `None` for `Custom`.
    pub fn name(self) -> Option<&'static str> {
        Some(match self {
            SearchEngine::DuckDuckGo => "DuckDuckGo",
            SearchEngine::Google => "Google",
            SearchEngine::Bing => "Bing",
            SearchEngine::Brave => "Brave Search",
            SearchEngine::Startpage => "Startpage",
            SearchEngine::Ecosia => "Ecosia",
            SearchEngine::Custom => return None,
        })
    }

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

/// What a new tab opens (Krunk 0.6.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NewTabPage {
    /// Krunk's new tab page (search box and shortcut tiles).
    #[default]
    Krunk,
    /// The homepage setting.
    Homepage,
    /// An empty page.
    Blank,
    /// `new_tab_url`.
    Custom,
}

/// Where a new tab goes in the strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NewTabPosition {
    #[default]
    End,
    AfterCurrent,
}

/// Whether a link opened in a new tab (middle click, Ctrl+click, `target=_blank`) becomes
/// the active tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LinkTabs {
    /// As the page asked: `target=_blank` in front, middle click and Ctrl+click behind.
    #[default]
    Auto,
    Switch,
    Background,
}

/// What Krunk does when a site asks for a permission and nothing is stored for the site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionDefault {
    #[default]
    Ask,
    Block,
}

/// Per-permission defaults (Krunk 0.6.0); everything asks unless changed.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PermissionDefaults {
    pub camera: PermissionDefault,
    pub microphone: PermissionDefault,
    pub geolocation: PermissionDefault,
    pub notifications: PermissionDefault,
    pub clipboard_read: PermissionDefault,
    pub midi: PermissionDefault,
    pub screen_share: PermissionDefault,
    pub local_network: PermissionDefault,
}

impl PermissionDefaults {
    pub fn get(&self, kind: crate::permissions::Kind) -> PermissionDefault {
        use crate::permissions::Kind as K;
        match kind {
            K::Camera => self.camera,
            K::Microphone => self.microphone,
            K::Geolocation => self.geolocation,
            K::Notifications => self.notifications,
            K::ClipboardRead => self.clipboard_read,
            K::Midi => self.midi,
            K::ScreenShare => self.screen_share,
            K::LocalNetwork => self.local_network,
        }
    }
}

/// Tab-strip density (Krunk 0.6.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Density {
    #[default]
    Normal,
    Compact,
}

/// Appearance options beyond the theme (Krunk 0.6.0). Colours are `#rrggbb` or empty for
/// the theme's own; everything applies live (CSS variables in the shell, popup and pages).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Appearance {
    /// Accent colour (links, highlights, the active toggle); empty = the theme's.
    pub accent: String,
    pub toolbar_color: String,
    pub strip_color: String,
    pub background_color: String,
    pub density: Density,
    /// UI font size in CSS px (12–16; 13 is the design size).
    pub ui_font_px: u8,
    pub rounded_tabs: bool,
    pub show_downloads_button: bool,
    pub show_zoom_chip: bool,
    pub show_extensions_button: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Appearance {
            accent: String::new(),
            toolbar_color: String::new(),
            strip_color: String::new(),
            background_color: String::new(),
            density: Density::Normal,
            ui_font_px: 13,
            rounded_tabs: true,
            show_downloads_button: true,
            show_zoom_chip: true,
            show_extensions_button: true,
        }
    }
}

/// The new tab page's background (Krunk 0.6.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NtpBackground {
    #[default]
    None,
    Color,
    /// The image the user picked, stored in the profile (`ntp-background.<ext>`).
    Image,
}

/// Which tiles the new tab page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TileSource {
    /// Pinned tiles, then the most visited sites.
    #[default]
    MostVisited,
    /// Only the user's own (pinned) list.
    Custom,
}

/// New tab page options (Krunk 0.6.0); the defaults are the 0.5 page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Ntp {
    pub background: NtpBackground,
    pub background_color: String,
    /// The stored image's file name (set by the host when one is picked).
    pub image: String,
    /// Darkening over the image, 0–80 percent.
    pub image_dim: u8,
    /// Blur over the image, 0–20 px.
    pub image_blur: u8,
    pub tiles: TileSource,
    pub columns: u8,
    pub rows: u8,
    pub show_shortcuts: bool,
    pub show_search: bool,
    pub show_tiles: bool,
}

impl Default for Ntp {
    fn default() -> Self {
        Ntp {
            background: NtpBackground::None,
            background_color: String::new(),
            image: String::new(),
            image_dim: 30,
            image_blur: 0,
            tiles: TileSource::MostVisited,
            columns: 6,
            rows: 2,
            show_shortcuts: true,
            show_search: true,
            show_tiles: true,
        }
    }
}

impl Ntp {
    /// Tiles the page shows at most.
    pub fn tile_count(&self) -> usize {
        usize::from(self.columns.clamp(1, 12)) * usize::from(self.rows.clamp(1, 6))
    }
}

/// `#rrggbb` (lower or upper case), or empty.
pub fn is_hex_color(s: &str) -> bool {
    s.is_empty() || (s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit()))
}

/// Where DevTools is docked next to its tab's page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DevToolsDock {
    #[default]
    Right,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    System,
    Light,
    Dark,
}

/// Identifiers of the bundled filter lists (see `assets/filters`).
pub const FILTER_LISTS: &[&str] = &[
    "easylist",
    "easyprivacy",
    "ublock-filters",
    "ublock-privacy",
    "ublock-unbreak",
    "ublock-quick-fixes",
    "ublock-badware",
];

/// Lists added after the first release, by revision: settings saved before
/// a revision get its lists enabled once (the user never saw them).
const LISTS_ADDED: &[(u32, &[&str])] = &[(1, &["ublock-quick-fixes", "ublock-badware"])];
/// The current filter-list revision (see `Settings::filter_lists_rev`).
pub const FILTER_LISTS_REV: u32 = 1;

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
    /// Replaces 0.3's `showBookmarksBar` (false became `never`; see `from_json_lenient`).
    pub bookmarks_bar: BookmarksBar,

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
    /// Which lists of `FILTER_LISTS` existed when these settings were saved
    /// (newer ones are enabled once on load).
    /// Lists the user added by address (in addition to the built-in ones).
    pub custom_lists: Vec<CustomList>,
    pub filter_lists_rev: u32,
    /// Block all third-party cookies: network cookies through the engine's
    /// cookie filter (`core::cookies`, applies at once), script access
    /// (`document.cookie`) through Chromium's switch (needs a restart).
    pub block_third_party_cookies: bool,
    pub https_only: bool,
    pub send_dnt: bool,
    pub send_gpc: bool,
    pub strip_tracking_params: bool,
    /// Let the engine download its security components from Google
    /// (certificate revocation list, Certificate Transparency list). Off:
    /// no contact with Google at all; these lists then only update with
    /// Krunk releases. Needs a restart.
    pub engine_security_components: bool,
    pub clear_on_exit: ClearOnExit,
    /// Permission defaults (Krunk 0.6.0).
    pub permissions: PermissionDefaults,
    /// Tracking protection (Krunk 0.6.0). `block_third_party_cookies` and
    /// `strip_tracking_params` above are derived from it (see `sync_protection`).
    pub protection: crate::protection::Protection,
    /// Request headers (Krunk 0.6.0): Accept-Language, client hints, custom rules. The
    /// user-agent string itself is `user_agent` below.
    pub headers: crate::headers::Headers,
    /// Custom DNS (Krunk 0.6.0).
    pub dns: crate::dns::Dns,

    // Passwords
    pub vault_autolock_minutes: u32,
    pub offer_save_passwords: bool,

    // Downloads
    /// Empty = the user's Downloads folder.
    pub download_dir: String,
    pub ask_where_to_save: bool,
    /// Connections a large download may use when its server sends parts (1 = the engine's
    /// own single download; see `segdl`).
    pub download_connections: u32,

    // Advanced
    /// Empty = the engine's default user agent.
    pub user_agent: String,
    /// Empty = system proxy settings. Passed as `--proxy-server`.
    pub proxy: String,
    /// Extra Chromium command-line switches, whitespace separated.
    pub extra_flags: String,

    // New tabs (Krunk 0.6.0; the defaults are the 0.5 behaviour)
    pub new_tab_page: NewTabPage,
    /// The address `NewTabPage::Custom` opens.
    pub new_tab_url: String,
    pub new_tab_position: NewTabPosition,
    pub link_tabs: LinkTabs,
    /// Where the new tab page's search box sends its words: empty = the default search
    /// engine, else a search URL template with `%s` or `{searchTerms}`.
    pub ntp_search_url: String,

    // Appearance
    pub default_zoom: f64,
    pub theme: Theme,
    pub appearance: Appearance,
    /// The new tab page's options.
    pub ntp: Ntp,

    // Updates
    /// Check `<update server>/updates/win-x64/latest.json` a minute after
    /// start and once a day (see `core::update`).
    pub check_updates_automatically: bool,

    // DevTools (set by dragging its splitter and its dock control, not on the settings page;
    // the host keeps its own values when the page saves)
    pub devtools_dock: DevToolsDock,
    /// Width of DevTools docked right, DIP.
    pub devtools_size_right: f64,
    /// Height of DevTools docked at the bottom, DIP.
    pub devtools_size_bottom: f64,
}

/// DevTools sizes kept in settings are within this range (DIP).
pub const DEVTOOLS_SIZE_RANGE: std::ops::RangeInclusive<f64> = 100.0..=4000.0;

impl Default for Settings {
    fn default() -> Self {
        Settings {
            search_engine: SearchEngine::DuckDuckGo,
            custom_search_url: String::new(),
            startup: Startup::RestoreSession,
            homepage: "https://duckduckgo.com/".into(),
            bookmarks_bar: BookmarksBar::Always,
            hardware_acceleration: true,
            tab_sleep_minutes: 5,
            tab_discard_enabled: false,
            tab_discard_minutes: 30,
            adblock_enabled: true,
            filter_lists: FILTER_LISTS.iter().map(|s| s.to_string()).collect(),
            custom_lists: Vec::new(),
            filter_lists_rev: FILTER_LISTS_REV,
            block_third_party_cookies: false,
            protection: crate::protection::Protection::default(),
            headers: crate::headers::Headers::default(),
            dns: crate::dns::Dns::default(),
            https_only: false,
            send_dnt: true,
            send_gpc: true,
            strip_tracking_params: true,
            engine_security_components: false,
            clear_on_exit: ClearOnExit::default(),
            permissions: PermissionDefaults::default(),
            vault_autolock_minutes: 15,
            offer_save_passwords: true,
            download_dir: String::new(),
            ask_where_to_save: false,
            download_connections: 4,
            user_agent: String::new(),
            proxy: String::new(),
            extra_flags: String::new(),
            new_tab_page: NewTabPage::Krunk,
            new_tab_url: String::new(),
            new_tab_position: NewTabPosition::End,
            link_tabs: LinkTabs::Auto,
            ntp_search_url: String::new(),
            default_zoom: 1.0,
            theme: Theme::System,
            appearance: Appearance::default(),
            ntp: Ntp::default(),
            check_updates_automatically: true,
            devtools_dock: DevToolsDock::Right,
            devtools_size_right: 480.0,
            devtools_size_bottom: 320.0,
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
/// lightweight browser with its own vault does not need. Each entry was
/// found with the logging-proxy audit (`.pipeline/cef-network-audit.md`).
pub const DISABLED_FEATURES: &[&str] = &[
    "AutofillServerCommunication",
    "MediaRouter",
    "OptimizationHints",
    "InterestFeedContentSuggestions",
    // Secure-time queries to clients2.google.com/time.
    "NetworkTimeServiceQuerying",
    // "AI mode" eligibility request to www.google.com at start-up.
    "AimEnabled",
    "AimServerEligibilityEnabled",
    "AimServerRequestOnStartupEnabled",
];

/// Switches we always pass (documented in the README).
pub const BASE_SWITCHES: &[&str] = &["--no-pings", "--disable-domain-reliability", "--disable-background-networking"];

/// No component updater: no contact with Google's update servers. Left out
/// when the user opts in to the engine's security components.
pub const NO_COMPONENT_UPDATE_SWITCH: &str = "--disable-component-update";

/// Hosts the engine must never reach: our internal origins (the engine
/// preconnects to them before our handler answers the request) and, unless
/// the user opted in, the component updater's server (one component is
/// installed "on demand" and ignores `--disable-component-update`).
pub const INTERNAL_HOSTS: &[&str] = &["krunk.invalid", "*.krunk.invalid"];
pub const UPDATER_HOSTS: &[&str] = &["update.googleapis.com"];

/// Blocks third-party cookies for scripts (`document.cookie`). Chromium's
/// third-party-cookie phase-out switch; a future Chromium may rename it.
/// Network cookies are filtered by the browser itself (`core::cookies`).
pub const BLOCK_THIRD_PARTY_COOKIES_SWITCH: &str = "--test-third-party-cookie-phaseout";

pub const MAX_SLEEP_MINUTES: u32 = 24 * 60;
pub const MAX_AUTOLOCK_MINUTES: u32 = 24 * 60;

impl Settings {
    /// The search URL template in effect.
    pub fn search_template(&self) -> &str {
        self.search_engine.template().unwrap_or(&self.custom_search_url)
    }

    /// What a new tab opens: `None` for Krunk's new tab page, else the address.
    pub fn new_tab_target(&self) -> Option<String> {
        match self.new_tab_page {
            NewTabPage::Krunk => None,
            NewTabPage::Homepage => Some(self.homepage.clone()),
            NewTabPage::Blank => Some("about:blank".into()),
            NewTabPage::Custom => {
                let t = self.new_tab_url.trim();
                Some(crate::pages::Page::from_display(t).map(|p| p.url()).unwrap_or_else(|| t.to_string()))
            }
        }
    }

    /// The search template the new tab page's box uses.
    pub fn ntp_search_template(&self) -> &str {
        let t = self.ntp_search_url.trim();
        if t.is_empty() {
            self.search_template()
        } else {
            t
        }
    }

    /// The name of the search engine in effect: the preset's name, or the custom
    /// template's host (without `www.`), or "your search engine".
    pub fn search_name(&self) -> String {
        if let Some(n) = self.search_engine.name() {
            return n.to_string();
        }
        url::Url::parse(&self.custom_search_url)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_string()))
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| "your search engine".to_string())
    }

    /// Checks every field; returns all problems at once so the settings page
    /// can mark each offending input.
    pub fn validate(&self) -> Result<(), Vec<FieldError>> {
        let mut errs = Vec::new();
        let mut err = |field: &'static str, message: &str| errs.push(FieldError { field, message: message.into() });

        if self.new_tab_page == NewTabPage::Custom {
            let t = self.new_tab_url.trim();
            let ok = url::Url::parse(t).map(|u| matches!(u.scheme(), "http" | "https")).unwrap_or(false)
                || crate::pages::Page::from_display(t).is_some();
            if !ok {
                err("newTabUrl", "Enter a web address (https://…) or a krunk:// page");
            }
        }
        if !self.ntp_search_url.trim().is_empty() {
            let t = self.ntp_search_url.trim();
            let ok = (t.contains("%s") || t.contains("{searchTerms}"))
                && url::Url::parse(t).map(|u| matches!(u.scheme(), "http" | "https")).unwrap_or(false);
            if !ok {
                err("ntpSearchUrl", "Enter a search URL with %s where the words go");
            }
        }
        for (field, value) in [
            ("appearance.accent", &self.appearance.accent),
            ("appearance.toolbarColor", &self.appearance.toolbar_color),
            ("appearance.stripColor", &self.appearance.strip_color),
            ("appearance.backgroundColor", &self.appearance.background_color),
            ("ntp.backgroundColor", &self.ntp.background_color),
        ] {
            if !is_hex_color(value) {
                err(field, "Enter a colour like #2563eb (or leave it empty)");
            }
        }
        if !(12..=16).contains(&self.appearance.ui_font_px) {
            err("appearance.uiFontPx", "Between 12 and 16 px");
        }
        if !(1..=12).contains(&self.ntp.columns) || !(1..=6).contains(&self.ntp.rows) {
            err("ntp.columns", "1–12 columns and 1–6 rows");
        }
        if self.ntp.image_dim > 80 || self.ntp.image_blur > 20 {
            err("ntp.imageDim", "Dim up to 80 %, blur up to 20 px");
        }
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
        if self.custom_lists.len() > MAX_CUSTOM_LISTS {
            err("customLists", "At most 20 lists of your own");
        } else if self.custom_lists.iter().enumerate().any(|(i, l)| {
            !is_web_url(&l.url, true)
                || l.url.len() > 2048
                || crate::blocker::is_builtin_list_url(&l.url)
                || self.custom_lists[..i].iter().any(|o| o.url == l.url)
        }) {
            err("customLists", "Each list needs its own https:// address");
        }
        if !(1..=crate::segdl::MAX_CONNECTIONS).contains(&self.download_connections) {
            err("downloadConnections", "Must be between 1 and 16");
        }
        if self.user_agent.len() > 512 || self.user_agent.chars().any(|c| c.is_control() || !c.is_ascii()) {
            err("userAgent", "Must be printable ASCII, at most 512 characters");
        }
        if let Err(errs) = self.headers.validate() {
            for (field, message) in errs {
                err(field, &message);
            }
        }
        if let Err(e) = self.dns.validate() {
            err("dns", &e);
        }
        if !self.proxy.is_empty() && !is_proxy(&self.proxy) {
            err("proxy", "Use host:port, scheme://host:port (http, https, socks4, socks5) or direct://");
        }
        for (field, v) in
            [("devtoolsSizeRight", self.devtools_size_right), ("devtoolsSizeBottom", self.devtools_size_bottom)]
        {
            if !v.is_finite() || !DEVTOOLS_SIZE_RANGE.contains(&v) {
                err(field, "Must be between 100 and 4000");
            }
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

    /// Derives the two 0.5 toggles from the protection level in force, so the code paths
    /// that read them (the engine switch, navigation rewriting) follow the level.
    pub fn sync_protection(&mut self) {
        let e = self.protection.effective();
        self.block_third_party_cookies = e.third_party_cookies == crate::protection::ThirdPartyCookies::All;
        self.strip_tracking_params = e.strip_params;
    }

    /// The filter lists the engine is built from: the user's choice plus the lists the
    /// protection level adds (cryptomining, fingerprinting).
    pub fn effective_filter_lists(&self) -> Vec<String> {
        let mut lists = self.filter_lists.clone();
        for extra in self.protection.extra_lists() {
            if !lists.iter().any(|l| l == extra) {
                lists.push(extra.to_string());
            }
        }
        lists
    }

    /// Loads settings from JSON, resetting any invalid field to its default
    /// instead of failing (a corrupt settings file must not brick startup).
    pub fn from_json_lenient(json: &str) -> Settings {
        let mut s: Settings = serde_json::from_str(json).unwrap_or_default();
        // 0.3 and earlier: `showBookmarksBar: false` hid the bar.
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(json) {
            if v.get("bookmarksBar").is_none() && v.get("showBookmarksBar") == Some(&serde_json::Value::Bool(false)) {
                s.bookmarks_bar = BookmarksBar::Never;
            }
        }
        // Filter lists added since these settings were saved start enabled.
        let saved_rev = serde_json::from_str::<serde_json::Value>(json)
            .ok()
            .and_then(|v| v.get("filterListsRev").and_then(serde_json::Value::as_u64))
            .unwrap_or(0) as u32;
        for (rev, lists) in LISTS_ADDED {
            if saved_rev < *rev {
                for l in *lists {
                    if !s.filter_lists.iter().any(|x| x == l) {
                        s.filter_lists.push(l.to_string());
                    }
                }
            }
        }
        s.filter_lists_rev = FILTER_LISTS_REV;
        // 0.5 and earlier had two toggles instead of the protection levels.
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(json) {
            if v.get("protection").is_none() {
                let b = v.get("blockThirdPartyCookies").and_then(serde_json::Value::as_bool).unwrap_or(true);
                let p = v.get("stripTrackingParams").and_then(serde_json::Value::as_bool).unwrap_or(true);
                s.protection = crate::protection::Protection::from_legacy(b, p);
            }
        }
        s.sync_protection();
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
                    "downloadConnections" => s.download_connections = d.download_connections,
                    "customLists" => s.custom_lists.clear(),
                    "userAgent" => s.user_agent.clear(),
                    "headers.acceptLanguage" => s.headers.accept_language.clear(),
                    "headers.hints" => s.headers.hints = None,
                    "headers.rules" => s.headers.rules.clear(),
                    "dns" => s.dns = crate::dns::Dns::default(),
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
        let mut unreachable: Vec<&str> = INTERNAL_HOSTS.to_vec();
        if !self.engine_security_components {
            args.push(NO_COMPONENT_UPDATE_SWITCH.into());
            unreachable.extend(UPDATER_HOSTS);
        }
        // DNS never answers for these hosts...
        let rules: Vec<String> = unreachable.iter().map(|h| format!("MAP {h} ~NOTFOUND")).collect();
        args.push(format!("--host-resolver-rules={}", rules.join(", ")));
        if !self.hardware_acceleration {
            args.push("--disable-gpu".into());
        }
        if self.block_third_party_cookies {
            args.push(BLOCK_THIRD_PARTY_COOKIES_SWITCH.into());
        }
        if !self.proxy.is_empty() {
            args.push(format!("--proxy-server={}", self.proxy));
            // ...and they never go to the proxy, which would resolve them.
            args.push(format!("--proxy-bypass-list={}", unreachable.join(";")));
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
    fn new_tab_settings_default_to_the_old_behaviour_and_validate() {
        // A 0.5 settings file has none of the new-tab fields.
        let s = Settings::from_json_lenient(r#"{"searchEngine":"google","homepage":"https://h.example/"}"#);
        assert_eq!(s.new_tab_page, NewTabPage::Krunk);
        assert_eq!(s.new_tab_position, NewTabPosition::End);
        assert_eq!(s.link_tabs, LinkTabs::Auto);
        assert_eq!(s.new_tab_url, "");
        assert_eq!(s.ntp_search_url, "");
        assert_eq!(s.new_tab_target(), None);
        assert_eq!(s.ntp_search_template(), "https://www.google.com/search?q={searchTerms}");
        assert!(s.validate().is_ok());
        let mut s = s;
        s.new_tab_page = NewTabPage::Homepage;
        assert_eq!(s.new_tab_target().as_deref(), Some("https://h.example/"));
        s.new_tab_page = NewTabPage::Blank;
        assert_eq!(s.new_tab_target().as_deref(), Some("about:blank"));
        s.new_tab_page = NewTabPage::Custom;
        s.new_tab_url = "krunk://history".into();
        assert_eq!(s.new_tab_target().as_deref(), Some(crate::pages::Page::History.url().as_str()));
        s.new_tab_url = "not a url".into();
        assert_eq!(fields(&s), vec!["newTabUrl"]);
        s.new_tab_url = "https://start.example/".into();
        s.ntp_search_url = "https://s.example/?q=%s".into();
        assert!(s.validate().is_ok());
        assert_eq!(s.ntp_search_template(), "https://s.example/?q=%s");
        s.ntp_search_url = "https://s.example/".into();
        assert_eq!(fields(&s), vec!["ntpSearchUrl"]);
        // Round trip keeps the camelCase names the settings page uses.
        s.ntp_search_url.clear();
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"newTabPage\":\"custom\"") && json.contains("\"linkTabs\":\"auto\""), "{json}");
    }

    #[test]
    fn permission_defaults_all_ask() {
        let s = Settings::from_json_lenient(r#"{"theme":"dark"}"#);
        for k in crate::permissions::Kind::ALL {
            assert_eq!(s.permissions.get(k), PermissionDefault::Ask, "{k:?}");
        }
        let s = Settings::from_json_lenient(r#"{"permissions":{"camera":"block","midi":"block"}}"#);
        assert_eq!(s.permissions.get(crate::permissions::Kind::Camera), PermissionDefault::Block);
        assert_eq!(s.permissions.get(crate::permissions::Kind::Geolocation), PermissionDefault::Ask);
        assert!(serde_json::to_string(&s).unwrap().contains("\"clipboardRead\":\"ask\""));
    }

    #[test]
    fn appearance_and_ntp_defaults_and_validation() {
        let s = Settings::from_json_lenient(r#"{"theme":"dark"}"#);
        assert_eq!(s.appearance, Appearance::default());
        assert_eq!(s.ntp, Ntp::default());
        assert_eq!(s.ntp.tile_count(), 12);
        assert!(s.validate().is_ok());
        let mut s = s;
        s.appearance.accent = "#AbCdEf".into();
        s.ntp.background = NtpBackground::Color;
        s.ntp.background_color = "#123456".into();
        assert!(s.validate().is_ok());
        s.appearance.accent = "red".into();
        s.appearance.ui_font_px = 20;
        s.ntp.columns = 0;
        s.ntp.image_dim = 90;
        assert_eq!(fields(&s), vec!["appearance.accent", "appearance.uiFontPx", "ntp.columns", "ntp.imageDim"]);
        assert!(is_hex_color("") && is_hex_color("#ff0000") && !is_hex_color("#ff00") && !is_hex_color("ff0000"));
        let json = serde_json::to_string(&Settings::default()).unwrap();
        assert!(
            json.contains("\"appearance\":{\"accent\":\"\"") && json.contains("\"ntp\":{\"background\":\"none\""),
            "{json}"
        );
    }

    #[test]
    fn search_name_follows_the_engine() {
        let mut s = Settings::default();
        assert_eq!(s.search_name(), "DuckDuckGo");
        s.search_engine = SearchEngine::Brave;
        assert_eq!(s.search_name(), "Brave Search");
        s.search_engine = SearchEngine::Custom;
        s.custom_search_url = "https://www.searx.example/search?q={searchTerms}".into();
        assert_eq!(s.search_name(), "searx.example");
        s.custom_search_url = "not a url".into();
        assert_eq!(s.search_name(), "your search engine");
    }

    #[test]
    fn bookmarks_bar_mode_and_migration() {
        assert_eq!(Settings::default().bookmarks_bar, BookmarksBar::Always);
        // 0.3: the bar hidden with a bool.
        let hidden = Settings::from_json_lenient(r#"{"showBookmarksBar":false,"theme":"dark"}"#);
        assert_eq!(hidden.bookmarks_bar, BookmarksBar::Never);
        let shown = Settings::from_json_lenient(r#"{"showBookmarksBar":true}"#);
        assert_eq!(shown.bookmarks_bar, BookmarksBar::Always);
        // The new field wins over a stale old one.
        let both = Settings::from_json_lenient(r#"{"showBookmarksBar":false,"bookmarksBar":"newTab"}"#);
        assert_eq!(both.bookmarks_bar, BookmarksBar::NewTab);
        let json = serde_json::to_string(&both).unwrap();
        assert!(json.contains(r#""bookmarksBar":"newTab""#) && !json.contains("showBookmarksBar"), "{json}");
    }

    /// 0.4 and earlier had a Home button with `showHomeButton`; 0.5 removed it.
    /// Old files still load, the field is ignored, and saving drops it.
    #[test]
    fn home_button_setting_is_ignored_and_dropped() {
        let old = Settings::from_json_lenient(
            r#"{"showHomeButton":false,"homepage":"https://a.example/","theme":"dark","startup":"homepage"}"#,
        );
        assert_eq!(old.homepage, "https://a.example/");
        assert_eq!(old.theme, Theme::Dark);
        assert_eq!(old.startup, Startup::Homepage);
        let strict: Settings = serde_json::from_str(r#"{"showHomeButton":true}"#).unwrap();
        assert_eq!(strict, Settings::default());
        let json = serde_json::to_string(&old).unwrap();
        assert!(!json.contains("showHomeButton") && !json.contains("homeButton"), "{json}");
    }

    #[test]
    fn devtools_layout_settings() {
        let d = Settings::default();
        assert_eq!(
            (d.devtools_dock, d.devtools_size_right, d.devtools_size_bottom),
            (DevToolsDock::Right, 480.0, 320.0)
        );
        // Settings saved by 0.2.x have none of these: defaults.
        let old: Settings = serde_json::from_str(r#"{"theme":"dark","homepage":"https://duckduckgo.com/"}"#).unwrap();
        assert_eq!(old.devtools_dock, DevToolsDock::Right);
        let s: Settings = serde_json::from_str(r#"{"devtoolsDock":"bottom","devtoolsSizeBottom":250}"#).unwrap();
        assert_eq!((s.devtools_dock, s.devtools_size_bottom), (DevToolsDock::Bottom, 250.0));
        let bad = Settings { devtools_size_right: 5.0, ..Settings::default() };
        assert!(bad.validate().unwrap_err().iter().any(|e| e.field == "devtoolsSizeRight"));
        let nan = Settings { devtools_size_bottom: f64::NAN, ..Settings::default() };
        assert!(nan.validate().is_err());
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
        assert_eq!(bad(Settings { download_connections: 0, ..Settings::default() }), ["downloadConnections"]);
        let list = |u: &str| CustomList { url: u.into(), enabled: true };
        assert!(Settings { custom_lists: vec![list("https://l.example/a.txt")], ..Settings::default() }
            .validate()
            .is_ok());
        for lists in [
            vec![list("http://l.example/a.txt")],
            vec![list("https://l.example/a.txt"), list("https://l.example/a.txt")],
            vec![list("https://easylist.to/easylist/easylist.txt")],
            (0..21).map(|i| list(&format!("https://l.example/{i}.txt"))).collect(),
        ] {
            assert_eq!(bad(Settings { custom_lists: lists, ..Settings::default() }), ["customLists"]);
        }
        let back = Settings::from_json_lenient(r#"{"customLists":[{"url":"javascript:x"}]}"#);
        assert!(back.custom_lists.is_empty());
        let kept = Settings::from_json_lenient(r#"{"customLists":[{"url":"https://l.example/a.txt"}]}"#);
        assert_eq!(kept.custom_lists, vec![list("https://l.example/a.txt")], "enabled when not said");
        assert_eq!(bad(Settings { download_connections: 17, ..Settings::default() }), ["downloadConnections"]);
        assert!(Settings { download_connections: 1, ..Settings::default() }.validate().is_ok());
        assert_eq!(Settings::from_json_lenient(r#"{"downloadConnections":99}"#).download_connections, 4);
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
        let a = s.browser_switches();
        let names: Vec<&str> = a.iter().map(|f| f.split('=').next().unwrap()).collect();
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(names.len(), unique.len(), "{a:?}");
        let disabled = a.iter().find(|f| f.starts_with("--disable-features=")).unwrap();
        assert_eq!(disabled.matches("MediaRouter").count(), 1);
        assert_eq!(disabled.matches("Foo").count(), 1);
        let enabled = a.iter().find(|f| f.starts_with("--enable-features=")).unwrap();
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
            Settings { block_third_party_cookies: true, ..Settings::default() },
        ] {
            assert!(changed.restart_required(&running), "{changed:?}");
        }
        // The level, not the derived toggle, is what the page sends: Strict blocks every
        // third-party cookie through the engine switch, so it needs a restart.
        let mut strict = Settings {
            protection: crate::protection::Protection::preset(crate::protection::Level::Strict),
            ..Settings::default()
        };
        strict.sync_protection();
        assert!(strict.block_third_party_cookies && strict.restart_required(&running));
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
        let on = Settings { block_third_party_cookies: true, ..Settings::default() };
        assert!(!Settings::default().block_third_party_cookies, "Standard blocks trackers' cookies, not all");
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
        assert_eq!(
            s.protection,
            crate::protection::Protection::default(),
            "a 0.5 file with the default toggles is Standard"
        );
        assert!(!s.block_third_party_cookies, "…which blocks trackers' cookies, not all");
        // 0.5 toggles the user changed survive as a Custom level.
        let custom = Settings::from_json_lenient(r#"{"blockThirdPartyCookies":false,"stripTrackingParams":false}"#);
        assert_eq!(custom.protection.level, crate::protection::Level::Custom);
        assert!(!custom.protection.strip_params && !custom.strip_tracking_params);
        assert_eq!(custom.protection.third_party_cookies, crate::protection::ThirdPartyCookies::Off);
        let all = Settings::from_json_lenient(r#"{"blockThirdPartyCookies":true,"stripTrackingParams":false}"#);
        assert!(all.block_third_party_cookies, "the derived toggle follows the level");
        let lists = Settings::from_json_lenient(r#"{"protection":{"level":"strict"}}"#);
        assert!(lists.effective_filter_lists().iter().any(|l| l == "krunk-fingerprinting"));
        assert!(!Settings::default().effective_filter_lists().iter().any(|l| l == "krunk-fingerprinting"));
        assert!(Settings::default().effective_filter_lists().iter().any(|l| l == "nocoin"));
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
        assert!(disabled.ends_with(",AimServerRequestOnStartupEnabled,Foo"), "{disabled}");
    }

    #[test]
    fn new_filter_lists_are_enabled_once() {
        // Saved by a version before quick fixes and badware existed.
        let old = r#"{"filterLists":["easylist","ublock-filters"]}"#;
        let s = Settings::from_json_lenient(old);
        assert_eq!(s.filter_lists, ["easylist", "ublock-filters", "ublock-quick-fixes", "ublock-badware"]);
        assert_eq!(s.filter_lists_rev, FILTER_LISTS_REV);
        // Turned off later by the user: stays off.
        let mut s2 = s.clone();
        s2.filter_lists.retain(|l| l != "ublock-badware");
        let saved = serde_json::to_string(&s2).unwrap();
        assert!(!Settings::from_json_lenient(&saved).filter_lists.iter().any(|l| l == "ublock-badware"));
        // Defaults include every list.
        assert_eq!(Settings::default().filter_lists.len(), FILTER_LISTS.len());
    }

    #[test]
    fn no_background_contact_by_default() {
        let d = Settings::default();
        assert!(!d.engine_security_components, "off by default");
        let a = d.browser_switches();
        for want in [
            "--disable-background-networking",
            "--disable-component-update",
            "--no-pings",
            "--disable-domain-reliability",
            "--host-resolver-rules=MAP krunk.invalid ~NOTFOUND, MAP *.krunk.invalid ~NOTFOUND, \
             MAP update.googleapis.com ~NOTFOUND",
        ] {
            assert!(a.iter().any(|s| s == want), "missing {want} in {a:?}");
        }
        let disabled = a.iter().find(|f| f.starts_with("--disable-features=")).unwrap();
        for f in ["NetworkTimeServiceQuerying", "AimServerRequestOnStartupEnabled"] {
            assert!(disabled.split([',', '=']).any(|x| x == f), "missing {f}");
        }
        // No proxy: nothing to bypass.
        assert!(!a.iter().any(|s| s.starts_with("--proxy-bypass-list")));
    }

    #[test]
    fn engine_components_opt_in() {
        let on = Settings { engine_security_components: true, ..Settings::default() };
        let a = on.browser_switches();
        assert!(!a.iter().any(|s| s == NO_COMPONENT_UPDATE_SWITCH), "the updater may run");
        assert!(
            a.contains(&"--host-resolver-rules=MAP krunk.invalid ~NOTFOUND, MAP *.krunk.invalid ~NOTFOUND".to_string()),
            "and reach its server"
        );
        assert!(a.iter().any(|s| s == "--disable-background-networking"), "everything else stays off");
        assert!(a.iter().any(|f| f.starts_with("--disable-features=") && f.contains("NetworkTimeServiceQuerying")));
        assert!(on.restart_required(&Settings::default()), "needs a restart");
        // Old settings files have no such field: off.
        assert!(!Settings::from_json_lenient(r#"{"theme":"dark"}"#).engine_security_components);
    }

    #[test]
    fn internal_origins_bypass_the_proxy() {
        let s = Settings { proxy: "127.0.0.1:8888".into(), ..Settings::default() };
        let a = s.browser_switches();
        assert!(a.contains(&"--proxy-bypass-list=krunk.invalid;*.krunk.invalid;update.googleapis.com".to_string()));
        // A user's own host rules cannot replace ours (first one wins).
        let s = Settings { extra_flags: "--host-resolver-rules=MAP*~NOTFOUND".into(), ..Settings::default() };
        let rules: Vec<_> =
            s.browser_switches().into_iter().filter(|x| x.starts_with("--host-resolver-rules")).collect();
        assert_eq!(rules.len(), 1);
        assert!(rules[0].starts_with("--host-resolver-rules=MAP krunk.invalid"));
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
