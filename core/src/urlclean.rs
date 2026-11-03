//! URL helpers: tracking-parameter stripping, address-bar input resolution,
//! origin comparison and HTTPS upgrade.

use crate::pages;
use url::Url;

/// Query parameters that only exist to track the user across sites.
/// Matched case-insensitively against the (raw) parameter name.
const TRACKING_PARAMS: &[&str] = &[
    "fbclid",
    "gclid",
    "gclsrc",
    "dclid",
    "gbraid",
    "wbraid",
    "msclkid",
    "yclid",
    "twclid",
    "ttclid",
    "igshid",
    "igsh",
    "li_fat_id",
    "mc_cid",
    "mc_eid",
    "mkt_tok",
    "_hsenc",
    "_hsmi",
    "__hssc",
    "__hstc",
    "__hsfp",
    "hsctatracking",
    "oly_anon_id",
    "oly_enc_id",
    "vero_id",
    "vero_conv",
    "_openstat",
    "wickedid",
    "rb_clickid",
    "s_cid",
    "_ga",
    "_gl",
    "ref_src",
    "ref_url",
    "srsltid",
    "si",
];

/// Parameter-name prefixes that are always tracking (`utm_source`, ...).
const TRACKING_PREFIXES: &[&str] = &["utm_", "pk_", "piwik_", "mtm_", "hmb_"];

/// Hosts where a generic-looking parameter is actually functional and must be
/// kept (`si` is used by some sites as a session id).
fn param_is_tracking(host: &str, name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower == "si" {
        // `si` is the share-tracking id on YouTube/Spotify links only.
        return host.ends_with("youtube.com") || host == "youtu.be" || host.ends_with("spotify.com");
    }
    TRACKING_PARAMS.contains(&lower.as_str()) || TRACKING_PREFIXES.iter().any(|p| lower.starts_with(p))
}

/// Removes known tracking parameters from `url`.
///
/// Returns `Some(cleaned)` only if something was removed, so the caller can
/// cheaply decide whether a navigation must be redirected. The remaining
/// parameters keep their original order and encoding.
pub fn strip_tracking_params(url: &str) -> Option<String> {
    let mut parsed = Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    let query = parsed.query()?;
    let mut removed = false;
    let kept: Vec<&str> = query
        .split('&')
        .filter(|pair| {
            if pair.is_empty() {
                return false;
            }
            let name = pair.split('=').next().unwrap_or("");
            let tracking = param_is_tracking(&host, name);
            removed |= tracking;
            !tracking
        })
        .collect();
    if !removed {
        return None;
    }
    if kept.is_empty() {
        parsed.set_query(None);
    } else {
        parsed.set_query(Some(&kept.join("&")));
    }
    Some(parsed.into())
}

/// Turns what the user typed into the address bar into a URL to load.
///
/// `search_template` contains `{searchTerms}`, which is replaced by the
/// form-encoded query when the input does not look like a URL.
pub fn input_to_url(input: &str, search_template: &str) -> String {
    let input = input.trim();
    if input.is_empty() {
        return pages::Page::NewTab.url();
    }
    if let Some(page) = pages::Page::from_display(input) {
        return page.url();
    }
    if !input.contains(char::is_whitespace) {
        if let Some(url) = as_url(input) {
            return url;
        }
    }
    search_url(input, search_template)
}

/// Builds a search URL from a template.
pub fn search_url(query: &str, search_template: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    search_template.replace("{searchTerms}", &encoded)
}

fn as_url(input: &str) -> Option<String> {
    // Explicit scheme.
    if let Ok(u) = Url::parse(input) {
        match u.scheme() {
            "http" | "https" | "file" => return Some(u.into()),
            "about" if input.eq_ignore_ascii_case("about:blank") => return Some(u.into()),
            // "localhost:8080" parses as scheme "localhost"; fall through.
            _ => {}
        }
    }
    let candidate = format!("http://{input}");
    let u = Url::parse(&candidate).ok()?;
    let host = u.host()?;
    let looks_like_host = match host {
        url::Host::Ipv4(_) | url::Host::Ipv6(_) => true,
        url::Host::Domain(d) => {
            d == "localhost"
                || (d.contains('.')
                    && !d.starts_with('.')
                    && !d.ends_with('.')
                    && d.rsplit('.')
                        .next()
                        .is_some_and(|tld| tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic())))
        }
    };
    if !looks_like_host {
        return None;
    }
    // Bare hosts default to HTTPS; local addresses keep HTTP.
    let local = matches!(u.host(), Some(url::Host::Domain("localhost")))
        || matches!(u.host(), Some(url::Host::Ipv4(ip)) if ip.is_private() || ip.is_loopback());
    if local {
        Some(u.into())
    } else {
        let mut u = u;
        u.set_scheme("https").ok()?;
        Some(u.into())
    }
}

/// The ASCII serialization of a URL's origin (`scheme://host[:port]`), or
/// `None` for opaque origins (data:, about:, file:, invalid URLs).
pub fn origin_of(url: &str) -> Option<String> {
    let u = Url::parse(url).ok()?;
    match u.origin() {
        o @ url::Origin::Tuple(..) => Some(o.ascii_serialization()),
        url::Origin::Opaque(_) => None,
    }
}

/// Exact origin match: same scheme, host and (effective) port.
/// Opaque origins never match anything, including themselves.
pub fn same_origin(a: &str, b: &str) -> bool {
    match (origin_of(a), origin_of(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// Lower-cased host of a URL, used for the per-site allowlist and shield.
pub fn host_of(url: &str) -> Option<String> {
    Url::parse(url).ok()?.host_str().map(|h| h.to_ascii_lowercase())
}

/// For HTTPS-only mode: the `https://` version of a plain `http://` URL.
/// Loopback/private addresses are left alone.
pub fn https_upgrade(url: &str) -> Option<String> {
    let mut u = Url::parse(url).ok()?;
    if u.scheme() != "http" {
        return None;
    }
    match u.host()? {
        url::Host::Domain(d) if d == "localhost" || d.ends_with(".localhost") => return None,
        url::Host::Ipv4(ip) if ip.is_loopback() || ip.is_private() || ip.is_link_local() => return None,
        url::Host::Ipv6(ip) if ip.is_loopback() => return None,
        _ => {}
    }
    if u.port() == Some(80) {
        u.set_port(None).ok()?;
    }
    u.set_scheme("https").ok()?;
    Some(u.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DDG: &str = "https://duckduckgo.com/?q={searchTerms}";

    #[test]
    fn strips_utm_and_click_ids() {
        assert_eq!(
            strip_tracking_params("https://example.com/a?utm_source=x&id=5&utm_medium=y&fbclid=abc").as_deref(),
            Some("https://example.com/a?id=5")
        );
        assert_eq!(
            strip_tracking_params("https://shop.example/p?gclid=1&msclkid=2&UTM_Campaign=z").as_deref(),
            Some("https://shop.example/p")
        );
    }

    #[test]
    fn keeps_order_encoding_and_fragment() {
        assert_eq!(
            strip_tracking_params("https://e.com/?b=2&utm_x=1&a=%20x#frag").as_deref(),
            Some("https://e.com/?b=2&a=%20x#frag")
        );
    }

    #[test]
    fn untouched_urls_return_none() {
        assert_eq!(strip_tracking_params("https://example.com/?q=rust&page=2"), None);
        assert_eq!(strip_tracking_params("https://example.com/"), None);
        assert_eq!(strip_tracking_params("ftp://e.com/?utm_source=1"), None);
        assert_eq!(strip_tracking_params("not a url"), None);
    }

    #[test]
    fn si_only_stripped_on_share_hosts() {
        assert_eq!(strip_tracking_params("https://youtu.be/abc?si=XYZ").as_deref(), Some("https://youtu.be/abc"));
        assert_eq!(strip_tracking_params("https://example.com/?si=session"), None);
    }

    #[test]
    fn address_bar_resolution() {
        assert_eq!(input_to_url("example.com", DDG), "https://example.com/");
        assert_eq!(input_to_url("  https://rust-lang.org/learn ", DDG), "https://rust-lang.org/learn");
        assert_eq!(input_to_url("localhost:8080/x", DDG), "http://localhost:8080/x");
        assert_eq!(input_to_url("192.168.1.1", DDG), "http://192.168.1.1/");
        assert_eq!(input_to_url("rust borrow checker", DDG), "https://duckduckgo.com/?q=rust+borrow+checker");
        assert_eq!(input_to_url("hello", DDG), "https://duckduckgo.com/?q=hello");
        assert_eq!(input_to_url("c++", DDG), "https://duckduckgo.com/?q=c%2B%2B");
        assert_eq!(input_to_url("version 1.2", DDG), "https://duckduckgo.com/?q=version+1.2");
        assert_eq!(input_to_url("lb://settings", DDG), pages::Page::Settings.url());
        assert_eq!(input_to_url("", DDG), pages::Page::NewTab.url());
    }

    #[test]
    fn javascript_urls_are_searched_not_run() {
        let u = input_to_url("javascript:alert(1)", DDG);
        assert!(u.starts_with("https://duckduckgo.com/?q="), "{u}");
    }

    #[test]
    fn origins() {
        assert!(same_origin("https://a.com/login", "https://a.com:443/x?y"));
        assert!(!same_origin("https://a.com/", "http://a.com/"));
        assert!(!same_origin("https://a.com/", "https://a.com:8443/"));
        assert!(!same_origin("https://a.com/", "https://sub.a.com/"));
        assert!(!same_origin("data:text/html,x", "data:text/html,x"));
        assert_eq!(origin_of("https://A.com:443/p").as_deref(), Some("https://a.com"));
    }

    #[test]
    fn https_upgrades() {
        assert_eq!(https_upgrade("http://example.com/x").as_deref(), Some("https://example.com/x"));
        assert_eq!(https_upgrade("http://example.com:80/").as_deref(), Some("https://example.com/"));
        assert_eq!(https_upgrade("https://example.com/"), None);
        assert_eq!(https_upgrade("http://localhost:3000/"), None);
        assert_eq!(https_upgrade("http://10.0.0.1/"), None);
    }
}
