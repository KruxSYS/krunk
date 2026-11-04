//! The mouse cursor while a page holds the pointer lock: confined to the
//! page, hidden, and kept at the centre so movement never stops at an edge.

use super::window::ScreenRect;
use std::cell::Cell;
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON, VK_XBUTTON1, VK_XBUTTON2,
};
use windows::Win32::UI::WindowsAndMessaging::{ClipCursor, GetCursorPos, SetCursorPos, ShowCursor};

thread_local! {
    /// We hid the cursor (ShowCursor keeps a count; we add exactly one).
    static HIDDEN: Cell<bool> = const { Cell::new(false) };
}

/// Confines the cursor to `rect` (screen pixels), or frees it.
pub fn confine(rect: Option<ScreenRect>) {
    // SAFETY: ClipCursor with a rect on the stack or null.
    unsafe {
        let _ = match rect {
            Some(r) => ClipCursor(Some(&RECT { left: r.x, top: r.y, right: r.x + r.width, bottom: r.y + r.height })),
            None => ClipCursor(None),
        };
    }
}

/// Hides or shows the cursor (balanced: repeated calls do nothing).
pub fn set_hidden(hidden: bool) {
    if HIDDEN.with(|h| h.replace(hidden)) == hidden {
        return;
    }
    // SAFETY: plain call; the count change is undone by the opposite call.
    unsafe {
        ShowCursor(!hidden);
    }
}

pub fn position() -> (i32, i32) {
    let mut p = POINT::default();
    // SAFETY: local out pointer.
    let _ = unsafe { GetCursorPos(&mut p) };
    (p.x, p.y)
}

pub fn move_to(x: i32, y: i32) {
    // SAFETY: plain call.
    let _ = unsafe { SetCursorPos(x, y) };
}

/// The mouse buttons held down, as DOM `MouseEvent.buttons`.
pub fn buttons() -> i32 {
    let down = |vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY| {
        // SAFETY: plain call.
        (unsafe { GetAsyncKeyState(i32::from(vk.0)) } as u16 & 0x8000) != 0
    };
    let mut b = 0;
    for (vk, bit) in [(VK_LBUTTON, 1), (VK_RBUTTON, 2), (VK_MBUTTON, 4), (VK_XBUTTON1, 8), (VK_XBUTTON2, 16)] {
        if down(vk) {
            b |= bit;
        }
    }
    b
}
