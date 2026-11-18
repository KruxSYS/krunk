//! 0.25.0: tabs woken ahead of a likely switch ("warm"). A warm tab is shown to the engine
//! beneath the page on screen, so a switch to it can bring it on top at once instead of waiting
//! for its renderer to draw again. At most two at a time — the tab the pointer rests on, and the
//! tab just left or the next one for Ctrl+Tab — and each only for [`WARM_MS`] after the
//! interaction, so resting CPU is unchanged (background tabs stay throttled).

/// How long a tab stays warm after the interaction that woke it.
pub const WARM_MS: i64 = 2500;

/// Why a tab is warm; one tab per slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// The pointer rests on its tab.
    Hover,
    /// The tab just left (back with a click) or the next one for Ctrl+Tab.
    Next,
}

#[derive(Debug)]
pub struct Warm<T> {
    tabs: Vec<(Slot, T, i64)>,
}

impl<T> Default for Warm<T> {
    fn default() -> Self {
        Warm { tabs: Vec::new() }
    }
}

impl<T: Copy + Eq> Warm<T> {
    /// `id` is warm in `slot` until `until`. Returns the tab that is no longer warm (the slot's
    /// previous tab, unless it is still warm in the other slot).
    pub fn set(&mut self, slot: Slot, id: T, until: i64) -> Option<T> {
        let old = self.tabs.iter().position(|(s, _, _)| *s == slot).map(|i| self.tabs.remove(i).1);
        self.tabs.push((slot, id, until));
        old.filter(|o| *o != id && !self.contains(*o))
    }

    /// The slot goes empty (the pointer left the strip): its tab, if no longer warm.
    pub fn clear(&mut self, slot: Slot) -> Option<T> {
        let old = self.tabs.iter().position(|(s, _, _)| *s == slot).map(|i| self.tabs.remove(i).1);
        old.filter(|o| !self.contains(*o))
    }

    pub fn contains(&self, id: T) -> bool {
        self.tabs.iter().any(|(_, t, _)| *t == id)
    }

    /// `id` is gone (closed, discarded) or became the active tab: no longer warm anywhere.
    pub fn forget(&mut self, id: T) {
        self.tabs.retain(|(_, t, _)| *t != id);
    }

    /// The tabs whose time is up at `now` (and are no longer warm in any slot).
    pub fn expire(&mut self, now: i64) -> Vec<T> {
        let mut out: Vec<T> = Vec::new();
        self.tabs.retain(|(_, t, until)| {
            let keep = *until > now;
            if !keep {
                out.push(*t);
            }
            keep
        });
        out.retain(|t| !self.contains(*t));
        out.dedup();
        out
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_most_one_tab_per_slot_and_two_in_all() {
        let mut w: Warm<u32> = Warm::default();
        assert_eq!(w.set(Slot::Hover, 1, 100), None);
        assert_eq!(w.set(Slot::Next, 2, 100), None);
        assert_eq!(w.set(Slot::Hover, 3, 100), Some(1), "the hover moved on: 1 cools");
        assert_eq!(w.len(), 2);
        assert!(w.contains(3) && w.contains(2) && !w.contains(1));
        // The same tab in both slots: replacing one slot keeps it warm.
        assert_eq!(w.set(Slot::Next, 3, 100), Some(2));
        assert_eq!(w.set(Slot::Hover, 4, 100), None, "3 is still warm as Next");
        assert!(w.contains(3));
    }

    #[test]
    fn tabs_cool_when_their_time_is_up() {
        let mut w: Warm<u32> = Warm::default();
        w.set(Slot::Hover, 1, 1000);
        w.set(Slot::Next, 2, 2000);
        assert!(w.expire(999).is_empty());
        assert_eq!(w.expire(1000), vec![1]);
        assert_eq!(w.expire(5000), vec![2]);
        assert!(w.is_empty());
        // A tab warm in both slots cools when the later one runs out.
        w.set(Slot::Hover, 7, 1000);
        w.set(Slot::Next, 7, 3000);
        assert!(w.expire(1500).is_empty());
        assert_eq!(w.expire(3000), vec![7]);
    }

    #[test]
    fn forget_and_clear() {
        let mut w: Warm<u32> = Warm::default();
        w.set(Slot::Hover, 1, 1000);
        w.set(Slot::Next, 2, 1000);
        w.forget(2);
        assert!(!w.contains(2));
        assert_eq!(w.clear(Slot::Hover), Some(1));
        assert_eq!(w.clear(Slot::Hover), None);
        assert!(w.is_empty());
    }
}
