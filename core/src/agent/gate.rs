//! The keyword gate for sensitive data (WP-B1): history, bookmarks, downloads and cookies are
//! offered only when the user's message asks for them in words.
//!
//! English only (a documented limitation). The message is split into words (letters and
//! digits); a category is requested when one word is:
//! * History: `history`, `histories`, a word starting with `visited` or `browsing`;
//! * Bookmarks: a word starting with `bookmark`, `favorite` or `favourite`;
//! * Downloads: a word starting with `download`;
//! * Cookies: a word starting with `cookie`.
//!
//! The gate only *offers* the tools: the first use per conversation still asks the user.

use super::{Category, CategorySet};

/// The sensitive categories `text` explicitly asks for (English keyword stems, whole-word
/// prefixes, case-insensitive).
pub fn requested(text: &str) -> CategorySet {
    let mut set = CategorySet::EMPTY;
    for word in text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
        let w = word.to_lowercase();
        if let Some(c) = category_of(&w) {
            set.insert(c);
        }
    }
    set
}

fn category_of(w: &str) -> Option<Category> {
    if w == "history" || w == "histories" || w.starts_with("visited") || w.starts_with("browsing") {
        Some(Category::History)
    } else if w.starts_with("bookmark") || w.starts_with("favorite") || w.starts_with("favourite") {
        Some(Category::Bookmarks)
    } else if w.starts_with("download") {
        Some(Category::Downloads)
    } else if w.starts_with("cookie") {
        Some(Category::Cookies)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cats(text: &str) -> Vec<Category> {
        requested(text).iter().collect()
    }

    #[test]
    fn positives() {
        use Category::*;
        for (text, want) in [
            ("What did I look at in my history yesterday?", vec![History]),
            ("Search my HISTORY for recipes", vec![History]),
            ("Which sites have I visited today", vec![History]),
            ("clear my browsing data", vec![History]),
            ("browsing-history please", vec![History]),
            ("show my bookmarks", vec![Bookmarks]),
            ("Bookmark this page", vec![Bookmarks]),
            ("open my favorite site", vec![Bookmarks]),
            ("list my Favourites", vec![Bookmarks]),
            ("what did I download last week?", vec![Downloads]),
            ("Downloads list", vec![Downloads]),
            ("save a screenshot to my downloads folder", vec![Downloads]),
            ("which cookies does this site set", vec![Cookies]),
            ("Cookie banner", vec![Cookies]),
            (
                "Compare my history and bookmarks, then check cookies and downloaded files",
                vec![History, Bookmarks, Downloads, Cookies],
            ),
            ("histories of the web", vec![History]),
        ] {
            assert_eq!(cats(text), want, "{text}");
        }
    }

    #[test]
    fn negatives() {
        for text in [
            "list the links",
            "List me all the links on this page",
            "fill in the form",
            "Change the title of this page to EXAMPLE",
            "a historical overview",
            "prehistory of computing",
            "visit example.com",
            "browse the shop",
            "the user's favor",
            "redownload", // not at a word start
            "Take a screenshot",
            "",
        ] {
            assert!(requested(text).is_empty(), "{text}");
        }
    }
}
