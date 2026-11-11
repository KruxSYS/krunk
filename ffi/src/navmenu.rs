//! The history lists behind Back and Forward (long press in the Android menu), with
//! `core::navmenu`'s rules: Back shows the earlier pages newest first, Forward the later ones in
//! order, at most `MAX_ITEMS`, and a tab's first blank page is never offered.
use lb_core::navmenu;

/// One entry of a tab's back/forward list, as the engine reports it.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NavEntry {
    pub url: String,
    pub title: String,
}

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct NavItem {
    /// The entry's position in the engine's list (what the app jumps to).
    pub index: u32,
    /// The title, or the address when the page has none.
    pub label: String,
    /// The address as the address bar shows it.
    pub url: String,
}

/// The rows of the Back (`forward = false`) or Forward list for a tab whose engine list is
/// `entries` with the page shown at `current`. A `current` outside the list gives no rows.
#[uniffi::export]
pub fn nav_menu(entries: Vec<NavEntry>, current: u32, forward: bool) -> Vec<NavItem> {
    let current = current as usize;
    if current >= entries.len() {
        return Vec::new();
    }
    let list: Vec<navmenu::Entry> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| navmenu::Entry { id: i as i64, url: e.url.clone(), title: e.title.clone() })
        .collect();
    navmenu::entries_for(current, &list, forward)
        .into_iter()
        .map(|e| {
            let shown = crate::display_url(e.url.clone());
            NavItem { index: e.id as u32, label: navmenu::label(&e.title, &shown), url: shown }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(url: &str, title: &str) -> NavEntry {
        NavEntry { url: url.into(), title: title.into() }
    }

    fn pages(n: usize) -> Vec<NavEntry> {
        (0..n).map(|i| entry(&format!("https://example.com/p{i}"), &format!("Page {i}"))).collect()
    }

    #[test]
    fn back_is_newest_first_and_forward_in_order() {
        let list = pages(5);
        let back: Vec<u32> = nav_menu(list.clone(), 3, false).iter().map(|i| i.index).collect();
        assert_eq!(back, [2, 1, 0]);
        let forward: Vec<u32> = nav_menu(list.clone(), 1, true).iter().map(|i| i.index).collect();
        assert_eq!(forward, [2, 3, 4]);
        assert!(nav_menu(list.clone(), 0, false).is_empty());
        assert!(nav_menu(list.clone(), 4, true).is_empty());
        // An index the list does not have: nothing, and no panic.
        assert!(nav_menu(list, 9, false).is_empty());
        assert!(nav_menu(Vec::new(), 0, true).is_empty());
    }

    #[test]
    fn rows_are_labelled_by_title_or_address_and_capped() {
        let list = vec![
            entry("https://example.com/a", "  A title  "),
            entry("https://example.com/b?x=1", ""),
            entry("https://example.com/c", "C"),
        ];
        let back = nav_menu(list, 2, false);
        assert_eq!(back[0].label, "https://example.com/b?x=1");
        assert_eq!(back[0].url, "https://example.com/b?x=1");
        assert_eq!(back[1].label, "A title");
        // At most the core's limit, the nearest pages first.
        let many = nav_menu(pages(40), 39, false);
        assert_eq!(many.len(), lb_core::navmenu::MAX_ITEMS);
        assert_eq!(many[0].index, 38);
    }

    #[test]
    fn the_blank_page_a_tab_started_with_is_not_offered() {
        let list = vec![entry("about:blank", ""), entry("https://a.example/", "A"), entry("https://b.example/", "B")];
        let back: Vec<u32> = nav_menu(list.clone(), 2, false).iter().map(|i| i.index).collect();
        assert_eq!(back, [1]);
        assert!(nav_menu(list, 1, false).is_empty());
    }
}
