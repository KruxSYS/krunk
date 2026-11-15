//! Certificate errors: the `krunk://cert-error` interstitial, "Continue anyway" and the
//! exceptions it makes (rules and storage in `lb_core::cert_exceptions`).
//!
//! The engine asks at every certificate error of a main-frame navigation. An exception the
//! user made for that site and that exact certificate lets it continue; otherwise it refuses
//! and, once the load has failed, the tab shows the interstitial. The engine itself also
//! remembers a "continue" for the session: removing an exception clears that too.

use super::blocking::TabNet;
use super::Browser;
use crate::ffi::cef::CertErrorSeen;
use crate::log::{info, warning};
use lb_core::cert_exceptions::{self, CertProblem};
use lb_core::navmenu;
use lb_core::pages::Page;
use lb_core::protocol::Request;
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;

impl Browser {
    /// The engine refused a certificate in tab `id`: true to continue past it.
    pub(super) fn cert_error_seen(&self, id: TabId, net: &TabNet, seen: &CertErrorSeen) -> bool {
        let private = self.tab(id).is_some_and(|t| t.private);
        let Some(problem) = CertProblem::new(&seen.url, seen.sha256.clone(), seen.error, seen.overridable) else {
            return false;
        };
        if problem.overridable && self.cert_allowed(private, &problem) {
            // No address: the log may be shared.
            info!("tab {id}: certificate error {} passed by an exception", problem.error);
            return true;
        }
        *net.cert_problem.borrow_mut() = Some(problem);
        net.cert_refused.set(true);
        false
    }

    fn cert_allowed(&self, private: bool, p: &CertProblem) -> bool {
        let session = if private { &self.filters.cert_private } else { &self.filters.cert_session };
        if session.borrow().allows(&p.host, p.port, &p.sha256) {
            return true;
        }
        // Remembered exceptions are the user's own choices about a site, so private tabs
        // honour them too (they never add any).
        match cert_exceptions::load(&self.store.borrow()) {
            Ok(list) => cert_exceptions::allows(&list, &p.host, p.port, &p.sha256),
            Err(e) => {
                warning!("certificate exceptions: {e}");
                false
            }
        }
    }

    /// The load refused for a certificate failed: show the interstitial instead of the
    /// engine's error page.
    pub(super) fn show_cert_interstitial(&self, id: TabId, net: &TabNet) {
        let Some(problem) = net.cert_problem.borrow().clone() else { return };
        let private = self.tab(id).is_some_and(|t| t.private);
        let page = problem.interstitial_url(private);
        self.defer(move |b| b.navigate_tab(id, &page));
    }

    pub(super) fn dispatch_certs(&self, tab: Option<TabId>, req_id: Option<u32>, request: Request) {
        match request {
            Request::CertProceed { url, sha256, remember } => self.cert_proceed(tab, &url, &sha256, remember),
            Request::CertBack {} => self.cert_back(tab),
            Request::CertExceptions => self.reply_db(tab, req_id, cert_exceptions::list(&self.store.borrow())),
            Request::CertExceptionRemove { host, port, sha256 } => {
                let r = cert_exceptions::remove(&self.store.borrow(), &host, port, &sha256);
                if matches!(r, Ok(true)) {
                    // The session's own copy (the same choice without "Remember") and the
                    // engine's memory of it go too, so the next visit warns again.
                    self.filters.cert_session.borrow_mut().remove(&host, port, &sha256);
                    crate::ffi::cef::clear_certificate_exceptions();
                }
                self.reply_db(tab, req_id, r);
            }
            _ => {}
        }
    }

    /// "Continue anyway": only from the interstitial the tab shows, and only for the error
    /// the tab really hit (the page's own values are not trusted).
    fn cert_proceed(&self, tab: Option<TabId>, url: &str, sha256: &str, remember: bool) {
        let Some(id) = tab else { return };
        let Some((private, on_interstitial)) =
            self.tab(id).map(|t| (t.private, Page::from_url(&t.url) == Some(Page::CertError)))
        else {
            return;
        };
        let net = self.views.borrow().get(&id).map(|v| v.net.clone());
        let matches =
            net.as_ref().is_some_and(|n| n.cert_problem.borrow().as_ref().is_some_and(|p| p.matches(url, sha256)));
        if !on_interstitial || !matches {
            warning!("tab {id}: \"Continue anyway\" refused: it does not match the certificate error the tab hit");
            return;
        }
        if private {
            self.filters.cert_private.borrow_mut().add(url, sha256);
        } else if remember {
            if let Err(e) = cert_exceptions::add(&self.store.borrow(), url, sha256, now_ms()) {
                warning!("certificate exceptions: {e}");
                return;
            }
        } else {
            self.filters.cert_session.borrow_mut().add(url, sha256);
        }
        self.navigate_tab(id, url);
    }

    /// "Back to safety": the last page before the refused address (the engine's error entry
    /// for it would only fail again), or the new tab page.
    fn cert_back(&self, tab: Option<TabId>) {
        let Some(id) = tab else { return };
        let Some(target) = self.tab(id).filter(|t| Page::from_url(&t.url) == Some(Page::CertError)).map(|t| {
            let net = self.views.borrow().get(&id).map(|v| v.net.clone());
            net.and_then(|n| n.cert_problem.borrow().as_ref().map(|p| p.url.clone()))
                .or_else(|| cert_exceptions::interstitial_target(&t.url))
                .unwrap_or_default()
        }) else {
            return;
        };
        let Some(view) = self.tab_view(id) else { return };
        let weak = self.me.clone();
        view.devtools_call("Page.getNavigationHistory", &serde_json::json!({}), move |r| {
            let Some(b) = weak.upgrade() else { return };
            let back = r.ok().as_ref().and_then(navmenu::parse_history).and_then(|(current, entries)| {
                entries
                    .iter()
                    .take(current)
                    .rev()
                    .find(|e| e.url != target && Page::from_url(&e.url) != Some(Page::CertError))
                    .map(|e| e.id)
            });
            match (back, b.tab_view(id)) {
                (Some(entry), Some(view)) => {
                    view.devtools_call("Page.navigateToHistoryEntry", &serde_json::json!({ "entryId": entry }), |r| {
                        if r.is_err() {
                            warning!("certificate interstitial: going back failed");
                        }
                    });
                }
                _ => b.navigate_tab(id, &Page::NewTab.url()),
            }
        });
    }

    /// The last private tab closed: its certificate exceptions go with it, also the engine's
    /// memory of them (the default profile's are asked again and still pass).
    pub(super) fn cert_private_closed(&self) {
        let had = !self.filters.cert_private.borrow().is_empty();
        self.filters.cert_private.borrow_mut().clear();
        if had {
            crate::ffi::cef::clear_certificate_exceptions();
        }
    }
}
