//! Website sign-in from a signed-in Krunk (service 2026-11-17).
//!
//! When Krunk, signed in to an account, opens the website's sign-in page (a visit to the site
//! without a website session ends up there), it trades its device token for a single-use code
//! (`POST device/web-sign-in`, 60 seconds, only its HMAC stored) and opens
//! `/account/login?auto=<code>`. The page trades the code for a website session
//! (`POST web/device-sign-in`), so the site is signed in like the browser.
//!
//! - The session is `via = 'device'`, names the device in the list of signed-in browsers and
//!   starts **without** a re-auth: changing the password, two-step verification, removing
//!   devices or deleting the account still ask for the password. A device token alone never
//!   gives full control of the account.
//! - The code works once, for 60 seconds, and dies with the device (revoked or removed).
//! - Krunk only does this on its own site, never in private tabs, and not right after the user
//!   signed out of the website (the app's side).

use crate::api::{ApiError, ApiResult, Client, JsonBody};
use crate::config::Keys;
use crate::crypto;
use crate::devices::{DeviceAuth, Empty};
use crate::events::{self, Source};
use crate::http::AppState;
use crate::ratelimit::{self as rl, hit};
use crate::sessions;
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

pub const CODE_TTL_S: i64 = 60;

fn code_hash(keys: &Keys, code: &str) -> [u8; 32] {
    crypto::hmac(&keys.token_hmac, "krunk-web-sign-in", &[code.as_bytes()])
}

/// `POST device/web-sign-in` (bearer) → `{code, expires_in}`.
pub async fn issue(State(state): State<AppState>, d: DeviceAuth, JsonBody(_): JsonBody<Empty>) -> ApiResult {
    hit(&state.db, rl::WEB_SIGN_IN_DEVICE, &rl::id_key(&d.device_id)).await?;
    let code = crypto::new_token();
    sqlx::query(
        "INSERT INTO web_sign_in_codes (hash, account_id, device_id, expires_at)
         VALUES ($1, $2, $3, now() + make_interval(secs => $4))",
    )
    .bind(code_hash(&state.config.keys, &code).as_slice())
    .bind(d.account_id)
    .bind(d.device_id)
    .bind(CODE_TTL_S as f64)
    .execute(&state.db)
    .await?;
    Ok(Json(json!({ "code": code, "expires_in": CODE_TTL_S })).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedeemReq {
    code: String,
}

fn invalid() -> ApiError {
    ApiError::bad_request("invalid_code", "This sign-in link has expired. Sign in below.")
}

/// `POST web/device-sign-in {code}` → 200 `{csrf}` + a website session (via `device`); 400
/// `invalid_code` when unknown, used, expired, or the device is gone.
pub async fn redeem(State(state): State<AppState>, client: Client, JsonBody(req): JsonBody<RedeemReq>) -> ApiResult {
    hit(&state.db, rl::WEB_SIGN_IN_IP, &rl::ip_key(&state.config.keys, client.ip())).await?;
    if req.code.len() > 64 || crypto::b64url_decode::<32>(&req.code).is_none() {
        return Err(invalid());
    }
    let keys = &state.config.keys;
    let mut tx = state.db.begin().await?;
    // Single use: the row goes whatever the outcome.
    let row: Option<(Uuid, Uuid, bool)> = sqlx::query_as(
        "DELETE FROM web_sign_in_codes c WHERE c.hash = $1
         RETURNING c.account_id, c.device_id,
                   c.expires_at > now() AND EXISTS (SELECT 1 FROM devices d WHERE d.id = c.device_id AND d.revoked_at IS NULL)",
    )
    .bind(code_hash(keys, &req.code).as_slice())
    .fetch_optional(&mut *tx)
    .await?;
    let Some((account_id, device_id, true)) = row else {
        tx.commit().await?;
        return Err(invalid());
    };
    let (session_id, token, csrf) =
        sessions::create(&mut tx, keys, account_id, &client, sessions::Via::Device, false).await?;
    sqlx::query("UPDATE web_sessions SET via_device = $2 WHERE id = $1")
        .bind(session_id)
        .bind(device_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE accounts SET last_active_at = now() WHERE id = $1").bind(account_id).execute(&mut *tx).await?;
    events::record(&mut tx, account_id, "device_sign_in", &client, Source::Device(device_id)).await?;
    tx.commit().await?;
    tracing::info!(device = %device_id, "website signed in by a device");
    let mut res = (StatusCode::OK, Json(json!({ "csrf": csrf }))).into_response();
    res.headers_mut().insert(header::SET_COOKIE, sessions::set_cookie(&token));
    Ok(res)
}
