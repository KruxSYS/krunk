//! Userscripts (0.12.0, WP-E): the `==UserScript==` metadata block, URL matching and the
//! per-tab loader the host registers with `Page.addScriptToEvaluateOnNewDocument`.
//!
//! Supported: `@name @namespace @version @description @match @include @exclude @run-at
//! @grant @noframes`. Grants: `none`, `GM_addStyle`, `GM_getValue`/`GM_setValue`/
//! `GM_deleteValue`/`GM_listValues`, `GM_setClipboard`, `GM_notification`, `GM_openInTab`
//! (and their `GM.*` promise forms; `unsafeWindow` is `window`). Not supported in 0.12.0
//! (warnings): `GM_xmlhttpRequest`, `@require`, `@resource`, regular-expression `@include`.
//!
//! The loader (`ui/inject/userscript-loader.js`) is a function expression; [`bundle`] and
//! [`bundle_parts`] call it with the binding name, the nonce, the version and the script ids,
//! then add each script as its own function, so a script never sees the loader's closure.

use crate::ext::{glob_match, MatchPattern};
use serde::{Deserialize, Serialize};

/// When a script runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunAt {
    DocumentStart,
    DocumentEnd,
    #[default]
    DocumentIdle,
}

impl RunAt {
    fn stage(self) -> u8 {
        match self {
            RunAt::DocumentStart => 0,
            RunAt::DocumentEnd => 1,
            RunAt::DocumentIdle => 2,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            RunAt::DocumentStart => "document-start",
            RunAt::DocumentEnd => "document-end",
            RunAt::DocumentIdle => "document-idle",
        }
    }
}

/// The parsed metadata block.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub name: String,
    pub namespace: String,
    pub version: String,
    pub description: String,
    pub matches: Vec<String>,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
    pub run_at: RunAt,
    pub grants: Vec<String>,
    pub noframes: bool,
    /// Unsupported features the script uses (shown in the editor).
    pub warnings: Vec<String>,
}

/// The GM functions the loader provides (as `GM_<name>` and `GM.<name>`).
const SUPPORTED_GM: &[&str] = &[
    "addStyle",
    "getValue",
    "setValue",
    "deleteValue",
    "listValues",
    "setClipboard",
    "notification",
    "openInTab",
    "info",
    // 0.19.0: the WASM memory inspector (`GM.wasm.read(…)` and the others, all promises).
    "wasm",
];

/// Grants that need no function from us.
const HARMLESS_GRANTS: &[&str] = &["none", "unsafeWindow", "window.close", "window.focus", "window.onurlchange"];

/// Parses the metadata block. Errors: no block, an unterminated block, no `@name`, no `@match`
/// or `@include`.
pub fn parse_meta(source: &str) -> Result<Meta, String> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let mut lines = source.lines().map(|l| l.trim());
    fn comment(l: &str) -> Option<&str> {
        l.strip_prefix("//").map(str::trim)
    }
    if !lines.by_ref().any(|l| comment(l) == Some("==UserScript==")) {
        return Err("No ==UserScript== block: add one at the top of the script.".into());
    }
    let mut meta = Meta::default();
    let mut warnings: Vec<String> = Vec::new();
    let mut warn = |w: String| {
        if !warnings.contains(&w) {
            warnings.push(w);
        }
    };
    let (mut closed, mut has_name, mut xhr) = (false, false, false);
    for line in lines {
        let Some(body) = comment(line) else { continue };
        if body == "==/UserScript==" {
            closed = true;
            break;
        }
        let Some(body) = body.strip_prefix('@') else { continue };
        let (key, value) = match body.find(char::is_whitespace) {
            Some(i) => (&body[..i], body[i..].trim()),
            None => (body, ""),
        };
        match key {
            "name" if !has_name && !value.is_empty() => {
                meta.name = value.to_string();
                has_name = true;
            }
            "namespace" if meta.namespace.is_empty() => meta.namespace = value.to_string(),
            "version" if meta.version.is_empty() => meta.version = value.to_string(),
            "description" if meta.description.is_empty() => meta.description = value.to_string(),
            "match" if !value.is_empty() => {
                if MatchPattern::parse(value).is_none() {
                    warn(format!("Invalid @match pattern \"{value}\": it is ignored."));
                }
                meta.matches.push(value.to_string());
            }
            "include" | "exclude" if !value.is_empty() => {
                if is_regex(value) {
                    warn(
                        "Regular-expression @include and @exclude (\"/…/\") are not supported: they are ignored."
                            .into(),
                    );
                }
                if key == "include" {
                    meta.includes.push(value.to_string());
                } else {
                    meta.excludes.push(value.to_string());
                }
            }
            "run-at" => {
                meta.run_at = match value {
                    "document-start" => RunAt::DocumentStart,
                    "document-end" | "document-body" => RunAt::DocumentEnd,
                    "document-idle" => RunAt::DocumentIdle,
                    other => {
                        warn(format!("Unknown @run-at \"{other}\": the script runs at document-idle."));
                        RunAt::DocumentIdle
                    }
                }
            }
            "grant" if !value.is_empty() => {
                let gm = value.strip_prefix("GM_").or_else(|| value.strip_prefix("GM."));
                match gm {
                    Some(f) if f.eq_ignore_ascii_case("xmlhttpRequest") => xhr = true,
                    Some(f) if SUPPORTED_GM.contains(&f) => {}
                    _ if HARMLESS_GRANTS.contains(&value) => {}
                    _ => warn(format!("@grant {value} is not supported.")),
                }
                if !meta.grants.iter().any(|g| g == value) {
                    meta.grants.push(value.to_string());
                }
            }
            "noframes" => meta.noframes = true,
            "require" => warn("@require is not supported: the file is not loaded.".into()),
            "resource" => warn("@resource is not supported.".into()),
            "connect" => warn("@connect is ignored (GM_xmlhttpRequest is not supported).".into()),
            _ => {}
        }
    }
    if !closed {
        return Err("The ==UserScript== block has no ==/UserScript== line.".into());
    }
    let lower = source.to_ascii_lowercase();
    if xhr || lower.contains("gm_xmlhttprequest") || lower.contains("gm.xmlhttprequest") {
        warn("GM_xmlhttpRequest is not supported: the script can't make cross-site requests.".into());
    }
    if !has_name {
        return Err("Add @name to the ==UserScript== block.".into());
    }
    if meta.matches.is_empty() && meta.includes.is_empty() {
        return Err("Add @match or @include".into());
    }
    meta.warnings = warnings;
    Ok(meta)
}

/// `/…/flags`: a regular-expression `@include` (not supported; it never matches).
fn is_regex(p: &str) -> bool {
    let Some(body) = p.strip_prefix('/') else { return false };
    match body.rfind('/') {
        Some(i) => body[i + 1..].chars().all(|c| "gimsuy".contains(c)),
        None => false,
    }
}

/// Krunk's own origins (pages, the shell, extension pages), with or without trailing dots.
fn own_host(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    let h = h.trim_end_matches('.');
    h == "krunk.invalid" || h.ends_with(".krunk.invalid")
}

/// May userscripts run on `url` at all? http, https and file, never Krunk's own origins.
pub fn injectable_url(url: &str) -> bool {
    let Ok(u) = url::Url::parse(url) else { return false };
    matches!(u.scheme(), "http" | "https" | "file") && !own_host(u.host_str().unwrap_or(""))
}

/// Does a script with this metadata run on `url`? (`@exclude` wins; `@match` uses Chrome match
/// patterns, `@include`/`@exclude` are whole-URL `*` globs; never on Krunk's own origins.)
/// The globs see the normalised address without its `#fragment`.
pub fn matches_url(meta: &Meta, url: &str) -> bool {
    let Ok(mut u) = url::Url::parse(url) else { return false };
    if !matches!(u.scheme(), "http" | "https" | "file") || own_host(u.host_str().unwrap_or("")) {
        return false;
    }
    u.set_fragment(None);
    let whole = u.as_str();
    let glob = |p: &String| !is_regex(p) && glob_match(p, whole);
    if meta.excludes.iter().any(glob) {
        return false;
    }
    meta.matches.iter().any(|p| MatchPattern::parse(p).is_some_and(|m| m.matches(whole)))
        || meta.includes.iter().any(glob)
}

/// A script as the loader gets it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoaderScript {
    pub id: String,
    pub meta: Meta,
    pub source: String,
    /// The script's GM values (key → JSON text), a snapshot.
    pub values: Vec<(String, String)>,
}

/// `GM_info.script`.
#[derive(Serialize)]
struct Info<'a> {
    name: &'a str,
    namespace: &'a str,
    version: &'a str,
    description: &'a str,
    matches: &'a [String],
    includes: &'a [String],
    excludes: &'a [String],
    grant: &'a [String],
    #[serde(rename = "run-at")]
    run_at: &'static str,
    noframes: bool,
}

/// What the loader's `add` gets about a script.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Entry<'a> {
    id: &'a str,
    name: &'a str,
    matches: &'a [String],
    includes: &'a [String],
    excludes: &'a [String],
    run_at: u8,
    noframes: bool,
    /// Run without the match test (run now, forced).
    force: bool,
    grants: &'a [String],
    info: Info<'a>,
    values: &'a [(String, String)],
}

/// JSON as a JavaScript literal: U+2028/2029 escaped too.
fn js_json<T: Serialize + ?Sized>(v: &T) -> String {
    serde_json::to_string(v)
        .unwrap_or_else(|_| "null".into())
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// The loader called with its arguments: an expression whose value is `{add, go}`.
fn loader_call(ids: &[&str], binding: &str, nonce: &str) -> String {
    format!(
        "({})({}, {}, {}, {})",
        crate::assets::inject_script("userscript-loader.js").trim().trim_end_matches(';'),
        js_json(binding),
        js_json(nonce),
        js_json(env!("CARGO_PKG_VERSION")),
        js_json(ids)
    )
}

/// `<reg>.add({…}, function (GM_info, …, unsafeWindow) { <source> });`
fn add_call(reg: &str, s: &LoaderScript, force: bool) -> String {
    let m = &s.meta;
    let entry = Entry {
        id: &s.id,
        name: &m.name,
        matches: &m.matches,
        includes: &m.includes,
        excludes: &m.excludes,
        run_at: m.run_at.stage(),
        noframes: m.noframes,
        force,
        grants: &m.grants,
        info: Info {
            name: &m.name,
            namespace: &m.namespace,
            version: &m.version,
            description: &m.description,
            matches: &m.matches,
            includes: &m.includes,
            excludes: &m.excludes,
            grant: &m.grants,
            run_at: m.run_at.as_str(),
            noframes: m.noframes,
        },
        values: &s.values,
    };
    format!(
        "{reg}.add({}, function (GM_info, GM_addStyle, GM_getValue, GM_setValue, GM_deleteValue, GM_listValues, \
         GM_setClipboard, GM_notification, GM_openInTab, GM, unsafeWindow, GM_wasm) {{\n{}\n}});\n",
        js_json(&entry),
        s.source
    )
}

fn ids_of(scripts: &[LoaderScript]) -> Vec<&str> {
    scripts.iter().map(|s| s.id.as_str()).collect()
}

/// The loader IIFE for one tab: deletes the binding `binding` from `window` first thing, then
/// runs each matching script at its run-at time with its granted GM functions, reporting GM
/// calls as JSON `{nonce, script, op, …}` through the binding. One script with a syntax error
/// stops the whole bundle: the host registers [`bundle_parts`] instead and uses this for
/// "run now".
pub fn bundle(scripts: &[LoaderScript], binding: &str, nonce: &str) -> String {
    bundle_with(scripts, binding, nonce, false)
}

/// [`bundle`] for one script run on demand; `force` skips the address test.
pub fn bundle_one(script: &LoaderScript, binding: &str, nonce: &str, force: bool) -> String {
    bundle_with(std::slice::from_ref(script), binding, nonce, force)
}

fn bundle_with(scripts: &[LoaderScript], binding: &str, nonce: &str, force: bool) -> String {
    let mut out = String::with_capacity(8192 + scripts.iter().map(|s| s.source.len() + 512).sum::<usize>());
    out.push_str("(function () {\nconst __krunkUS = ");
    out.push_str(&loader_call(&ids_of(scripts), binding, nonce));
    out.push_str(";\n");
    for s in scripts {
        out.push_str(&add_call("__krunkUS", s, force));
    }
    out.push_str("__krunkUS.go();\n})();\n");
    out
}

/// The same as [`bundle`], as separate new-document scripts (they run in order, before any
/// page script): the loader, which also parks `{add, go}` on `window[binding]` for the next
/// parts; one part per script, so a syntax error costs only that script; and the start, which
/// removes `window[binding]` again and runs the scripts.
pub fn bundle_parts(scripts: &[LoaderScript], binding: &str, nonce: &str) -> Vec<String> {
    let key = js_json(binding);
    let mut parts = Vec::with_capacity(scripts.len() + 2);
    parts.push(format!(
        "(function () {{\nconst r = {};\ntry {{ Object.defineProperty(window, {key}, {{ value: r, configurable: true }}); }} catch (_) {{}}\n}})();\n",
        loader_call(&ids_of(scripts), binding, nonce)
    ));
    for s in scripts {
        parts.push(format!(
            "(function (__krunkUS) {{\nif (!__krunkUS || typeof __krunkUS.add !== 'function') return;\n{}}})(window[{key}]);\n",
            add_call("__krunkUS", s, false)
        ));
    }
    parts.push(format!(
        "(function () {{\nconst r = window[{key}];\ntry {{ delete window[{key}]; }} catch (_) {{}}\nif (r && typeof r.go === 'function') r.go();\n}})();\n"
    ));
    parts
}

/// A GM call from a page, parsed strictly (anything else is dropped).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", deny_unknown_fields)]
pub enum GmCall {
    Set {
        nonce: String,
        script: String,
        key: String,
        value: String,
    },
    Delete {
        nonce: String,
        script: String,
        key: String,
    },
    Clipboard {
        nonce: String,
        script: String,
        text: String,
    },
    Notify {
        nonce: String,
        script: String,
        text: String,
        #[serde(default)]
        title: String,
    },
    OpenTab {
        nonce: String,
        script: String,
        url: String,
        #[serde(default)]
        background: bool,
    },
    Ran {
        nonce: String,
        script: String,
    },
    /// 0.19.0, `GM.wasm`: a request to the WASM inspector of the script's own tab. `id` numbers
    /// the script's waiting promise (the host answers with [`gm_reply`]); `body` is JSON
    /// `{ctx, req}` ([`wasm_body`]).
    Wasm {
        nonce: String,
        script: String,
        id: String,
        body: String,
    },
}

/// The body of a `GM.wasm` call: the context and the checked request.
pub fn wasm_body(body: &str) -> Result<(u32, crate::wasm::WasmReq), String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Body {
        #[serde(default)]
        ctx: u32,
        req: serde_json::Value,
    }
    if body.len() > crate::protocol::MAX_REQUEST_BYTES {
        return Err("the request is too large".into());
    }
    let b: Body = serde_json::from_str(body).map_err(|_| "not a valid request".to_string())?;
    Ok((b.ctx, crate::wasm::WasmReq::parse(&b.req)?))
}

/// A script may use `GM.wasm`.
pub fn grants_wasm(meta: &Meta) -> bool {
    meta.grants.iter().any(|g| g == "GM.wasm" || g == "GM_wasm")
}

/// The name of the loader's reply function on `window` (not enumerable, not replaceable; it
/// answers only to the tab's nonce). It exists only in documents where a script with a
/// promise API (`GM.wasm`) runs.
pub fn reply_key(binding: &str) -> String {
    format!("{binding}R")
}

/// The expression that settles promise `id` of a script in the document it is evaluated in:
/// resolved with `result`, or rejected with the error text.
pub fn gm_reply(binding: &str, nonce: &str, id: u32, result: &Result<serde_json::Value, String>) -> String {
    let (ok, payload) = match result {
        Ok(v) => (true, js_json(v)),
        Err(e) => (false, js_json(e)),
    };
    let key = js_json(&reply_key(binding));
    format!(
        "(function(){{var f=window[{key}];if(typeof f==='function')f({}, {id}, {ok}, {payload});}})()",
        js_json(nonce)
    )
}

impl GmCall {
    /// Parses a binding payload; `None` for anything that isn't exactly a GM call.
    pub fn parse(payload: &str) -> Option<GmCall> {
        serde_json::from_str(payload).ok()
    }

    pub fn nonce(&self) -> &str {
        match self {
            GmCall::Set { nonce, .. }
            | GmCall::Delete { nonce, .. }
            | GmCall::Clipboard { nonce, .. }
            | GmCall::Notify { nonce, .. }
            | GmCall::OpenTab { nonce, .. }
            | GmCall::Wasm { nonce, .. }
            | GmCall::Ran { nonce, .. } => nonce,
        }
    }

    pub fn script(&self) -> &str {
        match self {
            GmCall::Set { script, .. }
            | GmCall::Delete { script, .. }
            | GmCall::Clipboard { script, .. }
            | GmCall::Notify { script, .. }
            | GmCall::OpenTab { script, .. }
            | GmCall::Wasm { script, .. }
            | GmCall::Ran { script, .. } => script,
        }
    }

    /// The operation's name (for the log).
    pub fn op(&self) -> &'static str {
        match self {
            GmCall::Set { .. } => "set",
            GmCall::Delete { .. } => "delete",
            GmCall::Clipboard { .. } => "clipboard",
            GmCall::Notify { .. } => "notify",
            GmCall::OpenTab { .. } => "openTab",
            GmCall::Ran { .. } => "ran",
            GmCall::Wasm { .. } => "wasm",
        }
    }
}

/// Size limits.
pub const MAX_SOURCE_BYTES: usize = 200_000;
pub const MAX_VALUES_BYTES: usize = 1024 * 1024;
/// The longest binding payload the host reads (a value up to the storage limit, plus room).
pub const MAX_CALL_BYTES: usize = MAX_VALUES_BYTES + 4096;
/// `GM_setClipboard` text.
pub const MAX_CLIPBOARD_BYTES: usize = 100 * 1024;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn src(meta: &str) -> String {
        format!("// ==UserScript==\n{meta}// ==/UserScript==\n\nconsole.log('hi');\n")
    }

    fn meta_of(lines: &[&str]) -> Result<Meta, String> {
        let block: String = lines.iter().map(|l| format!("// {l}\n")).collect();
        parse_meta(&src(&block))
    }

    #[test]
    fn parses_all_keys() {
        let m = meta_of(&[
            "@name        Banner remover",
            "@namespace   https://example.com/ns",
            "@version     1.2.3",
            "@description Removes the banner",
            "@match       https://example.com/*",
            "@match       *://*.example.org/a/*",
            "@include     http*://news.example.net/*",
            "@exclude     *://example.com/admin*",
            "@exclude     *logout*",
            "@run-at      document-start",
            "@grant       GM_addStyle",
            "@grant       GM_getValue",
            "@grant       GM.setValue",
            "@grant       GM_addStyle",
            "@noframes",
            "@author      Someone",
            "@icon        https://example.com/i.png",
            "@name:de     Bannerentferner",
        ])
        .unwrap();
        assert_eq!(m.name, "Banner remover");
        assert_eq!(m.namespace, "https://example.com/ns");
        assert_eq!(m.version, "1.2.3");
        assert_eq!(m.description, "Removes the banner");
        assert_eq!(m.matches, ["https://example.com/*", "*://*.example.org/a/*"]);
        assert_eq!(m.includes, ["http*://news.example.net/*"]);
        assert_eq!(m.excludes, ["*://example.com/admin*", "*logout*"]);
        assert_eq!(m.run_at, RunAt::DocumentStart);
        assert_eq!(m.grants, ["GM_addStyle", "GM_getValue", "GM.setValue"], "deduplicated, in order");
        assert!(m.noframes);
        assert!(m.warnings.is_empty(), "{:?}", m.warnings);
    }

    #[test]
    fn defaults_and_first_value_wins() {
        let m = meta_of(&["@name A", "@name B", "@version 1", "@version 2", "@include *"]).unwrap();
        assert_eq!((m.name.as_str(), m.version.as_str()), ("A", "1"));
        assert_eq!(m.run_at, RunAt::DocumentIdle);
        assert!(!m.noframes && m.grants.is_empty() && m.matches.is_empty());
        assert_eq!(meta_of(&["@name x", "@match *://*/*", "@run-at document-end"]).unwrap().run_at, RunAt::DocumentEnd);
        assert_eq!(
            meta_of(&["@name x", "@match *://*/*", "@run-at document-idle"]).unwrap().run_at,
            RunAt::DocumentIdle
        );
        assert_eq!(
            meta_of(&["@name x", "@match *://*/*", "@run-at document-body"]).unwrap().run_at,
            RunAt::DocumentEnd
        );
        let m = meta_of(&["@name x", "@match *://*/*", "@run-at context-menu"]).unwrap();
        assert_eq!(m.run_at, RunAt::DocumentIdle);
        assert!(m.warnings[0].contains("context-menu"));
    }

    #[test]
    fn errors() {
        assert!(parse_meta("console.log(1)").unwrap_err().contains("No ==UserScript== block"));
        assert!(parse_meta("").is_err());
        assert!(parse_meta("// ==UserScript==\n// @name x\n// @match *://*/*\n")
            .unwrap_err()
            .contains("no ==/UserScript=="));
        assert_eq!(meta_of(&["@match *://*/*"]).unwrap_err(), "Add @name to the ==UserScript== block.");
        assert_eq!(meta_of(&["@name", "@match *://*/*"]).unwrap_err(), "Add @name to the ==UserScript== block.");
        assert_eq!(meta_of(&["@name x"]).unwrap_err(), "Add @match or @include");
        assert_eq!(meta_of(&["@name x", "@exclude *"]).unwrap_err(), "Add @match or @include");
        assert_eq!(meta_of(&["@name x", "@match"]).unwrap_err(), "Add @match or @include", "an empty value");
        // A block in a /* */ comment is not a metadata block.
        assert!(parse_meta("/* ==UserScript==\n@name x\n==/UserScript== */").is_err());
    }

    #[test]
    fn warnings() {
        let m = meta_of(&[
            "@name x",
            "@match *://*/*",
            "@grant GM_xmlhttpRequest",
            "@grant GM.xmlHttpRequest",
            "@grant GM_getResourceText",
            "@grant unsafeWindow",
            "@grant none",
            "@grant window.close",
            "@require https://code.jquery.com/jquery.js",
            "@require https://example.com/b.js",
            "@resource css https://example.com/a.css",
            "@include /^https://example\\.com/.*$/",
            "@exclude /x/i",
            "@connect example.com",
            "@match not a pattern",
        ])
        .unwrap();
        let w = m.warnings.join("\n");
        assert!(w.contains("GM_xmlhttpRequest is not supported"), "{w}");
        assert!(w.contains("@grant GM_getResourceText is not supported"), "{w}");
        assert!(w.contains("@require is not supported"), "{w}");
        assert!(w.contains("@resource is not supported"), "{w}");
        assert!(w.contains("Regular-expression @include"), "{w}");
        assert!(w.contains("@connect is ignored"), "{w}");
        assert!(w.contains("Invalid @match pattern \"not a pattern\""), "{w}");
        assert!(!w.contains("unsafeWindow") && !w.contains("none") && !w.contains("window.close"), "{w}");
        assert_eq!(m.warnings.len(), 7, "each warning once: {w}");
        // In the source only.
        let s = "// ==UserScript==\n// @name x\n// @match *://*/*\n// ==/UserScript==\nGM.xmlHttpRequest({url: 'x'});";
        assert!(parse_meta(s).unwrap().warnings[0].contains("GM_xmlhttpRequest"));
        let s = "// ==UserScript==\n// @name x\n// @match *://*/*\n// ==/UserScript==\nGM_xmlhttpRequest({});";
        assert_eq!(parse_meta(s).unwrap().warnings.len(), 1);
    }

    #[test]
    fn whitespace_crlf_and_bom() {
        let s = "\u{feff}  //   ==UserScript==  \r\n//@name\tTabbed  name \r\n\t//  @match\t\thttps://a.example/*\r\n// some text\r\n\r\n//\t@noframes\r\n//==/UserScript==\r\nlet a = 1;\r\n";
        let m = parse_meta(s).unwrap();
        assert_eq!(m.name, "Tabbed  name");
        assert_eq!(m.matches, ["https://a.example/*"]);
        assert!(m.noframes);
        // Code before the block is fine; lines after the end are not metadata.
        let s = "'use strict';\n// ==UserScript==\n// @name x\n// @include *\n// ==/UserScript==\n// @match https://late.example/*\n";
        let m = parse_meta(s).unwrap();
        assert!(m.matches.is_empty() && m.includes == ["*"]);
        // Non-comment lines inside the block are skipped.
        let s = "// ==UserScript==\n// @name x\nvar x;\n// @match https://b.example/*\n// ==/UserScript==\n";
        assert_eq!(parse_meta(s).unwrap().matches, ["https://b.example/*"]);
    }

    // ------------------------------------------------------------ matching

    fn vectors_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/userscripts/match-vectors.json")
    }

    #[derive(Deserialize)]
    struct Case {
        url: String,
        #[serde(default)]
        matches: Vec<String>,
        #[serde(default)]
        includes: Vec<String>,
        #[serde(default)]
        excludes: Vec<String>,
        expected: bool,
        #[serde(default)]
        note: String,
    }

    fn cases() -> Vec<Case> {
        let text = std::fs::read_to_string(vectors_path()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        serde_json::from_value(v["cases"].clone()).unwrap()
    }

    #[test]
    fn match_vectors() {
        let cases = cases();
        assert!(cases.len() >= 40, "{} cases", cases.len());
        let mut failed = Vec::new();
        for c in &cases {
            let meta = Meta {
                matches: c.matches.clone(),
                includes: c.includes.clone(),
                excludes: c.excludes.clone(),
                ..Meta::default()
            };
            if matches_url(&meta, &c.url) != c.expected {
                failed.push(format!("{} (expected {}): {}", c.url, c.expected, c.note));
            }
        }
        assert!(failed.is_empty(), "failed:\n{}", failed.join("\n"));
        assert!(cases.iter().any(|c| c.expected) && cases.iter().any(|c| !c.expected));
    }

    fn node() -> Option<&'static str> {
        std::process::Command::new("node").arg("--version").output().ok().filter(|o| o.status.success()).map(|_| "node")
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lb-us-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The loader's JS matcher (between the `match:begin`/`match:end` markers) agrees with the
    /// vector file (skipped without Node).
    #[test]
    fn js_matcher_agrees_with_the_vectors() {
        let Some(node) = node() else {
            eprintln!("node not found: JS matcher test skipped");
            return;
        };
        let loader = crate::assets::inject_script("userscript-loader.js");
        let start = loader.find("// match:begin").expect("match:begin marker");
        let end = loader.find("// match:end").expect("match:end marker");
        let dir = temp_dir("match");
        let script = dir.join("match-test.js");
        let test = format!(
            "const URLCtor = URL;\n{}\nconst v = JSON.parse(require('fs').readFileSync({}, 'utf8'));\nlet bad = 0;\n\
             for (const c of v.cases) {{ const m = {{ matches: c.matches || [], includes: c.includes || [], excludes: c.excludes || [] }};\n\
             const got = usMatches(m, c.url); if (got !== c.expected) {{ bad++; console.log('FAIL ' + c.url + ' expected ' + c.expected + ' ' + (c.note || '')); }} }}\n\
             console.log('cases ' + v.cases.length + ' failed ' + bad); process.exit(bad ? 1 : 0);\n",
            &loader[start..end],
            serde_json::to_string(&vectors_path()).unwrap()
        );
        std::fs::write(&script, test).unwrap();
        let out = std::process::Command::new(node).arg(&script).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(out.status.success(), "{text}");
        assert!(text.contains("failed 0"), "{text}");
    }

    #[test]
    fn never_on_own_origins_or_other_schemes() {
        let all = Meta { includes: vec!["*".into()], matches: vec!["<all_urls>".into()], ..Meta::default() };
        for url in [
            "https://krunk.invalid/newtab.html",
            "https://shell.krunk.invalid/agent.html",
            "https://abcdefghijklmnopabcdefghijklmnop.ext.krunk.invalid/popup.html",
            "https://KRUNK.invalid./x",
            "https://pages.krunk.invalid../x",
            "about:blank",
            "data:text/html,hi",
            "javascript:alert(1)",
            "chrome://settings",
            "devtools://devtools/bundled/inspector.html",
            "blob:https://example.com/1234",
            "ftp://example.com/",
            "not a url",
        ] {
            assert!(!matches_url(&all, url), "{url}");
            assert!(!injectable_url(url), "{url}");
        }
        assert!(matches_url(&all, "https://krunk.invalid.example.com/"));
        assert!(injectable_url("file:///C:/page.html") && injectable_url("http://localhost:8080/"));
    }

    // ------------------------------------------------------------ the bundle

    fn script(id: &str, meta_lines: &[&str], body: &str) -> LoaderScript {
        let block: String = meta_lines.iter().map(|l| format!("// {l}\n")).collect();
        let source = format!("// ==UserScript==\n{block}// ==/UserScript==\n{body}\n");
        LoaderScript { id: id.into(), meta: parse_meta(&source).unwrap(), source, values: Vec::new() }
    }

    /// 0.19.0: `GM.wasm` is a grant like the others; its calls carry a promise id and a
    /// checked request, and the host's answer names the tab's nonce.
    #[test]
    fn gm_wasm_grant_call_and_reply() {
        let m = parse_meta(&src("// @name W\n// @match *://*/*\n// @grant GM.wasm\n")).unwrap();
        assert!(m.warnings.is_empty(), "{:?}", m.warnings);
        assert!(grants_wasm(&m));
        let m2 = parse_meta(&src("// @name W\n// @match *://*/*\n// @grant GM_wasm\n")).unwrap();
        assert!(m2.warnings.is_empty() && grants_wasm(&m2));
        assert!(!grants_wasm(&parse_meta(&src("// @name W\n// @match *://*/*\n// @grant GM_addStyle\n")).unwrap()));
        let call = GmCall::parse(
            r#"{"nonce":"n","script":"s1","op":"wasm","id":"7","body":"{\"ctx\":0,\"req\":{\"op\":\"read\",\"mem\":0,\"where\":1024,\"type\":\"i32\"}}"}"#,
        )
        .unwrap();
        let GmCall::Wasm { id, body, .. } = &call else { panic!("{call:?}") };
        assert_eq!((id.as_str(), call.op(), call.nonce(), call.script()), ("7", "wasm", "n", "s1"));
        let (ctx, req) = wasm_body(body).unwrap();
        assert_eq!(ctx, 0);
        assert_eq!(req.to_json()["where"], 1024);
        assert!(wasm_body(r#"{"req":{"op":"eval"}}"#).is_err());
        assert!(wasm_body(r#"{"ctx":0,"req":{"op":"list"},"tab":3}"#).is_err(), "a script names no tab");
        let ok = gm_reply("__krunkUS_ab", "nonce1", 7, &Ok(serde_json::json!({"value": "a\u{2028}b"})));
        assert!(
            ok.contains(r#"window["__krunkUS_abR"]"#) && ok.contains(r#"f("nonce1", 7, true, {"value":"a\u2028b"})"#),
            "{ok}"
        );
        let err = gm_reply("__krunkUS_ab", "nonce1", 8, &Err("no memory 3".into()));
        assert!(err.contains(r#"f("nonce1", 8, false, "no memory 3")"#), "{err}");
    }

    /// Every `GM.wasm` function's body, as the loader really sends it (the Node test of the
    /// loader compares its bodies with this file), is a request core accepts.
    #[test]
    fn gm_wasm_bodies_from_the_shared_file() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/dev/wasm-loader-cases.json");
        let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let bodies = doc["bodies"].as_array().unwrap();
        let mut ops = std::collections::BTreeSet::new();
        for b in bodies {
            // (One body of the loader's test names a memory that is not there: still well-formed.)
            let (ctx, req) = wasm_body(&b.to_string()).unwrap_or_else(|e| panic!("{b}: {e}"));
            assert_eq!(u64::from(ctx), b["ctx"].as_u64().unwrap());
            ops.insert(req.to_json()["op"].as_str().unwrap().to_string());
        }
        let all = ["bytes", "discard", "freeze", "frozen", "globals", "list", "next", "read", "results", "scan"];
        for op in all.iter().chain(&["setGlobal", "unfreeze", "watch", "write"]) {
            assert!(ops.contains(*op), "no GM.wasm body for {op}");
        }
    }

    const BINDING: &str = "__krunkUS_0123456789abcdef";
    const NONCE: &str = "00112233445566778899aabbccddeeff";

    #[test]
    fn bundle_deletes_the_binding_first() {
        let a = script("a1", &["@name A", "@match *://*/*"], "document.title = 'A';");
        let b = bundle(&[a], BINDING, NONCE);
        let send = b.find("const send = window[binding]; try { delete window[binding]; }").expect("binding taken");
        assert!(send < b.find("document.title").unwrap(), "before any script");
        // Nothing but the loader's opening and its comment come first: no statement before it.
        let body_start = b.find("(function (binding, nonce, version, ids) {").unwrap();
        let first = b[body_start..].lines().nth(1).unwrap().trim();
        assert!(first.starts_with("const send = window[binding]"), "{first}");
        assert!(b.contains(&format!("({BINDING:?}, {NONCE:?}, ")), "called with the binding and nonce");
        // The parts: the first one is the loader.
        let parts = bundle_parts(&[script("a1", &["@name A", "@match *://*/*"], "x();")], BINDING, NONCE);
        assert_eq!(parts.len(), 3);
        assert!(parts[0].contains("delete window[binding]") && !parts[0].contains("x();"));
        assert!(parts[1].contains("x();") && parts[2].contains(".go()"));
    }

    #[test]
    fn bundle_embeds_grants_values_and_escapes() {
        let mut s = script(
            "s1",
            &[
                "@name Quote \" and </script> \u{2028} end",
                "@match https://example.com/*",
                "@grant GM_getValue",
                "@grant GM.setValue",
            ],
            "const sep = '\u{2028}'; GM_getValue('k');",
        );
        s.values = vec![("k".into(), "\"v\u{2029}\"".into()), ("__proto__".into(), "1".into())];
        let b = bundle(&[s], BINDING, NONCE);
        assert!(b.contains(r#""grants":["GM_getValue","GM.setValue"]"#), "only the granted names are passed");
        assert!(b.contains(r#""values":[["k","\"v\u2029\""],["__proto__","1"]]"#), "{b}");
        assert!(b.contains(r#""name":"Quote \" and </script> \u2028 end""#));
        assert!(!b.contains("\u{2029}"), "escaped in JSON");
        assert!(b.contains("const sep = '\u{2028}';"), "the source itself is unchanged");
    }

    /// The bundle and every part are valid JavaScript (`node --check`; skipped without Node).
    #[test]
    fn bundle_is_valid_javascript() {
        let Some(node) = node() else {
            eprintln!("node not found: syntax check skipped");
            return;
        };
        let scripts = [
            script("a1", &["@name A", "@match *://*/*", "@grant none"], "// ends in a line comment"),
            script(
                "b2",
                &["@name B", "@include *", "@grant GM_addStyle", "@run-at document-start"],
                "GM_addStyle('a{}')",
            ),
            script("c3", &["@name C", "@include *", "@noframes"], "'use strict'; return;"),
        ];
        let dir = temp_dir("check");
        let mut files = vec![("bundle.js".to_string(), bundle(&scripts, BINDING, NONCE))];
        files.push(("one.js".into(), bundle_one(&scripts[1], BINDING, NONCE, true)));
        for (i, p) in bundle_parts(&scripts, BINDING, NONCE).into_iter().enumerate() {
            files.push((format!("part{i}.js"), p));
        }
        for (name, text) in files {
            let path = dir.join(&name);
            std::fs::write(&path, text).unwrap();
            let out = std::process::Command::new(node).arg("--check").arg(&path).output().unwrap();
            assert!(out.status.success(), "{name}: {}", String::from_utf8_lossy(&out.stderr));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The bundle runs in a fake page (Node): the binding is gone from `window` at once, only
    /// granted functions exist, values round-trip, GM calls carry the nonce, `ran` is sent,
    /// non-matching and excluded scripts don't run, a script can't see the loader's closure.
    #[test]
    fn bundle_runs_in_a_fake_page() {
        let Some(node) = node() else {
            eprintln!("node not found: run test skipped");
            return;
        };
        let mut a = script(
            "a1",
            &["@name A", "@match https://example.com/*", "@grant GM_getValue", "@grant GM_setValue", "@grant GM.listValues", "@run-at document-start"],
            "out.a = { get: GM_getValue('k', 'def'), miss: GM_getValue('zz', 7), add: typeof GM_addStyle, clip: typeof GM_setClipboard, \
             gm: typeof GM.listValues, info: GM_info.script.name, uw: unsafeWindow === window, nonce: typeof nonce, send: typeof send, \
             binding: typeof window.__krunkUS_0123456789abcdef };\nGM_setValue('n', {x: 1}); out.a.after = GM_getValue('n').x;",
        );
        a.values = vec![("k".into(), "[1,2]".into())];
        let none =
            script("b2", &["@name B", "@match https://example.com/*", "@grant none"], "out.b = typeof GM_getValue;");
        let other = script("c3", &["@name C", "@match https://other.example/*"], "out.c = true;");
        let excl = script("d4", &["@name D", "@include *", "@exclude *example.com/page*"], "out.d = true;");
        let bad = script("e5", &["@name E", "@include *", "@run-at document-end"], "throw new Error('boom');");
        let b = bundle(&[a, none, other, excl, bad], BINDING, NONCE);
        let dir = temp_dir("run");
        let path = dir.join("run.js");
        let harness = format!(
            "const calls = []; const out = {{}}; const errors = [];\n\
             globalThis.window = globalThis; window.top = window;\n\
             globalThis.location = {{ href: 'https://example.com/page?q=1#h', protocol: 'https:', hostname: 'example.com' }};\n\
             globalThis.document = {{ readyState: 'loading', addEventListener: (e, f) => {{ document.l = f; }} }};\n\
             const loads = []; window.addEventListener = (e, f) => loads.push(f);\n\
             console.error = (...a) => errors.push(String(a[0]));\n\
             window.{BINDING} = (s) => calls.push(JSON.parse(s));\n\
             {b}\n\
             const gone = !('{BINDING}' in window);\n\
             document.readyState = 'interactive'; document.l();\n\
             document.readyState = 'complete'; loads.forEach((f) => f());\n\
             setTimeout(() => console.log(JSON.stringify({{ gone, out, calls, errors }})), 20);\n"
        );
        std::fs::write(&path, harness).unwrap();
        let o = std::process::Command::new(node).arg(&path).output().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
        assert_eq!(v["gone"], true, "{v}");
        let a = &v["out"]["a"];
        assert_eq!(a["get"], serde_json::json!([1, 2]));
        assert_eq!(a["miss"], 7);
        assert_eq!(
            (a["add"].as_str(), a["clip"].as_str(), a["gm"].as_str()),
            (Some("undefined"), Some("undefined"), Some("function"))
        );
        assert_eq!(a["info"], "A");
        assert_eq!(a["uw"], true);
        assert_eq!((a["nonce"].as_str(), a["send"].as_str()), (Some("undefined"), Some("undefined")), "closure hidden");
        assert_eq!(a["binding"], "undefined");
        assert_eq!(a["after"], 1);
        assert_eq!(v["out"]["b"], "undefined", "@grant none");
        assert!(v["out"].get("c").is_none() && v["out"].get("d").is_none(), "{v}");
        assert_eq!(v["errors"][0], "Userscript \"E\" failed:");
        let calls = v["calls"].as_array().unwrap();
        assert_eq!(
            calls[0],
            serde_json::json!({"nonce": NONCE, "script": "a1", "op": "set", "key": "n", "value": "{\"x\":1}"})
        );
        for c in calls {
            let parsed = GmCall::parse(&c.to_string()).expect("the host's parser takes it");
            assert_eq!(parsed.nonce(), NONCE);
        }
        let ran: Vec<&str> = calls.iter().filter(|c| c["op"] == "ran").map(|c| c["script"].as_str().unwrap()).collect();
        assert_eq!(ran, ["a1", "e5", "b2"], "start, end, idle");
    }

    #[test]
    fn gm_calls_are_parsed_strictly() {
        let ok = |s: &str| GmCall::parse(s).unwrap();
        assert_eq!(
            ok(r#"{"nonce":"n","script":"s","op":"set","key":"k","value":"1"}"#),
            GmCall::Set { nonce: "n".into(), script: "s".into(), key: "k".into(), value: "1".into() }
        );
        assert_eq!(ok(r#"{"nonce":"n","script":"s","op":"delete","key":"k"}"#).op(), "delete");
        assert_eq!(ok(r#"{"nonce":"n","script":"s","op":"clipboard","text":"t"}"#).script(), "s");
        assert_eq!(
            ok(r#"{"nonce":"n","script":"s","op":"notify","text":"t"}"#),
            GmCall::Notify { nonce: "n".into(), script: "s".into(), text: "t".into(), title: String::new() }
        );
        assert_eq!(
            ok(r#"{"nonce":"n","script":"s","op":"openTab","url":"https://a/","background":true}"#),
            GmCall::OpenTab { nonce: "n".into(), script: "s".into(), url: "https://a/".into(), background: true }
        );
        assert_eq!(ok(r#"{"op":"ran","nonce":"n","script":"s"}"#).op(), "ran");
        for bad in [
            r#"{"nonce":"n","script":"s","op":"ran","extra":1}"#,
            r#"{"nonce":"n","script":"s","op":"set","key":"k"}"#,
            r#"{"nonce":"n","script":"s","op":"set","key":"k","value":1}"#,
            r#"{"nonce":"n","script":"s","op":"eval","code":"x"}"#,
            r#"{"nonce":"n","script":"s"}"#,
            r#"{"script":"s","op":"ran"}"#,
            r#"{"nonce":"n","script":"s","op":"openTab","url":"u","background":"yes"}"#,
            r#"{"nonce":"n","script":"s","op":"clipboard","text":"t","title":"x"}"#,
            r#"["ran"]"#,
            "not json",
            "",
        ] {
            assert!(GmCall::parse(bad).is_none(), "{bad}");
        }
    }
}
