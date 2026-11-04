//! `CefApp` for the browser process (switches, start-up, profile
//! preferences) and for subprocesses (the renderer bridge).

use super::renderer;
use crate::log::warning;
use cef::*;

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
            for s in &self.switches {
                let Some(body) = s.strip_prefix("--") else { continue };
                match body.split_once('=') {
                    Some((name, value)) => {
                        cl.append_switch_with_value(Some(&CefString::from(name)), Some(&CefString::from(value)))
                    }
                    None => cl.append_switch(Some(&CefString::from(body))),
                }
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
    }
}

wrap_request_context_handler! {
    struct LbContextHandler;
    impl RequestContextHandler {
        fn on_request_context_initialized(&self, request_context: Option<&mut RequestContext>) {
            if let Some(ctx) = request_context {
                apply_profile_prefs(ctx);
            }
        }
    }
}

/// Handler for every request context (sets the profile preferences).
pub(super) fn context_handler() -> RequestContextHandler {
    LbContextHandler::new()
}

/// Profile preferences of every request context: no password manager or
/// autofill (LightBrowser has its own vault), no network prediction, and
/// third-party cookie blocking.
fn apply_profile_prefs(ctx: &mut RequestContext) {
    let block_3pc = super::block_third_party_cookies();
    let prefs: &[(&str, Pref)] = &[
        ("credentials_enable_service", Pref::Bool(false)),
        ("credentials_enable_autosignin", Pref::Bool(false)),
        ("profile.password_manager_leak_detection", Pref::Bool(false)),
        ("autofill.profile_enabled", Pref::Bool(false)),
        ("autofill.credit_card_enabled", Pref::Bool(false)),
        // 2 = never predict (no DNS prefetch or preconnect to link targets).
        ("net.network_prediction_options", Pref::Int(2)),
        // 1 = block third-party cookies, 0 = allow.
        ("profile.cookie_controls_mode", Pref::Int(if block_3pc { 1 } else { 0 })),
    ];
    for (name, value) in prefs {
        set_pref(ctx, name, *value);
    }
}

#[derive(Clone, Copy)]
enum Pref {
    Bool(bool),
    Int(i32),
}

fn set_pref(ctx: &mut RequestContext, name: &str, pref: Pref) {
    let Some(mut value) = value_create() else { return };
    match pref {
        Pref::Bool(b) => value.set_bool(i32::from(b)),
        Pref::Int(i) => value.set_int(i),
    };
    let name = CefString::from(name);
    if ctx.can_set_preference(Some(&name)) == 0 {
        warning!("engine preference {name} cannot be set");
        return;
    }
    let mut error = CefString::from("");
    if ctx.set_preference(Some(&name), Some(&mut value), Some(&mut error)) == 0 {
        warning!("engine preference {name}: {error}");
    }
}
