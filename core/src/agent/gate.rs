//! The keyword gate for sensitive data (WP-B1): history, bookmarks, downloads and cookies are
//! offered only when the user's message asks for them in words.

use super::CategorySet;

/// The sensitive categories `text` explicitly asks for (English keyword stems, whole-word
/// prefixes, case-insensitive).
pub fn requested(text: &str) -> CategorySet {
    let _ = text;
    CategorySet::EMPTY // WP-B1
}
