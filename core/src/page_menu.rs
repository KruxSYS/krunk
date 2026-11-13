//! The page context menu outside a tab (0.16.0): the AI panel gets the menu pages get (the
//! engine's native menu plus Krunk's items, built in `app/src/ffi/cef/client.rs`), without what
//! belongs to a page. Decided here, without the engine: which of the engine's own items stay,
//! and which separators are left over once the others are gone. Which image items the panel
//! offers is in `image_menu`.

/// The engine's command ids (CEF's `cef_menu_id_t`).
pub mod id {
    pub const BACK: i32 = 100;
    pub const FORWARD: i32 = 101;
    pub const RELOAD: i32 = 102;
    pub const RELOAD_NOCACHE: i32 = 103;
    pub const STOPLOAD: i32 = 104;
    pub const UNDO: i32 = 110;
    pub const REDO: i32 = 111;
    pub const CUT: i32 = 112;
    pub const COPY: i32 = 113;
    pub const PASTE: i32 = 114;
    pub const PASTE_MATCH_STYLE: i32 = 115;
    pub const DELETE: i32 = 116;
    pub const SELECT_ALL: i32 = 117;
    pub const FIND: i32 = 130;
    pub const PRINT: i32 = 131;
    pub const VIEW_SOURCE: i32 = 132;
    pub const SPELLCHECK_SUGGESTION_0: i32 = 200;
    pub const ADD_TO_DICTIONARY: i32 = 206;
}

/// Does the panel's menu keep this item of the engine's? Only the editing ones (Undo, Redo,
/// Cut, Copy, Paste, Paste as plain text, Delete, Select all) and the spelling suggestions of
/// a text field. Everything else goes: Back, Forward, Reload, Stop, Find, Print and "View page
/// source" act on a page, and an item the engine adds later is not shown before it is looked at.
pub fn panel_keeps(command_id: i32) -> bool {
    matches!(command_id, id::UNDO..=id::SELECT_ALL | id::SPELLCHECK_SUGGESTION_0..=id::ADD_TO_DICTIONARY)
}

/// The separators to remove from a menu (`separator[i]`: is item `i` one?) so that none
/// leads, none trails and no two follow each other: their indices, highest first (removing
/// them in this order keeps the remaining indices valid).
pub fn stray_separators(separator: &[bool]) -> Vec<usize> {
    let mut stray = Vec::new();
    // The separator kept last, while no item has followed it.
    let mut open: Option<usize> = None;
    let mut item_before = false;
    for (i, sep) in separator.iter().enumerate() {
        if !*sep {
            item_before = true;
            open = None;
        } else if item_before && open.is_none() {
            open = Some(i);
        } else {
            stray.push(i);
        }
    }
    stray.extend(open);
    stray.sort_unstable_by(|a, b| b.cmp(a));
    stray
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_keeps_editing_items_only() {
        for kept in [
            id::UNDO,
            id::REDO,
            id::CUT,
            id::COPY,
            id::PASTE,
            id::PASTE_MATCH_STYLE,
            id::DELETE,
            id::SELECT_ALL,
            id::SPELLCHECK_SUGGESTION_0,
            204,
            205,
            id::ADD_TO_DICTIONARY,
        ] {
            assert!(panel_keeps(kept), "{kept}");
        }
        for gone in [
            id::BACK,
            id::FORWARD,
            id::RELOAD,
            id::RELOAD_NOCACHE,
            id::STOPLOAD,
            id::FIND,
            id::PRINT,
            id::VIEW_SOURCE,
            // A page's own items (MENU_ID_CUSTOM_FIRST…LAST) and ids nobody knows yet.
            220,
            250,
            118,
            133,
            0,
            -1,
        ] {
            assert!(!panel_keeps(gone), "{gone}");
        }
    }

    /// The menu after the stray separators are removed, as a string: `-` a separator, `x` an item.
    fn tidy(menu: &str) -> String {
        let flags: Vec<bool> = menu.chars().map(|c| c == '-').collect();
        let mut items: Vec<char> = menu.chars().collect();
        let stray = stray_separators(&flags);
        assert!(stray.windows(2).all(|w| w[0] > w[1]), "highest first: {stray:?}");
        for i in stray {
            assert_eq!(items.remove(i), '-');
        }
        items.into_iter().collect()
    }

    #[test]
    fn separators_are_tidied() {
        assert_eq!(tidy(""), "");
        assert_eq!(tidy("-"), "");
        assert_eq!(tidy("---"), "");
        assert_eq!(tidy("x"), "x");
        assert_eq!(tidy("xx-x"), "xx-x");
        assert_eq!(tidy("-x"), "x");
        assert_eq!(tidy("x-"), "x");
        assert_eq!(tidy("x--x"), "x-x");
        assert_eq!(tidy("--x---x-x--"), "x-x-x");
        // An empty area of the panel: Back, Forward, a separator, Print and "View page source"
        // lose their items; nothing is left, so no menu shows.
        assert_eq!(tidy("-"), "");
        // A link: its four items and their separator, then the page's lone separator.
        assert_eq!(tidy("xxxx--"), "xxxx");
        // A text field: Undo Redo | Cut Copy Paste Paste-plain Delete | Select all.
        assert_eq!(tidy("xx-xxxxx-x"), "xx-xxxxx-x");
    }
}
