//! The vault's recovery phrase (Krunk 0.7.0, R13; the key slot is `lb_core::vault`, the words
//! `lb_core::recovery`). Making one needs the master password; the host makes the words, shows
//! them once and keeps them (zeroized on drop) until three of them are typed back or ten
//! minutes pass. A forgotten master password: the phrase opens the vault and a new password is
//! set at once. The words are never stored or logged; they leave the host only to the
//! passwords page that asked, to the clipboard (cleared) or into a file the user picks.

use super::vault::{vault_error_message, wrong_password_message, CLIPBOARD_TIMER};
use super::Browser;
use crate::ffi::secure::{self, Dpapi};
use crate::log::{info, warning};
use lb_core::protocol::{Event, RecoveryAnswer};
use lb_core::recovery::{ask_positions, Phrase, ASKED, WORDS};
use lb_core::secret::Secret;
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use lb_core::vault::{Vault, VaultError};
use zeroize::Zeroizing;

/// A shown phrase is forgotten after this long without confirmation.
const PENDING_MS: i64 = 10 * 60 * 1000;

/// A phrase shown on the passwords page, waiting for its confirmation.
pub(super) struct PendingPhrase {
    phrase: Phrase,
    /// The 1-based positions asked back, ascending.
    asked: [usize; ASKED],
    created: i64,
}

/// `{"words":[…],"confirm":[…]}` as a reply, built in a zeroizing buffer.
fn phrase_reply(req_id: Option<u32>, words: &[String], asked: &[usize; ASKED]) -> Zeroizing<String> {
    let mut out = Zeroizing::new(String::with_capacity(WORDS * 12 + 120));
    out.push_str("{\"event\":\"reply\",\"reqId\":");
    match req_id {
        Some(id) => out.push_str(&id.to_string()),
        None => out.push_str("null"),
    }
    out.push_str(",\"ok\":true,\"data\":{\"words\":[");
    for (i, w) in words.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        super::vault::push_json_string(&mut out, w);
    }
    out.push_str("],\"confirm\":[");
    out.push_str(&asked.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(","));
    out.push_str("]}}");
    out
}

/// The text of a saved phrase file.
fn phrase_file(words: &[String], created: i64) -> Zeroizing<String> {
    let mut out = Zeroizing::new(String::with_capacity(WORDS * 14 + 600));
    out.push_str("Krunk recovery phrase\r\n\r\n");
    out.push_str("These 24 words open your Krunk passwords and payment methods if you forget the\r\n");
    out.push_str("master password (krunk://passwords, \"Forgot master password?\"). Anyone with them\r\n");
    out.push_str("and your Windows account can open the vault: keep them offline and private.\r\n\r\n");
    for (i, w) in words.iter().enumerate() {
        out.push_str(&format!("{:>2}. {}\r\n", i + 1, w));
    }
    let date = chrono_like_date(created);
    out.push_str(&format!("\r\nMade on {date}. A newer phrase replaces this one.\r\n"));
    out
}

/// `YYYY-MM-DD` (UTC) of unix ms.
fn chrono_like_date(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

impl Browser {
    /// Re-checks the master password on a worker (rate limited like the unlock), then `then`.
    fn with_master_password(
        &self,
        tab: Option<TabId>,
        req_id: Option<u32>,
        password: Secret,
        then: impl FnOnce(&Browser) + 'static,
    ) {
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
                then(b);
            },
        );
    }

    /// "Create recovery phrase" (or a new one): after the master password, new words to show.
    pub(super) fn recovery_create(&self, tab: Option<TabId>, req_id: Option<u32>, password: Secret) {
        self.with_master_password(tab, req_id, password, move |b| {
            let (Ok(phrase), Some(asked)) = (Phrase::generate(), ask_positions()) else {
                b.reply(tab, Event::err(req_id, "System random number generator failed"));
                return;
            };
            let words = phrase.words();
            *b.vaults.recovery.borrow_mut() = Some(PendingPhrase { phrase, asked, created: now_ms() });
            info!("recovery phrase: new words shown");
            if let Some(c) = tab.and_then(|t| b.tab_view(t)) {
                c.post_json(&phrase_reply(req_id, &words, &asked));
            }
        });
    }

    /// The shown phrase, if still wanted.
    fn pending_phrase<T>(&self, f: impl FnOnce(&PendingPhrase) -> T) -> Option<T> {
        let mut slot = self.vaults.recovery.borrow_mut();
        if slot.as_ref().is_some_and(|p| now_ms() - p.created > PENDING_MS) {
            slot.take();
        }
        slot.as_ref().map(f)
    }

    /// The words typed back: right ones make the phrase the vault's.
    pub(super) fn recovery_confirm(&self, tab: Option<TabId>, req_id: Option<u32>, answers: Vec<RecoveryAnswer>) {
        let verdict = self.pending_phrase(|p| {
            if answers.len() != ASKED || answers.iter().map(|a| a.position as usize).ne(p.asked.iter().copied()) {
                return Err("Type the words asked for".to_string());
            }
            match answers.iter().find(|a| !p.phrase.word_is(a.position as usize, &a.word)) {
                Some(a) => Err(format!("Word {} is not right", a.position)),
                None => Ok(()),
            }
        });
        match verdict {
            None => {
                self.reply(tab, Event::err(req_id, "The phrase was shown too long ago; make a new one"));
                return;
            }
            Some(Err(why)) => {
                self.reply(tab, Event::err(req_id, why));
                return;
            }
            Some(Ok(())) => {}
        }
        if self.vaults.busy.replace(true) {
            self.reply(tab, Event::err(req_id, "Busy, try again"));
            return;
        }
        let Some(pending) = self.vaults.recovery.borrow_mut().take() else {
            self.vaults.busy.set(false);
            return;
        };
        let Some(mut vault) = self.vaults.vault.borrow_mut().take() else {
            self.vaults.busy.set(false);
            self.reply(tab, Event::err(req_id, "The vault is locked"));
            return;
        };
        let now = now_ms();
        self.spawn_job(
            "vault-recovery",
            move || {
                let r = vault.set_recovery(&pending.phrase, now, &Dpapi);
                (vault, r)
            },
            move |b, (vault, r)| {
                b.vaults.busy.set(false);
                *b.vaults.vault.borrow_mut() = Some(vault);
                b.touch_vault();
                match r {
                    Ok(()) => {
                        info!("recovery phrase set");
                        b.reply(tab, Event::ok(req_id, serde_json::json!({ "created": now })));
                        b.push_vault_state();
                    }
                    Err(e) => {
                        warning!("recovery phrase: {e}");
                        b.reply(tab, Event::err(req_id, vault_error_message(&e)));
                    }
                }
            },
        );
    }

    /// The shown phrase to the clipboard (kept out of clipboard history; cleared like a copied
    /// password).
    pub(super) fn recovery_copy(&self, tab: Option<TabId>, req_id: Option<u32>) {
        let Some(w) = self.app() else { return };
        let Some(text) = self.pending_phrase(|p| p.phrase.text()) else {
            self.reply(tab, Event::err(req_id, "No phrase to copy"));
            return;
        };
        match secure::copy_private(w.hwnd(), &text) {
            Some(seq) => {
                self.vaults.clipboard_seq.set(Some(seq));
                w.set_timer(CLIPBOARD_TIMER, super::vault::CLIPBOARD_CLEAR_MS);
                self.reply(tab, Event::ok(req_id, ()));
            }
            None => self.reply(tab, Event::err(req_id, "Could not access the clipboard")),
        }
    }

    /// The shown phrase into a text file the user picks.
    pub(super) fn recovery_save(&self, tab: Option<TabId>, req_id: Option<u32>) {
        let Some(content) = self.pending_phrase(|p| phrase_file(&p.phrase.words(), p.created)) else {
            self.reply(tab, Event::err(req_id, "No phrase to save"));
            return;
        };
        let Some(w) = self.window() else { return };
        let Some(path) = crate::ffi::dialogs::save_file(
            w.hwnd(),
            "Save recovery phrase",
            "Krunk recovery phrase.txt",
            ("Text files", "*.txt"),
            None,
        ) else {
            self.reply(tab, Event::ok(req_id, serde_json::json!({ "saved": false })));
            return;
        };
        match std::fs::write(&path, content.as_bytes()) {
            Ok(()) => {
                info!("recovery phrase saved to a file");
                self.reply(tab, Event::ok(req_id, serde_json::json!({ "saved": true })));
            }
            Err(e) => self.reply(tab, Event::err(req_id, format!("Could not write the file ({})", e.kind()))),
        }
    }

    /// Removes the recovery phrase, after the master password.
    pub(super) fn recovery_remove(&self, tab: Option<TabId>, req_id: Option<u32>, password: Secret) {
        self.with_master_password(tab, req_id, password, move |b| {
            let r = match b.vaults.vault.borrow_mut().as_mut() {
                Some(v) => v.remove_recovery(&Dpapi),
                None => Err(VaultError::NotFound),
            };
            if r.is_ok() {
                info!("recovery phrase removed");
            }
            b.reply_vault(tab, req_id, r);
            b.push_vault_state();
        });
    }

    /// Is the typed text a recovery phrase? (No vault involved: the page's word check.)
    pub(super) fn recovery_check(&self, tab: Option<TabId>, req_id: Option<u32>, phrase: Secret) {
        let reply = match Phrase::parse(&phrase) {
            Ok(_) => serde_json::json!({ "ok": true }),
            Err(e) => {
                let unknown = match &e {
                    lb_core::recovery::PhraseError::UnknownWords(w) => w.clone(),
                    _ => Vec::new(),
                };
                serde_json::json!({ "ok": false, "message": e.to_string(), "unknown": unknown })
            }
        };
        self.reply(tab, Event::ok(req_id, reply));
    }

    /// "Forgot master password?": the phrase opens the vault and `new` becomes the master
    /// password. Rate limited like the unlock.
    pub(super) fn recover_vault(&self, tab: Option<TabId>, req_id: Option<u32>, phrase: Secret, new: Secret) {
        if self.vault_unlocked() {
            self.reply(tab, Event::err(req_id, "The vault is open: change the master password instead"));
            return;
        }
        if !self.vault_exists() {
            self.reply(tab, Event::err(req_id, "No vault has been set up"));
            return;
        }
        let phrase = match Phrase::parse(&phrase) {
            Ok(p) => p,
            Err(e) => {
                self.reply(tab, Event::err(req_id, e.to_string()));
                return;
            }
        };
        let wait = self.vaults.rate.borrow().wait_ms(now_ms());
        if wait > 0 {
            self.reply(tab, Event::err(req_id, format!("Too many attempts. Try again in {} s.", wait.div_ceil(1000))));
            return;
        }
        if self.vaults.busy.replace(true) {
            self.reply(tab, Event::err(req_id, "Busy, try again"));
            return;
        }
        let path = self.dirs.vault();
        let new = new.into_zeroizing();
        self.spawn_job(
            "vault-recover",
            move || Vault::recover(&path, &phrase, &new, &Dpapi),
            move |b, r| {
                b.vaults.busy.set(false);
                let now = now_ms();
                match r {
                    Ok(v) => {
                        b.vaults.rate.borrow_mut().record_success();
                        let _ = b.store.borrow().save_rate_limiter(&b.vaults.rate.borrow());
                        info!("vault opened with the recovery phrase; new master password set");
                        b.vault_opened(v);
                        b.reply(tab, Event::ok(req_id, ()));
                        b.after_unlock();
                    }
                    Err(VaultError::WrongPhrase) => {
                        b.vaults.rate.borrow_mut().record_failure(now);
                        let _ = b.store.borrow().save_rate_limiter(&b.vaults.rate.borrow());
                        warning!("vault recovery failed: not this vault's phrase");
                        b.reply(tab, Event::err(req_id, VaultError::WrongPhrase.to_string()));
                    }
                    Err(e) => {
                        warning!("vault recovery error: {e}");
                        b.reply(tab, Event::err(req_id, vault_error_message(&e)));
                    }
                }
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_positions() {
        assert_eq!(chrono_like_date(0), "1970-01-01");
        assert_eq!(chrono_like_date(1_794_083_000_000), "2026-11-07");
    }
}
