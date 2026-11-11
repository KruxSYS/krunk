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
    /// 0.15.0: how the tab strip is filled (`ombre` spreads the gradient across the strip).
    pub tab_style: BarStyle,
    /// 0.15.0: how the toolbar is filled (`ombre` is not offered for it).
    pub toolbar_style: BarStyle,
    /// Gradient stops (`#rrggbb`; empty = derived from the accent/theme). `gradient_via` empty
    /// means a two-stop gradient.
    pub gradient_from: String,
    pub gradient_via: String,
    pub gradient_to: String,
    /// Gradient angle in degrees (0–360).
    pub gradient_angle: u16,
    /// A curated look (`crate::style::PRESETS`), empty = the user's own choices.
    pub preset: String,
    pub tab_shape: TabShape,
    /// Corner radius in px for `TabShape::Rounded` (0–16).
    pub tab_radius: u8,
    pub active_highlight: ActiveHighlight,
    /// A soft accent-coloured glow around the active tab.
    pub accent_glow: bool,
    /// 0.15.2: the outline of `ActiveHighlight::Outline` (`#rrggbb`, empty = the accent) and its
    /// width in px (1–4).
    pub active_outline_color: String,
    pub active_outline_width: u8,
    /// 0.15.2: the tab titles' colours (`#rrggbb`, empty = readable on the strip), for the
    /// other tabs and for the active one.
    pub tab_text_color: String,
    pub active_tab_text_color: String,
}

/// How a bar (tab strip or toolbar) is filled (0.15.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BarStyle {
    #[default]
    Solid,
    Gradient,
    /// The gradient spans the whole tab strip; each tab shows its slice.
    Ombre,
    /// Translucent: blur behind it where Windows supports it, otherwise semi-transparent.
    Glass,
}

/// The tabs' outline (0.15.0; replaces `rounded_tabs`, which is kept for older versions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TabShape {
    #[default]
    Rounded,
    Square,
    Pill,
}

/// How the active tab stands out (0.15.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActiveHighlight {
    #[default]
    Fill,
    Underline,
    Outline,
    /// 0.15.2: nothing beyond the title's colour (and the glow, if on).
    None,
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
            tab_style: BarStyle::Solid,
            toolbar_style: BarStyle::Solid,
            gradient_from: String::new(),
            gradient_via: String::new(),
            gradient_to: String::new(),
            gradient_angle: 90,
            preset: String::new(),
            tab_shape: TabShape::Rounded,
            tab_radius: 8,
            active_highlight: ActiveHighlight::Fill,
            accent_glow: false,
            active_outline_color: String::new(),
            active_outline_width: 2,
            tab_text_color: String::new(),
            active_tab_text_color: String::new(),
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
    /// One of Krunk's built-in wallpapers (`Ntp::wallpaper`; 0.7.0).
    Wallpaper,
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
    /// The built-in wallpaper's id (`assets::wallpapers`) when `background` is `Wallpaper`.
    pub wallpaper: String,
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
            wallpaper: String::new(),
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
    /// Switching away from a tab that plays a video puts the video in the floating window
    /// (Picture-in-Picture) and takes it back when the tab is shown again (Krunk 0.6.0).
    pub auto_pip: bool,
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
    /// Protected content (Krunk 0.6.0): lets sites play DRM media with Google's Widevine
    /// module, which the engine's component updater downloads into the profile (so the
    /// updater runs, as with `engine_security_components`). Needs a restart.
    pub protected_content: bool,
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
    /// "Remain unlocked this session" in the unlock dialog (0.11.0): the user's last choice,
    /// the dialog's default. Checked, an unlock lasts until Krunk quits (no inactivity lock).
    pub vault_remain_unlocked: bool,
    pub offer_save_passwords: bool,
    /// Offer to save a payment card after a checkout (Krunk 0.6.0).
    pub offer_save_cards: bool,
    /// The password generator (Krunk 0.6.0).
    pub passgen: crate::passgen::Passgen,

    // Downloads
    /// Empty = the user's Downloads folder.
    pub download_dir: String,
    pub ask_where_to_save: bool,
    /// Connections a large download may use when its server sends parts (1 = the engine's
    /// own single download; see `segdl`).
    pub download_connections: u32,
    /// PDFs open in Krunk's viewer, also those a site sends as a download (0.11.3); the viewer
    /// has a Download button. Off: they are saved like any download.
    pub pdf_in_browser: bool,

    // Email links (0.11.3)
    pub mail_handler: crate::mailto::MailHandler,
    /// The address `MailHandler::Custom` opens, with `%s` where the link goes.
    pub mail_custom_url: String,

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

    // 0.12.0: the optional AI agent, userscripts and macros (all device-local, never synced).
    pub ai: Ai,
    pub userscripts: UserscriptsSettings,
    pub macros: MacrosSettings,

    // 0.14.0: the in-browser VPN (device-local, never synced; the profiles are in the database).
    pub vpn: VpnSettings,

    // 0.15.0
    /// Hovering a tab shows a card with its title, site and a thumbnail.
    pub tab_hover_preview: bool,
    /// The address bar asks the search engine for suggestions (never in private tabs).
    pub search_suggestions: bool,
    /// Finished downloads older than this many days leave the list at start; 0 = never.
    pub download_auto_clear_days: u32,
    pub session_log: SessionLogSettings,
}

/// Session logs (0.15.0, `krunk://sessions`). Off by default: while off nothing is sampled,
/// buffered or written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionLogSettings {
    pub enabled: bool,
    /// Sessions older than this many days are deleted (1–365).
    pub retention_days: u32,
}

impl Default for SessionLogSettings {
    fn default() -> Self {
        SessionLogSettings { enabled: false, retention_days: 30 }
    }
}

/// Session-log retention limits (days).
pub const SESSION_RETENTION_RANGE: std::ops::RangeInclusive<u32> = 1..=365;
/// Download auto-clear: at most ten years.
pub const MAX_DOWNLOAD_AUTO_CLEAR_DAYS: u32 = 3650;

/// The in-browser VPN (0.14.0): only Krunk's own connections go through it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VpnSettings {
    /// The toolbar's VPN button (off: no button; Settings still connects).
    pub show_button: bool,
    /// Connect to `active_profile` when Krunk starts, before any page loads.
    pub auto_connect: bool,
    /// The profile used last (set by the host when it connects; the popup's default).
    pub active_profile: String,
    /// 0.14.2: addresses of the local network ([`LOCAL_NETWORK_BYPASS`]) skip the VPN, so
    /// routers, printers and home servers stay reachable while connected. Off by default.
    pub allow_local_network: bool,
}

/// What skips the VPN with `vpn.allow_local_network` (0.14.2), in Chromium's proxy-bypass
/// syntax: names without a dot, the usual local suffixes, and the private, link-local and
/// unique-local address ranges (they match addresses typed or linked as such; a name is matched
/// by its suffix, not by what it resolves to). 100.64.0.0/10 is left out on purpose: VPN
/// providers use it inside the tunnel (e.g. for their DNS).
pub const LOCAL_NETWORK_BYPASS: &[&str] = &[
    "<local>",
    "*.local",
    "*.lan",
    "*.home.arpa",
    "*.internal",
    "10.0.0.0/8",
    "172.16.0.0/12",
    "192.168.0.0/16",
    "169.254.0.0/16",
    "fc00::/7",
    "fe80::/10",
];

/// The AI agent (0.12.0). Off by default: while off there is no toolbar button, no agent and
/// no provider traffic. API keys are not here: they are DPAPI-protected in the database.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Ai {
    pub enabled: bool,
    pub providers: Vec<AiProvider>,
    /// The id of the provider and the model the agent uses.
    pub active_provider: String,
    pub active_model: String,
    /// Added to the system prompt.
    pub system_prompt_extra: String,
    /// Tool calls before the agent asks whether to continue.
    pub max_steps: u32,
    /// The agent panel's width (DIP), set by dragging its splitter.
    pub panel_width: f64,
}

impl Default for Ai {
    fn default() -> Self {
        Ai {
            enabled: false,
            providers: Vec::new(),
            active_provider: String::new(),
            active_model: String::new(),
            system_prompt_extra: String::new(),
            max_steps: 30,
            panel_width: crate::layout::PANEL_DEFAULT_DIP,
        }
    }
}

/// A configured model provider. Its key (if any) is stored separately under its `id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProvider {
    /// 8 lower-case hex digits.
    pub id: String,
    pub kind: crate::agent::ProviderKind,
    pub name: String,
    /// Only `custom` uses its own; the named providers have a fixed one.
    #[serde(default)]
    pub base_url: String,
    /// The user's answer to "does the model take images?"; `None` = what the model list says.
    #[serde(default)]
    pub vision: Option<bool>,
    /// 0.15.0: the output cap sent with each request; 0 = auto (OpenAI-compatible providers
    /// omit the field, Anthropic gets the model's known maximum or `DEFAULT_MAX_OUTPUT_TOKENS`).
    /// The model's known maximum caps it either way.
    #[serde(default = "default_max_output_tokens")]
    pub max_output_tokens: u32,
}

/// The default output cap (0.15.0; it was a fixed 4096).
pub const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 32_000;
/// The highest output cap a user can set.
pub const MAX_OUTPUT_TOKENS_LIMIT: u32 = 1_000_000;

fn default_max_output_tokens() -> u32 {
    DEFAULT_MAX_OUTPUT_TOKENS
}

impl Ai {
    pub fn provider(&self, id: &str) -> Option<&AiProvider> {
        self.providers.iter().find(|p| p.id == id)
    }
}

impl Ai {
    /// Every problem, as (field, message).
    pub fn problems(&self) -> Vec<(&'static str, String)> {
        use crate::agent::ProviderKind;
        let mut out = Vec::new();
        let providers = || -> Result<(), String> {
            if self.providers.len() > MAX_AI_PROVIDERS {
                return Err(format!("At most {MAX_AI_PROVIDERS} providers"));
            }
            let mut ids = std::collections::HashSet::new();
            for p in &self.providers {
                if p.id.len() != 8 || !p.id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
                    return Err("A provider has an invalid id".into());
                }
                if !ids.insert(p.id.as_str()) {
                    return Err("Two providers have the same id".into());
                }
                if p.name.trim().is_empty() || p.name.chars().count() > 60 || p.name.chars().any(char::is_control) {
                    return Err("A provider name must be 1 to 60 characters".into());
                }
                if p.max_output_tokens > MAX_OUTPUT_TOKENS_LIMIT {
                    return Err(format!("Max output tokens is at most {MAX_OUTPUT_TOKENS_LIMIT} (0 = auto)"));
                }
                match p.kind {
                    ProviderKind::Custom => crate::agent::provider::check_base_url(p.kind, &p.base_url)?,
                    _ => {
                        if !p.base_url.is_empty() && p.base_url != p.kind.default_base_url() {
                            return Err(format!("{} uses its own address", p.kind.label()));
                        }
                    }
                }
            }
            if !self.active_provider.is_empty() && self.provider(&self.active_provider).is_none() {
                return Err("The active provider does not exist".into());
            }
            Ok(())
        };
        if let Err(m) = providers() {
            out.push(("ai.providers", m));
        }
        if self.active_model.chars().count() > 200 || self.active_model.chars().any(char::is_control) {
            out.push(("ai.activeModel", "A model id is at most 200 printable characters".into()));
        }
        if self.system_prompt_extra.chars().count() > MAX_SYSTEM_PROMPT_EXTRA {
            out.push(("ai.systemPromptExtra", format!("At most {MAX_SYSTEM_PROMPT_EXTRA} characters")));
        }
        if !MAX_STEPS_RANGE.contains(&self.max_steps) {
            out.push(("ai.maxSteps", "Must be between 1 and 200".into()));
        }
        if !self.panel_width.is_finite() || !(100.0..=4000.0).contains(&self.panel_width) {
            out.push(("ai.panelWidth", "Must be between 100 and 4000".into()));
        }
        out
    }
}

/// Up to this many providers; prompt additions up to this long.
pub const MAX_AI_PROVIDERS: usize = 16;
pub const MAX_SYSTEM_PROMPT_EXTRA: usize = 4000;
pub const MAX_STEPS_RANGE: std::ops::RangeInclusive<u32> = 1..=200;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UserscriptsSettings {
    pub enabled: bool,
    /// Userscripts also run in private tabs.
    pub private_tabs: bool,
}

impl Default for UserscriptsSettings {
    fn default() -> Self {
        UserscriptsSettings { enabled: true, private_tabs: false }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MacrosSettings {
    pub enabled: bool,
}

impl Default for MacrosSettings {
    fn default() -> Self {
        MacrosSettings { enabled: true }
    }
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
            auto_pip: false,
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
            protected_content: false,
            clear_on_exit: ClearOnExit::default(),
            permissions: PermissionDefaults::default(),
            vault_autolock_minutes: 15,
            vault_remain_unlocked: true,
            offer_save_passwords: true,
            offer_save_cards: true,
            passgen: crate::passgen::Passgen::default(),
            download_dir: String::new(),
            ask_where_to_save: false,
            download_connections: 4,
            pdf_in_browser: true,
            mail_handler: crate::mailto::MailHandler::System,
            mail_custom_url: String::new(),
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
            ai: Ai::default(),
            userscripts: UserscriptsSettings::default(),
            macros: MacrosSettings::default(),
            vpn: VpnSettings::default(),
            tab_hover_preview: true,
            search_suggestions: true,
            download_auto_clear_days: 0,
            session_log: SessionLogSettings::default(),
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
    // 0.14.0: these change how (or whether) the engine uses a proxy, so the VPN's proxy could
    // be bypassed or replaced; WebRTC's policy is the VPN's to set.
    "--no-proxy-server",
    "--proxy-pac-url",
    "--proxy-auto-detect",
    "--proxy-bypass-list",
    "--winhttp-proxy-resolver",
    "--force-webrtc-ip-handling-policy",
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

/// Switches whose value is a comma-separated list. Chromium keeps only the last occurrence of a
/// switch (`base::CommandLine`), so every source (the engine's own start-up switches, Krunk's,
/// the user's extra flags) is merged into one value instead of appended again (0.7.0: CEF's
/// own `--disable-features` used to be silently replaced by Krunk's).
pub const LIST_SWITCHES: &[&str] =
    &["disable-features", "enable-features", "disable-blink-features", "enable-blink-features"];

/// Merges two values of a list switch: the items of `existing` first, then the new ones from
/// `ours`, each once, empty items dropped.
pub fn merge_switch_list(existing: &str, ours: &str) -> String {
    let mut items: Vec<String> =
        existing.split(',').chain(ours.split(',')).map(str::trim).filter(|f| !f.is_empty()).map(String::from).collect();
    dedup_keep_first(&mut items);
    items.join(",")
}

/// Switches we always pass (documented in the README). 0.8.0: the engine's own HTTP sign-in
/// dialog is off, so a site's Basic/Digest challenge reaches Krunk, whose dialog can fill logins
/// from the vault (`browser::http_auth`).
pub const BASE_SWITCHES: &[&str] =
    &["--no-pings", "--disable-domain-reliability", "--disable-background-networking", "--disable-chrome-login-prompt"];

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
    /// The search engine's name for menus ("Search DuckDuckGo for …"); a custom engine is
    /// named by its host.
    pub fn search_engine_label(&self) -> String {
        match self.search_engine.name() {
            Some(n) => n.to_string(),
            None => crate::urlclean::host_of(&self.custom_search_url.replace("{searchTerms}", "x").replace("%s", "x"))
                .map(|h| h.trim_start_matches("www.").to_string())
                .unwrap_or_else(|| "the web".into()),
        }
    }

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
        if self.ntp.background == NtpBackground::Wallpaper && crate::assets::wallpaper(&self.ntp.wallpaper).is_none() {
            err("ntp.wallpaper", "Choose one of the built-in wallpapers");
        }
        if self.mail_handler == crate::mailto::MailHandler::Custom
            && !crate::mailto::valid_template(&self.mail_custom_url)
        {
            err("mailCustomUrl", "Enter a web address with %s where the email link goes");
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
        for (field, value) in [
            ("appearance.gradientFrom", &self.appearance.gradient_from),
            ("appearance.gradientVia", &self.appearance.gradient_via),
            ("appearance.gradientTo", &self.appearance.gradient_to),
            ("appearance.activeOutlineColor", &self.appearance.active_outline_color),
            ("appearance.tabTextColor", &self.appearance.tab_text_color),
            ("appearance.activeTabTextColor", &self.appearance.active_tab_text_color),
        ] {
            if !is_hex_color(value) {
                err(field, "Enter a colour like #2563eb (or leave it empty)");
            }
        }
        if self.appearance.gradient_angle > 360 {
            err("appearance.gradientAngle", "Between 0 and 360 degrees");
        }
        if !(1..=4).contains(&self.appearance.active_outline_width) {
            err("appearance.activeOutlineWidth", "Between 1 and 4 px");
        }
        if self.appearance.tab_radius > 16 {
            err("appearance.tabRadius", "Between 0 and 16 px");
        }
        if self.appearance.toolbar_style == BarStyle::Ombre {
            err("appearance.toolbarStyle", "Ombre is for the tab strip; choose gradient for the toolbar");
        }
        if !self.appearance.preset.is_empty() && crate::style::preset(&self.appearance.preset).is_none() {
            err("appearance.preset", "Not one of Krunk's styles");
        }
        if !SESSION_RETENTION_RANGE.contains(&self.session_log.retention_days) {
            err("sessionLog.retentionDays", "Between 1 and 365 days");
        }
        if self.download_auto_clear_days > MAX_DOWNLOAD_AUTO_CLEAR_DAYS {
            err("downloadAutoClearDays", "At most 3650 days (0 = never)");
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
        if let Err(e) = self.passgen.validate() {
            err("passgen.length", &e);
        }
        if !self.proxy.is_empty() && !is_proxy(&self.proxy) {
            err("proxy", "Use host:port, scheme://host:port (http, https, socks4, socks5) or direct://");
        }
        // 0.14.0: the VPN is the engine's proxy while it runs; a manual one would take its place.
        if !self.vpn.active_profile.is_empty() && !crate::vpn::valid_id(&self.vpn.active_profile) {
            err("vpn.activeProfile", "Not a VPN profile");
        }
        if self.vpn.auto_connect {
            if self.vpn.active_profile.is_empty() {
                err("vpn.autoConnect", "Connect to a profile once first: Krunk connects to the one used last");
            }
            if !self.proxy.trim().is_empty() {
                err("vpn.autoConnect", "A manual proxy is set (Advanced): the VPN can't be used with it");
                err("proxy", "The VPN connects at start: turn that off first (Privacy & security → VPN)");
            }
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
        for (field, message) in self.ai.problems() {
            err(field, &message);
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
        // 0.14 and earlier: `appearance.roundedTabs: false` meant square tabs.
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(json) {
            if let Some(a) = v.get("appearance") {
                if a.get("tabShape").is_none() && a.get("roundedTabs") == Some(&serde_json::Value::Bool(false)) {
                    s.appearance.tab_shape = TabShape::Square;
                }
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
                    "ntp.wallpaper" => {
                        s.ntp.background = d.ntp.background;
                        s.ntp.wallpaper = d.ntp.wallpaper.clone();
                    }
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
                    "passgen.length" => s.passgen = crate::passgen::Passgen::default(),
                    // A valid proxy stays; the VPN's auto-connect goes instead (below).
                    "proxy" if !is_proxy(&s.proxy) => s.proxy.clear(),
                    "vpn.activeProfile" => s.vpn = VpnSettings::default(),
                    "vpn.autoConnect" => s.vpn.auto_connect = false,
                    "extraFlags" => s.extra_flags.clear(),
                    "defaultZoom" => s.default_zoom = d.default_zoom,
                    "ai.providers" => {
                        s.ai.providers.clear();
                        s.ai.active_provider.clear();
                    }
                    "ai.systemPromptExtra" => s.ai.system_prompt_extra.clear(),
                    "ai.maxSteps" => s.ai.max_steps = d.ai.max_steps,
                    "ai.panelWidth" => s.ai.panel_width = d.ai.panel_width,
                    "ai.activeModel" => s.ai.active_model.clear(),
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

    /// Hosts the engine must never reach (`INTERNAL_HOSTS`, and `UPDATER_HOSTS` unless the
    /// user opted in to the engine's components): DNS never answers for them and no proxy gets
    /// them.
    pub fn unreachable_hosts(&self) -> Vec<&'static str> {
        let mut hosts: Vec<&'static str> = INTERNAL_HOSTS.to_vec();
        if !self.engine_security_components && !self.protected_content {
            hosts.extend(UPDATER_HOSTS);
        }
        hosts
    }

    /// The `bypass_list` of the engine's proxy while the VPN runs (0.14.0): the same hosts as
    /// the manual proxy's `--proxy-bypass-list` (loopback is bypassed implicitly).
    pub fn vpn_bypass_list(&self) -> String {
        self.vpn_bypass_list_with(false)
    }

    /// The same, plus the local network when it may skip the VPN (0.14.2). The flag comes from
    /// the live settings, the hosts above from the running ones (they follow switches).
    pub fn vpn_bypass_list_with(&self, local_network: bool) -> String {
        let mut hosts = self.unreachable_hosts();
        if local_network {
            hosts.extend(LOCAL_NETWORK_BYPASS);
        }
        hosts.join(";")
    }

    /// The Chromium switches for the engine (`--name` or `--name=value`).
    pub fn browser_switches(&self) -> Vec<String> {
        let mut args: Vec<String> = BASE_SWITCHES.iter().map(|s| s.to_string()).collect();
        let unreachable = self.unreachable_hosts();
        if !self.engine_security_components && !self.protected_content {
            args.push(NO_COMPONENT_UPDATE_SWITCH.into());
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
        // The list switches, in LIST_SWITCHES order; the user's lists are merged into ours.
        let mut lists: Vec<String> = vec![DISABLED_FEATURES.join(","), String::new(), String::new(), String::new()];
        for flag in self.extra_flags.split_whitespace() {
            let (name, value) = flag.trim_start_matches('-').split_once('=').unwrap_or((flag, ""));
            match LIST_SWITCHES.iter().position(|l| l.eq_ignore_ascii_case(name)) {
                Some(i) if flag.starts_with("--") => lists[i] = merge_switch_list(&lists[i], value),
                _ => args.push(flag.to_string()),
            }
        }
        // A feature both enabled and disabled stays disabled.
        let disabled: Vec<&str> = lists[0].split(',').collect();
        lists[1] = lists[1].split(',').filter(|f| !disabled.contains(f)).collect::<Vec<_>>().join(",");
        for (name, list) in LIST_SWITCHES.iter().zip(&lists) {
            if !list.is_empty() {
                args.push(format!("--{name}={list}"));
            }
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
    /// The VPN's local proxy (`127.0.0.1:<port>`, 0.14.0): like `Named`, but nothing bypasses
    /// it (no `<local>` list).
    Vpn(String),
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

/// How Krunk's own requests (updates, sync, filter lists, extensions) reach the network: through
/// the VPN's local proxy (`host:port`) while it runs (0.14.0), else as the proxy setting says.
pub fn krunk_proxy(proxy: &str, vpn: Option<&str>) -> UpdaterProxy {
    match vpn {
        Some(addr) => UpdaterProxy::Vpn(addr.to_string()),
        None => updater_proxy(proxy),
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

/// The engine switches that would let the engine's traffic around the VPN's proxy (0.14.0):
/// they are refused in the extra flags, and the VPN refuses to connect while one is in the
/// settings or in the flags the engine runs with (an older settings file).
pub const VPN_CONFLICTING_FLAGS: &[&str] = &[
    "--proxy-server",
    "--no-proxy-server",
    "--proxy-pac-url",
    "--proxy-auto-detect",
    "--proxy-bypass-list",
    "--winhttp-proxy-resolver",
    "--force-webrtc-ip-handling-policy",
];

/// The first of [`VPN_CONFLICTING_FLAGS`] in `flags` (as written), if any.
pub fn vpn_conflicting_flag(flags: &str) -> Option<String> {
    flags.split_whitespace().find_map(|flag| {
        let name = flag.split_once('=').map_or(flag, |(n, _)| n).to_ascii_lowercase();
        if !name.starts_with('-') {
            return None;
        }
        // Chromium takes a single dash too.
        let name = format!("--{}", name.trim_start_matches('-'));
        VPN_CONFLICTING_FLAGS.contains(&name.as_str()).then_some(name)
    })
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
                "--proxy-server"
                | "--no-proxy-server"
                | "--proxy-pac-url"
                | "--proxy-auto-detect"
                | "--proxy-bypass-list"
                | "--winhttp-proxy-resolver" => " (use the proxy field)",
                "--force-webrtc-ip-handling-policy" => " (weakens privacy)",
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

    /// 0.14.2: the local network skips the VPN only when asked; it is off by default.
    #[test]
    fn vpn_local_network_bypass() {
        let s = Settings::default();
        assert!(!s.vpn.allow_local_network);
        let off = s.vpn_bypass_list_with(false);
        let on = s.vpn_bypass_list_with(true);
        assert_eq!(off, s.vpn_bypass_list());
        assert!(!off.contains("192.168.0.0/16") && !off.contains("<local>"));
        assert!(on.starts_with(&off) && on.contains(";192.168.0.0/16") && on.contains(";<local>"));
        assert!(!on.contains("100.64."), "the providers' in-tunnel range stays in the VPN");
        let parsed: Settings = serde_json::from_str(r#"{"vpn":{"allowLocalNetwork":true}}"#).unwrap();
        assert!(parsed.vpn.allow_local_network);
    }
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
    fn krunk_requests_follow_the_vpn() {
        assert_eq!(krunk_proxy("", Some("127.0.0.1:53127")), UpdaterProxy::Vpn("127.0.0.1:53127".into()));
        // SOCKS can't be used by WinHTTP, but the VPN's proxy can: nothing is skipped or direct.
        assert_eq!(krunk_proxy("socks5://h:1", Some("127.0.0.1:9")), UpdaterProxy::Vpn("127.0.0.1:9".into()));
        assert_eq!(krunk_proxy("", None), UpdaterProxy::System);
        assert_eq!(krunk_proxy("socks5://h:1", None), UpdaterProxy::Skip);
    }

    #[test]
    fn vpn_bypasses_what_the_manual_proxy_bypasses() {
        let s = Settings::default();
        assert_eq!(s.vpn_bypass_list(), "krunk.invalid;*.krunk.invalid;update.googleapis.com");
        let manual = Settings { proxy: "127.0.0.1:8888".into(), ..Settings::default() };
        let want = format!("--proxy-bypass-list={}", s.vpn_bypass_list());
        assert!(manual.browser_switches().contains(&want));
        let opted = Settings { engine_security_components: true, ..Settings::default() };
        assert_eq!(opted.vpn_bypass_list(), "krunk.invalid;*.krunk.invalid");
    }

    #[test]
    fn vpn_settings_validation() {
        let d = Settings::default();
        assert_eq!(
            d.vpn,
            VpnSettings {
                show_button: false,
                auto_connect: false,
                active_profile: String::new(),
                allow_local_network: false
            }
        );
        let j = serde_json::to_value(&d).unwrap();
        assert_eq!(
            j["vpn"],
            serde_json::json!({ "showButton": false, "autoConnect": false, "activeProfile": "", "allowLocalNetwork": false })
        );
        // Auto-connect needs a profile.
        let mut s =
            Settings { vpn: VpnSettings { auto_connect: true, ..VpnSettings::default() }, ..Settings::default() };
        assert_eq!(fields(&s), vec!["vpn.autoConnect"]);
        s.vpn.active_profile = "0123456789abcdef0123456789abcdef".into();
        assert_eq!(s.validate(), Ok(()));
        // Not together with a manual proxy.
        s.proxy = "127.0.0.1:8080".into();
        assert_eq!(fields(&s), vec!["vpn.autoConnect", "proxy"]);
        s.vpn.auto_connect = false;
        assert_eq!(s.validate(), Ok(()));
        s.vpn.active_profile = "../x".into();
        assert_eq!(fields(&s), vec!["vpn.activeProfile"]);
        // A stored combination of both keeps the proxy and drops the auto-connect.
        let l = Settings::from_json_lenient(
            r#"{"proxy":"127.0.0.1:8080","vpn":{"showButton":true,"autoConnect":true,"activeProfile":"ab12"}}"#,
        );
        assert_eq!(l.proxy, "127.0.0.1:8080");
        assert_eq!(
            l.vpn,
            VpnSettings {
                show_button: true,
                auto_connect: false,
                active_profile: "ab12".into(),
                allow_local_network: false
            }
        );
        let l = Settings::from_json_lenient(r#"{"vpn":{"showButton":true,"autoConnect":true,"activeProfile":"A/B"}}"#);
        assert_eq!(l.vpn, VpnSettings::default());
        // Older settings have none: the defaults.
        let old: Settings = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert_eq!(old.vpn, VpnSettings::default());
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

    /// 0.14.0: the switches that would take the engine's traffic around the VPN's proxy.
    #[test]
    fn proxy_and_webrtc_flags_rejected_and_found() {
        for bad in [
            "--no-proxy-server",
            "--proxy-pac-url=http://wpad/wpad.dat",
            "--proxy-auto-detect",
            "--proxy-bypass-list=*",
            "--winhttp-proxy-resolver",
            "--force-webrtc-ip-handling-policy=default",
            "--Proxy-Server=h:1",
        ] {
            let e = validate_flags(bad).unwrap_err();
            assert!(e.contains("is not allowed"), "{bad}: {e}");
            let s = Settings { extra_flags: format!("--force-dark-mode {bad}"), ..Settings::default() };
            assert_eq!(s.validate().unwrap_err()[0].field, "extraFlags");
            assert!(vpn_conflicting_flag(&s.extra_flags).is_some(), "{bad}");
        }
        for name in VPN_CONFLICTING_FLAGS {
            assert!(FORBIDDEN_FLAGS.contains(name), "{name}");
        }
        assert_eq!(
            vpn_conflicting_flag("--force-dark-mode -proxy-auto-detect").as_deref(),
            Some("--proxy-auto-detect")
        );
        assert_eq!(vpn_conflicting_flag("--no-proxy-server=1").as_deref(), Some("--no-proxy-server"));
        assert_eq!(vpn_conflicting_flag("--force-dark-mode --proxy-serverx"), None);
        assert_eq!(vpn_conflicting_flag("proxy-server"), None);
        assert_eq!(vpn_conflicting_flag(""), None);
        // A file from before 0.14.0 with one of them loads without it.
        let l = Settings::from_json_lenient(r#"{"extraFlags":"--no-proxy-server"}"#);
        assert_eq!(l.extra_flags, "");
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
    /// 0.12.0: AI is off by default with no providers; userscripts and macros are on.
    #[test]
    fn ai_defaults_and_validation() {
        let d = Settings::default();
        assert!(!d.ai.enabled && d.ai.providers.is_empty() && d.ai.max_steps == 30);
        assert!(d.userscripts.enabled && !d.userscripts.private_tabs && d.macros.enabled);
        let p = |id: &str, kind: crate::agent::ProviderKind, base: &str| AiProvider {
            id: id.into(),
            kind,
            name: "P".into(),
            base_url: base.into(),
            vision: None,
            max_output_tokens: DEFAULT_MAX_OUTPUT_TOKENS,
        };
        let mut s = Settings::default();
        s.ai.providers.push(p("0a1b2c3d", crate::agent::ProviderKind::OpenRouter, ""));
        s.ai.active_provider = "0a1b2c3d".into();
        assert!(s.validate().is_ok());
        s.ai.providers.push(p("0a1b2c3d", crate::agent::ProviderKind::OpenAi, ""));
        assert!(s.validate().unwrap_err().iter().any(|e| e.field == "ai.providers"));
        s.ai.providers.pop();
        s.ai.providers.push(p("ZZZ", crate::agent::ProviderKind::OpenAi, ""));
        assert!(s.validate().is_err());
        s.ai.providers.pop();
        s.ai.providers.push(p("11111111", crate::agent::ProviderKind::OpenAi, "https://evil.example/v1"));
        assert!(s.validate().is_err(), "named providers keep their own address");
        s.ai.providers.pop();
        s.ai.max_steps = 0;
        assert!(s.validate().unwrap_err().iter().any(|e| e.field == "ai.maxSteps"));
        s.ai.max_steps = 30;
        s.ai.active_provider = "99999999".into();
        assert!(s.validate().is_err());
        // A bad AI section loads leniently: the providers are dropped, nothing else.
        let mut j: serde_json::Value = serde_json::to_value(Settings::default()).unwrap();
        j["ai"]["providers"] = serde_json::json!([{"id":"x","kind":"openai","name":"n"}]);
        j["ai"]["maxSteps"] = serde_json::json!(5000);
        j["homepage"] = serde_json::json!("https://keep.example/");
        let l = Settings::from_json_lenient(&j.to_string());
        assert!(l.ai.providers.is_empty() && l.ai.max_steps == 30 && l.homepage == "https://keep.example/");
    }

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
    fn ntp_wallpaper_must_be_built_in() {
        let mut s = Settings::default();
        s.ntp.background = NtpBackground::Wallpaper;
        s.ntp.wallpaper = "aurora".into();
        assert_eq!(s.validate(), Ok(()));
        s.ntp.wallpaper = "../../secret".into();
        assert!(s.validate().unwrap_err().iter().any(|e| e.field == "ntp.wallpaper"));
        // A stored choice survives a round trip; an unknown id falls back to the default page.
        let ok = Settings::from_json_lenient(r#"{"ntp":{"background":"wallpaper","wallpaper":"dunes"}}"#);
        assert_eq!((ok.ntp.background, ok.ntp.wallpaper.as_str()), (NtpBackground::Wallpaper, "dunes"));
        let bad = Settings::from_json_lenient(r#"{"ntp":{"background":"wallpaper","wallpaper":"gone"}}"#);
        assert_eq!(bad.validate(), Ok(()));
        assert_ne!((bad.ntp.background, bad.ntp.wallpaper.as_str()), (NtpBackground::Wallpaper, "gone"));
    }

    #[test]
    fn list_switches_are_merged_into_one() {
        assert_eq!(
            merge_switch_list(
                "GlicActorUi,AutofillActorMode,LensOverlay,KillOnInvalidNavigationHeaders",
                &DISABLED_FEATURES.join(",")
            ),
            format!(
                "GlicActorUi,AutofillActorMode,LensOverlay,KillOnInvalidNavigationHeaders,{}",
                DISABLED_FEATURES.join(",")
            )
        );
        assert_eq!(merge_switch_list("A,B", "B,,C"), "A,B,C");
        assert_eq!(merge_switch_list("", "A"), "A");
        let s = Settings {
            extra_flags: "--disable-blink-features=Foo --disable-blink-features=Bar --enable-features=X,MediaRouter --disable-features=Y".into(),
            ..Settings::default()
        };
        let sw = s.browser_switches();
        assert_eq!(sw.iter().filter(|a| a.starts_with("--disable-blink-features=")).count(), 1);
        assert!(sw.contains(&"--disable-blink-features=Foo,Bar".to_string()), "{sw:?}");
        assert!(sw.contains(&format!("--disable-features={},Y", DISABLED_FEATURES.join(","))), "{sw:?}");
        // MediaRouter is disabled by Krunk: enabling it is ignored.
        assert!(sw.contains(&"--enable-features=X".to_string()), "{sw:?}");
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

    /// Protected content (Widevine, 0.6.0): off by default; on, the component updater runs so
    /// the engine can fetch the CDM — the same switches as the security-components setting.
    #[test]
    fn protected_content_opt_in() {
        let d = Settings::default();
        assert!(!d.protected_content, "off by default");
        assert!(d.browser_switches().iter().any(|s| s == NO_COMPONENT_UPDATE_SWITCH));
        let on = Settings { protected_content: true, ..Settings::default() };
        let a = on.browser_switches();
        assert!(!a.iter().any(|s| s == NO_COMPONENT_UPDATE_SWITCH), "the updater may run");
        assert!(a
            .iter()
            .any(|s| s == "--host-resolver-rules=MAP krunk.invalid ~NOTFOUND, MAP *.krunk.invalid ~NOTFOUND"));
        assert!(on.restart_required(&d), "needs a restart");
        let both = Settings { protected_content: true, engine_security_components: true, ..Settings::default() };
        assert_eq!(both.browser_switches(), a, "either setting alone opens the same door");
        assert!(!Settings::from_json_lenient(r#"{"theme":"dark"}"#).protected_content);
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
