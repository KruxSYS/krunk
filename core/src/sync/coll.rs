//! The AK collections (PROTOCOL §9.5): what each one pushes from local state, how a remote
//! record merges into local state, and the bookmark tree repair.
//!
//! **Natural keys** (the input of the record id, UTF-8): a bookmark's guid as 32 lowercase hex
//! characters; a history entry's normalized URL (§9.6); a setting's name; a pin's URL; an
//! allowlisted host; a device id for `tabs` and `device_meta`.
//!
//! **Conflicts.** A local change that wasn't pushed yet competes with a remote version by time:
//! the change's time (a bookmark's `modified`, else when it was queued) as an HLC value against
//! the record's `ver`; ties go to the higher `dev`. Bookmarks do this per field group: position
//! (parent, sort) and text (title, url) carry their own versions inside the record. History
//! never conflicts: visits are a union.

use super::envelope::RecordPlain;
use super::fracidx;
use super::hlc;
use super::local::{self, SyncItem};
use super::normalize::history_key;
use crate::settings::Settings;
use crate::storage::{DbResult, OTHER_BOOKMARKS, ROOT_GUID_BAR, ROOT_GUID_OTHER};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

/// The AK collections this version syncs, in the order a first sync pushes them.
pub const AK: [&str; 7] = ["device_meta", "settings", "allowlist", "pins", "bookmarks", "history", "tabs"];

/// The settings that travel (PLAN §5.5, with 0.9.0's names). Everything else is per device.
pub const SYNCED_SETTINGS: [&str; 20] = [
    "searchEngine",
    "customSearchUrl",
    "startup",
    "homepage",
    "bookmarksBar",
    "adblockEnabled",
    "filterLists",
    "customLists",
    // 0.9.0: the protection level (blockThirdPartyCookies is derived from it on each device).
    "protection",
    "httpsOnly",
    "sendDnt",
    "sendGpc",
    "stripTrackingParams",
    "clearOnExit",
    "engineSecurityComponents",
    "vaultAutolockMinutes",
    "offerSavePasswords",
    "askWhereToSave",
    "defaultZoom",
    "theme",
];

/// At most this many history entries are kept on the server; a first sync uploads the last 90
/// days, at most this many URLs (PROTOCOL §10.5).
pub const HISTORY_MAX: usize = 20_000;
pub const HISTORY_FIRST_DAYS: i64 = 90;
const VISITS_KEPT: usize = 20;
const TABS_MAX: usize = 500;

/// Per-run facts the collections need.
pub struct Ctx<'a> {
    pub device_id: &'a str,
    pub dev_short: u16,
    pub now_ms: i64,
    /// This device's name for `device_meta` (until renamed in krunk://sync).
    pub device_name: &'a str,
    /// The first sync of this device (bookmarks are matched by place and title, §10.5).
    pub initial: bool,
}

/// What changed locally by applying remote records (the app refreshes those parts).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Changed {
    pub bookmarks: bool,
    pub history: bool,
    pub settings: bool,
    pub pins: bool,
    pub allowlist: bool,
    pub tabs: bool,
    pub devices: bool,
}

impl Changed {
    pub fn any(&self) -> bool {
        self.bookmarks || self.history || self.settings || self.pins || self.allowlist || self.tabs || self.devices
    }
}

pub fn data_hash(v: &Value) -> String {
    let h = super::crypto::sha256(&[v.to_string().as_bytes()]);
    super::envelope::b64(&h[..16])
}

/// A remote record wins over a local change made at `local_ms`.
fn remote_wins(remote: &RecordPlain, local_ms: i64, my_dev: u16) -> bool {
    let local = hlc::encode(local_ms.max(0) as u64, 0);
    remote.ver > local || (remote.ver == local && remote.dev > my_dev)
}

fn queued_at(conn: &Connection, c: &str, raw: &str) -> DbResult<Option<i64>> {
    conn.query_row(
        "SELECT enqueued_at FROM sync_queue WHERE collection = ?1 AND natural_key = ?2",
        params![c, raw],
        |r| r.get(0),
    )
    .optional()
}

// ---- Keys ----------------------------------------------------------------------------------------

/// The natural keys a queue entry stands for.
pub fn expand(c: &str, raw: &str, ctx: &Ctx) -> Vec<String> {
    match c {
        "history" => history_key(raw).into_iter().collect(),
        "settings" => SYNCED_SETTINGS.iter().map(|s| s.to_string()).collect(),
        "tabs" => vec![ctx.device_id.to_string()],
        _ => vec![raw.to_string()],
    }
}

/// The natural key of a record's data (must match its id, or the record is refused).
pub fn natural_key(c: &str, data: &Value) -> Option<String> {
    let s = |k: &str| data.get(k).and_then(Value::as_str).map(str::to_string);
    match c {
        "bookmarks" => {
            s("guid").filter(|g| g.len() == 32 && g.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
        }
        "history" => s("url").and_then(|u| history_key(&u)),
        "settings" => s("key").filter(|k| SYNCED_SETTINGS.contains(&k.as_str())),
        "pins" => s("url"),
        "allowlist" => s("host"),
        "tabs" | "device_meta" => s("device"),
        _ => None,
    }
}

// ---- Local data (what a push sends) ----------------------------------------------------------------

/// The local data of an item, or `None` when it is gone here (a tombstone is pushed).
pub fn local_data(conn: &Connection, c: &str, nk: &str, item: Option<&SyncItem>, ctx: &Ctx) -> DbResult<Option<Value>> {
    match c {
        "bookmarks" => bookmark_data(conn, nk, item),
        "history" => history_data(conn, nk, item),
        "settings" => Ok(settings_json(conn)?.get(nk).cloned().map(|v| json!({"key": nk, "value": v}))),
        "pins" => conn
            .query_row("SELECT title, state, position FROM tiles WHERE url = ?1", [nk], |r| {
                Ok(json!({"url": nk, "title": r.get::<_, String>(0)?, "state": if r.get::<_, i64>(1)? == 1 { "pinned" } else { "hidden" },
                    "sort": format!("{:06}", r.get::<_, i64>(2)?)}))
            })
            .optional(),
        "allowlist" => conn
            .query_row("SELECT added FROM adblock_allowlist WHERE host = ?1", [nk], |r| Ok(json!({"host": nk, "added": r.get::<_, i64>(0)?})))
            .optional(),
        "tabs" if nk == ctx.device_id => {
            let raw: Option<String> = conn.query_row("SELECT value FROM kv WHERE key = 'session'", [], |r| r.get(0)).optional()?;
            let s: crate::windows::SessionV2 = raw.and_then(|r| serde_json::from_str(&r).ok()).unwrap_or_default();
            let mut left = TABS_MAX;
            let windows: Vec<Value> = s
                .windows
                .iter()
                .map(|w| {
                    let tabs: Vec<Value> = w.tabs.iter().take(left).map(|t| json!({"url": t.url, "title": t.title})).collect();
                    left -= tabs.len();
                    json!({"tabs": tabs, "active": w.active})
                })
                .collect();
            Ok(Some(json!({"device": nk, "updated": ctx.now_ms, "windows": windows})))
        }
        "device_meta" => {
            let names = meta_map(conn, "device_names")?;
            let name = names.get(nk).and_then(Value::as_str).map(str::to_string).or_else(|| (nk == ctx.device_id).then(|| ctx.device_name.to_string()));
            Ok(name.map(|n| json!({"device": nk, "name": n})))
        }
        _ => Ok(None),
    }
}

/// The parts of a record that must match for "nothing to push" (a tabs record differs only by
/// its time stamp otherwise).
pub fn comparable(c: &str, data: &Value) -> Value {
    let mut d = data.clone();
    if c == "tabs" {
        if let Some(o) = d.as_object_mut() {
            o.remove("updated");
        }
    }
    d
}

fn meta_map(conn: &Connection, key: &str) -> DbResult<serde_json::Map<String, Value>> {
    Ok(local::meta_get(conn, key)?
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default())
}

fn set_meta_map(conn: &Connection, key: &str, m: &serde_json::Map<String, Value>) -> DbResult<()> {
    local::meta_set(conn, key, &Value::Object(m.clone()).to_string())
}

/// The names other devices gave themselves (or were renamed to), by device id.
pub fn device_names(conn: &Connection) -> DbResult<BTreeMap<String, String>> {
    Ok(meta_map(conn, "device_names")?
        .into_iter()
        .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string())))
        .collect())
}

/// Rename a device (krunk://sync): kept and queued as its `device_meta` record.
pub fn rename_device(conn: &Connection, device_id: &str, name: &str, now_ms: i64) -> DbResult<()> {
    let mut m = meta_map(conn, "device_names")?;
    m.insert(device_id.into(), Value::String(name.chars().take(64).collect()));
    set_meta_map(conn, "device_names", &m)?;
    local::enqueue(conn, "device_meta", device_id, now_ms)
}

/// The open tabs of the other devices, as their `tabs` records said.
pub fn remote_tabs(conn: &Connection) -> DbResult<BTreeMap<String, Value>> {
    Ok(meta_map(conn, "remote_tabs")?.into_iter().collect())
}

/// A setting's record data (`{key, value}`) that holds the built-in default. One that no device
/// has a record of yet is not pushed in that state: it says nothing the user chose, and a fresh
/// device syncing first would otherwise reset another device's choices (found on the VM, 0.9.0).
pub fn is_default_setting(data: &Value) -> bool {
    thread_local! {
        static DEFAULTS: serde_json::Map<String, Value> =
            serde_json::to_value(Settings::default()).ok().and_then(|v| v.as_object().cloned()).unwrap_or_default();
    }
    let (Some(k), Some(v)) = (data.get("key").and_then(Value::as_str), data.get("value")) else { return false };
    DEFAULTS.with(|d| d.get(k) == Some(v))
}

fn settings_json(conn: &Connection) -> DbResult<serde_json::Map<String, Value>> {
    let raw: Option<String> =
        conn.query_row("SELECT value FROM kv WHERE key = 'settings'", [], |r| r.get(0)).optional()?;
    let s = raw.map(|r| Settings::from_json_lenient(&r)).unwrap_or_default();
    Ok(serde_json::to_value(&s).ok().and_then(|v| v.as_object().cloned()).unwrap_or_default())
}

// ---- Bookmarks ---------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Bm {
    id: i64,
    parent: Option<i64>,
    is_folder: bool,
    title: String,
    url: Option<String>,
    position: i64,
    added: i64,
    modified: i64,
    guid: String,
}

fn bm_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<Bm> {
    let guid: Vec<u8> = r.get(8)?;
    Ok(Bm {
        id: r.get(0)?,
        parent: r.get(1)?,
        is_folder: r.get::<_, i64>(2)? != 0,
        title: r.get(3)?,
        url: r.get(4)?,
        position: r.get(5)?,
        added: r.get(6)?,
        modified: r.get(7)?,
        guid: hex(&guid),
    })
}

const BM_COLS: &str = "id, parent, is_folder, title, url, position, added, modified, guid";

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    s.len()
        .is_multiple_of(2)
        .then(|| (0..s.len()).step_by(2).filter_map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect::<Vec<_>>())
        .filter(|v| v.len() * 2 == s.len())
}

fn is_root_guid(g: &str) -> bool {
    g == hex(&ROOT_GUID_BAR) || g == hex(&ROOT_GUID_OTHER)
}

fn bm_by_guid(conn: &Connection, guid: &str) -> DbResult<Option<Bm>> {
    let Some(raw) = unhex(guid) else { return Ok(None) };
    conn.query_row(&format!("SELECT {BM_COLS} FROM bookmarks WHERE guid = ?1"), [raw], bm_from).optional()
}

fn bm_by_id(conn: &Connection, id: i64) -> DbResult<Option<Bm>> {
    conn.query_row(&format!("SELECT {BM_COLS} FROM bookmarks WHERE id = ?1"), [id], bm_from).optional()
}

fn children(conn: &Connection, parent: i64) -> DbResult<Vec<Bm>> {
    let mut st = conn.prepare(&format!("SELECT {BM_COLS} FROM bookmarks WHERE parent = ?1 ORDER BY position, id"))?;
    let r = st.query_map([parent], bm_from)?;
    r.collect()
}

fn all_bookmarks(conn: &Connection) -> DbResult<Vec<Bm>> {
    let mut st = conn.prepare(&format!("SELECT {BM_COLS} FROM bookmarks"))?;
    let r = st.query_map([], bm_from)?;
    r.collect()
}

fn extra_str(it: Option<&SyncItem>, path: &[&str]) -> Option<String> {
    let mut v = &it?.extra;
    for p in path {
        v = v.get(p)?;
    }
    v.as_str().map(str::to_string)
}

fn extra_u64(it: Option<&SyncItem>, path: &[&str]) -> u64 {
    let mut v = match it {
        Some(i) => &i.extra,
        None => return 0,
    };
    for p in path {
        match v.get(p) {
            Some(x) => v = x,
            None => return 0,
        }
    }
    v.as_u64().unwrap_or(0)
}

/// Gives every child of `parent` a sort key consistent with the local order (existing keys are
/// kept where they still fit); writes them into the items' `extra.sort` and queues the children
/// whose key changed. Run before a push.
pub fn prepare_folder(conn: &Connection, parent: i64, now_ms: i64) -> DbResult<()> {
    let kids = children(conn, parent)?;
    let known: Vec<Option<String>> = kids
        .iter()
        .map(|k| {
            local::item(conn, "bookmarks", &k.guid)
                .map(|i| extra_str(i.as_ref(), &["sort"]).filter(|s| fracidx::valid(s)))
        })
        .collect::<DbResult<_>>()?;
    // Keep a known key if it is above the last kept one and below the next known one.
    let mut keep = vec![false; kids.len()];
    let mut last: Option<String> = None;
    for i in 0..kids.len() {
        if let Some(k) = &known[i] {
            let above = last.as_ref().is_none_or(|l| k > l);
            if above {
                keep[i] = true;
                last = Some(k.clone());
            }
        }
    }
    let mut prev: Option<String> = None;
    for i in 0..kids.len() {
        let sort = if keep[i] {
            known[i].clone().unwrap_or_default()
        } else {
            let next = (i + 1..kids.len()).find(|j| keep[*j]).and_then(|j| known[j].clone());
            fracidx::between(prev.as_deref(), next.as_deref())
        };
        let mut it = local::item(conn, "bookmarks", &kids[i].guid)?.unwrap_or_default();
        if extra_str(Some(&it), &["sort"]).as_deref() != Some(sort.as_str()) {
            if !it.extra.is_object() {
                it.extra = json!({});
            }
            it.extra["sort"] = Value::String(sort.clone());
            local::put_item(conn, "bookmarks", &kids[i].guid, &it)?;
            local::enqueue(conn, "bookmarks", &kids[i].guid, now_ms)?;
        }
        prev = Some(sort);
    }
    Ok(())
}

fn bookmark_data(conn: &Connection, nk: &str, item: Option<&SyncItem>) -> DbResult<Option<Value>> {
    if is_root_guid(nk) {
        return Ok(None);
    }
    let Some(b) = bm_by_guid(conn, nk)? else { return Ok(None) };
    let parent_guid = match b.parent {
        Some(p) => bm_by_id(conn, p)?.map(|x| x.guid).unwrap_or_default(),
        None => return Ok(None),
    };
    let sort = extra_str(item, &["sort"]).unwrap_or_else(|| fracidx::between(None, None));
    let synced = item.and_then(|i| i.extra.get("data")).cloned().unwrap_or(Value::Null);
    let same = |k: &str, v: &Value| synced.get(k) == Some(v);
    let pos_same = same("parent_guid", &json!(parent_guid)) && same("sort", &json!(sort));
    let text_same = same("title", &json!(b.title)) && same("url", &json!(b.url));
    let edit = hlc::encode(b.modified.max(b.added).max(0) as u64, 0);
    let v_pos =
        if pos_same { extra_u64(item, &["data", "v_pos"]) } else { edit.max(extra_u64(item, &["data", "v_pos"]) + 1) };
    let v_text = if text_same {
        extra_u64(item, &["data", "v_text"])
    } else {
        edit.max(extra_u64(item, &["data", "v_text"]) + 1)
    };
    Ok(Some(json!({
        "guid": nk, "parent_guid": parent_guid, "kind": if b.is_folder { "folder" } else { "bookmark" },
        "title": b.title, "url": b.url, "sort": sort, "added": b.added, "modified": b.modified,
        "v_pos": v_pos, "v_text": v_text,
    })))
}

/// The first sync of a device: a local bookmark that isn't synced yet and looks the same as a
/// remote one (same parent, kind, url and title) becomes that one (its guid is adopted).
fn adopt_match(conn: &Connection, data: &Value) -> DbResult<Option<Bm>> {
    let parent = data["parent_guid"].as_str().unwrap_or_default();
    let Some(p) = bm_by_guid(conn, parent)? else { return Ok(None) };
    let folder = data["kind"] == "folder";
    for k in children(conn, p.id)? {
        let synced = local::item(conn, "bookmarks", &k.guid)?.is_some_and(|i| i.server_seq > 0);
        if !synced
            && k.is_folder == folder
            && k.title == data["title"].as_str().unwrap_or_default()
            && k.url.as_deref() == data["url"].as_str()
        {
            let remote = data["guid"].as_str().and_then(unhex).unwrap_or_default();
            conn.execute("UPDATE bookmarks SET guid = ?2 WHERE id = ?1", params![k.id, remote])?;
            conn.execute(
                "DELETE FROM sync_items WHERE collection = 'bookmarks' AND natural_key = ?1",
                [k.guid.as_str()],
            )?;
            conn.execute(
                "DELETE FROM sync_queue WHERE collection = 'bookmarks' AND natural_key = ?1",
                [k.guid.as_str()],
            )?;
            return bm_by_id(conn, k.id);
        }
    }
    Ok(None)
}

fn apply_bookmark(
    conn: &Connection,
    nk: &str,
    r: &RecordPlain,
    item: Option<&SyncItem>,
    ctx: &Ctx,
    ch: &mut Changed,
) -> DbResult<Value> {
    let mut extra = item.map(|i| i.extra.clone()).filter(Value::is_object).unwrap_or_else(|| json!({}));
    if is_root_guid(nk) {
        return Ok(extra);
    }
    let local = bm_by_guid(conn, nk)?;
    let pending = queued_at(conn, "bookmarks", nk)?;
    if r.deleted {
        if let Some(b) = &local {
            let keep = pending.is_some() && !remote_wins(r, b.modified, ctx.dev_short);
            if !keep {
                // Children that aren't deleted too go to "Other bookmarks" (tree repair places them).
                conn.execute("UPDATE bookmarks SET parent = ?2 WHERE parent = ?1", params![b.id, OTHER_BOOKMARKS])?;
                conn.execute("DELETE FROM bookmarks WHERE id = ?1", [b.id])?;
                local::dequeue(conn, "bookmarks", nk)?;
                ch.bookmarks = true;
            }
        }
        extra["data"] = Value::Null;
        return Ok(extra);
    }
    let d = &r.data;
    let folder = d["kind"] == "folder";
    let url = d["url"].as_str().map(str::to_string);
    if !folder && !url.as_deref().is_some_and(crate::bookmarks::bookmarkable) {
        return Ok(extra);
    }
    let title: String = d["title"].as_str().unwrap_or_default().chars().take(1024).collect();
    let sort = d["sort"].as_str().filter(|s| fracidx::valid(s)).unwrap_or("V").to_string();
    let parent = match d["parent_guid"].as_str() {
        Some(g) => bm_by_guid(conn, g)?.filter(|p| p.is_folder).map(|p| p.id),
        None => None,
    }
    .unwrap_or(OTHER_BOOKMARKS);
    let local = match (local, ctx.initial) {
        (None, true) => adopt_match(conn, d)?,
        (l, _) => l,
    };
    match local {
        None => {
            let guid = unhex(nk).unwrap_or_default();
            let pos: i64 = conn.query_row(
                "SELECT coalesce(max(position), -1) + 1 FROM bookmarks WHERE parent = ?1",
                [parent],
                |x| x.get(0),
            )?;
            conn.execute(
                "INSERT INTO bookmarks(parent, is_folder, title, url, position, added, guid, modified) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![parent, folder, title, if folder { None } else { url }, pos, d["added"].as_i64().unwrap_or(ctx.now_ms), guid, d["modified"].as_i64().unwrap_or(ctx.now_ms)],
            )?;
            extra["sort"] = Value::String(sort);
            ch.bookmarks = true;
        }
        Some(b) => {
            let synced = extra.get("data").cloned().unwrap_or(Value::Null);
            let local_pos_changed = pending.is_some()
                && extra.get("sort").and_then(Value::as_str) != synced.get("sort").and_then(Value::as_str)
                || pending.is_some()
                    && b.parent.and_then(|p| bm_by_id(conn, p).ok().flatten()).map(|p| p.guid)
                        != synced.get("parent_guid").and_then(Value::as_str).map(str::to_string);
            let local_text_changed = pending.is_some()
                && (synced.get("title").and_then(Value::as_str) != Some(b.title.as_str())
                    || synced.get("url").and_then(Value::as_str) != b.url.as_deref());
            let local_ver = hlc::encode(b.modified.max(0) as u64, 0);
            let wins = |remote_v: u64| remote_v > local_ver || (remote_v == local_ver && r.dev > ctx.dev_short);
            if !local_pos_changed || wins(d["v_pos"].as_u64().unwrap_or(r.ver)) {
                if b.parent != Some(parent) {
                    conn.execute("UPDATE bookmarks SET parent = ?2 WHERE id = ?1", params![b.id, parent])?;
                }
                extra["sort"] = Value::String(sort);
                ch.bookmarks = true;
            }
            if (!local_text_changed || wins(d["v_text"].as_u64().unwrap_or(r.ver)))
                && (b.title != title || (!b.is_folder && b.url != url))
            {
                conn.execute(
                    "UPDATE bookmarks SET title = ?2, url = ?3 WHERE id = ?1",
                    params![b.id, title, if b.is_folder { None } else { url }],
                )?;
                ch.bookmarks = true;
            }
        }
    }
    extra["data"] = d.clone();
    Ok(extra)
}

/// The tree repair of PROTOCOL §9.5, deterministic on every device: each synced item goes under
/// the parent its record names when that folder exists (otherwise "Other bookmarks"), items with
/// a pending local move stay where the user put them, a cycle is broken by moving its lowest guid
/// to "Other bookmarks", and siblings are ordered by (`sort`, `guid`).
pub fn repair_tree(conn: &Connection) -> DbResult<()> {
    let all = all_bookmarks(conn)?;
    let by_guid: HashMap<String, &Bm> = all.iter().map(|b| (b.guid.clone(), b)).collect();
    let by_id: HashMap<i64, &Bm> = all.iter().map(|b| (b.id, b)).collect();
    let queued: HashSet<String> =
        local::queue(conn, usize::MAX >> 1)?.into_iter().filter(|(c, _)| c == "bookmarks").map(|(_, k)| k).collect();
    let mut sorts: HashMap<i64, String> = HashMap::new();
    let mut desired: HashMap<i64, i64> = HashMap::new();
    for b in &all {
        let Some(cur) = b.parent else { continue }; // a root
        let it = local::item(conn, "bookmarks", &b.guid)?;
        if let Some(s) = extra_str(it.as_ref(), &["sort"]) {
            sorts.insert(b.id, s);
        }
        let want = match extra_str(it.as_ref(), &["data", "parent_guid"]) {
            Some(g) if !queued.contains(&b.guid) => match by_guid.get(&g) {
                Some(p) if p.is_folder && p.id != b.id => p.id,
                _ => OTHER_BOOKMARKS,
            },
            _ => cur,
        };
        desired.insert(b.id, want);
    }
    // Break cycles: follow parents; a walk that comes back moves its lowest guid out.
    loop {
        let mut broke = false;
        let mut done: HashSet<i64> = HashSet::new();
        for start in desired.keys().copied().collect::<Vec<_>>() {
            if done.contains(&start) {
                continue;
            }
            let mut path = Vec::new();
            let mut seen = HashSet::new();
            let mut cur = start;
            while let Some(p) = desired.get(&cur).copied() {
                if !seen.insert(cur) {
                    let cycle: Vec<i64> = path.iter().copied().skip_while(|x| *x != cur).collect();
                    if let Some(low) =
                        cycle.iter().min_by_key(|id| by_id.get(id).map(|b| b.guid.clone()).unwrap_or_default())
                    {
                        desired.insert(*low, OTHER_BOOKMARKS);
                        broke = true;
                    }
                    break;
                }
                path.push(cur);
                cur = p;
            }
            done.extend(path);
        }
        if !broke {
            break;
        }
    }
    for b in &all {
        if let Some(want) = desired.get(&b.id) {
            if b.parent != Some(*want) {
                conn.execute("UPDATE bookmarks SET parent = ?2 WHERE id = ?1", params![b.id, want])?;
            }
        }
    }
    // Order every folder by (sort, guid); items without a key keep their place after those.
    let mut kids: HashMap<i64, Vec<&Bm>> = HashMap::new();
    for b in &all {
        if let Some(p) = desired.get(&b.id).copied().or(b.parent) {
            kids.entry(p).or_default().push(b);
        }
    }
    for list in kids.values_mut() {
        list.sort_by(|x, y| {
            let kx = sorts.get(&x.id);
            let ky = sorts.get(&y.id);
            match (kx, ky) {
                (Some(a), Some(b)) => a.cmp(b).then(x.guid.cmp(&y.guid)),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => x.position.cmp(&y.position).then(x.id.cmp(&y.id)),
            }
        });
        for (pos, b) in list.iter().enumerate() {
            if b.position != pos as i64 {
                conn.execute("UPDATE bookmarks SET position = ?2 WHERE id = ?1", params![b.id, pos as i64])?;
            }
        }
    }
    Ok(())
}

// ---- History ------------------------------------------------------------------------------------------

/// Local `urls` rows whose normalized URL is `nk` (the same page can be stored under variants).
/// (id, url, title, visit_count, last_visit)
type UrlRow = (i64, String, String, i64, i64);

fn url_rows(conn: &Connection, nk: &str) -> DbResult<Vec<UrlRow>> {
    let host_end = nk.find("://").map(|i| i + 3).and_then(|s| nk[s..].find('/').map(|j| s + j)).unwrap_or(nk.len());
    let prefix = nk[..host_end].to_ascii_lowercase();
    let mut st = conn.prepare(
        "SELECT id, url, title, visit_count, last_visit FROM urls WHERE url = ?1 OR (lower(url) >= ?2 AND lower(url) < ?3) LIMIT 200",
    )?;
    let rows = st.query_map(params![nk, prefix, format!("{prefix}\u{10ffff}")], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
    })?;
    let rows: Vec<(i64, String, String, i64, i64)> = rows.collect::<DbResult<_>>()?;
    Ok(rows.into_iter().filter(|(_, u, _, _, _)| u == nk || history_key(u).as_deref() == Some(nk)).collect())
}

fn visits_of(conn: &Connection, ids: &[i64]) -> DbResult<Vec<i64>> {
    let mut out = Vec::new();
    for id in ids {
        let mut st = conn.prepare("SELECT ts FROM visits WHERE url_id = ?1 ORDER BY ts DESC LIMIT 20")?;
        let r = st.query_map([id], |r| r.get(0))?;
        out.extend(r.collect::<DbResult<Vec<i64>>>()?);
    }
    Ok(out)
}

fn newest(mut v: Vec<i64>) -> Vec<i64> {
    v.sort_unstable_by(|a, b| b.cmp(a));
    v.dedup();
    v.truncate(VISITS_KEPT);
    v
}

fn history_data(conn: &Connection, nk: &str, item: Option<&SyncItem>) -> DbResult<Option<Value>> {
    let rows = url_rows(conn, nk)?;
    if rows.is_empty() {
        return Ok(None);
    }
    let ids: Vec<i64> = rows.iter().map(|r| r.0).collect();
    let mut visits = visits_of(conn, &ids)?;
    if let Some(prev) = item.and_then(|i| i.extra.get("data")).and_then(|d| d.get("visits")).and_then(Value::as_array) {
        visits.extend(prev.iter().filter_map(Value::as_i64));
    }
    let visits = newest(visits);
    let count: i64 = rows
        .iter()
        .map(|r| r.3)
        .sum::<i64>()
        .max(item.and_then(|i| i.extra.get("data")).and_then(|d| d["count"].as_i64()).unwrap_or(0))
        .max(visits.len() as i64);
    let title = rows.iter().max_by_key(|r| r.4).map(|r| r.2.clone()).unwrap_or_default();
    Ok(Some(json!({"url": nk, "title": title, "visits": visits, "count": count})))
}

fn apply_history(
    conn: &Connection,
    nk: &str,
    r: &RecordPlain,
    item: Option<&SyncItem>,
    ch: &mut Changed,
) -> DbResult<Value> {
    let mut extra = item.map(|i| i.extra.clone()).filter(Value::is_object).unwrap_or_else(|| json!({}));
    let rows = url_rows(conn, nk)?;
    if r.deleted {
        // Deleted elsewhere: gone here too, unless visited here after the deletion.
        let (del_ms, _) = hlc::decode(r.ver);
        let newer = rows.iter().any(|x| x.4 > del_ms as i64);
        if !newer {
            for x in &rows {
                conn.execute("DELETE FROM urls WHERE id = ?1", [x.0])?;
            }
            ch.history |= !rows.is_empty();
        }
        extra["data"] = Value::Null;
        return Ok(extra);
    }
    let d = &r.data;
    let remote_visits: Vec<i64> =
        d["visits"].as_array().map(|a| a.iter().filter_map(Value::as_i64).collect()).unwrap_or_default();
    let title: String = d["title"].as_str().unwrap_or_default().chars().take(1024).collect();
    let (id, existing): (i64, Vec<i64>) = match rows.iter().find(|x| x.1 == nk).or(rows.first()) {
        Some(x) => (x.0, visits_of(conn, &[x.0])?),
        None => {
            conn.execute(
                "INSERT INTO urls(url, title, visit_count, last_visit) VALUES (?1, ?2, 0, 0)",
                params![nk, title],
            )?;
            (conn.last_insert_rowid(), vec![])
        }
    };
    let have: HashSet<i64> = existing.iter().copied().collect();
    let mut added = 0;
    for ts in remote_visits.iter().filter(|t| !have.contains(t)) {
        conn.execute("INSERT INTO visits(url_id, ts) VALUES (?1, ?2)", params![id, ts])?;
        added += 1;
    }
    let last = remote_visits.iter().chain(existing.iter()).copied().max().unwrap_or(0);
    let local_last = existing.iter().copied().max().unwrap_or(0);
    let count = d["count"].as_i64().unwrap_or(0);
    conn.execute(
        "UPDATE urls SET visit_count = max(visit_count, ?2), last_visit = max(last_visit, ?3),
                title = CASE WHEN ?4 THEN ?5 ELSE title END WHERE id = ?1",
        params![id, count, last, last > local_last || local_last == 0, title],
    )?;
    ch.history |= added > 0;
    extra["data"] = d.clone();
    Ok(extra)
}

// ---- Settings, pins, allowlist, tabs, device names ------------------------------------------------------

fn apply_setting(
    conn: &Connection,
    nk: &str,
    r: &RecordPlain,
    item: Option<&SyncItem>,
    ctx: &Ctx,
    ch: &mut Changed,
) -> DbResult<Value> {
    let mut extra = item.map(|i| i.extra.clone()).filter(Value::is_object).unwrap_or_else(|| json!({}));
    if r.deleted || !SYNCED_SETTINGS.contains(&nk) {
        return Ok(extra);
    }
    let value = r.data.get("value").cloned().unwrap_or(Value::Null);
    let mut cur = settings_json(conn)?;
    let pending = queued_at(conn, "settings", "*")?;
    let local_changed = extra.get("data").and_then(|d| d.get("value")) != cur.get(nk);
    if let (Some(t), true) = (pending, local_changed) {
        if !remote_wins(r, t, ctx.dev_short) {
            extra["data"] = r.data.clone();
            return Ok(extra);
        }
    }
    if cur.get(nk) != Some(&value) {
        cur.insert(nk.into(), value);
        let json = Value::Object(cur).to_string();
        let s = Settings::from_json_lenient(&json);
        if s.validate().is_ok() {
            let out = serde_json::to_string(&s).unwrap_or(json);
            conn.execute("INSERT INTO kv(key, value) VALUES ('settings', ?1) ON CONFLICT(key) DO UPDATE SET value = excluded.value", [out])?;
            ch.settings = true;
        }
    }
    extra["data"] = r.data.clone();
    Ok(extra)
}

fn apply_pin(
    conn: &Connection,
    nk: &str,
    r: &RecordPlain,
    item: Option<&SyncItem>,
    ctx: &Ctx,
    ch: &mut Changed,
) -> DbResult<Value> {
    let mut extra = item.map(|i| i.extra.clone()).filter(Value::is_object).unwrap_or_else(|| json!({}));
    if let Some(t) = queued_at(conn, "pins", nk)? {
        if !remote_wins(r, t, ctx.dev_short) {
            extra["data"] = r.data.clone();
            return Ok(extra);
        }
    }
    if r.deleted {
        ch.pins |= conn.execute("DELETE FROM tiles WHERE url = ?1", [nk])? > 0;
        extra["data"] = Value::Null;
        return Ok(extra);
    }
    let d = &r.data;
    let state = if d["state"] == "pinned" { 1 } else { 2 };
    let pos: i64 = d["sort"].as_str().and_then(|s| s.parse().ok()).unwrap_or(999);
    conn.execute(
        "INSERT INTO tiles(url, title, state, position) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(url) DO UPDATE SET title = excluded.title, state = excluded.state, position = excluded.position",
        params![nk, d["title"].as_str().unwrap_or_default(), state, pos],
    )?;
    ch.pins = true;
    extra["data"] = d.clone();
    Ok(extra)
}

fn apply_allow(
    conn: &Connection,
    nk: &str,
    r: &RecordPlain,
    item: Option<&SyncItem>,
    ctx: &Ctx,
    ch: &mut Changed,
) -> DbResult<Value> {
    let mut extra = item.map(|i| i.extra.clone()).filter(Value::is_object).unwrap_or_else(|| json!({}));
    if let Some(t) = queued_at(conn, "allowlist", nk)? {
        if !remote_wins(r, t, ctx.dev_short) {
            extra["data"] = r.data.clone();
            return Ok(extra);
        }
    }
    if r.deleted {
        ch.allowlist |= conn.execute("DELETE FROM adblock_allowlist WHERE host = ?1", [nk])? > 0;
        extra["data"] = Value::Null;
    } else if let Some(host) = crate::blocker::normalize_host(nk) {
        ch.allowlist |= conn.execute(
            "INSERT OR IGNORE INTO adblock_allowlist(host, added) VALUES (?1, ?2)",
            params![host, r.data["added"].as_i64().unwrap_or(ctx.now_ms)],
        )? > 0;
        extra["data"] = r.data.clone();
    }
    Ok(extra)
}

fn apply_meta_map(conn: &Connection, key: &str, nk: &str, r: &RecordPlain, value: Option<Value>) -> DbResult<bool> {
    let mut m = meta_map(conn, key)?;
    let before = m.get(nk).cloned();
    match (r.deleted, value) {
        (false, Some(v)) => {
            m.insert(nk.into(), v);
        }
        _ => {
            m.remove(nk);
        }
    }
    let changed = m.get(nk) != before.as_ref();
    set_meta_map(conn, key, &m)?;
    Ok(changed)
}

/// Merges one remote record into local state. Returns the item's new `extra`.
pub fn apply(
    conn: &Connection,
    c: &str,
    nk: &str,
    r: &RecordPlain,
    item: Option<&SyncItem>,
    ctx: &Ctx,
    ch: &mut Changed,
) -> DbResult<Value> {
    match c {
        "bookmarks" => apply_bookmark(conn, nk, r, item, ctx, ch),
        "history" => apply_history(conn, nk, r, item, ch),
        "settings" => apply_setting(conn, nk, r, item, ctx, ch),
        "pins" => apply_pin(conn, nk, r, item, ctx, ch),
        "allowlist" => apply_allow(conn, nk, r, item, ctx, ch),
        "tabs" => {
            // Each device writes only its own; others are shown read-only.
            if nk != ctx.device_id {
                ch.tabs |= apply_meta_map(conn, "remote_tabs", nk, r, Some(r.data.clone()))?;
            }
            Ok(json!({"data": r.data}))
        }
        "device_meta" => {
            let name = r.data.get("name").and_then(Value::as_str).map(|s| Value::String(s.chars().take(64).collect()));
            if nk != ctx.device_id || queued_at(conn, "device_meta", nk)?.is_none() {
                ch.devices |= apply_meta_map(conn, "device_names", nk, r, name)?;
            }
            Ok(json!({"data": r.data}))
        }
        _ => Ok(Value::Null),
    }
}

/// Everything local that a first sync (or a full reconcile) offers, queued (PROTOCOL §10.5):
/// all bookmarks, folders before their contents; the last 90 days of history, at most 20,000
/// URLs; every synced setting; pins; the allowlist; this device's tabs and name.
pub fn enqueue_all(conn: &Connection, ctx: &Ctx) -> DbResult<()> {
    let t = ctx.now_ms;
    local::enqueue(conn, "device_meta", ctx.device_id, t)?;
    local::enqueue(conn, "settings", "*", t)?;
    let mut st = conn.prepare("SELECT host FROM adblock_allowlist")?;
    for h in st.query_map([], |r| r.get::<_, String>(0))? {
        local::enqueue(conn, "allowlist", &h?, t)?;
    }
    let mut st = conn.prepare("SELECT url FROM tiles")?;
    for u in st.query_map([], |r| r.get::<_, String>(0))? {
        local::enqueue(conn, "pins", &u?, t)?;
    }
    // Bookmarks breadth first (a parent is pushed before its children).
    let mut level = vec![crate::storage::BOOKMARKS_BAR, OTHER_BOOKMARKS];
    while !level.is_empty() {
        let mut next = Vec::new();
        for p in level {
            prepare_folder(conn, p, t)?;
            for k in children(conn, p)? {
                local::enqueue(conn, "bookmarks", &k.guid, t)?;
                if k.is_folder {
                    next.push(k.id);
                }
            }
        }
        level = next;
    }
    let since = t - HISTORY_FIRST_DAYS * 86_400_000;
    let mut st = conn.prepare("SELECT url FROM urls WHERE last_visit >= ?1 ORDER BY last_visit DESC LIMIT ?2")?;
    for u in st.query_map(params![since, HISTORY_MAX as i64], |r| r.get::<_, String>(0))? {
        local::enqueue(conn, "history", &u?, t)?;
    }
    local::enqueue(conn, "tabs", "*", t)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_setting_is_either_synced_or_device_local() {
        // A new setting must be sorted into one of the two lists (PLAN §5.5); this fails otherwise.
        const LOCAL: &[&str] = &[
            // 0.12.0: the AI agent (providers are per device; keys are never in settings),
            // userscripts and macros are local only.
            "ai",
            "macros",
            "userscripts",
            "appearance",
            "autoPip",
            "blockThirdPartyCookies",
            "checkUpdatesAutomatically",
            "devtoolsDock",
            "devtoolsSizeBottom",
            "devtoolsSizeRight",
            "dns",
            "downloadConnections",
            "downloadDir",
            "extraFlags",
            "filterListsRev",
            "hardwareAcceleration",
            "headers",
            "linkTabs",
            "newTabPage",
            "newTabPosition",
            "newTabUrl",
            "ntp",
            "ntpSearchUrl",
            "offerSaveCards",
            "passgen",
            "permissions",
            "protectedContent",
            "proxy",
            "tabDiscardEnabled",
            "tabDiscardMinutes",
            "tabSleepMinutes",
            "userAgent",
            // 0.11.0: how this device's unlock lasts is this device's choice.
            "vaultRemainUnlocked",
            // 0.11.3: PDFs and email links depend on this device's apps.
            "pdfInBrowser",
            "mailHandler",
            "mailCustomUrl",
            // 0.14.0: the VPN (its profiles are in this device's database only).
            "vpn",
        ];
        let v = serde_json::to_value(Settings::default()).unwrap();
        let unsorted: Vec<&String> = v
            .as_object()
            .unwrap()
            .keys()
            .filter(|k| !SYNCED_SETTINGS.contains(&k.as_str()) && !LOCAL.contains(&k.as_str()))
            .collect();
        assert!(unsorted.is_empty(), "settings in neither list: {unsorted:?}");
        for s in SYNCED_SETTINGS {
            assert!(v.get(s).is_some(), "{s} is not a setting");
        }
    }

    #[test]
    fn natural_keys() {
        let g = "00112233445566778899aabbccddeeff";
        assert_eq!(natural_key("bookmarks", &json!({"guid": g})).as_deref(), Some(g));
        assert_eq!(natural_key("bookmarks", &json!({"guid": g.to_uppercase()})), None);
        assert_eq!(
            natural_key("history", &json!({"url": "HTTPS://Example.com/a?utm_source=x"})).as_deref(),
            Some("https://example.com/a")
        );
        assert_eq!(natural_key("settings", &json!({"key": "proxy"})), None, "a device-local setting never travels");
        assert_eq!(natural_key("settings", &json!({"key": "homepage"})).as_deref(), Some("homepage"));
        assert_eq!(natural_key("tabs", &json!({"device": "d1"})).as_deref(), Some("d1"));
    }
}
