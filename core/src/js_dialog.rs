//! A page's JavaScript dialogs (0.12.4): `alert`, `confirm`, `prompt` and the "Leave site?" /
//! "Reload site?" of `beforeunload`. The engine's own dialogs were separate native windows that
//! could end up behind Krunk's window while the page waited for them: the tab looked frozen.
//! Krunk now shows them itself, as a prompt owned by the window (always above it), and every way
//! that prompt ends answers the page: a button, Esc, a click elsewhere, another prompt, the tab
//! closing or navigating. A dialog the user can't see (a background tab, no window) is answered
//! at once as if dismissed.

use serde::{Deserialize, Serialize};

/// The longest message shown (Chrome cuts them too); the rest is dropped.
pub const MAX_MESSAGE_CHARS: usize = 10_000;
/// The longest default text of a `prompt()` and the longest answer passed back.
pub const MAX_INPUT_CHARS: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JsDialogKind {
    Alert,
    Confirm,
    Prompt,
    /// `beforeunload` when leaving the page.
    BeforeUnload,
    /// `beforeunload` when reloading it.
    BeforeReload,
}

impl JsDialogKind {
    /// What the user's dismissal (Esc, a click elsewhere, the prompt going away) answers: `alert` is
    /// acknowledged; everything else is refused (the page stays).
    pub fn dismissed_answer(self) -> bool {
        matches!(self, JsDialogKind::Alert)
    }

    /// What a dialog nobody saw (a background tab, blocked, not shown) answers: as dismissed,
    /// except that leaving or reloading goes ahead. A page must never keep the user on it with a
    /// dialog that isn't there.
    pub fn unseen_answer(self) -> bool {
        matches!(self, JsDialogKind::Alert | JsDialogKind::BeforeUnload | JsDialogKind::BeforeReload)
    }

    pub fn is_before_unload(self) -> bool {
        matches!(self, JsDialogKind::BeforeUnload | JsDialogKind::BeforeReload)
    }
}

/// The page's message as shown: control characters other than line breaks and tabs removed,
/// line breaks normalised, at most [`MAX_MESSAGE_CHARS`] characters ("…" when cut).
pub fn clean_message(s: &str) -> String {
    let normalised = s.replace("\r\n", "\n").replace('\r', "\n");
    let mut out: String =
        normalised.chars().filter(|c| !c.is_control() || *c == '\n' || *c == '\t').take(MAX_MESSAGE_CHARS).collect();
    if normalised.chars().filter(|c| !c.is_control() || *c == '\n' || *c == '\t').count() > MAX_MESSAGE_CHARS {
        out.push('…');
    }
    out
}

/// A prompt's text, default or answer: one line, at most [`MAX_INPUT_CHARS`] characters.
pub fn clean_input(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(MAX_INPUT_CHARS).collect()
}

/// Who is asking, for the dialog's title ("a.example says"): the host of the frame's address;
/// a page without one (file:, data:, about:) is "This page".
pub fn source_label(origin_url: &str) -> String {
    let host = url::Url::parse(origin_url)
        .ok()
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .and_then(|u| u.host_str().map(|h| h.trim_start_matches("www.").to_string()))
        .filter(|h| !h.is_empty());
    match host {
        Some(h) => format!("{h} says"),
        None => "This page says".into(),
    }
}

/// One tab's dialogs since its page last loaded: a page that keeps opening them can be stopped
/// ("Don't let this page show more dialogs", offered from the second one on, as in Chrome).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TabDialogs {
    pub shown: u32,
    pub blocked: bool,
}

impl TabDialogs {
    /// A new dialog: `None` when the page is blocked (answer it at once, unseen), else whether
    /// to offer the block. Counts it.
    pub fn on_new_dialog(&mut self) -> Option<bool> {
        if self.blocked {
            return None;
        }
        self.shown = self.shown.saturating_add(1);
        Some(self.shown > 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages() {
        assert_eq!(clean_message("a\r\nb\rc\u{0}\u{7}\td"), "a\nb\nc\td");
        let long = "x".repeat(MAX_MESSAGE_CHARS + 5);
        let cut = clean_message(&long);
        assert_eq!(cut.chars().count(), MAX_MESSAGE_CHARS + 1);
        assert!(cut.ends_with('…'));
        assert_eq!(clean_message(&"y".repeat(MAX_MESSAGE_CHARS)).chars().count(), MAX_MESSAGE_CHARS);
        assert_eq!(clean_input("one\ntwo\u{1b}"), "onetwo");
        assert_eq!(clean_input(&"z".repeat(MAX_INPUT_CHARS * 2)).len(), MAX_INPUT_CHARS);
    }

    #[test]
    fn labels() {
        assert_eq!(source_label("https://www.example.com/x"), "example.com says");
        assert_eq!(source_label("http://127.0.0.1:8094/a.html"), "127.0.0.1 says");
        assert_eq!(source_label("file:///C:/x.html"), "This page says");
        assert_eq!(source_label("about:blank"), "This page says");
        assert_eq!(source_label(""), "This page says");
    }

    #[test]
    fn dismissal_answers() {
        assert!(JsDialogKind::Alert.dismissed_answer());
        for k in [JsDialogKind::Confirm, JsDialogKind::Prompt, JsDialogKind::BeforeUnload, JsDialogKind::BeforeReload] {
            assert!(!k.dismissed_answer(), "{k:?}");
        }
        assert!(JsDialogKind::BeforeReload.is_before_unload() && !JsDialogKind::Prompt.is_before_unload());
        // Unseen: leaving goes ahead; confirm and prompt are refused.
        assert!(JsDialogKind::BeforeUnload.unseen_answer() && JsDialogKind::BeforeReload.unseen_answer());
        assert!(
            JsDialogKind::Alert.unseen_answer()
                && !JsDialogKind::Confirm.unseen_answer()
                && !JsDialogKind::Prompt.unseen_answer()
        );
    }

    #[test]
    fn block_offered_from_the_second() {
        let mut t = TabDialogs::default();
        assert_eq!(t.on_new_dialog(), Some(false));
        assert_eq!(t.on_new_dialog(), Some(true));
        t.blocked = true;
        assert_eq!(t.on_new_dialog(), None);
        assert_eq!(t.shown, 2);
    }
}
