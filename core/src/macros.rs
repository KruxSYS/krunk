//! Macros and hotkeys (0.12.0, WP-F): chords, conflicts with Krunk's shortcuts and between
//! macros, and key names for engine key events (shared by macros and the agent's `press_keys`).

use serde::{Deserialize, Serialize};

/// A hotkey: modifiers plus a Windows virtual-key code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Chord {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub vk: u32,
}

impl Chord {
    /// Parses "Ctrl+Shift+K", "Alt+F2", "Ctrl+." (case-insensitive, any modifier order).
    pub fn parse(s: &str) -> Result<Chord, String> {
        let _ = s;
        Err("WP-F".into())
    }

    /// The canonical text: modifiers in the order Ctrl, Shift, Alt, then the key.
    pub fn format(&self) -> String {
        String::new()
    }
}

/// The built-in shortcut a chord collides with, as its name ("New tab").
pub fn builtin_conflict(c: Chord) -> Option<String> {
    let _ = c;
    None
}

/// Why a chord can't be a macro hotkey: a built-in shortcut, another macro's hotkey (by name),
/// or no modifier (plain keys would hijack typing; F1–F24 alone are allowed).
/// `others` are (macro id, name, hotkey); `except` is the macro being edited.
pub fn hotkey_problem(c: Chord, others: &[(String, String, Chord)], except: Option<&str>) -> Option<String> {
    let _ = (c, others, except);
    None
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

/// Modifiers held with a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

/// A key name ("a", "A", "Enter", "Space", "ArrowLeft", "F5", "Escape", …).
pub fn key_spec(name: &str) -> Option<KeySpec> {
    let _ = name;
    None
}

/// "Control+Shift+A" → the modifiers and the main key.
pub fn parse_keys(s: &str) -> Result<(Mods, KeySpec), String> {
    let _ = s;
    Err("WP-F".into())
}
