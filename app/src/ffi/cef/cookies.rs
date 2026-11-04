//! Listing and deleting the cookies of a profile (`krunk://storage`,
//! `krunk://sitedata`). The cookie manager visits cookies on the UI thread;
//! a visitor that is never called means there are no cookies, so a short
//! timer finishes the job then.

use super::{post_ui_delayed, request_context_get_global_context, PRIVATE_CONTEXT};
use cef::*;
use lb_core::sitedata::{windows_us_to_unix_ms, CookieInfo, SameSite};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::os::raw::c_int;

/// Which profile's cookies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Default,
    /// The in-memory profile of private tabs (if one is open).
    Private,
}

type Done = Box<dyn FnOnce(Vec<CookieInfo>)>;
type Predicate = Box<dyn Fn(&CookieInfo) -> bool>;
/// A finished job waiting for the cookie store flush.
type Flushing = std::rc::Rc<RefCell<Option<(Done, Vec<CookieInfo>)>>>;

struct Job {
    found: Vec<CookieInfo>,
    delete: Option<Predicate>,
    done: Option<Done>,
}

thread_local! {
    static JOBS: RefCell<HashMap<u64, Job>> = RefCell::new(HashMap::new());
    static NEXT: Cell<u64> = const { Cell::new(1) };
}

fn manager(profile: Profile) -> Option<CookieManager> {
    let ctx = match profile {
        Profile::Default => request_context_get_global_context(),
        Profile::Private => PRIVATE_CONTEXT.with(|c| c.borrow().clone()),
    };
    ctx?.cookie_manager(None)
}

/// Lists every cookie of `profile`.
pub fn list_cookies(profile: Profile, done: impl FnOnce(Vec<CookieInfo>) + 'static) {
    visit(profile, None, Box::new(done));
}

/// Deletes the cookies `which` selects; `done` gets the deleted ones.
pub fn delete_cookies(
    profile: Profile,
    which: impl Fn(&CookieInfo) -> bool + 'static,
    done: impl FnOnce(Vec<CookieInfo>) + 'static,
) {
    visit(profile, Some(Box::new(which)), Box::new(done));
}

fn visit(profile: Profile, delete: Option<Predicate>, done: Done) {
    let Some(manager) = manager(profile) else {
        done(Vec::new());
        return;
    };
    let id = NEXT.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    JOBS.with(|j| j.borrow_mut().insert(id, Job { found: Vec::new(), delete, done: Some(done) }));
    let mut visitor = Visitor::new(id, manager.clone());
    if manager.visit_all_cookies(Some(&mut visitor)) == 0 {
        finish(id, None);
        return;
    }
    // No cookies at all: the visitor is never called.
    post_ui_delayed(1500, move || finish(id, None));
}

/// Ends job `id` once; deletions are flushed to the store first.
fn finish(id: u64, flush: Option<&CookieManager>) {
    let Some(job) = JOBS.with(|j| j.borrow_mut().remove(&id)) else { return };
    let (Some(done), found) = (job.done, job.found) else { return };
    match flush {
        Some(m) if job.delete.is_some() => {
            let mut cb = Flushed::new(std::rc::Rc::new(RefCell::new(Some((done, found)))));
            m.flush_store(Some(&mut cb));
        }
        _ => done(found),
    }
}

fn cookie_info(c: &Cookie) -> CookieInfo {
    let same_site = c.same_site.get_raw();
    CookieInfo {
        name: c.name.to_string(),
        value: c.value.to_string(),
        domain: c.domain.to_string(),
        path: c.path.to_string(),
        expires: (c.has_expires != 0).then(|| windows_us_to_unix_ms(c.expires.val)),
        secure: c.secure != 0,
        http_only: c.httponly != 0,
        same_site: if same_site == CookieSameSite::NO_RESTRICTION.get_raw() {
            SameSite::None
        } else if same_site == CookieSameSite::LAX_MODE.get_raw() {
            SameSite::Lax
        } else if same_site == CookieSameSite::STRICT_MODE.get_raw() {
            SameSite::Strict
        } else {
            SameSite::Unspecified
        },
    }
}

wrap_cookie_visitor! {
    struct Visitor {
        id: u64,
        manager: CookieManager,
    }
    impl CookieVisitor {
        fn visit(
            &self,
            cookie: Option<&Cookie>,
            count: c_int,
            total: c_int,
            delete_cookie: Option<&mut c_int>,
        ) -> c_int {
            if let Some(c) = cookie {
                let info = cookie_info(c);
                JOBS.with(|j| {
                    let mut jobs = j.borrow_mut();
                    let Some(job) = jobs.get_mut(&self.id) else { return };
                    match &job.delete {
                        Some(pred) if pred(&info) => {
                            if let Some(d) = delete_cookie {
                                *d = 1;
                            }
                            job.found.push(info);
                        }
                        Some(_) => {}
                        None => job.found.push(info),
                    }
                });
            }
            if count + 1 >= total {
                finish(self.id, Some(&self.manager));
            }
            1
        }
    }
}

wrap_completion_callback! {
    struct Flushed {
        pending: Flushing,
    }
    impl CompletionCallback {
        fn on_complete(&self) {
            if let Some((done, found)) = self.pending.borrow_mut().take() {
                done(found);
            }
        }
    }
}
