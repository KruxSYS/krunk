//! Signing Krunk in to a Krunk account (0.8.0, sync milestone S3: PROTOCOL §6.1, §11.3, §11.4).
//!
//! **Opt-in:** nothing here talks to the network by itself. The host makes a request only after
//! the user clicks **Sign in** (linking), when the Account card is shown while signed in (one
//! refresh plus `device/me`, to show the current name and notice a device removed on the
//! website), and on **Sign out**. There is no background traffic and no sync yet.
//!
//! **Linking** (RFC 8628 pattern): `link/authorize` gives a device code (kept in memory only) and
//! a user code the user confirms on `https://krunk.209.wf/link?code=…`; Krunk polls `link/token`
//! every `interval` seconds (5 s more after `slow_down`) until it gets tokens, `access_denied`
//! or `expired_token`, or the 10 minutes run out. Since 0.13.0 the answer may carry the sign-in
//! link as a QR code (`qr_svg`), which the card shows next to the code after [`qr_data_url`]
//! checked it; without one (an older server, or an SVG that fails the checks) the card shows the
//! code alone.
//!
//! **Storage:** the refresh token is kept only DPAPI-protected, in `account.json` next to the
//! other profile files (format below); the access token lives in memory only. Tokens are
//! [`Secret`]s, so they never show up in a `Debug` print or a log line.

use crate::secret::Secret;
use base64ct::{Base64, Encoding};
use serde::{Deserialize, Serialize};

pub const PLATFORM: &str = "windows";
/// The longest wait between two polls Krunk accepts from the server.
const MAX_INTERVAL_S: u64 = 60;
/// The longest a sign-in keeps waiting (one-step registration, service 2026-11-17): a code
/// that a registration from its `/link` tab attached waits on the server for the confirmation
/// mail (an hour), so while the server still answers `authorization_pending` Krunk keeps
/// polling past the code's own 10 minutes, up to this cap from the start.
pub const MAX_LINK_WAIT_S: i64 = 3600;
/// Response bodies of the account API are small.
pub const MAX_RESPONSE: usize = 64 * 1024;

pub const AUTHORIZE: &str = "/api/v1/link/authorize";
pub const TOKEN: &str = "/api/v1/link/token";
pub const REFRESH: &str = "/api/v1/token/refresh";
pub const UNLINK: &str = "/api/v1/device/unlink";
pub const ME: &str = "/api/v1/device/me";
/// The account page on the website (Manage account).
pub const ACCOUNT_PAGE: &str = "/account/";

/// `base` (the build's site address) + `path`.
pub fn endpoint(base: &str, path: &str) -> String {
    format!("{}{path}", base.trim_end_matches('/'))
}

/// The device's name until the user renames it on the website: "Krunk on Windows 11". It has no
/// version (0.11.0): the name is made once at sign-in, so a version in it went stale with the
/// first update. The card and the website show the current version next to it.
/// Windows 11 reports itself as 10.0 with build 22000 or later.
pub fn device_label(windows_build: u32) -> String {
    let os = if windows_build >= 22000 { "Windows 11" } else { "Windows 10" };
    format!("Krunk on {os}")
}

/// A name as shown: one generated before 0.11.0 ("Krunk 0.8.0 on Windows 11", exactly that
/// pattern) loses its stale version; anything else, such as a name the user typed, stays as it
/// is. The server does the same (`server/src/devices.rs::display_label`).
pub fn display_label(label: &str) -> String {
    fn version(v: &str) -> bool {
        let (core, pre) = match v.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (v, None),
        };
        !core.is_empty()
            && core.split('.').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
            && pre
                .is_none_or(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_'))
    }
    let generated = label
        .strip_prefix("Krunk ")
        .and_then(|rest| rest.split_once(" on "))
        .filter(|(v, os)| version(v) && matches!(*os, "Windows 10" | "Windows 11" | "Android"));
    match generated {
        Some((_, os)) => format!("Krunk on {os}"),
        None => label.to_string(),
    }
}

/// The body of `link/authorize`.
pub fn authorize_body(label: &str, version: &str) -> String {
    serde_json::json!({ "platform": PLATFORM, "label": label, "client_version": version }).to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct Authorized {
    pub device_code: Secret,
    pub user_code: String,
    pub verification_uri_complete: String,
    pub expires_in: u64,
    pub interval: u64,
    /// The sign-in link as a QR code, an SVG document (0.13.0; older servers send none).
    #[serde(default)]
    pub qr_svg: Option<String>,
}

/// The largest QR SVG the card shows.
pub const MAX_QR_SVG: usize = 64 * 1024;

/// The server's QR SVG as a `data:image/svg+xml;base64,…` URL for the card's `<img>` (0.13.0),
/// or `None` (the card shows the code alone). The page loads it as an image, where SVG can't run
/// scripts or fetch anything; still, only what the server's QR renderer makes is accepted: at
/// most [`MAX_QR_SVG`], an `<svg>` root after an optional XML declaration, and none of script,
/// event attributes, links, styles, external images, `<use>`, entities or a DOCTYPE.
pub fn qr_data_url(svg: &str) -> Option<String> {
    if svg.is_empty() || svg.len() > MAX_QR_SVG {
        return None;
    }
    let lower = svg.to_ascii_lowercase();
    let mut rest = lower.trim_start();
    if let Some(decl) = rest.strip_prefix("<?xml") {
        if !decl.starts_with(|c: char| c.is_ascii_whitespace()) {
            return None;
        }
        let (decl, after) = decl.split_once("?>")?;
        if decl.contains(['<', '>', '&']) {
            return None;
        }
        rest = after.trim_start();
    }
    let root = rest.strip_prefix("<svg")?;
    if !root.starts_with(|c: char| c.is_ascii_whitespace() || c == '>') || !root.trim_end().ends_with("</svg>") {
        return None;
    }
    const REFUSED: [&str; 12] = [
        "<script",
        "foreignobject",
        "href",
        "<!entity",
        "<!doctype",
        "<!",
        "<?",
        "<style",
        "url(",
        "<image",
        "<use",
        // Character references could spell any of the above inside an attribute value; the QR
        // renderer never writes one.
        "&",
    ];
    if REFUSED.iter().any(|bad| rest.contains(bad)) || has_event_attribute(rest) {
        return None;
    }
    Some(format!("data:image/svg+xml;base64,{}", Base64::encode_string(svg.as_bytes())))
}

/// An `on…=` attribute (`onload=`, `onclick =`): `on` and a name right after whitespace, a quote
/// or `/`, then `=`. `lower` is lower case.
fn has_event_attribute(lower: &str) -> bool {
    let b = lower.as_bytes();
    (1..b.len()).any(|i| {
        let after_gap = matches!(b[i - 1], b' ' | b'\t' | b'\n' | b'\r' | b'"' | b'\'' | b'/');
        if !after_gap || !b[i..].starts_with(b"on") {
            return false;
        }
        let name_end = b[i + 2..]
            .iter()
            .position(|c| !(c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b':' | b'.')))
            .map_or(b.len(), |p| i + 2 + p);
        name_end > i + 2 && b[name_end..].iter().find(|c| !c.is_ascii_whitespace()) == Some(&b'=')
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tokens {
    pub device_id: String,
    pub account_id: String,
    pub access_token: Secret,
    pub refresh_token: Secret,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Refreshed {
    pub access_token: Secret,
    pub refresh_token: Secret,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeviceMe {
    pub email: String,
    pub account_id: String,
    pub device_id: String,
    pub label: String,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    error: String,
}

/// Why a request didn't give what was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Failure {
    /// No answer (no network, DNS, TLS, timeout).
    Offline,
    /// 404: this server has no sign-in yet.
    NotAvailable,
    /// 503 `accounts_disabled`.
    Closed,
    /// 429.
    RateLimited,
    /// 401: the device is no longer signed in (removed on the website, signed out elsewhere).
    SignedOut,
    /// Anything else (5xx, a malformed answer).
    Server,
}

impl Failure {
    /// The one sentence the Account card shows.
    pub fn text(self) -> &'static str {
        match self {
            Failure::Offline => "Krunk couldn't reach krunk.209.wf. Check your connection and try again.",
            Failure::NotAvailable => "Signing in isn't available on krunk.209.wf yet. Try again with a later version.",
            Failure::Closed => "Accounts are closed on krunk.209.wf at the moment. Try again later.",
            Failure::RateLimited => "Too many attempts. Wait a few minutes and try again.",
            Failure::SignedOut => "This device was signed out of your account (for example, removed on the website).",
            Failure::Server => "Something went wrong on krunk.209.wf. Try again later.",
        }
    }
}

/// An HTTP answer, or the transport's error text.
pub type Answer = Result<(u32, Vec<u8>), String>;

fn error_code(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<ApiError>(body).ok().map(|e| e.error)
}

/// Any answer that isn't the expected success.
pub fn failure(answer: &Answer) -> Failure {
    match answer {
        Err(_) => Failure::Offline,
        Ok((404, _)) => Failure::NotAvailable,
        Ok((503, body)) if error_code(body).as_deref() == Some("accounts_disabled") => Failure::Closed,
        Ok((429, _)) => Failure::RateLimited,
        Ok((401, _)) => Failure::SignedOut,
        Ok(_) => Failure::Server,
    }
}

/// A 200 answer parsed as `T`.
pub fn parse<T: for<'de> Deserialize<'de>>(answer: &Answer) -> Result<T, Failure> {
    match answer {
        Ok((200, body)) => serde_json::from_slice(body).map_err(|_| Failure::Server),
        other => Err(failure(other)),
    }
}

/// What a `link/token` poll said.
#[derive(Debug)]
pub enum Poll {
    Pending,
    SlowDown,
    Expired,
    Denied,
    Done(Tokens),
    Failed(Failure),
}

pub fn poll_answer(answer: &Answer) -> Poll {
    match answer {
        Ok((200, body)) => match serde_json::from_slice::<Tokens>(body) {
            Ok(t) => Poll::Done(t),
            Err(_) => Poll::Failed(Failure::Server),
        },
        Ok((400, body)) => match error_code(body).as_deref() {
            Some("authorization_pending") => Poll::Pending,
            Some("slow_down") => Poll::SlowDown,
            Some("expired_token") => Poll::Expired,
            Some("access_denied") => Poll::Denied,
            _ => Poll::Failed(Failure::Server),
        },
        other => Poll::Failed(failure(other)),
    }
}

/// A sign-in waiting for the approval on the website.
#[derive(Debug, Clone)]
pub struct Linking {
    pub device_code: Secret,
    pub user_code: String,
    pub verify_url: String,
    /// When the code expires (ms since the epoch). Moved on to `give_up_at_ms` once the server
    /// still answers `authorization_pending` after it (the code waits for a registration).
    pub expires_at_ms: i64,
    /// Polling ends here whatever the server says ([`MAX_LINK_WAIT_S`] after the start).
    pub give_up_at_ms: i64,
    pub interval_s: u64,
    /// The sign-in link as a QR code, a checked `data:` URL (0.13.0).
    pub qr: Option<String>,
}

/// What the host does after a poll.
#[derive(Debug)]
pub enum Next {
    /// Poll again in this many milliseconds.
    Poll(u64),
    /// Signed in.
    SignedIn(Tokens),
    /// The sign-in is over without an account; the card says why.
    Stop(&'static str),
}

pub const EXPIRED_TEXT: &str = "The code expired before it was approved. Sign in again for a new one.";
pub const DENIED_TEXT: &str = "The sign-in was denied on the website.";

impl Linking {
    /// From a `link/authorize` answer, at `now_ms`. The server's interval is used (5 s), within
    /// 1..60 s; the code's lifetime as given. `verify_base`: the page to open must be on this
    /// site (the build's own), whatever the answer says; `None` (test builds against a staging
    /// stack, whose site has another name) accepts any https address.
    pub fn start(a: Authorized, verify_base: Option<&str>, now_ms: i64) -> Result<Linking, Failure> {
        let ok = match verify_base {
            Some(base) => a.verification_uri_complete.starts_with(&endpoint(base, "/link?code=")),
            None => {
                a.verification_uri_complete.starts_with("https://")
                    && a.verification_uri_complete.contains("/link?code=")
            }
        };
        if !ok || a.user_code.len() > 12 {
            return Err(Failure::Server);
        }
        Ok(Linking {
            device_code: a.device_code,
            verify_url: a.verification_uri_complete,
            user_code: a.user_code,
            expires_at_ms: now_ms + (a.expires_in.min(3600) as i64) * 1000,
            give_up_at_ms: now_ms + MAX_LINK_WAIT_S * 1000,
            interval_s: a.interval.clamp(1, MAX_INTERVAL_S),
            qr: a.qr_svg.as_deref().and_then(qr_data_url),
        })
    }

    pub fn poll_body(&self) -> String {
        serde_json::json!({ "device_code": &*self.device_code }).to_string()
    }

    /// The next step after a poll answered `p` at `now_ms`.
    pub fn next(&mut self, p: Poll, now_ms: i64) -> Next {
        let wait = |s: &Linking| {
            if now_ms + (s.interval_s as i64) * 1000 > s.expires_at_ms {
                Next::Stop(EXPIRED_TEXT)
            } else {
                Next::Poll(s.interval_s * 1000)
            }
        };
        // The server is the judge of the code's lifetime: "pending" after the local deadline means
        // it waits longer (a registration attached it), so wait on, up to the cap.
        let extend = |s: &mut Linking| {
            let next_at = now_ms + (s.interval_s as i64) * 1000;
            if next_at > s.expires_at_ms && next_at <= s.give_up_at_ms {
                s.expires_at_ms = s.give_up_at_ms;
            }
        };
        match p {
            Poll::Done(t) => Next::SignedIn(t),
            Poll::Pending => {
                extend(self);
                wait(self)
            }
            Poll::SlowDown => {
                self.interval_s = (self.interval_s + 5).min(MAX_INTERVAL_S);
                extend(self);
                wait(self)
            }
            Poll::Expired => Next::Stop(EXPIRED_TEXT),
            Poll::Denied => Next::Stop(DENIED_TEXT),
            // A network hiccup while waiting: keep polling until the code expires.
            Poll::Failed(Failure::Offline | Failure::Server | Failure::RateLimited) => wait(self),
            Poll::Failed(f) => Next::Stop(f.text()),
        }
    }
}

/// `account.json` (version 1): who this device is signed in as, and the refresh token wrapped
/// by DPAPI (base64). The access token is never stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountFile {
    pub v: u32,
    pub account_id: String,
    pub device_id: String,
    pub email: String,
    pub label: String,
    pub linked_at: i64,
    /// DPAPI(refresh token), base64.
    pub refresh: String,
}

impl AccountFile {
    pub fn new(
        email: &str,
        label: &str,
        account_id: &str,
        device_id: &str,
        wrapped_refresh: &[u8],
        now_ms: i64,
    ) -> Self {
        AccountFile {
            v: 1,
            account_id: account_id.into(),
            device_id: device_id.into(),
            email: email.into(),
            label: label.into(),
            linked_at: now_ms,
            refresh: Base64::encode_string(wrapped_refresh),
        }
    }

    pub fn parse(bytes: &[u8]) -> Option<AccountFile> {
        serde_json::from_slice::<AccountFile>(bytes).ok().filter(|f| f.v == 1)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec_pretty(self).expect("plain fields")
    }

    /// After a rotation: the new wrapped refresh token.
    pub fn set_wrapped_refresh(&mut self, wrapped: &[u8]) {
        self.refresh = Base64::encode_string(wrapped);
    }

    pub fn wrapped_refresh(&self) -> Option<Vec<u8>> {
        Base64::decode_vec(&self.refresh).ok()
    }
}

/// What the Settings card and the ⋮ menu show (sent to Settings pages as the `account` event).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum CardState {
    SignedOut {
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    /// Waiting for a server answer (starting, signing out).
    Working { message: String },
    Linking {
        user_code: String,
        verify_url: String,
        expires_at_ms: i64,
        /// The sign-in link as a QR code (`data:image/svg+xml;base64,…`, 0.13.0).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        qr: Option<String>,
    },
    SignedIn {
        email: String,
        device_name: String,
        /// The running app's version, shown next to the name (0.11.0).
        version: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    /// A private window shows nothing of the account.
    Private,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authorized() -> Authorized {
        serde_json::from_str(
            r#"{"device_code":"dc-secret","user_code":"ABCD-EFGH","verification_uri":"https://krunk.209.wf/link",
               "verification_uri_complete":"https://krunk.209.wf/link?code=ABCD-EFGH","expires_in":600,"interval":5}"#,
        )
        .unwrap()
    }

    #[test]
    fn labels_and_endpoints() {
        assert_eq!(device_label(26100), "Krunk on Windows 11");
        assert_eq!(device_label(19045), "Krunk on Windows 10");
        assert_eq!(endpoint("https://krunk.209.wf/", TOKEN), "https://krunk.209.wf/api/v1/link/token");
        let b: serde_json::Value = serde_json::from_str(&authorize_body("Krunk on Windows 11", "0.11.0")).unwrap();
        assert_eq!(
            b,
            serde_json::json!({"platform": "windows", "label": "Krunk on Windows 11", "client_version": "0.11.0"})
        );
    }

    #[test]
    fn old_generated_names_lose_their_version_and_typed_ones_stay() {
        for (label, shown) in [
            ("Krunk 0.8.0 on Windows 11", "Krunk on Windows 11"),
            ("Krunk 0.10.2 on Windows 10", "Krunk on Windows 10"),
            ("Krunk 1.0.0-rc.1 on Windows 11", "Krunk on Windows 11"),
            ("Krunk on Windows 11", "Krunk on Windows 11"),
            ("Krunk 0.8.0 on Windows 11 (work)", "Krunk 0.8.0 on Windows 11 (work)"),
            ("My Krunk 0.8.0 on Windows 11", "My Krunk 0.8.0 on Windows 11"),
            ("Krunk 0.8.x on Windows 11", "Krunk 0.8.x on Windows 11"),
            ("Krunk 0.8.0 on Linux", "Krunk 0.8.0 on Linux"),
            ("Living room PC", "Living room PC"),
            ("Krunk  on Windows 11", "Krunk  on Windows 11"),
        ] {
            assert_eq!(display_label(label), shown, "{label}");
        }
    }

    #[test]
    fn every_server_answer() {
        let ok = |s: u32, b: &str| -> Answer { Ok((s, b.as_bytes().to_vec())) };
        assert!(matches!(poll_answer(&ok(400, r#"{"error":"authorization_pending","message":"x"}"#)), Poll::Pending));
        assert!(matches!(poll_answer(&ok(400, r#"{"error":"slow_down"}"#)), Poll::SlowDown));
        assert!(matches!(poll_answer(&ok(400, r#"{"error":"expired_token"}"#)), Poll::Expired));
        assert!(matches!(poll_answer(&ok(400, r#"{"error":"access_denied"}"#)), Poll::Denied));
        assert!(matches!(poll_answer(&ok(400, r#"{"error":"bad_request"}"#)), Poll::Failed(Failure::Server)));
        let t = r#"{"device_id":"d","account_id":"a","access_token":"at","expires_in":900,"refresh_token":"rt"}"#;
        match poll_answer(&ok(200, t)) {
            Poll::Done(t) => assert_eq!((&*t.access_token, &*t.refresh_token, t.device_id.as_str()), ("at", "rt", "d")),
            other => panic!("{other:?}"),
        }
        assert!(matches!(poll_answer(&ok(200, "{}")), Poll::Failed(Failure::Server)));
        assert_eq!(failure(&Err("could not connect".into())), Failure::Offline);
        assert_eq!(failure(&ok(404, r#"{"error":"not_found"}"#)), Failure::NotAvailable);
        assert_eq!(failure(&ok(404, "<html>")), Failure::NotAvailable, "an old server's 404 page");
        assert_eq!(failure(&ok(503, r#"{"error":"accounts_disabled"}"#)), Failure::Closed);
        assert_eq!(failure(&ok(503, "")), Failure::Server);
        assert_eq!(failure(&ok(429, "")), Failure::RateLimited);
        assert_eq!(failure(&ok(401, r#"{"error":"unauthorized"}"#)), Failure::SignedOut);
        assert_eq!(failure(&ok(502, "")), Failure::Server);
        let me: DeviceMe = parse(&ok(
            200,
            r#"{"email":"a@b.c","account_id":"a","device_id":"d","label":"PC","platform":"windows","created":"x"}"#,
        ))
        .unwrap();
        assert_eq!(me.email, "a@b.c");
        assert_eq!(parse::<DeviceMe>(&ok(401, "")).unwrap_err(), Failure::SignedOut);
        for f in [
            Failure::Offline,
            Failure::NotAvailable,
            Failure::Closed,
            Failure::RateLimited,
            Failure::SignedOut,
            Failure::Server,
        ] {
            assert!(f.text().ends_with('.') && f.text().len() < 120);
        }
    }

    #[test]
    fn linking_waits_slows_down_and_expires() {
        let mut l = Linking::start(authorized(), Some("https://krunk.209.wf"), 1_000_000).unwrap();
        assert_eq!((l.user_code.as_str(), l.interval_s, l.expires_at_ms), ("ABCD-EFGH", 5, 1_600_000));
        assert!(matches!(l.next(Poll::Pending, 1_005_000), Next::Poll(5000)));
        assert!(matches!(l.next(Poll::SlowDown, 1_010_000), Next::Poll(10000)));
        assert!(matches!(l.next(Poll::SlowDown, 1_020_000), Next::Poll(15000)));
        assert!(
            matches!(l.next(Poll::Failed(Failure::Offline), 1_030_000), Next::Poll(15000)),
            "a hiccup: keep waiting"
        );
        assert!(matches!(l.next(Poll::Expired, 1_100_000), Next::Stop(EXPIRED_TEXT)));
        assert!(matches!(l.next(Poll::Denied, 1_100_000), Next::Stop(DENIED_TEXT)));
        assert!(matches!(l.next(Poll::Failed(Failure::NotAvailable), 1_100_000), Next::Stop(_)));
        let body: serde_json::Value = serde_json::from_str(&l.poll_body()).unwrap();
        assert_eq!(body["device_code"], "dc-secret");
        assert!(!format!("{l:?}").contains("dc-secret"), "the device code is never printed");
        // A server that asks for silly intervals is clamped.
        let mut a = authorized();
        a.interval = 0;
        assert_eq!(Linking::start(a, Some("https://krunk.209.wf"), 0).unwrap().interval_s, 1);
        let mut a = authorized();
        a.interval = 999;
        let mut l = Linking::start(a, Some("https://krunk.209.wf"), 0).unwrap();
        assert!(matches!(l.next(Poll::SlowDown, 0), Next::Poll(60000)));
    }

    #[test]
    fn linking_waits_past_the_code_while_the_server_says_pending() {
        let start = 1_000_000;
        let mut l = Linking::start(authorized(), Some("https://krunk.209.wf"), start).unwrap();
        assert_eq!(l.give_up_at_ms, start + MAX_LINK_WAIT_S * 1000);
        // A hiccup near the code's end doesn't stretch it: only the server's "pending" does.
        assert!(matches!(l.next(Poll::Failed(Failure::Offline), 1_598_000), Next::Stop(EXPIRED_TEXT)));
        let mut l = Linking::start(authorized(), Some("https://krunk.209.wf"), start).unwrap();
        assert!(matches!(l.next(Poll::Pending, 1_598_000), Next::Poll(5000)), "past the 10 minutes: wait on");
        assert_eq!(l.expires_at_ms, l.give_up_at_ms);
        assert!(matches!(l.next(Poll::Pending, 3_000_000), Next::Poll(5000)));
        assert!(matches!(l.next(Poll::Failed(Failure::Server), 3_100_000), Next::Poll(5000)));
        assert!(matches!(l.next(Poll::SlowDown, 3_200_000), Next::Poll(10000)));
        // The server ends it as soon as the code is gone or denied ...
        assert!(matches!(l.clone().next(Poll::Expired, 3_300_000), Next::Stop(EXPIRED_TEXT)));
        assert!(matches!(l.clone().next(Poll::Denied, 3_300_000), Next::Stop(DENIED_TEXT)));
        // ... and the cap an hour after the start, whatever it says.
        assert!(matches!(l.next(Poll::Pending, start + 3_595_000), Next::Stop(EXPIRED_TEXT)));
    }

    #[test]
    fn the_page_to_open_is_the_builds_own_site() {
        let mut a = authorized();
        a.verification_uri_complete = "https://evil.example/link?code=ABCD-EFGH".into();
        assert_eq!(Linking::start(a, Some("https://krunk.209.wf"), 0).unwrap_err(), Failure::Server);
        let mut a = authorized();
        a.verification_uri_complete = "https://krunk.209.wf.evil.example/link?code=X".into();
        assert!(Linking::start(a, Some("https://krunk.209.wf"), 0).is_err());
        let a = authorized();
        assert!(Linking::start(a.clone(), Some("https://krunk.209.wf/"), 0).is_ok());
        // A test build against staging: any https page with a code.
        let mut staging = a;
        staging.verification_uri_complete = "https://krunk.test/link?code=ABCD-EFGH".into();
        assert!(Linking::start(staging.clone(), None, 0).is_ok());
        assert!(Linking::start(staging.clone(), Some("https://krunk.209.wf"), 0).is_err());
        staging.verification_uri_complete = "http://krunk.test/link?code=ABCD-EFGH".into();
        assert!(Linking::start(staging, None, 0).is_err(), "never a plain http page");
    }

    #[test]
    fn account_file_round_trip() {
        let f = AccountFile::new("a@b.c", "Krunk 0.8.0 on Windows 11", "acc", "dev", &[1, 2, 3, 250], 42);
        let bytes = f.to_bytes();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.contains("\"v\": 1") && text.contains("AQID+g=="), "{text}");
        let back = AccountFile::parse(&bytes).unwrap();
        assert_eq!(back, f);
        assert_eq!(back.wrapped_refresh().unwrap(), vec![1, 2, 3, 250]);
        let mut rotated = back.clone();
        rotated.set_wrapped_refresh(&[9, 9]);
        assert_eq!((rotated.wrapped_refresh().unwrap(), rotated.email.as_str()), (vec![9, 9], "a@b.c"));
        assert!(AccountFile::parse(b"{}").is_none());
        assert!(
            AccountFile::parse(text.replace("\"v\": 1", "\"v\": 2").as_bytes()).is_none(),
            "a newer format is not guessed at"
        );
        assert!(AccountFile::parse(text.replace("\"v\": 1", "\"v\": 1, \"access\": \"x\"").as_bytes()).is_none());
    }

    #[test]
    fn card_states_serialize_for_the_page() {
        let s = CardState::Linking {
            user_code: "ABCD-EFGH".into(),
            verify_url: "https://krunk.209.wf/link?code=ABCD-EFGH".into(),
            expires_at_ms: 5,
            qr: None,
        };
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            serde_json::json!({"state": "linking", "userCode": "ABCD-EFGH", "verifyUrl": "https://krunk.209.wf/link?code=ABCD-EFGH", "expiresAtMs": 5})
        );
        let s = CardState::SignedIn {
            email: "a@b.c".into(),
            device_name: "PC".into(),
            version: "0.11.0".into(),
            message: None,
        };
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            serde_json::json!({"state": "signedIn", "email": "a@b.c", "deviceName": "PC", "version": "0.11.0"})
        );
        assert_eq!(serde_json::to_value(CardState::Private).unwrap(), serde_json::json!({"state": "private"}));
        // 0.13.0: the QR only when there is one.
        let s = CardState::Linking {
            user_code: "ABCD-EFGH".into(),
            verify_url: "https://krunk.209.wf/link?code=ABCD-EFGH".into(),
            expires_at_ms: 5,
            qr: Some("data:image/svg+xml;base64,PHN2Zz4=".into()),
        };
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            serde_json::json!({"state": "linking", "userCode": "ABCD-EFGH", "verifyUrl": "https://krunk.209.wf/link?code=ABCD-EFGH", "expiresAtMs": 5, "qr": "data:image/svg+xml;base64,PHN2Zz4="})
        );
    }

    /// What the server's renderer makes (`qrcode` 0.14 `svg::Color`, `min_dimensions(200, 200)`,
    /// quiet zone), with a made-up module pattern.
    fn server_qr_svg() -> String {
        let mut d = String::new();
        for y in 0..29u32 {
            for x in (0..29u32).filter(|x| (x * 7 + y * 3) % 5 < 2) {
                d.push_str(&format!("M{l} {t}h7v7H{l}V{t}", l = 28 + x * 7, t = 28 + y * 7));
            }
        }
        format!(
            concat!(
                r#"<?xml version="1.0" standalone="yes"?>"#,
                r#"<svg xmlns="http://www.w3.org/2000/svg" version="1.1" width="259" height="259""#,
                r#" viewBox="0 0 259 259" shape-rendering="crispEdges">"#,
                r##"<rect x="0" y="0" width="259" height="259" fill="#fff"/><path fill="#000" d="{d}"/></svg>"##
            ),
            d = d
        )
    }

    #[test]
    fn the_qr_is_the_servers_svg_or_nothing() {
        // An older server: no QR, the code alone (and the rest exactly as before).
        let a = authorized();
        assert!(a.qr_svg.is_none());
        let l = Linking::start(a, Some("https://krunk.209.wf"), 0).unwrap();
        assert!(l.qr.is_none());

        let svg = server_qr_svg();
        let url = qr_data_url(&svg).expect("the server's own QR");
        let b64 = url.strip_prefix("data:image/svg+xml;base64,").unwrap();
        assert_eq!(Base64::decode_vec(b64).unwrap(), svg.as_bytes());
        // Through the authorize answer to the card.
        let body = serde_json::json!({
            "device_code": "dc", "user_code": "ABCD-EFGH", "verification_uri": "https://krunk.209.wf/link",
            "verification_uri_complete": "https://krunk.209.wf/link?code=ABCD-EFGH", "expires_in": 600,
            "interval": 5, "qr_svg": svg,
        });
        let a: Authorized = serde_json::from_value(body).unwrap();
        let l = Linking::start(a, Some("https://krunk.209.wf"), 0).unwrap();
        assert_eq!(l.qr.as_deref(), Some(url.as_str()));
        // Without the declaration, with whitespace around, too.
        let bare = svg.split_once("?>").unwrap().1;
        assert!(qr_data_url(bare).is_some());
        assert!(qr_data_url(&format!("\n  {bare}\n")).is_some());

        let inject = |what: &str| svg.replacen("<rect ", &format!("{what}<rect "), 1);
        let attr = |what: &str| svg.replacen("<rect ", &format!("<rect {what} "), 1);
        let too_big = svg.replacen(r#"d=""#, &format!(r#"d="{}"#, "M0 0h1v1H0V0".repeat(6000)), 1);
        assert!(too_big.len() > MAX_QR_SVG);
        for (why, bad) in [
            ("script", inject("<script>alert(1)</script>")),
            ("script, upper case", inject("<SCRIPT>alert(1)</SCRIPT>")),
            ("foreignObject", inject("<foreignObject><p>x</p></foreignObject>")),
            ("href", attr(r#"href="https://evil.example/""#)),
            ("xlink:href", inject(r#"<a xlink:href="javascript:alert(1)">x</a>"#)),
            ("onload", svg.replacen("<svg ", r#"<svg onload="alert(1)" "#, 1)),
            ("onclick, spaced and upper case", attr(r#"ONCLICK = "x""#)),
            ("onmouseover after a quote", svg.replacen(r##"fill="#fff""##, r##"fill="#fff"onmouseover="x""##, 1)),
            ("DOCTYPE with ENTITY", svg.replacen("?>", r#"?><!DOCTYPE svg [<!ENTITY x "y">]>"#, 1)),
            ("ENTITY", inject(r#"<!ENTITY x "y">"#)),
            ("character reference", attr(r#"fill="&#x75;rl(#a)""#)),
            ("style", inject("<style>rect{fill:red}</style>")),
            ("url(", attr(r#"fill="url(https://evil.example/#a)""#)),
            ("image", inject(r#"<image width="9" height="9"/>"#)),
            ("use", inject(r##"<use x="1"/>"##)),
            ("a second processing instruction", svg.replacen("?>", r#"?><?xml-stylesheet type="text/css"?>"#, 1)),
            ("too large", too_big),
            ("not svg", "<html><body>x</body></html>".into()),
            ("svg not at the root", format!("<g>{bare}</g>")),
            ("svgx root", bare.replacen("<svg ", "<svgx ", 1)),
            ("unclosed", bare.trim_end_matches("</svg>").to_string()),
            ("text before the root", format!("hello{bare}")),
            ("empty", String::new()),
        ] {
            assert_eq!(qr_data_url(&bad), None, "{why}");
            // The sign-in still works, with the code alone.
            let mut a = authorized();
            a.qr_svg = Some(bad);
            let l = Linking::start(a, Some("https://krunk.209.wf"), 0).unwrap();
            assert!(l.qr.is_none(), "{why}");
        }
        // Words that merely contain "on" are fine.
        assert!(qr_data_url(&attr(r#"data-version="1""#)).is_some());
        assert!(qr_data_url(&attr(r#"class="icon one""#)).is_some());
    }
}
