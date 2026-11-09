//! The PAK collections (S8, PROTOCOL §9.5): `passwords` (the vault's logins) and `never_save`
//! (the "never save passwords on this site" list). They are encrypted under PAK, which only an
//! unlocked vault holds, so they sync only while the vault is unlocked (THREAT_MODEL G4). Their
//! bookkeeping — the pull cursor, what each record last was, the previous versions of logins
//! that lost a conflict — lives in the vault's sealed sync part, never in browser.db.
//!
//! A run has three steps, so that the network part can run off the UI thread, where the open
//! vault lives:
//! 1. [`prepare`] (vault open): what changed here since sync last saw it, encrypted;
//! 2. [`exchange`] (network only): pull the PAK records since the cursor, push the batch;
//! 3. [`finish`] (vault open): record what was pushed, merge what came, one vault write.
//!
//! A local change is found by comparing each login's `updated` time (and the never-save list)
//! with what sync last recorded, so nothing is lost if Krunk stops between the steps. Logins merge
//! per entry, the newer edit (`updated`) winning; the losing version is kept inside the vault for
//! 30 days. A login from another device with the same site and user name as one never synced
//! here is adopted under the other device's id instead of being duplicated.

use super::api::ApiError;
use super::client::{Api, SResult, SyncFail};
use super::dir::DirError;
use super::envelope::{self, b64, unb64, Envelope, KeyKind, RecordPlain};
use super::hlc::Hlc;
use super::keys::{KeyError, PakState};
use super::AccountId;
use crate::storage::Store;
use crate::vault::{Entry, KeyWrapper, Vault, VaultSync};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use zeroize::{Zeroize, Zeroizing};

/// The PAK collections this version syncs (`payments` is defined but not synced in 0.9.0).
pub const COLLECTIONS: [&str; 2] = ["passwords", "never_save"];
/// How long a login that lost a conflict is kept (PROTOCOL §9.5).
pub const PREVIOUS_MS: i64 = 30 * 86_400_000;
const PAGE: usize = 500;
const BATCH: usize = 500;
const BATCH_BYTES: usize = 1024 * 1024;

/// What sync knows of the PAK collections (inside [`PakState`], so under the vault key).
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PakBook {
    /// The last `seq` pulled of the PAK records.
    #[serde(default)]
    pub pull_seq: u64,
    /// `"<collection>/<natural key>"` → what the record last was.
    #[serde(default)]
    pub items: BTreeMap<String, PakItem>,
    /// Logins that lost a conflict (or were deleted by another device), kept for 30 days.
    #[serde(default)]
    pub previous: Vec<Previous>,
    /// The first run is done: it only pulls and merges (PROTOCOL §10.5), so a login this vault
    /// shares with another device is adopted before anything is pushed.
    #[serde(default)]
    pub primed: bool,
}

impl std::fmt::Debug for PakBook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PakBook")
            .field("pull_seq", &self.pull_seq)
            .field("items", &self.items.len())
            .field("previous", &self.previous.len())
            .finish()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PakItem {
    /// The record id (base64url).
    pub rid: String,
    pub seq: i64,
    pub ver: u64,
    pub deleted: bool,
    /// passwords: the login's `updated` time as sync last saw it (a different one = changed here).
    #[serde(default)]
    pub stamp: i64,
    /// The padded size of the last version (a tombstone keeps it, PROTOCOL §9.4).
    #[serde(default)]
    pub bucket: usize,
}

/// A login's earlier version.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Previous {
    pub id: String,
    pub origin: String,
    pub username: String,
    pub password: Zeroizing<String>,
    pub note: Zeroizing<String>,
    pub updated: i64,
    /// Removed after this time (ms).
    pub until: i64,
}

impl std::fmt::Debug for Previous {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Previous").field("id", &self.id).field("until", &self.until).finish_non_exhaustive()
    }
}

fn key(c: &str, nk: &str) -> String {
    format!("{c}/{nk}")
}

/// Overwrites every string inside `v` (record plaintext with a password in it).
fn wipe(v: &mut Value) {
    match v {
        Value::String(s) => s.zeroize(),
        Value::Array(a) => a.iter_mut().for_each(wipe),
        Value::Object(o) => o.values_mut().for_each(wipe),
        _ => {}
    }
}

fn alarm() -> SyncFail {
    SyncFail::Keys(KeyError::Dir(DirError::BadTag))
}

/// The key of a PAK epoch (the head's or a kept older one).
fn pak_for(pak: &PakState, epoch: u32) -> Option<&[u8; 32]> {
    if epoch == pak.pak.pak_epoch {
        return Some(&pak.pak.pak);
    }
    pak.pak_old.iter().find(|k| k.epoch == epoch).map(|k| &*k.key)
}

/// The payload of a login (PROTOCOL §9.5; `id` added so a receiver knows the natural key).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginData {
    id: String,
    origin: String,
    username: String,
    password: Zeroizing<String>,
    #[serde(default)]
    note: Zeroizing<String>,
    created: i64,
    updated: i64,
}

impl LoginData {
    fn into_entry(self) -> Entry {
        Entry {
            id: self.id,
            origin: self.origin,
            username: self.username,
            password: self.password,
            note: self.note,
            created: self.created,
            updated: self.updated,
        }
    }
}

fn login_value(e: &Entry) -> Value {
    json!({
        "id": e.id, "origin": e.origin, "username": e.username, "password": e.password.as_str(),
        "note": e.note.as_str(), "created": e.created, "updated": e.updated,
    })
}

fn same_login(a: &Entry, b: &Entry) -> bool {
    a.origin == b.origin && a.username == b.username && *a.password == *b.password && *a.note == *b.note
}

/// `a` beats `b` (the later edit; equal times are decided by the content, the same way on every
/// device).
fn newer(a: &Entry, b: &Entry) -> bool {
    (a.updated, a.password.as_str(), a.note.as_str(), a.username.as_str())
        >= (b.updated, b.password.as_str(), b.note.as_str(), b.username.as_str())
}

fn keep_previous(book: &mut PakBook, e: &Entry, now_ms: i64) {
    book.previous.push(Previous {
        id: e.id.clone(),
        origin: e.origin.clone(),
        username: e.username.clone(),
        password: e.password.clone(),
        note: e.note.clone(),
        updated: e.updated,
        until: now_ms + PREVIOUS_MS,
    });
}

// ---- 1. prepare ----------------------------------------------------------------------------

struct Pend {
    c: &'static str,
    nk: String,
    rid: String,
    ver: u64,
    deleted: bool,
    stamp: i64,
    bucket: usize,
}

/// What this device sends: the write bodies (envelopes only) and what each one stands for.
pub struct Outgoing {
    pub writes: Vec<Value>,
    pend: Vec<Pend>,
}

impl Outgoing {
    pub fn is_empty(&self) -> bool {
        self.writes.is_empty()
    }
}

#[allow(clippy::too_many_arguments)]
fn seal_one(
    out: &mut Outgoing,
    pak: &PakState,
    acc: &AccountId,
    c: &'static str,
    nk: &str,
    mut data: Value,
    deleted: bool,
    stamp: i64,
    item: Option<&PakItem>,
    dev: u16,
    hlc: &mut Hlc,
    now_ms: i64,
) -> SResult<()> {
    let epoch = pak.pak.pak_epoch;
    let keys = envelope::collection_keys(&pak.pak.pak, acc, c, epoch);
    let id = envelope::record_id(&keys.ik, nk.as_bytes());
    let plain = RecordPlain { ver: hlc.tick(now_ms.max(0) as u64), dev, deleted, data: data.take() };
    let bucket = if deleted { item.map(|i| i.bucket).filter(|b| *b > 0) } else { None };
    let sealed = envelope::seal_record(&keys.ck, acc, c, &id, epoch, &plain, bucket);
    let mut plain = plain;
    wipe(&mut plain.data);
    let env = sealed?;
    let pad = unb64(&env.ct).map(|x| x.len().saturating_sub(16)).unwrap_or(0);
    out.writes.push(json!({"c": c, "id": env.id, "if_seq": item.map_or(0, |i| i.seq), "envelope": env}));
    out.pend.push(Pend { c, nk: nk.to_string(), rid: b64(&id), ver: plain.ver, deleted, stamp, bucket: pad });
    Ok(())
}

/// Step 1 (vault open): every login and never-save origin that changed here since sync last saw
/// it, encrypted under PAK.
pub fn prepare(
    vault: &Vault,
    pak: &PakState,
    never: &[String],
    acc: &AccountId,
    dev: u16,
    hlc: &mut Hlc,
    now_ms: i64,
) -> SResult<Outgoing> {
    let book = &pak.book;
    let mut out = Outgoing { writes: Vec::new(), pend: Vec::new() };
    if !book.primed {
        return Ok(out);
    }
    for e in vault.entries() {
        let item = book.items.get(&key("passwords", &e.id));
        if item.is_some_and(|i| !i.deleted && i.stamp == e.updated) {
            continue;
        }
        seal_one(&mut out, pak, acc, "passwords", &e.id, login_value(e), false, e.updated, item, dev, hlc, now_ms)?;
    }
    for (k, item) in &book.items {
        if item.deleted || item.seq == 0 {
            continue;
        }
        let gone = match k.split_once('/') {
            Some(("passwords", id)) => vault.entry(id).is_none().then_some(("passwords", id)),
            Some(("never_save", o)) => (!never.iter().any(|n| n == o)).then_some(("never_save", o)),
            _ => None,
        };
        if let Some((c, nk)) = gone {
            seal_one(&mut out, pak, acc, c, nk, json!({}), true, 0, Some(item), dev, hlc, now_ms)?;
        }
    }
    for o in never {
        let item = book.items.get(&key("never_save", o));
        if item.is_some_and(|i| !i.deleted) {
            continue;
        }
        seal_one(
            &mut out,
            pak,
            acc,
            "never_save",
            o,
            json!({"origin": o, "added": now_ms}),
            false,
            0,
            item,
            dev,
            hlc,
            now_ms,
        )?;
    }
    Ok(out)
}

// ---- 2. exchange ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct PullWire {
    records: Vec<Pulled>,
    has_more: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Pulled {
    pub seq: i64,
    pub envelope: Envelope,
}

#[derive(Deserialize)]
struct BatchWire {
    results: Vec<Value>,
}

/// What the server answered.
#[derive(Debug, Default)]
pub struct Exchanged {
    pub pulled: Vec<Pulled>,
    /// The cursor after the pull.
    pub pull_seq: u64,
    /// The pull went below the purge horizon and started over (a full reconcile).
    pub full: bool,
    /// One answer per write (`None`: not sent, writes were frozen).
    pub results: Vec<Option<Value>>,
    /// The PAK collections are frozen until the passwords key is rotated (423).
    pub frozen: bool,
}

/// Step 2 (network): pull the PAK records since `since`, then push `writes`.
pub fn exchange(api: &Api, since: u64, writes: &[Value]) -> SResult<Exchanged> {
    let mut ex = Exchanged::default();
    let mut since = since as i64;
    let mut restarted = false;
    loop {
        let page: PullWire = match api.get(&format!("records?since={since}&limit={PAGE}&k=pak")) {
            Ok(p) => p,
            Err(SyncFail::Api(ApiError::Gone)) if !restarted => {
                ex.full = true;
                restarted = true;
                since = 0;
                ex.pulled.clear();
                continue;
            }
            Err(e) => return Err(e),
        };
        for r in page.records {
            since = since.max(r.seq);
            if r.envelope.k == KeyKind::Pak {
                ex.pulled.push(r);
            }
        }
        if !page.has_more {
            break;
        }
    }
    ex.pull_seq = since.max(0) as u64;
    let mut i = 0;
    while i < writes.len() {
        let mut bytes = 0;
        let mut j = i;
        while j < writes.len() && j - i < BATCH && (j == i || bytes < BATCH_BYTES) {
            bytes += writes[j]["envelope"]["ct"].as_str().map_or(0, str::len);
            j += 1;
        }
        match api.post::<BatchWire>("records/batch", &json!({"writes": &writes[i..j]})) {
            Ok(b) => {
                let mut res = b.results.into_iter();
                for _ in i..j {
                    ex.results.push(res.next());
                }
            }
            Err(SyncFail::Api(ApiError::RotationPending)) => {
                ex.frozen = true;
                break;
            }
            Err(e) => return Err(e),
        }
        i = j;
    }
    ex.results.resize(writes.len(), None);
    Ok(ex)
}

// ---- 3. finish -----------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PakReport {
    pub pulled: usize,
    pub pushed: usize,
    pub conflicts: usize,
    /// Logins were added, changed or removed here.
    pub passwords: bool,
    /// The never-save list changed here.
    pub never_save: bool,
    /// Records that didn't pass the checks (skipped).
    pub skipped: usize,
    pub frozen: bool,
    pub full: bool,
}

/// What the merge does to the vault, collected for one write.
#[derive(Default)]
struct Changes {
    puts: Vec<Entry>,
    removes: Vec<String>,
}

impl Changes {
    /// The login as it stands after the changes so far.
    fn current<'a>(&'a self, vault: &'a Vault, id: &str) -> Option<&'a Entry> {
        if self.removes.iter().any(|r| r == id) {
            return None;
        }
        self.puts.iter().rev().find(|e| e.id == id).or_else(|| vault.entry(id))
    }

    fn put(&mut self, e: Entry) {
        self.removes.retain(|r| *r != e.id);
        self.puts.retain(|p| p.id != e.id);
        self.puts.push(e);
    }

    fn remove(&mut self, id: &str) {
        self.puts.retain(|p| p.id != id);
        if !self.removes.iter().any(|r| r == id) {
            self.removes.push(id.to_string());
        }
    }
}

/// Step 3 (vault open): what was pushed is recorded, what came is merged; one vault write (the
/// never-save list in browser.db is written first, so a failed vault write only repeats work).
#[allow(clippy::too_many_arguments)]
pub fn finish(
    vault: &mut Vault,
    pak: &mut PakState,
    store: &Store,
    acc: &AccountId,
    out: Outgoing,
    ex: Exchanged,
    hlc: &mut Hlc,
    now_ms: i64,
    wrapper: &dyn KeyWrapper,
) -> SResult<PakReport> {
    let mut report = PakReport { frozen: ex.frozen, full: ex.full, ..Default::default() };
    let mut incoming: Vec<Pulled> = ex.pulled;
    for (p, res) in out.pend.into_iter().zip(ex.results) {
        let Some(res) = res else { continue };
        if res["status"] == "ok" {
            let seq = res["seq"].as_i64().unwrap_or(0);
            pak.book.items.insert(
                key(p.c, &p.nk),
                PakItem { rid: p.rid, seq, ver: p.ver, deleted: p.deleted, stamp: p.stamp, bucket: p.bucket },
            );
            report.pushed += 1;
        } else {
            report.conflicts += 1;
            if let Some(cur) = res.get("current").filter(|c| !c.is_null()) {
                let envelope: Envelope =
                    serde_json::from_value(cur["envelope"].clone()).map_err(|_| SyncFail::BadAnswer("conflict"))?;
                incoming.push(Pulled { seq: cur["seq"].as_i64().unwrap_or(0), envelope });
            }
        }
    }
    let mut ch = Changes::default();
    let never: Vec<String> = store.never_save_list()?;
    let mut never_now = never.clone();
    for r in &incoming {
        if merge_one(vault, pak, acc, r, &mut ch, &mut never_now, hlc, now_ms, &mut report)? {
            report.pulled += 1;
        }
    }
    // The never-save list first (idempotent), then the vault with the bookkeeping.
    for o in never_now.iter().filter(|o| !never.contains(o)) {
        store.never_save_add(o, now_ms)?;
        report.never_save = true;
    }
    for o in never.iter().filter(|o| !never_now.contains(o)) {
        store.never_save_remove(o)?;
        report.never_save = true;
    }
    pak.book.previous.retain(|p| p.until > now_ms);
    // Everything pulled was merged (a freeze stops only the writes).
    pak.book.pull_seq = ex.pull_seq;
    pak.book.primed = true;
    report.passwords = !ch.puts.is_empty() || !ch.removes.is_empty();
    let part = VaultSync { state: pak.to_bytes(), dirty: Vec::new() };
    let skipped = vault.sync_apply(ch.puts, &ch.removes, part, wrapper).map_err(|e| SyncFail::Local(e.to_string()))?;
    report.skipped += skipped.len();
    Ok(report)
}

/// Merges one PAK record; `false` when it was not for this run (an unknown collection, an older
/// version than the one applied).
#[allow(clippy::too_many_arguments)]
fn merge_one(
    vault: &Vault,
    pak: &mut PakState,
    acc: &AccountId,
    r: &Pulled,
    ch: &mut Changes,
    never: &mut Vec<String>,
    hlc: &mut Hlc,
    now_ms: i64,
    report: &mut PakReport,
) -> SResult<bool> {
    let env = &r.envelope;
    let Some(c) = COLLECTIONS.iter().copied().find(|c| *c == env.c) else { return Ok(false) };
    let Some(root) = pak_for(pak, env.e) else {
        // A newer passwords key than ours: the directories must be followed first.
        return Err(SyncFail::Keys(KeyError::Dir(DirError::Gap)));
    };
    let keys = envelope::collection_keys(root, acc, c, env.e);
    let mut plain = envelope::open_record(&keys.ck, acc, env).map_err(|_| alarm())?;
    let rid = unb64(&env.id)?;
    let bucket = unb64(&env.ct).map(|ct| ct.len().saturating_sub(16)).unwrap_or(0);
    let nk = if plain.deleted {
        // A tombstone has no data: its natural key is one sync knows, or one held here.
        let known = pak.book.items.iter().find(|(k, i)| k.starts_with(&format!("{c}/")) && i.rid == env.id);
        let local: Vec<String> = match c {
            "passwords" => vault.entries().iter().chain(ch.puts.iter()).map(|e| e.id.clone()).collect(),
            _ => never.clone(),
        };
        match known {
            Some((k, _)) => k[c.len() + 1..].to_string(),
            None => match local
                .into_iter()
                .find(|n| envelope::record_id(&keys.ik, n.as_bytes()).as_slice() == rid.as_slice())
            {
                Some(n) => n,
                None => return Ok(false),
            },
        }
    } else {
        let nk = match c {
            "passwords" => plain.data["id"].as_str().map(str::to_string),
            _ => plain.data["origin"].as_str().map(str::to_string),
        };
        let Some(nk) = nk else {
            wipe(&mut plain.data);
            report.skipped += 1;
            return Ok(false);
        };
        if envelope::record_id(&keys.ik, nk.as_bytes()).as_slice() != rid.as_slice() {
            wipe(&mut plain.data);
            return Err(alarm());
        }
        nk
    };
    let k = key(c, &nk);
    let item = pak.book.items.get(&k).cloned();
    if let Some(it) = &item {
        if plain.ver <= it.ver {
            // This device's own write coming back, or an older version (a replay): nothing to do.
            wipe(&mut plain.data);
            if let Some(i) = pak.book.items.get_mut(&k) {
                i.seq = i.seq.max(r.seq);
            }
            return Ok(false);
        }
    }
    hlc.observe(plain.ver, now_ms.max(0) as u64);
    let mut stamp = item.as_ref().map_or(0, |i| i.stamp);
    match c {
        "passwords" => {
            let local = ch.current(vault, &nk).cloned();
            let changed_here = match (&local, &item) {
                (Some(l), Some(i)) => i.deleted || l.updated != i.stamp,
                (Some(_), None) => true,
                (None, Some(i)) => !i.deleted,
                (None, None) => false,
            };
            if plain.deleted {
                match local {
                    Some(l) if !changed_here => {
                        keep_previous(&mut pak.book, &l, now_ms);
                        ch.remove(&nk);
                    }
                    // Edited here meanwhile: the edit wins over the deletion (pushed next).
                    _ => {}
                }
            } else {
                let data = std::mem::take(&mut plain.data);
                let parsed: Result<LoginData, _> = serde_json::from_value(data);
                let Ok(remote) = parsed.map(LoginData::into_entry) else {
                    report.skipped += 1;
                    return Ok(false);
                };
                if remote.id != nk {
                    return Err(alarm());
                }
                match local {
                    None => {
                        // A login new here for a site and user name this vault already has (saved
                        // on two devices): one login on every device, under the smaller id, with
                        // the newer content; the other id is deleted everywhere.
                        let twin = if item.is_none() {
                            vault
                                .entries()
                                .iter()
                                .chain(ch.puts.iter())
                                .find(|e| {
                                    e.id != nk
                                        && e.origin == remote.origin
                                        && e.username == remote.username
                                        && !ch.removes.contains(&e.id)
                                })
                                .cloned()
                        } else {
                            None
                        };
                        if let Some(t) = twin {
                            let keep = if nk < t.id { nk.clone() } else { t.id.clone() };
                            let remote_won = newer(&remote, &t);
                            let created = remote.created.min(t.created);
                            let (win, lose) = if remote_won { (remote, t.clone()) } else { (t.clone(), remote) };
                            if !same_login(&win, &lose) {
                                keep_previous(&mut pak.book, &lose, now_ms);
                            }
                            stamp = if keep == nk && remote_won { win.updated } else { 0 };
                            if keep != t.id {
                                ch.remove(&t.id);
                            }
                            ch.put(Entry { id: keep, created, ..win });
                        } else {
                            // New here (or deleted here and edited there: the edit wins).
                            stamp = remote.updated;
                            ch.put(remote);
                        }
                    }
                    Some(l) => {
                        let remote_wins = !changed_here || newer(&remote, &l);
                        if remote_wins {
                            if changed_here && !same_login(&l, &remote) {
                                keep_previous(&mut pak.book, &l, now_ms);
                            }
                            stamp = remote.updated;
                            ch.put(remote);
                        } else if !same_login(&l, &remote) {
                            // The local edit is newer: kept (and pushed next); the remote one is
                            // the previous version.
                            keep_previous(&mut pak.book, &remote, now_ms);
                        }
                    }
                }
            }
        }
        _ => {
            let present = never.contains(&nk);
            let changed_here = match &item {
                Some(i) => present == i.deleted,
                None => present,
            };
            if plain.deleted {
                if present && !changed_here {
                    never.retain(|o| *o != nk);
                }
            } else if crate::urlclean::origin_of(&nk).as_deref() != Some(nk.as_str()) {
                report.skipped += 1;
                return Ok(false);
            } else if !present {
                never.push(nk.clone());
            }
            wipe(&mut plain.data);
        }
    }
    pak.book
        .items
        .insert(k, PakItem { rid: env.id.clone(), seq: r.seq, ver: plain.ver, deleted: plain.deleted, stamp, bucket });
    Ok(true)
}

// ---- One run (the test client, the tests) ----------------------------------------------------

/// The three steps in a row, for a caller that holds the vault on the same thread. `pak` is the
/// vault's sync state; it is written back with the vault.
#[allow(clippy::too_many_arguments)]
pub fn run(
    api: &Api,
    vault: &mut Vault,
    pak: &mut PakState,
    store: &Store,
    st: &mut super::state::SyncState,
    off: &[String],
    wrapper: &dyn KeyWrapper,
) -> SResult<PakReport> {
    if off.iter().any(|c| c == "passwords") {
        return Ok(PakReport::default());
    }
    let acc = st.account_bytes().map_err(|_| SyncFail::BadAnswer("account id"))?;
    let mut hlc = Hlc::new(st.hlc);
    let mut total = PakReport::default();
    // A first run only pulls; the next one pushes what is left, and a conflict is retried at once
    // (as the app's schedule does).
    for _ in 0..3 {
        let primed = pak.book.primed;
        let never = store.never_save_list()?;
        let out = prepare(vault, pak, &never, &acc, st.dev_short, &mut hlc, api.now_ms)?;
        let ex = exchange(api, pak.book.pull_seq, &out.writes)?;
        let r = finish(vault, pak, store, &acc, out, ex, &mut hlc, api.now_ms, wrapper)?;
        total.pulled += r.pulled;
        total.pushed += r.pushed;
        total.conflicts += r.conflicts;
        total.skipped += r.skipped;
        total.passwords |= r.passwords;
        total.never_save |= r.never_save;
        total.frozen |= r.frozen;
        total.full |= r.full;
        if primed && r.conflicts == 0 {
            break;
        }
    }
    st.hlc = hlc.last();
    Ok(total)
}
