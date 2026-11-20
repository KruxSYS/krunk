//! 0.26.0: the hosts the engine must never preconnect to or pre-resolve.
//!
//! Preconnects and DNS pre-resolves (a page's `<link rel=preconnect|dns-prefetch>`, Early Hints,
//! the predictor) open DNS, TCP and TLS to a host without any request, so the ad blocker never
//! sees them. The engine (patches/chromium-preconnect-blocked-hosts.patch) skips the hosts in
//! this set and their subdomains. It is made only from rules that block a whole host for every
//! page: `||host^` with nothing after it. A host that any `@@` exception touches (the host itself,
//! a parent or a subdomain) is left out. Being too lenient here only allows a connection the
//! lists might have blocked; being too strict could break a connection they allow.

use std::collections::HashSet;

/// The host of a `||host...` rule body, if it is a plain host name.
fn plain_host(rest: &str) -> Option<&str> {
    let end = rest.find(['^', '/', '$', '*', '|']).unwrap_or(rest.len());
    let host = &rest[..end];
    let ok = !host.is_empty()
        && host.len() <= 253
        && host.contains('.')
        && !host.starts_with('.')
        && host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
    ok.then_some(host)
}

/// The blocked-host set from the texts of the active filter lists (sorted, lowercase).
pub fn blocked_hosts<'a>(texts: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut blocked = HashSet::new();
    let mut excepted = HashSet::new();
    for text in texts {
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("@@||") {
                if let Some(h) = plain_host(rest) {
                    excepted.insert(h.to_ascii_lowercase());
                }
            } else if let Some(rest) = line.strip_prefix("||") {
                // Exactly `||host^`: no path, no options (`$third-party`, `$domain=`...).
                if let Some(host) = rest.strip_suffix('^') {
                    if let Some(h) = plain_host(host) {
                        if h.len() == host.len() {
                            blocked.insert(h.to_ascii_lowercase());
                        }
                    }
                }
            }
        }
    }
    // Every suffix of every excepted host: a blocked host found here has an excepted subdomain
    // (or is excepted itself).
    let mut excepted_suffixes = HashSet::new();
    for e in &excepted {
        let mut h = e.as_str();
        loop {
            excepted_suffixes.insert(h.to_string());
            match h.find('.') {
                Some(i) => h = &h[i + 1..],
                None => break,
            }
        }
    }
    let parent_excepted = |b: &str| {
        let mut h = b;
        while let Some(i) = h.find('.') {
            h = &h[i + 1..];
            if excepted.contains(h) {
                return true;
            }
        }
        false
    };
    let mut out: Vec<String> =
        blocked.into_iter().filter(|b| !excepted_suffixes.contains(b) && !parent_excepted(b)).collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_host_rules_only() {
        let list = "! comment\n||tracker.example^\n||ads.example^$third-party\n||cdn.example/ads/^\n\
                    ||Pixel.Example^\n||x.example^$domain=a.com\n/banner/*\n||*.wild.example^\n||nodot^\n";
        assert_eq!(blocked_hosts([list]), vec!["pixel.example", "tracker.example"]);
    }

    #[test]
    fn exceptions_remove_hosts_their_parents_and_subdomains() {
        let a = "||t1.example^\n||t2.example^\n||sub.t3.example^\n||t4.example^\n||keep.example^";
        let b = "@@||t1.example^\n@@||cdn.t2.example^$script\n@@||t3.example/path\n@@||other.example^";
        assert_eq!(blocked_hosts([a, b]), vec!["keep.example", "t4.example"]);
    }

    #[test]
    fn empty_lists() {
        assert!(blocked_hosts(std::iter::empty::<&str>()).is_empty());
    }
}
