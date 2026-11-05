//! Moving tabs between windows: dragging a tab out of its strip, onto another window's strip,
//! "Move to new window" and "New window" (docs: README "Windows and views").
//!
//! The shell reorders tabs within its own strip. When the pointer leaves the strip, it hands
//! the drag over (`tabDragOut`), because a shell view can't see other windows. From then on the
//! host follows the mouse: a tab with siblings moves at once into a new window that follows the
//! cursor (the window *is* the drag image); a window's only tab drags its whole window. A
//! timer polls the cursor and the left button (no mouse capture to fight over), finds the
//! topmost other Krunk window under the cursor, and while that point is over its tab strip
//! tells its shell where the tab would land. Releasing there moves the tab in (a window
//! emptied by that closes); releasing elsewhere leaves the window where it is; Esc puts the tab
//! back. The tab's browser moves between frames without reloading (`View::reparent`).

use super::Browser;
use crate::ffi::cursor;
use crate::ffi::window::{self, ScreenRect};
use crate::log::info;
use lb_core::frame;
use lb_core::protocol::Event;
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use lb_core::windows::{Bounds, WindowId};

/// Timer (on the app window) that follows the mouse during a drag.
pub(super) const DRAG_TIMER: usize = 12;
const DRAG_TICK_MS: u32 = 15;
/// A tab-strip band this much taller than the strip still counts as "over the strip".
const STRIP_SLACK_DIP: f64 = 8.0;
/// Where the first tab of a new window starts (CSS px), if its shell hasn't reported yet.
const FIRST_TAB_X_DIP: f64 = 8.0;
/// New windows ("New window", "Move to new window") open this far down-right of the current one.
const CASCADE_PX: i32 = 32;

#[derive(Debug, Clone, Copy)]
pub(super) struct TabDrag {
    tab: TabId,
    /// Where the tab came from (Esc puts it back).
    source: WindowId,
    source_index: usize,
    /// The window following the mouse: a new one for the tab, or the source itself.
    dragged: WindowId,
    torn: bool,
    /// The cursor's offset from the dragged window's top-left corner, and the window's size.
    grab: (i32, i32),
    size: (i32, i32),
    start: ScreenRect,
    /// The strip the tab would be dropped on, and the position.
    hint: Option<(WindowId, usize)>,
}

fn contains(r: ScreenRect, x: i32, y: i32) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

impl Browser {
    /// The restored rectangle of the current window, moved down-right: where a new window opens.
    fn cascade_bounds(&self) -> Option<Bounds> {
        let (r, _) = self.window()?.placement();
        Some(Bounds { x: r.x + CASCADE_PX, y: r.y + CASCADE_PX, width: r.width, height: r.height })
    }

    /// Ctrl+N / "New window": a new window with a new-tab page.
    pub(super) fn new_window(&self) {
        let bounds = self.cascade_bounds();
        let (_, tab, ops) = self.ws.borrow_mut().open_window("", false, bounds, false, now_ms());
        info!("new window");
        self.address_focus.set(Some(tab));
        self.apply(ops);
    }

    /// "Move to new window" (tab context menu). Refused for a window's only tab.
    pub(super) fn move_tab_to_new_window(&self, tab: TabId) {
        let bounds = self.cascade_bounds();
        let detached = self.ws.borrow_mut().detach(tab, bounds, false, now_ms());
        if let Some((win, ops)) = detached {
            info!("tab {tab}: moved to a new window {win}");
            self.apply(ops);
            self.make_current(win);
            self.focus_active();
        }
    }

    /// The shell of window `win` handed over a drag of `tab`, grabbed `offset_x` CSS px from
    /// the tab's left edge and `offset_y` from the top of the shell.
    pub(super) fn start_tab_drag(&self, win: WindowId, tab: TabId, offset_x: f64, offset_y: f64) {
        if self.drag.get().is_some() || self.window_of(tab) != Some(win) {
            return;
        }
        let Some(frame) = self.frame(win) else { return };
        let Some(app) = self.app() else { return };
        // The shell's browser holds the mouse capture from the press: the host takes over.
        cursor::release_capture();
        self.hide_popup();
        let (cx, cy) = cursor::position();
        let (index, siblings) = {
            let ws = self.ws.borrow();
            let Some(m) = ws.model(win) else { return };
            (m.index_of(tab).unwrap_or(0), m.len() > 1)
        };
        let start = frame.window.rect();
        let drag = if siblings {
            // The new window: the source's restored size, placed so the tab (its first one)
            // sits under the cursor where it was grabbed.
            let (normal, _) = frame.window.placement();
            let scale = frame.window.scale();
            let (ox, oy) = frame.window.client_to_screen(0, 0);
            let (client_dx, client_dy) =
                if frame.window.is_maximized() { (8, 0) } else { (ox - start.x, oy - start.y) };
            let first_tab = frame
                .hit_map
                .borrow()
                .as_ref()
                .and_then(|m| m.interactive.get(1).map(|r| r.x))
                .unwrap_or(FIRST_TAB_X_DIP);
            let grab = (
                client_dx + ((first_tab + offset_x.clamp(0.0, 400.0)) * scale) as i32,
                client_dy + (offset_y.clamp(0.0, 200.0) * scale) as i32,
            );
            let bounds = Bounds { x: cx - grab.0, y: cy - grab.1, width: normal.width, height: normal.height };
            let detached = self.ws.borrow_mut().detach(tab, Some(bounds), false, now_ms());
            let Some((new_win, ops)) = detached else { return };
            self.apply(ops);
            info!("tab {tab}: dragged out of window {win} into a new window {new_win}");
            TabDrag {
                tab,
                source: win,
                source_index: index,
                dragged: new_win,
                torn: true,
                grab,
                size: (normal.width, normal.height),
                start,
                hint: None,
            }
        } else {
            // The window's only tab: the whole window follows the mouse.
            if frame.window.is_maximized() {
                frame.window.control(lb_core::protocol::WindowAction::Maximize);
                // restores
            }
            let r = frame.window.rect();
            let grab = ((cx - start.x).clamp(0, r.width.max(1) - 1), (cy - start.y).clamp(0, r.height.max(1) - 1));
            info!("tab {tab}: dragging its window {win}");
            TabDrag {
                tab,
                source: win,
                source_index: index,
                dragged: win,
                torn: false,
                grab,
                size: (r.width, r.height),
                start,
                hint: None,
            }
        };
        // The dragged window stays above the others (it is the drag image): Windows may hand
        // activation to the window under it while the button is held.
        if let Some(f) = self.frame(drag.dragged) {
            f.window.set_topmost(true);
        }
        self.drag.set(Some(drag));
        app.set_timer(DRAG_TIMER, DRAG_TICK_MS);
    }

    /// `DRAG_TIMER`: follow the mouse.
    pub(super) fn drag_tick(&self) {
        let Some(mut d) = self.drag.get() else {
            if let Some(app) = self.app() {
                app.kill_timer(DRAG_TIMER);
            }
            return;
        };
        if cursor::escape_down() {
            return self.end_tab_drag(true);
        }
        if cursor::buttons() & 1 == 0 {
            return self.end_tab_drag(false);
        }
        let (cx, cy) = cursor::position();
        if let Some(f) = self.frame(d.dragged) {
            f.window.move_to(ScreenRect { x: cx - d.grab.0, y: cy - d.grab.1, width: d.size.0, height: d.size.1 });
        }
        let target = self.drop_target(d.dragged, cx, cy);
        if target != d.hint {
            match target {
                Some((w, i)) => info!("tab {}: over window {w}'s strip (position {i})", d.tab),
                None => info!("tab {}: not over a strip", d.tab),
            }
            if let Some((old, _)) = d.hint.filter(|(w, _)| target.map(|t| t.0) != Some(*w)) {
                self.post_shell_to(old, &Event::TabDropHint { index: None });
            }
            if let Some((win, index)) = target {
                self.post_shell_to(win, &Event::TabDropHint { index: Some(index as u32) });
            }
            d.hint = target;
        }
        self.drag.set(Some(d));
    }

    /// The topmost other window under `(x, y)`, if the point is over its tab strip: that
    /// window and the drop position.
    fn drop_target(&self, dragged: WindowId, x: i32, y: i32) -> Option<(WindowId, usize)> {
        let frames: Vec<_> = self.all_frames().into_iter().filter(|f| f.id != dragged).collect();
        let windows: Vec<_> = frames.iter().map(|f| f.window).collect();
        // Only the topmost window containing the point counts: one under it is covered.
        let ordered = window::in_z_order(&windows);
        let top = ordered.into_iter().find(|w| contains(w.rect(), x, y))?;
        let f = frames.iter().find(|f| f.window == top)?;
        if f.window.is_minimized() || self.is_fullscreen_frame(f.id) {
            return None;
        }
        let scale = f.window.scale();
        let (ox, oy) = f.window.client_to_screen(0, 0);
        let (css_x, css_y) = (f64::from(x - ox) / scale, f64::from(y - oy) / scale);
        let map = f.hit_map.borrow();
        let map = map.as_ref()?;
        if css_y < -STRIP_SLACK_DIP || css_y > map.strip_height + STRIP_SLACK_DIP {
            return None;
        }
        Some((f.id, frame::drop_index(&map.tab_centers, css_x)))
    }

    /// The button was released (or Esc pressed: `cancel`).
    fn end_tab_drag(&self, cancel: bool) {
        if let Some(app) = self.app() {
            app.kill_timer(DRAG_TIMER);
        }
        let Some(d) = self.drag.take() else { return };
        if let Some(f) = self.frame(d.dragged) {
            f.window.set_topmost(false);
        }
        if let Some((w, _)) = d.hint {
            self.post_shell_to(w, &Event::TabDropHint { index: None });
        }
        let now = now_ms();
        if cancel {
            info!("tab {}: drag cancelled", d.tab);
            if d.torn {
                if let Some(f) = self.frame(d.source) {
                    f.window.bring_to_front();
                }
                let ops = self.ws.borrow_mut().move_tab(d.tab, d.source, d.source_index, now);
                self.apply(ops);
            } else if let Some(f) = self.frame(d.dragged) {
                f.window.move_to(d.start);
            }
            self.focus_window(d.source);
            return;
        }
        match d.hint {
            Some((target, index)) => {
                info!("tab {}: dropped on window {target} at {index}", d.tab);
                // Raise and activate the target while the dragged window is still ours and in
                // front: once that window is destroyed, Windows may activate another program
                // and refuse the target the foreground.
                if let Some(f) = self.frame(target) {
                    f.window.bring_to_front();
                }
                let ops = self.ws.borrow_mut().move_tab(d.tab, target, index, now);
                self.apply(ops);
                self.focus_window(target);
            }
            None => {
                self.focus_window(d.dragged);
                self.schedule_session_save();
            }
        }
    }

    /// Brings a window to the front with the keyboard focus in its page.
    pub(super) fn focus_window(&self, win: WindowId) {
        if let Some(f) = self.frame(win) {
            f.window.bring_to_front();
            self.make_current(win);
            self.layout_frame(&f);
            self.focus_active();
        }
        // Activation changes during the drag were not passed on.
        self.send_window_state();
    }
}
