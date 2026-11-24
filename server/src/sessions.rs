//! Website sessions (docs/sync/PLAN.md §4.3, PROTOCOL §11.1).
//!
//! - Cookie `__Host-krunk_session` = 32 random bytes (base64url); `Secure`, `HttpOnly`,
//!   `SameSite=Strict`, `Path=/`, no `Domain`. The database keeps only its HMAC.
//! - Lifetime: 12 hours idle, 7 days absolute.
//! - CSRF: every state-changing request carries `X-Krunk-CSRF`, which is
//!   HMAC(token key, cookie token): nothing extra to store, and only same-origin script can read
//!   it (from the login response or `web/me`). On top: a strict `Origin` check (below) and
//!   `SameSite=Strict`.
//! - Re-authentication (`reauth_at`) is valid for 10 minutes.

use crate::api::{ApiError, Client};
use crate::config::Keys;
use crate::crypto;
use crate::http::AppState;
use axum::extract::{FromRequestParts, Request, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap, HeaderValue, Method};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, Utc};
use sqlx::PgConnection;
use uuid::Uuid;

pub const COOKIE: &str = "__Host-krunk_session";
pub const CSRF_HEADER: &str = "x-krunk-csrf";
pub const MAX_AGE_S: i64 = 7 * 86400;
pub const IDLE_S: i64 = 12 * 3600;
pub const REAUTH_S: i64 = 600;

pub fn token_hash(keys: &Keys, token: &str) -> [u8; 32] {
    crypto::hmac(&keys.token_hmac, "krunk-session", &[token.as_bytes()])
}

pub fn csrf_for(keys: &Keys, token: &str) -> String {
    crypto::b64url(&crypto::hmac(&keys.token_hmac, "krunk-csrf", &[token.as_bytes()]))
}

pub fn set_cookie(token: &str) -> HeaderValue {
    HeaderValue::from_str(&format!("{COOKIE}={token}; Path=/; Secure; HttpOnly; SameSite=Strict; Max-Age={MAX_AGE_S}"))
        .expect("a base64url token is a valid header value")
}

pub fn clear_cookie() -> HeaderValue {
    HeaderValue::from_static("__Host-krunk_session=; Path=/; Secure; HttpOnly; SameSite=Strict; Max-Age=0")
}

/// The session token from the `Cookie` header(s), if it has the right shape.
pub fn cookie_token(headers: &HeaderMap) -> Option<String> {
    named_cookie(headers, COOKIE)
}

/// The registration cookie (one-step registration): set by `web/register`, it lets the same
/// browser confirm the address without typing the password again. `SameSite=Lax`, so it comes
/// along when the link in the mail opens the verify page; HttpOnly and `__Host-` like the session
/// cookie, and only its HMAC is stored (`registrations.cookie_hash`). It grants nothing else.
pub const REG_COOKIE: &str = "__Host-krunk_reg";
pub const REG_MAX_AGE_S: i64 = 24 * 3600;

pub fn set_reg_cookie(token: &str) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "{REG_COOKIE}={token}; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age={REG_MAX_AGE_S}"
    ))
    .expect("a base64url token is a valid header value")
}

pub fn clear_reg_cookie() -> HeaderValue {
    HeaderValue::from_static("__Host-krunk_reg=; Path=/; Secure; HttpOnly; SameSite=Lax; Max-Age=0")
}

pub fn reg_cookie_token(headers: &HeaderMap) -> Option<String> {
    named_cookie(headers, REG_COOKIE)
}

fn named_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| crypto::b64url_decode::<32>(v).is_some())
}

/// How a website session was signed in (`web_sessions.via`, 0.13.0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    /// The sign-in form (password, and the second factor if on).
    Password,
    /// A phone that a signed-in browser approved by QR code (`web/pair/*`).
    Qr,
    /// A Krunk signed in to the account opened the site (`web/device-sign-in`, websignin.rs).
    Device,
}

impl Via {
    pub fn name(self) -> &'static str {
        match self {
            Via::Password => "password",
            Via::Qr => "qr",
            Via::Device => "device",
        }
    }
}

/// Creates a session; returns `(id, token, csrf)`. `reauth`: the sign-in itself proved the
/// password just now (login, email verification, registration poll), so the session starts
/// re-authenticated for [`REAUTH_S`].
pub async fn create(
    conn: &mut PgConnection,
    keys: &Keys,
    account_id: Uuid,
    client: &Client,
    via: Via,
    reauth: bool,
) -> Result<(Uuid, String, String), sqlx::Error> {
    let token = crypto::new_token();
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO web_sessions (id, account_id, token_hash, ip, country, user_agent, via, reauth_at)
         VALUES ($1, $2, $3, $4::inet, $5, $6, $7, CASE WHEN $8 THEN now() END)",
    )
    .bind(id)
    .bind(account_id)
    .bind(token_hash(keys, &token).as_slice())
    .bind(client.info.public_ip().map(|ip| ip.to_string()))
    .bind(client.info.country.as_deref())
    .bind(client.user_agent.as_deref())
    .bind(via.name())
    .bind(reauth)
    .execute(conn)
    .await?;
    let csrf = csrf_for(keys, &token);
    Ok((id, token, csrf))
}

/// A signed-in request. Extracting it checks the cookie, the lifetime and, for any method
/// other than GET/HEAD, the CSRF header.
#[derive(Debug, Clone)]
pub struct WebSession {
    pub id: Uuid,
    pub account_id: Uuid,
    pub email: String,
    pub email_verified: bool,
    pub created_at: DateTime<Utc>,
    pub totp_enabled: bool,
    pub reauth_at: Option<DateTime<Utc>>,
    pub csrf: String,
}

impl WebSession {
    pub fn reauth_valid(&self) -> bool {
        self.reauth_at.is_some_and(|t| (Utc::now() - t).num_seconds() <= REAUTH_S)
    }
}

#[derive(sqlx::FromRow)]
struct Row {
    id: Uuid,
    account_id: Uuid,
    email: String,
    email_verified: bool,
    created_at: DateTime<Utc>,
    totp_enabled: bool,
    reauth_at: Option<DateTime<Utc>>,
    last_used_at: DateTime<Utc>,
}

impl FromRequestParts<AppState> for WebSession {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let keys = &state.config.keys;
        let token = cookie_token(&parts.headers).ok_or_else(ApiError::not_signed_in)?;
        if parts.method != Method::GET && parts.method != Method::HEAD {
            let sent = parts.headers.get(CSRF_HEADER).and_then(|v| v.to_str().ok()).unwrap_or("");
            if !crypto::ct_eq(sent.as_bytes(), csrf_for(keys, &token).as_bytes()) {
                tracing::info!("csrf token missing or wrong");
                return Err(ApiError::forbidden());
            }
        }
        let row: Option<Row> = sqlx::query_as(
            "SELECT s.id, s.account_id, a.email::text AS email, a.email_verified_at IS NOT NULL AS email_verified,
                    a.created_at, a.totp_enabled, s.reauth_at, s.last_used_at
             FROM web_sessions s JOIN accounts a ON a.id = s.account_id
             WHERE s.token_hash = $1 AND s.revoked_at IS NULL
               AND s.created_at > now() - make_interval(secs => $2)
               AND s.last_used_at > now() - make_interval(secs => $3)",
        )
        .bind(token_hash(keys, &token).as_slice())
        .bind(MAX_AGE_S as f64)
        .bind(IDLE_S as f64)
        .fetch_optional(&state.db)
        .await?;
        let row = row.ok_or_else(ApiError::not_signed_in)?;
        // Record use (and the current IP) at most once a minute.
        if (Utc::now() - row.last_used_at).num_seconds() >= 60 {
            let client = Client::from_request_parts(parts, state).await?;
            sqlx::query("UPDATE web_sessions SET last_used_at = now(), ip = $2::inet, country = $3 WHERE id = $1")
                .bind(row.id)
                .bind(client.info.public_ip().map(|ip| ip.to_string()))
                .bind(client.info.country.as_deref())
                .execute(&state.db)
                .await?;
        }
        Ok(WebSession {
            id: row.id,
            account_id: row.account_id,
            email: row.email,
            email_verified: row.email_verified,
            created_at: row.created_at,
            totp_enabled: row.totp_enabled,
            reauth_at: row.reauth_at,
            csrf: csrf_for(keys, &token),
        })
    }
}

/// State-changing requests to the website API must come from the site itself: `Origin` equal
/// to PUBLIC_BASE_URL's origin, or (if a browser sent no `Origin`) `Sec-Fetch-Site:
/// same-origin`. Otherwise 403. This also stops login CSRF.
pub async fn same_origin(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if req.method() == Method::GET || req.method() == Method::HEAD {
        return next.run(req).await;
    }
    let h = req.headers();
    let ok = match h.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        Some(origin) => origin == state.config.public_origin(),
        None => h.get("sec-fetch-site").and_then(|v| v.to_str().ok()) == Some("same-origin"),
    };
    if !ok {
        tracing::info!("cross-origin request refused");
        return ApiError::forbidden().into_response();
    }
    next.run(req).await
}

/// Until Turnstile and SMTP are configured, every account endpoint is closed (Q1/Q2).
pub async fn accounts_gate(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if !state.config.accounts_enabled() {
        return ApiError::accounts_disabled().into_response();
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookies() {
        let t = crypto::new_token();
        let v = set_cookie(&t);
        let s = v.to_str().unwrap();
        for attr in ["__Host-krunk_session=", "Path=/", "Secure", "HttpOnly", "SameSite=Strict", "Max-Age=604800"] {
            assert!(s.contains(attr), "{s}");
        }
        assert!(!s.contains("Domain"), "__Host- cookies have no Domain");
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, HeaderValue::from_str(&format!("a=b; {COOKIE}={t}; c=d")).unwrap());
        assert_eq!(cookie_token(&h).as_deref(), Some(t.as_str()));
        h.insert(header::COOKIE, HeaderValue::from_static("__Host-krunk_session=short"));
        assert_eq!(cookie_token(&h), None);
        h.insert(
            header::COOKIE,
            HeaderValue::from_static("x__Host-krunk_session=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
        );
        assert_eq!(cookie_token(&h), None);
    }

    #[test]
    fn registration_cookie() {
        let t = crypto::new_token();
        let s = set_reg_cookie(&t).to_str().unwrap().to_string();
        for attr in ["__Host-krunk_reg=", "Path=/", "Secure", "HttpOnly", "SameSite=Lax", "Max-Age=86400"] {
            assert!(s.contains(attr), "{s}");
        }
        assert!(!s.contains("Domain"));
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, HeaderValue::from_str(&format!("{COOKIE}=x; {REG_COOKIE}={t}")).unwrap());
        assert_eq!(reg_cookie_token(&h).as_deref(), Some(t.as_str()));
        assert_eq!(cookie_token(&h), None, "the session cookie is not the registration cookie");
        assert!(clear_reg_cookie().to_str().unwrap().contains("Max-Age=0"));
    }

    #[test]
    fn csrf_is_bound_to_the_session_and_key() {
        let k = Keys { token_hmac: [1; 32], password_pepper: [2; 32], totp_enc: [3; 32] };
        let k2 = Keys { token_hmac: [9; 32], ..k.clone() };
        assert_eq!(csrf_for(&k, "a"), csrf_for(&k, "a"));
        assert_ne!(csrf_for(&k, "a"), csrf_for(&k, "b"));
        assert_ne!(csrf_for(&k, "a"), csrf_for(&k2, "a"));
        assert_ne!(crypto::b64url(&token_hash(&k, "a")), csrf_for(&k, "a"), "the CSRF token is not the stored hash");
    }
}
