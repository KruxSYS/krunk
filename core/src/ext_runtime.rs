//! The parts of Krunk's extension runtime that need no browser (0.17.0): the background
//! worker's bootstrap, Chrome-style sender addresses, sandboxed pages, the MAIN-world bundle,
//! long-lived ports between an extension's contexts, and the bookkeeping of a message sent to
//! several receivers.
//!
//! Why these exist (MetaMask "installs but does not load", see docs/EXTENSIONS.md): a
//! Manifest V3 background is a service worker. Krunk used to run it as a classic script in a
//! hidden page, where `importScripts` was an `eval` (blocked by the extensions' CSP), there was
//! no `self.serviceWorker` and no `install` event, and there were no ports at all.

use crate::ext::{glob_match, origin, ContentScript, Manifest, HOST_SUFFIX};
use std::collections::HashMap;

/// The file name of the worker bootstrap Krunk serves next to the service worker script.
/// Next to it, so that relative `importScripts` and webpack's automatic public path resolve
/// exactly as they do for the real script.
pub const WORKER_BOOT_NAME: &str = "__krunk_sw__.js";

/// Where the bootstrap of service worker `sw` (a path in the package) is served: the same
/// folder, `WORKER_BOOT_NAME`.
pub fn worker_boot_path(sw: &str) -> String {
    let sw = sw.trim_start_matches('/');
    match sw.rfind('/') {
        Some(i) => format!("/{}/{WORKER_BOOT_NAME}", &sw[..i]),
        None => format!("/{WORKER_BOOT_NAME}"),
    }
}

/// The service worker's own file name, relative to the bootstrap.
fn worker_script_name(sw: &str) -> &str {
    let sw = sw.trim_start_matches('/');
    sw.rsplit('/').next().unwrap_or(sw)
}

/// The script a dedicated `Worker` runs as the extension's background: `template`
/// (`ui/inject/ext-worker.js`) with the `chrome.*` shim (`ext-page.js`, given a bridge to the
/// hidden background page, which relays to the host) and the extension's service worker
/// filled in. The template takes `postMessage` and `addEventListener` before any extension
/// code runs: hardened code (SES / LavaMoat) may remove globals afterwards.
pub fn worker_bootstrap(template: &str, shim: &str, sw: &str, module: bool) -> String {
    let config = serde_json::json!({ "script": format!("./{}", worker_script_name(sw)), "module": module });
    // The shim last: its text is not searched for the other marker.
    template.replacen("/*KRUNK_SW*/null", &config.to_string(), 1).replacen("/*KRUNK_SHIM*/", shim.trim(), 1)
}

/// An address on an extension's origin as Chrome writes it (`chrome-extension://<id>/path`),
/// for `sender.url` / `sender.origin` and the extension's own address checks. Krunk serves
/// extension pages from `https://<id>.ext.krunk.invalid`; other addresses are unchanged.
pub fn chrome_url(url: &str) -> String {
    let Some(rest) = url.strip_prefix("https://") else { return url.to_string() };
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    match host.strip_suffix(HOST_SUFFIX) {
        Some(id) if crate::ext::valid_id(id) => format!("chrome-extension://{id}{path}"),
        _ => url.to_string(),
    }
}

/// The reverse of [`chrome_url`]: an extension's `chrome-extension://` address to the one
/// Krunk can load (an extension that builds its addresses itself).
pub fn krunk_url(url: &str) -> String {
    let Some(rest) = url.strip_prefix("chrome-extension://") else { return url.to_string() };
    let (id, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if crate::ext::valid_id(id) {
        format!("{}{path}", origin(id))
    } else {
        url.to_string()
    }
}

/// Chrome's CSP for an extension's sandboxed pages when the manifest gives none.
pub const DEFAULT_SANDBOX_CSP: &str =
    "sandbox allow-scripts allow-forms allow-popups allow-modals; script-src 'self' 'unsafe-inline' 'unsafe-eval'; child-src 'self';";

/// Is `path` (a request's path on the extension's origin) one of the manifest's sandboxed pages?
/// Sandboxed pages run in a unique origin, without extension APIs, under their own CSP.
/// The path is compared as the file is found: percent-decoded and without regard to case
/// (`/s%61ndbox.html` and `/Sandbox.html` are `sandbox.html` on disk).
pub fn is_sandbox_page(m: &Manifest, path: &str) -> bool {
    let path = crate::ext_guard::package_path(path);
    m.sandbox.as_ref().is_some_and(|s| s.pages.iter().any(|p| glob_match(&crate::ext_guard::manifest_path(p), &path)))
}

/// The CSP of the manifest's sandboxed pages (`content_security_policy.sandbox`). It always
/// starts with `sandbox`, so the page gets a unique origin whatever the manifest says.
pub fn sandbox_csp(m: &Manifest) -> String {
    let given = m
        .content_security_policy
        .as_ref()
        .and_then(|c| c.get("sandbox"))
        .and_then(|s| s.as_str())
        .map(|s| s.replace(['\r', '\n'], " "))
        .filter(|s| !s.trim().is_empty());
    match given {
        Some(s) if s.trim_start().starts_with("sandbox") => s,
        Some(s) => format!("sandbox allow-scripts; {s}"),
        None => DEFAULT_SANDBOX_CSP.to_string(),
    }
}

/// Whether an extension may keep more than the default 10 MiB in `chrome.storage.local`.
pub fn unlimited_storage(m: &Manifest) -> bool {
    m.permissions.iter().any(|p| p == "unlimitedStorage")
}

/// A content script for the page's own (MAIN) world: no extension API there, as in Chrome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MainScript {
    pub matches: Vec<String>,
    pub exclude_matches: Vec<String>,
    pub all_frames: bool,
    pub run_at: String,
    pub js: Vec<String>,
}

/// The script `Page.addScriptToEvaluateOnNewDocument` installs in the main world of a tab for
/// one extension's `world: "MAIN"` content scripts. Every file runs as its own function
/// (top-level declarations of two files do not clash), not in strict mode, after the same
/// match test as the isolated-world scripts. Krunk's own pages never get it.
pub fn main_bundle(ext_id: &str, scripts: &[MainScript]) -> String {
    let mut out = String::new();
    out.push_str("(function () {\n");
    out.push_str(
        "if (!/^https?:$/.test(location.protocol) || /(^|\\.)krunk\\.invalid$/.test(location.hostname)) return;\n",
    );
    out.push_str(MAIN_MATCHER);
    for s in scripts {
        out.push_str(&format!(
            "__krunkRun({id}, {matches}, {excludes}, {all_frames}, {run_at}, [\n",
            id = serde_json::to_string(ext_id).unwrap_or_default(),
            matches = serde_json::to_string(&s.matches).unwrap_or_else(|_| "[]".into()),
            excludes = serde_json::to_string(&s.exclude_matches).unwrap_or_else(|_| "[]".into()),
            all_frames = s.all_frames,
            run_at = serde_json::to_string(&s.run_at).unwrap_or_else(|_| "\"document_idle\"".into()),
        ));
        for src in &s.js {
            out.push_str("function () {\n");
            out.push_str(src);
            out.push_str("\n},\n");
        }
        out.push_str("]);\n");
    }
    out.push_str("})();\n");
    out
}

/// The MAIN-world bundle's match test and runner (the same rules as `ext-content.js`).
const MAIN_MATCHER: &str = r#"function __krunkGlob(p, t) {
  var pi = 0, ti = 0, star = -1, mark = 0;
  while (ti < t.length) {
    if (pi < p.length && p[pi] === t[ti]) { pi++; ti++; }
    else if (pi < p.length && p[pi] === '*') { star = pi; mark = ti; pi++; }
    else if (star >= 0) { pi = star + 1; mark++; ti = mark; }
    else return false;
  }
  while (pi < p.length && p[pi] === '*') pi++;
  return pi === p.length;
}
function __krunkMatch(pattern, url) {
  if (pattern === '<all_urls>') return /^https?:$/.test(location.protocol);
  var m = /^(\*|https?|file|ftp):\/\/([^/]*)(\/.*)?$/.exec(pattern); if (!m) return false;
  var u; try { u = new URL(url); } catch (e) { return false; }
  var scheme = u.protocol.replace(':', '');
  if (m[1] === '*' ? !(scheme === 'http' || scheme === 'https') : scheme !== m[1]) return false;
  var host = u.hostname.toLowerCase(), ph = m[2].toLowerCase();
  if (ph !== '*' && m[1] !== 'file') { if (ph.indexOf('*.') === 0) { var d = ph.slice(2); if (host !== d && host.slice(-d.length - 1) !== '.' + d) return false; } else if (host !== ph) return false; }
  return __krunkGlob(m[3] || '/', u.pathname + u.search);
}
function __krunkRun(id, includes, excludes, allFrames, when, files) {
  var url = location.href;
  if (!allFrames && window.top !== window) return;
  if (!includes.some(function (p) { return __krunkMatch(p, url); })) return;
  if (excludes.some(function (p) { return __krunkMatch(p, url); })) return;
  var go = function () { files.forEach(function (f) { try { f.call(window); } catch (e) { console.error('[' + id + '] ' + (e && e.stack || e)); } }); };
  if (when === 'document_start' || document.readyState === 'complete' || (when === 'document_end' && document.readyState !== 'loading')) go();
  else if (when === 'document_end') document.addEventListener('DOMContentLoaded', go, { once: true });
  else window.addEventListener('load', go, { once: true });
}
"#;

// ------------------------------------------------------------------ scripting.registerContentScripts

/// Most content scripts an extension may register at runtime.
pub const MAX_REGISTERED_SCRIPTS: usize = 100;

/// One entry of `chrome.scripting.registerContentScripts` (Chrome's camelCase) as the content
/// script it is, with its id. The files must be safe paths inside the package.
pub fn parse_registered(v: &serde_json::Value) -> Result<(String, ContentScript), String> {
    let id = v.get("id").and_then(|i| i.as_str()).unwrap_or("").to_string();
    if id.is_empty() || id.starts_with('_') || id.len() > 200 {
        return Err("Invalid script id".into());
    }
    let list = |k: &str| -> Vec<String> {
        v.get(k)
            .and_then(|a| a.as_array())
            .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    };
    let (js, css) = (list("js"), list("css"));
    if js.is_empty() && css.is_empty() {
        return Err(format!("Script with ID '{id}' must specify at least one js or css file"));
    }
    if let Some(bad) = js.iter().chain(css.iter()).find(|f| !crate::ext::safe_relative_path(f)) {
        return Err(format!("Script with ID '{id}': '{bad}' is not a file of the extension"));
    }
    let matches = list("matches");
    if matches.is_empty() {
        return Err(format!("Script with ID '{id}' must specify 'matches'"));
    }
    if let Some(bad) = matches.iter().find(|m| crate::ext::MatchPattern::parse(m).is_none()) {
        return Err(format!("Script with ID '{id}' has invalid value for matches[0]: '{bad}'"));
    }
    let world = match v.get("world").and_then(|w| w.as_str()) {
        Some("MAIN") => Some("MAIN".to_string()),
        _ => None,
    };
    let run_at = match v.get("runAt").and_then(|r| r.as_str()) {
        Some(r @ ("document_start" | "document_end" | "document_idle")) => Some(r.to_string()),
        _ => None,
    };
    Ok((
        id,
        ContentScript {
            matches,
            exclude_matches: list("excludeMatches"),
            js: js.iter().map(|f| f.trim_start_matches('/').to_string()).collect(),
            css: css.iter().map(|f| f.trim_start_matches('/').to_string()).collect(),
            run_at,
            all_frames: v.get("allFrames").and_then(|a| a.as_bool()).unwrap_or(false),
            match_about_blank: false,
            world,
        },
    ))
}

/// A registered script as `getRegisteredContentScripts` returns it.
pub fn registered_json(id: &str, s: &ContentScript) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "matches": s.matches,
        "excludeMatches": s.exclude_matches,
        "js": s.js,
        "css": s.css,
        "runAt": s.run_at.clone().unwrap_or_else(|| "document_idle".into()),
        "allFrames": s.all_frames,
        "world": s.world.clone().unwrap_or_else(|| "ISOLATED".into()),
        "persistAcrossSessions": false,
        "matchOriginAsFallback": false,
    })
}

// ------------------------------------------------------------------ errors

/// Most errors kept per extension (in memory; the oldest go first).
pub const MAX_ERRORS: usize = 50;

/// An error of one of an extension's contexts, for krunk://extensions.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ExtError {
    pub message: String,
    /// `file:line` when known (an address on the extension's origin is shown as its path).
    pub source: String,
    /// Where it happened: `background`, `popup`, `page`, `offscreen`, `window`, `content script`.
    pub context: String,
    /// How often in a row.
    pub count: u32,
}

/// Adds an error to an extension's list: a repeat of the last one is counted, the list is
/// capped, the text is shortened. Returns whether the list changed in a way worth showing
/// (a repeat past 99 is not).
pub fn push_error(list: &mut Vec<ExtError>, ext_id: &str, message: &str, file: &str, line: u64, context: &str) -> bool {
    let message: String = message.trim().chars().take(600).collect();
    if message.is_empty() {
        return false;
    }
    let own = format!("{}/", origin(ext_id));
    let file = file.strip_prefix(&own).unwrap_or(file);
    let file: String = file.chars().take(200).collect();
    let source = match (file.is_empty(), line) {
        (true, _) => String::new(),
        (false, 0) => file,
        (false, n) => format!("{file}:{n}"),
    };
    if let Some(last) = list.last_mut() {
        if last.message == message && last.source == source && last.context == context {
            if last.count >= 99 {
                return false;
            }
            last.count += 1;
            return true;
        }
    }
    list.push(ExtError { message, source, context: context.to_string(), count: 1 });
    if list.len() > MAX_ERRORS {
        let extra = list.len() - MAX_ERRORS;
        list.drain(..extra);
    }
    true
}

// ------------------------------------------------------------------ ports

/// One end of a port: the host's own type for an extension context (the background, the
/// popup, an offscreen document, an extension page in a tab, a content-script world).
pub trait Endpoint: Clone + PartialEq + std::fmt::Debug {}
impl<T: Clone + PartialEq + std::fmt::Debug> Endpoint for T {}

/// A `chrome.runtime.connect` / `tabs.connect` port between an opener and the contexts it was
/// offered to. A receiver without `onConnect` listeners closes its end at once; when no
/// receiver is left the opener's port disconnects ("Receiving end does not exist").
#[derive(Debug, Clone, PartialEq)]
pub struct Port<E> {
    pub ext: String,
    pub opener: E,
    pub receivers: Vec<E>,
    /// A receiver accepted (has listeners and kept the port).
    pub connected: bool,
}

/// What the host does after a port operation: deliver an event to these ends.
#[derive(Debug, Clone, PartialEq)]
pub enum PortAction<E> {
    /// Deliver `data` (a posted message) to `to`.
    Message { to: E, data: serde_json::Value },
    /// Tell `to` that the port closed (`error`: why, for the opener's `runtime.lastError`).
    Disconnect { to: E, error: Option<String> },
}

/// Ports are capped per extension: a buggy extension opening ports in a loop must not grow the
/// host without bound.
pub const MAX_PORTS_PER_EXT: usize = 2048;

/// All open ports, by the id the opener chose.
#[derive(Debug, Clone)]
pub struct PortTable<E> {
    ports: HashMap<String, Port<E>>,
}

impl<E> Default for PortTable<E> {
    fn default() -> Self {
        PortTable { ports: HashMap::new() }
    }
}

/// A port id as the shim makes it: `p` and up to 40 safe characters.
pub fn valid_port_id(id: &str) -> bool {
    id.len() >= 2 && id.len() <= 41 && id.starts_with('p') && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

impl<E: Endpoint> PortTable<E> {
    pub fn len(&self) -> usize {
        self.ports.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ports.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&Port<E>> {
        self.ports.get(id)
    }

    /// Opens port `id` from `opener` to `receivers`. `Err` with the opener's disconnect when
    /// it cannot open (bad or taken id, too many ports, nobody to receive it).
    pub fn open(&mut self, ext: &str, id: &str, opener: E, receivers: Vec<E>) -> Result<(), PortAction<E>> {
        let refuse = |why: &str| PortAction::Disconnect { to: opener.clone(), error: Some(why.to_string()) };
        if !valid_port_id(id) || self.ports.contains_key(id) {
            return Err(refuse("Krunk: invalid port"));
        }
        if self.ports.values().filter(|p| p.ext == ext).count() >= MAX_PORTS_PER_EXT {
            return Err(refuse("Krunk: too many open ports"));
        }
        let receivers: Vec<E> = receivers.into_iter().filter(|r| *r != opener).collect();
        if receivers.is_empty() {
            return Err(refuse(NO_RECEIVER));
        }
        self.ports.insert(id.to_string(), Port { ext: ext.to_string(), opener, receivers, connected: false });
        Ok(())
    }

    /// A receiver kept the port (it had `onConnect` listeners).
    pub fn accept(&mut self, ext: &str, id: &str, from: &E) {
        if let Some(p) = self.ports.get_mut(id).filter(|p| p.ext == ext && p.receivers.contains(from)) {
            p.connected = true;
        }
    }

    /// `port.postMessage` from `from`: to the other side (from the opener to every receiver
    /// still on it, from a receiver to the opener). Nothing for an end that is not on the port
    /// (an extension cannot write into another's port).
    pub fn post(&self, ext: &str, id: &str, from: &E, data: serde_json::Value) -> Vec<PortAction<E>> {
        let Some(p) = self.ports.get(id).filter(|p| p.ext == ext) else { return Vec::new() };
        if *from == p.opener {
            p.receivers.iter().map(|r| PortAction::Message { to: r.clone(), data: data.clone() }).collect()
        } else if p.receivers.contains(from) {
            vec![PortAction::Message { to: p.opener.clone(), data }]
        } else {
            Vec::new()
        }
    }

    /// `port.disconnect()` (or a receiver without listeners) from `from`.
    pub fn close(&mut self, ext: &str, id: &str, from: &E) -> Vec<PortAction<E>> {
        let Some(p) = self.ports.get_mut(id).filter(|p| p.ext == ext) else { return Vec::new() };
        if *from == p.opener {
            let p = self.ports.remove(id).expect("present");
            return p.receivers.into_iter().map(|r| PortAction::Disconnect { to: r, error: None }).collect();
        }
        if !p.receivers.contains(from) {
            return Vec::new();
        }
        p.receivers.retain(|r| r != from);
        if p.receivers.is_empty() {
            let p = self.ports.remove(id).expect("present");
            let error = (!p.connected).then(|| NO_RECEIVER.to_string());
            return vec![PortAction::Disconnect { to: p.opener, error }];
        }
        Vec::new()
    }

    /// A context went away (a tab closed or navigated, the popup closed, the background
    /// stopped): every port it was on closes from its side. `gone` decides which ends. Each
    /// action comes with its extension and port.
    pub fn context_gone(&mut self, gone: impl Fn(&E) -> bool) -> Vec<(String, String, PortAction<E>)> {
        let ids: Vec<(String, String, E)> = self
            .ports
            .iter()
            .flat_map(|(id, p)| {
                std::iter::once(&p.opener)
                    .chain(p.receivers.iter())
                    .filter(|e| gone(e))
                    .map(|e| (id.clone(), p.ext.clone(), e.clone()))
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut out = Vec::new();
        for (id, ext, e) in ids {
            out.extend(self.close(&ext, &id, &e).into_iter().map(|a| (ext.clone(), id.clone(), a)));
        }
        // An end that went away is not told about its own ports.
        out.retain(|(_, _, a)| match a {
            PortAction::Disconnect { to, .. } | PortAction::Message { to, .. } => !gone(to),
        });
        out
    }

    /// Every port of an extension closes (it was disabled or removed).
    pub fn drop_ext(&mut self, ext: &str) {
        self.ports.retain(|_, p| p.ext != ext);
    }
}

/// Chrome's error for a message or port nobody receives.
pub const NO_RECEIVER: &str = "Could not establish connection. Receiving end does not exist.";

// ------------------------------------------------------------------ one message, several receivers

/// `runtime.sendMessage` reaches every page of the extension (but not its sender). The answer
/// is the first real response; a receiver without listeners says so (`none`), one whose
/// listeners did not answer says it is done (`empty`). When all are done: `undefined` if any
/// had listeners, else Chrome's "Receiving end does not exist".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Broadcast {
    pub waiting: usize,
    pub listened: bool,
}

/// What a broadcast's response means for the caller.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Keep waiting for the other receivers.
    Wait,
    /// Answer the caller with this value.
    Answer(serde_json::Value),
    /// Answer the caller with this error.
    Fail(String),
}

impl Broadcast {
    pub fn new(receivers: usize) -> Broadcast {
        Broadcast { waiting: receivers, listened: false }
    }

    /// One receiver answered: `none` (it had no listeners), `empty` (listeners, no answer) or
    /// a real response.
    pub fn respond(&mut self, none: bool, empty: bool, data: serde_json::Value) -> Outcome {
        if !none && !empty {
            return Outcome::Answer(data);
        }
        self.waiting = self.waiting.saturating_sub(1);
        self.listened |= empty;
        if self.waiting > 0 {
            Outcome::Wait
        } else if self.listened {
            Outcome::Answer(serde_json::Value::Null)
        } else {
            Outcome::Fail(NO_RECEIVER.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ID: &str = "abcdefghijklmnopabcdefghijklmnop";

    #[test]
    fn the_bootstrap_sits_next_to_the_service_worker() {
        assert_eq!(worker_boot_path("service-worker.js"), "/__krunk_sw__.js");
        assert_eq!(worker_boot_path("/background/sw.js"), "/background/__krunk_sw__.js");
        assert_eq!(worker_boot_path("a/b/c.js"), "/a/b/__krunk_sw__.js");
        let template =
            "var b = self.postMessage.bind(self);\n(function () {\n/*KRUNK_SHIM*/\n})();\nstart(/*KRUNK_SW*/null);";
        let classic = worker_bootstrap(template, "var shim = '/*KRUNK_SW*/null';", "background/sw.js", false);
        assert!(classic.contains(r#""script":"./sw.js""#) && classic.contains(r#""module":false"#), "{classic}");
        assert!(!classic.contains("start(/*KRUNK_SW*/null)"));
        // The shim's own text is left alone, and it runs after the bridge took postMessage.
        assert!(classic.contains("var shim = '/*KRUNK_SW*/null';"));
        assert!(classic.find("self.postMessage.bind(self)").unwrap() < classic.find("var shim").unwrap());
        let module = worker_bootstrap(template, "", "sw.mjs", true);
        assert!(module.contains(r#""script":"./sw.mjs""#) && module.contains(r#""module":true"#));
        // A file name cannot break out of the string.
        let odd = worker_bootstrap(template, "", "x\");alert(1);(\".js", false);
        assert!(odd.contains(r#""script":"./x\");alert(1);(\".js""#), "{odd}");
    }

    #[test]
    fn extension_addresses_in_chromes_form_and_back() {
        let page = format!("https://{ID}.ext.krunk.invalid/popup.html?x=1#y");
        assert_eq!(chrome_url(&page), format!("chrome-extension://{ID}/popup.html?x=1#y"));
        assert_eq!(chrome_url(&format!("https://{ID}.ext.krunk.invalid")), format!("chrome-extension://{ID}"));
        assert_eq!(
            krunk_url(&format!("chrome-extension://{ID}/home.html#onboarding")),
            format!("https://{ID}.ext.krunk.invalid/home.html#onboarding")
        );
        assert_eq!(krunk_url(&format!("chrome-extension://{ID}")), format!("https://{ID}.ext.krunk.invalid/"));
        for other in
            ["https://example.com/", "https://evil.ext.krunk.invalid/", "chrome-extension://short/x", "about:blank"]
        {
            assert_eq!(chrome_url(other), other);
            assert_eq!(krunk_url(other), other);
        }
    }

    #[test]
    fn sandboxed_pages_get_their_own_csp() {
        let m = Manifest::parse(
            r#"{"manifest_version":3,"name":"x","version":"1","sandbox":{"pages":["snaps/index.html"]},
                "content_security_policy":{"sandbox":"sandbox allow-scripts; script-src 'self' 'unsafe-eval'"}}"#,
        )
        .unwrap();
        assert!(is_sandbox_page(&m, "snaps/index.html") && is_sandbox_page(&m, "/snaps/index.html"));
        assert!(!is_sandbox_page(&m, "popup.html"));
        assert_eq!(sandbox_csp(&m), "sandbox allow-scripts; script-src 'self' 'unsafe-eval'");
        let bare = Manifest::parse(r#"{"manifest_version":3,"name":"x","version":"1","sandbox":{"pages":["s.html"]}}"#)
            .unwrap();
        assert_eq!(sandbox_csp(&bare), DEFAULT_SANDBOX_CSP);
        // A sandbox CSP without the sandbox directive still gets a unique origin.
        let weak = Manifest::parse(
            r#"{"manifest_version":3,"name":"x","version":"1","content_security_policy":{"sandbox":"script-src 'self'"}}"#,
        )
        .unwrap();
        assert!(sandbox_csp(&weak).starts_with("sandbox"));
        assert!(!is_sandbox_page(&weak, "s.html"));
    }

    #[test]
    fn main_world_bundle_runs_each_file_on_its_own() {
        let b = main_bundle(
            ID,
            &[MainScript {
                matches: vec!["<all_urls>".into()],
                exclude_matches: vec![],
                all_frames: true,
                run_at: "document_start".into(),
                js: vec!["const a = 1; window.x = a;".into(), "const a = 2;".into()],
            }],
        );
        assert!(b.starts_with("(function () {") && b.contains("krunk\\.invalid"));
        assert_eq!(b.matches("function () {\nconst a").count(), 2, "two files, two functions");
        assert!(b.contains("[\"<all_urls>\"], [], true, \"document_start\""));
        assert!(!b.contains("'use strict'") && !b.contains("chrome."));
    }

    #[derive(Debug, Clone, PartialEq)]
    enum E {
        Bg,
        Popup,
        Tab(u32),
    }

    #[test]
    fn a_port_carries_messages_both_ways_and_closes() {
        let mut t: PortTable<E> = PortTable::default();
        t.open("x", "p1", E::Tab(1), vec![E::Bg, E::Popup, E::Tab(1)]).unwrap();
        assert_eq!(t.get("p1").unwrap().receivers, vec![E::Bg, E::Popup], "never to itself");
        // The popup has no onConnect listeners: it closes its end, the opener stays.
        assert!(t.close("x", "p1", &E::Popup).is_empty());
        t.accept("x", "p1", &E::Bg);
        assert_eq!(t.post("x", "p1", &E::Tab(1), json!(1)), vec![PortAction::Message { to: E::Bg, data: json!(1) }]);
        assert_eq!(t.post("x", "p1", &E::Bg, json!(2)), vec![PortAction::Message { to: E::Tab(1), data: json!(2) }]);
        // Another extension, or an end not on the port, cannot use it.
        assert!(t.post("y", "p1", &E::Bg, json!(3)).is_empty());
        assert!(t.post("x", "p1", &E::Popup, json!(3)).is_empty());
        // The background disconnects: the opener is told, without an error (it was connected).
        assert_eq!(t.close("x", "p1", &E::Bg), vec![PortAction::Disconnect { to: E::Tab(1), error: None }]);
        assert!(t.is_empty());
    }

    #[test]
    fn a_port_nobody_takes_fails_like_chrome() {
        let mut t: PortTable<E> = PortTable::default();
        assert_eq!(
            t.open("x", "p1", E::Bg, vec![E::Bg]),
            Err(PortAction::Disconnect { to: E::Bg, error: Some(NO_RECEIVER.into()) })
        );
        t.open("x", "p2", E::Tab(1), vec![E::Bg]).unwrap();
        assert_eq!(
            t.close("x", "p2", &E::Bg),
            vec![PortAction::Disconnect { to: E::Tab(1), error: Some(NO_RECEIVER.into()) }]
        );
        assert!(t.open("x", "bad id!", E::Tab(1), vec![E::Bg]).is_err());
        t.open("x", "p3", E::Tab(1), vec![E::Bg]).unwrap();
        assert!(t.open("x", "p3", E::Tab(2), vec![E::Bg]).is_err(), "an id that is taken");
    }

    #[test]
    fn a_context_that_goes_away_closes_its_ports() {
        let mut t: PortTable<E> = PortTable::default();
        t.open("x", "p1", E::Tab(1), vec![E::Bg]).unwrap();
        t.accept("x", "p1", &E::Bg);
        t.open("x", "p2", E::Tab(2), vec![E::Bg]).unwrap();
        t.accept("x", "p2", &E::Bg);
        t.open("x", "p3", E::Bg, vec![E::Tab(1)]).unwrap();
        // Tab 1 closed: its port to the background ends there; the port from the background to
        // it too (the background is told).
        let mut acts = t.context_gone(|e| *e == E::Tab(1));
        acts.sort_by_key(|a| format!("{a:?}"));
        assert_eq!(
            acts,
            vec![
                ("x".to_string(), "p1".to_string(), PortAction::Disconnect { to: E::Bg, error: None }),
                (
                    "x".to_string(),
                    "p3".to_string(),
                    PortAction::Disconnect { to: E::Bg, error: Some(NO_RECEIVER.into()) }
                ),
            ]
        );
        assert_eq!(t.len(), 1);
        // The background stopped: tab 2 is told.
        assert_eq!(
            t.context_gone(|e| *e == E::Bg),
            vec![("x".to_string(), "p2".to_string(), PortAction::Disconnect { to: E::Tab(2), error: None })]
        );
        assert!(t.is_empty());
    }

    #[test]
    fn ports_are_capped_per_extension() {
        let mut t: PortTable<E> = PortTable::default();
        for i in 0..MAX_PORTS_PER_EXT {
            t.open("x", &format!("p{i}"), E::Tab(1), vec![E::Bg]).unwrap();
        }
        assert!(t.open("x", "pmore", E::Tab(1), vec![E::Bg]).is_err());
        assert!(t.open("y", "pother", E::Tab(1), vec![E::Bg]).is_ok(), "per extension");
        t.drop_ext("x");
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn a_broadcast_answers_once_everyone_is_done() {
        // A real answer wins at once.
        assert_eq!(Broadcast::new(3).respond(false, false, json!("hi")), Outcome::Answer(json!("hi")));
        // Nobody listens anywhere: Chrome's error.
        let mut b = Broadcast::new(2);
        assert_eq!(b.respond(true, false, json!(null)), Outcome::Wait);
        assert_eq!(b.respond(true, false, json!(null)), Outcome::Fail(NO_RECEIVER.into()));
        // Someone listened but did not answer: undefined, once all are done.
        let mut b = Broadcast::new(2);
        assert_eq!(b.respond(false, true, json!(null)), Outcome::Wait);
        assert_eq!(b.respond(true, false, json!(null)), Outcome::Answer(json!(null)));
    }

    #[test]
    fn registered_scripts_are_checked_and_listed() {
        let (id, cs) = parse_registered(&json!({
            "id": "inpage", "matches": ["https://*/*"], "js": ["/scripts/inpage.js"],
            "runAt": "document_start", "world": "MAIN", "allFrames": true
        }))
        .unwrap();
        assert_eq!(id, "inpage");
        assert_eq!(cs.js, vec!["scripts/inpage.js"]);
        assert_eq!(cs.world.as_deref(), Some("MAIN"));
        assert!(cs.all_frames);
        let back = registered_json(&id, &cs);
        assert_eq!(back["runAt"], "document_start");
        assert_eq!(back["world"], "MAIN");
        // No id, a file outside the package, no or bad matches: refused.
        assert!(parse_registered(&json!({ "matches": ["https://*/*"], "js": ["a.js"] })).is_err());
        assert!(parse_registered(&json!({ "id": "x", "matches": ["https://*/*"], "js": ["../a.js"] })).is_err());
        assert!(parse_registered(&json!({ "id": "x", "js": ["a.js"] })).is_err());
        assert!(parse_registered(&json!({ "id": "x", "matches": ["nonsense"], "js": ["a.js"] })).is_err());
        assert!(parse_registered(&json!({ "id": "x", "matches": ["https://*/*"] })).is_err());
    }

    #[test]
    fn errors_are_capped_and_repeats_counted() {
        let mut list = Vec::new();
        let file = format!("{}/scripts/a.js", origin(ID));
        assert!(push_error(&mut list, ID, " boom ", &file, 12, "background"));
        assert_eq!(list[0].source, "scripts/a.js:12");
        assert_eq!(list[0].message, "boom");
        assert!(push_error(&mut list, ID, "boom", &file, 12, "background"));
        assert_eq!((list.len(), list[0].count), (1, 2));
        assert!(!push_error(&mut list, ID, "  ", "", 0, "popup"));
        for i in 0..(MAX_ERRORS + 10) {
            push_error(&mut list, ID, &format!("e{i}"), "", 0, "popup");
        }
        assert_eq!(list.len(), MAX_ERRORS);
        assert_eq!(list.last().unwrap().message, format!("e{}", MAX_ERRORS + 9));
        assert_eq!(list.last().unwrap().source, "");
        // A loop of the same error stops being news.
        let mut one = Vec::new();
        for _ in 0..99 {
            assert!(push_error(&mut one, ID, "again", "", 0, "page"));
        }
        assert!(!push_error(&mut one, ID, "again", "", 0, "page"));
    }
}
