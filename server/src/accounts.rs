//! The website account endpoints (PROTOCOL §11.2; docs/sync/PLAN.md §4.2, §4.3), milestone S2:
//! register, verify-email, resend-verification, login, logout, password reset, password
//! change, re-auth, me and TOTP.
//!
//! **Nothing reveals whether an account exists:** register, resend and reset always answer the
//! same; login runs one Argon2id either way and gives one error; the login lockout is keyed by
//! the email, not the account.
//!
//! **Pre-registration hijack** (someone registers your address first, you click the link):
//! verifying an email needs the token *and* the account's password; registering an address
//! that is still unverified replaces its password and voids the earlier links; and signing in
//! needs a verified email. So only someone holding both the mailbox and the password ends up
//! with a working account.

use crate::api::{ApiError, ApiResult, Client, JsonBody};
use crate::config::Keys;
use crate::crypto;
use crate::events::{self, Source};
use crate::http::AppState;
use crate::mail::{self, Template};
use crate::password::{self, PEPPER_ID};
use crate::ratelimit::{self as rl, hit};
use crate::sessions::{self, WebSession};
use crate::totp;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgConnection;
use uuid::Uuid;

pub const VERIFY_TTL_H: i32 = 24;
pub const RESET_TTL_H: i32 = 1;
const LOCK_AFTER: i32 = 5;

// ---- Inputs ---------------------------------------------------------------------------------

/// The email address as stored: trimmed, domain lower-cased; `None` if it isn't one.
/// Deliberately simple: one `@`, a 1–64 character local part without spaces or the specials
/// of RFC 5322, and an ASCII domain of at least two labels (IDNs in punycode).
pub fn normalize_email(s: &str) -> Option<String> {
    let s = s.trim();
    if s.len() > 254 {
        return None;
    }
    let (local, domain) = s.rsplit_once('@')?;
    let local_ok = (1..=64).contains(&local.len())
        && local.bytes().all(|b| b.is_ascii_graphic() && !b"\"(),:;<>@[\\]".contains(&b))
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..");
    let domain = domain.to_ascii_lowercase();
    let labels: Vec<&str> = domain.split('.').collect();
    let domain_ok = labels.len() >= 2
        && domain.len() <= 253
        && labels.iter().all(|l| {
            (1..=63).contains(&l.len())
                && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                && !l.starts_with('-')
                && !l.ends_with('-')
        })
        && !labels.last().is_some_and(|t| t.bytes().all(|b| b.is_ascii_digit()));
    (local_ok && domain_ok).then(|| format!("{local}@{domain}"))
}

pub(crate) fn email_or_400(s: &str) -> Result<String, ApiError> {
    normalize_email(s).ok_or_else(|| ApiError::bad_request("invalid_email", "Enter a valid email address."))
}

/// The key of everything limited per email (lower-cased, so it matches the citext column).
fn email_key(keys: &Keys, email: &str) -> [u8; 32] {
    rl::email_key(keys, &email.to_lowercase())
}

fn email_token_hash(keys: &Keys, token: &str) -> [u8; 32] {
    crypto::hmac(&keys.token_hmac, "krunk-email-token", &[token.as_bytes()])
}

fn reg_poll_hash(keys: &Keys, token: &str) -> [u8; 32] {
    crypto::hmac(&keys.token_hmac, "krunk-reg-poll", &[token.as_bytes()])
}

fn reg_cookie_hash(keys: &Keys, token: &str) -> [u8; 32] {
    crypto::hmac(&keys.token_hmac, "krunk-reg-cookie", &[token.as_bytes()])
}

/// The server's copy of the site's `safeNext()` (account.js): the only places a sign-in may go
/// on to.
pub(crate) fn safe_next(n: &str) -> bool {
    let link = n
        .strip_prefix("/link?code=")
        .is_some_and(|c| (1..=12).contains(&c.len()) && c.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
    link || matches!(n, "/account/" | "/account/#add-device" | "/account/#export" | "/account/delete")
}

fn backup_code_hash(keys: &Keys, normalized: &str) -> [u8; 32] {
    crypto::hmac(&keys.token_hmac, "krunk-backup-code", &[normalized.as_bytes()])
}

fn policy(password: &str, email: &str) -> Result<(), ApiError> {
    password::check_policy(password, email).map_err(|e| ApiError::bad_request(e.code(), e.message()))
}

fn too_long(s: &str, max: usize) -> bool {
    s.len() > max
}

pub(crate) fn check_lengths(fields: &[(&str, usize)]) -> Result<(), ApiError> {
    if fields.iter().any(|(s, max)| too_long(s, *max)) {
        return Err(ApiError::bad_request("bad_request", "The request is not valid."));
    }
    Ok(())
}

// Upper bounds in bytes; the password policy itself counts characters.
pub(crate) const EMAIL_MAX: usize = 254;
pub(crate) const PASSWORD_MAX: usize = 1024;
const TOKEN_MAX: usize = 64;
const NEXT_MAX: usize = 64;
pub(crate) const TURNSTILE_MAX: usize = 2048;
pub(crate) const CODE_MAX: usize = 32;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterReq {
    email: String,
    password: String,
    turnstile: String,
    /// Where the site goes once signed in (a `safeNext()` path; anything else is dropped).
    #[serde(default)]
    next: Option<String>,
    /// The `/link` code of the Krunk tab the registration started from (auto-link).
    #[serde(default)]
    link_code: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyReq {
    token: String,
    /// Not needed in the browser that registered (`__Host-krunk_reg`); elsewhere the first try
    /// without it answers 401 `password_required`.
    #[serde(default)]
    password: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegPollReq {
    reg_poll: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmailReq {
    email: String,
    turnstile: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginReq {
    email: String,
    password: String,
    turnstile: String,
    #[serde(default)]
    totp: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResetConfirmReq {
    token: String,
    new_password: String,
    #[serde(default)]
    totp: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeReq {
    current: String,
    new: String,
    #[serde(default)]
    totp: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReauthReq {
    password: String,
    #[serde(default)]
    totp: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordOnly {
    password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeReq {
    code: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisableReq {
    password: String,
    code: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

fn status(code: StatusCode, s: &'static str) -> Response {
    (code, Json(json!({ "status": s }))).into_response()
}

// ---- Shared steps ---------------------------------------------------------------------------

/// Creates a fresh email link token (after voiding the account's earlier ones of that kind),
/// returns the raw token for the mail.
async fn new_email_token(
    conn: &mut PgConnection,
    keys: &Keys,
    account_id: Uuid,
    kind: &str,
    ttl_h: i32,
) -> Result<String, sqlx::Error> {
    sqlx::query("DELETE FROM email_tokens WHERE account_id = $1 AND kind = $2")
        .bind(account_id)
        .bind(kind)
        .execute(&mut *conn)
        .await?;
    let token = crypto::new_token();
    sqlx::query(
        "INSERT INTO email_tokens (hash, account_id, kind, expires_at) VALUES ($1, $2, $3, now() + make_interval(hours => $4))",
    )
    .bind(email_token_hash(keys, &token).as_slice())
    .bind(account_id)
    .bind(kind)
    .bind(ttl_h)
    .execute(&mut *conn)
    .await?;
    Ok(token)
}

async fn ip_event(
    conn: &mut PgConnection,
    account_id: Uuid,
    kind: &str,
    client: &Client,
    source: Source,
) -> Result<(), sqlx::Error> {
    events::record(conn, account_id, kind, client, source).await
}

fn totp_required() -> ApiError {
    ApiError::new(StatusCode::UNAUTHORIZED, "totp_required", "Enter the code from your authenticator app.")
}

fn totp_invalid() -> ApiError {
    ApiError::new(StatusCode::UNAUTHORIZED, "totp_invalid", "The code is wrong. Try the current code or a backup code.")
}

pub(crate) fn wrong_password() -> ApiError {
    ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", "The password is wrong.")
}

#[derive(sqlx::FromRow)]
struct Factor {
    totp_enabled: bool,
    totp_secret_enc: Option<Vec<u8>>,
    totp_last_step: i64,
}

/// The second factor, if the account has one: a TOTP code (each time step once) or an unused
/// backup code. `totp_required` when none was given.
pub(crate) async fn second_factor(state: &AppState, account_id: Uuid, code: Option<&str>) -> Result<(), ApiError> {
    let f: Factor = sqlx::query_as("SELECT totp_enabled, totp_secret_enc, totp_last_step FROM accounts WHERE id = $1")
        .bind(account_id)
        .fetch_one(&state.db)
        .await?;
    if !f.totp_enabled {
        return Ok(());
    }
    let code = code.map(str::trim).filter(|c| !c.is_empty()).ok_or_else(totp_required)?;
    if code.len() > CODE_MAX {
        return Err(totp_invalid());
    }
    let keys = &state.config.keys;
    if code.len() == totp::DIGITS as usize && code.bytes().all(|b| b.is_ascii_digit()) {
        let secret = f
            .totp_secret_enc
            .and_then(|enc| crypto::open(&keys.totp_enc, account_id.as_bytes(), &enc))
            .ok_or_else(|| {
                tracing::error!("the TOTP secret can't be decrypted");
                ApiError::server_error()
            })?;
        let now = Utc::now().timestamp().max(0) as u64;
        let step = totp::verify(&secret, code, now, f.totp_last_step.max(0) as u64).ok_or_else(totp_invalid)?;
        // Atomic: two concurrent requests can't both use the same step.
        let n = sqlx::query("UPDATE accounts SET totp_last_step = $2 WHERE id = $1 AND totp_last_step < $2")
            .bind(account_id)
            .bind(step as i64)
            .execute(&state.db)
            .await?
            .rows_affected();
        return if n == 1 { Ok(()) } else { Err(totp_invalid()) };
    }
    let normalized = totp::normalize_backup_code(code).ok_or_else(totp_invalid)?;
    let n = sqlx::query(
        "UPDATE backup_codes SET used_at = now() WHERE account_id = $1 AND code_hash = $2 AND used_at IS NULL",
    )
    .bind(account_id)
    .bind(backup_code_hash(keys, &normalized).as_slice())
    .execute(&state.db)
    .await?
    .rows_affected();
    if n == 1 {
        tracing::info!("backup code used");
        Ok(())
    } else {
        Err(totp_invalid())
    }
}

/// Is this email locked out? Returns the seconds left.
async fn lockout(state: &AppState, key: &[u8; 32]) -> Result<Option<u64>, ApiError> {
    let left: Option<f64> = sqlx::query_scalar(
        "SELECT extract(epoch FROM locked_until - now())::float8 FROM login_throttle WHERE email_hash = $1 AND locked_until > now()",
    )
    .bind(key.as_slice())
    .fetch_optional(&state.db)
    .await?;
    Ok(left.map(|s| s.ceil().max(1.0) as u64))
}

/// Counts a failed login for this email: after 5 within 15 minutes, a lock of 1, 2, 4 … 60
/// minutes that doubles with each further failure.
async fn login_failed(state: &AppState, key: &[u8; 32]) -> Result<(), ApiError> {
    let failures: i32 = sqlx::query_scalar(
        "INSERT INTO login_throttle (email_hash, failures, updated_at) VALUES ($1, 1, now())
         ON CONFLICT (email_hash) DO UPDATE SET
           failures = CASE WHEN login_throttle.updated_at < now() - interval '15 minutes'
                            AND (login_throttle.locked_until IS NULL OR login_throttle.locked_until < now())
                           THEN 1 ELSE login_throttle.failures + 1 END,
           updated_at = now()
         RETURNING failures",
    )
    .bind(key.as_slice())
    .fetch_one(&state.db)
    .await?;
    if failures >= LOCK_AFTER {
        let minutes = 1i32 << (failures - LOCK_AFTER).min(6);
        sqlx::query("UPDATE login_throttle SET locked_until = now() + make_interval(mins => $2) WHERE email_hash = $1")
            .bind(key.as_slice())
            .bind(minutes.min(60))
            .execute(&state.db)
            .await?;
        tracing::info!(minutes = minutes.min(60), "login locked for an email");
    }
    Ok(())
}

async fn login_succeeded(conn: &mut PgConnection, key: &[u8; 32]) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM login_throttle WHERE email_hash = $1").bind(key.as_slice()).execute(conn).await?;
    Ok(())
}

fn ip_key(state: &AppState, client: &Client) -> [u8; 32] {
    rl::ip_key(&state.config.keys, client.ip())
}

// ---- Handlers -------------------------------------------------------------------------------

/// `POST web/register` → 202 `{status:"check_email", reg_poll}` + the registration cookie,
/// whatever the address: the same rows, mail and work for a new, an unverified and a verified
/// address (the last gets a "you already have an account" mail and a registration that never
/// completes). `reg_poll` is for `web/register/poll`.
pub async fn register(
    State(state): State<AppState>,
    client: Client,
    JsonBody(req): JsonBody<RegisterReq>,
) -> ApiResult {
    hit(&state.db, rl::REGISTER_IP, &ip_key(&state, &client)).await?;
    check_lengths(&[
        (&req.email, EMAIL_MAX),
        (&req.password, PASSWORD_MAX),
        (&req.turnstile, TURNSTILE_MAX),
        (req.next.as_deref().unwrap_or(""), NEXT_MAX),
        (req.link_code.as_deref().unwrap_or(""), CODE_MAX),
    ])?;
    let email = email_or_400(&req.email)?;
    policy(&req.password, &email)?;
    state.turnstile.verify(&state.db, &req.turnstile, "register", client.ip()).await?;
    // Always one hash, so a known address takes as long as a new one.
    let phc = state.passwords.hash(&req.password).await.map_err(|e| {
        tracing::error!(error = %e, "password hash");
        ApiError::server_error()
    })?;
    let keys = &state.config.keys;
    let site = &state.config.public_base_url;
    let next = req.next.filter(|n| safe_next(n));
    let mut tx = state.db.begin().await?;
    let existing: Option<(Uuid, bool)> =
        sqlx::query_as("SELECT id, email_verified_at IS NOT NULL FROM accounts WHERE email = $1::citext FOR UPDATE")
            .bind(&email)
            .fetch_optional(&mut *tx)
            .await?;
    let account_id = match existing {
        Some((id, true)) => {
            mail::enqueue(&mut tx, Some(id), &email, Template::RegisterExists, json!({})).await?;
            None
        }
        Some((id, false)) => {
            // Still unverified: whoever registers now owns the password; earlier links die.
            sqlx::query("UPDATE accounts SET pw_phc = $2, pepper_id = $3, pw_changed_at = now() WHERE id = $1")
                .bind(id)
                .bind(&phc)
                .bind(PEPPER_ID)
                .execute(&mut *tx)
                .await?;
            Some(id)
        }
        None => {
            let id = Uuid::new_v4();
            let inserted = sqlx::query(
                "INSERT INTO accounts (id, email, pw_phc, pepper_id) VALUES ($1, $2, $3, $4) ON CONFLICT (email) DO NOTHING",
            )
            .bind(id)
            .bind(&email)
            .bind(&phc)
            .bind(PEPPER_ID)
            .execute(&mut *tx)
            .await?
            .rows_affected();
            // Lost a race with another registration of the same address: answer as usual.
            (inserted == 1).then_some(id)
        }
    };
    if let Some(id) = account_id {
        let token = new_email_token(&mut tx, keys, id, "verify", VERIFY_TTL_H).await?;
        let link = format!("{site}/account/verify?token={token}");
        mail::enqueue(&mut tx, Some(id), &email, Template::Verify, json!({ "link": link })).await?;
        ip_event(&mut tx, id, "register", &client, Source::None).await?;
    }
    // The registration, for every address alike. Earlier registrations of this account are
    // voided (the query runs, matching nothing, when no mail was sent): their password is gone.
    sqlx::query("UPDATE registrations SET voided = true WHERE account_id = $1 AND NOT voided")
        .bind(account_id)
        .execute(&mut *tx)
        .await?;
    let attached = match req.link_code.as_deref() {
        Some(code) => crate::devices::attach_for_registration(&mut tx, keys, code, &client).await?,
        None => None,
    };
    let reg_poll = crypto::new_token();
    let reg_cookie = crypto::new_token();
    sqlx::query(
        "INSERT INTO registrations (id, poll_hash, cookie_hash, account_id, next, link_device_code_hash, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, now() + make_interval(hours => $7))",
    )
    .bind(Uuid::new_v4())
    .bind(reg_poll_hash(keys, &reg_poll).as_slice())
    .bind(reg_cookie_hash(keys, &reg_cookie).as_slice())
    .bind(account_id)
    .bind(next.as_deref())
    .bind(attached.as_deref())
    .bind(VERIFY_TTL_H)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    let mut res =
        (StatusCode::ACCEPTED, Json(json!({ "status": "check_email", "reg_poll": reg_poll }))).into_response();
    res.headers_mut().insert(header::SET_COOKIE, sessions::set_reg_cookie(&reg_cookie));
    Ok(res)
}

/// What a registration's sign-in (verify page or the registering page's poll) reports.
#[derive(Serialize)]
struct SignedIn {
    csrf: String,
    next: Option<String>,
    /// `linked`: the Krunk the registration started from is now signed in; `unavailable`: its
    /// code expired or was denied meanwhile (sign in from Krunk again); `null`: no auto-link.
    link: Option<String>,
    linked: Option<LinkedDevice>,
}

#[derive(Serialize)]
struct LinkedDevice {
    label: String,
    platform: String,
}

fn linked_device(outcome: Option<&str>, label: Option<String>, platform: Option<String>) -> Option<LinkedDevice> {
    match (outcome, label, platform) {
        (Some("linked"), Some(label), Some(platform)) => {
            Some(LinkedDevice { label: crate::devices::display_label(&label), platform })
        }
        _ => None,
    }
}

fn reg_poll_error(code: &'static str) -> ApiError {
    let message = match code {
        "authorization_pending" => "Waiting for the email address to be confirmed.",
        _ => "This registration has expired. Sign in, or register again.",
    };
    ApiError::bad_request(code, message)
}

#[derive(sqlx::FromRow)]
struct RegPollRow {
    id: Uuid,
    account_id: Option<Uuid>,
    expired: bool,
    verified: bool,
    voided: bool,
    next: Option<String>,
    link_outcome: Option<String>,
    linked_label: Option<String>,
    linked_platform: Option<String>,
}

/// `POST web/register/poll {reg_poll}` → 400 `authorization_pending` until the registration's
/// address is confirmed, then once 200 with a website session (cookie + `SignedIn`); 400
/// `expired_token` when unknown, spent or expired.
pub async fn register_poll(
    State(state): State<AppState>,
    client: Client,
    headers: HeaderMap,
    JsonBody(req): JsonBody<RegPollReq>,
) -> ApiResult {
    hit(&state.db, rl::REG_POLL_IP, &ip_key(&state, &client)).await?;
    if req.reg_poll.len() > TOKEN_MAX || crypto::b64url_decode::<32>(&req.reg_poll).is_none() {
        return Err(reg_poll_error("expired_token"));
    }
    let keys = &state.config.keys;
    let mut tx = state.db.begin().await?;
    let row: Option<RegPollRow> = sqlx::query_as(
        "SELECT id, account_id, expires_at < now() AS expired, verified_at IS NOT NULL AS verified, voided, next,
                link_outcome, linked_label, linked_platform
         FROM registrations WHERE poll_hash = $1 FOR UPDATE",
    )
    .bind(reg_poll_hash(keys, &req.reg_poll).as_slice())
    .fetch_optional(&mut *tx)
    .await?;
    let Some(r) = row else { return Err(reg_poll_error("expired_token")) };
    if r.expired {
        return Err(reg_poll_error("expired_token"));
    }
    let account_id = match (r.account_id, r.verified, r.voided) {
        (Some(id), true, false) => id,
        _ => return Err(reg_poll_error("authorization_pending")),
    };
    sqlx::query("UPDATE registrations SET poll_hash = NULL WHERE id = $1").bind(r.id).execute(&mut *tx).await?;
    // The address was confirmed in this very browser: it already has the session the verify page
    // made (same cookie jar). Keep that one; a second session's cookie would replace it under the
    // other tab, whose CSRF token would then no longer match (its Sign out failed, 0.24.0 smoke).
    let existing = match sessions::cookie_token(&headers) {
        Some(t) => sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (SELECT 1 FROM web_sessions WHERE token_hash = $1 AND account_id = $2
                            AND revoked_at IS NULL AND created_at > now() - make_interval(secs => $3)
                            AND last_used_at > now() - make_interval(secs => $4))",
        )
        .bind(sessions::token_hash(keys, &t).as_slice())
        .bind(account_id)
        .bind(sessions::MAX_AGE_S as f64)
        .bind(sessions::IDLE_S as f64)
        .fetch_one(&mut *tx)
        .await?
        .then_some(t),
        None => None,
    };
    let (csrf, new_token) = match existing {
        Some(t) => (sessions::csrf_for(keys, &t), None),
        None => {
            let (session_id, token, csrf) =
                sessions::create(&mut tx, keys, account_id, &client, sessions::Via::Password, true).await?;
            sqlx::query("UPDATE accounts SET last_active_at = now() WHERE id = $1")
                .bind(account_id)
                .execute(&mut *tx)
                .await?;
            ip_event(&mut tx, account_id, "login", &client, Source::Web(session_id)).await?;
            (csrf, Some(token))
        }
    };
    tx.commit().await?;
    tracing::info!(reused = new_token.is_none(), "signed in by the registration poll");
    let body = SignedIn {
        csrf,
        next: r.next,
        linked: linked_device(r.link_outcome.as_deref(), r.linked_label, r.linked_platform),
        link: r.link_outcome,
    };
    let mut res = Json(body).into_response();
    if let Some(token) = new_token {
        res.headers_mut().insert(header::SET_COOKIE, sessions::set_cookie(&token));
    }
    Ok(res)
}

#[derive(sqlx::FromRow)]
struct TokenRow {
    account_id: Uuid,
    expired: bool,
    used: bool,
    pw_phc: String,
    email: String,
}

async fn lookup_token(state: &AppState, token: &str, kind: &str) -> Result<TokenRow, ApiError> {
    let invalid = || ApiError::bad_request("invalid_link", "This link is not valid. Request a new one.");
    if crypto::b64url_decode::<32>(token).is_none() {
        return Err(invalid());
    }
    let row: Option<TokenRow> = sqlx::query_as(
        "SELECT t.account_id, t.expires_at < now() AS expired, t.used_at IS NOT NULL AS used, a.pw_phc, a.email::text AS email
         FROM email_tokens t JOIN accounts a ON a.id = t.account_id WHERE t.hash = $1 AND t.kind = $2",
    )
    .bind(email_token_hash(&state.config.keys, token).as_slice())
    .bind(kind)
    .fetch_optional(&state.db)
    .await?;
    match row {
        Some(r) if r.used => Err(invalid()),
        Some(r) if r.expired => {
            Err(ApiError::new(StatusCode::GONE, "gone", "This link has expired. Request a new one."))
        }
        Some(r) => Ok(r),
        None => Err(invalid()),
    }
}

fn password_required() -> ApiError {
    ApiError::new(StatusCode::UNAUTHORIZED, "password_required", "Enter your password to confirm this address.")
}

#[derive(sqlx::FromRow)]
struct VerifiedReg {
    id: Uuid,
    next: Option<String>,
    link_device_code_hash: Option<Vec<u8>>,
}

/// `POST web/verify-email` `{token, password?}` → 200 `SignedIn` + a website session; the
/// account's registration completes too (its page's poll signs in, and the Krunk it started from
/// is linked). The password is not needed in the browser that registered (its
/// `__Host-krunk_reg` matches a live registration of this very account); anywhere else the first
/// try without one gets 401 `password_required`, and the link stays usable.
pub async fn verify_email(
    State(state): State<AppState>,
    client: Client,
    headers: HeaderMap,
    JsonBody(req): JsonBody<VerifyReq>,
) -> ApiResult {
    hit(&state.db, rl::VERIFY_IP, &ip_key(&state, &client)).await?;
    check_lengths(&[(&req.token, TOKEN_MAX), (req.password.as_deref().unwrap_or(""), PASSWORD_MAX)])?;
    let row = lookup_token(&state, &req.token, "verify").await?;
    let keys = &state.config.keys;
    let same_browser = match sessions::reg_cookie_token(&headers) {
        Some(c) => {
            sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (SELECT 1 FROM registrations WHERE cookie_hash = $1 AND account_id = $2
                            AND NOT voided AND expires_at > now())",
            )
            .bind(reg_cookie_hash(keys, &c).as_slice())
            .bind(row.account_id)
            .fetch_one(&state.db)
            .await?
        }
        None => false,
    };
    // A password, when given, must be right even in the browser that registered.
    match req.password.as_deref().filter(|p| !p.is_empty()) {
        None if same_browser => {}
        None => return Err(password_required()),
        Some(p) if !state.passwords.verify(p, Some(&row.pw_phc)).await => return Err(wrong_password()),
        Some(_) => {}
    }
    let mut tx = state.db.begin().await?;
    // A registration of the address since the checks above deleted this token (and voided the
    // registration), so the token is the guard.
    let n = sqlx::query("UPDATE email_tokens SET used_at = now() WHERE hash = $1 AND used_at IS NULL")
        .bind(email_token_hash(keys, &req.token).as_slice())
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if n != 1 {
        return Err(ApiError::bad_request("invalid_link", "This link is not valid. Request a new one."));
    }
    sqlx::query("UPDATE accounts SET email_verified_at = coalesce(email_verified_at, now()), last_active_at = now() WHERE id = $1")
        .bind(row.account_id)
        .execute(&mut *tx)
        .await?;
    let (session_id, token, csrf) =
        sessions::create(&mut tx, keys, row.account_id, &client, sessions::Via::Password, true).await?;
    ip_event(&mut tx, row.account_id, "email_verified", &client, Source::Web(session_id)).await?;
    let regs: Vec<VerifiedReg> = sqlx::query_as(
        "UPDATE registrations SET verified_at = now()
         WHERE account_id = $1 AND NOT voided AND verified_at IS NULL AND expires_at > now()
         RETURNING id, next, link_device_code_hash",
    )
    .bind(row.account_id)
    .fetch_all(&mut *tx)
    .await?;
    let next = regs.iter().find_map(|r| r.next.clone());
    let mut link = None;
    let mut linked = None;
    if let Some(r) = regs.iter().find(|r| r.link_device_code_hash.is_some()) {
        let hash = r.link_device_code_hash.as_deref().unwrap_or_default();
        let approver = crate::devices::Approver { account_id: row.account_id, email: row.email.clone(), session_id };
        // A savepoint: whatever goes wrong with the link (quota, say), the address stays confirmed.
        let mut sp = sqlx::Acquire::begin(&mut tx).await?;
        let outcome = match crate::devices::attached_grant(&mut sp, hash).await? {
            Some(g) => match crate::devices::approve_grant(&mut sp, &state, &approver, &client, &g).await {
                Ok(_) => Some(g),
                Err(e) => {
                    tracing::info!(error = e.code, "auto-link at verification failed");
                    None
                }
            },
            None => None,
        };
        match outcome {
            Some(g) => {
                sp.commit().await?;
                tracing::info!("device linked at verification");
                sqlx::query(
                    "UPDATE registrations SET link_outcome = 'linked', linked_label = $2, linked_platform = $3 WHERE id = $1",
                )
                .bind(r.id)
                .bind(&g.label)
                .bind(&g.platform)
                .execute(&mut *tx)
                .await?;
                linked = linked_device(Some("linked"), Some(g.label), Some(g.platform));
                link = Some("linked".to_string());
            }
            None => {
                sp.rollback().await?;
                sqlx::query("UPDATE registrations SET link_outcome = 'unavailable' WHERE id = $1")
                    .bind(r.id)
                    .execute(&mut *tx)
                    .await?;
                link = Some("unavailable".to_string());
            }
        }
    }
    tx.commit().await?;
    let mut res = Json(SignedIn { csrf, next, link, linked }).into_response();
    res.headers_mut().insert(header::SET_COOKIE, sessions::set_cookie(&token));
    res.headers_mut().append(header::SET_COOKIE, sessions::clear_reg_cookie());
    Ok(res)
}

/// `POST web/resend-verification` → 202, whatever the address.
pub async fn resend(State(state): State<AppState>, client: Client, JsonBody(req): JsonBody<EmailReq>) -> ApiResult {
    hit(&state.db, rl::RESEND_IP, &ip_key(&state, &client)).await?;
    check_lengths(&[(&req.email, EMAIL_MAX), (&req.turnstile, TURNSTILE_MAX)])?;
    let email = email_or_400(&req.email)?;
    state.turnstile.verify(&state.db, &req.turnstile, "resend", client.ip()).await?;
    let keys = &state.config.keys;
    hit(&state.db, rl::RESEND_EMAIL, &email_key(keys, &email)).await?;
    let mut tx = state.db.begin().await?;
    let unverified: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT id, email::text FROM accounts WHERE email = $1::citext AND email_verified_at IS NULL FOR UPDATE",
    )
    .bind(&email)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some((id, stored)) = unverified {
        let token = new_email_token(&mut tx, keys, id, "verify", VERIFY_TTL_H).await?;
        let link = format!("{}/account/verify?token={token}", state.config.public_base_url);
        mail::enqueue(&mut tx, Some(id), &stored, Template::Verify, json!({ "link": link })).await?;
    }
    tx.commit().await?;
    Ok(status(StatusCode::ACCEPTED, "check_email"))
}

#[derive(sqlx::FromRow)]
struct LoginRow {
    id: Uuid,
    pw_phc: String,
    verified: bool,
}

#[derive(Serialize)]
struct CsrfBody {
    csrf: String,
}

/// `POST web/login` → 200 + cookie `{csrf}`; 401 `unauthorized` (generic), `totp_required`,
/// `totp_invalid`; 403 `email_unverified` (only after the right password).
pub async fn login(State(state): State<AppState>, client: Client, JsonBody(req): JsonBody<LoginReq>) -> ApiResult {
    hit(&state.db, rl::LOGIN_IP, &ip_key(&state, &client)).await?;
    check_lengths(&[
        (&req.email, EMAIL_MAX),
        (&req.password, PASSWORD_MAX),
        (&req.turnstile, TURNSTILE_MAX),
        (req.totp.as_deref().unwrap_or(""), CODE_MAX),
    ])?;
    let email = email_or_400(&req.email)?;
    state.turnstile.verify(&state.db, &req.turnstile, "login", client.ip()).await?;
    let keys = &state.config.keys;
    let ekey = email_key(keys, &email);
    if let Some(left) = lockout(&state, &ekey).await? {
        return Err(ApiError::rate_limited(left));
    }
    let row: Option<LoginRow> = sqlx::query_as(
        "SELECT id, pw_phc, email_verified_at IS NOT NULL AS verified FROM accounts WHERE email = $1::citext",
    )
    .bind(&email)
    .fetch_optional(&state.db)
    .await?;
    let ok = state.passwords.verify(&req.password, row.as_ref().map(|r| r.pw_phc.as_str())).await;
    let Some(row) = row.filter(|_| ok) else {
        login_failed(&state, &ekey).await?;
        return Err(ApiError::unauthorized());
    };
    if let Err(e) = second_factor(&state, row.id, req.totp.as_deref()).await {
        if e.code == "totp_invalid" {
            login_failed(&state, &ekey).await?;
        }
        return Err(e);
    }
    if !row.verified {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "email_unverified",
            "Please confirm your email address first. We can send the link again.",
        ));
    }
    let mut tx = state.db.begin().await?;
    login_succeeded(&mut tx, &ekey).await?;
    let (session_id, token, csrf) =
        sessions::create(&mut tx, keys, row.id, &client, sessions::Via::Password, true).await?;
    sqlx::query("UPDATE accounts SET last_active_at = now() WHERE id = $1").bind(row.id).execute(&mut *tx).await?;
    ip_event(&mut tx, row.id, "login", &client, Source::Web(session_id)).await?;
    tx.commit().await?;
    let mut res = Json(CsrfBody { csrf }).into_response();
    res.headers_mut().insert(header::SET_COOKIE, sessions::set_cookie(&token));
    Ok(res)
}

/// `POST web/logout` → 204, cookie cleared.
pub async fn logout(State(state): State<AppState>, s: WebSession, JsonBody(_): JsonBody<Empty>) -> ApiResult {
    sqlx::query("UPDATE web_sessions SET revoked_at = now() WHERE id = $1").bind(s.id).execute(&state.db).await?;
    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut().insert(header::SET_COOKIE, sessions::clear_cookie());
    Ok(res)
}

/// `POST web/password-reset/request` → 202, whatever the address.
pub async fn reset_request(
    State(state): State<AppState>,
    client: Client,
    JsonBody(req): JsonBody<EmailReq>,
) -> ApiResult {
    hit(&state.db, rl::RESET_IP, &ip_key(&state, &client)).await?;
    check_lengths(&[(&req.email, EMAIL_MAX), (&req.turnstile, TURNSTILE_MAX)])?;
    let email = email_or_400(&req.email)?;
    state.turnstile.verify(&state.db, &req.turnstile, "reset", client.ip()).await?;
    let keys = &state.config.keys;
    hit(&state.db, rl::RESET_EMAIL, &email_key(keys, &email)).await?;
    let mut tx = state.db.begin().await?;
    let account: Option<(Uuid, String)> =
        sqlx::query_as("SELECT id, email::text FROM accounts WHERE email = $1::citext FOR UPDATE")
            .bind(&email)
            .fetch_optional(&mut *tx)
            .await?;
    if let Some((id, stored)) = account {
        let token = new_email_token(&mut tx, keys, id, "reset", RESET_TTL_H).await?;
        let link = format!("{}/account/reset?token={token}", state.config.public_base_url);
        mail::enqueue(&mut tx, Some(id), &stored, Template::Reset, json!({ "link": link })).await?;
    }
    tx.commit().await?;
    Ok(status(StatusCode::ACCEPTED, "check_email"))
}

/// `POST web/password-reset/confirm` → 200. Signs the website out everywhere; devices, keys
/// and synced data are untouched (THREAT_MODEL G6). The link also proves the address.
pub async fn reset_confirm(
    State(state): State<AppState>,
    client: Client,
    JsonBody(req): JsonBody<ResetConfirmReq>,
) -> ApiResult {
    hit(&state.db, rl::RESET_CONFIRM_IP, &ip_key(&state, &client)).await?;
    check_lengths(&[
        (&req.token, TOKEN_MAX),
        (&req.new_password, PASSWORD_MAX),
        (req.totp.as_deref().unwrap_or(""), CODE_MAX),
    ])?;
    let row = lookup_token(&state, &req.token, "reset").await?;
    policy(&req.new_password, &row.email)?;
    // The second factor counts like a login's: wrong codes lock the address out, for
    // logins and resets alike (0.11.7, audit S2: a reset link allowed unlimited guesses).
    let keys = &state.config.keys;
    let ekey = email_key(keys, &row.email);
    if let Some(left) = lockout(&state, &ekey).await? {
        return Err(ApiError::rate_limited(left));
    }
    if let Err(e) = second_factor(&state, row.account_id, req.totp.as_deref()).await {
        if e.code == "totp_invalid" {
            login_failed(&state, &ekey).await?;
        }
        return Err(e);
    }
    let phc = state.passwords.hash(&req.new_password).await.map_err(|e| {
        tracing::error!(error = %e, "password hash");
        ApiError::server_error()
    })?;
    let mut tx = state.db.begin().await?;
    let n = sqlx::query("UPDATE email_tokens SET used_at = now() WHERE hash = $1 AND used_at IS NULL")
        .bind(email_token_hash(keys, &req.token).as_slice())
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if n != 1 {
        return Err(ApiError::bad_request("invalid_link", "This link is not valid. Request a new one."));
    }
    sqlx::query(
        "UPDATE accounts SET pw_phc = $2, pepper_id = $3, pw_changed_at = now(),
                email_verified_at = coalesce(email_verified_at, now()) WHERE id = $1",
    )
    .bind(row.account_id)
    .bind(&phc)
    .bind(PEPPER_ID)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM email_tokens WHERE account_id = $1 AND used_at IS NULL")
        .bind(row.account_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE web_sessions SET revoked_at = now() WHERE account_id = $1 AND revoked_at IS NULL")
        .bind(row.account_id)
        .execute(&mut *tx)
        .await?;
    login_succeeded(&mut tx, &email_key(keys, &row.email)).await?;
    mail::enqueue(&mut tx, Some(row.account_id), &row.email, Template::PasswordChanged, json!({})).await?;
    ip_event(&mut tx, row.account_id, "password_reset", &client, Source::None).await?;
    tx.commit().await?;
    Ok(status(StatusCode::OK, "password_reset"))
}

pub(crate) async fn current_phc(state: &AppState, account_id: Uuid) -> Result<String, ApiError> {
    Ok(sqlx::query_scalar("SELECT pw_phc FROM accounts WHERE id = $1").bind(account_id).fetch_one(&state.db).await?)
}

fn account_key(s: &WebSession) -> [u8; 32] {
    rl::id_key(&s.account_id)
}

/// `POST web/password/change` → 200. Other website sessions are signed out.
pub async fn password_change(
    State(state): State<AppState>,
    client: Client,
    s: WebSession,
    JsonBody(req): JsonBody<ChangeReq>,
) -> ApiResult {
    hit(&state.db, rl::PASSWORD_CHANGE, &account_key(&s)).await?;
    check_lengths(&[
        (&req.current, PASSWORD_MAX),
        (&req.new, PASSWORD_MAX),
        (req.totp.as_deref().unwrap_or(""), CODE_MAX),
    ])?;
    if !state.passwords.verify(&req.current, Some(&current_phc(&state, s.account_id).await?)).await {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", "The current password is wrong."));
    }
    second_factor(&state, s.account_id, req.totp.as_deref()).await?;
    policy(&req.new, &s.email)?;
    let phc = state.passwords.hash(&req.new).await.map_err(|e| {
        tracing::error!(error = %e, "password hash");
        ApiError::server_error()
    })?;
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE accounts SET pw_phc = $2, pepper_id = $3, pw_changed_at = now() WHERE id = $1")
        .bind(s.account_id)
        .bind(&phc)
        .bind(PEPPER_ID)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE web_sessions SET revoked_at = now() WHERE account_id = $1 AND id <> $2 AND revoked_at IS NULL")
        .bind(s.account_id)
        .bind(s.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM email_tokens WHERE account_id = $1 AND kind = 'reset'")
        .bind(s.account_id)
        .execute(&mut *tx)
        .await?;
    mail::enqueue(&mut tx, Some(s.account_id), &s.email, Template::PasswordChanged, json!({})).await?;
    ip_event(&mut tx, s.account_id, "password_change", &client, Source::Web(s.id)).await?;
    tx.commit().await?;
    Ok(status(StatusCode::OK, "password_changed"))
}

/// `POST web/reauth` → 204; sensitive actions are allowed for 10 minutes.
pub async fn reauth(State(state): State<AppState>, s: WebSession, JsonBody(req): JsonBody<ReauthReq>) -> ApiResult {
    hit(&state.db, rl::REAUTH, &account_key(&s)).await?;
    check_lengths(&[(&req.password, PASSWORD_MAX), (req.totp.as_deref().unwrap_or(""), CODE_MAX)])?;
    if !state.passwords.verify(&req.password, Some(&current_phc(&state, s.account_id).await?)).await {
        return Err(wrong_password());
    }
    second_factor(&state, s.account_id, req.totp.as_deref()).await?;
    sqlx::query("UPDATE web_sessions SET reauth_at = now() WHERE id = $1").bind(s.id).execute(&state.db).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Serialize)]
struct Me {
    email: String,
    email_verified: bool,
    created: DateTime<Utc>,
    totp: bool,
    backup_codes_left: i64,
    devices: i64,
    sync: SyncSummary,
    reauth_valid: bool,
    csrf: String,
}

#[derive(Serialize)]
struct SyncSummary {
    has_keyset: bool,
    records: i64,
    bytes: i64,
    /// A device was removed and the keys aren't rotated yet (PROTOCOL §7).
    rotation_pending: bool,
}

/// `GET web/me`. `sync` says whether sync is set up, how much it stores, and whether the keys
/// are being rotated; never anything about the data itself.
pub async fn me(State(state): State<AppState>, s: WebSession) -> ApiResult {
    hit(&state.db, rl::ME, &rl::id_key(&s.id)).await?;
    let backup_codes_left: i64 =
        sqlx::query_scalar("SELECT count(*) FROM backup_codes WHERE account_id = $1 AND used_at IS NULL")
            .bind(s.account_id)
            .fetch_one(&state.db)
            .await?;
    let devices = crate::devices::active_count(&state.db, s.account_id).await?;
    let st = crate::keys::state_row(&mut *state.db.acquire().await?, s.account_id, false).await?;
    let sync = SyncSummary {
        has_keyset: st.is_some(),
        records: st.as_ref().map_or(0, |x| i64::from(x.records)),
        bytes: st.as_ref().map_or(0, |x| x.bytes),
        rotation_pending: st.as_ref().is_some_and(|x| x.rotation_ak || x.rotation_pak),
    };
    Ok(Json(Me {
        email: s.email.clone(),
        email_verified: s.email_verified,
        created: s.created_at,
        totp: s.totp_enabled,
        backup_codes_left,
        devices,
        sync,
        reauth_valid: s.reauth_valid(),
        csrf: s.csrf.clone(),
    })
    .into_response())
}

#[derive(Serialize)]
struct TotpSetup {
    secret_b32: String,
    otpauth_uri: String,
    qr_svg: Option<String>,
}

/// `POST web/totp/setup` `{password}` → a new pending secret (not active until enabled).
pub async fn totp_setup(
    State(state): State<AppState>,
    s: WebSession,
    JsonBody(req): JsonBody<PasswordOnly>,
) -> ApiResult {
    hit(&state.db, rl::TOTP_SETUP, &account_key(&s)).await?;
    check_lengths(&[(&req.password, PASSWORD_MAX)])?;
    if s.totp_enabled {
        return Err(ApiError::new(StatusCode::CONFLICT, "conflict", "Two-step verification is already on."));
    }
    if !state.passwords.verify(&req.password, Some(&current_phc(&state, s.account_id).await?)).await {
        return Err(wrong_password());
    }
    let secret = crypto::random::<{ totp::SECRET_LEN }>();
    let enc = crypto::seal(&state.config.keys.totp_enc, s.account_id.as_bytes(), &secret);
    sqlx::query("UPDATE accounts SET totp_pending_enc = $2 WHERE id = $1")
        .bind(s.account_id)
        .bind(enc)
        .execute(&state.db)
        .await?;
    let b32 = totp::base32(&secret);
    let uri = totp::otpauth_uri(&b32, &s.email);
    let qr = crate::qr::svg(&uri);
    Ok(Json(TotpSetup { secret_b32: b32, otpauth_uri: uri, qr_svg: qr }).into_response())
}

/// `POST web/totp/enable` `{code}` → 200 `{backup_codes:[10]}` (shown once).
pub async fn totp_enable(
    State(state): State<AppState>,
    client: Client,
    s: WebSession,
    JsonBody(req): JsonBody<CodeReq>,
) -> ApiResult {
    hit(&state.db, rl::TOTP_ENABLE, &account_key(&s)).await?;
    check_lengths(&[(&req.code, CODE_MAX)])?;
    let keys = &state.config.keys;
    let (pending, last): (Option<Vec<u8>>, i64) =
        sqlx::query_as("SELECT totp_pending_enc, totp_last_step FROM accounts WHERE id = $1 AND NOT totp_enabled")
            .bind(s.account_id)
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(|| ApiError::new(StatusCode::CONFLICT, "conflict", "Two-step verification is already on."))?;
    let pending = pending.ok_or_else(|| ApiError::bad_request("bad_request", "Start the setup first."))?;
    let secret = crypto::open(&keys.totp_enc, s.account_id.as_bytes(), &pending).ok_or_else(|| {
        tracing::error!("the pending TOTP secret can't be decrypted");
        ApiError::server_error()
    })?;
    let now = Utc::now().timestamp().max(0) as u64;
    let step = totp::verify(&secret, &req.code, now, last.max(0) as u64)
        .ok_or_else(|| ApiError::bad_request("totp_invalid", "The code is wrong. Check the time on your phone."))?;
    let codes: Vec<String> = (0..totp::BACKUP_CODES).map(|_| totp::new_backup_code()).collect();
    let mut tx = state.db.begin().await?;
    let n = sqlx::query(
        "UPDATE accounts SET totp_secret_enc = totp_pending_enc, totp_pending_enc = NULL, totp_enabled = true,
                totp_last_step = $2 WHERE id = $1 AND NOT totp_enabled AND totp_pending_enc = $3",
    )
    .bind(s.account_id)
    .bind(step as i64)
    .bind(&pending)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if n != 1 {
        return Err(ApiError::new(StatusCode::CONFLICT, "conflict", "The setup changed meanwhile. Start again."));
    }
    sqlx::query("DELETE FROM backup_codes WHERE account_id = $1").bind(s.account_id).execute(&mut *tx).await?;
    for c in &codes {
        let normalized = totp::normalize_backup_code(c).expect("generated codes are valid");
        sqlx::query("INSERT INTO backup_codes (account_id, code_hash) VALUES ($1, $2)")
            .bind(s.account_id)
            .bind(backup_code_hash(keys, &normalized).as_slice())
            .execute(&mut *tx)
            .await?;
    }
    mail::enqueue(&mut tx, Some(s.account_id), &s.email, Template::TotpEnabled, json!({})).await?;
    ip_event(&mut tx, s.account_id, "totp_enabled", &client, Source::Web(s.id)).await?;
    tx.commit().await?;
    Ok(Json(json!({ "backup_codes": codes })).into_response())
}

/// `POST web/totp/disable` `{password, code}` → 200.
pub async fn totp_disable(
    State(state): State<AppState>,
    client: Client,
    s: WebSession,
    JsonBody(req): JsonBody<DisableReq>,
) -> ApiResult {
    hit(&state.db, rl::TOTP_DISABLE, &account_key(&s)).await?;
    check_lengths(&[(&req.password, PASSWORD_MAX), (&req.code, CODE_MAX)])?;
    if !s.totp_enabled {
        return Err(ApiError::bad_request("bad_request", "Two-step verification is not on."));
    }
    if !state.passwords.verify(&req.password, Some(&current_phc(&state, s.account_id).await?)).await {
        return Err(wrong_password());
    }
    second_factor(&state, s.account_id, Some(&req.code)).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query(
        "UPDATE accounts SET totp_enabled = false, totp_secret_enc = NULL, totp_pending_enc = NULL WHERE id = $1",
    )
    .bind(s.account_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM backup_codes WHERE account_id = $1").bind(s.account_id).execute(&mut *tx).await?;
    mail::enqueue(&mut tx, Some(s.account_id), &s.email, Template::TotpDisabled, json!({})).await?;
    ip_event(&mut tx, s.account_id, "totp_disabled", &client, Source::Web(s.id)).await?;
    tx.commit().await?;
    Ok(status(StatusCode::OK, "totp_disabled"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::truncate_chars;

    #[test]
    fn next_paths_match_the_site() {
        for ok in ["/link?code=ABCD-EFGH", "/account/", "/account/#add-device", "/account/#export", "/account/delete"] {
            assert!(safe_next(ok), "{ok}");
        }
        for bad in [
            "",
            "/",
            "https://evil.example/",
            "//evil.example",
            "/link?code=",
            "/link?code=ABCD-EFGH&x=1",
            "/link?code=ABCDEFGHJKMNP",
            "/account",
            "/account/#other",
            "/link?code=AB%2F",
        ] {
            assert!(!safe_next(bad), "{bad}");
        }
    }

    #[test]
    fn emails() {
        assert_eq!(normalize_email(" Alice@Example.ORG ").as_deref(), Some("Alice@example.org"));
        assert_eq!(normalize_email("a.b+tag@sub.example.co.uk").as_deref(), Some("a.b+tag@sub.example.co.uk"));
        assert_eq!(normalize_email("x@xn--bcher-kva.example").as_deref(), Some("x@xn--bcher-kva.example"));
        for bad in [
            "",
            "plain",
            "@example.org",
            "a@",
            "a@localhost",
            "a@b.123",
            "a b@example.org",
            "a@exa mple.org",
            "a@-x.org",
            "a@x-.org",
            ".a@x.org",
            "a..b@x.org",
            "a<b@x.org",
            "a@b@x.org",
            "a@bücher.de",
            "\"a\"@x.org",
        ] {
            assert_eq!(normalize_email(bad), None, "{bad}");
        }
        assert!(normalize_email(&format!("{}@x.org", "a".repeat(65))).is_none());
        assert!(normalize_email(&format!("a@{}.org", "b".repeat(64))).is_none());
        assert!(normalize_email(&format!("a@{}.org", vec!["b".repeat(60); 5].join("."))).is_none(), "over 254");
        assert_eq!(truncate_chars(&"é".repeat(300), 256).chars().count(), 256);
    }
}
