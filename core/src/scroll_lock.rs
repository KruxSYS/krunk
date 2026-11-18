//! The page stays still while a dropdown is open (Krunk 0.8.0, A).
//!
//! A `<select>` list (and a datalist or date picker) is the engine's own popup window, a
//! top-level `Chrome_WidgetWin_*` window with `WS_POPUP`, owned by the Krunk frame. Chrome and
//! the engine close it on the first wheel notch over the page and scroll the page. Krunk instead
//! keeps it open and drops wheel input that lands outside it: the host's message hook asks
//! `should_drop` for every wheel message (and the touchpad's DirectManipulation hit test) and
//! turns it into a no-op. Wheel over the list itself still scrolls the list; keys, a click
//! outside and Esc close the popup as before, and the very next wheel scrolls the page.
//!
//! There is no state to go stale: the open popups are looked up for each wheel message.

/// `WM_MOUSEWHEEL`, `WM_MOUSEHWHEEL` and `DM_POINTERHITTEST` (a precision touchpad starting a
/// DirectManipulation scroll).
pub const WM_MOUSEWHEEL: u32 = 0x020A;
pub const WM_MOUSEHWHEEL: u32 = 0x020E;
pub const DM_POINTERHITTEST: u32 = 0x0250;

const WS_POPUP: u32 = 0x8000_0000;
/// A click-through window: the engine's hover tooltip (`TooltipAura`, `accept_events = false`)
/// has it; a dropdown, which takes clicks, never does.
const WS_EX_TRANSPARENT: u32 = 0x0000_0020;

/// The messages the lock looks at; everything else passes untouched.
pub fn is_scroll_message(id: u32) -> bool {
    matches!(id, WM_MOUSEWHEEL | WM_MOUSEHWHEEL | DM_POINTERHITTEST)
}

/// Mouse, pen and touch presses (`WM_LBUTTONDOWN`, `WM_RBUTTONDOWN`, `WM_MBUTTONDOWN`,
/// `WM_XBUTTONDOWN`, their non-client forms and `WM_POINTERDOWN`). While the login suggestions
/// are open the host watches them: a press outside the dropdown and its field closes it (0.25.2;
/// before, only the page's blur did, and a page that cancels `mousedown` never blurs the field).
pub fn is_press_message(id: u32) -> bool {
    matches!(id, 0x0201 | 0x0204 | 0x0207 | 0x020B | 0x00A1 | 0x00A4 | 0x00A7 | 0x00AB | 0x0246)
}

/// A press at (`x`, `y`) closes the watched dropdown when it is outside every kept rectangle.
pub fn press_is_outside(x: i32, y: i32, keep: &[Rect]) -> bool {
    !keep.iter().any(|r| r.contains(x, y))
}

/// A screen rectangle (right and bottom exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

/// What the host knows about a top-level window of the UI thread.
#[derive(Debug, Clone)]
pub struct TopWindow {
    pub class: String,
    pub style: u32,
    /// The extended style (`GWL_EXSTYLE`).
    pub ex_style: u32,
    pub visible: bool,
    /// Class of the end of its owner chain (`GA_ROOTOWNER`).
    pub root_owner_class: String,
    /// The engine's Picture-in-Picture window, which Krunk tracks itself.
    pub is_pip: bool,
    pub rect: Rect,
}

/// An open page popup of the engine: a dropdown list, datalist suggestions or a picker.
pub fn is_page_popup(w: &TopWindow) -> bool {
    w.visible
        && w.class.starts_with("Chrome_WidgetWin_")
        && w.style & WS_POPUP != 0
        && w.ex_style & WS_EX_TRANSPARENT == 0
        && w.root_owner_class == "KrunkWindow"
        && !w.is_pip
        && w.rect.right > w.rect.left
        && w.rect.bottom > w.rect.top
}

/// Drop a scroll message at screen point (`x`, `y`) when a page popup is open and the point is
/// outside every one of them.
pub fn should_drop(x: i32, y: i32, popups: &[Rect]) -> bool {
    !popups.is_empty() && !popups.iter().any(|r| r.contains(x, y))
}

/// The screen point of a wheel message: `lParam` holds signed 16-bit x and y.
pub fn wheel_point(lparam: isize) -> (i32, i32) {
    let x = (lparam & 0xffff) as u16 as i16 as i32;
    let y = ((lparam >> 16) & 0xffff) as u16 as i16 as i32;
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn popup() -> TopWindow {
        TopWindow {
            class: "Chrome_WidgetWin_1".into(),
            style: 0x9600_0000,
            ex_style: 0x0000_0088,
            visible: true,
            root_owner_class: "KrunkWindow".into(),
            is_pip: false,
            rect: Rect { left: 48, top: 284, right: 448, bottom: 752 },
        }
    }

    #[test]
    fn the_measured_select_popup_counts() {
        // Krunk 0.7.4 on the VM: Chrome_WidgetWin_1 style=0x96000000 owner=KrunkWindow.
        assert!(is_page_popup(&popup()));
    }

    #[test]
    fn other_windows_do_not() {
        let cases: [fn(&mut TopWindow); 7] = [
            |w| w.visible = false,
            |w| w.class = "KrunkPopup".into(), // Krunk's own menus and prompts
            |w| w.style = 0x16CF_0000,         // an overlapped window (DevTools)
            |w| w.root_owner_class = "Chrome_WidgetWin_1".into(), // not inside a Krunk frame
            |w| w.is_pip = true,               // the floating video
            |w| w.ex_style |= 0x20,            // the hover tooltip (title / alt text): click-through
            |w| w.rect = Rect { left: 10, top: 10, right: 10, bottom: 50 },
        ];
        for change in cases {
            let mut w = popup();
            change(&mut w);
            assert!(!is_page_popup(&w), "{w:?}");
        }
    }

    #[test]
    fn dropping() {
        let r = popup().rect;
        assert!(!should_drop(600, 500, &[]), "no popup: the page scrolls");
        assert!(should_drop(1000, 520, &[r]), "outside the list: dropped");
        assert!(!should_drop(200, 400, &[r]), "over the list: the list scrolls");
        assert!(!should_drop(48, 284, &[r]) && should_drop(448, 284, &[r]), "edges");
        let other = Rect { left: 900, top: 100, right: 1100, bottom: 300 };
        assert!(!should_drop(1000, 200, &[r, other]));
    }

    #[test]
    fn presses() {
        for down in [0x0201, 0x0204, 0x0207, 0x020B, 0x00A1, 0x00A4, 0x0246] {
            assert!(is_press_message(down), "{down:#x}");
        }
        for other in [0x0200, 0x0202, 0x0205, 0x020A, 0x0100, 0x00A0] {
            assert!(!is_press_message(other), "{other:#x}");
        }
        let popup = Rect { left: 100, top: 220, right: 400, bottom: 500 };
        let field = Rect { left: 100, top: 180, right: 380, bottom: 216 };
        assert!(!press_is_outside(150, 300, &[popup, field]), "on the dropdown");
        assert!(!press_is_outside(150, 200, &[popup, field]), "in the field");
        assert!(press_is_outside(150, 218, &[popup, field]), "between them");
        assert!(press_is_outside(900, 600, &[popup, field]), "elsewhere on the page");
    }

    #[test]
    fn messages_and_points() {
        assert!(
            is_scroll_message(WM_MOUSEWHEEL)
                && is_scroll_message(WM_MOUSEHWHEEL)
                && is_scroll_message(DM_POINTERHITTEST)
        );
        for other in [0x0200, 0x0201, 0x00FF, 0x0100, 0x0000] {
            assert!(!is_scroll_message(other));
        }
        assert_eq!(wheel_point((300 << 16) | 1000), (1000, 300));
        // A monitor left of the primary one: negative x.
        let lp = ((-200i32 as u16 as isize) << 16) | (-1500i32 as u16 as isize);
        assert_eq!(wheel_point(lp), (-1500, -200));
    }
}
