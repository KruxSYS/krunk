//! The back and forward buttons' history menus (Krunk 0.8.0, B): right-click Back for the tab's
//! earlier pages (newest first), Forward for the later ones (in order). The host reads the tab's
//! navigation list from the engine (`Page.getNavigationHistory`) and jumps with
//! `Page.navigateToHistoryEntry`, which is the same as pressing Back or Forward that many times.
//! Nothing here is stored: the list lives in the engine and in the open menu only.

use serde::{Deserialize, Serialize};

/// Entries in one menu, as in Chrome; "Show full history" comes after them.
pub const MAX_ITEMS: usize = 15;

/// One entry of the engine's navigation list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: i64,
    pub url: String,
    pub title: String,
}

/// `{currentIndex, entries:[{id, url, title, …}]}` → the current index and the entries.
pub fn parse_history(v: &serde_json::Value) -> Option<(usize, Vec<Entry>)> {
    let current = usize::try_from(v.get("currentIndex")?.as_i64()?).ok()?;
    let entries: Vec<Entry> = v
        .get("entries")?
        .as_array()?
        .iter()
        .filter_map(|e| {
            Some(Entry {
                id: e.get("id")?.as_i64()?,
                url: e.get("url")?.as_str()?.to_string(),
                title: e.get("title").and_then(|t| t.as_str()).unwrap_or("").to_string(),
            })
        })
        .collect();
    (current < entries.len()).then_some((current, entries))
}

/// What one menu row shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub entry_id: i64,
    /// The title, or the address when the page has none.
    pub label: String,
    /// The address as the address bar shows it (the tooltip).
    pub url: String,
    /// Same-origin URL of the site's cached icon, if any (the popup shows a globe otherwise).
    pub icon: Option<String>,
}

/// The entries of one menu: before `current` newest first (Back), or after it in order
/// (Forward); at most `MAX_ITEMS`.
/// A tab's first entry `about:blank` (a tab that started blank before its page, 0.11.1) is
/// never offered: the user never saw it.
pub fn entries_for(current: usize, entries: &[Entry], forward: bool) -> Vec<&Entry> {
    let skip_first = entries.first().is_some_and(|e| crate::pages::is_blank(&e.url));
    if forward {
        entries.iter().skip(current + 1).take(MAX_ITEMS).collect()
    } else {
        entries[usize::from(skip_first).min(current)..current.min(entries.len())].iter().rev().take(MAX_ITEMS).collect()
    }
}

/// The label of an entry: its title, else the shown address; at most 200 characters (the
/// popup ellipsizes what doesn't fit).
pub fn label(title: &str, shown_url: &str) -> String {
    let t = title.trim();
    let s = if t.is_empty() { shown_url } else { t };
    s.chars().take(200).collect()
}

/// The entry ids a menu offered, so a click can only jump to one of them.
pub fn offered_ids(items: &[Item]) -> Vec<i64> {
    items.iter().map(|i| i.entry_id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn history(n: usize, current: usize) -> serde_json::Value {
        let entries: Vec<_> = (0..n)
            .map(|i| json!({"id": 100 + i, "url": format!("http://localhost:8094/p{i}.html"), "title": format!("P{i}"), "userTypedURL": "", "transitionType": "link"}))
            .collect();
        json!({"currentIndex": current, "entries": entries})
    }

    #[test]
    fn a_blank_first_entry_is_never_offered() {
        let e = |id, url: &str| Entry { id, url: url.into(), title: String::new() };
        let list = [e(1, "about:blank"), e(2, "https://a.example/"), e(3, "https://b.example/")];
        let back: Vec<i64> = entries_for(2, &list, false).iter().map(|x| x.id).collect();
        assert_eq!(back, [2], "about:blank (the start of a tab with edited headers) is left out");
        assert!(entries_for(1, &list, false).is_empty(), "nothing to go back to");
        // A blank page the user went to later stays.
        let later = [e(1, "https://a.example/"), e(2, "about:blank"), e(3, "https://b.example/")];
        assert_eq!(entries_for(2, &later, false).len(), 2);
    }

    #[test]
    fn parses_the_engine_answer() {
        let (cur, e) = parse_history(&history(5, 4)).unwrap();
        assert_eq!(cur, 4);
        assert_eq!(e.len(), 5);
        assert_eq!(e[0], Entry { id: 100, url: "http://localhost:8094/p0.html".into(), title: "P0".into() });
        // No title field: an empty title.
        let v = json!({"currentIndex": 0, "entries": [{"id": 1, "url": "about:blank"}]});
        assert_eq!(parse_history(&v).unwrap().1[0].title, "");
        for bad in [
            json!({}),
            json!({"currentIndex": 3, "entries": []}),
            json!({"currentIndex": -1, "entries": [{"id": 1, "url": "x"}]}),
        ] {
            assert!(parse_history(&bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn back_is_newest_first_forward_in_order() {
        let (_, e) = parse_history(&history(6, 3)).unwrap();
        let back: Vec<i64> = entries_for(3, &e, false).iter().map(|x| x.id).collect();
        assert_eq!(back, vec![102, 101, 100]);
        let fwd: Vec<i64> = entries_for(3, &e, true).iter().map(|x| x.id).collect();
        assert_eq!(fwd, vec![104, 105]);
        assert!(entries_for(0, &e, false).is_empty(), "nothing before the first entry");
        assert!(entries_for(5, &e, true).is_empty(), "nothing after the last one");
    }

    #[test]
    fn at_most_fifteen() {
        let (cur, e) = parse_history(&history(40, 30)).unwrap();
        let back = entries_for(cur, &e, false);
        assert_eq!(back.len(), MAX_ITEMS);
        assert_eq!(back[0].id, 129, "the nearest first");
        assert_eq!(back[14].id, 115);
        let (cur, e) = parse_history(&history(40, 2)).unwrap();
        let fwd = entries_for(cur, &e, true);
        assert_eq!((fwd.len(), fwd[0].id, fwd[14].id), (15, 103, 117));
    }

    #[test]
    fn labels() {
        assert_eq!(label("  News  ", "https://x.example/"), "News");
        assert_eq!(label("", "https://x.example/"), "https://x.example/");
        assert_eq!(label("   ", "krunk://newtab"), "krunk://newtab");
        assert_eq!(label(&"é".repeat(300), "u").chars().count(), 200);
    }
}
