//! The browser settings the agent may read and change (WP-B1).
//!
//! An explicit allow-list of camelCase key paths. Everything about the vault, payment cards,
//! sync, the account, the AI providers and keys, and security-relevant network settings
//! (proxy, DNS, extra flags, user agent, headers, updates, the download folder, custom search
//! or mail templates) is not on it and cannot be read or set.
//!
//! A change goes through a JSON round trip of the whole [`Settings`] (so the stored form and its
//! defaults apply), then [`Settings::validate`]; the restart flag is
//! [`Settings::restart_required`] against the settings before the change.

use crate::protection::Level;
use crate::settings::{
    is_hex_color, BookmarksBar, Density, DevToolsDock, LinkTabs, NewTabPage, NewTabPosition, NtpBackground,
    SearchEngine, Settings, Startup, Theme, TileSource,
};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

/// What a key holds, for checks and for the description `get_all` gives the model.
#[derive(Clone, Copy)]
enum Kind {
    Bool,
    Int(i64, i64),
    Num(f64, f64),
    /// `#rrggbb` or empty.
    Color,
    /// An http(s) address (or a krunk:// page the agent may open), or empty.
    Url,
    /// One of these values (serde names of an enum).
    Choice(fn() -> Vec<Value>),
}

struct Key {
    path: &'static str,
    kind: Kind,
    note: &'static str,
}

fn names<T: Serialize>(all: &[T]) -> Vec<Value> {
    all.iter().filter_map(|v| serde_json::to_value(v).ok()).collect()
}

fn themes() -> Vec<Value> {
    names(&[Theme::System, Theme::Light, Theme::Dark])
}
fn densities() -> Vec<Value> {
    names(&[Density::Normal, Density::Compact])
}
fn startups() -> Vec<Value> {
    names(&[Startup::NewTab, Startup::RestoreSession, Startup::Homepage])
}
fn new_tab_pages() -> Vec<Value> {
    names(&[NewTabPage::Krunk, NewTabPage::Homepage, NewTabPage::Blank, NewTabPage::Custom])
}
fn new_tab_positions() -> Vec<Value> {
    names(&[NewTabPosition::End, NewTabPosition::AfterCurrent])
}
fn link_tabs() -> Vec<Value> {
    names(&[LinkTabs::Auto, LinkTabs::Switch, LinkTabs::Background])
}
fn bookmarks_bars() -> Vec<Value> {
    names(&[BookmarksBar::Always, BookmarksBar::NewTab, BookmarksBar::Never])
}
/// The preset engines; `custom` needs a search address, which the agent may not set.
fn search_engines() -> Vec<Value> {
    names(&[
        SearchEngine::DuckDuckGo,
        SearchEngine::Google,
        SearchEngine::Bing,
        SearchEngine::Brave,
        SearchEngine::Startpage,
        SearchEngine::Ecosia,
    ])
}
fn protection_levels() -> Vec<Value> {
    names(&[Level::Standard, Level::Strict, Level::Custom])
}
fn ntp_backgrounds() -> Vec<Value> {
    names(&[NtpBackground::None, NtpBackground::Color, NtpBackground::Image, NtpBackground::Wallpaper])
}
fn tile_sources() -> Vec<Value> {
    names(&[TileSource::MostVisited, TileSource::Custom])
}
fn wallpapers() -> Vec<Value> {
    let mut v = vec![Value::String(String::new())];
    v.extend(crate::assets::wallpapers().into_iter().map(|w| Value::String(w.id)));
    v
}
fn docks() -> Vec<Value> {
    names(&[DevToolsDock::Right, DevToolsDock::Bottom])
}

static KEYS: &[Key] = &[
    Key { path: "theme", kind: Kind::Choice(themes), note: "" },
    Key { path: "appearance.accent", kind: Kind::Color, note: "empty = the theme's colour" },
    Key { path: "appearance.toolbarColor", kind: Kind::Color, note: "empty = the theme's colour" },
    Key { path: "appearance.stripColor", kind: Kind::Color, note: "tab strip; empty = the theme's colour" },
    Key { path: "appearance.backgroundColor", kind: Kind::Color, note: "empty = the theme's colour" },
    Key { path: "appearance.density", kind: Kind::Choice(densities), note: "tab strip density" },
    Key { path: "appearance.uiFontPx", kind: Kind::Int(12, 16), note: "font size of Krunk's own interface" },
    Key { path: "appearance.roundedTabs", kind: Kind::Bool, note: "" },
    Key { path: "appearance.showDownloadsButton", kind: Kind::Bool, note: "" },
    Key { path: "appearance.showZoomChip", kind: Kind::Bool, note: "" },
    Key { path: "appearance.showExtensionsButton", kind: Kind::Bool, note: "" },
    Key { path: "defaultZoom", kind: Kind::Num(0.25, 5.0), note: "1 = 100 %" },
    Key { path: "startup", kind: Kind::Choice(startups), note: "what Krunk opens when it starts" },
    Key { path: "homepage", kind: Kind::Url, note: "an http or https address" },
    Key { path: "newTabPage", kind: Kind::Choice(new_tab_pages), note: "custom opens newTabUrl" },
    Key { path: "newTabUrl", kind: Kind::Url, note: "an http(s) address or a krunk:// page" },
    Key { path: "newTabPosition", kind: Kind::Choice(new_tab_positions), note: "" },
    Key { path: "linkTabs", kind: Kind::Choice(link_tabs), note: "whether a link opened in a new tab becomes active" },
    Key { path: "bookmarksBar", kind: Kind::Choice(bookmarks_bars), note: "" },
    Key { path: "searchEngine", kind: Kind::Choice(search_engines), note: "" },
    Key { path: "tabSleepMinutes", kind: Kind::Int(0, 1440), note: "0 = never" },
    Key { path: "tabDiscardEnabled", kind: Kind::Bool, note: "unload background tabs" },
    Key { path: "tabDiscardMinutes", kind: Kind::Int(1, 1440), note: "must be longer than tabSleepMinutes" },
    Key { path: "autoPip", kind: Kind::Bool, note: "picture-in-picture when leaving a tab that plays video" },
    Key { path: "adblockEnabled", kind: Kind::Bool, note: "" },
    Key { path: "sendDnt", kind: Kind::Bool, note: "Do Not Track header" },
    Key { path: "sendGpc", kind: Kind::Bool, note: "Global Privacy Control" },
    Key { path: "protection.level", kind: Kind::Choice(protection_levels), note: "tracking protection" },
    Key { path: "pdfInBrowser", kind: Kind::Bool, note: "" },
    Key { path: "downloadConnections", kind: Kind::Int(1, 16), note: "connections per large download" },
    Key { path: "ntp.background", kind: Kind::Choice(ntp_backgrounds), note: "new tab page background" },
    Key { path: "ntp.backgroundColor", kind: Kind::Color, note: "" },
    Key { path: "ntp.imageDim", kind: Kind::Int(0, 80), note: "percent" },
    Key { path: "ntp.imageBlur", kind: Kind::Int(0, 20), note: "px" },
    Key { path: "ntp.wallpaper", kind: Kind::Choice(wallpapers), note: "used when ntp.background is wallpaper" },
    Key { path: "ntp.tiles", kind: Kind::Choice(tile_sources), note: "" },
    Key { path: "ntp.columns", kind: Kind::Int(1, 12), note: "" },
    Key { path: "ntp.rows", kind: Kind::Int(1, 6), note: "" },
    Key { path: "ntp.showShortcuts", kind: Kind::Bool, note: "" },
    Key { path: "ntp.showSearch", kind: Kind::Bool, note: "" },
    Key { path: "ntp.showTiles", kind: Kind::Bool, note: "" },
    Key { path: "devtoolsDock", kind: Kind::Choice(docks), note: "" },
    Key { path: "userscripts.enabled", kind: Kind::Bool, note: "" },
    Key { path: "macros.enabled", kind: Kind::Bool, note: "" },
    Key { path: "hardwareAcceleration", kind: Kind::Bool, note: "needs a restart" },
];

fn key(path: &str) -> Option<&'static Key> {
    KEYS.iter().find(|k| k.path == path)
}

/// The allowed key paths (`theme`, `appearance.showZoomChip`, …).
pub fn allowed_keys() -> &'static [&'static str] {
    static NAMES: OnceLock<Vec<&'static str>> = OnceLock::new();
    NAMES.get_or_init(|| KEYS.iter().map(|k| k.path).collect())
}

/// The value at a dotted path of a JSON object.
fn lookup<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(v, |cur, part| cur.get(part))
}

fn lookup_mut<'a>(v: &'a mut Value, path: &str) -> Option<&'a mut Value> {
    path.split('.').try_fold(v, |cur, part| cur.get_mut(part))
}

/// The allowed settings with their values and, for choices, the allowed values:
/// `{"theme": {"value": "system", "values": ["system","light","dark"]}, …}`. Numbers carry
/// `type`, `min` and `max`; every entry may carry a `note`.
pub fn get_all(s: &Settings) -> Value {
    let all = serde_json::to_value(s).unwrap_or(Value::Null);
    let mut out = Map::new();
    for k in KEYS {
        let mut e = Map::new();
        e.insert("value".into(), lookup(&all, k.path).cloned().unwrap_or(Value::Null));
        match k.kind {
            Kind::Bool => {
                e.insert("type".into(), json!("boolean"));
            }
            Kind::Int(min, max) => {
                e.insert("type".into(), json!("integer"));
                e.insert("min".into(), json!(min));
                e.insert("max".into(), json!(max));
            }
            Kind::Num(min, max) => {
                e.insert("type".into(), json!("number"));
                e.insert("min".into(), json!(min));
                e.insert("max".into(), json!(max));
            }
            Kind::Color => {
                e.insert("type".into(), json!("colour #rrggbb or empty"));
            }
            Kind::Url => {
                e.insert("type".into(), json!("address"));
            }
            Kind::Choice(values) => {
                e.insert("values".into(), Value::Array(values()));
            }
        }
        if !k.note.is_empty() {
            e.insert("note".into(), json!(k.note));
        }
        out.insert(k.path.into(), Value::Object(e));
    }
    Value::Object(out)
}

/// The result of a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    /// The change takes effect after a restart.
    pub restart: bool,
}

/// Checks `value` against the key's kind (the value itself is never echoed).
fn check(k: &Key, value: &Value) -> Result<Value, String> {
    match k.kind {
        Kind::Bool => value.as_bool().map(Value::Bool).ok_or_else(|| "the value must be true or false".to_string()),
        Kind::Int(min, max) => {
            let n = value
                .as_i64()
                .or_else(|| value.as_f64().filter(|f| f.fract() == 0.0 && f.abs() < 1e15).map(|f| f as i64))
                .ok_or_else(|| "the value must be a whole number".to_string())?;
            if (min..=max).contains(&n) {
                Ok(json!(n))
            } else {
                Err(format!("the value must be between {min} and {max}"))
            }
        }
        Kind::Num(min, max) => match value.as_f64() {
            Some(n) if n.is_finite() && (min..=max).contains(&n) => Ok(json!(n)),
            _ => Err(format!("the value must be a number between {min} and {max}")),
        },
        Kind::Color => match value.as_str() {
            Some(c) if is_hex_color(c) => Ok(json!(c)),
            _ => Err("the value must be a colour like #2563eb, or empty".into()),
        },
        Kind::Url => {
            let t = value.as_str().ok_or_else(|| "the value must be an address".to_string())?.trim();
            if t.is_empty() {
                return Ok(json!(""));
            }
            if t.len() > 2048 || t.chars().any(char::is_control) {
                return Err("the address is not valid".into());
            }
            super::pages::may_navigate(t).map_err(|e| e.to_string())?;
            Ok(json!(t))
        }
        Kind::Choice(values) => {
            let all = values();
            if all.contains(value) {
                Ok(value.clone())
            } else {
                Err(format!("the value must be one of {}", Value::Array(all)))
            }
        }
    }
}

/// Changes one allowed setting; validates the whole result with [`Settings::validate`].
pub fn set(s: &mut Settings, key_path: &str, value: Value) -> Result<Changed, String> {
    let k = key(key_path).ok_or_else(|| "this setting can't be read or changed by the agent".to_string())?;
    let value = check(k, &value)?;
    let mut all = serde_json::to_value(&*s).map_err(|_| "the settings could not be read".to_string())?;
    let slot = lookup_mut(&mut all, k.path).ok_or_else(|| "this setting can't be changed".to_string())?;
    *slot = value;
    let mut new: Settings =
        serde_json::from_value(all).map_err(|_| "the value doesn't fit this setting".to_string())?;
    if k.path.starts_with("protection.") {
        new.sync_protection();
    }
    if let Err(errs) = new.validate() {
        let msg = errs.iter().map(|e| e.message.as_str()).collect::<Vec<_>>().join("; ");
        return Err(format!("the value is not valid: {msg}"));
    }
    let restart = new.restart_required(s);
    *s = new;
    Ok(Changed { restart })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every leaf path of the settings JSON (arrays count as leaves).
    fn leaf_paths(v: &Value, prefix: &str, out: &mut Vec<String>) {
        match v {
            Value::Object(m) => {
                for (k, x) in m {
                    let p = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                    leaf_paths(x, &p, out);
                }
            }
            _ => out.push(prefix.to_string()),
        }
    }

    const EXCLUDED: &[&str] = &[
        "vault",
        "password",
        "card",
        "payment",
        "passgen",
        "offersave",
        "sync",
        "account",
        "proxy",
        // 0.14.0: the VPN (whether Krunk's traffic goes through it is the user's choice alone).
        "vpn",
        "dns",
        "extraflags",
        "useragent",
        "headers",
        "checkupdates",
        "downloaddir",
        "askwheretosave",
        "customsearchurl",
        "ntpsearchurl",
        "mailhandler",
        "mailcustomurl",
        "permissions",
        "clearonexit",
        "protectedcontent",
        "enginesecuritycomponents",
        "httpsonly",
        "filterlists",
        "customlists",
        "devtoolssize",
    ];

    /// Lower-case words of a camelCase dotted path (`tabDiscardEnabled` → tab, discard, enabled).
    fn words(path: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for part in path.split('.') {
            let mut cur = String::new();
            for c in part.chars() {
                if c.is_ascii_uppercase() && !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                cur.push(c.to_ascii_lowercase());
            }
            if !cur.is_empty() {
                out.push(cur);
            }
        }
        out
    }

    /// A pattern matches at a word start ("card" in `offerSaveCards`, not in `tabDiscardEnabled`).
    fn excluded(path: &str) -> bool {
        let w = words(path);
        w.first().is_some_and(|f| f == "ai")
            || EXCLUDED.iter().any(|p| (0..w.len()).any(|i| w[i..].concat().starts_with(p)))
    }

    #[test]
    fn exclusion_matcher() {
        assert!(excluded("offerSaveCards") && excluded("devtoolsSizeRight") && excluded("ai.maxSteps"));
        assert!(excluded("vaultAutolockMinutes") && excluded("headers.acceptLanguage") && excluded("dns.mode"));
        assert!(!excluded("tabDiscardEnabled") && !excluded("theme") && !excluded("autoPip"));
    }

    #[test]
    fn allowed_keys_exist_and_are_clean() {
        let all = serde_json::to_value(Settings::default()).unwrap();
        let mut leaves = Vec::new();
        leaf_paths(&all, "", &mut leaves);
        for k in allowed_keys() {
            assert!(leaves.iter().any(|l| l == k), "{k} is not a setting");
            assert!(!excluded(k), "{k} must not be allowed");
        }
        assert_eq!(allowed_keys().len(), KEYS.len());
        let got = get_all(&Settings::default());
        assert_eq!(got.as_object().unwrap().len(), KEYS.len());
        assert_eq!(got["theme"]["value"], "system");
        assert_eq!(got["theme"]["values"], json!(["system", "light", "dark"]));
        assert_eq!(got["tabSleepMinutes"]["type"], "integer");
        assert_eq!(got["tabSleepMinutes"]["max"], 1440);
        assert_eq!(got["protection.level"]["values"], json!(["standard", "strict", "custom"]));
        assert!(!got["searchEngine"]["values"].as_array().unwrap().contains(&json!("custom")));
        assert_eq!(got["hardwareAcceleration"]["note"], "needs a restart");
    }

    #[test]
    fn excluded_keys_absent_and_refused() {
        let mut s = Settings::default();
        let all = serde_json::to_value(&s).unwrap();
        let mut leaves = Vec::new();
        leaf_paths(&all, "", &mut leaves);
        let got = get_all(&s);
        let text = got.to_string().to_ascii_lowercase();
        let mut n = 0;
        for l in leaves.iter().filter(|l| excluded(l)) {
            n += 1;
            assert!(got.get(l.as_str()).is_none(), "{l} readable");
            let before = s.clone();
            assert!(set(&mut s, l, json!("x")).is_err(), "{l} settable");
            assert!(set(&mut s, l, json!(true)).is_err(), "{l} settable");
            assert_eq!(s, before);
        }
        assert!(n > 30, "{n} excluded leaves");
        for word in
            ["vault", "proxy", "vpn", "useragent", "extraflags", "providers", "systempromptextra", "customsearchurl"]
        {
            assert!(!text.contains(word), "{word} in get_all");
        }
        // Also the whole objects and odd forms of the keys.
        for k in [
            "ai",
            "ai.enabled",
            "ai.providers",
            "vpn",
            "vpn.autoConnect",
            "vpn.showButton",
            "dns",
            "headers",
            "passgen",
            "permissions",
            "Theme",
            "theme ",
            "",
        ] {
            assert!(set(&mut s, k, json!(false)).is_err(), "{k}");
        }
        // Keys that are not allowed even though they look harmless.
        for k in ["ntp.image", "userscripts.privateTabs", "devtoolsSizeRight", "filterListsRev", "stripTrackingParams"]
        {
            assert!(set(&mut s, k, json!(1)).is_err(), "{k}");
        }
    }

    /// A valid value different from the current one.
    fn other_value(kind: Kind, current: &Value) -> Value {
        match kind {
            Kind::Bool => json!(!current.as_bool().unwrap()),
            Kind::Int(min, max) => {
                if current.as_i64() == Some(max) {
                    json!(min)
                } else {
                    json!(max)
                }
            }
            Kind::Num(..) => json!(1.5),
            Kind::Color => json!("#123abc"),
            Kind::Url => json!("https://example.org/start"),
            Kind::Choice(values) => values().into_iter().find(|v| v != current).unwrap(),
        }
    }

    #[test]
    fn every_allowed_key_round_trips() {
        for k in KEYS {
            let mut s = Settings::default();
            let cur = get_all(&s)[k.path]["value"].clone();
            let v = other_value(k.kind, &cur);
            let r = set(&mut s, k.path, v.clone());
            assert!(r.is_ok(), "{}: {r:?}", k.path);
            let now = get_all(&s)[k.path]["value"].clone();
            match k.kind {
                Kind::Num(..) => assert_eq!(now.as_f64(), v.as_f64(), "{}", k.path),
                _ => assert_eq!(now, v, "{}", k.path),
            }
            assert!(s.validate().is_ok(), "{}", k.path);
        }
    }

    #[test]
    fn invalid_values_refused() {
        let mut s = Settings::default();
        let before = s.clone();
        for (k, v) in [
            ("theme", json!("purple")),
            ("theme", json!(1)),
            ("theme", json!(null)),
            ("searchEngine", json!("custom")),
            ("appearance.uiFontPx", json!(30)),
            ("appearance.uiFontPx", json!("14")),
            ("appearance.uiFontPx", json!(13.5)),
            ("appearance.accent", json!("red")),
            ("appearance.accent", json!("#12345")),
            ("defaultZoom", json!(10)),
            ("defaultZoom", json!("1")),
            ("tabSleepMinutes", json!(-1)),
            ("tabSleepMinutes", json!(99999)),
            ("downloadConnections", json!(0)),
            ("adblockEnabled", json!("yes")),
            ("homepage", json!("javascript:alert(1)")),
            ("homepage", json!("file:///C:/x")),
            ("homepage", json!("krunk://passwords")),
            ("homepage", json!("https://krunk.invalid/passwords.html")),
            ("newTabUrl", json!("krunk://sync")),
            ("newTabUrl", json!("data:text/html,x")),
            ("ntp.wallpaper", json!("no-such-wallpaper")),
            ("protection.level", json!("off")),
        ] {
            assert!(set(&mut s, k, v.clone()).is_err(), "{k} = {v}");
            assert_eq!(s, before, "{k} changed");
        }
        // Valid alone, invalid with the rest: the whole result is validated.
        s.tab_discard_enabled = true;
        s.tab_discard_minutes = 30;
        assert!(set(&mut s, "tabSleepMinutes", json!(60)).unwrap_err().contains("longer than the sleep"));
        // A homepage start needs a homepage; krunk:// is not a homepage.
        let mut s = Settings::default();
        assert!(set(&mut s, "homepage", json!("krunk://settings")).is_err());
        // newTabUrl is checked by Settings::validate once the page is custom.
        assert!(set(&mut s, "newTabUrl", json!("krunk://settings")).is_ok());
        assert!(set(&mut s, "newTabPage", json!("custom")).is_ok());
        assert_eq!(s.new_tab_target().as_deref(), Some("https://krunk.invalid/settings.html"));
    }

    #[test]
    fn restart_flag_and_derived_fields() {
        let mut s = Settings::default();
        assert_eq!(set(&mut s, "theme", json!("dark")), Ok(Changed { restart: false }));
        assert_eq!(s.theme, Theme::Dark);
        assert_eq!(set(&mut s, "hardwareAcceleration", json!(false)), Ok(Changed { restart: true }));
        assert!(!s.hardware_acceleration);
        assert_eq!(set(&mut s, "hardwareAcceleration", json!(true)), Ok(Changed { restart: true }));
        // Strict protection blocks all third-party cookies (an engine switch: restart).
        let mut s = Settings::default();
        let r = set(&mut s, "protection.level", json!("strict")).unwrap();
        assert_eq!(s.protection.level, Level::Strict);
        let mut want = s.clone();
        want.sync_protection();
        assert_eq!(s, want, "derived fields follow the level");
        assert_eq!(r.restart, s.block_third_party_cookies);
        // Other fields are untouched by a change.
        let mut s = Settings { proxy: "socks5://127.0.0.1:9050".into(), ..Settings::default() };
        set(&mut s, "ntp.columns", json!(8)).unwrap();
        assert_eq!(s.ntp.columns, 8);
        assert_eq!(s.proxy, "socks5://127.0.0.1:9050");
    }

    #[test]
    fn settings_json_round_trip_is_lossless() {
        let s = Settings::default();
        let back: Settings = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }
}
