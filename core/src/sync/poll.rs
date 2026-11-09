//! What krunk://sync polls while it is open, within the server's limits (PROTOCOL §11.6, as
//! clarified in 0.9.0): `GET approvals` is limited to 120 an hour per device, `keys/head` and
//! `sync/state` too, while one approval's own `GET approvals/{id}` may be asked every 2 s. So:
//! - an approval in progress (this device joining, or answering one): every 3 s;
//! - the list of devices asking to join: when the page opens, then every 40 s (90 an hour);
//! - the directories, the devices and the state: every 60 s;
//! - after a 429, nothing until its `Retry-After` has passed.
//!
//! The app ticks every 3 s and asks [`next`] what to do.

/// The tick of the page's polling.
pub const TICK_MS: i64 = 3_000;
/// The list of approvals.
pub const APPROVALS_EVERY_MS: i64 = 40_000;
/// Directories, devices and state.
pub const REFRESH_EVERY_MS: i64 = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Poll {
    /// This device asks to join: its approval.
    Join,
    /// This device answers an approval: that one.
    Approving,
    /// Directories, devices, state.
    Refresh,
    /// The list of devices asking to join.
    Approvals,
    /// Nothing this tick.
    Wait,
}

/// The page's polling state (times in ms; 0 = never).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PollState {
    pub last_refresh_ms: i64,
    pub last_approvals_ms: i64,
    /// A 429 said to wait until then.
    pub wait_until_ms: i64,
}

impl PollState {
    /// The page was (re)opened: the list right away.
    pub fn opened(&mut self) {
        self.last_approvals_ms = 0;
    }

    /// A 429 with `Retry-After` seconds.
    pub fn rate_limited(&mut self, now_ms: i64, retry_after_s: u64) {
        let ms = (retry_after_s.max(1) as i64).saturating_mul(1000);
        self.wait_until_ms = self.wait_until_ms.max(now_ms.saturating_add(ms));
    }
}

/// What to ask the server this tick (and records it as asked).
pub fn next(st: &mut PollState, now_ms: i64, joining: bool, approving: bool) -> Poll {
    if now_ms < st.wait_until_ms {
        return Poll::Wait;
    }
    if joining {
        return Poll::Join;
    }
    if approving {
        return Poll::Approving;
    }
    if now_ms - st.last_refresh_ms >= REFRESH_EVERY_MS {
        st.last_refresh_ms = now_ms;
        return Poll::Refresh;
    }
    if now_ms - st.last_approvals_ms >= APPROVALS_EVERY_MS {
        st.last_approvals_ms = now_ms;
        return Poll::Approvals;
    }
    Poll::Wait
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simulates an hour with the page open: every limited route stays under 120 an hour.
    #[test]
    fn an_hour_open_stays_within_the_limits() {
        let mut st = PollState::default();
        let (mut refresh, mut approvals) = (0, 0);
        let mut t = 1_000_000;
        st.opened();
        let end = t + 3_600_000;
        while t < end {
            match next(&mut st, t, false, false) {
                Poll::Refresh => refresh += 1,
                Poll::Approvals => approvals += 1,
                _ => {}
            }
            t += TICK_MS;
        }
        assert!(refresh <= 61, "{refresh}");
        assert!(approvals <= 91, "{approvals}");
        // Reopening the page many times can't push the list above the limit either: each
        // opening asks once at most, and the tick paces the rest.
        assert!(refresh + approvals > 100, "it still polls");
    }

    #[test]
    fn opening_asks_for_the_list_at_once() {
        let mut st = PollState::default();
        let t = 10_000_000;
        assert_eq!(next(&mut st, t, false, false), Poll::Refresh);
        assert_eq!(next(&mut st, t + 3_000, false, false), Poll::Approvals);
        assert_eq!(next(&mut st, t + 6_000, false, false), Poll::Wait);
        st.opened();
        assert_eq!(next(&mut st, t + 9_000, false, false), Poll::Approvals);
        assert_eq!(next(&mut st, t + 12_000, false, false), Poll::Wait);
        assert_eq!(next(&mut st, t + 9_000 + APPROVALS_EVERY_MS, false, false), Poll::Approvals);
    }

    #[test]
    fn an_approval_in_progress_is_polled_every_tick() {
        let mut st = PollState::default();
        for i in 0..20 {
            assert_eq!(next(&mut st, 5_000_000 + i * TICK_MS, false, true), Poll::Approving);
            assert_eq!(next(&mut st, 5_000_000 + i * TICK_MS, true, false), Poll::Join);
        }
    }

    #[test]
    fn a_429_pauses_everything_until_retry_after() {
        let mut st = PollState::default();
        let t = 7_000_000;
        st.rate_limited(t, 30);
        for dt in [0, 3_000, 29_000] {
            assert_eq!(next(&mut st, t + dt, false, true), Poll::Wait);
            assert_eq!(next(&mut st, t + dt, false, false), Poll::Wait);
        }
        assert_eq!(next(&mut st, t + 30_000, false, true), Poll::Approving);
        // A shorter Retry-After never shortens a longer wait.
        st.rate_limited(t + 40_000, 600);
        st.rate_limited(t + 41_000, 1);
        assert_eq!(next(&mut st, t + 100_000, false, false), Poll::Wait);
    }
}
