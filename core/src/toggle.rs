//! Guard against a popup reopening on the click that closed it.
//!
//! A toolbar button (⋮, shield, a bookmarks-bar folder) opens a popup
//! window. Pressing the mouse on the same button while the popup is open
//! deactivates the popup, which closes it before the click arrives; the
//! click would then open it again, so the button could never close it.
//! The host records which anchor's popup was closed by a loss of focus and
//! swallows the next request for that anchor within [`SUPPRESS_MS`].

/// How long after a focus-loss close a request for the same anchor is
/// taken as "the click that closed it" (a click lasts well under that).
pub const SUPPRESS_MS: u64 = 500;

#[derive(Debug, Default)]
pub struct ToggleGuard {
    closed: Option<(String, u64)>,
}

impl ToggleGuard {
    /// The popup of `anchor` was closed because its window lost activation
    /// or another view took focus (not by a choice in the popup).
    pub fn closed_by_focus_loss(&mut self, anchor: &str, now_ms: u64) {
        self.closed = Some((anchor.to_string(), now_ms));
    }

    /// Is the request to open `anchor` the click that just closed it? One
    /// request at most is swallowed per close.
    pub fn suppress(&mut self, anchor: &str, now_ms: u64) -> bool {
        let Some((closed, at)) = self.closed.take() else { return false };
        closed == anchor && now_ms.saturating_sub(at) <= SUPPRESS_MS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_clicks_alternate_open_closed_open() {
        let mut g = ToggleGuard::default();
        // Click 1: nothing was closed, the menu opens.
        assert!(!g.suppress("menu", 1000));
        // Click 2: the mousedown deactivated the popup; the click is swallowed.
        g.closed_by_focus_loss("menu", 1400);
        assert!(g.suppress("menu", 1450));
        // Click 3: no new close happened, the menu opens again.
        assert!(!g.suppress("menu", 1800));
    }

    #[test]
    fn a_late_click_opens() {
        let mut g = ToggleGuard::default();
        g.closed_by_focus_loss("shield", 1000);
        assert!(!g.suppress("shield", 1000 + SUPPRESS_MS + 1));
    }

    #[test]
    fn another_anchor_opens_and_clears_the_record() {
        let mut g = ToggleGuard::default();
        g.closed_by_focus_loss("folder:3", 1000);
        assert!(!g.suppress("menu", 1050));
        // The record is used up: a later request for the folder opens too.
        assert!(!g.suppress("folder:3", 1100));
    }
}
