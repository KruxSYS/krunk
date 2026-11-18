//! "Is this login already saved?" while the vault is locked (0.25.3).
//!
//! Krunk asks to save a login only when it is new or its password changed. While the vault is
//! unlocked the vault itself answers that. While it is locked (at every start, after
//! inactivity, a Windows lock or sleep) it can't, and Krunk used to ask on every sign-in.
//!
//! Two memory-only helpers fill the gap (director decision, 2026-11-18). By design nothing here
//! is ever written to disk: a verifier on disk would let anyone running as the Windows user test
//! password guesses offline, without the master password's key derivation.
//! - [`Verifiers`]: HMAC-SHA-256 tags of (site, username, password) under a key made at random
//!   at each process start. They are filled while the vault is unlocked (at unlock and on each
//!   save) and survive auto-lock; they go with the process.
//! - [`PendingLogins`]: before the first unlock of a session Krunk knows nothing yet. A login
//!   sent on a site that already has saved logins is then held here (at most one per site and
//!   ten in all, for 30 minutes) and decided silently at the next unlock: dropped when it is
//!   unchanged, offered as an update when it changed.

use crate::vault::registrable_domain;
use hmac::{Hmac, KeyInit, Mac};
use std::collections::HashSet;
use zeroize::Zeroizing;

/// Pending logins kept at most, and per site.
pub const MAX_PENDING: usize = 10;
/// A pending login is dropped (without a prompt) after this long.
pub const PENDING_MS: i64 = 30 * 60 * 1000;

/// The site a login belongs to, as the vault matches logins to pages (`vault::match_kind`):
/// https origins of one registrable domain are one site; anything else is its exact origin.
pub fn site_key(origin: &str) -> Option<String> {
    let o = crate::urlclean::origin_of(origin)?;
    if o.starts_with("https://") {
        if let Some(d) = registrable_domain(&o) {
            return Some(format!("https://{d}"));
        }
    }
    (o.starts_with("https://") || o.starts_with("http://")).then_some(o)
}

/// A username as it is compared: trimmed, lower case. (Sites differ on case; for "is this the
/// login I already have" a case difference is the same login.)
pub fn norm_user(u: &str) -> String {
    u.trim().to_lowercase()
}

/// The memory-only verifier set.
pub struct Verifiers {
    key: Zeroizing<[u8; 32]>,
    tags: HashSet<[u8; 32]>,
}

impl Default for Verifiers {
    fn default() -> Self {
        Self::new()
    }
}

impl Verifiers {
    /// A fresh key from the system's randomness (never derived from the vault's).
    pub fn new() -> Verifiers {
        let mut key = [0u8; 32];
        getrandom::fill(&mut key).expect("system randomness");
        Verifiers { key: Zeroizing::new(key), tags: HashSet::new() }
    }

    fn tag(&self, site: &str, user: Option<&str>, password: &str) -> [u8; 32] {
        let mut mac = Hmac::<sha2::Sha256>::new_from_slice(&*self.key).expect("any key length");
        // Lengths first: no two different triples give the same input.
        for part in [site, user.unwrap_or("\u{0}any"), password] {
            mac.update(&(part.len() as u64).to_le_bytes());
            mac.update(part.as_bytes());
        }
        mac.finalize().into_bytes().into()
    }

    /// A login that is in the vault (`origin` as saved).
    pub fn learn(&mut self, origin: &str, username: &str, password: &str) {
        let Some(site) = site_key(origin) else { return };
        let u = norm_user(username);
        let a = self.tag(&site, Some(&u), password);
        let b = self.tag(&site, None, password);
        self.tags.insert(a);
        self.tags.insert(b);
    }

    /// Forgets everything (the vault was deleted or replaced, or the user locked it on purpose).
    pub fn clear(&mut self) {
        self.tags.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }

    /// Is this login (sent on `origin`) known unchanged? An empty username (a password step
    /// whose username Krunk didn't see) matches any saved login of the site with this password.
    pub fn knows(&self, origin: &str, username: &str, password: &str) -> bool {
        let Some(site) = site_key(origin) else { return false };
        let u = norm_user(username);
        let user = if u.is_empty() { None } else { Some(u.as_str()) };
        self.tags.contains(&self.tag(&site, user, password))
    }
}

/// Whether a submitted login is already saved unchanged, against the logins the vault has for
/// the page (`(username, password)` of every same-site entry).
pub fn unchanged<'a>(saved: impl IntoIterator<Item = (&'a str, &'a str)>, username: &str, password: &str) -> bool {
    let u = norm_user(username);
    saved.into_iter().any(|(su, sp)| sp == password && (u.is_empty() || norm_user(su) == u))
}

/// A login held until the vault is unlocked.
pub struct Pending {
    pub site: String,
    pub origin: String,
    pub tab: u32,
    pub username: String,
    pub password: Zeroizing<String>,
    pub at: i64,
}

/// Logins sent while the vault was locked and Krunk couldn't tell yet.
#[derive(Default)]
pub struct PendingLogins {
    items: Vec<Pending>,
}

impl PendingLogins {
    /// Holds a login (the newest one per site replaces an older one; the oldest goes beyond
    /// [`MAX_PENDING`]).
    pub fn hold(&mut self, origin: &str, tab: u32, username: &str, password: Zeroizing<String>, now: i64) {
        let Some(site) = site_key(origin) else { return };
        self.expire(now);
        self.items.retain(|p| p.site != site);
        if self.items.len() >= MAX_PENDING {
            self.items.remove(0);
        }
        self.items.push(Pending {
            site,
            origin: origin.to_string(),
            tab,
            username: username.to_string(),
            password,
            at: now,
        });
    }

    /// Drops the logins older than [`PENDING_MS`].
    pub fn expire(&mut self, now: i64) {
        self.items.retain(|p| now - p.at < PENDING_MS);
    }

    /// Everything still pending (the vault was just unlocked), oldest first.
    pub fn take_all(&mut self, now: i64) -> Vec<Pending> {
        self.expire(now);
        std::mem::take(&mut self.items)
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Changed logins decided at unlock, waiting for the user on their site (the address bar's key
/// shows them): at most one per site, gone after [`PENDING_MS`] from when it was sent.
#[derive(Default)]
pub struct Offers {
    items: Vec<Pending>,
}

impl Offers {
    pub fn add(&mut self, p: Pending, now: i64) {
        self.items.retain(|o| o.site != p.site && now - o.at < PENDING_MS);
        if self.items.len() >= MAX_PENDING {
            self.items.remove(0);
        }
        self.items.push(p);
    }

    /// Is there an offer for the page at `url`?
    pub fn has(&self, url: &str, now: i64) -> bool {
        let Some(site) = site_key(url) else { return false };
        self.items.iter().any(|o| o.site == site && now - o.at < PENDING_MS)
    }

    /// Takes the offer for the page at `url`.
    pub fn take(&mut self, url: &str, now: i64) -> Option<Pending> {
        let site = site_key(url)?;
        self.items.retain(|o| now - o.at < PENDING_MS);
        let i = self.items.iter().position(|o| o.site == site)?;
        Some(self.items.remove(i))
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pw(s: &str) -> Zeroizing<String> {
        Zeroizing::new(s.to_string())
    }

    #[test]
    fn sites_match_like_the_vault() {
        assert_eq!(site_key("https://login.bank.example/x").as_deref(), Some("https://bank.example"));
        assert_eq!(site_key("https://www.bank.example").as_deref(), Some("https://bank.example"));
        assert_eq!(site_key("http://bank.example").as_deref(), Some("http://bank.example"), "http: exact only");
        assert_eq!(site_key("http://127.0.0.1:8094/a").as_deref(), Some("http://127.0.0.1:8094"));
        assert_eq!(site_key("krunk://passwords"), None);
    }

    #[test]
    fn verifiers() {
        let mut v = Verifiers::new();
        assert!(v.is_empty() && !v.knows("https://a.example", "me", "pw"));
        v.learn("https://accounts.a.example", "Me@Mail.example", "s3cret");
        assert!(v.knows("https://www.a.example", " me@mail.example ", "s3cret"), "same site, case and spaces");
        assert!(!v.knows("https://a.example", "me@mail.example", "s3cret2"), "changed password");
        assert!(!v.knows("https://a.example", "other", "s3cret"), "another user");
        assert!(v.knows("https://a.example", "", "s3cret"), "password step without the username");
        assert!(!v.knows("https://b.example", "me@mail.example", "s3cret"), "another site");
        assert!(!v.knows("http://a.example", "me@mail.example", "s3cret"), "http is another site");
        // A new process (a new key) knows nothing: the tags don't carry over.
        let w = Verifiers::new();
        assert!(!w.knows("https://a.example", "me@mail.example", "s3cret"));
        assert_ne!(v.tag("s", Some("u"), "p"), w.tag("s", Some("u"), "p"));
        // No ambiguity between the parts.
        assert_ne!(v.tag("ab", Some("c"), "d"), v.tag("a", Some("bc"), "d"));
        v.clear();
        assert!(!v.knows("https://a.example", "me@mail.example", "s3cret"));
    }

    #[test]
    fn unchanged_against_saved() {
        let saved = [("Me", "one"), ("other", "two")];
        assert!(unchanged(saved, " me ", "one"));
        assert!(!unchanged(saved, "me", "two"));
        assert!(unchanged(saved, "", "two"), "no username: any saved login with that password");
        assert!(!unchanged(saved, "", "three"));
        assert!(!unchanged(saved, "new", "one"));
    }

    #[test]
    fn pending_queue() {
        let mut q = PendingLogins::default();
        q.hold("https://a.example", 1, "me", pw("1"), 0);
        q.hold("https://www.a.example", 2, "me", pw("2"), 10);
        assert_eq!(q.len(), 1, "one per site, the newest");
        for i in 0..12 {
            q.hold(&format!("https://s{i}.example"), 1, "u", pw("p"), 20 + i);
        }
        assert_eq!(q.len(), MAX_PENDING);
        let all = q.take_all(100);
        assert_eq!(all.len(), MAX_PENDING);
        assert_eq!(all.last().unwrap().site, "https://s11.example");
        assert!(all.iter().all(|p| p.site != "https://a.example"), "the oldest went");
        assert!(q.is_empty());
        q.hold("https://a.example", 1, "me", pw("1"), 0);
        assert!(q.take_all(PENDING_MS).is_empty(), "expired after 30 minutes");
        q.hold("krunk://x", 1, "me", pw("1"), 0);
        assert!(q.is_empty(), "not a web page");
    }

    #[test]
    fn offers() {
        let mut o = Offers::default();
        let p = Pending {
            site: "https://a.example".into(),
            origin: "https://a.example".into(),
            tab: 1,
            username: "me".into(),
            password: pw("new"),
            at: 0,
        };
        o.add(p, 0);
        assert!(o.has("https://login.a.example/signin", 5) && !o.has("https://b.example", 5));
        assert!(!o.has("https://a.example", PENDING_MS), "expired");
        let got = o.take("https://a.example/x", 5).unwrap();
        assert_eq!(&*got.password, "new");
        assert!(!o.has("https://a.example", 5));
    }
}
