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
    if let Some((page, section)) = pages::Page::from_display_with_section(input) {
        return page.url_with_section(section.as_deref());
    }
    // `view-source:example.com` (0.11.0): Krunk's source viewer for that address.
    if let Some(rest) = input.get(..pages::VIEW_SOURCE.len()).filter(|p| p.eq_ignore_ascii_case(pages::VIEW_SOURCE)) {
        let target = input[rest.len()..].trim();
        if let Some(url) = as_url(target).filter(|u| u.starts_with("http://") || u.starts_with("https://")) {
            return pages::source_url(&url, None);
        }
    }
    if !input.contains(char::is_whitespace) {
        if let Some(url) = as_url(input) {
            return url;
        }
    }
    search_url(input, search_template)
}

/// The page context menu's search (0.11.0): the selected text, whitespace collapsed to single
/// spaces, at most 1,000 characters; `None` for an empty or blank selection.
pub fn selection_query(selection: &str) -> Option<String> {
    let q: String = selection.split_whitespace().collect::<Vec<_>>().join(" ");
    (!q.is_empty()).then(|| q.chars().take(1000).collect())
}

/// "Search Google for “krunk browser”", the query shortened to 32 characters with "…".
pub fn selection_label(engine: &str, query: &str) -> String {
    let shown: String = query.chars().take(32).collect();
    let cut = if query.chars().count() > 32 { "…" } else { "" };
    format!("Search {engine} for “{shown}{cut}”")
}

/// Builds a search URL from a template.
pub fn search_url(query: &str, search_template: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(query.as_bytes()).collect();
    search_template.replace("{searchTerms}", &encoded).replace("%s", &encoded)
}

/// Names that only exist on the local network (RFC 6761/6762/8375 and common router
/// defaults): typed without a scheme they are addresses although they are no public suffix.
const LOCAL_SUFFIXES: &[&str] = &[".local", ".lan", ".internal", ".home.arpa", ".localhost"];

/// An address typed without a scheme (0.15.0): an IP literal, `localhost`, a host with a
/// port, or a name ending in a real top-level domain (the public suffix list) or a
/// local-network suffix. `test.test`, `notes.txt` and `user@example.com` are searched.
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
    // `user@host` without a scheme is an e-mail address, not a sign-in (Chrome searches it too).
    if !u.username().is_empty() || u.password().is_some() {
        return None;
    }
    // The host and port as typed (the parser fills in `1.2` → `1.0.0.2` and drops `:80`).
    let typed_host = input.split(['/', '?', '#', '\\']).next().unwrap_or("");
    let has_port = !typed_host.ends_with(']')
        && typed_host.rsplit_once(':').is_some_and(|(_, p)| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    // (public: HTTPS by default; local names and addresses keep HTTP)
    let (looks_like_host, public) = match u.host()? {
        url::Host::Ipv4(ip) => {
            // Only the full dotted form: `1.5` or `3.14` are words.
            let quad = typed_host.split(':').next().unwrap_or("").trim_end_matches('.').split('.').count() == 4;
            (quad, !(ip.is_private() || ip.is_loopback() || ip.is_link_local()))
        }
        url::Host::Ipv6(ip) => {
            let seg = ip.segments()[0];
            let local = ip.is_loopback() || (seg & 0xfe00) == 0xfc00 || (seg & 0xffc0) == 0xfe80;
            (true, !local)
        }
        url::Host::Domain(d) => {
            let name = d.strip_suffix('.').unwrap_or(d);
            let labels_ok = !name.is_empty() && name.split('.').all(|l| !l.is_empty());
            let local = name == "localhost" || LOCAL_SUFFIXES.iter().any(|s| name.ends_with(s));
            // The parser lowercases and punycodes the name (`пример.рф` → `xn--e1afmkfd.xn--p1ai`).
            let known = labels_ok && name.contains('.') && psl::suffix(name.as_bytes()).is_some_and(|s| s.is_known());
            (labels_ok && (local || known || has_port), known && !local)
        }
    };
    if !looks_like_host {
        return None;
    }
    if public {
        let mut u = u;
        u.set_scheme("https").ok()?;
        Some(u.into())
    } else {
        Some(u.into())
    }
}

/// Removes `user:password@` from a URL before it is stored anywhere
/// (history, session). Other URLs are returned unchanged.
pub fn strip_credentials(url: &str) -> String {
    match Url::parse(url) {
        Ok(mut u) if !u.username().is_empty() || u.password().is_some() => {
            let _ = u.set_username("");
            let _ = u.set_password(None);
            u.into()
        }
        _ => url.to_string(),
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

/// Privacy rewrites applied to a top-level navigation in `NavigationStarting`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RewriteOptions {
    pub strip_tracking: bool,
    pub https_only: bool,
}

/// Result of [`navigation_rewrite`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewrite {
    /// The URL to navigate to instead.
    pub url: String,
    /// The navigation was upgraded to HTTPS (the host must show the
    /// HTTPS-only interstitial if it then fails).
    pub upgraded: bool,
}

/// Decides whether a top-level navigation must be cancelled and restarted
/// at another URL (tracking parameters removed, `http://` upgraded).
/// `http_exceptions` holds hosts the user allowed over HTTP this session.
///
/// The result is a fixed point: rewriting its URL again yields `None`, so
/// cancel-and-renavigate can never loop.
pub fn navigation_rewrite(
    uri: &str,
    opts: RewriteOptions,
    http_exceptions: &std::collections::HashSet<String>,
) -> Option<Rewrite> {
    let mut url = uri.to_string();
    let mut changed = false;
    let mut upgraded = false;
    if opts.strip_tracking {
        if let Some(clean) = strip_tracking_params(&url) {
            url = clean;
            changed = true;
        }
    }
    if opts.https_only {
        let excepted = host_of(&url).is_some_and(|h| http_exceptions.contains(&h));
        if !excepted {
            if let Some(https) = https_upgrade(&url) {
                url = https;
                changed = true;
                upgraded = true;
            }
        }
    }
    changed.then_some(Rewrite { url, upgraded })
}

/// Why a top-level navigation failed, from Chromium's net error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadFailure {
    /// Stopped by the user, by us (a rewrite) or replaced by another navigation.
    Cancelled,
    /// The host name does not exist.
    HostNotFound,
    /// A proxy or server asked for credentials.
    AuthRequired,
    /// TLS or certificate problem.
    Tls,
    /// Refused, reset, timed out, unreachable or offline.
    Connection,
    Other,
}

impl LoadFailure {
    /// Maps a Chromium `net::Error` (negative) code.
    pub fn from_net_error(code: i32) -> LoadFailure {
        match code {
            -3 => LoadFailure::Cancelled,
            -105 | -137 | -166 | -803 => LoadFailure::HostNotFound,
            -115 | -127 | -323 | -338 => LoadFailure::AuthRequired,
            // SSL_PROTOCOL_ERROR, version/cipher mismatch, client auth, pins,
            // unrecognized name, and the whole certificate range.
            -107 | -113 | -117 | -150 | -159 => LoadFailure::Tls,
            c if (-299..=-200).contains(&c) => LoadFailure::Tls,
            -7 | -102..=-100 | -104 | -106 | -109 | -118 | -324 => LoadFailure::Connection,
            _ => LoadFailure::Other,
        }
    }
}

/// After an HTTPS-only upgrade failed, the interstitial URL to show (with
/// the original `http://` address), or `None` to leave the error page.
/// Cancelled navigations, unknown hosts and authentication prompts are not
/// HTTPS problems. `failure` is `None` when the navigation succeeded.
pub fn https_only_fallback(upgraded_from: Option<&str>, failure: Option<LoadFailure>) -> Option<String> {
    let http = upgraded_from?;
    match failure? {
        LoadFailure::Cancelled | LoadFailure::HostNotFound | LoadFailure::AuthRequired => return None,
        LoadFailure::Tls | LoadFailure::Connection | LoadFailure::Other => {}
    }
    if !http.starts_with("http://") {
        return None;
    }
    Some(https_only_interstitial(http))
}

/// HTTPS-only: the server redirected an upgraded `https://` navigation back
/// to `http://`. Upgrading that again would loop forever, so the interstitial
/// for the `http://` target is shown instead. `upgrade_in_flight` is true
/// while the tab's current navigation is one of our upgrades.
pub fn https_only_downgrade(upgrade_in_flight: bool, redirected: bool, uri: &str) -> Option<String> {
    (upgrade_in_flight && redirected && uri.starts_with("http://")).then(|| https_only_interstitial(uri))
}

fn https_only_interstitial(http: &str) -> String {
    let enc: String = url::form_urlencoded::byte_serialize(http.as_bytes()).collect();
    format!("{}?url={enc}", pages::Page::HttpsOnly.url())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn searching_the_selection_and_view_source() {
        assert_eq!(selection_query("  krunk\n\t browser  "), Some("krunk browser".into()));
        assert_eq!(selection_query(" \n\t "), None);
        assert_eq!(selection_query(&"é".repeat(1500)).unwrap().chars().count(), 1000);
        assert_eq!(selection_label("Google", "krunk browser"), "Search Google for “krunk browser”");
        let long = "a".repeat(31) + "éxyz";
        assert_eq!(selection_label("Bing", &long), format!("Search Bing for “{}é…”", "a".repeat(31)));
        assert_eq!(
            search_url("rust & wasm", "https://duckduckgo.com/?q={searchTerms}"),
            "https://duckduckgo.com/?q=rust+%26+wasm"
        );
        // view-source: in the address bar opens Krunk's viewer.
        let v = input_to_url("view-source:example.com", "https://s/?q={searchTerms}");
        assert_eq!(crate::pages::source_target(&v), Some(("https://example.com/".into(), None)));
        let v = input_to_url("VIEW-SOURCE:https://a.example/x?y=1", "https://s/?q={searchTerms}");
        assert_eq!(crate::pages::source_target(&v).unwrap().0, "https://a.example/x?y=1");
        assert_eq!(crate::pages::display_url(&v), "view-source:https://a.example/x?y=1");
        // Not an address: a search for the words.
        assert!(input_to_url("view-source:not a url", "https://s/?q={searchTerms}").starts_with("https://s/?q="));
        assert!(crate::pages::source_target(&crate::pages::source_url("javascript:alert(1)", None)).is_none());
        assert_eq!(
            crate::pages::source_target(&crate::pages::source_url("https://a.example/", Some(7))).unwrap().1,
            Some(7)
        );
    }

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
        assert_eq!(input_to_url("krunk://settings", DDG), pages::Page::Settings.url());
        assert_eq!(input_to_url("", DDG), pages::Page::NewTab.url());
    }

    #[test]
    fn only_real_tlds_navigate() {
        let s = |q: &str| format!("https://duckduckgo.com/?q={q}");
        // 0.15.0: a made-up top-level domain is a search...
        assert_eq!(input_to_url("test.test", DDG), s("test.test"));
        assert_eq!(input_to_url("notes.txt", DDG), s("notes.txt"));
        assert_eq!(input_to_url("readme.md5", DDG), s("readme.md5"));
        assert_eq!(input_to_url("node.js", DDG), s("node.js"));
        assert_eq!(input_to_url("a.b", DDG), s("a.b"));
        assert_eq!(input_to_url("dev", DDG), s("dev"), "a bare TLD is a word");
        assert_eq!(input_to_url("test.test/a?b=1", DDG), s("test.test%2Fa%3Fb%3D1"));
        // ...unless asked for: a scheme, or a port.
        assert_eq!(input_to_url("http://test.test", DDG), "http://test.test/");
        assert_eq!(input_to_url("https://intranet/x", DDG), "https://intranet/x");
        assert_eq!(input_to_url("test.test:8080", DDG), "http://test.test:8080/");
        assert_eq!(input_to_url("myserver:3000/api", DDG), "http://myserver:3000/api");
        assert_eq!(input_to_url("localhost:3000", DDG), "http://localhost:3000/");
        assert_eq!(input_to_url("localhost", DDG), "http://localhost/");
        assert_eq!(input_to_url("app.localhost:5173", DDG), "http://app.localhost:5173/");
        // Real TLDs (ICANN and private suffixes), with paths, queries, fragments and ports.
        assert_eq!(input_to_url("example.com", DDG), "https://example.com/");
        assert_eq!(input_to_url("foo.dev", DDG), "https://foo.dev/");
        assert_eq!(input_to_url("hello.world", DDG), "https://hello.world/", ".world is a gTLD");
        assert_eq!(input_to_url("bbc.co.uk/news", DDG), "https://bbc.co.uk/news");
        assert_eq!(input_to_url("someone.github.io", DDG), "https://someone.github.io/");
        assert_eq!(input_to_url("example.com/a?b", DDG), "https://example.com/a?b");
        assert_eq!(input_to_url("example.com/a?b=c d", DDG), s("example.com%2Fa%3Fb%3Dc+d"), "a space: words");
        assert_eq!(input_to_url("example.com#top", DDG), "https://example.com/#top");
        assert_eq!(input_to_url("Example.COM:8443/X", DDG), "https://example.com:8443/X");
        assert_eq!(input_to_url("mastodon.social/@krunk", DDG), "https://mastodon.social/@krunk");
        // A trailing dot (fully qualified) keeps working; empty labels do not.
        assert_eq!(input_to_url("example.com.", DDG), "https://example.com./");
        assert_eq!(input_to_url("test.test.", DDG), s("test.test."));
        assert_eq!(input_to_url("a..com", DDG), s("a..com"));
        assert_eq!(input_to_url(".com", DDG), s(".com"));
        // IDN: Unicode and punycode TLDs.
        assert_eq!(input_to_url("пример.рф", DDG), "https://xn--e1afmkfd.xn--p1ai/");
        assert_eq!(input_to_url("xn--e1afmkfd.xn--p1ai", DDG), "https://xn--e1afmkfd.xn--p1ai/");
        assert_eq!(input_to_url("bücher.de", DDG), "https://xn--bcher-kva.de/");
        assert!(input_to_url("пример.тест", DDG).starts_with("https://duckduckgo.com/?q="));
        // Local-network names stay on HTTP.
        assert_eq!(input_to_url("nas.local", DDG), "http://nas.local/");
        assert_eq!(input_to_url("router.lan/admin", DDG), "http://router.lan/admin");
        assert_eq!(input_to_url("grafana.internal", DDG), "http://grafana.internal/");
        assert_eq!(input_to_url("printer.home.arpa", DDG), "http://printer.home.arpa/");
        // IP literals: the full dotted form only; private and loopback stay on HTTP.
        assert_eq!(input_to_url("192.168.1.1", DDG), "http://192.168.1.1/");
        assert_eq!(input_to_url("127.0.0.1:8080/x", DDG), "http://127.0.0.1:8080/x");
        assert_eq!(input_to_url("169.254.1.1", DDG), "http://169.254.1.1/");
        assert_eq!(input_to_url("8.8.8.8", DDG), "https://8.8.8.8/");
        assert_eq!(input_to_url("3.14", DDG), s("3.14"));
        assert_eq!(input_to_url("1.2.3", DDG), s("1.2.3"));
        assert_eq!(input_to_url("[::1]:8080", DDG), "http://[::1]:8080/");
        assert_eq!(input_to_url("[fd00::1]", DDG), "http://[fd00::1]/");
        assert_eq!(input_to_url("[2001:db8::1]", DDG), "https://[2001:db8::1]/");
        // user@host without a scheme is an e-mail address: searched (with a scheme it navigates).
        assert_eq!(input_to_url("me@example.com", DDG), s("me%40example.com"));
        assert_eq!(input_to_url("user:pw@localhost:8080", DDG), s("user%3Apw%40localhost%3A8080"));
        assert_eq!(input_to_url("https://me@example.com/", DDG), "https://me@example.com/");
        // Not ports.
        assert_eq!(input_to_url("note:todo", DDG), s("note%3Atodo"));
        assert_eq!(input_to_url("example.com:abc", DDG), s("example.com%3Aabc"));
    }

    #[test]
    fn percent_s_templates() {
        assert_eq!(search_url("a b", "https://s.example/search?q=%s&src=lb"), "https://s.example/search?q=a+b&src=lb");
        assert_eq!(input_to_url("100% sure", "https://s.example/?q=%s"), "https://s.example/?q=100%25+sure");
    }

    #[test]
    fn javascript_urls_are_searched_not_run() {
        let u = input_to_url("javascript:alert(1)", DDG);
        assert!(u.starts_with("https://duckduckgo.com/?q="), "{u}");
    }

    #[test]
    fn credentials_are_stripped() {
        assert_eq!(strip_credentials("https://alice:hunter2@example.com/x"), "https://example.com/x");
        assert_eq!(strip_credentials("ftp://bob@files.example/"), "ftp://files.example/");
        assert_eq!(strip_credentials("https://example.com/?q=a:b@c"), "https://example.com/?q=a:b@c");
        assert_eq!(strip_credentials("not a url"), "not a url");
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
    fn navigation_rewrites_are_fixed_points() {
        use std::collections::HashSet;
        let both = RewriteOptions { strip_tracking: true, https_only: true };
        let none = HashSet::new();
        let r = navigation_rewrite("http://shop.example/p?utm_source=x&id=3", both, &none).unwrap();
        assert_eq!(r, Rewrite { url: "https://shop.example/p?id=3".into(), upgraded: true });
        assert_eq!(navigation_rewrite(&r.url, both, &none), None, "no loop");
        for u in [
            "https://a.example/?fbclid=1&gclid=2&utm_campaign=3",
            "http://a.example/?q=1",
            "http://localhost:8080/?utm_source=x",
            "https://youtu.be/x?si=abc&t=10",
            "http://10.0.0.1/",
            "https://clean.example/",
        ] {
            if let Some(first) = navigation_rewrite(u, both, &none) {
                assert_eq!(navigation_rewrite(&first.url, both, &none), None, "{u} -> {}", first.url);
            }
        }
        // Options off: nothing happens.
        let off = RewriteOptions { strip_tracking: false, https_only: false };
        assert_eq!(navigation_rewrite("http://a.example/?utm_source=x", off, &none), None);
        // Only stripping: no upgrade flag.
        let strip = RewriteOptions { strip_tracking: true, https_only: false };
        assert_eq!(
            navigation_rewrite("http://a.example/?utm_source=x", strip, &none),
            Some(Rewrite { url: "http://a.example/".into(), upgraded: false })
        );
    }

    #[test]
    fn https_only_session_exceptions() {
        use std::collections::HashSet;
        let opts = RewriteOptions { strip_tracking: false, https_only: true };
        let mut ex = HashSet::new();
        assert!(navigation_rewrite("http://old.example/page", opts, &ex).unwrap().upgraded);
        ex.insert("old.example".to_string());
        assert_eq!(navigation_rewrite("http://old.example/page", opts, &ex), None, "user chose HTTP once");
        assert!(navigation_rewrite("http://other.example/", opts, &ex).is_some(), "exception is per host");
        assert!(navigation_rewrite("http://sub.old.example/", opts, &ex).is_some(), "not for subdomains");
        // Private/loopback hosts are never upgraded.
        assert_eq!(navigation_rewrite("http://192.168.0.1/", opts, &HashSet::new()), None);
    }

    #[test]
    fn https_only_interstitial_decision() {
        let http = Some("http://old.example/a?b=1");
        let fail = |code| Some(LoadFailure::from_net_error(code));
        // ERR_CONNECTION_REFUSED: nothing listens on port 443.
        let page = https_only_fallback(http, fail(-102)).unwrap();
        assert!(page.starts_with("https://krunk.invalid/https-only.html?url=http%3A%2F%2Fold.example%2Fa%3Fb%3D1"));
        assert_eq!(pages::Page::from_url(&page), Some(pages::Page::HttpsOnly));
        // Certificate errors (name, date, authority), SSL protocol, timeouts, resets.
        for code in [-200, -201, -202, -107, -113, -118, -101, -324, -999] {
            assert!(https_only_fallback(http, fail(code)).is_some(), "{code}");
        }
        assert_eq!(https_only_fallback(http, None), None, "upgrade worked");
        assert_eq!(https_only_fallback(None, fail(-102)), None, "was not upgraded");
        assert_eq!(https_only_fallback(http, fail(-3)), None, "cancelled");
        assert_eq!(https_only_fallback(http, fail(-105)), None, "host does not exist at all");
        assert_eq!(https_only_fallback(http, fail(-127)), None, "proxy wants credentials");
        assert_eq!(https_only_fallback(Some("https://x/"), fail(-102)), None);
    }

    #[test]
    fn net_error_classes() {
        use LoadFailure as F;
        let cases = [
            (-3, F::Cancelled),
            (-105, F::HostNotFound),
            (-137, F::HostNotFound),
            (-803, F::HostNotFound),
            (-115, F::AuthRequired),
            (-338, F::AuthRequired),
            (-107, F::Tls),
            (-200, F::Tls),
            (-202, F::Tls),
            (-217, F::Tls),
            (-7, F::Connection),
            (-100, F::Connection),
            (-102, F::Connection),
            (-106, F::Connection),
            (-118, F::Connection),
            (-324, F::Connection),
            (-2, F::Other),
            (-20, F::Other),
            (-311, F::Other),
        ];
        for (code, want) in cases {
            assert_eq!(LoadFailure::from_net_error(code), want, "{code}");
        }
    }

    #[test]
    fn https_only_redirect_back_to_http_shows_interstitial() {
        // http.badssl.com: https:// answers 301 -> http://. Upgrading again would loop.
        let page = https_only_downgrade(true, true, "http://http.badssl.com/").unwrap();
        assert!(page.starts_with(&pages::Page::HttpsOnly.url()));
        assert!(page.ends_with("?url=http%3A%2F%2Fhttp.badssl.com%2F"), "{page}");
        assert_eq!(https_only_downgrade(false, true, "http://a.example/"), None, "no upgrade in flight");
        assert_eq!(https_only_downgrade(true, false, "http://a.example/"), None, "user navigation, not a redirect");
        assert_eq!(https_only_downgrade(true, true, "https://a.example/"), None, "redirect stays on https");
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
