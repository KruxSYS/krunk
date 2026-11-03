//! Settings: get/save/reset with validation, live application of every
//! setting that does not need a restart, restart, and clear-on-exit.

use super::Browser;
use crate::ffi::dialogs;
use crate::ffi::process;
use crate::ffi::webview::{ClearKinds, Controller};
use crate::log::{info, warning};
use lb_core::pages::Page;
use lb_core::protocol::{Event, Request};
use lb_core::settings::{Settings, Theme, TrackingPrevention};
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
            Request::SaveSettings { settings } => self.save_settings(tab, req_id, *settings),
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
        serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "runtime": self.runtime_version,
            "loader": crate::ffi::loader::LOADER_VERSION,
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

    /// Applies everything that works without a restart.
    fn apply_settings(&self, old: &Settings, new: &Settings) {
        if old.theme != new.theme {
            self.apply_theme();
        }
        if old.show_bookmarks_bar != new.show_bookmarks_bar || old.show_home_button != new.show_home_button {
            self.send_shell_config();
            self.layout();
        }
        if old.user_agent != new.user_agent {
            for c in self.content_controllers() {
                self.apply_user_agent(&c);
            }
        }
        if old.tracking_prevention != new.tracking_prevention {
            if let Some(s) = self.shell() {
                s.set_tracking_prevention(tracking_level(new.tracking_prevention));
            }
            for c in self.content_controllers() {
                c.set_tracking_prevention(tracking_level(new.tracking_prevention));
            }
        }
        if old.download_dir != new.download_dir {
            if let Some(s) = self.shell() {
                self.apply_download_folder(&s);
            }
            for c in self.content_controllers() {
                self.apply_download_folder(&c);
            }
        }
        // Privacy headers and ad blocking switch immediately (the navigator
        // properties for DNT/GPC follow on newly opened tabs).
        self.filters.dnt.set(new.send_dnt);
        self.filters.gpc.set(new.send_gpc);
        if old.adblock_enabled != new.adblock_enabled || old.filter_lists != new.filter_lists {
            self.filters.enabled.set(new.adblock_enabled);
            if new.adblock_enabled {
                if self.filters.engine.borrow().is_none() {
                    self.init_filters();
                } else if old.filter_lists != new.filter_lists {
                    self.spawn_filter_job(false);
                }
            }
            info!("ad blocking {}", if new.adblock_enabled { "on" } else { "off" });
        }
        // Everything else (search engine, startup, HTTPS-only, tracking
        // parameters, vault auto-lock, zoom for new tabs, clear on exit) is
        // read from the settings when it is used.
    }

    fn content_controllers(&self) -> Vec<Controller> {
        self.views.borrow().values().map(|v| v.controller.clone()).collect()
    }

    /// Custom user agent, or the runtime's default when the field is empty.
    pub(super) fn apply_user_agent(&self, c: &Controller) {
        if self.default_ua.borrow().is_none() {
            let ua = c.user_agent();
            if !ua.is_empty() {
                *self.default_ua.borrow_mut() = Some(ua);
            }
        }
        let custom = self.settings.borrow().user_agent.clone();
        if !custom.is_empty() {
            c.set_user_agent(&custom);
        } else if let Some(d) = self.default_ua.borrow().as_ref() {
            c.set_user_agent(d);
        }
    }

    /// The theme for the shell, popup, built-in pages and (through the
    /// profile's preferred colour scheme) web pages.
    pub(super) fn apply_theme(&self) {
        let theme = self.settings.borrow().theme;
        let dark = match theme {
            Theme::System => None,
            Theme::Light => Some(false),
            Theme::Dark => Some(true),
        };
        if let Some(s) = self.shell() {
            s.set_color_scheme(dark);
        }
        for c in self.content_controllers() {
            if c.is_private() {
                c.set_color_scheme(dark);
            }
        }
        self.send_shell_config();
        self.apply_frame_theme();
        let e = Event::Theme { theme }.to_json();
        if let Some(c) = self.popup.borrow().as_ref().and_then(|p| p.controller.clone()) {
            c.post_json(&e);
        }
        let internal: Vec<TabId> =
            self.model.borrow().tabs().iter().filter(|t| Page::from_url(&t.url).is_some()).map(|t| t.id).collect();
        for id in internal {
            if let Some(c) = self.controller(id) {
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
        if let Some(w) = self.window() {
            w.set_dark_frame(dark);
        }
    }

    /// Theme for one page that just loaded.
    pub(super) fn send_theme_to(&self, c: &Controller) {
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
        let Some(w) = self.window() else { return };
        let clear = self.settings.borrow().clear_on_exit.clone();
        if !clear.any() || self.exit_cleared.get() {
            w.destroy();
            return;
        }
        if self.exit_clearing.replace(true) {
            return;
        }
        info!("clearing data on exit");
        self.hide_popup();
        w.hide();
        if clear.history {
            let _ = self.store.borrow_mut().clear_history();
        }
        if clear.downloads {
            self.downloads.borrow_mut().clear_finished();
        }
        let kinds = ClearKinds { cookies: clear.cookies, cache: clear.cache, site_data: clear.cookies };
        w.set_timer(EXIT_TIMER, EXIT_TIMEOUT_MS);
        let weak = self.me.clone();
        match self.shell() {
            Some(shell) => shell.clear_browsing_data(kinds, None, move |ok| {
                if let Some(b) = weak.upgrade() {
                    if !ok {
                        warning!("clearing data on exit failed");
                    }
                    b.finish_exit();
                }
            }),
            None => self.finish_exit(),
        }
    }

    pub(super) fn finish_exit(&self) {
        if !self.exit_cleared.get() {
            info!("data cleared, closing");
        }
        self.exit_cleared.set(true);
        if let Some(w) = self.window() {
            w.kill_timer(EXIT_TIMER);
            w.destroy();
        }
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

pub(super) fn tracking_level(t: TrackingPrevention) -> u8 {
    match t {
        TrackingPrevention::None => 0,
        TrackingPrevention::Basic => 1,
        TrackingPrevention::Balanced => 2,
        TrackingPrevention::Strict => 3,
    }
}
