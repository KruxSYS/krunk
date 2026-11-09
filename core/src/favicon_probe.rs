//! Icons for bookmarks of sites that were never visited (0.11.0). Krunk stores a site's icon
//! when a tab shows it; a bookmark that was imported or added without a visit had a plain dot
//! for ever. The app fetches such a site's icon once in the background: `/favicon.ico`, else
//! the page's `<link rel=icon>`.
//!
//! This module decides which hosts to probe and remembers the outcome, so that a site without
//! an icon is not fetched again on every start: a failure is retried after 7 days, or when the
//! user adds a bookmark of it again. A bookmark saved in a private window is never probed.
//! The log lives in the local kv table (`favicon_probe`; not synced, no schema change).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use url::Url;

/// The kv key of the probe log.
pub const KV_KEY: &str = "favicon_probe";
/// A failed probe is tried again after this long.
pub const RETRY_MS: i64 = 7 * 24 * 3600 * 1000;
/// The log keeps at most this many hosts (the oldest outcomes go first).
pub const MAX_HOSTS: usize = 5000;
/// Probes running at once.
pub const PARALLEL: usize = 4;
/// Limits of one fetch.
pub const ICON_MAX_BYTES: usize = 256 * 1024;
pub const HTML_MAX_BYTES: usize = 512 * 1024;
pub const TIMEOUT_MS: i64 = 8_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    /// When it was tried (ms since 1970).
    pub t: i64,
    pub ok: bool,
    /// Bookmarked in a private window: never probed (until bookmarked again in a normal one).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub private: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeLog {
    #[serde(default)]
    hosts: BTreeMap<String, Outcome>,
}

impl ProbeLog {
    /// The stored log; anything unreadable starts over (the worst case is one more probe).
    pub fn parse(json: Option<&str>) -> Self {
        json.and_then(|j| serde_json::from_str(j).ok()).unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".into())
    }

    pub fn get(&self, host: &str) -> Option<Outcome> {
        self.hosts.get(host).copied()
    }

    /// Should `host` (without a stored icon) be probed now?
    pub fn due(&self, host: &str, now: i64) -> bool {
        match self.hosts.get(host) {
            None => true,
            Some(o) if o.private || o.ok => false,
            Some(o) => now - o.t >= RETRY_MS,
        }
    }

    pub fn record(&mut self, host: &str, ok: bool, now: i64) {
        self.hosts.insert(host.to_string(), Outcome { t: now, ok, private: false });
        self.trim();
    }

    /// A bookmark of `host` was added: in a normal window it may be probed again at once; in a
    /// private one it is never probed.
    pub fn bookmark_added(&mut self, host: &str, private: bool, now: i64) {
        if private {
            if !self.hosts.get(host).is_some_and(|o| o.ok) {
                self.hosts.insert(host.to_string(), Outcome { t: now, ok: false, private: true });
            }
        } else {
            self.hosts.remove(host);
        }
        self.trim();
    }

    fn trim(&mut self) {
        while self.hosts.len() > MAX_HOSTS {
            let oldest = self.hosts.iter().min_by_key(|(_, o)| o.t).map(|(h, _)| h.clone());
            match oldest {
                Some(h) => self.hosts.remove(&h),
                None => break,
            };
        }
    }

    pub fn len(&self) -> usize {
        self.hosts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hosts.is_empty()
    }
}

/// A site to probe: its host (the favicon store's key) and its origin (`https://host[:port]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    pub host: String,
    pub origin: String,
}

impl Probe {
    pub fn of(url: &str) -> Option<Self> {
        let u = Url::parse(url).ok()?;
        if !matches!(u.scheme(), "http" | "https") {
            return None;
        }
        let host = u.host_str()?.to_ascii_lowercase();
        let origin = u.origin().ascii_serialization();
        Some(Probe { host, origin })
    }

    pub fn icon_url(&self) -> String {
        format!("{}/favicon.ico", self.origin)
    }

    pub fn page_url(&self) -> String {
        format!("{}/", self.origin)
    }
}

/// The hosts of `urls` (bookmark addresses) to probe now: http(s) only, each host once, without
/// a stored icon (`has_icon`), due in `log`. At most `limit`.
pub fn due_probes<'a>(
    urls: impl IntoIterator<Item = &'a str>,
    has_icon: impl Fn(&str) -> bool,
    log: &ProbeLog,
    now: i64,
    limit: usize,
) -> Vec<Probe> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for url in urls {
        let Some(p) = Probe::of(url) else { continue };
        if !seen.insert(p.host.clone()) || has_icon(&p.host) || !log.due(&p.host, now) {
            continue;
        }
        out.push(p);
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// The icon a page names in its HTML: the first `<link rel="icon">` (or `shortcut icon`), else
/// an `apple-touch-icon`, resolved against `base`. Only http(s) addresses count.
pub fn icon_link(html: &str, base: &str) -> Option<String> {
    let base = Url::parse(base).ok()?;
    let lower = html.to_ascii_lowercase();
    let mut touch = None;
    let mut at = 0;
    while let Some(i) = lower[at..].find("<link") {
        let start = at + i;
        let Some(len) = lower[start..].find('>') else { break };
        let tag = &html[start..start + len];
        at = start + len;
        let rel = attr(tag, "rel").unwrap_or_default().to_ascii_lowercase();
        let Some(href) = attr(tag, "href") else { continue };
        let tokens: Vec<&str> = rel.split_ascii_whitespace().collect();
        let resolved = base.join(href.trim()).ok().filter(|u| matches!(u.scheme(), "http" | "https"));
        let Some(resolved) = resolved else { continue };
        if tokens.contains(&"icon") {
            return Some(resolved.to_string());
        }
        if touch.is_none() && tokens.iter().any(|t| t.starts_with("apple-touch-icon")) {
            touch = Some(resolved.to_string());
        }
        // The body has begun: no more head links.
        if lower[..start].contains("<body") {
            break;
        }
    }
    touch
}

/// One attribute's value in a tag (quoted with " or ', or bare).
fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let pos = from + i;
        from = pos + name.len();
        let before_ok = pos > 0 && lower.as_bytes()[pos - 1].is_ascii_whitespace();
        let rest = lower[from..].trim_start();
        if !before_ok || !rest.starts_with('=') {
            continue;
        }
        let value_at = tag.len() - rest.len() + 1;
        let v = tag[value_at..].trim_start();
        let (val, _) = match v.chars().next()? {
            q @ ('"' | '\'') => v[1..].split_once(q).unwrap_or((&v[1..], "")),
            _ => v.split_once(|c: char| c.is_ascii_whitespace() || c == '>').unwrap_or((v, "")),
        };
        return Some(decode_entities(val));
    }
    None
}

fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&").replace("&#38;", "&").replace("&quot;", "\"").replace("&#39;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 24 * 3600 * 1000;

    #[test]
    fn a_host_is_probed_once_and_a_failure_is_remembered() {
        let mut log = ProbeLog::default();
        let now = 1_800_000_000_000;
        assert!(log.due("a.example", now));
        log.record("a.example", false, now);
        // Never again on the next starts…
        for later in [now + 1, now + DAY, now + 6 * DAY] {
            assert!(!log.due("a.example", later), "{later}");
        }
        // …until 7 days have passed.
        assert!(log.due("a.example", now + 7 * DAY));
        // A success is never probed again.
        log.record("b.example", true, now);
        assert!(!log.due("b.example", now + 365 * DAY));
        // The log survives a restart (kv round trip).
        let back = ProbeLog::parse(Some(&log.to_json()));
        assert_eq!(back, log);
        assert!(!back.due("a.example", now + DAY));
        // A broken value starts over.
        assert_eq!(ProbeLog::parse(Some("not json")), ProbeLog::default());
        assert_eq!(ProbeLog::parse(None), ProbeLog::default());
    }

    #[test]
    fn adding_the_bookmark_again_allows_a_new_probe_but_never_from_a_private_window() {
        let now = 1_800_000_000_000;
        let mut log = ProbeLog::default();
        log.record("a.example", false, now);
        log.bookmark_added("a.example", false, now + 1);
        assert!(log.due("a.example", now + 2), "re-added in a normal window");
        // Bookmarked in a private window: never probed, not even after 7 days…
        log.bookmark_added("p.example", true, now);
        assert!(!log.due("p.example", now + 30 * DAY));
        assert!(log.get("p.example").unwrap().private);
        // …until it is bookmarked in a normal window.
        log.bookmark_added("p.example", false, now + 31 * DAY);
        assert!(log.due("p.example", now + 31 * DAY));
        // A private add never forgets an icon that was found.
        log.record("ok.example", true, now);
        log.bookmark_added("ok.example", true, now + 1);
        assert!(log.get("ok.example").unwrap().ok);
    }

    #[test]
    fn due_probes_skip_hosts_with_icons_duplicates_and_other_schemes() {
        let now = 1_800_000_000_000;
        let mut log = ProbeLog::default();
        log.record("failed.example", false, now - DAY);
        let urls = [
            "https://www.a.example/x?y",
            "https://www.a.example/other",
            "http://plain.example:8080/",
            "https://iconed.example/",
            "https://failed.example/",
            "file:///C:/x.html",
            "javascript:alert(1)",
            "https://c.example/",
        ];
        let p = due_probes(urls, |h| h == "iconed.example", &log, now, 10);
        let hosts: Vec<_> = p.iter().map(|p| (p.host.as_str(), p.origin.as_str())).collect();
        assert_eq!(
            hosts,
            [
                ("www.a.example", "https://www.a.example"),
                ("plain.example", "http://plain.example:8080"),
                ("c.example", "https://c.example")
            ]
        );
        assert_eq!(p[0].icon_url(), "https://www.a.example/favicon.ico");
        assert_eq!(p[1].page_url(), "http://plain.example:8080/");
        assert_eq!(due_probes(urls, |_| false, &log, now, 2).len(), 2);
    }

    #[test]
    fn the_log_is_capped() {
        let mut log = ProbeLog::default();
        for i in 0..(MAX_HOSTS as i64 + 10) {
            log.record(&format!("h{i}.example"), false, i);
        }
        assert_eq!(log.len(), MAX_HOSTS);
        assert!(log.get("h0.example").is_none() && log.get(&format!("h{}.example", MAX_HOSTS + 9)).is_some());
    }

    #[test]
    fn icon_links_in_html() {
        let base = "https://site.example/blog/";
        let html = r#"<html><head><meta charset=utf-8>
            <link rel="stylesheet" href="/s.css">
            <link rel="apple-touch-icon" href="/touch.png">
            <LINK REL='Shortcut Icon' HREF='favicon.png?v=1&amp;x=2'>
            </head><body></body></html>"#;
        assert_eq!(icon_link(html, base).as_deref(), Some("https://site.example/blog/favicon.png?v=1&x=2"));
        let touch_only = r#"<link rel="apple-touch-icon-precomposed" href="//cdn.example/t.png">"#;
        assert_eq!(icon_link(touch_only, base).as_deref(), Some("https://cdn.example/t.png"));
        let bare = "<link href=/i.ico rel=icon>";
        assert_eq!(icon_link(bare, base).as_deref(), Some("https://site.example/i.ico"));
        // No http(s) icon: data: and javascript: are not fetched.
        assert_eq!(icon_link(r#"<link rel="icon" href="data:image/png;base64,AAAA">"#, base), None);
        assert_eq!(icon_link(r#"<link rel="icon" href="javascript:x">"#, base), None);
        assert_eq!(icon_link("<html><body>no icon</body>", base), None);
        // Links in the body after the first non-icon link stop the search.
        assert_eq!(icon_link(r#"<body><link rel="x" href="/a"><link rel="icon" href="/late.ico">"#, base), None);
        // "data-rel" is not "rel".
        assert_eq!(icon_link(r#"<link data-rel="icon" href="/no.ico">"#, base), None);
    }
}
