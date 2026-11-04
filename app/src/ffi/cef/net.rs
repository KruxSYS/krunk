//! Network hooks of a tab, on CEF's IO thread: every request of every tab
//! (sub-resources, sub-frames, workers) goes through [`RequestPolicy`],
//! which blocks ads and trackers and adds the privacy headers.

use cef::*;
use std::sync::Arc;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetVerdict {
    /// Send it, optionally with `DNT: 1` / `Sec-GPC: 1`.
    Allow {
        dnt: bool,
        gpc: bool,
    },
    Block,
}

/// The per-tab network policy. Runs on the IO thread for every request, so it
/// must be cheap and must not block.
pub trait RequestPolicy: Send + Sync {
    fn check(&self, request: &NetRequest<'_>) -> NetVerdict;
}

/// `RequestHandler::get_resource_request_handler` for a tab.
pub(super) fn handler_for(policy: &Arc<dyn RequestPolicy>, initiator: Option<&CefString>) -> ResourceRequestHandler {
    let initiator = initiator.map(|i| i.to_string()).unwrap_or_default();
    TabRequests::new(policy.clone(), initiator)
}

wrap_resource_request_handler! {
    struct TabRequests {
        policy: Arc<dyn RequestPolicy>,
        initiator: String,
    }
    impl ResourceRequestHandler {
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
