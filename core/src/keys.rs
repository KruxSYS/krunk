//! Keyboard shortcuts: maps a Windows virtual-key code plus modifiers to a
//! browser command. The host calls [`command_for`] from every controller's
//! `AcceleratorKeyPressed` and marks the event handled when it returns `Some`.

use serde::{Deserialize, Serialize};

/// Browser commands triggered by shortcuts (and by the shell's menu).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "command", content = "arg")]
pub enum Command {
    NewTab,
    /// Ctrl+N: a new window with a new-tab page.
    NewWindow,
    NewPrivateTab,
    /// A new window whose tabs are private (the title bar's menu).
    NewPrivateWindow,
    CloseTab,
    ReopenClosedTab,
    NextTab,
    PrevTab,
    /// Ctrl+1..8: select tab by 0-based position.
    SelectTab(u8),
    /// Ctrl+9.
    LastTab,
    FocusAddressBar,
    Reload,
    /// Ctrl+F5 / Ctrl+Shift+R: reload bypassing the cache.
    HardReload,
    Stop,
    Back,
    Forward,
    Home,
    Bookmark,
    History,
    Bookmarks,
    /// Ctrl+Shift+B: show or hide the bookmarks bar ("new tabs only" counts as shown).
    ToggleBookmarksBar,
    Downloads,
    Find,
    FindNext,
    FindPrev,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Print,
    DevTools,
    /// DevTools with the element under the mouse selected (Ctrl+Shift+C).
    InspectElement,
    /// Full screen without the tab strip and toolbar (F11).
    Fullscreen,
    /// Shift+Esc: the task manager window.
    TaskManager,
}

/// Virtual-key codes (winuser.h).
pub mod vk {
    pub const TAB: u32 = 0x09;
    pub const ESCAPE: u32 = 0x1B;
    pub const HOME: u32 = 0x24;
    pub const LEFT: u32 = 0x25;
    pub const RIGHT: u32 = 0x27;
    pub const KEY_0: u32 = 0x30;
    pub const KEY_1: u32 = 0x31;
    pub const KEY_9: u32 = 0x39;
    pub const NUMPAD0: u32 = 0x60;
    pub const NUMPAD1: u32 = 0x61;
    pub const NUMPAD9: u32 = 0x69;
    pub const ADD: u32 = 0x6B;
    pub const SUBTRACT: u32 = 0x6D;
    pub const F3: u32 = 0x72;
    pub const F5: u32 = 0x74;
    pub const F6: u32 = 0x75;
    pub const F11: u32 = 0x7A;
    pub const F12: u32 = 0x7B;
    pub const BROWSER_BACK: u32 = 0xA6;
    pub const BROWSER_FORWARD: u32 = 0xA7;
    pub const BROWSER_REFRESH: u32 = 0xA8;
    pub const BROWSER_HOME: u32 = 0xAC;
    pub const OEM_PLUS: u32 = 0xBB;
    pub const OEM_MINUS: u32 = 0xBD;

    pub const fn letter(c: char) -> u32 {
        c as u32
    }
}

/// The shortcuts the new tab page lists (label, keys), in display order.
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("New tab", "Ctrl+T"),
    ("Reopen closed tab", "Ctrl+Shift+T"),
    ("New window", "Ctrl+N"),
    ("New private tab", "Ctrl+Shift+N"),
    ("Address bar", "Ctrl+L"),
    ("Find in page", "Ctrl+F"),
    ("Zoom in / out / reset", "Ctrl++ / Ctrl+- / Ctrl+0"),
    ("Task manager", "Shift+Esc"),
    ("History", "Ctrl+H"),
    ("Bookmarks", "Ctrl+Shift+O"),
    ("Downloads", "Ctrl+J"),
    ("Bookmark this page", "Ctrl+D"),
    ("Developer tools", "F12"),
    ("Full screen", "F11"),
];

/// Maps a key press to a command. `vk` is the virtual-key code; letters
/// are their upper-case ASCII codes.
pub fn command_for(vk: u32, ctrl: bool, shift: bool, alt: bool) -> Option<Command> {
    use Command::*;
    // Ctrl+digit selects tabs (top row and numpad).
    if ctrl && !alt && !shift {
        let digit = match vk {
            vk::KEY_1..=vk::KEY_9 => Some(vk - vk::KEY_1),
            vk::NUMPAD1..=vk::NUMPAD9 => Some(vk - vk::NUMPAD1),
            _ => None,
        };
        if let Some(d) = digit {
            return Some(if d == 8 { LastTab } else { SelectTab(d as u8) });
        }
    }
    let cmd = match (ctrl, shift, alt, vk) {
        (true, false, false, k) if k == vk::letter('T') => NewTab,
        (true, false, false, k) if k == vk::letter('N') => NewWindow,
        (true, true, false, k) if k == vk::letter('N') => NewPrivateTab,
        (true, false, false, k) if k == vk::letter('W') => CloseTab,
        (true, false, false, k) if k == vk::letter('F') => Find,
        (true, true, false, k) if k == vk::letter('T') => ReopenClosedTab,
        (true, false, false, vk::TAB) => NextTab,
        (true, true, false, vk::TAB) => PrevTab,
        (true, false, false, k) if k == vk::letter('L') => FocusAddressBar,
        (false, false, true, k) if k == vk::letter('D') => FocusAddressBar,
        (false, false, false, vk::F6) => FocusAddressBar,
        (true, false, false, k) if k == vk::letter('R') => Reload,
        (false, false, false, vk::F5) | (false, false, false, vk::BROWSER_REFRESH) => Reload,
        (true, true, false, k) if k == vk::letter('R') => HardReload,
        (true, false, false, vk::F5) | (false, true, false, vk::F5) => HardReload,
        (false, false, true, vk::LEFT) | (false, false, false, vk::BROWSER_BACK) => Back,
        (false, false, true, vk::RIGHT) | (false, false, false, vk::BROWSER_FORWARD) => Forward,
        (false, false, true, vk::HOME) | (false, false, false, vk::BROWSER_HOME) => Home,
        (true, false, false, k) if k == vk::letter('D') => Bookmark,
        (true, false, false, k) if k == vk::letter('H') => History,
        (true, true, false, k) if k == vk::letter('O') => Bookmarks,
        (true, true, false, k) if k == vk::letter('B') => ToggleBookmarksBar,
        (true, false, false, k) if k == vk::letter('J') => Downloads,
        (false, false, false, vk::F3) => FindNext,
        (true, false, false, k) if k == vk::letter('G') => FindNext,
        (false, true, false, vk::F3) => FindPrev,
        (true, true, false, k) if k == vk::letter('G') => FindPrev,
        // Ctrl+= / Ctrl++ (Shift on most layouts) / numpad +.
        (true, _, false, vk::OEM_PLUS) | (true, _, false, vk::ADD) => ZoomIn,
        (true, _, false, vk::OEM_MINUS) | (true, _, false, vk::SUBTRACT) => ZoomOut,
        (true, false, false, vk::KEY_0) | (true, false, false, vk::NUMPAD0) => ZoomReset,
        (true, false, false, k) if k == vk::letter('P') => Print,
        (false, false, false, vk::F12) => DevTools,
        (false, false, false, vk::F11) => Fullscreen,
        (false, true, false, vk::ESCAPE) => TaskManager,
        (true, true, false, k) if k == vk::letter('I') => DevTools,
        (true, true, false, k) if k == vk::letter('C') => InspectElement,
        _ => return None,
    };
    Some(cmd)
}

/// Keys DevTools uses itself, so they are not Krunk shortcuts while DevTools has the focus:
/// find in panel (Ctrl+F, Ctrl+G), search all sources (Ctrl+Shift+F), quick open (Ctrl+P,
/// Ctrl+O), the command menu (Ctrl+Shift+P), the element picker (Ctrl+Shift+C), panel
/// switching (Ctrl+[ and Ctrl+]) and the console drawer (Esc). Every other Krunk shortcut
/// works in DevTools as anywhere else, including F12 and Ctrl+Shift+I, which close it.
pub fn devtools_keeps(vk: u32, ctrl: bool, shift: bool, alt: bool) -> bool {
    const OEM_4: u32 = 0xDB; // [
    const OEM_6: u32 = 0xDD; // ]
    if alt {
        return false;
    }
    match (ctrl, shift) {
        (false, false) => vk == vk::ESCAPE,
        (true, false) => {
            [vk::letter('F'), vk::letter('G'), vk::letter('P'), vk::letter('O'), OEM_4, OEM_6].contains(&vk)
        }
        (true, true) => [vk::letter('F'), vk::letter('P'), vk::letter('C')].contains(&vk),
        _ => false,
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn listed_shortcuts_are_real() {
        // Every listed single shortcut maps to a command.
        let parse = |k: &str| {
            let mut ctrl = false;
            let mut shift = false;
            let mut key = "";
            for part in k.split('+') {
                match part {
                    "Ctrl" => ctrl = true,
                    "Shift" => shift = true,
                    p => key = p,
                }
            }
            let vk = match key {
                "T" | "N" | "L" | "F" | "H" | "O" | "J" | "D" => vk::letter(key.chars().next().unwrap()),
                "Esc" => vk::ESCAPE,
                "F12" => vk::F12,
                "F11" => vk::F11,
                "0" => vk::KEY_0,
                _ => return None,
            };
            command_for(vk, ctrl, shift, false)
        };
        for (label, keys) in SHORTCUTS {
            if keys.contains(" / ") {
                continue;
            }
            assert!(parse(keys).is_some(), "{label}: {keys}");
        }
        assert_eq!(parse("Shift+Esc"), Some(Command::TaskManager));
    }
    use super::Command::*;
    use super::*;

    fn ctrl(k: u32) -> Option<Command> {
        command_for(k, true, false, false)
    }
    fn ctrl_shift(k: u32) -> Option<Command> {
        command_for(k, true, true, false)
    }
    fn alt(k: u32) -> Option<Command> {
        command_for(k, false, false, true)
    }
    fn plain(k: u32) -> Option<Command> {
        command_for(k, false, false, false)
    }
    fn l(c: char) -> u32 {
        vk::letter(c)
    }

    #[test]
    fn requirement_3_shortcuts() {
        assert_eq!(ctrl(l('T')), Some(NewTab));
        assert_eq!(ctrl(l('W')), Some(CloseTab));
        assert_eq!(ctrl(vk::TAB), Some(NextTab));
        assert_eq!(ctrl_shift(vk::TAB), Some(PrevTab));
        for i in 0..8u32 {
            assert_eq!(ctrl(vk::KEY_1 + i), Some(SelectTab(i as u8)));
            assert_eq!(ctrl(vk::NUMPAD1 + i), Some(SelectTab(i as u8)));
        }
        assert_eq!(ctrl(vk::KEY_9), Some(LastTab));
        assert_eq!(ctrl(vk::NUMPAD9), Some(LastTab));
        assert_eq!(ctrl(l('L')), Some(FocusAddressBar));
        assert_eq!(ctrl(l('R')), Some(Reload));
        assert_eq!(plain(vk::F5), Some(Reload));
        assert_eq!(alt(vk::LEFT), Some(Back));
        assert_eq!(alt(vk::RIGHT), Some(Forward));
        assert_eq!(ctrl_shift(l('T')), Some(ReopenClosedTab));
    }

    #[test]
    fn other_shortcuts() {
        assert_eq!(ctrl(l('D')), Some(Bookmark));
        assert_eq!(ctrl(l('H')), Some(History));
        assert_eq!(ctrl(l('F')), Some(Find));
        assert_eq!(plain(vk::F12), Some(DevTools));
        assert_eq!(plain(vk::F11), Some(Fullscreen));
        assert_eq!(ctrl_shift(l('I')), Some(DevTools));
        assert_eq!(ctrl_shift(l('C')), Some(InspectElement));
        assert_eq!(ctrl(l('C')), None, "Ctrl+C stays copy");
        assert_eq!(ctrl(vk::OEM_PLUS), Some(ZoomIn));
        assert_eq!(ctrl_shift(vk::OEM_PLUS), Some(ZoomIn), "Ctrl+Shift+= is Ctrl++");
        assert_eq!(ctrl(vk::ADD), Some(ZoomIn));
        assert_eq!(ctrl(vk::OEM_MINUS), Some(ZoomOut));
        assert_eq!(ctrl(vk::SUBTRACT), Some(ZoomOut));
        assert_eq!(ctrl(vk::KEY_0), Some(ZoomReset));
        assert_eq!(ctrl(vk::NUMPAD0), Some(ZoomReset));
        assert_eq!(ctrl(l('P')), Some(Print));
        assert_eq!(ctrl_shift(l('N')), Some(NewPrivateTab));
        assert_eq!(command_for(l('N'), true, false, false), Some(NewWindow));
        assert_eq!(ctrl_shift(l('B')), Some(ToggleBookmarksBar));
        // In DevTools: its own keys stay with it; Krunk's toggles still reach Krunk.
        assert!(devtools_keeps(l('F'), true, false, false));
        assert!(devtools_keeps(l('C'), true, true, false));
        assert!(devtools_keeps(vk::ESCAPE, false, false, false));
        assert!(devtools_keeps(0xDB, true, false, false));
        assert!(!devtools_keeps(vk::F12, false, false, false));
        assert!(!devtools_keeps(l('I'), true, true, false));
        assert!(!devtools_keeps(l('T'), true, false, false));
        assert!(!devtools_keeps(l('W'), true, false, false));
        assert!(!devtools_keeps(vk::ESCAPE, false, false, true));
        assert_eq!(ctrl(vk::F5), Some(HardReload));
        assert_eq!(ctrl_shift(l('R')), Some(HardReload));
        assert_eq!(alt(l('D')), Some(FocusAddressBar));
        assert_eq!(plain(vk::F6), Some(FocusAddressBar));
        assert_eq!(plain(vk::F3), Some(FindNext));
        assert_eq!(command_for(vk::F3, false, true, false), Some(FindPrev));
        assert_eq!(ctrl(l('J')), Some(Downloads));
        assert_eq!(ctrl_shift(l('O')), Some(Bookmarks));
        assert_eq!(alt(vk::HOME), Some(Home));
        assert_eq!(plain(vk::BROWSER_BACK), Some(Back));
    }

    #[test]
    fn unrelated_keys_pass_through() {
        assert_eq!(plain(l('T')), None, "typing is not a shortcut");
        assert_eq!(plain(vk::TAB), None);
        assert_eq!(command_for(l('T'), true, false, true), None, "Ctrl+Alt = AltGr");
        assert_eq!(command_for(vk::KEY_1, true, false, true), None);
        assert_eq!(ctrl_shift(vk::KEY_1), None);
        assert_eq!(ctrl(l('C')), None, "copy stays with the page");
        assert_eq!(ctrl(l('V')), None);
        assert_eq!(ctrl(l('A')), None);
        assert_eq!(plain(vk::LEFT), None);
        assert_eq!(plain(vk::ESCAPE), None);
    }

    #[test]
    fn serde_shape() {
        assert_eq!(serde_json::to_string(&SelectTab(3)).unwrap(), r#"{"command":"selectTab","arg":3}"#);
        assert_eq!(serde_json::to_string(&NewTab).unwrap(), r#"{"command":"newTab"}"#);
        let back: Command = serde_json::from_str(r#"{"command":"zoomIn"}"#).unwrap();
        assert_eq!(back, ZoomIn);
    }
}
