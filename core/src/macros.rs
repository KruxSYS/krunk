//! Macros and hotkeys (0.12.0, WP-F): chords, conflicts with Krunk's shortcuts and between
//! macros, and key names for engine key events (shared by macros and the agent's `press_keys`).
//!
//! Key names follow a US keyboard layout: "a", "A" (Shift+a), "!", "Enter", "ArrowLeft", "F5",
//! "Numpad3". Hotkeys are layout-independent virtual-key codes with a canonical text
//! ("Ctrl+Shift+K", "Alt+F2", "Ctrl+.").

use crate::keys::{self, Command};
use serde::{Deserialize, Serialize};

/// A hotkey: modifiers plus a Windows virtual-key code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Chord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub vk: u32,
}

/// Punctuation on a US layout: (character, its Shift character, virtual key, DOM code).
const PUNCT: &[(char, char, u32, &str)] = &[
    ('.', '>', 0xBE, "Period"),
    (',', '<', 0xBC, "Comma"),
    (';', ':', 0xBA, "Semicolon"),
    ('\'', '"', 0xDE, "Quote"),
    ('/', '?', 0xBF, "Slash"),
    ('\\', '|', 0xDC, "Backslash"),
    ('[', '{', 0xDB, "BracketLeft"),
    (']', '}', 0xDD, "BracketRight"),
    ('-', '_', 0xBD, "Minus"),
    ('=', '+', 0xBB, "Equal"),
    ('`', '~', 0xC0, "Backquote"),
];

/// Shift + digit 0..9 on a US layout.
const DIGIT_SHIFTED: [char; 10] = [')', '!', '@', '#', '$', '%', '^', '&', '*', '('];

/// A named key: canonical (hotkey) name, other accepted names, virtual key, DOM code, DOM key,
/// the text it types.
type Named = (&'static str, &'static [&'static str], u32, &'static str, &'static str, Option<&'static str>);

const NAMED: &[Named] = &[
    ("Space", &["Spacebar"], 0x20, "Space", " ", Some(" ")),
    ("Enter", &["Return"], 0x0D, "Enter", "Enter", Some("\r")),
    ("Tab", &[], 0x09, "Tab", "Tab", None),
    ("Esc", &["Escape"], 0x1B, "Escape", "Escape", None),
    ("Backspace", &[], 0x08, "Backspace", "Backspace", None),
    ("Delete", &["Del"], 0x2E, "Delete", "Delete", None),
    ("Insert", &["Ins"], 0x2D, "Insert", "Insert", None),
    ("Home", &[], 0x24, "Home", "Home", None),
    ("End", &[], 0x23, "End", "End", None),
    ("PageUp", &["PgUp"], 0x21, "PageUp", "PageUp", None),
    ("PageDown", &["PgDn"], 0x22, "PageDown", "PageDown", None),
    ("Left", &["ArrowLeft"], 0x25, "ArrowLeft", "ArrowLeft", None),
    ("Up", &["ArrowUp"], 0x26, "ArrowUp", "ArrowUp", None),
    ("Right", &["ArrowRight"], 0x27, "ArrowRight", "ArrowRight", None),
    ("Down", &["ArrowDown"], 0x28, "ArrowDown", "ArrowDown", None),
];

/// Modifier keys as keys of their own (`key.down('Shift')`): name, other names, virtual key,
/// DOM code. They are never a hotkey's main key.
const MODIFIER_KEYS: &[(&str, &[&str], u32, &str)] = &[
    ("Shift", &[], 0x10, "ShiftLeft"),
    ("Control", &["Ctrl"], 0x11, "ControlLeft"),
    ("Alt", &["Option"], 0x12, "AltLeft"),
    ("Meta", &["Win", "Cmd", "Command", "Super", "OS"], 0x5B, "MetaLeft"),
];

const VK_F1: u32 = 0x70;
const VK_F24: u32 = 0x87;

fn eq(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// "F1".."F24" → its number.
fn f_number(name: &str) -> Option<u32> {
    let rest = name.strip_prefix('F').or_else(|| name.strip_prefix('f'))?;
    if rest.is_empty() || rest.len() > 2 || rest.starts_with('0') || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u32 = rest.parse().ok()?;
    (1..=24).contains(&n).then_some(n)
}

/// "Numpad0".."Numpad9" → the digit.
fn numpad_digit(name: &str) -> Option<u32> {
    let lower = name.to_ascii_lowercase();
    let rest = lower.strip_prefix("numpad")?;
    let mut chars = rest.chars();
    let d = chars.next()?.to_digit(10)?;
    chars.next().is_none().then_some(d)
}

/// Which modifier a name means: 'c', 's', 'a', 'm' (Meta / the Windows key).
fn modifier_of(name: &str) -> Option<char> {
    MODIFIER_KEYS
        .iter()
        .find(|(n, aliases, _, _)| eq(n, name) || aliases.iter().any(|a| eq(a, name)))
        .map(|(n, ..)| n.chars().next().unwrap_or('?').to_ascii_lowercase())
}

/// Splits "Ctrl+Shift+K" into modifier names and the key; "Ctrl++" and "+" have "+" as key.
fn split_combo(s: &str) -> Result<(Vec<&str>, &str), String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("No key given".into());
    }
    let (mods, key) = if s == "+" {
        ("", "+")
    } else if let Some(m) = s.strip_suffix("++") {
        (m, "+")
    } else {
        match s.rfind('+') {
            Some(i) => (&s[..i], &s[i + 1..]),
            None => ("", s),
        }
    };
    let key = key.trim();
    if key.is_empty() {
        return Err(format!("“{s}” has no key after the last +"));
    }
    let mods: Vec<&str> = if mods.is_empty() { Vec::new() } else { mods.split('+').map(str::trim).collect() };
    if mods.iter().any(|m| m.is_empty()) {
        return Err(format!("“{s}” has an empty part"));
    }
    Ok((mods, key))
}

impl Chord {
    /// Parses "Ctrl+Shift+K", "Alt+F2", "Ctrl+." (case-insensitive, any modifier order).
    pub fn parse(s: &str) -> Result<Chord, String> {
        let (mods, key) = split_combo(s)?;
        let mut c = Chord { ctrl: false, shift: false, alt: false, vk: 0 };
        for m in mods {
            match modifier_of(m) {
                Some('c') => c.ctrl = true,
                Some('s') => c.shift = true,
                Some('a') => c.alt = true,
                Some(_) => return Err("The Windows key can't be part of a hotkey".into()),
                None => return Err(format!("Unknown modifier “{m}”")),
            }
        }
        if modifier_of(key).is_some() {
            return Err("Add a key after the modifiers".into());
        }
        c.vk = chord_vk(key).ok_or_else(|| format!("Unknown key “{key}”"))?;
        Ok(c)
    }

    /// The canonical text: modifiers in the order Ctrl, Shift, Alt, then the key.
    pub fn format(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        s.push_str(&key_name(self.vk));
        s
    }

    /// Does it use Ctrl or Alt (what keeps a hotkey from hijacking typing)?
    fn has_command_modifier(&self) -> bool {
        self.ctrl || self.alt
    }
}

/// The hotkey key of a name: letters, digits, F1–F24, the named keys, unshifted punctuation,
/// Numpad0–9, and "0xNN" for keys without a name (from recording).
fn chord_vk(name: &str) -> Option<u32> {
    let mut chars = name.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        if ch.is_ascii_alphanumeric() {
            return Some(ch.to_ascii_uppercase() as u32);
        }
        if ch == '+' {
            return Some(0xBB);
        }
        return PUNCT.iter().find(|p| p.0 == ch).map(|p| p.2);
    }
    if let Some(n) = f_number(name) {
        return Some(VK_F1 + n - 1);
    }
    if let Some(d) = numpad_digit(name) {
        return Some(keys::vk::NUMPAD0 + d);
    }
    if eq(name, "Plus") {
        return Some(0xBB);
    }
    if let Some(n) = NAMED.iter().find(|n| eq(n.0, name) || n.1.iter().any(|a| eq(a, name))) {
        return Some(n.2);
    }
    let hex = name.strip_prefix("0x").or_else(|| name.strip_prefix("0X"))?;
    let vk = u32::from_str_radix(hex, 16).ok()?;
    (1..=0xFE).contains(&vk).then_some(vk)
}

/// A hotkey key's display name ("K", "F2", ".", "Space", "Numpad3"; "0xE2" for others).
pub fn key_name(vk: u32) -> String {
    match vk {
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(vk).map(String::from).unwrap_or_default(),
        VK_F1..=VK_F24 => format!("F{}", vk - VK_F1 + 1),
        0x60..=0x69 => format!("Numpad{}", vk - 0x60),
        _ => {
            if let Some(n) = NAMED.iter().find(|n| n.2 == vk) {
                return n.0.to_string();
            }
            if let Some(p) = PUNCT.iter().find(|p| p.2 == vk) {
                return p.0.to_string();
            }
            format!("0x{vk:02X}")
        }
    }
}

/// Is `vk` a modifier or lock key (Shift, Ctrl, Alt, Windows, Caps/Num/Scroll Lock)? Pressing
/// one alone never records or triggers a hotkey.
pub fn is_modifier_vk(vk: u32) -> bool {
    matches!(vk, 0x10..=0x12 | 0x14 | 0x5B | 0x5C | 0x90 | 0x91 | 0xA0..=0xA5)
}

/// A shortcut's human name.
fn command_name(cmd: Command) -> String {
    use Command::*;
    let name = match cmd {
        NewTab => "New tab",
        NewWindow => "New window",
        NewPrivateTab => "New private tab",
        NewPrivateWindow => "New private window",
        CloseTab => "Close tab",
        ReopenClosedTab => "Reopen closed tab",
        NextTab => "Next tab",
        PrevTab => "Previous tab",
        SelectTab(n) => return format!("Go to tab {}", n + 1),
        LastTab => "Go to the last tab",
        FocusAddressBar => "Address bar",
        Reload => "Reload",
        HardReload => "Reload without the cache",
        Stop => "Stop loading",
        Back => "Back",
        Forward => "Forward",
        Home => "Home page",
        Bookmark => "Bookmark this page",
        History => "History",
        Bookmarks => "Bookmarks",
        ToggleBookmarksBar => "Bookmarks bar",
        Downloads => "Downloads",
        Find => "Find in page",
        FindNext => "Find next",
        FindPrev => "Find previous",
        ZoomIn => "Zoom in",
        ZoomOut => "Zoom out",
        ZoomReset => "Reset zoom",
        Print => "Print",
        DevTools => "Developer tools",
        InspectElement => "Inspect element",
        Fullscreen => "Full screen",
        TaskManager => "Task manager",
        PictureInPicture => "Picture-in-Picture",
        ViewSource => "View page source",
        ToggleAgent => "AI agent panel",
        StopMacros => "Stop macros",
    };
    name.to_string()
}

/// The built-in shortcut a chord collides with, as its name ("New tab"): Krunk's shortcuts,
/// the keys DevTools keeps, Windows' window keys and the basic editing keys.
pub fn builtin_conflict(c: Chord) -> Option<String> {
    if let Some(cmd) = keys::command_for(c.vk, c.ctrl, c.shift, c.alt) {
        return Some(command_name(cmd));
    }
    if keys::devtools_keeps(c.vk, c.ctrl, c.shift, c.alt) {
        return Some("Developer tools".into());
    }
    let l = keys::vk::letter;
    let name = match (c.ctrl, c.shift, c.alt, c.vk) {
        (false, false, true, keys::vk::SPACE) => "Window menu",
        (false, false, true, 0x73) => "Close window",
        (_, _, true, keys::vk::TAB) | (false, _, true, keys::vk::ESCAPE) => "Switch windows",
        (true, false, false, keys::vk::ESCAPE) => "Start menu",
        (true, true, false, keys::vk::ESCAPE) => "Windows Task Manager",
        (true, false, true, 0x2E) => "Ctrl+Alt+Delete",
        (true, false, false, k) if k == l('C') => "Copy",
        (true, false, false, k) if k == l('X') => "Cut",
        (true, false, false, k) if k == l('V') => "Paste",
        (true, false, false, k) if k == l('A') => "Select all",
        (true, false, false, k) if k == l('Z') => "Undo",
        (true, false, false, k) if k == l('Y') => "Redo",
        (true, true, false, k) if k == l('Z') => "Redo",
        _ => return None,
    };
    Some(name.into())
}

/// Why a chord can't be a macro hotkey: a built-in shortcut, another macro's hotkey (by name),
/// or no modifier (plain keys would hijack typing; F1–F24 alone are allowed).
/// `others` are (macro id, name, hotkey); `except` is the macro being edited.
pub fn hotkey_problem(c: Chord, others: &[(String, String, Chord)], except: Option<&str>) -> Option<String> {
    if let Some(name) = builtin_conflict(c) {
        return Some(format!("Already used: {name}"));
    }
    if let Some((_, name, _)) = others.iter().find(|(id, _, chord)| *chord == c && Some(id.as_str()) != except) {
        return Some(format!("Already used by the macro “{name}”"));
    }
    if !c.has_command_modifier() && !(VK_F1..=VK_F24).contains(&c.vk) {
        return Some("Add Ctrl or Alt: only F1–F24 work without them".into());
    }
    if is_modifier_vk(c.vk) {
        return Some("Add a key after the modifiers".into());
    }
    None
}

/// The first press of a held key repeats after the system's keyboard delay, then at its rate.
/// `delay_setting` is `SPI_GETKEYBOARDDELAY` (0–3: 250–1000 ms), `speed_setting` is
/// `SPI_GETKEYBOARDSPEED` (0–31: about 2.5–30 repeats a second). Returns (delay, interval) ms.
pub fn repeat_timing(delay_setting: u32, speed_setting: u32) -> (u64, u64) {
    let delay = (u64::from(delay_setting.min(3)) + 1) * 250;
    let rate = 2.5 + f64::from(speed_setting.min(31)) * (30.0 - 2.5) / 31.0;
    (delay, (1000.0 / rate).round() as u64)
}

/// A key for engine key events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeySpec {
    /// DOM `key` ("a", "Enter", "ArrowLeft").
    pub key: String,
    /// DOM `code` ("KeyA", "Enter").
    pub code: String,
    /// Windows virtual-key code.
    pub vk: u32,
    /// The character it types, if any.
    pub text: Option<String>,
}

impl KeySpec {
    fn new(key: impl Into<String>, code: impl Into<String>, vk: u32, text: Option<&str>) -> KeySpec {
        KeySpec { key: key.into(), code: code.into(), vk, text: text.map(str::to_string) }
    }

    /// The character variant: with Shift (`shifted`) or without, for letters, digits and
    /// punctuation; other keys stay as they are.
    fn with_shift(&self, shifted: bool) -> KeySpec {
        let mut chars = self.key.chars();
        let Some(ch) = chars.next().filter(|_| chars.next().is_none()) else { return self.clone() };
        let base = unshifted(ch).unwrap_or(ch);
        let target = if shifted { shifted_of(base).unwrap_or(base) } else { base };
        if self.code.starts_with("Numpad") {
            return self.clone();
        }
        char_spec(target).unwrap_or_else(|| self.clone())
    }

    /// Is it the Shift variant of a character key ("A", "!")?
    fn is_shifted_char(&self) -> bool {
        let mut chars = self.key.chars();
        match (chars.next(), chars.next()) {
            (Some(ch), None) => !self.code.starts_with("Numpad") && unshifted(ch).is_some(),
            _ => false,
        }
    }

    /// Is it a modifier key itself (Shift, Control, Alt, Meta)?
    pub fn is_modifier(&self) -> bool {
        is_modifier_vk(self.vk)
    }
}

/// The unshifted character of a Shift character ('A' → 'a', '!' → '1'); None for others.
fn unshifted(ch: char) -> Option<char> {
    if ch.is_ascii_uppercase() {
        return Some(ch.to_ascii_lowercase());
    }
    if let Some(d) = DIGIT_SHIFTED.iter().position(|&c| c == ch) {
        return char::from_digit(d as u32, 10);
    }
    PUNCT.iter().find(|p| p.1 == ch).map(|p| p.0)
}

fn shifted_of(ch: char) -> Option<char> {
    if ch.is_ascii_lowercase() {
        return Some(ch.to_ascii_uppercase());
    }
    if let Some(d) = ch.to_digit(10) {
        return Some(DIGIT_SHIFTED[d as usize]);
    }
    PUNCT.iter().find(|p| p.0 == ch).map(|p| p.1)
}

/// The key that types an ASCII character on a US layout (Shift characters included).
fn char_spec(ch: char) -> Option<KeySpec> {
    let text = ch.to_string();
    if ch.is_ascii_alphabetic() {
        let up = ch.to_ascii_uppercase();
        return Some(KeySpec::new(text.clone(), format!("Key{up}"), up as u32, Some(&text)));
    }
    if let Some(d) = ch.to_digit(10) {
        return Some(KeySpec::new(text.clone(), format!("Digit{d}"), 0x30 + d, Some(&text)));
    }
    if ch == ' ' {
        return Some(KeySpec::new(" ", "Space", 0x20, Some(" ")));
    }
    if let Some(d) = DIGIT_SHIFTED.iter().position(|&c| c == ch) {
        return Some(KeySpec::new(text.clone(), format!("Digit{d}"), 0x30 + d as u32, Some(&text)));
    }
    let p = PUNCT.iter().find(|p| p.0 == ch || p.1 == ch)?;
    Some(KeySpec::new(text.clone(), p.3, p.2, Some(&text)))
}

/// The key and modifiers that type character `ch` on a US layout ("A" → Shift+a); None for
/// characters without a key (they are inserted as text). '\n' is Enter, '\t' is Tab.
pub fn key_for_char(ch: char) -> Option<(Mods, KeySpec)> {
    let spec = match ch {
        '\n' | '\r' => key_spec("Enter")?,
        '\t' => key_spec("Tab")?,
        _ => char_spec(ch)?,
    };
    let mods = Mods { shift: spec.is_shifted_char(), ..Mods::default() };
    Some((mods, spec))
}

/// Modifiers held with a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

impl Mods {
    /// The DevTools `Input` modifiers bitmask: Alt 1, Ctrl 2, Meta 4, Shift 8.
    pub fn cdp_bits(&self) -> u32 {
        u32::from(self.alt) | u32::from(self.ctrl) << 1 | u32::from(self.meta) << 2 | u32::from(self.shift) << 3
    }

    /// Both together.
    pub fn union(self, o: Mods) -> Mods {
        Mods {
            ctrl: self.ctrl || o.ctrl,
            shift: self.shift || o.shift,
            alt: self.alt || o.alt,
            meta: self.meta || o.meta,
        }
    }

    /// The modifier a held modifier key adds (Shift held → shift).
    pub fn of_key(vk: u32) -> Mods {
        Mods {
            shift: matches!(vk, 0x10 | 0xA0 | 0xA1),
            ctrl: matches!(vk, 0x11 | 0xA2 | 0xA3),
            alt: matches!(vk, 0x12 | 0xA4 | 0xA5),
            meta: matches!(vk, 0x5B | 0x5C),
        }
    }
}

/// A key name ("a", "A", "Enter", "Space", "ArrowLeft", "F5", "Escape", …).
///
/// Single characters are the US-layout key that types them ("A" types "A", "!" is Shift+1);
/// named keys are case-insensitive; DOM codes ("KeyA", "Digit1") and the modifier keys
/// ("Shift", "Control", "Alt", "Meta") work too.
pub fn key_spec(name: &str) -> Option<KeySpec> {
    let mut chars = name.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        return char_spec(ch);
    }
    if let Some(n) = f_number(name) {
        let f = format!("F{n}");
        return Some(KeySpec::new(f.clone(), f, VK_F1 + n - 1, None));
    }
    if let Some(d) = numpad_digit(name) {
        let t = d.to_string();
        return Some(KeySpec::new(t.clone(), format!("Numpad{d}"), keys::vk::NUMPAD0 + d, Some(&t)));
    }
    if let Some(n) = NAMED.iter().find(|n| eq(n.0, name) || n.1.iter().any(|a| eq(a, name))) {
        return Some(KeySpec::new(n.4, n.3, n.2, n.5));
    }
    if let Some(m) = MODIFIER_KEYS.iter().find(|(n, aliases, _, _)| eq(n, name) || aliases.iter().any(|a| eq(a, name)))
    {
        return Some(KeySpec::new(m.0, m.3, m.2, None));
    }
    // DOM codes.
    if let Some(rest) = name.strip_prefix("Key") {
        let mut c = rest.chars();
        if let (Some(ch), None) = (c.next(), c.next()) {
            if ch.is_ascii_uppercase() {
                return char_spec(ch.to_ascii_lowercase());
            }
        }
    }
    if let Some(rest) = name.strip_prefix("Digit") {
        let mut c = rest.chars();
        if let (Some(ch), None) = (c.next(), c.next()) {
            if ch.is_ascii_digit() {
                return char_spec(ch);
            }
        }
    }
    PUNCT.iter().find(|p| p.3 == name).and_then(|p| char_spec(p.0))
}

/// "Control+Shift+A" → the modifiers and the main key.
///
/// Modifiers: Control/Ctrl, Shift, Alt/Option. Meta (Win/Cmd) is accepted and ignored: Windows
/// keeps its Windows-key combinations, so pages never see them. Without Ctrl or Alt, a Shift
/// character means Shift ("A" and "!" press Shift); with Shift, a character key becomes its
/// Shift character ("Shift+a" types "A"); with Ctrl or Alt a letter is the letter key ("Ctrl+A"
/// is select-all, not Ctrl+Shift+A). A modifier name alone is that key ("Shift").
pub fn parse_keys(s: &str) -> Result<(Mods, KeySpec), String> {
    let (mod_names, key) = split_combo(s)?;
    let mut mods = Mods::default();
    for m in mod_names {
        match modifier_of(m) {
            Some('c') => mods.ctrl = true,
            Some('s') => mods.shift = true,
            Some('a') => mods.alt = true,
            Some(_) => {} // Meta: ignored (see above).
            None => return Err(format!("Unknown modifier “{m}”")),
        }
    }
    if modifier_of(key) == Some('m') {
        return Err("The Windows key (Meta) can't be pressed".into());
    }
    let mut spec = key_spec(key).ok_or_else(|| format!("Unknown key “{key}”"))?;
    if spec.is_modifier() {
        return Ok((mods, spec));
    }
    let letter = spec.key.len() == 1 && spec.key.chars().all(|c| c.is_ascii_alphabetic());
    if mods.shift {
        spec = spec.with_shift(true);
    } else if (mods.ctrl || mods.alt) && letter {
        spec = spec.with_shift(false);
    } else if spec.is_shifted_char() {
        mods.shift = true;
    }
    Ok((mods, spec))
}

/// The tab a macro acts on in its window (0.12.0): the window's active tab when it shows a web
/// page (http, https or file), else the window's most recently active web tab (a macro run from
/// krunk://scripts acts on the page the user came from). `None`: no web tab in that window.
pub fn target_tab(model: &crate::tabs::TabModel) -> Option<crate::tabs::TabId> {
    let web = |t: &crate::tabs::Tab| {
        crate::pages::Page::from_url(&t.url).is_none()
            && url::Url::parse(&t.url).is_ok_and(|u| {
                matches!(u.scheme(), "http" | "https" | "file")
                    && !u.host_str().is_some_and(|h| h == "invalid" || h.ends_with(".invalid"))
            })
    };
    let active = model.active_id();
    if let Some(t) = model.tabs().iter().find(|t| Some(t.id) == active && web(t)) {
        return Some(t.id);
    }
    model.tabs().iter().filter(|t| web(t)).max_by_key(|t| t.last_active).map(|t| t.id)
}

#[cfg(test)]
mod tests {
    #[test]
    fn macro_target_tab() {
        use crate::tabs::{Position, TabModel};
        let mut m = TabModel::new();
        assert_eq!(target_tab(&m), None);
        let (web, _) = m.open("https://a.example/", false, Position::End, true, true, 10);
        assert_eq!(target_tab(&m), Some(web), "the active web tab");
        let (old, _) = m.open("https://b.example/", false, Position::End, true, true, 20);
        let (scripts, _) = m.open("https://krunk.invalid/scripts.html", false, Position::End, true, true, 30);
        assert_eq!(m.active_id(), Some(scripts));
        assert_eq!(target_tab(&m), Some(old), "from krunk://scripts: the last web tab used");
        m.activate(web, 40);
        m.activate(scripts, 50);
        assert_eq!(target_tab(&m), Some(web));
        let mut only = TabModel::new();
        only.open("https://krunk.invalid/settings.html", false, Position::End, true, true, 1);
        assert_eq!(target_tab(&only), None, "no web tab: nothing to act on");
    }

    use super::*;

    fn chord(s: &str) -> Chord {
        Chord::parse(s).unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    #[test]
    fn chords_round_trip() {
        for s in [
            "Ctrl+Shift+K",
            "Alt+F2",
            "Ctrl+.",
            "Ctrl+Alt+Space",
            "F8",
            "F24",
            "Ctrl+Shift+Alt+Enter",
            "Ctrl+Numpad7",
            "Alt+Left",
            "Ctrl+PageDown",
            "Ctrl+Esc",
            "Alt+Backspace",
            "Ctrl+0",
            "Ctrl+0xE2",
        ] {
            assert_eq!(chord(s).format(), s);
        }
        for (s, p) in [(".", 0xBE), (",", 0xBC), (";", 0xBA), ("'", 0xDE), ("/", 0xBF), ("\\", 0xDC)] {
            let c = chord(&format!("Ctrl+Alt+{s}"));
            assert_eq!(c.vk, p);
            assert_eq!(c.format(), format!("Ctrl+Alt+{s}"));
        }
        for (s, p) in [("[", 0xDB), ("]", 0xDD), ("-", 0xBD), ("=", 0xBB), ("`", 0xC0)] {
            assert_eq!(chord(&format!("Alt+{s}")).vk, p);
            assert_eq!(chord(&format!("Alt+{s}")).format(), format!("Alt+{s}"));
        }
    }

    #[test]
    fn chord_spellings() {
        let k = Chord { ctrl: true, shift: true, alt: false, vk: 0x4B };
        assert_eq!(chord("ctrl+shift+k"), k);
        assert_eq!(chord("Shift+Control+K"), k, "any order, Control");
        assert_eq!(chord(" Ctrl + Shift + k "), k, "spaces");
        assert_eq!(chord("Alt+Escape"), chord("Alt+Esc"));
        assert_eq!(chord("Ctrl+ArrowUp"), chord("Ctrl+Up"));
        assert_eq!(chord("Ctrl+Return").format(), "Ctrl+Enter");
        assert_eq!(chord("Ctrl+Del").format(), "Ctrl+Delete");
        assert_eq!(chord("Ctrl+PgUp").format(), "Ctrl+PageUp");
        assert_eq!(chord("Ctrl++").format(), "Ctrl+=");
        assert_eq!(chord("ctrl+f5").format(), "Ctrl+F5");
        assert_eq!(chord("Alt+numpad3").format(), "Alt+Numpad3");
        assert_eq!(chord("Ctrl+Insert").vk, 0x2D);
        assert_eq!(chord("Ctrl+Tab").vk, keys::vk::TAB);
        assert_eq!(chord("Alt+Home").vk, keys::vk::HOME);
        assert_eq!(chord("Alt+End").vk, 0x23);
        assert_eq!(chord("Alt+Right").vk, keys::vk::RIGHT);
        assert_eq!(chord("Alt+Down").vk, 0x28);
        assert_eq!(chord("Ctrl+Option+K"), chord("Ctrl+Alt+K"));
    }

    #[test]
    fn bad_chords() {
        for s in ["", "Ctrl+", "Ctrl+Shift", "Hyper+K", "Ctrl+Foo", "Win+K", "Ctrl+F25", "Ctrl+F0", "Ctrl++K", "Ctrl+!"]
        {
            assert!(Chord::parse(s).is_err(), "{s:?} should not parse");
        }
        assert_eq!(Chord::parse("Ctrl+Shift").unwrap_err(), "Add a key after the modifiers");
        assert_eq!(Chord::parse("Meta+K").unwrap_err(), "The Windows key can't be part of a hotkey");
    }

    fn problem(s: &str) -> Option<String> {
        hotkey_problem(chord(s), &[], None)
    }

    #[test]
    fn builtin_conflicts() {
        for (s, name) in [
            ("Ctrl+T", "New tab"),
            ("Ctrl+W", "Close tab"),
            ("F5", "Reload"),
            ("F12", "Developer tools"),
            ("Ctrl+Shift+I", "Developer tools"),
            ("Alt+Space", "Window menu"),
            ("Alt+F4", "Close window"),
            ("Ctrl+Shift+Space", "AI agent panel"),
            ("Ctrl+Shift+.", "Stop macros"),
            ("Ctrl+Tab", "Next tab"),
            ("Ctrl+Shift+Tab", "Previous tab"),
            ("Ctrl+3", "Go to tab 3"),
            ("Ctrl+9", "Go to the last tab"),
            ("Shift+Esc", "Task manager"),
            ("Ctrl+Shift+F", "Developer tools"),
            ("Ctrl+O", "Developer tools"),
            ("Ctrl+]", "Developer tools"),
            ("Ctrl+C", "Copy"),
            ("Ctrl+V", "Paste"),
            ("Ctrl+Shift+Z", "Redo"),
            ("Alt+Tab", "Switch windows"),
            ("Ctrl+=", "Zoom in"),
            ("Ctrl+Shift+=", "Zoom in"),
        ] {
            assert_eq!(builtin_conflict(chord(s)).as_deref(), Some(name), "{s}");
            assert_eq!(problem(s), Some(format!("Already used: {name}")), "{s}");
        }
        for s in ["Ctrl+Shift+K", "Ctrl+Alt+K", "Alt+F2", "F8", "Ctrl+.", "Ctrl+Alt+Space", "Alt+1", "Ctrl+Shift+1"] {
            assert_eq!(builtin_conflict(chord(s)), None, "{s}");
            assert_eq!(problem(s), None, "{s}");
        }
    }

    #[test]
    fn a_modifier_is_required() {
        for s in ["K", "Shift+K", "Space", "Enter", "Shift+Enter", "Numpad1", "1", "."] {
            assert_eq!(problem(s).as_deref(), Some("Add Ctrl or Alt: only F1–F24 work without them"), "{s}");
        }
        // F-keys alone (or with Shift) are fine when Krunk doesn't use them.
        for s in ["F1", "F2", "F8", "F9", "F10", "F13", "F24", "Shift+F8"] {
            assert_eq!(problem(s), None, "{s}");
        }
        // Krunk's own F-keys are not.
        assert!(problem("F11").is_some() && problem("F3").is_some() && problem("F6").is_some());
    }

    #[test]
    fn macro_vs_macro() {
        let others = vec![
            ("m1".to_string(), "Hold A".to_string(), chord("Ctrl+Alt+H")),
            ("m2".to_string(), "Fill form".to_string(), chord("F8")),
        ];
        assert_eq!(
            hotkey_problem(chord("ctrl+alt+h"), &others, None).as_deref(),
            Some("Already used by the macro “Hold A”")
        );
        assert_eq!(
            hotkey_problem(chord("F8"), &others, Some("m1")).as_deref(),
            Some("Already used by the macro “Fill form”")
        );
        // The macro being edited keeps its own hotkey.
        assert_eq!(hotkey_problem(chord("Ctrl+Alt+H"), &others, Some("m1")), None);
        assert_eq!(hotkey_problem(chord("Ctrl+Alt+J"), &others, None), None);
        // A built-in conflict wins over a macro's.
        let bad = vec![("m3".to_string(), "Old".to_string(), chord("Ctrl+T"))];
        assert_eq!(hotkey_problem(chord("Ctrl+T"), &bad, None).as_deref(), Some("Already used: New tab"));
    }

    #[test]
    fn key_specs() {
        let k = |n: &str| key_spec(n).unwrap_or_else(|| panic!("{n}"));
        assert_eq!(k("a"), KeySpec::new("a", "KeyA", 0x41, Some("a")));
        assert_eq!(k("A"), KeySpec::new("A", "KeyA", 0x41, Some("A")));
        assert_eq!(k("z").vk, 0x5A);
        assert_eq!(k("7"), KeySpec::new("7", "Digit7", 0x37, Some("7")));
        assert_eq!(k("!"), KeySpec::new("!", "Digit1", 0x31, Some("!")));
        assert_eq!(k("("), KeySpec::new("(", "Digit9", 0x39, Some("(")));
        assert_eq!(k("?"), KeySpec::new("?", "Slash", 0xBF, Some("?")));
        assert_eq!(k(" "), KeySpec::new(" ", "Space", 0x20, Some(" ")));
        assert_eq!(k("Space"), k(" "));
        assert_eq!(k("Enter"), KeySpec::new("Enter", "Enter", 0x0D, Some("\r")));
        assert_eq!(k("enter"), k("Enter"));
        assert_eq!(k("Tab"), KeySpec::new("Tab", "Tab", 0x09, None));
        assert_eq!(k("Escape"), KeySpec::new("Escape", "Escape", 0x1B, None));
        assert_eq!(k("Esc"), k("Escape"));
        assert_eq!(k("ArrowLeft"), KeySpec::new("ArrowLeft", "ArrowLeft", 0x25, None));
        assert_eq!(k("Left"), k("ArrowLeft"));
        assert_eq!(k("F5"), KeySpec::new("F5", "F5", 0x74, None));
        assert_eq!(k("F24").vk, 0x87);
        assert_eq!(k("Backspace").vk, 0x08);
        assert_eq!(k("Delete").vk, 0x2E);
        assert_eq!(k("PageDown").code, "PageDown");
        assert_eq!(k("Numpad4"), KeySpec::new("4", "Numpad4", 0x64, Some("4")));
        assert_eq!(k("Shift"), KeySpec::new("Shift", "ShiftLeft", 0x10, None));
        assert_eq!(k("Control"), k("Ctrl"));
        assert_eq!(k("KeyQ"), k("q"));
        assert_eq!(k("Digit3"), k("3"));
        assert_eq!(k("Period"), k("."));
        for n in ["", "é", "Foo", "F25", "F0", "Numpad10", "Keyq"] {
            assert_eq!(key_spec(n), None, "{n:?}");
        }
    }

    #[test]
    fn parse_key_combos() {
        let p = |s: &str| parse_keys(s).unwrap_or_else(|e| panic!("{s}: {e}"));
        let ctrl = Mods { ctrl: true, ..Mods::default() };
        let shift = Mods { shift: true, ..Mods::default() };
        let ctrl_shift = Mods { ctrl: true, shift: true, ..Mods::default() };
        assert_eq!(p("Control+Shift+A"), (ctrl_shift, key_spec("A").unwrap()));
        assert_eq!(p("Ctrl+a"), (ctrl, key_spec("a").unwrap()));
        assert_eq!(p("Control+A"), (ctrl, key_spec("a").unwrap()), "Ctrl+A is select-all, not Ctrl+Shift+A");
        assert_eq!(p("Enter"), (Mods::default(), key_spec("Enter").unwrap()));
        assert_eq!(p("a"), (Mods::default(), key_spec("a").unwrap()));
        assert_eq!(p("A"), (shift, key_spec("A").unwrap()), "A is Shift+a");
        assert_eq!(p("Shift+a"), (shift, key_spec("A").unwrap()));
        assert_eq!(p("Shift+1"), (shift, key_spec("!").unwrap()));
        assert_eq!(p("!"), (shift, key_spec("!").unwrap()));
        assert_eq!(p("Meta+a"), (Mods::default(), key_spec("a").unwrap()), "Meta is ignored");
        assert_eq!(p("Shift"), (Mods::default(), key_spec("Shift").unwrap()));
        assert_eq!(p("Control+Shift"), (ctrl, key_spec("Shift").unwrap()));
        assert_eq!(p("+"), (shift, key_spec("+").unwrap()));
        assert_eq!(p("Ctrl++").1, key_spec("+").unwrap());
        assert_eq!(p("Alt+ArrowLeft"), (Mods { alt: true, ..Mods::default() }, key_spec("ArrowLeft").unwrap()));
        assert_eq!(p("Shift+Tab"), (shift, key_spec("Tab").unwrap()));
        assert_eq!(p("Shift+Numpad1").1, key_spec("Numpad1").unwrap());
        assert!(parse_keys("").is_err());
        assert!(parse_keys("Ctrl+").is_err());
        assert!(parse_keys("Hyper+a").is_err());
        assert!(parse_keys("Ctrl+Nope").is_err());
        assert!(parse_keys("Meta").is_err());
    }

    #[test]
    fn chars_to_keys() {
        assert_eq!(key_for_char('a'), Some((Mods::default(), key_spec("a").unwrap())));
        assert!(key_for_char('Q').unwrap().0.shift);
        assert_eq!(key_for_char('@').unwrap(), (Mods { shift: true, ..Mods::default() }, key_spec("@").unwrap()));
        assert_eq!(key_for_char('\n').unwrap().1.key, "Enter");
        assert_eq!(key_for_char('\t').unwrap().1.key, "Tab");
        assert_eq!(key_for_char('é'), None);
        assert_eq!(key_for_char('€'), None);
        // Every printable ASCII character has a key.
        for c in ' '..='~' {
            let (m, k) = key_for_char(c).unwrap_or_else(|| panic!("{c:?}"));
            assert_eq!(k.text.as_deref(), Some(c.to_string().as_str()));
            assert_eq!(m.shift, k.is_shifted_char());
        }
    }

    #[test]
    fn modifier_bits_and_repeat_timing() {
        assert_eq!(Mods { alt: true, ctrl: true, meta: true, shift: true }.cdp_bits(), 15);
        assert_eq!(Mods { ctrl: true, ..Mods::default() }.cdp_bits(), 2);
        assert_eq!(Mods { shift: true, ..Mods::default() }.cdp_bits(), 8);
        assert_eq!(Mods::of_key(0x10).union(Mods::of_key(0xA3)), Mods { shift: true, ctrl: true, ..Mods::default() });
        assert_eq!(repeat_timing(0, 31), (250, 33));
        assert_eq!(repeat_timing(1, 31), (500, 33));
        assert_eq!(repeat_timing(3, 0), (1000, 400));
        assert_eq!(repeat_timing(9, 99), (1000, 33), "clamped");
        let (_, mid) = repeat_timing(1, 15);
        assert!((60..=80).contains(&mid), "{mid}");
        assert!(is_modifier_vk(0x11) && is_modifier_vk(0xA5) && !is_modifier_vk(0x41));
    }
}
