//! Sync's bookkeeping in `browser.db` (PLAN §5.2): what each synced item last looked like on the
//! server (`sync_items`), the local changes waiting to be pushed (`sync_queue`, filled by the
//! v6 triggers while `track` is on) and small state (`sync_meta`).

use crate::storage::{DbResult, Store};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

/// One synced item: its record on the server and the collection's own state (`extra`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SyncItem {
    pub record_id: Vec<u8>,
    pub server_seq: i64,
    pub ver: u64,
    pub bucket: i64,
    pub deleted: bool,
    /// The item came from sync (removed by "turn off and delete synced data").
    pub origin_sync: bool,
    pub extra: Value,
}

pub fn meta_get(conn: &Connection, key: &str) -> DbResult<Option<String>> {
    conn.query_row("SELECT value FROM sync_meta WHERE key = ?1", [key], |r| r.get(0)).optional()
}

pub fn meta_set(conn: &Connection, key: &str, value: &str) -> DbResult<()> {
    conn.execute("INSERT OR REPLACE INTO sync_meta(key, value) VALUES (?1, ?2)", params![key, value])?;
    Ok(())
}

pub fn meta_del(conn: &Connection, key: &str) -> DbResult<()> {
    conn.execute("DELETE FROM sync_meta WHERE key = ?1", [key])?;
    Ok(())
}

/// The collections switched off on this device (`sync_meta` "off", a JSON list).
pub fn off_collections(conn: &Connection) -> Vec<String> {
    meta_get(conn, "off").ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn item(conn: &Connection, c: &str, nk: &str) -> DbResult<Option<SyncItem>> {
    conn.query_row(
        "SELECT record_id, server_seq, ver, bucket, deleted, origin_sync, extra FROM sync_items WHERE collection = ?1 AND natural_key = ?2",
        params![c, nk],
        row,
    )
    .optional()
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<SyncItem> {
    let extra: Option<String> = r.get(6)?;
    Ok(SyncItem {
        record_id: r.get::<_, Option<Vec<u8>>>(0)?.unwrap_or_default(),
        server_seq: r.get(1)?,
        ver: r.get::<_, i64>(2)? as u64,
        bucket: r.get(3)?,
        deleted: r.get::<_, i64>(4)? != 0,
        origin_sync: r.get::<_, i64>(5)? != 0,
        extra: extra.and_then(|e| serde_json::from_str(&e).ok()).unwrap_or(Value::Null),
    })
}

/// The item whose record has this id (a tombstone carries no natural key).
pub fn item_by_record(conn: &Connection, c: &str, record_id: &[u8]) -> DbResult<Option<(String, SyncItem)>> {
    conn.query_row(
        "SELECT natural_key, record_id, server_seq, ver, bucket, deleted, origin_sync, extra FROM sync_items
         WHERE collection = ?1 AND record_id = ?2",
        params![c, record_id],
        |r| {
            let nk: String = r.get(0)?;
            let extra: Option<String> = r.get(7)?;
            Ok((
                nk,
                SyncItem {
                    record_id: r.get::<_, Option<Vec<u8>>>(1)?.unwrap_or_default(),
                    server_seq: r.get(2)?,
                    ver: r.get::<_, i64>(3)? as u64,
                    bucket: r.get(4)?,
                    deleted: r.get::<_, i64>(5)? != 0,
                    origin_sync: r.get::<_, i64>(6)? != 0,
                    extra: extra.and_then(|e| serde_json::from_str(&e).ok()).unwrap_or(Value::Null),
                },
            ))
        },
    )
    .optional()
}

pub fn put_item(conn: &Connection, c: &str, nk: &str, it: &SyncItem) -> DbResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO sync_items(collection, natural_key, record_id, server_seq, ver, bucket, dirty, deleted, origin_sync, extra)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8, ?9)",
        params![
            c,
            nk,
            it.record_id,
            it.server_seq,
            it.ver as i64,
            it.bucket,
            i64::from(it.deleted),
            i64::from(it.origin_sync),
            if it.extra.is_null() { None } else { Some(it.extra.to_string()) }
        ],
    )?;
    Ok(())
}

pub fn items(conn: &Connection, c: &str) -> DbResult<Vec<(String, SyncItem)>> {
    let mut st = conn.prepare(
        "SELECT natural_key, record_id, server_seq, ver, bucket, deleted, origin_sync, extra FROM sync_items WHERE collection = ?1",
    )?;
    let rows = st.query_map([c], |r| {
        let nk: String = r.get(0)?;
        let extra: Option<String> = r.get(7)?;
        Ok((
            nk,
            SyncItem {
                record_id: r.get::<_, Option<Vec<u8>>>(1)?.unwrap_or_default(),
                server_seq: r.get(2)?,
                ver: r.get::<_, i64>(3)? as u64,
                bucket: r.get(4)?,
                deleted: r.get::<_, i64>(5)? != 0,
                origin_sync: r.get::<_, i64>(6)? != 0,
                extra: extra.and_then(|e| serde_json::from_str(&e).ok()).unwrap_or(Value::Null),
            },
        ))
    })?;
    rows.collect()
}

/// The queued changes, oldest first.
pub fn queue(conn: &Connection, limit: usize) -> DbResult<Vec<(String, String)>> {
    let mut st = conn.prepare("SELECT collection, natural_key FROM sync_queue ORDER BY enqueued_at, rowid LIMIT ?1")?;
    let rows = st.query_map([limit as i64], |r| Ok((r.get(0)?, r.get(1)?)))?;
    rows.collect()
}

pub fn queue_len(conn: &Connection) -> DbResult<i64> {
    conn.query_row("SELECT count(*) FROM sync_queue", [], |r| r.get(0))
}

/// The oldest queued change (ms), for "30 s after a change" (PLAN §5.3).
pub fn queue_oldest_ms(conn: &Connection) -> DbResult<Option<i64>> {
    conn.query_row("SELECT min(enqueued_at) FROM sync_queue", [], |r| r.get(0))
}

pub fn enqueue(conn: &Connection, c: &str, nk: &str, now_ms: i64) -> DbResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO sync_queue(collection, natural_key, enqueued_at) VALUES (?1, ?2, ?3)",
        params![c, nk, now_ms],
    )?;
    Ok(())
}

pub fn dequeue(conn: &Connection, c: &str, nk: &str) -> DbResult<()> {
    conn.execute("DELETE FROM sync_queue WHERE collection = ?1 AND natural_key = ?2", params![c, nk])?;
    Ok(())
}

impl Store {
    /// Sync on (`true`): the triggers start queueing local changes. Off: they stop.
    pub fn sync_set_tracking(&self, on: bool) -> DbResult<()> {
        if on {
            meta_set(&self.conn, "track", "1")
        } else {
            meta_del(&self.conn, "track")
        }
    }

    pub fn sync_conn(&self) -> &Connection {
        &self.conn
    }

    /// Runs `f` in one transaction during which the triggers don't queue (remote changes being
    /// applied must not be pushed back).
    pub fn sync_applying<R>(&mut self, f: impl FnOnce(&Connection) -> DbResult<R>) -> DbResult<R> {
        let tx = self.conn.transaction()?;
        meta_set(&tx, "applying", "1")?;
        let r = f(&tx)?;
        meta_del(&tx, "applying")?;
        tx.commit()?;
        Ok(r)
    }

    /// Sync turned off: every trace of it goes; with `delete_data`, also the items that came
    /// from sync (bookmarks, history, pins, allowlist entries that were not created here).
    pub fn sync_forget(&mut self, delete_data: bool) -> DbResult<usize> {
        let tx = self.conn.transaction()?;
        meta_set(&tx, "applying", "1")?;
        let mut removed = 0;
        if delete_data {
            let rows: Vec<(String, String)> = {
                let mut st =
                    tx.prepare("SELECT collection, natural_key FROM sync_items WHERE origin_sync = 1 AND deleted = 0")?;
                let r = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
                r.collect::<DbResult<_>>()?
            };
            for (c, nk) in rows {
                removed += match c.as_str() {
                    "bookmarks" => {
                        tx.execute("DELETE FROM bookmarks WHERE lower(hex(guid)) = ?1 AND id > 2", [nk.as_str()])?
                    }
                    "history" => tx.execute("DELETE FROM urls WHERE url = ?1", [nk.as_str()])?,
                    "pins" => tx.execute("DELETE FROM tiles WHERE url = ?1", [nk.as_str()])?,
                    "allowlist" => tx.execute("DELETE FROM adblock_allowlist WHERE host = ?1", [nk.as_str()])?,
                    _ => 0,
                };
            }
        }
        tx.execute_batch("DELETE FROM sync_items; DELETE FROM sync_queue; DELETE FROM sync_meta;")?;
        tx.commit()?;
        Ok(removed)
    }
}
