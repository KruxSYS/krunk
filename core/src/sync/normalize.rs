//! URL normalisation for history record keys (PROTOCOL §9.6). Desktop and Android must
//! produce identical bytes, or the same page gets two records; the test vectors pin it.

use url::Url;

/// The natural key of a history entry, or `None` for URLs that aren't synced.
///
/// 1. Parse as a WHATWG URL (the `url` crate: IDNA UTS #46 non-transitional, punycode,
///    lowercase scheme and host, default ports removed, percent-encoding normalised).
/// 2. http and https only.
/// 3. Drop the fragment and userinfo.
/// 4. Remove tracking parameters (`urlclean`'s list).
/// 5. Serialise.
pub fn history_key(input: &str) -> Option<String> {
    let mut u = Url::parse(input.trim()).ok()?;
    if !matches!(u.scheme(), "http" | "https") || u.host_str().is_none() {
        return None;
    }
    u.set_fragment(None);
    let _ = u.set_username("");
    let _ = u.set_password(None);
    let s: String = u.into();
    let s = crate::urlclean::strip_tracking_params(&s).unwrap_or(s);
    // Removing every parameter can leave a bare '?' on some inputs; normalise that too.
    let mut u = Url::parse(&s).ok()?;
    if u.query() == Some("") {
        u.set_query(None);
    }
    Some(u.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn examples() {
        let cases = [
            ("HTTPS://Example.COM:443/a/b?x=1#frag", Some("https://example.com/a/b?x=1")),
            ("http://user:pw@example.com:80/", Some("http://example.com/")),
            ("https://example.com/?utm_source=x&id=5&fbclid=y", Some("https://example.com/?id=5")),
            ("https://example.com/?utm_source=x", Some("https://example.com/")),
            ("https://bücher.example/straße", Some("https://xn--bcher-kva.example/stra%C3%9Fe")),
            ("ftp://example.com/", None),
            ("about:blank", None),
            ("not a url", None),
        ];
        for (input, want) in cases {
            assert_eq!(history_key(input).as_deref(), want, "{input}");
        }
    }
}
