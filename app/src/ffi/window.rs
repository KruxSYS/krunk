//! Top-level windows (main frame and the popup) and the message loop.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, GetMonitorInfoW, GetStockObject, MonitorFromPoint, HBRUSH, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    WHITE_BRUSH,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW, IsWindowVisible,
    LoadCursorW, LoadIconW, PostQuitMessage, RegisterClassExW, SetWindowPos, SetWindowTextW, ShowWindow,
    TranslateMessage, CS_DROPSHADOW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, HWND_TOP, IDC_ARROW, IDI_APPLICATION,
    MINMAXINFO, MSG, SIZE_MINIMIZED, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE, SW_SHOW, WA_INACTIVE,
    WINDOW_EX_STYLE, WM_ACTIVATE, WM_APP, WM_CLOSE, WM_DESTROY, WM_DPICHANGED, WM_GETMINMAXINFO, WM_MOVE, WM_MOVING,
    WM_SIZE, WNDCLASSEXW, WS_CLIPCHILDREN, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_OVERLAPPEDWINDOW, WS_POPUP,
};

/// Callbacks from the window procedure. Implementations must tolerate
/// re-entrancy (a modal dialog pumps messages while a callback runs).
pub trait WindowEvents {
    fn resized(&self, _width: i32, _height: i32) {}
    fn minimized(&self, _minimized: bool) {}
    fn moved(&self) {}
    /// The window was activated or deactivated.
    fn activated(&self, _active: bool) {}
    /// Return `true` to let the window close.
    fn close_requested(&self) -> bool {
        true
    }
    fn destroyed(&self) {}
    /// A `WM_APP` message posted to this window.
    fn app_message(&self, _code: usize) {}
    /// A timer started with [`Window::set_timer`] fired.
    fn timer(&self, _id: usize) {}
}

thread_local! {
    static EVENTS: RefCell<HashMap<isize, Rc<dyn WindowEvents>>> = RefCell::new(HashMap::new());
}

fn events(hwnd: HWND) -> Option<Rc<dyn WindowEvents>> {
    EVENTS.with(|e| e.borrow().get(&(hwnd.0 as isize)).cloned())
}

const MAIN_CLASS: PCWSTR = w!("LightBrowserWindow");
const POPUP_CLASS: PCWSTR = w!("LightBrowserPopup");

/// Screen rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Handle to a window owned by the UI thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window(HWND);

fn register(class: PCWSTR, style: u32) -> Result<(), String> {
    // SAFETY: registering a window class with a static name and a window
    // procedure that lives for the whole program.
    unsafe {
        let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: windows::Win32::UI::WindowsAndMessaging::WNDCLASS_STYLES(style),
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hIcon: LoadIconW(None, IDI_APPLICATION).unwrap_or_default(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: HBRUSH(GetStockObject(WHITE_BRUSH).0),
            lpszClassName: class,
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return Err(format!("RegisterClassExW failed: {}", windows::core::Error::from_thread()));
        }
    }
    Ok(())
}

impl Window {
    /// Creates and shows the main browser window.
    pub fn create_main(title: &str, events: Rc<dyn WindowEvents>) -> Result<Window, String> {
        register(MAIN_CLASS, (CS_HREDRAW | CS_VREDRAW).0)?;
        // SAFETY: standard window creation on the UI thread.
        let hwnd = unsafe {
            let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                MAIN_CLASS,
                &HSTRING::from(title),
                WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                1280,
                860,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .map_err(|e| format!("CreateWindowExW failed: {e}"))?
        };
        EVENTS.with(|e| e.borrow_mut().insert(hwnd.0 as isize, events));
        // SAFETY: valid window handle.
        let _ = unsafe { ShowWindow(hwnd, SW_SHOW) };
        Ok(Window(hwnd))
    }

    /// Creates the (hidden) popup window owned by `owner`.
    pub fn create_popup(owner: Window, events: Rc<dyn WindowEvents>) -> Result<Window, String> {
        static REGISTERED: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
        REGISTERED.get_or_init(|| register(POPUP_CLASS, CS_DROPSHADOW.0)).clone()?;
        let ex = WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
        // SAFETY: creating an owned popup window on the UI thread.
        let hwnd = unsafe {
            let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
            CreateWindowExW(
                ex,
                POPUP_CLASS,
                w!("LightBrowser popup"),
                WS_POPUP | WS_CLIPCHILDREN,
                0,
                0,
                10,
                10,
                Some(owner.0),
                None,
                Some(instance.into()),
                None,
            )
            .map_err(|e| format!("CreateWindowExW (popup) failed: {e}"))?
        };
        EVENTS.with(|e| e.borrow_mut().insert(hwnd.0 as isize, events));
        Ok(Window(hwnd))
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

    /// DPI scale factor (1.0 = 96 DPI).
    pub fn scale(self) -> f64 {
        // SAFETY: valid window handle.
        let dpi = unsafe { GetDpiForWindow(self.0) };
        if dpi == 0 {
            1.0
        } else {
            f64::from(dpi) / 96.0
        }
    }

    /// Converts a client-area point to screen coordinates.
    pub fn client_to_screen(self, x: i32, y: i32) -> (i32, i32) {
        let mut p = POINT { x, y };
        // SAFETY: valid window handle and point.
        let _ = unsafe { ClientToScreen(self.0, &mut p) };
        (p.x, p.y)
    }

    /// Work area of the monitor containing the screen point.
    pub fn work_area_at(x: i32, y: i32) -> ScreenRect {
        // SAFETY: querying monitor info into a properly sized struct.
        unsafe {
            let mon = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            if GetMonitorInfoW(mon, &mut mi).as_bool() {
                let r = mi.rcWork;
                return ScreenRect { x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top };
            }
        }
        ScreenRect { x: 0, y: 0, width: 1920, height: 1080 }
    }

    /// Shows the window at `rect` (screen coordinates), on top.
    pub fn show_at(self, rect: ScreenRect, activate: bool) {
        let flags = if activate { SWP_SHOWWINDOW } else { SWP_SHOWWINDOW | SWP_NOACTIVATE };
        // SAFETY: valid window handle.
        unsafe {
            let _ = SetWindowPos(self.0, Some(HWND_TOP), rect.x, rect.y, rect.width, rect.height, flags);
            if activate {
                let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(self.0);
            }
        }
    }

    /// Toggles `WS_EX_NOACTIVATE` (popups that need no keyboard input never
    /// take activation from the main window).
    pub fn set_activatable(self, activatable: bool) {
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE};
        // SAFETY: reading and writing the extended style of our own window.
        unsafe {
            let ex = GetWindowLongPtrW(self.0, GWL_EXSTYLE);
            let flag = WS_EX_NOACTIVATE.0 as isize;
            let new = if activatable { ex & !flag } else { ex | flag };
            if new != ex {
                SetWindowLongPtrW(self.0, GWL_EXSTYLE, new);
            }
        }
    }

    pub fn hide(self) {
        // SAFETY: valid window handle.
        let _ = unsafe { ShowWindow(self.0, SW_HIDE) };
    }

    pub fn is_visible(self) -> bool {
        // SAFETY: valid window handle.
        unsafe { IsWindowVisible(self.0) }.as_bool()
    }

    pub fn set_title(self, title: &str) {
        // SAFETY: valid window handle; HSTRING outlives the call.
        let _ = unsafe { SetWindowTextW(self.0, &HSTRING::from(title)) };
    }

    /// Starts (or restarts) timer `id` firing every `ms` milliseconds.
    pub fn set_timer(self, id: usize, ms: u32) {
        // SAFETY: timer on our own window.
        unsafe { windows::Win32::UI::WindowsAndMessaging::SetTimer(Some(self.0), id, ms, None) };
    }

    pub fn kill_timer(self, id: usize) {
        // SAFETY: timer on our own window.
        let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(self.0), id) };
    }

    /// A `Send` handle for posting `WM_APP` to this window from worker threads.
    pub fn notifier(self) -> AppNotifier {
        AppNotifier(self.0 .0 as isize)
    }

    pub fn destroy(self) {
        // SAFETY: valid window handle owned by this thread.
        let _ = unsafe { DestroyWindow(self.0) };
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let ev = events(hwnd);
    match msg {
        WM_SIZE => {
            if let Some(ev) = ev {
                let minimized = wparam.0 as u32 == SIZE_MINIMIZED;
                ev.minimized(minimized);
                if !minimized {
                    ev.resized((lparam.0 & 0xffff) as i32, ((lparam.0 >> 16) & 0xffff) as i32);
                }
            }
            LRESULT(0)
        }
        WM_MOVE | WM_MOVING => {
            if let Some(ev) = ev {
                ev.moved();
            }
            // SAFETY: default processing for messages we only observe.
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_ACTIVATE => {
            if let Some(ev) = ev {
                ev.activated((wparam.0 & 0xffff) as u32 != WA_INACTIVE);
            }
            // SAFETY: default processing.
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_DPICHANGED => {
            // Resize to the rectangle Windows suggests for the new DPI.
            // SAFETY: lparam points to a RECT for this message.
            unsafe {
                let r = &*(lparam.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            // SAFETY: lparam points to a MINMAXINFO for this message.
            unsafe {
                let mmi = &mut *(lparam.0 as *mut MINMAXINFO);
                let scale = f64::from(GetDpiForWindow(hwnd).max(96)) / 96.0;
                mmi.ptMinTrackSize = POINT { x: (480.0 * scale) as i32, y: (320.0 * scale) as i32 };
            }
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_TIMER => {
            if let Some(ev) = ev {
                ev.timer(wparam.0);
            }
            LRESULT(0)
        }
        WM_APP => {
            if let Some(ev) = ev {
                ev.app_message(wparam.0);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            if ev.is_none_or(|ev| ev.close_requested()) {
                // SAFETY: destroying our own window on its thread.
                let _ = unsafe { DestroyWindow(hwnd) };
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            let is_main = ev.is_some() && {
                // The main window is the only one without an owner.
                // SAFETY: valid handle.
                unsafe {
                    windows::Win32::UI::WindowsAndMessaging::GetWindow(
                        hwnd,
                        windows::Win32::UI::WindowsAndMessaging::GW_OWNER,
                    )
                }
                .is_err()
            };
            if let Some(ev) = ev {
                ev.destroyed();
            }
            EVENTS.with(|e| e.borrow_mut().remove(&(hwnd.0 as isize)));
            if is_main {
                // SAFETY: ends the message loop of this thread.
                unsafe { PostQuitMessage(0) };
            }
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

/// Posts `WM_APP` messages to a window from any thread.
#[derive(Debug, Clone, Copy)]
pub struct AppNotifier(isize);

impl AppNotifier {
    /// Wakes the UI thread; `WindowEvents::app_message(code)` runs there.
    pub fn notify(self, code: usize) {
        // SAFETY: PostMessageW is thread-safe; a stale handle only fails.
        let _ = unsafe {
            windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                Some(HWND(self.0 as *mut std::ffi::c_void)),
                WM_APP,
                WPARAM(code),
                LPARAM(0),
            )
        };
    }
}
