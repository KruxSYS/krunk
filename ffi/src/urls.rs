//! Address-bar input, navigation rewrites, internal pages, search engines and their suggestion
//! endpoints: pure functions from `core::urlclean`, `core::pages`, `core::settings` and
//! `core::search_suggest`. The HTTP request for engine suggestions is Kotlin's job (no cookies,
//! only when the setting is on and the tab is not private).
use lb_core::pages::Page;
use lb_core::settings::SearchEngine;
use lb_core::{search_suggest, urlclean};
use std::collections::HashSet;

/// Where typed text goes: a URL (explicit scheme, IP, `localhost`, host:port, a real public
/// suffix, local suffixes), a `krunk://` page, or a search with `template`. Same rules as the
/// Windows address bar (`core::urlclean::input_to_url`).
#[uniffi::export]
pub fn input_to_url(input: String, search_template: String) -> String {
    urlclean::input_to_url(&input, &search_template)
}

/// The search URL for `query` from a template with `{searchTerms}` or `%s`.
#[uniffi::export]
pub fn search_url(query: String, search_template: String) -> String {
    urlclean::search_url(&query, &search_template)
}

/// `Some(url)` with tracking parameters (`utm_*`, `fbclid`, `gclid`, ...) removed, `None` when
/// nothing had to change.
#[uniffi::export]
pub fn strip_tracking_params(url: String) -> Option<String> {
    urlclean::strip_tracking_params(&url)
}

/// The `http://` URL as `https://`, or `None` when it already is or is not http.
#[uniffi::export]
pub fn https_upgrade(url: String) -> Option<String> {
    urlclean::https_upgrade(&url)
}

/// The URL's host (lower case), if it has one.
#[uniffi::export]
pub fn host_of(url: String) -> Option<String> {
    urlclean::host_of(&url)
}

/// The URL without user:password.
#[uniffi::export]
pub fn strip_credentials(url: String) -> String {
    urlclean::strip_credentials(&url)
}

/// Result of [`navigation_rewrite`].
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Rewrite {
    /// The URL to load instead.
    pub url: String,
    /// It was upgraded to HTTPS (show the HTTPS-only interstitial if it then fails).
    pub upgraded: bool,
}

/// Whether a top-level navigation must be restarted at another URL: tracking parameters removed
/// (`strip_tracking`) and `http://` upgraded (`https_only`), except for hosts in
/// `http_exceptions`. The result is a fixed point (rewriting it again gives `None`).
#[uniffi::export]
pub fn navigation_rewrite(
    url: String,
    strip_tracking: bool,
    https_only: bool,
    http_exceptions: Vec<String>,
) -> Option<Rewrite> {
    let exceptions: HashSet<String> = http_exceptions.into_iter().collect();
    urlclean::navigation_rewrite(&url, urlclean::RewriteOptions { strip_tracking, https_only }, &exceptions)
        .map(|r| Rewrite { url: r.url, upgraded: r.upgraded })
}

/// The URL as the address bar shows it: internal pages as `krunk://name`, blank pages as "".
#[uniffi::export]
pub fn display_url(url: String) -> String {
    lb_core::pages::display_url(&url)
}

/// The internal page a URL names, as its `krunk://` name (`newtab`, `history`, `bookmarks`,
/// `settings`, `about`, ...). Accepts both the display form `krunk://history` and the internal
/// origin form (`https://krunk.invalid/history`). `None` for web URLs.
#[uniffi::export]
pub fn internal_page(url: String) -> Option<String> {
    Page::from_url(&url).or_else(|| Page::from_display(&url)).map(|p| p.name().to_string())
}

/// The URL `core` stores for an internal page name (`newtab` -> the new-tab URL used in
/// sessions), or `None` for an unknown name.
#[uniffi::export]
pub fn internal_page_url(name: String) -> Option<String> {
    Page::from_name(&name).map(Page::url)
}

/// The new-tab page's stored URL (what `TabModel::open("")` records).
#[uniffi::export]
pub fn new_tab_url() -> String {
    Page::NewTab.url()
}

/// Whether a visit to `url` belongs in history at all (http/https, not too long).
#[uniffi::export]
pub fn should_record(url: String) -> bool {
    lb_core::history::should_record(&url)
}

/// The URL as history stores it (credentials and tracking parameters removed), or `None` for
/// private tabs and for URLs that are never recorded.
#[uniffi::export]
pub fn visit_url(url: String, private: bool) -> Option<String> {
    lb_core::history::visit_url(&url, private)
}

/// Whether `url` can be bookmarked (web and file URLs).
#[uniffi::export]
pub fn bookmarkable(url: String) -> bool {
    lb_core::bookmarks::bookmarkable(&url)
}

/// Search engines, in the order Settings lists them (`core::settings::SearchEngine`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SearchEngineId {
    DuckDuckGo,
    Google,
    Bing,
    Brave,
    Startpage,
    Ecosia,
    Custom,
}

impl From<SearchEngine> for SearchEngineId {
    fn from(e: SearchEngine) -> Self {
        match e {
            SearchEngine::DuckDuckGo => SearchEngineId::DuckDuckGo,
            SearchEngine::Google => SearchEngineId::Google,
            SearchEngine::Bing => SearchEngineId::Bing,
            SearchEngine::Brave => SearchEngineId::Brave,
            SearchEngine::Startpage => SearchEngineId::Startpage,
            SearchEngine::Ecosia => SearchEngineId::Ecosia,
            SearchEngine::Custom => SearchEngineId::Custom,
        }
    }
}

impl From<SearchEngineId> for SearchEngine {
    fn from(e: SearchEngineId) -> Self {
        match e {
            SearchEngineId::DuckDuckGo => SearchEngine::DuckDuckGo,
            SearchEngineId::Google => SearchEngine::Google,
            SearchEngineId::Bing => SearchEngine::Bing,
            SearchEngineId::Brave => SearchEngine::Brave,
            SearchEngineId::Startpage => SearchEngine::Startpage,
            SearchEngineId::Ecosia => SearchEngine::Ecosia,
            SearchEngineId::Custom => SearchEngine::Custom,
        }
    }
}

/// One search engine for the settings list.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct EngineInfo {
    pub id: SearchEngineId,
    /// Display name; "Custom" for the custom engine.
    pub name: String,
    /// The search URL template, `None` for `Custom` (uses the custom URL setting).
    pub template: Option<String>,
    /// Whether the engine has a suggestions endpoint.
    pub suggestions: bool,
}

const ALL_ENGINES: [SearchEngine; 7] = [
    SearchEngine::DuckDuckGo,
    SearchEngine::Google,
    SearchEngine::Bing,
    SearchEngine::Brave,
    SearchEngine::Startpage,
    SearchEngine::Ecosia,
    SearchEngine::Custom,
];

/// The selectable search engines; DuckDuckGo first (the default).
#[uniffi::export]
pub fn search_engines() -> Vec<EngineInfo> {
    ALL_ENGINES
        .iter()
        .map(|&e| EngineInfo {
            id: e.into(),
            name: e.name().unwrap_or("Custom").to_string(),
            template: e.template().map(str::to_string),
            suggestions: search_suggest::endpoint(e).is_some(),
        })
        .collect()
}

/// The engine's suggestions URL for `query`, or `None` (custom engine).
#[uniffi::export]
pub fn suggest_url(engine: SearchEngineId, query: String) -> Option<String> {
    search_suggest::suggest_url(engine.into(), &query)
}

/// Whether engine suggestions make sense for the typed text (not for addresses, schemes, blank
/// or very long input).
#[uniffi::export]
pub fn suggest_wanted(text: String, search_template: String) -> bool {
    search_suggest::wanted(&text, &search_template)
}

/// Parses a suggestions response (OpenSearch JSON or DuckDuckGo's list), at most 8 entries.
#[uniffi::export]
pub fn parse_suggest_body(body: Vec<u8>) -> Vec<String> {
    search_suggest::parse(&body)
}

/// The desktop user-agent presets (Settings > Advanced > User agent on Windows).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct UaPreset {
    pub id: String,
    pub label: String,
    pub user_agent: String,
}

#[uniffi::export]
pub fn ua_presets() -> Vec<UaPreset> {
    lb_core::useragents::PRESETS
        .iter()
        .map(|p| UaPreset { id: p.id.to_string(), label: p.label.to_string(), user_agent: p.user_agent.to_string() })
        .collect()
}

/// A built-in new-tab wallpaper (`ui/shared/wallpapers/{id}.webp`, copied into the APK's assets).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Wallpaper {
    pub id: String,
    pub title: String,
    /// "dark" or "light": what the page draws over it.
    pub look: String,
    /// Starting darkening, percent.
    pub dim: u8,
}

#[uniffi::export]
pub fn wallpapers() -> Vec<Wallpaper> {
    lb_core::assets::wallpapers()
        .into_iter()
        .map(|w| Wallpaper { id: w.id, title: w.title, look: w.look, dim: w.dim })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DDG: &str = "https://duckduckgo.com/?q={searchTerms}";

    #[test]
    fn typed_input_is_url_or_search() {
        assert_eq!(input_to_url("example.com".into(), DDG.into()), "https://example.com/");
        assert_eq!(input_to_url("krunk browser".into(), DDG.into()), "https://duckduckgo.com/?q=krunk+browser");
        assert_eq!(input_to_url("localhost:8080".into(), DDG.into()), "http://localhost:8080/");
        assert!(input_to_url("test.test".into(), DDG.into()).starts_with("https://duckduckgo.com/?q="));
        assert_eq!(internal_page(input_to_url("krunk://history".into(), DDG.into())), Some("history".into()));
    }

    #[test]
    fn rewrites() {
        let r = navigation_rewrite("http://example.com/?utm_source=x".into(), true, true, vec![]).unwrap();
        assert_eq!(r.url, "https://example.com/");
        assert!(r.upgraded);
        assert_eq!(navigation_rewrite("https://example.com/".into(), true, true, vec![]), None);
        assert_eq!(navigation_rewrite("http://example.com/".into(), true, true, vec!["example.com".into()]), None);
        assert_eq!(strip_tracking_params("https://a.com/?fbclid=1&x=2".into()), Some("https://a.com/?x=2".into()));
        assert_eq!(https_upgrade("http://a.com/".into()), Some("https://a.com/".into()));
        assert_eq!(host_of("https://www.Example.com/x".into()), Some("www.example.com".into()));
    }

    #[test]
    fn internal_pages_round_trip() {
        let ntp = new_tab_url();
        assert_eq!(internal_page(ntp.clone()), Some("newtab".into()));
        assert_eq!(internal_page("krunk://settings".into()), Some("settings".into()));
        assert_eq!(internal_page("https://example.com/".into()), None);
        assert_eq!(internal_page_url("history".into()).and_then(internal_page), Some("history".into()));
        assert_eq!(display_url(ntp), "");
        assert_eq!(display_url("https://krunk.invalid/history.html".into()), "krunk://history");
    }

    #[test]
    fn history_rules() {
        assert_eq!(visit_url("https://a.com/?utm_source=x".into(), false), Some("https://a.com/".into()));
        assert_eq!(visit_url("https://a.com/".into(), true), None);
        assert!(!should_record("file:///tmp/x".into()));
        assert!(bookmarkable("https://a.com/".into()));
    }

    #[test]
    fn engines() {
        let engines = search_engines();
        assert_eq!(engines[0].id, SearchEngineId::DuckDuckGo);
        assert_eq!(engines[0].template.as_deref(), Some(DDG));
        assert!(engines[0].suggestions);
        assert_eq!(engines.last().unwrap().id, SearchEngineId::Custom);
        assert!(!engines.last().unwrap().suggestions);
        assert!(suggest_url(SearchEngineId::DuckDuckGo, "kru".into()).unwrap().contains("kru"));
        assert_eq!(suggest_url(SearchEngineId::Custom, "kru".into()), None);
        assert!(suggest_wanted("krunk bro".into(), DDG.into()));
        assert!(!suggest_wanted("example.com".into(), DDG.into()));
        assert_eq!(
            parse_suggest_body(br#"["kru",["krunk","krunk browser"]]"#.to_vec()),
            vec!["krunk", "krunk browser"]
        );
    }

    #[test]
    fn presets_and_wallpapers() {
        assert_eq!(ua_presets().len(), 10);
        assert!(ua_presets().iter().any(|p| p.id == "chrome-android"));
        let w = wallpapers();
        assert_eq!(w.len(), 10);
        assert!(w.iter().all(|w| w.look == "dark" || w.look == "light"));
    }
}
