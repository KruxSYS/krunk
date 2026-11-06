//! Login suggestions under a focused password or username field (Krunk 0.5.0). When a page
//! (or an http(s) iframe) reports that a login field got the focus, the host shows a small
//! dropdown of the vault's matching usernames (exact origin first, then the same
//! registrable domain, see `lb_core::vault::match_kind`). Choosing one fills that frame's
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
const FOOT_CSS: f64 = 34.0;
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
        let entries: Vec<SuggestChoice> = if self.vault_unlocked() {
            let vault = self.vaults.vault.borrow();
            vault
                .as_ref()
                .map(|v| {
                    v.matches_for(&url)
                        .into_iter()
                        .map(|(e, k)| SuggestChoice {
                            id: e.id.clone(),
                            username: e.username.clone(),
                            host: (k == MatchKind::Domain)
                                .then(|| lb_core::urlclean::host_of(&e.origin).unwrap_or_default()),
                        })
                        .take(8)
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let locked = !self.vault_unlocked() && self.vault_exists() && self.site_has_logins(&url);
        if entries.is_empty() && !locked {
            if self.suggest.open.borrow().is_some() {
                self.hide_popup();
            }
            return;
        }
        let Some(anchor) = self.suggest_anchor(tab, main_frame, rect) else { return };
        let rows = if locked { 1 } else { entries.len() };
        let popup_rect = PopupRect {
            x: anchor.x,
            y: anchor.y,
            width: WIDTH_CSS.max(anchor.width.min(420.0)),
            height: rows as f64 * ROW_CSS + FOOT_CSS,
        };
        info!(
            "tab {tab}: login suggestions for a {} field ({}{})",
            match field {
                FieldKind::Password => "password",
                FieldKind::Username => "username",
            },
            if locked { "vault locked" } else { "matches" },
            if main_frame { "" } else { ", in a frame" }
        );
        self.suggest.shown_at.set(now);
        *self.suggest.open.borrow_mut() =
            Some(Open { tab, frame_id: frame_id.to_string(), origin: origin.to_string(), rect: popup_rect });
        self.show_popup(
            PopupContent::Suggest {
                tab_id: tab,
                frame_id: frame_id.to_string(),
                origin: origin.to_string(),
                locked,
                entries,
            },
            Some(popup_rect),
        );
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
        if !on_popup && self.popup_kind() == Some(PopupKind::Suggest) {
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
        *self.vaults.unlock_for.borrow_mut() = Some((tab, o.origin.clone(), UnlockPurpose::Suggest));
        self.show_unlock_prompt(None);
    }

    /// The vault was unlocked for a suggestion.
    pub(super) fn after_unlock_suggest(&self, tab: TabId, origin: &str) {
        let Some(o) = self.suggest.unlock_frame.borrow_mut().take().filter(|o| o.tab == tab && o.origin == origin)
        else {
            self.hide_popup();
            return;
        };
        let url = format!("{origin}/");
        let matches: Vec<(String, String, MatchKind)> = self
            .vaults
            .vault
            .borrow()
            .as_ref()
            .map(|v| v.matches_for(&url).into_iter().map(|(e, k)| (e.id.clone(), e.username.clone(), k)).collect())
            .unwrap_or_default();
        match matches.len() {
            0 => self.hide_popup(),
            1 => {
                self.hide_popup();
                self.fill_into_frame(&o, &matches[0].0);
            }
            _ => {
                let entries: Vec<SuggestChoice> = matches
                    .into_iter()
                    .map(|(id, username, k)| SuggestChoice {
                        id,
                        username,
                        host: (k == MatchKind::Domain).then(String::new),
                    })
                    .collect();
                let rect = PopupRect { height: entries.len() as f64 * ROW_CSS + FOOT_CSS, ..o.rect };
                *self.suggest.open.borrow_mut() = Some(Open { rect, ..o.clone() });
                self.show_popup(
                    PopupContent::Suggest {
                        tab_id: tab,
                        frame_id: o.frame_id,
                        origin: o.origin,
                        locked: false,
                        entries,
                    },
                    Some(rect),
                );
            }
        }
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
