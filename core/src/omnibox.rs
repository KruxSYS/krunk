//! Address-bar suggestions (Krunk 0.5.1): what to offer while the user types. The first row
//! is what Enter would do anyway (search the words, or go to the typed address), then the
//! bookmarks and history pages whose title or address matches, most visited first, without
//! duplicates. Since 0.10.2 a typed site name brings up the site itself (`youtube.com` for
//! "yo") with its most visited pages under it, and that row is completed inline and listed
//! first (see [`query`]). Pure logic over the store; the shell and the popup only display and
//! pick.

use crate::protocol::{AddressSuggestion, AddressSuggestionKind, InlineCompletion};
use crate::storage::Store;
use crate::urlclean;

/// Rows per dropdown (the first is the search/address row).
pub const MAX_ROWS: usize = 7;
/// Height of one row and the list's padding, in CSS px of the popup (`popup.css`).
pub const ROW_CSS: f64 = 38.0;
pub const PAD_CSS: f64 = 8.0;

/// The popup height for `rows` rows.
pub fn height_css(rows: usize) -> f64 {
    PAD_CSS + ROW_CSS * rows.max(1) as f64
}

/// Pages of the typed site listed under it.
const SITE_PAGES: usize = 3;

/// The rows and the inline completion for `text`, as the address bar shows them: a completed
/// row moves to the top (it is what Enter does), the search/address row comes next.
pub fn query(store: &Store, text: &str, search_template: &str) -> (Vec<AddressSuggestion>, Option<InlineCompletion>) {
    let mut entries = suggestions(store, text, search_template);
    let mut inline = inline_completion(text, &entries);
    if let Some(i) = inline.as_mut().filter(|i| i.index > 0) {
        let row = entries.remove(i.index);
        entries.insert(0, row);
        i.index = 0;
    }
    (entries, inline)
}

/// The suggestions for `text` (`search_template`: the settings' search URL template).
/// Nothing for blank input.
pub fn suggestions(store: &Store, text: &str, search_template: &str) -> Vec<AddressSuggestion> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(MAX_ROWS);
    let url = urlclean::input_to_url(text, search_template);
    let is_search = url == urlclean::search_url(text, search_template);
    out.push(AddressSuggestion {
        url: url.clone(),
        title: text.to_string(),
        kind: if is_search { AddressSuggestionKind::Search } else { AddressSuggestionKind::Url },
        fill: text.to_string(),
    });
    let mut seen: Vec<String> = vec![url];
    let mut push = |url: String, title: String, kind: AddressSuggestionKind, out: &mut Vec<AddressSuggestion>| {
        if out.len() >= MAX_ROWS || seen.contains(&url) {
            return;
        }
        seen.push(url.clone());
        let title = if title.trim().is_empty() { urlclean::host_of(&url).unwrap_or_default() } else { title };
        out.push(AddressSuggestion { fill: url.clone(), url, title, kind });
    };
    let bookmarks = store.search_bookmarks(text).unwrap_or_default();
    // The site being typed: its home page, then its most visited pages.
    if let Some(site) = typed_site(store, text, &bookmarks) {
        push(site.root, site.title, site.kind, &mut out);
        for (url, title) in site.pages.into_iter().take(SITE_PAGES) {
            push(url, title, AddressSuggestionKind::History, &mut out);
        }
    }
    // Bookmarks (a few), then history by visits.
    for b in bookmarks.into_iter().take(3) {
        if let Some(url) = b.url {
            push(url, b.title, AddressSuggestionKind::Bookmark, &mut out);
        }
    }
    for (url, title) in store.suggest(text, MAX_ROWS as u32 + 3).unwrap_or_default() {
        push(url, title, AddressSuggestionKind::History, &mut out);
    }
    out
}

/// A site whose name the user is typing (`yo` → `https://www.youtube.com/`).
struct Site {
    root: String,
    title: String,
    kind: AddressSuggestionKind,
    /// Its other history pages, most visited first.
    pages: Vec<(String, String)>,
}

/// The site whose host the typed text begins (scheme and `www.` as in [`inline_completion`]):
/// of the history pages and bookmarks on such hosts, the origin with the most visits (a
/// bookmark counts as one). Nothing once the text goes past the host.
fn typed_site(store: &Store, typed: &str, bookmarks: &[crate::bookmarks::Bookmark]) -> Option<Site> {
    let typed_l = typed.trim().to_lowercase();
    if typed_l.is_empty() || typed_l.chars().any(char::is_whitespace) {
        return None;
    }
    let rest = typed_l.strip_prefix("https://").or_else(|| typed_l.strip_prefix("http://")).unwrap_or(&typed_l);
    let start = rest.strip_prefix("www.").unwrap_or(rest);
    if start.is_empty() || start.contains(['/', '?', '#']) {
        return None;
    }
    struct Origin {
        root: String,
        visits: i64,
        title: Option<String>,
        bookmarked: Option<String>,
        pages: Vec<(String, String)>,
    }
    let mut origins: Vec<Origin> = Vec::new();
    let mut add = |url: &str, title: &str, visits: i64, bookmark: bool| {
        let Some(origin) = urlclean::origin_of(url) else { return };
        let root = format!("{origin}/");
        if completion_rest(&typed_l, &root.to_lowercase()).is_none() {
            return;
        }
        let i = match origins.iter().position(|o| o.root == root) {
            Some(i) => i,
            None => {
                origins.push(Origin {
                    root: root.clone(),
                    visits: 0,
                    title: None,
                    bookmarked: None,
                    pages: Vec::new(),
                });
                origins.len() - 1
            }
        };
        let o = &mut origins[i];
        o.visits += visits.max(1);
        if url == root {
            if bookmark {
                o.bookmarked.get_or_insert_with(|| title.to_string());
            } else {
                o.title.get_or_insert_with(|| title.to_string());
            }
        } else if !bookmark {
            o.pages.push((url.to_string(), title.to_string()));
        }
    };
    for (url, title, visits) in store.pages_starting_with(start, 200).unwrap_or_default() {
        add(&url, &title, visits, false);
    }
    for b in bookmarks {
        if let Some(url) = &b.url {
            add(url, &b.title, 1, true);
        }
    }
    // The most visited origin; on a tie the one met first (history is sorted by visits).
    let best = origins.into_iter().reduce(|a, b| if b.visits > a.visits { b } else { a })?;
    let kind = if best.bookmarked.is_some() { AddressSuggestionKind::Bookmark } else { AddressSuggestionKind::History };
    let title = best.title.or(best.bookmarked).filter(|t| !t.trim().is_empty()).unwrap_or_default();
    Some(Site { root: best.root, title, kind, pages: best.pages })
}

/// What follows `typed_l` in `url_l` (both lowercase) when the typed text begins the URL: a
/// typed scheme or `www.` is matched as typed, otherwise both are ignored.
fn completion_rest<'a>(typed_l: &str, url_l: &'a str) -> Option<&'a str> {
    let no_scheme = url_l.strip_prefix("https://").or_else(|| url_l.strip_prefix("http://"))?;
    let bare = no_scheme.strip_prefix("www.").unwrap_or(no_scheme);
    let target = if typed_l.starts_with("http://") || typed_l.starts_with("https://") {
        url_l
    } else if typed_l.starts_with("www.") {
        no_scheme
    } else {
        bare
    };
    target.strip_prefix(typed_l)
}

/// Inline autocomplete (Krunk 0.6.0): the text the address bar shows completed while the user
/// types, like Chrome and Firefox. Only an address is completed, never a search: the typed
/// text must be a prefix of a bookmark's or history page's URL once its scheme and `www.`
/// are ignored (a typed scheme or `www.` is matched as typed), and it must not contain a
/// space. From the first character on (0.10.2; it used to take two). While the text is still
/// within the host only a site's home page is completed (to its host, without the trailing
/// slash), never a deeper page; past the host, the first matching row's full URL. The first
/// matching row wins; the completion keeps what the user typed and appends the rest. Enter
/// goes to that row's URL.
pub fn inline_completion(typed: &str, entries: &[AddressSuggestion]) -> Option<InlineCompletion> {
    let typed = typed.trim();
    if typed.is_empty() || typed.chars().any(char::is_whitespace) {
        return None;
    }
    let typed_l = typed.to_lowercase();
    let typed_rest = typed_l.strip_prefix("https://").or_else(|| typed_l.strip_prefix("http://")).unwrap_or(&typed_l);
    let in_host = !typed_rest.contains('/');
    for (index, e) in entries.iter().enumerate() {
        if matches!(e.kind, AddressSuggestionKind::Search | AddressSuggestionKind::Url) {
            continue;
        }
        let url_l = e.url.to_lowercase();
        let Some(rest) = completion_rest(&typed_l, &url_l) else { continue };
        let no_scheme = url_l.split_once("://").map_or("", |(_, r)| r);
        // A root page completes to its host (no trailing slash); anything else to the full URL.
        let root = !no_scheme.trim_end_matches('/').contains('/');
        if in_host && !root {
            continue;
        }
        // The URL's own case for the completed part (a video id or path is case-sensitive):
        // the same bytes of the URL when lowercasing kept its length (ASCII), else lowercase.
        let rest = match e.url.len() == url_l.len() {
            true => e.url.get(e.url.len() - rest.len()..).unwrap_or(rest),
            false => rest,
        };
        let rest = if root { rest.strip_suffix('/').unwrap_or(rest) } else { rest };
        if rest.is_empty() {
            continue;
        }
        return Some(InlineCompletion { text: format!("{typed}{rest}"), url: e.url.clone(), index });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::BOOKMARKS_BAR;

    fn row(url: &str, kind: AddressSuggestionKind) -> AddressSuggestion {
        AddressSuggestion { url: url.into(), title: String::new(), kind, fill: url.into() }
    }

    #[test]
    fn inline_completion_rules() {
        use AddressSuggestionKind::*;
        let rows = vec![
            row("https://duckduckgo.com/?q=exa", Search),
            row("https://www.example.com/", Bookmark),
            row("https://example.org/docs/intro", History),
            row("http://exam.test/", History),
        ];
        let c = |t: &str| inline_completion(t, &rows);
        // A host prefix completes to the host of the first matching row (no trailing slash).
        let got = c("exa").unwrap();
        assert_eq!((got.text.as_str(), got.url.as_str(), got.index), ("example.com", "https://www.example.com/", 1));
        // The user's case is kept for the typed part.
        assert_eq!(c("EXA").unwrap().text, "EXAmple.com");
        // Past the host: the full URL of the row that still matches.
        assert_eq!(c("example.org/d").unwrap().text, "example.org/docs/intro");
        // A typed scheme or www. is matched literally.
        assert_eq!(c("https://www.exa").unwrap().text, "https://www.example.com");
        assert_eq!(c("www.exa").unwrap().text, "www.example.com");
        // With a scheme the scheme must match too: the http:// row is skipped for "https://exam",
        // and within the host a deeper page is never completed (0.10.2).
        assert!(c("https://exam").is_none());
        assert_eq!(c("https://example.org/").unwrap().text, "https://example.org/docs/intro");
        assert_eq!(c("http://exam.").unwrap().text, "http://exam.test");
        // One character is enough (0.10.2).
        assert_eq!(c("e").unwrap().text, "example.com");
        // Searches are never completed: a space, or no matching address.
        assert!(c("exa mple").is_none());
        assert!(c("zzz").is_none());
        assert!(c("   ").is_none());
        // The typed text equal to a row is not "completed" with nothing.
        assert!(inline_completion("example.com", &[row("https://example.com/", History)]).is_none());
        // A mid-word match only counts as a prefix of the host, never inside it.
        assert!(inline_completion("ample", &rows).is_none());
    }

    const DDG: &str = "https://duckduckgo.com/?q={searchTerms}";

    fn store() -> Store {
        let s = Store::open_in_memory().unwrap();
        s.record_visit("https://www.rust-lang.org/", "Rust Programming Language", 1_700_000_000_000).unwrap();
        s.record_visit("https://www.rust-lang.org/", "Rust Programming Language", 1_700_000_001_000).unwrap();
        s.record_visit("https://docs.rs/serde", "serde - Rust", 1_700_000_002_000).unwrap();
        s.record_visit("https://example.com/", "Example Domain", 1_700_000_003_000).unwrap();
        s.add_bookmark(BOOKMARKS_BAR, "Rust by Example", "https://doc.rust-lang.org/rust-by-example/").unwrap();
        s.add_bookmark(BOOKMARKS_BAR, "Rust home", "https://www.rust-lang.org/").unwrap();
        s
    }

    #[test]
    fn blank_input_offers_nothing() {
        assert!(suggestions(&store(), "   ", DDG).is_empty());
    }

    #[test]
    fn search_row_first_then_bookmarks_and_history_without_duplicates() {
        let got = suggestions(&store(), "rust", DDG);
        assert_eq!(got[0].kind, AddressSuggestionKind::Search);
        assert_eq!(got[0].url, "https://duckduckgo.com/?q=rust");
        assert_eq!(got[0].fill, "rust");
        let urls: Vec<&str> = got.iter().map(|s| s.url.as_str()).collect();
        assert!(urls.contains(&"https://doc.rust-lang.org/rust-by-example/"), "{urls:?}");
        assert!(urls.contains(&"https://www.rust-lang.org/"), "{urls:?}");
        assert!(urls.contains(&"https://docs.rs/serde"), "serde's title mentions Rust: {urls:?}");
        assert!(!urls.contains(&"https://example.com/"));
        assert_eq!(
            urls.iter().filter(|u| **u == "https://www.rust-lang.org/").count(),
            1,
            "a bookmark that is also in history appears once"
        );
        let bm = got.iter().find(|s| s.url == "https://www.rust-lang.org/").unwrap();
        assert_eq!(bm.kind, AddressSuggestionKind::Bookmark);
        assert_eq!(bm.fill, "https://www.rust-lang.org/");
        assert!(got.len() <= MAX_ROWS);
    }

    #[test]
    fn typed_address_is_a_url_row() {
        let got = suggestions(&store(), "example.com", DDG);
        assert_eq!(got[0].kind, AddressSuggestionKind::Url);
        assert_eq!(got[0].url, "https://example.com/");
        assert_eq!(got[0].title, "example.com");
        assert!(
            !got.iter().skip(1).any(|s| s.url == "https://example.com/" && s.kind == AddressSuggestionKind::History),
            "the history row equal to the typed address is not repeated: {got:?}"
        );
        assert_eq!(height_css(1), 46.0);
        assert_eq!(height_css(7), 8.0 + 7.0 * 38.0);
    }

    #[test]
    fn a_typed_site_comes_first_with_its_pages_under_it() {
        let s = Store::open_in_memory().unwrap();
        let watch = "https://www.youtube.com/watch?v=afI-E63m5Ag";
        for t in 0..5 {
            s.record_visit(watch, "A video - YouTube", 1_700_000_000_000 + t).unwrap();
        }
        s.record_visit("https://www.youtube.com/feed/subscriptions", "Subscriptions - YouTube", 1_700_000_010_000)
            .unwrap();
        s.record_visit("https://www.youtube.com/", "YouTube", 1_700_000_020_000).unwrap();
        s.record_visit("https://yourbank.example/login", "Your bank", 1_700_000_030_000).unwrap();
        s.record_visit("https://news.example/yo-yo-tricks", "Yo-yo tricks", 1_700_000_040_000).unwrap();
        for typed in ["y", "yo", "YOU", "www.you", "https://www.yo"] {
            let (rows, inline) = query(&s, typed, DDG);
            let inline = inline.unwrap_or_else(|| panic!("{typed}: no completion"));
            assert_eq!((inline.url.as_str(), inline.index), ("https://www.youtube.com/", 0), "{typed}");
            assert_eq!(rows[0].url, "https://www.youtube.com/", "{typed}");
            assert_eq!(rows[0].title, "YouTube");
            assert!(
                matches!(rows[1].kind, AddressSuggestionKind::Search | AddressSuggestionKind::Url),
                "{typed}: what Enter did before comes next"
            );
            assert_eq!(rows[2].url, watch, "{typed}: the site's most visited page under it");
            assert_eq!(rows[3].url, "https://www.youtube.com/feed/subscriptions", "{typed}");
        }
        assert_eq!(query(&s, "yo", DDG).1.unwrap().text, "youtube.com");
        assert_eq!(query(&s, "www.yo", DDG).1.unwrap().text, "www.youtube.com");
        // The other pages mentioning the text still follow.
        let urls: Vec<String> = query(&s, "yo", DDG).0.into_iter().map(|r| r.url).collect();
        assert!(urls.contains(&"https://news.example/yo-yo-tricks".to_string()), "{urls:?}");
        // A home page never visited still comes first, titled with its host.
        let s2 = Store::open_in_memory().unwrap();
        s2.record_visit(watch, "A video - YouTube", 1_700_000_000_000).unwrap();
        let (rows, inline) = query(&s2, "yo", DDG);
        assert_eq!(inline.unwrap().text, "youtube.com");
        assert_eq!((rows[0].url.as_str(), rows[0].title.as_str()), ("https://www.youtube.com/", "www.youtube.com"));
        assert_eq!(rows[2].url, watch);
        // Past the host the page itself completes; no site row once a path is typed.
        let (rows, inline) = query(&s, "youtube.com/w", DDG);
        assert_eq!(inline.unwrap().text, "youtube.com/watch?v=afI-E63m5Ag");
        assert_eq!(rows[0].url, watch);
        // The most visited of several matching sites wins.
        let (_, inline) = query(&s, "you", DDG);
        assert_eq!(inline.unwrap().url, "https://www.youtube.com/");
        let (_, inline) = query(&s, "your", DDG);
        assert_eq!(inline.unwrap().url, "https://yourbank.example/");
        // Words are searched, never completed.
        let (rows, inline) = query(&s, "yo yo", DDG);
        assert!(inline.is_none());
        assert_eq!(rows[0].kind, AddressSuggestionKind::Search);
    }

    #[test]
    fn a_page_without_title_shows_its_host() {
        let s = Store::open_in_memory().unwrap();
        s.record_visit("https://notitle.example/page", "", 1_700_000_000_000).unwrap();
        let got = suggestions(&s, "notitle", DDG);
        assert_eq!(got[1].title, "notitle.example");
    }
}
