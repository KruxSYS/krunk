//! The downloads list (state only; the host drives it from the engine's
//! download events) and the helpers for naming downloaded files.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Downloads {
    items: Vec<DownloadItem>,
    next_id: u32,
}

impl Downloads {
    pub fn new() -> Downloads {
        Downloads { items: Vec::new(), next_id: 1 }
    }

    /// Restores a saved list; downloads that were running when the browser
    /// exited are marked interrupted.
    pub fn from_json(json: &str) -> Downloads {
        let mut d: Downloads = serde_json::from_str(json).unwrap_or_default();
        d.next_id = d.items.iter().map(|i| i.id).max().unwrap_or(0) + 1;
        for i in &mut d.items {
            if !i.state.is_finished() {
                i.state = DownloadState::Interrupted;
                i.reason = Some("Browser closed".into());
            }
        }
        d
    }

    /// Serialized list for persistence (private downloads are left out).
    pub fn to_json(&self) -> String {
        let saved =
            Downloads { items: self.items.iter().filter(|i| !i.private).cloned().collect(), next_id: self.next_id };
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

    /// The page a download came from (only web pages; kept for "Copy page address").
    pub fn set_page(&mut self, id: u32, page: &str) {
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            let web =
                (page.starts_with("https://") || page.starts_with("http://")) && !crate::pages::is_pages_origin(page);
            i.page_url = web.then(|| page.to_string());
        }
    }

    pub fn set_state(&mut self, id: u32, state: DownloadState, reason: Option<String>, now: i64) {
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            i.state = state;
            i.reason = reason;
            i.finished = if state.is_finished() { i.finished.or(Some(now)) } else { None };
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
        let ext: String = out.rsplit_once('.').map(|(_, e)| format!(".{}", &e[..e.len().min(16)])).unwrap_or_default();
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

    #[test]
    fn unique_paths() {
        let taken = ["C:/dl/a.zip", "C:/dl/a (1).zip", "C:/dl/README"];
        let exists = |p: &Path| taken.iter().any(|t| Path::new(t) == p);
        assert_eq!(unique_path(Path::new("C:/dl"), "b.zip", exists), Path::new("C:/dl/b.zip"));
        assert_eq!(unique_path(Path::new("C:/dl"), "a.zip", exists), Path::new("C:/dl/a (2).zip"));
        assert_eq!(unique_path(Path::new("C:/dl"), "README", exists), Path::new("C:/dl/README (1)"));
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
