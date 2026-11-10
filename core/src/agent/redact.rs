//! Sensitive form fields, scrubbing and paging of tool results (WP-B1).
//!
//! The rules here are the single definition: the probe gets them as JSON ([`probe_rules`]) and
//! applies them in the page; the host applies [`redact_html`] and [`scrub`] again to every
//! result before the model sees it.
//!
//! A field is sensitive when it is (plan §5.3):
//! * `input[type=password]`;
//! * has an `autocomplete` token starting with `cc-` (`cc-number`, `section-x cc-csc`, …);
//! * has a `name`, `id`, `aria-label` or `placeholder` matching [`NAME_PATTERN`]
//!   (case-insensitive; `.?` is one optional character of any kind);
//! * holds a value that is a Luhn-valid run of 13–19 digits (spaces and dashes allowed).

use zeroize::Zeroizing;

/// What replaces a sensitive value.
pub const REDACTED: &str = "[redacted]";

/// The card-field name pattern, as a JavaScript regex source (used with the `i` flag).
/// [`name_matches`] is its Rust twin (the core has no regex engine).
pub const NAME_PATTERN: &str = r"card.?num|cc.?(num|no)|credit.?card|cvv|cvc|csc|security.?code|card.?code|cc.?exp";

/// The same pattern as alternatives of segments joined by `.?` (one optional character).
const NAME_ALTERNATIVES: &[&[&str]] = &[
    &["card", "num"],
    &["cc", "num"],
    &["cc", "no"],
    &["credit", "card"],
    &["cvv"],
    &["cvc"],
    &["csc"],
    &["security", "code"],
    &["card", "code"],
    &["cc", "exp"],
];

/// A CSS selector for the cases the probe can find without looking at values or names.
pub const SENSITIVE_SELECTOR: &str = r#"input[type="password" i], input[autocomplete^="cc-" i], input[autocomplete*=" cc-" i], textarea[autocomplete^="cc-" i], textarea[autocomplete*=" cc-" i], select[autocomplete^="cc-" i], select[autocomplete*=" cc-" i]"#;

/// The attributes the name pattern is tested against.
pub const NAME_ATTRIBUTES: [&str; 4] = ["name", "id", "aria-label", "placeholder"];

/// Digit counts of a card number.
pub const CARD_DIGITS: std::ops::RangeInclusive<usize> = 13..=19;

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
    let tag = f.tag.trim();
    if (tag.is_empty() || tag.eq_ignore_ascii_case("input")) && f.type_.trim().eq_ignore_ascii_case("password") {
        return true;
    }
    if autocomplete_is_card(f.autocomplete) {
        return true;
    }
    if [f.name, f.id, f.aria_label, f.placeholder].iter().any(|s| name_matches(s)) {
        return true;
    }
    is_card_number(f.value)
}

/// An `autocomplete` value with a `cc-*` token (`cc-number`, `billing cc-exp`).
pub fn autocomplete_is_card(autocomplete: &str) -> bool {
    autocomplete.split_ascii_whitespace().any(|t| t.len() > 3 && t[..3].eq_ignore_ascii_case("cc-"))
}

/// Does `s` match [`NAME_PATTERN`] anywhere (case-insensitive)?
pub fn name_matches(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let lower: Vec<char> = s.to_lowercase().chars().collect();
    (0..lower.len()).any(|i| NAME_ALTERNATIVES.iter().any(|alt| match_segments(&lower[i..], alt)))
}

/// `segments[0]` at the start of `text`, then each further segment after zero or one character.
fn match_segments(text: &[char], segments: &[&str]) -> bool {
    let Some((first, rest)) = segments.split_first() else { return true };
    let n = first.chars().count();
    if text.len() < n || !text.iter().zip(first.chars()).all(|(a, b)| *a == b) {
        return false;
    }
    let after = &text[n..];
    match rest.split_first() {
        None => true,
        Some(_) => match_segments(after, rest) || (!after.is_empty() && match_segments(&after[1..], rest)),
    }
}

/// A value that is a card number: only digits, single spaces or dashes between them, 13–19
/// digits, Luhn-valid.
pub fn is_card_number(value: &str) -> bool {
    let v = value.trim();
    if v.is_empty() || !v.starts_with(|c: char| c.is_ascii_digit()) || !v.ends_with(|c: char| c.is_ascii_digit()) {
        return false;
    }
    let mut prev_sep = false;
    for c in v.chars() {
        match c {
            '0'..='9' => prev_sep = false,
            ' ' | '-' if !prev_sep => prev_sep = true,
            _ => return false,
        }
    }
    let digits: Vec<u8> = v.bytes().filter(u8::is_ascii_digit).map(|b| b - b'0').collect();
    CARD_DIGITS.contains(&digits.len()) && luhn(&digits)
}

/// The Luhn checksum over decimal digits (most significant first).
pub fn luhn(digits: &[u8]) -> bool {
    if digits.is_empty() {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            let d = u32::from(d);
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 {
                    x - 9
                } else {
                    x
                }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// The rules as JSON for the probe:
///
/// ```json
/// {"selector": "input[type=\"password\" i], …", "namePattern": "card.?num|…",
///  "nameAttributes": ["name","id","aria-label","placeholder"], "luhn": true,
///  "minDigits": 13, "maxDigits": 19, "redacted": "[redacted]"}
/// ```
///
/// The probe treats an element as sensitive when it matches `selector`, when one of
/// `nameAttributes` matches `new RegExp(namePattern, "i")`, or (`luhn`) when its value is a
/// Luhn-valid run of `minDigits`–`maxDigits` digits (single spaces or dashes allowed).
pub fn probe_rules() -> String {
    serde_json::json!({
        "selector": SENSITIVE_SELECTOR,
        "namePattern": NAME_PATTERN,
        "nameAttributes": NAME_ATTRIBUTES,
        "luhn": true,
        "minDigits": CARD_DIGITS.start(),
        "maxDigits": CARD_DIGITS.end(),
        "redacted": REDACTED,
    })
    .to_string()
}

// ---- HTML ----

/// One attribute of a scanned tag: its lower-case name, its raw value and the byte span of the
/// whole `name=value` text in the source.
struct Attr<'a> {
    name: String,
    value: &'a str,
    has_value: bool,
    start: usize,
    end: usize,
}

/// Scans the attributes of the tag whose name ends at `i`; returns them and the index just
/// after the closing `>` (or the end of the text).
fn scan_attrs(html: &str, mut i: usize) -> (Vec<Attr<'_>>, usize) {
    let b = html.as_bytes();
    let mut attrs = Vec::new();
    loop {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        if i >= b.len() {
            return (attrs, i);
        }
        if b[i] == b'>' {
            return (attrs, i + 1);
        }
        let start = i;
        // An attribute name: up to whitespace, '=', '>' or '/' (a stray quote is part of it).
        while i < b.len() && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'>' | b'/') {
            i += 1;
        }
        if i == start {
            // A lone '=': skip it.
            i += 1;
            continue;
        }
        let name = html[start..i].to_ascii_lowercase();
        let mut j = i;
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        if j < b.len() && b[j] == b'=' {
            j += 1;
            while j < b.len() && b[j].is_ascii_whitespace() {
                j += 1;
            }
            let (value, end) = if j < b.len() && (b[j] == b'"' || b[j] == b'\'') {
                let q = b[j];
                let vs = j + 1;
                let ve = html[vs..].bytes().position(|c| c == q).map_or(b.len(), |p| vs + p);
                (&html[vs..ve], (ve + 1).min(b.len()))
            } else {
                let vs = j;
                let mut ve = j;
                while ve < b.len() && !b[ve].is_ascii_whitespace() && b[ve] != b'>' {
                    ve += 1;
                }
                (&html[vs..ve], ve)
            };
            attrs.push(Attr { name, value, has_value: true, start, end });
            i = end;
        } else {
            attrs.push(Attr { name, value: "", has_value: false, start, end: i });
        }
    }
}

/// Decodes the character references a value is likely to use to hide digits or words.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        let end = rest.find(';').filter(|&e| e <= 12);
        let decoded = end.and_then(|e| {
            let ent = &rest[1..e];
            let c = if let Some(h) = ent.strip_prefix("#x").or_else(|| ent.strip_prefix("#X")) {
                u32::from_str_radix(h, 16).ok().and_then(char::from_u32)
            } else if let Some(d) = ent.strip_prefix('#') {
                d.parse::<u32>().ok().and_then(char::from_u32)
            } else {
                match ent {
                    "amp" => Some('&'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "nbsp" => Some(' '),
                    _ => None,
                }
            };
            c.map(|c| (c, e + 1))
        });
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The field description of a scanned tag (the first of duplicate attributes counts, as in
/// HTML). Values are entity-decoded first.
fn field_sensitive(tag: &str, attrs: &[Attr], content: Option<&str>) -> bool {
    let get = |n: &str| attrs.iter().find(|a| a.name == n && a.has_value).map(|a| decode_entities(a.value));
    let type_ = get("type").unwrap_or_default();
    let autocomplete = get("autocomplete").unwrap_or_default();
    let name = get("name").unwrap_or_default();
    let id = get("id").unwrap_or_default();
    let aria = get("aria-label").unwrap_or_default();
    let placeholder = get("placeholder").unwrap_or_default();
    let value = match content {
        Some(c) => decode_entities(c),
        None => get("value").unwrap_or_default(),
    };
    is_sensitive(&FieldAttrs {
        tag,
        type_: &type_,
        autocomplete: &autocomplete,
        name: &name,
        id: &id,
        aria_label: &aria,
        placeholder: &placeholder,
        value: &value,
    })
}

/// The tag name starting at `i` (just after `<`) if it is `want` followed by a delimiter.
fn tag_is(html: &str, i: usize, want: &str) -> bool {
    let b = html.as_bytes();
    let n = want.len();
    i + n <= b.len()
        && b[i..i + n].eq_ignore_ascii_case(want.as_bytes())
        && b.get(i + n).is_none_or(|c| c.is_ascii_whitespace() || *c == b'>' || *c == b'/')
}

/// Blanks the `value` attribute of sensitive `<input>` tags (and the text of sensitive
/// `<textarea>`s) in an HTML fragment.
///
/// A tolerant scan, not a parser: attribute order, quoting style and case don't matter;
/// comments are skipped. Every `value` attribute of a sensitive input becomes
/// `value="[redacted]"`.
pub fn redact_html(html: &str) -> String {
    let b = html.as_bytes();
    let mut out = String::with_capacity(html.len());
    let mut copied = 0; // bytes of `html` already in `out`
    let mut i = 0;
    while let Some(p) = html[i..].find('<') {
        let lt = i + p;
        if html[lt..].starts_with("<!--") {
            i = html[lt + 4..].find("-->").map_or(b.len(), |e| lt + 4 + e + 3);
            continue;
        }
        if tag_is(html, lt + 1, "input") {
            let (attrs, end) = scan_attrs(html, lt + 6);
            if field_sensitive("input", &attrs, None) {
                for a in attrs.iter().filter(|a| a.name == "value" && a.has_value) {
                    out.push_str(&html[copied..a.start]);
                    out.push_str("value=\"");
                    out.push_str(REDACTED);
                    out.push('"');
                    copied = a.end;
                }
            }
            i = end.max(lt + 1);
        } else if tag_is(html, lt + 1, "textarea") {
            let (attrs, end) = scan_attrs(html, lt + 9);
            let close = find_ci(&html[end..], "</textarea").map_or(b.len(), |c| end + c);
            let content = &html[end..close];
            if !content.trim().is_empty() && field_sensitive("textarea", &attrs, Some(content)) {
                out.push_str(&html[copied..end]);
                out.push_str(REDACTED);
                copied = close;
            }
            i = close.max(lt + 1);
        } else {
            i = lt + 1;
        }
        if i >= b.len() {
            break;
        }
    }
    out.push_str(&html[copied..]);
    out
}

/// Case-insensitive ASCII search.
fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.len() > h.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

// ---- scrubbing ----

/// Replaces each of `secrets` (exact strings of 3+ characters) and every Luhn-valid run of
/// 13–19 digits (spaces and dashes allowed between digits) with [`REDACTED`].
///
/// Secrets are matched longest first in one pass, so a replacement is never matched again.
/// A card run is a sequence of digit groups separated by one space or dash; when it has
/// several groups, each group has at least 3 digits (card formats are 4-4-4-4, 4-6-5, …), so
/// page numbers, dates, times and phone numbers are left alone.
pub fn scrub(text: &str, secrets: &[Zeroizing<String>]) -> String {
    let mut list: Vec<&str> = secrets.iter().map(|s| s.as_str()).filter(|s| s.chars().count() >= 3).collect();
    list.sort_by_key(|s| std::cmp::Reverse(s.len()));
    list.dedup();
    let first = if list.is_empty() {
        text.to_string()
    } else {
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        while i < text.len() {
            let rest = &text[i..];
            if let Some(s) = list.iter().find(|s| rest.starts_with(**s)) {
                out.push_str(REDACTED);
                i += s.len();
            } else {
                let c = rest.chars().next().map_or(1, char::len_utf8);
                out.push_str(&rest[..c]);
                i += c;
            }
        }
        out
    };
    scrub_cards(&first)
}

/// Replaces Luhn-valid card numbers (see [`scrub`]).
fn scrub_cards(text: &str) -> String {
    let b = text.as_bytes();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        // A run: digit groups joined by single separators.
        let mut groups: Vec<(usize, usize)> = Vec::new();
        let mut j = i;
        loop {
            let gs = j;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            groups.push((gs, j));
            if j + 1 < b.len() && (b[j] == b' ' || b[j] == b'-') && b[j + 1].is_ascii_digit() {
                j += 1;
            } else {
                break;
            }
        }
        for s in 0..groups.len() {
            let mut digits: Vec<u8> = Vec::new();
            for (e, &(gs, ge)) in groups.iter().enumerate().skip(s) {
                if e > s && (groups[s].1 - groups[s].0 < 3 || ge - gs < 3) {
                    break;
                }
                digits.extend(b[gs..ge].iter().map(|c| c - b'0'));
                if digits.len() > *CARD_DIGITS.end() {
                    break;
                }
                if CARD_DIGITS.contains(&digits.len()) && luhn(&digits) {
                    spans.push((groups[s].0, ge));
                }
            }
        }
        i = j.max(i + 1);
    }
    if spans.is_empty() {
        return text.to_string();
    }
    spans.sort_unstable();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut cur: Option<(usize, usize)> = None;
    for (s, e) in spans {
        match cur {
            Some((cs, ce)) if s <= ce => cur = Some((cs, ce.max(e))),
            Some((cs, ce)) => {
                out.push_str(&text[copied..cs]);
                out.push_str(REDACTED);
                copied = ce;
                cur = Some((s, e));
            }
            None => cur = Some((s, e)),
        }
    }
    if let Some((cs, ce)) = cur {
        out.push_str(&text[copied..cs]);
        out.push_str(REDACTED);
        copied = ce;
    }
    out.push_str(&text[copied..]);
    out
}

// ---- paging ----

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

#[cfg(test)]
mod tests {
    use super::*;

    const VISA: &str = "4111111111111111";
    const AMEX: &str = "378282246310005";

    fn f<'a>(type_: &'a str, name: &'a str, value: &'a str) -> FieldAttrs<'a> {
        FieldAttrs { tag: "input", type_, name, value, ..Default::default() }
    }

    #[test]
    fn luhn_and_card_numbers() {
        assert!(luhn(&[4, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]));
        assert!(!luhn(&[4, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2]));
        assert!(is_card_number(VISA));
        assert!(is_card_number("4111 1111 1111 1111"));
        assert!(is_card_number("4111-1111-1111-1111"));
        assert!(is_card_number(AMEX));
        assert!(is_card_number(" 3782 822463 10005 "));
        assert!(!is_card_number("4111111111111112"), "checksum");
        assert!(!is_card_number("4111  1111 1111 1111"), "double space");
        assert!(!is_card_number("411111111111"), "12 digits");
        assert!(!is_card_number("41111111111111111111"), "20 digits");
        assert!(!is_card_number("4111x1111 1111 1111"));
        assert!(!is_card_number(""));
    }

    #[test]
    fn name_pattern() {
        for yes in [
            "cardnumber",
            "card_number",
            "card-num",
            "CardNum",
            "ccnum",
            "cc_no",
            "cc-number",
            "ccNo",
            "credit-card",
            "creditcard",
            "cvv",
            "CVC2",
            "csc",
            "security_code",
            "securitycode",
            "Security Code",
            "card-code",
            "cc-exp",
            "ccexp-month",
            "billing[cardNumber]",
            "Enter your CVV",
        ] {
            assert!(name_matches(yes), "{yes}");
        }
        for no in ["email", "name", "phone", "account_no", "success", "zip", "discount_code", "q", "", "card", "code"] {
            assert!(!name_matches(no), "{no}");
        }
    }

    #[test]
    fn sensitive_fields() {
        assert!(is_sensitive(&f("password", "x", "")));
        assert!(is_sensitive(&f("PASSWORD", "x", "")));
        assert!(is_sensitive(&f(" Password ", "x", "")));
        assert!(!is_sensitive(&f("text", "user", "ann")));
        assert!(is_sensitive(&FieldAttrs { autocomplete: "cc-number", ..Default::default() }));
        assert!(is_sensitive(&FieldAttrs { autocomplete: "section-x cc-number", ..Default::default() }));
        assert!(is_sensitive(&FieldAttrs { autocomplete: "billing CC-CSC", ..Default::default() }));
        assert!(!is_sensitive(&FieldAttrs { autocomplete: "email", ..Default::default() }));
        assert!(!is_sensitive(&FieldAttrs { autocomplete: "acc-number", ..Default::default() }));
        assert!(is_sensitive(&FieldAttrs { id: "cvc", ..Default::default() }));
        assert!(is_sensitive(&FieldAttrs { aria_label: "Card number", ..Default::default() }));
        assert!(is_sensitive(&FieldAttrs { placeholder: "1234 5678 9012 3456 (credit card)", ..Default::default() }));
        assert!(is_sensitive(&f("text", "ref", VISA)), "Luhn value");
        assert!(!is_sensitive(&f("text", "ref", "4111111111111112")));
        assert!(!is_sensitive(&f("tel", "phone", "+1 555 0100")));
    }

    #[test]
    fn probe_rules_json() {
        let v: serde_json::Value = serde_json::from_str(&probe_rules()).unwrap();
        assert_eq!(v["namePattern"], NAME_PATTERN);
        assert_eq!(v["luhn"], true);
        assert_eq!(v["minDigits"], 13);
        assert_eq!(v["maxDigits"], 19);
        assert_eq!(v["redacted"], REDACTED);
        assert_eq!(v["nameAttributes"].as_array().unwrap().len(), 4);
        let sel = v["selector"].as_str().unwrap();
        assert!(sel.contains(r#"input[type="password" i]"#) && sel.contains("cc-"));
        // Every alternative of the Rust twin is in the JS pattern.
        for alt in NAME_ALTERNATIVES {
            assert!(NAME_PATTERN.contains(alt[0]), "{}", alt[0]);
        }
    }

    #[test]
    fn redact_html_variants() {
        let cases = [
            (r#"<input type="password" value="hunter2">"#, r#"<input type="password" value="[redacted]">"#),
            (r#"<input value="hunter2" type="password">"#, r#"<input value="[redacted]" type="password">"#),
            ("<input type='password' value='hunter2'/>", r#"<input type='password' value="[redacted]"/>"#),
            ("<INPUT TYPE=PASSWORD VALUE=hunter2>", r#"<INPUT TYPE=PASSWORD value="[redacted]">"#),
            ("<input\ntype = \"password\"\n value = \"a b\" >", "<input\ntype = \"password\"\n value=\"[redacted]\" >"),
            (
                r#"<input autocomplete="section-x cc-number" value="4111 1111 1111 1111">"#,
                r#"<input autocomplete="section-x cc-number" value="[redacted]">"#,
            ),
            (r#"<input name="card_number" value="1234">"#, r#"<input name="card_number" value="[redacted]">"#),
            (r#"<input id="cvv" value="123">"#, r#"<input id="cvv" value="[redacted]">"#),
            (r#"<input name="ref" value="4111111111111111">"#, r#"<input name="ref" value="[redacted]">"#),
            (r#"<input name="ref" value="411111111111&#49;111">"#, r#"<input name="ref" value="[redacted]">"#),
            (r#"<input type="&#112;assword" value="x">"#, r#"<input type="&#112;assword" value="[redacted]">"#),
            (r#"<input type="password">"#, r#"<input type="password">"#),
            (r#"<input type="password" value>"#, r#"<input type="password" value>"#),
        ];
        for (input, want) in cases {
            assert_eq!(redact_html(input), want, "{input}");
        }
        // Several inputs, only the sensitive ones change; text around stays.
        let html = r#"<form><label>Name <input name="name" value="Ann"></label><input type=password value=s3cret><p>x > y</p><input type="text" value="4111111111111112"></form>"#;
        let out = redact_html(html);
        assert!(out.contains(r#"value="Ann""#) && !out.contains("s3cret") && out.contains("4111111111111112"));
        assert!(out.contains("<p>x > y</p>"));
        // A quoted '>' inside an attribute doesn't end the tag.
        let out = redact_html(r#"<input data-x="a>b" type="password" value="pw1">"#);
        assert!(!out.contains("pw1"), "{out}");
        // Duplicate value attributes: all are blanked.
        let out = redact_html(r#"<input type=password value="a1" value="b2">"#);
        assert!(!out.contains("a1") && !out.contains("b2"), "{out}");
        // Comments are skipped; non-inputs untouched.
        let html = r#"<!-- <input type="password" value="old"> --><inputs value="x"><img alt="password" value="y">"#;
        assert_eq!(redact_html(html), html);
        // Unterminated tags and odd input don't panic.
        assert_eq!(redact_html("<input type=password value=\"abc"), "<input type=password value=\"[redacted]\"");
        assert_eq!(redact_html("<textarea name=cvv>123"), "<textarea name=cvv>[redacted]");
        for odd in ["<input", "<", "<input =x>", "<<input>>", "<textarea", "<input type=password value"] {
            assert_eq!(redact_html(odd), odd);
        }
        assert_eq!(redact_html("é<input type=password value=é>é"), "é<input type=password value=\"[redacted]\">é");
    }

    #[test]
    fn redact_textarea() {
        assert_eq!(
            redact_html(r#"<textarea name="card-number">4111 1111 1111 1111</textarea>"#),
            r#"<textarea name="card-number">[redacted]</textarea>"#
        );
        assert_eq!(redact_html("<TEXTAREA>4111111111111111</TEXTAREA>"), "<TEXTAREA>[redacted]</TEXTAREA>");
        let plain = r#"<textarea name="comment">Hello 1234</textarea>"#;
        assert_eq!(redact_html(plain), plain);
        assert_eq!(redact_html(r#"<textarea id=cvc></textarea>"#), r#"<textarea id=cvc></textarea>"#);
    }

    #[test]
    fn redact_html_no_false_positives() {
        for html in [
            r#"<input type="text" name="email" value="ann@example.com">"#,
            r#"<input type="tel" name="phone" value="+1 555 0100">"#,
            r#"<input type="date" value="2026-11-10">"#,
            r#"<input type="hidden" name="csrf" value="abc123">"#,
            r#"<input name="discount_code" value="SAVE10">"#,
            r#"<a href="/password-reset">Forgot password?</a>"#,
            r#"<input type="search" placeholder="Search" value="password manager">"#,
        ] {
            assert_eq!(redact_html(html), html);
        }
    }

    fn secrets(list: &[&str]) -> Vec<Zeroizing<String>> {
        list.iter().map(|s| Zeroizing::new(s.to_string())).collect()
    }

    #[test]
    fn scrub_secrets() {
        let s = secrets(&["hunter2", "hu", "hunter22"]);
        assert_eq!(scrub("pw: hunter22 and hunter2, hu", &s), "pw: [redacted] and [redacted], hu");
        // A replacement is never matched again ("red" is inside "[redacted]").
        assert_eq!(scrub("red hunter2", &secrets(&["red", "hunter2"])), "[redacted] [redacted]");
        assert_eq!(scrub("ünïcode secret ü", &secrets(&["secret"])), "ünïcode [redacted] ü");
        assert_eq!(scrub("nothing here", &[]), "nothing here");
        assert_eq!(scrub("ab ab", &secrets(&["ab"])), "ab ab", "shorter than 3");
    }

    #[test]
    fn scrub_card_numbers() {
        let none: Vec<Zeroizing<String>> = Vec::new();
        assert_eq!(scrub("card 4111111111111111.", &none), "card [redacted].");
        assert_eq!(scrub("card 4111 1111 1111 1111 exp 12/26", &none), "card [redacted] exp 12/26");
        assert_eq!(scrub("4111-1111-1111-1111", &none), "[redacted]");
        assert_eq!(scrub("amex 3782 822463 10005", &none), "amex [redacted]");
        assert_eq!(scrub(r#"{"v":"4111111111111111"}"#, &none), r#"{"v":"[redacted]"}"#);
        // A short group in front doesn't hide the card.
        assert_eq!(scrub("No 12 4111 1111 1111 1111", &none), "No 12 [redacted]");
        assert_eq!(scrub("4111 1111 1111 1111 2026", &none).matches(REDACTED).count(), 1);
        // Left alone: wrong checksum, short numbers, dates, phone numbers, page lists, long ids.
        for keep in [
            "4111111111111112",
            "order 123456",
            "2026-11-10 12:30",
            "+1 555 0100",
            "call +44 20 7946 0958",
            "1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20",
            "id 12345678901234567890123",
            "ISBN 978-3-16-148410-0",
        ] {
            assert_eq!(scrub(keep, &none), keep, "{keep}");
        }
    }

    #[test]
    fn caps() {
        let p = cap("abcdef", 2, 3);
        assert_eq!((p.text.as_str(), p.truncated, p.next_offset, p.total), ("cde", true, 5, 6));
        assert!(p.into_result().ends_with(r#"{"next_offset":5,"total":6,"truncated":true}"#));
        let p = cap("äöü", 0, 10);
        assert!(!p.truncated);
        assert_eq!(p.into_result(), "äöü");
    }
}
