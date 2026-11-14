//! Background tab sleep (low memory, then suspend), optional discard, and
//! the `--mem-report` log. The decisions come from `core::memory::decide`.

use super::Browser;
use crate::ffi::procmem;
use crate::log::info;
use lb_core::memory::{decide, needs_wake, Action, MemorySample, Policy, Stage, TabState};
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use std::io::Write;

/// Timer: evaluate background tabs.
pub(super) const POWER_TIMER: usize = 7;
/// Timer: periodic `--mem-report` sample.
pub(super) const MEMREPORT_TIMER: usize = 8;
const MEMREPORT_INTERVAL_MS: u32 = 10_000;

/// Command-line options that affect memory handling.
#[derive(Debug, Clone, Copy, Default)]
pub struct PowerOptions {
    /// Hidden `--sleep-test-seconds=N`: shrink the sleep/discard timers.
    pub sleep_test_seconds: Option<u64>,
    /// `--mem-report`: log the browser's memory to `memory-report.log`.
    pub mem_report: bool,
}

impl PowerOptions {
    pub fn from_args(args: &[String]) -> PowerOptions {
        PowerOptions {
            sleep_test_seconds: args
                .iter()
                .find_map(|a| a.strip_prefix("--sleep-test-seconds=").and_then(|v| v.parse().ok())),
            mem_report: args.iter().any(|a| a == "--mem-report"),
        }
    }
}

impl Browser {
    fn power_policy(&self) -> Policy {
        Policy::from_settings(&self.settings.borrow(), self.power.sleep_test_seconds)
    }

    /// (Re)starts the tab-power timer; called at startup and when the
    /// sleep/discard settings change.
    pub(super) fn start_power_timer(&self) {
        let Some(w) = self.app() else { return };
        let p = self.power_policy();
        if p.sleep_after.is_none() && p.discard_after.is_none() {
            w.kill_timer(super::power::POWER_TIMER);
        } else {
            w.set_timer(POWER_TIMER, p.tick_ms());
        }
        if self.power.mem_report {
            w.set_timer(MEMREPORT_TIMER, MEMREPORT_INTERVAL_MS);
        }
        if let Some(n) = self.power.sleep_test_seconds {
            info!("tab sleep test mode: {n} s scale (policy {p:?})");
        }
    }

    /// Applies the memory policy to every live background tab.
    pub(super) fn power_tick(&self) {
        let now = now_ms();
        let policy = self.power_policy();
        let keep_media = self.settings.borrow().keep_media_tabs_awake;
        // Shown tabs are exempt: the active tab of every visible (not minimized) window.
        let frames = self.all_frames();
        let shown = |w| frames.iter().any(|f| f.id == w && f.window.is_visible() && !f.window.is_minimized());
        let actions: Vec<(TabId, Action)> = {
            let ws = self.ws.borrow();
            let views = self.views.borrow();
            let active = ws.visible_active_tabs(shown);
            let live_private = views.values().filter(|v| v.view.is_private()).count();
            ws.tabs()
                .filter_map(|(_, t)| {
                    let v = views.get(&t.id)?;
                    let state = TabState {
                        live: true,
                        active: active.contains(&t.id),
                        pinned: false,
                        // 0.20.1: also a call (camera or microphone in use), and media that
                        // plays without sound unless the user turned that off.
                        playing_audio: v.audio.get()
                            || v.media_use.get() != (false, false)
                            || (keep_media && v.media.get()),
                        downloading: v.downloads.get() > 0,
                        inspecting: v.devtools.get(),
                        private: t.private,
                        last_live_private: t.private && live_private == 1,
                        stage: v.stage.get(),
                        last_active: t.last_active,
                        stage_since: v.stage_since.get(),
                    };
                    decide(&policy, &state, now).map(|a| (t.id, a))
                })
                .collect()
        };
        for (id, action) in actions {
            self.apply_power(id, action, now);
        }
    }

    fn apply_power(&self, id: TabId, action: Action, now: i64) {
        let Some(c) = self.tab_view(id) else { return };
        match action {
            Action::SetLowMemory => {
                c.set_memory_low(true);
                self.set_stage(id, Stage::LowMemory, now);
                self.with_tab(id, |m, t| m.set_asleep(t, true));
                self.send_tab_update(id);
                info!("tab {id}: low memory");
            }
            Action::Suspend => {
                // The second stage does not freeze the page: in CEF 156 a
                // windowed browser never repaints after an unfreeze
                // (Page.setWebLifecycleState). It collects garbage again; the
                // engine throttles the hidden page meanwhile (since 0.10.0:
                // background tabs are hidden from it, see view.rs set_visible).
                c.set_memory_low(true);
                self.set_stage(id, Stage::Suspended, now);
                info!("tab {id}: asleep (second stage)");
                self.mem_report_line("suspend");
            }
            Action::Discard => {
                info!("tab {id}: discarded");
                let ops = match self.window_of(id) {
                    Some(win) => {
                        self.ws.borrow_mut().update(win, |m| ((), m.unload(id))).map(|(_, o)| o).unwrap_or_default()
                    }
                    None => Vec::new(),
                };
                self.apply(ops);
                self.mem_report_line("discard");
            }
        }
    }

    fn set_stage(&self, id: TabId, stage: Stage, now: i64) {
        if let Some(v) = self.views.borrow().get(&id) {
            v.stage.set(stage);
            v.stage_since.set(now);
        }
    }

    /// Activation: back to normal memory and resumed.
    pub(super) fn wake_tab(&self, id: TabId) {
        let Some((c, stage)) = self.views.borrow().get(&id).map(|v| (v.view.clone(), v.stage.get())) else {
            return;
        };
        if needs_wake(stage) {
            c.set_memory_low(false);
            self.set_stage(id, Stage::Awake, now_ms());
            info!("tab {id}: woken ({stage:?})");
        }
    }

    // ------------------------------------------------------------ --mem-report

    fn memory_sample(&self) -> MemorySample {
        let procs = procmem::browser_tree();
        let renderers = procs.iter().filter(|p| p.kind == procmem::Kind::Renderer).count();
        let tabs = self.ws.borrow().tabs().count();
        let views = self.views.borrow();
        let stages: Vec<Stage> = views.values().map(|v| v.stage.get()).collect();
        MemorySample {
            tabs,
            live: views.len(),
            asleep: stages.iter().filter(|s| **s != Stage::Awake).count(),
            suspended: stages.iter().filter(|s| **s == Stage::Suspended).count(),
            unloaded: tabs.saturating_sub(views.len()),
            processes: procs.len(),
            renderers,
            working_set: procs.iter().map(|p| p.working_set).sum(),
            private: procs.iter().map(|p| p.private).sum(),
        }
    }

    /// Appends a sample to `memory-report.log` (only with `--mem-report`).
    pub(super) fn mem_report_line(&self, label: &str) {
        if !self.power.mem_report {
            return;
        }
        let line = self.memory_sample().line(now_ms(), label);
        let path = self.dirs.root().join("memory-report.log");
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{line}");
        }
    }
}
