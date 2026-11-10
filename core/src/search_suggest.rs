//! Search suggestions in the address bar (Krunk 0.15.0), like Firefox: what the selected
//! search engine's suggest endpoint completes the typed words to ("why do" → "why do cats
//! purr"). This module knows each engine's endpoint, decides when asking is allowed at all
//! and parses the answers defensively; the host (`app/src/browser/omnibox.rs`) makes the
//! request without cookies and `crate::omnibox::with_search_suggestions` adds the rows.

use crate::settings::SearchEngine;
use crate::urlclean;

/// The biggest answer read (the request is cancelled past it).
pub const MAX_BODY: usize = 64 * 1024;
/// Suggestions kept from one answer.
pub const MAX_ITEMS: usize = 8;
/// Characters per suggestion (longer ones are dropped, not cut).
pub const MAX_CHARS: usize = 200;
/// Typed text longer than this is never sent.
pub const MAX_QUERY_CHARS: usize = 200;

/// The engine's suggest endpoint (`{searchTerms}` is the query); `None` for a custom engine.
/// All answer in the OpenSearch suggestions format, `["query", ["suggestion", …], …]`
/// (checked 2026-11-10; Brave's from its opensearch.xml).
pub fn endpoint(engine: SearchEngine) -> Option<&'static str> {
    Some(match engine {
        SearchEngine::DuckDuckGo => "https://duckduckgo.com/ac/?q={searchTerms}&type=list",
        SearchEngine::Google => {
            "https://suggestqueries.google.com/complete/search?client=firefox&oe=utf-8&q={searchTerms}"
        }
        SearchEngine::Bing => "https://api.bing.com/osjson.aspx?query={searchTerms}",
        SearchEngine::Brave => "https://search.brave.com/api/suggest?q={searchTerms}",
        SearchEngine::Startpage => "https://www.startpage.com/osuggestions?q={searchTerms}",
        SearchEngine::Ecosia => "https://ac.ecosia.org/autocomplete?q={searchTerms}&type=list",
        SearchEngine::Custom => return None,
    })
}

/// The suggest URL for `query` (form-encoded like the search itself).
pub fn suggest_url(engine: SearchEngine, query: &str) -> Option<String> {
    let encoded: String = url::form_urlencoded::byte_serialize(query.trim().as_bytes()).collect();
    endpoint(engine).map(|t| t.replace("{searchTerms}", &encoded))
}

/// Typed prefixes that are never words: Krunk's pages, schemes being typed.
const NOT_WORDS: &[&str] =
    &["krunk:", "view-source:", "about:", "file:", "javascript:", "data:", "blob:", "http:", "https:", "chrome:"];

/// Whether the engine may be asked about `text`: something Enter would search for (not an
/// address, not a Krunk page, not a scheme being typed), not blank, not too long.
pub fn wanted(text: &str, search_template: &str) -> bool {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > MAX_QUERY_CHARS {
        return false;
    }
    let lower = text.to_lowercase();
    if NOT_WORDS.iter().any(|p| lower.starts_with(p)) || lower.contains("://") {
        return false;
    }
    urlclean::input_to_url(text, search_template) == urlclean::search_url(text, search_template)
}

/// The suggestions in an answer: the OpenSearch array (`[query, [items…], …]`, items as
/// strings or as objects with `phrase`/`q`) or DuckDuckGo's `[{"phrase": …}, …]`. At most
/// [`MAX_ITEMS`], control characters removed, blank, overlong and repeated ones dropped.
/// Anything else (HTML from a rate limit, oversized, malformed) gives nothing.
pub fn parse(body: &[u8]) -> Vec<String> {
    if body.len() > MAX_BODY {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(body);
    // (Google's JSON hijacking guard, should an endpoint add it)
    let text = text.trim_start_matches('\u{feff}');
    let text = match text.strip_prefix(")]}'") {
        Some(rest) => rest,
        None => text,
    };
    let Ok(serde_json::Value::Array(top)) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let items: &[serde_json::Value] = match top.as_slice() {
        [serde_json::Value::String(_), serde_json::Value::Array(items), ..] => items,
        all if all.iter().all(|v| v.is_object()) => all,
        _ => return Vec::new(),
    };
    let mut out: Vec<String> = Vec::new();
    for item in items {
        if out.len() >= MAX_ITEMS {
            break;
        }
        let raw = match item {
            serde_json::Value::String(s) => s.as_str(),
            serde_json::Value::Object(o) => match o.get("phrase").or_else(|| o.get("q")) {
                Some(serde_json::Value::String(s)) => s.as_str(),
                _ => continue,
            },
            _ => continue,
        };
        let Some(clean) = clean(raw) else { continue };
        if !out.iter().any(|o| o.to_lowercase() == clean.to_lowercase()) {
            out.push(clean);
        }
    }
    out
}

/// One suggestion as shown: control characters (and bidi overrides, which could reorder
/// the row's text) gone, whitespace collapsed; `None` when blank or longer than [`MAX_CHARS`].
fn clean(raw: &str) -> Option<String> {
    let bidi = |c: char| matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}');
    let s: String = raw.chars().map(|c| if c.is_control() || bidi(c) { ' ' } else { c }).collect();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    (!s.is_empty() && s.chars().count() <= MAX_CHARS).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DDG: &str = "https://duckduckgo.com/?q={searchTerms}";

    #[test]
    fn urls_per_engine() {
        use SearchEngine::*;
        let q = "why do & c++ café";
        let enc = "why+do+%26+c%2B%2B+caf%C3%A9";
        let cases = [
            (DuckDuckGo, format!("https://duckduckgo.com/ac/?q={enc}&type=list")),
            (Google, format!("https://suggestqueries.google.com/complete/search?client=firefox&oe=utf-8&q={enc}")),
            (Bing, format!("https://api.bing.com/osjson.aspx?query={enc}")),
            (Brave, format!("https://search.brave.com/api/suggest?q={enc}")),
            (Startpage, format!("https://www.startpage.com/osuggestions?q={enc}")),
            (Ecosia, format!("https://ac.ecosia.org/autocomplete?q={enc}&type=list")),
        ];
        for (engine, want) in cases {
            assert_eq!(suggest_url(engine, q).as_deref(), Some(want.as_str()), "{engine:?}");
        }
        assert_eq!(suggest_url(Custom, q), None, "a custom engine has no known endpoint");
        assert_eq!(suggest_url(Google, "  a  ").unwrap(), suggest_url(Google, "a").unwrap());
    }

    #[test]
    fn only_words_are_sent() {
        assert!(wanted("why do", DDG));
        assert!(wanted("rust", DDG));
        assert!(wanted("test.test", DDG), "a made-up TLD is searched, so it may be completed");
        assert!(!wanted("", DDG) && !wanted("   ", DDG));
        assert!(!wanted("example.com", DDG), "an address");
        assert!(!wanted("localhost:3000", DDG));
        assert!(!wanted("192.168.1.1", DDG));
        assert!(!wanted("krunk://settings", DDG));
        assert!(!wanted("krunk://nonexistent", DDG), "a Krunk page being typed, even unknown");
        assert!(!wanted("https:", DDG) && !wanted("view-source:x y", DDG) && !wanted("javascript:alert(1)", DDG));
        assert!(!wanted("ftp://files.example", DDG));
        assert!(!wanted(&"a".repeat(MAX_QUERY_CHARS + 1), DDG));
        assert!(wanted(&"a ".repeat(MAX_QUERY_CHARS / 2), DDG));
    }

    // One answer per engine, as they came from each endpoint on 2026-11-10 (shortened).
    #[test]
    fn parses_google() {
        let body = br#"["why do",["why do cats make biscuits","why do dogs eat grass"],[],{"google:suggestsubtypes":[[512],[512]]}]"#;
        assert_eq!(parse(body), ["why do cats make biscuits", "why do dogs eat grass"]);
        let utf8 = "[\"café\",[\"café near me\",\"café au lait\"],[],{}]";
        assert_eq!(parse(utf8.as_bytes()), ["café near me", "café au lait"]);
    }

    #[test]
    fn parses_bing() {
        let body = br#"["why do",["why do we yawn","why don't we"],[],[],{"google:suggestrelevance":[1300,1299]}]"#;
        assert_eq!(parse(body), ["why do we yawn", "why don't we"]);
    }

    #[test]
    fn parses_duckduckgo() {
        assert_eq!(parse(br#"["why do",["why do we dream","why don't we"]]"#), ["why do we dream", "why don't we"]);
        // Without `type=list`: a list of objects.
        assert_eq!(
            parse(br#"[{"phrase":"why do cats purr"},{"phrase":"why do we yawn"}]"#),
            ["why do cats purr", "why do we yawn"]
        );
    }

    #[test]
    fn parses_brave() {
        assert_eq!(
            parse(br#"["why do",["why do cats purr","why do we dream"]]"#),
            ["why do cats purr", "why do we dream"]
        );
        // The rich format (objects with `q`).
        let rich = br#"["why do",[{"is_entity":false,"q":"why do cats purr"},{"is_entity":true,"q":"Why Do Fools Fall in Love","name":"x"}]]"#;
        assert_eq!(parse(rich), ["why do cats purr", "Why Do Fools Fall in Love"]);
        // Its rate-limit page is HTML: nothing.
        assert!(parse(b"<!DOCTYPE html><html><head>").is_empty());
    }

    #[test]
    fn parses_startpage() {
        let body = br#"["caf\u00e9",["caf\u00e9 terrace at night","caf\u00e9 racer"]]"#;
        assert_eq!(parse(body), ["café terrace at night", "café racer"]);
        // An empty 204 answer.
        assert!(parse(b"").is_empty());
    }

    #[test]
    fn parses_ecosia() {
        let body = br#"["why do",["why do we yawn","why do cats purr","why do computers use binary"]]"#;
        assert_eq!(parse(body), ["why do we yawn", "why do cats purr", "why do computers use binary"]);
    }

    #[test]
    fn malformed_and_oversized_answers() {
        for bad in [
            &b"not json"[..],
            b"{}",
            b"[]",
            b"\"why\"",
            b"[\"why\"]",
            b"[\"why\", \"not a list\"]",
            b"[1, [\"a\"]]",
            b"[\"why\", [\"unterminated\"",
            b"[{\"other\": 1}]",
        ] {
            assert!(parse(bad).is_empty(), "{}", String::from_utf8_lossy(bad));
        }
        // Non-strings and blanks inside the list are skipped.
        assert_eq!(parse(br#"["q",[1,null,"  ",{"x":1},"ok",["nested"]]]"#), ["ok"]);
        // Over the size limit: nothing at all.
        let mut big = br#"["q",["a""#.to_vec();
        big.extend(std::iter::repeat_n(b'a', MAX_BODY));
        big.extend(br#""]]"#);
        assert!(parse(&big).is_empty());
        // At most MAX_ITEMS, each at most MAX_CHARS, control and bidi characters out, repeats once.
        let many: Vec<String> = (0..20).map(|i| format!("q {i}")).collect();
        let body = serde_json::to_vec(&serde_json::json!(["q", many])).unwrap();
        assert_eq!(parse(&body).len(), MAX_ITEMS);
        let body = serde_json::to_vec(&serde_json::json!([
            "q",
            ["x".repeat(MAX_CHARS + 1), "y".repeat(MAX_CHARS), "bell\u{7}\nand\ttab\u{202e}", "Same", "same", "SAME "]
        ]))
        .unwrap();
        let got = parse(&body);
        assert_eq!(got.len(), 3, "{got:?}");
        assert_eq!(got[0], "y".repeat(MAX_CHARS));
        assert_eq!(got[1], "bell and tab");
        assert_eq!(got[2], "Same");
        // A BOM and Google's hijacking guard are tolerated; invalid UTF-8 is replaced.
        assert_eq!(parse("\u{feff}[\"q\",[\"a\"]]".as_bytes()), ["a"]);
        assert_eq!(parse(b")]}'\n[\"q\",[\"b\"]]"), ["b"]);
        assert_eq!(parse(b"[\"q\",[\"c\xff\"]]"), ["c\u{fffd}"]);
    }
}
