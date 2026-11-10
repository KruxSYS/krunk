//! Background requests the browser makes itself (0.11.0: the bookmark favicon probe; 0.12.0:
//! the AI providers, with POST, headers, a streamed response and cancel). They go through the
//! profile's network context, so the engine's proxy and secure DNS settings apply. No cookies
//! or credentials are sent or stored (no `UR_FLAG_ALLOW_STORED_CREDENTIALS`), and there is no
//! referrer. A body over `max_bytes` and a request over its time limit are cancelled.

use super::{post_ui_delayed, request_context_get_global_context, PRIVATE_CONTEXT};
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
    /// [`cancel`] was called (0.12.0).
    Cancelled,
}

type Done = Box<dyn FnOnce(Result<Fetched, FetchError>)>;
type Chunk = Box<dyn FnMut(i32, &[u8])>;

struct Job {
    body: Vec<u8>,
    max: usize,
    /// Bytes received so far (streamed jobs keep no body).
    received: usize,
    stopped: Option<FetchError>,
    request: Option<Urlrequest>,
    done: Option<Done>,
    /// A streamed job's chunk callback: (HTTP status, bytes). Taken out while it runs.
    on_chunk: Option<Chunk>,
    /// Streamed jobs: the time limit runs from the last data (idle), not from the start.
    idle_ms: Option<i64>,
    last_data: std::time::Instant,
}

/// The id of a running request, for [`cancel`].
pub type JobId = u64;

/// A request with a body and headers (0.12.0).
pub struct Req<'a> {
    pub method: &'a str,
    pub url: &'a str,
    /// Header values may hold an API key: the caller keeps them in zeroizing memory.
    pub headers: &'a [(String, String)],
    pub body: Option<&'a [u8]>,
    /// Cancel when no data came for this long (ms), once the request is under way.
    pub idle_timeout_ms: i64,
    /// Cancel when the response is bigger than this.
    pub max_bytes: usize,
}

thread_local! {
    static JOBS: RefCell<HashMap<u64, Job>> = RefCell::new(HashMap::new());
    static NEXT: Cell<u64> = const { Cell::new(1) };
}

/// How a GET is made.
#[derive(Debug, Clone, Copy, Default)]
pub struct Opts {
    /// The private profile's context (it must be open), not the default one.
    pub private: bool,
    /// Send and store cookies (the source viewer loads a page as the tab did).
    pub credentials: bool,
    /// From the cache only (fails if it is not there).
    pub cache_only: bool,
}

/// Starts a GET of `url` on the UI thread; `done` runs there once, with the result. No
/// cookies, the default profile.
pub fn get(url: &str, max_bytes: usize, timeout_ms: i64, done: impl FnOnce(Result<Fetched, FetchError>) + 'static) {
    get_with(url, max_bytes, timeout_ms, Opts::default(), done);
}

pub fn get_with(
    url: &str,
    max_bytes: usize,
    timeout_ms: i64,
    opts: Opts,
    done: impl FnOnce(Result<Fetched, FetchError>) + 'static,
) {
    let Some(request) = request_create() else {
        done(Err(FetchError::Failed));
        return;
    };
    request.set_url(Some(&CefString::from(url)));
    request.set_method(Some(&CefString::from("GET")));
    request.set_referrer(None, ReferrerPolicy::NO_REFERRER);
    // Without UR_FLAG_ALLOW_STORED_CREDENTIALS (8): no cookies, no HTTP auth, nothing saved
    // from the response. UR_FLAG_ONLY_FROM_CACHE is 2.
    request.set_flags(if opts.credentials { 8 } else { 0 } | if opts.cache_only { 2 } else { 0 });
    let id = NEXT.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    JOBS.with(|j| {
        j.borrow_mut().insert(
            id,
            Job {
                body: Vec::new(),
                max: max_bytes,
                received: 0,
                stopped: None,
                request: None,
                done: Some(Box::new(done)),
                on_chunk: None,
                idle_ms: None,
                last_data: std::time::Instant::now(),
            },
        )
    });
    let mut client = Client::new(id);
    let mut request = request;
    let mut ctx = if opts.private {
        match PRIVATE_CONTEXT.with(|c| c.borrow().clone()) {
            Some(c) => Some(c),
            None => {
                finish(id, None);
                return;
            }
        }
    } else {
        request_context_get_global_context()
    };
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

/// Starts a request (UI thread) with the default profile and no cookies. `on_chunk` gets each
/// piece of the response body as it arrives, with the HTTP status; `done` runs once at the end
/// (its body is empty: the chunks had it). Returns the id for [`cancel`].
pub fn request(
    req: Req,
    on_chunk: impl FnMut(i32, &[u8]) + 'static,
    done: impl FnOnce(Result<Fetched, FetchError>) + 'static,
) -> JobId {
    let id = NEXT.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    let Some(mut request) = request_create() else {
        done(Err(FetchError::Failed));
        return id;
    };
    request.set_url(Some(&CefString::from(req.url)));
    request.set_method(Some(&CefString::from(req.method)));
    request.set_referrer(None, ReferrerPolicy::NO_REFERRER);
    for (name, value) in req.headers {
        request.set_header_by_name(Some(&CefString::from(name.as_str())), Some(&CefString::from(value.as_str())), 1);
    }
    if let Some(body) = req.body {
        let (Some(mut post), Some(mut element)) = (post_data_create(), post_data_element_create()) else {
            done(Err(FetchError::Failed));
            return id;
        };
        element.set_to_bytes(body.len(), body.as_ptr());
        post.add_element(Some(&mut element));
        request.set_post_data(Some(&mut post));
    }
    // No stored credentials (8 left out), the cache neither read nor written
    // (UR_FLAG_DISABLE_CACHE 4), and a POST is not repeated on a 5xx (UR_FLAG_NO_RETRY_ON_5XX 64).
    request.set_flags(4 | 64);
    JOBS.with(|j| {
        j.borrow_mut().insert(
            id,
            Job {
                body: Vec::new(),
                max: req.max_bytes,
                received: 0,
                stopped: None,
                request: None,
                done: Some(Box::new(done)),
                on_chunk: Some(Box::new(on_chunk)),
                idle_ms: Some(req.idle_timeout_ms),
                last_data: std::time::Instant::now(),
            },
        )
    });
    let mut client = Client::new(id);
    let mut ctx = request_context_get_global_context();
    match urlrequest_create(Some(&mut request), Some(&mut client), ctx.as_mut()) {
        Some(r) => JOBS.with(|j| {
            if let Some(job) = j.borrow_mut().get_mut(&id) {
                job.request = Some(r);
            }
        }),
        None => {
            finish(id, None);
            return id;
        }
    }
    watch_idle(id, req.idle_timeout_ms);
    id
}

/// Cancels a running request: its `done` reports `Cancelled` (when the engine confirms).
pub fn cancel(id: JobId) {
    stop(id, FetchError::Cancelled);
}

/// Checks a streamed job for silence every `ms`; quiet that long cancels it as a timeout.
fn watch_idle(id: u64, ms: i64) {
    post_ui_delayed(ms.max(100), move || {
        let state = JOBS.with(|j| {
            j.borrow().get(&id).map(|job| (job.last_data.elapsed().as_millis() as i64, job.idle_ms.unwrap_or(ms)))
        });
        match state {
            Some((quiet, limit)) if quiet >= limit => stop(id, FetchError::Timeout),
            Some((quiet, limit)) => watch_idle(id, limit - quiet),
            None => {}
        }
    });
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
        fn on_download_data(&self, request: Option<&mut Urlrequest>, data: *const u8, data_length: usize) {
            if data.is_null() || data_length == 0 {
                return;
            }
            // SAFETY: CEF passes `data_length` readable bytes for the duration of the call.
            let chunk = unsafe { std::slice::from_raw_parts(data, data_length) };
            // (over the limit, the streamed job's callback taken out to call it)
            let (over, streamed) = JOBS.with(|j| {
                let mut jobs = j.borrow_mut();
                let Some(job) = jobs.get_mut(&self.id) else { return (false, None) };
                if job.stopped.is_some() {
                    return (false, None);
                }
                if job.received + chunk.len() > job.max {
                    return (true, None);
                }
                job.received += chunk.len();
                job.last_data = std::time::Instant::now();
                match job.on_chunk.take() {
                    Some(f) => (false, Some(f)),
                    None => {
                        job.body.extend_from_slice(chunk);
                        (false, None)
                    }
                }
            });
            if over {
                stop(self.id, FetchError::TooBig);
                return;
            }
            if let Some(mut f) = streamed {
                let status = request.and_then(|r| r.response()).map_or(0, |r| r.status());
                f(status, chunk);
                JOBS.with(|j| {
                    if let Some(job) = j.borrow_mut().get_mut(&self.id) {
                        job.on_chunk = Some(f);
                    }
                });
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
