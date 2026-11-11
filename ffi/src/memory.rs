//! Background-tab policy (`core::memory`): when a tab in the background is suspended or
//! discarded. Kotlin runs a ticker every `tick_ms` and applies the actions to its Gecko sessions.
use lb_core::memory;

/// Timing policy in milliseconds of being in the background (`None` = never).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct MemoryPolicy {
    pub sleep_after: Option<i64>,
    pub suspend_delay: i64,
    pub discard_after: Option<i64>,
}

impl From<memory::Policy> for MemoryPolicy {
    fn from(p: memory::Policy) -> Self {
        MemoryPolicy { sleep_after: p.sleep_after, suspend_delay: p.suspend_delay, discard_after: p.discard_after }
    }
}

impl From<MemoryPolicy> for memory::Policy {
    fn from(p: MemoryPolicy) -> Self {
        memory::Policy { sleep_after: p.sleep_after, suspend_delay: p.suspend_delay, discard_after: p.discard_after }
    }
}

/// Where a live background tab is in the sleep sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MemoryStage {
    Awake,
    LowMemory,
    Suspended,
}

/// What Kotlin should do with a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MemoryAction {
    /// Ask the engine to shed caches (`GeckoSession.setActive(false)` is the nearest fit).
    SetLowMemory,
    /// Freeze the session (`setActive(false)`); resumes on activation.
    Suspend,
    /// Close the session, keep the URL and saved state; reload on activation.
    Discard,
}

/// One tab as the policy sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct MemoryTabState {
    pub live: bool,
    pub active: bool,
    pub pinned: bool,
    pub playing_audio: bool,
    pub downloading: bool,
    pub private: bool,
    pub stage: MemoryStage,
    /// When the tab was last active (ms).
    pub last_active: i64,
    /// When it entered `stage` (ms).
    pub stage_since: i64,
}

fn to_core(t: MemoryTabState) -> memory::TabState {
    memory::TabState {
        live: t.live,
        active: t.active,
        pinned: t.pinned,
        playing_audio: t.playing_audio,
        downloading: t.downloading,
        inspecting: false,
        private: t.private,
        last_live_private: false,
        stage: match t.stage {
            MemoryStage::Awake => memory::Stage::Awake,
            MemoryStage::LowMemory => memory::Stage::LowMemory,
            MemoryStage::Suspended => memory::Stage::Suspended,
        },
        last_active: t.last_active,
        stage_since: t.stage_since,
    }
}

/// The policy for the user's settings (tab sleep minutes, discard on/off and minutes).
#[uniffi::export]
pub fn memory_policy(tab_sleep_minutes: u32, tab_discard_enabled: bool, tab_discard_minutes: u32) -> MemoryPolicy {
    let s = lb_core::settings::Settings {
        tab_sleep_minutes,
        tab_discard_enabled,
        tab_discard_minutes,
        ..lb_core::settings::Settings::default()
    };
    memory::Policy::from_settings(&s, None).into()
}

/// How often to evaluate the policy, in ms (1-15 s depending on the timeouts).
#[uniffi::export]
pub fn memory_tick_ms(policy: MemoryPolicy) -> u32 {
    memory::Policy::from(policy).tick_ms()
}

/// The action due for a tab now, if any. Active, pinned, audible, downloading tabs are exempt.
#[uniffi::export]
pub fn memory_decide(policy: MemoryPolicy, tab: MemoryTabState, now: i64) -> Option<MemoryAction> {
    memory::decide(&memory::Policy::from(policy), &to_core(tab), now).map(|a| match a {
        memory::Action::SetLowMemory => MemoryAction::SetLowMemory,
        memory::Action::Suspend => MemoryAction::Suspend,
        memory::Action::Discard => MemoryAction::Discard,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(stage: MemoryStage, last_active: i64, stage_since: i64) -> MemoryTabState {
        MemoryTabState {
            live: true,
            active: false,
            pinned: false,
            playing_audio: false,
            downloading: false,
            private: false,
            stage,
            last_active,
            stage_since,
        }
    }

    #[test]
    fn default_policy_sleeps_after_five_minutes() {
        let p = memory_policy(5, false, 30);
        assert_eq!(p.sleep_after, Some(5 * 60_000));
        assert_eq!(p.discard_after, None);
        assert!(memory_tick_ms(p) >= 1000);
        assert_eq!(memory_decide(p, tab(MemoryStage::Awake, 0, 0), 60_000), None);
        assert_eq!(memory_decide(p, tab(MemoryStage::Awake, 0, 0), 6 * 60_000), Some(MemoryAction::SetLowMemory));
    }

    #[test]
    fn exempt_and_discard() {
        let p = memory_policy(1, true, 2);
        let mut t = tab(MemoryStage::Awake, 0, 0);
        t.playing_audio = true;
        assert_eq!(memory_decide(p, t, 10 * 60_000), None);
        let t = tab(MemoryStage::Suspended, 0, 0);
        assert_eq!(memory_decide(p, t, 10 * 60_000), Some(MemoryAction::Discard));
        assert_eq!(memory_policy(0, false, 0).sleep_after, None);
    }
}
