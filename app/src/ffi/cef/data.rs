//! Clearing browsing data of the default profile. CEF has no single call
//! for this: cookies go through the cookie manager (all, or those created
//! in the time range), the HTTP cache is cleared as a whole, and the site
//! data of each given origin is cleared with the DevTools protocol.

use super::view::{ClearKinds, View};
use super::{post_ui_delayed, request_context_get_global_context};
use crate::log::warning;
use cef::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::os::raw::c_int;

/// Give up waiting for the engine after this long.
const TIMEOUT_MS: i64 = 10_000;
/// Microseconds between the Windows epoch (1601) and the Unix epoch.
const WINDOWS_TO_UNIX_US: i64 = 11_644_473_600_000_000;

struct Job {
    pending: usize,
    ok: bool,
    done: Option<Box<dyn FnOnce(bool)>>,
}

thread_local! {
    static JOBS: RefCell<HashMap<u64, Job>> = RefCell::new(HashMap::new());
    static NEXT: Cell<u64> = const { Cell::new(1) };
}

/// One part of job `id` finished (UI thread).
fn step_done(id: u64, ok: bool) {
    let finished = JOBS.with(|j| {
        let mut jobs = j.borrow_mut();
        let job = jobs.get_mut(&id)?;
        job.pending = job.pending.saturating_sub(1);
        job.ok &= ok;
        if job.pending == 0 {
            jobs.remove(&id).map(|j| (j.ok, j.done))
        } else {
            None
        }
    });
    if let Some((ok, Some(done))) = finished {
        done(ok);
    }
}

/// Clears the selected kinds of data (cookies only those created since
/// `since_ms`, Unix ms, if given). `origins` are the sites whose storage
/// (local storage, IndexedDB, service workers, cache storage) is cleared;
/// `via` is any live view of the default profile. `done(ok)` runs on the UI
/// thread when everything finished, or after a timeout with `false`.
pub fn clear_profile_data(
    kinds: ClearKinds,
    since_ms: Option<i64>,
    origins: &[String],
    via: Option<&View>,
    done: impl FnOnce(bool) + 'static,
) {
    let id = NEXT.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    // One extra step so that callbacks arriving synchronously cannot finish
    // the job before every part has been started.
    JOBS.with(|j| j.borrow_mut().insert(id, Job { pending: 1, ok: true, done: Some(Box::new(done)) }));
    let add_step = || JOBS.with(|j| j.borrow_mut().get_mut(&id).map(|job| job.pending += 1));
    let ctx = request_context_get_global_context();

    if kinds.cookies {
        if let Some(manager) = ctx.as_ref().and_then(|c| c.cookie_manager(None)) {
            add_step();
            match since_ms {
                None => {
                    let mut cb = CookiesDeleted::new(id);
                    if manager.delete_cookies(None, None, Some(&mut cb)) == 0 {
                        step_done(id, false);
                    }
                }
                Some(since) => {
                    let since_us = since.saturating_mul(1000).saturating_add(WINDOWS_TO_UNIX_US);
                    let mut visitor = CookieSweeper::new(id, since_us);
                    if manager.visit_all_cookies(Some(&mut visitor)) == 0 {
                        step_done(id, false);
                    } else {
                        // No cookies at all means the visitor is never called.
                        post_ui_delayed(1500, move || step_done_once(id));
                    }
                }
            }
        }
    }
    if kinds.cache {
        if let Some(c) = &ctx {
            add_step();
            let mut cb = CacheCleared::new(id);
            c.clear_http_cache(Some(&mut cb));
        }
    }
    if kinds.site_data {
        if let Some(v) = via {
            for origin in origins {
                v.devtools_method(
                    "Storage.clearDataForOrigin",
                    &serde_json::json!({
                        "origin": origin,
                        "storageTypes": "cookies,local_storage,indexeddb,websql,service_workers,cache_storage,\
                                         file_systems,shared_storage,storage_buckets",
                    }),
                );
            }
        }
    }
    step_done(id, true);
    post_ui_delayed(TIMEOUT_MS, move || {
        let timed_out = JOBS.with(|j| j.borrow_mut().remove(&id));
        if let Some(Job { done: Some(done), .. }) = timed_out {
            warning!("clearing browsing data timed out");
            done(false);
        }
    });
}

thread_local! {
    static SWEPT: RefCell<std::collections::HashSet<u64>> = RefCell::new(std::collections::HashSet::new());
}

/// The cookie sweep of job `id` counts once (last cookie or fallback timer).
fn step_done_once(id: u64) {
    if SWEPT.with(|s| s.borrow_mut().insert(id)) {
        step_done(id, true);
    }
}

wrap_delete_cookies_callback! {
    struct CookiesDeleted {
        id: u64,
    }
    impl DeleteCookiesCallback {
        fn on_complete(&self, _num_deleted: c_int) {
            step_done(self.id, true);
        }
    }
}

wrap_completion_callback! {
    struct CacheCleared {
        id: u64,
    }
    impl CompletionCallback {
        fn on_complete(&self) {
            step_done(self.id, true);
        }
    }
}

wrap_cookie_visitor! {
    struct CookieSweeper {
        id: u64,
        // Delete cookies created at or after this time (Windows epoch, µs).
        since_us: i64,
    }
    impl CookieVisitor {
        fn visit(
            &self,
            cookie: Option<&Cookie>,
            count: c_int,
            total: c_int,
            delete_cookie: Option<&mut c_int>,
        ) -> c_int {
            if let (Some(c), Some(d)) = (cookie, delete_cookie) {
                if c.creation.val >= self.since_us {
                    *d = 1;
                }
            }
            if count + 1 >= total {
                step_done_once(self.id);
            }
            1
        }
    }
}
