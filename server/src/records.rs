//! Milestone S7: the encrypted records (PROTOCOL §9, §10, §11.7).
//!
//! - **Pull** (`GET records?since=`): the account's records after a `seq`, in write order, 500 at
//!   most. Asking from below the purge horizon is 410: the client reconciles in full (§10.4).
//! - **Push** (`POST records/batch`): every write is conditional on the `seq` the client last saw
//!   (`if_seq`, 0 for a new record); a stale one comes back as a conflict with the current
//!   envelope, the others get a fresh `seq`. A purge removes a record (only a client knows it is
//!   an old tombstone) and raises the horizon. The server checks the envelope's structure only:
//!   the version, the collection and its key kind, the id, the epoch (the current one of that key
//!   kind), the nonce and the ciphertext's size. Quotas are checked for the batch as a whole.
//! - **Rotation** (§7): while a key kind is frozen, writes of it are 423; after the next epoch,
//!   `rekey/batch` swaps old-epoch records for re-encrypted ones atomically, and `sync/state`
//!   counts what is left (`remaining_old`).

use crate::api::{ApiError, ApiResult, JsonBody};
use crate::crypto;
use crate::devices::DeviceAuth;
use crate::http::AppState;
use crate::keys::{no_keyset, rotation_pending, state_row};
use crate::ratelimit::{self as rl, hit};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgConnection;
use uuid::Uuid;

/// Most writes per batch and records per pull (PROTOCOL §11.7).
pub const BATCH_MAX: usize = 500;
/// The largest ciphertext: the largest padding bucket plus the AEAD tag (§9.2).
pub const CT_MAX: usize = 65536 + 16;

/// The collections of v1 and their key kind (PROTOCOL §9.5).
const COLLECTIONS: &[(&str, &str)] = &[
    ("history", "ak"),
    ("bookmarks", "ak"),
    ("settings", "ak"),
    ("tabs", "ak"),
    ("allowlist", "ak"),
    ("pins", "ak"),
    ("device_meta", "ak"),
    ("passwords", "pak"),
    ("never_save", "pak"),
    ("payments", "pak"),
];

fn kind_of(c: &str) -> Option<&'static str> {
    COLLECTIONS.iter().find(|(n, _)| *n == c).map(|(_, k)| *k)
}

fn bad(message: &'static str) -> ApiError {
    ApiError::bad_request("bad_request", message)
}

fn quota_exceeded() -> ApiError {
    ApiError::bad_request("quota_exceeded", "This account's sync storage is full.")
}

/// Records still under an older epoch than their key kind's current one.
pub async fn remaining_old(conn: &mut PgConnection, account_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*) FROM records r JOIN sync_state s ON s.account_id = r.account_id
         WHERE r.account_id = $1 AND ((r.k = 'ak' AND r.e < s.current_epoch) OR (r.k = 'pak' AND r.e < s.pak_epoch))",
    )
    .bind(account_id)
    .fetch_one(conn)
    .await
}

async fn touch_sync(conn: &mut PgConnection, device: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE devices SET last_sync_at = now() WHERE id = $1 AND (last_sync_at IS NULL OR last_sync_at < now() - interval '1 minute')")
        .bind(device)
        .execute(conn)
        .await?;
    Ok(())
}

// ---- Envelope structure ----------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvelopeIn {
    v: u8,
    c: String,
    id: String,
    e: u32,
    k: String,
    n: String,
    ct: String,
}

struct Checked {
    id: [u8; 16],
    e: i32,
    k: &'static str,
    size: i32,
}

/// The structure of an envelope for collection `c` and record `id` (§11.7): what the server can
/// check without keys.
fn check_envelope(c: &str, id: &str, env: &Value) -> Result<Checked, ApiError> {
    let k = kind_of(c).ok_or_else(|| bad("Unknown collection."))?;
    let rid = crypto::b64url_decode::<16>(id).ok_or_else(|| bad("The record id is not valid."))?;
    let e: EnvelopeIn = serde_json::from_value(env.clone()).map_err(|_| bad("The envelope is not valid."))?;
    if e.v != 1 || e.c != c || e.id != id || e.k != k {
        return Err(bad("The envelope doesn't match its collection, id or key kind."));
    }
    if crypto::b64url_decode::<24>(&e.n).is_none() {
        return Err(bad("The envelope's nonce is not valid."));
    }
    if e.ct.len() > CT_MAX.div_ceil(3) * 4 {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large", "A record is too large."));
    }
    let ct = <base64ct::Base64UrlUnpadded as base64ct::Encoding>::decode_vec(&e.ct)
        .map_err(|_| bad("The ciphertext is not valid."))?;
    if ct.len() > CT_MAX {
        return Err(ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large", "A record is too large."));
    }
    if ct.len() < 16 + 256 {
        return Err(bad("The ciphertext is smaller than the smallest bucket."));
    }
    let epoch = i32::try_from(e.e).ok().filter(|x| *x >= 1).ok_or_else(|| bad("epoch"))?;
    Ok(Checked { id: rid, e: epoch, k, size: ct.len() as i32 })
}

// ---- Pull --------------------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct PullQuery {
    #[serde(default)]
    since: i64,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    epoch_lt: Option<i32>,
    #[serde(default)]
    k: Option<String>,
}

#[derive(sqlx::FromRow)]
struct Row {
    seq: i64,
    envelope: Value,
}

/// `GET records?since=<seq>&limit≤500[&k=ak|pak][&epoch_lt=<e>]` → `{records:[{seq, envelope}],
/// has_more, head_seq}`. With `k`, only records of that key kind (a device whose vault was locked
/// catches up on the passwords alone). With `epoch_lt`, only records of key kind `k` (default
/// `ak`) under an older epoch: what a rotator still has to re-encrypt.
pub async fn pull(State(state): State<AppState>, d: DeviceAuth, Query(q): Query<PullQuery>) -> ApiResult {
    hit(&state.db, rl::RECORDS_READ, &rl::id_key(&d.device_id)).await?;
    let limit = q.limit.unwrap_or(BATCH_MAX as i64).clamp(1, BATCH_MAX as i64);
    let mut conn = state.db.acquire().await?;
    let horizon: Option<(i64, i64)> =
        sqlx::query_as("SELECT head_seq, purge_horizon_seq FROM sync_state WHERE account_id = $1")
            .bind(d.account_id)
            .fetch_optional(&mut *conn)
            .await?;
    let (head, horizon) = horizon.ok_or_else(no_keyset)?;
    if q.since > 0 && q.since < horizon {
        return Err(ApiError::new(StatusCode::GONE, "gone", "Too old: fetch everything again."));
    }
    let kind = match q.k.as_deref() {
        None => None,
        Some("ak") => Some("ak"),
        Some("pak") => Some("pak"),
        _ => return Err(bad("k")),
    };
    let rows: Vec<Row> = match (q.epoch_lt, kind) {
        (None, None) => {
            sqlx::query_as("SELECT seq, envelope FROM records WHERE account_id = $1 AND seq > $2 ORDER BY seq LIMIT $3")
                .bind(d.account_id)
                .bind(q.since)
                .bind(limit + 1)
                .fetch_all(&mut *conn)
                .await?
        }
        (None, Some(k)) => {
            sqlx::query_as(
                "SELECT seq, envelope FROM records WHERE account_id = $1 AND seq > $2 AND k = $3 ORDER BY seq LIMIT $4",
            )
            .bind(d.account_id)
            .bind(q.since)
            .bind(k)
            .bind(limit + 1)
            .fetch_all(&mut *conn)
            .await?
        }
        (Some(e), k) => {
            let k = k.unwrap_or("ak");
            sqlx::query_as(
                "SELECT seq, envelope FROM records WHERE account_id = $1 AND seq > $2 AND k = $3 AND e < $4
                 ORDER BY seq LIMIT $5",
            )
            .bind(d.account_id)
            .bind(q.since)
            .bind(k)
            .bind(e)
            .bind(limit + 1)
            .fetch_all(&mut *conn)
            .await?
        }
    };
    let has_more = rows.len() as i64 > limit;
    let records: Vec<Value> =
        rows.into_iter().take(limit as usize).map(|r| json!({"seq": r.seq, "envelope": r.envelope})).collect();
    touch_sync(&mut conn, d.device_id).await?;
    Ok(Json(json!({"records": records, "has_more": has_more, "head_seq": head})).into_response())
}

// ---- Push --------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Write {
    c: String,
    id: String,
    #[serde(default)]
    if_seq: i64,
    #[serde(default)]
    envelope: Option<Value>,
    #[serde(default)]
    purge: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchReq {
    writes: Vec<Write>,
}

struct Counters {
    records: i64,
    bytes: i64,
    history: i64,
}

/// A write from a device that isn't in the account directory is logged, not refused: shipped
/// clients reach this after a sign-out and sign-in with sync.json left over (audit C19), so
/// refusing it (audit S4) waits for a client that handles that (0.12.0). The log shows how
/// many there are.
fn keyless_write(d: &DeviceAuth, route: &'static str) {
    if !d.has_keys {
        tracing::warn!(device = %d.device_id, route, "write from a device without keys (audit S4/C19)");
    }
}

/// `POST records/batch {writes}` → `{results:[{id, status:"ok", seq} | {id, status:"conflict",
/// current:{seq, envelope}}], head_seq}`.
pub async fn batch(State(state): State<AppState>, d: DeviceAuth, JsonBody(req): JsonBody<BatchReq>) -> ApiResult {
    hit(&state.db, rl::RECORDS_WRITE, &rl::id_key(&d.device_id)).await?;
    keyless_write(&d, "records/batch");
    if req.writes.is_empty() || req.writes.len() > BATCH_MAX {
        return Err(bad("A batch has 1 to 500 writes."));
    }
    let mut tx = state.db.begin().await?;
    let st = state_row(&mut tx, d.account_id, true).await?.ok_or_else(no_keyset)?;
    // Check every write before touching anything.
    let mut checked = Vec::with_capacity(req.writes.len());
    for w in &req.writes {
        let k = kind_of(&w.c).ok_or_else(|| bad("Unknown collection."))?;
        if (k == "ak" && st.rotation_ak) || (k == "pak" && st.rotation_pak) {
            return Err(rotation_pending());
        }
        let c = match (&w.envelope, w.purge) {
            (Some(env), false) => {
                let c = check_envelope(&w.c, &w.id, env)?;
                let current = if c.k == "ak" { st.current_epoch } else { st.pak_epoch };
                if c.e != current {
                    return Err(ApiError::bad_request(
                        "bad_request",
                        "Records are written under the current epoch only.",
                    ));
                }
                Some(c)
            }
            (None, true) => {
                crypto::b64url_decode::<16>(&w.id).ok_or_else(|| bad("The record id is not valid."))?;
                None
            }
            _ => return Err(bad("A write has an envelope or is a purge.")),
        };
        checked.push(c);
    }
    let mut counters =
        Counters { records: i64::from(st.records), bytes: st.bytes, history: i64::from(st.history_records) };
    let mut head = st.head_seq;
    let mut horizon_raise: i64 = 0;
    let mut results = Vec::with_capacity(req.writes.len());
    for (w, c) in req.writes.iter().zip(checked) {
        let rid = crypto::b64url_decode::<16>(&w.id).expect("checked");
        let cur: Option<(i64, Value, i32)> = sqlx::query_as(
            "SELECT seq, envelope, size FROM records WHERE account_id = $1 AND c = $2 AND id = $3 FOR UPDATE",
        )
        .bind(d.account_id)
        .bind(&w.c)
        .bind(rid.as_slice())
        .fetch_optional(&mut *tx)
        .await?;
        let cur_seq = cur.as_ref().map_or(0, |x| x.0);
        if w.if_seq != cur_seq {
            results.push(match cur {
                Some((seq, env, _)) => {
                    json!({"id": w.id, "status": "conflict", "current": {"seq": seq, "envelope": env}})
                }
                None => json!({"id": w.id, "status": "conflict", "current": null}),
            });
            continue;
        }
        let history = i64::from(w.c == "history");
        match c {
            None => {
                // A purge (a tombstone older than 180 days, PROTOCOL §9.4).
                if let Some((seq, _, size)) = cur {
                    sqlx::query("DELETE FROM records WHERE account_id = $1 AND c = $2 AND id = $3")
                        .bind(d.account_id)
                        .bind(&w.c)
                        .bind(rid.as_slice())
                        .execute(&mut *tx)
                        .await?;
                    counters.records -= 1;
                    counters.bytes -= i64::from(size);
                    counters.history -= history;
                    horizon_raise = horizon_raise.max(seq);
                }
                results.push(json!({"id": w.id, "status": "ok", "seq": null}));
            }
            Some(c) => {
                head += 1;
                let env = w.envelope.as_ref().expect("checked");
                match cur {
                    Some((_, _, old_size)) => {
                        sqlx::query(
                            "UPDATE records SET seq = $4, e = $5, k = $6, envelope = $7, size = $8, updated_at = now()
                             WHERE account_id = $1 AND c = $2 AND id = $3",
                        )
                        .bind(d.account_id)
                        .bind(&w.c)
                        .bind(c.id.as_slice())
                        .bind(head)
                        .bind(c.e)
                        .bind(c.k)
                        .bind(env)
                        .bind(c.size)
                        .execute(&mut *tx)
                        .await?;
                        counters.bytes += i64::from(c.size - old_size);
                    }
                    None => {
                        sqlx::query(
                            "INSERT INTO records (account_id, c, id, seq, e, k, envelope, size) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
                        )
                        .bind(d.account_id)
                        .bind(&w.c)
                        .bind(c.id.as_slice())
                        .bind(head)
                        .bind(c.e)
                        .bind(c.k)
                        .bind(env)
                        .bind(c.size)
                        .execute(&mut *tx)
                        .await?;
                        counters.records += 1;
                        counters.bytes += i64::from(c.size);
                        counters.history += history;
                    }
                }
                results.push(json!({"id": w.id, "status": "ok", "seq": head}));
            }
        }
    }
    let q = &state.config.quota;
    if counters.records > i64::from(q.max_records)
        || counters.bytes > q.max_bytes
        || counters.history > i64::from(q.max_history)
    {
        // Nothing of the batch is kept.
        return Err(quota_exceeded());
    }
    sqlx::query(
        "UPDATE sync_state SET head_seq = $2, records = $3, bytes = $4, history_records = $5,
                purge_horizon_seq = greatest(purge_horizon_seq, $6), updated_at = now() WHERE account_id = $1",
    )
    .bind(d.account_id)
    .bind(head)
    .bind(counters.records as i32)
    .bind(counters.bytes)
    .bind(counters.history as i32)
    .bind(horizon_raise)
    .execute(&mut *tx)
    .await?;
    touch_sync(&mut tx, d.device_id).await?;
    tx.commit().await?;
    Ok(Json(json!({"results": results, "head_seq": head})).into_response())
}

// ---- Rekey -------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Replace {
    old_id: String,
    c: String,
    envelope: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RekeyReq {
    epoch: u32,
    replaces: Vec<Replace>,
}

/// `POST rekey/batch {epoch, replaces:[{old_id, c, envelope}]}` → `{done, remaining_old}`: each
/// old-epoch record is swapped for its re-encryption under the current epoch (a new id; §7
/// step 5), atomically. One that another rotator already swapped is skipped.
pub async fn rekey(State(state): State<AppState>, d: DeviceAuth, JsonBody(req): JsonBody<RekeyReq>) -> ApiResult {
    hit(&state.db, rl::REKEY, &rl::id_key(&d.device_id)).await?;
    keyless_write(&d, "rekey/batch");
    if req.replaces.is_empty() || req.replaces.len() > BATCH_MAX {
        return Err(bad("A batch has 1 to 500 replacements."));
    }
    let mut tx = state.db.begin().await?;
    let st = state_row(&mut tx, d.account_id, true).await?.ok_or_else(no_keyset)?;
    let mut head = st.head_seq;
    let mut bytes = st.bytes;
    let mut done = 0;
    for r in &req.replaces {
        let c = check_envelope(&r.c, r.envelope.get("id").and_then(Value::as_str).unwrap_or_default(), &r.envelope)?;
        let current = if c.k == "ak" { st.current_epoch } else { st.pak_epoch };
        if c.e != current || req.epoch as i32 != current {
            return Err(ApiError::new(StatusCode::CONFLICT, "conflict", "The epoch changed: fetch the keys again."));
        }
        if (c.k == "ak" && st.rotation_ak) || (c.k == "pak" && st.rotation_pak) {
            return Err(rotation_pending());
        }
        let old = crypto::b64url_decode::<16>(&r.old_id).ok_or_else(|| bad("old_id"))?;
        let gone: Option<(i32, i32)> = sqlx::query_as(
            "DELETE FROM records WHERE account_id = $1 AND c = $2 AND id = $3 AND e < $4 RETURNING e, size",
        )
        .bind(d.account_id)
        .bind(&r.c)
        .bind(old.as_slice())
        .bind(current)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((_, old_size)) = gone else { continue };
        head += 1;
        let inserted = sqlx::query(
            "INSERT INTO records (account_id, c, id, seq, e, k, envelope, size) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (account_id, c, id) DO NOTHING",
        )
        .bind(d.account_id)
        .bind(&r.c)
        .bind(c.id.as_slice())
        .bind(head)
        .bind(c.e)
        .bind(c.k)
        .bind(&r.envelope)
        .bind(c.size)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        bytes += if inserted == 1 { i64::from(c.size - old_size) } else { -i64::from(old_size) };
        done += 1;
    }
    // The storage quota, as for writes: re-encryptions may not grow the account past it
    // (0.11.7, audit S3: rekey had no check, so an account could grow without limit). An
    // account already over it (a lowered quota) can still rekey without growing.
    if bytes > state.config.quota.max_bytes && bytes > st.bytes {
        return Err(quota_exceeded());
    }
    sqlx::query(
        "UPDATE sync_state SET head_seq = $2, bytes = $3,
                records = (SELECT count(*) FROM records WHERE account_id = $1),
                history_records = (SELECT count(*) FROM records WHERE account_id = $1 AND c = 'history'),
                updated_at = now() WHERE account_id = $1",
    )
    .bind(d.account_id)
    .bind(head)
    .bind(bytes)
    .execute(&mut *tx)
    .await?;
    let remaining = remaining_old(&mut tx, d.account_id).await?;
    touch_sync(&mut tx, d.device_id).await?;
    tx.commit().await?;
    Ok(Json(json!({"done": done, "remaining_old": remaining})).into_response())
}
