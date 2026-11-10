//! A page's JavaScript dialogs as Krunk's own prompt (0.12.4; the rules are in
//! `lb_core::js_dialog`). The engine's native dialog windows could end up behind Krunk's frame
//! while the page waited for them, and the tab looked frozen ("Leave site?" did). The prompt is
//! owned by the frame (always above it), and every way it ends answers the page:
//! - a button or Enter (OK / Leave), Esc or a click elsewhere (the dismissal's answer);
//! - another prompt replacing it, the tab going to the background or closing;
//! - the engine dropping the dialog (navigation): unanswered, the engine has let go of it.
//!
//! A dialog the user can't see (a background tab, another window, a page the user stopped) is
//! answered at once, and "Leave site?" then lets the page go. At most one is open; its responder
//! answers by itself if it is ever dropped (`JsDialogResponder`).

use super::Browser;
use crate::ffi::cef::{JsDialogRequest, JsDialogResponder};
use crate::log::info;
use lb_core::js_dialog::{self, JsDialogKind, TabDialogs};
use lb_core::protocol::PopupContent;
use lb_core::tabs::TabId;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

struct Pending {
    id: u32,
    tab: TabId,
    responder: JsDialogResponder,
}

#[derive(Default)]
pub(super) struct JsDialogHost {
    pending: RefCell<Option<Pending>>,
    tabs: RefCell<HashMap<TabId, TabDialogs>>,
    next: Cell<u32>,
}

impl Browser {
    pub(super) fn js_dialog(&self, tab: TabId, req: JsDialogRequest, responder: JsDialogResponder) {
        let kind = req.kind;
        // Only the current window's active tab can show one; anything else is answered now.
        if self.active_id() != Some(tab) || self.window().is_none() {
            info!("tab {tab}: {kind:?} dialog of a tab not in front: answered unseen");
            drop(responder);
            return;
        }
        let offer_block = self.js_dialogs.tabs.borrow_mut().entry(tab).or_default().on_new_dialog();
        // Never for "Leave site?": blocked, it would answer by itself (leaving goes ahead).
        let offer_block = offer_block.map(|o| o && !kind.is_before_unload());
        let Some(offer_block) = offer_block else {
            info!("tab {tab}: {kind:?} dialog blocked by the user: answered unseen");
            drop(responder);
            return;
        };
        let id = self.js_dialogs.next.get().wrapping_add(1);
        self.js_dialogs.next.set(id);
        // One at a time: an older one (none should be open) gives way, answered.
        let old = self.js_dialogs.pending.borrow_mut().replace(Pending { id, tab, responder });
        drop(old);
        let (title, message) = match kind {
            JsDialogKind::BeforeUnload => ("Leave site?".to_string(), "Changes you made may not be saved.".to_string()),
            JsDialogKind::BeforeReload => {
                ("Reload site?".to_string(), "Changes you made may not be saved.".to_string())
            }
            _ => (js_dialog::source_label(&req.origin_url), js_dialog::clean_message(&req.message)),
        };
        info!("tab {tab}: {kind:?} dialog {id} shown");
        self.show_prompt(
            PopupContent::JsDialog {
                tab_id: tab,
                dialog_id: id,
                dialog: kind,
                title,
                message,
                default_text: js_dialog::clean_input(&req.default_text),
                offer_block,
            },
            0,
        );
        // The prompt could not be shown: the page gets its answer now.
        if !self.js_dialog_showing(id) {
            info!("tab {tab}: dialog {id} could not be shown: answered unseen");
            let pending = self.js_dialogs.pending.borrow_mut().take_if(|p| p.id == id);
            drop(pending);
        }
    }

    fn js_dialog_showing(&self, id: u32) -> bool {
        self.popup.try_borrow().ok().is_some_and(|p| {
            p.as_ref().is_some_and(|p| {
                p.open() && matches!(p.content, Some(PopupContent::JsDialog { dialog_id, .. }) if dialog_id == id)
            })
        })
    }

    /// The prompt's answer.
    pub(super) fn js_dialog_reply(&self, id: u32, accept: bool, text: Option<String>, block: bool) {
        let pending = self.js_dialogs.pending.borrow_mut().take_if(|p| p.id == id);
        let Some(p) = pending else { return };
        if block {
            info!("tab {}: the user blocked this page's dialogs", p.tab);
            self.js_dialogs.tabs.borrow_mut().entry(p.tab).or_default().blocked = true;
        }
        // (Hiding counts as a dismissal; this one is taken already.)
        self.hide_popup();
        let text = js_dialog::clean_input(text.as_deref().unwrap_or(""));
        info!("tab {}: dialog {id} answered ({})", p.tab, if accept { "OK" } else { "Cancel" });
        p.responder.answer(accept, &text);
    }

    /// The prompt went away without an answer: the dismissal's answer.
    pub(super) fn js_dialog_dismissed(&self, id: u32) {
        let pending = self.js_dialogs.pending.borrow_mut().take_if(|p| p.id == id);
        if let Some(p) = pending {
            info!("tab {}: dialog {id} dismissed", p.tab);
            p.responder.dismiss();
        }
    }

    /// The engine let go of tab `tab`'s dialog (navigation, close): hide it, unanswered.
    pub(super) fn js_dialog_reset(&self, tab: TabId) {
        let pending = self.js_dialogs.pending.borrow_mut().take_if(|p| p.tab == tab);
        if let Some(p) = pending {
            info!("tab {tab}: dialog {} dropped by the engine", p.id);
            p.responder.disarm();
            if self.js_dialog_showing(p.id) {
                self.hide_popup();
            }
        }
    }

    /// Tab `tab` is no longer the one in front (another tab, it closed): its open dialog is
    /// dismissed (hidden and answered). With `gone`, its dialog bookkeeping goes too.
    pub(super) fn js_dialog_tab_left(&self, tab: TabId, gone: bool) {
        let id = self.js_dialogs.pending.borrow().as_ref().filter(|p| p.tab == tab).map(|p| p.id);
        if let Some(id) = id {
            if self.js_dialog_showing(id) {
                self.hide_popup();
            }
            self.js_dialog_dismissed(id);
        }
        if gone {
            self.js_dialogs.tabs.borrow_mut().remove(&tab);
        }
    }

    /// Tab `tab` loaded a page: its dialog count and block start again.
    pub(super) fn js_dialog_load_end(&self, tab: TabId) {
        self.js_dialogs.tabs.borrow_mut().remove(&tab);
    }

    /// Does tab `tab` wait for an answer (its page is blocked meanwhile, not hung)?
    pub(super) fn js_dialog_open_in(&self, tab: TabId) -> bool {
        self.js_dialogs.pending.borrow().as_ref().is_some_and(|p| p.tab == tab)
    }
}
