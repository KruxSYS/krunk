//! 0.13.0: the open account section and the account lifecycle (PLAN §7, milestone S10;
//! PROTOCOL §11.2): usage against the quotas, the activity and IP history, the personal-data
//! download (`krunk-account-export/1`, browser only, never mailed), sync reset and deletion.

use crate::accounts::{self, check_lengths, CODE_MAX, EMAIL_MAX, PASSWORD_MAX, TURNSTILE_MAX};
use crate::api::{ApiError, ApiResult, Client, JsonBody};
use crate::client_ip;
use crate::crypto;
use crate::devices::{display_label, reauth_required};
use crate::events::{self, Source};
use crate::http::AppState;
use crate::mail::{self, Template};
use crate::pairing::{self, PairExport};
use crate::ratelimit::{self as rl, hit};
use crate::sessions::{self, WebSession};
use axum::body::{Body, Bytes};
use axum::extract::{Query, State};
use axum::http::{header, HeaderValue};
use axum::response::IntoResponse;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

/// The largest record the server takes (PROTOCOL §9.2): 64 KiB of plaintext.
pub const MAX_RECORD_BYTES: i64 = 65536;
/// The export's schema name (PROTOCOL §11.2, `docs/sync/export-schema-1.json`).
pub const EXPORT_SCHEMA: &str = "krunk-account-export/1";
/// Records per database round trip while the export streams.
const EXPORT_PAGE: i64 = 500;
/// The confirmation typed for a sync reset.
pub const RESET_CONFIRM: &str = "DELETE SYNC DATA";

// ---- Usage ----------------------------------------------------------------------------------

#[derive(Serialize, sqlx::FromRow)]
struct CollectionUse {
    c: String,
    records: i64,
    bytes: i64,
}

#[derive(Serialize, sqlx::FromRow)]
struct DeviceUse {
    id: Uuid,
    label: String,
    platform: String,
    last_sync: Option<DateTime<Utc>>,
    last_used: DateTime<Utc>,
    last_ip: Option<String>,
    last_country: Option<String>,
}

#[derive(sqlx::FromRow)]
struct AccountFacts {
    created: DateTime<Utc>,
    password_changed: DateTime<Utc>,
    totp: bool,
    email_verified: bool,
    backup_codes_left: i64,
    last_sign_in: Option<DateTime<Utc>>,
}

/// `GET web/usage` → what the account stores against the quotas, its devices, and account facts.
pub async fn usage(State(state): State<AppState>, s: WebSession) -> ApiResult {
    hit(&state.db, rl::USAGE, &rl::id_key(&s.id)).await?;
    let collections: Vec<CollectionUse> = sqlx::query_as(
        "SELECT c, count(*)::int8 AS records, coalesce(sum(size), 0)::int8 AS bytes
         FROM records WHERE account_id = $1 GROUP BY c ORDER BY c",
    )
    .bind(s.account_id)
    .fetch_all(&state.db)
    .await?;
    let total: i64 = collections.iter().map(|c| c.records).sum();
    let bytes: i64 = collections.iter().map(|c| c.bytes).sum();
    let history = collections.iter().find(|c| c.c == "history").map_or(0, |c| c.records);
    let devices: Vec<DeviceUse> = sqlx::query_as(
        "SELECT d.id, d.label, d.platform, d.last_sync_at AS last_sync, d.last_used_at AS last_used,
                host(d.last_ip) AS last_ip, d.last_country
         FROM devices d WHERE d.account_id = $1 AND d.revoked_at IS NULL
           AND EXISTS (SELECT 1 FROM refresh_families f WHERE f.device_id = d.id)
         ORDER BY d.last_used_at DESC",
    )
    .bind(s.account_id)
    .fetch_all(&state.db)
    .await?;
    let devices: Vec<DeviceUse> = devices
        .into_iter()
        .map(|d| DeviceUse { label: display_label(&d.label), last_ip: client_ip::shown(d.last_ip.as_deref()), ..d })
        .collect();
    let a: AccountFacts = sqlx::query_as(
        "SELECT a.created_at AS created, a.pw_changed_at AS password_changed, a.totp_enabled AS totp,
                a.email_verified_at IS NOT NULL AS email_verified,
                (SELECT count(*) FROM backup_codes b WHERE b.account_id = a.id AND b.used_at IS NULL) AS backup_codes_left,
                (SELECT max(at) FROM ip_events e WHERE e.account_id = a.id AND e.kind IN ('login', 'qr_sign_in', 'email_verified', 'device_sign_in'))
                  AS last_sign_in
         FROM accounts a WHERE a.id = $1",
    )
    .bind(s.account_id)
    .fetch_one(&state.db)
    .await?;
    let q = &state.config.quota;
    Ok(Json(json!({
        "records": {
            "total": total, "max": q.max_records, "bytes": bytes, "max_bytes": q.max_bytes,
            "history": history, "max_history": q.max_history, "max_record_bytes": MAX_RECORD_BYTES,
            "collections": collections,
        },
        "devices": { "active": devices.len(), "max": state.config.quota_max_devices, "items": devices },
        "account": {
            "created": a.created, "last_sign_in": a.last_sign_in, "password_changed": a.password_changed,
            "totp": a.totp, "backup_codes_left": a.backup_codes_left, "email_verified": a.email_verified,
        },
    }))
    .into_response())
}

// ---- Activity -------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActivityQuery {
    before: Option<i64>,
    limit: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct EventRow {
    id: i64,
    kind: String,
    at: DateTime<Utc>,
    ip: Option<String>,
    country: Option<String>,
    source_kind: Option<String>,
    source_id: Option<Uuid>,
    device_label: Option<String>,
    session_agent: Option<String>,
    session_exists: bool,
}

/// `GET web/activity?before=&limit=` → the account's events of the last 90 days, newest first.
pub async fn activity(State(state): State<AppState>, s: WebSession, Query(q): Query<ActivityQuery>) -> ApiResult {
    hit(&state.db, rl::ACTIVITY, &rl::id_key(&s.id)).await?;
    let limit = q.limit.unwrap_or(50).clamp(1, 100);
    let rows: Vec<EventRow> = sqlx::query_as(
        "SELECT e.id, e.kind, e.at, host(e.ip) AS ip, e.country, e.source_kind, e.source_id,
                d.label AS device_label, w.user_agent AS session_agent, w.id IS NOT NULL AS session_exists
         FROM ip_events e
         LEFT JOIN devices d ON e.source_kind = 'device' AND d.id = e.source_id AND d.account_id = e.account_id
         LEFT JOIN web_sessions w ON e.source_kind = 'web' AND w.id = e.source_id AND w.account_id = e.account_id
         WHERE e.account_id = $1 AND ($2::int8 IS NULL OR e.id < $2)
         ORDER BY e.id DESC LIMIT $3",
    )
    .bind(s.account_id)
    .bind(q.before)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;
    let next_before = (rows.len() as i64 == limit).then(|| rows.last().map(|r| r.id)).flatten();
    let events: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            let label = match r.source_kind.as_deref() {
                Some("device") => r.device_label.as_deref().map(display_label),
                Some("web") if r.session_exists => Some(pairing::browser_name(r.session_agent.as_deref())),
                _ => None,
            };
            let removed = match r.source_kind.as_deref() {
                Some("device") => r.device_label.is_none(),
                Some("web") => !r.session_exists,
                _ => false,
            };
            json!({
                "id": r.id, "kind": r.kind, "at": r.at,
                "ip": client_ip::shown(r.ip.as_deref()), "country": r.country,
                "source": {
                    "kind": r.source_kind, "id": r.source_id, "label": label, "removed": removed,
                    "current": r.source_kind.as_deref() == Some("web") && r.source_id == Some(s.id),
                },
            })
        })
        .collect();
    Ok(Json(json!({
        "events": events,
        "next_before": next_before,
        "retention_days": state.config.ip_retention_days,
    }))
    .into_response())
}

// ---- Export ---------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Empty {}

#[derive(Serialize, sqlx::FromRow)]
struct ExportDevice {
    id: Uuid,
    short: i32,
    platform: String,
    label: String,
    client_version: String,
    created_at: DateTime<Utc>,
    last_used_at: DateTime<Utc>,
    last_sync_at: Option<DateTime<Utc>>,
    last_ip: Option<String>,
    last_country: Option<String>,
    user_agent: Option<String>,
    has_keys: bool,
    revoked_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, sqlx::FromRow)]
struct ExportSession {
    id: Uuid,
    created_at: DateTime<Utc>,
    last_used_at: DateTime<Utc>,
    ip: Option<String>,
    country: Option<String>,
    user_agent: Option<String>,
    via: Option<String>,
    revoked_at: Option<DateTime<Utc>>,
    #[sqlx(skip)]
    current: bool,
}

#[derive(Serialize, sqlx::FromRow)]
struct ExportEvent {
    id: i64,
    kind: String,
    at: DateTime<Utc>,
    ip: Option<String>,
    country: Option<String>,
    source_kind: Option<String>,
    source_id: Option<Uuid>,
}

#[derive(Serialize, sqlx::FromRow)]
struct ExportGrant {
    platform: String,
    label: String,
    client_version: String,
    ip: Option<String>,
    country: Option<String>,
    user_agent: Option<String>,
    created_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    status: String,
}

#[derive(Serialize, sqlx::FromRow)]
struct ExportMail {
    template: String,
    queued_at: DateTime<Utc>,
}

#[derive(Serialize, sqlx::FromRow)]
struct ExportSync {
    keyset_gen: Uuid,
    current_epoch: i32,
    pak_epoch: i32,
    rotation_ak: bool,
    rotation_pak: bool,
    head_seq: i64,
    records: i32,
    bytes: i64,
    history_records: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct ExportAccount {
    id: Uuid,
    email: String,
    email_verified_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    pw_changed_at: DateTime<Utc>,
    last_active_at: DateTime<Utc>,
    totp_enabled: bool,
    backup_total: i64,
    backup_unused: i64,
}

#[derive(sqlx::FromRow)]
struct RecordRow {
    c: String,
    id: Vec<u8>,
    seq: i64,
    e: i32,
    k: String,
    envelope: Value,
    size: i32,
    updated_at: DateTime<Utc>,
}

/// Everything of the export except the records, as a JSON object whose last member is an open
/// `"records":[` (the records stream after it).
async fn export_head(db: &PgPool, s: &WebSession) -> Result<String, sqlx::Error> {
    let id = s.account_id;
    let a: ExportAccount = sqlx::query_as(
        "SELECT a.id, a.email::text AS email, a.email_verified_at, a.created_at, a.pw_changed_at, a.last_active_at,
                a.totp_enabled,
                (SELECT count(*) FROM backup_codes b WHERE b.account_id = a.id) AS backup_total,
                (SELECT count(*) FROM backup_codes b WHERE b.account_id = a.id AND b.used_at IS NULL) AS backup_unused
         FROM accounts a WHERE a.id = $1",
    )
    .bind(id)
    .fetch_one(db)
    .await?;
    let devices: Vec<ExportDevice> = sqlx::query_as(
        "SELECT id, short, platform, label, client_version, created_at, last_used_at, last_sync_at,
                host(last_ip) AS last_ip, last_country, user_agent, has_keys, revoked_at
         FROM devices WHERE account_id = $1 ORDER BY created_at",
    )
    .bind(id)
    .fetch_all(db)
    .await?;
    // An address that isn't public came from the service's own network, not from the account
    // (server/src/client_ip.rs is_public); a row written before that rule still holds one.
    let devices: Vec<ExportDevice> =
        devices.into_iter().map(|d| ExportDevice { last_ip: client_ip::shown(d.last_ip.as_deref()), ..d }).collect();
    let sessions: Vec<ExportSession> = sqlx::query_as(
        "SELECT id, created_at, last_used_at, host(ip) AS ip, country, user_agent, coalesce(via, 'password') AS via,
                revoked_at
         FROM web_sessions WHERE account_id = $1 ORDER BY created_at",
    )
    .bind(id)
    .fetch_all(db)
    .await?;
    let sessions: Vec<ExportSession> = sessions
        .into_iter()
        .map(|x| ExportSession { current: x.id == s.id, ip: client_ip::shown(x.ip.as_deref()), ..x })
        .collect();
    let activity: Vec<ExportEvent> = sqlx::query_as(
        "SELECT id, kind, at, host(ip) AS ip, country, source_kind, source_id
         FROM ip_events WHERE account_id = $1 ORDER BY id",
    )
    .bind(id)
    .fetch_all(db)
    .await?;
    let activity: Vec<ExportEvent> =
        activity.into_iter().map(|x| ExportEvent { ip: client_ip::shown(x.ip.as_deref()), ..x }).collect();
    let link_grants: Vec<ExportGrant> = sqlx::query_as(
        "SELECT platform, label, client_version, host(ip) AS ip, country, user_agent, created_at, expires_at, status
         FROM link_grants
         WHERE account_id = $1 OR pair_id IN (SELECT p.id FROM pair_grants p WHERE p.account_id = $1)
         ORDER BY created_at",
    )
    .bind(id)
    .fetch_all(db)
    .await?;
    let link_grants: Vec<ExportGrant> =
        link_grants.into_iter().map(|x| ExportGrant { ip: client_ip::shown(x.ip.as_deref()), ..x }).collect();
    let pair_grants: Vec<PairExport> = sqlx::query_as(
        "SELECT created_at, expires_at, status, claim_kind, claimed_at, platform, label, client_version,
                host(ip) AS ip, country, user_agent
         FROM pair_grants WHERE account_id = $1",
    )
    .bind(id)
    .fetch_all(db)
    .await?;
    let pair_grants: Vec<PairExport> =
        pair_grants.into_iter().map(|x| PairExport { ip: client_ip::shown(x.ip.as_deref()), ..x }).collect();
    let emails: Vec<ExportMail> =
        sqlx::query_as("SELECT template, queued_at FROM email_log WHERE account_id = $1 ORDER BY id")
            .bind(id)
            .fetch_all(db)
            .await?;
    let sync: Option<ExportSync> = sqlx::query_as(
        "SELECT keyset_gen, current_epoch, pak_epoch, rotation_ak, rotation_pak, head_seq, records, bytes,
                history_records, created_at, updated_at
         FROM sync_state WHERE account_id = $1",
    )
    .bind(id)
    .fetch_optional(db)
    .await?;
    let record_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM records WHERE account_id = $1").bind(id).fetch_one(db).await?;
    let head = json!({
        "schema": EXPORT_SCHEMA,
        "generated_at": Utc::now(),
        "account": {
            "id": a.id, "email": a.email, "email_verified_at": a.email_verified_at, "created_at": a.created_at,
            "password_changed_at": a.pw_changed_at, "last_active_at": a.last_active_at,
            "totp_enabled": a.totp_enabled,
            "backup_codes": { "total": a.backup_total, "unused": a.backup_unused },
        },
        "devices": devices,
        "web_sessions": sessions,
        "activity": activity,
        "pending": { "link_grants": link_grants, "pair_grants": pair_grants },
        "emails": emails,
        "sync": {
            "has_keyset": sync.is_some(),
            "state": sync,
            "record_count": record_count,
            "note": "Records are end-to-end encrypted: only your devices hold the keys, and no key material is part of this download.",
        },
    });
    let mut s = serde_json::to_string(&head).expect("JSON values serialise");
    s.pop(); // the closing brace
    s.push_str(",\"records\":[");
    Ok(s)
}

fn record_json(r: RecordRow) -> Value {
    json!({
        "c": r.c, "id": crypto::b64url(&r.id), "seq": r.seq, "e": r.e, "k": r.k,
        "envelope": r.envelope, "size": r.size, "updated_at": r.updated_at,
    })
}

/// `POST web/export {}` (re-auth) → the account's data as a JSON download, streamed. Never
/// mailed.
pub async fn export(
    State(state): State<AppState>,
    client: Client,
    s: WebSession,
    JsonBody(_): JsonBody<Empty>,
) -> ApiResult {
    hit(&state.db, rl::EXPORT, &rl::id_key(&s.account_id)).await?;
    if !s.reauth_valid() {
        return Err(reauth_required());
    }
    let mut tx = state.db.begin().await?;
    events::record(&mut tx, s.account_id, "export", &client, Source::Web(s.id)).await?;
    tx.commit().await?;
    let head = export_head(&state.db, &s).await?;
    // Pages by seq (unique per account); records written meanwhile may or may not be included.
    struct Cursor {
        db: PgPool,
        account: Uuid,
        after: i64,
        first: bool,
        done: bool,
    }
    let start = Cursor { db: state.db.clone(), account: s.account_id, after: -1, first: true, done: false };
    let pages = futures_util::stream::unfold(start, |mut c| async move {
        if c.done {
            return None;
        }
        let rows: Result<Vec<RecordRow>, sqlx::Error> = sqlx::query_as(
            "SELECT c, id, seq, e, k, envelope, size, updated_at FROM records
             WHERE account_id = $1 AND seq > $2 ORDER BY seq LIMIT $3",
        )
        .bind(c.account)
        .bind(c.after)
        .bind(EXPORT_PAGE)
        .fetch_all(&c.db)
        .await;
        match rows {
            Err(e) => {
                tracing::error!(error = %e, "export failed while streaming");
                c.done = true;
                Some((Err(std::io::Error::other("database")), c))
            }
            Ok(rows) => {
                let mut out = String::new();
                if (rows.len() as i64) < EXPORT_PAGE {
                    c.done = true;
                }
                for r in rows {
                    if !c.first {
                        out.push(',');
                    }
                    c.first = false;
                    c.after = r.seq;
                    out.push_str(&record_json(r).to_string());
                }
                if c.done {
                    out.push_str("]}");
                }
                Some((Ok(Bytes::from(out)), c))
            }
        }
    });
    let body = futures_util::StreamExt::chain(
        futures_util::stream::once(async move { Ok::<_, std::io::Error>(Bytes::from(head)) }),
        pages,
    );
    let name = format!("krunk-account-{}.json", Utc::now().format("%Y-%m-%d"));
    let mut res = Body::from_stream(body).into_response();
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{name}\"")).expect("an ASCII file name"),
    );
    tracing::info!("data download");
    Ok(res)
}

// ---- Sync reset -----------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResetReq {
    confirm: String,
}

/// `POST web/sync/reset {confirm:"DELETE SYNC DATA"}` (re-auth) → 200: the keysets, escrow,
/// records and approvals are gone; the account and its devices stay, without keys.
pub async fn sync_reset(
    State(state): State<AppState>,
    client: Client,
    s: WebSession,
    JsonBody(req): JsonBody<ResetReq>,
) -> ApiResult {
    hit(&state.db, rl::SYNC_RESET, &rl::id_key(&s.account_id)).await?;
    if !s.reauth_valid() {
        return Err(reauth_required());
    }
    if req.confirm != RESET_CONFIRM {
        return Err(ApiError::bad_request("bad_request", "Type DELETE SYNC DATA to confirm."));
    }
    let id = s.account_id;
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT id FROM accounts WHERE id = $1 FOR UPDATE").bind(id).execute(&mut *tx).await?;
    let records =
        sqlx::query("DELETE FROM records WHERE account_id = $1").bind(id).execute(&mut *tx).await?.rows_affected();
    for t in ["approvals", "ak_dirs", "pak_dirs", "escrow", "sync_state"] {
        sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM {t} WHERE account_id = $1")))
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("UPDATE devices SET has_keys = false, had_keys = false WHERE account_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    mail::enqueue(&mut tx, Some(id), &s.email, Template::SyncReset, json!({ "records": records.to_string() })).await?;
    events::record(&mut tx, id, "sync_reset", &client, Source::Web(s.id)).await?;
    tx.commit().await?;
    tracing::info!(records, "sync reset");
    Ok(Json(json!({ "deleted": { "records": records, "keys": true } })).into_response())
}

// ---- Deletion -------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteReq {
    password: String,
    totp: Option<String>,
    turnstile: String,
    confirm_email: String,
}

#[derive(sqlx::FromRow)]
struct Counts {
    records: i64,
    devices: i64,
    sessions: i64,
    keys: i64,
}

const COUNTS: &str = "SELECT (SELECT count(*) FROM records WHERE account_id = $1) AS records,
        (SELECT count(*) FROM devices WHERE account_id = $1) AS devices,
        (SELECT count(*) FROM web_sessions WHERE account_id = $1) AS sessions,
        (SELECT count(*) FROM ak_dirs WHERE account_id = $1) + (SELECT count(*) FROM pak_dirs WHERE account_id = $1)
          + (SELECT count(*) FROM escrow WHERE account_id = $1) AS keys";

/// `POST web/account/delete {password, totp?, turnstile, confirm_email}` → 200: the account and
/// everything it owns are deleted at once (PLAN §7), the id goes to `account_deletions`, a
/// receipt is mailed and the cookie is cleared. Every token and cookie answers 401 afterwards.
pub async fn delete_account(
    State(state): State<AppState>,
    client: Client,
    s: WebSession,
    JsonBody(req): JsonBody<DeleteReq>,
) -> ApiResult {
    hit(&state.db, rl::DELETE_IP, &rl::ip_key(&state.config.keys, client.ip())).await?;
    hit(&state.db, rl::DELETE_ACCOUNT, &rl::id_key(&s.account_id)).await?;
    check_lengths(&[
        (&req.password, PASSWORD_MAX),
        (&req.turnstile, TURNSTILE_MAX),
        (&req.confirm_email, EMAIL_MAX),
        (req.totp.as_deref().unwrap_or(""), CODE_MAX),
    ])?;
    state.turnstile.verify(&state.db, &req.turnstile, "delete", client.ip()).await?;
    // Addresses compare case-insensitively, as the database does (citext).
    let norm = |e: &str| accounts::normalize_email(e).map(|e| e.to_lowercase());
    if norm(&req.confirm_email).is_none() || norm(&req.confirm_email) != norm(&s.email) {
        return Err(ApiError::bad_request("bad_request", "Type your account's email address to confirm."));
    }
    if !state.passwords.verify(&req.password, Some(&accounts::current_phc(&state, s.account_id).await?)).await {
        return Err(accounts::wrong_password());
    }
    accounts::second_factor(&state, s.account_id, req.totp.as_deref()).await?;
    let id = s.account_id;
    let mut tx = state.db.begin().await?;
    sqlx::query("SELECT id FROM accounts WHERE id = $1 FOR UPDATE").bind(id).execute(&mut *tx).await?;
    let before: Counts = sqlx::query_as(COUNTS).bind(id).fetch_one(&mut *tx).await?;
    sqlx::query("INSERT INTO account_deletions (account_id) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    // The account's other mails (queued or kept for 7 days) hold its address: gone too.
    sqlx::query("DELETE FROM email_outbox WHERE account_id = $1").bind(id).execute(&mut *tx).await?;
    // The receipt has no account (it is about to be deleted); the worker sends it as usual.
    mail::enqueue(
        &mut tx,
        None,
        &s.email,
        Template::AccountDeleted,
        json!({
            "time": Utc::now().format("%Y-%m-%d %H:%M UTC").to_string(),
            "records": before.records.to_string(),
            "devices": before.devices.to_string(),
            "sessions": before.sessions.to_string(),
        }),
    )
    .await?;
    sqlx::query("DELETE FROM accounts WHERE id = $1").bind(id).execute(&mut *tx).await?;
    let after: Counts = sqlx::query_as(COUNTS).bind(id).fetch_one(&mut *tx).await?;
    tx.commit().await?;
    tracing::info!(records = before.records, devices = before.devices, "account deleted");
    let mut res = Json(json!({
        "deleted": true,
        "remaining": { "records": after.records, "devices": after.devices, "sessions": after.sessions, "keys": after.keys },
    }))
    .into_response();
    res.headers_mut().insert(header::SET_COOKIE, sessions::clear_cookie());
    Ok(res)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_export_head_leaves_records_open() {
        let mut s = serde_json::to_string(&serde_json::json!({"schema": super::EXPORT_SCHEMA})).unwrap();
        s.pop();
        s.push_str(",\"records\":[");
        s.push_str("]}");
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["records"], serde_json::json!([]));
    }
}
