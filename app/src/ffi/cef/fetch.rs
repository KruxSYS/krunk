//! Small background GETs the browser makes itself (0.11.0: the bookmark favicon probe). They go
//! through the profile's network context, so the engine's proxy and secure DNS settings apply.
//! No cookies or credentials are sent or stored (no `UR_FLAG_ALLOW_STORED_CREDENTIALS`), and
//! there is no referrer. A body over `max_bytes` and a request over `timeout_ms` are cancelled.

use super::{post_ui_delayed, request_context_get_global_context};
use cef::*;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

/// A finished GET: the final address (after redirects), status, MIME type and body.
#[derive(Debug)]
pub struct Fetched {
    pub url: String,
    pub status: i32,
    pub mime: String,
    pub body: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FetchError {
    TooBig,
    Timeout,
    Failed,
}

type Done = Box<dyn FnOnce(Result<Fetched, FetchError>)>;

struct Job {
    body: Vec<u8>,
    max: usize,
    stopped: Option<FetchError>,
    request: Option<Urlrequest>,
    done: Option<Done>,
}

thread_local! {
    static JOBS: RefCell<HashMap<u64, Job>> = RefCell::new(HashMap::new());
    static NEXT: Cell<u64> = const { Cell::new(1) };
}

/// Starts a GET of `url` on the UI thread; `done` runs there once, with the result.
pub fn get(url: &str, max_bytes: usize, timeout_ms: i64, done: impl FnOnce(Result<Fetched, FetchError>) + 'static) {
    let Some(request) = request_create() else {
        done(Err(FetchError::Failed));
        return;
    };
    request.set_url(Some(&CefString::from(url)));
    request.set_method(Some(&CefString::from("GET")));
    request.set_referrer(None, ReferrerPolicy::NO_REFERRER);
    // No stored credentials: no cookies, no HTTP auth, nothing saved from the response.
    request.set_flags(0);
    let id = NEXT.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    JOBS.with(|j| {
        j.borrow_mut().insert(
            id,
            Job { body: Vec::new(), max: max_bytes, stopped: None, request: None, done: Some(Box::new(done)) },
        )
    });
    let mut client = Client::new(id);
    let mut request = request;
    let mut ctx = request_context_get_global_context();
    let started = urlrequest_create(Some(&mut request), Some(&mut client), ctx.as_mut());
    match started {
        Some(r) => JOBS.with(|j| {
            if let Some(job) = j.borrow_mut().get_mut(&id) {
                job.request = Some(r);
            }
        }),
        None => finish(id, None),
    }
    post_ui_delayed(timeout_ms, move || stop(id, FetchError::Timeout));
}

/// Cancels job `id` for `why` (its completion callback still comes, and reports it).
fn stop(id: u64, why: FetchError) {
    let request = JOBS.with(|j| {
        let mut jobs = j.borrow_mut();
        let job = jobs.get_mut(&id)?;
        if job.stopped.is_none() {
            job.stopped = Some(why);
        }
        job.request.clone()
    });
    if let Some(r) = request {
        r.cancel();
    }
}

fn finish(id: u64, request: Option<&Urlrequest>) {
    let Some(mut job) = JOBS.with(|j| j.borrow_mut().remove(&id)) else { return };
    let Some(done) = job.done.take() else { return };
    let result = match (job.stopped.take(), request) {
        (Some(why), _) => Err(why),
        (None, Some(r)) if r.request_status() == UrlrequestStatus::SUCCESS => match r.response() {
            Some(resp) => Ok(Fetched {
                url: CefString::from(&resp.url()).to_string(),
                status: resp.status(),
                mime: CefString::from(&resp.mime_type()).to_string(),
                body: std::mem::take(&mut job.body),
            }),
            None => Err(FetchError::Failed),
        },
        _ => Err(FetchError::Failed),
    };
    done(result);
}

wrap_urlrequest_client! {
    struct Client {
        id: u64,
    }
    impl UrlrequestClient {
        fn on_download_data(&self, _request: Option<&mut Urlrequest>, data: *const u8, data_length: usize) {
            if data.is_null() || data_length == 0 {
                return;
            }
            // SAFETY: CEF passes `data_length` readable bytes for the duration of the call.
            let chunk = unsafe { std::slice::from_raw_parts(data, data_length) };
            let over = JOBS.with(|j| {
                let mut jobs = j.borrow_mut();
                let Some(job) = jobs.get_mut(&self.id) else { return false };
                if job.stopped.is_some() {
                    return false;
                }
                if job.body.len() + chunk.len() > job.max {
                    return true;
                }
                job.body.extend_from_slice(chunk);
                false
            });
            if over {
                stop(self.id, FetchError::TooBig);
            }
        }

        fn on_request_complete(&self, request: Option<&mut Urlrequest>) {
            finish(self.id, request.as_deref());
        }

        fn auth_credentials(
            &self,
            _is_proxy: ::std::os::raw::c_int,
            _host: Option<&CefString>,
            _port: ::std::os::raw::c_int,
            _realm: Option<&CefString>,
            _scheme: Option<&CefString>,
            _callback: Option<&mut AuthCallback>,
        ) -> ::std::os::raw::c_int {
            // Never answer a sign-in prompt for a background request.
            0
        }
    }
}
