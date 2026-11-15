//! Custom DNS in the browser (0.6.0): starts the loopback stub when the settings ask for it,
//! points the engine's DoH client at the right place, and answers the settings page.

use super::Browser;
use crate::dns::{self, Route};
use crate::ffi::cef as engine;
use crate::log::{info, warning};
use lb_core::protocol::{Event, Request};
use lb_core::tabs::TabId;

impl Browser {
    /// Applies the DNS settings: the stub (started on first use), its list and the engine's
    /// DoH preferences. Called at startup and after a settings change.
    pub(super) fn apply_dns(&self) {
        let settings = self.settings.borrow().dns.clone();
        // 0.14.0: while the VPN runs (or should), names are resolved inside the tunnel: the
        // engine's own DNS-over-HTTPS is off and the stub refuses every query (nothing is asked
        // over the system's network).
        // 0.22.0: with the VPN for private tabs only, normal tabs keep the DNS settings (private
        // tabs' names are resolved by the VPN's proxy).
        let vpn = self.vpn_engaged() && !self.vpn_private_only();
        if let Some(e) = self.dns.borrow().as_ref() {
            e.set_refusing(vpn);
        }
        if vpn {
            let ok = engine::set_doh("off", "");
            info!(
                "dns: the VPN runs: the engine's secure DNS is off, the stub refuses{}",
                if ok { "" } else { " (not applied)" }
            );
            return;
        }
        if settings.active() && self.dns.borrow().is_none() {
            match dns::start(&settings) {
                Some(e) => *self.dns.borrow_mut() = Some(e),
                None => warning!("dns: the loopback stub could not start"),
            }
        }
        if let Some(e) = self.dns.borrow().as_ref() {
            e.configure(&settings);
        }
        let route = dns::route(&settings, self.dns.borrow().as_ref());
        let (mode, templates) = route.prefs();
        let ok = engine::set_doh(mode, &templates);
        match &route {
            Route::System => {
                info!("dns: the system resolver (engine mode {mode}{})", if ok { "" } else { ", not applied" })
            }
            Route::Direct(t) => info!(
                "dns: the engine asks {} DoH resolver(s) directly, secure mode{}",
                t.len(),
                if ok { "" } else { " (not applied)" }
            ),
            Route::Stub(t) => info!(
                "dns: the engine asks the loopback stub ({t}), secure mode{}; {} resolver(s) behind it",
                if ok { "" } else { " (not applied)" },
                settings.resolvers.iter().filter(|r| r.kind.supported()).count()
            ),
        }
    }

    pub(super) fn dispatch_dns(&self, tab: Option<TabId>, req_id: Option<u32>, request: Request) {
        match request {
            Request::DnsStatus => {
                let settings = self.settings.borrow().dns.clone();
                let engine = self.dns.borrow();
                let route = match dns::route(&settings, engine.as_ref()) {
                    Route::System => "system",
                    Route::Direct(_) => "direct",
                    Route::Stub(_) => "stub",
                };
                let mut v = engine.as_ref().map(|e| e.status()).unwrap_or_else(|| serde_json::json!({}));
                v["route"] = serde_json::Value::String(route.into());
                v["spki"] =
                    serde_json::Value::String(dns::identity().map(|i| i.spki_sha256.clone()).unwrap_or_default());
                self.reply(tab, Event::ok(req_id, v));
            }
            Request::DnsParse { text } => {
                if text.len() > 2048 {
                    return self.reply(tab, Event::err(req_id, "Too long"));
                }
                self.reply(
                    tab,
                    match lb_core::dns::Resolver::parse(&text) {
                        Ok(r) => Event::ok(req_id, r),
                        Err(e) => Event::err(req_id, e),
                    },
                );
            }
            Request::DnsTest { index } => {
                let Some(tester) = self.dns.borrow().as_ref().map(|e| e.tester()) else {
                    return self
                        .reply(tab, Event::err(req_id, "Custom DNS is off: choose it and add the resolver first"));
                };
                let address = self
                    .settings
                    .borrow()
                    .dns
                    .resolvers
                    .iter()
                    .filter(|r| r.kind.supported())
                    .nth(index as usize)
                    .map(|r| r.address());
                self.spawn_job(
                    "dns-test",
                    move || tester.test(index as usize),
                    move |b, result| {
                        match &result {
                            Ok(ms) => info!("dns: test of {}: {ms} ms", address.as_deref().unwrap_or("?")),
                            Err(e) => info!("dns: test of {}: {e}", address.as_deref().unwrap_or("?")),
                        }
                        b.reply(
                            tab,
                            match result {
                                Ok(ms) => Event::ok(req_id, serde_json::json!({ "ms": ms })),
                                Err(e) => Event::err(req_id, e),
                            },
                        );
                    },
                );
            }
            _ => {}
        }
    }
}
