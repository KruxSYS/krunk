//! The downloads list (state only; the host drives it from the engine's
//! download events) and the helpers for naming downloaded files.
//!
//! 0.15.0: the list is a table (`download_items`, storage V9) with one row per download that
//! isn't private. Running and paused downloads (and private ones, never saved) are also kept
//! in memory (`Downloads`); a row is written when a download starts, changes state or saves
//! where it stands, never per progress update. Finished downloads leave memory at the next
//! save and live only in the table, which krunk://downloads reads in pages (`Store::downloads_query`).

use crate::storage::{like_pattern, DbError, DbResult, Store};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Why a download stopped when the browser closed (a download in parts can go on; an engine
/// download is retried).
pub const PAUSED_BY_CLOSE: &str = "Paused when Krunk closed";
pub const INTERRUPTED_BY_CLOSE: &str = "Browser closed";

/// 0.15.0 (storage V9): every download that isn't private. `search` is the lower-case name and
/// address; the one index gives the newest-first order and covers the filters and the search,
/// so a page only reads the rows it shows.
pub(crate) const SCHEMA_V9: &str = "
CREATE TABLE IF NOT EXISTS download_items (
    id      INTEGER PRIMARY KEY,
    started INTEGER NOT NULL,
    ended   INTEGER NOT NULL,
    state   TEXT NOT NULL,
    search  TEXT NOT NULL,
    json    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS download_items_order ON download_items(started DESC, id DESC, state, search);
";

/// Moves the old `downloads` kv record (the whole list as JSON, up to 0.14) into
/// `download_items` as it was (the next start restores running ones as after any close), then
/// deletes the record. Private downloads were never in it; they are skipped all the same.
pub(crate) fn upgrade_v9(tx: &rusqlite::Transaction) -> DbResult<()> {
    let old: Option<String> =
        tx.query_row("SELECT value FROM kv WHERE key = 'downloads'", [], |r| r.get(0)).optional()?;
    let Some(json) = old else { return Ok(()) };
    let list: Downloads = serde_json::from_str(&json).unwrap_or_default();
    for item in list.items.iter().filter(|i| !i.private) {
        write_row(tx, item)?;
    }
    tx.execute("DELETE FROM kv WHERE key = 'downloads'", [])?;
    Ok(())
}

/// Rows kept in the table (0.15.0; the oldest finished ones go first, at start).
pub const MAX_ITEMS: usize = 100_000;
/// Downloads kept in memory: running ones, and finished private ones of this session (the
/// oldest finished go first beyond this).
const MAX_MEMORY_ITEMS: usize = 1000;
/// The states of a running (or paused) download, as stored.
const RUNNING_SQL: &str = "state IN ('inProgress', 'paused')";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadState {
    InProgress,
    Paused,
    Completed,
    Interrupted,
    Cancelled,
}

impl DownloadState {
    pub fn is_finished(self) -> bool {
        !matches!(self, DownloadState::InProgress | DownloadState::Paused)
    }

    /// As in the JSON and the table's `state` column.
    pub fn as_str(self) -> &'static str {
        match self {
            DownloadState::InProgress => "inProgress",
            DownloadState::Paused => "paused",
            DownloadState::Completed => "completed",
            DownloadState::Interrupted => "interrupted",
            DownloadState::Cancelled => "cancelled",
        }
    }
}

/// The downloads page's filter chips (0.15.0).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadFilter {
    #[default]
    All,
    /// Running or paused.
    Active,
    /// Completed.
    Done,
    /// Failed or cancelled.
    Failed,
}

impl DownloadFilter {
    pub fn matches(self, state: DownloadState) -> bool {
        match self {
            DownloadFilter::All => true,
            DownloadFilter::Active => !state.is_finished(),
            DownloadFilter::Done => state == DownloadState::Completed,
            DownloadFilter::Failed => matches!(state, DownloadState::Interrupted | DownloadState::Cancelled),
        }
    }

    fn sql(self) -> &'static str {
        match self {
            DownloadFilter::All => "1",
            DownloadFilter::Active => RUNNING_SQL,
            DownloadFilter::Done => "state = 'completed'",
            DownloadFilter::Failed => "state IN ('interrupted', 'cancelled')",
        }
    }
}

/// A page of the list (`QueryDownloads`): newest first, by search text and filter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DownloadQuery {
    pub text: String,
    pub filter: DownloadFilter,
    pub offset: u32,
    pub limit: u32,
    /// Count too (`DownloadPage::counts`): the page asks when its list changed, not per page.
    pub counts: bool,
    /// Unix ms (the page's local midnights, today's first): `counts.since` has, for each, how
    /// many matching downloads started at or after it (the Today / Yesterday groups).
    pub since: Vec<i64>,
}

/// Most rows one query returns.
pub const MAX_PAGE: u32 = 500;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadPage {
    pub offset: u32,
    pub items: Vec<DownloadItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counts: Option<DownloadCounts>,
}

/// Counts for the search text: per chip, and `filtered` for the chip shown.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadCounts {
    pub all: u32,
    pub active: u32,
    pub done: u32,
    pub failed: u32,
    pub filtered: u32,
    pub since: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadItem {
    pub id: u32,
    pub url: String,
    pub file_name: String,
    pub path: String,
    pub state: DownloadState,
    pub received: i64,
    /// Total size if the server sent one.
    pub total: Option<i64>,
    /// Unix ms.
    pub started: i64,
    /// Why the download was interrupted, if it was.
    pub reason: Option<String>,
    /// Started from a private tab: shown this session, never saved.
    #[serde(default)]
    pub private: bool,
    /// When it completed, failed or was cancelled (unix ms); none while running and for
    /// downloads saved before 0.4.
    #[serde(default)]
    pub finished: Option<i64>,
    /// The page it was downloaded from, if known.
    #[serde(default)]
    pub page_url: Option<String>,
    /// Fetched in parts over this many connections (see `segdl`); none for the engine's
    /// own single download.
    #[serde(default)]
    pub connections: Option<u32>,
    /// The part files of a download fetched in parts, while it runs: left over after a crash
    /// or quit, they are deleted at the next start.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<String>,
    /// Smoothed speed in bytes per second while it runs (Krunk 0.6.0; 0 when done).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub speed: u64,
    /// Seconds left at that speed, when the total is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eta: Option<u64>,
    /// Per connection of a download in parts: bytes done and the part's length (one bar
    /// segment each); empty for a single connection or when done.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub part_progress: Vec<PartProgress>,
    /// A download in parts that can go on after a restart (0.7.0): what is left of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<Resume>,
}

/// Where a download in parts stands, saved while it runs (0.7.0, R9): the file being written
/// (all of it allocated; the finished ranges are in place), what identifies the server's
/// version of the file, and the ranges still missing. The cookies are asked from the engine
/// again on resume, never saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resume {
    pub temp: String,
    pub total: u64,
    #[serde(default)]
    pub etag: Option<String>,
    #[serde(default)]
    pub last_modified: Option<String>,
    #[serde(default)]
    pub referer: Option<String>,
    pub segments: Vec<crate::segdl::Segment>,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PartProgress {
    pub done: u64,
    pub len: u64,
    /// Where the part starts in the file and which connection fetches it (0.7.0: parts are
    /// split while they run, so the view places each one at its offset).
    #[serde(default)]
    pub start: u64,
    #[serde(default)]
    pub conn: u32,
}

/// A download's speed: an exponential moving average over the samples of the last seconds,
/// so the number is steady but follows a change within a few seconds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Meter {
    last: Option<(i64, i64)>,
    rate: f64,
}

/// Half-life of the smoothing, ms.
const METER_HALF_LIFE_MS: f64 = 1500.0;

impl Meter {
    /// Records `received` bytes at `now` (unix ms); returns the smoothed bytes per second.
    pub fn sample(&mut self, now: i64, received: i64) -> u64 {
        if let Some((t, r)) = self.last {
            let dt = (now - t) as f64;
            if dt >= 50.0 {
                let inst = ((received - r).max(0) as f64) * 1000.0 / dt;
                let a = 1.0 - 0.5f64.powf(dt / METER_HALF_LIFE_MS);
                self.rate = if self.rate == 0.0 { inst } else { self.rate + a * (inst - self.rate) };
                self.last = Some((now, received));
            } else if received < r {
                self.last = Some((now, received));
            }
        } else {
            self.last = Some((now, received));
        }
        self.rate.max(0.0).round() as u64
    }
}

/// Seconds left at `speed` bytes/s, when the total is known and the speed is not zero.
pub fn eta_secs(speed: u64, received: i64, total: Option<i64>) -> Option<u64> {
    let total = total?;
    if speed == 0 || total <= received {
        return None;
    }
    Some(((total - received) as u64).div_ceil(speed))
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Downloads {
    /// In memory (0.15.0): running and paused downloads, private ones, and finished ones not
    /// saved yet (the oldest first).
    items: Vec<DownloadItem>,
    next_id: u32,
    /// Speed meters of the running downloads (never saved).
    #[serde(skip)]
    meters: std::collections::HashMap<u32, Meter>,
    /// 0.15.0: rows to write at the next save (`Store::downloads_save`).
    #[serde(skip)]
    dirty: BTreeSet<u32>,
    /// Rows to delete at the next save.
    #[serde(skip)]
    deleted: BTreeSet<u32>,
    /// Every finished row goes at the next save.
    #[serde(skip)]
    clear_stored: bool,
    /// Running downloads whose progress moved since the last push.
    #[serde(skip)]
    moved: BTreeSet<u32>,
    /// Something but progress changed since the last push (pages fetch their window again).
    #[serde(skip)]
    list_changed: bool,
}

impl Downloads {
    pub fn new() -> Downloads {
        Downloads { next_id: 1, ..Default::default() }
    }

    /// Restores a saved list; downloads that were running when the browser exited are marked
    /// interrupted, except downloads in parts that saved where they stood (0.7.0): those are
    /// paused, to go on with Resume.
    pub fn from_json(json: &str) -> Downloads {
        let mut d: Downloads = serde_json::from_str(json).unwrap_or_default();
        d.next_id = d.items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
        d.restore();
        d
    }

    /// After a start: running ones were stopped by the close (see `from_json`); their rows
    /// are written again at the next save.
    fn restore(&mut self) {
        for i in &mut self.items {
            i.part_progress.clear();
            i.speed = 0;
            i.eta = None;
            if i.state.is_finished() {
                continue;
            }
            if i.resume.is_some() {
                i.state = DownloadState::Paused;
                i.reason = Some(PAUSED_BY_CLOSE.into());
            } else {
                i.state = DownloadState::Interrupted;
                i.reason = Some(INTERRUPTED_BY_CLOSE.into());
            }
            self.dirty.insert(i.id);
        }
    }

    /// Running downloads whose progress moved since the last call (the page's live update).
    pub fn take_moved(&mut self) -> Vec<DownloadItem> {
        let moved = std::mem::take(&mut self.moved);
        self.items.iter().filter(|i| moved.contains(&i.id) && !i.state.is_finished()).cloned().collect()
    }

    /// Whether the list changed (beyond progress) since the last call.
    pub fn take_list_changed(&mut self) -> bool {
        std::mem::take(&mut self.list_changed)
    }

    /// The table changed behind the list's back (auto-clear).
    pub fn mark_list_changed(&mut self) {
        self.list_changed = true;
    }

    fn touch(&mut self, id: u32, list: bool) {
        self.dirty.insert(id);
        self.list_changed |= list;
    }

    /// The addresses of the downloads the last close stopped (paused or interrupted at this
    /// start): the engine may start them again by itself right after the start (0.7.0).
    pub fn stopped_by_close(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|i| matches!(i.reason.as_deref(), Some(PAUSED_BY_CLOSE | INTERRUPTED_BY_CLOSE)))
            .map(|i| i.url.clone())
            .collect()
    }

    /// Saves (or forgets) where download `id` in parts stands.
    pub fn set_resume(&mut self, id: u32, resume: Option<Resume>) {
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            i.resume = resume;
            self.touch(id, false);
        }
    }

    /// Serialized list for persistence (private downloads are left out).
    pub fn to_json(&self) -> String {
        let saved = Downloads {
            items: self.items.iter().filter(|i| !i.private).cloned().collect(),
            next_id: self.next_id,
            ..Default::default()
        };
        serde_json::to_string(&saved).unwrap_or_else(|_| "{}".into())
    }

    /// Newest first.
    pub fn items(&self) -> Vec<DownloadItem> {
        self.items.iter().rev().cloned().collect()
    }

    pub fn get(&self, id: u32) -> Option<&DownloadItem> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn active_count(&self) -> u32 {
        self.items.iter().filter(|i| !i.state.is_finished()).count() as u32
    }

    pub fn start(&mut self, url: &str, path: &Path, total: Option<i64>, now: i64, private: bool) -> u32 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        self.items.push(DownloadItem {
            id,
            url: url.to_string(),
            file_name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            path: path.to_string_lossy().into_owned(),
            state: DownloadState::InProgress,
            received: 0,
            total: total.filter(|t| *t >= 0),
            started: now,
            reason: None,
            private,
            finished: None,
            page_url: None,
            connections: None,
            parts: Vec::new(),
            speed: 0,
            eta: None,
            part_progress: Vec::new(),
            resume: None,
        });
        self.touch(id, true);
        // Drop the oldest finished entries beyond the cap (private ones; saved ones leave at
        // the next save).
        while self.items.len() > MAX_MEMORY_ITEMS {
            match self.items.iter().position(|i| i.state.is_finished()) {
                Some(p) => {
                    self.items.remove(p);
                }
                None => break,
            }
        }
        id
    }

    pub fn progress(&mut self, id: u32, received: i64, total: Option<i64>) {
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            i.received = received.max(0);
            if let Some(t) = total.filter(|t| *t >= 0) {
                i.total = Some(t);
            }
            self.moved.insert(id);
        }
    }

    /// Progress with a time stamp: also updates the smoothed speed and the time left.
    pub fn progress_at(&mut self, id: u32, received: i64, total: Option<i64>, now: i64) {
        self.progress(id, received, total);
        let speed = self.meters.entry(id).or_default().sample(now, received.max(0));
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            i.speed = speed;
            i.eta = eta_secs(speed, i.received, i.total);
        }
    }

    /// The per-connection progress of a download in parts (one bar segment each).
    pub fn set_part_progress(&mut self, id: u32, parts: Vec<PartProgress>) {
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            i.part_progress = parts;
            self.moved.insert(id);
        }
    }

    /// The page a download came from (only web pages; kept for "Copy page address").
    pub fn set_page(&mut self, id: u32, page: &str) {
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            let web =
                (page.starts_with("https://") || page.starts_with("http://")) && !crate::pages::is_pages_origin(page);
            i.page_url = web.then(|| page.to_string());
            self.touch(id, false);
        }
    }

    /// Fetched in parts: over `connections`, with these part files (cleared when it ends).
    pub fn set_parts(&mut self, id: u32, connections: u32, parts: Vec<String>) {
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            i.connections = Some(connections);
            i.parts = parts;
            self.touch(id, false);
        }
    }

    /// Part files of downloads that were running when the browser last exited (for cleanup);
    /// they are forgotten here.
    pub fn take_leftover_parts(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        for i in self.items.iter_mut().filter(|i| i.state.is_finished() && !i.parts.is_empty()) {
            out.append(&mut i.parts);
            self.dirty.insert(i.id);
        }
        out
    }

    pub fn set_state(&mut self, id: u32, state: DownloadState, reason: Option<String>, now: i64) {
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            self.list_changed |= i.state != state || i.reason != reason;
            self.dirty.insert(id);
            i.state = state;
            i.reason = reason;
            i.finished = if state.is_finished() { i.finished.or(Some(now)) } else { None };
            if state.is_finished() {
                i.parts.clear();
                i.resume = None;
                i.part_progress.clear();
                i.speed = 0;
                i.eta = None;
                self.meters.remove(&id);
            }
            if state == DownloadState::Completed {
                if let Some(t) = i.total {
                    i.received = t;
                }
            }
        }
    }

    /// Updates the final path (the engine may rename the file).
    pub fn set_path(&mut self, id: u32, path: &Path) {
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let path = path.to_string_lossy().into_owned();
            if i.path != path || i.file_name != name {
                i.path = path;
                i.file_name = name;
                self.dirty.insert(id);
                self.list_changed = true;
            }
        }
    }

    /// Removes a finished download from the list (in memory, or its row at the next save);
    /// `false` for a running one.
    pub fn remove(&mut self, id: u32) -> bool {
        if self.items.iter().any(|i| i.id == id && !i.state.is_finished()) {
            return false;
        }
        self.items.retain(|i| i.id != id);
        self.dirty.remove(&id);
        self.deleted.insert(id);
        self.list_changed = true;
        true
    }

    /// Removes every finished download (the files stay); the rows go at the next save.
    pub fn clear_finished(&mut self) {
        self.items.retain(|i| !i.state.is_finished());
        self.dirty.retain(|id| self.items.iter().any(|i| i.id == *id));
        self.clear_stored = true;
        self.list_changed = true;
    }

    /// The saved state is in the table: finished downloads that aren't private leave memory.
    fn saved(&mut self) {
        self.dirty.clear();
        self.deleted.clear();
        self.clear_stored = false;
        self.items.retain(|i| i.private || !i.state.is_finished());
    }
}

/// Writes (or rewrites) a download's row; the transient progress fields are left out.
fn write_row(c: &Connection, item: &DownloadItem) -> DbResult<()> {
    let mut i = item.clone();
    i.speed = 0;
    i.eta = None;
    i.part_progress.clear();
    let json = serde_json::to_string(&i).map_err(|e| DbError::ToSqlConversionFailure(Box::new(e)))?;
    c.prepare_cached(
        "INSERT INTO download_items(id, started, ended, state, search, json) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET started = excluded.started, ended = excluded.ended, state = excluded.state,
                                       search = excluded.search, json = excluded.json",
    )?
    .execute(params![i.id, i.started, i.finished.unwrap_or(i.started), i.state.as_str(), search_key(&i), json])?;
    Ok(())
}

/// What the search looks in: the name and the address, lower case.
fn search_key(i: &DownloadItem) -> String {
    format!("{}\n{}", i.file_name, i.url).to_lowercase()
}

fn row_item(json: &str) -> Option<DownloadItem> {
    serde_json::from_str(json).ok()
}

/// The rows a query reads: not in memory (memory has the newer state), not deleted, the
/// search text matching; `?1` is the JSON list of ids left out, `?2` the LIKE pattern.
fn rows_where(d: &Downloads, filter: DownloadFilter) -> String {
    format!(
        "id NOT IN (SELECT value FROM json_each(?1)) AND search LIKE ?2 ESCAPE '\\' AND ({}){}",
        filter.sql(),
        if d.clear_stored { format!(" AND {RUNNING_SQL}") } else { String::new() }
    )
}

impl Store {
    /// The list at start: rows beyond `MAX_ITEMS` and finished ones older than `auto_clear_days`
    /// (0 = never) go first; running and paused ones are restored into memory (see
    /// `Downloads::from_json`) and written again at the next save.
    pub fn downloads_load(&self, now: i64, auto_clear_days: u32) -> DbResult<Downloads> {
        self.downloads_prune(MAX_ITEMS)?;
        self.downloads_clear_older(now, auto_clear_days)?;
        let mut stmt =
            self.conn.prepare(&format!("SELECT json FROM download_items WHERE {RUNNING_SQL} ORDER BY started, id"))?;
        let items: Vec<DownloadItem> =
            stmt.query_map([], |r| r.get::<_, String>(0))?.filter_map(|j| j.ok().and_then(|j| row_item(&j))).collect();
        let max: Option<i64> = self.conn.query_row("SELECT max(id) FROM download_items", [], |r| r.get(0))?;
        let mut d = Downloads { items, ..Downloads::new() };
        d.next_id = (max.unwrap_or(0).max(0) as u32).max(d.items.iter().map(|i| i.id).max().unwrap_or(0)) + 1;
        d.restore();
        Ok(d)
    }

    /// Writes what changed in the list (rows of new and changed downloads, deletions, a clear);
    /// then the finished ones that aren't private leave memory. Private downloads are never written.
    pub fn downloads_save(&self, d: &mut Downloads) -> DbResult<()> {
        let tx = self.conn.unchecked_transaction()?;
        if d.clear_stored {
            tx.execute(&format!("DELETE FROM download_items WHERE NOT ({RUNNING_SQL})"), [])?;
        }
        for id in &d.deleted {
            tx.execute("DELETE FROM download_items WHERE id = ?1", [id])?;
        }
        for item in d.items.iter().filter(|i| !i.private && d.dirty.contains(&i.id)) {
            write_row(&tx, item)?;
        }
        tx.commit()?;
        d.saved();
        Ok(())
    }

    /// Deletes finished rows that ended (or, without an end, started) more than `days` days
    /// before `now`; 0 days = keep them. Returns how many went.
    pub fn downloads_clear_older(&self, now: i64, days: u32) -> DbResult<usize> {
        if days == 0 {
            return Ok(0);
        }
        let cutoff = now - i64::from(days) * 86_400_000;
        self.conn.execute(&format!("DELETE FROM download_items WHERE NOT ({RUNNING_SQL}) AND ended < ?1"), [cutoff])
    }

    /// Keeps at most `max` rows: the oldest finished ones go.
    pub fn downloads_prune(&self, max: usize) -> DbResult<usize> {
        self.conn.execute(
            &format!(
                "DELETE FROM download_items WHERE id IN (SELECT id FROM download_items WHERE NOT ({RUNNING_SQL})
                 ORDER BY started, id LIMIT max(0, (SELECT count(*) FROM download_items) - ?1))"
            ),
            [max as i64],
        )
    }

    /// Download `id`: from memory, else its row.
    pub fn download_get(&self, d: &Downloads, id: u32) -> DbResult<Option<DownloadItem>> {
        if let Some(i) = d.get(id) {
            return Ok(Some(i.clone()));
        }
        if d.deleted.contains(&id) {
            return Ok(None);
        }
        let json: Option<String> =
            self.conn.query_row("SELECT json FROM download_items WHERE id = ?1", [id], |r| r.get(0)).optional()?;
        Ok(json.and_then(|j| row_item(&j)).filter(|i| !(d.clear_stored && i.state.is_finished())))
    }

    /// A page of the list, newest first (by start, then id): the table's rows and the ones in
    /// memory (running, private, not saved yet) merged at their places, so the page can show
    /// any part of thousands of downloads at the cost of the rows it shows.
    pub fn downloads_query(&self, d: &Downloads, q: &DownloadQuery) -> DbResult<DownloadPage> {
        let limit = q.limit.clamp(1, MAX_PAGE);
        let text = q.text.trim().to_lowercase();
        let pattern = like_pattern(&text);
        let skip: Vec<u32> = d.items.iter().map(|i| i.id).chain(d.deleted.iter().copied()).collect();
        let skip = serde_json::to_string(&skip).unwrap_or_else(|_| "[]".into());
        let matches = |i: &DownloadItem, f: DownloadFilter| f.matches(i.state) && search_key(i).contains(&text);
        let mut mem: Vec<&DownloadItem> = d.items.iter().filter(|i| matches(i, q.filter)).collect();
        mem.sort_by_key(|i| std::cmp::Reverse((i.started, i.id)));
        let wh = rows_where(d, q.filter);
        // Where each one in memory is in the whole list: after the rows newer than it.
        let mut placed = Vec::with_capacity(mem.len());
        {
            let mut newer = self.conn.prepare_cached(&format!(
                "SELECT count(*) FROM download_items WHERE {wh} AND (started > ?3 OR (started = ?3 AND id > ?4))"
            ))?;
            for (n, i) in mem.iter().enumerate() {
                let rows: u32 = newer.query_row(params![skip, pattern, i.started, i.id], |r| r.get(0))?;
                placed.push((n as u32 + rows, *i));
            }
        }
        let end = q.offset.saturating_add(limit);
        let mem_before = placed.iter().filter(|(p, _)| *p < q.offset).count() as u32;
        let mut mem_in: std::collections::VecDeque<(u32, &DownloadItem)> =
            placed.iter().copied().filter(|(p, _)| *p >= q.offset && *p < end).collect();
        let need = limit - mem_in.len() as u32;
        let mut rows = std::collections::VecDeque::new();
        if need > 0 {
            let mut page = self.conn.prepare_cached(&format!(
                "SELECT json FROM download_items WHERE {wh} ORDER BY started DESC, id DESC LIMIT ?3 OFFSET ?4"
            ))?;
            let found =
                page.query_map(params![skip, pattern, need, q.offset - mem_before], |r| r.get::<_, String>(0))?;
            for j in found {
                rows.extend(row_item(&j?));
            }
        }
        let mut items = Vec::with_capacity(limit as usize);
        while items.len() < limit as usize {
            let at = q.offset + items.len() as u32;
            let next = if mem_in.front().is_some_and(|(p, _)| *p == at) || rows.is_empty() {
                mem_in.pop_front().map(|(_, i)| i.clone())
            } else {
                rows.pop_front()
            };
            match next {
                Some(i) => items.push(i),
                None => break,
            }
        }
        let counts = if q.counts { Some(self.downloads_counts(d, q, &skip, &pattern, &matches)?) } else { None };
        Ok(DownloadPage { offset: q.offset, items, counts })
    }

    fn downloads_counts(
        &self,
        d: &Downloads,
        q: &DownloadQuery,
        skip: &str,
        pattern: &str,
        matches: &dyn Fn(&DownloadItem, DownloadFilter) -> bool,
    ) -> DbResult<DownloadCounts> {
        let wh = rows_where(d, DownloadFilter::All);
        let (all, active, done, failed): (u32, u32, u32, u32) = self
            .conn
            .prepare_cached(&format!(
                "SELECT count(*), coalesce(sum({}), 0), coalesce(sum({}), 0), coalesce(sum({}), 0)
                 FROM download_items WHERE {wh}",
                DownloadFilter::Active.sql(),
                DownloadFilter::Done.sql(),
                DownloadFilter::Failed.sql()
            ))?
            .query_row(params![skip, pattern], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        let mem = |f: DownloadFilter| d.items.iter().filter(|i| matches(i, f)).count() as u32;
        let mut c = DownloadCounts {
            all: all + mem(DownloadFilter::All),
            active: active + mem(DownloadFilter::Active),
            done: done + mem(DownloadFilter::Done),
            failed: failed + mem(DownloadFilter::Failed),
            ..Default::default()
        };
        c.filtered = match q.filter {
            DownloadFilter::All => c.all,
            DownloadFilter::Active => c.active,
            DownloadFilter::Done => c.done,
            DownloadFilter::Failed => c.failed,
        };
        let wh = rows_where(d, q.filter);
        let mut since =
            self.conn.prepare_cached(&format!("SELECT count(*) FROM download_items WHERE {wh} AND started >= ?3"))?;
        for t in q.since.iter().take(4) {
            let rows: u32 = since.query_row(params![skip, pattern, t], |r| r.get(0))?;
            let mem = d.items.iter().filter(|i| matches(i, q.filter) && i.started >= *t).count() as u32;
            c.since.push(rows + mem);
        }
        Ok(c)
    }
}

/// Makes a server-suggested file name safe for Windows.
/// Readable text for Chromium's `DownloadInterruptReason` (`None` for a
/// cancel by the user, which is not an error).
pub fn interrupt_reason_text(code: i32) -> Option<String> {
    Some(match code {
        40 => return None,
        2 => "Access denied".into(),
        3 => "Disk full".into(),
        5 => "File name too long".into(),
        6 => "File too large".into(),
        7 => "Virus detected".into(),
        11 => "Blocked by policy".into(),
        12 => "Security check failed".into(),
        1 | 10 | 13..=15 => "File error".into(),
        20..=24 => "Network error".into(),
        34 | 36 => "Not allowed by the server".into(),
        35 => "Certificate problem".into(),
        30..=39 => "Server error".into(),
        41 => "Browser closed".into(),
        50 => "Download process crashed".into(),
        _ => format!("Interrupted (code {code})"),
    })
}

/// A PDF the viewer can show (0.11.3): by its MIME type, or by its name when the server
/// only says "a file" (`application/octet-stream`, `binary/octet-stream`, none).
pub fn is_pdf(mime: &str, name: &str) -> bool {
    let mime = mime.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    let pdf_name = name.to_ascii_lowercase().ends_with(".pdf");
    mime == "application/pdf"
        || mime == "application/x-pdf"
        || (pdf_name
            && matches!(
                mime.as_str(),
                "" | "application/octet-stream" | "binary/octet-stream" | "application/force-download"
            ))
}

/// The Mark-of-the-Web for a downloaded file: the text of its `Zone.Identifier` stream, as
/// Chrome writes it (zone 3, Internet; the addresses without credentials, http(s) only; none
/// for a private download). SmartScreen and Office's Protected View read it (0.11.7, audit
/// C7: files fetched in parts had none).
pub fn zone_identifier(url: &str, page_url: Option<&str>, private: bool) -> String {
    let clean = |u: &str| -> Option<String> {
        let u = crate::urlclean::strip_credentials(u);
        let parsed = url::Url::parse(&u).ok()?;
        (matches!(parsed.scheme(), "http" | "https") && !u.contains(['\r', '\n'])).then_some(u)
    };
    let mut out = String::from("[ZoneTransfer]\r\nZoneId=3\r\n");
    if !private {
        if let Some(r) = page_url.and_then(clean) {
            out.push_str(&format!("ReferrerUrl={r}\r\n"));
        }
        if let Some(h) = clean(url) {
            out.push_str(&format!("HostUrl={h}\r\n"));
        }
    }
    out
}

pub fn sanitize_file_name(name: &str) -> String {
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1",
        "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    // Only the last path component counts.
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let mut out: String = base
        .chars()
        .map(|c| if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') { '_' } else { c })
        .collect();
    out = out.trim().trim_end_matches(['.', ' ']).to_string();
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    if out.is_empty() || RESERVED.contains(&stem.as_str()) {
        out = format!("download{}", if out.is_empty() { String::new() } else { format!("_{out}") });
    }
    if out.chars().count() > 200 {
        // Characters, not bytes: a site's name may put a multi-byte character across byte 16.
        let ext: String = out
            .rsplit_once('.')
            .map(|(_, e)| format!(".{}", e.chars().take(16).collect::<String>()))
            .unwrap_or_default();
        out = out.chars().take(200 - ext.chars().count()).collect::<String>() + &ext;
    }
    out
}

/// `dir/name`, or `dir/name (1).ext`, `(2)`, ... if it already exists.
pub fn unique_path(dir: &Path, name: &str, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let candidate = dir.join(name);
    if !exists(&candidate) {
        return candidate;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    for n in 1..10_000 {
        let p = dir.join(format!("{stem} ({n}){ext}"));
        if !exists(&p) {
            return p;
        }
    }
    dir.join(format!("{stem} ({}){ext}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_is_smoothed_and_eta_follows_it() {
        let mut m = Meter::default();
        assert_eq!(m.sample(1000, 0), 0);
        // 1 MB/s for a second: the first sample sets the rate.
        assert_eq!(m.sample(2000, 1_000_000), 1_000_000);
        // A burst of 3 MB/s moves the average part of the way (half-life 1.5 s).
        let after = m.sample(3000, 4_000_000);
        assert!(after > 1_500_000 && after < 2_500_000, "{after}");
        // Samples closer than 50 ms are ignored (no division by nothing).
        assert_eq!(m.sample(3010, 4_000_100), after);
        assert_eq!(eta_secs(after, 4_000_000, Some(10_000_000)), Some(6_000_000u64.div_ceil(after)));
        assert_eq!(eta_secs(0, 1, Some(10)), None);
        assert_eq!(eta_secs(100, 10, Some(10)), None);
        assert_eq!(eta_secs(100, 10, None), None);
        let mut d = Downloads::new();
        let id = d.start("https://x.example/f.bin", std::path::Path::new("C:\\d\\f.bin"), Some(2_000_000), 1000, false);
        d.progress_at(id, 0, Some(2_000_000), 1000);
        d.progress_at(id, 500_000, Some(2_000_000), 1500);
        d.set_part_progress(
            id,
            vec![
                PartProgress { done: 250_000, len: 1_000_000, start: 0, conn: 0 },
                PartProgress { done: 250_000, len: 1_000_000, start: 1_000_000, conn: 1 },
            ],
        );
        let i = d.get(id).unwrap().clone();
        assert_eq!(i.speed, 1_000_000);
        assert_eq!(i.eta, Some(2));
        assert_eq!(i.part_progress.len(), 2);
        let json = serde_json::to_string(&i).unwrap();
        assert!(json.contains("\"speed\":1000000") && json.contains("\"partProgress\""), "{json}");
        // Finishing clears the transient fields, and they are not saved.
        d.set_state(id, DownloadState::Completed, None, 2000);
        let i = d.get(id).unwrap().clone();
        assert_eq!((i.speed, i.eta, i.part_progress.len()), (0, None, 0));
        let json = serde_json::to_string(&i).unwrap();
        assert!(!json.contains("speed") && !json.contains("partProgress") && !json.contains("eta"), "{json}");
        // A saved list restores without the meters.
        let back = Downloads::from_json(&d.to_json());
        assert_eq!(back.items().len(), 1);
    }

    #[test]
    fn lifecycle() {
        let mut d = Downloads::new();
        let a = d.start("https://e.com/a.zip", Path::new("C:/dl/a.zip"), Some(100), 1, false);
        let b = d.start("https://e.com/b.pdf", Path::new("C:/dl/b.pdf"), None, 2, false);
        assert_eq!(d.active_count(), 2);
        d.progress(a, 40, None);
        assert_eq!(d.get(a).unwrap().received, 40);
        d.set_state(a, DownloadState::Completed, None, 9);
        assert_eq!(d.get(a).unwrap().received, 100);
        assert_eq!(d.active_count(), 1);
        assert!(!d.remove(b), "running downloads cannot be removed");
        d.set_state(b, DownloadState::Interrupted, Some("Network".into()), 9);
        assert_eq!(d.items()[0].id, b, "newest first");
        d.set_path(a, Path::new("C:/dl/a (1).zip"));
        assert_eq!(d.get(a).unwrap().file_name, "a (1).zip");
        d.clear_finished();
        assert!(d.items().is_empty());
    }

    #[test]
    fn finish_time_and_page_and_old_lists() {
        let mut d = Downloads::new();
        let a = d.start("https://e.com/a.zip", Path::new("C:/dl/a.zip"), Some(10), 1_000, false);
        assert_eq!(d.get(a).unwrap().finished, None);
        d.set_page(a, "https://e.com/files.html");
        d.set_state(a, DownloadState::Paused, None, 2_000);
        assert_eq!(d.get(a).unwrap().finished, None, "paused is not finished");
        d.set_state(a, DownloadState::Completed, None, 3_000);
        d.set_state(a, DownloadState::Completed, None, 4_000);
        let item = d.get(a).unwrap();
        assert_eq!((item.started, item.finished), (1_000, Some(3_000)), "the first finish counts");
        assert_eq!(item.page_url.as_deref(), Some("https://e.com/files.html"));
        let b = d.start("https://e.com/b", Path::new("b"), None, 5, false);
        d.set_page(b, "https://krunk.invalid/downloads.html");
        assert_eq!(d.get(b).unwrap().page_url, None, "only web pages");
        let back = Downloads::from_json(&d.to_json());
        assert_eq!(back.get(a), d.get(a));
        // A list saved by 0.3.0 has neither field.
        let old = r#"{"items":[{"id":3,"url":"https://e.com/x.iso","fileName":"x.iso","path":"C:/dl/x.iso","state":"completed","received":5,"total":5,"started":7,"reason":null,"private":false}],"next_id":4}"#;
        let old = Downloads::from_json(old);
        let x = old.get(3).unwrap();
        assert_eq!((x.finished, x.page_url.as_deref(), x.url.as_str()), (None, None, "https://e.com/x.iso"));
        let json = serde_json::to_string(x).unwrap();
        assert!(json.contains(r#""finished":null"#) && json.contains(r#""pageUrl":null"#), "{json}");
    }

    #[test]
    fn parts_are_cleaned_up() {
        let mut d = Downloads::new();
        let a = d.start("https://e.com/big.iso", Path::new("C:/dl/big.iso"), Some(1 << 26), 1, false);
        d.set_parts(a, 4, vec!["C:/dl/big.iso.part0".into(), "C:/dl/big.iso.part1".into()]);
        assert_eq!(d.get(a).unwrap().connections, Some(4));
        // The browser quit while it ran: at the next start it is interrupted, its parts go.
        let mut back = Downloads::from_json(&d.to_json());
        assert_eq!(back.get(a).unwrap().state, DownloadState::Interrupted);
        assert_eq!(back.take_leftover_parts(), vec!["C:/dl/big.iso.part0".to_string(), "C:/dl/big.iso.part1".into()]);
        assert!(back.take_leftover_parts().is_empty());
        assert!(back.get(a).unwrap().parts.is_empty());
        // Finishing clears them too (the host removed or merged the files).
        d.set_state(a, DownloadState::Completed, None, 2);
        assert!(d.get(a).unwrap().parts.is_empty() && !d.to_json().contains("parts"));
    }

    #[test]
    fn a_download_in_parts_goes_on_after_a_restart() {
        use crate::segdl::Segment;
        let mut d = Downloads::new();
        let a = d.start("https://e.com/big.iso", Path::new("C:/dl/big.iso"), Some(1 << 26), 1, false);
        d.set_parts(a, 4, vec!["C:/dl/big.iso.krunkpart".into()]);
        d.set_part_progress(a, vec![PartProgress { done: 5, len: 10, start: 0, conn: 0 }]);
        let resume = Resume {
            temp: "C:/dl/big.iso.krunkpart".into(),
            total: 1 << 26,
            etag: Some("\"abc\"".into()),
            last_modified: None,
            referer: Some("https://e.com/".into()),
            segments: vec![Segment { start: 0, end: 99, next: 50 }, Segment { start: 1000, end: 1999, next: 1000 }],
        };
        d.set_resume(a, Some(resume.clone()));
        let json = d.to_json();
        assert!(!json.to_ascii_lowercase().contains("cookie"), "no cookies saved: {json}");
        // Krunk quit while it ran: paused, with its file kept and where it stood.
        let mut back = Downloads::from_json(&json);
        let item = back.get(a).unwrap();
        assert_eq!(item.state, DownloadState::Paused);
        assert_eq!(item.resume.as_ref(), Some(&resume));
        assert!(item.part_progress.is_empty() && item.speed == 0);
        assert!(back.take_leftover_parts().is_empty(), "the file is kept for Resume");
        assert_eq!(back.stopped_by_close(), vec!["https://e.com/big.iso".to_string()]);
        // It ends (done, cancelled or failed): nothing to resume any more.
        back.set_state(a, DownloadState::Cancelled, None, 9);
        assert!(back.get(a).unwrap().resume.is_none() && !back.to_json().contains("segments"));
        // A 0.6.0 list (no resume state) still restores as interrupted.
        let old = r#"{"items":[{"id":1,"url":"https://e.com/x","fileName":"x","path":"C:/dl/x","state":"inProgress","received":0,"total":null,"started":1,"reason":null,"parts":["C:/dl/x.part0"],"partProgress":[{"done":1,"len":2}]}],"next_id":2}"#;
        let mut old = Downloads::from_json(old);
        assert_eq!(old.get(1).unwrap().state, DownloadState::Interrupted);
        assert_eq!(old.take_leftover_parts(), vec!["C:/dl/x.part0".to_string()]);
    }

    #[test]
    fn restore_marks_running_as_interrupted() {
        let mut d = Downloads::new();
        let a = d.start("https://e.com/a", Path::new("a"), None, 1, false);
        let json = d.to_json();
        let r = Downloads::from_json(&json);
        assert_eq!(r.get(a).unwrap().state, DownloadState::Interrupted);
        let mut r = r;
        assert_ne!(r.start("https://e.com/b", Path::new("b"), None, 2, false), a, "ids not reused");
        assert_eq!(Downloads::from_json("junk").items().len(), 0);
    }

    #[test]
    fn private_downloads_are_not_saved() {
        let mut d = Downloads::new();
        d.start("https://e.com/public", Path::new("a"), None, 1, false);
        d.start("https://e.com/secret", Path::new("b"), None, 2, true);
        assert_eq!(d.items().len(), 2, "shown during the session");
        let json = d.to_json();
        assert!(!json.contains("secret"));
        assert_eq!(Downloads::from_json(&json).items().len(), 1);
    }

    #[test]
    fn capped() {
        // Private ones stay in memory for the session, up to a cap.
        let mut d = Downloads::new();
        for i in 0..MAX_MEMORY_ITEMS + 10 {
            let id = d.start("https://e.com/", Path::new("f"), None, i as i64, true);
            d.set_state(id, DownloadState::Completed, None, 9);
        }
        assert_eq!(d.items().len(), MAX_MEMORY_ITEMS);
    }

    // ------------------------------------------------------------ 0.15.0: the table

    const DAY: i64 = 86_400_000;

    fn q(text: &str, filter: DownloadFilter, offset: u32, limit: u32) -> DownloadQuery {
        DownloadQuery { text: text.into(), filter, offset, limit, counts: true, since: Vec::new() }
    }

    fn ids(p: &DownloadPage) -> Vec<u32> {
        p.items.iter().map(|i| i.id).collect()
    }

    /// `n` finished downloads, one a minute from `t0`: every 10th failed, every 25th cancelled,
    /// names by kind (`.zip`, `.pdf`, `.mp4`).
    fn fill(s: &Store, d: &mut Downloads, n: usize, t0: i64) {
        for k in 0..n {
            let name = format!("file-{k}.{}", ["zip", "pdf", "mp4"][k % 3]);
            let id = d.start(
                &format!("https://dl{}.example/{name}", k % 7),
                Path::new(&name),
                Some(1000),
                t0 + k as i64 * 60_000,
                false,
            );
            let state = if k % 25 == 0 {
                DownloadState::Cancelled
            } else if k % 10 == 0 {
                DownloadState::Interrupted
            } else {
                DownloadState::Completed
            };
            d.set_state(id, state, None, t0 + k as i64 * 60_000 + 5_000);
            if k % 200 == 199 {
                s.downloads_save(d).unwrap();
            }
        }
        s.downloads_save(d).unwrap();
    }

    fn rows(s: &Store) -> i64 {
        s.conn.query_row("SELECT count(*) FROM download_items", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn finished_downloads_go_to_the_table_and_progress_is_not_written() {
        let s = Store::open_in_memory().unwrap();
        let mut d = s.downloads_load(1, 0).unwrap();
        let a = d.start("https://e.com/a.zip", Path::new("C:/dl/a.zip"), Some(100), 10, false);
        s.downloads_save(&mut d).unwrap();
        assert_eq!(rows(&s), 1, "a row from the start");
        d.progress_at(a, 50, Some(100), 20);
        assert!(d.dirty.is_empty(), "progress alone is not written");
        assert_eq!(d.take_moved().iter().map(|i| i.received).collect::<Vec<_>>(), vec![50]);
        assert!(d.take_moved().is_empty(), "only what moved since the last push");
        assert!(d.take_list_changed() && !d.take_list_changed());
        d.set_state(a, DownloadState::Completed, None, 30);
        assert!(d.take_list_changed());
        s.downloads_save(&mut d).unwrap();
        assert!(d.items().is_empty(), "finished: only in the table");
        let item = s.download_get(&d, a).unwrap().unwrap();
        assert_eq!((item.state, item.received, item.finished), (DownloadState::Completed, 100, Some(30)));
        assert_eq!(ids(&s.downloads_query(&d, &q("", DownloadFilter::All, 0, 10)).unwrap()), vec![a]);
        // The next start counts on from the table's ids.
        let d2 = s.downloads_load(40, 0).unwrap();
        assert!(d2.items().is_empty());
        assert_eq!(d2.next_id, a + 1);
    }

    #[test]
    fn private_downloads_never_reach_the_table() {
        let s = Store::open_in_memory().unwrap();
        let mut d = Downloads::new();
        let p = d.start("https://e.com/secret.zip", Path::new("secret.zip"), None, 5, true);
        d.set_state(p, DownloadState::Completed, None, 6);
        let o = d.start("https://e.com/open.zip", Path::new("open.zip"), None, 7, false);
        s.downloads_save(&mut d).unwrap();
        let json: Vec<String> = s
            .conn
            .prepare("SELECT json FROM download_items")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(json.len(), 1);
        assert!(!json[0].contains("secret"));
        // Shown in this session, merged into the list.
        let page = s.downloads_query(&d, &q("", DownloadFilter::All, 0, 10)).unwrap();
        assert_eq!(ids(&page), vec![o, p]);
        assert_eq!(page.counts.unwrap().done, 1);
    }

    #[test]
    fn pages_come_newest_first_with_memory_merged_in_place() {
        let s = Store::open_in_memory().unwrap();
        let mut d = Downloads::new();
        fill(&s, &mut d, 2500, 1_000_000);
        // In memory: a running one started between stored ones, and a new private one.
        let mid =
            d.start("https://e.com/mid.iso", Path::new("mid.iso"), None, 1_000_000 + 1234 * 60_000 + 30_000, false);
        let newest = d.start("https://e.com/new.iso", Path::new("new.iso"), None, 9_000_000_000, true);
        // The whole order, one page at a time, equals the order of everything sorted.
        let mut all = Vec::new();
        let mut offset = 0;
        loop {
            let page = s.downloads_query(&d, &q("", DownloadFilter::All, offset, 97)).unwrap();
            assert_eq!(page.offset, offset);
            if page.items.is_empty() {
                break;
            }
            offset += page.items.len() as u32;
            all.extend(page.items);
        }
        assert_eq!(all.len(), 2502);
        assert!(all.windows(2).all(|w| (w[0].started, w[0].id) > (w[1].started, w[1].id)), "newest first");
        assert_eq!(all[0].id, newest);
        let at = all.iter().position(|i| i.id == mid).unwrap();
        assert_eq!(at, 2500 - 1235 + 1, "the running one sits by its start time");
        // A page in the middle without fetching the ones before it.
        let page = s.downloads_query(&d, &q("", DownloadFilter::All, 1200, 100)).unwrap();
        assert_eq!(ids(&page), all[1200..1300].iter().map(|i| i.id).collect::<Vec<_>>());
        assert_eq!(page.counts.unwrap().all, 2502);
        // Past the end: nothing; the limit is capped.
        assert!(s.downloads_query(&d, &q("", DownloadFilter::All, 5000, 10)).unwrap().items.is_empty());
        assert_eq!(
            s.downloads_query(&d, &q("", DownloadFilter::All, 0, 100_000)).unwrap().items.len(),
            MAX_PAGE as usize
        );
    }

    #[test]
    fn search_filters_and_counts() {
        let s = Store::open_in_memory().unwrap();
        let mut d = Downloads::new();
        fill(&s, &mut d, 300, 1_000_000);
        let run = d.start("https://e.com/Running.ZIP", Path::new("Running.ZIP"), None, 50_000_000, false);
        let c = s.downloads_query(&d, &q("", DownloadFilter::All, 0, 10)).unwrap().counts.unwrap();
        // 300: 12 cancelled (k % 25 == 0), 30 - 6 = 24 more failed (k % 10 == 0 but not % 25 == 0... 0,50,100,...).
        let cancelled = (0..300).filter(|k| k % 25 == 0).count() as u32;
        let failed = (0..300).filter(|k| k % 25 != 0 && k % 10 == 0).count() as u32;
        assert_eq!((c.all, c.active, c.failed), (301, 1, cancelled + failed));
        assert_eq!(c.done, 300 - cancelled - failed);
        // Filters.
        let page = s.downloads_query(&d, &q("", DownloadFilter::Active, 0, 50)).unwrap();
        assert_eq!(ids(&page), vec![run]);
        let page = s.downloads_query(&d, &q("", DownloadFilter::Failed, 0, 500)).unwrap();
        assert_eq!(page.items.len() as u32, cancelled + failed);
        assert!(page.items.iter().all(|i| matches!(i.state, DownloadState::Interrupted | DownloadState::Cancelled)));
        assert_eq!(page.counts.unwrap().filtered, cancelled + failed);
        let page = s.downloads_query(&d, &q("", DownloadFilter::Done, 0, 500)).unwrap();
        assert!(page.items.iter().all(|i| i.state == DownloadState::Completed));
        // Search: name or address, any case, in memory too; LIKE characters are literal.
        let page = s.downloads_query(&d, &q("ZIP", DownloadFilter::All, 0, 500)).unwrap();
        assert_eq!(page.items.len(), 101);
        assert_eq!(page.items[0].id, run);
        let c = page.counts.unwrap();
        assert_eq!((c.all, c.filtered, c.active), (101, 101, 1));
        let page = s.downloads_query(&d, &q("dl3.example", DownloadFilter::Done, 0, 500)).unwrap();
        assert!(!page.items.is_empty() && page.items.iter().all(|i| i.url.contains("dl3.example")));
        assert!(s.downloads_query(&d, &q("file-1%", DownloadFilter::All, 0, 10)).unwrap().items.is_empty());
        assert_eq!(s.downloads_query(&d, &q("file-299.", DownloadFilter::All, 0, 10)).unwrap().items.len(), 1);
        // Without `counts`, no counting.
        let mut nq = q("", DownloadFilter::All, 0, 5);
        nq.counts = false;
        assert!(s.downloads_query(&d, &nq).unwrap().counts.is_none());
    }

    #[test]
    fn date_groups_count_since_the_page_midnights() {
        let s = Store::open_in_memory().unwrap();
        let mut d = Downloads::new();
        let today = 100 * DAY;
        let yesterday = today - DAY;
        // 3 before yesterday, 4 yesterday, 5 today (one of them running, in memory), one failed today.
        for (k, t) in [(3, yesterday - 5 * DAY), (4, yesterday + 1000), (4, today + 1000)] {
            for n in 0..k {
                let id = d.start("https://e.com/x.bin", Path::new("x.bin"), None, t + n, false);
                d.set_state(id, DownloadState::Completed, None, t + n + 1);
            }
        }
        let f = d.start("https://e.com/y.bin", Path::new("y.bin"), None, today + 50, false);
        d.set_state(f, DownloadState::Interrupted, Some("Network error".into()), today + 60);
        s.downloads_save(&mut d).unwrap();
        d.start("https://e.com/run.bin", Path::new("run.bin"), None, today + 99, false);
        let mut query = q("", DownloadFilter::All, 0, 100);
        query.since = vec![today, yesterday];
        let c = s.downloads_query(&d, &query).unwrap().counts.unwrap();
        assert_eq!(c.since, vec![6, 10], "today 6, yesterday 4, earlier 3");
        query.filter = DownloadFilter::Done;
        let c = s.downloads_query(&d, &query).unwrap().counts.unwrap();
        assert_eq!((c.filtered, c.since.clone()), (11, vec![4, 8]), "the groups follow the filter");
        query.text = "y.bin".into();
        query.filter = DownloadFilter::All;
        assert_eq!(s.downloads_query(&d, &query).unwrap().counts.unwrap().since, vec![1, 1]);
    }

    #[test]
    fn clear_completed_remove_auto_clear_and_prune() {
        let s = Store::open_in_memory().unwrap();
        let mut d = Downloads::new();
        fill(&s, &mut d, 50, DAY);
        let run = d.start("https://e.com/run.bin", Path::new("run.bin"), None, 100 * DAY, false);
        s.downloads_save(&mut d).unwrap();
        // Remove one stored row: gone from pages before the save already, and from the table after.
        let gone = s.downloads_query(&d, &q("", DownloadFilter::All, 1, 1)).unwrap().items[0].id;
        assert!(d.remove(gone));
        assert!(!d.remove(run), "a running one stays");
        assert!(s.download_get(&d, gone).unwrap().is_none());
        assert_eq!(s.downloads_query(&d, &q("", DownloadFilter::All, 0, 100)).unwrap().items.len(), 50);
        s.downloads_save(&mut d).unwrap();
        assert_eq!(rows(&s), 50);
        // Auto-clear by age: what ended before the cut goes; running ones never; 0 = off.
        assert_eq!(s.downloads_clear_older(DAY + 20 * 60_000, 0).unwrap(), 0);
        let n = s.downloads_clear_older(DAY + 10 * 60_000 + DAY * 3, 3).unwrap();
        assert_eq!(n, 10, "the 10 that ended before minute 10");
        assert_eq!(rows(&s), 40);
        // And at start (`downloads_load`), with the setting.
        let loaded = s.downloads_load(DAY + 20 * 60_000 + 3 * DAY, 3).unwrap();
        assert_eq!(rows(&s), 30);
        assert_eq!(loaded.items().len(), 1, "the running one is restored");
        // Prune keeps the newest, never a running one.
        assert_eq!(s.downloads_prune(5).unwrap(), 25);
        let page = s.downloads_query(&d, &q("", DownloadFilter::All, 0, 100)).unwrap();
        assert_eq!(page.items.len(), 5);
        assert_eq!(page.items[0].id, run);
        // Clear completed: every finished one, at once in pages, then in the table.
        d.clear_finished();
        let page = s.downloads_query(&d, &q("", DownloadFilter::All, 0, 100)).unwrap();
        assert_eq!((ids(&page), page.counts.unwrap().all), (vec![run], 1));
        s.downloads_save(&mut d).unwrap();
        assert_eq!(rows(&s), 1);
    }

    #[test]
    fn the_old_kv_list_moves_into_the_table() {
        let dir = std::env::temp_dir().join(format!("lb-dl-v9-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("browser.db");
        {
            let s = Store::open_at_version(&path, 8).unwrap();
            // As 0.14 saved it: a finished, a failed and a running one (no private ones: never saved).
            let old = r#"{"items":[
                {"id":3,"url":"https://e.com/a.zip","fileName":"a.zip","path":"C:/dl/a.zip","state":"completed","received":5,"total":5,"started":7,"reason":null,"private":false,"finished":9},
                {"id":4,"url":"https://e.com/b.pdf","fileName":"b.pdf","path":"C:/dl/b.pdf","state":"interrupted","received":1,"total":null,"started":8,"reason":"Network error"},
                {"id":6,"url":"https://e.com/c.iso","fileName":"c.iso","path":"C:/dl/c.iso","state":"inProgress","received":1,"total":9,"started":10,"reason":null},
                {"id":7,"url":"https://e.com/p.iso","fileName":"p.iso","path":"C:/dl/p.iso","state":"completed","received":1,"total":1,"started":11,"reason":null,"private":true}
            ],"next_id":8}"#;
            s.kv_set("downloads", old).unwrap();
        }
        let s = Store::open(&path).unwrap();
        assert_eq!(s.kv_get("downloads").unwrap(), None, "the record is gone");
        assert_eq!(rows(&s), 3, "private ones are skipped");
        let mut d = s.downloads_load(100, 0).unwrap();
        assert_eq!(d.get(6).unwrap().state, DownloadState::Interrupted, "running: stopped by the close");
        assert_eq!(d.stopped_by_close(), vec!["https://e.com/c.iso".to_string()]);
        assert_eq!(d.next_id, 7);
        s.downloads_save(&mut d).unwrap();
        let page = s.downloads_query(&d, &q("", DownloadFilter::All, 0, 10)).unwrap();
        assert_eq!(ids(&page), vec![6, 4, 3]);
        let c = page.counts.unwrap();
        assert_eq!((c.done, c.failed, c.active), (1, 2, 0));
        assert_eq!(s.download_get(&d, 3).unwrap().unwrap().finished, Some(9));
        drop(s);
        // Opening again changes nothing.
        let s = Store::open(&path).unwrap();
        assert_eq!(rows(&s), 3);
        drop(s);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn paused_downloads_in_parts_survive_a_restart_through_the_table() {
        use crate::segdl::Segment;
        let s = Store::open_in_memory().unwrap();
        let mut d = Downloads::new();
        let a = d.start("https://e.com/big.iso", Path::new("C:/dl/big.iso"), Some(1 << 26), 1, false);
        d.set_parts(a, 4, vec!["C:/dl/big.iso.krunkpart".into()]);
        let resume = Resume {
            temp: "C:/dl/big.iso.krunkpart".into(),
            total: 1 << 26,
            etag: None,
            last_modified: None,
            referer: None,
            segments: vec![Segment { start: 0, end: 99, next: 50 }],
        };
        d.set_resume(a, Some(resume.clone()));
        s.downloads_save(&mut d).unwrap();
        let back = s.downloads_load(5, 0).unwrap();
        let item = back.get(a).unwrap();
        assert_eq!((item.state, item.resume.as_ref()), (DownloadState::Paused, Some(&resume)));
    }

    #[test]
    fn file_names() {
        assert_eq!(sanitize_file_name("report.pdf"), "report.pdf");
        assert_eq!(sanitize_file_name("../../evil.exe"), "evil.exe");
        assert_eq!(sanitize_file_name("C:\\Windows\\x.dll"), "x.dll");
        assert_eq!(sanitize_file_name("a<b>c:d\"e|f?g*.txt"), "a_b_c_d_e_f_g_.txt");
        assert_eq!(sanitize_file_name("CON.txt"), "download_CON.txt");
        assert_eq!(sanitize_file_name("trailing. . "), "trailing");
        assert_eq!(sanitize_file_name(""), "download");
        let long = format!("{}.tar.gz", "x".repeat(300));
        let s = sanitize_file_name(&long);
        assert!(s.chars().count() <= 200 && s.ends_with(".gz"));
    }

    #[test]
    fn mark_of_the_web() {
        assert_eq!(
            zone_identifier("https://u:p@dl.example/a.exe", Some("https://site.example/page"), false),
            "[ZoneTransfer]\r\nZoneId=3\r\nReferrerUrl=https://site.example/page\r\nHostUrl=https://dl.example/a.exe\r\n"
        );
        assert_eq!(zone_identifier("https://dl.example/a.exe", None, true), "[ZoneTransfer]\r\nZoneId=3\r\n");
        // Other schemes are left out, but the file is still marked.
        assert_eq!(zone_identifier("data:text/plain,x", Some("file:///C:/x"), false), "[ZoneTransfer]\r\nZoneId=3\r\n");
    }

    /// 0.11.7 (audit C2): a site's long name whose extension has a multi-byte character across
    /// byte 16 aborted the browser (a byte slice in the middle of a character).
    #[test]
    fn long_names_with_multibyte_extensions() {
        for ext in ["aaaaaaaaaaaaaaaé", "€€€€€€", "ab😀😀😀😀😀😀😀", "ééééééééééééééééééé"]
        {
            let s = sanitize_file_name(&format!("{}.{ext}", "a".repeat(250)));
            assert!(s.chars().count() <= 200, "{s}");
            let kept = s.rsplit_once('.').map(|(_, e)| e).unwrap_or_default();
            assert!(kept.chars().count() <= 16 && ext.starts_with(kept), "{ext} -> {kept}");
        }
        // A legitimate long name keeps its extension.
        let s = sanitize_file_name(&format!("{}.pdf", "Ü".repeat(250)));
        assert!(s.ends_with(".pdf") && s.chars().count() == 200);
    }

    #[test]
    fn unique_paths() {
        let taken = ["C:/dl/a.zip", "C:/dl/a (1).zip", "C:/dl/README"];
        let exists = |p: &Path| taken.iter().any(|t| Path::new(t) == p);
        assert_eq!(unique_path(Path::new("C:/dl"), "b.zip", exists), Path::new("C:/dl/b.zip"));
        assert_eq!(unique_path(Path::new("C:/dl"), "a.zip", exists), Path::new("C:/dl/a (2).zip"));
        assert_eq!(unique_path(Path::new("C:/dl"), "README", exists), Path::new("C:/dl/README (1)"));
    }

    #[test]
    fn pdfs_by_type_or_name() {
        assert!(is_pdf("application/pdf", "x.bin") && is_pdf("Application/PDF; charset=binary", ""));
        assert!(is_pdf("application/octet-stream", "Report.PDF") && is_pdf("", "a.pdf"));
        assert!(!is_pdf("application/zip", "a.pdf") && !is_pdf("application/octet-stream", "a.zip"));
    }

    #[test]
    fn interrupt_reasons() {
        assert_eq!(interrupt_reason_text(40), None, "user cancel is not an error");
        assert_eq!(interrupt_reason_text(3).as_deref(), Some("Disk full"));
        assert_eq!(interrupt_reason_text(21).as_deref(), Some("Network error"));
        assert_eq!(interrupt_reason_text(35).as_deref(), Some("Certificate problem"));
        assert_eq!(interrupt_reason_text(36).as_deref(), Some("Not allowed by the server"));
        assert_eq!(interrupt_reason_text(33).as_deref(), Some("Server error"));
        assert_eq!(interrupt_reason_text(41).as_deref(), Some("Browser closed"));
        assert_eq!(interrupt_reason_text(99).as_deref(), Some("Interrupted (code 99)"));
    }
}
