//! Settings: get/save/reset with validation, live application of every
//! setting that does not need a restart, restart, and clear-on-exit.

use super::blocking::Fetching;
use super::Browser;
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
                }
                self.save_settings(tab, req_id, s)
            }
            Request::ResetSettings => self.save_settings(tab, req_id, Settings::default()),
            Request::PickDownloadFolder => {
                let Some(w) = self.window() else { return };
                let current = self.settings.borrow().download_dir.clone();
                let start = (!current.is_empty()).then(|| Path::new(&current).to_path_buf());
                let picked = dialogs::pick_folder(w.hwnd(), "Save downloads to", start.as_deref());
                self.reply(tab, Event::ok(req_id, picked.map(|p| p.display().to_string())));
            }
            Request::Restart => self.restart(),
            Request::GetAbout => self.reply(tab, Event::ok(req_id, self.about())),
            _ => {}
        }
    }

    pub(super) fn about(&self) -> serde_json::Value {
        let (cef, chromium) = engine::versions();
        serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "cef": cef,
            "chromium": chromium,
            "dataDir": self.dirs.root().display().to_string(),
            "arguments": self.running_settings.browser_arguments(),
        })
    }

    /// Validates, persists and applies new settings. Invalid settings are
    /// never stored; the field errors go back to the page.
    fn save_settings(&self, tab: Option<TabId>, req_id: Option<u32>, new: Settings) {
        if let Err(errors) = new.validate() {
            self.reply(tab, Event::invalid(req_id, &errors));
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
    fn apply_settings(&self, old: &Settings, new: &Settings) {
        if old.tab_sleep_minutes != new.tab_sleep_minutes
            || old.tab_discard_enabled != new.tab_discard_enabled
            || old.tab_discard_minutes != new.tab_discard_minutes
        {
            self.start_power_timer();
        }
        if old.theme != new.theme {
            self.apply_theme();
        }
        if old.bookmarks_bar != new.bookmarks_bar || old.show_home_button != new.show_home_button {
            self.send_shell_config();
            self.send_nav();
            self.layout();
        }
        if old.user_agent != new.user_agent {
            let ids: Vec<TabId> = self.views.borrow().keys().copied().collect();
            for id in ids {
                self.apply_user_agent_to(id);
            }
        }
        // Privacy headers and ad blocking switch immediately (the navigator
        // properties for DNT/GPC follow on newly opened tabs).
        self.net.set_privacy_headers(new.send_dnt, new.send_gpc);
        let lists_changed = old.filter_lists != new.filter_lists || old.custom_lists != new.custom_lists;
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

    /// The user agent: the engine starts with the one from the settings; a
    /// change made while running is applied per tab (DevTools protocol)
    /// until the next start.
    pub(super) fn apply_user_agent_to(&self, id: TabId) {
        let (now, at_start) = (self.settings.borrow().user_agent.clone(), self.running_settings.user_agent.clone());
        if now == at_start {
            return;
        }
        let ua = if now.is_empty() { engine::default_user_agent() } else { now };
        if let Some(c) = self.tab_view(id) {
            c.set_user_agent(Some(&ua));
        }
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
        let theme = self.settings.borrow().theme;
        engine::set_color_scheme(self.color_scheme());
        let dark = self.color_scheme();
        let views: Vec<engine::View> = self.views.borrow().values().map(|v| v.view.clone()).collect();
        for v in views {
            v.set_color_scheme(dark);
        }
        self.send_shell_config();
        self.apply_frame_theme();
        let e = Event::Theme { theme }.to_json();
        let popup = self.popup.borrow().as_ref().and_then(|p| p.view.clone());
        if let Some(c) = popup {
            c.post_json(&e);
        }
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
        }
    }

    /// Theme for one page that just loaded.
    pub(super) fn send_theme_to(&self, c: &engine::View) {
        c.post_json(&Event::Theme { theme: self.settings.borrow().theme }.to_json());
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
