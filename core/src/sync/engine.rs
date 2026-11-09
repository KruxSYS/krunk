//! One sync run (PROTOCOL §10): check the server's state, pull and merge, repair the bookmark
//! tree, push the queued local changes. Platform-free; the app runs it on a worker thread with
//! its own connection to `browser.db`, the Linux test client in-process.
//!
//! Every pulled record is decrypted under its epoch's collection key, its id is checked against
//! its natural key (a swapped record is an alarm), and a version older than the one already
//! applied is ignored (a rollback). A record that fails authentication at a known epoch raises
//! the fork alarm: the run stops before anything is written back.

use super::api::ApiError;
use super::client::{self, Api, SResult, SyncFail};
use super::coll::{self, Changed, Ctx};
use super::dir::DirError;
use super::envelope::{self, unb64, Envelope, KeyKind, RecordPlain};
use super::hlc::Hlc;
use super::keys::KeyError;
use super::local::{self, SyncItem};
use super::rotate;
use super::state::SyncState;
use super::AccountId;
use crate::storage::Store;
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;

const PAGE: usize = 500;
const BATCH_BYTES: usize = 1024 * 1024;

pub struct RunOpts<'a> {
    pub device_name: &'a str,
    /// Collections switched off on this device.
    pub off: &'a [String],
    pub max_rounds: usize,
    /// The user confirmed the devices that keep access (or removed the device here): a rotation
    /// may go ahead even if one of them was added by the removed device [R3].
    pub confirm_rotation: bool,
}

impl Default for RunOpts<'_> {
    fn default() -> Self {
        RunOpts { device_name: "Krunk", off: &[], max_rounds: 20, confirm_rotation: false }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub pulled: usize,
    pub pushed: usize,
    pub conflicts: usize,
    pub changed: Changed,
    /// A 410 made this a full reconcile.
    pub full: bool,
    /// Passwords records left for an unlocked vault.
    pub pak_waiting: usize,
    /// Writes are frozen until the keys are rotated (423).
    pub frozen: bool,
    pub queue_left: i64,
    /// This run rotated the account key to this epoch (S9).
    pub rotated: Option<u32>,
    /// A rotation waits for the user to confirm these devices (added by a removed one).
    pub confirm: Vec<String>,
}

#[derive(Deserialize)]
struct PullWire {
    records: Vec<PulledRecord>,
    has_more: bool,
}

#[derive(Deserialize)]
struct PulledRecord {
    seq: i64,
    envelope: Envelope,
}

/// A record that fails authentication or carries another record's id: the fork alarm.
fn alarm() -> SyncFail {
    SyncFail::Keys(KeyError::Dir(DirError::BadTag))
}

/// The key of `epoch` for AK collections (the head's, or an older one still kept).
pub(crate) fn ak_for(st: &SyncState, epoch: u32) -> Option<[u8; 32]> {
    let pin = st.ak.as_ref()?;
    if epoch == pin.epoch {
        return Some(*pin.ak);
    }
    st.ak_old.iter().find(|k| k.epoch == epoch).map(|k| *k.key)
}

/// Runs one sync round; `st` is updated (cursor, HLC, keys after a refresh) for the caller to save.
pub fn run(api: &Api, store: &mut Store, st: &mut SyncState, opts: &RunOpts) -> SResult<Report> {
    let mut report = Report::default();
    let server = client::state(api)?;
    match (&server.keyset_gen, st.keyset_gen.is_empty()) {
        (Some(g), true) => st.keyset_gen = g.clone(),
        (Some(g), false) if *g != st.keyset_gen => return Err(SyncFail::Reset),
        (None, _) => return Err(SyncFail::Reset),
        _ => {}
    }
    if st.ak.as_ref().is_some_and(|a| server.current_epoch > a.epoch)
        && client::refresh(api, st, None)? == client::Refresh::SelfRevoked
    {
        return Err(SyncFail::Api(ApiError::Unauthorized));
    }
    // A removal waits for its rotation: this device does it (PROTOCOL §7), or finishes a rekey
    // another rotator left. Until both are done, local changes stay queued.
    if server.rotation.ak {
        match rotate::rotate_ak(api, st, store.sync_conn(), opts.confirm_rotation)? {
            rotate::AkRotation::Rotated(e) => report.rotated = Some(e),
            rotate::AkRotation::Confirm(ids) => report.confirm = ids,
            _ => {}
        }
    } else if st.ak.as_ref().is_some_and(|a| a.min_record_epoch < a.epoch) {
        rotate::rekey_ak(api, st, store.sync_conn(), &[])?;
        rotate::finish_ak(api, st)?;
    }
    if let Some(min) = st.ak.as_ref().map(|a| a.min_record_epoch) {
        st.ak_old.retain(|k| k.epoch >= min);
    }
    let writes_frozen = (server.rotation.ak && report.rotated.is_none())
        || st.ak.as_ref().is_some_and(|a| a.min_record_epoch < a.epoch);
    let acc = st.account_bytes().map_err(|_| SyncFail::BadAnswer("account id"))?;
    let mut hlc = Hlc::new(st.hlc);
    let device_id = st.device_id.clone();
    let initial = local::meta_get(store.sync_conn(), "initial")?.is_none();
    let ctx = Ctx {
        device_id: &device_id,
        dev_short: st.dev_short,
        now_ms: api.now_ms,
        device_name: opts.device_name,
        initial,
    };

    // Bookmarks changed here get their sort keys before the pull, so remote order merges in.
    prepare_bookmarks(store.sync_conn(), api.now_ms)?;
    pull(api, store, st, &acc, &mut hlc, &ctx, opts, &mut report)?;
    if report.changed.bookmarks {
        store.sync_applying(coll::repair_tree)?;
    }
    if ctx.initial || report.full || local::meta_get(store.sync_conn(), "reconcile")?.is_some() {
        coll::enqueue_all(store.sync_conn(), &ctx)?;
        local::meta_del(store.sync_conn(), "reconcile")?;
    }
    if writes_frozen {
        report.frozen = true;
    } else {
        push(api, store, st, &acc, &mut hlc, &ctx, opts, &mut report)?;
    }
    if ctx.initial && !report.frozen {
        local::meta_set(store.sync_conn(), "initial", "1")?;
    }
    st.hlc = hlc.last();
    report.queue_left = local::queue_len(store.sync_conn())?;
    Ok(report)
}

fn prepare_bookmarks(conn: &Connection, now_ms: i64) -> SResult<()> {
    let queued: Vec<String> =
        local::queue(conn, usize::MAX >> 1)?.into_iter().filter(|(c, _)| c == "bookmarks").map(|(_, k)| k).collect();
    let mut folders = HashSet::new();
    for g in queued {
        let raw =
            (0..g.len()).step_by(2).filter_map(|i| u8::from_str_radix(g.get(i..i + 2)?, 16).ok()).collect::<Vec<u8>>();
        let parent: Option<Option<i64>> =
            conn.query_row("SELECT parent FROM bookmarks WHERE guid = ?1", [raw], |r| r.get(0)).ok();
        if let Some(Some(p)) = parent {
            folders.insert(p);
        }
    }
    for f in folders {
        coll::prepare_folder(conn, f, now_ms)?;
    }
    Ok(())
}

/// Decrypts and checks one pulled record; `None` for what this run doesn't handle.
#[allow(clippy::too_many_arguments)]
fn open_one(
    conn: &Connection,
    st: &SyncState,
    acc: &AccountId,
    env: &Envelope,
    opts: &RunOpts,
    report: &mut Report,
) -> SResult<Option<(String, RecordPlain, Vec<u8>)>> {
    if env.k == KeyKind::Pak {
        report.pak_waiting += 1;
        return Ok(None);
    }
    if opts.off.contains(&env.c) || !coll::AK.contains(&env.c.as_str()) {
        return Ok(None);
    }
    let min = st.ak.as_ref().map_or(0, |a| a.min_record_epoch);
    if env.e < min {
        return Ok(None);
    }
    let Some(key) = ak_for(st, env.e) else {
        if st.ak.as_ref().is_some_and(|a| env.e < a.epoch) {
            // An epoch whose rekey is finished: never opened again.
            return Ok(None);
        }
        // A newer epoch than ours: the keys must be refreshed first (the next run does).
        return Err(SyncFail::Keys(KeyError::Dir(DirError::Gap)));
    };
    let keys = envelope::collection_keys(&key, acc, &env.c, env.e);
    let plain = envelope::open_record(&keys.ck, acc, env).map_err(|_| alarm())?;
    let rid = unb64(&env.id)?;
    let nk = if plain.deleted {
        match plain.data.get("nk").and_then(Value::as_str) {
            Some(nk) => {
                if envelope::record_id(&keys.ik, nk.as_bytes()).as_slice() != rid.as_slice() {
                    return Err(alarm());
                }
                nk.to_string()
            }
            None => match local::item_by_record(conn, &env.c, &rid)? {
                Some((nk, _)) => nk,
                None => return Ok(None),
            },
        }
    } else {
        let nk = coll::natural_key(&env.c, &plain.data).ok_or(SyncFail::BadAnswer("record data"))?;
        if envelope::record_id(&keys.ik, nk.as_bytes()).as_slice() != rid.as_slice() {
            return Err(alarm());
        }
        nk
    };
    Ok(Some((nk, plain, rid)))
}

/// Merges one record (pulled, or the current version of a conflict).
#[allow(clippy::too_many_arguments)]
fn merge_one(
    conn: &Connection,
    c: &str,
    nk: &str,
    plain: &RecordPlain,
    rid: Vec<u8>,
    seq: i64,
    bucket: i64,
    hlc: &mut Hlc,
    ctx: &Ctx,
    report: &mut Report,
) -> SResult<()> {
    let item = local::item(conn, c, nk)?;
    if let Some(it) = &item {
        if it.server_seq > 0 && plain.ver < it.ver {
            // An older version than the one applied (a replay or rollback): content ignored.
            let mut it = it.clone();
            it.server_seq = it.server_seq.max(seq);
            local::put_item(conn, c, nk, &it)?;
            return Ok(());
        }
    }
    hlc.observe(plain.ver, ctx.now_ms.max(0) as u64);
    let mut extra = coll::apply(conn, c, nk, plain, item.as_ref(), ctx, &mut report.changed)?;
    if extra.is_object() {
        extra["hash"] =
            if plain.deleted { Value::Null } else { Value::String(coll::data_hash(&coll::comparable(c, &plain.data))) };
    }
    let origin = item.as_ref().is_none_or(|i| i.origin_sync);
    local::put_item(
        conn,
        c,
        nk,
        &SyncItem {
            record_id: rid,
            server_seq: seq,
            ver: plain.ver,
            bucket,
            deleted: plain.deleted,
            origin_sync: origin,
            extra,
        },
    )?;
    // A local change that lost (or equals the remote now) needs no push.
    if !plain.deleted {
        if let Some(now) = coll::local_data(conn, c, nk, local::item(conn, c, nk)?.as_ref(), ctx)? {
            if coll::data_hash(&coll::comparable(c, &now)) == coll::data_hash(&coll::comparable(c, &plain.data)) {
                local::dequeue(conn, c, nk)?;
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn pull(
    api: &Api,
    store: &mut Store,
    st: &mut SyncState,
    acc: &AccountId,
    hlc: &mut Hlc,
    ctx: &Ctx,
    opts: &RunOpts,
    report: &mut Report,
) -> SResult<()> {
    let mut since = st.last_pull_seq as i64;
    let mut restarted = false;
    loop {
        let page: PullWire = match api.get(&format!("records?since={since}&limit={PAGE}")) {
            Ok(p) => p,
            Err(SyncFail::Api(ApiError::Gone)) if !restarted => {
                // Below the purge horizon: everything again, merged; nothing local is deleted
                // for being absent (PROTOCOL §10.4).
                report.full = true;
                restarted = true;
                since = 0;
                continue;
            }
            Err(e) => return Err(e),
        };
        let mut last = since;
        let mut opened = Vec::with_capacity(page.records.len());
        for r in &page.records {
            last = last.max(r.seq);
            if let Some(o) = open_one(store.sync_conn(), st, acc, &r.envelope, opts, report)? {
                let bucket = unb64(&r.envelope.ct).map(|ct| ct.len().saturating_sub(16) as i64).unwrap_or(0);
                opened.push((r.envelope.c.clone(), o, r.seq, bucket));
            }
        }
        store.sync_applying(|conn| {
            for (c, (nk, plain, rid), seq, bucket) in opened {
                merge_one(conn, &c, &nk, &plain, rid, seq, bucket, hlc, ctx, report).map_err(|e| match e {
                    SyncFail::Local(m) => rusqlite::Error::InvalidParameterName(m),
                    other => rusqlite::Error::InvalidParameterName(other.to_string()),
                })?;
                report.pulled += 1;
            }
            Ok(())
        })?;
        since = last;
        st.last_pull_seq = since.max(0) as u64;
        if !page.has_more {
            return Ok(());
        }
    }
}

struct Pending {
    c: String,
    nk: String,
    data: Option<Value>,
    plain: RecordPlain,
    bucket: i64,
    item: Option<SyncItem>,
}

#[derive(Deserialize)]
struct BatchWire {
    results: Vec<Value>,
    #[serde(default)]
    head_seq: i64,
}

#[allow(clippy::too_many_arguments)]
fn push(
    api: &Api,
    store: &mut Store,
    st: &mut SyncState,
    acc: &AccountId,
    hlc: &mut Hlc,
    ctx: &Ctx,
    opts: &RunOpts,
    report: &mut Report,
) -> SResult<()> {
    let pin = st.ak.clone().ok_or(SyncFail::BadAnswer("no keys"))?;
    for _round in 0..opts.max_rounds {
        let conn = store.sync_conn();
        let entries = local::queue(conn, PAGE)?;
        if entries.is_empty() {
            return Ok(());
        }
        let mut writes: Vec<Value> = Vec::new();
        let mut pending: Vec<Pending> = Vec::new();
        let mut done_raw: Vec<(String, String)> = Vec::new();
        let mut seen: HashSet<(String, String)> = HashSet::new();
        let mut bytes = 0usize;
        for (c, raw) in entries {
            if c == "passwords" || c == "never_save" {
                continue; // the PAK collections push with an unlocked vault (S8)
            }
            if opts.off.contains(&c) || !coll::AK.contains(&c.as_str()) {
                local::dequeue(conn, &c, &raw)?;
                continue;
            }
            if writes.len() >= PAGE || bytes >= BATCH_BYTES {
                break;
            }
            for nk in coll::expand(&c, &raw, ctx) {
                if !seen.insert((c.clone(), nk.clone())) {
                    continue;
                }
                let item = local::item(conn, &c, &nk)?;
                let data = coll::local_data(conn, &c, &nk, item.as_ref(), ctx)?;
                let (deleted, d, bucket) = match (&data, &item) {
                    (None, None) => continue,
                    (None, Some(it)) if it.deleted || it.server_seq == 0 => continue,
                    // A tombstone names its natural key (inside the ciphertext), so a rotator can
                    // re-encrypt it under the next epoch's record id.
                    (None, Some(it)) => (true, json!({"nk": nk}), Some(it.bucket as usize)),
                    (Some(d), Some(it))
                        if !it.deleted
                            && it.server_seq > 0
                            && it.extra.get("hash").and_then(Value::as_str)
                                == Some(coll::data_hash(&coll::comparable(&c, d)).as_str()) =>
                    {
                        continue
                    }
                    (Some(d), _) => (false, d.clone(), None),
                };
                let plain =
                    RecordPlain { ver: hlc.tick(ctx.now_ms.max(0) as u64), dev: st.dev_short, deleted, data: d };
                let keys = envelope::collection_keys(&pin.ak, acc, &c, pin.epoch);
                let id = envelope::record_id(&keys.ik, nk.as_bytes());
                let env = envelope::seal_record(&keys.ck, acc, &c, &id, pin.epoch, &plain, bucket.filter(|b| *b > 0))?;
                let pad = unb64(&env.ct).map(|x| x.len().saturating_sub(16) as i64).unwrap_or(0);
                bytes += env.ct.len();
                writes.push(
                    json!({"c": c, "id": env.id, "if_seq": item.as_ref().map_or(0, |i| i.server_seq), "envelope": env}),
                );
                pending.push(Pending { c: c.clone(), nk, data, plain, bucket: pad, item });
            }
            done_raw.push((c, raw));
        }
        if writes.is_empty() {
            for (c, raw) in &done_raw {
                local::dequeue(conn, c, raw)?;
            }
            if done_raw.is_empty() {
                return Ok(());
            }
            continue;
        }
        let resp: BatchWire = match api.post("records/batch", &json!({"writes": writes})) {
            Ok(r) => r,
            Err(SyncFail::Api(ApiError::RotationPending)) => {
                report.frozen = true;
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        let mut retry: HashSet<String> = HashSet::new();
        let mut own: Vec<i64> = Vec::new();
        for (p, res) in pending.into_iter().zip(resp.results) {
            if res["status"] == "ok" {
                let seq = res["seq"].as_i64().unwrap_or(0);
                if seq > 0 {
                    own.push(seq);
                }
                let mut extra =
                    p.item.as_ref().map(|i| i.extra.clone()).filter(Value::is_object).unwrap_or_else(|| json!({}));
                extra["data"] = p.data.clone().unwrap_or(Value::Null);
                extra["hash"] = p
                    .data
                    .as_ref()
                    .map(|d| Value::String(coll::data_hash(&coll::comparable(&p.c, d))))
                    .unwrap_or(Value::Null);
                let keys = envelope::collection_keys(&pin.ak, acc, &p.c, pin.epoch);
                let rid = envelope::record_id(&keys.ik, p.nk.as_bytes()).to_vec();
                local::put_item(
                    store.sync_conn(),
                    &p.c,
                    &p.nk,
                    &SyncItem {
                        record_id: rid,
                        server_seq: seq,
                        ver: p.plain.ver,
                        bucket: p.bucket,
                        deleted: p.plain.deleted,
                        origin_sync: p.item.as_ref().is_some_and(|i| i.origin_sync),
                        extra,
                    },
                )?;
                report.pushed += 1;
            } else {
                report.conflicts += 1;
                retry.insert(p.c.clone() + "\u{0}" + &p.nk);
                if let Some(cur) = res.get("current").filter(|c| !c.is_null()) {
                    let env: Envelope =
                        serde_json::from_value(cur["envelope"].clone()).map_err(|_| SyncFail::BadAnswer("conflict"))?;
                    let seq = cur["seq"].as_i64().unwrap_or(0);
                    if let Some((nk, plain, rid)) = open_one(store.sync_conn(), st, acc, &env, opts, report)? {
                        let bucket = unb64(&env.ct).map(|ct| ct.len().saturating_sub(16) as i64).unwrap_or(0);
                        store.sync_applying(|conn| {
                            merge_one(conn, &p.c, &nk, &plain, rid, seq, bucket, hlc, ctx, report)
                                .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))
                        })?;
                    }
                }
            }
        }
        // Only this batch's own writes since the last pull: no need to pull them back.
        own.sort_unstable();
        let from = st.last_pull_seq as i64;
        if !own.is_empty()
            && resp.head_seq == from + own.len() as i64
            && own.iter().enumerate().all(|(i, s)| *s == from + 1 + i as i64)
        {
            st.last_pull_seq = resp.head_seq as u64;
        }
        let conn = store.sync_conn();
        for (c, raw) in &done_raw {
            let keep = coll::expand(c, raw, ctx).iter().any(|nk| retry.contains(&(c.clone() + "\u{0}" + nk)));
            if !keep {
                local::dequeue(conn, c, raw)?;
            }
        }
    }
    Ok(())
}
