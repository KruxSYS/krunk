//! Sensitive form fields, scrubbing and paging of tool results (WP-B1).
//!
//! The rules here are the single definition: the probe gets them as JSON ([`probe_rules`]) and
//! applies them in the page; the host applies [`redact_html`] and [`scrub`] again to every
//! result before the model sees it.

use zeroize::Zeroizing;

/// What replaces a sensitive value.
pub const REDACTED: &str = "[redacted]";

/// The attributes of a form field that decide whether it is sensitive.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FieldAttrs<'a> {
    pub tag: &'a str,
    pub type_: &'a str,
    pub autocomplete: &'a str,
    pub name: &'a str,
    pub id: &'a str,
    pub aria_label: &'a str,
    pub placeholder: &'a str,
    /// The current value (for the Luhn rule); empty if unknown.
    pub value: &'a str,
}

/// Is this a password or payment-card field? (`input[type=password]`, an `autocomplete` `cc-*`
/// token, a card-like name/id/label/placeholder, or a Luhn-valid 13–19 digit value.)
pub fn is_sensitive(f: &FieldAttrs) -> bool {
    let _ = f;
    false // WP-B1
}

/// The rules as JSON for the probe: `{"selector": "...", "namePattern": "...", "luhn": true}`.
pub fn probe_rules() -> String {
    String::new() // WP-B1
}

/// Blanks the `value` attribute of sensitive `<input>` tags (and the text of sensitive
/// `<textarea>`s) in an HTML fragment.
pub fn redact_html(html: &str) -> String {
    html.to_string() // WP-B1
}

/// Replaces each of `secrets` (exact strings of 3+ characters) and every Luhn-valid run of
/// 13–19 digits (spaces and dashes allowed between digits) with [`REDACTED`].
pub fn scrub(text: &str, secrets: &[Zeroizing<String>]) -> String {
    let _ = secrets;
    text.to_string() // WP-B1
}

/// A page of a long result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paged {
    pub text: String,
    pub truncated: bool,
    pub next_offset: usize,
    pub total: usize,
}

/// Cuts `text` to `max` characters from `offset` (on character boundaries).
pub fn cap(text: &str, offset: usize, max: usize) -> Paged {
    let total = text.chars().count();
    let t: String = text.chars().skip(offset).take(max).collect();
    let end = (offset + max).min(total);
    Paged { text: t, truncated: end < total, next_offset: end, total }
}

impl Paged {
    /// The result text for the model, with `{"truncated":true,"next_offset":N,"total":T}` appended
    /// when cut.
    pub fn into_result(self) -> String {
        if self.truncated {
            format!(
                "{}\n{}",
                self.text,
                serde_json::json!({"truncated": true, "next_offset": self.next_offset, "total": self.total})
            )
        } else {
            self.text
        }
    }
}
