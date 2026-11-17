//! 0.13.0: "Add a device" by QR code (PROTOCOL §6.5).
//!
//! 1. The signed-in browser (re-authenticated within 10 minutes) calls `web/pair/start` and shows
//!    a QR code of `<base>/pair#<token>`: 32 random bytes, valid 5 minutes, single use. The token
//!    sits in the URL fragment, so it never reaches a request log; only its HMAC is stored. One
//!    pairing per account: a new one replaces the old.
//! 2. A phone scans it and claims the token:
//!    - **natively** (the Android app; `krunk-sync-cli pair` today) with `link/pair/claim`, which
//!      creates an ordinary pending link grant tied to the pairing; the app then polls
//!      `link/token` exactly as after `link/authorize`;
//!    - **in a browser** (`/pair`) with `web/pair/claim`, then polls `web/pair/poll` with a poll
//!      token of its own.
//!
//!    Either way the phone shows a 6-digit `confirm_code`.
//! 3. The signed-in browser polls `web/pair`, shows who claimed it (kind, label or browser, IP,
//!    country, how long ago) and the same code; the user checks it and approves (re-auth again)
//!    or denies. A device approval goes through the same steps as `/link` (quota, the next
//!    `short`, the `device_linked` mail, the event); a browser approval mails `qr_sign_in`, and
//!    the phone's next poll gets an ordinary website session (`via = 'qr'`).
//!
//! A pairing gives no keys, like a link: a phished approval gets ciphertext and metadata only, and
//! shows in the devices list as "no keys" (PROTOCOL §6.1).

use crate::api::{ApiError, ApiResult, Client, JsonBody};
use crate::config::Keys;
use crate::crypto;
use crate::devices::{self, reauth_required, GrantView};
use crate::events::{self, Source};
use crate::http::AppState;
use crate::mail::{self, Template};
use crate::ratelimit::{self as rl, hit};
use crate::sessions::{self, WebSession};
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgConnection;
use uuid::Uuid;

/// A pairing's lifetime: 5 minutes from the start.
pub const PAIR_TTL_S: i64 = 300;
/// After a claim the approval has at least this long, even if the pairing is about to expire.
pub const CLAIMED_MIN_S: i64 = 120;
/// The phone browser's poll interval (`web/pair/poll`); faster gets `slow_down`.
pub const WEB_POLL_S: i32 = 2;

fn token_hash(keys: &Keys, token: &str) -> [u8; 32] {
    crypto::hmac(&keys.token_hmac, "krunk-pair-token", &[token.as_bytes()])
}

fn poll_hash(keys: &Keys, token: &str) -> [u8; 32] {
    crypto::hmac(&keys.token_hmac, "krunk-pair-poll", &[token.as_bytes()])
}

/// A 6-digit code from the OS random source, uniform (rejection sampling).
pub fn new_confirm_code() -> String {
    loop {
        let v = u32::from_be_bytes(crypto::random::<4>());
        // 4_294_000_000 is the largest multiple of 1_000_000 below 2^32.
        if v < 4_294_000_000 {
            return format!("{:06}", v % 1_000_000);
        }
    }
}

/// "Chrome on Android" from a user agent: a short, non-identifying summary for the approval
/// page and the mail. Unknown parts are left out.
pub fn browser_name(user_agent: Option<&str>) -> String {
    let ua = user_agent.unwrap_or("");
    let browser = [
        ("Krunk/", "Krunk"),
        ("Edg/", "Edge"),
        ("EdgA/", "Edge"),
        ("OPR/", "Opera"),
        ("SamsungBrowser/", "Samsung Internet"),
        ("Firefox/", "Firefox"),
        ("FxiOS/", "Firefox"),
        ("CriOS/", "Chrome"),
        ("Chrome/", "Chrome"),
        ("Safari/", "Safari"),
    ]
    .into_iter()
    .find(|(k, _)| ua.contains(k))
    .map(|(_, n)| n);
    let os = [
        ("Android", "Android"),
        ("iPhone", "iOS"),
        ("iPad", "iPadOS"),
        ("Windows", "Windows"),
        ("Mac OS X", "macOS"),
        ("CrOS", "ChromeOS"),
        ("Linux", "Linux"),
    ]
    .into_iter()
    .find(|(k, _)| ua.contains(k))
    .map(|(_, n)| n);
    match (browser, os) {
        (Some(b), Some(o)) => format!("{b} on {o}"),
        (Some(b), None) => b.to_string(),
        (None, Some(o)) => format!("A browser on {o}"),
        (None, None) => "A browser".to_string(),
    }
}

fn not_found() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "not_found", "This code is not valid. Make a new one on your account page.")
}

fn expired() -> ApiError {
    ApiError::new(StatusCode::GONE, "expired_token", "This code has expired. Make a new one on your account page.")
}

fn used() -> ApiError {
    ApiError::new(StatusCode::CONFLICT, "pair_used", "This code was already used. Make a new one on your account page.")
}

fn not_claimed() -> ApiError {
    ApiError::new(StatusCode::CONFLICT, "pair_not_claimed", "No phone has scanned this code yet.")
}

fn poll_error(code: &'static str) -> ApiError {
    let message = match code {
        "authorization_pending" => "Waiting for the approval on your other browser.",
        "slow_down" => "Polling too fast: wait 2 seconds more between requests.",
        "access_denied" => "The sign-in was denied on your other browser.",
        _ => "This code has expired. Make a new one on your account page.",
    };
    ApiError::bad_request(code, message)
}

/// The token of a claim: 43 base64url characters (32 bytes), or `None`.
fn valid_token(t: &str) -> bool {
    t.len() <= devices::TOKEN_MAX && crypto::b64url_decode::<32>(t).is_some()
}

#[derive(sqlx::FromRow)]
struct PairRow {
    id: Uuid,
    account_id: Uuid,
    status: String,
    expired: bool,
    claim_kind: Option<String>,
    confirm_code: Option<String>,
    platform: Option<String>,
    label: Option<String>,
    client_version: Option<String>,
    ip: Option<String>,
    country: Option<String>,
    user_agent: Option<String>,
    expires_in: f64,
    claimed_age: Option<f64>,
}

const PAIR_COLS: &str = "id, account_id, status, expires_at < now() AS expired, claim_kind, confirm_code, platform,
    label, client_version, host(ip) AS ip, country, user_agent,
    greatest(extract(epoch FROM expires_at - now()), 0)::float8 AS expires_in,
    extract(epoch FROM now() - claimed_at)::float8 AS claimed_age";

async fn pair_of_account(
    conn: &mut PgConnection,
    account_id: Uuid,
    lock: bool,
) -> Result<Option<PairRow>, sqlx::Error> {
    let sql =
        format!("SELECT {PAIR_COLS} FROM pair_grants WHERE account_id = $1{}", if lock { " FOR UPDATE" } else { "" });
    sqlx::query_as(sqlx::AssertSqlSafe(sql)).bind(account_id).fetch_optional(conn).await
}

/// Deletes the account's pairing and the pending link grant of its native claim, if any.
async fn delete_pairing(conn: &mut PgConnection, account_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM link_grants WHERE status = 'pending'
           AND pair_id IN (SELECT id FROM pair_grants WHERE account_id = $1)",
    )
    .bind(account_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query("DELETE FROM pair_grants WHERE account_id = $1").bind(account_id).execute(conn).await?;
    Ok(())
}

// ---- The signed-in browser ------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

#[derive(Serialize)]
struct Started {
    pair_id: Uuid,
    url: String,
    qr_svg: Option<String>,
    expires_in: i64,
}

/// `POST web/pair/start {}` → the QR code of a new pairing (re-auth). Replaces the earlier one.
pub async fn start(
    State(state): State<AppState>,
    client: Client,
    s: WebSession,
    JsonBody(_): JsonBody<Empty>,
) -> ApiResult {
    hit(&state.db, rl::PAIR_START, &rl::id_key(&s.account_id)).await?;
    if !s.reauth_valid() {
        return Err(reauth_required());
    }
    let token = crypto::new_token();
    let id = Uuid::new_v4();
    let mut tx = state.db.begin().await?;
    delete_pairing(&mut tx, s.account_id).await?;
    sqlx::query(
        "INSERT INTO pair_grants (id, account_id, token_hash, started_session, expires_at)
         VALUES ($1, $2, $3, $4, now() + make_interval(secs => $5))",
    )
    .bind(id)
    .bind(s.account_id)
    .bind(token_hash(&state.config.keys, &token).as_slice())
    .bind(s.id)
    .bind(PAIR_TTL_S as f64)
    .execute(&mut *tx)
    .await?;
    events::record(&mut tx, s.account_id, "pair_started", &client, Source::Web(s.id)).await?;
    tx.commit().await?;
    tracing::info!("pairing started");
    let url = format!("{}/pair#{token}", state.config.public_base_url);
    Ok(Json(Started { pair_id: id, qr_svg: crate::qr::svg(&url), url, expires_in: PAIR_TTL_S }).into_response())
}

#[derive(Serialize)]
struct Claimant {
    kind: String,
    platform: Option<String>,
    label: Option<String>,
    client_version: Option<String>,
    browser: Option<String>,
    ip: Option<String>,
    country: Option<String>,
    age_s: i64,
}

#[derive(Serialize)]
struct Status {
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pair_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_in: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    confirm_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    claimant: Option<Claimant>,
}

fn state_of(p: &PairRow) -> &'static str {
    match (p.status.as_str(), p.expired) {
        ("approved", _) => "approved",
        ("denied", _) => "denied",
        (_, true) => "expired",
        ("claimed", _) => "claimed",
        _ => "waiting",
    }
}

/// `GET web/pair` → the account's pairing: its state, and who claimed it.
pub async fn status(State(state): State<AppState>, s: WebSession) -> ApiResult {
    hit(&state.db, rl::PAIR_STATUS, &rl::id_key(&s.id)).await?;
    let mut conn = state.db.acquire().await?;
    let Some(p) = pair_of_account(&mut conn, s.account_id, false).await? else {
        return Ok(Json(Status { state: "none", pair_id: None, expires_in: None, confirm_code: None, claimant: None })
            .into_response());
    };
    let st = state_of(&p);
    let claimed = p.claim_kind.is_some() && st != "expired";
    let claimant = claimed.then(|| Claimant {
        kind: p.claim_kind.clone().unwrap_or_default(),
        browser: (p.claim_kind.as_deref() == Some("web")).then(|| browser_name(p.user_agent.as_deref())),
        platform: p.platform.clone(),
        label: p.label.as_deref().map(devices::display_label),
        client_version: p.client_version.clone(),
        ip: p.ip.clone(),
        country: p.country.clone(),
        age_s: p.claimed_age.unwrap_or(0.0).max(0.0) as i64,
    });
    Ok(Json(Status {
        state: st,
        pair_id: Some(p.id),
        expires_in: Some(p.expires_in as i64),
        confirm_code: if claimed && st == "claimed" { p.confirm_code.clone() } else { None },
        claimant,
    })
    .into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApproveReq {
    confirm_code: String,
}

/// `POST web/pair/approve {confirm_code}` → 200 `{kind, device_id?}` (re-auth; the code the
/// phone shows, confirmed).
pub async fn approve(
    State(state): State<AppState>,
    client: Client,
    s: WebSession,
    JsonBody(req): JsonBody<ApproveReq>,
) -> ApiResult {
    hit(&state.db, rl::PAIR_ACT, &rl::id_key(&s.account_id)).await?;
    if !s.reauth_valid() {
        return Err(reauth_required());
    }
    let mut tx = state.db.begin().await?;
    let p = pair_of_account(&mut tx, s.account_id, true).await?.ok_or_else(not_found)?;
    match state_of(&p) {
        "claimed" => {}
        "expired" => return Err(expired()),
        "waiting" => return Err(not_claimed()),
        _ => return Err(used()),
    }
    let code = p.confirm_code.as_deref().unwrap_or("");
    if req.confirm_code.len() > 16 || !crypto::ct_eq(req.confirm_code.trim().as_bytes(), code.as_bytes()) {
        return Err(ApiError::bad_request("bad_request", "The code you confirmed is not the one your phone shows."));
    }
    let mut device_id = None;
    if p.claim_kind.as_deref() == Some("device") {
        let g: Option<GrantView> = sqlx::query_as(
            "SELECT label, platform, country, extract(epoch FROM now() - created_at)::float8 AS age_s,
                    device_code_hash AS hash
             FROM link_grants WHERE pair_id = $1 AND status = 'pending' AND expires_at > now() FOR UPDATE",
        )
        .bind(p.id)
        .fetch_optional(&mut *tx)
        .await?;
        let g = g.ok_or_else(expired)?;
        let (id, _) = devices::approve_grant(&mut tx, &state, &devices::Approver::from(&s), &client, &g).await?;
        device_id = Some(id);
    } else {
        let when = Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
        mail::enqueue(
            &mut tx,
            Some(s.account_id),
            &s.email,
            Template::QrSignIn,
            json!({
                "browser": browser_name(p.user_agent.as_deref()),
                "country": p.country.clone().unwrap_or_else(|| "an unknown country".into()),
                "time": when,
            }),
        )
        .await?;
    }
    sqlx::query("UPDATE pair_grants SET status = 'approved' WHERE id = $1").bind(p.id).execute(&mut *tx).await?;
    events::record(&mut tx, s.account_id, "pair_approved", &client, Source::Web(s.id)).await?;
    tx.commit().await?;
    tracing::info!(kind = p.claim_kind.as_deref().unwrap_or(""), "pairing approved");
    Ok(Json(json!({ "kind": p.claim_kind, "device_id": device_id })).into_response())
}

/// `POST web/pair/deny {}` → 204. The phone's next poll gets `access_denied`.
pub async fn deny(
    State(state): State<AppState>,
    client: Client,
    s: WebSession,
    JsonBody(_): JsonBody<Empty>,
) -> ApiResult {
    hit(&state.db, rl::PAIR_ACT, &rl::id_key(&s.account_id)).await?;
    let mut tx = state.db.begin().await?;
    let p = pair_of_account(&mut tx, s.account_id, true).await?.ok_or_else(not_found)?;
    if !matches!(state_of(&p), "waiting" | "claimed") {
        return Err(if state_of(&p) == "expired" { expired() } else { used() });
    }
    sqlx::query("UPDATE pair_grants SET status = 'denied' WHERE id = $1").bind(p.id).execute(&mut *tx).await?;
    sqlx::query("UPDATE link_grants SET status = 'denied' WHERE pair_id = $1 AND status = 'pending'")
        .bind(p.id)
        .execute(&mut *tx)
        .await?;
    events::record(&mut tx, s.account_id, "pair_denied", &client, Source::Web(s.id)).await?;
    tx.commit().await?;
    tracing::info!("pairing denied");
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// `POST web/pair/cancel {}` → 204: the pairing (and a native claim's pending grant) is gone.
pub async fn cancel(State(state): State<AppState>, s: WebSession, JsonBody(_): JsonBody<Empty>) -> ApiResult {
    hit(&state.db, rl::PAIR_ACT, &rl::id_key(&s.account_id)).await?;
    let mut tx = state.db.begin().await?;
    delete_pairing(&mut tx, s.account_id).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

// ---- The phone ------------------------------------------------------------------------------

/// Locks the pairing of a claimed token, refusing unknown, expired and used ones. Every claim
/// counts against the per-IP limit first, whether or not the token is known.
async fn claimable(
    conn: &mut PgConnection,
    state: &AppState,
    client: &Client,
    token: &str,
) -> Result<PairRow, ApiError> {
    hit(&state.db, rl::PAIR_CLAIM_IP, &rl::ip_key(&state.config.keys, client.ip())).await?;
    if !valid_token(token) {
        return Err(not_found());
    }
    // An equality lookup of an HMAC: what an attacker controls (the token) never meets a
    // stored value byte by byte, so timing tells nothing about other tokens.
    let sql = format!("SELECT {PAIR_COLS} FROM pair_grants WHERE token_hash = $1 FOR UPDATE");
    let p: Option<PairRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(token_hash(&state.config.keys, token).as_slice())
        .fetch_optional(&mut *conn)
        .await?;
    let p = p.ok_or_else(not_found)?;
    match state_of(&p) {
        "waiting" => Ok(p),
        "expired" => Err(expired()),
        _ => Err(used()),
    }
}

/// Marks a pairing claimed by `kind` from this client; returns the confirm code.
async fn mark_claimed(
    conn: &mut PgConnection,
    p: &PairRow,
    client: &Client,
    kind: &str,
    native: Option<(&str, &str, &str)>,
    poll: Option<&[u8]>,
) -> Result<String, sqlx::Error> {
    let code = new_confirm_code();
    let (platform, label, version) = match native {
        Some((p, l, v)) => (Some(p), Some(l), Some(v)),
        None => (None, None, None),
    };
    sqlx::query(
        "UPDATE pair_grants SET status = 'claimed', claim_kind = $2, claimed_at = now(), confirm_code = $3,
                platform = $4, label = $5, client_version = $6, ip = $7::inet, country = $8, user_agent = $9,
                poll_hash = $10, expires_at = greatest(expires_at, now() + make_interval(secs => $11))
         WHERE id = $1",
    )
    .bind(p.id)
    .bind(kind)
    .bind(&code)
    .bind(platform)
    .bind(label)
    .bind(version)
    .bind(client.ip().to_string())
    .bind(client.info.country.as_deref())
    .bind(client.user_agent.as_deref())
    .bind(poll)
    .bind(CLAIMED_MIN_S as f64)
    .execute(&mut *conn)
    .await?;
    events::record(conn, p.account_id, "pair_claimed", client, Source::None).await?;
    Ok(code)
}

async fn expires_in(conn: &mut PgConnection, id: Uuid) -> Result<i64, sqlx::Error> {
    let s: f64 = sqlx::query_scalar(
        "SELECT greatest(extract(epoch FROM expires_at - now()), 0)::float8 FROM pair_grants WHERE id = $1",
    )
    .bind(id)
    .fetch_one(conn)
    .await?;
    Ok(s as i64)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebClaimReq {
    token: String,
}

#[derive(Serialize)]
struct WebClaimed {
    confirm_code: String,
    poll_token: String,
    interval: i32,
    expires_in: i64,
}

/// `POST web/pair/claim {token}` (a phone's browser, no session) → the confirm code and a poll
/// token for `web/pair/poll`.
pub async fn web_claim(
    State(state): State<AppState>,
    client: Client,
    JsonBody(req): JsonBody<WebClaimReq>,
) -> ApiResult {
    let mut tx = state.db.begin().await?;
    let p = claimable(&mut tx, &state, &client, &req.token).await?;
    let poll_token = crypto::new_token();
    let ph = poll_hash(&state.config.keys, &poll_token);
    let code = mark_claimed(&mut tx, &p, &client, "web", None, Some(ph.as_slice())).await?;
    let expires_in = expires_in(&mut tx, p.id).await?;
    tx.commit().await?;
    tracing::info!("pairing claimed by a browser");
    Ok(Json(WebClaimed { confirm_code: code, poll_token, interval: WEB_POLL_S, expires_in }).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PollReq {
    poll_token: String,
}

#[derive(sqlx::FromRow)]
struct PollRow {
    id: Uuid,
    account_id: Uuid,
    status: String,
    expired: bool,
    too_fast: bool,
}

#[derive(Serialize)]
struct CsrfBody {
    csrf: String,
}

/// `POST web/pair/poll {poll_token}` → 400 `authorization_pending` | `slow_down` |
/// `access_denied` | `expired_token`; once approved, 200 with a new website session (cookie and
/// `{csrf}`), once.
pub async fn web_poll(State(state): State<AppState>, client: Client, JsonBody(req): JsonBody<PollReq>) -> ApiResult {
    hit(&state.db, rl::PAIR_POLL_IP, &rl::ip_key(&state.config.keys, client.ip())).await?;
    if !valid_token(&req.poll_token) {
        return Err(poll_error("expired_token"));
    }
    let keys = &state.config.keys;
    let mut tx = state.db.begin().await?;
    let row: Option<PollRow> = sqlx::query_as(
        "SELECT id, account_id, status, expires_at < now() AS expired,
                coalesce(last_poll_at > now() - make_interval(secs => $2), false) AS too_fast
         FROM pair_grants WHERE poll_hash = $1 FOR UPDATE",
    )
    .bind(poll_hash(keys, &req.poll_token).as_slice())
    .bind(f64::from(WEB_POLL_S) - 0.5)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(r) = row else { return Err(poll_error("expired_token")) };
    let spent = "UPDATE pair_grants SET poll_hash = NULL WHERE id = $1";
    match (r.status.as_str(), r.expired) {
        ("approved", _) => {
            let (session_id, token, csrf) =
                sessions::create(&mut tx, keys, r.account_id, &client, sessions::Via::Qr, false).await?;
            sqlx::query(spent).bind(r.id).execute(&mut *tx).await?;
            sqlx::query("UPDATE accounts SET last_active_at = now() WHERE id = $1")
                .bind(r.account_id)
                .execute(&mut *tx)
                .await?;
            events::record(&mut tx, r.account_id, "qr_sign_in", &client, Source::Web(session_id)).await?;
            tx.commit().await?;
            tracing::info!("signed in by QR");
            let mut res: Response = Json(CsrfBody { csrf }).into_response();
            res.headers_mut().insert(header::SET_COOKIE, sessions::set_cookie(&token));
            Ok(res)
        }
        ("denied", _) => {
            sqlx::query(spent).bind(r.id).execute(&mut *tx).await?;
            tx.commit().await?;
            Err(poll_error("access_denied"))
        }
        (_, true) => {
            sqlx::query(spent).bind(r.id).execute(&mut *tx).await?;
            tx.commit().await?;
            Err(poll_error("expired_token"))
        }
        _ if r.too_fast => {
            sqlx::query("UPDATE pair_grants SET last_poll_at = now() WHERE id = $1")
                .bind(r.id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Err(poll_error("slow_down"))
        }
        _ => {
            sqlx::query("UPDATE pair_grants SET last_poll_at = now() WHERE id = $1")
                .bind(r.id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            Err(poll_error("authorization_pending"))
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeClaimReq {
    token: String,
    platform: String,
    label: String,
    client_version: String,
}

#[derive(Serialize)]
struct NativeClaimed {
    device_code: String,
    interval: i32,
    expires_in: i64,
    confirm_code: String,
}

/// `POST link/pair/claim {token, platform, label, client_version}` (a native client) → a device
/// code to poll `link/token` with, and the confirm code to show.
pub async fn native_claim(
    State(state): State<AppState>,
    client: Client,
    JsonBody(req): JsonBody<NativeClaimReq>,
) -> ApiResult {
    if !matches!(req.platform.as_str(), "windows" | "android")
        || req.client_version.len() > devices::VERSION_MAX
        || req.client_version.chars().any(char::is_control)
    {
        hit(&state.db, rl::PAIR_CLAIM_IP, &rl::ip_key(&state.config.keys, client.ip())).await?;
        return Err(devices::bad());
    }
    let Some(label) = devices::clean_label(&req.label) else {
        hit(&state.db, rl::PAIR_CLAIM_IP, &rl::ip_key(&state.config.keys, client.ip())).await?;
        return Err(devices::bad());
    };
    let keys = &state.config.keys;
    let mut tx = state.db.begin().await?;
    let p = claimable(&mut tx, &state, &client, &req.token).await?;
    let code =
        mark_claimed(&mut tx, &p, &client, "device", Some((&req.platform, &label, &req.client_version)), None).await?;
    let device_code = crypto::new_token();
    // The grant's user code is random and never shown: /link can't reach a paired grant.
    let hidden_user_code = crypto::random::<32>();
    sqlx::query(
        "INSERT INTO link_grants (device_code_hash, user_code_hash, platform, label, client_version, ip, country,
                                  user_agent, expires_at, interval_s, pair_id)
         SELECT $1, $2, $3, $4, $5, $6::inet, $7, $8, expires_at, $9, id FROM pair_grants WHERE id = $10",
    )
    .bind(devices::device_code_hash(keys, &device_code).as_slice())
    .bind(crypto::hmac(&keys.token_hmac, "krunk-user-code", &[hidden_user_code.as_slice()]).as_slice())
    .bind(&req.platform)
    .bind(&label)
    .bind(&req.client_version)
    .bind(client.ip().to_string())
    .bind(client.info.country.as_deref())
    .bind(client.user_agent.as_deref())
    .bind(devices::POLL_INTERVAL_S)
    .bind(p.id)
    .execute(&mut *tx)
    .await?;
    let expires_in = expires_in(&mut tx, p.id).await?;
    tx.commit().await?;
    tracing::info!("pairing claimed by a device");
    Ok(Json(NativeClaimed { device_code, interval: devices::POLL_INTERVAL_S, expires_in, confirm_code: code })
        .into_response())
}

/// For the export: the account's pairing, without its hashes.
#[derive(Serialize, sqlx::FromRow)]
pub struct PairExport {
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub status: String,
    pub claim_kind: Option<String>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub platform: Option<String>,
    pub label: Option<String>,
    pub client_version: Option<String>,
    pub ip: Option<String>,
    pub country: Option<String>,
    pub user_agent: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirm_codes_are_six_digits() {
        for _ in 0..200 {
            let c = new_confirm_code();
            assert_eq!(c.len(), 6);
            assert!(c.bytes().all(|b| b.is_ascii_digit()));
        }
    }

    #[test]
    fn browser_names() {
        let chrome_android = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Mobile Safari/537.36";
        assert_eq!(browser_name(Some(chrome_android)), "Chrome on Android");
        let safari_ios = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1";
        assert_eq!(browser_name(Some(safari_ios)), "Safari on iOS");
        assert_eq!(
            browser_name(Some("Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0")),
            "Firefox on Linux"
        );
        assert_eq!(browser_name(None), "A browser");
        assert_eq!(browser_name(Some("curl/8")), "A browser");
    }

    #[test]
    fn tokens_are_checked_before_the_database() {
        assert!(valid_token(&crypto::new_token()));
        assert!(!valid_token("short"));
        assert!(!valid_token(&"A".repeat(100)));
    }
}
