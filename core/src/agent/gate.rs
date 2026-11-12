//! The keyword gate for sensitive data (WP-B1): history, bookmarks, downloads and cookies are
//! offered only when the user's message asks for them in words. 0.16.0: so is entering a
//! password (`fill_password`).
//!
//! English only (a documented limitation). The message is split into words (letters and
//! digits); a category is requested when one word is:
//! * History: `history`, `histories`, a word starting with `visited` or `browsing`;
//! * Bookmarks: a word starting with `bookmark`, `favorite` or `favourite`;
//! * Downloads: a word starting with `download`;
//! * Cookies: a word starting with `cookie`;
//! * Passwords (0.16.0), when the message asks for a password to be created or entered:
//!   - a word starting with `password`, `passcode` or `passphrase`;
//!   - `signup`, `register`, `registers`, `registering`, `registration`;
//!   - `sign` followed by `up` within two words ("sign up", "sign me up");
//!   - `create`, `creating`, `make` or `making` followed by `account` or `accounts` within
//!     three words ("create an account", "make me a new account"), or `new account`.
//!
//!   "log in" or "sign in" alone is not enough: the message has to name the password.
//!
//! The gate only *offers* the tools: the first use per conversation still asks the user (for a
//! password: the first use on each site).

use super::{Category, CategorySet};

/// The sensitive categories `text` explicitly asks for (English keyword stems, whole-word
/// prefixes, case-insensitive).
pub fn requested(text: &str) -> CategorySet {
    let words: Vec<String> =
        text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect();
    let mut set = CategorySet::EMPTY;
    for w in &words {
        if let Some(c) = category_of(w) {
            set.insert(c);
        }
    }
    if asks_for_password(&words) {
        set.insert(Category::Passwords);
    }
    set
}

/// Does the message ask for a password to be created or entered? (`words`: lower case.)
fn asks_for_password(words: &[String]) -> bool {
    words.iter().enumerate().any(|(i, w)| {
        // One of `what` among the next `n` words.
        let soon = |n: usize, what: &[&str]| words[i + 1..].iter().take(n).any(|x| what.contains(&x.as_str()));
        w.starts_with("password")
            || w.starts_with("passcode")
            || w.starts_with("passphrase")
            || matches!(w.as_str(), "signup" | "register" | "registers" | "registering" | "registration")
            || (w == "sign" && soon(2, &["up"]))
            || (matches!(w.as_str(), "create" | "creating" | "make" | "making") && soon(3, &["account", "accounts"]))
            || (w == "new" && soon(1, &["account"]))
    })
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

    /// 0.16.0: entering a password is offered only when the message asks for one.
    #[test]
    fn passwords_positives() {
        for text in [
            "Create an account with a generated password",
            "sign me up for this newsletter",
            "Sign up here with a strong password",
            "signup please",
            "Register on this site",
            "start the registration",
            "create a new account here",
            "Make me an account on this forum",
            "I need a new account",
            "log in with password hunter2",
            "Log in: my PASSWORD is abc",
            "change my password to something strong",
            "enter the passcode 1234",
            "set a passphrase",
            "generate passwords for both fields",
        ] {
            assert!(requested(text).contains(Category::Passwords), "{text}");
        }
        // With other categories in the same message.
        assert_eq!(cats("sign up, then show my bookmarks"), vec![Category::Bookmarks, Category::Passwords]);
    }

    #[test]
    fn passwords_negatives() {
        for text in [
            "fill in the form",
            "Fill in this form with test data",
            "log in to the site",
            "sign in with my saved login",
            "click the login button",
            "open my account page",
            "what does my account say",
            "which accounts are listed here",
            "sign the petition and scroll up", // "up" is too far from "sign"
            "make a list of the new accounts", // "accounts" is four words after "make"
            "pass the word on",
            "this page is registered to someone",
            "list the links",
            "",
        ] {
            assert!(!requested(text).contains(Category::Passwords), "{text}");
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
