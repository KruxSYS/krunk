//! A site's HTTP sign-in (0.8.0): the rules of Krunk's dialog for Basic and Digest challenges.

/// How long after an answer a second challenge from the same origin and realm counts as "the
/// user name or password was not accepted", and a successful page still offers to save.
pub const ANSWER_WINDOW_MS: i64 = 60_000;

/// The server's realm as the dialog shows it: no control characters, at most 100 characters
/// ("…" when shortened). It is untrusted text; the popup puts it in as text, never as markup.
pub fn clean_realm(realm: &str) -> String {
    let clean: String = realm.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let clean = clean.trim();
    if clean.chars().count() > 100 {
        format!("{}…", clean.chars().take(99).collect::<String>())
    } else {
        clean.to_string()
    }
}

/// The page that came after an answer: anything but 401/407 means the credentials worked.
pub fn accepted(http_status: i32) -> bool {
    http_status != 401 && http_status != 407 && http_status > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realms() {
        assert_eq!(clean_realm("Krunk test area"), "Krunk test area");
        assert_eq!(clean_realm("  a\u{0}b\nc\u{1b}[31m  "), "a b c [31m");
        let long = clean_realm(&"é".repeat(300));
        assert_eq!(long.chars().count(), 100);
        assert!(long.ends_with('…'));
        assert_eq!(clean_realm(""), "");
    }

    #[test]
    fn acceptance() {
        assert!(accepted(200) && accepted(302) && accepted(404) && accepted(500));
        assert!(!accepted(401) && !accepted(407) && !accepted(0));
    }
}
