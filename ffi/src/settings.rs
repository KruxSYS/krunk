//! Settings as the Android app uses them: the shared `core::settings::Settings` fields that make
//! sense on a phone, plus a few Android-only ones. Shared fields are stored in the same JSON as
//! Windows (`kv["settings"]`, camelCase names, same validation), so sync can carry them later;
//! Android-only fields live in `kv["android.settings"]` and Windows never sees them.
use lb_core::settings::{self as cs, Settings};
use serde::{Deserialize, Serialize};

pub(crate) const EXTRAS_KEY: &str = "android.settings";

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum StartupMode {
    NewTab,
    RestoreSession,
    Homepage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum NewTabMode {
    /// Krunk's new-tab page.
    Krunk,
    Homepage,
    Blank,
    /// `new_tab_url`.
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ThemeMode {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum NtpBackgroundMode {
    None,
    Color,
    /// A picture the user picked (not offered on Android yet; kept so a synced value survives).
    Image,
    Wallpaper,
}

/// Android-only: where the toolbar sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "camelCase")]
pub enum ToolbarPosition {
    #[default]
    Bottom,
    Top,
}

/// Android-only fields (`kv["android.settings"]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Extras {
    toolbar_position: ToolbarPosition,
    /// Web text scale when `follow_system_font` is off (0.5-2.0).
    text_scale: f64,
    /// Scale web text with the system font size.
    follow_system_font: bool,
    /// Allow pinch zoom on pages that forbid it.
    force_zoom: bool,
}

impl Default for Extras {
    fn default() -> Self {
        Extras {
            toolbar_position: ToolbarPosition::Bottom,
            text_scale: 1.0,
            follow_system_font: true,
            force_zoom: false,
        }
    }
}

/// The settings record Kotlin edits. Field names mirror `core::settings::Settings`.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct AndroidSettings {
    // Search and startup (shared)
    pub search_engine: crate::SearchEngineId,
    /// Custom engine URL with `%s`; must be https.
    pub custom_search_url: String,
    pub homepage: String,
    pub startup: StartupMode,
    pub new_tab_page: NewTabMode,
    pub new_tab_url: String,
    pub search_suggestions: bool,
    // Privacy (shared)
    pub https_only: bool,
    pub strip_tracking_params: bool,
    // Appearance (shared)
    pub theme: ThemeMode,
    /// Default page zoom (0.25-5.0; GeckoView applies it as a text scale).
    pub default_zoom: f64,
    // New-tab page (shared)
    pub ntp_background: NtpBackgroundMode,
    pub ntp_background_color: String,
    pub ntp_wallpaper: String,
    pub ntp_image_dim: u8,
    pub ntp_columns: u8,
    pub ntp_rows: u8,
    pub ntp_show_search: bool,
    pub ntp_show_tiles: bool,
    pub ntp_show_shortcuts: bool,
    /// Pinned tiles only (`Custom`) or also the most visited (`false`).
    pub ntp_custom_tiles_only: bool,
    // Tabs and memory (shared)
    pub tab_sleep_minutes: u32,
    pub tab_discard_enabled: bool,
    pub tab_discard_minutes: u32,
    // Advanced (shared)
    /// Empty = the engine's default user agent; else a preset's string (`ua_presets`) or custom.
    pub user_agent: String,
    // Android only
    pub toolbar_position: ToolbarPosition,
    pub text_scale: f64,
    pub follow_system_font: bool,
    pub force_zoom: bool,
}

impl AndroidSettings {
    pub(crate) fn from_core(s: &Settings, extras_json: Option<&str>) -> AndroidSettings {
        let extras: Extras = extras_json.and_then(|j| serde_json::from_str(j).ok()).unwrap_or_default();
        AndroidSettings {
            search_engine: s.search_engine.into(),
            custom_search_url: s.custom_search_url.clone(),
            homepage: s.homepage.clone(),
            startup: match s.startup {
                cs::Startup::NewTab => StartupMode::NewTab,
                cs::Startup::RestoreSession => StartupMode::RestoreSession,
                cs::Startup::Homepage => StartupMode::Homepage,
            },
            new_tab_page: match s.new_tab_page {
                cs::NewTabPage::Krunk => NewTabMode::Krunk,
                cs::NewTabPage::Homepage => NewTabMode::Homepage,
                cs::NewTabPage::Blank => NewTabMode::Blank,
                cs::NewTabPage::Custom => NewTabMode::Custom,
            },
            new_tab_url: s.new_tab_url.clone(),
            search_suggestions: s.search_suggestions,
            https_only: s.https_only,
            strip_tracking_params: s.strip_tracking_params,
            theme: match s.theme {
                cs::Theme::System => ThemeMode::System,
                cs::Theme::Light => ThemeMode::Light,
                cs::Theme::Dark => ThemeMode::Dark,
            },
            default_zoom: s.default_zoom,
            ntp_background: match s.ntp.background {
                cs::NtpBackground::None => NtpBackgroundMode::None,
                cs::NtpBackground::Color => NtpBackgroundMode::Color,
                cs::NtpBackground::Image => NtpBackgroundMode::Image,
                cs::NtpBackground::Wallpaper => NtpBackgroundMode::Wallpaper,
            },
            ntp_background_color: s.ntp.background_color.clone(),
            ntp_wallpaper: s.ntp.wallpaper.clone(),
            ntp_image_dim: s.ntp.image_dim,
            ntp_columns: s.ntp.columns,
            ntp_rows: s.ntp.rows,
            ntp_show_search: s.ntp.show_search,
            ntp_show_tiles: s.ntp.show_tiles,
            ntp_show_shortcuts: s.ntp.show_shortcuts,
            ntp_custom_tiles_only: s.ntp.tiles == cs::TileSource::Custom,
            tab_sleep_minutes: s.tab_sleep_minutes,
            tab_discard_enabled: s.tab_discard_enabled,
            tab_discard_minutes: s.tab_discard_minutes,
            user_agent: s.user_agent.clone(),
            toolbar_position: extras.toolbar_position,
            text_scale: extras.text_scale,
            follow_system_font: extras.follow_system_font,
            force_zoom: extras.force_zoom,
        }
    }

    /// Writes the shared fields into a full `Settings` (desktop-only fields untouched).
    pub(crate) fn apply_to_core(&self, s: &mut Settings) {
        s.search_engine = self.search_engine.into();
        s.custom_search_url = self.custom_search_url.trim().to_string();
        s.homepage = self.homepage.trim().to_string();
        s.startup = match self.startup {
            StartupMode::NewTab => cs::Startup::NewTab,
            StartupMode::RestoreSession => cs::Startup::RestoreSession,
            StartupMode::Homepage => cs::Startup::Homepage,
        };
        s.new_tab_page = match self.new_tab_page {
            NewTabMode::Krunk => cs::NewTabPage::Krunk,
            NewTabMode::Homepage => cs::NewTabPage::Homepage,
            NewTabMode::Blank => cs::NewTabPage::Blank,
            NewTabMode::Custom => cs::NewTabPage::Custom,
        };
        s.new_tab_url = self.new_tab_url.trim().to_string();
        s.search_suggestions = self.search_suggestions;
        s.https_only = self.https_only;
        s.strip_tracking_params = self.strip_tracking_params;
        s.theme = match self.theme {
            ThemeMode::System => cs::Theme::System,
            ThemeMode::Light => cs::Theme::Light,
            ThemeMode::Dark => cs::Theme::Dark,
        };
        s.default_zoom = self.default_zoom;
        s.ntp.background = match self.ntp_background {
            NtpBackgroundMode::None => cs::NtpBackground::None,
            NtpBackgroundMode::Color => cs::NtpBackground::Color,
            NtpBackgroundMode::Image => cs::NtpBackground::Image,
            NtpBackgroundMode::Wallpaper => cs::NtpBackground::Wallpaper,
        };
        s.ntp.background_color = self.ntp_background_color.clone();
        s.ntp.wallpaper = self.ntp_wallpaper.clone();
        s.ntp.image_dim = self.ntp_image_dim;
        s.ntp.columns = self.ntp_columns;
        s.ntp.rows = self.ntp_rows;
        s.ntp.show_search = self.ntp_show_search;
        s.ntp.show_tiles = self.ntp_show_tiles;
        s.ntp.show_shortcuts = self.ntp_show_shortcuts;
        s.ntp.tiles = if self.ntp_custom_tiles_only { cs::TileSource::Custom } else { cs::TileSource::MostVisited };
        s.tab_sleep_minutes = self.tab_sleep_minutes;
        s.tab_discard_enabled = self.tab_discard_enabled;
        s.tab_discard_minutes = self.tab_discard_minutes;
        s.user_agent = self.user_agent.trim().to_string();
    }

    pub(crate) fn extras_json(&self) -> String {
        let e = Extras {
            toolbar_position: self.toolbar_position,
            text_scale: self.text_scale.clamp(0.5, 2.0),
            follow_system_font: self.follow_system_font,
            force_zoom: self.force_zoom,
        };
        serde_json::to_string(&e).unwrap_or_else(|_| "{}".to_string())
    }
}

/// The defaults (DuckDuckGo, restore session, system theme, bottom toolbar, ...).
#[uniffi::export]
pub fn default_settings() -> AndroidSettings {
    AndroidSettings::from_core(&Settings::default(), None)
}

/// The search URL template in effect (the engine's, or the custom URL).
#[uniffi::export]
pub fn settings_search_template(settings: AndroidSettings) -> String {
    let mut s = Settings::default();
    settings.apply_to_core(&mut s);
    s.search_template().to_string()
}

/// The engine name in effect ("DuckDuckGo", or the custom template's host).
#[uniffi::export]
pub fn settings_search_name(settings: AndroidSettings) -> String {
    let mut s = Settings::default();
    settings.apply_to_core(&mut s);
    s.search_name()
}

/// What a new tab opens: `None` for Krunk's new-tab page, else a URL.
#[uniffi::export]
pub fn settings_new_tab_target(settings: AndroidSettings) -> Option<String> {
    let mut s = Settings::default();
    settings.apply_to_core(&mut s);
    s.new_tab_target()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{KrunkStore, SearchEngineId, SettingsError};

    #[test]
    fn defaults_match_windows() {
        let d = default_settings();
        assert_eq!(d.search_engine, SearchEngineId::DuckDuckGo);
        assert_eq!(d.startup, StartupMode::RestoreSession);
        assert_eq!(d.theme, ThemeMode::System);
        assert!(d.search_suggestions);
        assert!(!d.https_only);
        assert!(d.strip_tracking_params);
        assert_eq!(d.tab_sleep_minutes, 5);
        assert_eq!(d.toolbar_position, ToolbarPosition::Bottom);
        assert_eq!(settings_search_template(d.clone()), "https://duckduckgo.com/?q={searchTerms}");
        assert_eq!(settings_search_name(d.clone()), "DuckDuckGo");
        assert_eq!(settings_new_tab_target(d), None);
    }

    #[test]
    fn save_and_load_keep_desktop_fields_and_extras() {
        let store = KrunkStore::in_memory().unwrap();
        // A desktop-only field already stored must survive an Android save.
        let core = Settings { hardware_acceleration: false, tab_hover_preview: false, ..Settings::default() };
        store.kv_set("settings".into(), serde_json::to_string(&core).unwrap()).unwrap();

        let mut s = store.load_settings().unwrap();
        s.search_engine = SearchEngineId::Google;
        s.theme = ThemeMode::Dark;
        s.toolbar_position = ToolbarPosition::Top;
        s.text_scale = 1.25;
        s.follow_system_font = false;
        store.save_settings(s.clone()).unwrap();

        let back = store.load_settings().unwrap();
        assert_eq!(back, s);
        let raw: Settings = serde_json::from_str(&store.kv_get("settings".into()).unwrap().unwrap()).unwrap();
        assert!(!raw.hardware_acceleration);
        assert!(!raw.tab_hover_preview);
        assert_eq!(raw.search_engine, cs::SearchEngine::Google);
        let extras = store.kv_get(EXTRAS_KEY.into()).unwrap().unwrap();
        assert!(extras.contains("\"toolbarPosition\":\"top\""));
        assert_eq!(settings_search_name(back), "Google");
    }

    #[test]
    fn validation_reports_fields() {
        let store = KrunkStore::in_memory().unwrap();
        let mut s = default_settings();
        s.search_engine = SearchEngineId::Custom;
        s.custom_search_url = "http://insecure.example/?q=%s".into();
        s.default_zoom = 99.0;
        match store.save_settings(s).unwrap_err() {
            SettingsError::Validation { errors } => {
                let fields: Vec<&str> = errors.iter().map(|e| e.field.as_str()).collect();
                assert!(fields.iter().any(|f| f.contains("customSearchUrl") || f.contains("custom")), "{fields:?}");
                assert!(fields.iter().any(|f| f.contains("zoom") || f.contains("Zoom")), "{fields:?}");
            }
            other => panic!("expected validation error, got {other:?}"),
        }
        // Nothing was written.
        assert_eq!(store.load_settings().unwrap(), default_settings());
    }

    #[test]
    fn custom_engine_template_and_new_tab_target() {
        let mut s = default_settings();
        s.search_engine = SearchEngineId::Custom;
        s.custom_search_url = "https://search.example/?q=%s".into();
        assert_eq!(settings_search_template(s.clone()), "https://search.example/?q=%s");
        assert_eq!(settings_search_name(s.clone()), "search.example");
        s.new_tab_page = NewTabMode::Homepage;
        s.homepage = "https://krunk.209.wf/".into();
        assert_eq!(settings_new_tab_target(s), Some("https://krunk.209.wf/".into()));
    }
}
