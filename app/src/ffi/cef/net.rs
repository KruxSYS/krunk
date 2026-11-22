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
    /// Send it, with the `Referer` trimmed to `referrer` (`Some("")` drops it; `None` leaves the
    /// engine's choice). (0.28.0: `DNT` and `Sec-GPC` come from the engine itself.)
    Allow {
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

    /// Request `id` (the engine's identifier, 0 for none) is about to be sent; `get`: its
    /// method is GET. `true`: it should not wait for the cache (0.18.0, docs/NETWORK.md:
    /// another tab's request for the same address has no answer yet).
    fn started(&self, _id: u64, _request: &NetRequest<'_>, _get: bool) -> bool {
        false
    }

    /// The headers of request `id`'s answer arrived.
    fn answered(&self, _id: u64) {}

    /// Request `id` ended: done, failed or cancelled.
    fn finished(&self, _id: u64) {}

    /// A request was answered with a redirect to `new_url` (bounce-tracker bookkeeping).
    fn redirected(&self, _resource_type: i32, _from_url: &str, _new_url: &str) {}

    /// 0.27.1: a top-level document arrived; `cf_challenge`: Cloudflare answered it with its
    /// challenge (`cf-mitigated: challenge`).
    fn main_frame_response(&self, _cf_challenge: bool) {}

    /// Where the load trace counts this tab's filtering cost (0.26.0; read only while it is on).
    fn perf_cost(&self) -> Option<&crate::perf::NetCost> {
        None
    }
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
            request: Option<&mut Request>,
        ) -> Option<ResourceHandler> {
            // An extension's page in a tab asking one of its granted hosts (0.17.0).
            if let Some(r) = request {
                if super::relay::wanted(&self.initiator, r) {
                    return Some(super::relay::handler());
                }
            }
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

        // A shared worker's script, loaded for the page that starts it (0.7.1, R12).
        fn resource_response_filter(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            response: Option<&mut Response>,
        ) -> Option<ResponseFilter> {
            let kind = sys::cef_resource_type_t::from(request?.resource_type());
            if kind != sys::cef_resource_type_t::RT_SHARED_WORKER || !super::hints::current().off {
                return None;
            }
            worker_prefix(response?)
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
            request: Option<&mut Request>,
            _response: Option<&mut Response>,
            _status: UrlrequestStatus,
            received_content_length: i64,
        ) {
            self.policy.loaded(received_content_length.max(0) as u64);
            if let Some(request) = request {
                self.policy.finished(request.identifier());
            }
        }

        fn on_resource_response(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            response: Option<&mut Response>,
        ) -> ::std::os::raw::c_int {
            if let Some(request) = request {
                self.policy.answered(request.identifier());
                if sys::cef_resource_type_t::from(request.resource_type()) == sys::cef_resource_type_t::RT_MAIN_FRAME {
                    let challenge = response.is_some_and(|r| {
                        CefString::from(&r.header_by_name(Some(&CefString::from("cf-mitigated"))))
                            .to_string()
                            .eq_ignore_ascii_case("challenge")
                    });
                    self.policy.main_frame_response(challenge);
                }
            }
            0
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
            let net_request = NetRequest { url: &url, resource_type, initiator: &self.initiator, referrer: &referrer };
            let t = crate::perf::now();
            let verdict = self.policy.check(&net_request);
            if matches!(verdict, NetVerdict::Allow { .. }) {
                let get = CefString::from(&request.method()).to_string().eq_ignore_ascii_case("GET");
                if self.policy.started(request.identifier(), &net_request, get) {
                    // What a hard reload sends: the cache then starts a new entry instead of
                    // queueing behind the request that has it (measured, docs/NETWORK.md).
                    // (A header, not `set_flags`: CEF would replace all the request's load
                    // flags and drop its credentials mode.)
                    for (name, value) in [("Cache-Control", "no-cache"), ("Pragma", "no-cache")] {
                        request.set_header_by_name(Some(&CefString::from(name)), Some(&CefString::from(value)), 1);
                    }
                }
            }
            if t != 0 {
                if let Some(cost) = self.policy.perf_cost() {
                    cost.request(t, resource_type == 0, !matches!(verdict, NetVerdict::Allow { .. }));
                }
            }
            match verdict {
                NetVerdict::Block => ReturnValue::CANCEL,
                NetVerdict::Respond { content_type, body } => {
                    if let Ok(mut r) = self.respond.lock() {
                        *r = Some((content_type, body));
                    }
                    ReturnValue::CONTINUE
                }
                NetVerdict::Allow { referrer, headers } => {
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
            ::std::os::raw::c_int::from(cookie_allowed(&self.policy, request))
        }

        fn can_save_cookie(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _response: Option<&mut Response>,
            _cookie: Option<&Cookie>,
        ) -> ::std::os::raw::c_int {
            ::std::os::raw::c_int::from(cookie_allowed(&self.policy, request))
        }
    }
}

/// The cookie filter's answer, timed for the load trace.
fn cookie_allowed(policy: &Arc<dyn RequestPolicy>, request: Option<&mut Request>) -> bool {
    let t = crate::perf::now();
    let url = request.map(|r| CefString::from(&r.url()).to_string()).unwrap_or_default();
    let allowed = policy.cookie_allowed(&url);
    if t != 0 {
        if let Some(cost) = policy.perf_cost() {
            cost.cookie(t);
        }
    }
    allowed
}

/// The filter for a script response (2xx, a JavaScript type), else none.
fn worker_prefix(response: &mut Response) -> Option<ResponseFilter> {
    let mime = CefString::from(&response.mime_type()).to_string();
    if !(200..300).contains(&response.status()) || !lb_core::workerfix::is_script_mime(&mime) {
        return None;
    }
    // The first few in the log.
    static LOGGED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if LOGGED.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 5 {
        crate::log::info!("client hints off: a worker script gets the userAgentData removal");
    }
    Some(WorkerPrefix::new(Arc::new(Mutex::new(lb_core::workerfix::FilterState::new()))))
}

wrap_response_filter! {
    struct WorkerPrefix {
        state: Arc<Mutex<lb_core::workerfix::FilterState>>,
    }
    impl ResponseFilter {
        fn init_filter(&self) -> ::std::os::raw::c_int {
            1
        }

        fn filter(
            &self,
            data_in: Option<&mut Vec<u8>>,
            data_in_read: Option<&mut usize>,
            data_out: Option<&mut Vec<u8>>,
            data_out_written: Option<&mut usize>,
        ) -> ResponseFilterStatus {
            let input: &[u8] = data_in.as_deref().map_or(&[], Vec::as_slice);
            let mut none: Vec<u8> = Vec::new();
            let out = data_out.unwrap_or(&mut none);
            let Ok(mut state) = self.state.lock() else { return ResponseFilterStatus::ERROR };
            // All input is taken every time (the state holds what it cannot pass on yet).
            let (written, pending) = state.step(input, out.as_mut_slice());
            if let Some(r) = data_in_read {
                *r = input.len();
            }
            if let Some(w) = data_out_written {
                *w = written;
            }
            if pending {
                ResponseFilterStatus::NEED_MORE_DATA
            } else {
                ResponseFilterStatus::DONE
            }
        }
    }
}
