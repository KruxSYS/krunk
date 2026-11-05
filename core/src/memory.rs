//! Background-tab memory policy: when a live background tab is put into
//! low-memory mode, suspended, or discarded (its browser view closed).
//!
//! The host runs [`decide`] for every tab on a timer and applies the
//! returned action; everything here is pure so it can be unit-tested.

use crate::settings::Settings;

/// Where a live tab is in the sleep sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Stage {
    /// Running normally.
    #[default]
    Awake,
    /// `MemoryUsageTargetLevel = Low` was set.
    LowMemory,
    /// `TrySuspend` succeeded (renderer frozen; resumes on activation).
    Suspended,
}

/// What the host should do with a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    SetLowMemory,
    Suspend,
    /// Close the controller; the tab keeps its URL and reloads when activated.
    Discard,
}

/// Timing policy, in milliseconds of being in the background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    /// Low memory after this long; `None` = never sleep.
    pub sleep_after: Option<i64>,
    /// Suspend this long after the low-memory step.
    pub suspend_delay: i64,
    /// Discard after this long; `None` = discard off.
    pub discard_after: Option<i64>,
}

/// Default gap between "low memory" and "suspend".
pub const SUSPEND_DELAY_MS: i64 = 60_000;

impl Policy {
    /// From the settings. `test_seconds` (the hidden `--sleep-test-seconds=N`
    /// option) shrinks the timers for testing: sleep after N s, suspend N s
    /// later, discard (if enabled) after 4·N s.
    pub fn from_settings(s: &Settings, test_seconds: Option<u64>) -> Policy {
        match test_seconds.filter(|n| *n > 0) {
            Some(n) => {
                let n = (n.min(3600) * 1000) as i64;
                Policy {
                    sleep_after: (s.tab_sleep_minutes > 0).then_some(n),
                    suspend_delay: n,
                    discard_after: s.tab_discard_enabled.then_some(4 * n),
                }
            }
            None => Policy {
                sleep_after: (s.tab_sleep_minutes > 0).then_some(i64::from(s.tab_sleep_minutes) * 60_000),
                suspend_delay: SUSPEND_DELAY_MS,
                discard_after: s.tab_discard_enabled.then_some(i64::from(s.tab_discard_minutes) * 60_000),
            },
        }
    }

    /// How often the host should evaluate tabs.
    pub fn tick_ms(&self) -> u32 {
        let shortest = [self.sleep_after, Some(self.suspend_delay), self.discard_after]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(60_000);
        (shortest / 4).clamp(1_000, 15_000) as u32
    }
}

/// Facts about one tab, gathered by the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabState {
    /// Has a browser view (not restored-but-unloaded or already discarded).
    pub live: bool,
    pub active: bool,
    pub pinned: bool,
    pub playing_audio: bool,
    pub downloading: bool,
    /// DevTools is open for the tab (docked next to it): never sleep or discard it.
    pub inspecting: bool,
    pub private: bool,
    /// The only live private tab: discarding it would end the InPrivate
    /// session (its cookies and storage), so it is never discarded.
    pub last_live_private: bool,
    pub stage: Stage,
    /// Unix ms when the tab was last active (or opened).
    pub last_active: i64,
    /// Unix ms when the current stage was entered.
    pub stage_since: i64,
}

/// Is the tab exempt from every memory action?
pub fn exempt(t: &TabState) -> bool {
    !t.live || t.active || t.pinned || t.playing_audio || t.downloading || t.inspecting
}

/// The next action for a tab at `now`, if any.
pub fn decide(p: &Policy, t: &TabState, now: i64) -> Option<Action> {
    if exempt(t) {
        return None;
    }
    let idle = now.saturating_sub(t.last_active);
    if let Some(d) = p.discard_after {
        if idle >= d && !(t.private && t.last_live_private) {
            return Some(Action::Discard);
        }
    }
    let sleep = p.sleep_after?;
    match t.stage {
        Stage::Awake if idle >= sleep => Some(Action::SetLowMemory),
        Stage::LowMemory if idle >= sleep && now.saturating_sub(t.stage_since) >= p.suspend_delay => {
            Some(Action::Suspend)
        }
        _ => None,
    }
}

/// Does activating a tab in `stage` need a wake-up (normal memory level,
/// resume)?
pub fn needs_wake(stage: Stage) -> bool {
    stage != Stage::Awake
}

/// All processes descended from `root` (children, grandchildren, ...),
/// given `(pid, parent pid)` pairs from a process snapshot. Robust against
/// cycles caused by PID reuse.
pub fn descendants(root: u32, procs: &[(u32, u32)]) -> Vec<u32> {
    let mut out = Vec::new();
    let mut frontier = vec![root];
    let mut seen = std::collections::HashSet::from([root]);
    while let Some(p) = frontier.pop() {
        for &(pid, parent) in procs {
            if parent == p && pid != parent && seen.insert(pid) {
                out.push(pid);
                frontier.push(pid);
            }
        }
    }
    out.sort_unstable();
    out
}

/// One line of the `--mem-report` log.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemorySample {
    pub tabs: usize,
    pub live: usize,
    pub asleep: usize,
    pub suspended: usize,
    pub unloaded: usize,
    pub processes: usize,
    pub renderers: usize,
    /// Summed working set of the browser process tree, bytes.
    pub working_set: u64,
    /// Summed private bytes (commit charge), bytes.
    pub private: u64,
}

impl MemorySample {
    pub fn line(&self, now_ms: i64, label: &str) -> String {
        format!(
            "{now_ms} {label}: tabs={} live={} asleep={} suspended={} unloaded={} processes={} renderers={} working_set_mb={:.1} private_mb={:.1}",
            self.tabs,
            self.live,
            self.asleep,
            self.suspended,
            self.unloaded,
            self.processes,
            self.renderers,
            self.working_set as f64 / 1_048_576.0,
            self.private as f64 / 1_048_576.0
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: i64 = 60_000;

    fn policy() -> Policy {
        Policy { sleep_after: Some(5 * MIN), suspend_delay: MIN, discard_after: Some(30 * MIN) }
    }

    fn bg(stage: Stage, last_active: i64, stage_since: i64) -> TabState {
        TabState {
            live: true,
            active: false,
            pinned: false,
            playing_audio: false,
            downloading: false,
            inspecting: false,
            private: false,
            last_live_private: false,
            stage,
            last_active,
            stage_since,
        }
    }

    #[test]
    fn sleep_sequence() {
        let p = policy();
        let t = bg(Stage::Awake, 0, 0);
        assert_eq!(decide(&p, &t, 5 * MIN - 1), None);
        assert_eq!(decide(&p, &t, 5 * MIN), Some(Action::SetLowMemory));
        let low = bg(Stage::LowMemory, 0, 5 * MIN);
        assert_eq!(decide(&p, &low, 5 * MIN + 30_000), None, "suspend waits for the delay");
        assert_eq!(decide(&p, &low, 6 * MIN), Some(Action::Suspend));
        let susp = bg(Stage::Suspended, 0, 6 * MIN);
        assert_eq!(decide(&p, &susp, 20 * MIN), None, "nothing more until discard time");
        assert_eq!(decide(&p, &susp, 30 * MIN), Some(Action::Discard));
    }

    #[test]
    fn exemptions() {
        let p = policy();
        let late = 60 * MIN;
        for (name, t) in [
            ("active", TabState { active: true, ..bg(Stage::Awake, 0, 0) }),
            ("pinned", TabState { pinned: true, ..bg(Stage::Awake, 0, 0) }),
            ("audio", TabState { playing_audio: true, ..bg(Stage::LowMemory, 0, 0) }),
            ("download", TabState { downloading: true, ..bg(Stage::Awake, 0, 0) }),
            ("devtools open", TabState { inspecting: true, ..bg(Stage::Awake, 0, 0) }),
            ("not live", TabState { live: false, ..bg(Stage::Awake, 0, 0) }),
        ] {
            assert_eq!(decide(&p, &t, late), None, "{name}");
            assert!(exempt(&t), "{name}");
        }
    }

    #[test]
    fn discard_off_or_sleep_off() {
        let no_discard = Policy { discard_after: None, ..policy() };
        assert_eq!(decide(&no_discard, &bg(Stage::Suspended, 0, 0), 999 * MIN), None);
        let no_sleep = Policy { sleep_after: None, ..policy() };
        assert_eq!(decide(&no_sleep, &bg(Stage::Awake, 0, 0), 10 * MIN), None);
        assert_eq!(decide(&no_sleep, &bg(Stage::Awake, 0, 0), 30 * MIN), Some(Action::Discard));
    }

    #[test]
    fn last_private_tab_keeps_its_session() {
        let p = policy();
        let mut t = bg(Stage::Suspended, 0, 0);
        t.private = true;
        t.last_live_private = true;
        assert_eq!(decide(&p, &t, 60 * MIN), None, "never discards the last live private tab");
        t.last_live_private = false;
        assert_eq!(decide(&p, &t, 60 * MIN), Some(Action::Discard), "another private tab keeps the session");
        // Sleeping is fine for private tabs.
        let mut s = bg(Stage::Awake, 0, 0);
        s.private = true;
        s.last_live_private = true;
        assert_eq!(decide(&p, &s, 5 * MIN), Some(Action::SetLowMemory));
    }

    #[test]
    fn idle_is_measured_from_last_activation() {
        let p = policy();
        let t = bg(Stage::Awake, 100 * MIN, 0);
        assert_eq!(decide(&p, &t, 104 * MIN), None);
        assert_eq!(decide(&p, &t, 105 * MIN), Some(Action::SetLowMemory));
        // Clock going backwards never triggers anything.
        assert_eq!(decide(&p, &t, 0), None);
    }

    #[test]
    fn policy_from_settings_and_test_scaling() {
        let s = Settings::default();
        let p = Policy::from_settings(&s, None);
        assert_eq!(p.sleep_after, Some(5 * MIN));
        assert_eq!(p.discard_after, None, "discard is off by default");
        assert_eq!(p.suspend_delay, SUSPEND_DELAY_MS);
        let s2 = Settings { tab_discard_enabled: true, tab_discard_minutes: 30, tab_sleep_minutes: 0, ..s.clone() };
        let p2 = Policy::from_settings(&s2, None);
        assert_eq!((p2.sleep_after, p2.discard_after), (None, Some(30 * MIN)));
        let t = Policy::from_settings(&Settings { tab_discard_enabled: true, ..s.clone() }, Some(10));
        assert_eq!((t.sleep_after, t.suspend_delay, t.discard_after), (Some(10_000), 10_000, Some(40_000)));
        assert_eq!(Policy::from_settings(&s, Some(0)), p, "0 = not in test mode");
        assert!(t.tick_ms() >= 1_000 && t.tick_ms() <= 2_500);
        assert_eq!(p.tick_ms(), 15_000);
    }

    #[test]
    fn process_tree() {
        // 10 = us; 20 = an engine subprocess; 21..23 = its children; 30 = unrelated.
        let procs = [(10, 1), (20, 10), (21, 20), (22, 20), (23, 22), (30, 1), (31, 30)];
        assert_eq!(descendants(10, &procs), vec![20, 21, 22, 23]);
        assert_eq!(descendants(99, &procs), Vec::<u32>::new());
        // PID reuse can create a cycle; it must terminate.
        assert_eq!(descendants(1, &[(2, 1), (1, 2)]), vec![2]);
    }

    #[test]
    fn sample_line() {
        let s = MemorySample {
            tabs: 10,
            live: 3,
            asleep: 2,
            suspended: 1,
            unloaded: 7,
            processes: 9,
            renderers: 4,
            working_set: 300 * 1_048_576,
            private: 150 * 1_048_576,
        };
        assert_eq!(
            s.line(5, "tick"),
            "5 tick: tabs=10 live=3 asleep=2 suspended=1 unloaded=7 processes=9 renderers=4 working_set_mb=300.0 private_mb=150.0"
        );
    }

    #[test]
    fn wake() {
        assert!(!needs_wake(Stage::Awake));
        assert!(needs_wake(Stage::LowMemory));
        assert!(needs_wake(Stage::Suspended));
    }
}
