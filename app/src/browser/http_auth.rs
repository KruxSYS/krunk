//! A site's HTTP sign-in (0.8.0; the operator's report "password autofill doesn't work for nginx
//! basic auth"). The engine's own sign-in dialog is switched off (`--disable-chrome-login-prompt`)
//! so a Basic or Digest challenge comes here: Krunk shows its sign-in dialog for the active tab,
//! with the vault's logins for exactly that origin. Picking one or typing is the user's click;
//! nothing is ever sent by itself. Typed credentials that work are offered for saving through
//! the usual "Save password?" (not in private windows). Proxy challenges are refused as before.
//! No user name, password or realm is ever logged.

use super::Browser;
use crate::ffi::cef::{AuthChallenge, AuthResponder};
use crate::log::info;
use lb_core::http_auth::{accepted, clean_realm, ANSWER_WINDOW_MS};
use lb_core::protocol::{AuthLogin, PopupContent, UnlockPurpose};
use lb_core::secret::Secret;
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use lb_core::urlclean;
use std::cell::RefCell;
use zeroize::Zeroizing;

/// The open dialog's challenge.
pub(super) struct Pending {
    tab: TabId,
    origin: String,
    realm: String,
    responder: AuthResponder,
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
    pending: RefCell<Option<Pending>>,
    answered: RefCell<Option<Answered>>,
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
        {
            let open = self.http_auth.pending.borrow();
            if let Some(p) = open.as_ref() {
                if p.tab == tab && p.origin == origin && p.realm == realm {
                    // The same challenge again while its dialog is open: one dialog is enough.
                    drop(open);
                    responder.cancel();
                    return;
                }
                if p.tab != tab {
                    // Another tab's dialog is open: this request gets the server's 401 page.
                    drop(open);
                    info!("http auth: tab {tab} asked while another tab's dialog was open; refused");
                    responder.cancel();
                    return;
                }
            }
        }
        if self.active_id() != Some(tab) {
            info!("http auth: a background tab asked for a sign-in; refused (reload it to sign in)");
            responder.cancel();
            return;
        }
        let failed = self.http_auth.answered.borrow().as_ref().is_some_and(|a| {
            a.tab == tab && a.origin == origin && a.realm == realm && now_ms() - a.at < ANSWER_WINDOW_MS
        });
        if failed {
            // The last answer was refused: nothing to save.
            self.http_auth.answered.borrow_mut().take();
        }
        info!("http auth: tab {tab}: {} sign-in asked", challenge.scheme.to_ascii_lowercase());
        // A previous dialog of this tab (another origin or realm) gives way.
        let old = self.http_auth.pending.borrow_mut().replace(Pending { tab, origin, realm, responder });
        if let Some(old) = old {
            old.responder.cancel();
        }
        self.show_http_auth(failed);
    }

    fn show_http_auth(&self, failed: bool) {
        let Some((tab, origin, realm)) =
            self.http_auth.pending.borrow().as_ref().map(|p| (p.tab, p.origin.clone(), p.realm.clone()))
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
        self.show_prompt(content, n);
    }

    /// The dialog's answer: typed credentials, or a saved login of this origin.
    pub(super) fn http_auth_reply(&self, tab: TabId, username: String, password: Secret, entry_id: Option<String>) {
        let pending = self.http_auth.pending.borrow_mut().take_if(|p| p.tab == tab);
        let Some(p) = pending else { return };
        self.hide_popup();
        let (user, pass, typed) = match entry_id {
            Some(id) => {
                let vault = self.vaults.vault.borrow();
                let entry = vault.as_ref().and_then(|v| v.for_origin(&p.origin).into_iter().find(|e| e.id == id));
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
            origin: p.origin.clone(),
            realm: p.realm.clone(),
            at: now_ms(),
            typed: (typed && !private).then(|| (user.clone(), pass.clone())),
        });
        info!("http auth: tab {tab}: answered ({})", if typed { "typed" } else { "a saved login" });
        p.responder.answer(&user, &pass);
    }

    pub(super) fn http_auth_cancel(&self, tab: TabId) {
        let pending = self.http_auth.pending.borrow_mut().take_if(|p| p.tab == tab);
        if let Some(p) = pending {
            info!("http auth: tab {tab}: cancelled");
            p.responder.cancel();
        }
        self.hide_popup();
    }

    /// "Unlock to use saved passwords": the vault's unlock prompt, then the dialog again.
    pub(super) fn http_auth_unlock(&self, tab: TabId) {
        let Some(origin) = self.http_auth.pending.borrow().as_ref().filter(|p| p.tab == tab).map(|p| p.origin.clone())
        else {
            return;
        };
        *self.vaults.unlock_for.borrow_mut() = Some((tab, origin, UnlockPurpose::HttpAuth));
        self.show_unlock_prompt(None);
    }

    /// After the vault unlocked for the dialog.
    pub(super) fn after_unlock_http_auth(&self, tab: TabId) {
        if self.http_auth.pending.borrow().as_ref().is_some_and(|p| p.tab == tab) {
            self.show_http_auth(false);
        }
    }

    /// The dialog (or its unlock prompt) closed without an answer: the request is cancelled and
    /// the page shows the server's 401 page.
    pub(super) fn http_auth_dismissed(&self) {
        let pending = self.http_auth.pending.borrow_mut().take();
        if let Some(p) = pending {
            info!("http auth: tab {}: dialog closed without an answer", p.tab);
            p.responder.cancel();
        }
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
