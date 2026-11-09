//! Login suggestions under a focused password or username field (Krunk 0.5.0). When a page
//! (or an http(s) iframe) reports that a login field got the focus, the host shows a small
//! dropdown of the vault's matching usernames (the exact origin's, else the same
//! registrable domain's, see `lb_core::vault::Vault::matches_for`). Choosing one fills that frame's
//! form. The dropdown is a popup that never takes the focus from the field. While the vault
//! is locked, a keyed index of the sites that have logins (`HintIndex`, in the database with
//! a DPAPI-wrapped key) decides whether to offer "Unlock Krunk passwords"; the unlock prompt
//! is Krunk's own, never in the page, and after it one match fills at once.

use super::Browser;
use crate::ffi::cursor;
use crate::ffi::secure::Dpapi;
use crate::log::{info, warning};
use lb_core::protocol::{FieldKind, FieldRect, PopupContent, PopupKind, PopupRect, SuggestChoice, UnlockPurpose};
use lb_core::storage::now_ms;
use lb_core::tabs::TabId;
use lb_core::vault::{match_kind, HintIndex, KeyWrapper, MatchKind};
use std::cell::{Cell, RefCell};
use zeroize::Zeroizing;

/// Pages can fire focus events freely: the dropdown shows at most this often.
const MIN_INTERVAL_MS: i64 = 250;
const ROW_CSS: f64 = 38.0;
const FOOT_CSS: f64 = 42.0;
/// Rows the dropdown shows before it scrolls (half a row more hints at the rest).
const VISIBLE_ROWS: usize = 8;
/// A sanity limit on the rows sent to the popup.
const MAX_CHOICES: usize = 500;

/// The dropdown's height for `rows` rows (CSS px): at most `VISIBLE_ROWS` and a half, then it
/// scrolls.
fn list_height(rows: usize) -> f64 {
    let shown = if rows > VISIBLE_ROWS { VISIBLE_ROWS as f64 + 0.5 } else { rows as f64 };
    shown * ROW_CSS + FOOT_CSS
}
const WIDTH_CSS: f64 = 300.0;
const KEY_KV: &str = "vault_hint_key";
const TAGS_KV: &str = "vault_hint_tags";

/// The suggestion dropdown that is open now.
#[derive(Debug, Clone)]
struct Open {
    tab: TabId,
    frame_id: String,
    origin: String,
    rect: PopupRect,
    /// The dropdown is for a payment-card field (0.6.0).
    card: bool,
}

/// After a fill the page focuses the filled field (and the focus returns from the unlock
/// prompt): focus reports from that frame are ignored for this long, or the dropdown would
/// come straight back.
const QUIET_AFTER_FILL_MS: i64 = 1000;

#[derive(Default)]
pub(super) struct SuggestHost {
    shown_at: Cell<i64>,
    /// The frame (tab, frame id) that was just filled and until when its focus reports are ignored.
    quiet: RefCell<Option<(TabId, String, i64)>>,
    open: RefCell<Option<Open>>,
    /// The frame whose "Unlock" was chosen (the unlock prompt then fills it).
    unlock_frame: RefCell<Option<Open>>,
    index: RefCell<Option<HintIndex>>,
    tags: RefCell<Option<Vec<String>>>,
    /// The password the open dropdown offers for a new-password field (0.6.0).
    generated: RefCell<Option<Zeroizing<String>>>,
}

impl Browser {
    /// A login field got the focus in frame `frame_id` of tab `tab` (`origin`: the frame's
    /// browser-reported origin). Shows the matching logins, or the unlock entry.
    pub(super) fn field_focus(
        &self,
        tab: TabId,
        frame_id: &str,
        origin: &str,
        main_frame: bool,
        field: FieldKind,
        rect: FieldRect,
    ) {
        if self.active_id() != Some(tab) || !rect.is_sane() {
            return;
        }
        if field == FieldKind::Card {
            return self.card_field_focus(tab, frame_id, origin, main_frame, rect);
        }
        let now = now_ms();
        if now - self.suggest.shown_at.get() < MIN_INTERVAL_MS {
            return;
        }
        let just_filled = self.suggest.quiet.borrow();
        if just_filled.as_ref().is_some_and(|(t, f, until)| *t == tab && f == frame_id && now < *until) {
            return;
        }
        drop(just_filled);
        let url = format!("{origin}/");
        let entries = if self.vault_unlocked() { self.suggest_choices(&url) } else { Vec::new() };
        let locked = !self.vault_unlocked() && self.vault_exists() && self.site_has_logins(&url);
        // A new password is being chosen: offer a generated one (never in private tabs; the
        // page gets it only when the user picks it).
        let generated = if field == FieldKind::NewPassword && !self.tab(tab).is_some_and(|t| t.private) {
            let p = self.settings.borrow().passgen.clone();
            match lb_core::passgen::generate(p.length, p.symbols) {
                Ok(pw) => Some(pw),
                Err(e) => {
                    warning!("password generator: {e}");
                    None
                }
            }
        } else {
            None
        };
        if entries.is_empty() && !locked && generated.is_none() {
            if self.suggest.open.borrow().is_some() {
                self.hide_popup();
            }
            return;
        }
        let Some(anchor) = self.suggest_anchor(tab, main_frame, rect) else { return };
        let rows = if locked { 1 } else { entries.len() + usize::from(generated.is_some()) };
        let popup_rect = PopupRect {
            x: anchor.x,
            y: anchor.y,
            width: WIDTH_CSS.max(anchor.width.min(420.0)),
            height: list_height(rows),
        };
        info!(
            "tab {tab}: login suggestions for a {} field ({}{})",
            match field {
                FieldKind::Password => "password",
                FieldKind::Username => "username",
                FieldKind::NewPassword => "new-password",
                FieldKind::Card => "card",
            },
            if locked { "vault locked" } else { "matches" },
            if main_frame { "" } else { ", in a frame" }
        );
        self.suggest.shown_at.set(now);
        *self.suggest.open.borrow_mut() = Some(Open {
            tab,
            frame_id: frame_id.to_string(),
            origin: origin.to_string(),
            rect: popup_rect,
            card: false,
        });
        let offered = generated.as_ref().map(|g| g.to_string());
        *self.suggest.generated.borrow_mut() = generated;
        self.show_popup(
            PopupContent::Suggest {
                tab_id: tab,
                frame_id: frame_id.to_string(),
                origin: origin.to_string(),
                locked,
                entries,
                generated: offered,
            },
            Some(popup_rect),
        );
    }

    /// A payment-card field got the focus: the saved cards (masked), or "Unlock" while the
    /// vault is locked and has cards. Never in private tabs.
    fn card_field_focus(&self, tab: TabId, frame_id: &str, origin: &str, main_frame: bool, rect: FieldRect) {
        if self.tab(tab).is_some_and(|t| t.private) || !self.vault_exists() {
            return;
        }
        let now = now_ms();
        if now - self.suggest.shown_at.get() < MIN_INTERVAL_MS {
            return;
        }
        let just_filled = self.suggest.quiet.borrow();
        if just_filled.as_ref().is_some_and(|(t, f, until)| *t == tab && f == frame_id && now < *until) {
            return;
        }
        drop(just_filled);
        let locked = !self.vault_unlocked();
        let cards = if locked { Vec::new() } else { self.card_choices() };
        if !locked && cards.is_empty() {
            return;
        }
        let Some(anchor) = self.suggest_anchor(tab, main_frame, rect) else { return };
        let rows = if locked { 1 } else { cards.len() };
        let popup_rect = PopupRect {
            x: anchor.x,
            y: anchor.y,
            width: WIDTH_CSS.max(anchor.width.min(420.0)),
            height: list_height(rows),
        };
        info!("tab {tab}: saved cards for a card field ({})", if locked { "vault locked" } else { "choices" });
        self.suggest.shown_at.set(now);
        *self.suggest.open.borrow_mut() = Some(Open {
            tab,
            frame_id: frame_id.to_string(),
            origin: origin.to_string(),
            rect: popup_rect,
            card: true,
        });
        self.show_popup(
            PopupContent::CardFill {
                tab_id: tab,
                frame_id: frame_id.to_string(),
                origin: origin.to_string(),
                locked,
                cards,
            },
            Some(popup_rect),
        );
    }

    fn card_choices(&self) -> Vec<lb_core::protocol::CardChoice> {
        self.vaults
            .vault
            .borrow()
            .as_ref()
            .map(|v| {
                v.card_summaries()
                    .into_iter()
                    .map(|c| lb_core::protocol::CardChoice {
                        id: c.id,
                        label: if c.nickname.is_empty() {
                            format!("{} •••• {}", c.brand, c.last4)
                        } else {
                            format!("{} ({} •••• {})", c.nickname, c.brand, c.last4)
                        },
                        name: c.name,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A card chosen in the dropdown: its number, expiry and name go to that frame's form.
    pub(super) fn card_fill_pick(&self, tab: TabId, frame_id: &str, card_id: &str) {
        let open = self.suggest.open.borrow_mut().take();
        self.hide_popup();
        let Some(o) = open.filter(|o| o.tab == tab && o.frame_id == frame_id && o.card) else {
            info!("card fill dropped: no dropdown for that frame");
            return;
        };
        if self.active_id() != Some(o.tab) {
            return;
        }
        let Some(view) = self.tab_view(o.tab) else { return };
        let Some(frame_url) = view.frame_url(&o.frame_id) else { return };
        if lb_core::urlclean::origin_of(&frame_url).as_deref() != Some(o.origin.as_str()) {
            info!("card fill dropped: the frame navigated");
            return;
        }
        let message = {
            let vault = self.vaults.vault.borrow();
            let Some(c) = vault.as_ref().and_then(|v| v.card(card_id)) else { return };
            let mut m = Zeroizing::new(String::with_capacity(c.number.len() + c.name.len() + 96));
            m.push_str("{\"lbFill\":{\"card\":{\"number\":");
            super::vault::push_json_string(&mut m, &c.number);
            m.push_str(&format!(",\"expMonth\":{},\"expYear\":{},\"name\":", c.exp_month, c.exp_year));
            super::vault::push_json_string(&mut m, &c.name);
            m.push_str("}}}");
            m
        };
        if view.post_json_to_frame(&o.frame_id, &message) {
            info!("tab {tab}: card filled");
            *self.suggest.quiet.borrow_mut() = Some((o.tab, o.frame_id.clone(), now_ms() + QUIET_AFTER_FILL_MS));
            self.touch_vault();
        }
    }

    /// The page's view of a password field, checked against Krunk's heuristics: only a
    /// field where a new password is chosen gets the generator.
    pub(super) fn resolve_field_kind(
        &self,
        field: FieldKind,
        autocomplete: &str,
        passwords: u32,
        text: &str,
    ) -> FieldKind {
        match field {
            FieldKind::Username => FieldKind::Username,
            FieldKind::Card => FieldKind::Card,
            FieldKind::Password | FieldKind::NewPassword => {
                if lb_core::passgen::wants_new_password(autocomplete, passwords, text) {
                    FieldKind::NewPassword
                } else {
                    FieldKind::Password
                }
            }
        }
    }

    /// "Use a strong password" in the dropdown: the password the dropdown offered goes into
    /// the new-password fields of that frame, and the submit that follows saves it without
    /// asking (see `login_submitted`).
    pub(super) fn suggest_generate(&self, tab: TabId, frame_id: &str) {
        let open = self.suggest.open.borrow_mut().take();
        let generated = self.suggest.generated.borrow_mut().take();
        self.hide_popup();
        let (Some(o), Some(pw)) = (open.filter(|o| o.tab == tab && o.frame_id == frame_id), generated) else {
            info!("generated password dropped: no dropdown for that frame");
            return;
        };
        if self.active_id() != Some(o.tab) {
            return;
        }
        let Some(view) = self.tab_view(o.tab) else { return };
        let Some(frame_url) = view.frame_url(&o.frame_id) else { return };
        if lb_core::urlclean::origin_of(&frame_url).as_deref() != Some(o.origin.as_str()) {
            info!("generated password dropped: the frame navigated");
            return;
        }
        let mut m = Zeroizing::new(String::with_capacity(pw.len() + 48));
        m.push_str("{\"lbFill\":{\"generated\":true,\"password\":");
        super::vault::push_json_string(&mut m, &pw);
        m.push_str("}}");
        if view.post_json_to_frame(&o.frame_id, &m) {
            info!("tab {tab}: generated password filled ({} characters)", pw.len());
            *self.suggest.quiet.borrow_mut() = Some((o.tab, o.frame_id.clone(), now_ms() + QUIET_AFTER_FILL_MS));
            self.remember_generated(o.tab, &o.origin, pw);
        }
    }

    /// The field lost the focus (or the user typed or pressed Esc): the dropdown goes,
    /// unless the mouse is on it (the click that chooses an entry is on its way).
    pub(super) fn field_blur(&self, tab: TabId, frame_id: &str) {
        let open = self.suggest.open.borrow().clone();
        let Some(o) = open.filter(|o| o.tab == tab && o.frame_id == frame_id) else { return };
        let _ = o;
        let on_popup = self
            .popup
            .try_borrow()
            .ok()
            .and_then(|p| p.as_ref().filter(|p| p.window.is_visible()).map(|p| p.window.rect()))
            .is_some_and(|r| {
                let (x, y) = cursor::position();
                x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
            });
        if !on_popup && matches!(self.popup_kind(), Some(PopupKind::Suggest | PopupKind::CardFill)) {
            self.hide_popup();
        }
    }

    /// Where the dropdown goes (shell CSS px): under the field for the main frame; by the
    /// cursor for a frame (its place in the page is not known to the host).
    fn suggest_anchor(&self, tab: TabId, main_frame: bool, rect: FieldRect) -> Option<PopupRect> {
        let win = self.window_of(tab)?;
        let f = self.frame(win)?;
        let scale = f.window.scale();
        let (_, content) = self.frame_areas(&f);
        let page = self.devtools_split_of(&f).map(|s| s.page).unwrap_or(content);
        let (px, py, pw, ph) = (
            f64::from(page.x) / scale,
            f64::from(page.y) / scale,
            f64::from(page.width) / scale,
            f64::from(page.height) / scale,
        );
        let (x, y, width) = if main_frame {
            let zoom = self.tab_view(tab).map(|v| v.zoom()).unwrap_or(1.0);
            let zoom = if zoom.is_finite() && zoom > 0.0 { zoom } else { 1.0 };
            (px + rect.x * zoom, py + (rect.y + rect.height) * zoom + 2.0, rect.width * zoom)
        } else {
            let (cx, cy) = cursor::position();
            let (ox, oy) = f.window.client_to_screen(0, 0);
            (f64::from(cx - ox) / scale - 20.0, f64::from(cy - oy) / scale + 16.0, 0.0)
        };
        // Inside the page area, at least.
        let x = x.clamp(px, (px + pw - WIDTH_CSS).max(px));
        let y = y.clamp(py, (py + ph - 60.0).max(py));
        Some(PopupRect { x, y, width: width.max(0.0), height: 0.0 })
    }

    /// The user chose an entry in the dropdown.
    pub(super) fn suggest_pick(&self, tab: TabId, frame_id: &str, entry_id: &str) {
        let open = self.suggest.open.borrow_mut().take();
        self.hide_popup();
        let Some(o) = open.filter(|o| o.tab == tab && o.frame_id == frame_id) else {
            info!("suggestion dropped: no dropdown for that frame");
            return;
        };
        self.fill_into_frame(&o, entry_id);
    }

    /// Fills entry `entry_id` into the frame of `o`, re-checking everything: the tab is
    /// active, the frame still exists on the same origin, and the entry matches that origin.
    fn fill_into_frame(&self, o: &Open, entry_id: &str) {
        if self.active_id() != Some(o.tab) {
            info!("fill dropped: tab changed");
            return;
        }
        let Some(view) = self.tab_view(o.tab) else { return };
        let Some(frame_url) = view.frame_url(&o.frame_id) else {
            info!("fill dropped: the frame is gone");
            return;
        };
        if lb_core::urlclean::origin_of(&frame_url).as_deref() != Some(o.origin.as_str()) {
            info!("fill dropped: the frame navigated");
            return;
        }
        let message = {
            let vault = self.vaults.vault.borrow();
            let Some(e) = vault.as_ref().and_then(|v| v.entry(entry_id)) else { return };
            if match_kind(&e.origin, &frame_url).is_none() {
                warning!("fill refused: entry origin does not match the frame");
                return;
            }
            let mut m = Zeroizing::new(String::with_capacity(e.password.len() + e.username.len() + 48));
            m.push_str("{\"lbFill\":{\"username\":");
            super::vault::push_json_string(&mut m, &e.username);
            m.push_str(",\"password\":");
            super::vault::push_json_string(&mut m, &e.password);
            m.push_str("}}");
            m
        };
        if view.post_json_to_frame(&o.frame_id, &message) {
            info!("login filled from the suggestions");
            *self.suggest.quiet.borrow_mut() = Some((o.tab, o.frame_id.clone(), now_ms() + QUIET_AFTER_FILL_MS));
            self.touch_vault();
        }
    }

    /// "Unlock Krunk passwords" in the dropdown: Krunk's own master-password prompt, then
    /// the fill (one match) or the list (several).
    pub(super) fn suggest_unlock(&self, tab: TabId, frame_id: &str) {
        let open = self.suggest.open.borrow_mut().take();
        let Some(o) = open.filter(|o| o.tab == tab && o.frame_id == frame_id) else { return };
        *self.suggest.unlock_frame.borrow_mut() = Some(o.clone());
        let purpose = if o.card { UnlockPurpose::CardFill } else { UnlockPurpose::Suggest };
        *self.vaults.unlock_for.borrow_mut() = Some((tab, o.origin.clone(), purpose));
        self.show_unlock_prompt(None);
    }

    /// The vault was unlocked for a suggestion.
    pub(super) fn after_unlock_suggest(&self, tab: TabId, origin: &str) {
        let Some(o) = self.suggest.unlock_frame.borrow_mut().take().filter(|o| o.tab == tab && o.origin == origin)
        else {
            self.hide_popup();
            return;
        };
        if o.card {
            // The card dropdown again, now with the choices.
            let cards = self.card_choices();
            if cards.is_empty() {
                self.hide_popup();
                return;
            }
            let rect = PopupRect { height: list_height(cards.len()), ..o.rect };
            *self.suggest.open.borrow_mut() = Some(Open { rect, ..o.clone() });
            self.show_popup(
                PopupContent::CardFill {
                    tab_id: o.tab,
                    frame_id: o.frame_id.clone(),
                    origin: o.origin.clone(),
                    locked: false,
                    cards,
                },
                Some(rect),
            );
            return;
        }
        // The same list as on a focus of the field (it used to have no limit and no hosts).
        let entries = self.suggest_choices(&format!("{origin}/"));
        match entries.len() {
            0 => self.hide_popup(),
            1 => {
                self.hide_popup();
                self.fill_into_frame(&o, &entries[0].id);
            }
            _ => {
                let rect = PopupRect { height: list_height(entries.len()), ..o.rect };
                *self.suggest.open.borrow_mut() = Some(Open { rect, ..o.clone() });
                self.show_popup(
                    PopupContent::Suggest {
                        tab_id: tab,
                        frame_id: o.frame_id,
                        origin: o.origin,
                        locked: false,
                        entries,
                        generated: None,
                    },
                    Some(rect),
                );
            }
        }
    }

    /// The dropdown's rows for a page at `url` (see `Vault::matches_for`): logins of a sister
    /// subdomain show their host.
    fn suggest_choices(&self, url: &str) -> Vec<SuggestChoice> {
        let vault = self.vaults.vault.borrow();
        let Some(v) = vault.as_ref() else { return Vec::new() };
        v.matches_for(url)
            .into_iter()
            .map(|(e, k)| SuggestChoice {
                id: e.id.clone(),
                username: e.username.clone(),
                host: (k == MatchKind::Domain).then(|| lb_core::urlclean::host_of(&e.origin).unwrap_or_default()),
            })
            // Every login of the site (0.11.3: it used to stop at 8); the list scrolls.
            .take(MAX_CHOICES)
            .collect()
    }

    /// The tab whose suggestion dropdown is open.
    pub(super) fn suggest_open_tab(&self) -> Option<TabId> {
        self.suggest.open.borrow().as_ref().map(|o| o.tab)
    }

    /// The dropdown was hidden without a choice.
    pub(super) fn suggest_dismissed(&self) {
        self.suggest.open.borrow_mut().take();
    }

    // ------------------------------------------------------------ the locked-vault index

    /// The keyed index of sites with logins (made once, its key DPAPI-wrapped in the database).
    fn hint_index(&self) -> Option<HintIndex> {
        if let Some(i) = self.suggest.index.borrow().as_ref() {
            return Some(i.clone());
        }
        let store = self.store.borrow();
        let loaded = store.kv_get(KEY_KV).ok().flatten().and_then(|hex| {
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
                .collect::<Option<Vec<u8>>>()?;
            let plain = Dpapi.unprotect(&bytes).ok()?;
            let key: [u8; 32] = plain.as_slice().try_into().ok()?;
            Some(HintIndex::new(key))
        });
        let index = match loaded {
            Some(i) => i,
            None => {
                let i = HintIndex::random();
                match Dpapi.protect(i.key()) {
                    Ok(wrapped) => {
                        let hex: String = wrapped.iter().map(|b| format!("{b:02x}")).collect();
                        if let Err(e) = store.kv_set(KEY_KV, &hex) {
                            warning!("login hint index: {e}");
                        }
                    }
                    Err(e) => {
                        warning!("login hint index key: {e}");
                        return None;
                    }
                }
                i
            }
        };
        *self.suggest.index.borrow_mut() = Some(index.clone());
        Some(index)
    }

    /// Does the (locked) vault have logins for `url`, by the index?
    fn site_has_logins(&self, url: &str) -> bool {
        let Some(index) = self.hint_index() else { return false };
        if self.suggest.tags.borrow().is_none() {
            let tags: Vec<String> = self
                .store
                .borrow()
                .kv_get(TAGS_KV)
                .ok()
                .flatten()
                .and_then(|j| serde_json::from_str(&j).ok())
                .unwrap_or_default();
            *self.suggest.tags.borrow_mut() = Some(tags);
        }
        self.suggest.tags.borrow().as_ref().is_some_and(|t| index.has_logins(url, t))
    }

    /// The vault is open: the index follows its entries (written only when it changed).
    pub(super) fn refresh_hint_index(&self) {
        let Some(index) = self.hint_index() else { return };
        let tags = {
            let vault = self.vaults.vault.borrow();
            let Some(v) = vault.as_ref() else { return };
            index.tags_for(v.summaries().iter().map(|s| s.origin.as_str()))
        };
        if self.suggest.tags.borrow().as_ref() == Some(&tags) {
            return;
        }
        match serde_json::to_string(&tags) {
            Ok(json) => {
                if let Err(e) = self.store.borrow().kv_set(TAGS_KV, &json) {
                    warning!("login hint index: {e}");
                }
            }
            Err(e) => warning!("login hint index: {e}"),
        }
        *self.suggest.tags.borrow_mut() = Some(tags);
    }
}
