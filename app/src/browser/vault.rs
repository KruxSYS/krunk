//! Password vault on the host: setup/unlock/lock, rate limiting, auto-lock,
//! the save and fill prompts, and the passwords page.
//!
//! Secrets only ever live in `Zeroizing` buffers here. Nothing in this file
//! logs, stores in SQLite or puts into an error string any password, master
//! password or vault content.

use super::Browser;
use crate::ffi::secure::{self, Dpapi};
use crate::log::{info, warning};
use lb_core::layout;
use lb_core::login_memory;
use lb_core::pages::Page;
use lb_core::passgen;
use lb_core::protocol::{
    CardInput, CopyField, EntryInput, Event, FillChoice, PopupContent, PopupKind, Request, UnlockPurpose,
};
use lb_core::secret::Secret;
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use lb_core::urlclean;
use lb_core::vault::{self as core_vault, fill_allowed, prompt_still_valid, KdfParams, Vault, VaultError};
use std::cell::{Cell, RefCell};
use zeroize::Zeroizing;

/// A card seen at checkout (0.6.0): the number stays in a zeroizing buffer until saved.
pub(super) struct PendingCard {
    tab_id: TabId,
    origin: String,
    number: Zeroizing<String>,
    exp_month: u8,
    exp_year: u16,
    name: String,
    created: i64,
}

/// Timer: auto-lock check.
pub(super) const VAULT_TIMER: usize = 4;
/// A generated password counts as chosen for this long (sign-up forms take a while).
const GENERATED_MS: i64 = 30 * 60 * 1000;
const VAULT_CHECK_MS: u32 = 30_000;
/// Timer: clear the clipboard after a copy.
pub(super) const CLIPBOARD_TIMER: usize = 5;
pub(super) const CLIPBOARD_CLEAR_MS: u32 = 30_000;
/// A pending "Save password?" is dropped after this long.
const PENDING_SAVE_MS: i64 = 5 * 60 * 1000;

/// A submitted login waiting for the user's answer.
pub(super) struct PendingSave {
    tab_id: TabId,
    origin: String,
    username: String,
    password: Zeroizing<String>,
    created: i64,
}

/// A generated password the user chose: tab, origin, the password, when (ms).
type GeneratedPassword = (TabId, String, Zeroizing<String>, i64);

/// Vault state owned by the browser.
#[derive(Default)]
pub(super) struct VaultHost {
    pub(super) vault: RefCell<Option<Vault>>,
    pub(super) rate: RefCell<core_vault::RateLimiter>,
    /// A key derivation (create/unlock/verify/change) is running.
    pub(super) busy: Cell<bool>,
    /// A new recovery phrase shown but not yet confirmed (0.7.0).
    pub(super) recovery: RefCell<Option<super::vault_recovery::PendingPhrase>>,
    last_activity: Cell<i64>,
    pending_save: RefCell<Option<PendingSave>>,
    /// A card offered to save at checkout (0.6.0), waiting for the answer or the unlock.
    pending_card: RefCell<Option<PendingCard>>,
    /// What to do after the unlock prompt succeeds.
    pub(super) unlock_for: RefCell<Option<(TabId, String, UnlockPurpose)>>,
    /// A generated password the user chose (tab, origin, password, when): its submit is
    /// saved without asking (0.6.0).
    generated: RefCell<Option<GeneratedPassword>>,
    /// The last automatic save, for the toast's Undo: the entry and the password it replaced
    /// (`None`: the entry was new and is deleted).
    auto_saved: RefCell<Option<(String, Option<Zeroizing<String>>)>>,
    pub(super) clipboard_seq: Cell<Option<u32>>,
    /// 0.25.3: memory-only "already saved" verifiers, the logins held while the vault was
    /// locked, and the changed ones offered on their site (see `lb_core::login_memory`).
    pub(super) verifiers: RefCell<login_memory::Verifiers>,
    held: RefCell<login_memory::PendingLogins>,
    pub(super) offers: RefCell<login_memory::Offers>,
    /// Unlocked with "Remain unlocked this session" (0.11.0): no inactivity lock until quit.
    remain_this_session: Cell<bool>,
    /// The username sent on the first step of a login in steps (0.11.2): tab, origin, name,
    /// when. The password step's save uses it when its page has no username field.
    identifier: RefCell<Option<(TabId, String, String, i64)>>,
}

/// How long a login's first step (the username) counts for its password step.
const IDENTIFIER_MS: i64 = 10 * 60 * 1000;

pub(super) fn wrong_password_message(wait_ms: u64) -> String {
    if wait_ms > 0 {
        format!("Wrong master password. Try again in {} s.", wait_ms.div_ceil(1000))
    } else {
        "Wrong master password.".into()
    }
}

/// User-facing text for a vault error (contains no secrets by construction).
pub(super) fn vault_error_message(e: &VaultError) -> String {
    match e {
        VaultError::Wrap(_) => "This vault can only be opened by the Windows account (and PC) that created it.".into(),
        other => other.to_string(),
    }
}

/// A reply carrying a secret, built in a zeroizing buffer (the generic
/// `Event::ok` would leave unwiped copies in a serde_json::Value).
pub(super) fn secret_reply(req_id: Option<u32>, secret: &str, note: Option<&str>) -> Zeroizing<String> {
    let mut out = Zeroizing::new(String::with_capacity(secret.len() + note.map_or(0, str::len) + 80));
    out.push_str("{\"event\":\"reply\",\"reqId\":");
    match req_id {
        Some(id) => out.push_str(&id.to_string()),
        None => out.push_str("null"),
    }
    out.push_str(",\"ok\":true,\"data\":{\"password\":");
    push_json_string(&mut out, secret);
    if let Some(note) = note {
        out.push_str(",\"note\":");
        push_json_string(&mut out, note);
    }
    out.push_str("}}");
    out
}

/// Appends `s` as a JSON string literal without intermediate allocations.
pub(super) fn push_json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

impl Browser {
    pub(super) fn init_vault(&self) {
        *self.vaults.rate.borrow_mut() = self.store.borrow().load_rate_limiter().unwrap_or_default();
        if let Some(w) = self.app() {
            w.watch_session_lock();
        }
    }

    pub(super) fn vault_exists(&self) -> bool {
        core_vault::exists(&self.dirs.vault())
    }

    pub(super) fn vault_unlocked(&self) -> bool {
        self.vaults.vault.borrow().is_some()
    }

    pub(super) fn touch_vault(&self) {
        self.refresh_hint_index();
        self.vaults.last_activity.set(now_ms());
    }

    fn vault_status(&self) -> serde_json::Value {
        serde_json::json!({
            "exists": self.vault_exists(),
            "locked": !self.vault_unlocked(),
            "busy": self.vaults.busy.get(),
            "waitMs": self.vaults.rate.borrow().wait_ms(now_ms()),
            // 0.7.0: a recovery phrase exists (when it was made), readable while locked.
            "recovery": core_vault::recovery_in_file(&self.dirs.vault()),
            // 0.11.0: the unlock form's "Remain unlocked this session" default.
            "remain": self.settings.borrow().vault_remain_unlocked,
        })
    }

    /// Tells passwords pages and the shell that the vault state changed.
    pub(super) fn push_vault_state(&self) {
        let e = Event::VaultState { exists: self.vault_exists(), locked: !self.vault_unlocked() }.to_json();
        let tabs: Vec<TabId> = self
            .ws
            .borrow()
            .tabs()
            .filter(|(_, t)| matches!(Page::from_url(&t.url), Some(Page::Passwords | Page::Sync)))
            .map(|(_, t)| t.id)
            .collect();
        for id in tabs {
            if let Some(c) = self.tab_view(id) {
                c.post_json(&e);
            }
        }
        if let Some(s) = self.shell() {
            s.post_json(&e);
        }
        // Sync's view says whether passwords take part (0.9.0).
        self.push_sync();
    }

    pub(super) fn vault_opened(&self, v: Vault) {
        *self.vaults.vault.borrow_mut() = Some(v);
        self.refresh_hint_index();
        self.touch_vault();
        if let Some(w) = self.app() {
            w.set_timer(VAULT_TIMER, VAULT_CHECK_MS);
        }
        self.push_vault_state();
        self.sync_vault_unlocked();
        self.decide_held_logins();
    }

    /// Locks the vault: key material and decrypted entries are dropped
    /// (and wiped), pending prompts are discarded.
    pub(super) fn lock_vault(&self, reason: &str) {
        let had = self.vaults.vault.borrow_mut().take().is_some();
        self.vaults.remain_this_session.set(false);
        self.vaults.pending_save.borrow_mut().take();
        self.vaults.unlock_for.borrow_mut().take();
        // 0.25.3: the user locked on purpose (or Krunk exits): the memory-only verifiers and
        // held logins go too. An automatic lock (inactivity, Windows lock, sleep) keeps them.
        if reason == "user" || reason == "exit" {
            self.vaults.verifiers.borrow_mut().clear();
            self.vaults.held.borrow_mut().clear();
            self.vaults.offers.borrow_mut().clear();
        }
        if let Some(w) = self.app() {
            w.kill_timer(VAULT_TIMER);
        }
        if had {
            info!("vault locked ({reason})");
            let prompt_open = self.popup.borrow().as_ref().is_some_and(|p| {
                matches!(p.content.as_ref().map(PopupContent::kind), Some(PopupKind::Fill | PopupKind::Unlock))
            });
            if prompt_open {
                self.hide_popup();
            }
            self.push_vault_state();
        }
    }

    /// Periodic auto-lock check (VAULT_TIMER).
    pub(super) fn vault_timer(&self) {
        let minutes = self.settings.borrow().vault_autolock_minutes;
        let idle = now_ms() - self.vaults.last_activity.get();
        if core_vault::autolock_due(self.vault_unlocked(), self.vaults.remain_this_session.get(), idle, minutes) {
            self.lock_vault("inactivity");
        }
    }

    pub(super) fn clipboard_timer(&self) {
        if let Some(w) = self.app() {
            w.kill_timer(CLIPBOARD_TIMER);
            if let Some(seq) = self.vaults.clipboard_seq.take() {
                if secure::clear_clipboard_if(w.hwnd(), seq) {
                    info!("clipboard cleared");
                }
            }
        }
    }

    /// On exit: lock and clear a password we put on the clipboard.
    pub(super) fn shutdown_vault(&self) {
        self.clipboard_timer();
        self.lock_vault("exit");
    }

    // ------------------------------------------------------------ requests

    /// Vault requests from the passwords page (`tab`) or the popup.
    pub(super) fn dispatch_vault(&self, tab: Option<TabId>, req_id: Option<u32>, request: Request) {
        match request {
            Request::VaultStatus => self.reply(tab, Event::ok(req_id, self.vault_status())),
            Request::VaultSetup { password } => self.vault_setup(tab, req_id, password),
            Request::VaultUnlock { password, remain } => self.vault_unlock(tab, req_id, password, remain),
            Request::VaultLock => {
                self.lock_vault("user");
                self.reply(tab, Event::ok(req_id, ()));
            }
            Request::GeneratePassword { length, symbols } => match passgen::generate(length, symbols) {
                Ok(pw) => self.reply_secret(tab, req_id, &pw, None),
                Err(_) => self.reply(tab, Event::err(req_id, "Random number generator failed")),
            },
            Request::GetNeverSave => self.reply_db(tab, req_id, self.store.borrow().never_save_list()),
            Request::NeverSaveRemove { origin } => {
                self.reply_db(tab, req_id, self.store.borrow().never_save_remove(&origin))
            }
            // The recovery phrase: these work while locked (or need nothing of the vault).
            Request::VaultRecoveryCancel => {
                self.vaults.recovery.borrow_mut().take();
                self.reply(tab, Event::ok(req_id, ()));
            }
            Request::VaultRecoveryCheck { phrase } => self.recovery_check(tab, req_id, phrase),
            Request::VaultRecover { phrase, new } => self.recover_vault(tab, req_id, phrase, new),
            other => {
                // Everything else needs an unlocked vault.
                if !self.vault_unlocked() {
                    self.reply(tab, Event::err(req_id, "The vault is locked"));
                    return;
                }
                self.touch_vault();
                match other {
                    Request::VaultList => {
                        let list = self.vaults.vault.borrow().as_ref().map(Vault::summaries).unwrap_or_default();
                        self.reply(tab, Event::ok(req_id, list));
                    }
                    Request::VaultReveal { id, password } => self.vault_reveal(tab, req_id, id, password),
                    Request::VaultCopy { id, field } => self.vault_copy(tab, req_id, &id, field),
                    Request::VaultSave { entry } => self.vault_save_entry(tab, req_id, entry),
                    Request::VaultDelete { id } => {
                        let r = match self.vaults.vault.borrow_mut().as_mut() {
                            Some(v) => v.delete(&id, &Dpapi),
                            None => Err(VaultError::NotFound),
                        };
                        self.reply_vault(tab, req_id, r);
                    }
                    Request::VaultChangePassword { old, new } => self.vault_change_password(tab, req_id, old, new),
                    Request::VaultRecoveryCreate { password } => self.recovery_create(tab, req_id, password),
                    Request::VaultRecoveryConfirm { answers } => self.recovery_confirm(tab, req_id, answers),
                    Request::VaultRecoveryCopy => self.recovery_copy(tab, req_id),
                    Request::VaultRecoverySave => self.recovery_save(tab, req_id),
                    Request::VaultRecoveryRemove { password } => self.recovery_remove(tab, req_id, password),
                    Request::CardList => {
                        let list = self.vaults.vault.borrow().as_ref().map(Vault::card_summaries).unwrap_or_default();
                        self.reply(tab, Event::ok(req_id, list));
                    }
                    Request::CardSave { card } => {
                        let CardInput { id, number, exp_month, exp_year, name, nickname, address } = card;
                        let r = match self.vaults.vault.borrow_mut().as_mut() {
                            Some(v) => v.save_card(
                                id.as_deref(),
                                number.map(Secret::into_zeroizing),
                                exp_month,
                                exp_year,
                                &name,
                                &nickname,
                                address.map(Zeroizing::new),
                                now_ms(),
                                &Dpapi,
                            ),
                            None => Err(VaultError::NotFound),
                        };
                        if r.is_ok() {
                            self.touch_vault();
                        }
                        self.reply_vault(tab, req_id, r);
                    }
                    Request::CardDelete { id } => {
                        let r = match self.vaults.vault.borrow_mut().as_mut() {
                            Some(v) => v.delete_card(&id, &Dpapi),
                            None => Err(VaultError::NotFound),
                        };
                        self.reply_vault(tab, req_id, r);
                    }
                    Request::CardReveal { id, password } => self.card_reveal(tab, req_id, id, password),
                    Request::CardCopy { id } => self.card_copy(tab, req_id, &id),
                    _ => {}
                }
            }
        }
    }

    pub(super) fn reply_vault<T: serde::Serialize>(
        &self,
        tab: Option<TabId>,
        req_id: Option<u32>,
        r: Result<T, VaultError>,
    ) {
        match r {
            Ok(v) => self.reply(tab, Event::ok(req_id, v)),
            Err(e) => self.reply(tab, Event::err(req_id, vault_error_message(&e))),
        }
    }

    fn reply_secret(&self, tab: Option<TabId>, req_id: Option<u32>, secret: &str, note: Option<&str>) {
        if let Some(c) = tab.and_then(|t| self.tab_view(t)) {
            let json = secret_reply(req_id, secret, note);
            c.post_json(&json);
        }
    }

    fn vault_setup(&self, tab: Option<TabId>, req_id: Option<u32>, password: Secret) {
        if self.vault_exists() {
            self.reply(tab, Event::err(req_id, "A vault already exists"));
            return;
        }
        if self.vaults.busy.replace(true) {
            self.reply(tab, Event::err(req_id, "Busy, try again"));
            return;
        }
        let path = self.dirs.vault();
        let password = password.into_zeroizing();
        let for_sync = password.clone();
        self.spawn_job(
            "vault-setup",
            move || Vault::create(&path, &password, KdfParams::default(), &Dpapi),
            move |b, r| {
                b.vaults.busy.set(false);
                match r {
                    Ok(v) => {
                        info!("vault created");
                        b.vault_opened(v);
                        b.reply(tab, Event::ok(req_id, ()));
                        b.after_unlock();
                        b.sync_vault_password(for_sync, false);
                    }
                    Err(e) => b.reply(tab, Event::err(req_id, vault_error_message(&e))),
                }
            },
        );
    }

    /// Unlock from the passwords page (`tab`) or the unlock popup (`None`).
    /// The rate limit is enforced here, not in the UI.
    fn vault_unlock(&self, tab: Option<TabId>, req_id: Option<u32>, password: Secret, remain: Option<bool>) {
        // "Remain unlocked this session" (0.11.0): the dialog's choice, remembered as the next
        // default; a caller without the checkbox uses the remembered one.
        let remembered = self.settings.borrow().vault_remain_unlocked;
        let remain = remain.unwrap_or(remembered);
        if remain != remembered {
            self.settings.borrow_mut().vault_remain_unlocked = remain;
            let saved = self.store.borrow().save_settings(&self.settings.borrow());
            if let Err(e) = saved {
                warning!("vault: the unlock choice could not be saved: {e}");
            }
        }
        if self.vault_unlocked() {
            self.vaults.remain_this_session.set(remain);
            self.reply(tab, Event::ok(req_id, ()));
            self.after_unlock();
            return;
        }
        if !self.vault_exists() {
            self.reply(tab, Event::err(req_id, "No vault has been set up"));
            return;
        }
        let wait = self.vaults.rate.borrow().wait_ms(now_ms());
        if wait > 0 {
            let msg = format!("Too many attempts. Try again in {} s.", wait.div_ceil(1000));
            self.reply(tab, Event::err(req_id, msg.clone()));
            self.unlock_prompt_error(Some(msg), wait);
            return;
        }
        if self.vaults.busy.replace(true) {
            self.reply(tab, Event::err(req_id, "Busy, try again"));
            return;
        }
        let path = self.dirs.vault();
        let password = password.into_zeroizing();
        let for_sync = password.clone();
        self.spawn_job(
            "vault-unlock",
            move || Vault::open(&path, &password, &Dpapi),
            move |b, r| {
                b.vaults.busy.set(false);
                let now = now_ms();
                match r {
                    Ok(v) => {
                        b.vaults.rate.borrow_mut().record_success();
                        let _ = b.store.borrow().save_rate_limiter(&b.vaults.rate.borrow());
                        info!("vault unlocked{}", if remain { " for this session" } else { "" });
                        b.vaults.remain_this_session.set(remain);
                        b.vault_opened(v);
                        b.reply(tab, Event::ok(req_id, ()));
                        b.after_unlock();
                        b.sync_vault_password(for_sync, false);
                    }
                    Err(VaultError::WrongPassword) => {
                        b.vaults.rate.borrow_mut().record_failure(now);
                        let _ = b.store.borrow().save_rate_limiter(&b.vaults.rate.borrow());
                        let wait = b.vaults.rate.borrow().wait_ms(now);
                        warning!("vault unlock failed (attempt {})", b.vaults.rate.borrow().failures);
                        let msg = wrong_password_message(wait);
                        b.reply(tab, Event::err(req_id, msg.clone()));
                        b.unlock_prompt_error(Some(msg), wait);
                    }
                    Err(e) => {
                        warning!("vault unlock error: {e}");
                        let msg = vault_error_message(&e);
                        b.reply(tab, Event::err(req_id, msg.clone()));
                        b.unlock_prompt_error(Some(msg), 0);
                    }
                }
            },
        );
    }

    fn vault_reveal(&self, tab: Option<TabId>, req_id: Option<u32>, id: String, password: Secret) {
        let wait = self.vaults.rate.borrow().wait_ms(now_ms());
        if wait > 0 {
            self.reply(tab, Event::err(req_id, format!("Too many attempts. Try again in {} s.", wait.div_ceil(1000))));
            return;
        }
        let Some(verifier) = self.vaults.vault.borrow().as_ref().map(Vault::verifier) else { return };
        let password = password.into_zeroizing();
        self.spawn_job(
            "vault-verify",
            move || verifier.verify(&password),
            move |b, ok| {
                let now = now_ms();
                if !ok {
                    b.vaults.rate.borrow_mut().record_failure(now);
                    let _ = b.store.borrow().save_rate_limiter(&b.vaults.rate.borrow());
                    let wait = b.vaults.rate.borrow().wait_ms(now);
                    b.reply(tab, Event::err(req_id, wrong_password_message(wait)));
                    return;
                }
                b.vaults.rate.borrow_mut().record_success();
                let _ = b.store.borrow().save_rate_limiter(&b.vaults.rate.borrow());
                // The note is a secret too: shown only after the same re-authentication.
                let found = b
                    .vaults
                    .vault
                    .borrow()
                    .as_ref()
                    .and_then(|v| v.entry(&id).map(|e| (e.password.clone(), e.note.clone())));
                match found {
                    Some((pw, note)) => b.reply_secret(tab, req_id, &pw, Some(&note)),
                    None => b.reply(tab, Event::err(req_id, "Entry not found (or the vault was locked)")),
                }
            },
        );
    }

    fn vault_copy(&self, tab: Option<TabId>, req_id: Option<u32>, id: &str, field: CopyField) {
        let Some(w) = self.app() else { return };
        let value: Option<Zeroizing<String>> = self.vaults.vault.borrow().as_ref().and_then(|v| {
            v.entry(id).map(|e| match field {
                CopyField::Username => Zeroizing::new(e.username.clone()),
                CopyField::Password => e.password.clone(),
            })
        });
        let Some(value) = value else {
            self.reply(tab, Event::err(req_id, "Entry not found"));
            return;
        };
        match secure::copy_private(w.hwnd(), &value) {
            Some(seq) => {
                self.vaults.clipboard_seq.set(Some(seq));
                w.set_timer(CLIPBOARD_TIMER, CLIPBOARD_CLEAR_MS);
                self.reply(tab, Event::ok(req_id, ()));
            }
            None => self.reply(tab, Event::err(req_id, "Could not access the clipboard")),
        }
    }

    /// The full card number and the address, after the master password again (like a
    /// password reveal); the reply is built in a zeroizing buffer.
    fn card_reveal(&self, tab: Option<TabId>, req_id: Option<u32>, id: String, password: Secret) {
        let ok = self.vaults.vault.borrow().as_ref().is_some_and(|v| v.verify_password(&password));
        if !ok {
            self.reply(tab, Event::err(req_id, "Wrong master password."));
            return;
        }
        let card = self
            .vaults
            .vault
            .borrow()
            .as_ref()
            .and_then(|v| v.card(&id).map(|c| (c.number.clone(), c.address.clone())));
        let Some((number, address)) = card else {
            self.reply(tab, Event::err(req_id, "Card not found"));
            return;
        };
        let Some(c) = tab.and_then(|t| self.tab_view(t)) else { return };
        let mut out = Zeroizing::new(String::with_capacity(number.len() + address.len() + 96));
        out.push_str("{\"event\":\"reply\",\"reqId\":");
        match req_id {
            Some(id) => out.push_str(&id.to_string()),
            None => out.push_str("null"),
        }
        out.push_str(",\"ok\":true,\"data\":{\"number\":");
        push_json_string(&mut out, &number);
        out.push_str(",\"address\":");
        push_json_string(&mut out, &address);
        out.push_str("}}");
        c.post_json(&out);
        self.touch_vault();
    }

    fn card_copy(&self, tab: Option<TabId>, req_id: Option<u32>, id: &str) {
        let Some(w) = self.app() else { return };
        let number = self.vaults.vault.borrow().as_ref().and_then(|v| v.card(id).map(|c| c.number.clone()));
        let Some(number) = number else {
            self.reply(tab, Event::err(req_id, "Card not found"));
            return;
        };
        match secure::copy_private(w.hwnd(), &number) {
            Some(seq) => {
                self.vaults.clipboard_seq.set(Some(seq));
                w.set_timer(CLIPBOARD_TIMER, CLIPBOARD_CLEAR_MS);
                self.reply(tab, Event::ok(req_id, ()));
            }
            None => self.reply(tab, Event::err(req_id, "Could not access the clipboard")),
        }
    }

    // ------------------------------------------------------------ cards at checkout (0.6.0)

    /// A checkout form with a card number was sent on `origin`: offer to save the card
    /// (never in private tabs, not when it is stored already, only with a vault).
    pub(super) fn card_submitted(
        &self,
        tab_id: TabId,
        origin: &str,
        number: &str,
        exp_month: u8,
        exp_year: u16,
        name: String,
    ) {
        if !self.settings.borrow().offer_save_cards
            || self.active_id() != Some(tab_id)
            || self.tab(tab_id).is_some_and(|t| t.private)
            || !self.vault_exists()
        {
            return;
        }
        let Some(number) = core_vault::normalize_card_number(number) else { return };
        if self.vaults.vault.borrow().as_ref().is_some_and(|v| v.has_card_number(&number)) {
            return;
        }
        let brand = core_vault::card_brand(&number).to_string();
        let last4: String = number.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
        *self.vaults.pending_card.borrow_mut() = Some(PendingCard {
            tab_id,
            origin: origin.to_string(),
            number: Zeroizing::new(number),
            exp_month,
            exp_year,
            name,
            created: now_ms(),
        });
        info!("tab {tab_id}: card seen at checkout ({brand} •••• {last4}); offering to save");
        self.show_prompt(PopupContent::SaveCard { tab_id, origin: origin.to_string(), brand, last4 }, 0);
    }

    pub(super) fn save_card_reply(&self, tab_id: TabId, origin: &str, accept: bool) {
        let pending = self.vaults.pending_card.borrow_mut().take();
        self.hide_popup();
        let Some(p) = pending else { return };
        if !accept || p.tab_id != tab_id || p.origin != origin || now_ms() - p.created > PENDING_SAVE_MS {
            return;
        }
        if !self.vault_unlocked() {
            *self.vaults.pending_card.borrow_mut() = Some(p);
            *self.vaults.unlock_for.borrow_mut() = Some((tab_id, origin.to_string(), UnlockPurpose::SaveCard));
            self.show_unlock_prompt(None);
            return;
        }
        self.store_pending_card(p);
    }

    fn store_pending_card(&self, p: PendingCard) {
        let (month, year) = if (1..=12).contains(&p.exp_month) && (2000..=2100).contains(&p.exp_year) {
            (p.exp_month, p.exp_year)
        } else {
            // The page gave no usable expiry: the user completes it on the passwords page.
            (1, 2000 + (now_ms() / 31_557_600_000) as u16 + 1)
        };
        let r = match self.vaults.vault.borrow_mut().as_mut() {
            Some(v) => v.save_card(None, Some(p.number), month, year, &p.name, "", None, now_ms(), &Dpapi).map(|_| ()),
            None => return,
        };
        match r {
            Ok(()) => info!("card saved"),
            Err(e) => warning!("saving the card failed: {e}"),
        }
        self.touch_vault();
    }

    fn vault_save_entry(&self, tab: Option<TabId>, req_id: Option<u32>, entry: EntryInput) {
        let EntryInput { id, origin, username, password, note } = entry;
        let r = match self.vaults.vault.borrow_mut().as_mut() {
            Some(v) => v.upsert(
                id.as_deref(),
                &origin,
                &username,
                password.map(Secret::into_zeroizing),
                note.map(Zeroizing::new),
                now_ms(),
                &Dpapi,
            ),
            None => Err(VaultError::NotFound),
        };
        self.reply_vault(tab, req_id, r);
    }

    /// Re-encrypts the vault under a new master password on a worker; the
    /// vault is unavailable (locked) while that runs.
    fn vault_change_password(&self, tab: Option<TabId>, req_id: Option<u32>, old: Secret, new: Secret) {
        if self.vaults.busy.replace(true) {
            self.reply(tab, Event::err(req_id, "Busy, try again"));
            return;
        }
        let Some(mut vault) = self.vaults.vault.borrow_mut().take() else {
            self.vaults.busy.set(false);
            return;
        };
        let (old, new) = (old.into_zeroizing(), new.into_zeroizing());
        let for_sync = new.clone();
        self.spawn_job(
            "vault-rekey",
            move || {
                let r = vault.change_password(&old, &new, &Dpapi);
                (vault, r)
            },
            move |b, (vault, r)| {
                b.vaults.busy.set(false);
                *b.vaults.vault.borrow_mut() = Some(vault);
                b.touch_vault();
                match r {
                    Ok(()) => {
                        info!("master password changed");
                        b.reply(tab, Event::ok(req_id, ()));
                        b.sync_vault_password(for_sync, true);
                    }
                    Err(e) => b.reply(tab, Event::err(req_id, vault_error_message(&e))),
                }
            },
        );
    }

    // ------------------------------------------------------------ save prompt

    /// The first step of a login in steps was sent on `origin` in tab `tab_id`: the username,
    /// for the password step that follows (on this page or the next one of the same site).
    pub(super) fn login_identifier(&self, tab_id: TabId, origin: &str, username: String) {
        let name = username.trim().to_string();
        if name.is_empty() || name.chars().count() > 256 || name.chars().any(char::is_control) {
            return;
        }
        *self.vaults.identifier.borrow_mut() = Some((tab_id, origin.to_string(), name, now_ms()));
    }

    /// A login form was submitted on `origin` (the browser-reported origin
    /// of the top-level document of tab `tab_id`).
    pub(super) fn login_submitted(&self, tab_id: TabId, origin: &str, username: String, password: Secret) {
        // A password step without a username field: the name from the login's first step
        // (same tab, same site, recent).
        let username = if username.trim().is_empty() {
            let remembered = self.vaults.identifier.borrow().as_ref().and_then(|(t, o, name, when)| {
                let same_site = o == origin
                    || core_vault::registrable_domain(o)
                        .is_some_and(|d| core_vault::registrable_domain(origin) == Some(d));
                (*t == tab_id && same_site && now_ms() - when < IDENTIFIER_MS).then(|| name.clone())
            });
            remembered.unwrap_or(username)
        } else {
            username
        };
        if !self.settings.borrow().offer_save_passwords
            || self.active_id() != Some(tab_id)
            || self.tab(tab_id).is_some_and(|t| t.private)
            || self.store.borrow().never_save(origin).unwrap_or(false)
        {
            return;
        }
        // A generated password the user chose on this site: saved at once with an Undo
        // toast when the vault is open; otherwise the usual prompt.
        let generated = self.vaults.generated.borrow().as_ref().is_some_and(|(t, o, pw, when)| {
            *t == tab_id && o == origin && **pw == *password && now_ms() - when < GENERATED_MS
        });
        if generated && self.vault_unlocked() {
            self.vaults.generated.borrow_mut().take();
            self.auto_save(tab_id, origin, username, password.into_zeroizing());
            return;
        }
        // Already saved with this password: nothing to ask (0.25.3: any login of the same
        // site, the username trimmed and in any case, or no username on a password step).
        let known = self.vaults.vault.borrow().as_ref().map(|v| {
            let m = v.matches_for(origin);
            let saved = m.iter().map(|(e, _)| (e.username.as_str(), e.password.as_str()));
            let unchanged = login_memory::unchanged(saved, &username, &password);
            let u = login_memory::norm_user(&username);
            (m.iter().any(|(e, _)| login_memory::norm_user(&e.username) == u), unchanged)
        });
        let update = match known {
            Some((_, true)) => return,
            Some((update, false)) => update,
            // Locked: the memory-only verifiers of this session know it unchanged.
            None if self.vaults.verifiers.borrow().knows(origin, &username, &password) => {
                info!("login unchanged (known this session): no save prompt");
                return;
            }
            // Locked and not unlocked yet this session, on a site with saved logins: decided
            // at the next unlock, without a prompt now.
            None if self.vaults.verifiers.borrow().is_empty()
                && self.vault_exists()
                && self.site_has_logins(origin) =>
            {
                info!("login on a site with saved logins while locked: decided at the next unlock");
                self.vaults.held.borrow_mut().hold(origin, tab_id, &username, password.into_zeroizing(), now_ms());
                return;
            }
            None => false,
        };
        *self.vaults.pending_save.borrow_mut() = Some(PendingSave {
            tab_id,
            origin: origin.to_string(),
            username: username.clone(),
            password: password.into_zeroizing(),
            created: now_ms(),
        });
        let content = PopupContent::SavePassword { tab_id, origin: origin.to_string(), username, update };
        self.show_prompt(content, 0);
    }

    /// Shows a host-initiated prompt under the right end of the toolbar.
    pub(super) fn show_prompt(&self, content: PopupContent, entries: usize) {
        let Some(w) = self.window() else { return };
        let width_css = f64::from(w.client_size().0) / w.scale();
        let rect = layout::prompt_rect(content.kind(), entries, width_css);
        self.show_popup(content, Some(rect));
    }

    /// Answer to "Save password?". The prompt must still be valid: same tab,
    /// still active, still on the same origin.
    pub(super) fn save_password_reply(
        &self,
        tab_id: TabId,
        origin: &str,
        accept: bool,
        never: bool,
        username: Option<String>,
    ) {
        // Take the pending login before hiding (hiding counts as dismissal).
        let pending = self.vaults.pending_save.borrow_mut().take();
        self.hide_popup();
        let Some(p) = pending else { return };
        if p.tab_id != tab_id || p.origin != origin || now_ms() - p.created > PENDING_SAVE_MS {
            return;
        }
        let active = self.active_id();
        let active_url = self.active_tab().map(|t| t.url.clone()).unwrap_or_default();
        if !prompt_still_valid(tab_id, origin, active, &active_url) {
            info!("save prompt dropped: tab or origin changed");
            return;
        }
        if never {
            let _ = self.store.borrow().never_save_add(origin, now_ms());
            return;
        }
        if !accept {
            return;
        }
        // The username as the user left it in the prompt (0.11.2: it can be corrected there).
        let mut p = p;
        if let Some(name) = username.map(|u| u.trim().to_string()) {
            if name.chars().count() <= 256 && !name.chars().any(char::is_control) {
                p.username = name;
            }
        }
        if !self.vault_exists() {
            // Keep the login until the vault is set up on the passwords page.
            *self.vaults.pending_save.borrow_mut() = Some(p);
            *self.vaults.unlock_for.borrow_mut() = Some((tab_id, origin.to_string(), UnlockPurpose::Save));
            self.open_page(Page::Passwords);
            return;
        }
        if !self.vault_unlocked() {
            *self.vaults.pending_save.borrow_mut() = Some(p);
            *self.vaults.unlock_for.borrow_mut() = Some((tab_id, origin.to_string(), UnlockPurpose::Save));
            self.show_unlock_prompt(None);
            return;
        }
        self.store_pending(p);
    }

    /// Puts a secret on the clipboard the way a password copied on the passwords page goes
    /// there: kept out of the clipboard history and cleared after 30 s. (0.16.0: for the agent
    /// panel's password card.)
    pub(super) fn copy_secret(&self, value: &str) -> bool {
        let Some(w) = self.app() else { return false };
        match secure::copy_private(w.hwnd(), value) {
            Some(seq) => {
                self.vaults.clipboard_seq.set(Some(seq));
                w.set_timer(CLIPBOARD_TIMER, CLIPBOARD_CLEAR_MS);
                true
            }
            None => false,
        }
    }

    pub(super) fn remember_generated(&self, tab: TabId, origin: &str, password: Zeroizing<String>) {
        *self.vaults.generated.borrow_mut() = Some((tab, origin.to_string(), password, now_ms()));
    }

    /// Saves (or updates) the login and tells the shell, which shows "Saved · Undo · Edit".
    fn auto_save(&self, tab_id: TabId, origin: &str, username: String, password: Zeroizing<String>) {
        let (result, previous) = {
            let mut guard = self.vaults.vault.borrow_mut();
            let Some(v) = guard.as_mut() else { return };
            let previous = v
                .for_origin(origin)
                .into_iter()
                .find(|e| e.username == username)
                .map(|e| Zeroizing::new(e.password.to_string()));
            (v.save_login(origin, &username, password, now_ms(), &Dpapi), previous)
        };
        match result {
            Ok(id) => {
                let update = previous.is_some();
                info!("login {} automatically (generated password)", if update { "updated" } else { "saved" });
                *self.vaults.auto_saved.borrow_mut() = Some((id.clone(), previous));
                self.show_prompt(
                    PopupContent::PasswordSaved { tab_id, entry_id: id, origin: origin.to_string(), username, update },
                    0,
                );
            }
            Err(e) => warning!("saving the generated login failed: {e}"),
        }
        self.touch_vault();
    }

    /// Undo of the toast: delete the new entry, or put the previous password back.
    pub(super) fn password_saved_undo(&self, entry_id: &str) {
        let Some((id, previous)) = self.vaults.auto_saved.borrow_mut().take().filter(|(id, _)| id == entry_id) else {
            return;
        };
        let r = {
            let mut guard = self.vaults.vault.borrow_mut();
            let Some(v) = guard.as_mut() else { return };
            match previous {
                None => v.delete(&id, &Dpapi),
                Some(old) => {
                    let (origin, username) =
                        v.entry(&id).map(|e| (e.origin.clone(), e.username.clone())).unwrap_or_default();
                    v.upsert(Some(&id), &origin, &username, Some(old), None, now_ms(), &Dpapi).map(|_| ())
                }
            }
        };
        match r {
            Ok(()) => info!("automatic save undone"),
            Err(e) => warning!("undoing the save failed: {e}"),
        }
        self.touch_vault();
    }

    fn store_pending(&self, p: PendingSave) {
        let r = match self.vaults.vault.borrow_mut().as_mut() {
            Some(v) => v.save_login(&p.origin, &p.username, p.password, now_ms(), &Dpapi).map(|_| ()),
            None => return,
        };
        match r {
            Ok(()) => info!("login saved"),
            Err(e) => warning!("saving login failed: {e}"),
        }
        self.touch_vault();
    }

    // ------------------------------------------------------------ fill

    /// The key icon (shell) or the page asked to fill the active tab's login.
    /// `claimed_origin` is the sender's browser-reported origin for page
    /// requests; it must match the tab's current top-level origin.
    pub(super) fn request_fill(&self, tab_id: TabId, claimed_origin: Option<&str>) {
        let (active, url) = { (self.active_id(), self.tab(tab_id).map(|t| t.url.clone()).unwrap_or_default()) };
        if active != Some(tab_id) {
            return;
        }
        // 0.25.3: the key icon first offers a changed login decided at the last unlock.
        if claimed_origin.is_none() {
            let offer = self.vaults.offers.borrow_mut().take(&url, now_ms());
            if let Some(p) = offer {
                self.offer_update(tab_id, p);
                self.send_nav_if_active(tab_id);
                return;
            }
        }
        let Some(origin) = urlclean::origin_of(&url) else { return };
        if claimed_origin.is_some_and(|c| c != origin)
            || !(origin.starts_with("https://") || origin.starts_with("http://"))
        {
            return;
        }
        if !self.vault_exists() {
            self.show_prompt(PopupContent::Fill { tab_id, origin, entries: Vec::new() }, 1);
            return;
        }
        if !self.vault_unlocked() {
            *self.vaults.unlock_for.borrow_mut() = Some((tab_id, origin, UnlockPurpose::Fill));
            self.show_unlock_prompt(None);
            return;
        }
        self.show_fill_choices(tab_id, &origin, &url);
    }

    fn show_fill_choices(&self, tab_id: TabId, origin: &str, url: &str) {
        let entries: Vec<FillChoice> = self
            .vaults
            .vault
            .borrow()
            .as_ref()
            .map(|v| {
                v.for_origin(url)
                    .into_iter()
                    .filter(|e| fill_allowed(&e.origin, url))
                    .map(|e| FillChoice { id: e.id.clone(), username: e.username.clone() })
                    .collect()
            })
            .unwrap_or_default();
        let n = entries.len();
        self.touch_vault();
        self.show_prompt(PopupContent::Fill { tab_id, origin: origin.to_string(), entries }, n);
    }

    /// The user picked an entry in the fill prompt. Fills only if the tab is
    /// still active, still on the prompt's origin, and the entry belongs to
    /// exactly that origin.
    pub(super) fn fill_reply(&self, tab_id: TabId, origin: &str, entry_id: Option<String>) {
        self.hide_popup();
        let Some(entry_id) = entry_id else { return };
        let active = self.active_id();
        let url = self.tab(tab_id).map(|t| t.url.clone()).unwrap_or_default();
        if !prompt_still_valid(tab_id, origin, active, &url) {
            info!("fill dropped: tab or origin changed");
            return;
        }
        let message = {
            let vault = self.vaults.vault.borrow();
            let Some(e) = vault.as_ref().and_then(|v| v.entry(&entry_id)) else { return };
            if !fill_allowed(&e.origin, &url) {
                warning!("fill refused: entry origin does not match the page");
                return;
            }
            let mut m = Zeroizing::new(String::with_capacity(e.password.len() + e.username.len() + 48));
            m.push_str("{\"lbFill\":{\"username\":");
            push_json_string(&mut m, &e.username);
            m.push_str(",\"password\":");
            push_json_string(&mut m, &e.password);
            m.push_str("}}");
            m
        };
        // Posted to the main frame only (`View::post_json` sends the process
        // message to the main frame; iframes have no bridge).
        if let Some(c) = self.tab_view(tab_id) {
            c.post_json(&message);
            self.touch_vault();
        }
    }

    /// The popup was hidden while showing `content`: a hidden prompt is a
    /// dismissal, never a confirmation (0.15.0: a sign-in dialog only goes out of sight; it
    /// waits for its tab, see http_auth.rs).
    pub(super) fn prompt_dismissed(&self, content: &PopupContent) {
        match content {
            PopupContent::SavePassword { .. } => {
                self.vaults.pending_save.borrow_mut().take();
            }
            PopupContent::Suggest { .. } | PopupContent::CardFill { .. } => self.suggest_dismissed(),
            PopupContent::SaveCard { .. } => {
                self.vaults.pending_card.borrow_mut().take();
            }
            PopupContent::Permission { prompt_id, .. } => self.permission_dismissed(*prompt_id),
            PopupContent::ExtInstall { request_id, .. } => self.ext_install_dismissed(*request_id),
            PopupContent::HttpAuth { tab_id, .. } => self.http_auth_hidden(*tab_id),
            PopupContent::JsDialog { dialog_id, .. } => self.js_dialog_dismissed(*dialog_id),
            PopupContent::Unlock { purpose, .. } => {
                self.vaults.unlock_for.borrow_mut().take();
                match purpose {
                    UnlockPurpose::Save => {
                        self.vaults.pending_save.borrow_mut().take();
                    }
                    UnlockPurpose::SaveCard => {
                        self.vaults.pending_card.borrow_mut().take();
                    }
                    UnlockPurpose::Import => self.import_unlock_dismissed(),
                    UnlockPurpose::HttpAuth => self.http_auth_unlock_dismissed(),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    // ------------------------------------------------------------ unlock prompt

    pub(super) fn show_unlock_prompt(&self, error: Option<String>) {
        let Some((tab_id, origin, purpose)) = self.vaults.unlock_for.borrow().clone() else { return };
        let wait_ms = self.vaults.rate.borrow().wait_ms(now_ms());
        let remain = self.settings.borrow().vault_remain_unlocked;
        self.show_prompt(PopupContent::Unlock { tab_id, origin, purpose, error, wait_ms, remain }, 0);
    }

    /// Refreshes an open unlock prompt with an error message.
    fn unlock_prompt_error(&self, error: Option<String>, _wait_ms: u64) {
        let open = self.popup.borrow().as_ref().is_some_and(|p| matches!(p.content, Some(PopupContent::Unlock { .. })));
        if open {
            self.show_unlock_prompt(error);
        }
    }

    /// Continues a save or fill that was waiting for the vault.
    pub(super) fn after_unlock(&self) {
        let Some((tab_id, origin, purpose)) = self.vaults.unlock_for.borrow_mut().take() else { return };
        let active = self.active_id();
        let url = self.tab(tab_id).map(|t| t.url.clone()).unwrap_or_default();
        match purpose {
            UnlockPurpose::Save => {
                let pending = self.vaults.pending_save.borrow_mut().take();
                self.hide_popup();
                if let Some(p) = pending.filter(|p| p.tab_id == tab_id && p.origin == origin) {
                    // The login was submitted on `origin`; the user already
                    // confirmed. (The tab may be on another page by now.)
                    self.store_pending(p);
                }
            }
            UnlockPurpose::Fill => {
                if prompt_still_valid(tab_id, &origin, active, &url) {
                    self.show_fill_choices(tab_id, &origin, &url);
                } else {
                    self.hide_popup();
                }
            }
            UnlockPurpose::SaveCard => {
                if let Some(p) = self.vaults.pending_card.borrow_mut().take() {
                    self.store_pending_card(p);
                }
            }
            UnlockPurpose::Suggest | UnlockPurpose::CardFill => self.after_unlock_suggest(tab_id, &origin),
            UnlockPurpose::Import => self.after_unlock_import(),
            UnlockPurpose::HttpAuth => self.after_unlock_http_auth(tab_id),
        }
    }

    // ------------------------------------------------------------ logins held while locked (0.25.3)

    /// The vault was just unlocked: the logins held while it was locked are compared silently.
    /// Unchanged ones go; a changed one is offered at once when the unlock happened on its site
    /// (the active tab), else on its site's key icon.
    fn decide_held_logins(&self) {
        let now = now_ms();
        let held = self.vaults.held.borrow_mut().take_all(now);
        if held.is_empty() {
            return;
        }
        let active = self.active_id();
        let active_url = active.and_then(|t| self.tab(t)).map(|t| t.url.clone()).unwrap_or_default();
        let active_site = login_memory::site_key(&active_url);
        let mut shown = false;
        let (mut same, mut changed) = (0, 0);
        for p in held {
            let unchanged = self.vaults.vault.borrow().as_ref().is_some_and(|v| {
                let m = v.matches_for(&p.origin);
                login_memory::unchanged(
                    m.iter().map(|(e, _)| (e.username.as_str(), e.password.as_str())),
                    &p.username,
                    &p.password,
                )
            });
            if unchanged {
                same += 1;
                continue;
            }
            changed += 1;
            if !shown && active.is_some() && active_site.as_deref() == Some(p.site.as_str()) {
                shown = true;
                let tab = active.unwrap_or(p.tab);
                self.offer_update(tab, p);
            } else {
                self.vaults.offers.borrow_mut().add(p, now);
            }
        }
        info!("held logins at unlock: {same} unchanged (dropped), {changed} changed");
        if let Some(t) = active {
            self.send_nav_if_active(t);
        }
    }

    /// Asks about a held login (on its site's tab `tab`): "Save password?" / "Update password?".
    fn offer_update(&self, tab_id: TabId, p: login_memory::Pending) {
        let url = self.tab(tab_id).map(|t| t.url.clone()).unwrap_or_default();
        let Some(origin) = urlclean::origin_of(&url) else { return };
        if login_memory::site_key(&origin).as_deref() != Some(p.site.as_str()) {
            return;
        }
        let update = self.vaults.vault.borrow().as_ref().is_some_and(|v| {
            let u = login_memory::norm_user(&p.username);
            v.matches_for(&p.origin).iter().any(|(e, _)| login_memory::norm_user(&e.username) == u)
        });
        *self.vaults.pending_save.borrow_mut() = Some(PendingSave {
            tab_id,
            origin: p.origin.clone(),
            username: p.username.clone(),
            password: p.password,
            created: now_ms(),
        });
        let content = PopupContent::SavePassword { tab_id, origin: p.origin, username: p.username, update };
        self.show_prompt(content, 0);
    }

    /// Is there a changed login waiting on the page at `url` (the key icon shows it)?
    pub(super) fn has_login_offer(&self, url: &str) -> bool {
        self.vaults.offers.try_borrow().is_ok_and(|o| o.has(url, now_ms()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_strings_are_escaped() {
        let mut s = String::new();
        push_json_string(&mut s, "a\"b\\c\n\u{1}\u{2028}é");
        assert_eq!(s, r#""a\"b\\c\n\u0001\u2028é""#);
        let r = secret_reply(Some(5), "p\"w", None);
        let v: serde_json::Value = serde_json::from_str(&r).unwrap();
        assert_eq!(v["data"]["password"], "p\"w");
        assert_eq!(v["reqId"], 5);
        assert!(v["data"].get("note").is_none());
        let r = secret_reply(None, "pw", Some("PIN 1234\nline 2"));
        let v: serde_json::Value = serde_json::from_str(&r).unwrap();
        assert_eq!(v["data"]["note"], "PIN 1234\nline 2");
    }
}
