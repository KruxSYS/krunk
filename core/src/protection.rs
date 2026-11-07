//! Tracking protection (Krunk 0.6.0): the Standard / Strict / Custom levels and what each
//! one switches on, the known-tracker hosts taken from the privacy filter lists (for the
//! third-party cookie policy), referrer trimming for cross-site requests, and the
//! bookkeeping of bounce trackers (sites only ever seen as a redirect hop). The engine
//! calls are in the app; everything here is pure. See `docs/TRACKING_PROTECTION.md`.

use crate::cookies::site_of;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

/// The bundled list ids the levels switch on and off.
pub const CRYPTOMINING_LIST: &str = "nocoin";
pub const FINGERPRINTING_LIST: &str = "krunk-fingerprinting";
/// The bundled lists that are not on the filter-lists page: the levels manage them.
pub const EXTRA_LISTS: &[&str] = &[CRYPTOMINING_LIST, FINGERPRINTING_LIST];
/// The lists whose network rules name known trackers (the cookie policy "Trackers").
pub const TRACKER_LISTS: &[&str] = &["easyprivacy", "ublock-privacy"];

/// How long a redirect hop counts (a site seen only as a hop in this window is cleaned).
pub const BOUNCE_WINDOW_MS: i64 = 7 * 24 * 3600 * 1000;
/// A hop younger than this is left alone: the redirect chain may still be in flight and
/// the user may still land on the site itself.
pub const BOUNCE_GRACE_MS: i64 = 10 * 60 * 1000;
/// Cleanup intervals per level.
pub const BOUNCE_INTERVAL_STANDARD_MS: i64 = 24 * 3600 * 1000;
pub const BOUNCE_INTERVAL_STRICT_MS: i64 = 3600 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    #[default]
    Standard,
    Strict,
    Custom,
}

/// Which third-party requests lose their cookies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ThirdPartyCookies {
    Off,
    /// Only requests to hosts the privacy lists name (EasyPrivacy, uBO privacy).
    #[default]
    Trackers,
    /// Every third-party request (and `document.cookie` through the engine switch,
    /// which needs a restart).
    All,
}

/// What a cross-site request may say about the page it comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Referrer {
    /// The engine's default (`strict-origin-when-cross-origin`: the origin for cross-origin
    /// requests, the full URL within the site).
    Full,
    /// The origin only, for every cross-site request.
    #[default]
    Origin,
    /// Nothing, for every cross-site request.
    None,
}

/// The tracking-protection settings. Standard and Strict are presets: the other fields
/// are what the user chose for Custom (kept while a preset is active, so switching back
/// restores them).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Protection {
    pub level: Level,
    /// Block known fingerprinting scripts (`krunk-fingerprinting`).
    pub fingerprinting: bool,
    /// Block cryptominers (NoCoin).
    pub cryptomining: bool,
    pub third_party_cookies: ThirdPartyCookies,
    /// Remove tracking parameters (`utm_*`, `fbclid`, …) from navigations.
    pub strip_params: bool,
    pub referrer: Referrer,
    /// Clear the cookies and storage of bounce trackers periodically.
    pub bounce_cleanup: bool,
}

impl Default for Protection {
    fn default() -> Protection {
        Protection::preset(Level::Standard)
    }
}

impl Protection {
    /// The settings a level stands for (Custom: the Standard values as a starting point).
    pub fn preset(level: Level) -> Protection {
        match level {
            Level::Strict => Protection {
                level,
                fingerprinting: true,
                cryptomining: true,
                third_party_cookies: ThirdPartyCookies::All,
                strip_params: true,
                referrer: Referrer::None,
                bounce_cleanup: true,
            },
            Level::Standard | Level::Custom => Protection {
                level,
                fingerprinting: false,
                cryptomining: true,
                third_party_cookies: ThirdPartyCookies::Trackers,
                strip_params: true,
                referrer: Referrer::Origin,
                bounce_cleanup: true,
            },
        }
    }

    /// What is in force: the preset for Standard and Strict, the stored choices for Custom.
    pub fn effective(&self) -> Protection {
        match self.level {
            Level::Custom => self.clone(),
            level => Protection::preset(level),
        }
    }

    /// Settings saved before 0.6.0 had two toggles; the defaults become Standard, anything
    /// else is kept as a Custom configuration.
    pub fn from_legacy(block_third_party_cookies: bool, strip_tracking_params: bool) -> Protection {
        if block_third_party_cookies && strip_tracking_params {
            return Protection::default();
        }
        Protection {
            level: Level::Custom,
            third_party_cookies: if block_third_party_cookies {
                ThirdPartyCookies::All
            } else {
                ThirdPartyCookies::Off
            },
            strip_params: strip_tracking_params,
            ..Protection::preset(Level::Custom)
        }
    }

    /// The bundled lists this configuration adds to the user's filter lists.
    pub fn extra_lists(&self) -> Vec<&'static str> {
        let e = self.effective();
        let mut v = Vec::new();
        if e.cryptomining {
            v.push(CRYPTOMINING_LIST);
        }
        if e.fingerprinting {
            v.push(FINGERPRINTING_LIST);
        }
        v
    }

    /// How often bounce trackers are cleaned at this level.
    pub fn bounce_interval_ms(&self) -> i64 {
        if self.level == Level::Strict {
            BOUNCE_INTERVAL_STRICT_MS
        } else {
            BOUNCE_INTERVAL_STANDARD_MS
        }
    }
}

/// The hosts the privacy lists block outright (`||host^` network rules): a request to one
/// of them, or to a subdomain, is a request to a known tracker.
#[derive(Debug, Default, Clone)]
pub struct TrackerSet {
    hosts: HashSet<String>,
}

impl TrackerSet {
    /// Collects the hosts from filter-list texts. Only plain `||host^` rules count (with
    /// or without options): exceptions, paths, wildcards and cosmetic rules are skipped.
    pub fn from_lists<'a>(texts: impl IntoIterator<Item = &'a str>) -> TrackerSet {
        let mut hosts = HashSet::new();
        for text in texts {
            for line in text.lines() {
                let Some(rest) = line.strip_prefix("||") else { continue };
                let Some(end) = rest.find('^') else { continue };
                let host = &rest[..end];
                let after = &rest[end + 1..];
                let plain_host = !host.is_empty()
                    && host.len() <= 253
                    && host.contains('.')
                    && host.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-');
                let no_path = after.is_empty() || after.starts_with('$');
                if plain_host && no_path {
                    hosts.insert(host.to_ascii_lowercase());
                }
            }
        }
        TrackerSet { hosts }
    }

    pub fn len(&self) -> usize {
        self.hosts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hosts.is_empty()
    }

    /// Is `host` (or a parent domain of it) a known tracker?
    pub fn is_tracker(&self, host: &str) -> bool {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        let mut h = host.as_str();
        loop {
            if self.hosts.contains(h) {
                return true;
            }
            match h.find('.') {
                Some(i) if i + 1 < h.len() => h = &h[i + 1..],
                _ => return false,
            }
        }
    }
}

/// The `Referer` a request should carry under `policy`: `None` leaves the engine's
/// choice alone (same-site requests, no referrer, the Full policy); `Some("")` drops
/// it; `Some(origin)` trims it to the origin (with a trailing slash, as browsers send it).
pub fn trim_referrer(referrer: &str, url: &str, policy: Referrer) -> Option<String> {
    if policy == Referrer::Full || referrer.is_empty() {
        return None;
    }
    let from = url::Url::parse(referrer).ok()?;
    let to = url::Url::parse(url).ok()?;
    if !matches!(from.scheme(), "http" | "https") {
        return None;
    }
    let same_site = match (from.host_str(), to.host_str()) {
        (Some(a), Some(b)) => site_of(a) == site_of(b),
        _ => false,
    };
    if same_site {
        return None;
    }
    match policy {
        Referrer::None => Some(String::new()),
        Referrer::Origin => {
            let origin = from.origin().ascii_serialization();
            let trimmed = format!("{origin}/");
            (trimmed != referrer).then_some(trimmed)
        }
        Referrer::Full => None,
    }
}

/// Bounce trackers: sites that were only ever a redirect hop on the way somewhere else.
/// A site the user visits directly (a history entry) is never one, so the caller removes
/// visited sites before clearing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Bounces {
    /// Site (registrable domain) → when it was last seen as a hop (Unix ms).
    pub hops: BTreeMap<String, i64>,
    pub last_cleanup_ms: i64,
    /// Sites cleaned so far (for the settings page).
    pub cleaned_total: u32,
}

impl Bounces {
    pub fn from_json(json: &str) -> Bounces {
        serde_json::from_str(json).unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// A top-level request to `from_url` was redirected to `to_url`: the first site is a
    /// hop unless the redirect stays within the site (www → apex, http → https).
    pub fn record_hop(&mut self, from_url: &str, to_url: &str, now_ms: i64) -> bool {
        let (Some(from), Some(to)) = (host_site(from_url), host_site(to_url)) else { return false };
        if from == to {
            return false;
        }
        self.hops.insert(from, now_ms);
        true
    }

    /// Drops hops older than the window and returns the sites due for cleaning: seen as a
    /// hop at least the grace period ago and not in `visited` (sites with a history entry).
    pub fn candidates(&mut self, now_ms: i64, visited: &HashSet<String>) -> Vec<String> {
        self.hops.retain(|_, t| now_ms - *t <= BOUNCE_WINDOW_MS);
        let mut due: Vec<String> = self
            .hops
            .iter()
            .filter(|(site, t)| now_ms - **t >= BOUNCE_GRACE_MS && !visited.contains(*site))
            .map(|(s, _)| s.clone())
            .collect();
        // A visited site is never a bounce tracker: forget it.
        self.hops.retain(|s, _| !visited.contains(s));
        due.sort();
        due
    }

    /// The listed sites were cleared.
    pub fn cleared(&mut self, sites: &[String], now_ms: i64) {
        for s in sites {
            self.hops.remove(s);
        }
        self.cleaned_total = self.cleaned_total.saturating_add(sites.len() as u32);
        self.last_cleanup_ms = now_ms;
    }

    pub fn cleanup_due(&self, now_ms: i64, interval_ms: i64) -> bool {
        now_ms - self.last_cleanup_ms >= interval_ms
    }
}

/// The registrable domain of a web URL's host.
pub fn host_site(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    u.host_str().map(site_of)
}

/// The sites (registrable domains) of a list of origins.
pub fn sites_of(origins: &[String]) -> HashSet<String> {
    origins.iter().filter_map(|o| host_site(o)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_are_presets_and_custom_keeps_choices() {
        let std = Protection::default();
        assert_eq!(std.level, Level::Standard);
        assert_eq!(std.third_party_cookies, ThirdPartyCookies::Trackers);
        assert!(!std.fingerprinting && std.cryptomining && std.strip_params && std.bounce_cleanup);
        assert_eq!(std.referrer, Referrer::Origin);
        let strict = Protection::preset(Level::Strict);
        assert!(strict.fingerprinting);
        assert_eq!((strict.third_party_cookies, strict.referrer), (ThirdPartyCookies::All, Referrer::None));
        // A Custom choice survives a preset.
        let mut p = Protection {
            level: Level::Custom,
            fingerprinting: true,
            referrer: Referrer::Full,
            ..Protection::default()
        };
        assert_eq!(p.effective(), p);
        p.level = Level::Standard;
        assert_eq!(p.effective(), Protection::preset(Level::Standard), "Standard ignores the stored choices");
        assert!(p.fingerprinting, "…but keeps them for later");
        assert_eq!(p.extra_lists(), [CRYPTOMINING_LIST]);
        p.level = Level::Strict;
        assert_eq!(p.extra_lists(), [CRYPTOMINING_LIST, FINGERPRINTING_LIST]);
        assert_eq!(p.bounce_interval_ms(), BOUNCE_INTERVAL_STRICT_MS);
    }

    #[test]
    fn legacy_toggles_migrate() {
        assert_eq!(Protection::from_legacy(true, true), Protection::default());
        let p = Protection::from_legacy(false, true);
        assert_eq!((p.level, p.third_party_cookies, p.strip_params), (Level::Custom, ThirdPartyCookies::Off, true));
        let p = Protection::from_legacy(true, false);
        assert_eq!((p.level, p.third_party_cookies, p.strip_params), (Level::Custom, ThirdPartyCookies::All, false));
        let json = serde_json::to_string(&Protection::preset(Level::Strict)).unwrap();
        assert!(json.contains("\"level\":\"strict\"") && json.contains("\"thirdPartyCookies\":\"all\""));
        let back: Protection = serde_json::from_str(r#"{"level":"custom","referrer":"none"}"#).unwrap();
        assert_eq!((back.level, back.referrer, back.cryptomining), (Level::Custom, Referrer::None, true));
    }

    #[test]
    fn tracker_hosts_from_list_rules() {
        let list = "! comment\n||tracker.example^\n||ads.example^$third-party\n@@||good.example^\n||cdn.example^/pixel.gif\n||*.wild.example^\n##.ad\n||UPPER.example^$script,3p\nnot-a-rule\n||localhost^\n";
        let t = TrackerSet::from_lists([list]);
        assert_eq!(t.len(), 3, "{:?}", t.hosts);
        assert!(t.is_tracker("tracker.example") && t.is_tracker("a.b.tracker.example") && t.is_tracker("ADS.example."));
        assert!(t.is_tracker("upper.example"));
        assert!(!t.is_tracker("good.example") && !t.is_tracker("cdn.example") && !t.is_tracker("example"));
        assert!(!t.is_tracker("nottracker.example"), "only a parent domain matches, not a suffix");
        assert!(TrackerSet::default().is_empty());
    }

    #[test]
    fn referrer_trimming() {
        let page = "https://news.example/story?id=1";
        assert_eq!(
            trim_referrer(page, "https://cdn.news.example/a.js", Referrer::Origin),
            None,
            "same site: untouched"
        );
        assert_eq!(
            trim_referrer(page, "https://tracker.example/p", Referrer::Origin),
            Some("https://news.example/".into())
        );
        assert_eq!(trim_referrer(page, "https://tracker.example/p", Referrer::None), Some(String::new()));
        assert_eq!(trim_referrer(page, "https://tracker.example/p", Referrer::Full), None);
        assert_eq!(
            trim_referrer("https://news.example/", "https://other.example/", Referrer::Origin),
            None,
            "already the origin"
        );
        assert_eq!(trim_referrer("", "https://other.example/", Referrer::None), None);
        assert_eq!(trim_referrer("krunk://newtab", "https://other.example/", Referrer::None), None);
        assert_eq!(
            trim_referrer("https://a.example:8443/x", "https://b.example/", Referrer::Origin),
            Some("https://a.example:8443/".into())
        );
    }

    #[test]
    fn bounce_bookkeeping() {
        let mut b = Bounces::default();
        let t0 = 1_000_000_000_000;
        assert!(b.record_hop("https://go.tracker.example/r?u=1", "https://shop.example/", t0));
        assert!(!b.record_hop("http://shop.example/", "https://www.shop.example/", t0), "within the site");
        assert!(!b.record_hop("krunk://newtab", "https://shop.example/", t0));
        b.record_hop("https://link.other.example/x", "https://shop.example/", t0 + 1);
        let visited: HashSet<String> = sites_of(&["https://www.other.example/".to_string()]);
        assert!(b.candidates(t0 + 1000, &visited).is_empty(), "within the grace period");
        assert!(!b.hops.contains_key("other.example"), "a visited site is forgotten");
        let due = b.candidates(t0 + BOUNCE_GRACE_MS, &HashSet::new());
        assert_eq!(due, ["tracker.example"]);
        b.cleared(&due, t0 + BOUNCE_GRACE_MS);
        assert!(b.hops.is_empty() && b.cleaned_total == 1);
        assert!(!b.cleanup_due(t0 + BOUNCE_GRACE_MS + 10, BOUNCE_INTERVAL_STANDARD_MS));
        assert!(b.cleanup_due(t0 + BOUNCE_GRACE_MS + BOUNCE_INTERVAL_STANDARD_MS, BOUNCE_INTERVAL_STANDARD_MS));
        b.record_hop("https://old.example/", "https://x.example/", t0);
        assert!(b.candidates(t0 + BOUNCE_WINDOW_MS + 1, &HashSet::new()).is_empty(), "too old: dropped");
        let json = b.to_json();
        assert_eq!(Bounces::from_json(&json), b);
        assert_eq!(Bounces::from_json("garbage"), Bounces::default());
    }
}
