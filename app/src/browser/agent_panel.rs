//! The AI agent's side panel (0.12.0): one view per window, docked right of the tab area like
//! DevTools docked right, with a splitter the frame paints and drags.
//!
//! The panel is a `Role::Popup` view (no context menu, no dialogs, links open as tabs) on its
//! own channel: `https://shell.krunk.invalid/agent.html`, which no tab can reach. It is created
//! the first time it opens and then hidden and shown. While AI is off there is no panel and no
//! button. What the panel says is the agent's business (`agent/`); this file only hosts it.

use super::{Browser, Frame, UI_BACKGROUND, UI_BACKGROUND_DARK};
use crate::ffi::cef::{Role, Scripts, View, ViewOptions};
use crate::log::{info, warning};
use lb_core::assets;
use lb_core::layout::{self, PanelSplit};
use lb_core::protocol::{Channel, Event};
use lb_core::windows::WindowId;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

pub(super) const AGENT_URL: &str = "https://shell.krunk.invalid/agent.html";
/// Extra pixels around the splitter that grab it.
const SPLITTER_SLACK: i32 = 2;

struct Panel {
    view: View,
    open: bool,
    ready: bool,
}

#[derive(Default)]
pub(super) struct PanelHost {
    panels: RefCell<HashMap<WindowId, Panel>>,
    /// The window whose panel splitter is being dragged.
    dragging: Cell<Option<WindowId>>,
}

impl Browser {
    pub(super) fn ai_enabled(&self) -> bool {
        self.settings.borrow().ai.enabled
    }

    /// Is the agent panel of window `win` open?
    pub(super) fn agent_panel_open(&self, win: WindowId) -> bool {
        self.panel.panels.borrow().get(&win).is_some_and(|p| p.open)
    }

    /// Posts an event to window `win`'s panel (dropped while it has not loaded).
    pub(super) fn post_panel(&self, win: WindowId, event: &Event) {
        let view = self.panel.panels.borrow().get(&win).filter(|p| p.ready).map(|p| p.view.clone());
        if let Some(v) = view {
            v.post_json(&event.to_json());
        }
    }

    /// The window whose panel sent the request being dispatched: the current window (its
    /// message made it current).
    pub(super) fn agent_request_window(&self) -> Option<WindowId> {
        self.cur_id()
    }

    /// The toolbar button and Ctrl+Shift+Space: show or hide the current window's panel.
    /// The panel's view in window `win`, if it has one (open or closed).
    pub(super) fn agent_panel_view(&self, win: WindowId) -> Option<View> {
        self.panel.panels.borrow().get(&win).map(|p| p.view.clone())
    }

    pub(super) fn toggle_agent(&self) {
        if !self.ai_enabled() {
            return;
        }
        let Some(win) = self.cur_id() else { return };
        let open = !self.agent_panel_open(win);
        self.set_agent_panel(win, open);
    }

    /// Opens or closes window `win`'s panel.
    pub(super) fn set_agent_panel(&self, win: WindowId, open: bool) {
        if open && !self.ai_enabled() {
            return;
        }
        let Some(f) = self.frame(win) else { return };
        if open && !self.panel.panels.borrow().contains_key(&win) {
            match self.create_agent_panel(&f) {
                Ok(view) => {
                    self.panel.panels.borrow_mut().insert(win, Panel { view, open: true, ready: false });
                }
                Err(e) => {
                    warning!("window {win}: agent panel not created: {e}");
                    return;
                }
            }
        }
        let view = {
            let mut panels = self.panel.panels.borrow_mut();
            let Some(p) = panels.get_mut(&win) else { return };
            p.open = open;
            p.view.clone()
        };
        info!("window {win}: agent panel {}", if open { "open" } else { "closed" });
        view.set_visible(open);
        self.layout_frame(&f);
        f.window.invalidate();
        if open {
            view.focus();
        } else {
            self.focus_active();
        }
        self.send_agent_button_to(win);
    }

    fn create_agent_panel(&self, f: &Frame) -> Result<View, String> {
        let win = f.id;
        let view = View::new(ViewOptions {
            role: Role::Popup,
            private: false,
            scripts: Scripts::default(),
            background: if self.frame_dark() { UI_BACKGROUND_DARK } else { UI_BACKGROUND },
            policy: None,
        });
        view.on_before_browse(|nav| assets::allow_navigation(Channel::Agent, "", &nav.url, true, nav.redirected));
        let weak = self.me.clone();
        view.on_message(move |source, json| {
            if let Some(b) = weak.upgrade() {
                b.on_ui_message(Channel::Agent, Some(win), source, json);
            }
        });
        let weak = self.me.clone();
        view.on_open_tab(move |url, _| {
            // Links in the agent's answers: web addresses only, as tabs of this window.
            if let Some(b) = weak.upgrade() {
                if url.starts_with("https://") || url.starts_with("http://") {
                    b.make_current(win);
                    b.open_url(&url, true, false);
                }
            }
        });
        let weak = self.me.clone();
        view.on_key(move |key| {
            weak.upgrade().is_some_and(|b| {
                b.make_current(win);
                b.on_key(key)
            })
        });
        let weak = self.me.clone();
        view.on_crashed(move |reason| {
            let Some(b) = weak.upgrade() else { return };
            warning!("window {win}: the agent panel's renderer ended ({reason}); reloading it");
            let view = b.panel.panels.borrow_mut().get_mut(&win).map(|p| {
                p.ready = false;
                p.view.clone()
            });
            if let Some(v) = view {
                v.navigate(AGENT_URL);
            }
        });
        self.hook_focus(&view, None);
        view.start(f.window.hwnd(), AGENT_URL).map_err(|e| e.to_string())?;
        Ok(view)
    }

    /// The panel document has loaded (its `ready`): its theme, then the agent's state.
    pub(super) fn agent_panel_ready(&self) {
        let Some(win) = self.cur_id() else { return };
        let view = {
            let mut panels = self.panel.panels.borrow_mut();
            let Some(p) = panels.get_mut(&win) else { return };
            p.ready = true;
            p.view.clone()
        };
        self.send_theme_to(&view);
        self.agent_panel_loaded(win);
    }

    /// The theme changed (0.15.0): every loaded panel takes the new `Event::Theme` (`json`) at
    /// once, and its engine background (what shows before a paint, e.g. while it is resized or
    /// shown again) follows the theme instead of the colour it was created with.
    pub(super) fn agent_panels_theme(&self, json: &str) {
        let views: Vec<(View, bool)> = self.panel.panels.borrow().values().map(|p| (p.view.clone(), p.ready)).collect();
        let argb = if self.frame_dark() { UI_BACKGROUND_DARK } else { UI_BACKGROUND };
        let color = serde_json::json!({ "color": {
            "r": (argb >> 16) & 0xff, "g": (argb >> 8) & 0xff, "b": argb & 0xff, "a": 1
        } });
        for (view, ready) in views {
            view.devtools_method("Emulation.setDefaultBackgroundColorOverride", &color);
            if ready {
                view.post_json(json);
            }
        }
    }

    /// Tells window `win`'s shell whether to show the agent button and whether it is pressed.
    pub(super) fn send_agent_button_to(&self, win: WindowId) {
        let visible = self.ai_enabled();
        let open = visible && self.agent_panel_open(win);
        self.post_shell_to(win, &Event::AgentButton { visible, open });
    }

    /// AI was turned on or off in Settings: off closes every panel (the agent stops) and hides
    /// the buttons.
    pub(super) fn agent_setting_changed(&self) {
        if !self.ai_enabled() {
            let wins: Vec<WindowId> = self.panel.panels.borrow().keys().copied().collect();
            for win in wins {
                self.agent_window_closed(win);
                if let Some(p) = self.panel.panels.borrow_mut().remove(&win) {
                    p.view.close();
                }
            }
            self.layout();
        }
        for f in self.all_frames() {
            self.send_agent_button_to(f.id);
        }
        self.agent_config_changed();
    }

    /// Window `win` is closing: its panel and its conversation go.
    pub(super) fn agent_panel_window_closed(&self, win: WindowId) {
        self.agent_window_closed(win);
        if let Some(p) = self.panel.panels.borrow_mut().remove(&win) {
            p.view.close();
        }
    }

    /// The content area split for frame `f`'s open panel (`content` is the area below the
    /// shell). None while it is closed or in full screen.
    pub(super) fn panel_split_of(&self, f: &Frame, content: layout::Rect) -> Option<PanelSplit> {
        if !self.agent_panel_open(f.id) || self.is_fullscreen_frame(f.id) || self.is_page_fullscreen_frame(f.id) {
            return None;
        }
        let width = self.settings.borrow().ai.panel_width;
        Some(layout::panel_split(content, width, f.window.scale()))
    }

    /// Places frame `f`'s panel (or hides it in full screen).
    pub(super) fn layout_panel(&self, f: &Frame, split: Option<PanelSplit>) {
        let Some(view) = self.panel.panels.borrow().get(&f.id).filter(|p| p.open).map(|p| p.view.clone()) else {
            return;
        };
        match split {
            Some(s) => {
                view.set_bounds(s.panel.x, s.panel.y, s.panel.width, s.panel.height);
                view.set_visible(!f.window.is_minimized());
            }
            None => view.set_visible(false),
        }
    }

    /// Is the client point on the panel's splitter?
    pub(super) fn panel_splitter_at(&self, win: WindowId, x: i32, y: i32) -> bool {
        let Some(f) = self.frame(win) else { return false };
        let Some(s) = self.panel_split_full(&f) else { return false };
        let r = s.splitter;
        x >= r.x - SPLITTER_SLACK
            && x < r.x + r.width + SPLITTER_SLACK
            && y >= r.y - SPLITTER_SLACK
            && y < r.y + r.height + SPLITTER_SLACK
    }

    fn panel_split_full(&self, f: &Frame) -> Option<PanelSplit> {
        let (_, content) = self.frame_areas_full(f);
        self.panel_split_of(f, content)
    }

    /// A splitter drag in window `win`: the panel's when it started on the panel's splitter,
    /// else DevTools'.
    pub(super) fn any_splitter_drag(&self, win: WindowId, x: i32, y: i32, done: bool) {
        let panel = match self.panel.dragging.get() {
            Some(w) => w == win,
            None => {
                let on = self.panel_splitter_at(win, x, y);
                if on {
                    self.panel.dragging.set(Some(win));
                }
                on
            }
        };
        if !panel {
            self.splitter_drag(win, x, y, done);
            return;
        }
        let Some(f) = self.frame(win) else { return };
        let (_, content) = self.frame_areas_full(&f);
        let width = layout::panel_width_at(content, x, f.window.scale());
        self.settings.borrow_mut().ai.panel_width = width;
        self.layout();
        if done {
            self.panel.dragging.set(None);
            info!("agent panel width {width:.0} DIP");
            let s = self.settings.borrow().clone();
            if let Err(e) = self.store.borrow().save_settings(&s) {
                warning!("saving the panel width: {e}");
            }
        }
    }
}
