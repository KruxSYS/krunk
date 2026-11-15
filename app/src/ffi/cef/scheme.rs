//! The internal origins (`https://shell.krunk.invalid` and
//! `https://krunk.invalid`): every request to them is answered from
//! embedded bytes or the favicon cache, and never reaches the network
//! (unknown paths get a 404 from here, not a DNS lookup).

use super::post_ui_from_any;
use cef::*;
use lb_core::pages;
use std::borrow::Cow;
use std::sync::{Arc, Mutex};

/// What to answer for one request.
pub struct InternalResponse {
    pub status: i32,
    pub reason: &'static str,
    /// CRLF-separated `Name: value` lines.
    pub headers: String,
    pub body: Cow<'static, [u8]>,
}

impl InternalResponse {
    pub fn not_found() -> InternalResponse {
        InternalResponse {
            status: 404,
            reason: "Not Found",
            headers: "Content-Type: text/plain\r\nX-Content-Type-Options: nosniff".into(),
            body: Cow::Borrowed(b"Not found"),
        }
    }
}

/// Registers the factory for the internal origins on a request context. The
/// shell origin exists only in the default profile (the shell and popup).
pub(super) fn register(ctx: Option<RequestContext>, with_shell: bool) {
    let Some(ctx) = ctx else { return };
    let https = CefString::from("https");
    let mut hosts = vec![host_of(pages::PAGES_ORIGIN)];
    if with_shell {
        hosts.push(host_of(pages::SHELL_ORIGIN));
    }
    for host in hosts {
        let mut factory = InternalFactory::new();
        ctx.register_scheme_handler_factory(Some(&https), Some(&CefString::from(host)), Some(&mut factory));
    }
}

/// Registers the factory for one more host on the default profile (an extension's origin,
/// 0.6.0). Registering a host twice is harmless.
pub(super) fn register_host(ctx: Option<RequestContext>, host: &str) {
    let Some(ctx) = ctx else { return };
    let mut factory = InternalFactory::new();
    ctx.register_scheme_handler_factory(
        Some(&CefString::from("https")),
        Some(&CefString::from(host)),
        Some(&mut factory),
    );
}

fn host_of(origin: &str) -> &str {
    origin.strip_prefix("https://").unwrap_or(origin)
}

wrap_scheme_handler_factory! {
    struct InternalFactory;
    impl SchemeHandlerFactory {
        fn create(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _scheme_name: Option<&CefString>,
            _request: Option<&mut Request>,
        ) -> Option<ResourceHandler> {
            // Always a handler: falling back to default handling would send
            // the request to the network.
            Some(InternalHandler::new(Arc::new(Mutex::new(Exchange::default()))))
        }
    }
}

/// A handler that answers with `response` at once (no UI-thread lookup);
/// used for the stand-ins of `$redirect` filters.
pub(super) fn static_handler(response: InternalResponse) -> ResourceHandler {
    InternalHandler::new(Arc::new(Mutex::new(Exchange { response: Some(response), offset: 0 })))
}

/// State of one request, shared between the IO thread and the UI thread
/// that resolves it.
#[derive(Default)]
struct Exchange {
    response: Option<InternalResponse>,
    offset: usize,
}

wrap_resource_handler! {
    struct InternalHandler {
        state: Arc<Mutex<Exchange>>,
    }
    impl ResourceHandler {
        fn open(
            &self,
            request: Option<&mut Request>,
            handle_request: Option<&mut ::std::os::raw::c_int>,
            callback: Option<&mut Callback>,
        ) -> ::std::os::raw::c_int {
            // A response known up front (a `$redirect` stand-in).
            if self.state.lock().is_ok_and(|s| s.response.is_some()) {
                if let Some(h) = handle_request {
                    *h = 1;
                }
                return 1;
            }
            let (Some(request), Some(callback)) = (request, callback) else { return 0 };
            let url = CefString::from(&request.url()).to_string();
            if let Some(h) = handle_request {
                // Asynchronous: the favicon cache lives on the UI thread.
                *h = 0;
            }
            let state = self.state.clone();
            let callback = CallbackHandle(Arc::new(Mutex::new(Some(callback.clone()))));
            // 0.22.0: a watchdog answers 404 if the UI thread did not in time, so a held
            // callback never keeps the request (and the page's load) open for good.
            let (watch_state, watch_cb) = (state.clone(), callback.clone());
            super::post_io_delayed(INTERNAL_TIMEOUT_MS, move || {
                if let Some(cb) = watch_cb.take() {
                    if let Ok(mut s) = watch_state.lock() {
                        s.response.get_or_insert_with(InternalResponse::not_found);
                    }
                    cb.cont();
                }
            });
            post_ui_from_any(move || {
                let Some(cb) = callback.take() else { return };
                let response = super::internal_response(&url);
                if let Ok(mut s) = state.lock() {
                    s.response = Some(response);
                }
                cb.cont();
            });
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
            let fallback = InternalResponse::not_found();
            let r = s.response.as_ref().unwrap_or(&fallback);
            response.set_status(r.status);
            response.set_status_text(Some(&CefString::from(r.reason)));
            for line in r.headers.split("\r\n") {
                let Some((name, value)) = line.split_once(':') else { continue };
                let (name, value) = (name.trim(), value.trim());
                if name.eq_ignore_ascii_case("Content-Type") {
                    let (mime, charset) = split_content_type(value);
                    response.set_mime_type(Some(&CefString::from(mime)));
                    if let Some(c) = charset {
                        response.set_charset(Some(&CefString::from(c)));
                    }
                }
                response.set_header_by_name(Some(&CefString::from(name)), Some(&CefString::from(value)), 1);
            }
            if let Some(len) = response_length {
                *len = r.body.len() as i64;
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
            let Some(body) = s.response.as_ref().map(|r| &r.body) else { return 0 };
            let n = body.len().saturating_sub(offset).min(usize::try_from(bytes_to_read).unwrap_or(0));
            if n == 0 || data_out.is_null() {
                if let Some(b) = bytes_read {
                    *b = 0;
                }
                return 0;
            }
            // SAFETY: CEF provides `data_out` with room for `bytes_to_read`
            // bytes; `n` is at most that and within `body`.
            unsafe { std::ptr::copy_nonoverlapping(body.as_ptr().add(offset), data_out, n) };
            s.offset += n;
            if let Some(b) = bytes_read {
                *b = n as i32;
            }
            1
        }

        fn cancel(&self) {
            if let Ok(mut s) = self.state.lock() {
                s.response = None;
            }
        }
    }
}

/// How long an internal request may wait for the UI thread's answer (0.22.0).
const INTERNAL_TIMEOUT_MS: i64 = 10_000;

/// A CEF callback moved to the UI thread, shared with its watchdog: whoever takes it first
/// continues it. CEF callbacks are reference counted with atomic counts and may be executed
/// on any thread.
#[derive(Clone)]
struct CallbackHandle(Arc<Mutex<Option<Callback>>>);
// SAFETY: see above; the callback is behind a mutex and `cont` is called once.
unsafe impl Send for CallbackHandle {}

impl CallbackHandle {
    fn take(&self) -> Option<Callback> {
        self.0.lock().ok().and_then(|mut c| c.take())
    }
}

/// `"text/html; charset=utf-8"` -> `("text/html", Some("utf-8"))`.
fn split_content_type(value: &str) -> (&str, Option<&str>) {
    let mut parts = value.split(';');
    let mime = parts.next().unwrap_or("").trim();
    let charset = parts.find_map(|p| p.trim().strip_prefix("charset=")).map(str::trim);
    (mime, charset)
}
