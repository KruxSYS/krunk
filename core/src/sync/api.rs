//! The sync API from the client's side (PROTOCOL §11): the transport trait every platform
//! implements (WinHTTP on Windows, `ureq` in the Linux test client, a mock in tests), how
//! answers map to errors, back-off and `Retry-After`, and the clock-skew check.

/// One HTTP answer, whatever its status.
#[derive(Debug, Clone, Default)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub retry_after: Option<String>,
    pub date: Option<String>,
}

/// The transport: `method` on `path` (below `/api/v1/`), with the bearer token and a JSON body.
/// `Err` only when no answer arrived (offline, DNS, TLS, timeout); its text never holds the
/// token or the body.
pub trait HttpClient {
    fn call(&self, method: &str, path: &str, bearer: Option<&str>, body: Option<&[u8]>)
        -> Result<HttpResponse, String>;
}

/// Why a call didn't succeed, as the engine needs to tell them apart (PLAN §5.3, §5.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    /// No answer: back off and retry.
    Offline(String),
    /// 401: the token is dead (the device was removed or signed out elsewhere).
    Unauthorized,
    /// 404, with the server's error code (`no_keyset`, `not_found` …).
    NotFound(String),
    /// 409: a conditional write lost; the body holds the current state.
    Conflict(Vec<u8>),
    /// 410: below the purge horizon: a full reconcile (PROTOCOL §10.4).
    Gone,
    /// 413: the request was too large.
    TooLarge,
    /// 423: writes are frozen until the keys are rotated (PROTOCOL §7).
    RotationPending,
    /// 429: wait this many seconds.
    RateLimited(u64),
    /// 400 `quota_exceeded`, or another 400 with its code.
    Quota(String),
    BadRequest(String),
    /// 403 with its code (`reauth_required` never reaches a device; `forbidden` may).
    Forbidden(String),
    /// 5xx or an unexpected status: back off.
    Server(u16),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::Offline(e) => write!(f, "offline ({e})"),
            ApiError::Unauthorized => f.write_str("signed out"),
            ApiError::NotFound(c) => write!(f, "not found ({c})"),
            ApiError::Conflict(_) => f.write_str("conflict"),
            ApiError::Gone => f.write_str("gone (full reconcile)"),
            ApiError::TooLarge => f.write_str("request too large"),
            ApiError::RotationPending => f.write_str("writes frozen until the keys are rotated"),
            ApiError::RateLimited(s) => write!(f, "rate limited ({s} s)"),
            ApiError::Quota(c) => write!(f, "quota ({c})"),
            ApiError::BadRequest(c) => write!(f, "bad request ({c})"),
            ApiError::Forbidden(c) => write!(f, "forbidden ({c})"),
            ApiError::Server(s) => write!(f, "server error {s}"),
        }
    }
}

impl std::error::Error for ApiError {}

/// The server's `{"error": "<code>"}`, if the body has one (never more than a short code).
fn error_code(body: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
        .filter(|c| c.len() <= 40 && c.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'))
        .unwrap_or_default()
}

/// Maps an answer: the body of a 2xx, or the error. `now_ms` turns an HTTP-date `Retry-After`
/// into seconds.
pub fn classify(r: HttpResponse, now_ms: i64) -> Result<Vec<u8>, ApiError> {
    match r.status {
        200..=299 => Ok(r.body),
        401 => Err(ApiError::Unauthorized),
        403 => Err(ApiError::Forbidden(error_code(&r.body))),
        404 => Err(ApiError::NotFound(error_code(&r.body))),
        409 => Err(ApiError::Conflict(r.body)),
        410 => Err(ApiError::Gone),
        413 => Err(ApiError::TooLarge),
        423 => Err(ApiError::RotationPending),
        429 => {
            Err(ApiError::RateLimited(retry_after_s(r.retry_after.as_deref(), r.date.as_deref(), now_ms).unwrap_or(60)))
        }
        400 => {
            let code = error_code(&r.body);
            Err(if code == "quota_exceeded" { ApiError::Quota(code) } else { ApiError::BadRequest(code) })
        }
        s => Err(ApiError::Server(s)),
    }
}

/// The longest wait a `Retry-After` can ask for (a day); longer values are clamped.
pub const MAX_RETRY_AFTER_S: u64 = 86_400;

/// `Retry-After` in seconds: either delta-seconds or an HTTP-date (relative to the answer's
/// `Date` when there is one, else to `now_ms`).
pub fn retry_after_s(value: Option<&str>, date: Option<&str>, now_ms: i64) -> Option<u64> {
    let v = value?.trim();
    if let Ok(s) = v.parse::<u64>() {
        return Some(s.min(MAX_RETRY_AFTER_S));
    }
    let at = http_date_ms(v)?;
    let base = date.and_then(http_date_ms).unwrap_or(now_ms);
    Some((((at - base).max(0) + 999) / 1000).min(MAX_RETRY_AFTER_S as i64) as u64)
}

/// An IMF-fixdate (`Sun, 06 Nov 1994 08:49:37 GMT`) as Unix milliseconds.
pub fn http_date_ms(text: &str) -> Option<i64> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    if parts.len() != 6 || parts[5] != "GMT" {
        return None;
    }
    let day: i64 = parts[1].parse().ok()?;
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let month = MONTHS.iter().position(|m| *m == parts[2])? as i64 + 1;
    let year: i64 = parts[3].parse().ok()?;
    let hms: Vec<i64> = parts[4].split(':').map(|x| x.parse().ok()).collect::<Option<_>>()?;
    if hms.len() != 3 || !(1..=31).contains(&day) || hms[0] > 23 || hms[1] > 59 || hms[2] > 60 {
        return None;
    }
    // Days from the civil date (Howard Hinnant's days_from_civil).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 24 + hms[0]) * 60 + hms[1]) * 60_000 + hms[2] * 1000)
}

/// The local clock minus the server's (`Date`), in ms; PLAN §5.6 warns above 5 minutes.
pub fn clock_skew_ms(date: Option<&str>, now_ms: i64) -> Option<i64> {
    date.and_then(http_date_ms).map(|server| now_ms - server)
}

/// Exponential back-off after network or server errors: 1, 2, 4 … 60 minutes (PLAN §5.3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Backoff {
    pub failures: u32,
}

impl Backoff {
    pub const MAX_MS: i64 = 60 * 60_000;

    /// The delay after one more failure.
    pub fn fail(&mut self) -> i64 {
        self.failures = self.failures.saturating_add(1);
        (60_000i64 << (self.failures - 1).min(6)).min(Self::MAX_MS)
    }

    pub fn reset(&mut self) {
        self.failures = 0;
    }
}

/// What the engine does next after an error (the scheduler's view of [`ApiError`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// Retry after this many ms (back-off or `Retry-After`).
    RetryIn(i64),
    /// Stop until the user signs in again.
    SignedOut,
    /// Writes frozen: keep queueing, try to rotate if eligible, check again at the normal pace.
    Frozen,
    /// A full reconcile now.
    Reconcile,
    /// Stop and show the error (quota, a bad request: a bug or a tampering server).
    Stop,
}

pub fn next_step(e: &ApiError, backoff: &mut Backoff) -> Next {
    match e {
        ApiError::Offline(_) | ApiError::Server(_) => Next::RetryIn(backoff.fail()),
        ApiError::RateLimited(s) => Next::RetryIn((*s as i64).max(1) * 1000),
        ApiError::Unauthorized => Next::SignedOut,
        ApiError::RotationPending => Next::Frozen,
        ApiError::Gone => Next::Reconcile,
        ApiError::Conflict(_)
        | ApiError::NotFound(_)
        | ApiError::TooLarge
        | ApiError::Quota(_)
        | ApiError::BadRequest(_)
        | ApiError::Forbidden(_) => Next::Stop,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A transport that answers from a script and records the requests (no network).
    pub(crate) struct MockClient {
        pub answers: RefCell<Vec<Result<HttpResponse, String>>>,
        pub seen: RefCell<Vec<(String, String, Option<String>)>>,
    }

    impl MockClient {
        pub fn new(answers: Vec<Result<HttpResponse, String>>) -> MockClient {
            MockClient { answers: RefCell::new(answers), seen: RefCell::new(Vec::new()) }
        }
    }

    impl HttpClient for MockClient {
        fn call(
            &self,
            method: &str,
            path: &str,
            bearer: Option<&str>,
            _body: Option<&[u8]>,
        ) -> Result<HttpResponse, String> {
            self.seen.borrow_mut().push((method.into(), path.into(), bearer.map(str::to_string)));
            self.answers.borrow_mut().remove(0)
        }
    }

    fn resp(status: u16, body: &str) -> HttpResponse {
        HttpResponse { status, body: body.as_bytes().to_vec(), ..Default::default() }
    }

    #[test]
    fn statuses_map_to_what_the_engine_does() {
        let now = 1_794_000_000_000;
        let client = MockClient::new(vec![
            Ok(resp(200, r#"{"head_seq":4}"#)),
            Ok(resp(401, r#"{"error":"unauthorized"}"#)),
            Ok(resp(409, r#"{"error":"conflict","head":{"seq":3}}"#)),
            Ok(resp(410, r#"{"error":"gone"}"#)),
            Ok(resp(423, r#"{"error":"rotation_pending"}"#)),
            Ok(HttpResponse { status: 429, retry_after: Some("120".into()), ..Default::default() }),
            Ok(resp(400, r#"{"error":"quota_exceeded"}"#)),
            Ok(resp(400, r#"{"error":"bad_request","message":"seq"}"#)),
            Ok(resp(404, r#"{"error":"no_keyset"}"#)),
            Ok(resp(413, "")),
            Ok(resp(503, "<html>")),
            Err("connection refused".into()),
        ]);
        let mut out = Vec::new();
        for _ in 0..12 {
            out.push(match client.call("POST", "records/batch", Some("tok"), Some(b"{}")) {
                Ok(r) => classify(r, now),
                Err(e) => Err(ApiError::Offline(e)),
            });
        }
        assert_eq!(out[0], Ok(br#"{"head_seq":4}"#.to_vec()));
        assert_eq!(out[1], Err(ApiError::Unauthorized));
        assert!(matches!(&out[2], Err(ApiError::Conflict(b)) if b.starts_with(b"{\"error\":\"conflict\"")));
        assert_eq!(out[3], Err(ApiError::Gone));
        assert_eq!(out[4], Err(ApiError::RotationPending));
        assert_eq!(out[5], Err(ApiError::RateLimited(120)));
        assert_eq!(out[6], Err(ApiError::Quota("quota_exceeded".into())));
        assert_eq!(out[7], Err(ApiError::BadRequest("bad_request".into())));
        assert_eq!(out[8], Err(ApiError::NotFound("no_keyset".into())));
        assert_eq!(out[9], Err(ApiError::TooLarge));
        assert_eq!(out[10], Err(ApiError::Server(503)));
        assert_eq!(out[11], Err(ApiError::Offline("connection refused".into())));
        assert_eq!(client.seen.borrow()[0], ("POST".into(), "records/batch".into(), Some("tok".into())));

        let mut b = Backoff::default();
        let steps: Vec<Next> = out.iter().skip(1).map(|r| next_step(r.as_ref().unwrap_err(), &mut b)).collect();
        assert_eq!(steps[0], Next::SignedOut);
        assert_eq!(steps[1], Next::Stop, "a conflict is merged by the caller, not retried blindly");
        assert_eq!(steps[2], Next::Reconcile);
        assert_eq!(steps[3], Next::Frozen);
        assert_eq!(steps[4], Next::RetryIn(120_000));
        assert_eq!(steps[9], Next::RetryIn(60_000), "the first server error: 1 minute");
        assert_eq!(steps[10], Next::RetryIn(120_000), "then 2");
    }

    #[test]
    fn backoff_doubles_to_an_hour_and_resets() {
        let mut b = Backoff::default();
        let mins: Vec<i64> = (0..9).map(|_| b.fail() / 60_000).collect();
        assert_eq!(mins, [1, 2, 4, 8, 16, 32, 60, 60, 60]);
        b.reset();
        assert_eq!(b.fail(), 60_000);
    }

    #[test]
    fn retry_after_seconds_dates_and_clamps() {
        let now = http_date_ms("Mon, 09 Nov 2026 03:00:00 GMT").unwrap();
        assert_eq!(retry_after_s(Some("30"), None, now), Some(30));
        assert_eq!(retry_after_s(Some("999999"), None, now), Some(MAX_RETRY_AFTER_S));
        assert_eq!(retry_after_s(Some("Mon, 09 Nov 2026 03:02:00 GMT"), None, now), Some(120));
        // Relative to the server's Date, not to a skewed local clock.
        assert_eq!(
            retry_after_s(
                Some("Mon, 09 Nov 2026 03:02:00 GMT"),
                Some("Mon, 09 Nov 2026 03:01:00 GMT"),
                now + 3_600_000
            ),
            Some(60)
        );
        assert_eq!(retry_after_s(Some("Mon, 09 Nov 2026 02:00:00 GMT"), None, now), Some(0));
        assert_eq!(retry_after_s(Some("soon"), None, now), None);
        assert_eq!(retry_after_s(None, None, now), None);
        // A 429 without the header waits a minute.
        assert_eq!(classify(HttpResponse { status: 429, ..Default::default() }, now), Err(ApiError::RateLimited(60)));
    }

    #[test]
    fn http_dates_and_clock_skew() {
        assert_eq!(http_date_ms("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
        assert_eq!(http_date_ms("Sun, 06 Nov 1994 08:49:37 GMT"), Some(784_111_777_000));
        assert_eq!(http_date_ms("Sun, 06 Nov 1994 08:49:37 UTC"), None);
        assert_eq!(http_date_ms("Sun, 32 Nov 1994 08:49:37 GMT"), None);
        let server = http_date_ms("Mon, 09 Nov 2026 03:00:00 GMT").unwrap();
        assert_eq!(clock_skew_ms(Some("Mon, 09 Nov 2026 03:00:00 GMT"), server + 360_000), Some(360_000));
        assert_eq!(clock_skew_ms(None, server), None);
    }

    #[test]
    fn error_codes_are_short_and_plain() {
        assert_eq!(error_code(br#"{"error":"rotation_pending","message":"x"}"#), "rotation_pending");
        assert_eq!(error_code(br#"{"error":"<script>"}"#), "");
        assert_eq!(error_code(b"not json"), "");
    }
}
