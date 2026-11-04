//! uBlock Origin's filter-list preprocessor: `!#if` / `!#else` / `!#endif`
//! blocks and `!#include` of sub-lists.
//!
//! The `adblock` crate reads `!#` lines as comments, so without this every
//! platform's rules would apply (Firefox-only, mobile-only, uBO Lite-only).
//! [`preprocess`] keeps what applies to Krunk: desktop Chromium, without
//! uBO's extension-only capabilities. [`inline_includes`] replaces each
//! `!#include name.txt` line with the sub-list's text (fetched from the list's
//! own directory) before preprocessing, so an include inside an inactive
//! `!#if` block is dropped as uBO drops it.

use std::borrow::Cow;

/// Preprocessor tokens that are true for Krunk. Everything else (including
/// unknown tokens) is false.
const TRUE_TOKENS: &[&str] = &["env_chromium", "cap_user_stylesheet"];

/// Largest sub-list accepted by [`inline_includes`].
pub const MAX_INCLUDE_BYTES: usize = 8 * 1024 * 1024;
/// Most `!#include` lines followed in one list.
pub const MAX_INCLUDES: usize = 32;

/// Evaluates a `!#if` condition: tokens, `!`, `&&`, `||` and parentheses.
/// Malformed expressions are false.
pub fn condition_holds(expr: &str) -> bool {
    let tokens = tokenize(expr);
    let mut pos = 0;
    match parse_or(&tokens, &mut pos) {
        Some(v) if pos == tokens.len() => v,
        _ => false,
    }
}

fn tokenize(expr: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = expr.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if (c == '&' || c == '|') && chars.get(i + 1) == Some(&c) {
            out.push(format!("{c}{c}"));
            i += 2;
        } else if c == '!' || c == '(' || c == ')' {
            out.push(c.to_string());
            i += 1;
        } else {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            if i == start {
                // Something we do not understand: make the expression fail.
                out.push("?".into());
                i += 1;
            } else {
                out.push(chars[start..i].iter().collect());
            }
        }
    }
    out
}

fn parse_or(t: &[String], pos: &mut usize) -> Option<bool> {
    let mut v = parse_and(t, pos)?;
    while t.get(*pos).is_some_and(|s| s == "||") {
        *pos += 1;
        let r = parse_and(t, pos)?;
        v = v || r;
    }
    Some(v)
}

fn parse_and(t: &[String], pos: &mut usize) -> Option<bool> {
    let mut v = parse_not(t, pos)?;
    while t.get(*pos).is_some_and(|s| s == "&&") {
        *pos += 1;
        let r = parse_not(t, pos)?;
        v = v && r;
    }
    Some(v)
}

fn parse_not(t: &[String], pos: &mut usize) -> Option<bool> {
    let tok = t.get(*pos)?;
    if tok == "!" {
        *pos += 1;
        return parse_not(t, pos).map(|v| !v);
    }
    if tok == "(" {
        *pos += 1;
        let v = parse_or(t, pos)?;
        if t.get(*pos).is_some_and(|s| s == ")") {
            *pos += 1;
            return Some(v);
        }
        return None;
    }
    if tok.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        *pos += 1;
        return Some(TRUE_TOKENS.contains(&tok.as_str()));
    }
    None
}

/// Drops the lines of inactive `!#if` branches (and the directives
/// themselves). Unbalanced `!#endif`s are ignored; an unterminated `!#if`
/// runs to the end of the list, as in uBO.
pub fn preprocess(text: &str) -> Cow<'_, str> {
    if !text.contains("!#if") {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    // For each open block: is this branch active, and was the parent active?
    let mut stack: Vec<(bool, bool)> = Vec::new();
    let active = |stack: &[(bool, bool)]| stack.last().is_none_or(|&(on, parent)| on && parent);
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(expr) = trimmed.strip_prefix("!#if ") {
            let parent = active(&stack);
            stack.push((condition_holds(expr), parent));
            continue;
        }
        if trimmed == "!#else" {
            if let Some(top) = stack.last_mut() {
                top.0 = !top.0;
            }
            continue;
        }
        if trimmed == "!#endif" {
            stack.pop();
            continue;
        }
        if active(&stack) {
            out.push_str(line);
            out.push('\n');
        }
    }
    Cow::Owned(out)
}

/// The sub-list names a list includes (`!#include filters-2025.txt`), in
/// order. Only plain file names in the list's own directory are followed.
pub fn includes(text: &str) -> Vec<String> {
    text.lines().filter_map(|l| include_name(l.trim())).map(str::to_string).collect()
}

fn include_name(line: &str) -> Option<&str> {
    let name = line.strip_prefix("!#include ")?.trim();
    let ok = !name.is_empty()
        && name.len() <= 100
        && name.ends_with(".txt")
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    ok.then_some(name)
}

/// The URL of sub-list `name` next to `list_url`.
pub fn sibling_url(list_url: &str, name: &str) -> Option<String> {
    let base = url::Url::parse(list_url).ok()?;
    let u = base.join(name).ok()?;
    // Same scheme, host and directory only.
    (u.scheme() == "https" && u.origin() == base.origin()).then(|| u.to_string())
}

/// Replaces every followed `!#include` line with the sub-list's text, as
/// returned by `fetch(name)`. A sub-list that cannot be fetched fails the
/// whole list, so a partial list never replaces a complete older copy.
pub fn inline_includes(text: &str, fetch: impl Fn(&str) -> Result<String, String>) -> Result<String, String> {
    if includes(text).len() > MAX_INCLUDES {
        return Err("too many included lists".into());
    }
    let mut out = String::with_capacity(text.len() * 4);
    for line in text.lines() {
        match include_name(line.trim()) {
            Some(name) => {
                let sub = fetch(name).map_err(|e| format!("{name}: {e}"))?;
                if sub.len() > MAX_INCLUDE_BYTES {
                    return Err(format!("{name}: too large"));
                }
                out.push_str(&format!("! >>> {name}\n"));
                // Sub-lists may not include further lists.
                for l in sub.lines().filter(|l| include_name(l.trim()).is_none()) {
                    out.push_str(l);
                    out.push('\n');
                }
                out.push_str(&format!("! <<< {name}\n"));
            }
            None => {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    Ok(out)
}

/// A downloaded sub-list is text, not an HTML error page or binary data.
pub fn validate_include(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > MAX_INCLUDE_BYTES {
        return Err("too large".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "not UTF-8 text".to_string())?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let head = text[..text.len().min(512)].to_ascii_lowercase();
    if head.trim_start().starts_with('<') || head.contains("<html") {
        return Err("looks like an HTML page".into());
    }
    if text.contains('\0') {
        return Err("binary data".into());
    }
    Ok(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditions() {
        assert!(condition_holds("env_chromium"));
        assert!(!condition_holds("env_firefox"));
        assert!(condition_holds("!env_mobile"));
        assert!(!condition_holds("ext_ubol"));
        assert!(!condition_holds("cap_html_filtering"));
        assert!(condition_holds("env_chromium && !env_mobile"));
        assert!(condition_holds("env_firefox || env_chromium"));
        assert!(!condition_holds("!(env_chromium || env_firefox)"));
        assert!(!condition_holds("unknown_token"));
        assert!(!condition_holds("env_chromium &&"), "malformed is false");
        assert!(!condition_holds("(env_chromium"));
        assert!(!condition_holds("env_chromium $ x"));
    }

    #[test]
    fn blocks() {
        let list = "\
a.com##.ad
!#if env_firefox
firefox.only##.x
!#else
not.firefox##.y
!#endif
!#if !env_mobile
desktop##.d
!#if env_safari
nested.safari##.n
!#endif
!#endif
!#if ext_ubol
!#if env_chromium
inner.of.inactive##.z
!#endif
!#endif
b.com##.ad
";
        let out = preprocess(list);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines, ["a.com##.ad", "not.firefox##.y", "desktop##.d", "b.com##.ad"]);
        assert!(matches!(preprocess("x.com##.ad\n"), Cow::Borrowed(_)));
    }

    #[test]
    fn includes_are_inlined_in_place() {
        let list =
            "! Title: t\n!#include filters-2025.txt\n!#if env_mobile\n!#include filters-mobile.txt\n!#endif\nend##.x\n";
        assert_eq!(includes(list), ["filters-2025.txt", "filters-mobile.txt"]);
        let inlined = inline_includes(list, |name| match name {
            "filters-2025.txt" => Ok("y.com##+js(set-constant, a, 1)\n!#include nested.txt\n".into()),
            "filters-mobile.txt" => Ok("mobile##.m\n".into()),
            _ => Err("404".into()),
        })
        .unwrap();
        assert!(inlined.contains("y.com##+js(set-constant, a, 1)"));
        assert!(!inlined.contains("nested.txt"), "sub-lists do not include further");
        // The mobile include stays inside its block and is dropped by preprocessing.
        let out = preprocess(&inlined);
        assert!(out.contains("y.com##+js") && out.contains("end##.x") && !out.contains("mobile##.m"));
        assert!(inline_includes(list, |_| Err("offline".into())).is_err(), "a missing sub-list fails the list");
    }

    #[test]
    fn only_plain_sibling_names() {
        for bad in [
            "!#include ../secret.txt",
            "!#include https://evil.example/x.txt",
            "!#include sub/dir.txt",
            "!#include .hidden.txt",
            "!#include list.js",
            "!#include ",
        ] {
            assert!(includes(bad).is_empty(), "{bad}");
        }
        assert_eq!(
            sibling_url("https://ublockorigin.github.io/uAssets/filters/filters.txt", "filters-2025.txt").as_deref(),
            Some("https://ublockorigin.github.io/uAssets/filters/filters-2025.txt")
        );
        assert_eq!(sibling_url("http://insecure.example/a/list.txt", "b.txt"), None);
    }

    #[test]
    fn sub_list_validation() {
        assert!(validate_include(b"! small list\nx##.y\n").is_ok(), "sub-lists may be small");
        assert!(validate_include(b"<!DOCTYPE html><html>").is_err());
        assert!(validate_include(b"a\0b").is_err());
        assert!(validate_include(&[0xff, 0xfe]).is_err());
    }
}
