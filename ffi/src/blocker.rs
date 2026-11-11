//! The per-site allow list of the ad and tracker blocker, with `core::blocker`'s rules: the
//! same normalisation and the same "an entry covers its subdomains" matching as on Windows.
//! The rows live in the shared `adblock_allowlist` table (`KrunkStore::allowlist*`).
use lb_core::blocker::{normalize_host, Allowlist};

/// The form a host is stored in on the allow list (lower case, without `*.` and a trailing
/// dot), or `None` when it is not a host name (an address with a scheme, a path, a space).
#[uniffi::export]
pub fn allow_host(host: String) -> Option<String> {
    normalize_host(&host)
}

/// The entry of `entries` that covers `host` (`news.example.com` is covered by `example.com`),
/// if any. Entries that are not host names are ignored.
#[uniffi::export]
pub fn allow_entry_for(entries: Vec<String>, host: String) -> Option<String> {
    Allowlist::new(entries).matching(&host).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_are_normalised_like_on_windows() {
        assert_eq!(allow_host("WWW.Example.com.".into()).as_deref(), Some("www.example.com"));
        assert_eq!(allow_host("*.news.org".into()).as_deref(), Some("news.org"));
        assert_eq!(allow_host("10.250.136.3".into()).as_deref(), Some("10.250.136.3"));
        for bad in ["", "https://example.com", "example.com/path", "two words", "user@example.com"] {
            assert_eq!(allow_host(bad.into()), None, "{bad}");
        }
    }

    #[test]
    fn an_entry_covers_its_subdomains_and_nothing_else() {
        let list = vec!["example.com".to_string(), "shop.site.org".to_string(), "not a host".to_string()];
        let m = |h: &str| allow_entry_for(list.clone(), h.to_string());
        assert_eq!(m("example.com").as_deref(), Some("example.com"));
        assert_eq!(m("News.Example.com").as_deref(), Some("example.com"));
        assert_eq!(m("a.b.example.com.").as_deref(), Some("example.com"));
        assert_eq!(m("shop.site.org").as_deref(), Some("shop.site.org"));
        assert_eq!(m("site.org"), None);
        assert_eq!(m("badexample.com"), None);
        assert_eq!(m("example.com.evil.net"), None);
        assert_eq!(allow_entry_for(Vec::new(), "example.com".into()), None);
    }
}
