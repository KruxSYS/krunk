//! Cookies and site storage for the `krunk://storage` and `krunk://sitedata`
//! pages: the view model, grouping by site, and which cookies a delete
//! request means. The engine calls (cookie manager, DevTools protocol) are
//! in the app; everything here is pure.

use crate::cookies::site_of;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Microseconds between the Windows epoch (1601, CEF's base time) and the
/// Unix epoch.
pub const WINDOWS_TO_UNIX_US: i64 = 11_644_473_600_000_000;

/// Limits for edits from the site-data page.
pub const MAX_KEY_LEN: usize = 4096;
pub const MAX_VALUE_LEN: usize = 1 << 20;
/// Most origins one usage request may ask for.
pub const MAX_USAGE_ORIGINS: usize = 200;

/// A cookie's SameSite attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SameSite {
    Unspecified,
    None,
    Lax,
    Strict,
}

/// One cookie as the pages show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CookieInfo {
    pub name: String,
    pub value: String,
    /// As stored: a leading dot means "and subdomains".
    pub domain: String,
    pub path: String,
    /// Unix ms; `None` for session cookies.
    pub expires: Option<i64>,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: SameSite,
}

impl CookieInfo {
    /// The site (registrable domain) the cookie belongs to.
    pub fn site(&self) -> String {
        site_of(self.domain.trim_start_matches('.'))
    }

    pub fn key(&self) -> CookieKey {
        CookieKey { name: self.name.clone(), domain: self.domain.clone(), path: self.path.clone() }
    }
}

/// Identifies one cookie (name, domain and path are unique together).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CookieKey {
    pub name: String,
    pub domain: String,
    pub path: String,
}

impl CookieKey {
    pub fn is_valid(&self) -> bool {
        self.name.len() <= MAX_KEY_LEN && !self.domain.is_empty() && self.domain.len() <= 255 && self.path.len() <= 4096
    }
}

/// What a cookie deletion means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CookieSelection {
    One(CookieKey),
    /// Every cookie of this site (registrable domain).
    Site(String),
    All,
}

impl CookieSelection {
    pub fn matches(&self, c: &CookieInfo) -> bool {
        match self {
            CookieSelection::One(k) => c.name == k.name && c.domain == k.domain && c.path == k.path,
            CookieSelection::Site(site) => c.site() == *site,
            CookieSelection::All => true,
        }
    }
}

/// A site in the cookie and storage list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteSummary {
    pub site: String,
    pub cookies: u32,
    /// Web origins of this site that may hold storage (visited or open).
    pub origins: Vec<String>,
}

/// Groups cookies and known origins by site, sorted by site name.
pub fn summarize(cookies: &[CookieInfo], origins: &[String]) -> Vec<SiteSummary> {
    let mut sites: BTreeMap<String, SiteSummary> = BTreeMap::new();
    fn site_entry(sites: &mut BTreeMap<String, SiteSummary>, site: String) -> &mut SiteSummary {
        sites.entry(site.clone()).or_insert_with(|| SiteSummary { site, cookies: 0, origins: Vec::new() })
    }
    for c in cookies {
        site_entry(&mut sites, c.site()).cookies += 1;
    }
    for o in origins {
        let Some(host) = origin_host(o) else { continue };
        let s = site_entry(&mut sites, site_of(&host));
        if !s.origins.contains(o) {
            s.origins.push(o.clone());
        }
    }
    let mut list: Vec<SiteSummary> = sites.into_values().collect();
    for s in &mut list {
        s.origins.sort();
    }
    list
}

/// The cookies of one site, sorted by domain, path and name.
pub fn cookies_of_site(cookies: &[CookieInfo], site: &str) -> Vec<CookieInfo> {
    let mut list: Vec<CookieInfo> = cookies.iter().filter(|c| c.site() == site).cloned().collect();
    list.sort_by(|a, b| (&a.domain, &a.path, &a.name).cmp(&(&b.domain, &b.path, &b.name)));
    list
}

/// The cookies a page on `origin` can see: its host and parent domains
/// (path and Secure are not considered: this is for listing and deleting
/// a site's cookies, not for sending them).
pub fn cookies_for_origin(cookies: &[CookieInfo], origin: &str) -> Vec<CookieInfo> {
    let Some(host) = origin_host(origin) else { return Vec::new() };
    let mut list: Vec<CookieInfo> = cookies
        .iter()
        .filter(|c| {
            let d = c.domain.trim_start_matches('.');
            host == d || (c.domain.starts_with('.') && host.ends_with(&format!(".{d}")))
        })
        .cloned()
        .collect();
    list.sort_by(|a, b| (&a.domain, &a.path, &a.name).cmp(&(&b.domain, &b.path, &b.name)));
    list
}

/// `scheme://host[:port]` of an http(s) URL, or `None`. Hosts in the
/// reserved `.invalid` TLD (Krunk's own pages) are not web origins.
pub fn web_origin(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    let host = u.host_str()?;
    if !matches!(u.scheme(), "http" | "https") || host == "invalid" || host.ends_with(".invalid") {
        return None;
    }
    Some(u.origin().ascii_serialization())
}

/// Is `origin` exactly an http(s) origin (no path, query or credentials)?
pub fn is_web_origin(origin: &str) -> bool {
    web_origin(origin).is_some_and(|o| o == origin)
}

fn origin_host(origin: &str) -> Option<String> {
    web_origin(origin)?;
    let u = url::Url::parse(origin).ok()?;
    u.host_str().map(|h| h.trim_start_matches('[').trim_end_matches(']').to_string())
}

/// Is `site` a plausible registrable domain (what the storage page sends)?
pub fn is_site(site: &str) -> bool {
    !site.is_empty()
        && site.len() <= 255
        && site.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '[' | ']'))
}

/// CEF base time (µs since 1601) to Unix ms.
pub fn windows_us_to_unix_ms(us: i64) -> i64 {
    us.saturating_sub(WINDOWS_TO_UNIX_US) / 1000
}

/// Usage of one origin from `Storage.getUsageAndQuota`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OriginUsage {
    pub origin: String,
    /// Bytes; `None` if the engine could not tell.
    pub usage: Option<f64>,
    /// Non-zero parts: (`local_storage`, bytes), (`indexeddb`, bytes), ...
    pub breakdown: Vec<(String, f64)>,
}

impl OriginUsage {
    /// Reads the DevTools protocol result
    /// (`{"usage":n,"quota":n,"usageBreakdown":[{"storageType":"indexeddb","usage":n}]}`).
    pub fn from_devtools(origin: &str, result: Option<&serde_json::Value>) -> OriginUsage {
        let usage = result.and_then(|r| r.get("usage")).and_then(serde_json::Value::as_f64);
        let mut breakdown: Vec<(String, f64)> = result
            .and_then(|r| r.get("usageBreakdown"))
            .and_then(serde_json::Value::as_array)
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|p| {
                        let kind = p.get("storageType")?.as_str()?;
                        let bytes = p.get("usage")?.as_f64()?;
                        (bytes > 0.0 && kind != "all").then(|| (kind.to_string(), bytes))
                    })
                    .collect()
            })
            .unwrap_or_default();
        breakdown.sort_by(|a, b| b.1.total_cmp(&a.1));
        OriginUsage { origin: origin.to_string(), usage, breakdown }
    }
}

/// Which DOM storage the site-data page edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StorageArea {
    Local,
    Session,
}

/// Reads `DOMStorage.getDOMStorageItems` (`{"entries":[["k","v"],...]}`).
pub fn dom_storage_items(result: Option<&serde_json::Value>) -> Vec<(String, String)> {
    let mut items: Vec<(String, String)> = result
        .and_then(|r| r.get("entries"))
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|e| {
                    let pair = e.as_array()?;
                    Some((pair.first()?.as_str()?.to_string(), pair.get(1)?.as_str()?.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    items.sort();
    items
}

/// The `storageId` of DOMStorage calls for `origin` (storage key of a
/// first-party, unpartitioned page: the origin with a trailing slash).
pub fn dom_storage_id(origin: &str, area: StorageArea) -> serde_json::Value {
    serde_json::json!({ "storageKey": format!("{origin}/"), "isLocalStorage": area == StorageArea::Local })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cookie(name: &str, domain: &str) -> CookieInfo {
        CookieInfo {
            name: name.into(),
            value: "v".into(),
            domain: domain.into(),
            path: "/".into(),
            expires: None,
            secure: true,
            http_only: false,
            same_site: SameSite::Lax,
        }
    }

    #[test]
    fn grouping_by_site() {
        let cookies = [
            cookie("a", ".example.com"),
            cookie("b", "www.example.com"),
            cookie("c", "news.example.co.uk"),
            cookie("d", "localhost"),
        ];
        let origins =
            ["https://shop.example.com".to_string(), "http://localhost:8081".into(), "https://other.org".into()];
        let s = summarize(&cookies, &origins);
        let names: Vec<&str> = s.iter().map(|x| x.site.as_str()).collect();
        assert_eq!(names, ["example.co.uk", "example.com", "localhost", "other.org"]);
        let ex = &s[1];
        assert_eq!(ex.cookies, 2);
        assert_eq!(ex.origins, ["https://shop.example.com"]);
        assert_eq!(s[2].origins, ["http://localhost:8081"]);
        assert_eq!(s[3].cookies, 0, "an origin without cookies is listed for its storage");
        assert_eq!(cookies_of_site(&cookies, "example.com").len(), 2);
    }

    #[test]
    fn selections() {
        let a = cookie("a", ".example.com");
        let b = cookie("b", "www.example.com");
        let other = cookie("a", "tracker.example");
        let one = CookieSelection::One(a.key());
        assert!(one.matches(&a) && !one.matches(&b) && !one.matches(&other));
        let site = CookieSelection::Site("example.com".into());
        assert!(site.matches(&a) && site.matches(&b) && !site.matches(&other));
        assert!(CookieSelection::All.matches(&other));
        let mut wrong_path = a.key();
        wrong_path.path = "/x".into();
        assert!(!CookieSelection::One(wrong_path).matches(&a));
    }

    #[test]
    fn cookies_visible_to_an_origin() {
        let cookies = [
            cookie("host", "www.example.com"),
            cookie("parent", ".example.com"),
            cookie("sibling", "shop.example.com"),
            cookie("other", ".example.org"),
        ];
        let names: Vec<String> =
            cookies_for_origin(&cookies, "https://www.example.com").into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["parent", "host"]);
        assert!(cookies_for_origin(&cookies, "krunk://settings").is_empty());
    }

    #[test]
    fn origins_and_sites() {
        assert_eq!(web_origin("https://a.example:8443/x?y#z").as_deref(), Some("https://a.example:8443"));
        assert_eq!(web_origin("krunk://storage"), None);
        assert_eq!(web_origin("https://krunk.invalid/storage.html"), None, "our own pages");
        assert_eq!(web_origin("https://shell.krunk.invalid/shell.html"), None);
        assert_eq!(web_origin("file:///C:/x"), None);
        assert!(is_web_origin("https://a.example"));
        assert!(!is_web_origin("https://a.example/"));
        assert!(!is_web_origin("javascript:alert(1)"));
        assert!(is_site("example.co.uk") && is_site("127.0.0.1"));
        assert!(!is_site("") && !is_site("a b") && !is_site("<x>"));
    }

    #[test]
    fn times() {
        assert_eq!(windows_us_to_unix_ms(WINDOWS_TO_UNIX_US + 1_500_000), 1_500);
    }

    #[test]
    fn devtools_results() {
        let r = serde_json::json!({"usage": 2048.0, "quota": 1e9, "usageBreakdown": [
            {"storageType": "indexeddb", "usage": 1500}, {"storageType": "local_storage", "usage": 548},
            {"storageType": "cache_storage", "usage": 0}, {"storageType": "all", "usage": 2048}]});
        let u = OriginUsage::from_devtools("https://a.example", Some(&r));
        assert_eq!(u.usage, Some(2048.0));
        assert_eq!(u.breakdown, [("indexeddb".to_string(), 1500.0), ("local_storage".to_string(), 548.0)]);
        assert_eq!(OriginUsage::from_devtools("https://a.example", None).usage, None);
        let items = serde_json::json!({"entries": [["theme", "dark"], ["a", "1"], ["bad"]]});
        assert_eq!(dom_storage_items(Some(&items)), [("a".into(), "1".into()), ("theme".into(), "dark".into())]);
        assert_eq!(
            dom_storage_id("https://a.example", StorageArea::Session),
            serde_json::json!({"storageKey": "https://a.example/", "isLocalStorage": false})
        );
    }
}
