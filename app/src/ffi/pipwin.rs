//! The engine's Picture-in-Picture window (Krunk 0.7.0, R8): a views widget the engine creates
//! on Krunk's UI thread. Krunk finds it, follows its moves and takes the mouse wheel over it for
//! the volume; Krunk's volume bar sits next to it (`browser::pip`).

use super::window::ScreenRect;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumThreadWindows, GetClassNameW, GetWindowLongPtrW, GetWindowRect, IsWindow, IsWindowVisible, GWL_EXSTYLE,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCDESTROY, WM_WINDOWPOSCHANGED, WS_EX_TOPMOST,
};

const SUBCLASS_ID: usize = 0x4b50;

/// What happens to the watched window.
pub trait PipEvents {
    /// The mouse wheel over it (`delta` in notches, up is positive).
    fn wheel(&self, delta: f64);
    /// It moved or changed size.
    fn moved(&self);
    /// The pointer moves over it.
    fn hovered(&self);
    /// It is gone.
    fn destroyed(&self);
}

thread_local! {
    static WATCHED: RefCell<HashMap<isize, Rc<dyn PipEvents>>> = RefCell::new(HashMap::new());
}

/// Is this window the engine's floating video window Krunk is following? (The scroll lock leaves
/// it alone.)
pub fn is_watched(hwnd: HWND) -> bool {
    WATCHED.with(|w| w.try_borrow().map(|m| m.contains_key(&(hwnd.0 as isize))).unwrap_or(false))
}

/// A Picture-in-Picture window of the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PipWindow(HWND);

/// The engine's floating video window: a visible, always-on-top top-level window of the engine's
/// widget class on this thread (the UI thread runs the engine). `skip` are windows already
/// known; with several candidates the largest wins.
pub fn find(skip: &[PipWindow]) -> Option<PipWindow> {
    extern "system" fn each(h: HWND, data: LPARAM) -> windows::core::BOOL {
        // SAFETY: `data` is the Vec below, alive for the whole EnumThreadWindows call.
        let found = unsafe { &mut *(data.0 as *mut Vec<HWND>) };
        found.push(h);
        true.into()
    }
    let mut all: Vec<HWND> = Vec::new();
    // SAFETY: the callback only appends to `all`; this thread's windows only.
    unsafe {
        let _ = EnumThreadWindows(
            windows::Win32::System::Threading::GetCurrentThreadId(),
            Some(each),
            LPARAM(&mut all as *mut Vec<HWND> as isize),
        );
    }
    all.into_iter().map(PipWindow).filter(|w| !skip.contains(w) && w.is_candidate()).max_by_key(|w| {
        let r = w.rect();
        i64::from(r.width) * i64::from(r.height)
    })
}

impl PipWindow {
    fn is_candidate(self) -> bool {
        let mut class = [0u16; 64];
        // SAFETY: plain queries on a window of this thread; the buffer is on the stack.
        unsafe {
            let n = GetClassNameW(self.0, &mut class).max(0) as usize;
            let class = String::from_utf16_lossy(&class[..n]);
            let ex = GetWindowLongPtrW(self.0, GWL_EXSTYLE) as u32;
            let r = self.rect();
            class.starts_with("Chrome_WidgetWin_")
                && IsWindowVisible(self.0).as_bool()
                && ex & WS_EX_TOPMOST.0 != 0
                && r.width >= 100
                && r.height >= 60
        }
    }

    pub fn exists(self) -> bool {
        // SAFETY: IsWindow accepts any handle value.
        unsafe { IsWindow(Some(self.0)) }.as_bool()
    }

    pub fn is_visible(self) -> bool {
        // SAFETY: as above.
        unsafe { IsWindowVisible(self.0) }.as_bool()
    }

    /// Its screen rectangle in physical pixels.
    pub fn rect(self) -> ScreenRect {
        let mut r = RECT::default();
        // SAFETY: valid out pointer; an invalid handle leaves it empty.
        let _ = unsafe { GetWindowRect(self.0, &mut r) };
        ScreenRect { x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top }
    }

    /// The scale the engine draws the floating window at: its DPI scale times Windows' "Text
    /// size" (as `Window::scale`).
    pub fn scale(self) -> f64 {
        // SAFETY: plain query; 0 for an invalid handle.
        let dpi = unsafe { GetDpiForWindow(self.0) };
        let raw = if dpi == 0 { 1.0 } else { f64::from(dpi) / 96.0 };
        lb_core::dpi::Scale::new(raw * super::window::ui_factor()).factor()
    }

    /// Delivers its moves, the wheel and hovering to `events` until it is destroyed or
    /// [`PipWindow::unwatch`].
    pub fn watch(self, events: Rc<dyn PipEvents>) {
        WATCHED.with(|w| w.borrow_mut().insert(self.0 .0 as isize, events));
        // SAFETY: subclassing a window of this process and thread; removed on WM_NCDESTROY.
        unsafe {
            let _ = SetWindowSubclass(self.0, Some(pip_subclass), SUBCLASS_ID, 0);
        }
    }

    pub fn unwatch(self) {
        WATCHED.with(|w| w.borrow_mut().remove(&(self.0 .0 as isize)));
        if self.exists() {
            // SAFETY: removing our own subclass.
            unsafe {
                let _ = RemoveWindowSubclass(self.0, Some(pip_subclass), SUBCLASS_ID);
            }
        }
    }
}

fn watcher(hwnd: HWND) -> Option<Rc<dyn PipEvents>> {
    WATCHED.with(|w| w.try_borrow().ok().and_then(|m| m.get(&(hwnd.0 as isize)).cloned()))
}

extern "system" fn pip_subclass(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        WM_MOUSEWHEEL => {
            if let Some(ev) = watcher(hwnd) {
                let delta = ((wparam.0 >> 16) & 0xffff) as u16 as i16;
                ev.wheel(f64::from(delta) / 120.0);
                return LRESULT(0);
            }
        }
        WM_WINDOWPOSCHANGED => {
            if let Some(ev) = watcher(hwnd) {
                ev.moved();
            }
        }
        WM_MOUSEMOVE => {
            if let Some(ev) = watcher(hwnd) {
                ev.hovered();
            }
        }
        WM_NCDESTROY => {
            let ev = WATCHED.with(|w| w.try_borrow_mut().ok().and_then(|mut m| m.remove(&(hwnd.0 as isize))));
            // SAFETY: removing our own subclass from a window being destroyed.
            unsafe {
                let _ = RemoveWindowSubclass(hwnd, Some(pip_subclass), SUBCLASS_ID);
            }
            if let Some(ev) = ev {
                ev.destroyed();
            }
        }
        _ => {}
    }
    // SAFETY: default subclass processing.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
