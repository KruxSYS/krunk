//! Krunk's own downloads (Krunk for Android 0.4.0): the list, its table and the rules for
//! names and for going on after a pause, all from `core::downloads`, `core::segdl` and
//! `core::dlresume`. The app saves the engine's response stream itself; this side keeps what
//! the list shows and what survives a restart. Private downloads stay in memory and are
//! never written (`core::downloads`).
use crate::error::FfiError;
use crate::store::KrunkStore;
use lb_core::dlresume;
use lb_core::downloads as dl;
use lb_core::segdl::{Segment, Validator};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum DownloadState {
    InProgress,
    Paused,
    Completed,
    /// Failed for good (the reason says why), also when the app was closed under a download
    /// that cannot go on. Final: starting over is a new download. An error on a download that
    /// can go on is `Paused` with a reason instead.
    Interrupted,
    Cancelled,
}

impl From<dl::DownloadState> for DownloadState {
    fn from(s: dl::DownloadState) -> Self {
        match s {
            dl::DownloadState::InProgress => DownloadState::InProgress,
            dl::DownloadState::Paused => DownloadState::Paused,
            dl::DownloadState::Completed => DownloadState::Completed,
            dl::DownloadState::Interrupted => DownloadState::Interrupted,
            dl::DownloadState::Cancelled => DownloadState::Cancelled,
        }
    }
}

impl From<DownloadState> for dl::DownloadState {
    fn from(s: DownloadState) -> Self {
        match s {
            DownloadState::InProgress => dl::DownloadState::InProgress,
            DownloadState::Paused => dl::DownloadState::Paused,
            DownloadState::Completed => dl::DownloadState::Completed,
            DownloadState::Interrupted => dl::DownloadState::Interrupted,
            DownloadState::Cancelled => dl::DownloadState::Cancelled,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum DownloadFilter {
    All,
    /// Running or paused.
    Active,
    Done,
    /// Failed or cancelled.
    Failed,
}

/// What lets a download go on later: the part file, the size and the server's version of the
/// file. Cookies are never saved; the engine sends them again with the next request.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DownloadResume {
    /// The part file (the bytes received so far, from the start of the file).
    pub temp: String,
    pub total: u64,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub referer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DownloadEntry {
    pub id: u32,
    pub url: String,
    pub file_name: String,
    /// Where the file is (or will be): the app decides what it puts here.
    pub path: String,
    pub state: DownloadState,
    pub received: i64,
    pub total: Option<i64>,
    /// Unix ms.
    pub started: i64,
    pub finished: Option<i64>,
    pub reason: Option<String>,
    pub private: bool,
    pub page_url: Option<String>,
    /// Smoothed bytes per second while it runs.
    pub speed: u64,
    /// Seconds left at that speed, when the size is known.
    pub eta: Option<u64>,
    pub resume: Option<DownloadResume>,
}

impl From<&dl::DownloadItem> for DownloadEntry {
    fn from(i: &dl::DownloadItem) -> Self {
        DownloadEntry {
            id: i.id,
            url: i.url.clone(),
            file_name: i.file_name.clone(),
            path: i.path.clone(),
            state: i.state.into(),
            received: i.received,
            total: i.total,
            started: i.started,
            finished: i.finished,
            reason: i.reason.clone(),
            private: i.private,
            page_url: i.page_url.clone(),
            speed: i.speed,
            eta: i.eta,
            resume: i.resume.as_ref().map(|r| DownloadResume {
                temp: r.temp.clone(),
                total: r.total,
                etag: r.etag.clone(),
                last_modified: r.last_modified.clone(),
                referer: r.referer.clone(),
            }),
        }
    }
}

/// A page of the list, newest first, with the counts for the filter chips.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DownloadPage {
    pub items: Vec<DownloadEntry>,
    pub all: u32,
    pub active: u32,
    pub done: u32,
    pub failed: u32,
}

/// The downloads list. One per app, next to the [`KrunkStore`] it saves to.
///
/// Every change of state is written at once (a row per download that is not private);
/// progress is kept in memory only. After a restart, [`DownloadList::load`] finds a download
/// that was running: paused if it saved how to go on ([`DownloadList::set_resume`]),
/// otherwise failed with "Browser closed". It is never shown as complete.
#[derive(uniffi::Object)]
pub struct DownloadList {
    store: Arc<KrunkStore>,
    list: Mutex<dl::Downloads>,
}

impl DownloadList {
    fn list(&self) -> MutexGuard<'_, dl::Downloads> {
        self.list.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Writes what changed. The list's lock is taken before the store's, everywhere.
    fn save(&self, list: &mut dl::Downloads) -> Result<(), FfiError> {
        Ok(self.store.lock().downloads_save(list)?)
    }
}

#[uniffi::export]
impl DownloadList {
    /// The list as saved; downloads that were running when the app ended are paused (they
    /// can go on) or failed, and written back that way.
    #[uniffi::constructor]
    pub fn load(store: Arc<KrunkStore>, now: i64) -> Result<Arc<Self>, FfiError> {
        let mut list = store.lock().downloads_load(now, 0)?;
        store.lock().downloads_save(&mut list)?;
        Ok(Arc::new(DownloadList { store, list: Mutex::new(list) }))
    }

    /// A new running download; `path` ends in the file's name. Returns its id.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &self,
        url: String,
        path: String,
        total: Option<i64>,
        now: i64,
        private: bool,
        page_url: Option<String>,
        resume: Option<DownloadResume>,
    ) -> Result<u32, FfiError> {
        let mut list = self.list();
        let id = list.start(&url, Path::new(&path), total, now, private);
        if let Some(page) = page_url.filter(|p| !private && !p.is_empty()) {
            list.set_page(id, &page);
        }
        list.set_resume(id, resume.map(core_resume));
        self.save(&mut list)?;
        Ok(id)
    }

    /// Bytes so far (memory only); also moves the speed and the time left.
    pub fn progress(&self, id: u32, received: i64, total: Option<i64>, now: i64) {
        self.list().progress_at(id, received, total, now);
    }

    pub fn set_state(&self, id: u32, state: DownloadState, reason: Option<String>, now: i64) -> Result<(), FfiError> {
        let mut list = self.list();
        list.set_state(id, state.into(), reason, now);
        self.save(&mut list)
    }

    /// How the download can go on (or `None`: it cannot). A finished state clears it.
    pub fn set_resume(&self, id: u32, resume: Option<DownloadResume>) -> Result<(), FfiError> {
        let mut list = self.list();
        list.set_resume(id, resume.map(core_resume));
        self.save(&mut list)
    }

    /// Where the file ended up (its name is the last part).
    pub fn set_path(&self, id: u32, path: String) -> Result<(), FfiError> {
        let mut list = self.list();
        list.set_path(id, Path::new(&path));
        self.save(&mut list)
    }

    /// Takes a finished download off the list (the file stays); false for a running one.
    pub fn remove(&self, id: u32) -> Result<bool, FfiError> {
        let mut list = self.list();
        let gone = list.remove(id);
        self.save(&mut list)?;
        Ok(gone)
    }

    /// Takes every finished download off the list (the files stay).
    pub fn clear_finished(&self) -> Result<(), FfiError> {
        let mut list = self.list();
        list.clear_finished();
        self.save(&mut list)
    }

    pub fn get(&self, id: u32) -> Result<Option<DownloadEntry>, FfiError> {
        let list = self.list();
        Ok(self.store.lock().download_get(&list, id)?.as_ref().map(DownloadEntry::from))
    }

    /// Downloads that are running or paused, oldest first.
    pub fn unfinished(&self) -> Vec<DownloadEntry> {
        let mut items: Vec<DownloadEntry> =
            self.list().items().iter().filter(|i| !i.state.is_finished()).map(DownloadEntry::from).collect();
        items.sort_by_key(|i| (i.started, i.id));
        items
    }

    pub fn active_count(&self) -> u32 {
        self.list().active_count()
    }

    /// A page of the list, newest first: `text` searches names and addresses.
    pub fn query(
        &self,
        text: String,
        filter: DownloadFilter,
        offset: u32,
        limit: u32,
    ) -> Result<DownloadPage, FfiError> {
        let list = self.list();
        let q = dl::DownloadQuery {
            text,
            filter: match filter {
                DownloadFilter::All => dl::DownloadFilter::All,
                DownloadFilter::Active => dl::DownloadFilter::Active,
                DownloadFilter::Done => dl::DownloadFilter::Done,
                DownloadFilter::Failed => dl::DownloadFilter::Failed,
            },
            offset,
            limit,
            counts: true,
            since: Vec::new(),
        };
        let page = self.store.lock().downloads_query(&list, &q)?;
        let c = page.counts.unwrap_or_default();
        Ok(DownloadPage {
            items: page.items.iter().map(DownloadEntry::from).collect(),
            all: c.all,
            active: c.active,
            done: c.done,
            failed: c.failed,
        })
    }
}

fn core_resume(r: DownloadResume) -> dl::Resume {
    dl::Resume {
        temp: r.temp,
        total: r.total,
        etag: r.etag,
        last_modified: r.last_modified,
        referer: r.referer,
        // One connection: the whole file is one range, and what is there is the part file's
        // length (asked from the file system when the download goes on).
        segments: vec![Segment { start: 0, end: r.total.saturating_sub(1), next: 0 }],
    }
}

/// One header of a request or response.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HttpHeader {
    pub name: String,
    pub value: String,
}

fn pairs(headers: Vec<HttpHeader>) -> Vec<(String, String)> {
    headers.into_iter().map(|h| (h.name, h.value)).collect()
}

/// The server's version of a file that can be continued (see [`download_resumable`]).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DownloadVersion {
    pub total: u64,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

/// Can a download that started with this response go on after a pause or an error? Only a
/// whole http(s) file of known size, not compressed, with a strong ETag or a Last-Modified
/// (`core::dlresume::resumable`); `None` otherwise (`blob:`, `data:`, streams).
#[uniffi::export]
pub fn download_resumable(url: String, status: u16, headers: Vec<HttpHeader>) -> Option<DownloadVersion> {
    let r = dlresume::resumable(&url, status, &pairs(headers)).ok()?;
    let (etag, last_modified) = match r.validator {
        Validator::ETag(e) => (Some(e), None),
        Validator::LastModified(m) => (None, Some(m)),
    };
    Some(DownloadVersion { total: r.total, etag, last_modified })
}

/// The request headers asking for the file from byte `offset` on, of the saved version only
/// (`Range`, `If-Range`, no compression); empty when no version was saved.
#[uniffi::export]
pub fn download_range_headers(offset: u64, etag: Option<String>, last_modified: Option<String>) -> Vec<HttpHeader> {
    let Some(v) = dlresume::saved_validator(etag.as_deref(), last_modified.as_deref()) else { return Vec::new() };
    dlresume::range_headers(offset, &v).into_iter().map(|(name, value)| HttpHeader { name, value }).collect()
}

/// What the answer to the range request means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ResumeVerdict {
    /// The rest of the same file: append it to the part file.
    Append,
    /// The whole file came again (no ranges, or it changed): the download cannot go on.
    WholeFile,
    /// The file changed or the answer does not fit: the download cannot go on.
    Mismatch,
}

/// Is this the answer for bytes `offset..total` of the saved version?
#[uniffi::export]
pub fn download_resume_check(
    status: u16,
    headers: Vec<HttpHeader>,
    offset: u64,
    total: u64,
    etag: Option<String>,
    last_modified: Option<String>,
) -> ResumeVerdict {
    let Some(v) = dlresume::saved_validator(etag.as_deref(), last_modified.as_deref()) else {
        return ResumeVerdict::Mismatch;
    };
    match dlresume::check(status, &pairs(headers), offset, total, &v) {
        dlresume::Verdict::Append => ResumeVerdict::Append,
        dlresume::Verdict::WholeFile => ResumeVerdict::WholeFile,
        dlresume::Verdict::Mismatch(_) => ResumeVerdict::Mismatch,
    }
}

/// Did every byte arrive (or no size was named and the stream ended cleanly)?
#[uniffi::export]
pub fn download_complete(received: u64, total: Option<u64>) -> bool {
    dlresume::complete(received, total)
}

/// A file name safe to save under (`core::downloads::sanitize_file_name`): the last path
/// component, no control or reserved characters, never empty, at most 200 characters.
#[uniffi::export]
pub fn download_clean_name(name: String) -> String {
    dl::sanitize_file_name(&name)
}

/// `name`, or `name (1).ext`, `(2)`, ... when `taken` (compared without case) has it already.
#[uniffi::export]
pub fn download_unique_name(name: String, taken: Vec<String>) -> String {
    let taken: std::collections::HashSet<String> = taken.iter().map(|t| t.to_lowercase()).collect();
    let p = dl::unique_path(Path::new(""), &name, |p| taken.contains(&p.to_string_lossy().to_lowercase()));
    p.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> (Arc<KrunkStore>, Arc<DownloadList>) {
        let store = KrunkStore::in_memory().unwrap();
        let list = DownloadList::load(store.clone(), 1).unwrap();
        (store, list)
    }

    fn resume() -> DownloadResume {
        DownloadResume {
            temp: "/data/part/1.part".into(),
            total: 1000,
            etag: Some("\"v1\"".into()),
            last_modified: None,
            referer: Some("https://a.test/page".into()),
        }
    }

    fn h(pairs: &[(&str, &str)]) -> Vec<HttpHeader> {
        pairs.iter().map(|(n, v)| HttpHeader { name: n.to_string(), value: v.to_string() }).collect()
    }

    #[test]
    fn start_progress_complete() {
        let (_s, l) = list();
        let id =
            l.start("https://a.test/f.bin".into(), "Download/f.bin".into(), Some(1000), 10, false, None, None).unwrap();
        l.progress(id, 400, None, 20);
        l.progress(id, 800, None, 1020);
        let e = l.get(id).unwrap().unwrap();
        assert_eq!(
            (e.state, e.received, e.total, e.file_name.as_str()),
            (DownloadState::InProgress, 800, Some(1000), "f.bin")
        );
        assert!(e.speed > 0 && e.eta.is_some());
        assert_eq!(l.active_count(), 1);
        l.set_path(id, "content://media/external/downloads/7/f (1).bin".into()).unwrap();
        l.set_state(id, DownloadState::Completed, None, 2000).unwrap();
        let e = l.get(id).unwrap().unwrap();
        assert_eq!((e.state, e.received, e.finished), (DownloadState::Completed, 1000, Some(2000)));
        assert_eq!(e.file_name, "f (1).bin");
        assert_eq!(e.path, "content://media/external/downloads/7/f (1).bin");
        assert_eq!(l.active_count(), 0);
        assert!(l.unfinished().is_empty());
        let page = l.query(String::new(), DownloadFilter::All, 0, 50).unwrap();
        assert_eq!((page.items.len(), page.all, page.done, page.active, page.failed), (1, 1, 1, 0, 0));
    }

    #[test]
    fn pause_resume_fail_retry_cancel() {
        let (_s, l) = list();
        let id = l
            .start("https://a.test/f".into(), "Download/f".into(), Some(1000), 10, false, None, Some(resume()))
            .unwrap();
        l.set_state(id, DownloadState::Paused, None, 20).unwrap();
        let e = l.get(id).unwrap().unwrap();
        assert_eq!(e.state, DownloadState::Paused);
        assert_eq!(e.resume, Some(resume()), "a pause keeps how to go on");
        assert!(e.finished.is_none());
        l.set_state(id, DownloadState::InProgress, None, 30).unwrap();
        // An error on a download that can go on is a pause with its reason: Retry continues it.
        l.set_state(id, DownloadState::Paused, Some("Network error".into()), 35).unwrap();
        let e = l.get(id).unwrap().unwrap();
        assert_eq!((e.state, e.reason.as_deref()), (DownloadState::Paused, Some("Network error")));
        assert_eq!(e.resume, Some(resume()));
        l.set_state(id, DownloadState::InProgress, None, 36).unwrap();
        assert_eq!(l.get(id).unwrap().unwrap().reason, None);
        // One that cannot go on has failed for good: its record of the part file is gone and
        // the entry no longer changes (starting over is a new download).
        l.set_state(id, DownloadState::Interrupted, Some("Network error".into()), 40).unwrap();
        let e = l.get(id).unwrap().unwrap();
        assert_eq!(
            (e.state, e.reason.as_deref(), e.finished),
            (DownloadState::Interrupted, Some("Network error"), Some(40))
        );
        assert!(e.resume.is_none());
        assert_eq!(l.query(String::new(), DownloadFilter::Failed, 0, 10).unwrap().items.len(), 1);
        l.set_state(id, DownloadState::InProgress, None, 50).unwrap();
        assert_eq!(l.get(id).unwrap().unwrap().state, DownloadState::Interrupted);
        assert!(l.remove(id).unwrap());
        assert!(l.get(id).unwrap().is_none());
        let id = l.start("https://a.test/f".into(), "Download/f".into(), Some(1000), 55, false, None, None).unwrap();
        assert!(!l.remove(id).unwrap(), "a running download is not removed");
        l.set_state(id, DownloadState::Cancelled, None, 60).unwrap();
        assert!(l.remove(id).unwrap());
        assert!(l.get(id).unwrap().is_none());
        assert_eq!(l.query(String::new(), DownloadFilter::All, 0, 10).unwrap().all, 0);
    }

    #[test]
    fn after_the_process_died_nothing_is_complete() {
        let (s, l) = list();
        let can = l
            .start("https://a.test/big".into(), "Download/big".into(), Some(1000), 10, false, None, Some(resume()))
            .unwrap();
        let cannot =
            l.start("blob:https://a.test/1".into(), "Download/b.txt".into(), None, 11, false, None, None).unwrap();
        let paused = l
            .start("https://a.test/p".into(), "Download/p".into(), Some(1000), 12, false, None, Some(resume()))
            .unwrap();
        l.set_state(paused, DownloadState::Paused, None, 13).unwrap();
        let done = l.start("https://a.test/d".into(), "Download/d".into(), Some(5), 14, false, None, None).unwrap();
        l.set_state(done, DownloadState::Completed, None, 15).unwrap();
        let private =
            l.start("https://a.test/s".into(), "Download/s".into(), Some(5), 16, true, None, Some(resume())).unwrap();
        l.progress(can, 600, None, 20);
        drop(l); // the process ends without another word

        let l = DownloadList::load(s, 100).unwrap();
        let e = l.get(can).unwrap().unwrap();
        assert_eq!(e.state, DownloadState::Paused);
        assert_eq!(e.reason.as_deref(), Some("Paused when Krunk closed"));
        assert_eq!(e.resume, Some(resume()));
        let e = l.get(cannot).unwrap().unwrap();
        assert_eq!((e.state, e.reason.as_deref()), (DownloadState::Interrupted, Some("Browser closed")));
        assert_eq!(l.get(paused).unwrap().unwrap().state, DownloadState::Paused);
        assert_eq!(l.get(done).unwrap().unwrap().state, DownloadState::Completed);
        assert!(l.get(private).unwrap().is_none(), "a private download was never written");
        let ids: Vec<u32> = l.unfinished().iter().map(|e| e.id).collect();
        assert_eq!(ids, [can, paused]);
        // New ids never reuse an old one (the private one's included or not, they go up).
        let next = l.start("https://a.test/n".into(), "Download/n".into(), None, 200, false, None, None).unwrap();
        assert!(next > done);
    }

    #[test]
    fn private_downloads_stay_out_of_the_table() {
        let (s, l) = list();
        let id = l
            .start(
                "https://a.test/secret.pdf".into(),
                "Download/secret.pdf".into(),
                Some(9),
                10,
                true,
                Some("https://a.test/".into()),
                None,
            )
            .unwrap();
        let e = l.get(id).unwrap().unwrap();
        assert!(e.private);
        assert!(e.page_url.is_none(), "the page of a private download is not kept");
        assert_eq!(l.unfinished().len(), 1, "shown while it runs");
        l.set_state(id, DownloadState::Completed, None, 20).unwrap();
        assert!(l.remove(id).unwrap());
        assert!(l.get(id).unwrap().is_none());
        assert_eq!(l.query("secret".into(), DownloadFilter::All, 0, 10).unwrap().items.len(), 0);
        let rows: i64 = s
            .lock()
            .downloads_query(
                &dl::Downloads::new(),
                &dl::DownloadQuery { limit: 10, counts: true, ..Default::default() },
            )
            .unwrap()
            .counts
            .unwrap()
            .all
            .into();
        assert_eq!(rows, 0);
    }

    #[test]
    fn search_filters_and_clear() {
        let (_s, l) = list();
        for (n, name) in ["report.pdf", "photo.jpg", "notes.txt"].iter().enumerate() {
            let id = l
                .start(
                    format!("https://a.test/{name}"),
                    format!("Download/{name}"),
                    None,
                    10 + n as i64,
                    false,
                    None,
                    None,
                )
                .unwrap();
            if n < 2 {
                l.set_state(id, DownloadState::Completed, None, 100).unwrap();
            }
        }
        let p = l.query(String::new(), DownloadFilter::All, 0, 10).unwrap();
        assert_eq!(
            p.items.iter().map(|i| i.file_name.as_str()).collect::<Vec<_>>(),
            ["notes.txt", "photo.jpg", "report.pdf"]
        );
        assert_eq!((p.all, p.active, p.done), (3, 1, 2));
        assert_eq!(l.query("PHOTO".into(), DownloadFilter::All, 0, 10).unwrap().items.len(), 1);
        assert_eq!(l.query(String::new(), DownloadFilter::Active, 0, 10).unwrap().items.len(), 1);
        assert_eq!(l.query(String::new(), DownloadFilter::All, 1, 1).unwrap().items[0].file_name, "photo.jpg");
        l.clear_finished().unwrap();
        let p = l.query(String::new(), DownloadFilter::All, 0, 10).unwrap();
        assert_eq!((p.items.len(), p.all), (1, 1), "the running one stays");
    }

    #[test]
    fn resume_rules() {
        let v = download_resumable(
            "https://a.test/f".into(),
            200,
            h(&[("Content-Length", "1000"), ("ETag", "\"v1\""), ("Accept-Ranges", "bytes")]),
        )
        .unwrap();
        assert_eq!(v, DownloadVersion { total: 1000, etag: Some("\"v1\"".into()), last_modified: None });
        assert!(download_resumable(
            "blob:https://a.test/x".into(),
            200,
            h(&[("Content-Length", "10"), ("ETag", "\"a\"")])
        )
        .is_none());
        assert!(download_resumable("data:text/plain,hi".into(), 200, vec![]).is_none());
        assert!(download_resumable("https://a.test/stream".into(), 200, h(&[("ETag", "\"a\"")])).is_none());

        let req = download_range_headers(400, v.etag.clone(), None);
        assert!(req.contains(&HttpHeader { name: "Range".into(), value: "bytes=400-".into() }));
        assert!(req.contains(&HttpHeader { name: "If-Range".into(), value: "\"v1\"".into() }));
        assert!(download_range_headers(400, None, None).is_empty());

        let ok = h(&[("Content-Range", "bytes 400-999/1000"), ("ETag", "\"v1\"")]);
        assert_eq!(download_resume_check(206, ok.clone(), 400, 1000, v.etag.clone(), None), ResumeVerdict::Append);
        assert_eq!(download_resume_check(200, vec![], 400, 1000, v.etag.clone(), None), ResumeVerdict::WholeFile);
        let changed = h(&[("Content-Range", "bytes 400-999/1000"), ("ETag", "\"v2\"")]);
        assert_eq!(download_resume_check(206, changed, 400, 1000, v.etag.clone(), None), ResumeVerdict::Mismatch);
        assert_eq!(download_resume_check(206, ok, 400, 1000, None, None), ResumeVerdict::Mismatch);
        assert!(download_complete(1000, Some(1000)));
        assert!(!download_complete(999, Some(1000)), "a short file is never complete");
        assert!(download_complete(7, None));
    }

    #[test]
    fn names() {
        assert_eq!(download_clean_name("../../etc/passwd".into()), "passwd");
        assert_eq!(download_clean_name("a<b>:c?.txt".into()), "a_b__c_.txt");
        assert_eq!(download_clean_name("".into()), "download");
        assert_eq!(download_clean_name("report.pdf. ".into()), "report.pdf");
        assert!(download_clean_name("x".repeat(400) + ".tar.gz").chars().count() <= 200);
        assert_eq!(download_unique_name("a.txt".into(), vec![]), "a.txt");
        assert_eq!(download_unique_name("a.txt".into(), vec!["A.TXT".into()]), "a (1).txt");
        assert_eq!(download_unique_name("a.txt".into(), vec!["a.txt".into(), "a (1).txt".into()]), "a (2).txt");
        assert_eq!(download_unique_name("noext".into(), vec!["noext".into()]), "noext (1)");
    }
}
