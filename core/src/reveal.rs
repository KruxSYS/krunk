//! Tab switches that wait for the new tab's first paint (0.7.0, R7): the old page stays on
//! screen until then (at most a second). Kept per window (0.11.0): one shared slot let a tab
//! switch in another window cancel this window's wait, and that window then kept showing the
//! old page under a different active tab.

use std::collections::HashMap;

/// Per window: (the tab still on screen, the tab waiting to be shown). Generic over the ids.
#[derive(Debug, Default)]
pub struct Reveals<W, T> {
    waiting: HashMap<W, (T, T)>,
}

impl<W: Copy + Eq + std::hash::Hash, T: Copy + Eq> Reveals<W, T> {
    /// A switch in `win` from `from` to `to` begins: the tab whose page is on screen now (the
    /// one a still-waiting switch of this window left there, else `from`).
    pub fn begin(&mut self, win: W, from: Option<T>, to: T) -> Option<T> {
        match self.waiting.remove(&win) {
            Some((shown, _)) if shown != to => Some(shown),
            _ => from,
        }
    }

    /// `to` waits in `win` while `shown` stays on screen.
    pub fn wait(&mut self, win: W, shown: T, to: T) {
        self.waiting.insert(win, (shown, to));
    }

    /// Is `to` waiting to be shown (in any window)?
    pub fn is_waiting(&self, to: T) -> bool {
        self.waiting.values().any(|(_, t)| *t == to)
    }

    /// `to` is ready: its window and the tab to hide; it no longer waits.
    pub fn take(&mut self, to: T) -> Option<(W, T)> {
        let win = *self.waiting.iter().find(|(_, (_, t))| *t == to)?.0;
        self.waiting.remove(&win).map(|(shown, _)| (win, shown))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_switch_in_one_window_never_cancels_another_windows_wait() {
        let mut r: Reveals<u32, u32> = Reveals::default();
        // Window 1: tab 10 → tab 11 (not painted yet): 10 stays on screen.
        assert_eq!(r.begin(1, Some(10), 11), Some(10));
        r.wait(1, 10, 11);
        // Window 2: a switch there (20 → 21, ready at once).
        assert_eq!(r.begin(2, Some(20), 21), Some(20));
        // Window 1's tab 11 paints: it is revealed, 10 is hidden.
        assert!(r.is_waiting(11));
        assert_eq!(r.take(11), Some((1, 10)));
        assert!(!r.is_waiting(11) && r.take(11).is_none());
    }

    #[test]
    fn a_second_switch_while_waiting_replaces_the_page_on_screen() {
        let mut r: Reveals<u32, u32> = Reveals::default();
        assert_eq!(r.begin(1, Some(10), 11), Some(10));
        r.wait(1, 10, 11);
        // 11 has not painted; the user switches on to 12: 10 is still the page on screen.
        assert_eq!(r.begin(1, Some(11), 12), Some(10));
        assert!(!r.is_waiting(11), "11 no longer waits");
        // Back to the tab on screen: nothing to hide but the previous active one.
        r.wait(1, 10, 12);
        assert_eq!(r.begin(1, Some(12), 10), Some(12));
    }
}
