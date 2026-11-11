//! A site's HTTP sign-in (0.8.0): the rules of Krunk's dialog for Basic and Digest challenges.
//!
//! 0.15.0: the dialog belongs to its tab. Each tab keeps its own open challenge (and a short
//! queue behind it); switching tabs, another window or another app only hides the dialog, and
//! it comes back with its tab. A challenge is cancelled only by the user's Cancel, by closing
//! the tab or by the tab navigating elsewhere.

use std::collections::{HashMap, VecDeque};

/// How long after an answer a second challenge from the same origin and realm counts as "the
/// user name or password was not accepted", and a successful page still offers to save.
pub const ANSWER_WINDOW_MS: i64 = 60_000;

/// The server's realm as the dialog shows it: no control characters, at most 100 characters
/// ("…" when shortened). It is untrusted text; the popup puts it in as text, never as markup.
pub fn clean_realm(realm: &str) -> String {
    let clean: String = realm.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let clean = clean.trim();
    if clean.chars().count() > 100 {
        format!("{}…", clean.chars().take(99).collect::<String>())
    } else {
        clean.to_string()
    }
}

/// The page that came after an answer: anything but 401/407 means the credentials worked.
pub fn accepted(http_status: i32) -> bool {
    http_status != 401 && http_status != 407 && http_status > 0
}

/// How many challenges one tab keeps (the one shown and those waiting behind it); more are
/// refused (the request gets the server's 401 page).
pub const QUEUE_CAP: usize = 4;

/// One challenge waiting for an answer, with the host's handle to answer it (`P`).
#[derive(Debug)]
pub struct Challenge<P> {
    pub origin: String,
    pub realm: String,
    pub payload: P,
}

/// What became of a new challenge.
#[derive(Debug, PartialEq, Eq)]
pub enum Arrival<P> {
    /// It is the tab's open challenge now (the dialog can show it).
    Open,
    /// It waits behind the tab's open challenge (shown once that one is answered).
    Queued,
    /// The same origin and realm is open or waiting already: one dialog is enough, this one
    /// is refused.
    Duplicate(P),
    /// The tab has [`QUEUE_CAP`] challenges already: refused.
    Full(P),
}

/// The open sign-in challenges of every tab, and which tab's dialog is on screen. `T` is the
/// tab id, `P` the host's answer handle (its drop or cancel refuses the request).
#[derive(Debug)]
pub struct AuthTabs<T, P> {
    tabs: HashMap<T, VecDeque<Challenge<P>>>,
    shown: Option<T>,
}

impl<T, P> Default for AuthTabs<T, P> {
    fn default() -> Self {
        AuthTabs { tabs: HashMap::new(), shown: None }
    }
}

impl<T: Copy + Eq + std::hash::Hash, P> AuthTabs<T, P> {
    /// A challenge for `tab` arrives.
    pub fn arrive(&mut self, tab: T, origin: String, realm: String, payload: P) -> Arrival<P> {
        let queue = self.tabs.entry(tab).or_default();
        if queue.iter().any(|c| c.origin == origin && c.realm == realm) {
            return Arrival::Duplicate(payload);
        }
        if queue.len() >= QUEUE_CAP {
            return Arrival::Full(payload);
        }
        queue.push_back(Challenge { origin, realm, payload });
        if queue.len() == 1 {
            Arrival::Open
        } else {
            Arrival::Queued
        }
    }

    /// The tab's open challenge (the one its dialog shows).
    pub fn open(&self, tab: T) -> Option<&Challenge<P>> {
        self.tabs.get(&tab).and_then(|q| q.front())
    }

    /// Does `tab` wait for a sign-in (the tab strip's badge)?
    pub fn pending(&self, tab: T) -> bool {
        self.open(tab).is_some()
    }

    /// Every tab with a sign-in waiting.
    pub fn tabs(&self) -> impl Iterator<Item = T> + '_ {
        self.tabs.iter().filter(|(_, q)| !q.is_empty()).map(|(t, _)| *t)
    }

    /// The tab whose dialog is on screen.
    pub fn shown(&self) -> Option<T> {
        self.shown
    }

    /// The dialog of `tab` is on screen now.
    pub fn set_shown(&mut self, tab: T) {
        self.shown = Some(tab);
    }

    /// The dialog of `tab` went out of sight without an answer (a tab switch, another window or
    /// app, another popup): the challenge stays. `true` if it was on screen.
    pub fn hidden(&mut self, tab: T) -> bool {
        let was = self.shown == Some(tab);
        if was {
            self.shown = None;
        }
        was
    }

    /// Should the dialog of `tab` show now? It waits for a sign-in, is the active tab (`active`)
    /// and its dialog is not on screen already.
    pub fn should_show(&self, tab: T, active: Option<T>) -> bool {
        active == Some(tab) && self.pending(tab) && self.shown != Some(tab)
    }

    /// The user answered or cancelled the open challenge of `tab`: it is taken (the next one of
    /// the tab, if any, opens).
    pub fn take(&mut self, tab: T) -> Option<Challenge<P>> {
        let queue = self.tabs.get_mut(&tab)?;
        let c = queue.pop_front();
        if queue.is_empty() {
            self.tabs.remove(&tab);
        }
        if self.shown == Some(tab) {
            self.shown = None;
        }
        c
    }

    /// The tab closed or navigated elsewhere: all of its challenges, to refuse.
    pub fn drop_tab(&mut self, tab: T) -> Vec<P> {
        if self.shown == Some(tab) {
            self.shown = None;
        }
        self.tabs.remove(&tab).map(|q| q.into_iter().map(|c| c.payload).collect()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An answer handle.
    #[derive(Debug, PartialEq, Eq)]
    struct H(u32);

    #[test]
    fn a_tab_switch_hides_the_dialog_and_keeps_the_challenge() {
        let mut a: AuthTabs<u32, H> = AuthTabs::default();
        assert_eq!(a.arrive(1, "http://a".into(), "r".into(), H(1)), Arrival::Open);
        assert!(a.should_show(1, Some(1)));
        a.set_shown(1);
        assert!(!a.should_show(1, Some(1)), "on screen already");
        // Switch to tab 2: hidden, not cancelled; the badge stays.
        assert!(a.hidden(1));
        assert!(a.pending(1) && a.shown().is_none());
        assert!(!a.should_show(1, Some(2)), "not while another tab is active");
        // Back to tab 1: shown again with the same challenge.
        assert!(a.should_show(1, Some(1)));
        a.set_shown(1);
        assert_eq!(a.open(1).map(|c| c.payload.0), Some(1));
        // Hidden twice (focus loss, then the switch) is one hide.
        assert!(a.hidden(1) && !a.hidden(1));
        // Cancel: taken; nothing pending.
        assert_eq!(a.take(1).map(|c| c.payload), Some(H(1)));
        assert!(!a.pending(1) && a.tabs().count() == 0);
    }

    #[test]
    fn background_tabs_wait_and_tabs_are_independent() {
        let mut a: AuthTabs<u32, H> = AuthTabs::default();
        a.arrive(1, "http://a".into(), "r".into(), H(1));
        a.set_shown(1);
        // A background tab asks: it waits with its badge; tab 1's dialog stays.
        assert_eq!(a.arrive(2, "http://b".into(), "r".into(), H(2)), Arrival::Open);
        assert_eq!(a.shown(), Some(1));
        let mut t: Vec<u32> = a.tabs().collect();
        t.sort();
        assert_eq!(t, vec![1, 2]);
        // Tab 2 closes: its challenge is refused; tab 1 is untouched.
        assert_eq!(a.drop_tab(2), vec![H(2)]);
        assert!(a.pending(1) && a.shown() == Some(1));
        // Tab 1 navigates elsewhere: refused, and its dialog is no longer on screen.
        assert_eq!(a.drop_tab(1), vec![H(1)]);
        assert!(a.shown().is_none() && a.drop_tab(1).is_empty());
    }

    #[test]
    fn a_second_challenge_of_a_tab_waits_behind_the_first() {
        let mut a: AuthTabs<u32, H> = AuthTabs::default();
        a.arrive(1, "http://a".into(), "r".into(), H(1));
        // The same origin and realm again: refused (one dialog is enough).
        assert_eq!(a.arrive(1, "http://a".into(), "r".into(), H(9)), Arrival::Duplicate(H(9)));
        // Another realm: queued behind, shown after the first is answered.
        assert_eq!(a.arrive(1, "http://a".into(), "other".into(), H(2)), Arrival::Queued);
        assert_eq!(a.arrive(1, "http://a".into(), "other".into(), H(8)), Arrival::Duplicate(H(8)));
        a.set_shown(1);
        assert_eq!(a.take(1).map(|c| c.payload), Some(H(1)));
        assert_eq!(a.open(1).map(|c| c.realm.as_str()), Some("other"));
        assert!(a.should_show(1, Some(1)), "the next one shows");
        // At most QUEUE_CAP per tab.
        for i in 0..10 {
            let _ = a.arrive(1, format!("http://x{i}"), "r".into(), H(100 + i));
        }
        assert_eq!(a.arrive(1, "http://y".into(), "r".into(), H(7)), Arrival::Full(H(7)));
        assert_eq!(a.drop_tab(1).len(), QUEUE_CAP);
    }

    #[test]
    fn realms() {
        assert_eq!(clean_realm("Krunk test area"), "Krunk test area");
        assert_eq!(clean_realm("  a\u{0}b\nc\u{1b}[31m  "), "a b c [31m");
        let long = clean_realm(&"é".repeat(300));
        assert_eq!(long.chars().count(), 100);
        assert!(long.ends_with('…'));
        assert_eq!(clean_realm(""), "");
    }

    #[test]
    fn acceptance() {
        assert!(accepted(200) && accepted(302) && accepted(404) && accepted(500));
        assert!(!accepted(401) && !accepted(407) && !accepted(0));
    }
}
