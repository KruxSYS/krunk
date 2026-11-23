//! Cloudflare Turnstile server-side validation (docs/sync/PLAN.md §4.5).
//!
//! A token is accepted only if all hold: it is 1–2048 characters; it wasn't presented before
//! (SHA-256 in `turnstile_seen` for 10 minutes); siteverify says `success`; `hostname` and
//! `action` are the expected ones; `challenge_ts` is at most 300 s old. siteverify gets the
//! secret, the token, the trusted client IP and an idempotency key that is reused only for
//! retries of the same validation (3 attempts, 3 s each). If Cloudflare can't be reached, the
//! request fails closed with 503. Error codes are logged; tokens never are.
//!
//! **Test mode:** with one of Cloudflare's documented test secrets (staging), siteverify
//! answers `hostname: "example.com"` with no action and accepts the same dummy token every
//! time. Then only `success` is checked and the replay cache is skipped. A warning is logged at
//! start; the test secrets can never accept a real visitor's widget.

use crate::api::ApiError;
use crate::crypto;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::PgPool;
use std::net::IpAddr;
use std::time::Duration;

pub const SITEVERIFY_URL: &str = "https://challenges.cloudflare.com/turnstile/v0/siteverify";
pub const MAX_TOKEN_LEN: usize = 2048;
pub const MAX_AGE_S: i64 = 300;
const ATTEMPTS: u32 = 3;
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Default, Deserialize)]
pub struct SiteverifyResponse {
    #[serde(default)]
    pub success: bool,
    pub hostname: Option<String>,
    pub action: Option<String>,
    pub challenge_ts: Option<String>,
    #[serde(rename = "error-codes", default)]
    pub error_codes: Vec<String>,
    #[serde(default)]
    pub metadata: Metadata,
}

#[derive(Debug, Default, Deserialize)]
pub struct Metadata {
    #[serde(default)]
    pub result_with_testing_key: bool,
}

#[derive(Clone)]
pub struct Turnstile {
    client: reqwest::Client,
    url: String,
    secret: String,
    hostname: String,
    test_mode: bool,
}

pub fn failed() -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "turnstile_failed", "Please complete the check again.")
}

fn unavailable() -> ApiError {
    ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "server_error", "The check is unavailable. Please try again later.")
}

/// The checks after a successful siteverify call; `Err(reason)` for the log.
pub fn check_response(
    r: &SiteverifyResponse,
    action: &str,
    hostname: &str,
    test_mode: bool,
    now: DateTime<Utc>,
) -> Result<(), &'static str> {
    if !r.success {
        return Err("not successful");
    }
    if test_mode {
        return Ok(());
    }
    if r.metadata.result_with_testing_key {
        return Err("a testing-key result with a production secret");
    }
    if r.hostname.as_deref() != Some(hostname) {
        return Err("hostname mismatch");
    }
    if r.action.as_deref() != Some(action) {
        return Err("action mismatch");
    }
    let ts = r
        .challenge_ts
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .ok_or("no challenge_ts")?
        .with_timezone(&Utc);
    let age = (now - ts).num_seconds();
    if age > MAX_AGE_S {
        return Err("challenge too old");
    }
    if age < -60 {
        return Err("challenge from the future");
    }
    Ok(())
}

impl Turnstile {
    pub fn new(secret: &str, hostname: &str, test_mode: bool) -> Result<Turnstile, String> {
        Turnstile::with_url(SITEVERIFY_URL, secret, hostname, test_mode)
    }

    pub fn with_url(url: &str, secret: &str, hostname: &str, test_mode: bool) -> Result<Turnstile, String> {
        crypto::ensure_tls_provider();
        let client = reqwest::Client::builder()
            .timeout(ATTEMPT_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| format!("HTTP client: {e}"))?;
        Ok(Turnstile { client, url: url.into(), secret: secret.into(), hostname: hostname.into(), test_mode })
    }

    /// One validation: the siteverify call with its retries.
    pub async fn siteverify(&self, token: &str, ip: IpAddr) -> Result<SiteverifyResponse, ApiError> {
        let idempotency_key = uuid::Uuid::new_v4().to_string();
        let ip = ip.to_string();
        let form = [
            ("secret", self.secret.as_str()),
            ("response", token),
            ("remoteip", ip.as_str()),
            ("idempotency_key", idempotency_key.as_str()),
        ];
        for attempt in 1..=ATTEMPTS {
            match self.client.post(&self.url).form(&form).send().await {
                Ok(res) if res.status().is_success() => match res.json::<SiteverifyResponse>().await {
                    Ok(r) => return Ok(r),
                    Err(e) => tracing::warn!(attempt, error = %e, "turnstile: unreadable siteverify response"),
                },
                Ok(res) => tracing::warn!(attempt, status = res.status().as_u16(), "turnstile: siteverify status"),
                Err(e) => tracing::warn!(attempt, timeout = e.is_timeout(), "turnstile: siteverify unreachable"),
            }
            if attempt < ATTEMPTS {
                tokio::time::sleep(Duration::from_millis(250 * 3u64.pow(attempt - 1))).await;
            }
        }
        Err(unavailable())
    }

    /// Validates `token` for `action` from `ip`; `turnstile_failed` or a 503.
    pub async fn verify(&self, db: &PgPool, token: &str, action: &'static str, ip: IpAddr) -> Result<(), ApiError> {
        if token.is_empty() || token.len() > MAX_TOKEN_LEN {
            tracing::info!(action, "turnstile: missing or oversized token");
            return Err(failed());
        }
        if !self.test_mode {
            let fresh = sqlx::query(
                "INSERT INTO turnstile_seen (hash, expires_at) VALUES ($1, now() + interval '10 minutes')
                 ON CONFLICT (hash) DO NOTHING",
            )
            .bind(crypto::sha256(token.as_bytes()).as_slice())
            .execute(db)
            .await?
            .rows_affected()
                == 1;
            if !fresh {
                tracing::info!(action, "turnstile: token presented twice");
                return Err(failed());
            }
        }
        let r = self.siteverify(token, ip).await?;
        match check_response(&r, action, &self.hostname, self.test_mode, Utc::now()) {
            Ok(()) => Ok(()),
            Err(reason) => {
                tracing::info!(action, reason, error_codes = ?r.error_codes, "turnstile: rejected");
                Err(failed())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::post;
    use std::sync::{Arc, Mutex};

    fn ok_response(action: &str, hostname: &str, ts: DateTime<Utc>) -> SiteverifyResponse {
        SiteverifyResponse {
            success: true,
            hostname: Some(hostname.into()),
            action: Some(action.into()),
            challenge_ts: Some(ts.to_rfc3339()),
            ..Default::default()
        }
    }

    #[test]
    fn acceptance_rules() {
        let now = Utc::now();
        let h = "krunk.kruxsys.com";
        assert_eq!(check_response(&ok_response("login", h, now), "login", h, false, now), Ok(()));
        assert!(check_response(&SiteverifyResponse::default(), "login", h, false, now).is_err());
        assert_eq!(check_response(&ok_response("register", h, now), "login", h, false, now), Err("action mismatch"));
        assert_eq!(
            check_response(&ok_response("login", "evil.example", now), "login", h, false, now),
            Err("hostname mismatch")
        );
        let old = now - chrono::Duration::seconds(301);
        assert_eq!(check_response(&ok_response("login", h, old), "login", h, false, now), Err("challenge too old"));
        let ok_old = now - chrono::Duration::seconds(299);
        assert_eq!(check_response(&ok_response("login", h, ok_old), "login", h, false, now), Ok(()));
        let mut testing = ok_response("login", h, now);
        testing.metadata.result_with_testing_key = true;
        assert!(check_response(&testing, "login", h, false, now).is_err(), "a test result never passes in production");
        // What Cloudflare's test secret really answers (2026-11-05).
        let cf_test: SiteverifyResponse = serde_json::from_str(
            r#"{"challenge_ts":"2026-11-05T11:59:10.990Z","error-codes":[],"hostname":"example.com","metadata":{"result_with_testing_key":true},"success":true}"#,
        )
        .unwrap();
        assert_eq!(check_response(&cf_test, "login", h, true, now), Ok(()));
        assert!(check_response(&cf_test, "login", h, false, now).is_err());
        let spent: SiteverifyResponse =
            serde_json::from_str(r#"{"error-codes":["timeout-or-duplicate"],"success":false,"messages":[]}"#).unwrap();
        assert!(check_response(&spent, "login", h, true, now).is_err());
    }

    type Seen = Arc<Mutex<Vec<Vec<(String, String)>>>>;

    /// A local siteverify: records the forms it gets and answers from a script.
    async fn mock(statuses: Vec<u16>) -> (String, Seen) {
        let seen: Seen = Arc::default();
        let script = Arc::new(Mutex::new(statuses));
        let (s2, sc) = (seen.clone(), script.clone());
        let app = axum::Router::new().route(
            "/siteverify",
            post(move |axum::extract::Form(f): axum::extract::Form<Vec<(String, String)>>| {
                let (s2, sc) = (s2.clone(), sc.clone());
                async move {
                    s2.lock().unwrap().push(f);
                    let status = { let mut g = sc.lock().unwrap(); if g.is_empty() { 200 } else { g.remove(0) } };
                    let body = format!(
                        r#"{{"success":true,"hostname":"krunk.kruxsys.com","action":"login","challenge_ts":"{}","error-codes":[]}}"#,
                        Utc::now().to_rfc3339()
                    );
                    (StatusCode::from_u16(status).unwrap(), body)
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}/siteverify"), seen)
    }

    #[tokio::test]
    async fn siteverify_sends_the_fields_and_retries_with_the_same_key() {
        let (url, seen) = mock(vec![500, 503]).await;
        let t = Turnstile::with_url(&url, "sekrit", "krunk.kruxsys.com", false).unwrap();
        let r = t.siteverify("tok", "203.0.113.9".parse().unwrap()).await.unwrap();
        assert!(r.success);
        let forms = seen.lock().unwrap().clone();
        assert_eq!(forms.len(), 3, "two retries");
        let get = |f: &Vec<(String, String)>, k: &str| f.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap();
        assert_eq!(get(&forms[0], "secret"), "sekrit");
        assert_eq!(get(&forms[0], "response"), "tok");
        assert_eq!(get(&forms[0], "remoteip"), "203.0.113.9");
        let key = get(&forms[0], "idempotency_key");
        assert!(uuid::Uuid::parse_str(&key).is_ok());
        assert!(forms.iter().all(|f| get(f, "idempotency_key") == key), "retries reuse the key");
        // A new validation gets a new key.
        t.siteverify("tok2", "203.0.113.9".parse().unwrap()).await.unwrap();
        let forms = seen.lock().unwrap().clone();
        assert_ne!(get(&forms[3], "idempotency_key"), key);
    }

    #[tokio::test]
    async fn siteverify_down_fails_closed() {
        let (url, _) = mock(vec![500, 500, 500]).await;
        let t = Turnstile::with_url(&url, "s", "krunk.kruxsys.com", false).unwrap();
        let e = t.siteverify("tok", "203.0.113.9".parse().unwrap()).await.unwrap_err();
        assert_eq!(e.status, StatusCode::SERVICE_UNAVAILABLE);
        // Nothing listening at all.
        let t = Turnstile::with_url("http://127.0.0.1:9/siteverify", "s", "h", false).unwrap();
        assert_eq!(
            t.siteverify("tok", "203.0.113.9".parse().unwrap()).await.unwrap_err().status,
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
}
