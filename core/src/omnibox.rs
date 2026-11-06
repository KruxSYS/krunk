//! Address-bar suggestions (Krunk 0.5.1): what to offer while the user types. The first row
//! is what Enter would do anyway (search the words, or go to the typed address), then the
//! bookmarks and history pages whose title or address matches, most visited first, without
//! duplicates. Pure logic over the store; the shell and the popup only display and pick.

use crate::protocol::{AddressSuggestion, AddressSuggestionKind};
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
    // Bookmarks first (a few), then history by visits.
    for b in store.search_bookmarks(text).unwrap_or_default().into_iter().take(3) {
        if let Some(url) = b.url {
            push(url, b.title, AddressSuggestionKind::Bookmark, &mut out);
        }
    }
    for (url, title) in store.suggest(text, MAX_ROWS as u32 + 3).unwrap_or_default() {
        push(url, title, AddressSuggestionKind::History, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::BOOKMARKS_BAR;

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
    fn a_page_without_title_shows_its_host() {
        let s = Store::open_in_memory().unwrap();
        s.record_visit("https://notitle.example/page", "", 1_700_000_000_000).unwrap();
        let got = suggestions(&s, "notitle", DDG);
        assert_eq!(got[1].title, "notitle.example");
    }
}
