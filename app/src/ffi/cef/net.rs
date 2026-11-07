//! Network hooks of a tab, on CEF's IO thread: every request of every tab
//! (sub-resources, sub-frames, workers) goes through [`RequestPolicy`],
//! which blocks ads and trackers, adds the privacy headers and decides
//! which requests may send and store cookies.

use cef::*;
use std::sync::{Arc, Mutex};

/// A request about to be sent.
pub struct NetRequest<'a> {
    pub url: &'a str,
    /// CEF `cef_resource_type_t`.
    pub resource_type: i32,
    /// Origin of the document that made the request (empty for the
    /// browser's own navigations).
    pub initiator: &'a str,
    /// The `Referer` the engine is sending (empty for none).
    pub referrer: &'a str,
}

/// What to do with a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetVerdict {
    /// Send it, optionally with `DNT: 1` / `Sec-GPC: 1`, and with the `Referer` trimmed to
    /// `referrer` (`Some("")` drops it; `None` leaves the engine's choice).
    Allow {
        dnt: bool,
        gpc: bool,
        referrer: Option<String>,
        /// Custom headers: set (`Some(value)`) or removed (`None`).
        headers: Vec<(String, Option<String>)>,
    },
    Block,
    /// Do not send it; answer with this body instead (`$redirect`).
    Respond {
        content_type: String,
        body: Vec<u8>,
    },
}

/// The per-tab network policy. Runs on the IO thread for every request, so it
/// must be cheap and must not block.
pub trait RequestPolicy: Send + Sync {
    fn check(&self, request: &NetRequest<'_>) -> NetVerdict;

    /// May a request to `url` send or store cookies (third-party blocking)?
    fn cookie_allowed(&self, url: &str) -> bool;

    /// A request finished with `bytes` of body received (the task manager's network column).
    fn loaded(&self, _bytes: u64) {}

    /// A request was answered with a redirect to `new_url` (bounce-tracker bookkeeping).
    fn redirected(&self, _resource_type: i32, _from_url: &str, _new_url: &str) {}
}

/// Rewrites the request's header map without the named headers (case-insensitive).
fn remove_headers(request: &mut Request, names: &[&str]) {
    let Some(mut map) = string_multimap_alloc() else { return };
    request.header_map(Some(&mut map));
    let Some(mut kept) = string_multimap_alloc() else { return };
    let n = string_multimap_size(Some(&mut map));
    for i in 0..n {
        let mut key = CefString::from("");
        let mut value = CefString::from("");
        string_multimap_key(Some(&mut map), i, Some(&mut key));
        string_multimap_value(Some(&mut map), i, Some(&mut value));
        let k = key.to_string();
        if !names.iter().any(|r| r.eq_ignore_ascii_case(&k)) {
            string_multimap_append(Some(&mut kept), Some(&key), Some(&value));
        }
    }
    request.set_header_map(Some(&mut kept));
    string_multimap_free(Some(&mut map));
    string_multimap_free(Some(&mut kept));
}

/// A `$redirect` stand-in (content type, body) waiting to be served.
type Standin = Arc<Mutex<Option<(String, Vec<u8>)>>>;

/// `RequestHandler::get_resource_request_handler` for a tab.
pub(super) fn handler_for(policy: &Arc<dyn RequestPolicy>, initiator: Option<&CefString>) -> ResourceRequestHandler {
    let initiator = initiator.map(|i| i.to_string()).unwrap_or_default();
    TabRequests::new(policy.clone(), initiator, Arc::new(Mutex::new(None)))
}

wrap_resource_request_handler! {
    struct TabRequests {
        policy: Arc<dyn RequestPolicy>,
        initiator: String,
        // A stand-in response decided in `on_before_resource_load`.
        respond: Standin,
    }
    impl ResourceRequestHandler {
        fn cookie_access_filter(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
        ) -> Option<CookieAccessFilter> {
            Some(CookieFilter::new(self.policy.clone()))
        }

        fn resource_handler(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
        ) -> Option<ResourceHandler> {
            let (content_type, body) = self.respond.lock().ok()?.take()?;
            Some(super::scheme::static_handler(super::InternalResponse {
                status: 200,
                reason: "OK",
                headers: format!(
                    "Content-Type: {content_type}\r\nAccess-Control-Allow-Origin: *\r\nCache-Control: no-store"
                ),
                body: std::borrow::Cow::Owned(body),
            }))
        }

        fn on_resource_redirect(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _response: Option<&mut Response>,
            new_url: Option<&mut CefString>,
        ) {
            let (Some(request), Some(new_url)) = (request, new_url) else { return };
            let resource_type = sys::cef_resource_type_t::from(request.resource_type()) as i32;
            let from = CefString::from(&request.url()).to_string();
            self.policy.redirected(resource_type, &from, &new_url.to_string());
        }

        fn on_resource_load_complete(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            _response: Option<&mut Response>,
            _status: UrlrequestStatus,
            received_content_length: i64,
        ) {
            self.policy.loaded(received_content_length.max(0) as u64);
        }

        fn on_before_resource_load(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _callback: Option<&mut Callback>,
        ) -> ReturnValue {
            let Some(request) = request else { return ReturnValue::CONTINUE };
            let url = CefString::from(&request.url()).to_string();
            let resource_type = sys::cef_resource_type_t::from(request.resource_type()) as i32;
            let referrer = CefString::from(&request.referrer_url()).to_string();
            let verdict = self.policy.check(&NetRequest {
                url: &url,
                resource_type,
                initiator: &self.initiator,
                referrer: &referrer,
            });
            match verdict {
                NetVerdict::Block => ReturnValue::CANCEL,
                NetVerdict::Respond { content_type, body } => {
                    if let Ok(mut r) = self.respond.lock() {
                        *r = Some((content_type, body));
                    }
                    ReturnValue::CONTINUE
                }
                NetVerdict::Allow { dnt, gpc, referrer, headers } => {
                    if dnt {
                        request.set_header_by_name(Some(&CefString::from("DNT")), Some(&CefString::from("1")), 1);
                    }
                    if gpc {
                        request.set_header_by_name(Some(&CefString::from("Sec-GPC")), Some(&CefString::from("1")), 1);
                    }
                    match referrer.as_deref() {
                        Some("") => request.set_referrer(None, ReferrerPolicy::NO_REFERRER),
                        Some(r) => request.set_referrer(Some(&CefString::from(r)), ReferrerPolicy::NEVER_CLEAR_REFERRER),
                        None => {}
                    }
                    let removed: Vec<&str> = headers.iter().filter(|(_, v)| v.is_none()).map(|(n, _)| n.as_str()).collect();
                    if !removed.is_empty() {
                        remove_headers(request, &removed);
                    }
                    for (name, value) in &headers {
                        if let Some(v) = value {
                            request.set_header_by_name(Some(&CefString::from(name.as_str())), Some(&CefString::from(v.as_str())), 1);
                        }
                    }
                    ReturnValue::CONTINUE
                }
            }
        }
    }
}

wrap_cookie_access_filter! {
    struct CookieFilter {
        policy: Arc<dyn RequestPolicy>,
    }
    impl CookieAccessFilter {
        fn can_send_cookie(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _cookie: Option<&Cookie>,
        ) -> ::std::os::raw::c_int {
            let url = request.map(|r| CefString::from(&r.url()).to_string()).unwrap_or_default();
            ::std::os::raw::c_int::from(self.policy.cookie_allowed(&url))
        }

        fn can_save_cookie(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _response: Option<&mut Response>,
            _cookie: Option<&Cookie>,
        ) -> ::std::os::raw::c_int {
            let url = request.map(|r| CefString::from(&r.url()).to_string()).unwrap_or_default();
            ::std::os::raw::c_int::from(self.policy.cookie_allowed(&url))
        }
    }
}
