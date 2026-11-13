//! The agent's tools: names, descriptions, JSON schemas and typed arguments.
//!
//! The schemas are what the provider sees ([`specs`]); [`validate`] checks a call's arguments
//! against the same schema before the host runs anything, and [`args`] holds the typed form
//! the host parses them into. Results are capped by the host ([`super::redact::cap`]).

use super::{Category, CategorySet, ToolSpec};
use serde::Deserialize;
use serde_json::Value;

/// Default cap of a tool result (characters).
pub const RESULT_CAP: usize = 8_000;
/// `get_html` / `get_text` may ask for up to this much per call.
pub const RESULT_CAP_MAX: usize = 20_000;
/// Default tool timeout (ms); `wait_for` uses its own (at most [`WAIT_MAX_MS`]).
pub const TOOL_TIMEOUT_MS: u64 = 10_000;
pub const WAIT_MAX_MS: u64 = 30_000;

/// What a tool works on; decides when it is offered and what the host checks first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// Reads or acts on the target tab's page: refused on built-in pages.
    Page,
    /// Navigation, tabs and windows.
    Browser,
    Settings,
    Userscripts,
    Macros,
    /// `ask_user`: handled by the session, not the host.
    Ask,
    /// History, bookmarks, downloads, cookies: offered only through the gate. 0.16.0:
    /// `fill_password` too (`Category::Passwords`).
    Sensitive(Category),
    /// `finish`: handled by the session.
    Finish,
    /// 0.16.0: the scratchpad's notes and plan ([`super::scratch`]): handled by the session,
    /// always offered, never seen by the host.
    Scratch,
}

pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    /// JSON schema of the arguments (an object).
    pub schema: &'static str,
    pub group: Group,
}

impl Tool {
    pub fn parameters(&self) -> Value {
        serde_json::from_str(self.schema).unwrap_or_else(|_| serde_json::json!({"type":"object"}))
    }
    pub fn spec(&self) -> ToolSpec {
        ToolSpec { name: self.name.into(), description: self.description.into(), parameters: self.parameters() }
    }
}

const EMPTY: &str = r#"{"type":"object","properties":{},"additionalProperties":false}"#;

pub static TOOLS: &[Tool] = &[
    Tool {
        name: "get_page_context",
        description: "The target tab's address, title, load state, private flag, viewport, scroll position, selected text and a compact outline (headings, forms with their fields, link and button counts).",
        schema: EMPTY,
        group: Group::Page,
    },
    Tool {
        name: "get_html",
        description: "HTML of the page or of the first element matching a CSS selector, cleaned (script/style bodies dropped unless asked) and paged. Password and card field values are always redacted.",
        schema: r#"{"type":"object","properties":{
            "selector":{"type":"string","maxLength":1000,"description":"CSS selector; the whole document when omitted"},
            "offset":{"type":"integer","minimum":0,"description":"Character offset to continue a truncated result"},
            "max_chars":{"type":"integer","minimum":1,"maximum":20000},
            "include_scripts":{"type":"boolean"}},"additionalProperties":false}"#,
        group: Group::Page,
    },
    Tool {
        name: "get_text",
        description: "Readable text of the page or of an element (like innerText), paged. Form field values are not included.",
        schema: r#"{"type":"object","properties":{
            "selector":{"type":"string","maxLength":1000},
            "offset":{"type":"integer","minimum":0},
            "max_chars":{"type":"integer","minimum":1,"maximum":20000}},"additionalProperties":false}"#,
        group: Group::Page,
    },
    Tool {
        name: "query",
        description: "Elements matching a CSS selector: tag, text, attributes, bounding box in viewport CSS pixels, visibility.",
        schema: r#"{"type":"object","properties":{
            "selector":{"type":"string","minLength":1,"maxLength":1000},
            "limit":{"type":"integer","minimum":1,"maximum":50},
            "offset":{"type":"integer","minimum":0}},"required":["selector"],"additionalProperties":false}"#,
        group: Group::Page,
    },
    Tool {
        name: "list_links",
        description: "Links on the page: text, absolute address, rel and target. Optional filter on text or address.",
        schema: r#"{"type":"object","properties":{
            "filter":{"type":"string","maxLength":200},
            "limit":{"type":"integer","minimum":1,"maximum":200},
            "offset":{"type":"integer","minimum":0}},"additionalProperties":false}"#,
        group: Group::Page,
    },
    Tool {
        name: "list_forms",
        description: "Forms and their fields (a selector for each field, label, type, name, required, value; password and card values redacted) and buttons.",
        schema: EMPTY,
        group: Group::Page,
    },
    Tool {
        name: "list_media",
        description: "Video and audio on the page (src, currentSrc, type, duration, paused) plus media and XHR requests of this tab (m3u8, mpd, mp4 and similar). blob: addresses are reported as such.",
        schema: EMPTY,
        group: Group::Page,
    },
    Tool {
        name: "screenshot",
        description: "A screenshot of the visible part of the page, given to you as an image when the model accepts images.",
        schema: EMPTY,
        group: Group::Page,
    },
    Tool {
        name: "save_screenshot",
        description: "Save a PNG screenshot of the page (the visible part, or the full page) to the user's Downloads folder.",
        schema: r#"{"type":"object","properties":{
            "full_page":{"type":"boolean"},
            "file_name":{"type":"string","maxLength":120}},"additionalProperties":false}"#,
        group: Group::Page,
    },
    Tool {
        name: "click",
        description: "Click an element (by CSS selector, scrolled into view) or a point (viewport CSS pixels), with real mouse events.",
        schema: r#"{"type":"object","properties":{
            "selector":{"type":"string","maxLength":1000},
            "x":{"type":"number"},"y":{"type":"number"},
            "button":{"type":"string","enum":["left","middle","right"]},
            "count":{"type":"integer","minimum":1,"maximum":2}},"additionalProperties":false}"#,
        group: Group::Page,
    },
    Tool {
        name: "type_text",
        description: "Type text with key events into the focused element, or into the element matching the selector (focused first). Optionally press Enter afterwards. Refused for password and card fields (a password is entered only with fill_password, which you get when the user asks for it).",
        schema: r#"{"type":"object","properties":{
            "text":{"type":"string","maxLength":5000},
            "selector":{"type":"string","maxLength":1000},
            "submit":{"type":"boolean"}},"required":["text"],"additionalProperties":false}"#,
        group: Group::Page,
    },
    Tool {
        name: "fill_fields",
        description: "Set the values of several fields at once (CSS selector -> value) and fire input and change events. Password and card fields are refused (a password is entered only with fill_password, which you get when the user asks for it).",
        schema: r#"{"type":"object","properties":{
            "fields":{"type":"object","additionalProperties":{"type":"string","maxLength":5000},"minProperties":1,"maxProperties":50}},
            "required":["fields"],"additionalProperties":false}"#,
        group: Group::Page,
    },
    Tool {
        name: "select_option",
        description: "Choose options of a <select> by value or visible label and fire change.",
        schema: r#"{"type":"object","properties":{
            "selector":{"type":"string","minLength":1,"maxLength":1000},
            "values":{"type":"array","items":{"type":"string","maxLength":500},"minItems":1,"maxItems":50}},
            "required":["selector","values"],"additionalProperties":false}"#,
        group: Group::Page,
    },
    Tool {
        name: "press_keys",
        description: "Press a key or chord (\"Enter\", \"a\", \"Control+A\", \"Shift+Tab\") on the page. action \"down\" holds it (auto-repeating unless repeat is false) until \"up\", Stop or the end of the task; hold_ms holds it for that long.",
        schema: r#"{"type":"object","properties":{
            "keys":{"type":"string","minLength":1,"maxLength":100},
            "action":{"type":"string","enum":["press","down","up"]},
            "hold_ms":{"type":"integer","minimum":0,"maximum":10000},
            "repeat":{"type":"boolean"}},"required":["keys"],"additionalProperties":false}"#,
        group: Group::Page,
    },
    Tool {
        name: "scroll",
        description: "Scroll the page by dx/dy CSS pixels, to the top or bottom, or an element into view. Returns the new scroll position.",
        schema: r#"{"type":"object","properties":{
            "dx":{"type":"number"},"dy":{"type":"number"},
            "to":{"type":"string","enum":["top","bottom"]},
            "selector":{"type":"string","maxLength":1000}},"additionalProperties":false}"#,
        group: Group::Page,
    },
    Tool {
        name: "wait_for",
        description: "Wait for an element to appear (selector), for the next navigation to finish loading, or a number of milliseconds.",
        schema: r#"{"type":"object","properties":{
            "selector":{"type":"string","maxLength":1000},
            "navigation":{"type":"boolean"},
            "ms":{"type":"integer","minimum":0,"maximum":30000},
            "timeout_ms":{"type":"integer","minimum":100,"maximum":30000}},"additionalProperties":false}"#,
        group: Group::Page,
    },
    Tool {
        name: "eval_js",
        description: "Run JavaScript in the page's main world and return its (JSON-serialised) result; promises are awaited. Not available on Krunk's own pages or while a password or card field holds a value.",
        schema: r#"{"type":"object","properties":{
            "code":{"type":"string","minLength":1,"maxLength":20000},
            "await_promise":{"type":"boolean"}},"required":["code"],"additionalProperties":false}"#,
        group: Group::Page,
    },
    Tool {
        name: "edit_page",
        description: "Change the current document live: set an element's text, set or remove an attribute, set inline style (CSS declarations), remove elements, or inject a stylesheet (insert_css). all=true applies to every match.",
        schema: r#"{"type":"object","properties":{
            "action":{"type":"string","enum":["set_text","set_attribute","remove_attribute","set_style","remove","insert_css"]},
            "selector":{"type":"string","maxLength":1000},
            "name":{"type":"string","maxLength":200},
            "value":{"type":"string","maxLength":20000},
            "css":{"type":"string","maxLength":20000},
            "all":{"type":"boolean"}},"required":["action"],"additionalProperties":false}"#,
        group: Group::Page,
    },
    Tool {
        name: "set_title",
        description: "Set the page's title (document.title) as shown on its tab.",
        schema: r#"{"type":"object","properties":{"title":{"type":"string","maxLength":500}},"required":["title"],"additionalProperties":false}"#,
        group: Group::Page,
    },
    Tool {
        name: "navigate",
        description: "Load an address (http or https, or a krunk:// page) in the target tab.",
        schema: r#"{"type":"object","properties":{"url":{"type":"string","minLength":1,"maxLength":8000}},"required":["url"],"additionalProperties":false}"#,
        group: Group::Browser,
    },
    Tool { name: "back", description: "Go back in the target tab's history.", schema: EMPTY, group: Group::Browser },
    Tool { name: "forward", description: "Go forward in the target tab's history.", schema: EMPTY, group: Group::Browser },
    Tool { name: "reload", description: "Reload the target tab.", schema: EMPTY, group: Group::Browser },
    Tool {
        name: "open_tab",
        description: "Open a new tab in this window with an address, or with a web search for query (the user's search engine). It becomes the target unless background is true.",
        schema: r#"{"type":"object","properties":{
            "url":{"type":"string","maxLength":8000},
            "query":{"type":"string","maxLength":1000},
            "background":{"type":"boolean"}},"additionalProperties":false}"#,
        group: Group::Browser,
    },
    Tool {
        name: "switch_tab",
        description: "Make another tab of this window active (and the target).",
        schema: r#"{"type":"object","properties":{"tab_id":{"type":"integer","minimum":1}},"required":["tab_id"],"additionalProperties":false}"#,
        group: Group::Browser,
    },
    Tool {
        name: "close_tab",
        description: "Close a tab of this window (the target tab when tab_id is omitted).",
        schema: r#"{"type":"object","properties":{"tab_id":{"type":"integer","minimum":1}},"additionalProperties":false}"#,
        group: Group::Browser,
    },
    Tool {
        name: "list_tabs",
        description: "The tabs of this window: id, title, address, active, private, loading.",
        schema: EMPTY,
        group: Group::Browser,
    },
    Tool {
        name: "get_settings",
        description: "The browser settings you may read and change, with their values and allowed values.",
        schema: EMPTY,
        group: Group::Settings,
    },
    Tool {
        name: "set_setting",
        description: "Change one allowed browser setting (see get_settings for keys and values).",
        schema: r#"{"type":"object","properties":{"key":{"type":"string","minLength":1,"maxLength":100},"value":{}},"required":["key","value"],"additionalProperties":false}"#,
        group: Group::Settings,
    },
    // 0.15.0
    Tool {
        name: "set_new_tab_background_url",
        description: "Download a picture from an https address (PNG, JPEG, WebP, GIF or AVIF, at most 15 MB) and make it the new tab page's background, as if the user had chosen it as their own image.",
        schema: r#"{"type":"object","properties":{"url":{"type":"string","minLength":1,"maxLength":2048}},"required":["url"],"additionalProperties":false}"#,
        group: Group::Settings,
    },
    Tool {
        name: "list_userscripts",
        description: "The user's userscripts: id, name, enabled, @match patterns, created by AI, last run.",
        schema: EMPTY,
        group: Group::Userscripts,
    },
    Tool {
        name: "save_userscript",
        description: "Create (no id) or update a Tampermonkey-style userscript. The source must start with a ==UserScript== metadata block with @name and @match (or @include). A new script is enabled and runs on the target tab at once when it matches. Supported grants: none, GM_addStyle, GM_getValue/GM_setValue/GM_deleteValue/GM_listValues, GM_setClipboard, GM_notification, GM_openInTab, and GM.wasm (the WASM memory inspector, every function returns a promise: GM.wasm.memories(), read({mem, addr, type}), write({mem, addr, type, value}), scan({mem, type, kind, value}), next({scan, kind, value}), results({scan, offset, limit}), freeze({mem, addr, type, value}) giving a number for unfreeze(number); a pointer path is where: {base, offsets} instead of addr). GM_xmlhttpRequest, @require and @resource are not supported. A long script (more than about 20,000 characters) goes in parts: the first with more=true, the next ones with the returned draft id and more=true, the last with the draft id and without more; it is checked and saved when the last part arrives.",
        schema: r#"{"type":"object","properties":{
            "id":{"type":"string","maxLength":64},
            "source":{"type":"string","minLength":1,"maxLength":200000,"description":"The script, or one part of it"},
            "draft":{"type":"string","maxLength":64,"description":"The draft id of earlier parts: this source is appended to them"},
            "more":{"type":"boolean","description":"true: more parts follow and nothing is saved yet"}},"required":["source"],"additionalProperties":false}"#,
        group: Group::Userscripts,
    },
    Tool {
        name: "toggle_userscript",
        description: "Enable or disable a userscript.",
        schema: r#"{"type":"object","properties":{"id":{"type":"string","minLength":1,"maxLength":64},"enabled":{"type":"boolean"}},"required":["id","enabled"],"additionalProperties":false}"#,
        group: Group::Userscripts,
    },
    Tool {
        name: "list_macros",
        description: "The user's macros: id, name, hotkey, created by AI.",
        schema: EMPTY,
        group: Group::Macros,
    },
    Tool {
        name: "save_macro",
        description: "Create (no id) or update a macro: JavaScript run with an async API against the active tab. API: await key.down(k,{repeat}), key.up(k), key.press(k), key.hold(k,ms); mouse.click(x,y | selector), mouse.move(x,y); type(text); wait(ms); page.eval(js); page.query(sel); tabs.open(url); tabs.switch(id); navigate(url); repeat(n | 'forever', fn); toast(msg). Keys are held until key.up or Stop. Optional hotkey like \"Ctrl+Alt+K\". A long macro goes in parts like a long userscript (more=true, then the returned draft id); it is checked and saved when the last part arrives.",
        schema: r#"{"type":"object","properties":{
            "id":{"type":"string","maxLength":64},
            "name":{"type":"string","minLength":1,"maxLength":200},
            "source":{"type":"string","minLength":1,"maxLength":200000,"description":"The macro, or one part of it"},
            "hotkey":{"type":"string","maxLength":40},
            "draft":{"type":"string","maxLength":64,"description":"The draft id of earlier parts: this source is appended to them"},
            "more":{"type":"boolean","description":"true: more parts follow and nothing is saved yet"}},"required":["name","source"],"additionalProperties":false}"#,
        group: Group::Macros,
    },
    Tool {
        name: "run_macro",
        description: "Run a macro now. Returns when it ends, or after 2 seconds while it keeps running (the toolbar then shows Stop).",
        schema: r#"{"type":"object","properties":{"id":{"type":"string","minLength":1,"maxLength":64}},"required":["id"],"additionalProperties":false}"#,
        group: Group::Macros,
    },
    Tool {
        name: "ask_user",
        description: "Ask the user a multiple-choice question and wait for the answer. Use it when you need a decision or a detail only the user knows.",
        schema: r#"{"type":"object","properties":{
            "question":{"type":"string","minLength":1,"maxLength":500},
            "options":{"type":"array","items":{"type":"string","minLength":1,"maxLength":120},"minItems":2,"maxItems":8},
            "multi":{"type":"boolean"},
            "allow_other":{"type":"boolean"}},"required":["question","options"],"additionalProperties":false}"#,
        group: Group::Ask,
    },
    // 0.16.0: the scratchpad. The schemas' bounds are loose on purpose: the exact limits are
    // checked by `scratch`, whose errors name them.
    Tool {
        name: "note_write",
        description: "Write a note in your scratchpad, your working memory for this conversation (kept in memory only, never saved). mode \"replace\" (the default) sets the note's content, \"append\" adds to its end as it is. Use notes for intermediate findings and extracted data. Limits: 64 notes, 32 KiB per note, 256 KiB in total; a name is 1 to 64 characters of letters, digits, '-', '_' and '.'.",
        schema: r#"{"type":"object","properties":{
            "name":{"type":"string","minLength":1,"maxLength":200},
            "content":{"type":"string","maxLength":200000},
            "mode":{"type":"string","enum":["replace","append"]}},"required":["name","content"],"additionalProperties":false}"#,
        group: Group::Scratch,
    },
    Tool {
        name: "note_read",
        description: "Read a note of your scratchpad back (its text, paged like get_text: offset and max_chars for a long note).",
        schema: r#"{"type":"object","properties":{
            "name":{"type":"string","minLength":1,"maxLength":200},
            "offset":{"type":"integer","minimum":0,"description":"Character offset to continue a truncated result"},
            "max_chars":{"type":"integer","minimum":1,"maximum":20000}},"required":["name"],"additionalProperties":false}"#,
        group: Group::Scratch,
    },
    Tool {
        name: "note_delete",
        description: "Delete a note of your scratchpad.",
        schema: r#"{"type":"object","properties":{"name":{"type":"string","minLength":1,"maxLength":200}},"required":["name"],"additionalProperties":false}"#,
        group: Group::Scratch,
    },
    Tool {
        name: "note_list",
        description: "The notes of your scratchpad: name, size in bytes and first line of each, and the limits.",
        schema: EMPTY,
        group: Group::Scratch,
    },
    Tool {
        name: "plan_set",
        description: "Set your plan for the task: replaces the whole list (an empty list clears it). Each item has a text and a state: todo (the default), doing, done or dropped; an optional id of your own (else its position, \"1\", \"2\", …). Limits: 100 items of at most 300 characters each. The plan comes back to you with every request.",
        schema: r#"{"type":"object","properties":{
            "items":{"type":"array","maxItems":1000,"items":{"type":"object","properties":{
                "text":{"type":"string","maxLength":5000},
                "state":{"type":"string","enum":["todo","doing","done","dropped"]},
                "id":{"type":"string","maxLength":200}},"required":["text"],"additionalProperties":false}}},
            "required":["items"],"additionalProperties":false}"#,
        group: Group::Scratch,
    },
    Tool {
        name: "plan_update",
        description: "Change one item of your plan, found by index (1 is the first item) or by id: a new state (todo, doing, done, dropped), a new text (at most 300 characters), or both.",
        schema: r#"{"type":"object","properties":{
            "index":{"type":"integer","minimum":1,"maximum":1000},
            "id":{"type":"string","maxLength":200},
            "state":{"type":"string","enum":["todo","doing","done","dropped"]},
            "text":{"type":"string","maxLength":5000}},"additionalProperties":false}"#,
        group: Group::Scratch,
    },
    Tool {
        name: "get_history",
        description: "The user's browsing history (newest first), optionally filtered by words in the title or address.",
        schema: r#"{"type":"object","properties":{
            "query":{"type":"string","maxLength":200},
            "days":{"type":"integer","minimum":1,"maximum":365},
            "limit":{"type":"integer","minimum":1,"maximum":100}},"additionalProperties":false}"#,
        group: Group::Sensitive(Category::History),
    },
    Tool {
        name: "get_bookmarks",
        description: "The user's bookmarks (title, address, folder), optionally filtered.",
        schema: r#"{"type":"object","properties":{
            "query":{"type":"string","maxLength":200},
            "limit":{"type":"integer","minimum":1,"maximum":200}},"additionalProperties":false}"#,
        group: Group::Sensitive(Category::Bookmarks),
    },
    Tool {
        name: "get_downloads",
        description: "The downloads list (file name, address, state, size, date), newest first.",
        schema: r#"{"type":"object","properties":{"limit":{"type":"integer","minimum":1,"maximum":100}},"additionalProperties":false}"#,
        group: Group::Sensitive(Category::Downloads),
    },
    Tool {
        name: "get_cookies_for_site",
        description: "Cookies stored for a site (name, domain, path, expiry, flags, value shortened).",
        schema: r#"{"type":"object","properties":{"site":{"type":"string","minLength":1,"maxLength":253}},"required":["site"],"additionalProperties":false}"#,
        group: Group::Sensitive(Category::Cookies),
    },
    // 0.16.0: the one tool that writes password fields. Gated like the sensitive data: offered
    // only when the user's message asks for a password, and the user allows the site first.
    Tool {
        name: "fill_password",
        description: "Enter a password into the page's password field(s). You have this tool only because the user's message asks for a password to be created or entered; the user confirms the site first. mode \"generate\": Krunk creates a strong password and fills it into the new-password field and its confirmation of a sign-up or change-password form. You never see it: the user gets it in the panel, and Krunk's password manager saves it (or offers to) when the form is sent. mode \"provided\": enters a password the user gave you in this conversation; pass it as password, exactly as given. selector: a password field or its form (default: the form of the focused password field, else the page's only form with password fields). The result says which fields were filled, never the value. Card fields are never written, and every other tool still refuses password fields.",
        schema: r#"{"type":"object","properties":{
            "mode":{"type":"string","enum":["generate","provided"]},
            "password":{"type":"string","minLength":1,"maxLength":1000,"description":"mode provided only: the password the user gave"},
            "selector":{"type":"string","maxLength":1000}},"required":["mode"],"additionalProperties":false}"#,
        group: Group::Sensitive(Category::Passwords),
    },
    // 0.19.0: the WASM memory inspector (page tools, with eval_js's limits: not on Krunk's own
    // pages, not while a password or card field holds a value; results capped and scrubbed).
    Tool {
        name: "wasm_memories",
        description: "The WebAssembly memories and instances of the page (games and apps compiled to WASM keep their state there): per context (the page, same-process frames, dedicated workers) each memory's number and size, and each instance's number, which memories it exports, its exported globals (export number, name, type, mutable, value) and its first function names. Frames of other sites, service workers and shared workers are not reachable. Like eval_js, the wasm_* tools are not available on Krunk's own pages or while a password or card field holds a value. Typical use: wasm_scan for a value the page shows, change it in the page, wasm_scan mode next until few addresses are left, then wasm_write or wasm_freeze.",
        schema: EMPTY,
        group: Group::Page,
    },
    Tool {
        name: "wasm_read",
        description: "Read a value from a WebAssembly memory (little-endian). type bytes, utf8 and utf16 read length bytes (default 16, at most 65536).",
        schema: r#"{"type":"object","properties":{
            "ctx":{"type":"integer","minimum":0,"description":"Context from wasm_memories (default 0: the page)"},
            "mem":{"type":"integer","minimum":0},
            "addr":{"type":"integer","minimum":0,"maximum":4294967295,"description":"Byte address in the memory"},
            "offsets":{"type":"array","maxItems":8,"items":{"type":"integer"},"description":"Pointer path: for each offset, the 32-bit pointer at the address is read and the offset added"},
            "type":{"type":"string","enum":["i8","u8","i16","u16","i32","u32","i64","u64","f32","f64","bytes","utf8","utf16"]},
            "length":{"type":"integer","minimum":0,"maximum":65536}},"required":["mem","addr","type"],"additionalProperties":false}"#,
        group: Group::Page,
    },
    Tool {
        name: "wasm_write",
        description: "Write a value into a WebAssembly memory. The page may overwrite it again: use wasm_freeze to hold a value.",
        schema: r#"{"type":"object","properties":{
            "ctx":{"type":"integer","minimum":0,"description":"Context from wasm_memories (default 0: the page)"},
            "mem":{"type":"integer","minimum":0},
            "addr":{"type":"integer","minimum":0,"maximum":4294967295,"description":"Byte address in the memory"},
            "offsets":{"type":"array","maxItems":8,"items":{"type":"integer"},"description":"Pointer path: for each offset, the 32-bit pointer at the address is read and the offset added"},
            "type":{"type":"string","enum":["i8","u8","i16","u16","i32","u32","i64","u64","f32","f64","bytes","utf8","utf16"]},
            "value":{"type":["number","string","array"],"items":{"type":"integer","minimum":0,"maximum":255},"description":"A number (i64/u64: a decimal string; utf8/utf16: the text; bytes: an array of 0..255, at most 65536)"}},"required":["mem","addr","type","value"],"additionalProperties":false}"#,
        group: Group::Page,
    },
    Tool {
        name: "wasm_scan",
        description: "Search a WebAssembly memory for a value, Cheat-Engine style. mode first (needs mem, type, kind exact | range | bigger | smaller | unknown) starts a scan and returns its number and the count of matching addresses; mode next (needs scan, kind exact | range | changed | unchanged | increased | decreased | increasedBy | decreasedBy) keeps the addresses whose value now fits. value (and to, for range) as the kind needs. At most the first 512 MiB are scanned and 4,194,304 addresses kept; 8 scans are kept per context. Read the addresses with wasm_results.",
        schema: r#"{"type":"object","properties":{
            "ctx":{"type":"integer","minimum":0},
            "mode":{"type":"string","enum":["first","next"]},
            "mem":{"type":"integer","minimum":0},
            "type":{"type":"string","enum":["i8","u8","i16","u16","i32","u32","i64","u64","f32","f64"]},
            "scan":{"type":"integer","minimum":0},
            "kind":{"type":"string","enum":["exact","range","bigger","smaller","unknown","changed","unchanged","increased","decreased","increasedBy","decreasedBy"]},
            "value":{"type":["number","string"],"description":"A number (i64/u64: a decimal string; utf8/utf16: the text)"},
            "to":{"type":["number","string"],"description":"Upper end of a range"}},"required":["mode","kind"],"additionalProperties":false}"#,
        group: Group::Page,
    },
    Tool {
        name: "wasm_results",
        description: "Addresses a scan found, with the value now and at the scan (at most 200 per call; offset pages).",
        schema: r#"{"type":"object","properties":{
            "ctx":{"type":"integer","minimum":0},
            "scan":{"type":"integer","minimum":0},
            "offset":{"type":"integer","minimum":0},
            "limit":{"type":"integer","minimum":1,"maximum":200}},"required":["scan"],"additionalProperties":false}"#,
        group: Group::Page,
    },
    Tool {
        name: "wasm_freeze",
        description: "Hold a value in a WebAssembly memory: it is written back about 20 times a second (slower in a background tab) until wasm_unfreeze, a navigation or the tab's end. Without value: the value it has now. At most 256 frozen values. Returns the freeze's number.",
        schema: r#"{"type":"object","properties":{
            "ctx":{"type":"integer","minimum":0,"description":"Context from wasm_memories (default 0: the page)"},
            "mem":{"type":"integer","minimum":0},
            "addr":{"type":"integer","minimum":0,"maximum":4294967295,"description":"Byte address in the memory"},
            "offsets":{"type":"array","maxItems":8,"items":{"type":"integer"},"description":"Pointer path: for each offset, the 32-bit pointer at the address is read and the offset added"},
            "type":{"type":"string","enum":["i8","u8","i16","u16","i32","u32","i64","u64","f32","f64"]},
            "value":{"type":["number","string"],"description":"A number (i64/u64: a decimal string; utf8/utf16: the text)"}},"required":["mem","addr","type"],"additionalProperties":false}"#,
        group: Group::Page,
    },
    Tool {
        name: "wasm_watch",
        description: "Read several places of a WebAssembly memory in one call (at most 32): each entry is an address or a pointer path with a type, like wasm_read. Gives, in the same order, {addr, value} or {error} per entry. Use it to look at the values you found again, also after the memory grew (addresses stay valid).",
        schema: r#"{"type":"object","properties":{
            "ctx":{"type":"integer","minimum":0,"description":"Context from wasm_memories (default 0: the page)"},
            "entries":{"type":"array","minItems":1,"maxItems":32,"items":{"type":"object","properties":{
                "mem":{"type":"integer","minimum":0},
                "addr":{"type":"integer","minimum":0,"maximum":4294967295},
                "offsets":{"type":"array","maxItems":8,"items":{"type":"integer"}},
                "type":{"type":"string","enum":["i8","u8","i16","u16","i32","u32","i64","u64","f32","f64","bytes","utf8","utf16"]},
                "length":{"type":"integer","minimum":0,"maximum":65536}},"required":["mem","addr","type"],"additionalProperties":false}}},"required":["entries"],"additionalProperties":false}"#,
        group: Group::Page,
    },
    Tool {
        name: "wasm_global",
        description: "The exported globals of a WebAssembly instance (inst from wasm_memories). Without value: their export number, name, type, mutable and value now. With export and value: write that global (mutable i32, i64, f32 and f64 globals only; i64 as a decimal string).",
        schema: r#"{"type":"object","properties":{
            "ctx":{"type":"integer","minimum":0,"description":"Context from wasm_memories (default 0: the page)"},
            "inst":{"type":"integer","minimum":0,"maximum":255},
            "export":{"type":"integer","minimum":0,"description":"The global's export number, from wasm_memories or this tool"},
            "value":{"type":["number","string"]}},"required":["inst"],"additionalProperties":false}"#,
        group: Group::Page,
    },
    Tool {
        name: "wasm_unfreeze",
        description: "Stop holding a frozen value.",
        schema: r#"{"type":"object","properties":{
            "ctx":{"type":"integer","minimum":0},
            "freeze":{"type":"integer","minimum":0}},"required":["freeze"],"additionalProperties":false}"#,
        group: Group::Page,
    },
    Tool {
        name: "finish",
        description: "End the task with your final answer to the user (Markdown).",
        schema: r#"{"type":"object","properties":{"answer":{"type":"string","maxLength":20000}},"required":["answer"],"additionalProperties":false}"#,
        group: Group::Finish,
    },
];

/// Look a tool up by name.
pub fn find(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

/// What decides which tools a task gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Offer {
    pub userscripts: bool,
    pub macros: bool,
    /// Sensitive categories the user's message asked for (see [`super::gate`]).
    pub gated: CategorySet,
}

/// The tools offered for a task.
pub fn offered(o: Offer) -> Vec<&'static Tool> {
    TOOLS
        .iter()
        .filter(|t| match t.group {
            Group::Userscripts => o.userscripts,
            Group::Macros => o.macros,
            Group::Sensitive(c) => o.gated.contains(c),
            _ => true,
        })
        .collect()
}

pub fn specs(o: Offer) -> Vec<ToolSpec> {
    offered(o).into_iter().map(Tool::spec).collect()
}

/// Is `name` offered under `o`?
pub fn is_offered(name: &str, o: Offer) -> bool {
    offered(o).iter().any(|t| t.name == name)
}

// ---- argument validation (the subset of JSON schema the table uses) ----

/// Checks `value` against the tool's schema. The error names the field, never its value.
pub fn validate(tool: &Tool, value: &Value) -> Result<(), String> {
    check(&tool.parameters(), value, "arguments")
}

fn check(schema: &Value, v: &Value, at: &str) -> Result<(), String> {
    let Some(s) = schema.as_object() else { return Ok(()) };
    if let Some(t) = s.get("type").and_then(Value::as_str) {
        let ok = match t {
            "object" => v.is_object(),
            "array" => v.is_array(),
            "string" => v.is_string(),
            "integer" => v.as_i64().is_some() || v.as_u64().is_some(),
            "number" => v.as_f64().is_some_and(f64::is_finite),
            "boolean" => v.is_boolean(),
            _ => true,
        };
        if !ok {
            return Err(format!("{at} must be {}", if t == "object" { "an object" } else { t }));
        }
    }
    if let Some(e) = s.get("enum").and_then(Value::as_array) {
        if !e.contains(v) {
            return Err(format!("{at} must be one of {}", Value::Array(e.clone())));
        }
    }
    if let Some(n) = v.as_f64() {
        if let Some(min) = s.get("minimum").and_then(Value::as_f64) {
            if n < min {
                return Err(format!("{at} must be at least {min}"));
            }
        }
        if let Some(max) = s.get("maximum").and_then(Value::as_f64) {
            if n > max {
                return Err(format!("{at} must be at most {max}"));
            }
        }
    }
    if let Some(text) = v.as_str() {
        let len = text.chars().count() as u64;
        if s.get("minLength").and_then(Value::as_u64).is_some_and(|m| len < m) {
            return Err(format!("{at} is too short"));
        }
        if s.get("maxLength").and_then(Value::as_u64).is_some_and(|m| len > m) {
            return Err(format!("{at} is too long"));
        }
    }
    if let Some(items) = v.as_array() {
        let n = items.len() as u64;
        if s.get("minItems").and_then(Value::as_u64).is_some_and(|m| n < m) {
            return Err(format!("{at} needs at least {} items", s["minItems"]));
        }
        if s.get("maxItems").and_then(Value::as_u64).is_some_and(|m| n > m) {
            return Err(format!("{at} has more than {} items", s["maxItems"]));
        }
        if let Some(item) = s.get("items") {
            for (i, x) in items.iter().enumerate() {
                check(item, x, &format!("{at}[{i}]"))?;
            }
        }
    }
    if let Some(obj) = v.as_object() {
        let props = s.get("properties").and_then(Value::as_object);
        if let Some(req) = s.get("required").and_then(Value::as_array) {
            for r in req.iter().filter_map(Value::as_str) {
                if !obj.contains_key(r) {
                    return Err(format!("{at}.{r} is missing"));
                }
            }
        }
        let n = obj.len() as u64;
        if s.get("minProperties").and_then(Value::as_u64).is_some_and(|m| n < m) {
            return Err(format!("{at} needs at least {} entries", s["minProperties"]));
        }
        if s.get("maxProperties").and_then(Value::as_u64).is_some_and(|m| n > m) {
            return Err(format!("{at} has more than {} entries", s["maxProperties"]));
        }
        for (k, x) in obj {
            let name = if k.len() > 40 || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                "an entry".to_string()
            } else {
                k.clone()
            };
            match props.and_then(|p| p.get(k)) {
                Some(ps) => check(ps, x, &format!("{at}.{name}"))?,
                None => match s.get("additionalProperties") {
                    Some(Value::Bool(false)) => return Err(format!("{at}.{name} is not a known argument")),
                    Some(extra @ Value::Object(_)) => check(extra, x, &format!("{at}.{name}"))?,
                    _ => {}
                },
            }
        }
    }
    Ok(())
}

/// Parses a call's raw JSON arguments (empty text = `{}`) and validates them.
pub fn parse_call(name: &str, arguments: &str) -> Result<(&'static Tool, Value), String> {
    let tool = find(name).ok_or_else(|| "there is no such tool".to_string())?;
    let text = arguments.trim();
    let v: Value = if text.is_empty() {
        Value::Object(Default::default())
    } else {
        serde_json::from_str(text).map_err(|_| "the arguments are not valid JSON".to_string())?
    };
    validate(tool, &v)?;
    Ok((tool, v))
}

/// Typed arguments, parsed from validated JSON with `serde_json::from_value`.
pub mod args {
    use super::*;
    use crate::agent::scratch::ItemState;
    use std::collections::BTreeMap;

    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct GetHtml {
        pub selector: Option<String>,
        #[serde(default)]
        pub offset: usize,
        pub max_chars: Option<usize>,
        #[serde(default)]
        pub include_scripts: bool,
    }
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct GetText {
        pub selector: Option<String>,
        #[serde(default)]
        pub offset: usize,
        pub max_chars: Option<usize>,
    }
    #[derive(Debug, Clone, Deserialize)]
    pub struct Query {
        pub selector: String,
        pub limit: Option<u32>,
        #[serde(default)]
        pub offset: u32,
    }
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct ListLinks {
        pub filter: Option<String>,
        pub limit: Option<u32>,
        #[serde(default)]
        pub offset: u32,
    }
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct SaveScreenshot {
        #[serde(default)]
        pub full_page: bool,
        pub file_name: Option<String>,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum Button {
        #[default]
        Left,
        Middle,
        Right,
    }
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct Click {
        pub selector: Option<String>,
        pub x: Option<f64>,
        pub y: Option<f64>,
        #[serde(default)]
        pub button: Button,
        pub count: Option<u8>,
    }
    #[derive(Debug, Clone, Deserialize)]
    pub struct TypeText {
        pub text: String,
        pub selector: Option<String>,
        #[serde(default)]
        pub submit: bool,
    }
    #[derive(Debug, Clone, Deserialize)]
    pub struct FillFields {
        pub fields: BTreeMap<String, String>,
    }
    #[derive(Debug, Clone, Deserialize)]
    pub struct SelectOption {
        pub selector: String,
        pub values: Vec<String>,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum KeyAction {
        #[default]
        Press,
        Down,
        Up,
    }
    #[derive(Debug, Clone, Deserialize)]
    pub struct PressKeys {
        pub keys: String,
        #[serde(default)]
        pub action: KeyAction,
        pub hold_ms: Option<u64>,
        pub repeat: Option<bool>,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum ScrollTo {
        Top,
        Bottom,
    }
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct Scroll {
        pub dx: Option<f64>,
        pub dy: Option<f64>,
        pub to: Option<ScrollTo>,
        pub selector: Option<String>,
    }
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct WaitFor {
        pub selector: Option<String>,
        #[serde(default)]
        pub navigation: bool,
        pub ms: Option<u64>,
        pub timeout_ms: Option<u64>,
    }
    #[derive(Debug, Clone, Deserialize)]
    pub struct EvalJs {
        pub code: String,
        pub await_promise: Option<bool>,
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum EditAction {
        SetText,
        SetAttribute,
        RemoveAttribute,
        SetStyle,
        Remove,
        InsertCss,
    }
    #[derive(Debug, Clone, Deserialize)]
    pub struct EditPage {
        pub action: EditAction,
        pub selector: Option<String>,
        pub name: Option<String>,
        pub value: Option<String>,
        pub css: Option<String>,
        #[serde(default)]
        pub all: bool,
    }
    #[derive(Debug, Clone, Deserialize)]
    pub struct SetTitle {
        pub title: String,
    }
    #[derive(Debug, Clone, Deserialize)]
    pub struct Navigate {
        pub url: String,
    }
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct OpenTab {
        pub url: Option<String>,
        pub query: Option<String>,
        #[serde(default)]
        pub background: bool,
    }
    #[derive(Debug, Clone, Deserialize)]
    pub struct SwitchTab {
        pub tab_id: u32,
    }
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct CloseTab {
        pub tab_id: Option<u32>,
    }
    #[derive(Debug, Clone, Deserialize)]
    pub struct SetSetting {
        pub key: String,
        pub value: Value,
    }
    /// 0.15.0
    #[derive(Debug, Clone, Deserialize)]
    pub struct SetBackgroundUrl {
        pub url: String,
    }
    /// `draft` and `more` (0.15.0) are taken out by the session before the host sees the call.
    #[derive(Debug, Clone, Deserialize)]
    pub struct SaveUserscript {
        pub id: Option<String>,
        pub source: String,
    }
    #[derive(Debug, Clone, Deserialize)]
    pub struct Toggle {
        pub id: String,
        pub enabled: bool,
    }
    #[derive(Debug, Clone, Deserialize)]
    pub struct SaveMacro {
        pub id: Option<String>,
        pub name: String,
        pub source: String,
        pub hotkey: Option<String>,
    }
    #[derive(Debug, Clone, Deserialize)]
    pub struct Id {
        pub id: String,
    }
    #[derive(Debug, Clone, Deserialize)]
    pub struct AskUser {
        pub question: String,
        pub options: Vec<String>,
        #[serde(default)]
        pub multi: bool,
        #[serde(default)]
        pub allow_other: bool,
    }
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct GetHistory {
        pub query: Option<String>,
        pub days: Option<u32>,
        pub limit: Option<u32>,
    }
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct GetBookmarks {
        pub query: Option<String>,
        pub limit: Option<u32>,
    }
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct GetDownloads {
        pub limit: Option<u32>,
    }
    #[derive(Debug, Clone, Deserialize)]
    pub struct GetCookies {
        pub site: String,
    }
    #[derive(Debug, Clone, Deserialize)]
    pub struct Finish {
        pub answer: String,
    }
    // 0.16.0: the scratchpad.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum NoteMode {
        #[default]
        Replace,
        Append,
    }
    #[derive(Debug, Clone, Deserialize)]
    pub struct NoteWrite {
        pub name: String,
        pub content: String,
        #[serde(default)]
        pub mode: NoteMode,
    }
    #[derive(Debug, Clone, Deserialize)]
    pub struct NoteRead {
        pub name: String,
        #[serde(default)]
        pub offset: usize,
        pub max_chars: Option<usize>,
    }
    #[derive(Debug, Clone, Deserialize)]
    pub struct NoteName {
        pub name: String,
    }
    #[derive(Debug, Clone, Deserialize)]
    pub struct PlanSetItem {
        pub text: String,
        pub state: Option<ItemState>,
        pub id: Option<String>,
    }
    #[derive(Debug, Clone, Deserialize)]
    pub struct PlanSet {
        pub items: Vec<PlanSetItem>,
    }
    /// 0.16.0. `origin` is not the model's: the session adds the site the user allowed, and
    /// the host enters the password only while the tab is still on it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum PasswordMode {
        Generate,
        Provided,
    }
    #[derive(Clone, Deserialize)]
    pub struct FillPassword {
        pub mode: PasswordMode,
        pub password: Option<zeroize::Zeroizing<String>>,
        pub selector: Option<String>,
        #[serde(default)]
        pub origin: String,
    }
    #[derive(Debug, Clone, Default, Deserialize)]
    pub struct PlanUpdate {
        pub index: Option<usize>,
        pub id: Option<String>,
        pub state: Option<ItemState>,
        pub text: Option<String>,
    }

    /// Typed parse of validated arguments.
    pub fn parse<T: for<'de> Deserialize<'de>>(v: &Value) -> Result<T, String> {
        serde_json::from_value(v.clone()).map_err(|_| "the arguments don't fit this tool".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn all() -> Offer {
        Offer { userscripts: true, macros: true, gated: Category::ALL.into_iter().collect() }
    }

    #[test]
    fn every_schema_is_a_closed_object() {
        let mut names = std::collections::HashSet::new();
        for t in TOOLS {
            let p: Value = serde_json::from_str(t.schema).unwrap_or_else(|e| panic!("{}: {e}", t.name));
            assert_eq!(p["type"], "object", "{}", t.name);
            assert_eq!(p["additionalProperties"], false, "{}", t.name);
            assert!(t.name.len() <= 64 && t.name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'));
            assert!(names.insert(t.name), "duplicate {}", t.name);
            assert!(!t.description.is_empty());
        }
        // The brief's tool list (C.8), nothing missing.
        for n in [
            "get_page_context",
            "get_html",
            "get_text",
            "query",
            "list_links",
            "list_forms",
            "list_media",
            "screenshot",
            "save_screenshot",
            "click",
            "type_text",
            "fill_fields",
            "select_option",
            "press_keys",
            "scroll",
            "wait_for",
            "eval_js",
            "edit_page",
            "set_title",
            "navigate",
            "back",
            "forward",
            "reload",
            "open_tab",
            "switch_tab",
            "close_tab",
            "list_tabs",
            "get_settings",
            "set_setting",
            "set_new_tab_background_url",
            "list_userscripts",
            "save_userscript",
            "toggle_userscript",
            "list_macros",
            "save_macro",
            "run_macro",
            "ask_user",
            "get_history",
            "get_bookmarks",
            "get_downloads",
            "get_cookies_for_site",
            "finish",
            // 0.16.0: the scratchpad.
            "note_write",
            "note_read",
            "note_delete",
            "note_list",
            "plan_set",
            "plan_update",
            // 0.16.0: passwords on request.
            "fill_password",
            // 0.19.0: the WASM memory inspector.
            "wasm_memories",
            "wasm_read",
            "wasm_write",
            "wasm_scan",
            "wasm_results",
            "wasm_freeze",
            "wasm_unfreeze",
            "wasm_watch",
            "wasm_global",
        ] {
            assert!(find(n).is_some(), "{n} is missing");
        }
        assert_eq!(TOOLS.len(), 58);
    }

    /// No tool reaches the vault, cards, sync, the account or API keys (brief C.10). 0.16.0:
    /// `fill_password` is the one tool about passwords: it only writes (its arguments are a
    /// mode, a password to enter and a selector), and it is gated.
    #[test]
    fn no_tool_reaches_secrets() {
        let fp = find("fill_password").unwrap();
        assert_eq!(fp.group, Group::Sensitive(Category::Passwords));
        let props: Vec<String> = fp.parameters()["properties"].as_object().unwrap().keys().cloned().collect();
        assert_eq!(props, ["mode", "password", "selector"]);
        for t in TOOLS.iter().filter(|t| t.name != "fill_password") {
            let text = format!("{} {}", t.name, t.schema).to_lowercase();
            for bad in
                ["vault", "passw", "card", "payment", "sync", "account", "token", "api_key", "apikey", "recovery"]
            {
                assert!(!text.contains(bad), "{} mentions {bad}", t.name);
            }
        }
    }

    #[test]
    fn offer_follows_switches_and_gate() {
        let none = Offer { userscripts: false, macros: false, gated: CategorySet::EMPTY };
        let names: Vec<_> = offered(none).iter().map(|t| t.name).collect();
        assert!(names.contains(&"click") && names.contains(&"finish") && names.contains(&"ask_user"));
        // 0.16.0: the scratchpad is always offered.
        let scratch: Vec<_> = TOOLS.iter().filter(|t| t.group == Group::Scratch).map(|t| t.name).collect();
        assert_eq!(scratch, ["note_write", "note_read", "note_delete", "note_list", "plan_set", "plan_update"]);
        assert!(scratch.iter().all(|n| names.contains(n)));
        for hidden in ["save_userscript", "run_macro", "get_history", "get_cookies_for_site", "fill_password"] {
            assert!(!names.contains(&hidden), "{hidden}");
        }
        // 0.16.0: entering a password is offered through the gate only, like the sensitive data.
        let asked = Offer { gated: super::super::gate::requested("sign up with a strong password"), ..none };
        assert!(is_offered("fill_password", asked) && !is_offered("get_history", asked));
        assert!(!is_offered(
            "fill_password",
            Offer { gated: super::super::gate::requested("fill in the form"), ..none }
        ));
        let mut gated = CategorySet::EMPTY;
        gated.insert(Category::History);
        let o = Offer { gated, ..none };
        assert!(is_offered("get_history", o) && !is_offered("get_bookmarks", o));
        assert_eq!(specs(all()).len(), TOOLS.len());
    }

    #[test]
    fn validation() {
        let ok = |n: &str, v: Value| validate(find(n).unwrap(), &v);
        assert!(ok("click", json!({"selector":"#a"})).is_ok());
        assert!(ok("click", json!({"x":1.5,"y":2})).is_ok());
        assert!(ok("click", json!({"button":"side"})).is_err());
        assert!(ok("click", json!({"count":3})).is_err());
        assert!(ok("query", json!({})).unwrap_err().contains("selector is missing"));
        assert!(ok("query", json!({"selector":"a","bogus":1})).unwrap_err().contains("bogus"));
        assert!(ok("ask_user", json!({"question":"Which?","options":["a"]})).is_err());
        assert!(ok("ask_user", json!({"question":"Which?","options":["a","b"],"multi":true})).is_ok());
        let nine: Vec<String> = (0..9).map(|i| i.to_string()).collect();
        assert!(ok("ask_user", json!({"question":"Q","options":nine})).is_err());
        assert!(ok("fill_fields", json!({"fields":{"#name":"Ann","#mail":"a@b.c"}})).is_ok());
        assert!(ok("fill_fields", json!({"fields":{}})).is_err());
        assert!(ok("fill_fields", json!({"fields":{"#n":5}})).is_err());
        assert!(ok("get_html", json!({"max_chars":20001})).is_err());
        assert!(ok("set_setting", json!({"key":"theme","value":"dark"})).is_ok());
        assert!(ok("wait_for", json!({"ms":30001})).is_err());
        // 0.15.0: parts of long scripts.
        assert!(ok("save_userscript", json!({"source":"a","more":true})).is_ok());
        assert!(ok("save_userscript", json!({"source":"a","draft":"draft-1"})).is_ok());
        assert!(ok("save_userscript", json!({"source":"a","more":"yes"})).is_err());
        assert!(ok("save_macro", json!({"name":"m","source":"a","draft":"draft-1","more":false})).is_ok());
        assert!(ok("set_new_tab_background_url", json!({"url":"https://example.com/a.png"})).is_ok());
        assert!(ok("set_new_tab_background_url", json!({})).is_err());
        // 0.16.0, the scratchpad: the schema bounds are loose (the exact limits and their errors
        // are `scratch`'s), the shapes are checked.
        assert!(ok("note_write", json!({"name":"n","content":"x".repeat(40_000)})).is_ok());
        assert!(ok("note_write", json!({"name":"n","content":"x","mode":"prepend"})).is_err());
        assert!(ok("note_write", json!({"name":"n"})).unwrap_err().contains("content is missing"));
        assert!(ok("note_read", json!({"name":"n","offset":10,"max_chars":20000})).is_ok());
        assert!(ok("note_read", json!({"name":"n","max_chars":20001})).is_err());
        assert!(ok("note_delete", json!({})).is_err());
        assert!(ok("note_list", json!({})).is_ok());
        assert!(ok("plan_set", json!({"items":[{"text":"a"},{"text":"b","state":"doing","id":"x"}]})).is_ok());
        assert!(ok("plan_set", json!({"items":[]})).is_ok());
        assert!(ok("plan_set", json!({"items":[{"state":"todo"}]})).unwrap_err().contains("items[0].text is missing"));
        assert!(ok("plan_set", json!({"items":[{"text":"a","state":"started"}]})).is_err());
        assert!(ok("plan_set", json!({"items":[{"text":"a","note":"b"}]})).is_err());
        assert!(ok("plan_update", json!({"index":2,"state":"done"})).is_ok());
        assert!(ok("plan_update", json!({"id":"x","text":"new"})).is_ok());
        assert!(ok("plan_update", json!({"index":0,"state":"done"})).is_err());
        // 0.16.0: fill_password. `origin` is the session's to add, not the model's.
        assert!(ok("fill_password", json!({"mode":"generate"})).is_ok());
        assert!(ok("fill_password", json!({"mode":"provided","password":"x","selector":"#pw"})).is_ok());
        assert!(ok("fill_password", json!({})).unwrap_err().contains("mode is missing"));
        assert!(ok("fill_password", json!({"mode":"read"})).is_err());
        assert!(ok("fill_password", json!({"mode":"provided","password":""})).is_err());
        assert!(ok("fill_password", json!({"mode":"generate","origin":"https://a.example"})).is_err());
        let e = ok("fill_password", json!({"mode":"provided","password":"SECRET".repeat(200)})).unwrap_err();
        assert!(e.contains("password is too long") && !e.contains("SECRET"), "{e}");
        // An odd key name is not echoed.
        let e = ok("query", json!({"selector":"a","<script>secret":1})).unwrap_err();
        assert!(!e.contains("secret"), "{e}");
    }

    #[test]
    fn parse_call_and_typed_args() {
        let (t, v) = parse_call("press_keys", r#"{"keys":"a","action":"down","repeat":true}"#).unwrap();
        assert_eq!(t.name, "press_keys");
        let p: args::PressKeys = args::parse(&v).unwrap();
        assert_eq!(p.action, args::KeyAction::Down);
        let (_, v) = parse_call("list_tabs", "").unwrap();
        assert_eq!(v, json!({}));
        assert!(parse_call("nope", "{}").is_err());
        assert!(parse_call("click", "{not json").is_err());
        let (_, v) = parse_call("edit_page", r##"{"action":"insert_css","css":"#b{display:none}"}"##).unwrap();
        let e: args::EditPage = args::parse(&v).unwrap();
        assert_eq!(e.action, args::EditAction::InsertCss);
        let (_, v) = parse_call("click", r#"{"x":10,"y":20,"button":"right"}"#).unwrap();
        let c: args::Click = args::parse(&v).unwrap();
        assert_eq!((c.x, c.button), (Some(10.0), args::Button::Right));
        let (t, v) = parse_call("note_write", r#"{"name":"n","content":"x","mode":"append"}"#).unwrap();
        assert_eq!(t.group, Group::Scratch);
        let w: args::NoteWrite = args::parse(&v).unwrap();
        assert_eq!((w.name.as_str(), w.mode), ("n", args::NoteMode::Append));
        let mut v = parse_call("fill_password", r#"{"mode":"provided","password":"pw"}"#).unwrap().1;
        v["origin"] = json!("https://a.example");
        let f: args::FillPassword = args::parse(&v).unwrap();
        assert_eq!((f.mode, f.password.as_deref().map(String::as_str)), (args::PasswordMode::Provided, Some("pw")));
        assert_eq!((f.origin.as_str(), f.selector), ("https://a.example", None));
        let (_, v) = parse_call("plan_set", r#"{"items":[{"text":"a","state":"dropped"}]}"#).unwrap();
        let p: args::PlanSet = args::parse(&v).unwrap();
        assert_eq!(p.items[0].state, Some(crate::agent::scratch::ItemState::Dropped));
    }
}
