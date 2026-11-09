//! Milestone S6: the relay of a device approval (PROTOCOL §6.2, §11.6).
//!
//! A new device N (linked, no keys) and a device A (with keys) run a SAS-confirmed key exchange
//! through the server, which only passes the fields along in a fixed order:
//! `requested` (N: `commit_N`) → `responded` (A: `epk_A`, `nonce_A`) → `revealed` (N: `epk_N`,
//! `nonce_N`, `box_N`) → `confirmed` (N: `tag_N`, sent only if the typed SAS matched) → `done`
//! (A: `box_A`, after committing N into `ak_dir`). Each step happens once; a wrong order is 409.
//! The approval is deleted when N has collected `box_A`, when either side cancels (a SAS
//! mismatch), or after 10 minutes. At most 5 are created per account per day, each one mailed.

use crate::api::{ApiError, ApiResult, Client, JsonBody};
use crate::crypto;
use crate::devices::{device_not_found, DeviceAuth, Empty};
use crate::http::AppState;
use crate::mail::{self, Template};
use crate::ratelimit::{self as rl, hit};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

pub const APPROVAL_TTL_S: i64 = 600;

fn wrong_step() -> ApiError {
    ApiError::new(StatusCode::CONFLICT, "conflict", "This approval is at another step.")
}

fn bad() -> ApiError {
    ApiError::bad_request("bad_request", "The request is not valid.")
}

fn b32(s: &str) -> Result<Vec<u8>, ApiError> {
    crypto::b64url_decode::<32>(s).map(|x| x.to_vec()).ok_or_else(bad)
}

fn boxed(s: &str, max: usize) -> Result<Vec<u8>, ApiError> {
    use base64ct::{Base64UrlUnpadded, Encoding};
    if s.len() > max.div_ceil(3) * 4 {
        return Err(bad());
    }
    Base64UrlUnpadded::decode_vec(s).ok().filter(|b| b.len() >= 40 && b.len() <= max).ok_or_else(bad)
}

async fn has_keys(db: &sqlx::PgPool, device: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT has_keys FROM devices WHERE id = $1").bind(device).fetch_one(db).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateReq {
    commit_n: String,
}

/// `POST approvals {commit_N}` (a device without keys) → `{approval_id, expires_in}`. An earlier
/// open approval of the same device is replaced.
pub async fn create(State(state): State<AppState>, d: DeviceAuth, JsonBody(req): JsonBody<CreateReq>) -> ApiResult {
    let commit = b32(&req.commit_n)?;
    if has_keys(&state.db, d.device_id).await? {
        return Err(ApiError::new(StatusCode::CONFLICT, "conflict", "This device already has the keys."));
    }
    if crate::keys::state_row(&mut *state.db.acquire().await?, d.account_id, false).await?.is_none() {
        return Err(crate::keys::no_keyset());
    }
    hit(&state.db, rl::APPROVALS_CREATE, &rl::id_key(&d.account_id)).await?;
    let id = Uuid::new_v4();
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM approvals WHERE new_device_id = $1").bind(d.device_id).execute(&mut *tx).await?;
    sqlx::query(
        "INSERT INTO approvals (id, account_id, new_device_id, step, commit_n, expires_at)
         VALUES ($1, $2, $3, 'requested', $4, now() + make_interval(secs => $5))",
    )
    .bind(id)
    .bind(d.account_id)
    .bind(d.device_id)
    .bind(&commit)
    .bind(APPROVAL_TTL_S as f64)
    .execute(&mut *tx)
    .await?;
    let (email, label): (String, String) = sqlx::query_as(
        "SELECT a.email::text, d.label FROM accounts a JOIN devices d ON d.account_id = a.id WHERE d.id = $1",
    )
    .bind(d.device_id)
    .fetch_one(&mut *tx)
    .await?;
    mail::enqueue(&mut tx, Some(d.account_id), &email, Template::ApprovalRequested, json!({ "label": label })).await?;
    tx.commit().await?;
    tracing::info!(approval = %id, "approval requested");
    Ok(Json(json!({"approval_id": id, "expires_in": APPROVAL_TTL_S})).into_response())
}

#[derive(sqlx::FromRow)]
struct ListRow {
    id: Uuid,
    step: String,
    device_id: Uuid,
    label: String,
    platform: String,
    country: Option<String>,
    expires_in: f64,
}

/// `GET approvals` (a device with keys) → the open approvals of the account.
pub async fn list(State(state): State<AppState>, d: DeviceAuth) -> ApiResult {
    hit(&state.db, rl::APPROVALS_LIST, &rl::id_key(&d.device_id)).await?;
    if !has_keys(&state.db, d.device_id).await? {
        return Err(ApiError::forbidden());
    }
    let rows: Vec<ListRow> = sqlx::query_as(
        "SELECT a.id, a.step, n.id AS device_id, n.label, n.platform, n.last_country AS country,
                extract(epoch FROM a.expires_at - now())::float8 AS expires_in
         FROM approvals a JOIN devices n ON n.id = a.new_device_id
         WHERE a.account_id = $1 AND a.expires_at > now() AND n.revoked_at IS NULL
           AND (a.approver_device_id IS NULL OR a.approver_device_id = $2)
         ORDER BY a.created_at",
    )
    .bind(d.account_id)
    .bind(d.device_id)
    .fetch_all(&state.db)
    .await?;
    let items: Vec<_> = rows
        .into_iter()
        .map(|r| {
            json!({
                "approval_id": r.id,
                "state": r.step,
                "device": {"id": r.device_id, "label": r.label, "platform": r.platform, "country": r.country},
                "expires_in": r.expires_in.max(0.0) as i64,
            })
        })
        .collect();
    Ok(Json(items).into_response())
}

#[derive(sqlx::FromRow)]
struct Row {
    new_device_id: Uuid,
    approver_device_id: Option<Uuid>,
    step: String,
    commit_n: Vec<u8>,
    epk_a: Option<Vec<u8>>,
    nonce_a: Option<Vec<u8>>,
    epk_n: Option<Vec<u8>>,
    nonce_n: Option<Vec<u8>>,
    box_n: Option<Vec<u8>>,
    tag_n: Option<Vec<u8>>,
    box_a: Option<Vec<u8>>,
    label: String,
    platform: String,
    country: Option<String>,
    expires_in: f64,
}

async fn load(conn: &mut sqlx::PgConnection, account_id: Uuid, id: &str, lock: bool) -> Result<Row, ApiError> {
    let id: Uuid = id.parse().map_err(|_| device_not_found())?;
    let sql = format!(
        "SELECT a.new_device_id, a.approver_device_id, a.step, a.commit_n, a.epk_a, a.nonce_a, a.epk_n, a.nonce_n,
                a.box_n, a.tag_n, a.box_a, n.label, n.platform, n.last_country AS country,
                extract(epoch FROM a.expires_at - now())::float8 AS expires_in
         FROM approvals a JOIN devices n ON n.id = a.new_device_id
         WHERE a.id = $1 AND a.account_id = $2 AND a.expires_at > now(){}",
        if lock { " FOR UPDATE OF a" } else { "" }
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(id)
        .bind(account_id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(device_not_found)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RespondReq {
    #[serde(rename = "epk_A")]
    epk_a: String,
    #[serde(rename = "nonce_A")]
    nonce_a: String,
}

/// `POST approvals/{id}/respond {epk_A, nonce_A}` (A, with keys) → 204.
pub async fn respond(
    State(state): State<AppState>,
    d: DeviceAuth,
    Path(id): Path<String>,
    JsonBody(req): JsonBody<RespondReq>,
) -> ApiResult {
    hit(&state.db, rl::APPROVAL_STEP, &rl::id_key(&d.device_id)).await?;
    if !has_keys(&state.db, d.device_id).await? {
        return Err(ApiError::forbidden());
    }
    let (epk, nonce) = (b32(&req.epk_a)?, b32(&req.nonce_a)?);
    let mut tx = state.db.begin().await?;
    let r = load(&mut tx, d.account_id, &id, true).await?;
    if r.new_device_id == d.device_id {
        return Err(ApiError::forbidden());
    }
    if r.step != "requested" {
        return Err(wrong_step());
    }
    sqlx::query(
        "UPDATE approvals SET approver_device_id = $2, epk_a = $3, nonce_a = $4, step = 'responded' WHERE id = $1",
    )
    .bind(id.parse::<Uuid>().map_err(|_| device_not_found())?)
    .bind(d.device_id)
    .bind(&epk)
    .bind(&nonce)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevealReq {
    #[serde(rename = "epk_N")]
    epk_n: String,
    #[serde(rename = "nonce_N")]
    nonce_n: String,
    #[serde(rename = "box_N")]
    box_n: String,
}

/// `POST approvals/{id}/reveal {epk_N, nonce_N, box_N}` (N) → 204.
pub async fn reveal(
    State(state): State<AppState>,
    d: DeviceAuth,
    Path(id): Path<String>,
    JsonBody(req): JsonBody<RevealReq>,
) -> ApiResult {
    hit(&state.db, rl::APPROVAL_STEP, &rl::id_key(&d.device_id)).await?;
    let (epk, nonce, bx) = (b32(&req.epk_n)?, b32(&req.nonce_n)?, boxed(&req.box_n, 4096)?);
    // (A checks that this matches `commit_N`; the server only relays.)
    let mut tx = state.db.begin().await?;
    let r = load(&mut tx, d.account_id, &id, true).await?;
    if r.new_device_id != d.device_id {
        return Err(ApiError::forbidden());
    }
    if r.step != "responded" {
        return Err(wrong_step());
    }
    sqlx::query("UPDATE approvals SET epk_n = $2, nonce_n = $3, box_n = $4, step = 'revealed' WHERE id = $1")
        .bind(id.parse::<Uuid>().map_err(|_| device_not_found())?)
        .bind(&epk)
        .bind(&nonce)
        .bind(&bx)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfirmReq {
    #[serde(rename = "tag_N")]
    tag_n: String,
}

/// `POST approvals/{id}/confirm {tag_N}` (N, after the typed SAS matched) → 204.
pub async fn confirm(
    State(state): State<AppState>,
    d: DeviceAuth,
    Path(id): Path<String>,
    JsonBody(req): JsonBody<ConfirmReq>,
) -> ApiResult {
    hit(&state.db, rl::APPROVAL_STEP, &rl::id_key(&d.device_id)).await?;
    let tag = b32(&req.tag_n)?;
    let mut tx = state.db.begin().await?;
    let r = load(&mut tx, d.account_id, &id, true).await?;
    if r.new_device_id != d.device_id {
        return Err(ApiError::forbidden());
    }
    if r.step != "revealed" {
        return Err(wrong_step());
    }
    sqlx::query("UPDATE approvals SET tag_n = $2, step = 'confirmed' WHERE id = $1")
        .bind(id.parse::<Uuid>().map_err(|_| device_not_found())?)
        .bind(&tag)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeysReq {
    #[serde(rename = "box_A")]
    box_a: String,
}

/// `POST approvals/{id}/keys {box_A}` (A, after verifying `tag_N` and committing N into
/// `ak_dir`) → 204.
pub async fn keys(
    State(state): State<AppState>,
    d: DeviceAuth,
    Path(id): Path<String>,
    JsonBody(req): JsonBody<KeysReq>,
) -> ApiResult {
    hit(&state.db, rl::APPROVAL_STEP, &rl::id_key(&d.device_id)).await?;
    let bx = boxed(&req.box_a, 8192)?;
    let mut tx = state.db.begin().await?;
    let r = load(&mut tx, d.account_id, &id, true).await?;
    if r.approver_device_id != Some(d.device_id) {
        return Err(ApiError::forbidden());
    }
    if r.step != "confirmed" {
        return Err(wrong_step());
    }
    sqlx::query("UPDATE approvals SET box_a = $2, step = 'done' WHERE id = $1")
        .bind(id.parse::<Uuid>().map_err(|_| device_not_found())?)
        .bind(&bx)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    tracing::info!("approval keys delivered");
    Ok(StatusCode::NO_CONTENT.into_response())
}

fn b(x: &Option<Vec<u8>>) -> Option<String> {
    x.as_deref().map(crypto::b64url)
}

/// `GET approvals/{id}` (A or N) → the fields of the current step that the caller needs. When N
/// collects `box_A`, the approval is deleted (it is complete).
pub async fn get_one(State(state): State<AppState>, d: DeviceAuth, Path(id): Path<String>) -> ApiResult {
    hit(&state.db, rl::APPROVAL_POLL, &rl::id_key(&d.device_id)).await?;
    let mut tx = state.db.begin().await?;
    let r = load(&mut tx, d.account_id, &id, true).await?;
    let device = json!({"id": r.new_device_id, "label": r.label, "platform": r.platform, "country": r.country});
    let out = if r.new_device_id == d.device_id {
        json!({
            "approval_id": id, "state": r.step, "device": device, "expires_in": r.expires_in.max(0.0) as i64,
            "approver": r.approver_device_id,
            "epk_A": b(&r.epk_a), "nonce_A": b(&r.nonce_a), "box_A": b(&r.box_a),
        })
    } else if r.approver_device_id == Some(d.device_id)
        || (r.approver_device_id.is_none() && has_keys(&state.db, d.device_id).await?)
    {
        json!({
            "approval_id": id, "state": r.step, "device": device, "expires_in": r.expires_in.max(0.0) as i64,
            "commit_N": crypto::b64url(&r.commit_n), "epk_N": b(&r.epk_n), "nonce_N": b(&r.nonce_n),
            "box_N": b(&r.box_n), "tag_N": b(&r.tag_n),
        })
    } else {
        return Err(device_not_found());
    };
    if r.new_device_id == d.device_id && r.step == "done" {
        sqlx::query("DELETE FROM approvals WHERE id = $1")
            .bind(id.parse::<Uuid>().map_err(|_| device_not_found())?)
            .execute(&mut *tx)
            .await?;
        tracing::info!("approval completed");
    }
    tx.commit().await?;
    Ok(Json(out).into_response())
}

/// `POST approvals/{id}/cancel` (A or N; 0.9.0 addition) → 204: a SAS mismatch, a bad tag, or
/// the user gave up. The approval is gone; N must ask again.
pub async fn cancel(
    State(state): State<AppState>,
    _client: Client,
    d: DeviceAuth,
    Path(id): Path<String>,
    JsonBody(_): JsonBody<Empty>,
) -> ApiResult {
    hit(&state.db, rl::APPROVAL_STEP, &rl::id_key(&d.device_id)).await?;
    let uid: Uuid = id.parse().map_err(|_| device_not_found())?;
    let n = sqlx::query(
        "DELETE FROM approvals WHERE id = $1 AND account_id = $2
           AND (new_device_id = $3 OR approver_device_id = $3
                OR (approver_device_id IS NULL AND (SELECT has_keys FROM devices WHERE id = $3)))",
    )
    .bind(uid)
    .bind(d.account_id)
    .bind(d.device_id)
    .execute(&state.db)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(device_not_found());
    }
    tracing::info!("approval cancelled");
    Ok(StatusCode::NO_CONTENT.into_response())
}
