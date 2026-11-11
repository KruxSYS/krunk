//! Session logs (0.15.0, `krunk://sessions`): the host side of `lb_core::sessionlog`. While
//! `settings.sessionLog.enabled` is on, a session runs from start (or turning it on) to exit (or
//! turning it off): visits come from the history path, foreground time from tab and window
//! activation (re-checked with every sample), CPU use from the engine's task manager every 5 s.
//! Events wait in memory and go to the database every 30 s and at the end, in one transaction.
//!
//! While off nothing runs: every hook returns on the `SWITCH` flag, no timer is armed and the task
//! manager is not opened (and is let go when logging is turned off).

use std::cell::{Cell, RefCell};

use lb_core::history;
use lb_core::pages::Page;
use lb_core::protocol::{Event, Request};
use lb_core::sessionlog::{self, Focus, ProcessClass, Recorder, Switch, Transition, Visit};
use lb_core::settings::Settings;
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;

use super::Browser;
use crate::ffi::cef::tasks::{self, TaskKind};
use crate::log::{info, warning};

/// The 5 s sampler (CPU use, foreground re-check).
pub(super) const SAMPLE_TIMER: usize = 34;
/// The 30 s flush (and the daily purge).
pub(super) const FLUSH_TIMER: usize = 35;

thread_local! {
    /// Logging is on: the one check every hook makes first.
    static SWITCH: Cell<Switch> = const { Cell::new(Switch::OFF) };
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// The running session.
struct State {
    rec: Recorder,
    /// The task manager's first figures after it opens are zero: the first sample is skipped.
    primed: bool,
    last_purge: i64,
}

fn on() -> bool {
    SWITCH.with(Cell::get).on()
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.borrow_mut().as_mut().map(f))
}

fn class_of(kind: TaskKind) -> ProcessClass {
    match kind {
        TaskKind::Browser => ProcessClass::Browser,
        TaskKind::Gpu => ProcessClass::Gpu,
        TaskKind::Renderer | TaskKind::Extension => ProcessClass::Renderer,
        TaskKind::Utility => ProcessClass::Other,
        TaskKind::Worker | TaskKind::Other => ProcessClass::Skip,
    }
}

impl Browser {
    /// At start: sessions a crash left open are ended, old ones purged; a session starts if
    /// logging is on.
    pub(super) fn sessionlog_init(&self) {
        let (enabled, days) = {
            let s = self.settings.borrow();
            (s.session_log.enabled, s.session_log.retention_days)
        };
        if let Ok(store) = self.store.try_borrow() {
            let repaired = store.log_repair();
            let purged = store.log_purge(now_ms(), days, None);
            match (repaired, purged) {
                (Ok(r), Ok(p)) if r + p > 0 => info!("session logs: {r} unfinished session(s) ended, {p} purged"),
                (Err(e), _) | (_, Err(e)) => warning!("session logs: {e}"),
                _ => {}
            }
        }
        if enabled {
            self.sessionlog_switch(true);
        }
    }

    /// `apply_settings`: the toggle or the retention changed.
    pub(super) fn sessionlog_settings_changed(&self, old: &Settings, new: &Settings) {
        if old.session_log.enabled != new.session_log.enabled {
            self.sessionlog_switch(new.session_log.enabled);
        }
        if new.session_log.retention_days < old.session_log.retention_days {
            self.sessionlog_purge(now_ms());
        }
    }

    fn sessionlog_switch(&self, enabled: bool) {
        let mut sw = SWITCH.with(Cell::get);
        let t = sw.set(enabled);
        SWITCH.with(|s| s.set(sw));
        match t {
            Some(Transition::Start) => self.sessionlog_start(sw),
            Some(Transition::Stop) => self.sessionlog_stop(false),
            None => {}
        }
    }

    fn sessionlog_start(&self, sw: Switch) {
        let now = now_ms();
        let id = match self.store.borrow().log_start(now) {
            Ok(id) => id,
            Err(e) => {
                warning!("session logs: could not start a session: {e}");
                SWITCH.with(|s| s.set(Switch::OFF));
                return;
            }
        };
        info!("session logs: session {id} started");
        STATE.with(|s| *s.borrow_mut() = Some(State { rec: Recorder::new(id), primed: false, last_purge: now }));
        if let Some(w) = self.app() {
            let [sample, flush] = sw.timers() else { return };
            w.set_timer(SAMPLE_TIMER, *sample);
            w.set_timer(FLUSH_TIMER, *flush);
        }
        // Opens the task manager: its first figures are zero, the next sample has real ones.
        let _ = tasks::tasks();
        self.sessionlog_refresh();
    }

    /// Ends the session: the open interval closes and everything is written. `exit`: Krunk is
    /// closing (the engine releases the task manager itself).
    fn sessionlog_stop(&self, exit: bool) {
        SWITCH.with(|s| s.set(Switch::OFF));
        if let Some(w) = self.app() {
            w.kill_timer(SAMPLE_TIMER);
            w.kill_timer(FLUSH_TIMER);
        }
        let Some(mut st) = STATE.with(|s| s.borrow_mut().take()) else { return };
        let now = now_ms();
        let batch = st.rec.finish(now);
        match self.store.try_borrow() {
            Ok(store) => match store.log_flush(st.rec.session(), &batch, Some(now)) {
                Ok(()) => info!("session logs: session {} ended", st.rec.session()),
                Err(e) => warning!("session logs: {e}"),
            },
            // Not ended here: the next start ends it at its last written event.
            Err(_) => warning!("session logs: database busy at the end of the session"),
        }
        // Stop the engine refreshing its task list every second, unless the task manager page
        // still shows it.
        let tasks_open = self
            .ws
            .try_borrow()
            .map(|ws| ws.tabs().any(|(_, t)| Page::from_url(&t.url) == Some(Page::Tasks)))
            .unwrap_or(true);
        if !exit && !tasks_open {
            tasks::release();
        }
    }

    /// `shutdown`, while the database is still open.
    pub(super) fn sessionlog_shutdown(&self) {
        self.sessionlog_stop(true);
    }

    /// A main-frame page finished loading (the history path: `url` is already cleaned, never
    /// a private tab's).
    pub(super) fn sessionlog_visit(&self, tab: TabId, url: &str, title: &str) {
        if !on() {
            return;
        }
        if let Some(host) = sessionlog::site_of(url) {
            let v = Visit { ts: now_ms(), tab, host, url: url.to_string(), title: title.to_string() };
            with_state(|s| s.rec.visit(v));
        }
        self.sessionlog_refresh();
    }

    /// Tab or window activation, minimize, screen lock: the foreground is checked again next
    /// turn (when the activation has settled).
    pub(super) fn sessionlog_focus(&self) {
        if !on() {
            return;
        }
        self.defer(|b| b.sessionlog_refresh());
    }

    /// The active tab of the current window, if Krunk is in front, not minimized, and the tab
    /// shows a web page in a normal (not private) tab.
    fn sessionlog_foreground(&self) -> Option<Focus> {
        if !crate::ffi::window::foreground_is_ours() {
            return None;
        }
        let f = self.cur()?;
        if f.window.is_minimized() {
            return None;
        }
        let t = self.active_tab()?;
        let url = history::visit_url(&t.url, t.private)?;
        Some(Focus { tab: t.id, host: sessionlog::site_of(&url)? })
    }

    fn sessionlog_refresh(&self) {
        if !on() {
            return;
        }
        let fg = self.sessionlog_foreground();
        with_state(|s| s.rec.focus(now_ms(), fg));
    }

    /// `SAMPLE_TIMER`.
    pub(super) fn sessionlog_sample(&self) {
        if !on() {
            return;
        }
        let now = now_ms();
        let sample =
            sessionlog::sample_from(now, tasks::tasks().into_iter().map(|t| (class_of(t.kind), t.cpu, t.memory)));
        with_state(|s| {
            if s.primed {
                s.rec.sample(sample);
            }
            s.primed = true;
        });
        self.sessionlog_refresh();
    }

    /// `FLUSH_TIMER`: the buffer goes to the database; once a day old sessions are purged.
    pub(super) fn sessionlog_flush(&self) {
        if !on() {
            return;
        }
        let Ok(store) = self.store.try_borrow() else { return };
        let now = now_ms();
        let Some((id, batch)) = with_state(|s| (s.rec.session(), s.rec.take(now))) else { return };
        if !batch.is_empty() {
            if let Err(e) = store.log_flush(id, &batch, None) {
                warning!("session logs: {e}");
            }
        }
        drop(store);
        let purge = with_state(|s| now - s.last_purge >= sessionlog::PURGE_EVERY_MS).unwrap_or(false);
        if purge {
            self.sessionlog_purge(now);
        }
    }

    fn sessionlog_purge(&self, now: i64) {
        let days = self.settings.borrow().session_log.retention_days;
        let current = with_state(|s| {
            s.last_purge = now;
            s.rec.session()
        });
        if let Ok(store) = self.store.try_borrow() {
            match store.log_purge(now, days, current) {
                Ok(n) if n > 0 => info!("session logs: {n} session(s) older than {days} days deleted"),
                Ok(_) => {}
                Err(e) => warning!("session logs: {e}"),
            }
        }
    }

    fn sessionlog_current(&self) -> Option<i64> {
        with_state(|s| s.rec.session())
    }

    fn sessionlog_list(&self) -> Result<serde_json::Value, lb_core::storage::DbError> {
        // The running session's latest events first.
        self.sessionlog_flush();
        let (enabled, days) = {
            let s = self.settings.borrow();
            (s.session_log.enabled, s.session_log.retention_days)
        };
        let current = self.sessionlog_current();
        let sessions = self.store.borrow().log_sessions(now_ms(), current)?;
        Ok(serde_json::json!({
            "enabled": enabled,
            "retentionDays": days,
            "current": current,
            "sessions": sessions,
        }))
    }

    /// The requests of krunk://sessions.
    pub(super) fn sessionlog_request(&self, tab: Option<TabId>, req_id: Option<u32>, request: Request) {
        match request {
            Request::SessionList => self.reply_db(tab, req_id, self.sessionlog_list()),
            Request::SessionDetail { id } => {
                let current = self.sessionlog_current();
                if current == Some(id) {
                    self.sessionlog_flush();
                }
                match self.store.borrow().log_detail(id, now_ms(), current) {
                    Ok(Some(d)) => self.reply(tab, Event::ok(req_id, d)),
                    Ok(None) => self.reply(tab, Event::err(req_id, "This session is gone")),
                    Err(e) => self.reply_db::<()>(tab, req_id, Err(e)),
                }
            }
            Request::SessionDelete { id } => {
                if self.sessionlog_current() == Some(id) {
                    return self
                        .reply(tab, Event::err(req_id, "The running session can't be deleted (Clear all empties it)"));
                }
                let r = self.store.borrow().log_delete(id);
                self.reply_db(tab, req_id, r.map(|deleted| serde_json::json!({ "deleted": deleted })));
            }
            Request::SessionClear => {
                let current = self.sessionlog_current();
                let r = self.store.borrow().log_clear(current);
                if r.is_ok() {
                    with_state(|s| s.rec.clear(now_ms()));
                    info!("session logs: cleared");
                }
                self.reply_db(tab, req_id, r.map(|n| serde_json::json!({ "deleted": n })));
            }
            Request::SessionLogSet { enabled, retention_days } => {
                let r = self.try_change_settings(|s| {
                    if let Some(e) = enabled {
                        s.session_log.enabled = e;
                    }
                    if let Some(d) = retention_days {
                        s.session_log.retention_days = d;
                    }
                });
                match r {
                    Ok(()) => self.reply_db(tab, req_id, self.sessionlog_list()),
                    Err(e) => self.reply(tab, Event::err(req_id, e)),
                }
            }
            _ => {}
        }
    }
}
