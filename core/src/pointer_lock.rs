//! Pointer lock (`element.requestPointerLock()`) for browser games.
//!
//! CEF's Alloy runtime style never implements
//! `WebContentsDelegate::RequestPointerLock`; Chromium's default answers
//! every request with `kUnknownError`, so the API can never succeed in our
//! browsers. Krunk therefore provides it itself: a document-start shim in
//! every frame implements the API and asks the host; the host confines and
//! hides the cursor, reads raw mouse input and streams the movement back to
//! the frame (see the app's `browser::pointer`). This module holds the
//! decisions: who may lock, when the lock ends, and how movement is batched.
//! (Moving tab browsers to CEF's Chrome runtime style, which has native
//! pointer lock, is the long-term fix.)

use serde::Deserialize;
use std::collections::HashMap;

/// How often a tab shows "Press Esc to show your cursor" (per tab, per run of the app).
pub const HINT_SHOW_LIMIT: u8 = 3;

/// Counts the pointer-lock hints shown per tab. In memory only: closing the tab or
/// restarting the app forgets the count.
#[derive(Debug, Default)]
pub struct HintCounter {
    shown: HashMap<crate::tabs::TabId, u8>,
}

impl HintCounter {
    /// Should the hint show for this lock? Counts it when it does.
    pub fn should_show(&mut self, tab: crate::tabs::TabId) -> bool {
        let n = self.shown.entry(tab).or_insert(0);
        if *n >= HINT_SHOW_LIMIT {
            return false;
        }
        *n += 1;
        true
    }

    /// The tab was closed: a tab with the same id later starts afresh.
    pub fn forget(&mut self, tab: crate::tabs::TabId) {
        self.shown.remove(&tab);
    }
}

/// Largest pointer-lock message a frame may send.
pub const MAX_MESSAGE_BYTES: usize = 256;

/// What a frame asks for. Strict: nothing else is accepted on this channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "lb", rename_all = "camelCase", deny_unknown_fields)]
pub enum LockMessage {
    /// `requestPointerLock({unadjustedMovement})`, called with transient
    /// user activation (checked by the shim; the host checks the rest).
    PointerLock { unadjusted: bool },
    /// `document.exitPointerLock()`.
    PointerUnlock {},
}

/// Parses a message from the pointer-lock channel.
pub fn parse_message(json: &str) -> Option<LockMessage> {
    if json.len() > MAX_MESSAGE_BYTES {
        return None;
    }
    serde_json::from_str(json).ok()
}

/// The situation in which a frame asks for the lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestContext {
    /// The request comes from the tab that is shown.
    pub tab_active: bool,
    /// The browser window is the foreground window and not minimized.
    pub window_active: bool,
    /// The frame is still attached to the tab's current document.
    pub frame_live: bool,
}

/// Why a request is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deny {
    BackgroundTab,
    WindowInactive,
    FrameGone,
}

/// Why a lock ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Release {
    /// The user pressed Esc (the page does not get that key).
    Escape,
    /// Alt+Tab, a click elsewhere, or the window was minimized.
    WindowDeactivated,
    TabSwitched,
    TabClosed,
    /// The tab started another page, or the frame went away.
    Navigated,
    RendererGone,
    /// `document.exitPointerLock()`, or another frame took the lock.
    Exited,
}

/// The lock held by one frame of one tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lock {
    pub tab: u32,
    /// CEF frame identifier.
    pub frame: String,
    /// Raw (unaccelerated) movement was asked for.
    pub unadjusted: bool,
}

/// At most one lock exists at a time (as in Chrome).
#[derive(Debug, Default)]
pub struct LockState {
    held: Option<Lock>,
}

impl LockState {
    pub fn held(&self) -> Option<&Lock> {
        self.held.as_ref()
    }

    pub fn is_locked(&self) -> bool {
        self.held.is_some()
    }

    /// A frame asks for the lock. On success the previous lock (another
    /// frame's) is returned so it can be told it lost it.
    pub fn request(&mut self, lock: Lock, ctx: RequestContext) -> Result<Option<Lock>, Deny> {
        if !ctx.frame_live {
            return Err(Deny::FrameGone);
        }
        if !ctx.tab_active {
            return Err(Deny::BackgroundTab);
        }
        if !ctx.window_active {
            return Err(Deny::WindowInactive);
        }
        let previous = self.held.replace(lock.clone());
        Ok(previous.filter(|p| p.frame != lock.frame || p.tab != lock.tab))
    }

    /// Ends the lock for any reason; returns it if there was one.
    pub fn release(&mut self, _why: Release) -> Option<Lock> {
        self.held.take()
    }

    /// Ends the lock if it belongs to `frame` of `tab` (exitPointerLock).
    pub fn exit_from(&mut self, tab: u32, frame: &str) -> Option<Lock> {
        if self.held.as_ref().is_some_and(|l| l.tab == tab && l.frame == frame) {
            return self.release(Release::Exited);
        }
        None
    }

    /// Tab `active` became the shown tab.
    pub fn tab_activated(&mut self, active: u32) -> Option<Lock> {
        if self.held.as_ref().is_some_and(|l| l.tab != active) {
            return self.release(Release::TabSwitched);
        }
        None
    }

    /// Something happened to tab `tab` that ends its lock (closed,
    /// navigated, renderer gone).
    pub fn tab_event(&mut self, tab: u32, why: Release) -> Option<Lock> {
        if self.held.as_ref().is_some_and(|l| l.tab == tab) {
            return self.release(why);
        }
        None
    }
}

/// Mouse movement collected between two deliveries to the frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Movement {
    pub dx: i32,
    pub dy: i32,
    /// Raw input reports in this batch.
    pub reports: u32,
}

/// Batches raw input reports (up to 1000 Hz) into deliveries to the
/// renderer: at most one per `interval_ms`, and at once when a delivery is
/// overdue, so the stream never stalls (no matter where the cursor is).
#[derive(Debug, Clone, Copy)]
pub struct Batcher {
    pending: Movement,
    last_flush_ms: i64,
    pub interval_ms: i64,
}

/// Should the hidden cursor go back to the centre of the lock area? Only once it has moved
/// into the outer quarter of the area on some side. Between those warps it stays where the
/// movement put it, so Windows keeps making real mouse-move messages (it makes one only when
/// the cursor actually moved), which the page receives as trusted events. `area` is
/// (x, y, width, height) in screen pixels.
pub fn needs_recentre(pos: (i32, i32), area: (i32, i32, i32, i32)) -> bool {
    let (x, y, w, h) = area;
    let (mx, my) = (w / 4, h / 4);
    pos.0 < x + mx || pos.0 >= x + w - mx || pos.1 < y + my || pos.1 >= y + h - my
}

/// Accelerated movement is measured from the (hidden) cursor's position, which Windows reports
/// a moment late after the browser moves it back to the centre: a batch then reads as a jump
/// of a quarter of the page, forwards or backwards. Each batch also has the raw device
/// movement, so a measured movement that goes against it or is bigger than any pointer
/// acceleration makes it is replaced by the raw movement times the acceleration seen lately.
#[derive(Debug, Clone, Copy)]
pub struct AccelFilter {
    /// Recent ratio of measured (accelerated) to raw movement.
    ratio: f64,
}

/// Windows' pointer speed and "enhance pointer precision" together stay well below this.
const MAX_ACCEL: i32 = 10;

impl Default for AccelFilter {
    fn default() -> Self {
        AccelFilter { ratio: 1.0 }
    }
}

impl AccelFilter {
    /// The movement to report for a batch: `measured` from the cursor, `raw` from the device.
    pub fn filter(&mut self, measured: (i32, i32), raw: (i32, i32)) -> (i32, i32) {
        let plausible = |m: i32, r: i32| {
            let against = m != 0 && r != 0 && m.signum() != r.signum() && m.abs() > 2;
            let too_far = m.abs() > r.abs().saturating_mul(MAX_ACCEL) + 4;
            !against && !too_far
        };
        if plausible(measured.0, raw.0) && plausible(measured.1, raw.1) {
            let (m, r) = (measured.0.abs() + measured.1.abs(), raw.0.abs() + raw.1.abs());
            if r >= 4 && m > 0 {
                let seen = (f64::from(m) / f64::from(r)).clamp(0.1, f64::from(MAX_ACCEL));
                self.ratio = self.ratio * 0.9 + seen * 0.1;
            }
            return measured;
        }
        let scale = |r: i32| (f64::from(r) * self.ratio).round() as i32;
        (scale(raw.0), scale(raw.1))
    }
}

/// Deliveries happen about every 2 ms (within the 1–4 ms budget).
pub const FLUSH_INTERVAL_MS: i64 = 2;

impl Default for Batcher {
    fn default() -> Self {
        Batcher { pending: Movement::default(), last_flush_ms: 0, interval_ms: FLUSH_INTERVAL_MS }
    }
}

impl Batcher {
    /// Adds one report. Returns the batch to deliver now, if it is time.
    pub fn push(&mut self, dx: i32, dy: i32, now_ms: i64) -> Option<Movement> {
        self.pending.dx = self.pending.dx.saturating_add(dx);
        self.pending.dy = self.pending.dy.saturating_add(dy);
        self.pending.reports += 1;
        if now_ms - self.last_flush_ms >= self.interval_ms {
            return self.flush(now_ms);
        }
        None
    }

    /// Delivers whatever is pending (a timer calls this so the last
    /// movement of a burst is not held back).
    pub fn flush(&mut self, now_ms: i64) -> Option<Movement> {
        if self.pending.reports == 0 {
            return None;
        }
        self.last_flush_ms = now_ms;
        Some(std::mem::take(&mut self.pending))
    }

    pub fn has_pending(&self) -> bool {
        self.pending.reports > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_shows_three_times_per_tab() {
        let mut c = HintCounter::default();
        assert!(c.should_show(1) && c.should_show(1) && c.should_show(1), "locks 1-3 show the hint");
        assert!(!c.should_show(1), "the 4th lock in the same tab does not");
        assert!(!c.should_show(1));
        assert!(c.should_show(2), "another tab has its own count");
        c.forget(1);
        assert!(c.should_show(1), "a closed tab's count is gone");
    }

    #[test]
    fn accel_filter_keeps_plausible_movement_and_replaces_warp_glitches() {
        let mut f = AccelFilter::default();
        // Steady movement with 2x acceleration: passed through, and the ratio is learnt.
        for _ in 0..60 {
            assert_eq!(f.filter((20, 0), (10, 0)), (20, 0));
        }
        assert_eq!(f.filter((0, 0), (0, 0)), (0, 0));
        assert_eq!(f.filter((3, -1), (1, 0)), (3, -1), "small jitter across an axis is fine");
        // The cursor read a moment late after re-centring: a backward jump of 266 px …
        let (x, y) = f.filter((-266, 0), (10, 0));
        assert!((18..=22).contains(&x) && y == 0, "replaced by raw x ratio: {x},{y}");
        // … or a forward one far beyond any acceleration.
        let (x, _) = f.filter((379, 0), (10, 0));
        assert!((18..=22).contains(&x), "{x}");
        // Raw movement with no measured movement (the cursor sat at the clip edge) is fine too.
        assert_eq!(f.filter((0, 0), (10, 0)), (0, 0));
    }

    #[test]
    fn recentre_only_near_the_edge() {
        let area = (100, 50, 800, 400); // centre (500, 250); inner box x 300..700, y 150..350
        assert!(!needs_recentre((500, 250), area));
        assert!(!needs_recentre((300, 150), area));
        assert!(!needs_recentre((699, 349), area));
        assert!(needs_recentre((299, 250), area));
        assert!(needs_recentre((700, 250), area));
        assert!(needs_recentre((500, 149), area));
        assert!(needs_recentre((500, 350), area));
        // A tiny area: always back to the centre once off it.
        assert!(!needs_recentre((2, 2), (0, 0, 4, 4)));
        assert!(needs_recentre((0, 0), (0, 0, 4, 4)));
    }

    fn ok() -> RequestContext {
        RequestContext { tab_active: true, window_active: true, frame_live: true }
    }

    fn lock(tab: u32, frame: &str) -> Lock {
        Lock { tab, frame: frame.into(), unadjusted: false }
    }

    #[test]
    fn messages_are_strict() {
        assert_eq!(
            parse_message(r#"{"lb":"pointerLock","unadjusted":true}"#),
            Some(LockMessage::PointerLock { unadjusted: true })
        );
        assert_eq!(parse_message(r#"{"lb":"pointerUnlock"}"#), Some(LockMessage::PointerUnlock {}));
        assert_eq!(parse_message(r#"{"lb":"pointerLock"}"#), None, "unadjusted is required");
        assert_eq!(parse_message(r#"{"lb":"pointerLock","unadjusted":false,"tab":3}"#), None);
        assert_eq!(parse_message(r#"{"lb":"pointerUnlock","frame":"x"}"#), None);
        assert_eq!(
            parse_message(r#"{"lb":"submit","username":"a","password":"b"}"#),
            None,
            "nothing else on this channel"
        );
        assert_eq!(parse_message(r#"{"cmd":"vaultList"}"#), None);
        let big = format!(r#"{{"lb":"pointerLock","unadjusted":false{}}}"#, " ".repeat(300));
        assert_eq!(parse_message(&big), None);
    }

    #[test]
    fn requests_need_an_active_tab_and_window() {
        let mut s = LockState::default();
        assert_eq!(s.request(lock(1, "f"), RequestContext { tab_active: false, ..ok() }), Err(Deny::BackgroundTab));
        assert_eq!(s.request(lock(1, "f"), RequestContext { window_active: false, ..ok() }), Err(Deny::WindowInactive));
        assert_eq!(s.request(lock(1, "f"), RequestContext { frame_live: false, ..ok() }), Err(Deny::FrameGone));
        assert!(!s.is_locked());
        assert_eq!(s.request(lock(1, "f"), ok()), Ok(None));
        assert_eq!(s.held(), Some(&lock(1, "f")));
        // Asking again from the same frame keeps it; another frame takes it.
        assert_eq!(s.request(lock(1, "f"), ok()), Ok(None));
        assert_eq!(s.request(lock(1, "g"), ok()), Ok(Some(lock(1, "f"))));
    }

    #[test]
    fn releases() {
        let mut s = LockState::default();
        s.request(lock(1, "f"), ok()).unwrap();
        assert_eq!(s.exit_from(1, "other"), None, "only the holder exits");
        assert_eq!(s.tab_activated(1), None);
        assert_eq!(s.tab_event(2, Release::TabClosed), None);
        assert_eq!(s.tab_activated(2), Some(lock(1, "f")), "switching tabs releases");
        s.request(lock(1, "f"), ok()).unwrap();
        assert_eq!(s.tab_event(1, Release::Navigated), Some(lock(1, "f")));
        s.request(lock(1, "f"), ok()).unwrap();
        assert_eq!(s.exit_from(1, "f"), Some(lock(1, "f")));
        assert_eq!(s.release(Release::Escape), None);
        s.request(lock(1, "f"), ok()).unwrap();
        assert!(s.release(Release::WindowDeactivated).is_some());
        assert!(!s.is_locked());
        // Re-requesting after a release works.
        assert_eq!(s.request(lock(1, "f"), ok()), Ok(None));
    }

    #[test]
    fn movement_is_batched_without_stalling() {
        let mut b = Batcher::default();
        assert_eq!(b.push(3, -1, 100), Some(Movement { dx: 3, dy: -1, reports: 1 }), "first report goes out at once");
        assert_eq!(b.push(2, 0, 100), None, "within the interval: held");
        assert_eq!(b.push(1, 1, 101), None);
        assert!(b.has_pending());
        assert_eq!(b.push(4, 4, 102), Some(Movement { dx: 7, dy: 5, reports: 3 }));
        assert_eq!(b.flush(103), None);
        b.push(5, 5, 103);
        assert_eq!(b.flush(104), Some(Movement { dx: 5, dy: 5, reports: 1 }), "the timer delivers the tail");
        // Big movements do not overflow.
        b.push(i32::MAX, 0, 200);
        assert_eq!(b.push(i32::MAX, 0, 201), None);
        assert_eq!(b.flush(202).unwrap().dx, i32::MAX);
    }
}
