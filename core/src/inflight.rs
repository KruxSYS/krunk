//! The requests under way (Krunk 0.18.0, docs/NETWORK.md): which tab waits for what, since
//! when. Three uses:
//!
//! - **Around the cache lock.** The engine's HTTP cache lets one request at a time fetch a
//!   URL; a second one for the same URL waits for the first one's answer, up to 20 s
//!   (measured). A request for a URL that another tab has been waiting on for
//!   [`BYPASS_AFTER_MS`] goes around the cache instead ([`InFlight::start`] says so).
//! - **krunk://tasks** shows each tab's pending requests and the age of the oldest
//!   ([`InFlight::pending`]).
//! - **krunk.log** gets a line for requests pending over [`SLOW_AFTER_MS`]
//!   ([`InFlight::slow`]): the tab, the host (never the address or its query; nothing of a
//!   private tab), the kind and how many. Rate-limited.
//!
//! Bounded: at most [`MAX_TRACKED`] requests are remembered (the oldest is forgotten).

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use crate::urlclean;

/// A request goes around the cache when another tab's request for the same URL has waited
/// this long for its answer. (Shorter waits are ordinary; then the two share one fetch.)
pub const BYPASS_AFTER_MS: i64 = 1_000;
/// A request pending this long is worth a line in the log.
pub const SLOW_AFTER_MS: i64 = 30_000;
/// A tab that waits for this many answers may hold every connection the engine opens to one
/// host (six): before the host navigates or reloads that tab, its loads are stopped, or the
/// new navigation would wait for one of the very connections the old page holds.
pub const STUCK_WAITING: u32 = 6;
/// Requests remembered at once.
pub const MAX_TRACKED: usize = 4096;
/// At most one batch of log lines in this time, of at most [`MAX_REPORTS`] lines.
pub const REPORT_EVERY_MS: i64 = 10_000;
pub const MAX_REPORTS: usize = 4;
/// The same tab and host are not reported again within this time.
pub const REPORT_AGAIN_MS: i64 = 10 * 60 * 1000;
const MAX_REPORTED_KEYS: usize = 256;

#[derive(Debug, Clone)]
struct Req {
    tab: u32,
    private: bool,
    /// A hash of the whole address: enough to tell "the same URL", and nothing to leak.
    url: u64,
    host: String,
    resource_type: i32,
    get: bool,
    started: i64,
    /// The answer's headers have arrived (its body may still be coming).
    answered: bool,
    reported: bool,
}

/// A tab's pending requests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pending {
    pub count: u32,
    /// Of those, the ones without an answer yet.
    pub waiting: u32,
    /// Age of the oldest pending request.
    pub oldest_ms: i64,
}

/// One log line: requests of one tab to one host, of one kind, pending too long.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlowReport {
    pub tab: u32,
    /// `None` for a private tab (nothing about its pages is written anywhere).
    pub host: Option<String>,
    pub kind: &'static str,
    pub count: u32,
    /// Of those, the ones without an answer yet.
    pub waiting: u32,
    pub oldest_ms: i64,
}

impl SlowReport {
    /// The line for krunk.log.
    pub fn line(&self) -> String {
        let to = match &self.host {
            Some(h) => format!(" to {h}"),
            None => " (private tab)".to_string(),
        };
        let state = if self.waiting == self.count {
            "no answer yet".to_string()
        } else if self.waiting == 0 {
            "answer still arriving".to_string()
        } else {
            format!("{} without an answer yet", self.waiting)
        };
        format!(
            "tab {}: {} {} request{}{to} pending for over {} s (oldest {} s, {state})",
            self.tab,
            self.count,
            self.kind,
            if self.count == 1 { "" } else { "s" },
            SLOW_AFTER_MS / 1000,
            self.oldest_ms / 1000,
        )
    }
}

/// CEF's `cef_resource_type_t` in words.
pub fn kind_name(resource_type: i32) -> &'static str {
    match resource_type {
        0 | 19 => "page",
        1 | 20 => "frame",
        2 => "stylesheet",
        3 => "script",
        4 => "image",
        5 => "font",
        8 => "media",
        9 | 10 | 15 => "worker",
        12 => "favicon",
        13 => "fetch",
        14 => "ping",
        _ => "other",
    }
}

fn url_hash(url: &str) -> u64 {
    // Without the fragment: it is not part of what is fetched.
    let mut h = DefaultHasher::new();
    url.split('#').next().unwrap_or("").hash(&mut h);
    h.finish()
}

#[derive(Debug, Default)]
pub struct InFlight {
    reqs: HashMap<u64, Req>,
    last_report: i64,
    reported: HashMap<(u32, String), i64>,
}

impl InFlight {
    pub fn new() -> InFlight {
        InFlight::default()
    }

    /// Request `id` of `tab` is about to be sent (again after a redirect: its start stays).
    /// `true`: it should go around the cache, because another tab of the same kind (normal or
    /// private: each has its own cache) waits for this very URL since [`BYPASS_AFTER_MS`].
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &mut self,
        id: u64,
        tab: u32,
        private: bool,
        url: &str,
        get: bool,
        resource_type: i32,
        now: i64,
    ) -> bool {
        let web = url.starts_with("http://") || url.starts_with("https://");
        if id == 0 || !web {
            return false;
        }
        let hash = url_hash(url);
        let bypass = get
            && self.reqs.iter().any(|(other, r)| {
                *other != id
                    && r.url == hash
                    && r.get
                    && r.tab != tab
                    && r.private == private
                    && !r.answered
                    && now - r.started >= BYPASS_AFTER_MS
            });
        let host = urlclean::host_of(url).unwrap_or_default();
        match self.reqs.get_mut(&id) {
            Some(r) => {
                r.url = hash;
                r.host = host;
                r.get = get;
                r.answered = false;
            }
            None => {
                if self.reqs.len() >= MAX_TRACKED {
                    self.forget_one();
                }
                self.reqs.insert(
                    id,
                    Req {
                        tab,
                        private,
                        url: hash,
                        host,
                        resource_type,
                        get,
                        started: now,
                        answered: false,
                        reported: false,
                    },
                );
            }
        }
        bypass
    }

    /// Full: the oldest answered request goes (a stream, most likely), else the oldest.
    fn forget_one(&mut self) {
        let oldest = |answered: bool| {
            self.reqs.iter().filter(|(_, r)| r.answered == answered).min_by_key(|(_, r)| r.started).map(|(id, _)| *id)
        };
        if let Some(id) = oldest(true).or_else(|| oldest(false)) {
            self.reqs.remove(&id);
        }
    }

    /// The answer's headers arrived.
    pub fn answered(&mut self, id: u64) {
        if let Some(r) = self.reqs.get_mut(&id) {
            r.answered = true;
        }
    }

    /// The request ended (done, failed or cancelled).
    pub fn finish(&mut self, id: u64) {
        self.reqs.remove(&id);
    }

    /// The tab closed: whatever the engine still says about its requests is ignored.
    pub fn drop_tab(&mut self, tab: u32) {
        self.reqs.retain(|_, r| r.tab != tab);
        self.reported.retain(|(t, _), _| *t != tab);
    }

    pub fn len(&self) -> usize {
        self.reqs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.reqs.is_empty()
    }

    /// What `tab` waits for.
    pub fn pending(&self, tab: u32, now: i64) -> Pending {
        let mut p = Pending::default();
        for r in self.reqs.values().filter(|r| r.tab == tab) {
            p.count += 1;
            p.waiting += u32::from(!r.answered);
            p.oldest_ms = p.oldest_ms.max(now - r.started);
        }
        p
    }

    /// The lines to log now: requests pending over [`SLOW_AFTER_MS`] not reported before,
    /// grouped by tab, host and kind. At most [`MAX_REPORTS`] every [`REPORT_EVERY_MS`], and
    /// a tab's host once in [`REPORT_AGAIN_MS`] (a page that polls slowly would fill the log).
    pub fn slow(&mut self, now: i64) -> Vec<SlowReport> {
        if now - self.last_report < REPORT_EVERY_MS && self.last_report != 0 {
            return Vec::new();
        }
        self.reported.retain(|_, at| now - *at < REPORT_AGAIN_MS);
        let mut groups: Vec<SlowReport> = Vec::new();
        let mut keys: Vec<(u32, String, &'static str)> = Vec::new();
        for r in self.reqs.values_mut() {
            if r.reported || now - r.started < SLOW_AFTER_MS {
                continue;
            }
            if self.reported.contains_key(&(r.tab, r.host.clone())) {
                // Said already for this tab and host: this one is not news.
                r.reported = true;
                continue;
            }
            let kind = kind_name(r.resource_type);
            let at = match keys.iter().position(|k| k.0 == r.tab && k.1 == r.host && k.2 == kind) {
                Some(i) => i,
                None => {
                    if groups.len() >= MAX_REPORTS {
                        continue; // next time
                    }
                    keys.push((r.tab, r.host.clone(), kind));
                    groups.push(SlowReport {
                        tab: r.tab,
                        host: (!r.private).then(|| r.host.clone()),
                        kind,
                        count: 0,
                        waiting: 0,
                        oldest_ms: 0,
                    });
                    groups.len() - 1
                }
            };
            r.reported = true;
            let g = &mut groups[at];
            g.count += 1;
            g.waiting += u32::from(!r.answered);
            g.oldest_ms = g.oldest_ms.max(now - r.started);
        }
        if !groups.is_empty() {
            self.last_report = now;
            if self.reported.len() + keys.len() > MAX_REPORTED_KEYS {
                self.reported.clear();
            }
            for (tab, host, _) in keys {
                self.reported.insert((tab, host), now);
            }
        }
        groups.sort_by(|a, b| (a.tab, a.kind, &a.host).cmp(&(b.tab, b.kind, &b.host)));
        groups
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://slow.example/app.js?v=3";

    #[test]
    fn a_url_another_tab_waits_for_goes_around_the_cache() {
        let mut f = InFlight::new();
        assert!(!f.start(1, 10, false, URL, true, 3, 0), "the first request uses the cache");
        // At once: the two share one fetch, as the engine does it.
        assert!(!f.start(2, 11, false, URL, true, 3, 400));
        // A second later the first still has no answer: don't queue behind it.
        assert!(f.start(3, 12, false, URL, true, 3, BYPASS_AFTER_MS));
        // The fragment is not part of the request.
        assert!(f.start(4, 12, false, "https://slow.example/app.js?v=3#top", true, 3, 2_000));
        // Not for another URL, another query, a POST, or the other cache.
        assert!(!f.start(6, 13, false, "https://slow.example/app.js?v=4", true, 3, 5_000));
        assert!(!f.start(7, 13, false, "https://slow.example/other.js", true, 3, 5_000));
        assert!(!f.start(8, 13, false, URL, false, 3, 5_000), "a POST is not cached");
        assert!(!f.start(9, 14, true, URL, true, 3, 5_000), "a private tab has its own cache");
        assert_eq!(f.len(), 8);
        // Nor behind its own tab's request (a page asking twice shares the fetch).
        let mut f = InFlight::new();
        f.start(1, 10, false, URL, true, 3, 0);
        assert!(!f.start(2, 10, false, URL, true, 3, 5_000));
    }

    #[test]
    fn an_answered_or_finished_request_holds_nobody_up() {
        let mut f = InFlight::new();
        f.start(1, 10, false, URL, true, 0, 0);
        f.answered(1);
        // Its headers are there: the engine shares the body or fetches at once (measured).
        assert!(!f.start(2, 11, false, URL, true, 0, 9_000));
        f.finish(2);
        f.finish(1);
        assert!(f.is_empty());
        f.start(3, 10, false, URL, true, 0, 10_000);
        f.finish(3);
        assert!(!f.start(4, 11, false, URL, true, 0, 20_000));
        // Unknown ids and non-web addresses are nothing.
        f.finish(99);
        f.answered(99);
        assert!(!f.start(0, 11, false, URL, true, 0, 20_000));
        assert!(!f.start(5, 11, false, "krunk://tasks", true, 0, 20_000));
        assert!(!f.start(6, 11, false, "data:text/plain,x", true, 0, 20_000));
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn a_redirect_keeps_the_start_and_takes_the_new_address() {
        let mut f = InFlight::new();
        f.start(1, 10, false, "https://a.example/r", true, 0, 0);
        f.answered(1);
        f.start(1, 10, false, "https://b.example/landing", true, 0, 3_000);
        assert_eq!(f.len(), 1);
        assert_eq!(f.pending(10, 5_000), Pending { count: 1, waiting: 1, oldest_ms: 5_000 });
        assert!(f.start(2, 11, false, "https://b.example/landing", true, 0, 5_000));
        assert!(!f.start(3, 12, false, "https://a.example/r", true, 0, 5_000));
    }

    #[test]
    fn pending_counts_per_tab_with_the_oldest_age() {
        let mut f = InFlight::new();
        assert_eq!(f.pending(10, 0), Pending::default());
        f.start(1, 10, false, URL, true, 3, 1_000);
        f.start(2, 10, false, "https://slow.example/b", true, 4, 4_000);
        f.start(3, 11, false, "https://slow.example/c", true, 4, 5_000);
        f.answered(2);
        assert_eq!(f.pending(10, 6_000), Pending { count: 2, waiting: 1, oldest_ms: 5_000 });
        assert_eq!(f.pending(11, 6_000), Pending { count: 1, waiting: 1, oldest_ms: 1_000 });
        f.drop_tab(10);
        assert_eq!(f.pending(10, 6_000), Pending::default());
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn it_is_bounded() {
        let mut f = InFlight::new();
        for i in 1..=(MAX_TRACKED as u64 + 50) {
            f.start(i, 10, false, &format!("https://x.example/{i}"), true, 4, i as i64);
            if i == 7 {
                f.answered(7);
            }
        }
        assert_eq!(f.len(), MAX_TRACKED);
        // The answered one went first, then the oldest.
        assert_eq!(f.pending(10, 0).waiting as usize, MAX_TRACKED);
        f.finish(1);
        assert_eq!(f.len(), MAX_TRACKED, "request 1 was forgotten already");
    }

    #[test]
    fn slow_requests_are_reported_once_by_host_without_the_address() {
        let mut f = InFlight::new();
        f.start(1, 10, false, "https://user:pw@slow.example/secret/path?token=abc", true, 13, 0);
        f.start(2, 10, false, "https://slow.example/other?token=def", true, 13, 1_000);
        f.start(3, 10, false, "https://slow.example/pic.png", true, 4, 2_000);
        f.start(4, 11, true, "https://private.example/x", true, 0, 0);
        f.start(5, 12, false, "https://fast.example/", true, 0, 25_000);
        f.answered(2);
        assert!(f.slow(SLOW_AFTER_MS - 1).is_empty());
        let r = f.slow(40_000);
        assert_eq!(
            r,
            vec![
                SlowReport {
                    tab: 10,
                    host: Some("slow.example".into()),
                    kind: "fetch",
                    count: 2,
                    waiting: 1,
                    oldest_ms: 40_000
                },
                SlowReport {
                    tab: 10,
                    host: Some("slow.example".into()),
                    kind: "image",
                    count: 1,
                    waiting: 1,
                    oldest_ms: 38_000
                },
                SlowReport { tab: 11, host: None, kind: "page", count: 1, waiting: 1, oldest_ms: 40_000 },
            ]
        );
        let lines: Vec<String> = r.iter().map(SlowReport::line).collect();
        assert_eq!(
            lines[0],
            "tab 10: 2 fetch requests to slow.example pending for over 30 s (oldest 40 s, 1 without an answer yet)"
        );
        assert_eq!(
            lines[1],
            "tab 10: 1 image request to slow.example pending for over 30 s (oldest 38 s, no answer yet)"
        );
        assert_eq!(lines[2], "tab 11: 1 page request (private tab) pending for over 30 s (oldest 40 s, no answer yet)");
        for l in &lines {
            assert!(
                !l.contains("token") && !l.contains("secret") && !l.contains("pw") && !l.contains("private.example"),
                "{l}"
            );
        }
        f.finish(5);
        // Not again: neither the same requests nor new ones of that tab and host for a while.
        assert!(f.slow(60_000).is_empty());
        f.start(6, 10, false, "https://slow.example/again", true, 13, 50_000);
        assert!(f.slow(100_000).is_empty());
        // Another tab's request is news; the rate limit holds it back for a moment.
        f.start(7, 12, false, "https://slow.example/again", true, 13, 100_000);
        let later = 40_000 + REPORT_AGAIN_MS;
        f.start(8, 10, false, "https://slow.example/late", true, 13, later - SLOW_AFTER_MS);
        let r = f.slow(later);
        assert_eq!(r.len(), 2, "{r:?}");
        assert_eq!((r[0].tab, r[0].kind, r[0].count), (10, "fetch", 1));
        assert_eq!((r[1].tab, r[1].kind, r[1].oldest_ms), (12, "fetch", later - 100_000));
        assert!(f.slow(later + REPORT_EVERY_MS - 1).is_empty());
    }

    #[test]
    fn a_batch_has_at_most_a_few_lines_and_the_rest_follows() {
        let mut f = InFlight::new();
        for i in 1..=7u64 {
            f.start(i, 10, false, &format!("https://h{i}.example/x"), true, 3, 0);
        }
        assert_eq!(f.slow(SLOW_AFTER_MS).len(), MAX_REPORTS);
        assert!(f.slow(SLOW_AFTER_MS + 1_000).is_empty(), "rate limit");
        assert_eq!(f.slow(SLOW_AFTER_MS + REPORT_EVERY_MS).len(), 3);
        assert!(f.slow(SLOW_AFTER_MS + 2 * REPORT_EVERY_MS).is_empty());
    }
}
