//! The bookmark favicon probe (0.11.0): a bookmark of a site that was never visited gets its
//! icon once, in the background. The rules (once per host, a failure retried after 7 days or
//! when the bookmark is added again, never for a bookmark saved in a private window) are in
//! `lb_core::favicon_probe`; this side fetches through the profile's network context without
//! cookies (`ffi::cef::fetch`) and decodes on a job thread (`ffi::icon`).

use super::Browser;
use crate::ffi::cef::fetch::{self, FetchError, Fetched};
use crate::log::info;
use lb_core::favicon_probe::{self as probe, Probe, ProbeLog};
use lb_core::storage::now_ms;
use std::collections::VecDeque;

/// The first sweep runs this long after the start (not to compete with the session's tabs).
const FIRST_SWEEP_MS: i64 = 30_000;
/// After a bookmark is added.
const ADDED_SWEEP_MS: i64 = 2_000;
/// One sweep looks at this many hosts at most (the rest wait for the next start).
const SWEEP_MAX: usize = 200;

#[derive(Default)]
pub(super) struct ProbeState {
    queue: VecDeque<Probe>,
    running: usize,
    log: Option<ProbeLog>,
    sweep_planned: bool,
}

impl Browser {
    /// At start: the first sweep a little later.
    pub(super) fn init_favicon_probe(&self) {
        self.plan_favicon_sweep(FIRST_SWEEP_MS);
    }

    /// A bookmark of `url` was added (`private`: in a private window): a failed site may be
    /// tried again at once, and it is probed soon.
    pub(super) fn favicon_bookmark_added(&self, url: &str, private: bool) {
        let Some(p) = Probe::of(url) else { return };
        self.with_probe_log(|log| log.bookmark_added(&p.host, private, now_ms()));
        if !private {
            self.plan_favicon_sweep(ADDED_SWEEP_MS);
        }
    }

    fn plan_favicon_sweep(&self, delay_ms: i64) {
        if std::mem::replace(&mut self.favicon_probe.borrow_mut().sweep_planned, true) {
            return;
        }
        let weak = self.me.clone();
        crate::ffi::cef::post_ui_delayed(delay_ms, move || {
            if let Some(b) = weak.upgrade() {
                b.favicon_probe.borrow_mut().sweep_planned = false;
                b.favicon_sweep();
            }
        });
    }

    fn with_probe_log<R>(&self, f: impl FnOnce(&mut ProbeLog) -> R) -> R {
        let mut st = self.favicon_probe.borrow_mut();
        let log = st.log.get_or_insert_with(|| {
            let stored = self.store.borrow().kv_get(probe::KV_KEY).ok().flatten();
            ProbeLog::parse(stored.as_deref())
        });
        let before = log.clone();
        let r = f(log);
        if *log != before {
            if let Err(e) = self.store.borrow().kv_set(probe::KV_KEY, &log.to_json()) {
                crate::log::warning!("favicon-probe: the log could not be saved: {e}");
            }
        }
        r
    }

    /// Queues every bookmarked host without an icon that is due, and starts up to 4 probes.
    fn favicon_sweep(&self) {
        let urls = self.store.borrow().bookmark_urls().unwrap_or_default();
        let queued: Vec<String> = self.favicon_probe.borrow().queue.iter().map(|p| p.host.clone()).collect();
        let due = {
            let store = self.store.borrow();
            let has_icon = |h: &str| store.has_favicon(h).unwrap_or(true) || queued.iter().any(|q| q == h);
            self.with_probe_log(|log| {
                probe::due_probes(urls.iter().map(String::as_str), has_icon, log, now_ms(), SWEEP_MAX)
            })
        };
        if due.is_empty() {
            return;
        }
        info!("favicon-probe: {} site(s) to probe", due.len());
        self.favicon_probe.borrow_mut().queue.extend(due);
        self.favicon_pump();
    }

    fn favicon_pump(&self) {
        loop {
            let next = {
                let mut st = self.favicon_probe.borrow_mut();
                if st.running >= probe::PARALLEL {
                    return;
                }
                let Some(p) = st.queue.pop_front() else { return };
                st.running += 1;
                p
            };
            self.favicon_probe_icon(next);
        }
    }

    /// Step 1: `/favicon.ico`.
    fn favicon_probe_icon(&self, p: Probe) {
        let weak = self.me.clone();
        fetch::get(&p.icon_url(), probe::ICON_MAX_BYTES, probe::TIMEOUT_MS, move |r| {
            let Some(b) = weak.upgrade() else { return };
            match image_body(r) {
                Some(body) => b.favicon_decode(p, body, "favicon.ico", true),
                None => b.favicon_probe_page(p),
            }
        });
    }

    /// Step 2: the page's `<link rel=icon>`.
    fn favicon_probe_page(&self, p: Probe) {
        let weak = self.me.clone();
        fetch::get(&p.page_url(), probe::HTML_MAX_BYTES, probe::TIMEOUT_MS, move |r| {
            let Some(b) = weak.upgrade() else { return };
            let link = match r {
                Ok(f) if (200..300).contains(&f.status) => {
                    let base = if f.url.is_empty() { p.page_url() } else { f.url.clone() };
                    probe::icon_link(&String::from_utf8_lossy(&f.body), &base)
                }
                Ok(f) => {
                    b.favicon_done(&p, false, &format!("page HTTP {}", f.status));
                    return;
                }
                Err(e) => {
                    b.favicon_done(&p, false, &format!("page {}", fetch_error(&e)));
                    return;
                }
            };
            let Some(link) = link else {
                b.favicon_done(&p, false, "no icon");
                return;
            };
            let weak = b.me.clone();
            fetch::get(&link, probe::ICON_MAX_BYTES, probe::TIMEOUT_MS, move |r| {
                let Some(b) = weak.upgrade() else { return };
                match image_body(r) {
                    Some(body) => b.favicon_decode(p, body, "link rel=icon", false),
                    None => b.favicon_done(&p, false, "link icon not loaded"),
                }
            });
        });
    }

    /// Decodes on a job thread; `/favicon.ico` that is no image falls back to the page.
    fn favicon_decode(&self, p: Probe, body: Vec<u8>, from: &'static str, fallback: bool) {
        self.spawn_job(
            "favicon-decode",
            move || crate::ffi::icon::to_png32(&body),
            move |b, png: Option<Vec<u8>>| match png {
                Some(png) => {
                    let stored = b.store.borrow().set_favicon(&p.host, &png, now_ms());
                    match stored {
                        Ok(()) => {
                            b.favicon_done(&p, true, from);
                            b.send_bookmarks_bar();
                            if let Some(open) = b.open_folder() {
                                b.show_bookmark_folder(open, None, None);
                            }
                        }
                        Err(e) => b.favicon_done(&p, false, &format!("store: {e}")),
                    }
                }
                None if fallback => b.favicon_probe_page(p),
                None => b.favicon_done(&p, false, "not an image"),
            },
        );
    }

    fn favicon_done(&self, p: &Probe, ok: bool, what: &str) {
        info!("favicon-probe host={} result={} ({what})", p.host, if ok { "ok" } else { "failed" });
        self.with_probe_log(|log| log.record(&p.host, ok, now_ms()));
        {
            let mut st = self.favicon_probe.borrow_mut();
            st.running = st.running.saturating_sub(1);
        }
        self.favicon_pump();
    }
}

/// A successful image response's body (an HTML error page with status 200 is weeded out by
/// the decoder).
fn image_body(r: Result<Fetched, FetchError>) -> Option<Vec<u8>> {
    let f = r.ok()?;
    let image_like = f.mime.is_empty() || f.mime.starts_with("image/") || f.mime == "application/octet-stream";
    ((200..300).contains(&f.status) && image_like && !f.body.is_empty()).then_some(f.body)
}

fn fetch_error(e: &FetchError) -> &'static str {
    match e {
        FetchError::TooBig => "too big",
        FetchError::Timeout => "timeout",
        FetchError::Failed => "failed",
        FetchError::Cancelled => "cancelled",
    }
}
