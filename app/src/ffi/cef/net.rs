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
}

/// What to do with a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetVerdict {
    /// Send it, optionally with `DNT: 1` / `Sec-GPC: 1`.
    Allow {
        dnt: bool,
        gpc: bool,
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
            let verdict = self.policy.check(&NetRequest { url: &url, resource_type, initiator: &self.initiator });
            match verdict {
                NetVerdict::Block => ReturnValue::CANCEL,
                NetVerdict::Respond { content_type, body } => {
                    if let Ok(mut r) = self.respond.lock() {
                        *r = Some((content_type, body));
                    }
                    ReturnValue::CONTINUE
                }
                NetVerdict::Allow { dnt, gpc } => {
                    if dnt {
                        request.set_header_by_name(Some(&CefString::from("DNT")), Some(&CefString::from("1")), 1);
                    }
                    if gpc {
                        request.set_header_by_name(Some(&CefString::from("Sec-GPC")), Some(&CefString::from("1")), 1);
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
