//! Milestone S3: device linking, device tokens, and the devices and sessions lists
//! (PROTOCOL §6.1, §11.3, §11.4, §11.2; docs/sync/PLAN.md §4.3).
//!
//! **Linking** follows RFC 8628 (the OAuth device grant) as a pattern:
//! 1. Krunk calls `link/authorize` and gets a `device_code` (32 random bytes, kept by the client)
//!    and a `user_code` (8 Crockford symbols, 40 bits, shown as `XXXX-XXXX`), both valid 10
//!    minutes. Only their HMACs are stored.
//! 2. The user, signed in on the website (and re-authenticated within 10 minutes), opens
//!    `/link?code=…`, checks the device, its platform and country, and approves or denies.
//!    Approving mints the device id and emails the account.
//! 3. Krunk polls `link/token` (at least 5 s apart; faster gets `slow_down` and 5 s more) until it
//!    gets the tokens, or `access_denied` / `expired_token`. Every grant is single use.
//!
//! **Tokens** are opaque 32-byte values; the server keeps only HMACs. Access tokens live 15
//! minutes. Refresh tokens rotate on every use; presenting a spent one again revokes the whole
//! family and the device and emails the account (reuse detection). Revoking a device takes effect
//! on its very next request, because the bearer check reads `revoked_at` every time.
//!
//! **No keys:** S3 hands out no key material of any kind; a linked device has none until S6.

use crate::api::{ApiError, ApiResult, Client, JsonBody};
use crate::client_ip;
use crate::config::Keys;
use crate::crypto;
use crate::events::{self, Source};
use crate::http::AppState;
use crate::mail::{self, Template};
use crate::ratelimit::{self as rl, hit};
use crate::sessions::WebSession;
use axum::extract::{FromRequestParts, Path, Request, State};
use axum::http::request::Parts;
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgConnection;
use uuid::Uuid;

pub const LINK_TTL_S: i64 = 600;
/// How long a link grant waits once a registration from its `/link` tab attached it (auto-link):
/// long enough to open the confirmation mail. `link/token` answers `authorization_pending` until
/// then.
pub const AUTO_LINK_TTL_S: i64 = 3600;
pub const POLL_INTERVAL_S: i32 = 5;
pub const ACCESS_TTL_S: i64 = 900;
pub const REFRESH_IDLE_DAYS: i64 = 90;
pub const LABEL_MAX: usize = 64;
pub(crate) const VERSION_MAX: usize = 32;
pub(crate) const TOKEN_MAX: usize = 64;

/// The app version in a Krunk user agent (`Krunk/0.10.1`), if it is one.
fn agent_version(user_agent: Option<&str>) -> Option<&str> {
    let v = user_agent?.strip_prefix("Krunk/")?.split_whitespace().next()?;
    let ok = !v.is_empty()
        && v.len() <= VERSION_MAX
        && v.starts_with(|c: char| c.is_ascii_digit())
        && v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'));
    ok.then_some(v)
}

// ---- Codes ----------------------------------------------------------------------------------

/// Crockford base32 (PROTOCOL §3.2).
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A new user code: 8 symbols (40 bits) from the OS random source, shown as `XXXX-XXXX`.
pub fn new_user_code() -> String {
    let b = crypto::random::<5>();
    let v = b.iter().fold(0u64, |acc, x| (acc << 8) | u64::from(*x));
    let s: String = (0..8).map(|i| CROCKFORD[((v >> (35 - 5 * i)) & 31) as usize] as char).collect();
    format!("{}-{}", &s[..4], &s[4..])
}

/// The canonical 8 symbols of a typed code: case-insensitive, `O`→`0`, `I`/`L`→`1`, dashes and
/// spaces ignored. `None` if it isn't one.
pub fn normalize_user_code(s: &str) -> Option<String> {
    if s.len() > 32 {
        return None;
    }
    let mut out = String::with_capacity(8);
    for c in s.chars() {
        let c = match c.to_ascii_uppercase() {
            '-' | ' ' => continue,
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        };
        if !c.is_ascii() || !CROCKFORD.contains(&(c as u8)) {
            return None;
        }
        out.push(c);
    }
    (out.len() == 8).then_some(out)
}

pub fn display_user_code(normalized: &str) -> String {
    format!("{}-{}", &normalized[..4], &normalized[4..])
}

fn user_code_hash(keys: &Keys, normalized: &str) -> [u8; 32] {
    crypto::hmac(&keys.token_hmac, "krunk-user-code", &[normalized.as_bytes()])
}

pub(crate) fn device_code_hash(keys: &Keys, code: &str) -> [u8; 32] {
    crypto::hmac(&keys.token_hmac, "krunk-device-code", &[code.as_bytes()])
}

fn access_hash(keys: &Keys, token: &str) -> [u8; 32] {
    crypto::hmac(&keys.token_hmac, "krunk-access-token", &[token.as_bytes()])
}

fn refresh_hash(keys: &Keys, token: &str) -> [u8; 32] {
    crypto::hmac(&keys.token_hmac, "krunk-refresh-token", &[token.as_bytes()])
}

/// A device label: trimmed, 1–64 characters, no control characters (it goes into emails).
pub fn clean_label(s: &str) -> Option<String> {
    let s = s.trim();
    let ok = !s.is_empty() && s.chars().count() <= LABEL_MAX && !s.chars().any(char::is_control);
    ok.then(|| s.to_string())
}

/// A label as shown: one the app generated before 0.11.0 ("Krunk 0.8.0 on Windows 11", that exact
/// pattern) is shown without its version, which went stale with the first update (the version is
/// shown from `client_version`). Display only: the stored label is never rewritten, and anything
/// else (a name the user typed) is returned as it is. The app does the same
/// (`core/src/account.rs::display_label`).
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

pub(crate) fn bad() -> ApiError {
    ApiError::bad_request("bad_request", "The request is not valid.")
}

fn grant_error(code: &'static str) -> ApiError {
    let message = match code {
        "authorization_pending" => "Waiting for the approval on the website.",
        "slow_down" => "Polling too fast: wait 5 seconds more between requests.",
        "access_denied" => "The sign-in was denied on the website.",
        _ => "The code has expired. Start again.",
    };
    ApiError::bad_request(code, message)
}

pub fn reauth_required() -> ApiError {
    ApiError::new(StatusCode::FORBIDDEN, "reauth_required", "Enter your password again to continue.")
}

fn link_not_found() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "not_found", "This code is not valid or has expired.")
}

pub fn device_not_found() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "not_found", "Not found.")
}

fn ip_key(state: &AppState, client: &Client) -> [u8; 32] {
    rl::ip_key(&state.config.keys, client.ip())
}

// ---- Native clients -------------------------------------------------------------------------

/// The device endpoints are for native clients, which send no `Origin`. A browser page that
/// tries them from another site is refused outright (CORS would hide the answer anyway).
pub async fn no_foreign_origin(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if let Some(origin) = req.headers().get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        if origin != state.config.public_origin() {
            tracing::info!("cross-origin request to a device endpoint refused");
            return ApiError::forbidden().into_response();
        }
    }
    next.run(req).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizeReq {
    platform: String,
    label: String,
    client_version: String,
}

#[derive(Serialize)]
struct AuthorizeResp {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: String,
    expires_in: i64,
    interval: i32,
    /// 0.13.0: a QR code (SVG) of `verification_uri_complete`, so a phone that is signed in can
    /// approve by scanning it. Additive; clients ignore it or fall back to the code.
    #[serde(skip_serializing_if = "Option::is_none")]
    qr_svg: Option<String>,
}

/// `POST link/authorize` `{platform, label, client_version}` → the codes (PROTOCOL §11.3).
pub async fn authorize(
    State(state): State<AppState>,
    client: Client,
    JsonBody(req): JsonBody<AuthorizeReq>,
) -> ApiResult {
    hit(&state.db, rl::LINK_AUTHORIZE_IP, &ip_key(&state, &client)).await?;
    if !matches!(req.platform.as_str(), "windows" | "android")
        || req.client_version.len() > VERSION_MAX
        || req.client_version.chars().any(char::is_control)
    {
        return Err(bad());
    }
    let label = clean_label(&req.label).ok_or_else(bad)?;
    let keys = &state.config.keys;
    let device_code = crypto::new_token();
    // A clash of 40-bit codes among live grants is all but impossible; retry once anyway.
    for _ in 0..2 {
        let user_code = new_user_code();
        let normalized = normalize_user_code(&user_code).expect("generated codes are valid");
        let inserted = sqlx::query(
            "INSERT INTO link_grants (device_code_hash, user_code_hash, platform, label, client_version, ip, country,
                                      user_agent, expires_at, interval_s)
             VALUES ($1, $2, $3, $4, $5, $6::inet, $7, $8, now() + make_interval(secs => $9), $10)
             ON CONFLICT (user_code_hash) DO NOTHING",
        )
        .bind(device_code_hash(keys, &device_code).as_slice())
        .bind(user_code_hash(keys, &normalized).as_slice())
        .bind(&req.platform)
        .bind(&label)
        .bind(&req.client_version)
        .bind(client.ip().to_string())
        .bind(client.info.country.as_deref())
        .bind(client.user_agent.as_deref())
        .bind(LINK_TTL_S as f64)
        .bind(POLL_INTERVAL_S)
        .execute(&state.db)
        .await?
        .rows_affected();
        if inserted == 1 {
            tracing::info!("link grant created");
            let base = &state.config.public_base_url;
            let complete = format!("{base}/link?code={user_code}");
            return Ok(Json(AuthorizeResp {
                device_code,
                verification_uri: format!("{base}/link"),
                qr_svg: crate::qr::svg(&complete),
                verification_uri_complete: complete,
                user_code,
                expires_in: LINK_TTL_S,
                interval: POLL_INTERVAL_S,
            })
            .into_response());
        }
    }
    Err(ApiError::server_error())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenReq {
    device_code: String,
}

#[derive(Serialize)]
struct Tokens {
    device_id: Uuid,
    account_id: Uuid,
    access_token: String,
    expires_in: i64,
    refresh_token: String,
}

#[derive(sqlx::FromRow)]
struct GrantPoll {
    status: String,
    expired: bool,
    too_fast: bool,
    account_id: Option<Uuid>,
    device_id: Option<Uuid>,
}

/// Issues a new access token for a device.
async fn new_access_token(conn: &mut PgConnection, keys: &Keys, device_id: Uuid) -> Result<String, sqlx::Error> {
    let token = crypto::new_token();
    sqlx::query(
        "INSERT INTO access_tokens (hash, device_id, expires_at) VALUES ($1, $2, now() + make_interval(secs => $3))",
    )
    .bind(access_hash(keys, &token).as_slice())
    .bind(device_id)
    .bind(ACCESS_TTL_S as f64)
    .execute(conn)
    .await?;
    Ok(token)
}

/// `POST link/token` `{device_code}` → the tokens once approved; otherwise 400 with
/// `authorization_pending`, `slow_down`, `expired_token` or `access_denied`.
pub async fn token(State(state): State<AppState>, client: Client, JsonBody(req): JsonBody<TokenReq>) -> ApiResult {
    hit(&state.db, rl::LINK_TOKEN_IP, &ip_key(&state, &client)).await?;
    if req.device_code.len() > TOKEN_MAX || crypto::b64url_decode::<32>(&req.device_code).is_none() {
        return Err(bad());
    }
    let keys = &state.config.keys;
    let hash = device_code_hash(keys, &req.device_code);
    let mut tx = state.db.begin().await?;
    let g: Option<GrantPoll> = sqlx::query_as(
        "SELECT status, expires_at < now() AS expired,
                coalesce(last_poll_at > now() - make_interval(secs => interval_s), false) AS too_fast,
                account_id, device_id
         FROM link_grants WHERE device_code_hash = $1 FOR UPDATE",
    )
    .bind(hash.as_slice())
    .fetch_optional(&mut *tx)
    .await?;
    // Unknown, already used or deleted: the code is no good any more.
    let Some(g) = g else { return Err(grant_error("expired_token")) };
    let delete = "DELETE FROM link_grants WHERE device_code_hash = $1";
    if g.expired {
        sqlx::query(delete).bind(hash.as_slice()).execute(&mut *tx).await?;
        tx.commit().await?;
        return Err(grant_error("expired_token"));
    }
    if g.too_fast {
        sqlx::query(
            "UPDATE link_grants SET interval_s = interval_s + 5, last_poll_at = now() WHERE device_code_hash = $1",
        )
        .bind(hash.as_slice())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Err(grant_error("slow_down"));
    }
    match (g.status.as_str(), g.account_id, g.device_id) {
        ("approved", Some(account_id), Some(device_id)) => {
            let refresh = crypto::new_token();
            sqlx::query("INSERT INTO refresh_families (id, device_id, current_hash) VALUES ($1, $2, $3)")
                .bind(Uuid::new_v4())
                .bind(device_id)
                .bind(refresh_hash(keys, &refresh).as_slice())
                .execute(&mut *tx)
                .await?;
            let access = new_access_token(&mut tx, keys, device_id).await?;
            sqlx::query(
                "UPDATE devices SET last_used_at = now(), last_ip = $2::inet, last_country = $3, user_agent = $4 WHERE id = $1",
            )
            .bind(device_id)
            .bind(client.ip().to_string())
            .bind(client.info.country.as_deref())
            .bind(client.user_agent.as_deref())
            .execute(&mut *tx)
            .await?;
            sqlx::query(delete).bind(hash.as_slice()).execute(&mut *tx).await?;
            tx.commit().await?;
            tracing::info!(device = %device_id, "device tokens issued");
            Ok(Json(Tokens {
                device_id,
                account_id,
                access_token: access,
                expires_in: ACCESS_TTL_S,
                refresh_token: refresh,
            })
            .into_response())
        }
        ("denied", _, _) => {
            sqlx::query(delete).bind(hash.as_slice()).execute(&mut *tx).await?;
            tx.commit().await?;
            Err(grant_error("access_denied"))
        }
        _ => {
            sqlx::query("UPDATE link_grants SET last_poll_at = now() WHERE device_code_hash = $1")
                .bind(hash.as_slice())
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Err(grant_error("authorization_pending"))
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefreshReq {
    refresh_token: String,
}

#[derive(Serialize)]
struct Refreshed {
    access_token: String,
    expires_in: i64,
    refresh_token: String,
}

#[derive(sqlx::FromRow)]
struct FamilyRow {
    id: Uuid,
    device_id: Uuid,
    usable: bool,
}

#[derive(sqlx::FromRow)]
struct ReuseRow {
    family_id: Uuid,
    device_id: Uuid,
    account_id: Uuid,
    email: String,
    label: String,
    already_revoked: bool,
}

/// Revokes a device: its refresh family and access tokens stop working at once. If the device
/// ever held sync keys, the account's keys must be rotated (PROTOCOL §7): writes freeze until the
/// next epoch is committed. Every way of removing a device comes through here.
pub async fn revoke_device(conn: &mut PgConnection, device_id: Uuid) -> Result<(), sqlx::Error> {
    let had_keys: Option<bool> = sqlx::query_scalar(
        "UPDATE devices SET revoked_at = now(), has_keys = false WHERE id = $1 AND revoked_at IS NULL RETURNING had_keys",
    )
    .bind(device_id)
    .fetch_optional(&mut *conn)
    .await?;
    if had_keys == Some(true) {
        sqlx::query(
            "UPDATE sync_state SET rotation_ak = true, rotation_pak = true, updated_at = now()
             WHERE account_id = (SELECT account_id FROM devices WHERE id = $1)",
        )
        .bind(device_id)
        .execute(&mut *conn)
        .await?;
    }
    sqlx::query("UPDATE refresh_families SET revoked_at = now() WHERE device_id = $1 AND revoked_at IS NULL")
        .bind(device_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("DELETE FROM access_tokens WHERE device_id = $1").bind(device_id).execute(&mut *conn).await?;
    Ok(())
}

fn token_unauthorized() -> ApiError {
    ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", "Signed out: sign in again.")
}

/// `POST token/refresh` `{refresh_token}` → new access and refresh tokens (rotation). A spent
/// refresh token revokes the family and the device and emails the account.
pub async fn refresh(State(state): State<AppState>, client: Client, JsonBody(req): JsonBody<RefreshReq>) -> ApiResult {
    hit(&state.db, rl::REFRESH_IP, &ip_key(&state, &client)).await?;
    if req.refresh_token.len() > TOKEN_MAX || crypto::b64url_decode::<32>(&req.refresh_token).is_none() {
        return Err(token_unauthorized());
    }
    let keys = &state.config.keys;
    let h = refresh_hash(keys, &req.refresh_token);
    // The device's limit is counted before the row lock (0.11.7, audit S8): counting it inside
    // took a second connection while holding the lock, and parallel refreshes of one token
    // could use up the pool and stall every request.
    let device: Option<Uuid> = sqlx::query_scalar("SELECT device_id FROM refresh_families WHERE current_hash = $1")
        .bind(h.as_slice())
        .fetch_optional(&state.db)
        .await?;
    if let Some(device) = device {
        hit(&state.db, rl::REFRESH_DEVICE, &rl::id_key(&device)).await?;
    }
    let mut tx = state.db.begin().await?;
    let fam: Option<FamilyRow> = sqlx::query_as(
        "SELECT f.id, f.device_id,
                (f.revoked_at IS NULL AND d.revoked_at IS NULL
                 AND f.rotated_at > now() - make_interval(days => $2)) AS usable
         FROM refresh_families f JOIN devices d ON d.id = f.device_id
         WHERE f.current_hash = $1 FOR UPDATE OF f",
    )
    .bind(h.as_slice())
    .bind(REFRESH_IDLE_DAYS as i32)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(f) = fam {
        if !f.usable {
            return Err(token_unauthorized());
        }
        let new_refresh = crypto::new_token();
        sqlx::query("INSERT INTO refresh_used (hash, family_id) VALUES ($1, $2)")
            .bind(h.as_slice())
            .bind(f.id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE refresh_families SET current_hash = $2, rotated_at = now() WHERE id = $1")
            .bind(f.id)
            .bind(refresh_hash(keys, &new_refresh).as_slice())
            .execute(&mut *tx)
            .await?;
        let access = new_access_token(&mut tx, keys, f.device_id).await?;
        tx.commit().await?;
        return Ok(Json(Refreshed { access_token: access, expires_in: ACCESS_TTL_S, refresh_token: new_refresh })
            .into_response());
    }
    // A spent token presented again: someone holds a copy. Revoke the family and the device.
    let reuse: Option<ReuseRow> = sqlx::query_as(
        "SELECT u.family_id, f.device_id, d.account_id, a.email::text AS email, d.label,
                d.revoked_at IS NOT NULL AS already_revoked
         FROM refresh_used u JOIN refresh_families f ON f.id = u.family_id
         JOIN devices d ON d.id = f.device_id JOIN accounts a ON a.id = d.account_id
         WHERE u.hash = $1 FOR UPDATE OF f",
    )
    .bind(h.as_slice())
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(r) = reuse {
        if !r.already_revoked {
            revoke_device(&mut tx, r.device_id).await?;
            mail::enqueue(&mut tx, Some(r.account_id), &r.email, Template::TokenReuse, json!({ "label": r.label }))
                .await?;
            events::record(&mut tx, r.account_id, "token_reuse", &client, Source::Device(r.device_id)).await?;
            tracing::warn!(device = %r.device_id, family = %r.family_id, "refresh token reuse: device revoked");
        }
        tx.commit().await?;
    }
    Err(token_unauthorized())
}

/// A request with a valid access token of a device that isn't revoked.
#[derive(Debug, Clone)]
pub struct DeviceAuth {
    pub device_id: Uuid,
    pub account_id: Uuid,
    /// The device is in the current account directory (only logged for now, audit S4/C19).
    pub has_keys: bool,
}

#[derive(sqlx::FromRow)]
struct AuthRow {
    device_id: Uuid,
    account_id: Uuid,
    has_keys: bool,
    last_used_at: DateTime<Utc>,
}

/// The token of an `Authorization: Bearer <43 characters>` header.
pub fn bearer(parts: &Parts) -> Option<&str> {
    let v = parts.headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let t = v.strip_prefix("Bearer ")?.trim();
    crypto::b64url_decode::<32>(t).map(|_| t)
}

impl FromRequestParts<AppState> for DeviceAuth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let token = bearer(parts).ok_or_else(token_unauthorized)?;
        let row: Option<AuthRow> = sqlx::query_as(
            "SELECT d.id AS device_id, d.account_id, d.has_keys, d.last_used_at
             FROM access_tokens t JOIN devices d ON d.id = t.device_id
             WHERE t.hash = $1 AND t.expires_at > now() AND d.revoked_at IS NULL",
        )
        .bind(access_hash(&state.config.keys, token).as_slice())
        .fetch_optional(&state.db)
        .await?;
        let row = row.ok_or_else(token_unauthorized)?;
        if (Utc::now() - row.last_used_at).num_seconds() >= 60 {
            let client = Client::from_request_parts(parts, state).await?;
            // The version follows the app's updates (it was the one at link time only).
            sqlx::query(
                "UPDATE devices SET last_used_at = now(), last_ip = $2::inet, last_country = $3,
                        user_agent = coalesce($4, user_agent), client_version = coalesce($5, client_version)
                 WHERE id = $1",
            )
            .bind(row.device_id)
            .bind(client.ip().to_string())
            .bind(client.info.country.as_deref())
            .bind(client.user_agent.as_deref())
            .bind(agent_version(client.user_agent.as_deref()))
            .execute(&state.db)
            .await?;
        }
        Ok(DeviceAuth { device_id: row.device_id, account_id: row.account_id, has_keys: row.has_keys })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

/// `POST device/unlink` → 204: the calling device signs itself out.
pub async fn unlink(State(state): State<AppState>, d: DeviceAuth, JsonBody(_): JsonBody<Empty>) -> ApiResult {
    let mut tx = state.db.begin().await?;
    revoke_device(&mut tx, d.device_id).await?;
    tx.commit().await?;
    tracing::info!(device = %d.device_id, "device unlinked itself");
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Serialize, sqlx::FromRow)]
struct DeviceMe {
    email: String,
    account_id: Uuid,
    device_id: Uuid,
    label: String,
    platform: String,
    created: DateTime<Utc>,
}

/// `GET device/me` → who this device is linked to (0.8.0 addition to PROTOCOL §11.4).
pub async fn device_me(State(state): State<AppState>, d: DeviceAuth) -> ApiResult {
    hit(&state.db, rl::DEVICE_ME, &rl::id_key(&d.device_id)).await?;
    let me: DeviceMe = sqlx::query_as(
        "SELECT a.email::text AS email, a.id AS account_id, d.id AS device_id, d.label, d.platform, d.created_at AS created
         FROM devices d JOIN accounts a ON a.id = d.account_id WHERE d.id = $1 AND d.account_id = $2",
    )
    .bind(d.device_id)
    .bind(d.account_id)
    .fetch_one(&state.db)
    .await?;
    let me = DeviceMe { label: display_label(&me.label), ..me };
    Ok(Json(me).into_response())
}

// ---- Website: approval page -----------------------------------------------------------------

#[derive(sqlx::FromRow)]
pub(crate) struct GrantView {
    pub(crate) label: String,
    pub(crate) platform: String,
    pub(crate) country: Option<String>,
    pub(crate) age_s: f64,
    pub(crate) hash: Vec<u8>,
}

/// Counts one code lookup against the brute-force limits: 5 per session per 10 minutes, 20 per
/// IP per hour (PROTOCOL §6.1). Viewing a code always counts.
async fn lookup_limits(state: &AppState, s: &WebSession, client: &Client) -> Result<(), ApiError> {
    hit(&state.db, rl::LINK_LOOKUP_SESSION, &rl::id_key(&s.id)).await?;
    hit(&state.db, rl::LINK_LOOKUP_IP, &ip_key(state, client)).await
}

/// Approving or denying needs the limits not to be used up, but counts only when the code isn't
/// found: acting on the code just viewed (and retrying after a re-auth) costs nothing more, while
/// guessing through these endpoints costs the same as through the view.
async fn act_limits_check(state: &AppState, s: &WebSession, client: &Client) -> Result<(), ApiError> {
    rl::check(&state.db, rl::LINK_LOOKUP_SESSION, &rl::id_key(&s.id)).await?;
    rl::check(&state.db, rl::LINK_LOOKUP_IP, &ip_key(state, client)).await
}

async fn act_not_found(state: &AppState, s: &WebSession, client: &Client) -> ApiError {
    match lookup_limits(state, s, client).await {
        Err(e) => e,
        Ok(()) => link_not_found(),
    }
}

async fn pending_grant(
    conn: &mut PgConnection,
    keys: &Keys,
    code: &str,
    lock: bool,
) -> Result<Option<GrantView>, ApiError> {
    let Some(normalized) = normalize_user_code(code) else { return Ok(None) };
    let sql = format!(
        "SELECT label, platform, country, extract(epoch FROM now() - created_at)::float8 AS age_s,
                device_code_hash AS hash
         FROM link_grants WHERE user_code_hash = $1 AND status = 'pending' AND expires_at > now(){}",
        if lock { " FOR UPDATE" } else { "" }
    );
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(user_code_hash(keys, &normalized).as_slice())
        .fetch_optional(conn)
        .await?)
}

/// Auto-link, at registration: the device-code HMAC of the grant behind `code` if the code is
/// pending and unexpired, the grant was asked for by Krunk itself (its stored user agent parses
/// as `Krunk/x.y`; Krunk's tabs send the engine's user agent, so the register request's own can't
/// tell) and the registration comes from the same client IP as the grant. Such a grant then waits
/// [`AUTO_LINK_TTL_S`]. `None` otherwise: the code is ignored and `/link` works as always. Run for
/// every registration, so the grant's extension says nothing about the address.
pub(crate) async fn attach_for_registration(
    conn: &mut PgConnection,
    keys: &Keys,
    code: &str,
    client: &Client,
) -> Result<Option<Vec<u8>>, sqlx::Error> {
    let Some(normalized) = normalize_user_code(code) else { return Ok(None) };
    let row: Option<(Vec<u8>, Option<String>, bool)> = sqlx::query_as(
        "SELECT device_code_hash, user_agent, ip IS NOT DISTINCT FROM $2::inet
         FROM link_grants WHERE user_code_hash = $1 AND status = 'pending' AND expires_at > now() FOR UPDATE",
    )
    .bind(user_code_hash(keys, &normalized).as_slice())
    .bind(client.ip().to_string())
    .fetch_optional(&mut *conn)
    .await?;
    let Some((hash, ua, same_ip)) = row else { return Ok(None) };
    if !same_ip || agent_version(ua.as_deref()).is_none() {
        tracing::info!(same_ip, "link code on a registration ignored");
        return Ok(None);
    }
    sqlx::query(
        "UPDATE link_grants SET expires_at = greatest(expires_at, now() + make_interval(secs => $2))
         WHERE device_code_hash = $1",
    )
    .bind(&hash)
    .bind(AUTO_LINK_TTL_S as f64)
    .execute(&mut *conn)
    .await?;
    tracing::info!("link grant attached to a registration");
    Ok(Some(hash))
}

/// The grant a registration attached, if it is still pending and unexpired (locked).
pub(crate) async fn attached_grant(conn: &mut PgConnection, hash: &[u8]) -> Result<Option<GrantView>, sqlx::Error> {
    sqlx::query_as(
        "SELECT label, platform, country, extract(epoch FROM now() - created_at)::float8 AS age_s,
                device_code_hash AS hash
         FROM link_grants WHERE device_code_hash = $1 AND status = 'pending' AND expires_at > now() FOR UPDATE",
    )
    .bind(hash)
    .fetch_optional(conn)
    .await
}

#[derive(Serialize)]
struct LinkView {
    user_code: String,
    label: String,
    platform: String,
    country: Option<String>,
    your_country: Option<String>,
    age_s: i64,
}

/// `GET web/link/{code}` → what asks to be linked.
pub async fn link_view(
    State(state): State<AppState>,
    client: Client,
    s: WebSession,
    Path(code): Path<String>,
) -> ApiResult {
    lookup_limits(&state, &s, &client).await?;
    let mut conn = state.db.acquire().await?;
    let g = pending_grant(&mut conn, &state.config.keys, &code, false).await?.ok_or_else(link_not_found)?;
    let normalized = normalize_user_code(&code).ok_or_else(link_not_found)?;
    Ok(Json(LinkView {
        user_code: display_user_code(&normalized),
        label: g.label,
        platform: g.platform,
        country: g.country,
        your_country: client.info.country.clone(),
        age_s: g.age_s.max(0.0) as i64,
    })
    .into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApproveReq {
    confirm_code: String,
}

/// `POST web/link/{code}/approve` `{confirm_code}` → 200 `{device_id, short}`. Needs a re-auth
/// within 10 minutes and the code typed or confirmed again.
pub async fn link_approve(
    State(state): State<AppState>,
    client: Client,
    s: WebSession,
    Path(code): Path<String>,
    JsonBody(req): JsonBody<ApproveReq>,
) -> ApiResult {
    act_limits_check(&state, &s, &client).await?;
    hit(&state.db, rl::LINK_APPROVE, &rl::id_key(&s.account_id)).await?;
    if !s.reauth_valid() {
        return Err(reauth_required());
    }
    let (Some(a), Some(b)) = (normalize_user_code(&code), normalize_user_code(&req.confirm_code)) else {
        return Err(act_not_found(&state, &s, &client).await);
    };
    if !crypto::ct_eq(a.as_bytes(), b.as_bytes()) {
        return Err(ApiError::bad_request("bad_request", "The code you confirmed is not the one on this page."));
    }
    let keys = &state.config.keys;
    let mut tx = state.db.begin().await?;
    let Some(g) = pending_grant(&mut tx, keys, &code, true).await? else {
        drop(tx);
        return Err(act_not_found(&state, &s, &client).await);
    };
    let (device_id, short) = approve_grant(&mut tx, &state, &Approver::from(&s), &client, &g).await?;
    tx.commit().await?;
    tracing::info!(device = %device_id, "link approved");
    Ok(Json(json!({ "device_id": device_id, "short": short })).into_response())
}

/// Who approves a link grant: the signed-in account, and the website session the event is
/// recorded under (`/link`, "Add a device", or the session a verification just created).
pub(crate) struct Approver {
    pub account_id: Uuid,
    pub email: String,
    pub session_id: Uuid,
}

impl From<&WebSession> for Approver {
    fn from(s: &WebSession) -> Self {
        Approver { account_id: s.account_id, email: s.email.clone(), session_id: s.id }
    }
}

/// Approves a pending, locked link grant for the signed-in account: the device quota, the next
/// free `short`, the device row, the `device_linked` mail and the event. Shared by `/link` and by
/// "Add a device" (`pairing.rs`); the caller commits. Returns `(device_id, short)`.
pub(crate) async fn approve_grant(
    tx: &mut PgConnection,
    state: &AppState,
    s: &Approver,
    client: &Client,
    g: &GrantView,
) -> Result<(Uuid, i32), ApiError> {
    let active: i64 =
        sqlx::query_scalar("SELECT count(*) FROM devices d WHERE d.account_id = $1 AND d.revoked_at IS NULL")
            .bind(s.account_id)
            .fetch_one(&mut *tx)
            .await?;
    if active >= i64::from(state.config.quota_max_devices) {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "quota_exceeded",
            "Your account has the most devices it can have. Remove one first.",
        ));
    }
    // Serialise approvals of one account (the next free short).
    sqlx::query("SELECT id FROM accounts WHERE id = $1 FOR UPDATE").bind(s.account_id).execute(&mut *tx).await?;
    let short: i32 = sqlx::query_scalar(
        "SELECT min(n)::int FROM generate_series(0, 65535) n
         WHERE NOT EXISTS (SELECT 1 FROM devices WHERE account_id = $1 AND short = n)",
    )
    .bind(s.account_id)
    .fetch_one(&mut *tx)
    .await?;
    let device_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO devices (id, account_id, short, platform, label, client_version, last_ip, last_country, user_agent)
         SELECT $1, $2, $3, platform, label, client_version, ip, country, user_agent FROM link_grants
         WHERE device_code_hash = $4",
    )
    .bind(device_id)
    .bind(s.account_id)
    .bind(short)
    .bind(&g.hash)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE link_grants SET status = 'approved', account_id = $2, device_id = $3 WHERE device_code_hash = $1",
    )
    .bind(&g.hash)
    .bind(s.account_id)
    .bind(device_id)
    .execute(&mut *tx)
    .await?;
    let when = Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
    mail::enqueue(
        &mut *tx,
        Some(s.account_id),
        &s.email,
        Template::DeviceLinked,
        json!({
            "label": g.label,
            "platform": platform_name(&g.platform),
            "country": g.country.clone().unwrap_or_else(|| "an unknown country".into()),
            "time": when,
        }),
    )
    .await?;
    events::record(&mut *tx, s.account_id, "device_linked", client, Source::Web(s.session_id)).await?;
    Ok((device_id, short))
}

/// `POST web/link/{code}/deny` → 204.
pub async fn link_deny(
    State(state): State<AppState>,
    client: Client,
    s: WebSession,
    Path(code): Path<String>,
    JsonBody(_): JsonBody<Empty>,
) -> ApiResult {
    act_limits_check(&state, &s, &client).await?;
    let mut tx = state.db.begin().await?;
    let Some(g) = pending_grant(&mut tx, &state.config.keys, &code, true).await? else {
        drop(tx);
        return Err(act_not_found(&state, &s, &client).await);
    };
    sqlx::query("UPDATE link_grants SET status = 'denied' WHERE device_code_hash = $1")
        .bind(&g.hash)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    tracing::info!("link denied");
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub(crate) fn platform_name(p: &str) -> &'static str {
    match p {
        "android" => "Android",
        _ => "Windows",
    }
}

// ---- Website: devices and sessions ----------------------------------------------------------

#[derive(Serialize, sqlx::FromRow)]
pub struct DeviceItem {
    pub id: Uuid,
    short: i32,
    platform: String,
    label: String,
    client_version: String,
    created: DateTime<Utc>,
    last_used: DateTime<Utc>,
    last_sync: Option<DateTime<Utc>>,
    last_country: Option<String>,
    /// 0.13.0: the full last IP, for the account owner (purged after 90 days).
    last_ip: Option<String>,
    has_keys: bool,
    revoked_at: Option<DateTime<Utc>>,
    /// Set in the device API's list (PROTOCOL §11.8): the calling device.
    #[sqlx(skip)]
    #[serde(rename = "self", skip_serializing_if = "Option::is_none")]
    pub self_: Option<bool>,
}

/// The account's devices: linked ones and those revoked in the last 30 days. An approved link
/// whose tokens were never collected isn't a device yet and isn't listed.
pub async fn list_devices(db: &sqlx::PgPool, account_id: Uuid) -> Result<Vec<DeviceItem>, sqlx::Error> {
    let rows: Vec<DeviceItem> = sqlx::query_as(
        "SELECT d.id, d.short, d.platform, d.label, d.client_version, d.created_at AS created,
                d.last_used_at AS last_used, d.last_sync_at AS last_sync, d.last_country,
                host(d.last_ip) AS last_ip, d.has_keys, d.revoked_at
         FROM devices d WHERE d.account_id = $1
           AND EXISTS (SELECT 1 FROM refresh_families f WHERE f.device_id = d.id)
         ORDER BY d.revoked_at IS NOT NULL, d.last_used_at DESC",
    )
    .bind(account_id)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|d| DeviceItem { label: display_label(&d.label), ..d }).collect())
}

/// `GET web/devices`: the account's devices (linked ones and those revoked in the last 30 days).
/// An approved link whose tokens were never collected isn't a device yet and isn't listed.
pub async fn devices_list(State(state): State<AppState>, s: WebSession) -> ApiResult {
    hit(&state.db, rl::DEVICES_LIST, &rl::id_key(&s.id)).await?;
    Ok(Json(list_devices(&state.db, s.account_id).await?).into_response())
}

async fn own_device(conn: &mut PgConnection, account_id: Uuid, id: &str) -> Result<(Uuid, String, bool), ApiError> {
    let id: Uuid = id.parse().map_err(|_| device_not_found())?;
    let row: Option<(String, bool)> = sqlx::query_as(
        "SELECT label, revoked_at IS NOT NULL FROM devices WHERE id = $1 AND account_id = $2 FOR UPDATE",
    )
    .bind(id)
    .bind(account_id)
    .fetch_optional(conn)
    .await?;
    let (label, revoked) = row.ok_or_else(device_not_found)?;
    Ok((id, label, revoked))
}

async fn revoke_own_device(state: &AppState, s: &WebSession, client: &Client, id: &str) -> Result<(), ApiError> {
    hit(&state.db, rl::DEVICE_REVOKE, &rl::id_key(&s.account_id)).await?;
    if !s.reauth_valid() {
        return Err(reauth_required());
    }
    let id: Uuid = id.parse().map_err(|_| device_not_found())?;
    revoke_from(state, s.account_id, id, client, "the website", Source::Web(s.id)).await
}

/// Removes device `id` of the account (from the website or from another device): revoked, the
/// account mailed, the event kept with the remover's IP.
pub async fn revoke_from(
    state: &AppState,
    account_id: Uuid,
    id: Uuid,
    client: &Client,
    by: &str,
    source: Source,
) -> Result<(), ApiError> {
    let mut tx = state.db.begin().await?;
    let (id, label, revoked) = own_device(&mut tx, account_id, &id.to_string()).await?;
    if !revoked {
        revoke_device(&mut tx, id).await?;
        let email: String = sqlx::query_scalar("SELECT email::text FROM accounts WHERE id = $1")
            .bind(account_id)
            .fetch_one(&mut *tx)
            .await?;
        mail::enqueue(&mut tx, Some(account_id), &email, Template::DeviceRevoked, json!({ "label": label })).await?;
        events::record(&mut tx, account_id, "device_revoked", client, source).await?;
    }
    tx.commit().await?;
    tracing::info!(device = %id, "device revoked from {by}");
    Ok(())
}

/// `POST web/devices/{id}/revoke` (re-auth) → 200. The device's tokens die at once.
pub async fn device_revoke(
    State(state): State<AppState>,
    client: Client,
    s: WebSession,
    Path(id): Path<String>,
    JsonBody(_): JsonBody<Empty>,
) -> ApiResult {
    revoke_own_device(&state, &s, &client, &id).await?;
    Ok(Json(json!({ "status": "revoked" })).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameReq {
    label: String,
}

/// `POST web/devices/{id}/rename` `{label}` → 200 (0.8.0 addition: the server-held label, until
/// the encrypted device name of S6).
pub async fn device_rename(
    State(state): State<AppState>,
    s: WebSession,
    Path(id): Path<String>,
    JsonBody(req): JsonBody<RenameReq>,
) -> ApiResult {
    hit(&state.db, rl::DEVICE_RENAME, &rl::id_key(&s.account_id)).await?;
    let label = clean_label(&req.label)
        .ok_or_else(|| ApiError::bad_request("bad_request", "A name has 1 to 64 characters."))?;
    let mut tx = state.db.begin().await?;
    let (id, _, revoked) = own_device(&mut tx, s.account_id, &id).await?;
    if revoked {
        return Err(device_not_found());
    }
    sqlx::query("UPDATE devices SET label = $2 WHERE id = $1").bind(id).bind(&label).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(json!({ "label": label })).into_response())
}

#[derive(Serialize)]
pub struct SessionItem {
    id: Uuid,
    kind: &'static str,
    created: DateTime<Utc>,
    last_used: DateTime<Utc>,
    ip_masked: Option<String>,
    /// 0.13.0: the full IP, for the account owner (purged after 90 days).
    ip: Option<String>,
    country: Option<String>,
    user_agent: Option<String>,
    label: Option<String>,
    /// 0.13.0: how a website session was signed in, `password`, `qr` or (2026-11-17) `device`, a
    /// Krunk signed in to the account (its name in `label`); devices: absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    via: Option<String>,
    current: bool,
}

#[derive(sqlx::FromRow)]
struct SessionRow {
    id: Uuid,
    created_at: DateTime<Utc>,
    last_used_at: DateTime<Utc>,
    ip: Option<String>,
    country: Option<String>,
    user_agent: Option<String>,
    label: Option<String>,
    via: Option<String>,
}

fn mask(ip: Option<String>) -> Option<String> {
    ip.and_then(|s| s.parse().ok()).map(client_ip::masked)
}

/// `GET web/sessions`: the website sessions and the linked devices of the account.
pub async fn sessions_list(State(state): State<AppState>, s: WebSession) -> ApiResult {
    hit(&state.db, rl::SESSIONS_LIST, &rl::id_key(&s.id)).await?;
    Ok(Json(list_sessions(&state.db, s.account_id, Some(s.id)).await?).into_response())
}

/// The website sessions and the linked devices of an account; `current` marks that website
/// session.
pub async fn list_sessions(
    db: &sqlx::PgPool,
    account_id: Uuid,
    current: Option<Uuid>,
) -> Result<Vec<SessionItem>, sqlx::Error> {
    let web: Vec<SessionRow> = sqlx::query_as(
        "SELECT s.id, s.created_at, s.last_used_at, host(s.ip) AS ip, s.country, s.user_agent,
                CASE WHEN s.via = 'device' THEN d.label END AS label, coalesce(s.via, 'password') AS via
         FROM web_sessions s LEFT JOIN devices d ON d.id = s.via_device
         WHERE s.account_id = $1 AND s.revoked_at IS NULL
           AND s.created_at > now() - make_interval(secs => $2) AND s.last_used_at > now() - make_interval(secs => $3)
         ORDER BY s.last_used_at DESC",
    )
    .bind(account_id)
    .bind(crate::sessions::MAX_AGE_S as f64)
    .bind(crate::sessions::IDLE_S as f64)
    .fetch_all(db)
    .await?;
    let dev: Vec<SessionRow> = sqlx::query_as(
        "SELECT d.id, d.created_at, d.last_used_at, host(d.last_ip) AS ip, d.last_country AS country, d.user_agent,
                d.label, NULL::text AS via
         FROM devices d WHERE d.account_id = $1 AND d.revoked_at IS NULL
           AND EXISTS (SELECT 1 FROM refresh_families f WHERE f.device_id = d.id)
         ORDER BY d.last_used_at DESC",
    )
    .bind(account_id)
    .fetch_all(db)
    .await?;
    Ok(web
        .into_iter()
        .map(|r| (r, "web"))
        .chain(dev.into_iter().map(|r| (r, "device")))
        .map(|(r, kind)| SessionItem {
            current: kind == "web" && Some(r.id) == current,
            id: r.id,
            kind,
            created: r.created_at,
            last_used: r.last_used_at,
            ip_masked: mask(r.ip.clone()),
            ip: r.ip,
            via: r.via,
            country: r.country,
            user_agent: r.user_agent,
            label: r.label.as_deref().map(display_label),
        })
        .collect())
}

/// `POST web/sessions/{id}/revoke` → 204. A website session needs no re-auth (so a suspicious
/// one can be ended quickly); a device does, as on the devices list.
pub async fn session_revoke(
    State(state): State<AppState>,
    client: Client,
    s: WebSession,
    Path(id): Path<String>,
    JsonBody(_): JsonBody<Empty>,
) -> ApiResult {
    hit(&state.db, rl::SESSION_REVOKE, &rl::id_key(&s.account_id)).await?;
    let uid: Uuid = id.parse().map_err(|_| device_not_found())?;
    let n = sqlx::query(
        "UPDATE web_sessions SET revoked_at = now() WHERE id = $1 AND account_id = $2 AND revoked_at IS NULL",
    )
    .bind(uid)
    .bind(s.account_id)
    .execute(&state.db)
    .await?
    .rows_affected();
    if n == 0 {
        revoke_own_device(&state, &s, &client, &id).await?;
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevokeAllReq {
    #[serde(default)]
    include_devices: bool,
}

/// `POST web/sessions/revoke-all` `{include_devices}` (re-auth) → 204 (0.9.0, "Sign out
/// everywhere else"): every other website session ends; with `include_devices` every linked
/// device is removed too, as if removed one by one (tokens dead at once, a mail each).
pub async fn sessions_revoke_all(
    State(state): State<AppState>,
    client: Client,
    s: WebSession,
    JsonBody(req): JsonBody<RevokeAllReq>,
) -> ApiResult {
    hit(&state.db, rl::SESSIONS_REVOKE_ALL, &rl::id_key(&s.account_id)).await?;
    if !s.reauth_valid() {
        return Err(reauth_required());
    }
    let mut tx = state.db.begin().await?;
    let web = sqlx::query(
        "UPDATE web_sessions SET revoked_at = now() WHERE account_id = $1 AND id <> $2 AND revoked_at IS NULL",
    )
    .bind(s.account_id)
    .bind(s.id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    let mut devices = 0;
    if req.include_devices {
        let rows: Vec<(Uuid, String)> = sqlx::query_as(
            "SELECT id, label FROM devices WHERE account_id = $1 AND revoked_at IS NULL ORDER BY created_at FOR UPDATE",
        )
        .bind(s.account_id)
        .fetch_all(&mut *tx)
        .await?;
        for (id, label) in rows {
            revoke_device(&mut tx, id).await?;
            mail::enqueue(&mut tx, Some(s.account_id), &s.email, Template::DeviceRevoked, json!({ "label": label }))
                .await?;
            devices += 1;
        }
    }
    events::record(&mut tx, s.account_id, "sessions_revoked", &client, Source::Web(s.id)).await?;
    tx.commit().await?;
    tracing::info!(web, devices, "signed out everywhere else");
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// Active (linked, not revoked) devices of an account, for `web/me`.
pub async fn active_count(db: &sqlx::PgPool, account_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*) FROM devices d WHERE d.account_id = $1 AND d.revoked_at IS NULL
           AND EXISTS (SELECT 1 FROM refresh_families f WHERE f.device_id = d.id)",
    )
    .bind(account_id)
    .fetch_one(db)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_versions() {
        assert_eq!(agent_version(Some("Krunk/0.10.1")), Some("0.10.1"));
        assert_eq!(agent_version(Some("Krunk/1.2.3-beta+4 (Windows)")), Some("1.2.3-beta+4"));
        for ua in [None, Some(""), Some("Krunk/"), Some("Mozilla/5.0"), Some("krunk/0.10.1"), Some("Krunk/x1")] {
            assert_eq!(agent_version(ua), None, "{ua:?}");
        }
        assert_eq!(agent_version(Some(&format!("Krunk/1.{}", "0".repeat(40)))), None, "too long for the column");
        assert_eq!(agent_version(Some("Krunk/1.0<script>")), None);
    }

    #[test]
    fn user_codes() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..2000 {
            let c = new_user_code();
            assert_eq!(c.len(), 9, "{c}");
            assert_eq!(&c[4..5], "-");
            let n = normalize_user_code(&c).unwrap();
            assert_eq!(display_user_code(&n), c);
            assert!(n.bytes().all(|b| CROCKFORD.contains(&b)), "{c}");
            seen.insert(c);
        }
        assert!(seen.len() > 1990, "40 random bits: no visible repetition");
        // Every symbol shows up (the 5-bit groups are uniform).
        let all: std::collections::HashSet<char> = seen.iter().flat_map(|c| c.chars()).filter(|c| *c != '-').collect();
        assert_eq!(all.len(), 32);
    }

    #[test]
    fn typed_codes_are_forgiving() {
        assert_eq!(normalize_user_code("abcd-efgh").as_deref(), Some("ABCDEFGH"));
        assert_eq!(normalize_user_code(" ab cd ef gh ").as_deref(), Some("ABCDEFGH"));
        assert_eq!(normalize_user_code("OIL0-1234").as_deref(), Some("01101234"));
        for bad in ["", "ABCD-EFG", "ABCD-EFGHJ", "ABCD-EFGU", "ABCD_EFGH", "ÄBCD-EFGH", &"A".repeat(40)] {
            assert_eq!(normalize_user_code(bad), None, "{bad}");
        }
    }

    #[test]
    fn shown_labels() {
        for (label, shown) in [
            ("Krunk 0.8.0 on Windows 11", "Krunk on Windows 11"),
            ("Krunk 0.10.2 on Windows 10", "Krunk on Windows 10"),
            ("Krunk 1.0.0-rc.1 on Android", "Krunk on Android"),
            ("Krunk on Windows 11", "Krunk on Windows 11"),
            ("Krunk 0.8.0 on Windows 11 (work)", "Krunk 0.8.0 on Windows 11 (work)"),
            ("My Krunk 0.8.0 on Windows 11", "My Krunk 0.8.0 on Windows 11"),
            ("Krunk 0.8 beta on Windows 11", "Krunk 0.8 beta on Windows 11"),
            ("Krunk 0.8.0 on Linux", "Krunk 0.8.0 on Linux"),
            ("Living room PC", "Living room PC"),
        ] {
            assert_eq!(display_label(label), shown, "{label}");
        }
    }

    #[test]
    fn labels() {
        assert_eq!(clean_label("  Krunk on Windows 11 ").as_deref(), Some("Krunk on Windows 11"));
        assert_eq!(clean_label(&"é".repeat(64)).map(|s| s.chars().count()), Some(64));
        for bad in ["", "   ", "two\nlines", "tab\there", &"x".repeat(65)] {
            assert_eq!(clean_label(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn hashes_are_domain_separated() {
        let k = Keys { token_hmac: [1; 32], password_pepper: [2; 32], totp_enc: [3; 32] };
        let t = crypto::new_token();
        let all = [access_hash(&k, &t), refresh_hash(&k, &t), device_code_hash(&k, &t), user_code_hash(&k, &t)];
        for i in 0..all.len() {
            for j in i + 1..all.len() {
                assert_ne!(all[i], all[j]);
            }
        }
    }
}
