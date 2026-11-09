//! Revocation and key rotation (S9, PROTOCOL §7).
//!
//! When a device that held keys is removed, the server freezes record writes and flags both key
//! kinds for rotation. The first remaining device that runs:
//! 1. claims the advisory lease, builds the next `ak_dir` (epoch + 1, the removed devices out and
//!    listed as revoked, the new AK sealed to the remaining devices and to the pinned escrow key,
//!    the old AK in `hist`, `min_record_epoch` = the old epoch), and commits it by CAS;
//! 2. re-encrypts every record of the old epoch under the new one (`rekey/batch`), turning the
//!    removed devices' open tabs and names into tombstones;
//! 3. commits `ak_dir` once more with `min_record_epoch` = the new epoch and no `hist`.
//!
//! Steps 2–3 resume on any device of the new epoch. PAK rotates the same way, by a device whose
//! vault is unlocked ([`rotate_pak`]); its rekey also resumes ([`rekey_pak`]). A rotator that
//! loses the CAS takes the winner's directory through the normal refresh.

use super::api::ApiError;
use super::client::{self, Api, SResult, SyncFail};
use super::coll;
use super::crypto::{self, SealKind};
use super::dir::{self, AkDir, AkHist, DirError, PakHist};
use super::engine::ak_for;
use super::envelope::{self, unb64, Envelope, RecordPlain};
use super::hlc::Hlc;
use super::keys::{KeyError, PakPin, PakState};
use super::local;
use super::state::{AkPin, EpochKey, SyncState};
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{json, Value};
use zeroize::Zeroizing;

const PAGE: usize = 500;
const BATCH_BYTES: usize = 1024 * 1024;

/// What an AK rotation attempt came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AkRotation {
    /// No rotation is pending.
    Nothing,
    /// Another device holds the lease (it rotates; this one waits).
    Busy,
    /// A device that stays was added by a removed one [R3]: the user confirms the list first.
    Confirm(Vec<String>),
    /// Another device rotated first; its directory was taken.
    Lost,
    /// Rotated to this epoch (and rekeyed as far as it got).
    Rotated(u32),
}

#[derive(Deserialize)]
struct PullWire {
    records: Vec<Pulled>,
    has_more: bool,
}

#[derive(Deserialize)]
struct Pulled {
    seq: i64,
    envelope: Envelope,
}

fn alarm() -> SyncFail {
    SyncFail::Keys(KeyError::Dir(DirError::BadTag))
}

fn d32(s: &str, what: &'static str) -> SResult<[u8; 32]> {
    unb64(s)?.try_into().map_err(|_| SyncFail::BadAnswer(what))
}

/// The account's active devices, as the server lists them.
fn active_devices(api: &Api) -> SResult<Vec<String>> {
    Ok(client::devices(api)?.into_iter().filter(|d| d.revoked_at.is_none()).map(|d| d.id).collect())
}

fn claim(api: &Api, kind: &str) -> SResult<bool> {
    match api.post::<Value>("keys/rotation/claim", &json!({ "kind": kind })) {
        Ok(_) => Ok(true),
        Err(SyncFail::Api(ApiError::Conflict(_))) => Ok(false),
        Err(e) => Err(e),
    }
}

/// Steps 1–3 for AK. `confirmed`: the user already saw the devices that stay (or started the
/// removal on this device).
pub fn rotate_ak(api: &Api, st: &mut SyncState, conn: &Connection, confirmed: bool) -> SResult<AkRotation> {
    if client::refresh(api, st, None)? == client::Refresh::SelfRevoked {
        return Err(SyncFail::Api(ApiError::Unauthorized));
    }
    let server = client::state(api)?;
    if !server.rotation.ak {
        return Ok(AkRotation::Nothing);
    }
    let acc = st.account_bytes().map_err(|_| SyncFail::BadAnswer("account id"))?;
    let pin = st.ak.clone().ok_or(SyncFail::BadAnswer("no keys"))?;
    let (ak_blob, pak_blob) = client::head(api)?.ok_or(SyncFail::Reset)?;
    if dir::hash(&ak_blob.bytes) != pin.hash {
        // The refresh above took the head; a different one now is a race: next run.
        return Ok(AkRotation::Lost);
    }
    let head = dir::parse_ak(&ak_blob.bytes)?;
    let active = active_devices(api)?;
    let removed: Vec<String> = head.devices.iter().filter(|d| !active.contains(&d.id)).map(|d| d.id.clone()).collect();
    let mut next: AkDir = head.clone();
    next.devices.retain(|d| active.contains(&d.id));
    let by_removed: Vec<String> =
        next.devices.iter().filter(|d| removed.contains(&d.added_by)).map(|d| d.id.clone()).collect();
    if !by_removed.is_empty() && !confirmed {
        return Ok(AkRotation::Confirm(by_removed));
    }
    if !claim(api, "ak")? {
        return Ok(AkRotation::Busy);
    }
    let e = pin.epoch + 1;
    let new_ak = Zeroizing::new(crypto::random::<32>()?);
    next.epoch = e;
    next.seq = pin.seq + 1;
    next.prev_hash = envelope::b64(&pin.hash);
    for r in &removed {
        if !next.revoked.contains(r) {
            next.revoked.push(r.clone());
        }
    }
    next.min_record_epoch = pin.epoch;
    let active_refs: Vec<&str> = active.iter().map(String::as_str).collect();
    let mut seals = Vec::new();
    for (id, pk) in dir::seal_targets(&next, &active_refs)? {
        seals.push(dir::seal_key(SealKind::Ak, &new_ak, &acc, e, &id, &pk)?);
    }
    // The escrow key this device pinned from an authenticated source; one the server names
    // (pak_dir, unverifiable without PAK) only for state files from before the pin.
    let esc_pk = match st.esc_pk {
        Some(k) => k,
        None => d32(&dir::parse_pak(&pak_blob.bytes)?.esc_pk, "esc_pk")?,
    };
    seals.push(dir::seal_key(SealKind::Ak, &new_ak, &acc, e, "escrow", &esc_pk)?);
    next.seals = seals;
    let (n, ct) = dir::hist_seal(SealKind::Ak, &new_ak, &pin.ak, &acc, e)?;
    next.hist = Some(AkHist { epoch: pin.epoch, n, ct });
    next.created_ms = api.now_ms.max(0) as u64;
    next.created_by = st.device_id.clone();
    let blob = dir::sign_ak(&next, &new_ak, Some(&pin.ak), None, &acc)?;
    let commit =
        super::keys::Commit { ak_dir: Some(blob.clone()), if_ak_prev_hash: Some(pin.hash), ..Default::default() };
    match client::commit(api, &commit) {
        Ok(_) => {}
        Err(SyncFail::Api(ApiError::Conflict(_))) => {
            client::refresh(api, st, None)?;
            return Ok(AkRotation::Lost);
        }
        Err(e) => return Err(e),
    }
    st.ak_old.retain(|k| k.epoch != pin.epoch);
    st.ak_old.push(EpochKey { epoch: pin.epoch, key: pin.ak.clone() });
    st.ak =
        Some(AkPin { epoch: e, ak: new_ak, seq: next.seq, hash: dir::hash(&blob.bytes), min_record_epoch: pin.epoch });
    st.secure_needed = true;
    rekey_ak(api, st, conn, &removed)?;
    finish_ak(api, st)?;
    Ok(AkRotation::Rotated(e))
}

/// Step 2 for AK (resumable): every record of an older epoch, re-encrypted under the current
/// one. The removed devices' `tabs` and `device_meta` become tombstones. Returns how many.
pub fn rekey_ak(api: &Api, st: &mut SyncState, conn: &Connection, removed: &[String]) -> SResult<usize> {
    let acc = st.account_bytes().map_err(|_| SyncFail::BadAnswer("account id"))?;
    let pin = st.ak.clone().ok_or(SyncFail::BadAnswer("no keys"))?;
    let e = pin.epoch;
    // Every revoked device's open tabs and name go (a device resuming the rekey knows them from
    // the directory's revoked list).
    let mut removed: Vec<String> = removed.to_vec();
    if let Some((ak_blob, _)) = client::head(api)? {
        removed.extend(dir::parse_ak(&ak_blob.bytes)?.revoked);
    }
    let mut hlc = Hlc::new(st.hlc);
    let mut since = 0i64;
    let mut done = 0;
    loop {
        let page: PullWire = api.get(&format!("records?since={since}&limit={PAGE}&epoch_lt={e}&k=ak"))?;
        let mut replaces: Vec<Value> = Vec::new();
        for r in &page.records {
            since = since.max(r.seq);
            let env = &r.envelope;
            let Some(old) = ak_for(st, env.e) else { continue };
            let ko = envelope::collection_keys(&old, &acc, &env.c, env.e);
            let mut plain = envelope::open_record(&ko.ck, &acc, env).map_err(|_| alarm())?;
            let rid = unb64(&env.id)?;
            let nk = if plain.deleted {
                match plain.data.get("nk").and_then(Value::as_str) {
                    Some(n) => Some(n.to_string()),
                    None => local::item_by_record(conn, &env.c, &rid)?.map(|(n, _)| n),
                }
            } else {
                coll::natural_key(&env.c, &plain.data)
            };
            let Some(nk) = nk else { continue };
            if envelope::record_id(&ko.ik, nk.as_bytes()).as_slice() != rid.as_slice() {
                return Err(alarm());
            }
            if matches!(env.c.as_str(), "tabs" | "device_meta") && removed.contains(&nk) && !plain.deleted {
                plain = RecordPlain {
                    ver: hlc.tick(api.now_ms.max(0) as u64),
                    dev: st.dev_short,
                    deleted: true,
                    data: json!({ "nk": nk }),
                };
            } else if plain.deleted && plain.data.get("nk").is_none() {
                plain.data = json!({ "nk": nk });
            }
            let kn = envelope::collection_keys(&pin.ak, &acc, &env.c, e);
            let id = envelope::record_id(&kn.ik, nk.as_bytes());
            let bucket = unb64(&env.ct)?.len().saturating_sub(16);
            let sealed = envelope::seal_record(&kn.ck, &acc, &env.c, &id, e, &plain, Some(bucket))?;
            replaces.push(json!({"old_id": env.id, "c": env.c, "envelope": sealed}));
        }
        done += send_rekey(api, e, &replaces)?;
        if !page.has_more {
            break;
        }
    }
    st.hlc = st.hlc.max(hlc.last());
    Ok(done)
}

fn send_rekey(api: &Api, epoch: u32, replaces: &[Value]) -> SResult<usize> {
    let mut i = 0;
    let mut sent = 0;
    while i < replaces.len() {
        let mut j = i;
        let mut bytes = 0;
        while j < replaces.len() && j - i < PAGE && (j == i || bytes < BATCH_BYTES) {
            bytes += replaces[j]["envelope"]["ct"].as_str().map_or(0, str::len);
            j += 1;
        }
        let _: Value = api.post("rekey/batch", &json!({"epoch": epoch, "replaces": &replaces[i..j]}))?;
        sent += j - i;
        i = j;
    }
    Ok(sent)
}

/// Step 3 for AK (resumable): when no record of an older epoch is left, `ak_dir` again without
/// `hist` and with `min_record_epoch` = the epoch; the older keys are dropped.
pub fn finish_ak(api: &Api, st: &mut SyncState) -> SResult<bool> {
    let pin = st.ak.clone().ok_or(SyncFail::BadAnswer("no keys"))?;
    if pin.min_record_epoch >= pin.epoch {
        st.ak_old.retain(|k| k.epoch >= pin.min_record_epoch);
        return Ok(false);
    }
    let left: PullWire = api.get(&format!("records?since=0&limit=1&epoch_lt={}&k=ak", pin.epoch))?;
    if !left.records.is_empty() {
        return Ok(false);
    }
    let acc = st.account_bytes().map_err(|_| SyncFail::BadAnswer("account id"))?;
    let (ak_blob, _) = client::head(api)?.ok_or(SyncFail::Reset)?;
    if dir::hash(&ak_blob.bytes) != pin.hash {
        client::refresh(api, st, None)?;
        return Ok(false);
    }
    let mut next = dir::parse_ak(&ak_blob.bytes)?;
    next.seq = pin.seq + 1;
    next.prev_hash = envelope::b64(&pin.hash);
    next.hist = None;
    next.min_record_epoch = pin.epoch;
    next.created_ms = api.now_ms.max(0) as u64;
    next.created_by = st.device_id.clone();
    let blob = dir::sign_ak(&next, &pin.ak, None, None, &acc)?;
    let commit =
        super::keys::Commit { ak_dir: Some(blob.clone()), if_ak_prev_hash: Some(pin.hash), ..Default::default() };
    match client::commit(api, &commit) {
        Ok(_) => {}
        Err(SyncFail::Api(ApiError::Conflict(_))) => {
            client::refresh(api, st, None)?;
            return Ok(false);
        }
        Err(e) => return Err(e),
    }
    st.ak = Some(AkPin { seq: next.seq, hash: dir::hash(&blob.bytes), min_record_epoch: pin.epoch, ..pin });
    st.ak_old.clear();
    Ok(true)
}

/// PAK rotation (step 7) by a device whose vault is unlocked: `pak` is its vault state, updated.
/// Returns the new PAK epoch, or `None` (nothing pending, another device rotates, or it lost).
pub fn rotate_pak(api: &Api, st: &mut SyncState, pak: &mut PakState) -> SResult<Option<u32>> {
    if client::refresh(api, st, Some(pak))? == client::Refresh::SelfRevoked {
        return Ok(None);
    }
    let server = client::state(api)?;
    if !server.rotation.pak || server.rotation.ak {
        return Ok(None);
    }
    let acc = st.account_bytes().map_err(|_| SyncFail::BadAnswer("account id"))?;
    let (ak_blob, pak_blob) = client::head(api)?.ok_or(SyncFail::Reset)?;
    let pin = pak.pak.clone();
    if dir::hash(&pak_blob.bytes) != pin.hash || st.ak.as_ref().is_none_or(|a| a.hash != dir::hash(&ak_blob.bytes)) {
        return Ok(None);
    }
    let ak_dir = dir::parse_ak(&ak_blob.bytes)?;
    let head = dir::parse_pak(&pak_blob.bytes)?;
    let active = active_devices(api)?;
    let active_refs: Vec<&str> = active.iter().map(String::as_str).collect();
    let targets = dir::vault_seal_targets(&head, &ak_dir, &active_refs)?;
    if !claim(api, "pak")? {
        return Ok(None);
    }
    let p = pin.pak_epoch + 1;
    let new_pak = Zeroizing::new(crypto::random::<32>()?);
    let mut next = head.clone();
    next.pak_epoch = p;
    next.seq = pin.seq + 1;
    next.prev_hash = envelope::b64(&pin.hash);
    next.vault_keys.retain(|v| targets.iter().any(|(id, _)| *id == v.device));
    let mut seals = Vec::new();
    for (id, pk) in &targets {
        seals.push(dir::seal_key(SealKind::Pak, &new_pak, &acc, p, id, pk)?);
    }
    // PAK holders verified esc_pk through PAK-authenticated directories.
    seals.push(dir::seal_key(SealKind::Pak, &new_pak, &acc, p, "escrow", &pin.esc_pk)?);
    next.seals = seals;
    let (n, ct) = dir::hist_seal(SealKind::Pak, &new_pak, &pin.pak, &acc, p)?;
    next.hist = Some(PakHist { pak_epoch: pin.pak_epoch, n, ct });
    next.created_ms = api.now_ms.max(0) as u64;
    next.created_by = st.device_id.clone();
    let blob = dir::sign_pak(&next, &new_pak, Some(&pin.pak), None, &acc)?;
    let commit =
        super::keys::Commit { pak_dir: Some(blob.clone()), if_pak_prev_hash: Some(pin.hash), ..Default::default() };
    match client::commit(api, &commit) {
        Ok(_) => {}
        Err(SyncFail::Api(ApiError::Conflict(_))) => {
            client::refresh(api, st, Some(pak))?;
            return Ok(None);
        }
        Err(e) => return Err(e),
    }
    pak.pak_old.retain(|k| k.epoch != pin.pak_epoch);
    pak.pak_old.push(EpochKey { epoch: pin.pak_epoch, key: pin.pak.clone() });
    pak.pak = PakPin { pak_epoch: p, pak: new_pak, seq: next.seq, hash: dir::hash(&blob.bytes), ..pin };
    st.secure_needed = true;
    rekey_pak(api, st, pak)?;
    Ok(Some(p))
}

/// The PAK collections' rekey (resumable by any PAK holder): records of an older PAK epoch,
/// re-encrypted under the current one. When none is left, the older keys are dropped, so a
/// record replayed under an old epoch is never opened again.
pub fn rekey_pak(api: &Api, st: &SyncState, pak: &mut PakState) -> SResult<usize> {
    if pak.pak_old.is_empty() {
        return Ok(0);
    }
    let acc = st.account_bytes().map_err(|_| SyncFail::BadAnswer("account id"))?;
    let p = pak.pak.pak_epoch;
    let mut since = 0i64;
    let mut done = 0;
    loop {
        let page: PullWire = api.get(&format!("records?since={since}&limit={PAGE}&epoch_lt={p}&k=pak"))?;
        let mut replaces: Vec<Value> = Vec::new();
        for r in &page.records {
            since = since.max(r.seq);
            let env = &r.envelope;
            let Some(old) = pak.pak_old.iter().find(|k| k.epoch == env.e).map(|k| k.key.clone()) else { continue };
            let ko = envelope::collection_keys(&old, &acc, &env.c, env.e);
            let mut plain = envelope::open_record(&ko.ck, &acc, env).map_err(|_| alarm())?;
            let nk = if plain.deleted {
                plain.data.get("nk").and_then(Value::as_str).map(str::to_string).or_else(|| {
                    pak.book
                        .items
                        .iter()
                        .find(|(k, i)| k.starts_with(&format!("{}/", env.c)) && i.rid == env.id)
                        .map(|(k, _)| k[env.c.len() + 1..].to_string())
                })
            } else {
                match env.c.as_str() {
                    "passwords" => plain.data["id"].as_str().map(str::to_string),
                    _ => plain.data["origin"].as_str().map(str::to_string),
                }
            };
            let Some(nk) = nk else {
                super::pak::wipe(&mut plain.data);
                continue;
            };
            if envelope::record_id(&ko.ik, nk.as_bytes()).as_slice() != unb64(&env.id)?.as_slice() {
                super::pak::wipe(&mut plain.data);
                return Err(alarm());
            }
            if plain.deleted && plain.data.get("nk").is_none() {
                plain.data = json!({ "nk": nk });
            }
            let kn = envelope::collection_keys(&pak.pak.pak, &acc, &env.c, p);
            let id = envelope::record_id(&kn.ik, nk.as_bytes());
            let bucket = unb64(&env.ct)?.len().saturating_sub(16);
            let sealed = envelope::seal_record(&kn.ck, &acc, &env.c, &id, p, &plain, Some(bucket));
            super::pak::wipe(&mut plain.data);
            let sealed = sealed?;
            if let Some(item) = pak.book.items.get_mut(&format!("{}/{nk}", env.c)) {
                item.rid = sealed.id.clone();
            }
            replaces.push(json!({"old_id": env.id, "c": env.c, "envelope": sealed}));
        }
        done += send_rekey(api, p, &replaces)?;
        if !page.has_more {
            break;
        }
    }
    let left: PullWire = api.get(&format!("records?since=0&limit=1&epoch_lt={p}&k=pak"))?;
    if left.records.is_empty() {
        pak.pak_old.clear();
    }
    Ok(done)
}

/// What a PAK holder does before its passwords run: rotate if asked, finish a rekey if one is
/// left. `true` when `pak` changed (the caller writes it into the vault).
pub fn pak_maintenance(api: &Api, st: &mut SyncState, pak: &mut PakState) -> SResult<bool> {
    let before = pak.pak.hash;
    let old_before = pak.pak_old.len();
    rotate_pak(api, st, pak)?;
    rekey_pak(api, st, pak)?;
    Ok(pak.pak.hash != before || pak.pak_old.len() != old_before)
}
