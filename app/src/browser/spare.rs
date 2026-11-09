//! The spare frame (0.11.1): a hidden window whose shell has already loaded. The next window
//! Krunk opens (a tab torn off by a drag, "Move to new window", a new window) takes it and is
//! complete at once; a new shell takes a moment to load, and a torn-off window showed an
//! empty tab row meanwhile. One at a time, prepared a few seconds after it is used. Its shell
//! gets what all shells share (config, bookmarks bar, downloads, update and extension
//! buttons), so a window that takes it only needs its own tabs.

use super::engine::View;
use super::{Browser, Frame, FrameEvents};
use crate::ffi::window::WindowEvents;
use crate::ffi::window::{ScreenRect, Window};
use crate::log::{info, warning};
use lb_core::layout;
use lb_core::protocol::WindowAction;
use lb_core::windows::{Bounds, WindowId};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

/// Timer (on the app window): prepares the spare when Krunk has been idle a moment.
pub(super) const SPARE_TIMER: usize = 27;
const SPARE_DELAY_MS: u32 = 3000;

pub(super) struct SpareFrame {
    window: Window,
    shell: View,
    /// The window id its shell's callbacks use: none while it is the spare.
    id: Rc<Cell<Option<WindowId>>>,
    /// The shell has loaded (said "ready").
    ready: bool,
}

impl Browser {
    /// Prepares a spare frame a few seconds from now, if there is none.
    pub(super) fn schedule_spare(&self) {
        if self.spare.borrow().is_none() {
            if let Some(app) = self.app() {
                app.set_timer(SPARE_TIMER, SPARE_DELAY_MS);
            }
        }
    }

    /// `SPARE_TIMER`.
    pub(super) fn spare_timer(&self) {
        if let Some(app) = self.app() {
            app.kill_timer(SPARE_TIMER);
        }
        if self.spare.borrow().is_some() || self.closing.get() || self.session_final.get() {
            return;
        }
        // Not while a tab is dragged (creating a browser costs the drag frames).
        if self.drag.get().is_some() || self.bar_drag.get().is_some() {
            return self.schedule_spare();
        }
        // The size a new window gets: the current one's restored size.
        let Some(current) = self.window() else { return };
        let (normal, _) = current.placement();
        let window = match Window::create_frame("Krunk", Some(normal), false) {
            Ok(w) => w,
            Err(e) => return warning!("spare frame: {e}"),
        };
        window.set_events(Rc::new(SpareEvents { b: self.me.clone() }));
        let id = Rc::new(Cell::new(None));
        let shell = match self.new_shell(window, id.clone()) {
            Ok(s) => s,
            Err(e) => {
                window.destroy();
                return warning!("spare frame: {e}");
            }
        };
        let (width, height) = window.client_size();
        let (area, _) = layout::split(width, height, window.scale(), false);
        shell.set_bounds(area.x, area.y, area.width, area.height);
        shell.set_visible(true);
        *self.spare.borrow_mut() = Some(SpareFrame { window, shell, id, ready: false });
    }

    /// A message from the spare's shell: only "ready" matters (it then gets what all shells
    /// share).
    pub(super) fn spare_message(&self, json: &str) {
        if !json.contains(r#""cmd":"ready""#) {
            return;
        }
        match self.spare.borrow_mut().as_mut() {
            Some(sp) => sp.ready = true,
            None => return,
        }
        self.spare_only.set(true);
        self.send_shell_config();
        self.send_shell_shared();
        self.spare_only.set(false);
    }

    /// Posts a shell event to the spare's shell (once it has loaded).
    pub(super) fn post_spare(&self, json: &str) {
        if let Ok(spare) = self.spare.try_borrow() {
            if let Some(sp) = spare.as_ref().filter(|sp| sp.ready) {
                sp.shell.post_json(json);
            }
        }
    }

    /// The spare's shell renderer ended: it goes, and a new one is prepared later.
    pub(super) fn spare_crashed(&self) {
        warning!("spare frame: its shell's renderer ended");
        self.drop_spare();
        self.schedule_spare();
    }

    /// Closes the spare (shutdown, or its shell is gone).
    pub(super) fn drop_spare(&self) {
        let spare = self.spare.borrow_mut().take();
        if let Some(sp) = spare {
            sp.shell.close();
            if sp.window.exists() {
                sp.window.destroy();
            }
        }
    }

    /// New window `win` takes the spare frame, if there is one: placed at `bounds`, shown, and
    /// given its tabs. False if there is none.
    pub(super) fn adopt_spare(&self, win: WindowId, bounds: Option<Bounds>, maximized: bool) -> bool {
        let spare = self.spare.borrow_mut().take();
        let Some(sp) = spare else { return false };
        if !sp.window.exists() {
            sp.shell.close();
            return false;
        }
        let window = sp.window;
        sp.id.set(Some(win));
        sp.shell.allow_activation();
        let frame = Rc::new(Frame {
            id: win,
            window,
            shell: RefCell::new(Some(sp.shell)),
            hit_map: RefCell::new(None),
            active: Cell::new(true),
            bar: Cell::new(false),
            last_zoom: Cell::new(1.0),
        });
        window.set_events(Rc::new(FrameEvents { b: self.me.clone(), win }));
        self.frames.borrow_mut().push(frame);
        if let Some(b) = bounds {
            window.place_hidden(ScreenRect { x: b.x, y: b.y, width: b.width, height: b.height });
        }
        self.apply_frame_theme();
        self.layout();
        if sp.ready {
            // What the shell would have been sent when it said "ready" as this window's.
            self.shell_loaded(win);
            self.defer(move |b| {
                if b.drag.get().is_none() && b.frame(win).is_some() {
                    b.make_current(win);
                    b.focus_new_window();
                }
            });
        }
        if self.tearing.get() {
            window.set_transitions(false);
        }
        window.show(maximized);
        info!("window {win}: opened with the spare frame (shell ready: {})", sp.ready);
        self.schedule_spare();
        true
    }
}

/// The spare's window while it is the spare (a window that takes it gets `FrameEvents`).
struct SpareEvents {
    b: Weak<Browser>,
}

impl WindowEvents for SpareEvents {
    /// The hidden spare must not keep the activation (the keyboard would type into it): the
    /// current window gets it back.
    fn activated(&self, active: bool) {
        if active {
            if let Some(b) = self.b.upgrade() {
                warning!("spare frame: it was activated; the current window gets the activation back");
                b.defer(|b| {
                    if let Some(w) = b.window() {
                        w.bring_to_front();
                    }
                });
            }
        }
    }

    /// A close request for "the Krunk window" (the installer's update finds a window by its
    /// class, and may find this one): it goes to the current window, as it would have.
    fn close_requested(&self) -> bool {
        if let Some(w) = self.b.upgrade().and_then(|b| b.window()) {
            w.control(WindowAction::Close);
        }
        false
    }
}
