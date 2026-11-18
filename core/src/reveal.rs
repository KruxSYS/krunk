//! Tab switches that wait for the new tab's first paint (0.7.0, R7): the old page stays on
//! screen until then (at most a second). Kept per window (0.11.0): one shared slot let a tab
//! switch in another window cancel this window's wait, and that window then kept showing the
//! old page under a different active tab.
//!
//! 0.15.0: a closed tab hands over too. The tab model closes a tab before it activates the next
//! one (`DestroyView`, then `Activate { from: None }`); until 0.14 the closed page went first and
//! the next tab showed with nothing beneath it (black or blank for a frame or two). Now the
//! closed tab's view stays on screen ("kept") until the next tab has painted (at most
//! [`REVEAL_TIMEOUT_MS`]) and goes after the swap.
//!
//! 0.15.2: a switch to a painted tab in the background waits too. Hidden from the engine, its
//! view has lost its frame; brought on top at once, it showed black until it rendered again.

use crate::tabs::TabId;
use crate::windows::{Op, WindowId};
use std::collections::HashMap;

/// How long a switch keeps the old page while the new tab has never painted (0.15.0: was
/// 1000 ms; a slow page never holds the swap longer). The last resort for new, discarded and
/// never-loaded views only.
pub const REVEAL_TIMEOUT_MS: i64 = 500;

/// 0.25.0: the cap for a tab that has painted before and only waits for its renderer to draw
/// again after being hidden from the engine. Its page is there; a busy or long-hidden renderer
/// (long tasks, timers catching up) answered its animation frames 150–530 ms late on the VM
/// (40 busy tabs) and held the old page that long. Shown beneath the old page meanwhile, its
/// compositor has drawn by then.
pub const REVEAL_PAINTED_CAP_MS: i64 = 150;

/// The cap of a wait: `painted_before` (out of sight) or never painted.
pub fn cap_for(painted_before: bool) -> i64 {
    if painted_before {
        REVEAL_PAINTED_CAP_MS
    } else {
        REVEAL_TIMEOUT_MS
    }
}

/// Per window: (the tab still on screen, the tab waiting to be shown, since when in ms, its cap
/// in ms), and the closed tab whose view is kept on screen. Generic over the ids.
#[derive(Debug)]
pub struct Reveals<W, T> {
    waiting: HashMap<W, (T, T, i64, i64)>,
    kept: HashMap<W, T>,
}

impl<W, T> Default for Reveals<W, T> {
    fn default() -> Self {
        Reveals { waiting: HashMap::new(), kept: HashMap::new() }
    }
}

/// What a beginning switch finds on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Begin<T> {
    /// The tab whose page is on screen now: the one a still-waiting switch of this window left
    /// there, else the tab switched from, else a closed tab kept on screen.
    pub shown: Option<T>,
    /// The tab an earlier, unfinished switch of this window was waiting for (shown beneath the
    /// page on screen, so it could paint): it goes out of sight again unless it is the new one.
    pub abandoned: Option<T>,
}

impl<W: Copy + Eq + std::hash::Hash, T: Copy + Eq> Reveals<W, T> {
    /// A switch in `win` from `from` to `to` begins.
    pub fn begin(&mut self, win: W, from: Option<T>, to: T) -> Begin<T> {
        match self.waiting.remove(&win) {
            Some((shown, waited, _, _)) => Begin {
                shown: if shown != to { Some(shown) } else { from },
                abandoned: Some(waited).filter(|w| *w != to),
            },
            None => {
                Begin { shown: from.or_else(|| self.kept.get(&win).copied().filter(|k| *k != to)), abandoned: None }
            }
        }
    }

    /// `to` waits in `win` (from `now`, ms) while `shown` stays on screen, at most `cap` ms
    /// ([`cap_for`]).
    pub fn wait(&mut self, win: W, shown: T, to: T, now: i64, cap: i64) {
        self.waiting.insert(win, (shown, to, now, cap.min(REVEAL_TIMEOUT_MS)));
    }

    /// Is `to` waiting to be shown (in any window)?
    pub fn is_waiting(&self, to: T) -> bool {
        self.waiting.values().any(|(_, t, _, _)| *t == to)
    }

    /// Since when (ms) `to` waits to be shown: tells a late answer for an earlier wait of the
    /// same tab from one for this wait.
    pub fn waiting_since(&self, to: T) -> Option<i64> {
        self.waiting.values().find(|(_, t, _, _)| *t == to).map(|(_, _, since, _)| *since)
    }

    /// The tab on screen in `win` while a switch waits there.
    pub fn shown_in(&self, win: W) -> Option<T> {
        self.waiting.get(&win).map(|(s, _, _, _)| *s)
    }

    /// `to` is ready (its first frame is painted): its window and the tab to hide; it no
    /// longer waits.
    pub fn take(&mut self, to: T) -> Option<(W, T)> {
        let win = *self.waiting.iter().find(|(_, (_, t, _, _))| *t == to)?.0;
        self.waiting.remove(&win).map(|(shown, _, _, _)| (win, shown))
    }

    /// The timeout of `to`'s wait: once its cap has passed since it began, it is shown painted
    /// or not ([`Reveals::take`]); before that (a later wait of the same tab) and after it was
    /// revealed, nothing.
    pub fn take_if_due(&mut self, to: T, now: i64) -> Option<(W, T)> {
        let due = self.waiting.values().any(|(_, t, since, cap)| *t == to && now - since >= *cap);
        if due {
            self.take(to)
        } else {
            None
        }
    }

    /// 0.15.0: tab `tab`, on screen in `win`, closed and another tab of the window is about to
    /// be shown: its view stays until then. Returns a tab kept earlier in that window, whose
    /// view can go now (the page on top is the one closed last).
    pub fn keep(&mut self, win: W, tab: T) -> Option<T> {
        self.kept.insert(win, tab).filter(|k| *k != tab)
    }

    /// The closed tab whose view is kept on screen in `win`.
    pub fn kept_tab_of(&self, win: W) -> Option<T> {
        self.kept.get(&win).copied()
    }

    /// The window a closed tab's view is kept on screen in.
    pub fn kept_in(&self, tab: T) -> Option<W> {
        self.kept.iter().find(|(_, k)| **k == tab).map(|(w, _)| *w)
    }

    /// The kept view of `tab` goes (hidden after the swap, or waited long enough): `true` if it
    /// was still kept. A switch still waiting with it on screen shows its tab at once.
    pub fn release(&mut self, tab: T) -> bool {
        let Some(win) = self.kept_in(tab) else { return false };
        self.kept.remove(&win);
        true
    }

    /// The window closes: its kept tab, if any.
    pub fn release_window(&mut self, win: W) -> Option<T> {
        self.waiting.remove(&win);
        self.kept.remove(&win)
    }
}

/// 0.15.0: the tabs closed in a batch of workspace ops that hand over to another tab of their
/// window: an `Activate` of the same window follows, and the window does not close. Only these
/// may keep their view on screen until the next tab has painted.
pub fn handed_over(ops: &[Op]) -> Vec<(WindowId, TabId)> {
    ops.iter()
        .enumerate()
        .filter_map(|(i, op)| match op {
            Op::DestroyView { win, tab } => {
                let rest = &ops[i + 1..];
                let activates = rest.iter().any(|o| matches!(o, Op::Activate { win: w, .. } if w == win));
                let closes = rest.iter().any(|o| matches!(o, Op::CloseWindow(w) if w == win));
                (activates && !closes).then_some((*win, *tab))
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tabs::Position;
    use crate::windows::Workspace;

    #[test]
    fn a_switch_in_one_window_never_cancels_another_windows_wait() {
        let mut r: Reveals<u32, u32> = Reveals::default();
        // Window 1: tab 10 → tab 11 (not painted yet): 10 stays on screen.
        assert_eq!(r.begin(1, Some(10), 11), Begin { shown: Some(10), abandoned: None });
        r.wait(1, 10, 11, 0, REVEAL_TIMEOUT_MS);
        // Window 2: a switch there (20 → 21, ready at once).
        assert_eq!(r.begin(2, Some(20), 21).shown, Some(20));
        // Window 1's tab 11 paints: it is revealed, 10 is hidden.
        assert!(r.is_waiting(11));
        assert_eq!(r.take(11), Some((1, 10)));
        assert!(!r.is_waiting(11) && r.take(11).is_none());
    }

    #[test]
    fn a_second_switch_while_waiting_replaces_the_page_on_screen() {
        let mut r: Reveals<u32, u32> = Reveals::default();
        assert_eq!(r.begin(1, Some(10), 11).shown, Some(10));
        r.wait(1, 10, 11, 0, REVEAL_TIMEOUT_MS);
        // 11 has not painted; the user switches on to 12: 10 is still the page on screen, and 11
        // (painting beneath it) goes out of sight again.
        assert_eq!(r.begin(1, Some(11), 12), Begin { shown: Some(10), abandoned: Some(11) });
        assert!(!r.is_waiting(11), "11 no longer waits");
        // Back to the tab on screen: nothing to hide but the previous active one.
        r.wait(1, 10, 12, 0, REVEAL_TIMEOUT_MS);
        assert_eq!(r.begin(1, Some(12), 10), Begin { shown: Some(12), abandoned: Some(12) });
    }

    #[test]
    fn revealed_on_first_frame_or_after_the_timeout() {
        let mut r: Reveals<u32, u32> = Reveals::default();
        // 10 → 11 at t=1000; 11 paints at t=1200: revealed then, the timeout finds nothing.
        r.begin(1, Some(10), 11);
        r.wait(1, 10, 11, 1000, REVEAL_TIMEOUT_MS);
        assert_eq!(r.waiting_since(11), Some(1000));
        assert_eq!(r.take_if_due(11, 1200), None, "not due yet");
        assert_eq!(r.take(11), Some((1, 10)));
        assert_eq!(r.waiting_since(11), None);
        assert_eq!(r.take_if_due(11, 1000 + REVEAL_TIMEOUT_MS), None);
        // 11 → 12 at t=2000; 12 never paints: shown at the timeout, 11 hidden.
        r.begin(1, Some(11), 12);
        r.wait(1, 11, 12, 2000, REVEAL_TIMEOUT_MS);
        assert_eq!(r.take_if_due(12, 2000 + REVEAL_TIMEOUT_MS - 1), None);
        assert_eq!(r.take_if_due(12, 2000 + REVEAL_TIMEOUT_MS), Some((1, 11)));
        assert!(!r.is_waiting(12));
        // An old timer of a tab that waits again later does not cut the new wait short.
        r.begin(1, Some(12), 13);
        r.wait(1, 12, 13, 3000, REVEAL_TIMEOUT_MS);
        // (back to 12, on screen: no wait; then to 13 again at t=3400)
        r.begin(1, Some(13), 12);
        r.begin(1, Some(12), 13);
        r.wait(1, 12, 13, 3400, REVEAL_TIMEOUT_MS);
        assert_eq!(r.take_if_due(13, 3000 + REVEAL_TIMEOUT_MS), None);
        assert_eq!(r.take_if_due(13, 3400 + REVEAL_TIMEOUT_MS), Some((1, 12)));
        const { assert!(REVEAL_TIMEOUT_MS <= 500) };
    }

    #[test]
    fn a_painted_tab_waits_at_most_its_short_cap_a_never_painted_one_the_long_one() {
        assert_eq!(cap_for(true), REVEAL_PAINTED_CAP_MS);
        assert_eq!(cap_for(false), REVEAL_TIMEOUT_MS);
        const { assert!(REVEAL_PAINTED_CAP_MS < REVEAL_TIMEOUT_MS) };
        let mut r: Reveals<u32, u32> = Reveals::default();
        // Out of sight (painted before): due after 150 ms.
        r.begin(1, Some(10), 11);
        r.wait(1, 10, 11, 0, cap_for(true));
        assert_eq!(r.take_if_due(11, REVEAL_PAINTED_CAP_MS - 1), None);
        assert_eq!(r.take_if_due(11, REVEAL_PAINTED_CAP_MS), Some((1, 10)));
        // Never painted (new or discarded): the 500 ms fallback, not the short cap.
        r.begin(1, Some(11), 12);
        r.wait(1, 11, 12, 1000, cap_for(false));
        assert_eq!(r.take_if_due(12, 1000 + REVEAL_PAINTED_CAP_MS), None);
        assert_eq!(r.take_if_due(12, 1000 + REVEAL_TIMEOUT_MS), Some((1, 11)));
        // No cap ever exceeds the fallback.
        r.begin(1, Some(12), 13);
        r.wait(1, 12, 13, 2000, 10_000);
        assert_eq!(r.take_if_due(13, 2000 + REVEAL_TIMEOUT_MS), Some((1, 12)));
    }

    #[test]
    fn the_shown_signal_reveals_before_the_cap_and_a_late_timer_finds_nothing() {
        let mut r: Reveals<u32, u32> = Reveals::default();
        r.begin(1, Some(10), 11);
        r.wait(1, 10, 11, 0, cap_for(true));
        // The page's "shown" (two frames drawn after it was un-hidden) at 40 ms.
        assert!(r.is_waiting(11));
        assert_eq!(r.take(11), Some((1, 10)));
        assert_eq!(r.take_if_due(11, REVEAL_PAINTED_CAP_MS), None);
        // A second signal for the same tab does nothing either.
        assert_eq!(r.take(11), None);
    }

    #[test]
    fn a_closed_tab_stays_on_screen_until_the_next_one_is_revealed() {
        let mut r: Reveals<u32, u32> = Reveals::default();
        // Tab 10 (on screen) closes; the model activates 11 with `from: None`.
        assert_eq!(r.keep(1, 10), None);
        assert_eq!(r.kept_in(10), Some(1));
        let b = r.begin(1, None, 11);
        assert_eq!(b.shown, Some(10), "the closed page is what is on screen");
        r.wait(1, 10, 11, 0, REVEAL_TIMEOUT_MS);
        // 11 paints (or the timeout passes): revealed over 10, which then goes.
        assert_eq!(r.take(11), Some((1, 10)));
        assert!(r.release(10));
        assert!(!r.release(10), "released once");
        assert_eq!(r.kept_in(10), None);
        // Nothing kept: a switch from nothing shows nothing beneath.
        assert_eq!(r.begin(1, None, 12).shown, None);
    }

    #[test]
    fn closing_again_while_waiting_keeps_the_page_on_screen() {
        let mut r: Reveals<u32, u32> = Reveals::default();
        r.keep(1, 10);
        let shown = r.begin(1, None, 11).shown.unwrap();
        r.wait(1, shown, 11, 0, REVEAL_TIMEOUT_MS);
        // 11 (still waiting, beneath 10) is closed too; the model activates 12. 11 is no page on
        // screen, so it is not kept; 10 still is.
        assert_eq!(r.shown_in(1), Some(10));
        assert_eq!(r.begin(1, None, 12), Begin { shown: Some(10), abandoned: Some(11) });
        r.wait(1, 10, 12, 0, REVEAL_TIMEOUT_MS);
        assert_eq!(r.take(12), Some((1, 10)));
        assert!(r.release(10));
        // Two quick closes without a wait: the earlier kept page goes when the next is kept.
        assert_eq!(r.keep(2, 20), None);
        assert_eq!(r.keep(2, 21), Some(20));
        assert_eq!(r.release_window(2), Some(21));
        assert_eq!(r.kept_in(21), None);
    }

    #[test]
    fn only_a_close_that_hands_over_keeps_its_view() {
        let now = 1;
        let mut ws = Workspace::new();
        let (w, a, _) = ws.open_window("https://a.example/", false, None, false, now);
        let b = ws.open(w, "https://b.example/", false, Position::End, true, now).unwrap().0;
        let c = ws.open(w, "https://c.example/", false, Position::End, false, now).unwrap().0;
        // Closing the active tab: the destroy comes first, then the next tab is activated.
        let ops = ws.close_tab(b, now);
        let destroy = ops.iter().position(|o| matches!(o, Op::DestroyView { tab, .. } if *tab == b)).unwrap();
        let activate = ops.iter().position(|o| matches!(o, Op::Activate { from: None, .. })).unwrap();
        assert!(destroy < activate, "{ops:?}");
        assert_eq!(handed_over(&ops), vec![(w, b)]);
        // A background tab (c is active now): nothing hands over.
        assert_eq!(ws.model(w).and_then(|m| m.active_id()), Some(c));
        let ops = ws.close_tab(a, now);
        assert!(handed_over(&ops).is_empty(), "{ops:?}");
        // A second window's last tab closes the window: its view simply goes.
        let (w2, d, _) = ws.open_window("https://d.example/", false, None, false, now);
        let ops = ws.close_tab(d, now);
        assert!(ops.contains(&Op::CloseWindow(w2)) && handed_over(&ops).is_empty(), "{ops:?}");
    }
}
