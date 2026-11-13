//! Extension requests to the hosts the extension's manifest names, past CORS (0.17.0).
//!
//! Chrome exempts an extension's pages and background from CORS for its `host_permissions`.
//! This engine's network service knows no such exemption for Krunk's extension origins, and
//! CEF's cross-origin allow-list is consulted only for responses the application serves
//! itself. So that is what happens here: a `fetch`/XHR from an extension's own origin to a
//! granted host is answered by a [`ResourceHandler`] that makes the request with the
//! browser's own URL request (the profile's network context: proxy, VPN and DNS settings
//! apply) and hands the response back. CEF then applies its allow-list, which
//! `Browser::ext_sync_cors` keeps in step with the same grants.
//!
//! Only requests whose initiator is exactly an enabled extension's origin are taken, never a
//! web page's and never a content script's (those run in the page's origin). No cookies and
//! no stored credentials are sent or kept (an extension's default for a cross-origin `fetch`).
//! The response is buffered: at most [`MAX_BODY`], no streaming.

use super::{post_ui_from_any, request_context_get_global_context};
use cef::*;
use lb_core::ext_runtime::CorsGrant;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

/// The largest response relayed (a wallet's token lists and snap packages are a few MiB).
const MAX_BODY: usize = 64 * 1024 * 1024;

static GRANTS: RwLock<Option<HashMap<String, Vec<CorsGrant>>>> = RwLock::new(None);

/// The grants of every enabled extension, by its origin (`https://<id>.ext.krunk.invalid`).
pub fn set_grants(grants: HashMap<String, Vec<CorsGrant>>) {
    if let Ok(mut g) = GRANTS.write() {
        *g = Some(grants);
    }
}

/// Is `initiator` an extension origin with grants at all (any thread)?
pub fn is_source(initiator: &str) -> bool {
    let initiator = initiator.trim_end_matches('/');
    GRANTS.read().is_ok_and(|g| g.as_ref().is_some_and(|g| g.contains_key(initiator)))
}

/// Should this request be relayed (IO thread)? A `fetch`/XHR of an extension origin to
/// another origin its manifest grants.
pub fn wanted(initiator: &str, request: &Request) -> bool {
    if sys::cef_resource_type_t::from(request.resource_type()) != sys::cef_resource_type_t::RT_XHR {
        return false;
    }
    let initiator = initiator.trim_end_matches('/');
    let url = CefString::from(&request.url()).to_string();
    GRANTS.read().is_ok_and(|g| {
        g.as_ref().and_then(|g| g.get(initiator)).is_some_and(|grants| lb_core::ext_runtime::cors_granted(grants, &url))
    })
}

#[derive(Default)]
struct Exchange {
    status: i32,
    status_text: String,
    mime: String,
    charset: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    offset: usize,
    failed: bool,
    cancelled: bool,
    /// The running request's key in [`RUNNING`] (UI thread).
    job: u64,
}

struct Outgoing {
    url: String,
    method: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}

thread_local! {
    /// The requests under way, kept alive until they complete (UI thread).
    static RUNNING: RefCell<HashMap<u64, Urlrequest>> = RefCell::new(HashMap::new());
    static NEXT: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

/// A handler that answers `request` by making it again from the browser process.
pub fn handler() -> ResourceHandler {
    RelayHandler::new(Arc::new(Mutex::new(Exchange::default())))
}

fn headers_of(map: &mut CefStringMultimap) -> Vec<(String, String)> {
    let n = string_multimap_size(Some(map));
    (0..n)
        .map(|i| {
            let (mut key, mut value) = (CefString::from(""), CefString::from(""));
            string_multimap_key(Some(map), i, Some(&mut key));
            string_multimap_value(Some(map), i, Some(&mut value));
            (key.to_string(), value.to_string())
        })
        .collect()
}

fn outgoing(request: &mut Request) -> Outgoing {
    let mut headers = Vec::new();
    if let Some(mut map) = string_multimap_alloc() {
        request.header_map(Some(&mut map));
        headers = headers_of(&mut map);
        string_multimap_free(Some(&mut map));
    }
    let body = request.post_data().map(|post| {
        let mut elements: Vec<Option<PostDataElement>> = vec![None; post.element_count()];
        post.elements(Some(&mut elements));
        let mut out = Vec::new();
        for e in elements.into_iter().flatten() {
            let n = e.bytes_count();
            let start = out.len();
            out.resize(start + n, 0u8);
            // SAFETY: `out` has `n` writable bytes from `start`.
            let got = e.bytes(n, unsafe { out.as_mut_ptr().add(start) });
            out.truncate(start + got.min(n));
        }
        out
    });
    Outgoing {
        url: CefString::from(&request.url()).to_string(),
        method: CefString::from(&request.method()).to_string(),
        headers,
        body,
    }
}

/// UI thread: makes the request; `callback` continues the page's request when it is done.
fn start(out: Outgoing, state: Arc<Mutex<Exchange>>, callback: CallbackHandle) {
    let fail = |state: &Arc<Mutex<Exchange>>, callback: &CallbackHandle| {
        if let Ok(mut s) = state.lock() {
            s.failed = true;
        }
        callback.0.cont();
    };
    let Some(mut request) = request_create() else { return fail(&state, &callback) };
    request.set_url(Some(&CefString::from(out.url.as_str())));
    request.set_method(Some(&CefString::from(out.method.as_str())));
    request.set_referrer(None, ReferrerPolicy::NO_REFERRER);
    for (name, value) in &out.headers {
        // The page's own origin and cookies are not this request's.
        if ["origin", "referer", "cookie", "host", "content-length"].iter().any(|h| name.eq_ignore_ascii_case(h)) {
            continue;
        }
        request.set_header_by_name(Some(&CefString::from(name.as_str())), Some(&CefString::from(value.as_str())), 1);
    }
    if let Some(body) = out.body.as_ref().filter(|b| !b.is_empty()) {
        let (Some(mut post), Some(mut element)) = (post_data_create(), post_data_element_create()) else {
            return fail(&state, &callback);
        };
        element.set_to_bytes(body.len(), body.as_ptr());
        post.add_element(Some(&mut element));
        request.set_post_data(Some(&mut post));
    }
    // No stored credentials (UR_FLAG_ALLOW_STORED_CREDENTIALS 8 left out); a POST is not
    // repeated on a 5xx (UR_FLAG_NO_RETRY_ON_5XX 64).
    request.set_flags(64);
    let id = NEXT.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    if let Ok(mut s) = state.lock() {
        if s.cancelled {
            return;
        }
        s.job = id;
    }
    let callback = Arc::new(callback);
    let mut client = RelayClient::new(id, state.clone(), callback.clone());
    let mut ctx = request_context_get_global_context();
    match urlrequest_create(Some(&mut request), Some(&mut client), ctx.as_mut()) {
        Some(r) => RUNNING.with(|j| {
            j.borrow_mut().insert(id, r);
        }),
        None => {
            if let Ok(mut s) = state.lock() {
                s.failed = true;
            }
            callback.0.cont();
        }
    }
}

wrap_urlrequest_client! {
    struct RelayClient {
        id: u64,
        state: Arc<Mutex<Exchange>>,
        callback: Arc<CallbackHandle>,
    }
    impl UrlrequestClient {
        fn on_download_data(&self, request: Option<&mut Urlrequest>, data: *const u8, data_length: usize) {
            if data.is_null() || data_length == 0 {
                return;
            }
            // SAFETY: CEF passes `data_length` readable bytes for the duration of the call.
            let chunk = unsafe { std::slice::from_raw_parts(data, data_length) };
            let over = self.state.lock().is_ok_and(|mut s| {
                if s.body.len() + chunk.len() > MAX_BODY {
                    s.failed = true;
                    true
                } else {
                    s.body.extend_from_slice(chunk);
                    false
                }
            });
            if over {
                if let Some(r) = request {
                    r.cancel();
                }
            }
        }

        fn on_request_complete(&self, request: Option<&mut Urlrequest>) {
            RUNNING.with(|j| j.borrow_mut().remove(&self.id));
            if let Ok(mut s) = self.state.lock() {
                let response = request
                    .as_deref()
                    .filter(|r| r.request_status() == UrlrequestStatus::SUCCESS)
                    .and_then(|r| r.response());
                match response {
                    Some(resp) if !s.failed => {
                        s.status = resp.status();
                        s.status_text = CefString::from(&resp.status_text()).to_string();
                        s.mime = CefString::from(&resp.mime_type()).to_string();
                        s.charset = CefString::from(&resp.charset()).to_string();
                        if let Some(mut map) = string_multimap_alloc() {
                            resp.header_map(Some(&mut map));
                            s.headers = headers_of(&mut map);
                            string_multimap_free(Some(&mut map));
                        }
                    }
                    _ => s.failed = true,
                }
            }
            self.callback.0.cont();
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
            // Never answer a sign-in prompt for an extension's background request.
            0
        }
    }
}

wrap_resource_handler! {
    struct RelayHandler {
        state: Arc<Mutex<Exchange>>,
    }
    impl ResourceHandler {
        fn open(
            &self,
            request: Option<&mut Request>,
            handle_request: Option<&mut ::std::os::raw::c_int>,
            callback: Option<&mut Callback>,
        ) -> ::std::os::raw::c_int {
            let (Some(request), Some(callback)) = (request, callback) else { return 0 };
            let out = outgoing(request);
            if let Some(h) = handle_request {
                *h = 0;
            }
            let state = self.state.clone();
            let callback = CallbackHandle(callback.clone());
            post_ui_from_any(move || start(out, state, callback));
            1
        }

        fn response_headers(
            &self,
            response: Option<&mut Response>,
            response_length: Option<&mut i64>,
            _redirect_url: Option<&mut CefString>,
        ) {
            let Some(response) = response else { return };
            let Ok(s) = self.state.lock() else { return };
            if s.failed || s.cancelled {
                response.set_error(Errorcode::FAILED);
                if let Some(len) = response_length {
                    *len = 0;
                }
                return;
            }
            response.set_status(s.status);
            response.set_status_text(Some(&CefString::from(s.status_text.as_str())));
            if !s.mime.is_empty() {
                response.set_mime_type(Some(&CefString::from(s.mime.as_str())));
            }
            if !s.charset.is_empty() {
                response.set_charset(Some(&CefString::from(s.charset.as_str())));
            }
            for (name, value) in &s.headers {
                // The body is handed over decoded and whole; cookies are not the extension's.
                if ["content-encoding", "content-length", "transfer-encoding", "set-cookie"]
                    .iter()
                    .any(|h| name.eq_ignore_ascii_case(h))
                {
                    continue;
                }
                response.set_header_by_name(Some(&CefString::from(name.as_str())), Some(&CefString::from(value.as_str())), 0);
            }
            if let Some(len) = response_length {
                *len = s.body.len() as i64;
            }
        }

        fn read(
            &self,
            data_out: *mut u8,
            bytes_to_read: ::std::os::raw::c_int,
            bytes_read: Option<&mut ::std::os::raw::c_int>,
            _callback: Option<&mut ResourceReadCallback>,
        ) -> ::std::os::raw::c_int {
            let Ok(mut s) = self.state.lock() else { return 0 };
            let offset = s.offset;
            let n = s.body.len().saturating_sub(offset).min(usize::try_from(bytes_to_read).unwrap_or(0));
            if n == 0 || data_out.is_null() {
                if let Some(b) = bytes_read {
                    *b = 0;
                }
                return 0;
            }
            // SAFETY: CEF provides `data_out` with room for `bytes_to_read` bytes; `n` is at
            // most that and within the body.
            unsafe { std::ptr::copy_nonoverlapping(s.body.as_ptr().add(offset), data_out, n) };
            s.offset += n;
            if let Some(b) = bytes_read {
                *b = n as i32;
            }
            1
        }

        fn cancel(&self) {
            let job = self.state.lock().map_or(0, |mut s| {
                s.cancelled = true;
                s.job
            });
            if job != 0 {
                post_ui_from_any(move || {
                    if let Some(r) = RUNNING.with(|j| j.borrow().get(&job).cloned()) {
                        r.cancel();
                    }
                });
            }
        }
    }
}

/// A CEF callback moved between threads. CEF callbacks are reference counted with atomic
/// counts and may be executed on any thread.
struct CallbackHandle(Callback);
// SAFETY: see above; the handle is only used to call `cont`.
unsafe impl Send for CallbackHandle {}
// SAFETY: as above.
unsafe impl Sync for CallbackHandle {}

// For views without a request policy (an extension's background, popup, offscreen document,
// windows): nothing but the relay.
wrap_resource_request_handler! {
    struct RelayOnly {
        initiator: String,
    }
    impl ResourceRequestHandler {
        // The default answer of the binding would cancel every request of the view.
        fn on_before_resource_load(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            _callback: Option<&mut Callback>,
        ) -> ReturnValue {
            ReturnValue::CONTINUE
        }

        fn resource_handler(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
        ) -> Option<ResourceHandler> {
            wanted(&self.initiator, request?).then(handler)
        }
    }
}

/// The request handler of a view without a policy, when its request comes from an
/// extension origin (IO thread).
pub fn handler_for(initiator: Option<&CefString>) -> Option<ResourceRequestHandler> {
    let initiator = initiator?.to_string();
    is_source(&initiator).then(|| RelayOnly::new(initiator))
}
