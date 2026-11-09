//! The page stays still while a dropdown is open (Krunk 0.8.0, A; the rules are in
//! `lb_core::scroll_lock`). A `WH_GETMESSAGE` hook on the UI thread, which also runs the
//! engine, sees every message before it is dispatched. Anything but a wheel message (or a
//! touchpad's DirectManipulation hit test) passes at once; for those, the engine's open page
//! popups are looked up and the message becomes `WM_NULL` when it lands outside all of them, so
//! the engine neither scrolls the page nor closes the popup.

use super::pipwin;
use lb_core::scroll_lock::{self, Rect, TopWindow};
use std::cell::Cell;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, EnumThreadWindows, GetAncestor, GetClassNameW, GetWindowLongPtrW, GetWindowRect, IsWindowVisible,
    SetWindowsHookExW, GA_ROOTOWNER, GWL_STYLE, HC_ACTION, HHOOK, MSG, WH_GETMESSAGE, WM_NULL,
};

thread_local! {
    static HOOK: Cell<Option<HHOOK>> = const { Cell::new(None) };
}

/// Installs the hook on the calling (UI) thread. Without it Krunk works as before, so a failure
/// is only logged by the caller.
pub fn install() -> Result<(), String> {
    if HOOK.with(|h| h.get()).is_some() {
        return Ok(());
    }
    // SAFETY: a thread hook (no module handle) on this thread; the procedure is a plain function.
    let hook = unsafe { SetWindowsHookExW(WH_GETMESSAGE, Some(hook_proc), None, GetCurrentThreadId()) }
        .map_err(|e| format!("scroll lock: SetWindowsHookEx failed: {e}"))?;
    HOOK.with(|h| h.set(Some(hook)));
    Ok(())
}

fn class_of(h: HWND) -> String {
    let mut buf = [0u16; 64];
    // SAFETY: a stack buffer; any window handle is accepted.
    let n = unsafe { GetClassNameW(h, &mut buf) }.max(0) as usize;
    String::from_utf16_lossy(&buf[..n])
}

fn describe(h: HWND) -> TopWindow {
    let mut r = windows::Win32::Foundation::RECT::default();
    // SAFETY: plain queries on a window of this thread.
    unsafe {
        let _ = GetWindowRect(h, &mut r);
        let root = GetAncestor(h, GA_ROOTOWNER);
        TopWindow {
            class: class_of(h),
            style: GetWindowLongPtrW(h, GWL_STYLE) as u32,
            visible: IsWindowVisible(h).as_bool(),
            root_owner_class: if root.0.is_null() || root == h { String::new() } else { class_of(root) },
            is_pip: pipwin::is_watched(h),
            rect: Rect { left: r.left, top: r.top, right: r.right, bottom: r.bottom },
        }
    }
}

/// The engine's open page popups on this thread.
fn open_popups() -> Vec<Rect> {
    extern "system" fn each(h: HWND, data: LPARAM) -> windows::core::BOOL {
        // SAFETY: `data` is the Vec below, alive for the whole EnumThreadWindows call.
        let found = unsafe { &mut *(data.0 as *mut Vec<Rect>) };
        // Cheap tests first: most top-level windows are invisible message windows.
        // SAFETY: plain query.
        if unsafe { IsWindowVisible(h) }.as_bool() {
            let w = describe(h);
            if scroll_lock::is_page_popup(&w) {
                found.push(w.rect);
            }
        }
        true.into()
    }
    let mut found: Vec<Rect> = Vec::new();
    // SAFETY: the callback only appends to `found`; this thread's windows only.
    unsafe {
        let _ = EnumThreadWindows(GetCurrentThreadId(), Some(each), LPARAM(&mut found as *mut Vec<Rect> as isize));
    }
    found
}

extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lparam.0 != 0 {
        // SAFETY: for WH_GETMESSAGE, lParam points to the MSG being retrieved; the hook may
        // change it.
        let msg = unsafe { &mut *(lparam.0 as *mut MSG) };
        if scroll_lock::is_scroll_message(msg.message) {
            let POINT { x, y } = msg.pt;
            let (x, y) = if msg.message == scroll_lock::DM_POINTERHITTEST {
                (x, y)
            } else {
                scroll_lock::wheel_point(msg.lParam.0)
            };
            if scroll_lock::should_drop(x, y, &open_popups()) {
                msg.message = WM_NULL;
            }
        }
    }
    // SAFETY: passes the message on to any other hook of this thread.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}
