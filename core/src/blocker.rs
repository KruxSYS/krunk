//! Ad and tracker blocking on top of Brave's `adblock` engine.
//!
//! Network requests are checked with [`Blocker::check`]; cosmetic filtering
//! gives the host a stylesheet per page ([`Blocker::cosmetic`]) plus generic
//! selectors for the classes and ids a page actually uses
//! ([`Blocker::generic_selectors`]).
//!
//! Compiling the lists takes a moment, so the compiled engine is serialized
//! to disk ([`Blocker::serialize`]) and reloaded on the next start. The cache
//! header records a key derived from the engine version and the exact list
//! sources (see `filterlists::cache_key`), so a stale cache is rebuilt.

use crate::settings::FILTER_LISTS;
use ::adblock::lists::{FilterSet, ParseOptions};
use ::adblock::request::Request;
use ::adblock::Engine;
use std::collections::HashSet;

const CACHE_MAGIC: &[u8; 4] = b"LBAB";
const CACHE_VERSION: u32 = 2;
/// Version of the `adblock` crate the cache was made with (bumped by hand
/// when the dependency is upgraded).
const ENGINE_VERSION: &str = "adblock-0.13";

/// Bundled lists, deflated by `build.rs`.
const BUNDLED: &[(&str, &[u8])] = &[
    ("easylist", include_bytes!(concat!(env!("OUT_DIR"), "/easylist.txt.deflate"))),
    ("easyprivacy", include_bytes!(concat!(env!("OUT_DIR"), "/easyprivacy.txt.deflate"))),
    ("ublock-filters", include_bytes!(concat!(env!("OUT_DIR"), "/ublock-filters.txt.deflate"))),
    ("ublock-privacy", include_bytes!(concat!(env!("OUT_DIR"), "/ublock-privacy.txt.deflate"))),
    ("ublock-unbreak", include_bytes!(concat!(env!("OUT_DIR"), "/ublock-unbreak.txt.deflate"))),
];

/// Where each list can be refreshed from (used by the daily updater).
pub fn list_url(id: &str) -> Option<&'static str> {
    Some(match id {
        "easylist" => "https://easylist.to/easylist/easylist.txt",
        "easyprivacy" => "https://easylist.to/easylist/easyprivacy.txt",
        "ublock-filters" => "https://ublockorigin.github.io/uAssets/filters/filters.txt",
        "ublock-privacy" => "https://ublockorigin.github.io/uAssets/filters/privacy.txt",
        "ublock-unbreak" => "https://ublockorigin.github.io/uAssets/filters/unbreak.txt",
        _ => return None,
    })
}

/// Human-readable list titles for the settings page.
pub fn list_title(id: &str) -> &'static str {
    match id {
        "easylist" => "EasyList (ads)",
        "easyprivacy" => "EasyPrivacy (trackers)",
        "ublock-filters" => "uBlock filters",
        "ublock-privacy" => "uBlock filters – Privacy",
        "ublock-unbreak" => "uBlock filters – Unbreak",
        _ => "Unknown list",
    }
}

/// The text of a bundled list.
pub fn bundled_list(id: &str) -> Option<String> {
    let (_, packed) = BUNDLED.iter().find(|(n, _)| *n == id)?;
    let raw = miniz_oxide::inflate::decompress_to_vec(packed).ok()?;
    String::from_utf8(raw).ok()
}

/// Total size of the embedded (deflated) lists in bytes.
pub fn bundled_size() -> usize {
    BUNDLED.iter().map(|(_, b)| b.len()).sum()
}

/// A downloaded list is accepted only if it looks like a filter list.
pub fn looks_like_list(text: &str) -> bool {
    let lines = text.lines().take(20_000).count();
    lines >= 50 && !text.trim_start().starts_with('<') && text.len() < 32 * 1024 * 1024
}

/// Request type, mapped from the engine's resource type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    Document,
    SubFrame,
    Stylesheet,
    Image,
    Media,
    Font,
    Script,
    Xhr,
    WebSocket,
    Ping,
    Other,
}

impl ResourceKind {
    fn as_str(self) -> &'static str {
        match self {
            ResourceKind::Document => "document",
            ResourceKind::SubFrame => "sub_frame",
            ResourceKind::Stylesheet => "stylesheet",
            ResourceKind::Image => "image",
            ResourceKind::Media => "media",
            ResourceKind::Font => "font",
            ResourceKind::Script => "script",
            ResourceKind::Xhr => "xmlhttprequest",
            ResourceKind::WebSocket => "websocket",
            ResourceKind::Ping => "ping",
            ResourceKind::Other => "other",
        }
    }

    /// Maps CEF's `cef_resource_type_t`. The tab's own top-level navigation
    /// is never checked (`None`); `ws://`/`wss://` URLs are WebSockets.
    pub fn from_cef_resource_type(resource_type: i32, url: &str) -> Option<ResourceKind> {
        if url.starts_with("wss://") || url.starts_with("ws://") {
            return Some(ResourceKind::WebSocket);
        }
        Some(match resource_type {
            // RT_MAIN_FRAME, RT_NAVIGATION_PRELOAD_MAIN_FRAME
            0 | 19 => return None,
            // RT_SUB_FRAME, RT_NAVIGATION_PRELOAD_SUB_FRAME
            1 | 20 => ResourceKind::SubFrame,
            2 => ResourceKind::Stylesheet,
            // RT_SCRIPT and the worker scripts
            3 | 9 | 10 | 15 => ResourceKind::Script,
            // RT_IMAGE, RT_FAVICON
            4 | 12 => ResourceKind::Image,
            5 => ResourceKind::Font,
            8 => ResourceKind::Media,
            13 => ResourceKind::Xhr,
            14 => ResourceKind::Ping,
            _ => ResourceKind::Other,
        })
    }
}

/// Our own origins are served by the host and never go through the engine.
/// (Prefix check: this runs for every request.)
pub fn is_internal(uri: &str) -> bool {
    uri.starts_with("https://krunk.invalid/") || uri.starts_with("https://shell.krunk.invalid/")
}

/// Only network schemes are filtered.
pub fn is_filterable(uri: &str) -> bool {
    (uri.starts_with("https://") || uri.starts_with("http://") || uri.starts_with("wss://") || uri.starts_with("ws://"))
        && !is_internal(uri)
}

/// Sites where ad blocking is turned off. Kept in memory as a set; an entry
/// covers the host itself and all its subdomains.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Allowlist {
    hosts: HashSet<String>,
}

impl Allowlist {
    pub fn new<I: IntoIterator<Item = String>>(hosts: I) -> Allowlist {
        Allowlist { hosts: hosts.into_iter().filter_map(|h| normalize_host(&h)).collect() }
    }

    pub fn insert(&mut self, host: &str) -> bool {
        normalize_host(host).is_some_and(|h| self.hosts.insert(h))
    }

    pub fn remove(&mut self, host: &str) -> bool {
        normalize_host(host).is_some_and(|h| self.hosts.remove(&h))
    }

    pub fn len(&self) -> usize {
        self.hosts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hosts.is_empty()
    }

    /// The allowlist entry that covers `host`, if any (walks up the labels:
    /// `a.b.example.com`, `b.example.com`, `example.com`, `com`).
    pub fn matching(&self, host: &str) -> Option<&str> {
        if self.hosts.is_empty() {
            return None;
        }
        let host = host.trim_end_matches('.');
        let mut rest = host;
        loop {
            if let Some(h) = self.hosts.get(&rest.to_ascii_lowercase()) {
                return Some(h);
            }
            match rest.split_once('.') {
                Some((_, tail)) if !tail.is_empty() => rest = tail,
                _ => return None,
            }
        }
    }

    pub fn contains(&self, host: &str) -> bool {
        self.matching(host).is_some()
    }
}

/// Lower-cases and validates an allowlist host (`*.` prefixes are dropped).
pub fn normalize_host(host: &str) -> Option<String> {
    let h = host.trim().trim_start_matches("*.").trim_end_matches('.').to_ascii_lowercase();
    crate::settings::is_hostname(&h).then_some(h)
}

/// What to do with a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Block,
}

/// Cosmetic filtering data for one page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cosmetic {
    /// A stylesheet hiding the site-specific selectors (may be empty).
    pub css: String,
    /// When true, the page must not ask for generic class/id selectors.
    pub generichide: bool,
    /// Pass back to [`Blocker::generic_selectors`].
    pub exceptions: HashSet<String>,
}

pub struct Blocker {
    engine: Engine,
    key: u64,
}

impl std::fmt::Debug for Blocker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Blocker").field("key", &self.key).finish()
    }
}

/// FNV-1a 64-bit hash (stable across builds; used for cache keys).
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Hash of an embedded list's compressed bytes (cheap: no inflating).
pub fn embedded_hash(id: &str) -> Option<u64> {
    BUNDLED.iter().find(|(n, _)| *n == id).map(|(_, b)| fnv64(b))
}

/// Version string of the engine format, part of every cache key.
pub fn engine_version() -> &'static str {
    ENGINE_VERSION
}

impl Blocker {
    /// Compiles an engine from list texts; `key` identifies the sources.
    pub fn from_lists(lists: &[&str], key: u64) -> Blocker {
        let mut set = FilterSet::new(false);
        for text in lists {
            set.add_filter_list(text.to_string(), ParseOptions::default());
        }
        Blocker { engine: Engine::new_with_filter_set(set), key }
    }

    /// Compiles the enabled bundled lists (used when nothing was downloaded).
    pub fn bundled(enabled: &[String]) -> Blocker {
        let texts: Vec<String> = FILTER_LISTS
            .iter()
            .filter(|id| enabled.iter().any(|e| e == *id))
            .filter_map(|id| bundled_list(id))
            .collect();
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let key = crate::filterlists::cache_key(enabled, &crate::filterlists::UpdateState::default());
        Blocker::from_lists(&refs, key)
    }

    pub fn key(&self) -> u64 {
        self.key
    }

    /// Serializes the compiled engine with our cache header.
    pub fn serialize(&self) -> Vec<u8> {
        let body = self.engine.serialize();
        let mut out = Vec::with_capacity(body.len() + 16);
        out.extend_from_slice(CACHE_MAGIC);
        out.extend_from_slice(&CACHE_VERSION.to_le_bytes());
        out.extend_from_slice(&self.key.to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    /// The key stored in a cache file, without loading it.
    pub fn cached_key(data: &[u8]) -> Option<u64> {
        if data.len() < 16 || &data[..4] != CACHE_MAGIC || data[4..8] != CACHE_VERSION.to_le_bytes() {
            return None;
        }
        Some(u64::from_le_bytes(data[8..16].try_into().ok()?))
    }

    /// Loads a cached engine; fails unless it was built for `expect`.
    pub fn deserialize(data: &[u8], expect: u64) -> Result<Blocker, String> {
        match Blocker::cached_key(data) {
            None => return Err("not an engine cache (or an old format)".into()),
            Some(k) if k != expect => return Err("cache built from different lists".into()),
            Some(_) => {}
        }
        let mut engine = Engine::default();
        engine.deserialize(&data[16..]).map_err(|e| format!("{e:?}"))?;
        Ok(Blocker { engine, key: expect })
    }

    /// Checks a subresource or navigation request made by `source_url`.
    pub fn check(&self, url: &str, source_url: &str, kind: ResourceKind) -> Decision {
        // Never block our own pages or non-network schemes.
        if !(url.starts_with("http://") || url.starts_with("https://") || url.starts_with("ws")) {
            return Decision::Allow;
        }
        let Ok(req) = Request::new(url, source_url, kind.as_str(), "get") else {
            return Decision::Allow;
        };
        let r = self.engine.check_network_request(&req);
        if r.should_block() {
            Decision::Block
        } else {
            Decision::Allow
        }
    }

    /// Site-specific cosmetic filters for a page URL.
    pub fn cosmetic(&self, url: &str) -> Cosmetic {
        let res = self.engine.url_cosmetic_resources(url);
        Cosmetic { css: hide_css(res.hide_selectors.iter()), generichide: res.generichide, exceptions: res.exceptions }
    }

    /// Generic selectors (`##.ad-banner`) relevant to the classes and ids
    /// present on a page.
    pub fn generic_selectors(&self, classes: &[String], ids: &[String], exceptions: &HashSet<String>) -> Vec<String> {
        self.engine.hidden_class_id_selectors(classes, ids, exceptions)
    }
}

/// Builds a `display: none` stylesheet from selectors. Selectors that could
/// break out of the rule are dropped.
pub fn hide_css<'a>(selectors: impl Iterator<Item = &'a String>) -> String {
    let safe: Vec<&str> = selectors
        .map(String::as_str)
        .filter(|s| !s.is_empty() && !s.contains(['{', '}', '<']) && !s.contains("/*"))
        .collect();
    if safe.is_empty() {
        return String::new();
    }
    let mut css = String::new();
    // Chunk so one invalid selector only disables its own group.
    for chunk in safe.chunks(64) {
        css.push_str(&chunk.join(",\n"));
        css.push_str(" { display: none !important; }\n");
    }
    css
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::OnceLock;

    /// The full bundled engine, compiled once for all tests.
    fn full() -> &'static Blocker {
        static B: OnceLock<Blocker> = OnceLock::new();
        B.get_or_init(|| {
            let all: Vec<String> = FILTER_LISTS.iter().map(|s| s.to_string()).collect();
            Blocker::bundled(&all)
        })
    }

    const NEWS: &str = "https://news.example.com/article";

    #[test]
    fn bundled_lists_are_embedded() {
        for id in FILTER_LISTS {
            let text = bundled_list(id).unwrap_or_else(|| panic!("{id} missing"));
            assert!(looks_like_list(&text), "{id}");
            assert!(list_url(id).is_some());
        }
        assert!(bundled_size() < 2 * 1024 * 1024, "deflated lists {} bytes", bundled_size());
    }

    #[test]
    fn blocks_ads_and_trackers() {
        let b = full();
        for (url, kind) in [
            ("https://securepubads.g.doubleclick.net/tag/js/gpt.js", ResourceKind::Script),
            ("https://ad.doubleclick.net/ddm/trackclk/N123", ResourceKind::Image),
            ("https://stats.g.doubleclick.net/r/collect?v=1", ResourceKind::Image),
            ("https://www.google-analytics.com/analytics.js", ResourceKind::Script),
            ("https://www.google-analytics.com/collect?v=1&tid=UA-1", ResourceKind::Xhr),
            ("https://www.googletagmanager.com/gtag/js?id=G-XXXX", ResourceKind::Script),
            ("https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js", ResourceKind::Script),
            ("https://connect.facebook.net/en_US/fbevents.js", ResourceKind::Script),
        ] {
            assert_eq!(b.check(url, NEWS, kind), Decision::Block, "{url} should be blocked");
        }
    }

    #[test]
    fn allows_normal_sites() {
        let b = full();
        for (url, src, kind) in [
            ("https://example.com/", "https://example.com/", ResourceKind::Document),
            ("https://example.com/style.css", "https://example.com/", ResourceKind::Stylesheet),
            ("https://example.com/app.js", "https://example.com/", ResourceKind::Script),
            ("https://www.wikipedia.org/", "https://www.wikipedia.org/", ResourceKind::Document),
            ("https://upload.wikimedia.org/a.png", "https://en.wikipedia.org/wiki/Rust", ResourceKind::Image),
            ("https://doc.rust-lang.org/std/", "https://doc.rust-lang.org/", ResourceKind::Document),
            ("https://krunk.invalid/newtab.html", "", ResourceKind::Document),
            ("data:image/png;base64,AAAA", NEWS, ResourceKind::Image),
        ] {
            assert_eq!(b.check(url, src, kind), Decision::Allow, "{url} should be allowed");
        }
    }

    #[test]
    fn cosmetic_filters() {
        let b = full();
        // Site-specific rule from EasyList (advfn.com###APS_300_X_600).
        let c = b.cosmetic("https://www.advfn.com/");
        assert!(c.css.contains("#APS_300_X_600"), "site css: {}", &c.css[..c.css.len().min(200)]);
        assert!(c.css.contains("display: none !important"));
        // Generic rule (##.ad-banner-300) only when the class is on the page.
        let sel = b.generic_selectors(&["ad-banner-300".into(), "content".into()], &[], &c.exceptions);
        assert!(sel.iter().any(|s| s == ".ad-banner-300"), "{sel:?}");
        assert!(b.generic_selectors(&["content".into()], &[], &HashSet::new()).is_empty());
    }

    #[test]
    fn serialize_round_trip() {
        let lists = ["||ads.example^\n||tracker.example^$third-party\nexample.org##.promo\n"];
        let fp = 0x1234;
        let b = Blocker::from_lists(&lists, fp);
        let bytes = b.serialize();
        assert_eq!(Blocker::cached_key(&bytes), Some(fp));
        let back = Blocker::deserialize(&bytes, fp).unwrap();
        assert_eq!(back.check("https://ads.example/x.js", NEWS, ResourceKind::Script), Decision::Block);
        assert_eq!(back.check("https://tracker.example/p", NEWS, ResourceKind::Image), Decision::Block);
        assert_eq!(back.check("https://ok.example/", NEWS, ResourceKind::Image), Decision::Allow);
        assert!(back.cosmetic("https://example.org/").css.contains(".promo"));
        // Stale or corrupt caches are refused.
        assert!(Blocker::deserialize(&bytes, fp ^ 1).is_err());
        assert!(Blocker::deserialize(b"garbage", fp).is_err());
        let mut bad = bytes.clone();
        bad.truncate(bytes.len() / 2);
        assert!(Blocker::deserialize(&bad, fp).is_err());
        // The full engine round-trips too.
        let full_bytes = full().serialize();
        let full_back = Blocker::deserialize(&full_bytes, full().key()).unwrap();
        assert_eq!(
            full_back.check("https://securepubads.g.doubleclick.net/tag/js/gpt.js", NEWS, ResourceKind::Script),
            Decision::Block
        );
    }

    #[test]
    fn allowlist_matching() {
        let mut list =
            Allowlist::new(vec!["Example.com".to_string(), "*.news.org".to_string(), "bad host".to_string()]);
        assert_eq!(list.len(), 2, "invalid entries dropped, wildcard normalized");
        assert!(list.contains("example.com"));
        assert!(list.contains("www.example.com"));
        assert!(list.contains("deep.a.example.com."));
        assert!(list.contains("a.news.org"));
        assert!(list.contains("news.org"));
        assert!(!list.contains("badexample.com"));
        assert!(!list.contains("example.com.evil.net"));
        assert!(!list.contains("com"));
        assert_eq!(list.matching("www.example.com"), Some("example.com"));
        assert!(list.insert("Shop.Example.net"));
        assert!(!list.insert("shop.example.net"), "no duplicates");
        assert!(!list.insert("not a host"));
        assert!(list.contains("cart.shop.example.net"));
        assert!(list.remove("SHOP.example.net"));
        assert!(!list.contains("cart.shop.example.net"));
        assert!(Allowlist::default().matching("example.com").is_none());
    }

    #[test]
    fn cef_resource_types() {
        use ResourceKind as K;
        let page = "https://news.example/a.js";
        let cases = [
            (1, K::SubFrame),
            (20, K::SubFrame),
            (2, K::Stylesheet),
            (3, K::Script),
            (9, K::Script),
            (10, K::Script),
            (15, K::Script),
            (4, K::Image),
            (12, K::Image),
            (5, K::Font),
            (8, K::Media),
            (13, K::Xhr),
            (14, K::Ping),
            (6, K::Other),
            (7, K::Other),
            (11, K::Other),
            (16, K::Other),
            (17, K::Other),
            (99, K::Other),
        ];
        for (rt, kind) in cases {
            assert_eq!(ResourceKind::from_cef_resource_type(rt, page), Some(kind), "type {rt}");
        }
        assert_eq!(ResourceKind::from_cef_resource_type(0, page), None, "top-level navigation never checked");
        assert_eq!(ResourceKind::from_cef_resource_type(19, page), None, "navigation preload of the main frame");
        assert_eq!(ResourceKind::from_cef_resource_type(6, "wss://live.example/s"), Some(K::WebSocket));
        assert_eq!(ResourceKind::from_cef_resource_type(13, "ws://live.example/s"), Some(K::WebSocket));
    }

    #[test]
    fn request_scope() {
        assert!(is_internal("https://krunk.invalid/newtab.html"));
        assert!(is_internal("https://shell.krunk.invalid/shell.js"));
        assert!(!is_internal("https://krunk.invalid.evil.com/x"));
        assert!(!is_filterable("https://krunk.invalid/newtab.html"));
        assert!(is_filterable("https://ads.example/x.js"));
        assert!(is_filterable("wss://live.example/socket"));
        assert!(!is_filterable("data:image/png;base64,AA"));
        assert!(!is_filterable("blob:https://a.example/123"));
    }

    #[test]
    fn full_engine_with_subframes() {
        // An ad iframe is blocked as a sub-frame; the page itself never is.
        let b = full();
        let ad_frame = "https://googleads.g.doubleclick.net/pagead/ads?client=ca-pub-1";
        assert_eq!(b.check(ad_frame, NEWS, ResourceKind::SubFrame), Decision::Block);
        assert_eq!(ResourceKind::from_cef_resource_type(0, NEWS), None);
    }

    #[test]
    fn css_is_sanitized() {
        let sels = [".ad".to_string(), "x { } body { display:none".to_string(), "#a".to_string()];
        let css = hide_css(sels.iter());
        assert!(css.contains(".ad"));
        assert!(!css.contains("body"));
        assert_eq!(hide_css(Vec::<String>::new().iter()), "");
    }
}

#[cfg(test)]
mod thread_tests {
    #[test]
    fn engine_is_shareable_across_threads() {
        fn shareable<T: Send + Sync>() {}
        shareable::<super::Blocker>();
        shareable::<super::Allowlist>();
    }
}
