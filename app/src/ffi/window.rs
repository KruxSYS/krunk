//! Top-level windows (main frame and the popup). The message loop is CEF's
//! (`ffi::cef`), which dispatches to these window procedures as usual.

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
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, IsWindowVisible, LoadCursorW, LoadIconW,
    RegisterClassExW, SetWindowPos, SetWindowTextW, ShowWindow, CS_DROPSHADOW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT,
    HWND_TOP, IDC_ARROW, IDI_APPLICATION, MINMAXINFO, SIZE_MINIMIZED, SWP_NOACTIVATE, SWP_NOZORDER, SWP_SHOWWINDOW,
    SW_HIDE, SW_SHOW, WA_INACTIVE, WINDOW_EX_STYLE, WM_ACTIVATE, WM_APP, WM_CLOSE, WM_DESTROY, WM_DPICHANGED,
    WM_GETMINMAXINFO, WM_MOVE, WM_MOVING, WM_SIZE, WNDCLASSEXW, WS_CLIPCHILDREN, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_OVERLAPPEDWINDOW, WS_POPUP,
};

/// Callbacks from the window procedure. Implementations must tolerate
/// re-entrancy (a modal dialog pumps messages while a callback runs).
pub trait WindowEvents {
    fn resized(&self, _width: i32, _height: i32) {}
    fn minimized(&self, _minimized: bool) {}
    fn moved(&self) {}
    /// The window was activated or deactivated.
    fn activated(&self, _active: bool) {}
    /// The window itself received keyboard focus (e.g. after a click on its
    /// frame); the browser passes it on to a WebView.
    fn got_focus(&self) {}
    /// Return `true` to let the window close.
    fn close_requested(&self) -> bool {
        true
    }
    fn destroyed(&self) {}
    /// A `WM_APP` message posted to this window.
    fn app_message(&self, _code: usize) {}
    /// A timer started with [`Window::set_timer`] fired.
    fn timer(&self, _id: usize) {}
    /// The Windows session was locked (Win+L, lock screen).
    fn session_locked(&self) {}
    /// The machine is about to sleep or hibernate.
    fn suspending(&self) {}
    /// Windows switched between light and dark app mode.
    fn system_theme_changed(&self) {}
}

thread_local! {
    static EVENTS: RefCell<HashMap<isize, Rc<dyn WindowEvents>>> = RefCell::new(HashMap::new());
}

fn events(hwnd: HWND) -> Option<Rc<dyn WindowEvents>> {
    EVENTS.with(|e| e.borrow().get(&(hwnd.0 as isize)).cloned())
}

const WM_WTSSESSION_CHANGE: u32 = 0x02B1;
const WTS_SESSION_LOCK: usize = 0x7;
const WM_POWERBROADCAST: u32 = 0x0218;
const PBT_APMSUSPEND: usize = 0x4;

const MAIN_CLASS: PCWSTR = w!("KrunkWindow");
const POPUP_CLASS: PCWSTR = w!("KrunkPopup");

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
            hIcon: app_icon(instance.into(), 32),
            hIconSm: app_icon(instance.into(), 16),
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

/// Krunk's icon (`MAINICON` in Krunk.exe, set by tools/pebrand) at the
/// system's size for `base` DIP, or the stock application icon.
fn app_icon(
    instance: windows::Win32::Foundation::HINSTANCE,
    base: i32,
) -> windows::Win32::UI::WindowsAndMessaging::HICON {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, LoadImageW, HICON, IMAGE_ICON, LR_DEFAULTCOLOR, SM_CXICON, SM_CXSMICON,
    };
    // SAFETY: loading an icon resource from our own exe module by name.
    unsafe {
        let size = GetSystemMetrics(if base <= 16 { SM_CXSMICON } else { SM_CXICON });
        match LoadImageW(Some(instance), w!("MAINICON"), IMAGE_ICON, size, size, LR_DEFAULTCOLOR) {
            Ok(h) => HICON(h.0),
            Err(_) => LoadIconW(None, IDI_APPLICATION).unwrap_or_default(),
        }
    }
}

impl Window {
    /// Creates and shows the main browser window. It is shown before the
    /// engine starts so it gets the foreground (the engine's start-up would
    /// otherwise use up the process's right to activate a window); the
    /// browser attaches its events with [`Window::set_events`].
    pub fn create_main(title: &str) -> Result<Window, String> {
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
        // SAFETY: valid window handle.
        let _ = unsafe { ShowWindow(hwnd, SW_SHOW) };
        Ok(Window(hwnd))
    }

    /// Delivers this window's events to `events`.
    pub fn set_events(self, events: Rc<dyn WindowEvents>) {
        EVENTS.with(|e| e.borrow_mut().insert(self.0 .0 as isize, events));
    }

    /// Does the window still exist (it may be closed during start-up)?
    pub fn exists(self) -> bool {
        // SAFETY: IsWindow accepts any handle value.
        unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(self.0)) }.as_bool()
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
                w!("Krunk popup"),
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

    /// Restores the window if minimized and brings it to the foreground.
    pub fn bring_to_front(self) {
        use windows::Win32::UI::WindowsAndMessaging::{IsIconic, SetForegroundWindow, SW_RESTORE};
        // SAFETY: valid window handle.
        unsafe {
            if IsIconic(self.0).as_bool() {
                let _ = ShowWindow(self.0, SW_RESTORE);
            }
            let _ = SetForegroundWindow(self.0);
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

    /// Dark or light native title bar (Windows 10 20H1+ / Windows 11).
    pub fn set_dark_frame(self, dark: bool) {
        use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
        let value = windows::core::BOOL::from(dark);
        // SAFETY: valid window handle; the attribute value is a BOOL on the stack.
        let _ = unsafe {
            DwmSetWindowAttribute(
                self.0,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &value as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<windows::core::BOOL>() as u32,
            )
        };
    }

    /// Delivers `session_locked` notifications for this window.
    pub fn watch_session_lock(self) {
        use windows::Win32::System::RemoteDesktop::{WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION};
        // SAFETY: registering our own top-level window.
        let _ = unsafe { WTSRegisterSessionNotification(self.0, NOTIFY_FOR_THIS_SESSION) };
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
        windows::Win32::UI::WindowsAndMessaging::WM_SETFOCUS => {
            if let Some(ev) = ev {
                ev.got_focus();
            }
            LRESULT(0)
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
        windows::Win32::UI::WindowsAndMessaging::WM_SETTINGCHANGE => {
            // lParam names the changed setting; "ImmersiveColorSet" = app theme.
            let changed = lparam.0 != 0 && {
                // SAFETY: for WM_SETTINGCHANGE lParam is null or a NUL-terminated wide string.
                unsafe { PCWSTR(lparam.0 as *const u16).to_string() }.is_ok_and(|s| s == "ImmersiveColorSet")
            };
            if changed {
                if let Some(ev) = ev {
                    ev.system_theme_changed();
                }
            }
            // SAFETY: default processing.
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_WTSSESSION_CHANGE => {
            if wparam.0 == WTS_SESSION_LOCK {
                if let Some(ev) = ev {
                    ev.session_locked();
                }
            }
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            if wparam.0 == PBT_APMSUSPEND {
                if let Some(ev) = ev {
                    ev.suspending();
                }
            }
            LRESULT(1)
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
            // The browser ends the engine's message loop once its views
            // have closed (see `Browser::shutdown`).
            if let Some(ev) = ev {
                ev.destroyed();
            }
            EVENTS.with(|e| e.borrow_mut().remove(&(hwnd.0 as isize)));
            LRESULT(0)
        }
        // SAFETY: default processing for everything else.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Is there a top-level window of this class (in this session)?
pub fn class_exists(class: &str) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
    // SAFETY: FindWindowW with a NUL-terminated class name and no title.
    unsafe { FindWindowW(&HSTRING::from(class), PCWSTR::null()) }.is_ok_and(|h| !h.is_invalid())
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

/// Does Windows use dark mode for apps? (`AppsUseLightTheme` = 0)
pub fn system_prefers_dark() -> bool {
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let mut value: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: reading a DWORD into a properly sized local.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut std::ffi::c_void),
            Some(&mut size),
        )
    };
    r.is_ok() && value == 0
}
