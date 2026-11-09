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
//! or `expired_token`, or the 10 minutes run out.
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

/// The device's name until the user renames it on the website: "Krunk 0.8.0 on Windows 11".
/// Windows 11 reports itself as 10.0 with build 22000 or later.
pub fn device_label(version: &str, windows_build: u32) -> String {
    let os = if windows_build >= 22000 { "Windows 11" } else { "Windows 10" };
    format!("Krunk {version} on {os}")
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
    /// When the code expires (ms since the epoch).
    pub expires_at_ms: i64,
    pub interval_s: u64,
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
            interval_s: a.interval.clamp(1, MAX_INTERVAL_S),
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
        match p {
            Poll::Done(t) => Next::SignedIn(t),
            Poll::Pending => wait(self),
            Poll::SlowDown => {
                self.interval_s = (self.interval_s + 5).min(MAX_INTERVAL_S);
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
    Working {
        message: String,
    },
    Linking {
        user_code: String,
        verify_url: String,
        expires_at_ms: i64,
    },
    SignedIn {
        email: String,
        device_name: String,
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
        assert_eq!(device_label("0.8.0", 26100), "Krunk 0.8.0 on Windows 11");
        assert_eq!(device_label("0.8.0", 19045), "Krunk 0.8.0 on Windows 10");
        assert_eq!(endpoint("https://krunk.209.wf/", TOKEN), "https://krunk.209.wf/api/v1/link/token");
        let b: serde_json::Value = serde_json::from_str(&authorize_body("Krunk 0.8.0 on Windows 11", "0.8.0")).unwrap();
        assert_eq!(
            b,
            serde_json::json!({"platform": "windows", "label": "Krunk 0.8.0 on Windows 11", "client_version": "0.8.0"})
        );
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
        assert!(
            matches!(l.next(Poll::Pending, 1_590_000), Next::Stop(EXPIRED_TEXT)),
            "the next poll would be after expiry"
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
        };
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            serde_json::json!({"state": "linking", "userCode": "ABCD-EFGH", "verifyUrl": "https://krunk.209.wf/link?code=ABCD-EFGH", "expiresAtMs": 5})
        );
        let s = CardState::SignedIn { email: "a@b.c".into(), device_name: "PC".into(), message: None };
        assert_eq!(
            serde_json::to_value(&s).unwrap(),
            serde_json::json!({"state": "signedIn", "email": "a@b.c", "deviceName": "PC"})
        );
        assert_eq!(serde_json::to_value(CardState::Private).unwrap(), serde_json::json!({"state": "private"}));
    }
}
