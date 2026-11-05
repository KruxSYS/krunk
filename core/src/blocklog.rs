//! What the ad blocker blocked on a page (0.4.0): each tab keeps its last [`MAX_ENTRIES`]
//! blocked requests in memory (never on disk, cleared when the tab navigates), and the shield
//! panel and `krunk://adblock` show them grouped by address. The full address is kept for
//! looking up the filter that matched; only the address without its query is shown.

use crate::blocker::ResourceKind;
use serde::{Deserialize, Serialize};

/// Entries a tab keeps (the oldest go first).
pub const MAX_ENTRIES: usize = 500;
/// Characters of an address shown.
const SHOWN_CHARS: usize = 300;

/// One blocked (or replaced) request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub url: String,
    /// The page that made it.
    pub page: String,
    pub kind: ResourceKind,
    /// Answered with a stand-in (`$redirect`) instead of being cancelled.
    pub redirected: bool,
}

/// A blocked address as shown: grouped, with how often it was blocked, and (when looked up)
/// the filter and its list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// The address without its query or fragment.
    pub url: String,
    pub host: String,
    pub kind: String,
    pub count: u32,
    pub redirected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list: Option<String>,
}

/// The address without its query and fragment, shortened.
pub fn shown_url(url: &str) -> String {
    let base = url.split(['?', '#']).next().unwrap_or("");
    base.chars().take(SHOWN_CHARS).collect()
}

/// The entries grouped by shown address and kind, newest first; each with the index of its
/// newest entry (whose full address the filter lookup uses).
pub fn summarize(entries: &[Entry]) -> Vec<(Item, usize)> {
    let mut out: Vec<(Item, usize)> = Vec::new();
    for (i, e) in entries.iter().enumerate().rev() {
        let url = shown_url(&e.url);
        let kind = e.kind.as_str().to_string();
        if let Some((item, _)) = out.iter_mut().find(|(it, _)| it.url == url && it.kind == kind) {
            item.count += 1;
            continue;
        }
        let host = crate::urlclean::host_of(&e.url).unwrap_or_default();
        out.push((Item { url, host, kind, count: 1, redirected: e.redirected, rule: None, list: None }, i));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(url: &str, kind: ResourceKind) -> Entry {
        Entry { url: url.into(), page: "https://news.example/".into(), kind, redirected: false }
    }

    #[test]
    fn groups_newest_first_without_queries() {
        let entries = vec![
            e("https://ads.example/a.js?id=1&user=secret", ResourceKind::Script),
            e("https://px.example/p.gif?u=me#x", ResourceKind::Image),
            e("https://ads.example/a.js?id=2", ResourceKind::Script),
            e("https://ads.example/a.js", ResourceKind::Image),
        ];
        let s = summarize(&entries);
        let shown: Vec<_> = s.iter().map(|(i, n)| (i.url.as_str(), i.kind.as_str(), i.count, *n)).collect();
        assert_eq!(
            shown,
            [
                ("https://ads.example/a.js", "image", 1, 3),
                ("https://ads.example/a.js", "script", 2, 2),
                ("https://px.example/p.gif", "image", 1, 1),
            ]
        );
        assert_eq!(s[1].0.host, "ads.example");
        let json = serde_json::to_string(&s[2].0).unwrap();
        assert!(!json.contains("secret") && !json.contains("u=me") && !json.contains("rule"), "{json}");
        assert_eq!(shown_url(&format!("https://a.example/{}", "x".repeat(500))).chars().count(), SHOWN_CHARS);
        assert!(summarize(&[]).is_empty());
    }
}
