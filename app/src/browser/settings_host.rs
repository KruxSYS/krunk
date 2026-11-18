//! Settings: get/save/reset with validation, live application of every
//! setting that does not need a restart, restart, and clear-on-exit.

use super::blocking::Fetching;
use super::{Browser, PopupSlot};
use crate::ffi::cef::{self as engine, ClearKinds};
use crate::ffi::dialogs;
use crate::ffi::process;
use crate::log::{info, warning};
use lb_core::pages::Page;
use lb_core::protocol::{Event, Request};
use lb_core::settings::{Settings, Theme};
use lb_core::tabs::TabId;
use std::path::Path;

/// Timer: give up waiting for clear-on-exit after this long.
pub(super) const EXIT_TIMER: usize = 6;
const EXIT_TIMEOUT_MS: u32 = 5000;

impl Browser {
    pub(super) fn dispatch_settings(&self, tab: Option<TabId>, req_id: Option<u32>, request: Request) {
        match request {
            Request::GetSettings => {
                let s = self.settings.borrow().clone();
                let data = serde_json::json!({
                    "settings": s,
                    "restartRequired": s.restart_required(&self.running_settings),
                    "about": self.about(),
                    "userAgents": lb_core::useragents::PRESETS,
                    "defaultUserAgent": engine::default_user_agent(),
                    "engineHints": self.engine_hints.borrow().clone(),
                    "cdm": widevine_cdm_version(&self.dirs.profile()),
                    "dnsPresets": lb_core::dns::PRESETS.iter().map(|p| serde_json::json!({ "id": p.id, "name": p.name, "resolver": p.resolver() })).collect::<Vec<_>>(),
                    "wallpapers": super::library::wallpaper_list(),
                    "ntpImageUrl": (!s.ntp.image.is_empty()).then(|| lb_core::assets::ntp_image_url(&s.ntp.image)),
                });
                self.reply(tab, Event::ok(req_id, data));
            }
            Request::SaveSettings { settings } => {
                // The DevTools layout is set by its splitter and dock control, not on the
                // settings page: keep the live values (the page may have loaded older ones).
                let mut s = *settings;
                {
                    let cur = self.settings.borrow();
                    // The filter lists are edited on krunk://adblock.
                    s.filter_lists = cur.filter_lists.clone();
                    s.custom_lists = cur.custom_lists.clone();
                    s.devtools_dock = cur.devtools_dock;
                    s.devtools_size_right = cur.devtools_size_right;
                    s.devtools_size_bottom = cur.devtools_size_bottom;
                    // 0.12.0: providers change through their own commands (aiProviderSave …),
                    // the panel width by its splitter.
                    s.ai.providers = cur.ai.providers.clone();
                    s.ai.panel_width = cur.ai.panel_width;
                    // 0.14.0: the VPN's profile used last is set by connecting.
                    s.vpn.active_profile = cur.vpn.active_profile.clone();
                    if s.ai.provider(&s.ai.active_provider).is_none() {
                        s.ai.active_provider.clear();
                    }
                }
                self.save_settings(tab, req_id, s)
            }
            Request::ResetSettings => {
                // The AI providers stay (their keys are kept); AI itself goes off.
                let mut d = Settings::default();
                d.ai.providers = self.settings.borrow().ai.providers.clone();
                // The VPN profiles stay too (the button and auto-connect go off).
                d.vpn.active_profile = self.settings.borrow().vpn.active_profile.clone();
                self.save_settings(tab, req_id, d)
            }
            Request::PickDownloadFolder => {
                let Some(w) = self.window() else { return };
                let current = self.settings.borrow().download_dir.clone();
                let start = (!current.is_empty()).then(|| Path::new(&current).to_path_buf());
                let picked = dialogs::pick_folder(w.hwnd(), "Save downloads to", start.as_deref());
                self.reply(tab, Event::ok(req_id, picked.map(|p| p.display().to_string())));
            }
            Request::DefaultBrowserStatus => self.spawn_job(
                "default-browser",
                crate::ffi::default_browser::is_default,
                move |b, is_default: Option<bool>| {
                    let installed = process::is_installed();
                    b.reply(
                        tab,
                        Event::ok(req_id, serde_json::json!({ "installed": installed, "isDefault": is_default })),
                    );
                },
            ),
            Request::MakeDefaultBrowser => {
                let exe = std::env::current_exe().ok().filter(|_| process::is_installed());
                let Some(exe) = exe else {
                    self.reply(tab, Event::err(req_id, "Only an installed Krunk can be the default browser"));
                    return;
                };
                self.spawn_job(
                    "default-browser",
                    move || {
                        crate::ffi::default_browser::register(&exe)?;
                        crate::ffi::default_browser::open_settings()
                    },
                    move |b, r: Result<(), String>| match r {
                        Ok(()) => {
                            info!("default browser: Windows Settings opened");
                            b.reply(tab, Event::ok(req_id, serde_json::json!({ "opened": true })));
                        }
                        Err(e) => {
                            warning!("default browser: {e}");
                            b.reply(tab, Event::err(req_id, e));
                        }
                    },
                );
            }
            Request::Restart => self.restart(),
            Request::GetAbout => self.reply(tab, Event::ok(req_id, self.about())),
            Request::GetGpuInfo => self.reply(tab, Event::ok(req_id, self.gpu_info())),
            Request::ImportDetect => self.import_detect(tab, req_id),
            Request::ImportRun { profile, types, primary_password } => {
                self.import_run(tab, req_id, profile, types, primary_password)
            }
            Request::ImportCsv => self.import_csv(tab, req_id),
            Request::RepairEngine => self.repair_now(),
            Request::OpenGpuInternals => {
                if let Some(t) = tab {
                    self.navigate_tab(t, "chrome://gpu/");
                }
            }
            _ => {}
        }
    }

    /// krunk://gpu: what the host knows (the page probes WebGL and WebGPU itself).
    fn gpu_info(&self) -> serde_json::Value {
        let args = self.running_settings.browser_arguments();
        let (cef, chromium) = engine::versions();
        serde_json::json!({
            "hardwareAcceleration": self.settings.borrow().hardware_acceleration,
            "runningWithGpu": !args.split_whitespace().any(|a| a == "--disable-gpu"),
            "arguments": args,
            // What the engine really runs with: Krunk's switches merged into CEF's own (0.7.0).
            "commandLine": engine::command_line(),
            "integrity": self.integrity_json(),
            "gpuProcess": crate::ffi::procmem::browser_tree().iter().any(|p| p.kind == crate::ffi::procmem::Kind::Gpu),
            "cef": cef,
            "chromium": chromium,
            "widevine": widevine_cdm_version(&self.dirs.profile()),
            "protectedContent": self.running_settings.protected_content,
        })
    }

    pub(super) fn about(&self) -> serde_json::Value {
        let (cef, chromium) = engine::versions();
        serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "cef": cef,
            "chromium": chromium,
            "dataDir": self.dirs.root().display().to_string(),
            "arguments": self.running_settings.browser_arguments(),
            // What the engine runs with: CEF's switches and Krunk's, merged (0.7.1).
            "commandLine": engine::command_line(),
            "integrity": self.integrity_json(),
        })
    }

    /// Validates, persists and applies new settings. Invalid settings are
    /// never stored; the field errors go back to the page.
    fn save_settings(&self, tab: Option<TabId>, req_id: Option<u32>, mut new: Settings) {
        new.sync_protection();
        if let Err(errors) = new.validate() {
            self.reply(tab, Event::invalid(req_id, &errors));
            return;
        }
        // 0.14.0: the VPN is the engine's proxy while it runs; a manual one can't be added then.
        if self.vpn_engaged() && !new.proxy.trim().is_empty() {
            let e = lb_core::settings::FieldError {
                field: "proxy",
                message: "The VPN is on: disconnect it first (Privacy & security → VPN)".into(),
            };
            self.reply(tab, Event::invalid(req_id, &[e]));
            return;
        }
        if let Err(e) = self.store.borrow().save_settings(&new) {
            warning!("saving settings: {e}");
            self.reply(tab, Event::err(req_id, "Could not save settings"));
            return;
        }
        let old = std::mem::replace(&mut *self.settings.borrow_mut(), new.clone());
        self.apply_settings(&old, &new);
        let restart = new.restart_required(&self.running_settings);
        self.reply(tab, Event::ok(req_id, serde_json::json!({ "settings": new, "restartRequired": restart })));
    }

    /// A whole candidate from a page other than the settings page (the new tab page's
    /// customize panel): validated, saved and applied; the caller replies.
    pub(super) fn save_settings_from(
        &self,
        _tab: Option<TabId>,
        _req_id: Option<u32>,
        mut new: Settings,
    ) -> Result<(), Vec<String>> {
        new.sync_protection();
        if let Err(errors) = new.validate() {
            return Err(errors.into_iter().map(|e| format!("{}: {}", e.field, e.message)).collect());
        }
        if let Err(e) = self.store.borrow().save_settings(&new) {
            warning!("saving settings: {e}");
            return Err(vec!["Could not save settings".into()]);
        }
        let old = std::mem::replace(&mut *self.settings.borrow_mut(), new.clone());
        self.apply_settings(&old, &new);
        Ok(())
    }

    /// A change of the settings from elsewhere than the settings page (the bookmarks bar's
    /// menu): validated, saved and applied the same way.
    pub(super) fn change_settings(&self, f: impl FnOnce(&mut Settings)) {
        let _ = self.try_change_settings(f);
    }

    /// [`Browser::change_settings`] with the reason it was refused.
    pub(super) fn try_change_settings(&self, f: impl FnOnce(&mut Settings)) -> Result<(), String> {
        let mut new = self.settings.borrow().clone();
        f(&mut new);
        if let Err(errors) = new.validate() {
            return Err(errors.into_iter().map(|e| e.message).collect::<Vec<_>>().join("; "));
        }
        if let Err(e) = self.store.borrow().save_settings(&new) {
            warning!("saving settings: {e}");
            return Err("Could not save settings".into());
        }
        let old = std::mem::replace(&mut *self.settings.borrow_mut(), new.clone());
        self.apply_settings(&old, &new);
        Ok(())
    }

    /// Applies everything that works without a restart.
    pub(super) fn apply_settings(&self, old: &Settings, new: &Settings) {
        // 0.12.0: the agent, userscripts and macros.
        if old.ai.enabled != new.ai.enabled {
            self.agent_setting_changed();
        } else if old.ai != new.ai {
            self.agent_config_changed();
        }
        if old.userscripts != new.userscripts {
            self.userscripts_settings_changed();
        }
        if old.macros != new.macros {
            self.macros_settings_changed();
        }
        // 0.15.0: session logs on/off, retention.
        if old.session_log != new.session_log {
            self.sessionlog_settings_changed(old, new);
        }
        if old.vpn != new.vpn {
            self.vpn_settings_changed(
                old.vpn.allow_local_network != new.vpn.allow_local_network,
                old.vpn.scope != new.vpn.scope,
            );
        }
        // 0.15.0: finished downloads older than the new limit leave the list at once.
        if old.download_auto_clear_days != new.download_auto_clear_days {
            self.downloads_auto_clear();
        }
        if old.tab_sleep_minutes != new.tab_sleep_minutes
            || old.tab_discard_enabled != new.tab_discard_enabled
            || old.tab_discard_minutes != new.tab_discard_minutes
        {
            self.start_power_timer();
        }
        if old.theme != new.theme || old.appearance != new.appearance {
            // Appearance options are CSS variables: the shell, the popup and the pages take
            // them live through the same path as the theme.
            self.apply_theme();
        }
        if old.ntp != new.ntp {
            self.push_ntp_config();
        }
        if old.bookmarks_bar != new.bookmarks_bar {
            self.send_shell_config();
            self.send_nav();
            self.layout();
        } else if old.search_engine != new.search_engine || old.custom_search_url != new.custom_search_url {
            // The address bar's placeholder names the engine, in every window.
            self.send_shell_config();
        }
        if old.user_agent != new.user_agent || old.headers != new.headers {
            self.net.set_headers(&new.headers);
            // Every renderer, also those of tabs and frames already open (0.7.1).
            crate::ffi::cef::set_no_hints(!new.headers.send_client_hints);
            let ids: Vec<TabId> = self.views.borrow().keys().copied().collect();
            for id in ids {
                self.apply_user_agent_to(id);
                self.hints_attach(id);
            }
            info!(
                "request headers: user agent {}, accept-language {:?}, client hints {}, {} custom rule(s)",
                if new.user_agent.is_empty() { "default" } else { "custom" },
                new.headers.accept_language_list(),
                if !new.headers.send_client_hints {
                    "off"
                } else if new.headers.hints.is_some() {
                    "overridden"
                } else {
                    "engine"
                },
                new.headers.rules.len()
            );
        }
        // Privacy headers and ad blocking switch immediately (the navigator
        // properties for DNT/GPC follow on newly opened tabs).
        self.net.set_privacy_headers(new.send_dnt, new.send_gpc);
        // Tracking protection: cookie and referrer policies at once; the lists a level adds
        // go through the rebuild below.
        if old.protection != new.protection {
            self.net.set_protection(&new.protection);
            let e = new.protection.effective();
            info!(
                "tracking protection: {:?} (cookies {:?}, referrer {:?}, fingerprinting {}, cryptomining {}, bounce cleanup {})",
                e.level, e.third_party_cookies, e.referrer, e.fingerprinting, e.cryptomining, e.bounce_cleanup
            );
        }
        if old.dns != new.dns {
            self.apply_dns();
        }
        let lists_changed =
            old.effective_filter_lists() != new.effective_filter_lists() || old.custom_lists != new.custom_lists;
        if old.adblock_enabled != new.adblock_enabled || lists_changed {
            self.net.set_enabled(new.adblock_enabled);
            if new.adblock_enabled {
                if !self.net.has_engine() {
                    self.init_filters();
                } else if lists_changed {
                    // A list just added is downloaded first.
                    self.spawn_filter_job(Fetching::Added);
                }
            }
            self.set_scriptlet_engine(None);
            info!("ad blocking {}", if new.adblock_enabled { "on" } else { "off" });
        }
        // Everything else (search engine, startup, HTTPS-only, tracking
        // parameters, vault auto-lock, default zoom, download folder, clear
        // on exit) is read from the settings when it is used.
    }

    /// The user agent, `Accept-Language` and client hints of a tab (0.6.0 request headers):
    /// the engine starts with the settings' user agent; everything else, and any change made
    /// while running, is a DevTools override per tab (`core::headers::user_agent_override`).
    /// `done` runs once the engine acknowledged it (or at once when there is nothing to send).
    pub(super) fn apply_user_agent_to_then(&self, id: TabId, done: impl FnOnce(&Browser) + 'static) {
        let (ua_now, at_start, headers) = {
            let s = self.settings.borrow();
            (s.user_agent.clone(), self.running_settings.user_agent.clone(), s.headers.clone())
        };
        let Some(c) = self.tab_view(id) else { return done(self) };
        // The language alone still needs the override; it then carries the engine's own
        // hints (read from the shell page at startup) so nothing else changes.
        // "Send client hints" off (0.7.0): an override without metadata, so none are sent.
        let hints = if !headers.send_client_hints {
            None
        } else {
            headers
                .hints
                .clone()
                .or_else(|| (!headers.accept_language.is_empty()).then(|| self.engine_hints.borrow().clone()).flatten())
        };
        let params = if ua_now == at_start && !headers.overrides() {
            // Back to what the engine started with: tabs given an override meanwhile drop
            // it (new tabs never had one).
            if !self.ua_overridden.get() {
                return done(self);
            }
            serde_json::json!({ "userAgent": "" })
        } else {
            let ua = if ua_now.is_empty() { engine::default_user_agent() } else { ua_now };
            self.ua_overridden.set(true);
            lb_core::headers::user_agent_override(
                &ua,
                &headers.accept_language_list(),
                hints.as_ref(),
                &engine::versions().1,
            )
        };
        let weak = self.me.clone();
        let done = std::cell::RefCell::new(Some(done));
        c.set_user_agent_override(&params, move |ok| {
            if !ok {
                warning!("tab {id}: the user-agent override was not applied");
            }
            if let (Some(b), Some(done)) = (weak.upgrade(), done.borrow_mut().take()) {
                done(&b);
            }
        });
    }

    pub(super) fn apply_user_agent_to(&self, id: TabId) {
        self.apply_user_agent_to_then(id, |_| {});
    }

    /// Reads the engine's own client hints from the shell page (once, when it has loaded).
    pub(super) fn probe_engine_hints(&self) {
        if self.engine_hints.borrow().is_some() {
            return;
        }
        let Some(shell) = self.shell() else { return };
        let weak = self.me.clone();
        shell.eval_json(
            "navigator.userAgentData.getHighEntropyValues(['architecture','bitness','model','platformVersion','uaFullVersion','wow64']).then(h => Object.assign(h, { brands: navigator.userAgentData.brands, mobile: navigator.userAgentData.mobile, platform: navigator.userAgentData.platform }))",
            move |v| {
                let Some(b) = weak.upgrade() else { return };
                match v.as_ref().and_then(lb_core::headers::ClientHints::from_user_agent_data) {
                    Some(h) => {
                        info!("engine client hints: {} on {} {}", h.sec_ch_ua(), h.platform, h.platform_version);
                        *b.engine_hints.borrow_mut() = Some(h);
                        // Tabs that only change the language were waiting for these.
                        if !b.settings.borrow().headers.accept_language.is_empty() {
                            let ids: Vec<TabId> = b.views.borrow().keys().copied().collect();
                            for id in ids {
                                b.apply_user_agent_to(id);
                            }
                        }
                    }
                    None => warning!("engine client hints: not readable from the shell page"),
                }
            },
        );
    }

    /// Does a new tab need the override before its first request?
    pub(super) fn headers_overridden(&self) -> bool {
        let s = self.settings.borrow();
        s.user_agent != self.running_settings.user_agent || s.headers.overrides()
    }

    /// Default zoom for a page that has no zoom level of its own yet (the
    /// engine remembers zoom per site, like Chrome).
    pub(super) fn apply_default_zoom(&self, c: &engine::View) {
        let default = self.settings.borrow().default_zoom;
        if (default - 1.0).abs() > 1e-3 && (c.zoom() - 1.0).abs() < 1e-3 {
            c.set_zoom(default);
        }
    }

    /// The theme for the shell, popup, built-in pages and (through the
    /// profiles' preferred colour scheme) web pages.
    pub(super) fn apply_theme(&self) {
        // 0.25.0: the pre-loaded new tab page is made again in the new look.
        self.spare_tab_refresh();
        let theme = self.settings.borrow().theme;
        engine::set_color_scheme(self.color_scheme());
        let dark = self.color_scheme();
        let views: Vec<engine::View> = self.views.borrow().values().map(|v| v.view.clone()).collect();
        for v in views {
            v.set_color_scheme(dark);
        }
        self.send_shell_config();
        self.apply_frame_theme();
        let e = {
            let s = self.settings.borrow();
            Event::Theme {
                theme,
                accent: s.appearance.accent.clone(),
                ui_font_px: s.appearance.ui_font_px,
                colors: lb_core::color::ThemeColors::from_appearance(&s.appearance),
            }
            .to_json()
        };
        // Both popup windows: the second one shows the bookmark menu over a folder dropdown
        // (0.7.2, R8: it kept the theme it was made with).
        for slot in [PopupSlot::Main, PopupSlot::Menu, PopupSlot::Preview] {
            let popup = self.popup_slot(slot).borrow().as_ref().and_then(|p| p.view.clone());
            if let Some(c) = popup {
                c.post_json(&e);
            }
        }
        // 0.15.0: the AI panels switch live too (they only got the theme at load).
        self.agent_panels_theme(&e);
        let internal: Vec<TabId> =
            self.ws.borrow().tabs().filter(|(_, t)| Page::from_url(&t.url).is_some()).map(|(_, t)| t.id).collect();
        for id in internal {
            if let Some(c) = self.tab_view(id) {
                c.post_json(&e);
            }
        }
    }

    /// Native title bars (main window and popup) follow the theme.
    pub(super) fn apply_frame_theme(&self) {
        let dark = match self.settings.borrow().theme {
            Theme::Dark => true,
            Theme::Light => false,
            Theme::System => crate::ffi::window::system_prefers_dark(),
        };
        for f in self.all_frames() {
            f.window.set_dark_frame(dark);
            f.window.repaint_background();
        }
    }

    /// Theme for one page that just loaded.
    pub(super) fn send_theme_to(&self, c: &engine::View) {
        let s = self.settings.borrow();
        c.post_json(
            &Event::Theme {
                theme: s.theme,
                accent: s.appearance.accent.clone(),
                ui_font_px: s.appearance.ui_font_px,
                colors: lb_core::color::ThemeColors::from_appearance(&s.appearance),
            }
            .to_json(),
        );
    }

    // ------------------------------------------------------------ exit / restart

    /// "Restart now": close (running clear-on-exit), then start a new
    /// instance that waits for this one to exit.
    fn restart(&self) {
        info!("restart requested");
        self.restart_after_exit.set(true);
        self.begin_exit();
    }

    /// Closes the browser. With clear-on-exit, the window is hidden and the
    /// profile data is cleared first; the window is destroyed when that
    /// completes (or after a timeout).
    pub(super) fn begin_exit(&self) {
        self.stop_parts();
        let clear = self.settings.borrow().clear_on_exit.clone();
        if !clear.any() || self.exit_cleared.get() {
            self.destroy_frames();
            return;
        }
        if self.exit_clearing.replace(true) {
            return;
        }
        info!("clearing data on exit");
        self.hide_popup();
        for f in self.all_frames() {
            f.window.hide();
        }
        let Some(w) = self.app() else { return };
        // Sites whose storage to clear, before their history goes.
        let origins = self.origins_to_clear(clear.cookies, None);
        if clear.history {
            let _ = self.store.borrow_mut().clear_history();
        }
        if clear.downloads {
            self.downloads.borrow_mut().clear_finished();
        }
        let kinds = ClearKinds { cookies: clear.cookies, cache: clear.cache, site_data: clear.cookies };
        w.set_timer(EXIT_TIMER, EXIT_TIMEOUT_MS);
        let weak = self.me.clone();
        self.clear_engine_data(kinds, None, origins, move |ok| {
            if let Some(b) = weak.upgrade() {
                if !ok {
                    warning!("clearing data on exit failed");
                }
                b.finish_exit();
            }
        });
    }

    pub(super) fn finish_exit(&self) {
        if !self.exit_cleared.get() {
            info!("data cleared, closing");
        }
        self.exit_cleared.set(true);
        if let Some(w) = self.app() {
            w.kill_timer(EXIT_TIMER);
        }
        self.destroy_frames();
    }

    /// Called from `shutdown`: start the new instance for a restart.
    pub(super) fn relaunch_if_requested(&self) {
        if self.restart_after_exit.get() {
            let pid = std::process::id();
            if let Err(e) = process::relaunch(&[format!("--wait-for-pid={pid}")]) {
                warning!("restart failed: {e}");
            }
        }
    }
}

/// The Widevine CDM the engine's component updater put into the profile (0.6.0): the highest
/// `profile\WidevineCdm\<version>\_platform_specific\win_x64\widevinecdm.dll`, if any.
pub(super) fn widevine_cdm_version(profile: &std::path::Path) -> Option<String> {
    let dir = std::fs::read_dir(profile.join("WidevineCdm")).ok()?;
    let mut versions: Vec<(Vec<u32>, String)> = dir
        .flatten()
        .filter(|e| e.path().join("_platform_specific").join("win_x64").join("widevinecdm.dll").is_file())
        .map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let parts: Vec<u32> = name.split('.').map(|p| p.parse().unwrap_or(0)).collect();
            (parts, name)
        })
        .collect();
    versions.sort();
    versions.pop().map(|(_, name)| name)
}
