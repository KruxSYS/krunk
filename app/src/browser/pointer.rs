//! Pointer lock for pages (see `lb_core::pointer_lock` for why Krunk does it
//! itself) and full screen (page `requestFullscreen()` and F11).
//!
//! While a frame holds the lock the cursor is confined to the page and hidden; raw input
//! (`WM_INPUT`) drives the movement, which goes to the frame in small batches as `lb.plock.evt`
//! messages. Unadjusted movement sends the raw device deltas; otherwise the cursor's own
//! (accelerated) movement since the last batch is measured, as Chrome does. The hidden cursor
//! goes back to the page's centre only when it nears the edge: Windows makes a mouse-move
//! message only when the cursor really moved, and those carry the movement to the page as
//! trusted events.

use super::Browser;
use crate::ffi::cursor;
use crate::ffi::overlay::{Overlay, OverlaySpec};
use crate::ffi::window::ScreenRect;
use crate::log::info;
use lb_core::layout;
use lb_core::pointer_lock::{
    needs_recentre, AccelFilter, Batcher, HintCounter, Lock, LockMessage, LockState, Movement, Release, RequestContext,
};
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use std::cell::{Cell, RefCell};

/// Timer: the "Press Esc" hint's fade in, life and fade out.
pub(super) const HINT_TIMER: usize = 9;
const HINT_MS: u32 = 3000;
const HINT_FADE_MS: u32 = 150;
const HINT_TICK_MS: u32 = 30;

/// What is shown full screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum FullscreenMode {
    #[default]
    Off,
    /// A page asked (`requestFullscreen`); Esc leaves it.
    Page(TabId),
    /// F11: the whole browser without its tab strip and toolbar.
    Browser,
}

#[derive(Default)]
pub(super) struct PointerHost {
    state: RefCell<LockState>,
    batch: Cell<Batcher>,
    flush_scheduled: Cell<bool>,
    /// The lock area and its centre (screen pixels), and where the hidden cursor was at the
    /// last delivery.
    area: Cell<(i32, i32, i32, i32)>,
    center: Cell<(i32, i32)>,
    last: Cell<(i32, i32)>,
    accel: Cell<AccelFilter>,
    pub fullscreen: Cell<FullscreenMode>,
    /// The frame that is full screen.
    pub fullscreen_window: Cell<Option<lb_core::windows::WindowId>>,
    /// How often each tab showed the pointer-lock hint (at most three times per tab).
    pub hints: RefCell<HintCounter>,
    /// The hint overlay (owned by the frame it shows in) and when it appeared.
    hint: RefCell<Option<(lb_core::windows::WindowId, Overlay)>>,
    hint_shown_at: Cell<i64>,
}

impl Browser {
    // ------------------------------------------------------------ pointer lock

    /// A frame of tab `tab` sent a pointer-lock message.
    pub(super) fn pointer_message(&self, tab: TabId, frame: String, json: &str) {
        let Some(msg) = lb_core::pointer_lock::parse_message(json) else { return };
        let Some(view) = self.tab_view(tab) else { return };
        match msg {
            LockMessage::PointerLock { unadjusted } => {
                let ctx = RequestContext {
                    tab_active: self.active_id() == Some(tab),
                    window_active: self
                        .window_of(tab)
                        .and_then(|w| self.frame(w))
                        .is_some_and(|f| f.window.is_active()),
                    frame_live: view.frame_live(&frame),
                };
                let lock = Lock { tab, frame: frame.clone(), unadjusted };
                let result = self.pointer.state.borrow_mut().request(lock, ctx);
                match result {
                    Ok(previous) => {
                        if let Some(p) = previous {
                            self.tell_frame(&p, r#"{"t":"released"}"#);
                        }
                        self.engage_pointer();
                        view.send_to_frame(&frame, r#"{"t":"granted"}"#);
                        info!("tab {tab}: pointer locked{}", if unadjusted { " (unadjusted movement)" } else { "" });
                        // At most three times per tab and run of the app.
                        if self.pointer.hints.borrow_mut().should_show(tab) {
                            self.show_hint("Press [Esc] to show your cursor");
                        } else {
                            info!("tab {tab}: pointer-lock hint not shown again");
                        }
                    }
                    Err(why) => {
                        info!("tab {tab}: pointer lock refused ({why:?})");
                        view.send_to_frame(&frame, r#"{"t":"denied"}"#);
                    }
                }
            }
            LockMessage::PointerUnlock {} => {
                let lock = self.pointer.state.borrow_mut().exit_from(tab, &frame);
                if let Some(lock) = lock {
                    self.disengage_pointer(lock, Release::Exited);
                }
            }
        }
    }

    pub(super) fn pointer_locked(&self) -> bool {
        self.pointer.state.borrow().is_locked()
    }

    /// Ends the lock (if any) for `why`.
    pub(super) fn release_pointer(&self, why: Release) {
        let lock = self.pointer.state.borrow_mut().release(why);
        if let Some(lock) = lock {
            self.disengage_pointer(lock, why);
        }
    }

    /// Ends tab `tab`'s lock (closed, navigated, renderer gone).
    pub(super) fn release_pointer_of(&self, tab: TabId, why: Release) {
        let lock = self.pointer.state.borrow_mut().tab_event(tab, why);
        if let Some(lock) = lock {
            self.disengage_pointer(lock, why);
        }
    }

    /// Tab `to` is now shown: a lock of another tab ends.
    pub(super) fn pointer_tab_activated(&self, to: TabId) {
        let lock = self.pointer.state.borrow_mut().tab_activated(to);
        if let Some(lock) = lock {
            self.disengage_pointer(lock, Release::TabSwitched);
        }
    }

    /// The page's area on screen, where the cursor is confined.
    fn lock_area(&self) -> Option<ScreenRect> {
        let (x, y, width, height) = self.active_view()?.screen_rect()?;
        (width > 0 && height > 0).then_some(ScreenRect { x, y, width, height })
    }

    /// Confines, hides and centres the cursor and starts raw input. Called
    /// again when the window moves or changes size.
    fn engage_pointer(&self) {
        self.hide_status();
        let Some(w) = self.window() else { return };
        let Some(area) = self.lock_area() else { return };
        let center = (area.x + area.width / 2, area.y + area.height / 2);
        self.pointer.area.set((area.x, area.y, area.width, area.height));
        self.pointer.center.set(center);
        self.pointer.last.set(center);
        cursor::confine(Some(area));
        cursor::set_hidden(true);
        cursor::move_to(center.0, center.1);
        w.capture_raw_mouse(true);
    }

    /// The window moved, resized or changed DPI or full screen: keep the
    /// cursor confined to the page's new place.
    pub(super) fn reapply_pointer(&self) {
        if self.pointer_locked() {
            self.engage_pointer();
        }
    }

    fn disengage_pointer(&self, lock: Lock, why: Release) {
        cursor::confine(None);
        cursor::set_hidden(false);
        if let Some(w) = self.window() {
            w.capture_raw_mouse(false);
        }
        self.pointer.batch.set(Batcher::default());
        self.tell_frame(&lock, r#"{"t":"released"}"#);
        self.hide_hint();
        info!("tab {}: pointer unlocked ({why:?})", lock.tab);
    }

    fn tell_frame(&self, lock: &Lock, json: &str) -> bool {
        self.tab_view(lock.tab).is_some_and(|v| v.send_to_frame(&lock.frame, json))
    }

    /// Raw mouse movement (WM_INPUT) while locked.
    pub(super) fn raw_mouse(&self, dx: i32, dy: i32) {
        if !self.pointer_locked() {
            return;
        }
        let mut b = self.pointer.batch.get();
        let out = b.push(dx, dy, now_ms());
        self.pointer.batch.set(b);
        match out {
            Some(m) => self.deliver_movement(m),
            None => self.schedule_flush(),
        }
    }

    /// The tail of a burst goes out after one interval at the latest.
    fn schedule_flush(&self) {
        if self.pointer.flush_scheduled.replace(true) {
            return;
        }
        let weak = self.me.clone();
        let delay = Batcher::default().interval_ms;
        crate::ffi::cef::post_ui_delayed(delay, move || {
            let Some(b) = weak.upgrade() else { return };
            b.pointer.flush_scheduled.set(false);
            let mut batch = b.pointer.batch.get();
            let out = batch.flush(now_ms());
            b.pointer.batch.set(batch);
            if let Some(m) = out {
                b.deliver_movement(m);
            }
        });
    }

    fn deliver_movement(&self, m: Movement) {
        let Some(lock) = self.pointer.state.borrow().held().cloned() else { return };
        // Accelerated movement: how far the (hidden) cursor moved since the last batch. It
        // stays there (see the module docs) unless it came near the edge of the page.
        let (x, y) = cursor::position();
        let (lx, ly) = self.pointer.last.get();
        if needs_recentre((x, y), self.pointer.area.get()) {
            let (cx, cy) = self.pointer.center.get();
            cursor::move_to(cx, cy);
            self.pointer.last.set((cx, cy));
        } else {
            self.pointer.last.set((x, y));
        }
        let (dx, dy) = if lock.unadjusted {
            (m.dx, m.dy)
        } else {
            // Checked against the raw movement (see AccelFilter).
            let mut f = self.pointer.accel.get();
            let d = f.filter((x - lx, y - ly), (m.dx, m.dy));
            self.pointer.accel.set(f);
            d
        };
        if dx == 0 && dy == 0 {
            return;
        }
        let json = format!(r#"{{"t":"move","dx":{dx},"dy":{dy},"b":{}}}"#, cursor::buttons());
        if !self.tell_frame(&lock, &json) {
            // The frame went away (navigated, removed): the lock ends.
            self.release_pointer(Release::Navigated);
        }
    }

    // ------------------------------------------------------------ hint

    /// A centred, rounded, semi-transparent pill with a key cap for the bracketed key,
    /// 24 DIP below the toolbar (or the top of the screen in full screen). A native overlay:
    /// it takes no focus and no clicks, and fades in and out (not under reduced motion).
    fn show_hint(&self, text: &str) {
        let Some(win) = self.cur_id() else { return };
        let Some(f) = self.frame(win) else { return };
        let w = f.window;
        let scale = w.scale();
        let (width, height) = w.client_size();
        let top = if self.is_fullscreen_frame(win) { 0 } else { layout::shell_height(scale, self.bar_shown(win)) };
        let (ox, oy) = w.client_to_screen(0, top);
        let area = lb_core::layout::Rect { x: ox, y: oy, width, height: height - top };
        let mut slot = self.pointer.hint.borrow_mut();
        if slot.as_ref().is_none_or(|(o, _)| *o != win) {
            match Overlay::create(w) {
                Ok(o) => *slot = Some((win, o)),
                Err(e) => return crate::log::warning!("hint overlay: {e}"),
            }
        }
        let Some((_, o)) = slot.as_ref() else { return };
        let spec = OverlaySpec {
            palette: lb_core::overlay::hint_palette(),
            runs: lb_core::overlay::hint_runs(text),
            font_px: (14.0 * scale).round() as i32,
            pad_x: (16.0 * scale).round() as i32,
            pad_y: (10.0 * scale).round() as i32,
            radius: 20.0 * scale,
            border: 0,
        };
        let (hw, hh) = o.size_of(&spec);
        let r = lb_core::overlay::hint_rect(area, hw, hh, (24.0 * scale).round() as i32);
        let fade = if crate::ffi::window::client_area_animation() { HINT_FADE_MS } else { 0 };
        let alpha = lb_core::overlay::fade_alpha(0, HINT_MS, fade).unwrap_or(255);
        o.show(&spec, r.x, r.y, alpha);
        self.pointer.hint_shown_at.set(now_ms());
        if let Some(app) = self.app() {
            app.set_timer(HINT_TIMER, HINT_TICK_MS);
        }
    }

    /// `HINT_TIMER`: the fade in, the life and the fade out of the hint.
    pub(super) fn hint_tick(&self) {
        let elapsed = (now_ms() - self.pointer.hint_shown_at.get()).clamp(0, i64::from(u32::MAX)) as u32;
        let fade = if crate::ffi::window::client_area_animation() { HINT_FADE_MS } else { 0 };
        match lb_core::overlay::fade_alpha(elapsed, HINT_MS, fade) {
            Some(alpha) => {
                if let Some((_, o)) = self.pointer.hint.borrow().as_ref() {
                    o.set_alpha(alpha);
                }
            }
            None => self.hide_hint(),
        }
    }

    pub(super) fn hide_hint(&self) {
        if let Some(w) = self.app() {
            w.kill_timer(HINT_TIMER);
        }
        if let Some((_, o)) = self.pointer.hint.borrow().as_ref() {
            o.hide();
        }
    }

    // ------------------------------------------------------------ full screen

    /// A page entered or left full screen.
    pub(super) fn page_fullscreen(&self, tab: TabId, on: bool) {
        if on {
            self.enter_fullscreen(FullscreenMode::Page(tab));
            self.show_hint("Press [Esc] to exit full screen");
        } else if self.pointer.fullscreen.get() == FullscreenMode::Page(tab) {
            self.leave_fullscreen();
        }
    }

    /// F11: full screen without the browser's chrome, or back.
    pub(super) fn toggle_fullscreen(&self) {
        match self.pointer.fullscreen.get() {
            FullscreenMode::Off => self.enter_fullscreen(FullscreenMode::Browser),
            FullscreenMode::Browser => self.leave_fullscreen(),
            // The page's fullscreen element goes; the engine reports it.
            FullscreenMode::Page(tab) => match self.tab_view(tab) {
                Some(v) => v.exit_fullscreen(),
                None => self.leave_fullscreen(),
            },
        }
    }

    /// Esc in page full screen (not while the pointer is locked: Esc first
    /// releases the pointer, as in Chrome).
    pub(super) fn escape_page_fullscreen(&self) -> bool {
        if let FullscreenMode::Page(tab) = self.pointer.fullscreen.get() {
            if let Some(v) = self.tab_view(tab) {
                v.exit_fullscreen();
                return true;
            }
        }
        false
    }

    fn enter_fullscreen(&self, mode: FullscreenMode) {
        self.hide_status();
        // A page's own window, or (F11) the current one.
        let win = match mode {
            FullscreenMode::Page(tab) => self.window_of(tab),
            _ => self.cur_id(),
        };
        let Some(frame) = win.and_then(|w| self.frame(w)) else { return };
        if let Some(other) = self.pointer.fullscreen_window.get().filter(|w| *w != frame.id) {
            // Only one window is full screen at a time.
            if let Some(f) = self.frame(other) {
                f.window.set_fullscreen(false);
                if let Some(s) = f.shell.borrow().as_ref() {
                    s.set_visible(true);
                }
            }
        }
        self.hide_popup();
        self.pointer.fullscreen.set(mode);
        self.pointer.fullscreen_window.set(Some(frame.id));
        frame.window.set_fullscreen(true);
        if let Some(s) = frame.shell.borrow().as_ref() {
            s.set_visible(false);
        }
        self.layout();
        self.send_window_state();
        self.reapply_pointer();
        info!("full screen ({mode:?})");
    }

    pub(super) fn leave_fullscreen(&self) {
        if self.pointer.fullscreen.replace(FullscreenMode::Off) == FullscreenMode::Off {
            return;
        }
        if let Some(f) = self.pointer.fullscreen_window.take().and_then(|w| self.frame(w)) {
            f.window.set_fullscreen(false);
            if let Some(s) = f.shell.borrow().as_ref() {
                s.set_visible(true);
            }
        }
        self.hide_hint();
        self.layout();
        self.send_window_state();
        self.reapply_pointer();
        info!("full screen ended");
    }

    pub(super) fn is_fullscreen(&self) -> bool {
        self.pointer.fullscreen.get() != FullscreenMode::Off
    }

    /// Is frame `win` the one shown full screen?
    pub(super) fn is_fullscreen_frame(&self, win: lb_core::windows::WindowId) -> bool {
        self.is_fullscreen() && self.pointer.fullscreen_window.get() == Some(win)
    }

    /// Is frame `win` showing a page full screen (not F11)?
    pub(super) fn is_page_fullscreen_frame(&self, win: lb_core::windows::WindowId) -> bool {
        matches!(self.pointer.fullscreen.get(), FullscreenMode::Page(_))
            && self.pointer.fullscreen_window.get() == Some(win)
    }

    /// The tab shown full screen by its page is no longer shown or exists.
    pub(super) fn fullscreen_tab_gone(&self, tab: TabId) {
        if self.pointer.fullscreen.get() == FullscreenMode::Page(tab) {
            if let Some(v) = self.tab_view(tab) {
                v.exit_fullscreen();
            }
            self.leave_fullscreen();
        }
    }
}
