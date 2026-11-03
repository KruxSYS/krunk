//! The top-level browser window and the message loop.

use std::cell::RefCell;
use std::rc::Rc;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetStockObject, HBRUSH, WHITE_BRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW, LoadCursorW,
    LoadIconW, PostQuitMessage, RegisterClassExW, ShowWindow, TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT,
    IDC_ARROW, IDI_APPLICATION, MSG, SIZE_MINIMIZED, SW_SHOW, WINDOW_EX_STYLE, WM_CLOSE, WM_DESTROY, WM_MOVE,
    WM_MOVING, WM_SIZE, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};

/// Callbacks from the window procedure. Implementations must tolerate
/// re-entrancy (a modal dialog pumps messages while a callback runs).
pub trait WindowEvents {
    fn resized(&self, width: i32, height: i32);
    fn minimized(&self, minimized: bool);
    fn moved(&self);
    /// Return `true` to let the window close.
    fn close_requested(&self) -> bool;
    fn destroyed(&self);
}

thread_local! {
    static EVENTS: RefCell<Option<Rc<dyn WindowEvents>>> = const { RefCell::new(None) };
}

fn events() -> Option<Rc<dyn WindowEvents>> {
    EVENTS.with(|e| e.borrow().clone())
}

const CLASS_NAME: PCWSTR = w!("LightBrowserWindow");

/// Handle to the main window (UI thread only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window(HWND);

impl Window {
    /// Creates and shows the main window; `events` receives its messages.
    pub fn create(title: &str, events: Rc<dyn WindowEvents>) -> Result<Window, String> {
        EVENTS.with(|e| *e.borrow_mut() = Some(events));
        // SAFETY: plain Win32 window creation with a static class name and a
        // window procedure that lives for the whole program.
        unsafe {
            let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wndproc),
                hInstance: instance.into(),
                hIcon: LoadIconW(None, IDI_APPLICATION).unwrap_or_default(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: HBRUSH(GetStockObject(WHITE_BRUSH).0),
                lpszClassName: CLASS_NAME,
                ..Default::default()
            };
            if RegisterClassExW(&class) == 0 {
                return Err(format!("RegisterClassExW failed: {}", windows::core::Error::from_thread()));
            }
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                CLASS_NAME,
                &HSTRING::from(title),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                1280,
                860,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .map_err(|e| format!("CreateWindowExW failed: {e}"))?;
            let _ = ShowWindow(hwnd, SW_SHOW);
            Ok(Window(hwnd))
        }
    }

    pub fn hwnd(self) -> HWND {
        self.0
    }

    /// Client area as `(width, height)` in physical pixels.
    pub fn client_size(self) -> (i32, i32) {
        let mut r = RECT::default();
        // SAFETY: valid window handle and out pointer.
        let _ = unsafe { GetClientRect(self.0, &mut r) };
        (r.right - r.left, r.bottom - r.top)
    }

    pub fn destroy(self) {
        // SAFETY: valid window handle owned by this thread.
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_SIZE => {
            if let Some(ev) = events() {
                let minimized = wparam.0 as u32 == SIZE_MINIMIZED;
                ev.minimized(minimized);
                if !minimized {
                    let w = (lparam.0 & 0xffff) as i32;
                    let h = ((lparam.0 >> 16) & 0xffff) as i32;
                    ev.resized(w, h);
                }
            }
            LRESULT(0)
        }
        WM_MOVE | WM_MOVING => {
            if let Some(ev) = events() {
                ev.moved();
            }
            // SAFETY: default processing for messages we only observe.
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_CLOSE => {
            if events().is_none_or(|ev| ev.close_requested()) {
                // SAFETY: destroying our own window on its thread.
                let _ = unsafe { DestroyWindow(hwnd) };
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            if let Some(ev) = events() {
                ev.destroyed();
            }
            EVENTS.with(|e| e.borrow_mut().take());
            // SAFETY: ends the message loop of this thread.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        // SAFETY: default processing for everything else.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Runs the message loop until `WM_QUIT`; returns the exit code.
pub fn run_message_loop() -> i32 {
    let mut msg = MSG::default();
    // SAFETY: standard message loop on the UI thread.
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    msg.wParam.0 as i32
}
