//! Certificate exceptions: "Continue anyway" past a bad TLS certificate on the
//! `krunk://cert-error` interstitial.
//!
//! An exception is bound to the site (host and port) and to the exact leaf certificate
//! (its SHA-256): a site that later shows another certificate gets the interstitial again,
//! so a remembered self-signed router page does not also wave through an attacker's
//! certificate for the same name. "Remember for this site" keeps it in `browser.db`
//! (`kv["cert_exceptions"]`, a JSON list, never synced); otherwise the host keeps it in
//! memory for the session ([`SessionExceptions`]). Private tabs never persist one.

use crate::pages::Page;
use crate::storage::{DbError, DbResult, Store};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use url::Url;

/// The `kv` row that holds the remembered exceptions.
pub const KV_KEY: &str = "cert_exceptions";
/// Most remembered exceptions; adding one more drops the oldest.
pub const MAX_ENTRIES: usize = 500;

/// A remembered exception.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CertException {
    /// Lower case, as `url::Url` prints it (IPv6 in brackets).
    pub host: String,
    pub port: u16,
    /// The leaf certificate's SHA-256 of its DER, 64 lower-case hex digits.
    pub sha256: String,
    /// When it was added (Unix ms).
    pub added: i64,
}

impl CertException {
    fn key(&self) -> (&str, u16, &str) {
        (&self.host, self.port, &self.sha256)
    }
}

/// The site of an `https://` URL: its host (lower case) and port (443 when none is given).
/// Anything else has no certificate to make an exception for.
pub fn site_of(url: &str) -> Option<(String, u16)> {
    let u = Url::parse(url).ok()?;
    if u.scheme() != "https" {
        return None;
    }
    let host = u.host_str().filter(|h| !h.is_empty())?.to_ascii_lowercase();
    Some((host, u.port_or_known_default()?))
}

/// A host as the Settings page sends it back: only what [`site_of`] itself produces.
fn valid_host(host: &str, port: u16) -> bool {
    !host.is_empty()
        && host.len() <= 253 + 2
        && site_of(&format!("https://{host}:{port}/")).is_some_and(|(h, p)| h == host && p == port)
}

/// A fingerprint as stored: 64 lower-case hex digits.
pub fn valid_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The SHA-256 of a certificate's DER, lower-case hex.
pub fn sha256_hex(der: &[u8]) -> String {
    Sha256::digest(der).iter().map(|b| format!("{b:02x}")).collect()
}

/// The remembered exceptions. A missing or unreadable row is an empty list, entries that
/// don't validate are skipped and unknown fields are ignored (a later version may add some).
pub fn load(store: &Store) -> DbResult<Vec<CertException>> {
    Ok(store.kv_get(KV_KEY)?.map(|j| parse(&j)).unwrap_or_default())
}

fn parse(json: &str) -> Vec<CertException> {
    let raw: Vec<serde_json::Value> = serde_json::from_str(json).unwrap_or_default();
    raw.into_iter()
        .filter_map(|v| serde_json::from_value::<CertException>(v).ok())
        .filter(|e| valid_host(&e.host, e.port) && valid_sha256(&e.sha256))
        .collect()
}

fn save(store: &Store, list: &[CertException]) -> DbResult<()> {
    let json = serde_json::to_string(list).map_err(|e| DbError::ToSqlConversionFailure(Box::new(e)))?;
    store.kv_set(KV_KEY, &json)
}

/// Whether the remembered list allows this certificate on this site.
pub fn allows(list: &[CertException], host: &str, port: u16, sha256: &str) -> bool {
    list.iter().any(|e| e.key() == (host, port, sha256))
}

/// The list for the Settings page: newest first.
pub fn list(store: &Store) -> DbResult<Vec<CertException>> {
    let mut l = load(store)?;
    l.sort_by(|a, b| b.added.cmp(&a.added).then_with(|| a.host.cmp(&b.host)));
    Ok(l)
}

/// Remembers the certificate `sha256` for the site of `url` (an `https://` address). Returns
/// `Ok(false)` for input that doesn't validate. Adding one again only refreshes its date; at
/// [`MAX_ENTRIES`] the oldest goes.
pub fn add(store: &Store, url: &str, sha256: &str, now: i64) -> DbResult<bool> {
    let (Some((host, port)), true) = (site_of(url), valid_sha256(sha256)) else { return Ok(false) };
    let mut l = load(store)?;
    l.retain(|e| e.key() != (host.as_str(), port, sha256));
    l.push(CertException { host, port, sha256: sha256.to_string(), added: now });
    if l.len() > MAX_ENTRIES {
        l.sort_by_key(|e| e.added);
        let extra = l.len() - MAX_ENTRIES;
        l.drain(..extra);
    }
    save(store, &l)?;
    Ok(true)
}

/// Forgets one exception. `Ok(false)`: malformed input or no such entry.
pub fn remove(store: &Store, host: &str, port: u16, sha256: &str) -> DbResult<bool> {
    if !valid_host(host, port) || !valid_sha256(sha256) {
        return Ok(false);
    }
    let mut l = load(store)?;
    let before = l.len();
    l.retain(|e| e.key() != (host, port, sha256));
    if l.len() == before {
        return Ok(false);
    }
    save(store, &l)?;
    Ok(true)
}

/// Exceptions for the rest of the session (without "Remember", and every one in private
/// tabs). Never written anywhere.
#[derive(Debug, Default)]
pub struct SessionExceptions(HashSet<(String, u16, String)>);

impl SessionExceptions {
    /// False for input that doesn't validate.
    pub fn add(&mut self, url: &str, sha256: &str) -> bool {
        let (Some((host, port)), true) = (site_of(url), valid_sha256(sha256)) else { return false };
        self.0.insert((host, port, sha256.to_string()));
        true
    }

    pub fn allows(&self, host: &str, port: u16, sha256: &str) -> bool {
        self.0.contains(&(host.to_string(), port, sha256.to_string()))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn remove(&mut self, host: &str, port: u16, sha256: &str) {
        self.0.remove(&(host.to_string(), port, sha256.to_string()));
    }
}

/// What a tab hit: kept by the host while the interstitial shows, so "Continue anyway" is
/// checked against the real error, never against what the page sends back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertProblem {
    pub url: String,
    pub host: String,
    pub port: u16,
    pub sha256: String,
    /// The engine's net error (`ERR_CERT_*`, -200…-299).
    pub error: i32,
    /// False when the engine allows no exception (HSTS, pinned keys).
    pub overridable: bool,
}

impl CertProblem {
    /// `None` for a URL that is not `https://` (the engine reports certificates for those only).
    pub fn new(url: &str, sha256: Option<String>, error: i32, overridable: bool) -> Option<CertProblem> {
        let (host, port) = site_of(url)?;
        let sha256 = sha256.filter(|s| valid_sha256(s)).unwrap_or_default();
        // Without a fingerprint there is nothing to bind an exception to.
        let overridable = overridable && !sha256.is_empty();
        Some(CertProblem { url: url.to_string(), host, port, sha256, error, overridable })
    }

    /// The interstitial for this problem. `private`: the page offers no "Remember".
    pub fn interstitial_url(&self, private: bool) -> String {
        let mut u = Url::parse(&Page::CertError.url()).expect("pages URL");
        u.query_pairs_mut()
            .append_pair("url", &self.url)
            .append_pair("err", &self.error.to_string())
            .append_pair("sha", &self.sha256)
            .append_pair("ov", if self.overridable { "1" } else { "0" })
            .append_pair("p", if private { "1" } else { "0" });
        u.into()
    }

    /// "Continue anyway" for `url` and `sha256` matches this problem.
    pub fn matches(&self, url: &str, sha256: &str) -> bool {
        self.overridable && self.url == url && self.sha256 == sha256
    }
}

/// The address an interstitial URL is about (`https://` only), for the address bar.
pub fn interstitial_target(url: &str) -> Option<String> {
    if Page::from_url(url) != Some(Page::CertError) {
        return None;
    }
    let u = Url::parse(url).ok()?;
    let target = u.query_pairs().find(|(k, _)| k == "url").map(|(_, v)| v.into_owned())?;
    site_of(&target).map(|_| target)
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn site_and_fingerprint_validation() {
        assert_eq!(site_of("https://Router.Lan/admin"), Some(("router.lan".into(), 443)));
        assert_eq!(site_of("https://10.0.0.1:8443/"), Some(("10.0.0.1".into(), 8443)));
        assert_eq!(site_of("https://[::1]:9/"), Some(("[::1]".into(), 9)));
        assert_eq!(site_of("http://router.lan/"), None);
        assert_eq!(site_of("not a url"), None);
        assert!(valid_sha256(A));
        assert!(!valid_sha256(&A.to_uppercase()));
        assert!(!valid_sha256(&A[1..]));
        assert!(!valid_sha256(&format!("{}g", &A[1..])));
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert!(valid_host("[::1]", 9) && valid_host("a.example", 443));
        assert!(!valid_host("A.example", 443) && !valid_host("a.example/x", 443) && !valid_host("", 443));
    }

    #[test]
    fn add_allows_remove() {
        let s = Store::open_in_memory().unwrap();
        assert!(load(&s).unwrap().is_empty());
        assert!(add(&s, "https://router.lan/admin?x", A, 5).unwrap());
        let l = load(&s).unwrap();
        assert!(allows(&l, "router.lan", 443, A), "port defaults to 443");
        assert!(!allows(&l, "router.lan", 443, B), "another certificate is not allowed");
        assert!(!allows(&l, "router.lan", 8443, A), "another port is another site");
        assert!(!allows(&l, "other.lan", 443, A));
        // Adding again refreshes the date, no duplicate.
        assert!(add(&s, "https://router.lan:443/", A, 9).unwrap());
        assert_eq!(
            load(&s).unwrap(),
            vec![CertException { host: "router.lan".into(), port: 443, sha256: A.into(), added: 9 }]
        );
        assert!(add(&s, "https://router.lan/", B, 10).unwrap());
        assert_eq!(list(&s).unwrap()[0].sha256, B, "newest first");
        assert!(remove(&s, "router.lan", 443, A).unwrap());
        assert!(!remove(&s, "router.lan", 443, A).unwrap(), "already gone");
        let l = load(&s).unwrap();
        assert!(!allows(&l, "router.lan", 443, A) && allows(&l, "router.lan", 443, B));
    }

    #[test]
    fn malformed_input_is_refused() {
        let s = Store::open_in_memory().unwrap();
        assert!(!add(&s, "http://router.lan/", A, 1).unwrap(), "http has no certificate");
        assert!(!add(&s, "https://router.lan/", "abc", 1).unwrap());
        assert!(!add(&s, "https://router.lan/", &A.to_uppercase(), 1).unwrap());
        assert!(!add(&s, "javascript:alert(1)", A, 1).unwrap());
        assert!(!remove(&s, "Router.lan", 443, A).unwrap());
        assert!(!remove(&s, "router.lan", 443, "x").unwrap());
        assert!(s.kv_get(KV_KEY).unwrap().is_none(), "nothing written");
    }

    #[test]
    fn capped_oldest_first() {
        let s = Store::open_in_memory().unwrap();
        let many: Vec<CertException> = (0..MAX_ENTRIES)
            .map(|i| CertException { host: format!("h{i}.example"), port: 443, sha256: A.into(), added: i as i64 + 10 })
            .collect();
        save(&s, &many).unwrap();
        assert!(add(&s, "https://new.example/", B, 100_000).unwrap());
        let l = load(&s).unwrap();
        assert_eq!(l.len(), MAX_ENTRIES);
        assert!(!allows(&l, "h0.example", 443, A), "the oldest went");
        assert!(allows(&l, "h1.example", 443, A) && allows(&l, "new.example", 443, B));
    }

    #[test]
    fn kv_json_round_trips_and_tolerates_unknown_fields() {
        let s = Store::open_in_memory().unwrap();
        add(&s, "https://a.example:8443/", A, 7).unwrap();
        let json = s.kv_get(KV_KEY).unwrap().unwrap();
        assert_eq!(json, format!(r#"[{{"host":"a.example","port":8443,"sha256":"{A}","added":7}}]"#));
        assert_eq!(parse(&json), load(&s).unwrap());
        // A later version's extra field, a broken entry and a bad fingerprint.
        let mixed = format!(
            r#"[{{"host":"a.example","port":443,"sha256":"{A}","added":1,"note":"x"}},{{"host":3}},{{"host":"b.example","port":443,"sha256":"XYZ","added":1}}]"#
        );
        s.kv_set(KV_KEY, &mixed).unwrap();
        let l = load(&s).unwrap();
        assert_eq!(l.len(), 1);
        assert!(allows(&l, "a.example", 443, A));
        s.kv_set(KV_KEY, "{broken").unwrap();
        assert!(load(&s).unwrap().is_empty());
    }

    #[test]
    fn session_exceptions() {
        let mut m = SessionExceptions::default();
        assert!(m.add("https://a.example/x", A));
        assert!(!m.add("http://a.example/", A) && !m.add("https://a.example/", "nope"));
        assert!(m.allows("a.example", 443, A) && !m.allows("a.example", 443, B));
        m.remove("a.example", 443, A);
        assert!(!m.allows("a.example", 443, A));
        m.add("https://a.example/", A);
        m.clear();
        assert!(!m.allows("a.example", 443, A) && m.is_empty());
    }

    #[test]
    fn problem_and_interstitial() {
        let p = CertProblem::new("https://self-signed.example/a?b=1&c", Some(A.into()), -202, true).unwrap();
        assert_eq!((p.host.as_str(), p.port), ("self-signed.example", 443));
        assert!(p.matches("https://self-signed.example/a?b=1&c", A));
        assert!(!p.matches("https://self-signed.example/a?b=1&c", B), "the page's fingerprint is not trusted");
        assert!(!p.matches("https://other.example/", A));
        let page = p.interstitial_url(false);
        assert_eq!(Page::from_url(&page), Some(Page::CertError));
        assert!(
            page.contains("err=-202")
                && page.contains(&format!("sha={A}"))
                && page.contains("ov=1")
                && page.contains("p=0")
        );
        assert_eq!(interstitial_target(&page).as_deref(), Some("https://self-signed.example/a?b=1&c"));
        assert_eq!(interstitial_target("https://krunk.invalid/cert-error.html?url=javascript:x"), None);
        assert_eq!(interstitial_target("https://krunk.invalid/crashed.html?url=https://a.example/"), None);
        // Not overridable: HSTS / pinned (no engine callback), or no certificate to bind to.
        let hsts = CertProblem::new("https://a.example/", Some(A.into()), -202, false).unwrap();
        assert!(!hsts.matches("https://a.example/", A) && hsts.interstitial_url(true).contains("ov=0"));
        let no_cert = CertProblem::new("https://a.example/", None, -202, true).unwrap();
        assert!(!no_cert.overridable);
        assert!(CertProblem::new("http://a.example/", Some(A.into()), -202, true).is_none());
    }
}
