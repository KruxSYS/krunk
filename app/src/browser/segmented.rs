//! Large downloads fetched in parts over several connections (0.4.0; the decisions are in
//! `lb_core::segdl`). The engine announces a download; while it waits, the host asks the
//! server for the first byte over WinHTTP, with the cookies the engine would send for that
//! address and the engine's user agent. If the server answers
//! with a part, the engine's download is cancelled and N worker threads fetch the parts into
//! `<file>.partN` files, which are joined into the file at the end. The parts carry the
//! cookies and the Referer the engine itself sent (seen on its request), nothing more. Otherwise the engine's
//! download simply goes on. Private tabs, SOCKS proxies and a setting of 1 connection always
//! use the engine's download.

use super::Browser;
use crate::ffi::cef::{cookie_header, DownloadStart, Profile};
use crate::ffi::http::{self, ProxyMode};
use crate::log::{info, warning};
use lb_core::downloads::DownloadState;
use lb_core::segdl::{self, Part, Probed, Validator};
use lb_core::settings::{updater_proxy, UpdaterProxy};
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

/// Timer (on the app window): progress of downloads fetched in parts.
pub(super) const PARTS_TIMER: usize = 13;
const PARTS_TICK_MS: u32 = 400;
/// The probe gives up (and the engine downloads) after this.
const PROBE_TIMEOUT_MS: i32 = 8_000;
const PART_TIMEOUT_MS: i32 = 30_000;
/// A part that fails is tried again this often (network errors only).
const RETRIES: u32 = 3;

const RUN: u8 = 0;
const PAUSE: u8 = 1;
const CANCEL: u8 = 2;

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
    headers: Vec<(String, String)>,
    proxy: ProxyMode,
    agent: String,
}

/// A download fetched in parts.
pub(super) struct PartsJob {
    fetch: Fetch,
    path: PathBuf,
    shared: Arc<Shared>,
    running: bool,
    tab: Option<TabId>,
}

#[derive(Default)]
pub(super) struct PartsJobs {
    jobs: RefCell<HashMap<u32, PartsJob>>,
}

/// How a run of the workers ended.
enum Outcome {
    Done,
    Stopped,
    Failed(String),
}

fn part_path(path: &Path, i: usize) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".part{i}"));
    path.with_file_name(name)
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
        let (connections, proxy, agent) = {
            let s = self.settings.borrow();
            let agent =
                if s.user_agent.is_empty() { crate::ffi::cef::default_user_agent() } else { s.user_agent.clone() };
            (s.download_connections, s.proxy.clone(), agent)
        };
        let web = start.url.starts_with("https://") || start.url.starts_with("http://");
        let small = start.total.is_some_and(|t| (t as u64) < segdl::MIN_SEGMENTED);
        let proxy = match updater_proxy(&proxy) {
            _ if proxy.trim() == "direct://" => None,
            UpdaterProxy::System => Some(ProxyMode::System),
            UpdaterProxy::Named(p) => Some(ProxyMode::Named(p)),
            UpdaterProxy::Skip => None,
        };
        let Some(proxy) = proxy.filter(|_| connections > 1 && !private && web && !small) else {
            return Some((start, path));
        };
        let page = tab.and_then(|t| self.tab(t).map(|t| t.url.clone()));
        // Exactly the Referer the engine sent for this download (none if it sent none or the
        // request was not seen): never more than the engine would say.
        let referer = tab
            .and_then(|t| self.views.borrow().get(&t).and_then(|v| v.net.engine_referrer(&start.url)))
            .filter(|r| !r.is_empty());
        let weak = self.me.clone();
        let url = start.url.clone();
        // The engine's cookies for the address, then the probe on a worker thread.
        cookie_header(Profile::Default, &url.clone(), move |cookies| {
            let Some(b) = weak.upgrade() else { return };
            let mut headers = Vec::new();
            if !cookies.is_empty() {
                headers.push(("Cookie".to_string(), cookies));
            }
            if let Some(r) = referer {
                headers.push(("Referer".to_string(), r));
            }
            let fetch_headers = headers.clone();
            let probe_url = url.clone();
            let probe_proxy = proxy.clone();
            let probe_agent = agent.clone();
            b.spawn_job(
                "download-probe",
                move || probe(&probe_url, &fetch_headers, &probe_proxy, &probe_agent),
                move |b, result| match result {
                    Ok(p) => {
                        // The engine's download is not needed: dropping `start` cancels it.
                        drop(start);
                        let fetch = Fetch { url, total: p.total, validator: p.validator, headers, proxy, agent };
                        b.start_parts(fetch, path, tab, connections, page);
                    }
                    Err(why) => {
                        info!("download: one connection ({why})");
                        b.accept_download(start, path, private, tab);
                    }
                },
            );
        });
        None
    }

    fn start_parts(&self, fetch: Fetch, path: PathBuf, tab: Option<TabId>, connections: u32, page: Option<String>) {
        let parts = segdl::plan(fetch.total, connections);
        let files: Vec<String> = (0..parts.len()).map(|i| part_path(&path, i).to_string_lossy().into_owned()).collect();
        let id = {
            let mut d = self.downloads.borrow_mut();
            let id = d.start(&fetch.url, &path, Some(fetch.total as i64), now_ms(), false);
            if let Some(p) = &page {
                d.set_page(id, p);
            }
            d.set_parts(id, parts.len() as u32, files);
            id
        };
        info!("download {id} started: {} bytes over {} connections", fetch.total, parts.len());
        self.download_flight(tab);
        if let Some(v) = tab.and_then(|t| self.views.borrow().get(&t).map(|v| v.downloads.clone())) {
            v.set(v.get() + 1);
        }
        let shared =
            Arc::new(Shared { received: AtomicU64::new(0), stop: AtomicU8::new(RUN), parts: Mutex::new(parts) });
        self.parts_jobs.jobs.borrow_mut().insert(id, PartsJob { fetch, path, shared, running: false, tab });
        self.save_downloads();
        self.push_downloads();
        self.run_parts(id);
    }

    /// (Re)starts the workers of download `id` from where its parts are.
    fn run_parts(&self, id: u32) {
        let (fetch, path, shared) = {
            let mut jobs = self.parts_jobs.jobs.borrow_mut();
            let Some(job) = jobs.get_mut(&id).filter(|j| !j.running) else { return };
            job.running = true;
            job.shared.stop.store(RUN, Ordering::SeqCst);
            (job.fetch.clone(), job.path.clone(), job.shared.clone())
        };
        if let Some(app) = self.app() {
            app.set_timer(PARTS_TIMER, PARTS_TICK_MS);
        }
        self.spawn_job(
            "download-parts",
            move || run_workers(&fetch, &path, &shared),
            move |b, outcome| b.parts_ended(id, outcome),
        );
    }

    fn parts_ended(&self, id: u32, outcome: Outcome) {
        let Some((stop, tab, path, n)) = self.parts_jobs.jobs.borrow_mut().get_mut(&id).map(|j| {
            j.running = false;
            let n = j.shared.parts.lock().map(|p| p.len()).unwrap_or(0);
            (j.shared.stop.load(Ordering::SeqCst), j.tab, j.path.clone(), n)
        }) else {
            return;
        };
        let now = now_ms();
        let finished = match outcome {
            Outcome::Done => {
                self.downloads.borrow_mut().set_state(id, DownloadState::Completed, None, now);
                info!("download {id} finished: Completed (in parts)");
                true
            }
            Outcome::Stopped if stop == PAUSE => {
                self.downloads.borrow_mut().set_state(id, DownloadState::Paused, None, now);
                false
            }
            Outcome::Stopped => {
                remove_parts(&path, n);
                self.downloads.borrow_mut().set_state(id, DownloadState::Cancelled, None, now);
                info!("download {id} finished: Cancelled");
                true
            }
            Outcome::Failed(why) => {
                warning!("download {id} failed: {why}");
                remove_parts(&path, n);
                self.downloads.borrow_mut().set_state(id, DownloadState::Interrupted, Some(why), now);
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

    /// `PARTS_TIMER`: progress into the list.
    pub(super) fn parts_tick(&self) {
        let progress: Vec<(u32, u64, u64, Vec<lb_core::downloads::PartProgress>)> = self
            .parts_jobs
            .jobs
            .borrow()
            .iter()
            .filter(|(_, j)| j.running)
            .map(|(id, j)| {
                let parts = j
                    .shared
                    .parts
                    .lock()
                    .map(|p| {
                        p.iter()
                            .map(|part| lb_core::downloads::PartProgress {
                                done: part.next.saturating_sub(part.start),
                                len: (part.end - part.start).saturating_add(1),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                (*id, j.shared.received.load(Ordering::Relaxed), j.fetch.total, parts)
            })
            .collect();
        if progress.is_empty() {
            if let Some(app) = self.app() {
                app.kill_timer(PARTS_TIMER);
            }
            return;
        }
        {
            let now = now_ms();
            let mut d = self.downloads.borrow_mut();
            for (id, received, total, parts) in progress {
                d.progress_at(id, received as i64, Some(total as i64), now);
                d.set_part_progress(id, parts);
            }
        }
        self.push_downloads();
    }

    /// Pause, resume or cancel a download fetched in parts; `false` if `id` is not one.
    pub(super) fn parts_control(&self, id: u32, action: lb_core::protocol::DownloadAction) -> bool {
        use lb_core::protocol::DownloadAction as A;
        let state = self.parts_jobs.jobs.borrow().get(&id).map(|j| (j.running, j.shared.clone(), j.path.clone()));
        let Some((running, shared, path)) = state else { return false };
        match action {
            A::Pause if running => shared.stop.store(PAUSE, Ordering::SeqCst),
            A::Resume if !running => {
                self.downloads.borrow_mut().set_state(id, DownloadState::InProgress, None, now_ms());
                self.run_parts(id);
            }
            A::Cancel if running => shared.stop.store(CANCEL, Ordering::SeqCst),
            A::Cancel => {
                // Paused: nothing runs, so it ends here.
                shared.stop.store(CANCEL, Ordering::SeqCst);
                let n = shared.parts.lock().map(|p| p.len()).unwrap_or(0);
                remove_parts(&path, n);
                self.parts_ended(id, Outcome::Stopped);
            }
            _ => {}
        }
        true
    }

    /// At exit: running workers stop (their part files are deleted at the next start).
    pub(super) fn stop_parts(&self) {
        for j in self.parts_jobs.jobs.borrow().values() {
            j.shared.stop.store(CANCEL, Ordering::SeqCst);
        }
    }

    /// At start: part files of downloads that were running when Krunk last exited.
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

fn remove_parts(path: &Path, n: usize) {
    for i in 0..n {
        let _ = std::fs::remove_file(part_path(path, i));
    }
}

fn options<'a>(proxy: &'a ProxyMode, agent: &'a str, timeout_ms: i32) -> http::Options<'a> {
    http::Options { proxy, timeout_ms, user_agent: agent, allow_http: true }
}

/// The first byte, to see whether the server sends parts.
fn probe(url: &str, headers: &[(String, String)], proxy: &ProxyMode, agent: &str) -> Result<Probed, String> {
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
        (Some(Err(why)), _) => Err(why),
        (_, Err(e)) => Err(e),
        (None, Ok(_)) => Err("no answer".into()),
    }
}

/// Runs one worker per unfinished part until all are done, one fails for good, or the
/// download is paused or cancelled. Done: the parts are joined into `path`.
fn run_workers(fetch: &Fetch, path: &Path, shared: &Arc<Shared>) -> Outcome {
    let parts: Vec<(usize, Part)> = match shared.parts.lock() {
        Ok(p) => p.iter().copied().enumerate().filter(|(_, p)| !p.done()).collect(),
        Err(_) => return Outcome::Failed("internal error".into()),
    };
    // Bytes already there (a resumed download).
    let total_left: u64 = parts.iter().map(|(_, p)| p.left()).sum();
    shared.received.store(fetch.total - total_left, Ordering::SeqCst);
    let failure: Mutex<Option<String>> = Mutex::new(None);
    std::thread::scope(|scope| {
        for (i, _) in &parts {
            let i = *i;
            let failure = &failure;
            scope.spawn(move || {
                if let Err(e) = fetch_part(fetch, path, shared, i) {
                    if shared.stop.load(Ordering::SeqCst) == RUN {
                        // One part failed for good: the others stop too.
                        if let Ok(mut f) = failure.lock() {
                            f.get_or_insert(e);
                        }
                        shared.stop.store(CANCEL, Ordering::SeqCst);
                    }
                }
            });
        }
    });
    if let Some(why) = failure.into_inner().ok().flatten() {
        return Outcome::Failed(why);
    }
    let all_done = shared.parts.lock().map(|p| p.iter().all(Part::done)).unwrap_or(false);
    if !all_done || shared.stop.load(Ordering::SeqCst) != RUN {
        return Outcome::Stopped;
    }
    match join_parts(path, shared, fetch.total) {
        Ok(()) => Outcome::Done,
        Err(e) => Outcome::Failed(e),
    }
}

/// Fetches part `i` from where it is, appending to its file; retries network errors.
fn fetch_part(fetch: &Fetch, path: &Path, shared: &Arc<Shared>, i: usize) -> Result<(), String> {
    let file_path = part_path(path, i);
    let mut tries = 0;
    loop {
        let part = shared.parts.lock().map_err(|_| "internal error")?[i];
        if part.done() {
            return Ok(());
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .map_err(|e| format!("could not write the file ({e})"))?;
        // The file holds exactly the bytes before `next`.
        let have = file.metadata().map(|m| m.len()).unwrap_or(0);
        if have != part.next - part.start {
            file.set_len(part.next - part.start).map_err(|e| format!("could not write the file ({e})"))?;
        }
        let mut headers = fetch.headers.clone();
        headers.push(("If-Range".into(), fetch.validator.header_value().to_string()));
        let fatal: RefCell<Option<String>> = RefCell::new(None);
        let result = http::get_range(
            &fetch.url,
            (part.next, part.end),
            &headers,
            &options(&fetch.proxy, &fetch.agent, PART_TIMEOUT_MS),
            &mut |raw| {
                segdl::check_part(&segdl::parse_head(raw), part.next, part.end, fetch.total, &fetch.validator).map_err(
                    |why| {
                        *fatal.borrow_mut() = Some(why.to_string());
                        why.to_string()
                    },
                )
            },
            &mut |chunk| {
                if shared.stop.load(Ordering::Relaxed) != RUN {
                    return Err("stopped".into());
                }
                file.write_all(chunk).map_err(|e| {
                    let why = format!("could not write the file ({e})");
                    *fatal.borrow_mut() = Some(why.clone());
                    why
                })?;
                shared.received.fetch_add(chunk.len() as u64, Ordering::Relaxed);
                if let Ok(mut parts) = shared.parts.lock() {
                    parts[i].next += chunk.len() as u64;
                }
                Ok(())
            },
        );
        let _ = file.flush();
        if shared.stop.load(Ordering::SeqCst) != RUN {
            return Err("stopped".into());
        }
        if let Some(why) = fatal.into_inner() {
            return Err(why);
        }
        match result {
            Ok(_) if shared.parts.lock().map(|p| p[i].done()).unwrap_or(false) => return Ok(()),
            // Network trouble, or the server closed early: again from where it is.
            Ok(_) | Err(_) if tries < RETRIES => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(500 * u64::from(tries)));
            }
            Ok(_) => return Err("the server ended a part early".into()),
            Err(e) => return Err(e),
        }
    }
}

/// Appends parts 1.. to part 0, checks the size and gives it the download's name.
fn join_parts(path: &Path, shared: &Arc<Shared>, total: u64) -> Result<(), String> {
    let n = shared.parts.lock().map(|p| p.len()).map_err(|_| "internal error")?;
    let first = part_path(path, 0);
    let mut out =
        std::fs::OpenOptions::new().append(true).open(&first).map_err(|e| format!("could not join the parts ({e})"))?;
    for i in 1..n {
        let p = part_path(path, i);
        let mut f = std::fs::File::open(&p).map_err(|e| format!("could not join the parts ({e})"))?;
        std::io::copy(&mut f, &mut out).map_err(|e| format!("could not join the parts ({e})"))?;
        drop(f);
        let _ = std::fs::remove_file(&p);
    }
    out.flush().map_err(|e| format!("could not join the parts ({e})"))?;
    drop(out);
    let size = std::fs::metadata(&first).map(|m| m.len()).unwrap_or(0);
    if size != total {
        let _ = std::fs::remove_file(&first);
        return Err(format!("the file has {size} bytes instead of {total}"));
    }
    std::fs::rename(&first, path).map_err(|e| format!("could not name the file ({e})"))
}
