//! Milestone S6: the sync keys on the server (PROTOCOL §4, §5, §7, §11.4–§11.5, §11.8).
//!
//! The server stores what it can't read and checks only what it can check without keys:
//! - **Directories** (`ak_dir`, `pak_dir`): their exact bytes and tags, one row per version.
//!   `keys/commit` is a compare-and-swap on the head's hash and checks the structure: the next
//!   `seq`, the `prev_hash`, the account, that every listed device is an active device of the
//!   account, that a revoked device never comes back, the sizes, and that the passphrase KDF
//!   parameters match the escrow's. The cryptographic checks (MACs, seals) are the clients'
//!   (§4.4): a server that lies is caught there.
//! - **Escrow:** the recovery root wrapped under the passphrase and the recovery key. Every fetch
//!   is mailed to the account (5 per day).
//! - **Rotation state:** removing a device that ever held keys sets `rotation_ak/pak`; until the
//!   next epoch is committed, a same-epoch commit is refused (423) and so are record writes (S7).
//!
//! No endpoint here ever returns a key: there is none on the server.

use crate::api::{ApiError, ApiResult, Client, JsonBody};
use crate::crypto;
use crate::devices::{self, DeviceAuth};
use crate::events::Source;
use crate::http::AppState;
use crate::mail::{self, Template};
use crate::ratelimit::{self as rl, hit};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgConnection;
use std::collections::BTreeSet;
use uuid::Uuid;

/// The largest directory accepted (a few dozen devices, seals and a history wrap).
pub const DIR_MAX: usize = 256 * 1024;
/// Most devices, vault keys or revoked ids in a directory; most seals.
const LIST_MAX: usize = 256;
const SEALS_MAX: usize = 512;
/// The longest chain `keys/chain` returns in one answer.
const CHAIN_MAX: i64 = 1000;
/// A rotation claim lasts this long (advisory; PROTOCOL §7 step 2).
pub const LEASE_S: i64 = 600;

fn conflict_head(ak: Option<&Head>, pak: Option<&Head>) -> Response {
    let h = |x: Option<&Head>| x.map(|h| json!({"seq": h.seq, "hash": crypto::b64url(&h.hash)}));
    (
        StatusCode::CONFLICT,
        Json(json!({
            "error": "conflict",
            "message": "The keys changed meanwhile. Fetch them again.",
            "head": {"ak": h(ak), "pak": h(pak)},
        })),
    )
        .into_response()
}

fn bad(message: &'static str) -> ApiError {
    ApiError::bad_request("bad_request", message)
}

pub fn no_keyset() -> ApiError {
    ApiError::new(StatusCode::NOT_FOUND, "no_keyset", "Sync isn't set up on this account yet.")
}

pub fn rotation_pending() -> ApiError {
    ApiError::new(StatusCode::LOCKED, "rotation_pending", "A device was removed: the keys must be rotated first.")
}

// ---- Stored directories ---------------------------------------------------------------------

#[derive(Clone, sqlx::FromRow)]
pub struct Head {
    pub seq: i64,
    pub epoch: i32,
    pub hash: Vec<u8>,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Ak,
    Pak,
}

impl Kind {
    fn table(self) -> &'static str {
        match self {
            Kind::Ak => "ak_dirs",
            Kind::Pak => "pak_dirs",
        }
    }
}

pub async fn head(conn: &mut PgConnection, account_id: Uuid, kind: Kind) -> Result<Option<Head>, sqlx::Error> {
    let sql =
        format!("SELECT seq, epoch, hash, bytes FROM {} WHERE account_id = $1 ORDER BY seq DESC LIMIT 1", kind.table());
    sqlx::query_as(sqlx::AssertSqlSafe(sql)).bind(account_id).fetch_optional(conn).await
}

#[derive(sqlx::FromRow)]
struct BlobRow {
    seq: i64,
    bytes: Vec<u8>,
    mac: Vec<u8>,
    chain: Option<Vec<u8>>,
    escrow_mac: Option<Vec<u8>>,
}

#[derive(Serialize)]
struct BlobOut {
    seq: i64,
    bytes: String,
    mac: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    chain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    escrow_mac: Option<String>,
}

impl From<BlobRow> for BlobOut {
    fn from(r: BlobRow) -> BlobOut {
        BlobOut {
            seq: r.seq,
            bytes: crypto::b64url(&r.bytes),
            mac: crypto::b64url(&r.mac),
            chain: r.chain.as_deref().map(crypto::b64url),
            escrow_mac: r.escrow_mac.as_deref().map(crypto::b64url),
        }
    }
}

async fn blobs(
    conn: &mut PgConnection,
    account_id: Uuid,
    kind: Kind,
    from_seq: i64,
) -> Result<Vec<BlobOut>, sqlx::Error> {
    let sql = format!(
        "SELECT seq, bytes, mac, chain, escrow_mac FROM {} WHERE account_id = $1 AND seq >= $2 ORDER BY seq LIMIT $3",
        kind.table()
    );
    let rows: Vec<BlobRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(account_id)
        .bind(from_seq)
        .bind(CHAIN_MAX)
        .fetch_all(conn)
        .await?;
    Ok(rows.into_iter().map(BlobOut::from).collect())
}

// ---- The structure of a directory (what the server can check) --------------------------------

#[derive(Deserialize)]
struct Ref {
    #[serde(alias = "device")]
    id: String,
}

#[derive(Deserialize)]
struct SealRef {
    to: String,
}

/// The fields the server looks at. Unknown fields are ignored here (the clients are strict).
#[derive(Deserialize)]
struct DirFields {
    v: u8,
    account_id: String,
    #[serde(default)]
    epoch: Option<u32>,
    #[serde(default)]
    pak_epoch: Option<u32>,
    seq: u64,
    prev_hash: String,
    #[serde(default)]
    devices: Option<Vec<Ref>>,
    #[serde(default)]
    revoked: Option<Vec<String>>,
    #[serde(default)]
    vault_keys: Option<Vec<Ref>>,
    seals: Vec<SealRef>,
    #[serde(default)]
    pp_kdf: Option<Value>,
    #[serde(default)]
    esc_pk: Option<String>,
    #[serde(default)]
    rr_gen: Option<u32>,
    created_by: String,
}

/// A directory version as sent in `keys/commit`: base64url bytes and tags.
#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct BlobIn {
    pub bytes: String,
    pub mac: String,
    #[serde(default)]
    pub chain: Option<String>,
    #[serde(default)]
    pub escrow_mac: Option<String>,
}

struct Checked {
    bytes: Vec<u8>,
    hash: [u8; 32],
    mac: [u8; 32],
    chain: Option<[u8; 32]>,
    escrow_mac: Option<[u8; 32]>,
    seq: i64,
    epoch: i32,
    /// ak: the listed devices; pak: the devices with a vault key.
    members: BTreeSet<Uuid>,
    revoked: BTreeSet<Uuid>,
    pp_kdf: Option<Value>,
    esc_pk: Option<String>,
    rr_gen: Option<u32>,
}

fn tag(s: &Option<String>) -> Result<Option<[u8; 32]>, ApiError> {
    match s {
        None => Ok(None),
        Some(t) => crypto::b64url_decode::<32>(t).map(Some).ok_or_else(|| bad("A tag is not valid.")),
    }
}

fn uuid_set(ids: &[Ref], what: &'static str) -> Result<BTreeSet<Uuid>, ApiError> {
    if ids.len() > LIST_MAX {
        return Err(bad(what));
    }
    let mut out = BTreeSet::new();
    for r in ids {
        let id: Uuid = r.id.parse().map_err(|_| bad(what))?;
        if !out.insert(id) {
            return Err(bad(what));
        }
    }
    Ok(out)
}

/// The structure of one directory version against the account, the caller and the head.
fn check_blob(
    kind: Kind,
    blob: &BlobIn,
    account_id: Uuid,
    caller: Uuid,
    head: Option<&Head>,
) -> Result<Checked, ApiError> {
    let bytes = <base64ct::Base64UrlUnpadded as base64ct::Encoding>::decode_vec(&blob.bytes)
        .ok()
        .filter(|b| !b.is_empty() && b.len() <= DIR_MAX)
        .ok_or_else(|| bad("The directory is not valid."))?;
    let mac = crypto::b64url_decode::<32>(&blob.mac).ok_or_else(|| bad("A tag is not valid."))?;
    let chain = tag(&blob.chain)?;
    let escrow_mac = tag(&blob.escrow_mac)?;
    let f: DirFields = serde_json::from_slice(&bytes).map_err(|_| bad("The directory is not valid."))?;
    if f.v != 1 || crypto::b64url_decode::<16>(&f.account_id) != Some(*account_id.as_bytes()) {
        return Err(bad("The directory is for another account or version."));
    }
    if f.created_by.parse::<Uuid>().ok() != Some(caller) {
        return Err(bad("The directory names another author."));
    }
    let seq = i64::try_from(f.seq).map_err(|_| bad("seq"))?;
    let epoch = match kind {
        Kind::Ak => f.epoch,
        Kind::Pak => f.pak_epoch,
    }
    .and_then(|e| i32::try_from(e).ok())
    .filter(|e| *e >= 1)
    .ok_or_else(|| bad("The directory has no epoch."))?;
    // Order: seq 1 with an empty prev_hash, or the head's successor.
    match head {
        None => {
            if seq != 1 || !f.prev_hash.is_empty() {
                return Err(bad("The first directory has seq 1."));
            }
        }
        Some(h) => {
            if seq != h.seq + 1 || crypto::b64url_decode::<32>(&f.prev_hash).map(|x| x.to_vec()) != Some(h.hash.clone())
            {
                return Err(bad("The directory doesn't follow the current one."));
            }
            if epoch != h.epoch && epoch != h.epoch + 1 {
                return Err(bad("Epochs move one at a time."));
            }
            if (epoch == h.epoch + 1) != chain.is_some() {
                return Err(bad("A new epoch carries a chain tag, and only then."));
            }
        }
    }
    if f.seals.len() > SEALS_MAX {
        return Err(bad("Too many seals."));
    }
    let (members, revoked) = match kind {
        Kind::Ak => {
            let devices =
                uuid_set(f.devices.as_deref().ok_or_else(|| bad("devices"))?, "The device list is not valid.")?;
            let revoked: Vec<Ref> = f.revoked.ok_or_else(|| bad("revoked"))?.into_iter().map(|id| Ref { id }).collect();
            let revoked = uuid_set(&revoked, "The revoked list is not valid.")?;
            if devices.is_empty() || devices.iter().any(|d| revoked.contains(d)) {
                return Err(bad("A revoked device is listed."));
            }
            (devices, revoked)
        }
        Kind::Pak => {
            if f.pp_kdf.is_none()
                || f.esc_pk.as_deref().and_then(crypto::b64url_decode::<32>).is_none()
                || f.rr_gen.is_none()
            {
                return Err(bad("The passwords directory is incomplete."));
            }
            (
                uuid_set(f.vault_keys.as_deref().ok_or_else(|| bad("vault_keys"))?, "The vault keys are not valid.")?,
                BTreeSet::new(),
            )
        }
    };
    for s in &f.seals {
        let ok = s.to == "escrow" || s.to.parse::<Uuid>().is_ok_and(|id| members.contains(&id));
        if !ok {
            return Err(bad("A seal is addressed to someone not in the directory."));
        }
    }
    Ok(Checked {
        hash: crypto::sha256(&bytes),
        bytes,
        mac,
        chain,
        escrow_mac,
        seq,
        epoch,
        members,
        revoked,
        pp_kdf: f.pp_kdf,
        esc_pk: f.esc_pk,
        rr_gen: f.rr_gen,
    })
}

/// The ids in the head `ak_dir`'s revoked list (a later version must keep them).
fn head_revoked(h: &Head) -> BTreeSet<Uuid> {
    serde_json::from_slice::<DirFields>(&h.bytes)
        .ok()
        .and_then(|f| f.revoked)
        .map(|r| r.iter().filter_map(|x| x.parse().ok()).collect())
        .unwrap_or_default()
}

fn head_pak_fields(h: &Head) -> (Option<String>, Option<u32>) {
    serde_json::from_slice::<DirFields>(&h.bytes).map(|f| (f.esc_pk, f.rr_gen)).unwrap_or((None, None))
}

/// Active devices of the account: linked (a token family exists) and not revoked.
async fn active_devices(conn: &mut PgConnection, account_id: Uuid) -> Result<BTreeSet<Uuid>, sqlx::Error> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(
        "SELECT d.id FROM devices d WHERE d.account_id = $1 AND d.revoked_at IS NULL
           AND EXISTS (SELECT 1 FROM refresh_families f WHERE f.device_id = d.id)",
    )
    .bind(account_id)
    .fetch_all(conn)
    .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

// ---- sync/state -----------------------------------------------------------------------------

#[derive(sqlx::FromRow)]
pub struct StateRow {
    pub keyset_gen: Uuid,
    pub current_epoch: i32,
    pub pak_epoch: i32,
    pub rr_gen: i32,
    pub rotation_ak: bool,
    pub rotation_pak: bool,
    pub head_seq: i64,
    pub records: i32,
    pub bytes: i64,
    pub history_records: i32,
}

pub async fn state_row(conn: &mut PgConnection, account_id: Uuid, lock: bool) -> Result<Option<StateRow>, sqlx::Error> {
    let sql = format!(
        "SELECT keyset_gen, current_epoch, pak_epoch, rr_gen, rotation_ak, rotation_pak, head_seq, records, bytes,
                history_records FROM sync_state WHERE account_id = $1{}",
        if lock { " FOR UPDATE" } else { "" }
    );
    sqlx::query_as(sqlx::AssertSqlSafe(sql)).bind(account_id).fetch_optional(conn).await
}

/// `GET sync/state` → what a device needs to decide its next step.
pub async fn sync_state(State(state): State<AppState>, d: DeviceAuth) -> ApiResult {
    hit(&state.db, rl::SYNC_STATE, &rl::id_key(&d.device_id)).await?;
    let mut conn = state.db.acquire().await?;
    let st = state_row(&mut conn, d.account_id, false).await?;
    let pending: i64 = sqlx::query_scalar(
        // Not the caller's own request (a `pak` approval comes from a device with keys).
        "SELECT count(*) FROM approvals WHERE account_id = $1 AND expires_at > now() AND step IN ('requested', 'confirmed')
           AND new_device_id <> $2",
    )
    .bind(d.account_id)
    .bind(d.device_id)
    .fetch_one(&mut *conn)
    .await?;
    let escrow_vault: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM escrow WHERE account_id = $1 AND rr_wrap_vault IS NOT NULL)")
            .bind(d.account_id)
            .fetch_one(&mut *conn)
            .await?;
    let remaining_old = crate::records::remaining_old(&mut conn, d.account_id).await?;
    let q = &state.config.quota;
    Ok(Json(json!({
        "keyset_gen": st.as_ref().map(|s| s.keyset_gen),
        "current_epoch": st.as_ref().map_or(0, |s| s.current_epoch),
        "pak_epoch": st.as_ref().map_or(0, |s| s.pak_epoch),
        "rr_gen": st.as_ref().map_or(0, |s| s.rr_gen),
        "rotation": {"ak": st.as_ref().is_some_and(|s| s.rotation_ak), "pak": st.as_ref().is_some_and(|s| s.rotation_pak)},
        "rekey": {"remaining_old": remaining_old},
        "head_seq": st.as_ref().map_or(0, |s| s.head_seq),
        "pending_approvals": pending,
        "escrow_vault": escrow_vault,
        "quota": {
            "records": st.as_ref().map_or(0, |s| s.records),
            "bytes": st.as_ref().map_or(0, |s| s.bytes),
            "history": st.as_ref().map_or(0, |s| s.history_records),
            "max_records": q.max_records,
            "max_bytes": q.max_bytes,
            "max_history": q.max_history,
        },
    }))
    .into_response())
}

// ---- keys/head, keys/chain, keys/escrow ------------------------------------------------------

/// `GET keys/head` → the head directories, or 404 `no_keyset`.
pub async fn keys_head(State(state): State<AppState>, d: DeviceAuth) -> ApiResult {
    hit(&state.db, rl::KEYS_HEAD, &rl::id_key(&d.device_id)).await?;
    let mut conn = state.db.acquire().await?;
    if state_row(&mut conn, d.account_id, false).await?.is_none() {
        return Err(no_keyset());
    }
    let mut out = serde_json::Map::new();
    for (kind, name) in [(Kind::Ak, "ak_dir"), (Kind::Pak, "pak_dir")] {
        let Some(h) = head(&mut conn, d.account_id, kind).await? else { return Err(no_keyset()) };
        let b = blobs(&mut conn, d.account_id, kind, h.seq).await?;
        out.insert(name.into(), serde_json::to_value(&b[0]).map_err(|_| ApiError::server_error())?);
    }
    Ok(Json(Value::Object(out)).into_response())
}

#[derive(Deserialize)]
pub struct ChainQuery {
    from: String,
}

/// `GET keys/chain?from=anchor|<ak_seq>,<pak_seq>` → the versions from the latest escrow anchor
/// (or after the given seqs) to the head.
pub async fn keys_chain(State(state): State<AppState>, d: DeviceAuth, Query(q): Query<ChainQuery>) -> ApiResult {
    hit(&state.db, rl::KEYS_CHAIN, &rl::id_key(&d.device_id)).await?;
    let mut conn = state.db.acquire().await?;
    if state_row(&mut conn, d.account_id, false).await?.is_none() {
        return Err(no_keyset());
    }
    let (ak_from, pak_from) = if q.from == "anchor" {
        let mut froms = [0i64; 2];
        for (i, kind) in [Kind::Ak, Kind::Pak].into_iter().enumerate() {
            let sql = format!(
                "SELECT coalesce(max(seq), 1) FROM {} WHERE account_id = $1 AND escrow_mac IS NOT NULL",
                kind.table()
            );
            froms[i] = sqlx::query_scalar(sqlx::AssertSqlSafe(sql)).bind(d.account_id).fetch_one(&mut *conn).await?;
        }
        (froms[0], froms[1])
    } else {
        let (a, p) = q.from.split_once(',').ok_or_else(|| bad("from"))?;
        let a: i64 = a.parse().map_err(|_| bad("from"))?;
        let p: i64 = p.parse().map_err(|_| bad("from"))?;
        (a + 1, p + 1)
    };
    let ak = blobs(&mut conn, d.account_id, Kind::Ak, ak_from).await?;
    let pak = blobs(&mut conn, d.account_id, Kind::Pak, pak_from).await?;
    Ok(Json(json!({"ak": ak, "pak": pak})).into_response())
}

#[derive(sqlx::FromRow)]
struct EscrowRow {
    rr_gen: i32,
    rr_wrap_pp: Value,
    rr_wrap_rk: Value,
    pp_kdf: Value,
    rr_wrap_vault: Option<Value>,
}

/// `GET keys/escrow` → the recovery root's wraps (5 per day per account). A fetch by a device
/// that holds no keys yet (a new device restoring) is mailed; devices that sync aren't (0.21.1).
pub async fn keys_escrow(State(state): State<AppState>, d: DeviceAuth) -> ApiResult {
    hit(&state.db, rl::KEYS_ESCROW, &rl::id_key(&d.account_id)).await?;
    let mut tx = state.db.begin().await?;
    let row: Option<EscrowRow> =
        sqlx::query_as("SELECT rr_gen, rr_wrap_pp, rr_wrap_rk, pp_kdf, rr_wrap_vault FROM escrow WHERE account_id = $1")
            .bind(d.account_id)
            .fetch_optional(&mut *tx)
            .await?;
    let row = row.ok_or_else(no_keyset)?;
    let (email, label, keyed): (String, String, bool) = sqlx::query_as(
        "SELECT a.email::text, d.label, d.has_keys FROM accounts a JOIN devices d ON d.account_id = a.id WHERE d.id = $1",
    )
    .bind(d.device_id)
    .fetch_one(&mut *tx)
    .await?;
    if !keyed {
        mail::enqueue(&mut tx, Some(d.account_id), &email, Template::EscrowFetched, json!({ "label": label })).await?;
    }
    tx.commit().await?;
    tracing::info!(device = %d.device_id, "escrow fetched");
    Ok(Json(json!({
        "account_id": crypto::b64url(d.account_id.as_bytes()),
        "rr_gen": row.rr_gen,
        "rr_wrap_pp": row.rr_wrap_pp,
        "rr_wrap_rk": row.rr_wrap_rk,
        "pp_kdf": row.pp_kdf,
        "rr_wrap_vault": row.rr_wrap_vault,
    }))
    .into_response())
}

// ---- keys/commit ----------------------------------------------------------------------------

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct RrWrapIn {
    pub n: String,
    pub ct: String,
    pub commit: String,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct EscrowIn {
    pub rr_gen: u32,
    pub rr_wrap_pp: RrWrapIn,
    pub rr_wrap_rk: RrWrapIn,
    pub pp_kdf: Value,
    /// The recovery root under the password-vault master password (0.21.1).
    #[serde(default)]
    pub rr_wrap_vault: Option<VaultWrapIn>,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct VaultKdfIn {
    pub alg: String,
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
    pub salt: String,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct VaultWrapIn {
    pub n: String,
    pub ct: String,
    pub commit: String,
    pub kdf: VaultKdfIn,
}

impl EscrowIn {
    fn valid(&self) -> bool {
        let w = |n: &str, ct: &str, commit: &str| {
            crypto::b64url_decode::<24>(n).is_some()
                && crypto::b64url_decode::<48>(ct).is_some()
                && crypto::b64url_decode::<32>(commit).is_some()
        };
        let vault_ok = self.rr_wrap_vault.as_ref().is_none_or(|v| {
            let k = &v.kdf;
            w(&v.n, &v.ct, &v.commit)
                && k.alg == "argon2id"
                && (131_072..=1_048_576).contains(&k.m_kib)
                && (3..=10).contains(&k.t)
                && (1..=8).contains(&k.p)
                && crypto::b64url_decode::<16>(&k.salt).is_some()
        });
        self.rr_gen >= 1
            && w(&self.rr_wrap_pp.n, &self.rr_wrap_pp.ct, &self.rr_wrap_pp.commit)
            && w(&self.rr_wrap_rk.n, &self.rr_wrap_rk.ct, &self.rr_wrap_rk.commit)
            && self.pp_kdf.to_string().len() <= 512
            && vault_ok
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommitReq {
    #[serde(default)]
    ak_dir: Option<BlobIn>,
    #[serde(default)]
    pak_dir: Option<BlobIn>,
    #[serde(default)]
    if_ak_prev_hash: Option<String>,
    #[serde(default)]
    if_pak_prev_hash: Option<String>,
    #[serde(default)]
    if_absent: bool,
    #[serde(default)]
    keyset_gen: Option<Uuid>,
    #[serde(default)]
    escrow: Option<EscrowIn>,
}

/// `POST keys/commit` → `{ak_seq, pak_seq}`; 409 `{head}` when the head moved (CAS).
pub async fn keys_commit(
    State(state): State<AppState>,
    d: DeviceAuth,
    JsonBody(req): JsonBody<CommitReq>,
) -> ApiResult {
    hit(&state.db, rl::KEYS_COMMIT, &rl::id_key(&d.device_id)).await?;
    hit(&state.db, rl::KEYS_COMMIT_ACCOUNT, &rl::id_key(&d.account_id)).await?;
    if req.ak_dir.is_none() && req.pak_dir.is_none() {
        return Err(bad("Nothing to commit."));
    }
    if req.escrow.as_ref().is_some_and(|e| !e.valid()) {
        return Err(bad("The escrow is not valid."));
    }
    let mut tx = state.db.begin().await?;
    // One commit at a time per account: the account row is the lock (sync_state may not exist).
    sqlx::query("SELECT 1 FROM accounts WHERE id = $1 FOR UPDATE").bind(d.account_id).execute(&mut *tx).await?;
    let st = state_row(&mut tx, d.account_id, true).await?;
    let ak_head = head(&mut tx, d.account_id, Kind::Ak).await?;
    let pak_head = head(&mut tx, d.account_id, Kind::Pak).await?;

    if req.if_absent {
        // First device (PROTOCOL §6.4): both directories at seq 1 and the escrow, in one go.
        if st.is_some() {
            return Ok(conflict_head(ak_head.as_ref(), pak_head.as_ref()));
        }
        if req.ak_dir.is_none() || req.pak_dir.is_none() || req.escrow.is_none() {
            return Err(bad("The first commit carries both directories and the escrow."));
        }
    } else {
        let Some(st) = st.as_ref() else { return Err(no_keyset()) };
        if req.keyset_gen.is_some_and(|g| g != st.keyset_gen) {
            return Ok(conflict_head(ak_head.as_ref(), pak_head.as_ref()));
        }
        // The compare-and-swap: the caller saw the current heads.
        let cas = |want: &Option<String>, h: &Option<Head>| match (want, h) {
            (Some(w), Some(h)) => crypto::b64url_decode::<32>(w).map(|x| x.to_vec()) == Some(h.hash.clone()),
            _ => false,
        };
        if (req.ak_dir.is_some() && !cas(&req.if_ak_prev_hash, &ak_head))
            || (req.pak_dir.is_some() && !cas(&req.if_pak_prev_hash, &pak_head))
        {
            return Ok(conflict_head(ak_head.as_ref(), pak_head.as_ref()));
        }
    }

    let active = active_devices(&mut tx, d.account_id).await?;
    if !active.contains(&d.device_id) {
        return Err(ApiError::unauthorized());
    }
    let ak = match &req.ak_dir {
        Some(b) => {
            let c = check_blob(Kind::Ak, b, d.account_id, d.device_id, ak_head.as_ref())?;
            if !c.members.is_subset(&active) {
                return Err(bad("A listed device is not an active device of this account."));
            }
            if let Some(h) = &ak_head {
                if !head_revoked(h).is_subset(&c.revoked) {
                    return Err(bad("A revoked device can't come back."));
                }
                if st.as_ref().is_some_and(|s| s.rotation_ak) && c.epoch == h.epoch {
                    return Err(rotation_pending());
                }
            }
            if req.if_absent && c.epoch != 1 {
                return Err(bad("The first epoch is 1."));
            }
            Some(c)
        }
        None => None,
    };
    let pak = match &req.pak_dir {
        Some(b) => {
            let c = check_blob(Kind::Pak, b, d.account_id, d.device_id, pak_head.as_ref())?;
            // Vault keys only of devices that are in the account directory being committed or
            // the current one.
            let listed = match (&ak, &ak_head) {
                (Some(a), _) => a.members.clone(),
                (None, Some(h)) => serde_json::from_slice::<DirFields>(&h.bytes)
                    .ok()
                    .and_then(|f| f.devices)
                    .map(|v| v.iter().filter_map(|r| r.id.parse().ok()).collect())
                    .unwrap_or_default(),
                (None, None) => BTreeSet::new(),
            };
            if !c.members.is_subset(&listed) || !c.members.is_subset(&active) {
                return Err(bad("A vault key belongs to a device not in the account directory."));
            }
            if let Some(h) = &pak_head {
                if st.as_ref().is_some_and(|s| s.rotation_pak) && c.epoch == h.epoch {
                    return Err(rotation_pending());
                }
                // `esc_pk` changes only with a new recovery root (rr_gen + 1).
                let (old_pk, old_gen) = head_pak_fields(h);
                let new_gen = c.rr_gen.unwrap_or(0);
                if new_gen < old_gen.unwrap_or(0) || (c.esc_pk != old_pk && new_gen <= old_gen.unwrap_or(0)) {
                    return Err(bad("The escrow key changes only with a new recovery root."));
                }
            }
            if req.if_absent && (c.epoch != 1 || c.rr_gen != Some(1)) {
                return Err(bad("The first epoch is 1."));
            }
            Some(c)
        }
        None => None,
    };
    // The escrow travels with the passwords directory and matches it.
    let escrow_kdf: Option<Value> = match &req.escrow {
        Some(e) => {
            let Some(p) = &pak else { return Err(bad("The escrow travels with the passwords directory.")) };
            if p.rr_gen != Some(e.rr_gen) {
                return Err(bad("The escrow's generation doesn't match the directory."));
            }
            Some(e.pp_kdf.clone())
        }
        None => {
            sqlx::query_scalar("SELECT pp_kdf FROM escrow WHERE account_id = $1")
                .bind(d.account_id)
                .fetch_optional(&mut *tx)
                .await?
        }
    };
    if let Some(p) = &pak {
        if p.pp_kdf != escrow_kdf {
            return Err(bad("The passphrase parameters don't match the escrow's."));
        }
        if req.escrow.is_none() && p.rr_gen != head_pak_fields(pak_head.as_ref().expect("checked")).1 {
            return Err(bad("A new recovery root comes with its escrow."));
        }
    }

    // Write.
    for (kind, c) in [(Kind::Ak, &ak), (Kind::Pak, &pak)] {
        let Some(c) = c else { continue };
        let sql = format!(
            "INSERT INTO {} (account_id, seq, epoch, bytes, hash, mac, chain, escrow_mac, created_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            kind.table()
        );
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(d.account_id)
            .bind(c.seq)
            .bind(c.epoch)
            .bind(&c.bytes)
            .bind(c.hash.as_slice())
            .bind(c.mac.as_slice())
            .bind(c.chain.map(|x| x.to_vec()))
            .bind(c.escrow_mac.map(|x| x.to_vec()))
            .bind(d.device_id)
            .execute(&mut *tx)
            .await?;
    }
    if req.if_absent {
        let (a, p) = (ak.as_ref().expect("checked"), pak.as_ref().expect("checked"));
        sqlx::query(
            "INSERT INTO sync_state (account_id, keyset_gen, current_epoch, pak_epoch, rr_gen) VALUES ($1, $2, $3, $4, 1)",
        )
        .bind(d.account_id)
        .bind(Uuid::new_v4())
        .bind(a.epoch)
        .bind(p.epoch)
        .execute(&mut *tx)
        .await?;
    } else {
        let st = st.as_ref().expect("checked");
        if let Some(a) = &ak {
            if a.epoch > st.current_epoch {
                sqlx::query(
                    "UPDATE sync_state SET current_epoch = $2, rotation_ak = false, lease_ak_device = NULL,
                            lease_ak_until = NULL, updated_at = now() WHERE account_id = $1",
                )
                .bind(d.account_id)
                .bind(a.epoch)
                .execute(&mut *tx)
                .await?;
            }
        }
        if let Some(p) = &pak {
            sqlx::query(
                "UPDATE sync_state SET pak_epoch = $2, rr_gen = $3,
                        rotation_pak = CASE WHEN $2 > pak_epoch THEN false ELSE rotation_pak END,
                        lease_pak_device = CASE WHEN $2 > pak_epoch THEN NULL ELSE lease_pak_device END,
                        lease_pak_until = CASE WHEN $2 > pak_epoch THEN NULL ELSE lease_pak_until END,
                        updated_at = now() WHERE account_id = $1",
            )
            .bind(d.account_id)
            .bind(p.epoch)
            .bind(p.rr_gen.unwrap_or(1) as i32)
            .execute(&mut *tx)
            .await?;
        }
    }
    if let Some(e) = &req.escrow {
        sqlx::query(
            "INSERT INTO escrow (account_id, rr_gen, rr_wrap_pp, rr_wrap_rk, pp_kdf, rr_wrap_vault)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (account_id) DO UPDATE SET rr_gen = excluded.rr_gen, rr_wrap_pp = excluded.rr_wrap_pp,
                 rr_wrap_rk = excluded.rr_wrap_rk, pp_kdf = excluded.pp_kdf,
                 rr_wrap_vault = excluded.rr_wrap_vault, updated_at = now()",
        )
        .bind(d.account_id)
        .bind(e.rr_gen as i32)
        .bind(serde_json::to_value(&e.rr_wrap_pp).map_err(|_| ApiError::server_error())?)
        .bind(serde_json::to_value(&e.rr_wrap_rk).map_err(|_| ApiError::server_error())?)
        .bind(&e.pp_kdf)
        .bind(match &e.rr_wrap_vault {
            Some(v) => Some(serde_json::to_value(v).map_err(|_| ApiError::server_error())?),
            None => None,
        })
        .execute(&mut *tx)
        .await?;
    }
    if let Some(a) = &ak {
        let ids: Vec<Uuid> = a.members.iter().copied().collect();
        sqlx::query(
            "UPDATE devices SET has_keys = (id = ANY($2)), had_keys = had_keys OR (id = ANY($2)) WHERE account_id = $1",
        )
        .bind(d.account_id)
        .bind(&ids)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    let seq = |c: &Option<Checked>, h: &Option<Head>| c.as_ref().map(|c| c.seq).or(h.as_ref().map(|h| h.seq));
    tracing::info!(
        ak = ?ak.as_ref().map(|c| (c.seq, c.epoch)),
        pak = ?pak.as_ref().map(|c| (c.seq, c.epoch)),
        "keys committed"
    );
    Ok(Json(json!({"ak_seq": seq(&ak, &ak_head), "pak_seq": seq(&pak, &pak_head)})).into_response())
}

// ---- keys/rotation/claim --------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimReq {
    kind: String,
}

/// `POST keys/rotation/claim {kind}` → `{lease_until}` (advisory, 10 min); 409 if another
/// device holds it.
pub async fn rotation_claim(
    State(state): State<AppState>,
    d: DeviceAuth,
    JsonBody(req): JsonBody<ClaimReq>,
) -> ApiResult {
    hit(&state.db, rl::ROTATION_CLAIM, &rl::id_key(&d.device_id)).await?;
    let (dev_col, until_col) = match req.kind.as_str() {
        "ak" => ("lease_ak_device", "lease_ak_until"),
        "pak" => ("lease_pak_device", "lease_pak_until"),
        _ => return Err(bad("kind")),
    };
    let sql = format!(
        "UPDATE sync_state SET {dev_col} = $2, {until_col} = now() + make_interval(secs => $3), updated_at = now()
         WHERE account_id = $1 AND ({dev_col} IS NULL OR {dev_col} = $2 OR {until_col} < now())
         RETURNING {until_col}"
    );
    let until: Option<DateTime<Utc>> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(d.account_id)
        .bind(d.device_id)
        .bind(LEASE_S as f64)
        .fetch_optional(&state.db)
        .await?;
    match until {
        Some(u) => Ok(Json(json!({"lease_until": u})).into_response()),
        None => {
            let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM sync_state WHERE account_id = $1)")
                .bind(d.account_id)
                .fetch_one(&state.db)
                .await?;
            if exists {
                Err(ApiError::new(StatusCode::CONFLICT, "conflict", "Another device is rotating the keys."))
            } else {
                Err(no_keyset())
            }
        }
    }
}

// ---- Devices and sessions from a device (PROTOCOL §11.8) ------------------------------------

/// `GET devices` → the account's devices, as on the website, with `self` for the caller.
pub async fn device_devices(State(state): State<AppState>, d: DeviceAuth) -> ApiResult {
    hit(&state.db, rl::DEVICES_LIST_DEVICE, &rl::id_key(&d.device_id)).await?;
    let mut items = devices::list_devices(&state.db, d.account_id).await?;
    for it in items.iter_mut() {
        it.self_ = Some(it.id == d.device_id);
    }
    Ok(Json(items).into_response())
}

/// `POST devices/{id}/revoke` → 200: the device is removed and the keys must be rotated (§7);
/// the caller is expected to rotate at once.
pub async fn device_revoke(
    State(state): State<AppState>,
    client: Client,
    d: DeviceAuth,
    Path(id): Path<String>,
    JsonBody(_): JsonBody<devices::Empty>,
) -> ApiResult {
    hit(&state.db, rl::DEVICE_REVOKE, &rl::id_key(&d.account_id)).await?;
    let id: Uuid = id.parse().map_err(|_| devices::device_not_found())?;
    if id == d.device_id {
        return Err(bad("Use device/unlink to remove this device."));
    }
    devices::revoke_from(&state, d.account_id, id, &client, "a device", Source::Device(d.device_id)).await?;
    Ok(Json(json!({ "status": "revoked" })).into_response())
}

/// `GET sessions` → website sessions and devices of the account (`current` is never set: a
/// device is not a website session).
pub async fn device_sessions(State(state): State<AppState>, d: DeviceAuth) -> ApiResult {
    hit(&state.db, rl::SESSIONS_LIST_DEVICE, &rl::id_key(&d.device_id)).await?;
    Ok(Json(devices::list_sessions(&state.db, d.account_id, None).await?).into_response())
}

/// `POST sessions/{id}/revoke` → 204: a website session ends, or a device is removed.
pub async fn device_session_revoke(
    State(state): State<AppState>,
    client: Client,
    d: DeviceAuth,
    Path(id): Path<String>,
    JsonBody(_): JsonBody<devices::Empty>,
) -> ApiResult {
    hit(&state.db, rl::SESSION_REVOKE, &rl::id_key(&d.account_id)).await?;
    let uid: Uuid = id.parse().map_err(|_| devices::device_not_found())?;
    let n = sqlx::query(
        "UPDATE web_sessions SET revoked_at = now() WHERE id = $1 AND account_id = $2 AND revoked_at IS NULL",
    )
    .bind(uid)
    .bind(d.account_id)
    .execute(&state.db)
    .await?
    .rows_affected();
    if n == 0 {
        if uid == d.device_id {
            return Err(bad("Use device/unlink to remove this device."));
        }
        devices::revoke_from(&state, d.account_id, uid, &client, "a device", Source::Device(d.device_id)).await?;
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(account: Uuid, caller: Uuid, seq: u64, prev: &str, extra: Value) -> BlobIn {
        let mut v = json!({
            "v": 1, "account_id": crypto::b64url(account.as_bytes()), "epoch": 1, "seq": seq, "prev_hash": prev,
            "devices": [{"id": caller.to_string(), "short": 0, "pk": "x", "added_by": caller.to_string(), "added_seq": 1}],
            "revoked": [], "min_record_epoch": 1,
            "seals": [{"to": caller.to_string(), "enc": "e", "ct": "c"}, {"to": "escrow", "enc": "e", "ct": "c"}],
            "created_ms": 1, "created_by": caller.to_string(),
        });
        for (k, x) in extra.as_object().unwrap() {
            v[k] = x.clone();
        }
        BlobIn {
            bytes: crypto::b64url(&serde_json::to_vec(&v).unwrap()),
            mac: crypto::b64url(&[1; 32]),
            chain: None,
            escrow_mac: None,
        }
    }

    #[test]
    fn the_structure_of_a_first_directory() {
        let (acc, me) = (Uuid::new_v4(), Uuid::new_v4());
        let c = check_blob(Kind::Ak, &dir(acc, me, 1, "", json!({})), acc, me, None).unwrap();
        assert_eq!((c.seq, c.epoch), (1, 1));
        assert!(c.members.contains(&me));
        // Another account, another author, not seq 1, a seal to a stranger, a revoked member.
        let other = Uuid::new_v4();
        assert!(check_blob(Kind::Ak, &dir(other, me, 1, "", json!({})), acc, me, None).is_err());
        assert!(check_blob(Kind::Ak, &dir(acc, me, 1, "", json!({})), acc, other, None).is_err());
        assert!(check_blob(Kind::Ak, &dir(acc, me, 2, "", json!({})), acc, me, None).is_err());
        let stranger = json!({"seals": [{"to": other.to_string(), "enc": "e", "ct": "c"}]});
        assert!(check_blob(Kind::Ak, &dir(acc, me, 1, "", stranger), acc, me, None).is_err());
        let revoked = json!({"revoked": [me.to_string()]});
        assert!(check_blob(Kind::Ak, &dir(acc, me, 1, "", revoked), acc, me, None).is_err());
        // A passwords directory needs its own fields.
        assert!(check_blob(Kind::Pak, &dir(acc, me, 1, "", json!({})), acc, me, None).is_err());
    }

    #[test]
    fn successors_follow_the_head_one_epoch_at_a_time() {
        let (acc, me) = (Uuid::new_v4(), Uuid::new_v4());
        let first = dir(acc, me, 1, "", json!({}));
        let bytes = <base64ct::Base64UrlUnpadded as base64ct::Encoding>::decode_vec(&first.bytes).unwrap();
        let h = Head { seq: 1, epoch: 1, hash: crypto::sha256(&bytes).to_vec(), bytes };
        let prev = crypto::b64url(&h.hash);
        assert!(check_blob(Kind::Ak, &dir(acc, me, 2, &prev, json!({})), acc, me, Some(&h)).is_ok());
        assert!(check_blob(Kind::Ak, &dir(acc, me, 3, &prev, json!({})), acc, me, Some(&h)).is_err(), "a gap");
        let wrong = crypto::b64url(&[0; 32]);
        assert!(check_blob(Kind::Ak, &dir(acc, me, 2, &wrong, json!({})), acc, me, Some(&h)).is_err(), "a fork");
        // Epoch + 1 needs a chain tag; + 2 is refused; a chain without a new epoch too.
        let mut e2 = dir(acc, me, 2, &prev, json!({"epoch": 2}));
        assert!(check_blob(Kind::Ak, &e2, acc, me, Some(&h)).is_err());
        e2.chain = Some(crypto::b64url(&[2; 32]));
        assert_eq!(check_blob(Kind::Ak, &e2, acc, me, Some(&h)).unwrap().epoch, 2);
        let mut e3 = dir(acc, me, 2, &prev, json!({"epoch": 3}));
        e3.chain = Some(crypto::b64url(&[2; 32]));
        assert!(check_blob(Kind::Ak, &e3, acc, me, Some(&h)).is_err());
        let mut same = dir(acc, me, 2, &prev, json!({}));
        same.chain = Some(crypto::b64url(&[2; 32]));
        assert!(check_blob(Kind::Ak, &same, acc, me, Some(&h)).is_err());
    }
}
