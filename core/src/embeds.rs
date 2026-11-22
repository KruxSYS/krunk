//! Embedded content under tracking protection (Krunk 0.28.0). Strict sends no `Referer`
//! to other sites, which breaks video players that check where they are embedded
//! (YouTube's "error 153"): their frames, and what those frames load, get the origin, as
//! Chrome sends by default. Social-media widgets (like buttons, embedded posts, comment
//! threads) are click-to-load on Strict: the frame shows a local placeholder with "Load"
//! and "Always allow on this site" (Firefox's SmartBlock placeholders do the same). A
//! filter list blocking a known embed frame shows the placeholder too, at any level.
//!
//! The placeholder's buttons navigate the frame to its address plus a marker parameter
//! (`krunk-embed=<nonce>`); the browser answers the marked request itself (the marker never
//! reaches the site), records the allow and sends the frame on to the clean address.
//! Everything here is pure; the IO-thread hook is in the app. See
//! `docs/TRACKING_PROTECTION.md`.

use crate::cookies::site_of;
use crate::protection::{trim_referrer, Protection, Referrer};
use std::collections::{BTreeMap, HashMap, HashSet};

/// The marker parameter the placeholder's buttons add.
pub const MARKER: &str = "krunk-embed";
/// Added next to the marker by "Always allow on this site".
pub const MARKER_ALWAYS: &str = "krunk-embed-always";
/// How long a placeholder's buttons work.
pub const NONCE_TTL_MS: i64 = 10 * 60 * 1000;
/// Most nonces kept per tab (the oldest go first).
const MAX_NONCES: usize = 64;

/// Which paths of a host count as an embed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathRule {
    Any,
    Prefix(&'static str),
    Contains(&'static str),
}

/// A host (and its subdomains) whose frames are embeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbedHost {
    pub host: &'static str,
    pub path: PathRule,
}

const fn any(host: &'static str) -> EmbedHost {
    EmbedHost { host, path: PathRule::Any }
}

/// Video and audio players.
pub const VIDEO_EMBED_HOSTS: &[EmbedHost] = &[
    any("youtube.com"),
    any("youtube-nocookie.com"),
    any("ytimg.com"),
    any("googlevideo.com"),
    any("player.vimeo.com"),
    any("vimeo.com"),
    any("vimeocdn.com"),
    any("dailymotion.com"),
    any("dmcdn.net"),
    any("player.twitch.tv"),
    EmbedHost { host: "google.com", path: PathRule::Prefix("/maps/embed") },
    EmbedHost { host: "open.spotify.com", path: PathRule::Prefix("/embed") },
    any("w.soundcloud.com"),
];

/// Social-media widgets (click-to-load on Strict).
pub const SOCIAL_EMBED_HOSTS: &[EmbedHost] = &[
    EmbedHost { host: "facebook.com", path: PathRule::Prefix("/plugins/") },
    any("platform.twitter.com"),
    any("syndication.twitter.com"),
    any("platform.x.com"),
    any("syndication.x.com"),
    EmbedHost { host: "instagram.com", path: PathRule::Contains("/embed") },
    EmbedHost { host: "tiktok.com", path: PathRule::Prefix("/embed") },
    EmbedHost { host: "linkedin.com", path: PathRule::Prefix("/embed") },
    any("disqus.com"),
    any("embed.reddit.com"),
];

/// What kind of embed an address is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedKind {
    Video,
    Social,
}

fn host_matches(host: &str, list_host: &str) -> bool {
    host == list_host || host.strip_suffix(list_host).is_some_and(|p| p.ends_with('.'))
}

fn matches_list(list: &[EmbedHost], host: &str, path: Option<&str>) -> bool {
    list.iter().any(|e| {
        host_matches(host, e.host)
            && match (e.path, path) {
                (PathRule::Any, _) => true,
                // An origin (no path) matches only a host-wide entry.
                (_, None) => false,
                (PathRule::Prefix(p), Some(path)) => path.starts_with(p),
                (PathRule::Contains(p), Some(path)) => path.contains(p),
            }
    })
}

fn parse_web(url: &str) -> Option<url::Url> {
    let u = url::Url::parse(url).ok()?;
    matches!(u.scheme(), "http" | "https").then_some(u)
}

/// The embed kind of a frame address (host and path), if any.
pub fn kind_of(url: &str) -> Option<EmbedKind> {
    let u = parse_web(url)?;
    let host = u.host_str()?.trim_end_matches('.').to_ascii_lowercase();
    if matches_list(VIDEO_EMBED_HOSTS, &host, Some(u.path())) {
        Some(EmbedKind::Video)
    } else if matches_list(SOCIAL_EMBED_HOSTS, &host, Some(u.path())) {
        Some(EmbedKind::Social)
    } else {
        None
    }
}

/// Is `origin` (a request's initiator) a video player's? Only host-wide entries count.
pub fn is_video_origin(origin: &str) -> bool {
    parse_web(origin)
        .and_then(|u| u.host_str().map(|h| h.trim_end_matches('.').to_ascii_lowercase()))
        .is_some_and(|h| matches_list(VIDEO_EMBED_HOSTS, &h, None))
}

/// The `Referer` a request should carry (same convention as [`trim_referrer`]). Under the
/// None policy (Strict), a cross-site sub-frame document (`resource_type` 1) of a video
/// player, and every request a video player's document makes, get the origin instead.
pub fn referrer_for(referrer: &str, url: &str, resource_type: i32, initiator: &str, policy: Referrer) -> Option<String> {
    if policy == Referrer::None
        && ((resource_type == 1 && kind_of(url) == Some(EmbedKind::Video)) || is_video_origin(initiator))
    {
        return trim_referrer(referrer, url, Referrer::Origin);
    }
    trim_referrer(referrer, url, policy)
}

fn site_of_url(url: &str) -> Option<String> {
    parse_web(url).and_then(|u| u.host_str().map(site_of))
}

/// The embeds the user let load: stored per top site (Settings `embedAllow`) and for this
/// tab's page only (the sites of frames loaded with "Load").
#[derive(Debug, Clone, Copy)]
pub struct EmbedAllows<'a> {
    pub stored: &'a BTreeMap<String, Vec<String>>,
    pub session: &'a HashSet<String>,
}

impl EmbedAllows<'_> {
    /// May content from `url`'s site (a frame address or an initiator origin) load on the
    /// page `top_url`?
    pub fn allows(&self, top_url: &str, url: &str) -> bool {
        let Some(site) = site_of_url(url) else { return false };
        if self.session.contains(&site) {
            return true;
        }
        site_of_url(top_url).and_then(|t| self.stored.get(&t)).is_some_and(|v| v.contains(&site))
    }
}

/// What a sub-frame document request becomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedDecision {
    /// Not ours to change (loads, or is blocked by the lists as usual).
    Load,
    /// Answered with the click-to-load placeholder.
    Placeholder,
}

/// The decision for a sub-frame document `frame_url` on the page `top_url`. `protection` is
/// the effective configuration; `list_blocked`: a filter list blocks the frame.
pub fn embed_decision(
    protection: &Protection,
    top_url: &str,
    frame_url: &str,
    list_blocked: bool,
    allows: &EmbedAllows<'_>,
) -> EmbedDecision {
    let (Some(top), Some(frame)) = (site_of_url(top_url), site_of_url(frame_url)) else {
        return EmbedDecision::Load;
    };
    if top == frame || allows.allows(top_url, frame_url) {
        return EmbedDecision::Load;
    }
    match kind_of(frame_url) {
        Some(_) if list_blocked => EmbedDecision::Placeholder,
        Some(EmbedKind::Social) if protection.social_embeds => EmbedDecision::Placeholder,
        _ => EmbedDecision::Load,
    }
}

/// `url` without the marker parameters, and the marker (nonce, "always") if it had one.
/// Other parameters keep their order and exact spelling; the fragment is kept.
pub fn strip_marker(url: &str) -> (String, Option<(String, bool)>) {
    let (before_frag, frag) = match url.find('#') {
        Some(i) => (&url[..i], Some(&url[i..])),
        None => (url, None),
    };
    let Some(q) = before_frag.find('?') else { return (url.to_string(), None) };
    let (base, query) = (&before_frag[..q], &before_frag[q + 1..]);
    let mut nonce = None;
    let mut always = false;
    let mut kept = Vec::new();
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k == MARKER {
            nonce = Some(v.to_string());
        } else if k == MARKER_ALWAYS {
            always = v == "1";
        } else {
            kept.push(pair);
        }
    }
    let Some(nonce) = nonce else { return (url.to_string(), None) };
    let mut clean = base.to_string();
    if !kept.is_empty() {
        clean.push('?');
        clean.push_str(&kept.join("&"));
    }
    if let Some(f) = frag {
        clean.push_str(f);
    }
    (clean, Some((nonce, always)))
}

/// `url` with the marker added (before the fragment).
pub fn marked_url(url: &str, nonce: &str, always: bool) -> String {
    let (before_frag, frag) = match url.find('#') {
        Some(i) => (&url[..i], &url[i..]),
        None => (url, ""),
    };
    let sep = if before_frag.contains('?') { '&' } else { '?' };
    let extra = if always { format!("&{MARKER_ALWAYS}=1") } else { String::new() };
    format!("{before_frag}{sep}{MARKER}={nonce}{extra}{frag}")
}

/// The placeholders' nonces of one tab: each is for one frame address, works once and
/// expires after [`NONCE_TTL_MS`].
#[derive(Debug, Default)]
pub struct Nonces {
    issued: HashMap<String, (String, i64)>,
}

impl Nonces {
    /// A new nonce for a placeholder of `url` (the clean address).
    pub fn issue(&mut self, url: &str, now_ms: i64) -> String {
        let mut b = [0u8; 16];
        if getrandom::fill(&mut b).is_err() {
            // No randomness: a nonce nobody can guess is not possible; one that never
            // matches keeps the placeholder in place.
            return String::new();
        }
        let nonce: String = b.iter().map(|x| format!("{x:02x}")).collect();
        self.insert(nonce.clone(), url, now_ms);
        nonce
    }

    fn insert(&mut self, nonce: String, url: &str, now_ms: i64) {
        self.issued.retain(|_, (_, t)| now_ms - *t < NONCE_TTL_MS);
        if self.issued.len() >= MAX_NONCES {
            if let Some(oldest) = self.issued.iter().min_by_key(|(_, (_, t))| *t).map(|(k, _)| k.clone()) {
                self.issued.remove(&oldest);
            }
        }
        self.issued.insert(nonce, (url.to_string(), now_ms));
    }

    /// Is `nonce` one issued for `url` and still valid? It is used up either way it matched.
    pub fn consume(&mut self, nonce: &str, url: &str, now_ms: i64) -> bool {
        if nonce.is_empty() {
            return false;
        }
        match self.issued.remove(nonce) {
            Some((u, t)) => u == url && now_ms - t < NONCE_TTL_MS && now_ms >= t,
            None => false,
        }
    }

    pub fn len(&self) -> usize {
        self.issued.len()
    }

    pub fn is_empty(&self) -> bool {
        self.issued.is_empty()
    }
}

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// A JavaScript string literal safe inside `<script>`.
fn js_string(s: &str) -> String {
    serde_json::to_string(s)
        .unwrap_or_else(|_| "\"\"".into())
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// The placeholder for the frame `url` (clean address). `always_nonce`: offer "Always allow
/// on this site" (not in private tabs). Self-contained: no script, no external loads.
pub fn placeholder_html(url: &str, nonce: &str, always_nonce: Option<&str>) -> String {
    let host = parse_web(url).and_then(|u| u.host_str().map(str::to_string)).unwrap_or_default();
    let load = html_escape(&marked_url(url, nonce, false));
    let always = always_nonce
        .map(|n| {
            format!(
                r#"<a class="b" href="{}" target="_self">Always allow on this site</a>"#,
                html_escape(&marked_url(url, n, true))
            )
        })
        .unwrap_or_default();
    let host = html_escape(&host);
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="referrer" content="no-referrer"><meta name="color-scheme" content="light dark"><title>Blocked by Krunk</title><style>
html,body{{margin:0;height:100%}}body{{display:flex;align-items:center;justify-content:center;font:13px/1.4 system-ui,"Segoe UI",sans-serif;background:#f3f3f3;color:#222;overflow:auto}}
.w{{max-width:100%;padding:8px 10px;box-sizing:border-box;text-align:center}}p{{margin:0 0 8px;overflow-wrap:anywhere}}
.b{{display:inline-block;margin:2px 3px;padding:4px 10px;border-radius:6px;border:1px solid #bbb;background:#fff;color:#1a56c9;text-decoration:none;white-space:nowrap}}
.b:hover{{background:#e8eefc}}.b.p{{background:#1a56c9;color:#fff;border-color:#1a56c9}}
@media (prefers-color-scheme:dark){{body{{background:#232323;color:#ddd}}.b{{background:#333;border-color:#555;color:#8ab4f8}}.b:hover{{background:#3b4250}}.b.p{{background:#8ab4f8;color:#111;border-color:#8ab4f8}}}}
</style></head><body><div class="w"><p>Krunk blocked content from <b>{host}</b></p><a class="b p" href="{load}" target="_self">Load</a>{always}</div></body></html>"#
    )
}

/// The answer to a marked request: sends the frame on to `clean_url` (no history entry,
/// no `Referer` from this stand-in page).
pub fn redirect_html(clean_url: &str) -> String {
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta name="referrer" content="no-referrer"><script>location.replace({})</script></head><body></body></html>"#,
        js_string(clean_url)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protection::Level;

    const TOP: &str = "https://news.example/story";

    fn none<'a>(stored: &'a BTreeMap<String, Vec<String>>, session: &'a HashSet<String>) -> EmbedAllows<'a> {
        EmbedAllows { stored, session }
    }

    #[test]
    fn kinds_by_host_and_path() {
        assert_eq!(kind_of("https://www.youtube.com/embed/abc"), Some(EmbedKind::Video));
        assert_eq!(kind_of("https://www.youtube-nocookie.com/embed/abc"), Some(EmbedKind::Video));
        assert_eq!(kind_of("https://player.vimeo.com/video/1"), Some(EmbedKind::Video));
        assert_eq!(kind_of("https://www.google.com/maps/embed?pb=1"), Some(EmbedKind::Video));
        assert_eq!(kind_of("https://www.google.com/search?q=x"), None);
        assert_eq!(kind_of("https://open.spotify.com/embed/track/1"), Some(EmbedKind::Video));
        assert_eq!(kind_of("https://open.spotify.com/track/1"), None);
        assert_eq!(kind_of("https://www.facebook.com/plugins/post.php?href=x"), Some(EmbedKind::Social));
        assert_eq!(kind_of("https://www.facebook.com/somepage"), None);
        assert_eq!(kind_of("https://platform.twitter.com/embed/Tweet.html?id=20"), Some(EmbedKind::Social));
        assert_eq!(kind_of("https://www.instagram.com/p/XYZ/embed/"), Some(EmbedKind::Social));
        assert_eq!(kind_of("https://www.tiktok.com/embed/v2/1"), Some(EmbedKind::Social));
        assert_eq!(kind_of("https://www.linkedin.com/embed/feed/update/1"), Some(EmbedKind::Social));
        assert_eq!(kind_of("https://foo.disqus.com/embed/comments"), Some(EmbedKind::Social));
        assert_eq!(kind_of("https://embed.reddit.com/r/x"), Some(EmbedKind::Social));
        assert_eq!(kind_of("https://notyoutube.com/embed"), None, "suffix without a dot");
        assert_eq!(kind_of("data:text/html,x"), None);
        assert!(is_video_origin("https://www.youtube.com"));
        assert!(!is_video_origin("https://www.google.com"), "path-restricted entries need a path");
        assert!(!is_video_origin(""));
    }

    #[test]
    fn video_embeds_get_the_origin_on_strict() {
        let r = "https://news.example/story?id=1";
        let yt = "https://www.youtube.com/embed/abc";
        assert_eq!(referrer_for(r, yt, 1, "https://news.example", Referrer::None), Some("https://news.example/".into()));
        // The player's own requests (initiator youtube.com) to googlevideo.com.
        assert_eq!(
            referrer_for("https://www.youtube.com/embed/abc", "https://rr1.googlevideo.com/v", 6, "https://www.youtube.com", Referrer::None),
            Some("https://www.youtube.com/".into())
        );
        // A script (not a frame) to youtube.com from the page: nothing.
        assert_eq!(referrer_for(r, "https://www.youtube.com/iframe_api", 2, "https://news.example", Referrer::None), Some(String::new()));
        // Other frames: nothing.
        assert_eq!(referrer_for(r, "https://ads.example/f", 1, "https://news.example", Referrer::None), Some(String::new()));
        // Standard and Full unchanged.
        assert_eq!(referrer_for(r, yt, 1, "https://news.example", Referrer::Origin), trim_referrer(r, yt, Referrer::Origin));
        assert_eq!(referrer_for(r, yt, 1, "https://news.example", Referrer::Full), None);
    }

    #[test]
    fn decisions() {
        let strict = Protection::preset(Level::Strict);
        let standard = Protection::preset(Level::Standard);
        assert!(strict.social_embeds && !standard.social_embeds);
        let (stored, session) = (BTreeMap::new(), HashSet::new());
        let a = none(&stored, &session);
        let tw = "https://platform.twitter.com/embed/Tweet.html?id=20";
        let yt = "https://www.youtube.com/embed/abc";
        assert_eq!(embed_decision(&strict, TOP, tw, false, &a), EmbedDecision::Placeholder);
        assert_eq!(embed_decision(&strict, TOP, yt, false, &a), EmbedDecision::Load, "video embeds load on Strict");
        assert_eq!(embed_decision(&strict, TOP, "https://ads.example/f", false, &a), EmbedDecision::Load);
        assert_eq!(embed_decision(&strict, TOP, "https://ads.example/f", true, &a), EmbedDecision::Load, "list blocks it");
        // Standard never placeholders unless a list blocks a known embed.
        assert_eq!(embed_decision(&standard, TOP, tw, false, &a), EmbedDecision::Load);
        assert_eq!(embed_decision(&standard, TOP, tw, true, &a), EmbedDecision::Placeholder);
        assert_eq!(embed_decision(&standard, TOP, yt, true, &a), EmbedDecision::Placeholder);
        // First-party frames never.
        assert_eq!(embed_decision(&strict, "https://www.facebook.com/", "https://www.facebook.com/plugins/x", true, &a), EmbedDecision::Load);
        // Custom follows the toggle.
        let custom = Protection { level: Level::Custom, social_embeds: true, ..Protection::preset(Level::Custom) };
        assert_eq!(embed_decision(&custom.effective(), TOP, tw, false, &a), EmbedDecision::Placeholder);
        // Allowed: stored per top site, or for this page.
        let mut stored = BTreeMap::new();
        stored.insert("news.example".to_string(), vec!["twitter.com".to_string()]);
        let a = none(&stored, &session);
        assert_eq!(embed_decision(&strict, TOP, tw, true, &a), EmbedDecision::Load);
        assert_eq!(embed_decision(&strict, "https://other.example/", tw, false, &a), EmbedDecision::Placeholder);
        let session: HashSet<String> = ["facebook.com".to_string()].into();
        let empty = BTreeMap::new();
        let a = none(&empty, &session);
        assert_eq!(embed_decision(&strict, TOP, "https://www.facebook.com/plugins/post.php", false, &a), EmbedDecision::Load);
        assert!(a.allows(TOP, "https://static.xx.facebook.com"), "initiators of the allowed site");
        assert!(!a.allows(TOP, "https://twitter.com"));
    }

    #[test]
    fn marker_round_trip() {
        let url = "https://e.example/p?b=2&a=1%2F#frag";
        let marked = marked_url(url, "n1", false);
        assert_eq!(marked, "https://e.example/p?b=2&a=1%2F&krunk-embed=n1#frag");
        assert_eq!(strip_marker(&marked), (url.to_string(), Some(("n1".into(), false))));
        let marked = marked_url("https://e.example/p", "n2", true);
        assert_eq!(marked, "https://e.example/p?krunk-embed=n2&krunk-embed-always=1");
        assert_eq!(strip_marker(&marked), ("https://e.example/p".to_string(), Some(("n2".into(), true))));
        // In the middle; order of the rest preserved.
        assert_eq!(
            strip_marker("https://e.example/?z=1&krunk-embed=n3&y=2&krunk-embed-always=1&x"),
            ("https://e.example/?z=1&y=2&x".to_string(), Some(("n3".into(), true)))
        );
        // No marker: unchanged (also a lone "always").
        assert_eq!(strip_marker("https://e.example/?a=1"), ("https://e.example/?a=1".into(), None));
        assert_eq!(strip_marker("https://e.example/?krunk-embed-always=1"), ("https://e.example/?krunk-embed-always=1".into(), None));
        assert_eq!(strip_marker("https://e.example/?krunk-embedx=1").1, None, "exact name only");
    }

    #[test]
    fn nonces_single_use_and_expiring() {
        let mut n = Nonces::default();
        let url = "https://platform.twitter.com/embed/Tweet.html?id=20";
        let a = n.issue(url, 1000);
        let b = n.issue(url, 1000);
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
        assert!(!n.consume("forged", url, 1001));
        assert!(!n.consume(&a, "https://other.example/", 1001), "bound to its address");
        assert!(!n.consume(&a, url, 1002), "used up by the wrong-address try");
        assert!(n.consume(&b, url, 1001));
        assert!(!n.consume(&b, url, 1002), "single use");
        let c = n.issue(url, 1000);
        assert!(!n.consume(&c, url, 1000 + NONCE_TTL_MS), "expired");
        assert!(!n.consume("", url, 1000));
        for i in 0..(MAX_NONCES as i64 + 10) {
            n.issue(url, 2000 + i);
        }
        assert!(n.len() <= MAX_NONCES);
    }

    #[test]
    fn pages_escape() {
        let url = "https://platform.twitter.com/embed/T.html?id=\"><script>x</script>&a=1";
        let html = placeholder_html(url, "abc", Some("def"));
        assert!(!html.contains("<script>x"));
        assert!(html.contains("Krunk blocked content from <b>platform.twitter.com</b>"));
        assert!(html.contains("krunk-embed=abc\"") && html.contains("krunk-embed=def&amp;krunk-embed-always=1"));
        assert!(!html.contains("src=") && !html.contains("<link"), "no external loads");
        assert!(!placeholder_html(url, "abc", None).contains("Always allow"));
        let r = redirect_html("https://e.example/?q=</script><script>alert(1)</script>\u{2028}");
        assert!(r.contains("location.replace(\"https://e.example/?q=\\u003c/script\\u003e"));
        assert_eq!(r.matches("</script>").count(), 1);
        assert!(!r.contains('\u{2028}'));
    }
}
