//! `CefApp` for the browser process (switches, start-up, profile
//! preferences) and for subprocesses (the renderer bridge).

use super::renderer;
use crate::log::{info, warning};
use cef::*;
use std::cell::RefCell;

/// WebRTC's interfaces: the default public one only (no local addresses, no mDNS responder)…
const WEBRTC_NORMAL: &str = "default_public_interface_only";
/// …and while the VPN runs (0.14.0) no UDP that doesn't go through the proxy: WebRTC can't
/// reveal the real address (the proxy carries no UDP, so peer-to-peer media relay over TCP or stop).
const WEBRTC_VPN: &str = "disable_non_proxied_udp";

/// The engine's proxy while the VPN runs: the `server` (`http://127.0.0.1:<port>`) and the
/// `bypass_list`. Kept so a private context created later gets it too.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct VpnProxy {
    pub server: String,
    pub bypass: String,
    /// 0.22.0: only the private context uses it; the global one stays direct.
    pub private_only: bool,
}

/// The VPN's proxy as `ctx` should have it: none for the global context when the VPN is for
/// private tabs only.
fn proxy_for<'a>(ctx: &RequestContext, proxy: Option<&'a VpnProxy>) -> Option<&'a VpnProxy> {
    proxy.filter(|p| !(p.private_only && ctx.is_global() != 0))
}

thread_local! {
    static VPN_PROXY: RefCell<Option<VpnProxy>> = const { RefCell::new(None) };
}

wrap_app! {
    struct LbApp {
        // Chromium switches (browser process only), `--name[=value]`.
        switches: Vec<String>,
    }
    impl App {
        fn on_before_command_line_processing(
            &self,
            process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            // Empty process type = the browser process.
            let browser = process_type.is_none_or(|t| t.to_string().is_empty());
            let Some(cl) = command_line else { return };
            if !browser {
                return;
            }
            // CEF has already added its own start-up switches (e.g. a --disable-features list).
            // Chromium keeps only the last occurrence of a switch, so a second one would replace
            // CEF's: list switches are merged, any other repeated switch is replaced by ours.
            for s in &self.switches {
                let Some(body) = s.strip_prefix("--") else { continue };
                let (name, value) = body.split_once('=').unwrap_or((body, ""));
                let key = CefString::from(name);
                let mut value = value.to_string();
                if cl.has_switch(Some(&key)) != 0 {
                    let existing = CefString::from(&cl.switch_value(Some(&key))).to_string();
                    if lb_core::settings::LIST_SWITCHES.contains(&name) {
                        value = lb_core::settings::merge_switch_list(&existing, &value);
                    } else if existing != value {
                        warning!("engine switch --{name}: the engine's value {existing:?} is replaced by Krunk's");
                    }
                    cl.remove_switch(Some(&key));
                }
                if value.is_empty() && !body.contains('=') {
                    cl.append_switch(Some(&key));
                } else {
                    cl.append_switch_with_value(Some(&key), Some(&CefString::from(value.as_str())));
                }
            }
            // Test builds only (`--features devtools-port`, scripts/build-dist.sh --fast):
            // KRUNK_DEVTOOLS_PORT=9222 opens the engine's DevTools port on 127.0.0.1, so a test
            // harness can read the console and network of every target, extension workers
            // included. Release builds do not contain this.
            #[cfg(feature = "devtools-port")]
            if let Some(port) = std::env::var("KRUNK_DEVTOOLS_PORT").ok().and_then(|p| p.parse::<u16>().ok()) {
                warning!("TEST BUILD: DevTools port {port} is open on 127.0.0.1");
                cl.append_switch_with_value(
                    Some(&CefString::from("remote-debugging-port")),
                    Some(&CefString::from(port.to_string().as_str())),
                );
                cl.append_switch_with_value(Some(&CefString::from("remote-allow-origins")), Some(&CefString::from("*")));
            }
            // Page-load harness builds only (`--features perf-harness`, build-dist.sh --harness):
            // KRUNK_HOST_RULES (e.g. "MAP *:443 127.0.0.1:8443, EXCLUDE localhost") goes after
            // Krunk's own rules, which keep matching first (internal hosts never resolve).
            #[cfg(feature = "perf-harness")]
            if let Some(extra) = std::env::var("KRUNK_HOST_RULES").ok().filter(|r| !r.trim().is_empty()) {
                let key = CefString::from("host-resolver-rules");
                let own = CefString::from(&cl.switch_value(Some(&key))).to_string();
                let rules = if own.is_empty() { extra } else { format!("{own}, {extra}") };
                warning!("TEST BUILD: host resolver rules {rules}");
                cl.remove_switch(Some(&key));
                cl.append_switch_with_value(Some(&key), Some(&CefString::from(rules.as_str())));
            }
        }

        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(LbBrowserProcess::new())
        }

        fn render_process_handler(&self) -> Option<RenderProcessHandler> {
            Some(renderer::handler())
        }
    }
}

/// The `CefApp` passed to `cef::initialize` in the browser process.
pub(super) fn browser_app(switches: Vec<String>) -> App {
    LbApp::new(switches)
}

/// The `CefApp` passed to `cef::execute_process` (subprocesses).
pub(super) fn subprocess_app() -> App {
    LbApp::new(Vec::new())
}

wrap_browser_process_handler! {
    struct LbBrowserProcess;
    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            super::context_initialized();
        }

        fn on_already_running_app_relaunch(
            &self,
            command_line: Option<&mut CommandLine>,
            _current_directory: Option<&CefString>,
        ) -> ::std::os::raw::c_int {
            let mut args = Vec::new();
            if let Some(cl) = command_line {
                let mut list = CefStringList::new();
                cl.arguments(Some(&mut list));
                args = list.into_iter().collect();
            }
            super::relaunched(args);
            // Handled: never let CEF open its own default window.
            1
        }

        fn default_request_context_handler(&self) -> Option<RequestContextHandler> {
            Some(context_handler())
        }

        fn on_before_child_process_launch(&self, command_line: Option<&mut CommandLine>) {
            // Chromium's thread profiler treats every build that is not a branded Chrome
            // release as a developer build and samples the main thread of each engine process
            // ten times a second (`--start-stack-profiler`, for crash-free UMA reports Krunk never
            // sends). It cost the pages a measurable share of their time (R24): never in Krunk.
            if let Some(cl) = command_line {
                cl.remove_switch(Some(&CefString::from(STACK_PROFILER_SWITCH)));
                // "Send client hints" off as of now (0.7.1).
                super::hints::add_to_child(cl);
                // 0.26.0: the load trace is on; renderers time Krunk's scripts per page context.
                if crate::perf::on() {
                    cl.append_switch(Some(&CefString::from(super::renderer::PERF_SWITCH)));
                }
            }
        }
    }
}

/// Chromium's `switches::kStartStackProfiler`.
const STACK_PROFILER_SWITCH: &str = "start-stack-profiler";

wrap_request_context_handler! {
    struct LbContextHandler;
    impl RequestContextHandler {
        fn on_request_context_initialized(&self, request_context: Option<&mut RequestContext>) {
            if let Some(ctx) = request_context {
                // Its browsers wait for this (`when_private_ready`): they never load a page
                // before the VPN's proxy is the profile's.
                if apply_profile_prefs(ctx) {
                    super::private_context_ready(ctx);
                } else {
                    super::private_context_failed(ctx);
                }
            }
        }
    }
}

/// Handler for the contexts created with `request_context_create_context` (the private
/// one): it sets the profile preferences. CEF never calls `on_request_context_initialized`
/// of the *default* handler for the global context, so `context_initialized` applies
/// them there itself.
pub(super) fn context_handler() -> RequestContextHandler {
    LbContextHandler::new()
}

/// 0.26.0 "Preload pages for faster browsing" (`Settings::preload_pages`), as of now.
static PRELOAD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// `net.network_prediction_options` for a context: 0 (standard: the page's preconnect and
/// DNS-prefetch hints, preconnect on link press, the site's usual hosts, speculation-rules
/// prefetch) in the default profile with the setting on; 2 (none) otherwise, and always in the
/// private profile. Every early connection uses the profile's proxy (the VPN's when it runs).
fn prediction_value(global: bool) -> i32 {
    use std::sync::atomic::Ordering::Relaxed;
    if global && PRELOAD.load(Relaxed) && !PRELOAD_OFFLINE.load(Relaxed) {
        0
    } else {
        2
    }
}

/// The VPN could not even put its stand-in proxy in place and Krunk is offline for normal tabs
/// (only its request filter refuses pages): no early connections then, they would go direct.
static PRELOAD_OFFLINE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(super) fn set_preload_offline(offline: bool, ctx: Option<&mut RequestContext>) -> bool {
    PRELOAD_OFFLINE.store(offline, std::sync::atomic::Ordering::Relaxed);
    match ctx {
        Some(ctx) => set_pref(ctx, "net.network_prediction_options", Pref::Int(prediction_value(ctx.is_global() != 0))),
        None => true,
    }
}

/// Changes the setting; applied to `ctx` (the default profile) at once when given.
pub(super) fn set_preload(on: bool, ctx: Option<&mut RequestContext>) -> bool {
    PRELOAD.store(on, std::sync::atomic::Ordering::Relaxed);
    match ctx {
        Some(ctx) => set_pref(ctx, "net.network_prediction_options", Pref::Int(prediction_value(ctx.is_global() != 0))),
        None => true,
    }
}

/// Profile preferences of every request context: no password manager or
/// autofill (Krunk has its own vault), network prediction only as `prediction_value` says, no
/// local IP addresses in WebRTC. False when the VPN runs and the context did not take its
/// proxy (its pages must not load).
pub(super) fn apply_profile_prefs(ctx: &mut RequestContext) -> bool {
    let prefs: &[(&str, Pref)] = &[
        ("credentials_enable_service", Pref::Bool(false)),
        ("credentials_enable_autosignin", Pref::Bool(false)),
        ("profile.password_manager_leak_detection", Pref::Bool(false)),
        ("autofill.profile_enabled", Pref::Bool(false)),
        ("autofill.credit_card_enabled", Pref::Bool(false)),
        // 0.26.0: "Preload pages" (0 = standard) in the default profile, never in private tabs
        // (2 = no preconnect, DNS prefetch or prefetch at all).
        ("net.network_prediction_options", Pref::Int(prediction_value(ctx.is_global() != 0))),
        // WebRTC uses only the default public interface: no local IP
        // addresses are gathered, so no mDNS responder listens on the LAN
        // (which also triggered a Windows Firewall prompt).
        ("webrtc.ip_handling_policy", Pref::Str(WEBRTC_NORMAL)),
    ];
    let applied = prefs.iter().filter(|(name, value)| set_pref(ctx, name, *value)).count();
    let scope = if ctx.is_global() != 0 { "default" } else { "private" };
    info!("engine preferences applied ({scope} profile): {applied}/{}", prefs.len());
    // 0.14.0: a context created while the VPN runs (the private one) uses it from the start;
    // 0.22.0: with the VPN for private tabs only, the global context is left direct.
    let proxy = VPN_PROXY.with(|v| v.borrow().clone());
    match proxy_for(ctx, proxy.as_ref()) {
        Some(p) => apply_vpn(ctx, Some(p)),
        None => true,
    }
}

/// Points the context at the VPN's proxy (`Some`) or back at the system's (`None`): the
/// `proxy` dictionary and WebRTC's UDP policy. False when the engine refused or kept another
/// value.
fn apply_vpn(ctx: &mut RequestContext, proxy: Option<&VpnProxy>) -> bool {
    let scope = if ctx.is_global() != 0 { "default" } else { "private" };
    let ok = match proxy {
        Some(p) => {
            let dict = [("mode", "fixed_servers"), ("server", p.server.as_str()), ("bypass_list", p.bypass.as_str())];
            set_pref(ctx, "proxy", Pref::Dict(&dict))
                & set_pref(ctx, "webrtc.ip_handling_policy", Pref::Str(WEBRTC_VPN))
        }
        None => reset_pref(ctx, "proxy") & set_pref(ctx, "webrtc.ip_handling_policy", Pref::Str(WEBRTC_NORMAL)),
    };
    match (proxy, ok) {
        (Some(p), true) => info!("vpn: the {scope} profile uses the proxy {}", p.server),
        (Some(_), false) => warning!("vpn: the {scope} profile did not take the proxy"),
        (None, true) => info!("vpn: the {scope} profile is back on the system's proxy settings"),
        (None, false) => warning!("vpn: the {scope} profile's proxy could not be reset"),
    }
    ok
}

/// Whether a context still holds a proxy of Krunk's VPN (a `fixed_servers` proxy on
/// 127.0.0.1), e.g. left on disk by a run that ended with the VPN on (0.14.1).
pub(super) fn vpn_proxy_left(ctx: &RequestContext) -> bool {
    let name = CefString::from("proxy");
    ctx.preference(Some(&name)).and_then(|v| v.dictionary()).is_some_and(|d| {
        let get = |k: &str| CefString::from(&d.string(Some(&CefString::from(k)))).to_string();
        get("mode") == "fixed_servers" && get("server").starts_with("http://127.0.0.1:")
    })
}

/// The VPN's proxy for every context: the global one now, the private one now (when it exists)
/// and whenever it is created; with `private_only` the global context goes back to the system's
/// settings instead. True when the engine took it everywhere.
pub(super) fn set_vpn_proxy(proxy: Option<VpnProxy>, contexts: &mut [RequestContext]) -> bool {
    VPN_PROXY.with(|v| *v.borrow_mut() = proxy.clone());
    let mut ok = true;
    for ctx in contexts {
        let want = proxy_for(ctx, proxy.as_ref());
        // Private tabs only, and the global context holds none of ours: already direct
        // (resetting an unset proxy preference makes the engine log a check failure, 0.14.1).
        if proxy.is_some() && want.is_none() && !vpn_proxy_left(ctx) {
            continue;
        }
        ok &= apply_vpn(ctx, want);
    }
    ok
}

/// Chromium's own DNS-over-HTTPS client (0.6.0 custom DNS): `mode` is `automatic` (the
/// system resolver, upgraded when possible) or `secure` (the templates only, never the
/// system resolver); `templates` is a space-separated list of DoH URLs. Global (local
/// state) preferences: they apply at once to every profile.
pub(super) fn set_doh(mode: &str, templates: &str) -> bool {
    let Some(pm) = preference_manager_get_global() else {
        warning!("dns: no global preference manager");
        return false;
    };
    let mut ok = true;
    for (name, value) in [("dns_over_https.mode", mode), ("dns_over_https.templates", templates)] {
        let Some(mut v) = value_create() else { return false };
        v.set_string(Some(&CefString::from(value)));
        let n = CefString::from(name);
        let mut error = CefString::from("");
        if pm.can_set_preference(Some(&n)) == 0 || pm.set_preference(Some(&n), Some(&mut v), Some(&mut error)) == 0 {
            warning!("dns: engine preference {name}: {error}");
            ok = false;
        }
    }
    ok
}

#[derive(Clone, Copy)]
enum Pref<'a> {
    Bool(bool),
    Int(i32),
    Str(&'a str),
    /// A dictionary of strings (0.14.0: the `proxy` preference).
    Dict(&'a [(&'a str, &'a str)]),
}

/// Sets one preference and reads it back; false (with a warning) when the
/// engine refused it or kept another value.
fn set_pref(ctx: &mut RequestContext, name: &str, pref: Pref) -> bool {
    let Some(mut value) = value_create() else { return false };
    match pref {
        Pref::Bool(b) => value.set_bool(i32::from(b)),
        Pref::Int(i) => value.set_int(i),
        Pref::Str(v) => value.set_string(Some(&CefString::from(v))),
        Pref::Dict(items) => {
            let Some(mut dict) = dictionary_value_create() else { return false };
            for (k, v) in items {
                dict.set_string(Some(&CefString::from(*k)), Some(&CefString::from(*v)));
            }
            value.set_dictionary(Some(&mut dict))
        }
    };
    let name = CefString::from(name);
    if ctx.can_set_preference(Some(&name)) == 0 {
        warning!("engine preference {name} cannot be set");
        return false;
    }
    let mut error = CefString::from("");
    if ctx.set_preference(Some(&name), Some(&mut value), Some(&mut error)) == 0 {
        warning!("engine preference {name}: {error}");
        return false;
    }
    let kept = ctx.preference(Some(&name)).is_some_and(|v| match pref {
        Pref::Bool(b) => (v.bool() != 0) == b,
        Pref::Int(i) => v.int() == i,
        Pref::Str(s) => CefString::from(&v.string()).to_string() == s,
        Pref::Dict(items) => v.dictionary().is_some_and(|d| {
            items.iter().all(|(k, want)| CefString::from(&d.string(Some(&CefString::from(*k)))).to_string() == *want)
        }),
    });
    if !kept {
        warning!("engine preference {name} was not kept");
    }
    kept
}

/// 0.26.0: the engine's `krunk.*` pseudo-preferences (patches/krunk-preconnect-pref.patch): never
/// stored, nothing to read back. False (with a warning, once per name) on an engine without them.
pub(super) fn set_krunk_pref(ctx: &mut RequestContext, name: &str, mut value: Value) -> bool {
    let n = CefString::from(name);
    let mut error = CefString::from("");
    let ok =
        ctx.can_set_preference(Some(&n)) != 0 && ctx.set_preference(Some(&n), Some(&mut value), Some(&mut error)) != 0;
    if !ok {
        thread_local! {
            static WARNED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        }
        WARNED.with(|w| {
            let mut w = w.borrow_mut();
            if !w.iter().any(|x| x == name) {
                warning!("engine preference {name}: not supported by this engine {error}");
                w.push(name.to_string());
            }
        });
    }
    ok
}

/// Resets a preference to its default (0.14.0: the `proxy` dictionary, back to the system's
/// settings) and checks that no value of Krunk's is left.
fn reset_pref(ctx: &mut RequestContext, name: &str) -> bool {
    let name = CefString::from(name);
    if ctx.can_set_preference(Some(&name)) == 0 {
        warning!("engine preference {name} cannot be set");
        return false;
    }
    let mut error = CefString::from("");
    if ctx.set_preference(Some(&name), None, Some(&mut error)) == 0 {
        warning!("engine preference {name}: {error}");
        return false;
    }
    let left = ctx
        .preference(Some(&name))
        .and_then(|v| v.dictionary())
        .is_some_and(|d| CefString::from(&d.string(Some(&CefString::from("mode")))).to_string() == "fixed_servers");
    if left {
        warning!("engine preference {name} was not reset");
    }
    !left
}
