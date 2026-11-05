//! Hybrid logical clock for record versions (PROTOCOL §9.3): `ver` = 48-bit milliseconds ‖
//! 16-bit counter. The local clock never runs more than 24 h ahead of the wall clock, so a
//! poisoned remote version (e.g. 2^48−1) can't drag every device into the future [R13].

pub const MAX_AHEAD_MS: u64 = 24 * 60 * 60 * 1000;
const MS_MASK: u64 = (1 << 48) - 1;

pub fn encode(ms: u64, counter: u16) -> u64 {
    ((ms & MS_MASK) << 16) | u64::from(counter)
}

pub fn decode(ver: u64) -> (u64, u16) {
    (ver >> 16, (ver & 0xffff) as u16)
}

/// Orders two versions; `dev_short` breaks ties deterministically.
pub fn newer(a: (u64, u16), b: (u64, u16)) -> bool {
    a > b
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Hlc {
    last: u64,
}

impl Hlc {
    pub fn new(last: u64) -> Hlc {
        Hlc { last }
    }

    pub fn last(&self) -> u64 {
        self.last
    }

    /// A version for a local change at wall-clock `now_ms`.
    pub fn tick(&mut self, now_ms: u64) -> u64 {
        let (last_ms, last_c) = decode(self.last);
        let next = if now_ms > last_ms {
            encode(now_ms, 0)
        } else if last_c < u16::MAX {
            encode(last_ms, last_c + 1)
        } else {
            encode(last_ms + 1, 0)
        };
        self.last = next;
        next
    }

    /// Merges a received version. Returns `(version used for merging, flagged)`: a version
    /// more than 24 h ahead is clamped to now + 24 h and flagged for the log.
    pub fn observe(&mut self, remote: u64, now_ms: u64) -> (u64, bool) {
        let limit = encode(now_ms.saturating_add(MAX_AHEAD_MS), u16::MAX);
        let (used, flagged) = if remote > limit { (limit, true) } else { (remote, false) };
        if used > self.last {
            self.last = used.min(limit);
        }
        (used, flagged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_are_monotonic() {
        let mut h = Hlc::default();
        let a = h.tick(1000);
        let b = h.tick(1000);
        let c = h.tick(999);
        let d = h.tick(2000);
        assert!(a < b && b < c && c < d);
        assert_eq!(decode(b), (1000, 1));
        assert_eq!(decode(d), (2000, 0));
    }

    #[test]
    fn poisoned_versions_are_clamped() {
        let now = 1_793_000_000_000;
        let mut h = Hlc::default();
        let (used, flagged) = h.observe(u64::MAX, now);
        assert!(flagged);
        assert_eq!(decode(used).0, now + MAX_AHEAD_MS);
        // The local clock didn't jump past the limit either.
        assert!(decode(h.last()).0 <= now + MAX_AHEAD_MS);
        let (used, flagged) = h.observe(encode(now + 1000, 5), now);
        assert!(!flagged);
        assert_eq!(decode(used), (now + 1000, 5));
    }

    #[test]
    fn ties_break_on_device() {
        let v = encode(5, 0);
        assert!(newer((v, 2), (v, 1)));
        assert!(!newer((v, 1), (v, 2)));
        assert!(newer((encode(6, 0), 1), (v, 9)));
    }
}
