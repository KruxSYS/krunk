//! Userscripts (0.12.0, WP-E): the `==UserScript==` metadata block, URL matching and the
//! per-tab loader the host registers with `Page.addScriptToEvaluateOnNewDocument`.
//!
//! Supported: `@name @namespace @version @description @match @include @exclude @run-at
//! @grant @noframes`. Grants: `none`, `GM_addStyle`, `GM_getValue`/`GM_setValue`/
//! `GM_deleteValue`/`GM_listValues`, `GM_setClipboard`, `GM_notification`, `GM_openInTab`
//! (`unsafeWindow` is `window`). Not supported in 0.12.0 (warnings): `GM_xmlhttpRequest`,
//! `@require`, `@resource`, regular-expression `@include`.

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

/// Parses the metadata block. Errors: no block, or no `@name`.
pub fn parse_meta(source: &str) -> Result<Meta, String> {
    let _ = source;
    Err("WP-E".into())
}

/// Does a script with this metadata run on `url`? (`@exclude` wins; `@match` uses Chrome match
/// patterns, `@include`/`@exclude` are whole-URL `*` globs; never on Krunk's own origins.)
pub fn matches_url(meta: &Meta, url: &str) -> bool {
    let _ = (meta, url);
    false
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

/// The loader IIFE for one tab: deletes the binding `binding` from `window` first thing, then
/// runs each matching script at its run-at time with its granted GM functions, reporting GM
/// calls as JSON `{nonce, script, op, …}` through the binding.
pub fn bundle(scripts: &[LoaderScript], binding: &str, nonce: &str) -> String {
    let _ = (scripts, binding, nonce);
    String::new()
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
}

/// Size limits.
pub const MAX_SOURCE_BYTES: usize = 200_000;
pub const MAX_VALUES_BYTES: usize = 1024 * 1024;
