//! Engine input for the agent and macros (0.12.0, WP-F): key and mouse events sent to a tab's
//! page with CDP `Input.*` (engine-level, never OS-wide injection). Held keys repeat at the
//! system's keyboard delay and rate until released; every Stop releases all of them.

// 0.12.0 spine: parts of this are used once the work packages are wired in (M4).
#![allow(dead_code)]

use super::Browser;
use lb_core::macros::{KeySpec, Mods};
use lb_core::tabs::TabId;

/// What a key event does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KeyAction {
    Press,
    Down { repeat: bool },
    Up,
}

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MouseButton {
    Left,
    Middle,
    Right,
}

#[derive(Default)]
pub(super) struct InputHost {}

impl Browser {
    /// Sends a key to tab `tab`'s page.
    pub(super) fn input_key(&self, tab: TabId, key: &KeySpec, mods: Mods, action: KeyAction) {
        let _ = (tab, key, mods, action); // WP-F
    }

    /// Clicks at a viewport point (CSS px) of tab `tab`.
    pub(super) fn input_click(&self, tab: TabId, x: f64, y: f64, button: MouseButton, count: u8) {
        let _ = (tab, x, y, button, count); // WP-F
    }

    pub(super) fn input_mouse_move(&self, tab: TabId, x: f64, y: f64) {
        let _ = (tab, x, y); // WP-F
    }

    /// Types text into the focused element of tab `tab` (key events; non-ASCII as text input).
    pub(super) fn input_type(&self, tab: TabId, text: &str, done: impl FnOnce() + 'static) {
        let _ = (tab, text);
        done(); // WP-F
    }

    /// Releases every key and button held in any tab.
    pub(super) fn input_release_all(&self) {
        // WP-F
    }

    pub(super) fn input_tab_gone(&self, tab: TabId) {
        let _ = tab; // WP-F
    }
}
