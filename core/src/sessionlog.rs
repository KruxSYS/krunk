//! Session logs (0.15.0, `krunk://sessions`): per browser run, the sites visited, the time each
//! tab and site was in the foreground, and Krunk's CPU/GPU use over time. Off by default
//! (`settings.sessionLog.enabled`); private tabs are never logged.
//!
//! The host feeds a [`Recorder`] (memory only) and writes its [`Batch`] every
//! [`FLUSH_MS`] and at exit, in one transaction. A session whose `ended` is still empty at the
//! next start was cut short (a crash): [`Store::log_repair`] ends it at its last event.

use crate::storage::{DbResult, Store};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use std::collections::HashMap;

/// Storage V10.
pub(crate) const SCHEMA_V10: &str = "
CREATE TABLE IF NOT EXISTS log_sessions (
    id INTEGER PRIMARY KEY,
    started INTEGER NOT NULL,
    ended INTEGER
);
CREATE TABLE IF NOT EXISTS log_visits (
    session_id INTEGER NOT NULL REFERENCES log_sessions(id) ON DELETE CASCADE,
    ts INTEGER NOT NULL,
    tab INTEGER NOT NULL,
    host TEXT NOT NULL,
    url TEXT NOT NULL,
    title TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS log_visits_session ON log_visits(session_id, ts);
CREATE TABLE IF NOT EXISTS log_focus (
    session_id INTEGER NOT NULL REFERENCES log_sessions(id) ON DELETE CASCADE,
    tab INTEGER NOT NULL,
    host TEXT NOT NULL,
    started INTEGER NOT NULL,
    ended INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS log_focus_session ON log_focus(session_id, started);
CREATE TABLE IF NOT EXISTS log_samples (
    session_id INTEGER NOT NULL REFERENCES log_sessions(id) ON DELETE CASCADE,
    ts INTEGER NOT NULL,
    browser REAL NOT NULL,
    gpu REAL NOT NULL,
    renderers REAL NOT NULL,
    other REAL NOT NULL,
    memory INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS log_samples_session ON log_samples(session_id, ts);
";

/// How often the host samples CPU use and re-checks the foreground tab.
pub const SAMPLE_MS: u32 = 5_000;
/// How often the buffer goes to the database.
pub const FLUSH_MS: u32 = 30_000;
/// Old sessions are purged at start and then once a day.
pub const PURGE_EVERY_MS: i64 = 86_400_000;
/// Points of the CPU chart at most (longer sessions are averaged into buckets).
pub const MAX_CHART_POINTS: usize = 360;
/// Visits and foreground intervals a detail lists at most.
pub const MAX_TIMELINE: usize = 2_000;
/// Events kept in memory at most when the database can't take them (oldest dropped).
const MAX_BUFFERED: usize = 10_000;
/// Foreground intervals of the same tab and site this close together read as one.
const JOIN_GAP_MS: i64 = 1_000;
/// Top sites shown per session in the list.
const TOP_SITES: usize = 3;

/// The site a URL counts for: its host, lower case, without a leading `www.`.
pub fn site_of(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    let host = u.host_str()?.to_ascii_lowercase();
    let host = host.strip_prefix("www.").map(str::to_string).unwrap_or(host);
    (!host.is_empty()).then_some(host)
}

/// A main-frame page that finished loading in a tab.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Visit {
    pub ts: i64,
    pub tab: u32,
    pub host: String,
    pub url: String,
    pub title: String,
}

/// What was in the foreground: the focused window's active tab and its site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Focus {
    pub tab: u32,
    pub host: String,
}

/// A stretch of foreground time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Interval {
    pub tab: u32,
    pub host: String,
    pub started: i64,
    pub ended: i64,
}

impl Interval {
    pub fn ms(&self) -> i64 {
        (self.ended - self.started).max(0)
    }
}

/// Krunk's resource use at one moment. CPU figures are percent of all processors; "GPU" is the
/// GPU process (the engine reports no GPU utilisation).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    pub ts: i64,
    pub browser: f32,
    pub gpu: f32,
    pub renderers: f32,
    /// Utility processes (network, audio, storage …).
    pub other: f32,
    /// Private memory of all processes, bytes.
    pub memory: u64,
}

/// Events not yet written.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Batch {
    pub visits: Vec<Visit>,
    pub intervals: Vec<Interval>,
    pub samples: Vec<Sample>,
}

impl Batch {
    pub fn is_empty(&self) -> bool {
        self.visits.is_empty() && self.intervals.is_empty() && self.samples.is_empty()
    }
}

fn push_capped<T>(v: &mut Vec<T>, item: T) {
    if v.len() >= MAX_BUFFERED {
        v.remove(0);
    }
    v.push(item);
}

/// The running session in memory: buffered events and the open foreground interval.
#[derive(Debug)]
pub struct Recorder {
    session: i64,
    open: Option<(Focus, i64)>,
    batch: Batch,
}

impl Recorder {
    pub fn new(session: i64) -> Recorder {
        Recorder { session, open: None, batch: Batch::default() }
    }

    pub fn session(&self) -> i64 {
        self.session
    }

    pub fn visit(&mut self, v: Visit) {
        push_capped(&mut self.batch.visits, v);
    }

    pub fn sample(&mut self, s: Sample) {
        push_capped(&mut self.batch.samples, s);
    }

    /// The foreground is now `current` (None: Krunk is in the background, minimized, the screen
    /// is locked, or the tab is private or not a web page). A change closes the open interval.
    pub fn focus(&mut self, now: i64, current: Option<Focus>) {
        if self.open.as_ref().map(|(f, _)| f) == current.as_ref() {
            return;
        }
        self.close(now);
        self.open = current.map(|f| (f, now));
    }

    fn close(&mut self, now: i64) {
        if let Some((f, started)) = self.open.take() {
            if now > started {
                push_capped(&mut self.batch.intervals, Interval { tab: f.tab, host: f.host, started, ended: now });
            }
        }
    }

    /// Nothing to write (no events, no foreground time running).
    pub fn is_idle(&self) -> bool {
        self.batch.is_empty() && self.open.is_none()
    }

    /// Everything since the last flush. The open interval is cut at `now` and goes on from
    /// there, so a crash loses at most one flush period.
    pub fn take(&mut self, now: i64) -> Batch {
        let open = self.open.as_ref().map(|(f, _)| f.clone());
        self.close(now);
        self.open = open.map(|f| (f, now));
        std::mem::take(&mut self.batch)
    }

    /// The session ends (exit, logging turned off): the open interval closes.
    pub fn finish(&mut self, now: i64) -> Batch {
        self.close(now);
        std::mem::take(&mut self.batch)
    }

    /// "Clear all": the buffered events go; time in front counts again from `now`.
    pub fn clear(&mut self, now: i64) {
        self.batch = Batch::default();
        if let Some((_, started)) = self.open.as_mut() {
            *started = now;
        }
    }
}

/// Time on one site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteTime {
    pub host: String,
    /// Foreground milliseconds.
    pub ms: i64,
    pub visits: i64,
}

/// Time of one tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabTime {
    pub tab: u32,
    /// The title of the tab's last page in the session (or its site).
    pub title: String,
    pub host: String,
    pub ms: i64,
}

/// A session in the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: i64,
    pub started: i64,
    /// None while it runs.
    pub ended: Option<i64>,
    pub duration_ms: i64,
    pub current: bool,
    /// Foreground milliseconds in total.
    pub active_ms: i64,
    pub visits: i64,
    pub sites: i64,
    pub top_sites: Vec<SiteTime>,
}

/// CPU use over a session: averages and peaks of the total (percent of all processors).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuSummary {
    pub avg_total: f32,
    pub peak_total: f32,
    pub avg_gpu: f32,
    pub peak_memory: u64,
}

/// One session in full.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    pub session: SessionSummary,
    /// In time order, [`MAX_TIMELINE`] at most (`visits_total` says how many there were).
    pub visits: Vec<Visit>,
    pub visits_total: i64,
    /// Foreground intervals in time order, adjacent pieces of one tab and site joined.
    pub focus: Vec<Interval>,
    pub sites: Vec<SiteTime>,
    pub tabs: Vec<TabTime>,
    /// Downsampled to [`MAX_CHART_POINTS`] at most.
    pub samples: Vec<Sample>,
    pub cpu: CpuSummary,
}

/// Joins adjacent intervals of the same tab and site (the flush cuts long ones into pieces).
pub fn join_intervals(mut list: Vec<Interval>) -> Vec<Interval> {
    list.sort_by_key(|i| (i.started, i.ended));
    let mut out: Vec<Interval> = Vec::with_capacity(list.len());
    for i in list {
        match out.last_mut() {
            Some(l) if l.tab == i.tab && l.host == i.host && i.started - l.ended <= JOIN_GAP_MS => {
                l.ended = l.ended.max(i.ended);
            }
            _ => out.push(i),
        }
    }
    out
}

/// At most `max` points: equal time buckets, each the average of its samples.
pub fn downsample(samples: &[Sample], max: usize) -> Vec<Sample> {
    if samples.len() <= max || max == 0 {
        return samples.to_vec();
    }
    let first = samples[0].ts;
    let span = (samples[samples.len() - 1].ts - first).max(1) as f64;
    let mut buckets: Vec<(Sample, u32)> = vec![(Sample::default(), 0); max];
    for s in samples {
        let i = ((((s.ts - first) as f64) / span) * max as f64) as usize;
        let (acc, n) = &mut buckets[i.min(max - 1)];
        acc.ts += s.ts;
        acc.browser += s.browser;
        acc.gpu += s.gpu;
        acc.renderers += s.renderers;
        acc.other += s.other;
        acc.memory += s.memory;
        *n += 1;
    }
    buckets
        .into_iter()
        .filter(|(_, n)| *n > 0)
        .map(|(s, n)| {
            let f = n as f32;
            Sample {
                ts: s.ts / n as i64,
                browser: s.browser / f,
                gpu: s.gpu / f,
                renderers: s.renderers / f,
                other: s.other / f,
                memory: s.memory / n as u64,
            }
        })
        .collect()
}

fn total(s: &Sample) -> f32 {
    s.browser + s.gpu + s.renderers + s.other
}

fn cpu_summary(samples: &[Sample]) -> CpuSummary {
    if samples.is_empty() {
        return CpuSummary::default();
    }
    let n = samples.len() as f32;
    CpuSummary {
        avg_total: samples.iter().map(total).sum::<f32>() / n,
        peak_total: samples.iter().map(total).fold(0.0, f32::max),
        avg_gpu: samples.iter().map(|s| s.gpu).sum::<f32>() / n,
        peak_memory: samples.iter().map(|s| s.memory).max().unwrap_or(0),
    }
}

/// Sites by foreground time (then visits, then name).
fn rank_sites(focus: HashMap<String, i64>, visits: HashMap<String, i64>) -> Vec<SiteTime> {
    let mut hosts: Vec<String> = focus.keys().chain(visits.keys()).cloned().collect();
    hosts.sort();
    hosts.dedup();
    let mut out: Vec<SiteTime> = hosts
        .into_iter()
        .map(|h| SiteTime {
            ms: focus.get(&h).copied().unwrap_or(0),
            visits: visits.get(&h).copied().unwrap_or(0),
            host: h,
        })
        .collect();
    out.sort_by(|a, b| b.ms.cmp(&a.ms).then(b.visits.cmp(&a.visits)).then(a.host.cmp(&b.host)));
    out
}

impl Store {
    /// A new session starting at `now`.
    pub fn log_start(&self, now: i64) -> DbResult<i64> {
        self.conn.execute("INSERT INTO log_sessions(started) VALUES (?1)", [now])?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Writes a batch in one transaction; `ended` closes the session as well.
    pub fn log_flush(&self, session: i64, batch: &Batch, ended: Option<i64>) -> DbResult<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut st = tx.prepare_cached(
                "INSERT INTO log_visits(session_id, ts, tab, host, url, title) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for v in &batch.visits {
                st.execute(params![session, v.ts, v.tab, v.host, v.url, v.title])?;
            }
            let mut st = tx.prepare_cached(
                "INSERT INTO log_focus(session_id, tab, host, started, ended) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for i in &batch.intervals {
                st.execute(params![session, i.tab, i.host, i.started, i.ended])?;
            }
            let mut st = tx.prepare_cached(
                "INSERT INTO log_samples(session_id, ts, browser, gpu, renderers, other, memory)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for s in &batch.samples {
                st.execute(params![session, s.ts, s.browser, s.gpu, s.renderers, s.other, s.memory as i64])?;
            }
        }
        if let Some(end) = ended {
            tx.execute("UPDATE log_sessions SET ended = max(started, ?2) WHERE id = ?1", params![session, end])?;
        }
        tx.commit()
    }

    /// Sessions a crash left open (no `ended`) end at their last event. Run at start, before a
    /// new session begins.
    pub fn log_repair(&self) -> DbResult<usize> {
        self.conn.execute(
            "UPDATE log_sessions SET ended = max(started,
                 coalesce((SELECT max(ts) FROM log_visits WHERE session_id = log_sessions.id), 0),
                 coalesce((SELECT max(ended) FROM log_focus WHERE session_id = log_sessions.id), 0),
                 coalesce((SELECT max(ts) FROM log_samples WHERE session_id = log_sessions.id), 0))
             WHERE ended IS NULL",
            [],
        )
    }

    /// Deletes sessions that ended more than `days` days before `now` (never `keep`).
    pub fn log_purge(&self, now: i64, days: u32, keep: Option<i64>) -> DbResult<usize> {
        let cutoff = now - i64::from(days) * 86_400_000;
        self.conn.execute(
            "DELETE FROM log_sessions WHERE coalesce(ended, started) < ?1 AND id IS NOT ?2",
            params![cutoff, keep],
        )
    }

    pub fn log_delete(&self, id: i64) -> DbResult<bool> {
        Ok(self.conn.execute("DELETE FROM log_sessions WHERE id = ?1", [id])? > 0)
    }

    /// Deletes every session but `keep`, and `keep`'s events (it goes on, empty).
    pub fn log_clear(&self, keep: Option<i64>) -> DbResult<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let n = tx.execute("DELETE FROM log_sessions WHERE id IS NOT ?1", [keep])?;
        if let Some(k) = keep {
            for t in ["log_visits", "log_focus", "log_samples"] {
                tx.execute(&format!("DELETE FROM {t} WHERE session_id = ?1"), [k])?;
            }
        }
        tx.commit()?;
        Ok(n)
    }

    /// All sessions, newest first. `current`: the running one (its duration runs to `now`).
    pub fn log_sessions(&self, now: i64, current: Option<i64>) -> DbResult<Vec<SessionSummary>> {
        let mut focus: HashMap<i64, HashMap<String, i64>> = HashMap::new();
        let mut st = self.conn.prepare(
            "SELECT session_id, host, sum(ended - started) FROM log_focus GROUP BY session_id, host",
        )?;
        for r in st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))? {
            let (s, h, ms) = r?;
            focus.entry(s).or_default().insert(h, ms);
        }
        let mut visits: HashMap<i64, HashMap<String, i64>> = HashMap::new();
        let mut st =
            self.conn.prepare("SELECT session_id, host, count(*) FROM log_visits GROUP BY session_id, host")?;
        for r in st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))? {
            let (s, h, n) = r?;
            visits.entry(s).or_default().insert(h, n);
        }
        let mut st = self.conn.prepare("SELECT id, started, ended FROM log_sessions ORDER BY started DESC, id DESC")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<i64>>(2)?)))?;
        let mut out = Vec::new();
        for r in rows {
            let (id, started, ended) = r?;
            let f = focus.remove(&id).unwrap_or_default();
            let v = visits.remove(&id).unwrap_or_default();
            out.push(summary(id, started, ended, now, current, f, v));
        }
        Ok(out)
    }

    /// One session with its timeline, time per site and tab, and the CPU series.
    pub fn log_detail(&self, id: i64, now: i64, current: Option<i64>) -> DbResult<Option<SessionDetail>> {
        let Some((started, ended)) = self
            .conn
            .query_row("SELECT started, ended FROM log_sessions WHERE id = ?1", [id], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?))
            })
            .optional()?
        else {
            return Ok(None);
        };
        let visits_total: i64 =
            self.conn.query_row("SELECT count(*) FROM log_visits WHERE session_id = ?1", [id], |r| r.get(0))?;
        let mut st = self.conn.prepare(
            "SELECT ts, tab, host, url, title FROM log_visits WHERE session_id = ?1 ORDER BY ts, rowid LIMIT ?2",
        )?;
        let visits = st
            .query_map(params![id, MAX_TIMELINE as i64], |r| {
                Ok(Visit { ts: r.get(0)?, tab: r.get(1)?, host: r.get(2)?, url: r.get(3)?, title: r.get(4)? })
            })?
            .collect::<DbResult<Vec<_>>>()?;
        let mut st = self
            .conn
            .prepare("SELECT tab, host, started, ended FROM log_focus WHERE session_id = ?1 ORDER BY started")?;
        let raw = st
            .query_map([id], |r| Ok(Interval { tab: r.get(0)?, host: r.get(1)?, started: r.get(2)?, ended: r.get(3)? }))?
            .collect::<DbResult<Vec<_>>>()?;
        let mut st = self.conn.prepare(
            "SELECT ts, browser, gpu, renderers, other, memory FROM log_samples WHERE session_id = ?1 ORDER BY ts",
        )?;
        let samples = st
            .query_map([id], |r| {
                Ok(Sample {
                    ts: r.get(0)?,
                    browser: r.get::<_, f64>(1)? as f32,
                    gpu: r.get::<_, f64>(2)? as f32,
                    renderers: r.get::<_, f64>(3)? as f32,
                    other: r.get::<_, f64>(4)? as f32,
                    memory: r.get::<_, i64>(5)?.max(0) as u64,
                })
            })?
            .collect::<DbResult<Vec<_>>>()?;
        // Per site and per tab, from every interval (not only the listed ones).
        let mut site_ms: HashMap<String, i64> = HashMap::new();
        let mut tab_ms: HashMap<u32, (i64, String)> = HashMap::new();
        for i in &raw {
            *site_ms.entry(i.host.clone()).or_default() += i.ms();
            let e = tab_ms.entry(i.tab).or_insert((0, i.host.clone()));
            e.0 += i.ms();
            e.1 = i.host.clone();
        }
        let mut site_visits: HashMap<String, i64> = HashMap::new();
        let mut st = self.conn.prepare("SELECT host, count(*) FROM log_visits WHERE session_id = ?1 GROUP BY host")?;
        for r in st.query_map([id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))? {
            let (h, n) = r?;
            site_visits.insert(h, n);
        }
        // A tab's title: its last page in the session.
        let mut titles: HashMap<u32, String> = HashMap::new();
        let mut st = self.conn.prepare(
            "SELECT tab, title FROM log_visits WHERE session_id = ?1 AND title <> '' ORDER BY ts, rowid",
        )?;
        for r in st.query_map([id], |r| Ok((r.get::<_, u32>(0)?, r.get::<_, String>(1)?)))? {
            let (t, title) = r?;
            titles.insert(t, title);
        }
        let mut tabs: Vec<TabTime> = tab_ms
            .into_iter()
            .map(|(tab, (ms, host))| TabTime { tab, title: titles.remove(&tab).unwrap_or_else(|| host.clone()), host, ms })
            .collect();
        tabs.sort_by(|a, b| b.ms.cmp(&a.ms).then(a.tab.cmp(&b.tab)));
        let session = summary(id, started, ended, now, current, site_ms.clone(), site_visits.clone());
        let mut focus = join_intervals(raw);
        focus.truncate(MAX_TIMELINE);
        Ok(Some(SessionDetail {
            session,
            visits,
            visits_total,
            focus,
            sites: rank_sites(site_ms, site_visits),
            tabs,
            cpu: cpu_summary(&samples),
            samples: downsample(&samples, MAX_CHART_POINTS),
        }))
    }
}

fn summary(
    id: i64,
    started: i64,
    ended: Option<i64>,
    now: i64,
    current: Option<i64>,
    focus: HashMap<String, i64>,
    visits: HashMap<String, i64>,
) -> SessionSummary {
    let is_current = current == Some(id);
    let end = if is_current { now } else { ended.unwrap_or(started) };
    let active_ms = focus.values().sum();
    let visit_count = visits.values().sum();
    let mut top = rank_sites(focus, visits);
    let sites = top.len() as i64;
    top.truncate(TOP_SITES);
    SessionSummary {
        id,
        started,
        ended: if is_current { None } else { ended },
        duration_ms: (end - started).max(0),
        current: is_current,
        active_ms,
        visits: visit_count,
        sites,
        top_sites: top,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_800_000_000_000;

    fn focus(tab: u32, host: &str) -> Option<Focus> {
        Some(Focus { tab, host: host.into() })
    }

    fn visit(ts: i64, tab: u32, url: &str, title: &str) -> Visit {
        Visit { ts, tab, host: site_of(url).unwrap(), url: url.into(), title: title.into() }
    }

    fn sample(ts: i64, cpu: f32) -> Sample {
        Sample { ts, browser: cpu, gpu: cpu / 2.0, renderers: cpu * 2.0, other: 0.0, memory: 100 }
    }

    #[test]
    fn sites_drop_www_and_case() {
        assert_eq!(site_of("https://WWW.Example.org/a?b").as_deref(), Some("example.org"));
        assert_eq!(site_of("http://docs.rs/").as_deref(), Some("docs.rs"));
        assert_eq!(site_of("not a url"), None);
        assert_eq!(site_of("data:text/plain,x"), None);
    }

    #[test]
    fn foreground_intervals_close_on_switch_blur_and_exit() {
        let mut r = Recorder::new(1);
        r.focus(T0, focus(1, "a.example"));
        // A re-check with the same foreground changes nothing.
        r.focus(T0 + 500, focus(1, "a.example"));
        r.focus(T0 + 1_000, focus(2, "b.example"));
        // Another site in the same tab is a new interval too.
        r.focus(T0 + 3_000, focus(2, "c.example"));
        r.focus(T0 + 4_000, None);
        r.focus(T0 + 9_000, focus(1, "a.example"));
        let b = r.finish(T0 + 10_000);
        let got: Vec<(u32, &str, i64)> = b.intervals.iter().map(|i| (i.tab, i.host.as_str(), i.ms())).collect();
        assert_eq!(
            got,
            vec![(1, "a.example", 1_000), (2, "b.example", 2_000), (2, "c.example", 1_000), (1, "a.example", 1_000)]
        );
        assert!(r.is_idle());
    }

    #[test]
    fn take_cuts_the_open_interval_and_keeps_counting() {
        let mut r = Recorder::new(1);
        r.focus(T0, focus(1, "a.example"));
        r.visit(visit(T0, 1, "https://a.example/", "A"));
        r.sample(sample(T0 + 5_000, 1.0));
        let b = r.take(T0 + 30_000);
        assert_eq!((b.visits.len(), b.samples.len(), b.intervals.len()), (1, 1, 1));
        assert_eq!(b.intervals[0].ms(), 30_000);
        assert!(!r.is_idle(), "the interval goes on");
        let b = r.take(T0 + 45_000);
        assert_eq!(b.intervals, vec![Interval { tab: 1, host: "a.example".into(), started: T0 + 30_000, ended: T0 + 45_000 }]);
        // Joined again when read.
        let joined = join_intervals(vec![
            Interval { tab: 1, host: "a.example".into(), started: T0, ended: T0 + 30_000 },
            b.intervals[0].clone(),
        ]);
        assert_eq!(joined.len(), 1);
        assert_eq!(joined[0].ms(), 45_000);
        // Nothing in front: nothing to write.
        r.focus(T0 + 50_000, None);
        r.take(T0 + 50_000);
        assert!(r.is_idle());
    }

    #[test]
    fn clear_drops_the_buffer_and_restarts_the_clock() {
        let mut r = Recorder::new(1);
        r.focus(T0, focus(1, "a.example"));
        r.visit(visit(T0, 1, "https://a.example/", "A"));
        r.clear(T0 + 10_000);
        let b = r.finish(T0 + 12_000);
        assert!(b.visits.is_empty());
        assert_eq!(b.intervals[0].ms(), 2_000);
    }

    #[test]
    fn the_buffer_is_capped() {
        let mut r = Recorder::new(1);
        for i in 0..(MAX_BUFFERED + 5) {
            r.sample(sample(i as i64, 1.0));
        }
        let b = r.take(0);
        assert_eq!(b.samples.len(), MAX_BUFFERED);
        assert_eq!(b.samples[0].ts, 5, "the oldest go first");
    }

    #[test]
    fn joining_needs_the_same_tab_and_site_and_no_gap() {
        let i = |tab, host: &str, s, e| Interval { tab, host: host.into(), started: s, ended: e };
        let joined = join_intervals(vec![
            i(1, "a", 0, 10),
            i(1, "a", 10, 20),
            i(1, "a", 2_000, 2_100),
            i(2, "a", 2_100, 2_200),
            i(2, "b", 2_200, 2_300),
        ]);
        assert_eq!(joined, vec![i(1, "a", 0, 20), i(1, "a", 2_000, 2_100), i(2, "a", 2_100, 2_200), i(2, "b", 2_200, 2_300)]);
    }

    #[test]
    fn long_series_are_averaged_into_buckets() {
        let s: Vec<Sample> = (0..1_000).map(|i| sample(T0 + i * 5_000, (i % 2) as f32 * 2.0)).collect();
        let d = downsample(&s, 100);
        assert!(d.len() <= 100 && d.len() >= 95, "{}", d.len());
        assert!(d.iter().all(|p| (p.browser - 1.0).abs() < 0.3), "pairs average out");
        assert!(d.windows(2).all(|w| w[0].ts < w[1].ts));
        assert_eq!(downsample(&s[..10], 100).len(), 10);
    }

    fn store() -> Store {
        Store::open_in_memory().unwrap()
    }

    fn count(s: &Store, table: &str) -> i64 {
        s.conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0)).unwrap()
    }

    /// A session with two tabs on three sites and some samples, written in two flushes.
    fn recorded(s: &Store, start: i64) -> i64 {
        let id = s.log_start(start).unwrap();
        let mut r = Recorder::new(id);
        r.visit(visit(start + 100, 1, "https://www.a.example/", "A home"));
        r.focus(start + 100, focus(1, "a.example"));
        r.sample(sample(start + 5_000, 1.0));
        s.log_flush(id, &r.take(start + 30_000), None).unwrap();
        r.visit(visit(start + 31_000, 2, "https://b.example/x", "B x"));
        r.focus(start + 31_000, focus(2, "b.example"));
        r.visit(visit(start + 40_000, 1, "https://a.example/two", "A two"));
        r.visit(visit(start + 41_000, 2, "https://c.example/", ""));
        r.focus(start + 41_000, focus(2, "c.example"));
        r.sample(sample(start + 35_000, 3.0));
        s.log_flush(id, &r.finish(start + 50_000), Some(start + 50_000)).unwrap();
        id
    }

    #[test]
    fn flush_writes_everything_in_one_go() {
        let s = store();
        let id = recorded(&s, T0);
        assert_eq!((count(&s, "log_visits"), count(&s, "log_focus"), count(&s, "log_samples")), (4, 4, 2));
        let ended: Option<i64> =
            s.conn.query_row("SELECT ended FROM log_sessions WHERE id = ?1", [id], |r| r.get(0)).unwrap();
        assert_eq!(ended, Some(T0 + 50_000));
        // A failing batch writes nothing (one transaction): the session is gone.
        let b = Batch { visits: vec![visit(T0, 1, "https://a.example/", "")], ..Batch::default() };
        assert!(s.log_flush(9_999, &b, None).is_err());
        assert_eq!(count(&s, "log_visits"), 4);
    }

    #[test]
    fn queries_list_and_detail() {
        let s = store();
        let old = recorded(&s, T0);
        let cur = s.log_start(T0 + 100_000).unwrap();
        let list = s.log_sessions(T0 + 160_000, Some(cur)).unwrap();
        assert_eq!(list.iter().map(|x| x.id).collect::<Vec<_>>(), vec![cur, old], "newest first");
        assert!(list[0].current && list[0].ended.is_none());
        assert_eq!(list[0].duration_ms, 60_000);
        let o = &list[1];
        assert!(!o.current);
        assert_eq!((o.duration_ms, o.active_ms, o.visits, o.sites), (50_000, 49_900, 4, 3));
        assert_eq!(
            o.top_sites.iter().map(|t| (t.host.as_str(), t.ms, t.visits)).collect::<Vec<_>>(),
            vec![("a.example", 30_900, 2), ("b.example", 10_000, 1), ("c.example", 9_000, 1)]
        );

        let d = s.log_detail(old, T0 + 160_000, Some(cur)).unwrap().unwrap();
        assert_eq!(d.visits_total, 4);
        assert_eq!(d.visits.iter().map(|v| v.ts - T0).collect::<Vec<_>>(), vec![100, 31_000, 40_000, 41_000]);
        assert_eq!(d.focus.len(), 3, "the piece cut by the first flush is joined");
        assert_eq!(d.sites[0].host, "a.example");
        assert_eq!(
            d.tabs.iter().map(|t| (t.tab, t.title.as_str(), t.ms)).collect::<Vec<_>>(),
            vec![(1, "A two", 30_900), (2, "B x", 19_000)],
            "a tab's title is its last titled page"
        );
        assert_eq!(d.samples.len(), 2);
        assert!((d.cpu.avg_total - 7.0).abs() < 1e-4, "{:?}", d.cpu);
        assert!((d.cpu.peak_total - 10.5).abs() < 1e-4);
        assert!(s.log_detail(12_345, T0, None).unwrap().is_none());
    }

    #[test]
    fn repair_ends_a_crashed_session_at_its_last_event() {
        let s = store();
        let id = s.log_start(T0).unwrap();
        let mut r = Recorder::new(id);
        r.focus(T0, focus(1, "a.example"));
        r.sample(sample(T0 + 20_000, 1.0));
        s.log_flush(id, &r.take(T0 + 30_000), None).unwrap();
        // An empty one too.
        let empty = s.log_start(T0 + 1_000).unwrap();
        let done = recorded(&s, T0 + 2_000);
        assert_eq!(s.log_repair().unwrap(), 2);
        let ended = |i: i64| -> Option<i64> {
            s.conn.query_row("SELECT ended FROM log_sessions WHERE id = ?1", [i], |r| r.get(0)).unwrap()
        };
        assert_eq!(ended(id), Some(T0 + 30_000));
        assert_eq!(ended(empty), Some(T0 + 1_000));
        assert_eq!(ended(done), Some(T0 + 52_000), "closed sessions stay as they were");
        assert_eq!(s.log_repair().unwrap(), 0);
    }

    #[test]
    fn retention_delete_and_clear() {
        let s = store();
        let day = 86_400_000;
        let old = recorded(&s, T0);
        let recent = recorded(&s, T0 + 20 * day);
        let cur = s.log_start(T0 + 31 * day).unwrap();
        // 30 days: the first one (ended T0 + 50 s) is past it 31 days later.
        assert_eq!(s.log_purge(T0 + 31 * day, 30, Some(cur)).unwrap(), 1);
        let ids: Vec<i64> = s.log_sessions(T0 + 31 * day, Some(cur)).unwrap().iter().map(|x| x.id).collect();
        assert_eq!(ids, vec![cur, recent]);
        assert!(!ids.contains(&old));
        assert_eq!(count(&s, "log_visits"), 4, "the purged session's events went with it");
        // The running session is never purged.
        assert_eq!(s.log_purge(T0 + 400 * day, 1, Some(cur)).unwrap(), 1);
        assert_eq!(s.log_sessions(T0 + 400 * day, Some(cur)).unwrap().len(), 1);

        let a = recorded(&s, T0 + 401 * day);
        assert!(s.log_delete(a).unwrap());
        assert!(!s.log_delete(a).unwrap());
        recorded(&s, T0 + 402 * day);
        let mut r = Recorder::new(cur);
        r.visit(visit(T0 + 402 * day, 1, "https://a.example/", "A"));
        s.log_flush(cur, &r.take(T0 + 402 * day), None).unwrap();
        assert_eq!(s.log_clear(Some(cur)).unwrap(), 1);
        let list = s.log_sessions(T0 + 403 * day, Some(cur)).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].visits, 0, "the running session goes on, empty");
        assert_eq!((count(&s, "log_visits"), count(&s, "log_focus"), count(&s, "log_samples")), (0, 0, 0));
        assert_eq!(s.log_clear(None).unwrap(), 1);
        assert_eq!(count(&s, "log_sessions"), 0);
    }
}
