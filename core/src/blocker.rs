//! Ad and tracker blocking on top of Brave's `adblock` engine.
//!
//! Network requests are checked with [`Blocker::check`]; cosmetic filtering
//! gives the host a stylesheet per page ([`Blocker::cosmetic`]) plus generic
//! selectors for the classes and ids a page actually uses
//! ([`Blocker::generic_selectors`]).
//!
//! Compiling the lists takes a moment, so the compiled engine is serialized
//! to disk ([`Blocker::serialize`]) and reloaded on the next start. The cache
//! header records the crate version and a fingerprint of the list texts, so
//! a stale cache is detected and rebuilt.

use crate::settings::FILTER_LISTS;
use ::adblock::lists::{FilterSet, ParseOptions};
use ::adblock::request::Request;
use ::adblock::Engine;
use std::collections::HashSet;

const CACHE_MAGIC: &[u8; 4] = b"LBAB";
const CACHE_VERSION: u32 = 1;
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

/// Request type, mapped from the WebView2 resource context.
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
    fingerprint: u64,
}

impl std::fmt::Debug for Blocker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Blocker").field("fingerprint", &self.fingerprint).finish()
    }
}

/// FNV-1a: a stable fingerprint of the list texts for cache invalidation.
fn fingerprint<'a>(lists: impl IntoIterator<Item = (&'a str, &'a str)>) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };
    eat(ENGINE_VERSION.as_bytes());
    for (id, text) in lists {
        eat(id.as_bytes());
        eat(&[0]);
        eat(text.as_bytes());
        eat(&[0]);
    }
    h
}

impl Blocker {
    /// Compiles an engine from `(list id, list text)` pairs.
    pub fn from_lists(lists: &[(&str, &str)]) -> Blocker {
        let mut set = FilterSet::new(false);
        for (_, text) in lists {
            set.add_filter_list(text.to_string(), ParseOptions::default());
        }
        Blocker { engine: Engine::new_with_filter_set(set), fingerprint: fingerprint(lists.iter().copied()) }
    }

    /// Compiles the enabled bundled lists.
    pub fn bundled(enabled: &[String]) -> Blocker {
        let texts: Vec<(&str, String)> = FILTER_LISTS
            .iter()
            .filter(|id| enabled.iter().any(|e| e == *id))
            .filter_map(|id| Some((*id, bundled_list(id)?)))
            .collect();
        let refs: Vec<(&str, &str)> = texts.iter().map(|(i, t)| (*i, t.as_str())).collect();
        Blocker::from_lists(&refs)
    }

    /// Fingerprint the cache must match for these list texts.
    pub fn expected_fingerprint(lists: &[(&str, &str)]) -> u64 {
        fingerprint(lists.iter().copied())
    }

    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    /// Serializes the compiled engine with our cache header.
    pub fn serialize(&self) -> Vec<u8> {
        let body = self.engine.serialize();
        let mut out = Vec::with_capacity(body.len() + 16);
        out.extend_from_slice(CACHE_MAGIC);
        out.extend_from_slice(&CACHE_VERSION.to_le_bytes());
        out.extend_from_slice(&self.fingerprint.to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    /// Loads a cached engine. Fails if the header does not match or, when
    /// `expect` is given, the cache was built from different lists.
    pub fn deserialize(data: &[u8], expect: Option<u64>) -> Result<Blocker, String> {
        if data.len() < 16 || &data[..4] != CACHE_MAGIC {
            return Err("not an engine cache".into());
        }
        let version = u32::from_le_bytes(data[4..8].try_into().map_err(|_| "header")?);
        if version != CACHE_VERSION {
            return Err("cache version mismatch".into());
        }
        let fp = u64::from_le_bytes(data[8..16].try_into().map_err(|_| "header")?);
        if expect.is_some_and(|e| e != fp) {
            return Err("cache built from different lists".into());
        }
        let mut engine = Engine::default();
        engine.deserialize(&data[16..]).map_err(|e| format!("{e:?}"))?;
        Ok(Blocker { engine, fingerprint: fp })
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

/// Is ad blocking disabled for `host` (exact host or any subdomain of an
/// allowlist entry)?
pub fn is_allowlisted(host: &str, allowlist: &[String]) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    allowlist.iter().any(|entry| {
        let e = entry.trim_start_matches("*.").trim_end_matches('.').to_ascii_lowercase();
        !e.is_empty() && (host == e || host.ends_with(&format!(".{e}")))
    })
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
            ("https://lightbrowser.invalid/newtab.html", "", ResourceKind::Document),
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
        let lists = [("test", "||ads.example^\n||tracker.example^$third-party\nexample.org##.promo\n")];
        let b = Blocker::from_lists(&lists);
        let bytes = b.serialize();
        let fp = Blocker::expected_fingerprint(&lists);
        let back = Blocker::deserialize(&bytes, Some(fp)).unwrap();
        assert_eq!(back.check("https://ads.example/x.js", NEWS, ResourceKind::Script), Decision::Block);
        assert_eq!(back.check("https://tracker.example/p", NEWS, ResourceKind::Image), Decision::Block);
        assert_eq!(back.check("https://ok.example/", NEWS, ResourceKind::Image), Decision::Allow);
        assert!(back.cosmetic("https://example.org/").css.contains(".promo"));
        // Stale or corrupt caches are refused.
        assert!(Blocker::deserialize(&bytes, Some(fp ^ 1)).is_err());
        assert!(Blocker::deserialize(b"garbage", None).is_err());
        let mut bad = bytes.clone();
        bad.truncate(bytes.len() / 2);
        assert!(Blocker::deserialize(&bad, None).is_err());
        // The full engine round-trips too.
        let full_bytes = full().serialize();
        let full_back = Blocker::deserialize(&full_bytes, Some(full().fingerprint())).unwrap();
        assert_eq!(
            full_back.check("https://securepubads.g.doubleclick.net/tag/js/gpt.js", NEWS, ResourceKind::Script),
            Decision::Block
        );
    }

    #[test]
    fn allowlist_matching() {
        let list = vec!["example.com".to_string(), "*.news.org".to_string()];
        assert!(is_allowlisted("example.com", &list));
        assert!(is_allowlisted("www.example.com", &list));
        assert!(is_allowlisted("a.news.org", &list));
        assert!(is_allowlisted("news.org", &list));
        assert!(!is_allowlisted("badexample.com", &list));
        assert!(!is_allowlisted("example.com.evil.net", &list));
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
