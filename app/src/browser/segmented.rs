//! Large downloads fetched in parts over several connections (0.4.0; the decisions are in
//! `lb_core::segdl`). The engine announces a download; while it waits, the host asks the
//! server for the first byte over WinHTTP, with the cookies the engine would send for that
//! address and the engine's user agent. If the server answers with a part, the engine's
//! download is cancelled and worker threads fetch the parts. The parts carry the cookies and
//! the Referer the engine itself sent (seen on its request), nothing more. Otherwise the
//! engine's download simply goes on. Private tabs, SOCKS proxies and a setting of 1 connection
//! always use the engine's download.
//!
//! 0.7.0 (R9): all parts go into one file of the full size (`<file>.krunkpart`, each worker
//! writing at its own offsets), renamed when complete. A connection that finishes takes over
//! a piece of the part that would take longest (`segdl::next_work`), so a slow connection no
//! longer holds up the end. Where the download stands is saved with the list: after a restart
//! it is paused and Resume goes on, once the server confirms the same file (same size and
//! validator, ranges still served). A changed file or a server that stops sending ranges is
//! downloaded again by the engine, saying why.

use super::Browser;
use crate::ffi::cef::{cookie_header, DownloadStart, Profile};
use crate::ffi::http::{self, ProxyMode};
use crate::log::{info, warning};
use lb_core::downloads::{DownloadState, Resume};
use lb_core::segdl::{self, Part, Probed, Validator};
use lb_core::settings::{updater_proxy, UpdaterProxy};
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::os::windows::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Timer (on the app window): progress of downloads fetched in parts.
pub(super) const PARTS_TIMER: usize = 13;
const PARTS_TICK_MS: u32 = 400;
/// Where a running download stands is saved this often (in ticks).
const SAVE_EVERY_TICKS: u32 = 8;
/// The probe gives up (and the engine downloads) after this.
const PROBE_TIMEOUT_MS: i32 = 8_000;
const PART_TIMEOUT_MS: i32 = 30_000;
/// A part that fails is tried again this often (network errors only).
const RETRIES: u32 = 3;
/// A connection's speed is measured over at least this long.
const RATE_SAMPLE_MS: u64 = 250;

const RUN: u8 = 0;
const PAUSE: u8 = 1;
const CANCEL: u8 = 2;

/// The chunk callback's way to end a request whose part was shortened by a split (not an error).
const PART_COMPLETE: &str = "part complete";

/// What the worker threads share.
struct Shared {
    received: AtomicU64,
    /// RUN, PAUSE or CANCEL.
    stop: AtomicU8,
    parts: Mutex<Vec<Part>>,
}

/// How the parts are fetched (fixed for the download).
#[derive(Clone)]
struct Fetch {
    url: String,
    total: u64,
    validator: Validator,
    /// Cookie (asked from the engine, never saved) and Referer.
    headers: Vec<(String, String)>,
    referer: Option<String>,
    proxy: ProxyMode,
    agent: String,
    connections: u32,
}

/// A download fetched in parts.
pub(super) struct PartsJob {
    fetch: Fetch,
    /// The file being written, and the download's name for it when complete.
    temp: PathBuf,
    path: PathBuf,
    shared: Arc<Shared>,
    running: bool,
    tab: Option<TabId>,
}

#[derive(Default)]
pub(super) struct PartsJobs {
    jobs: RefCell<HashMap<u32, PartsJob>>,
    /// Addresses the engine downloads itself next time (a part download fell back to it).
    engine_only: RefCell<HashSet<String>>,
    ticks: Cell<u32>,
}

/// How a run of the workers ended.
enum Outcome {
    Done,
    Stopped,
    Failed(String),
    /// The file changed on the server or ranges stopped: the engine downloads it again.
    Restart(String),
}

/// Why the probe said no.
enum ProbeError {
    /// The server answered, but not with parts of this file.
    Refused(String),
    /// No answer.
    Network(String),
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".krunkpart");
    path.with_file_name(name)
}

fn validator_of(r: &Resume) -> Option<Validator> {
    match (&r.etag, &r.last_modified) {
        (Some(e), _) => Some(Validator::ETag(e.clone())),
        (None, Some(m)) => Some(Validator::LastModified(m.clone())),
        _ => None,
    }
}

impl Browser {
    /// A download is starting into `path`: fetch it in parts if it qualifies (then `start`
    /// is used up), else hand `start` back for the engine's own download.
    pub(super) fn try_parts(
        &self,
        start: DownloadStart,
        path: PathBuf,
        private: bool,
        tab: Option<TabId>,
    ) -> Option<(DownloadStart, PathBuf)> {
        if self.parts_jobs.engine_only.borrow_mut().remove(&start.url) {
            info!("download: one connection (downloading again after a change on the server)");
            return Some((start, path));
        }
        let (connections, proxy, agent) = self.parts_settings();
        let web = start.url.starts_with("https://") || start.url.starts_with("http://");
        let small = start.total.is_some_and(|t| (t as u64) < segdl::MIN_SEGMENTED);
        let Some(proxy) = proxy.filter(|_| connections > 1 && !private && web && !small) else {
            return Some((start, path));
        };
        let page = tab.and_then(|t| self.tab(t).map(|t| t.url.clone()));
        // Exactly the Referer the engine sent for this download (none if it sent none or the
        // request was not seen): never more than the engine would say.
        let referer = tab
            .and_then(|t| self.views.borrow().get(&t).and_then(|v| v.net.engine_referrer(&start.url)))
            .filter(|r| !r.is_empty());
        let url = start.url.clone();
        self.with_part_headers(
            url.clone(),
            referer.clone(),
            proxy.clone(),
            agent.clone(),
            move |b, headers, result| {
                match result {
                    Ok(p) => {
                        // The engine's download is not needed.
                        b.abandon_download(start);
                        let fetch = Fetch {
                            url,
                            total: p.total,
                            validator: p.validator,
                            headers,
                            referer,
                            proxy,
                            agent,
                            connections,
                        };
                        b.start_parts(fetch, path, tab, page);
                    }
                    Err(ProbeError::Refused(why) | ProbeError::Network(why)) => {
                        info!("download: one connection ({why})");
                        b.accept_download(start, path, private, tab);
                    }
                }
            },
        );
        None
    }

    /// Connections, proxy (`None`: parts are not used with it) and user agent.
    fn parts_settings(&self) -> (u32, Option<ProxyMode>, String) {
        let s = self.settings.borrow();
        let agent = if s.user_agent.is_empty() { crate::ffi::cef::default_user_agent() } else { s.user_agent.clone() };
        let proxy = match updater_proxy(&s.proxy) {
            _ if s.proxy.trim() == "direct://" => None,
            UpdaterProxy::System => Some(ProxyMode::System),
            UpdaterProxy::Named(p) => Some(ProxyMode::Named(p)),
            UpdaterProxy::Skip => None,
        };
        (s.download_connections, proxy, agent)
    }

    /// The engine's cookies for `url` and the Referer as request headers, then the probe on a
    /// worker thread; `done` gets the headers and the probe's answer.
    fn with_part_headers(
        &self,
        url: String,
        referer: Option<String>,
        proxy: ProxyMode,
        agent: String,
        done: impl FnOnce(&Browser, Vec<(String, String)>, Result<Probed, ProbeError>) + 'static,
    ) {
        let weak = self.me.clone();
        cookie_header(Profile::Default, &url.clone(), move |cookies| {
            let Some(b) = weak.upgrade() else { return };
            let mut headers = Vec::new();
            if !cookies.is_empty() {
                headers.push(("Cookie".to_string(), cookies));
            }
            if let Some(r) = referer {
                headers.push(("Referer".to_string(), r));
            }
            let probe_headers = headers.clone();
            b.spawn_job(
                "download-probe",
                move || probe(&url, &probe_headers, &proxy, &agent),
                move |b, result| done(b, headers, result),
            );
        });
    }

    fn start_parts(&self, fetch: Fetch, path: PathBuf, tab: Option<TabId>, page: Option<String>) {
        let parts = segdl::plan(fetch.total, fetch.connections);
        let temp = temp_path(&path);
        let id = {
            let mut d = self.downloads.borrow_mut();
            let id = d.start(&fetch.url, &path, Some(fetch.total as i64), now_ms(), false);
            if let Some(p) = &page {
                d.set_page(id, p);
            }
            d.set_parts(id, parts.len() as u32, vec![temp.to_string_lossy().into_owned()]);
            id
        };
        info!("download {id} started: {} bytes over {} connections", fetch.total, parts.len());
        self.download_flight(tab);
        if let Some(v) = tab.and_then(|t| self.views.borrow().get(&t).map(|v| v.downloads.clone())) {
            v.set(v.get() + 1);
        }
        let shared =
            Arc::new(Shared { received: AtomicU64::new(0), stop: AtomicU8::new(RUN), parts: Mutex::new(parts) });
        self.parts_jobs.jobs.borrow_mut().insert(id, PartsJob { fetch, temp, path, shared, running: false, tab });
        self.save_parts_state(id);
        self.save_downloads();
        self.push_downloads();
        self.run_parts(id);
    }

    /// Saves where download `id` stands (the unfinished ranges) with the list.
    fn save_parts_state(&self, id: u32) {
        let resume = self.parts_jobs.jobs.borrow().get(&id).map(|j| {
            let segments = j.shared.parts.lock().map(|p| segdl::snapshot(&p)).unwrap_or_default();
            let (etag, last_modified) = match &j.fetch.validator {
                Validator::ETag(e) => (Some(e.clone()), None),
                Validator::LastModified(m) => (None, Some(m.clone())),
            };
            Resume {
                temp: j.temp.to_string_lossy().into_owned(),
                total: j.fetch.total,
                etag,
                last_modified,
                referer: j.fetch.referer.clone(),
                segments,
            }
        });
        if let Some(r) = resume {
            self.downloads.borrow_mut().set_resume(id, Some(r));
        }
    }

    /// (Re)starts the workers of download `id` from where its parts are.
    fn run_parts(&self, id: u32) {
        let (fetch, temp, path, shared) = {
            let mut jobs = self.parts_jobs.jobs.borrow_mut();
            let Some(job) = jobs.get_mut(&id).filter(|j| !j.running) else { return };
            job.running = true;
            job.shared.stop.store(RUN, Ordering::SeqCst);
            (job.fetch.clone(), job.temp.clone(), job.path.clone(), job.shared.clone())
        };
        if let Some(app) = self.app() {
            app.set_timer(PARTS_TIMER, PARTS_TICK_MS);
        }
        self.spawn_job(
            "download-parts",
            move || run_workers(id, &fetch, &temp, &path, &shared),
            move |b, outcome| b.parts_ended(id, outcome),
        );
    }

    fn parts_ended(&self, id: u32, outcome: Outcome) {
        let Some((stop, tab, temp, url)) = self.parts_jobs.jobs.borrow_mut().get_mut(&id).map(|j| {
            j.running = false;
            (j.shared.stop.load(Ordering::SeqCst), j.tab, j.temp.clone(), j.fetch.url.clone())
        }) else {
            return;
        };
        let now = now_ms();
        let finished = match outcome {
            Outcome::Done => {
                self.downloads.borrow_mut().set_state(id, DownloadState::Completed, None, now);
                self.mark_of_the_web(id);
                info!("download {id} finished: Completed (in parts)");
                true
            }
            Outcome::Stopped if stop == PAUSE => {
                self.save_parts_state(id);
                self.downloads.borrow_mut().set_state(id, DownloadState::Paused, None, now);
                false
            }
            Outcome::Stopped => {
                let _ = std::fs::remove_file(&temp);
                self.downloads.borrow_mut().set_state(id, DownloadState::Cancelled, None, now);
                info!("download {id} finished: Cancelled");
                true
            }
            Outcome::Failed(why) => {
                warning!("download {id} failed: {why}");
                let _ = std::fs::remove_file(&temp);
                self.downloads.borrow_mut().set_state(id, DownloadState::Interrupted, Some(why), now);
                true
            }
            Outcome::Restart(why) => {
                let _ = std::fs::remove_file(&temp);
                self.parts_restart_with_engine(id, &url, &why);
                true
            }
        };
        if finished {
            self.parts_jobs.jobs.borrow_mut().remove(&id);
            if let Some(v) = tab.and_then(|t| self.views.borrow().get(&t).map(|v| v.downloads.clone())) {
                v.set(v.get().saturating_sub(1));
            }
        }
        self.save_downloads();
        self.push_downloads();
    }

    /// The file changed on the server (or it stopped sending parts): this entry ends saying so
    /// and the engine downloads the file again from the start.
    fn parts_restart_with_engine(&self, id: u32, url: &str, why: &str) {
        warning!("download {id}: {why}; the engine downloads it again");
        self.downloads.borrow_mut().set_state(
            id,
            DownloadState::Interrupted,
            Some(format!("{why} — downloading it again with one connection")),
            now_ms(),
        );
        self.parts_jobs.engine_only.borrow_mut().insert(url.to_string());
        match self.download_view(false) {
            Some(view) => view.start_download(url),
            None => warning!("download {id}: no tab to download it again from"),
        }
    }

    /// `PARTS_TIMER`: progress into the list; where each download stands is saved now and then.
    pub(super) fn parts_tick(&self) {
        let progress: Vec<(u32, u64, u64, Vec<lb_core::downloads::PartProgress>)> = self
            .parts_jobs
            .jobs
            .borrow()
            .iter()
            .filter(|(_, j)| j.running)
            .map(|(id, j)| {
                let mut parts: Vec<lb_core::downloads::PartProgress> = j
                    .shared
                    .parts
                    .lock()
                    .map(|p| {
                        p.iter()
                            .map(|part| lb_core::downloads::PartProgress {
                                done: part.next.saturating_sub(part.start),
                                len: (part.end - part.start).saturating_add(1),
                                start: part.start,
                                conn: part.conn,
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                parts.sort_by_key(|p| p.start);
                (*id, j.shared.received.load(Ordering::Relaxed), j.fetch.total, parts)
            })
            .collect();
        if progress.is_empty() {
            if let Some(app) = self.app() {
                app.kill_timer(PARTS_TIMER);
            }
            return;
        }
        let tick = self.parts_jobs.ticks.get().wrapping_add(1);
        self.parts_jobs.ticks.set(tick);
        let save = tick.is_multiple_of(SAVE_EVERY_TICKS);
        {
            let now = now_ms();
            let mut d = self.downloads.borrow_mut();
            for (id, received, total, parts) in &progress {
                d.progress_at(*id, *received as i64, Some(*total as i64), now);
                d.set_part_progress(*id, parts.clone());
            }
        }
        if save {
            for (id, ..) in &progress {
                self.save_parts_state(*id);
            }
            self.save_downloads();
        }
        self.push_downloads();
    }

    /// Pause, resume or cancel a download fetched in parts; `false` if `id` is not one.
    pub(super) fn parts_control(&self, id: u32, action: lb_core::protocol::DownloadAction) -> bool {
        use lb_core::protocol::DownloadAction as A;
        let state = self.parts_jobs.jobs.borrow().get(&id).map(|j| (j.running, j.shared.clone(), j.temp.clone()));
        let Some((running, shared, temp)) = state else {
            return self.saved_parts_control(id, action);
        };
        match action {
            A::Pause if running => shared.stop.store(PAUSE, Ordering::SeqCst),
            A::Resume if !running => {
                self.downloads.borrow_mut().set_state(id, DownloadState::InProgress, None, now_ms());
                self.run_parts(id);
            }
            A::Cancel if running => {
                // The list says so at once (0.11.4): the connections stop on their next piece of
                // data, and the end (`parts_ended`) removes the partial file.
                shared.stop.store(CANCEL, Ordering::SeqCst);
                self.downloads.borrow_mut().set_state(id, DownloadState::Cancelled, None, now_ms());
                info!("download {id}: cancelled (its connections are stopping)");
                self.save_downloads();
                self.push_downloads();
            }
            A::Cancel => {
                // Paused: nothing runs, so it ends here.
                shared.stop.store(CANCEL, Ordering::SeqCst);
                let _ = std::fs::remove_file(&temp);
                self.parts_ended(id, Outcome::Stopped);
            }
            _ => {}
        }
        true
    }

    /// Resume or cancel a download in parts saved before Krunk last closed (no job yet).
    fn saved_parts_control(&self, id: u32, action: lb_core::protocol::DownloadAction) -> bool {
        use lb_core::protocol::DownloadAction as A;
        let item = self.downloads.borrow().get(id).cloned();
        let Some((item, resume)) = item.and_then(|i| i.resume.clone().map(|r| (i, r))) else { return false };
        if item.state != DownloadState::Paused {
            return false;
        }
        match action {
            A::Cancel => {
                let _ = std::fs::remove_file(&resume.temp);
                self.downloads.borrow_mut().set_state(id, DownloadState::Cancelled, None, now_ms());
                info!("download {id} finished: Cancelled (saved)");
                self.save_downloads();
            }
            A::Resume => self.resume_saved_parts(id, item.url, PathBuf::from(&item.path), resume),
            _ => {}
        }
        true
    }

    /// Goes on with a saved download in parts once the server confirms the same file.
    fn resume_saved_parts(&self, id: u32, url: String, path: PathBuf, resume: Resume) {
        let (connections, proxy, agent) = self.parts_settings();
        let Some(proxy) = proxy else {
            self.parts_restart_with_engine(id, &url, "The proxy setting no longer allows parts");
            return;
        };
        {
            let mut d = self.downloads.borrow_mut();
            d.set_state(id, DownloadState::InProgress, None, now_ms());
        }
        self.push_downloads();
        info!("download {id}: resuming after a restart, checking the file on the server");
        let referer = resume.referer.clone();
        self.with_part_headers(
            url.clone(),
            referer.clone(),
            proxy.clone(),
            agent.clone(),
            move |b, headers, result| {
                let temp = PathBuf::from(&resume.temp);
                let same = |p: &Probed| p.total == resume.total && Some(&p.validator) == validator_of(&resume).as_ref();
                let parts = segdl::restore(&resume.segments, resume.total);
                let file_ok = std::fs::metadata(&temp).is_ok_and(|m| m.len() == resume.total);
                match result {
                    Ok(p) if same(&p) && file_ok && parts.is_some() => {
                        let parts = parts.unwrap_or_default();
                        let left: u64 = parts.iter().map(Part::left).sum();
                        info!("download {id}: the same file; {left} bytes left in {} ranges", parts.len());
                        let fetch = Fetch {
                            url,
                            total: p.total,
                            validator: p.validator,
                            headers,
                            referer,
                            proxy,
                            agent,
                            connections,
                        };
                        let shared = Arc::new(Shared {
                            received: AtomicU64::new(resume.total - left),
                            stop: AtomicU8::new(RUN),
                            parts: Mutex::new(parts),
                        });
                        b.downloads.borrow_mut().set_parts(id, connections, vec![resume.temp.clone()]);
                        b.parts_jobs
                            .jobs
                            .borrow_mut()
                            .insert(id, PartsJob { fetch, temp, path, shared, running: false, tab: None });
                        b.run_parts(id);
                    }
                    Ok(_) => {
                        let _ = std::fs::remove_file(&temp);
                        let why = if !file_ok || parts.is_none() {
                            "The partly downloaded file is gone or damaged"
                        } else {
                            "The file changed on the server"
                        };
                        b.parts_restart_with_engine(id, &url, why);
                    }
                    Err(ProbeError::Refused(why)) => {
                        let _ = std::fs::remove_file(&temp);
                        b.parts_restart_with_engine(id, &url, &format!("The server no longer sends parts ({why})"));
                    }
                    Err(ProbeError::Network(why)) => {
                        warning!("download {id}: resume: {why}");
                        b.downloads.borrow_mut().set_state(
                            id,
                            DownloadState::Paused,
                            Some("Could not reach the server; try Resume again".into()),
                            now_ms(),
                        );
                    }
                }
                b.save_downloads();
                b.push_downloads();
            },
        );
    }

    /// At exit: running workers stop and where they stand is saved, so the downloads can go on
    /// after the next start.
    pub(super) fn stop_parts(&self) {
        let ids: Vec<u32> = self.parts_jobs.jobs.borrow().keys().copied().collect();
        for id in &ids {
            if let Some(j) = self.parts_jobs.jobs.borrow().get(id) {
                j.shared.stop.store(PAUSE, Ordering::SeqCst);
            }
            self.save_parts_state(*id);
        }
        if !ids.is_empty() {
            self.save_downloads();
        }
    }

    /// At start: part files of downloads that were running when Krunk last exited and cannot
    /// go on (0.6.0's `.partN` files, or a download without saved state).
    pub(super) fn remove_leftover_parts(&self) {
        let files = self.downloads.borrow_mut().take_leftover_parts();
        for f in &files {
            let _ = std::fs::remove_file(f);
        }
        if !files.is_empty() {
            info!("removed {} part files of interrupted downloads", files.len());
            self.save_downloads();
        }
    }
}

fn options<'a>(proxy: &'a ProxyMode, agent: &'a str, timeout_ms: i32) -> http::Options<'a> {
    http::Options { proxy, timeout_ms, user_agent: agent, allow_http: true }
}

/// The first byte, to see whether the server sends parts.
fn probe(url: &str, headers: &[(String, String)], proxy: &ProxyMode, agent: &str) -> Result<Probed, ProbeError> {
    let mut decided: Option<Result<Probed, String>> = None;
    let r = http::get_range(
        url,
        (0, 0),
        headers,
        &options(proxy, agent, PROBE_TIMEOUT_MS),
        &mut |raw| {
            let d = segdl::probe(&segdl::parse_head(raw)).map_err(str::to_string);
            let ok = d.is_ok();
            decided = Some(d);
            if ok {
                Ok(())
            } else {
                Err("no parts".into())
            }
        },
        &mut |_| Ok(()),
    );
    match (decided, r) {
        (Some(Ok(p)), Ok(_)) => Ok(p),
        (Some(Err(why)), _) => Err(ProbeError::Refused(why)),
        (_, Err(e)) => Err(ProbeError::Network(e)),
        (None, Ok(_)) => Err(ProbeError::Network("no answer".into())),
    }
}

/// Runs the connections until every part is done, one fails for good, or the download is
/// paused or cancelled; each connection takes the next piece of work when its part is done.
/// Done: the file gets the download's name.
fn run_workers(id: u32, fetch: &Fetch, temp: &Path, path: &Path, shared: &Arc<Shared>) -> Outcome {
    // The whole file, allocated once (a resumed download keeps its bytes).
    let prepared = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(temp).and_then(|f| {
        if f.metadata()?.len() != fetch.total {
            f.set_len(fetch.total)
        } else {
            Ok(())
        }
    });
    if let Err(e) = prepared {
        return Outcome::Failed(format!("could not write the file ({e})"));
    }
    let left: u64 = match shared.parts.lock() {
        Ok(mut p) => {
            // Nobody works on anything yet.
            for part in p.iter_mut() {
                part.active = false;
                part.busy = 0;
            }
            p.iter().map(Part::left).sum()
        }
        Err(_) => return Outcome::Failed("internal error".into()),
    };
    shared.received.store(fetch.total - left, Ordering::SeqCst);
    let failure: Mutex<Option<Outcome>> = Mutex::new(None);
    std::thread::scope(|scope| {
        for conn in 0..fetch.connections.max(1) {
            let failure = &failure;
            scope.spawn(move || {
                if let Err(o) = worker(id, fetch, temp, shared, conn) {
                    if shared.stop.load(Ordering::SeqCst) == RUN {
                        // One connection failed for good: the others stop too.
                        if let Ok(mut f) = failure.lock() {
                            f.get_or_insert(o);
                        }
                        shared.stop.store(CANCEL, Ordering::SeqCst);
                    }
                }
            });
        }
    });
    if let Some(o) = failure.into_inner().ok().flatten() {
        return o;
    }
    let all_done = shared.parts.lock().map(|p| p.iter().all(Part::done)).unwrap_or(false);
    if !all_done || shared.stop.load(Ordering::SeqCst) != RUN {
        return Outcome::Stopped;
    }
    let size = std::fs::metadata(temp).map(|m| m.len()).unwrap_or(0);
    if size != fetch.total {
        return Outcome::Failed(format!("the file has {size} bytes instead of {}", fetch.total));
    }
    match std::fs::rename(temp, path) {
        Ok(()) => Outcome::Done,
        Err(e) => Outcome::Failed(format!("could not name the file ({e})")),
    }
}

/// One connection: its part, then the next piece of work, until there is none.
fn worker(id: u32, fetch: &Fetch, temp: &Path, shared: &Arc<Shared>, conn: u32) -> Result<(), Outcome> {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(temp)
        .map_err(|e| Outcome::Failed(format!("could not write the file ({e})")))?;
    let mut rate = 0;
    let mut pieces = 0;
    loop {
        if shared.stop.load(Ordering::SeqCst) != RUN {
            return Ok(());
        }
        let next = {
            let mut parts = shared.parts.lock().map_err(|_| Outcome::Failed("internal error".into()))?;
            segdl::next_work(&mut parts, conn, rate)
        };
        let Some(i) = next else {
            if pieces > 0 && shared.stop.load(Ordering::SeqCst) == RUN {
                info!("download {id}: connection {} done after {pieces} pieces ({} KB/s)", conn + 1, rate / 1024);
            }
            return Ok(());
        };
        pieces += 1;
        rate = fetch_part(fetch, &file, shared, i)?;
        if let Ok(mut p) = shared.parts.lock() {
            p[i].active = false;
        }
    }
}

/// Fetches part `i` from where it is into its place in the file, until it is done (also when
/// a split shortened it); retries network errors. Returns the connection's speed.
fn fetch_part(fetch: &Fetch, file: &std::fs::File, shared: &Arc<Shared>, i: usize) -> Result<u64, Outcome> {
    let internal = || Outcome::Failed("internal error".into());
    let mut tries = 0;
    loop {
        let part = shared.parts.lock().map_err(|_| internal())?[i];
        if part.done() {
            return Ok(part.rate);
        }
        let mut headers = fetch.headers.clone();
        headers.push(("If-Range".into(), fetch.validator.header_value().to_string()));
        // A refused answer (the file changed) or a write error ends the download.
        let fatal: RefCell<Option<Outcome>> = RefCell::new(None);
        let mut sample = (Instant::now(), 0u64);
        let result = http::get_range(
            &fetch.url,
            (part.next, part.end),
            &headers,
            &options(&fetch.proxy, &fetch.agent, PART_TIMEOUT_MS),
            &mut |raw| {
                segdl::check_part(&segdl::parse_head(raw), part.next, part.end, fetch.total, &fetch.validator).map_err(
                    |why| {
                        *fatal.borrow_mut() = Some(Outcome::Restart(capitalized(why)));
                        why.to_string()
                    },
                )
            },
            &mut |chunk| {
                if shared.stop.load(Ordering::Relaxed) != RUN {
                    return Err("stopped".into());
                }
                // How much of the chunk is still this part's (a split may have shortened it).
                let (at, take) = {
                    let mut parts = shared.parts.lock().map_err(|_| "internal error".to_string())?;
                    let take = parts[i].begin_write(chunk.len() as u64);
                    (parts[i].next, take)
                };
                if take == 0 {
                    return Err(PART_COMPLETE.into());
                }
                if let Err(e) = write_all_at(file, &chunk[..take as usize], at) {
                    let why = format!("could not write the file ({e})");
                    *fatal.borrow_mut() = Some(Outcome::Failed(why.clone()));
                    return Err(why);
                }
                shared.received.fetch_add(take, Ordering::Relaxed);
                sample.1 += take;
                let elapsed = sample.0.elapsed().as_millis() as u64;
                if let Ok(mut parts) = shared.parts.lock() {
                    parts[i].end_write(take);
                    if elapsed >= RATE_SAMPLE_MS {
                        parts[i].rate = segdl::smooth_rate(parts[i].rate, sample.1, elapsed);
                    }
                }
                if elapsed >= RATE_SAMPLE_MS {
                    sample = (Instant::now(), 0);
                }
                if take < chunk.len() as u64 {
                    return Err(PART_COMPLETE.into());
                }
                Ok(())
            },
        );
        if shared.stop.load(Ordering::SeqCst) != RUN {
            return Err(Outcome::Stopped);
        }
        if let Some(o) = fatal.into_inner() {
            return Err(o);
        }
        let part = shared.parts.lock().map_err(|_| internal())?[i];
        if part.done() {
            return Ok(part.rate);
        }
        match result {
            // Network trouble, or the server closed early: again from where it is.
            _ if tries < RETRIES => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(500 * u64::from(tries)));
            }
            Ok(_) => return Err(Outcome::Failed("the server ended a part early".into())),
            Err(e) => return Err(Outcome::Failed(e)),
        }
    }
}

fn write_all_at(file: &std::fs::File, mut buf: &[u8], mut at: u64) -> std::io::Result<()> {
    while !buf.is_empty() {
        let n = file.seek_write(buf, at)?;
        if n == 0 {
            return Err(std::io::ErrorKind::WriteZero.into());
        }
        buf = &buf[n..];
        at += n as u64;
    }
    Ok(())
}

fn capitalized(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}
