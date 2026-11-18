//! The page stays still while a dropdown is open (Krunk 0.8.0, A; the rules are in
//! `lb_core::scroll_lock`). A `WH_GETMESSAGE` hook on the UI thread, which also runs the
//! engine, sees every message before it is dispatched. Anything but a wheel message (or a
//! touchpad's DirectManipulation hit test) passes at once; for those, the engine's open page
//! popups (and Krunk's own open menus) are looked up and the message becomes `WM_NULL` when it
//! lands outside all of them, so the engine neither scrolls the page nor closes the popup.

use super::pipwin;
use lb_core::scroll_lock::{self, Rect, TopWindow};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, EnumThreadWindows, GetAncestor, GetClassNameW, GetWindowLongPtrW, GetWindowRect, IsWindowVisible,
    SetWindowsHookExW, GA_ROOTOWNER, GWL_EXSTYLE, GWL_STYLE, HC_ACTION, HHOOK, MSG, WH_GETMESSAGE, WM_NULL,
};

thread_local! {
    static HOOK: Cell<Option<HHOOK>> = const { Cell::new(None) };
    /// Krunk's open menus (the main popup slot and the item menu over a folder dropdown): the
    /// page stays still under them too (0.8.0, A item 6; the history menus let it scroll).
    static MENUS: Cell<[Option<isize>; 2]> = const { Cell::new([None, None]) };
}

/// A press watch (0.25.2): the window and rectangles where presses don't count, and what runs
/// once on the first press outside them (it should only queue work: it runs inside the hook).
struct PressWatch {
    window: Option<isize>,
    keep: Vec<Rect>,
    outside: Rc<dyn Fn()>,
}

thread_local! {
    static PRESS_WATCH: RefCell<Option<PressWatch>> = const { RefCell::new(None) };
}

/// Calls `outside` once on the first mouse, pen or touch press on this thread outside `window`
/// (its live rectangle) and `keep` (screen rectangles). Replaces an earlier watch.
pub fn watch_presses(window: Option<HWND>, keep: Vec<Rect>, outside: Rc<dyn Fn()>) {
    PRESS_WATCH.with(|w| *w.borrow_mut() = Some(PressWatch { window: window.map(|h| h.0 as isize), keep, outside }));
}

/// Ends the press watch.
pub fn unwatch_presses() {
    PRESS_WATCH.with(|w| {
        if let Ok(mut w) = w.try_borrow_mut() {
            w.take();
        }
    });
}

/// A press at (`x`, `y`): the watch's callback when it lands outside (the watch then ends).
fn press_outside(x: i32, y: i32) -> Option<Rc<dyn Fn()>> {
    PRESS_WATCH.with(|w| {
        let mut w = w.try_borrow_mut().ok()?;
        let watch = w.as_ref()?;
        let mut keep = watch.keep.clone();
        if let Some(h) = watch.window {
            let h = HWND(h as *mut std::ffi::c_void);
            let mut r = windows::Win32::Foundation::RECT::default();
            // SAFETY: plain queries; a stale handle just fails.
            if unsafe { IsWindowVisible(h) }.as_bool() && unsafe { GetWindowRect(h, &mut r) }.is_ok() {
                keep.push(Rect { left: r.left, top: r.top, right: r.right, bottom: r.bottom });
            }
        }
        if !scroll_lock::press_is_outside(x, y, &keep) {
            return None;
        }
        w.take().map(|watch| watch.outside)
    })
}

/// Krunk's menu window in `slot` (0: the main popup, 1: the item menu) opened (`Some`) or closed.
pub fn set_menu(slot: usize, hwnd: Option<HWND>) {
    MENUS.with(|m| {
        let mut v = m.get();
        if let Some(x) = v.get_mut(slot) {
            *x = hwnd.map(|h| h.0 as isize);
        }
        m.set(v);
    });
}

/// The open menus' screen rectangles.
fn open_menus() -> Vec<Rect> {
    MENUS
        .with(|m| m.get())
        .into_iter()
        .flatten()
        .filter_map(|h| {
            let h = HWND(h as *mut std::ffi::c_void);
            let mut r = windows::Win32::Foundation::RECT::default();
            // SAFETY: plain queries; a stale handle just fails.
            let visible = unsafe { IsWindowVisible(h) }.as_bool() && unsafe { GetWindowRect(h, &mut r) }.is_ok();
            visible.then_some(Rect { left: r.left, top: r.top, right: r.right, bottom: r.bottom })
        })
        .collect()
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
            ex_style: GetWindowLongPtrW(h, GWL_EXSTYLE) as u32,
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
        if scroll_lock::is_press_message(msg.message) {
            let POINT { x, y } = msg.pt;
            if let Some(outside) = press_outside(x, y) {
                outside();
            }
        } else if scroll_lock::is_scroll_message(msg.message) {
            let POINT { x, y } = msg.pt;
            let (x, y) = if msg.message == scroll_lock::DM_POINTERHITTEST {
                (x, y)
            } else {
                scroll_lock::wheel_point(msg.lParam.0)
            };
            let mut targets = open_menus();
            targets.extend(open_popups());
            if scroll_lock::should_drop(x, y, &targets) {
                msg.message = WM_NULL;
            }
        }
    }
    // SAFETY: passes the message on to any other hook of this thread.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}
