//! Moving tabs between windows: dragging a tab out of its strip, onto another window's strip,
//! "Move to new window" and "New window" (docs: README "Windows and views").
//!
//! The shell reorders tabs within its own strip. When the pointer leaves the strip, it hands
//! the drag over (`tabDragOut`), because a shell view can't see other windows. From then on the
//! host follows the mouse: a tab with siblings moves at once into a new window that follows the
//! cursor (the window *is* the drag image); a window's only tab drags its whole window.
//!
//! 0.11.1: the source frame takes the mouse capture, and the window moves on every mouse move
//! as it arrives (a timer only backs that up: a lost capture, Esc). The other windows' places
//! and z-order are taken once at the start. Over another window's tab strip the dragged window
//! steps aside and that strip opens a gap with the tab in it (a "ghost") where it would land;
//! leaving the strip brings the window back under the cursor. Releasing there moves the tab in
//! (a window emptied by that closes); releasing elsewhere leaves the window where it is; Esc
//! puts the tab back. The tab's browser moves between frames without reloading
//! (`View::reparent`).

use super::Browser;
use crate::ffi::cursor;
use crate::ffi::window::{self, ScreenRect};
use crate::log::info;
use lb_core::frame::{self, DropWindow};
use lb_core::protocol::{Event, TabGhost};
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use lb_core::windows::{Bounds, WindowId};

/// Timer (on the app window) that backs up the mouse capture during a tab drag.
pub(super) const DRAG_TIMER: usize = 12;
/// Timer that follows the mouse while a bookmark is dragged out of a folder.
pub(super) const BAR_DRAG_TIMER: usize = 26;
const DRAG_TICK_MS: u32 = 15;
/// A tab-strip band this much taller than the strip still counts as "over the strip".
const STRIP_SLACK_DIP: f64 = 8.0;
/// Where the first tab of a new window starts (CSS px), if its shell hasn't reported yet.
const FIRST_TAB_X_DIP: f64 = 8.0;

#[derive(Debug, Clone, Copy)]
pub(super) struct TabDrag {
    tab: TabId,
    /// Where the tab came from (Esc puts it back).
    source: WindowId,
    source_index: usize,
    /// The window following the mouse: a new one for the tab, or the source itself.
    dragged: WindowId,
    torn: bool,
    /// The cursor's offset from the dragged window's top-left corner.
    grab: (i32, i32),
    start: ScreenRect,
    /// The strip the tab would be dropped on, and the position.
    hint: Option<(WindowId, usize)>,
    /// The cursor where the window was last put.
    last: (i32, i32),
    /// The dragged window stepped aside (off-screen): the tab shows in `hint`'s strip.
    aside: bool,
}

impl TabDrag {
    /// The window following the mouse.
    pub(super) fn dragged(&self) -> WindowId {
        self.dragged
    }
}

/// A bookmark (or folder) dragged out of an open folder dropdown: the host follows the
/// mouse and tells the shell under it where it is over the bookmarks bar.
#[derive(Debug, Clone, Copy)]
pub(super) struct BarDrag {
    id: i64,
    /// The window whose bar the cursor is over (its shell shows the mark).
    over: Option<WindowId>,
}

impl Browser {
    /// The restored rectangle of the current window, moved down-right: where a new window opens.
    pub(super) fn cascade_bounds(&self) -> Option<Bounds> {
        let w = self.window()?;
        let (r, _) = w.placement();
        let work = crate::ffi::window::Window::work_area_at(r.x + r.width / 2, r.y + r.height / 2);
        let to = lb_core::layout::cascade_window(
            lb_core::layout::Rect { x: r.x, y: r.y, width: r.width, height: r.height },
            w.scale(),
            lb_core::layout::Rect { x: work.x, y: work.y, width: work.width, height: work.height },
        );
        Some(Bounds { x: to.x, y: to.y, width: to.width, height: to.height })
    }

    /// Ctrl+N / "New window": a new window with a new-tab page.
    pub(super) fn new_window(&self) {
        let bounds = self.cascade_bounds();
        let (_, tab, ops) = self.ws.borrow_mut().open_window("", false, bounds, false, now_ms());
        info!("new window");
        self.address_focus.set(Some(tab));
        self.apply(ops);
    }

    /// "New private window" (the title bar's menu): a window whose first tab is private.
    pub(super) fn new_private_window(&self) {
        let bounds = self.cascade_bounds();
        let (_, tab, ops) = self.ws.borrow_mut().open_window("", true, bounds, false, now_ms());
        info!("new private window");
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
    /// the tab's left edge and `offset_y` from the top of the shell; a window's first tab
    /// starts at `first_x` there.
    pub(super) fn start_tab_drag(&self, win: WindowId, tab: TabId, offset_x: f64, offset_y: f64, first_x: Option<f64>) {
        if self.drag.get().is_some() || self.window_of(tab) != Some(win) {
            return;
        }
        let Some(frame) = self.frame(win) else { return };
        let Some(app) = self.app() else { return };
        // The shell's browser holds the mouse capture from the press: the host takes over.
        cursor::release_capture();
        self.hide_popup();
        // An Esc tapped before the drag is not a cancel (the check also counts taps since the last one).
        let _ = cursor::escape_down();
        let (cx, cy) = cursor::position();
        let ghost = self.ws.borrow().model(win).and_then(|m| m.info(tab)).map(|t| TabGhost {
            id: t.id,
            title: t.title,
            favicon: t.favicon,
            private: t.private,
        });
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
            // The new window's only tab starts where a row that does not scroll starts.
            let first_tab = first_x.filter(|x| x.is_finite()).unwrap_or(FIRST_TAB_X_DIP).clamp(0.0, 400.0);
            let grab = (
                client_dx + ((first_tab + offset_x.clamp(0.0, 400.0)) * scale).round() as i32,
                client_dy + (offset_y.clamp(0.0, 200.0) * scale).round() as i32,
            );
            let bounds = Bounds { x: cx - grab.0, y: cy - grab.1, width: normal.width, height: normal.height };
            let detached = self.ws.borrow_mut().detach(tab, Some(bounds), false, now_ms());
            let Some((new_win, ops)) = detached else { return };
            // The new window appears at once (no fade-in) under the cursor.
            self.tearing.set(true);
            self.apply(ops);
            self.tearing.set(false);
            info!("tab {tab}: dragged out of window {win} into a new window {new_win}");
            TabDrag {
                tab,
                source: win,
                source_index: index,
                dragged: new_win,
                torn: true,
                grab,
                start,
                hint: None,
                last: (cx, cy),
                aside: false,
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
                start,
                hint: None,
                last: (cx, cy),
                aside: false,
            }
        };
        // The dragged window stays above the others (it is the drag image): Windows may hand
        // activation to the window under it while the button is held.
        if let Some(f) = self.frame(drag.dragged) {
            f.window.set_topmost(true);
        }
        // The other windows, topmost first, as they are now.
        let others: Vec<_> = self.all_frames().into_iter().filter(|f| f.id != drag.dragged).collect();
        let order = window::in_z_order(&others.iter().map(|f| f.window).collect::<Vec<_>>());
        let targets = order
            .into_iter()
            .filter_map(|w| others.iter().find(|f| f.window == w))
            .filter(|f| !f.window.is_minimized() && !self.is_fullscreen_frame(f.id))
            .map(|f| {
                let r = f.window.rect();
                DropWindow {
                    win: f.id,
                    rect: (r.x, r.y, r.width, r.height),
                    origin: f.window.client_to_screen(0, 0),
                    scale: f.window.scale(),
                }
            })
            .collect();
        *self.drop_windows.borrow_mut() = targets;
        *self.drag_ghost.borrow_mut() = ghost;
        self.drag.set(Some(drag));
        // The source frame holds the mouse until the release (it outlives the drag: a torn-off
        // window is new, and a dragged whole window is the source).
        frame.window.follow_mouse();
        app.set_timer(DRAG_TIMER, DRAG_TICK_MS);
    }

    /// An item of the open folder dropdown was dragged out of it: the dropdown closes and
    /// the host follows the mouse until the button is released over a bookmarks bar.
    pub(super) fn start_bar_drag(&self, id: i64) {
        if self.drag.get().is_some() || self.bar_drag.get().is_some() {
            return;
        }
        let Some(app) = self.app() else { return };
        cursor::release_capture();
        self.hide_popup();
        let _ = cursor::escape_down();
        info!("bookmark {id}: dragged out of its folder");
        self.bar_drag.set(Some(BarDrag { id, over: None }));
        app.set_timer(BAR_DRAG_TIMER, DRAG_TICK_MS);
    }

    /// The topmost window under `(x, y)` whose bookmarks bar is there, with the point in
    /// CSS px of its shell.
    fn bar_under(&self, x: i32, y: i32) -> Option<(WindowId, f64, f64)> {
        let frames = self.all_frames();
        let windows: Vec<_> = frames.iter().map(|f| f.window).collect();
        let top = window::in_z_order(&windows).into_iter().find(|w| {
            let r = w.rect();
            x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
        })?;
        let f = frames.iter().find(|f| f.window == top)?;
        if f.window.is_minimized() || self.is_fullscreen_frame(f.id) {
            return None;
        }
        let scale = f.window.scale();
        let (ox, oy) = f.window.client_to_screen(0, 0);
        let (css_x, css_y) = (f64::from(x - ox) / scale, f64::from(y - oy) / scale);
        let width = f64::from(f.window.client_size().0) / scale;
        lb_core::layout::over_bookmarks_bar(f.bar.get(), css_x, css_y, width).then_some((f.id, css_x, css_y))
    }

    fn bar_drag_tick(&self, mut d: BarDrag) {
        let cancel = cursor::escape_down();
        let released = cursor::buttons() & 1 == 0;
        let (cx, cy) = cursor::position();
        let target = if cancel { None } else { self.bar_under(cx, cy) };
        if let Some(old) = d.over.filter(|w| target.map(|t| t.0) != Some(*w)) {
            self.post_shell_to(old, &Event::BarDragEnd);
        }
        if cancel || released {
            if let Some(app) = self.app() {
                app.kill_timer(BAR_DRAG_TIMER);
            }
            self.bar_drag.set(None);
            match target {
                Some((win, x, y)) if !cancel => {
                    info!("bookmark {}: dropped on window {win}'s bar", d.id);
                    self.post_shell_to(win, &Event::BarDragDrop { id: d.id, x, y });
                }
                _ => info!("bookmark {}: drag ended off the bar", d.id),
            }
            return;
        }
        if let Some((win, x, y)) = target {
            self.post_shell_to(win, &Event::BarDragAt { x, y });
        }
        d.over = target.map(|t| t.0);
        self.bar_drag.set(Some(d));
    }

    /// `BAR_DRAG_TIMER`: follow the mouse with a bookmark.
    pub(super) fn bar_drag_timer(&self) {
        match self.bar_drag.get() {
            Some(bd) => self.bar_drag_tick(bd),
            None => {
                if let Some(app) = self.app() {
                    app.kill_timer(BAR_DRAG_TIMER);
                }
            }
        }
    }

    /// A mouse move (or the release: `released`) during a tab drag, from the frame holding
    /// the capture.
    pub(super) fn drag_mouse(&self, released: bool) {
        if released {
            if self.drag.get().is_some() {
                self.end_tab_drag(cursor::escape_down());
            }
        } else {
            self.drag_tick();
        }
    }

    /// `DRAG_TIMER` and every mouse move: follow the mouse.
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
        if (cx, cy) == d.last {
            return;
        }
        d.last = (cx, cy);
        let target = self.drop_target(cx, cy);
        if let Some(f) = self.frame(d.dragged) {
            if target.is_some() {
                // Over a strip, the strip shows the tab: the window steps aside.
                if !d.aside {
                    let (x, y) = window::offscreen_point(f.window.rect().width);
                    f.window.move_pos(x, y);
                    d.aside = true;
                }
            } else {
                f.window.move_pos(cx - d.grab.0, cy - d.grab.1);
                d.aside = false;
            }
        }
        if target != d.hint {
            match target {
                Some((w, i)) => info!("tab {}: over window {w}'s strip (position {i})", d.tab),
                None => info!("tab {}: not over a strip", d.tab),
            }
            if let Some((old, _)) = d.hint.filter(|(w, _)| target.map(|t| t.0) != Some(*w)) {
                self.post_shell_to(old, &Event::TabDropHint { index: None, ghost: None });
            }
            if let Some((win, index)) = target {
                let ghost = self.drag_ghost.borrow().clone();
                self.post_shell_to(win, &Event::TabDropHint { index: Some(index as u32), ghost });
            }
            d.hint = target;
        }
        self.drag.set(Some(d));
    }

    /// The window (of those taken at the start) whose tab strip is under `(x, y)`, and the
    /// drop position there. The strip's tab centres are the ones its shell reported before it
    /// opened a gap (it reports none while the gap is open).
    fn drop_target(&self, x: i32, y: i32) -> Option<(WindowId, usize)> {
        let (win, css_x, css_y) = frame::point_in_topmost(&self.drop_windows.borrow(), x, y)?;
        let f = self.frame(win)?;
        let map = f.hit_map.borrow();
        let map = map.as_ref()?;
        frame::over_strip(map.strip_height, css_y, STRIP_SLACK_DIP)
            .then(|| (win, frame::drop_index(&map.tab_centers, css_x)))
    }

    /// The button was released (or Esc pressed: `cancel`).
    fn end_tab_drag(&self, cancel: bool) {
        if let Some(app) = self.app() {
            app.kill_timer(DRAG_TIMER);
        }
        window::stop_following();
        let Some(d) = self.drag.take() else { return };
        self.drop_windows.borrow_mut().clear();
        self.drag_ghost.borrow_mut().take();
        if let Some(f) = self.frame(d.dragged) {
            f.window.set_topmost(false);
        }
        let now = now_ms();
        if cancel {
            info!("tab {}: drag cancelled", d.tab);
            if let Some((w, _)) = d.hint {
                self.post_shell_to(w, &Event::TabDropHint { index: None, ghost: None });
            }
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
                // The strip has the tab in its gap now (its list came first); this clears a gap
                // the move did not fill.
                self.post_shell_to(target, &Event::TabDropHint { index: None, ghost: None });
                self.focus_window(target);
            }
            None => {
                // A torn-off window that stays gets Windows' animations back (minimize,
                // restore); one that goes closes at once.
                if let Some(f) = self.frame(d.dragged).filter(|_| d.torn) {
                    f.window.set_transitions(true);
                }
                // What the window's moves skipped while it followed the mouse.
                self.frame_moved(d.dragged);
                self.focus_window(d.dragged);
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
