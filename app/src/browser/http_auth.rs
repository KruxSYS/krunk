//! A site's HTTP sign-in (0.8.0; the operator's report "password autofill doesn't work for nginx
//! basic auth"). The engine's own sign-in dialog is switched off (`--disable-chrome-login-prompt`)
//! so a Basic or Digest challenge comes here: Krunk shows its sign-in dialog for the active tab,
//! with the vault's logins for exactly that origin. Picking one or typing is the user's click;
//! nothing is ever sent by itself. Typed credentials that work are offered for saving through
//! the usual "Save password?" (not in private windows). Proxy challenges are refused as before.
//! No user name, password or realm is ever logged.
//!
//! 0.15.0: the dialog belongs to its tab (`lb_core::http_auth::AuthTabs`). A tab switch, a
//! click elsewhere, another popup or another app only hides it; it shows again when its tab is
//! active again (the tab strip's key badge says which tabs wait; a click on the active tab
//! brings its dialog back). A background tab's challenge waits for its tab instead of being
//! refused. Only Cancel (or Esc), closing the tab or the tab navigating elsewhere refuse it.

use super::Browser;
use crate::ffi::cef::{AuthChallenge, AuthResponder};
use crate::log::info;
use lb_core::http_auth::{accepted, clean_realm, Arrival, AuthTabs, ANSWER_WINDOW_MS};
use lb_core::protocol::{AuthLogin, PopupContent, PopupKind, UnlockPurpose};
use lb_core::secret::Secret;
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use lb_core::urlclean;
use std::cell::{Cell, RefCell};
use zeroize::Zeroizing;

/// A challenge's answer handle, and whether it came right after a refused answer.
#[derive(Debug)]
pub(super) struct Pending {
    responder: AuthResponder,
    failed: bool,
}

/// The last answer: to tell a refused one (a new challenge right after it) and, for typed
/// credentials, what to offer to save once the page loads.
struct Answered {
    tab: TabId,
    origin: String,
    realm: String,
    at: i64,
    typed: Option<(String, Zeroizing<String>)>,
}

#[derive(Default)]
pub(super) struct HttpAuthHost {
    tabs: RefCell<AuthTabs<TabId, Pending>>,
    answered: RefCell<Option<Answered>>,
    /// The dialog went because Krunk lost the foreground to another app: it comes back when a
    /// Krunk window is activated again.
    away: Cell<bool>,
}

impl Browser {
    pub(super) fn http_auth_challenge(&self, tab: TabId, challenge: AuthChallenge, responder: AuthResponder) {
        let origin = urlclean::origin_of(&challenge.origin_url)
            .filter(|o| o.starts_with("http://") || o.starts_with("https://"));
        let Some(origin) = origin else {
            responder.cancel();
            return;
        };
        let realm = clean_realm(&challenge.realm);
        let failed = self.http_auth.answered.borrow().as_ref().is_some_and(|a| {
            a.tab == tab && a.origin == origin && a.realm == realm && now_ms() - a.at < ANSWER_WINDOW_MS
        });
        if failed {
            // The last answer was refused: nothing to save.
            self.http_auth.answered.borrow_mut().take();
        }
        let arrival = self.http_auth.tabs.borrow_mut().arrive(tab, origin, realm, Pending { responder, failed });
        match arrival {
            Arrival::Duplicate(p) => {
                // The same challenge again while its dialog is open or waiting: one is enough.
                p.responder.cancel();
                return;
            }
            Arrival::Full(p) => {
                info!("http auth: tab {tab}: too many sign-ins waiting; refused");
                p.responder.cancel();
                return;
            }
            Arrival::Open | Arrival::Queued => {}
        }
        info!("http auth: tab {tab}: {} sign-in asked", challenge.scheme.to_ascii_lowercase());
        self.send_tab_update(tab);
        self.http_auth_reshow();
    }

    /// Does `tab` wait for a sign-in (the tab strip's key badge)?
    pub(super) fn http_auth_pending(&self, tab: TabId) -> bool {
        self.http_auth.tabs.try_borrow().is_ok_and(|t| t.pending(tab))
    }

    /// Shows the dialog of the current window's active tab if it waits for a sign-in, its
    /// dialog is not on screen, no other popup is open and Krunk has the foreground.
    pub(super) fn http_auth_reshow(&self) {
        let active = self.active_id();
        let Some(tab) = active.filter(|t| self.http_auth.tabs.borrow().should_show(*t, active)) else { return };
        if self.popup_kind().is_some() || !crate::ffi::window::foreground_is_ours() {
            return;
        }
        self.show_http_auth(tab);
    }

    fn show_http_auth(&self, tab: TabId) {
        let Some((origin, realm, failed)) =
            self.http_auth.tabs.borrow().open(tab).map(|c| (c.origin.clone(), c.realm.clone(), c.payload.failed))
        else {
            return;
        };
        let logins: Vec<AuthLogin> = self
            .vaults
            .vault
            .borrow()
            .as_ref()
            .map(|v| {
                v.for_origin(&origin)
                    .into_iter()
                    .map(|e| AuthLogin { entry_id: e.id.clone(), username: e.username.clone() })
                    .collect()
            })
            .unwrap_or_default();
        let locked = self.vault_exists() && !self.vault_unlocked();
        let n = logins.len();
        let content = PopupContent::HttpAuth {
            tab_id: tab,
            insecure: origin.starts_with("http://"),
            origin,
            realm,
            logins,
            locked,
            failed,
        };
        self.http_auth.away.set(false);
        self.show_prompt(content, n);
        self.http_auth.tabs.borrow_mut().set_shown(tab);
    }

    /// The dialog's answer: typed credentials, or a saved login of this origin.
    pub(super) fn http_auth_reply(&self, tab: TabId, username: String, password: Secret, entry_id: Option<String>) {
        // Taken before hiding (a hidden dialog only goes out of sight).
        let taken = self.http_auth.tabs.borrow_mut().take(tab);
        let Some(c) = taken else { return };
        self.hide_popup();
        self.http_auth_next(tab);
        let p = c.payload;
        let (user, pass, typed) = match entry_id {
            Some(id) => {
                let vault = self.vaults.vault.borrow();
                let entry = vault.as_ref().and_then(|v| v.for_origin(&c.origin).into_iter().find(|e| e.id == id));
                let Some(e) = entry else {
                    // Locked meanwhile or not this origin's: nothing is sent.
                    drop(vault);
                    p.responder.cancel();
                    return;
                };
                (e.username.clone(), Zeroizing::new(e.password.to_string()), false)
            }
            None => (username, password.into_zeroizing(), true),
        };
        let private = self.tab(tab).is_some_and(|t| t.private);
        *self.http_auth.answered.borrow_mut() = Some(Answered {
            tab,
            origin: c.origin.clone(),
            realm: c.realm.clone(),
            at: now_ms(),
            typed: (typed && !private).then(|| (user.clone(), pass.clone())),
        });
        info!("http auth: tab {tab}: answered ({})", if typed { "typed" } else { "a saved login" });
        p.responder.answer(&user, &pass);
    }

    /// Cancel or Esc in the dialog: the only way the user refuses a sign-in.
    pub(super) fn http_auth_cancel(&self, tab: TabId) {
        let taken = self.http_auth.tabs.borrow_mut().take(tab);
        if let Some(c) = taken {
            info!("http auth: tab {tab}: cancelled");
            c.payload.responder.cancel();
        }
        self.hide_popup();
        self.http_auth_next(tab);
    }

    /// After an answer: the badge follows, and the tab's next challenge (if any) shows.
    fn http_auth_next(&self, tab: TabId) {
        self.send_tab_update(tab);
        self.defer(|b| b.http_auth_reshow());
    }

    /// "Unlock to use saved passwords": the vault's unlock prompt, then the dialog again.
    pub(super) fn http_auth_unlock(&self, tab: TabId) {
        let Some(origin) = self.http_auth.tabs.borrow().open(tab).map(|c| c.origin.clone()) else {
            return;
        };
        *self.vaults.unlock_for.borrow_mut() = Some((tab, origin, UnlockPurpose::HttpAuth));
        self.show_unlock_prompt(None);
    }

    /// After the vault unlocked for the dialog.
    pub(super) fn after_unlock_http_auth(&self, tab: TabId) {
        if self.http_auth.tabs.borrow().pending(tab) {
            self.show_http_auth(tab);
        }
    }

    /// The dialog of `tab` went out of sight without an answer (a tab switch, a click elsewhere,
    /// another popup over it): the request stays and waits for its tab.
    pub(super) fn http_auth_hidden(&self, tab: TabId) {
        if let Ok(mut t) = self.http_auth.tabs.try_borrow_mut() {
            t.hidden(tab);
        }
    }

    /// The unlock prompt the dialog opened was closed without unlocking: the dialog comes back.
    pub(super) fn http_auth_unlock_dismissed(&self) {
        self.defer(|b| b.http_auth_reshow());
    }

    /// The popup lost activation while it showed the dialog (`hide_popup_by_focus_loss`): when
    /// the foreground went to another app, the dialog comes back with Krunk's window.
    pub(super) fn http_auth_focus_lost(&self) {
        let showing = matches!(self.popup_kind(), Some(PopupKind::HttpAuth))
            || self.popup.try_borrow().is_ok_and(|p| {
                p.as_ref().is_some_and(|p| {
                    matches!(p.content, Some(PopupContent::Unlock { purpose: UnlockPurpose::HttpAuth, .. }))
                })
            });
        if showing {
            // Looked at a moment later: the activation has settled by then.
            self.defer(|b| {
                if !crate::ffi::window::foreground_is_ours() {
                    b.http_auth.away.set(true);
                }
            });
        }
    }

    /// A Krunk window was activated: a dialog that went with the foreground comes back.
    pub(super) fn http_auth_window_activated(&self) {
        if self.http_auth.away.replace(false) {
            self.defer(|b| b.http_auth_reshow());
        }
    }

    /// The tab closed or its page navigates elsewhere: its sign-ins are refused.
    pub(super) fn http_auth_tab_gone(&self, tab: TabId) {
        let (gone, was_shown) = {
            let Ok(mut t) = self.http_auth.tabs.try_borrow_mut() else { return };
            let shown = t.shown() == Some(tab);
            (t.drop_tab(tab), shown)
        };
        if gone.is_empty() {
            return;
        }
        info!("http auth: tab {tab}: closed or navigated; {} sign-in(s) refused", gone.len());
        for p in gone {
            p.responder.cancel();
        }
        let on_screen = matches!(
            self.popup.try_borrow().ok().and_then(|p| p.as_ref().and_then(|p| p.content.clone())),
            Some(PopupContent::HttpAuth { tab_id, .. }) if tab_id == tab
        );
        if was_shown || on_screen {
            self.hide_popup();
        }
        self.send_tab_update(tab);
    }

    /// The main frame loaded with `status`: after typed credentials that worked, offer to save
    /// them (the usual prompt; it knows about "never for this site" and unchanged logins).
    pub(super) fn http_auth_load_end(&self, tab: TabId, status: i32) {
        let ready = self.http_auth.answered.borrow().as_ref().is_some_and(|a| a.tab == tab);
        if !ready {
            return;
        }
        let Some(a) = self.http_auth.answered.borrow_mut().take() else { return };
        if now_ms() - a.at > ANSWER_WINDOW_MS || !accepted(status) {
            return;
        }
        let on_origin = self.tab(tab).and_then(|t| urlclean::origin_of(&t.url)).is_some_and(|o| o == a.origin);
        if let (true, Some((user, pass))) = (on_origin, a.typed) {
            self.login_submitted(tab, &a.origin, user, Secret::new(pass.to_string()));
        }
    }
}
