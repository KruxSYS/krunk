//! Bounce-tracker cleanup (Krunk 0.6.0 tracking protection): sites that were only ever a
//! redirect hop on the way to somewhere else (`go.tracker.example/r?u=…` → the shop) get
//! their cookies and storage cleared, daily at Standard and hourly at Strict. The IO thread
//! records top-level redirects (`NetShared::take_hops`); a timer moves them into the
//! bookkeeping (`core::protection::Bounces`, persisted in the key-value store) and runs the
//! cleanup when it is due. A site with a history entry is never cleaned: the user went there.

use super::Browser;
use crate::ffi::cef::{delete_cookies, Profile};
use crate::log::{info, warning};
use lb_core::protection::{self, Bounces};
use lb_core::storage::now_ms;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

pub(super) const BOUNCE_TIMER: usize = 17;
/// Called with the number of sites cleared once the cookies are gone.
pub(super) type CleanDone = Box<dyn FnOnce(&Browser, usize)>;
/// How often the hops are collected and the cleanup checked.
const BOUNCE_TICK_MS: u32 = 10 * 60 * 1000;
const KV_KEY: &str = "bounces";

impl Browser {
    pub(super) fn start_bounce_timer(&self) {
        if let Some(w) = self.app() {
            w.set_timer(BOUNCE_TIMER, BOUNCE_TICK_MS);
        }
    }

    fn bounces(&self) -> Bounces {
        self.store.borrow().kv_get(KV_KEY).ok().flatten().map(|j| Bounces::from_json(&j)).unwrap_or_default()
    }

    fn save_bounces(&self, b: &Bounces) {
        if let Err(e) = self.store.borrow().kv_set(KV_KEY, &b.to_json()) {
            warning!("bounce trackers: could not save the state: {e}");
        }
    }

    /// Moves the IO thread's hops into the store; returns the bookkeeping.
    fn collect_hops(&self) -> Bounces {
        let hops = self.net.take_hops();
        let mut b = self.bounces();
        let now = now_ms();
        let mut added = 0;
        for (from, to) in &hops {
            if b.record_hop(from, to, now) {
                added += 1;
            }
        }
        if added > 0 {
            info!("bounce trackers: {added} redirect hop(s) noted ({} sites watched)", b.hops.len());
            self.save_bounces(&b);
        }
        b
    }

    /// BOUNCE_TIMER: collect the hops, and clean when the level's interval has passed.
    pub(super) fn bounce_tick(&self) {
        let b = self.collect_hops();
        let p = self.settings.borrow().protection.clone();
        if p.effective().bounce_cleanup && b.cleanup_due(now_ms(), p.bounce_interval_ms()) {
            self.clean_bounces(None);
        }
    }

    /// What the settings page shows.
    pub(super) fn bounce_status(&self) -> serde_json::Value {
        let b = self.collect_hops();
        serde_json::json!({ "watched": b.hops.len(), "cleanedTotal": b.cleaned_total, "lastCleanupMs": b.last_cleanup_ms })
    }

    /// Clears the cookies and storage of every due site; `done` gets the number of sites.
    pub(super) fn clean_bounces(&self, done: Option<CleanDone>) {
        let mut b = self.collect_hops();
        let now = now_ms();
        let visited: HashSet<String> =
            protection::sites_of(&self.store.borrow().visited_origins(None).unwrap_or_default());
        let due = b.candidates(now, &visited);
        if due.is_empty() {
            b.last_cleanup_ms = now;
            self.save_bounces(&b);
            if let Some(done) = done {
                done(self, 0);
            }
            return;
        }
        info!("bounce trackers: clearing {} site(s): {}", due.len(), due.join(", "));
        let origins: Vec<String> = due
            .iter()
            .flat_map(|s| [format!("https://{s}"), format!("https://www.{s}"), format!("http://{s}")])
            .collect();
        if let Some(shell) = self.shell() {
            for origin in origins {
                shell.devtools_call(
                    "Storage.clearDataForOrigin",
                    &serde_json::json!({ "origin": origin, "storageTypes": "all" }),
                    |_| {},
                );
            }
        }
        let sites: HashSet<String> = due.iter().cloned().collect();
        let weak = self.me.clone();
        let done = Rc::new(RefCell::new(done));
        let count = due.len();
        b.cleared(&due, now);
        self.save_bounces(&b);
        delete_cookies(
            Profile::Default,
            move |c| sites.contains(&c.site()),
            move |gone| {
                info!("bounce trackers: {} cookie(s) deleted", gone.len());
                if let (Some(b), Some(done)) = (weak.upgrade(), done.borrow_mut().take()) {
                    done(&b, count);
                }
            },
        );
    }
}
