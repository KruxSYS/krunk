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

/// What "search for the selected text" offers (the page's selection menu).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SelectionSearch {
    /// The selection as a query: whitespace collapsed, at most 1,000 characters.
    pub query: String,
    /// The menu text, e.g. `Search DuckDuckGo for “krunk browser”` (the query shortened).
    pub label: String,
    /// The search address to open.
    pub url: String,
}

/// The search the selection menu offers for `selection` with the engine in effect, or `None`
/// for an empty or blank selection. `engine_name` and `search_template` come from the settings
/// (`settings_search_name`, `settings_search_template`).
#[uniffi::export]
pub fn selection_search(selection: String, engine_name: String, search_template: String) -> Option<SelectionSearch> {
    let query = urlclean::selection_query(&selection)?;
    Some(SelectionSearch {
        label: urlclean::selection_label(&engine_name, &query),
        url: urlclean::search_url(&query, &search_template),
        query,
    })
}

/// A selected address the selection menu can open.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SelectionAddress {
    /// The address as the address bar would read it (http or https only).
    pub url: String,
    /// The address as the menu shows it: no scheme, at most 32 characters.
    pub label: String,
}

/// The address the selected text names (one word, quotes and a sentence's final punctuation
/// dropped), or `None` when it is not an address. Never `file:` or another scheme from a page.
#[uniffi::export]
pub fn selection_address(selection: String) -> Option<SelectionAddress> {
    let url = urlclean::selection_url(&selection)?;
    Some(SelectionAddress { label: urlclean::selection_url_label(&url), url })
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

/// The user agent a tab sends for the user-agent setting `setting` ("" = Default: the latest
/// Chrome on Android, or on Windows when the tab asks for the desktop site).
#[uniffi::export]
pub fn effective_user_agent(setting: String, desktop: bool) -> String {
    lb_core::useragents::effective(&setting, desktop).to_string()
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

/// One built-in wallpaper by id (`None` for an unknown id, as settings validation sees it).
#[uniffi::export]
pub fn wallpaper(id: String) -> Option<Wallpaper> {
    lb_core::assets::wallpaper(&id).map(|w| Wallpaper { id: w.id, title: w.title, look: w.look, dim: w.dim })
}

/// Whether `text` is a colour the settings accept: `#rrggbb` or empty (the same rule the core's
/// validation applies to `ntp.backgroundColor`).
#[uniffi::export]
pub fn is_hex_color(text: String) -> bool {
    lb_core::settings::is_hex_color(&text)
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
        assert!(effective_user_agent(String::new(), false).contains("Mobile Safari"));
        assert!(effective_user_agent(String::new(), true).contains("Windows NT 10.0"));
        assert_eq!(effective_user_agent("X/1".into(), true), "X/1");
        let w = wallpapers();
        assert_eq!(w.len(), 10);
        assert!(w.iter().all(|w| w.look == "dark" || w.look == "light"));
        assert_eq!(wallpaper("aurora".into()).map(|w| w.look), Some("dark".into()));
        assert_eq!(wallpaper("nope".into()), None);
        assert!(is_hex_color("#2563eb".into()));
        assert!(is_hex_color(String::new()));
        assert!(!is_hex_color("2563eb".into()));
        assert!(!is_hex_color("#25".into()));
    }
    #[test]
    fn the_selection_menu_offers_a_search_and_an_address_by_the_cores_rules() {
        const DDG: &str = "https://duckduckgo.com/?q={searchTerms}";
        let s = selection_search("  krunk \n browser ".into(), "DuckDuckGo".into(), DDG.into()).unwrap();
        assert_eq!(s.query, "krunk browser");
        assert_eq!(s.label, "Search DuckDuckGo for “krunk browser”");
        assert_eq!(s.url, "https://duckduckgo.com/?q=krunk+browser");
        assert_eq!(selection_search(" \n ".into(), "DuckDuckGo".into(), DDG.into()), None);
        // A long selection is searched in full (up to the core's limit) but shown shortened.
        let long = selection_search("word ".repeat(40), "Brave".into(), DDG.into()).unwrap();
        assert!(long.label.ends_with("…”"), "{}", long.label);
        assert!(long.query.len() > 100);

        let a = selection_address("(example.com/docs).".into()).unwrap();
        assert_eq!(a.url, "https://example.com/docs");
        assert_eq!(a.label, "example.com/docs");
        assert_eq!(selection_address("http://10.0.0.1:8080/x".into()).unwrap().url, "http://10.0.0.1:8080/x");
        for not_an_address in ["two words", "", "file:///etc/passwd", "javascript:alert(1)", "hello"] {
            assert_eq!(selection_address(not_an_address.into()), None, "{not_an_address}");
        }
    }
}
