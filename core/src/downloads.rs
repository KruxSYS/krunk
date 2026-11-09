//! The downloads list (state only; the host drives it from the engine's
//! download events) and the helpers for naming downloaded files.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Why a download stopped when the browser closed (a download in parts can go on; an engine
/// download is retried).
pub const PAUSED_BY_CLOSE: &str = "Paused when Krunk closed";
pub const INTERRUPTED_BY_CLOSE: &str = "Browser closed";

/// Entries kept in the list (oldest finished ones are dropped first).
pub const MAX_ITEMS: usize = 200;

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
    items: Vec<DownloadItem>,
    next_id: u32,
    /// Speed meters of the running downloads (never saved).
    #[serde(skip)]
    meters: std::collections::HashMap<u32, Meter>,
}

impl Downloads {
    pub fn new() -> Downloads {
        Downloads { items: Vec::new(), next_id: 1, meters: Default::default() }
    }

    /// Restores a saved list; downloads that were running when the browser exited are marked
    /// interrupted, except downloads in parts that saved where they stood (0.7.0): those are
    /// paused, to go on with Resume.
    pub fn from_json(json: &str) -> Downloads {
        let mut d: Downloads = serde_json::from_str(json).unwrap_or_default();
        d.next_id = d.items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
        for i in &mut d.items {
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
        }
        d
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
        }
    }

    /// Serialized list for persistence (private downloads are left out).
    pub fn to_json(&self) -> String {
        let saved = Downloads {
            items: self.items.iter().filter(|i| !i.private).cloned().collect(),
            next_id: self.next_id,
            meters: Default::default(),
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
        // Drop the oldest finished entries beyond the cap.
        while self.items.len() > MAX_ITEMS {
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
        }
    }

    /// The page a download came from (only web pages; kept for "Copy page address").
    pub fn set_page(&mut self, id: u32, page: &str) {
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            let web =
                (page.starts_with("https://") || page.starts_with("http://")) && !crate::pages::is_pages_origin(page);
            i.page_url = web.then(|| page.to_string());
        }
    }

    /// Fetched in parts: over `connections`, with these part files (cleared when it ends).
    pub fn set_parts(&mut self, id: u32, connections: u32, parts: Vec<String>) {
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            i.connections = Some(connections);
            i.parts = parts;
        }
    }

    /// Part files of downloads that were running when the browser last exited (for cleanup);
    /// they are forgotten here.
    pub fn take_leftover_parts(&mut self) -> Vec<String> {
        self.items.iter_mut().filter(|i| i.state.is_finished()).flat_map(|i| std::mem::take(&mut i.parts)).collect()
    }

    pub fn set_state(&mut self, id: u32, state: DownloadState, reason: Option<String>, now: i64) {
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
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
            i.path = path.to_string_lossy().into_owned();
            i.file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        }
    }

    pub fn remove(&mut self, id: u32) -> bool {
        let before = self.items.len();
        self.items.retain(|i| i.id != id || !i.state.is_finished());
        before != self.items.len()
    }

    pub fn clear_finished(&mut self) {
        self.items.retain(|i| !i.state.is_finished());
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
        let mut d = Downloads::new();
        for i in 0..MAX_ITEMS + 10 {
            let id = d.start("https://e.com/", Path::new("f"), None, i as i64, false);
            d.set_state(id, DownloadState::Completed, None, 9);
        }
        assert_eq!(d.items().len(), MAX_ITEMS);
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
