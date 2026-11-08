//! Top-level windows (main frame and the popup). The message loop is CEF's
//! (`ffi::cef`), which dispatches to these window procedures as usual.

pub use lb_core::frame::Hit;
use lb_core::layout::{strip_hit, Rect, StripButton, StripLayout};
pub use lb_core::protocol::CaptionButton;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    ClientToScreen, GetMonitorInfoW, GetStockObject, MonitorFromPoint, HBRUSH, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    NULL_BRUSH,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, IsWindowVisible, LoadCursorW, LoadIconW,
    RegisterClassExW, SetWindowPos, SetWindowTextW, ShowWindow, CS_DROPSHADOW, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT,
    HWND_TOP, IDC_ARROW, IDI_APPLICATION, MINMAXINFO, SIZE_MINIMIZED, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE, SW_SHOW, WA_INACTIVE, WINDOW_EX_STYLE, WM_ACTIVATE, WM_APP,
    WM_CLOSE, WM_DESTROY, WM_DPICHANGED, WM_GETMINMAXINFO, WM_MOVE, WM_MOVING, WM_SIZE, WNDCLASSEXW, WS_CLIPCHILDREN,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_OVERLAPPEDWINDOW, WS_POPUP,
};

/// Callbacks from the window procedure. Implementations must tolerate
/// re-entrancy (a modal dialog pumps messages while a callback runs).
pub trait WindowEvents {
    fn resized(&self, _width: i32, _height: i32) {}
    /// An edge or corner drag started (`true`) or ended.
    fn size_loop(&self, _active: bool) {}
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
    /// Frameless main window: what is at this client point (physical px)?
    fn frame_hit(&self, _x: i32, _y: i32) -> Hit {
        Hit::Client
    }
    /// The mouse is over (or pressing) a caption button, or left them.
    fn caption_state(&self, _hover: Option<CaptionButton>, _pressed: bool) {}
    /// Maximized or restored.
    fn maximized_changed(&self, _maximized: bool) {}
    /// The main window lost activation to its own popup window.
    fn activation_to_popup(&self) {}
    /// Right-click on the caption (or the empty tab strip): the host's menu at this screen point.
    fn caption_menu(&self, _x: i32, _y: i32) -> bool {
        false
    }
    /// Relative mouse movement from raw input (only while it is registered
    /// with [`Window::capture_raw_mouse`]).
    fn raw_mouse(&self, _dx: i32, _dy: i32) {}
    /// Is the client point (physical pixels) on the splitter of a docked DevTools? `Some(true)`
    /// for a vertical one (DevTools on the right), `Some(false)` for a horizontal one.
    fn splitter_at(&self, _x: i32, _y: i32) -> Option<bool> {
        None
    }
    /// The splitter is being dragged to the client point; `done` when the button is released.
    fn splitter_drag(&self, _x: i32, _y: i32, _done: bool) {}
    /// The docked DevTools' splitter and strip (with its buttons) to paint and hit-test, if
    /// DevTools is docked in this window.
    fn devtools_chrome(&self) -> Option<DevToolsChrome> {
        None
    }
    /// The frame's theme is dark (what shows between its child views; 0.7.0).
    fn background_dark(&self) -> bool {
        false
    }
    /// A strip button was clicked.
    fn strip_clicked(&self, _button: StripButton) {}
}

/// What the frame paints itself between its child views: the DevTools splitter and the
/// strip above DevTools with its dock-side and close buttons (physical client pixels).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DevToolsChrome {
    pub splitter: Rect,
    pub strip: StripLayout,
    /// DevTools is docked on the right (the dock button then offers the bottom).
    pub dock_right: bool,
    pub dark: bool,
}

/// Registers a window class that only does default processing (overlays drawn with
/// `UpdateLayeredWindow` need no window procedure of their own).
pub fn register_plain_class(class: PCWSTR) -> Result<(), String> {
    register(class, 0, plain_wndproc)
}

unsafe extern "system" fn plain_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: default processing only.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// `WM_MOUSELEAVE` (from `TrackMouseEvent`).
const WM_MOUSELEAVE: u32 = 0x02A3;

/// Windows' "Animation effects" setting (the system's reduced-motion switch, which Chromium
/// maps to `prefers-reduced-motion`): true when animations are wanted.
pub fn client_area_animation() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION};
    let mut on = windows::core::BOOL(1);
    // SAFETY: a BOOL out-parameter of the documented size for this SPI.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut on as *mut _ as *mut std::ffi::c_void),
            windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    ok.is_err() || on.as_bool()
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
const APP_CLASS: PCWSTR = w!("KrunkApp");

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

type WndProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

fn register(class: PCWSTR, style: u32, proc_: WndProc) -> Result<(), String> {
    // SAFETY: registering a window class with a static name and a window
    // procedure that lives for the whole program.
    unsafe {
        let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: windows::Win32::UI::WindowsAndMessaging::WNDCLASS_STYLES(style),
            lpfnWndProc: Some(proc_),
            hInstance: instance.into(),
            hIcon: app_icon(instance.into(), 32),
            hIconSm: app_icon(instance.into(), 16),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            // No class brush: a white erase showed through between two views while switching
            // tabs (0.7.0, R7). The frame paints its own theme colour (`paint_chrome`).
            hbrBackground: HBRUSH(GetStockObject(NULL_BRUSH).0),
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
        Window::create_frame(title, None, true)
    }

    /// A browser window (frame) at `rect` (screen pixels; `None` = the default placement),
    /// shown (and activated) or left hidden. The class is registered once, so any number of
    /// frames can exist.
    pub fn create_frame(title: &str, rect: Option<ScreenRect>, show: bool) -> Result<Window, String> {
        static REGISTERED: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
        REGISTERED.get_or_init(|| register(MAIN_CLASS, (CS_HREDRAW | CS_VREDRAW).0, main_wndproc)).clone()?;
        let (x, y, width, height) = match rect {
            Some(r) => (r.x, r.y, r.width, r.height),
            None => (CW_USEDEFAULT, CW_USEDEFAULT, 1280, 860),
        };
        // SAFETY: standard window creation on the UI thread.
        let hwnd = unsafe {
            let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                MAIN_CLASS,
                &HSTRING::from(title),
                WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
                x,
                y,
                width,
                height,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .map_err(|e| format!("CreateWindowExW failed: {e}"))?
        };
        // SAFETY: valid window handle. SWP_FRAMECHANGED makes Windows ask
        // WM_NCCALCSIZE again now that the frameless handling is in place.
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
            if show {
                let _ = ShowWindow(hwnd, SW_SHOW);
            }
        }
        Ok(Window(hwnd))
    }

    /// The hidden application window: timers, `WM_APP` from worker threads, session-lock and
    /// power notifications and theme changes, which belong to the browser rather than to one
    /// of its frames. A never-shown top-level tool window (a message-only window would miss
    /// the broadcasts).
    pub fn create_app(events: Rc<dyn WindowEvents>) -> Result<Window, String> {
        static REGISTERED: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
        REGISTERED.get_or_init(|| register(APP_CLASS, 0, wndproc)).clone()?;
        // SAFETY: creating a hidden window on the UI thread.
        let hwnd = unsafe {
            let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                APP_CLASS,
                w!("Krunk"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .map_err(|e| format!("CreateWindowExW (app) failed: {e}"))?
        };
        EVENTS.with(|e| e.borrow_mut().insert(hwnd.0 as isize, events));
        Ok(Window(hwnd))
    }

    /// The restored (not maximized) rectangle and whether the window is maximized.
    pub fn placement(self) -> (ScreenRect, bool) {
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowPlacement, WINDOWPLACEMENT};
        let mut wp = WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
        // SAFETY: our own window; the struct is properly sized.
        let _ = unsafe { GetWindowPlacement(self.0, &mut wp) };
        // In full screen the frame's own placement is the saved one.
        let saved = SAVED.with(|s| s.borrow().get(&(self.0 .0 as isize)).copied());
        let wp = saved.unwrap_or(wp);
        let r = wp.rcNormalPosition;
        let maximized = wp.showCmd == windows::Win32::UI::WindowsAndMessaging::SW_SHOWMAXIMIZED.0 as u32;
        (ScreenRect { x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top }, maximized)
    }

    /// Shows a frame created hidden, maximized or not, and activates it.
    pub fn show(self, maximized: bool) {
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWMAXIMIZED;
        // SAFETY: our own window.
        unsafe {
            let _ = ShowWindow(self.0, if maximized { SW_SHOWMAXIMIZED } else { SW_SHOW });
            let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(self.0);
        }
    }

    /// Moves (and sizes) a top-level window, without activating it.
    pub fn move_to(self, rect: ScreenRect) {
        // SAFETY: our own window.
        let _ = unsafe {
            SetWindowPos(self.0, None, rect.x, rect.y, rect.width, rect.height, SWP_NOZORDER | SWP_NOACTIVATE)
        };
    }

    /// The window rectangle in screen pixels.
    pub fn rect(self) -> ScreenRect {
        let mut r = RECT::default();
        // SAFETY: valid window handle and out pointer.
        let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowRect(self.0, &mut r) };
        ScreenRect { x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top }
    }

    /// Is the window maximized?
    pub fn is_maximized(self) -> bool {
        // SAFETY: valid window handle.
        unsafe { windows::Win32::UI::WindowsAndMessaging::IsZoomed(self.0) }.as_bool()
    }

    /// Minimize, maximize/restore or close (the caption buttons).
    pub fn control(self, action: lb_core::protocol::WindowAction) {
        use lb_core::protocol::WindowAction;
        use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE};
        // SAFETY: our own window.
        unsafe {
            match action {
                WindowAction::Minimize => {
                    let _ = ShowWindow(self.0, SW_MINIMIZE);
                }
                WindowAction::Maximize => {
                    let _ = ShowWindow(self.0, if self.is_maximized() { SW_RESTORE } else { SW_MAXIMIZE });
                }
                WindowAction::Close => {
                    let _ = PostMessageW(Some(self.0), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
        }
    }

    /// Delivers raw mouse movement to [`WindowEvents::raw_mouse`] (or stops).
    pub fn capture_raw_mouse(self, on: bool) {
        use windows::Win32::UI::Input::{RegisterRawInputDevices, RAWINPUTDEVICE, RAWINPUTDEVICE_FLAGS, RIDEV_REMOVE};
        let device = RAWINPUTDEVICE {
            usUsagePage: 0x01, // generic desktop
            usUsage: 0x02,     // mouse
            dwFlags: if on { RAWINPUTDEVICE_FLAGS(0) } else { RIDEV_REMOVE },
            hwndTarget: if on { self.0 } else { HWND::default() },
        };
        // SAFETY: registering one device for our own window.
        let _ = unsafe { RegisterRawInputDevices(&[device], std::mem::size_of::<RAWINPUTDEVICE>() as u32) };
    }

    /// Full screen: no frame, covering the monitor (on) or back to the
    /// placement it had before (off).
    pub fn set_fullscreen(self, on: bool) {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, GetWindowPlacement, SetWindowLongPtrW, SetWindowPlacement, GWL_STYLE, WINDOWPLACEMENT,
            WS_CAPTION, WS_THICKFRAME,
        };
        if is_fullscreen(self.0) == on {
            return;
        }
        let frame = (WS_CAPTION | WS_THICKFRAME).0 as isize;
        // SAFETY: our own window; the placement struct is properly sized.
        unsafe {
            let style = GetWindowLongPtrW(self.0, GWL_STYLE);
            if on {
                let mut wp =
                    WINDOWPLACEMENT { length: std::mem::size_of::<WINDOWPLACEMENT>() as u32, ..Default::default() };
                if GetWindowPlacement(self.0, &mut wp).is_ok() {
                    SAVED.with(|s| s.borrow_mut().insert(self.0 .0 as isize, wp));
                }
                FULLSCREEN.with(|f| f.borrow_mut().insert(self.0 .0 as isize));
                SetWindowLongPtrW(self.0, GWL_STYLE, style & !frame);
                let mon = windows::Win32::Graphics::Gdi::MonitorFromWindow(self.0, MONITOR_DEFAULTTONEAREST);
                let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
                let _ = GetMonitorInfoW(mon, &mut mi);
                let r = mi.rcMonitor;
                let _ = SetWindowPos(
                    self.0,
                    Some(HWND_TOP),
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_FRAMECHANGED | windows::Win32::UI::WindowsAndMessaging::SWP_NOOWNERZORDER,
                );
            } else {
                FULLSCREEN.with(|f| f.borrow_mut().remove(&(self.0 .0 as isize)));
                SetWindowLongPtrW(self.0, GWL_STYLE, style | frame);
                if let Some(wp) = SAVED.with(|s| s.borrow_mut().remove(&(self.0 .0 as isize))) {
                    let _ = SetWindowPlacement(self.0, &wp);
                }
                let _ = SetWindowPos(
                    self.0,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }
    }

    /// Is this window the active (foreground) one and not minimized?
    pub fn is_active(self) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsIconic};
        // SAFETY: plain queries.
        unsafe { GetForegroundWindow() == self.0 && !IsIconic(self.0).as_bool() }
    }

    /// Opens the window's system menu (Alt+Space) at its top-left corner.
    pub fn show_system_menu(self) {
        use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, SC_KEYMENU, WM_SYSCOMMAND};
        // SAFETY: our own window; SC_KEYMENU with ' ' is what Alt+Space sends.
        let _ =
            unsafe { PostMessageW(Some(self.0), WM_SYSCOMMAND, WPARAM(SC_KEYMENU as usize), LPARAM(b' ' as isize)) };
    }

    /// Lets the frame hit-test through `root` and all its child windows
    /// (the shell browser's): where [`WindowEvents::frame_hit`] says the
    /// point is not client area, they answer `HTTRANSPARENT` and the main
    /// window decides (drag, resize, caption buttons). Call again when the
    /// browser may have created new child windows; repeats are harmless.
    pub fn pass_hit_tests_through(self, root: HWND) {
        use windows::Win32::UI::Shell::SetWindowSubclass;
        use windows::Win32::UI::WindowsAndMessaging::EnumChildWindows;
        extern "system" fn each(child: HWND, _: LPARAM) -> windows::core::BOOL {
            // SAFETY: subclassing a window of our own process and thread.
            unsafe {
                let _ = SetWindowSubclass(child, Some(transparent_subclass), SUBCLASS_ID, 0);
            }
            true.into()
        }
        // SAFETY: as above; EnumChildWindows walks all descendants.
        unsafe {
            let _ = SetWindowSubclass(root, Some(transparent_subclass), SUBCLASS_ID, 0);
            let _ = EnumChildWindows(Some(root), Some(each), LPARAM(0));
        }
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
        REGISTERED.get_or_init(|| register(POPUP_CLASS, CS_DROPSHADOW.0, wndproc)).clone()?;
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

    /// Changes the window's height, keeping its position and width.
    pub fn resize_height(self, height: i32) {
        // SAFETY: valid window handle.
        unsafe {
            let _ = SetWindowPos(
                self.0,
                None,
                0,
                0,
                self.rect().width,
                height.max(1),
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    /// The theme changed: the colour beneath the views (what a resize shows, R6) and the
    /// frame's own parts follow.
    pub fn repaint_background(self) {
        fill_background(self.0);
        self.invalidate();
    }

    /// Repaints the window's own client area (the DevTools splitter and strip).
    pub fn invalidate(self) {
        // SAFETY: valid window handle.
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(self.0), None, true);
        }
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

    /// Makes `owner` the owner of this (popup) window: it stays above that frame and is
    /// hidden with it.
    pub fn set_owner(self, owner: Window) {
        use windows::Win32::UI::WindowsAndMessaging::{SetWindowLongPtrW, GWLP_HWNDPARENT};
        // SAFETY: re-owning our own popup to our own frame.
        unsafe {
            SetWindowLongPtrW(self.0, GWLP_HWNDPARENT, owner.0 .0 as isize);
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
            // On top of our other windows even if Windows refuses the activation below.
            let _ = SetWindowPos(self.0, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            let _ = SetForegroundWindow(self.0);
        }
    }

    /// Keeps the window above every non-topmost window (while it follows a tab drag), or
    /// ends that.
    pub fn set_topmost(self, on: bool) {
        use windows::Win32::UI::WindowsAndMessaging::{HWND_NOTOPMOST, HWND_TOPMOST};
        let after = if on { HWND_TOPMOST } else { HWND_NOTOPMOST };
        // SAFETY: valid window handle; only the z-order changes.
        let _ = unsafe { SetWindowPos(self.0, Some(after), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) };
    }

    pub fn hide(self) {
        // SAFETY: valid window handle.
        let _ = unsafe { ShowWindow(self.0, SW_HIDE) };
    }

    /// Is this the foreground window (the one the keyboard goes to)?
    pub fn is_foreground(self) -> bool {
        // SAFETY: no arguments.
        unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() == self.0 }
    }

    pub fn is_minimized(self) -> bool {
        // SAFETY: valid window handle.
        unsafe { windows::Win32::UI::WindowsAndMessaging::IsIconic(self.0) }.as_bool()
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

const SUBCLASS_ID: usize = 0x4b52_4e4b; // "KRNK"
/// Timer of the main window that follows the mouse over the caption buttons.
const CAPTION_TIMER: usize = 0x4b52;

thread_local! {
    /// Per frame: the caption button under the mouse, and whether it is pressed.
    static CAPTION: RefCell<HashMap<isize, (Option<CaptionButton>, bool)>> = RefCell::new(HashMap::new());
}

fn caption_of(hwnd: HWND) -> (Option<CaptionButton>, bool) {
    CAPTION.with(|c| c.borrow().get(&(hwnd.0 as isize)).copied().unwrap_or((None, false)))
}

fn hit_code(hit: Hit) -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::{
        HTCAPTION, HTCLIENT, HTCLOSE, HTMAXBUTTON, HTMINBUTTON, HTTOP, HTTOPLEFT, HTTOPRIGHT,
    };
    match hit {
        Hit::Client => HTCLIENT,
        Hit::Caption => HTCAPTION,
        Hit::Top => HTTOP,
        Hit::TopLeft => HTTOPLEFT,
        Hit::TopRight => HTTOPRIGHT,
        Hit::Minimize => HTMINBUTTON,
        Hit::Maximize => HTMAXBUTTON,
        Hit::Close => HTCLOSE,
    }
}

fn caption_button(ht: usize) -> Option<CaptionButton> {
    use windows::Win32::UI::WindowsAndMessaging::{HTCLOSE, HTMAXBUTTON, HTMINBUTTON};
    match ht as u32 {
        HTMINBUTTON => Some(CaptionButton::Minimize),
        HTMAXBUTTON => Some(CaptionButton::Maximize),
        HTCLOSE => Some(CaptionButton::Close),
        _ => None,
    }
}

fn set_caption(hwnd: HWND, ev: &Option<Rc<dyn WindowEvents>>, hover: Option<CaptionButton>, pressed: bool) {
    let before = CAPTION.with(|c| c.borrow_mut().insert(hwnd.0 as isize, (hover, pressed))).unwrap_or((None, false));
    if before == (hover, pressed) {
        return;
    }
    // The mouse is over the shell's child windows (which only pass hit tests
    // through), so TrackMouseEvent reports leaving at once and is no use:
    // a short timer checks whether the mouse is still over the button.
    // SAFETY: timers on our own window.
    unsafe {
        if hover.is_some() {
            windows::Win32::UI::WindowsAndMessaging::SetTimer(Some(hwnd), CAPTION_TIMER, 50, None);
        } else {
            let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(hwnd), CAPTION_TIMER);
        }
    }
    if let Some(ev) = ev {
        ev.caption_state(hover, pressed);
    }
}

/// The caption button under the mouse right now, if any.
fn caption_under_cursor(hwnd: HWND) -> Option<CaptionButton> {
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, WindowFromPoint};
    let mut p = POINT::default();
    // SAFETY: plain queries; the point is ours.
    unsafe {
        GetCursorPos(&mut p).ok()?;
        // Another window (a menu, another app) on top of ours: not hovering.
        let under = WindowFromPoint(p);
        if windows::Win32::UI::WindowsAndMessaging::GetAncestor(under, windows::Win32::UI::WindowsAndMessaging::GA_ROOT)
            != hwnd
        {
            return None;
        }
        let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut p);
    }
    match events(hwnd)?.frame_hit(p.x, p.y) {
        Hit::Minimize => Some(CaptionButton::Minimize),
        Hit::Maximize => Some(CaptionButton::Maximize),
        Hit::Close => Some(CaptionButton::Close),
        _ => None,
    }
}

/// Opens the system menu at a screen point and runs the chosen command.
fn system_menu_at(hwnd: HWND, x: i32, y: i32) {
    use windows::Win32::UI::WindowsAndMessaging::{
        EnableMenuItem, GetSystemMenu, PostMessageW, SetMenuDefaultItem, TrackPopupMenu, MF_BYCOMMAND, MF_ENABLED,
        MF_GRAYED, SC_CLOSE, SC_MAXIMIZE, SC_MINIMIZE, SC_MOVE, SC_RESTORE, SC_SIZE, TPM_RETURNCMD, TPM_RIGHTBUTTON,
        WM_SYSCOMMAND,
    };
    // SAFETY: our own window's system menu; the command is posted back to it.
    unsafe {
        let menu = GetSystemMenu(hwnd, false);
        if menu.is_invalid() {
            return;
        }
        let max = windows::Win32::UI::WindowsAndMessaging::IsZoomed(hwnd).as_bool();
        let state = |on: bool| if on { MF_BYCOMMAND | MF_ENABLED } else { MF_BYCOMMAND | MF_GRAYED };
        let _ = EnableMenuItem(menu, SC_RESTORE, state(max));
        let _ = EnableMenuItem(menu, SC_MOVE, state(!max));
        let _ = EnableMenuItem(menu, SC_SIZE, state(!max));
        let _ = EnableMenuItem(menu, SC_MINIMIZE, state(true));
        let _ = EnableMenuItem(menu, SC_MAXIMIZE, state(!max));
        let _ = EnableMenuItem(menu, SC_CLOSE, state(true));
        let _ = SetMenuDefaultItem(menu, SC_CLOSE, 0);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, x, y, None, hwnd, None);
        if cmd.0 != 0 {
            let _ = PostMessageW(Some(hwnd), WM_SYSCOMMAND, WPARAM(cmd.0 as usize), LPARAM(0));
        }
    }
}

/// Thickness of the resize border Windows keeps around a sizable window
/// (outside the visible frame on Windows 10/11).
fn frame_thickness(hwnd: HWND) -> i32 {
    use windows::Win32::UI::HiDpi::GetSystemMetricsForDpi;
    use windows::Win32::UI::WindowsAndMessaging::{SM_CXPADDEDBORDER, SM_CYFRAME};
    // SAFETY: plain queries.
    unsafe {
        let dpi = GetDpiForWindow(hwnd).max(96);
        GetSystemMetricsForDpi(SM_CYFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi)
    }
}

/// The main window has no title bar: the tab strip (the shell view) is the
/// top of the window. Left, right and bottom keep the native resize border;
/// the caption and the top border are removed in `WM_NCCALCSIZE`, and
/// `WM_NCHITTEST` asks the browser what is under the mouse (see
/// `lb_core::frame`). Mouse clicks on the caption buttons are handled here
/// (Windows would draw classic buttons otherwise); returning `HTMAXBUTTON`
/// is what gives the Windows 11 Snap Layouts flyout.
extern "system" fn main_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{
        HTCLIENT, NCCALCSIZE_PARAMS, SC_KEYMENU, WM_NCCALCSIZE, WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN,
        WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WM_SYSCOMMAND,
    };
    match msg {
        // Alt pressed and released alone: there is no menu bar, so the menu mode Windows
        // enters would be invisible and swallow the next key (Alt+Space still opens the
        // system menu: it comes with ' ').
        WM_SYSCOMMAND if (wparam.0 & 0xFFF0) == SC_KEYMENU as usize && lparam.0 == 0 => LRESULT(0),
        WM_NCCALCSIZE if wparam.0 != 0 => {
            // SAFETY: with wParam TRUE, lParam points to NCCALCSIZE_PARAMS.
            unsafe {
                let params = &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS);
                let top = params.rgrc[0].top;
                let r = DefWindowProcW(hwnd, msg, wparam, lparam);
                params.rgrc[0].top = top;
                // Maximized, the window hangs over the monitor by its resize
                // border on every side: keep the client area on screen.
                if windows::Win32::UI::WindowsAndMessaging::IsZoomed(hwnd).as_bool() && !is_fullscreen(hwnd) {
                    params.rgrc[0].top += frame_thickness(hwnd);
                }
                r
            }
        }
        WM_NCHITTEST => {
            // SAFETY: default processing gives the native side and bottom borders.
            let native = unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            if native.0 as u32 != HTCLIENT {
                return native;
            }
            let ev = events(hwnd);
            let (sx, sy) = ((lparam.0 & 0xffff) as i16 as i32, ((lparam.0 >> 16) & 0xffff) as i16 as i32);
            let mut p = POINT { x: sx, y: sy };
            // SAFETY: valid window handle and point.
            let _ = unsafe { windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut p) };
            let hit = ev.map(|e| e.frame_hit(p.x, p.y)).unwrap_or(Hit::Client);
            LRESULT(hit_code(hit) as isize)
        }
        WM_NCMOUSEMOVE => {
            let ev = events(hwnd);
            let hover = caption_button(wparam.0);
            let pressed = caption_of(hwnd).1 && hover.is_some();
            set_caption(hwnd, &ev, hover, pressed);
            // SAFETY: default processing.
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_NCMOUSELEAVE => {
            if caption_under_cursor(hwnd).is_none() {
                set_caption(hwnd, &events(hwnd), None, false);
            }
            // SAFETY: default processing.
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        windows::Win32::UI::WindowsAndMessaging::WM_TIMER if wparam.0 == CAPTION_TIMER => {
            let now = caption_under_cursor(hwnd);
            let (hover, pressed) = caption_of(hwnd);
            if now != hover {
                set_caption(hwnd, &events(hwnd), now, pressed && now.is_some());
            }
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_NCRBUTTONUP
            if wparam.0 as u32 == windows::Win32::UI::WindowsAndMessaging::HTCAPTION =>
        {
            let (x, y) = ((lparam.0 & 0xffff) as i16 as i32, ((lparam.0 >> 16) & 0xffff) as i16 as i32);
            // Krunk's own menu (0.6.0); the system menu stays on Alt+Space.
            if !events(hwnd).is_some_and(|ev| ev.caption_menu(x, y)) {
                system_menu_at(hwnd, x, y);
            }
            LRESULT(0)
        }
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK if caption_button(wparam.0).is_some() => {
            set_caption(hwnd, &events(hwnd), caption_button(wparam.0), true);
            LRESULT(0)
        }
        // Edge and corner drags run without the system's modal size loop: inside that loop
        // the engine runs no tasks (CEF only allows them for its own windows), so pages would
        // freeze until the mouse is released. This loop resizes on every mouse move instead.
        WM_NCLBUTTONDOWN if (HTLEFT..=HTBOTTOMRIGHT).contains(&(wparam.0 as u32)) && !Window(hwnd).is_maximized() => {
            sizing_start(hwnd, wparam.0 as u32);
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_MOUSEMOVE if sizing_active(hwnd) => {
            sizing_move(hwnd);
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONUP if sizing_active(hwnd) => {
            sizing_end(hwnd, false);
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_CAPTURECHANGED if sizing_active(hwnd) => {
            sizing_end(hwnd, false);
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_KEYDOWN
            if sizing_active(hwnd) && wparam.0 as u16 == windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE.0 =>
        {
            sizing_end(hwnd, true);
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_SETCURSOR if sizing_active(hwnd) => {
            sizing_cursor(hwnd);
            LRESULT(1)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_TIMER if wparam.0 == SIZING_TIMER => {
            use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE};
            // SAFETY: a plain key-state query.
            // Bit 15: down now; bit 0: pressed since the last call (a tap between two ticks).
            let state = unsafe { GetAsyncKeyState(i32::from(VK_ESCAPE.0)) } as u16;
            if sizing_active(hwnd) && state & 0x8001 != 0 {
                sizing_end(hwnd, true);
            }
            LRESULT(0)
        }
        WM_NCLBUTTONUP if caption_button(wparam.0).is_some() => {
            let button = caption_button(wparam.0);
            let (_, pressed) = caption_of(hwnd);
            set_caption(hwnd, &events(hwnd), button, false);
            if pressed {
                use lb_core::protocol::WindowAction;
                let action = match button {
                    Some(CaptionButton::Minimize) => WindowAction::Minimize,
                    Some(CaptionButton::Maximize) => WindowAction::Maximize,
                    _ => WindowAction::Close,
                };
                // Minimizing or restoring moves the window away from the mouse.
                if action != WindowAction::Close {
                    set_caption(hwnd, &events(hwnd), None, false);
                }
                Window(hwnd).control(action);
            }
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_INPUT => {
            use windows::Win32::UI::Input::{
                GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTHEADER, RID_INPUT, RIM_TYPEMOUSE,
            };
            let mut raw = RAWINPUT::default();
            let mut size = std::mem::size_of::<RAWINPUT>() as u32;
            // SAFETY: lParam is the HRAWINPUT of this message; the buffer is a RAWINPUT.
            let n = unsafe {
                GetRawInputData(
                    HRAWINPUT(lparam.0 as *mut std::ffi::c_void),
                    RID_INPUT,
                    Some(&mut raw as *mut RAWINPUT as *mut std::ffi::c_void),
                    &mut size,
                    std::mem::size_of::<RAWINPUTHEADER>() as u32,
                )
            };
            if n != u32::MAX && raw.header.dwType == RIM_TYPEMOUSE.0 {
                // SAFETY: the header says this is mouse data.
                let m = unsafe { raw.data.mouse };
                // MOUSE_MOVE_ABSOLUTE (tablets, remote desktop) is not a delta.
                if m.usFlags.0 & 0x01 == 0 && (m.lLastX != 0 || m.lLastY != 0) {
                    if let Some(ev) = events(hwnd) {
                        ev.raw_mouse(m.lLastX, m.lLastY);
                    }
                }
            }
            // SAFETY: default processing (cleans up the input).
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        // The splitter between a page and its docked DevTools is the frame's own client area
        // (the gap between the two child windows): resize cursor, and dragging with capture.
        windows::Win32::UI::WindowsAndMessaging::WM_SETCURSOR
            if (lparam.0 & 0xffff) as u32 == HTCLIENT && HWND(wparam.0 as *mut std::ffi::c_void) == hwnd =>
        {
            use windows::Win32::UI::WindowsAndMessaging::{
                GetCursorPos, LoadCursorW, SetCursor, IDC_SIZENS, IDC_SIZEWE,
            };
            let mut p = POINT::default();
            // SAFETY: plain queries on our own window.
            unsafe {
                let _ = GetCursorPos(&mut p);
                let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut p);
            }
            match events(hwnd).and_then(|e| e.splitter_at(p.x, p.y)) {
                Some(vertical) => {
                    // SAFETY: a stock cursor.
                    unsafe {
                        let _ = SetCursor(LoadCursorW(None, if vertical { IDC_SIZEWE } else { IDC_SIZENS }).ok());
                    }
                    LRESULT(1)
                }
                // SAFETY: default processing.
                None => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
            }
        }
        // The frame's own client area is the DevTools splitter and strip; it paints them
        // (no background erase: the children cover the rest, and erasing would flash).
        windows::Win32::UI::WindowsAndMessaging::WM_ERASEBKGND => LRESULT(1),
        windows::Win32::UI::WindowsAndMessaging::WM_PAINT => {
            paint_chrome(hwnd);
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_MOUSEMOVE
            if !SPLITTING.with(|s| s.borrow().contains(&(hwnd.0 as isize))) =>
        {
            let (x, y) = ((lparam.0 & 0xffff) as i16 as i32, ((lparam.0 >> 16) & 0xffff) as i16 as i32);
            let hover = events(hwnd).and_then(|e| e.devtools_chrome()).and_then(|c| strip_hit(&c.strip, x, y));
            let key = hwnd.0 as isize;
            let (was, pressed) = STRIP.with(|s| s.borrow().get(&key).copied()).unwrap_or((None, false));
            if !pressed && was != hover {
                STRIP.with(|s| s.borrow_mut().insert(key, (hover, false)));
                invalidate_strip(hwnd);
                if hover.is_some() {
                    track_leave(hwnd);
                }
            }
            // SAFETY: default processing.
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_MOUSELEAVE => {
            let key = hwnd.0 as isize;
            if STRIP.with(|s| s.borrow().get(&key).is_some_and(|(h, p)| h.is_some() && !p)) {
                STRIP.with(|s| s.borrow_mut().insert(key, (None, false)));
                invalidate_strip(hwnd);
            }
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONDOWN if strip_button_at(hwnd, lparam).is_some() => {
            let b = strip_button_at(hwnd, lparam);
            STRIP.with(|s| s.borrow_mut().insert(hwnd.0 as isize, (b, true)));
            // SAFETY: capturing the mouse for our own window until the release.
            unsafe { windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(hwnd) };
            invalidate_strip(hwnd);
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONUP
        | windows::Win32::UI::WindowsAndMessaging::WM_CAPTURECHANGED
            if STRIP.with(|s| s.borrow().get(&(hwnd.0 as isize)).is_some_and(|(_, p)| *p)) =>
        {
            let key = hwnd.0 as isize;
            let (pressed_on, _) = STRIP.with(|s| s.borrow_mut().remove(&key)).unwrap_or((None, false));
            if msg == windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONUP {
                // SAFETY: releasing our own capture.
                let _ = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture() };
                let under = strip_button_at(hwnd, lparam);
                if let (Some(b), true) = (pressed_on, under == pressed_on) {
                    if let Some(ev) = events(hwnd) {
                        ev.strip_clicked(b);
                    }
                }
            }
            invalidate_strip(hwnd);
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONDOWN => {
            let (x, y) = ((lparam.0 & 0xffff) as i16 as i32, ((lparam.0 >> 16) & 0xffff) as i16 as i32);
            if let Some(ev) = events(hwnd).filter(|e| e.splitter_at(x, y).is_some()) {
                SPLITTING.with(|s| s.borrow_mut().insert(hwnd.0 as isize));
                // SAFETY: capturing the mouse for our own window during the drag.
                unsafe { windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(hwnd) };
                ev.splitter_drag(x, y, false);
            }
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_MOUSEMOVE
            if SPLITTING.with(|s| s.borrow().contains(&(hwnd.0 as isize))) =>
        {
            let (x, y) = ((lparam.0 & 0xffff) as i16 as i32, ((lparam.0 >> 16) & 0xffff) as i16 as i32);
            if let Some(ev) = events(hwnd) {
                ev.splitter_drag(x, y, false);
            }
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONUP
        | windows::Win32::UI::WindowsAndMessaging::WM_CAPTURECHANGED
            if SPLITTING.with(|s| s.borrow().contains(&(hwnd.0 as isize))) =>
        {
            SPLITTING.with(|s| s.borrow_mut().remove(&(hwnd.0 as isize)));
            let (x, y) = if msg == windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONUP {
                ((lparam.0 & 0xffff) as i16 as i32, ((lparam.0 >> 16) & 0xffff) as i16 as i32)
            } else {
                let mut p = POINT::default();
                // SAFETY: plain queries on our own window.
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p);
                    let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut p);
                }
                (p.x, p.y)
            };
            if msg == windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONUP {
                // SAFETY: releasing our own capture.
                let _ = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture() };
            }
            if let Some(ev) = events(hwnd) {
                ev.splitter_drag(x, y, true);
            }
            LRESULT(0)
        }
        WM_SIZE => {
            if wparam.0 as u32 != SIZE_MINIMIZED {
                fill_background(hwnd);
            }
            let maximized = wparam.0 as u32 == windows::Win32::UI::WindowsAndMessaging::SIZE_MAXIMIZED;
            let key = hwnd.0 as isize;
            let was = MAXIMIZED.with(|m| {
                let mut m = m.borrow_mut();
                let was = m.contains(&key);
                if maximized {
                    m.insert(key);
                } else {
                    m.remove(&key);
                }
                was
            });
            let r = wndproc(hwnd, msg, wparam, lparam);
            if was != maximized && wparam.0 as u32 != SIZE_MINIMIZED {
                if let Some(ev) = events(hwnd) {
                    ev.maximized_changed(maximized);
                }
            }
            r
        }
        _ => wndproc(hwnd, msg, wparam, lparam),
    }
}

/// The smallest window (DIP), also WM_GETMINMAXINFO's answer.
const MIN_WIDTH_DIP: f64 = 480.0;
const MIN_HEIGHT_DIP: f64 = 320.0;

/// An edge or corner drag in progress: the hit code, the window rect and the cursor at the press.
struct Sizing {
    edge: u32,
    start: RECT,
    cursor: POINT,
}

/// While an edge drag runs, this timer polls Esc (the keyboard stays with the page).
const SIZING_TIMER: usize = 0x5a1;

thread_local! {
    static SIZING: RefCell<HashMap<isize, Sizing>> = RefCell::new(HashMap::new());
}

use windows::Win32::UI::WindowsAndMessaging::{
    HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT,
};

fn sizing_active(hwnd: HWND) -> bool {
    SIZING.with(|s| s.borrow().contains_key(&(hwnd.0 as isize)))
}

fn sizing_start(hwnd: HWND, edge: u32) {
    use windows::Win32::UI::Input::KeyboardAndMouse::SetCapture;
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetWindowRect, SetTimer};
    let (mut start, mut cursor) = (RECT::default(), POINT::default());
    // SAFETY: plain queries on our own window; capture and the timer end in sizing_end.
    unsafe {
        let _ = GetWindowRect(hwnd, &mut start);
        let _ = GetCursorPos(&mut cursor);
        SetCapture(hwnd);
        SetTimer(Some(hwnd), SIZING_TIMER, 50, None);
    }
    SIZING.with(|s| s.borrow_mut().insert(hwnd.0 as isize, Sizing { edge, start, cursor }));
    if let Some(ev) = events(hwnd) {
        ev.size_loop(true);
    }
    sizing_cursor(hwnd);
}

fn sizing_move(hwnd: HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER};
    let Some((edge, start, at)) =
        SIZING.with(|s| s.borrow().get(&(hwnd.0 as isize)).map(|z| (z.edge, z.start, z.cursor)))
    else {
        return;
    };
    let mut cursor = POINT::default();
    // SAFETY: plain query.
    unsafe {
        let _ = GetCursorPos(&mut cursor);
    }
    let (dx, dy) = (cursor.x - at.x, cursor.y - at.y);
    let scale = Window(hwnd).scale();
    let (min_w, min_h) = ((MIN_WIDTH_DIP * scale) as i32, (MIN_HEIGHT_DIP * scale) as i32);
    let mut r = start;
    let left = matches!(edge, HTLEFT | HTTOPLEFT | HTBOTTOMLEFT);
    let right = matches!(edge, HTRIGHT | HTTOPRIGHT | HTBOTTOMRIGHT);
    let top = matches!(edge, HTTOP | HTTOPLEFT | HTTOPRIGHT);
    let bottom = matches!(edge, HTBOTTOM | HTBOTTOMLEFT | HTBOTTOMRIGHT);
    if left {
        r.left = (start.left + dx).min(start.right - min_w);
    }
    if right {
        r.right = (start.right + dx).max(start.left + min_w);
    }
    if top {
        r.top = (start.top + dy).min(start.bottom - min_h);
    }
    if bottom {
        r.bottom = (start.bottom + dy).max(start.top + min_h);
    }
    // SAFETY: moving our own window; WM_SIZE lays the children out.
    unsafe {
        let _ =
            SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

fn sizing_end(hwnd: HWND, cancel: bool) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetCapture, ReleaseCapture};
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER};
    let Some(z) = SIZING.with(|s| s.borrow_mut().remove(&(hwnd.0 as isize))) else { return };
    // SAFETY: releasing our own capture; restoring our own rect and the focus we moved.
    unsafe {
        if GetCapture() == hwnd {
            let _ = ReleaseCapture();
        }
        if cancel {
            let r = z.start;
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
        let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(hwnd), SIZING_TIMER);
    }
    if let Some(ev) = events(hwnd) {
        ev.size_loop(false);
    }
}

fn sizing_cursor(hwnd: HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{
        LoadCursorW, SetCursor, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE, IDC_SIZEWE,
    };
    let Some(edge) = SIZING.with(|s| s.borrow().get(&(hwnd.0 as isize)).map(|z| z.edge)) else { return };
    let id = match edge {
        HTLEFT | HTRIGHT => IDC_SIZEWE,
        HTTOP | HTBOTTOM => IDC_SIZENS,
        HTTOPLEFT | HTBOTTOMRIGHT => IDC_SIZENWSE,
        _ => IDC_SIZENESW,
    };
    // SAFETY: system cursors.
    unsafe {
        if let Ok(c) = LoadCursorW(None, id) {
            SetCursor(Some(c));
        }
    }
}

thread_local! {
    /// Frames (by handle) that are maximized now.
    static MAXIMIZED: RefCell<std::collections::HashSet<isize>> = RefCell::new(std::collections::HashSet::new());
    /// Frames shown full screen, and the placement each had before.
    static FULLSCREEN: RefCell<std::collections::HashSet<isize>> = RefCell::new(std::collections::HashSet::new());
    static SAVED: RefCell<HashMap<isize, windows::Win32::UI::WindowsAndMessaging::WINDOWPLACEMENT>> =
        RefCell::new(HashMap::new());
    /// Frames whose DevTools splitter is being dragged.
    static SPLITTING: RefCell<std::collections::HashSet<isize>> = RefCell::new(std::collections::HashSet::new());
    /// Per frame: the DevTools strip button under the mouse, and whether it is pressed.
    static STRIP: RefCell<HashMap<isize, (Option<StripButton>, bool)>> = RefCell::new(HashMap::new());
}

/// The strip button at the mouse position of a mouse message's lParam.
fn strip_button_at(hwnd: HWND, lparam: LPARAM) -> Option<StripButton> {
    let (x, y) = ((lparam.0 & 0xffff) as i16 as i32, ((lparam.0 >> 16) & 0xffff) as i16 as i32);
    events(hwnd).and_then(|e| e.devtools_chrome()).and_then(|c| strip_hit(&c.strip, x, y))
}

fn invalidate_strip(hwnd: HWND) {
    if let Some(c) = events(hwnd).and_then(|e| e.devtools_chrome()) {
        let r = c.strip.strip;
        let rect = RECT { left: r.x, top: r.y, right: r.x + r.width, bottom: r.y + r.height };
        // SAFETY: valid window handle and rect.
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), Some(&rect), false);
        }
    }
}

fn track_leave(hwnd: HWND) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
    let mut t = TRACKMOUSEEVENT {
        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    // SAFETY: a filled TRACKMOUSEEVENT for our own window.
    let _ = unsafe { TrackMouseEvent(&mut t) };
}

/// The pages' --bg (ui/shared/common.css): what the frame shows between and beneath its views.
fn frame_bg(hwnd: HWND) -> windows::Win32::Foundation::COLORREF {
    let ev = events(hwnd);
    let dark = ev
        .as_ref()
        .and_then(|e| e.devtools_chrome())
        .map_or_else(|| ev.as_ref().is_some_and(|e| e.background_dark()), |c| c.dark);
    let rgb = |r: u32, g: u32, b: u32| windows::Win32::Foundation::COLORREF(r | (g << 8) | (b << 16));
    if dark {
        rgb(31, 32, 35)
    } else {
        rgb(243, 244, 246)
    }
}

/// Fills the whole client area with the theme colour, beneath the child views too (0.7.2, R6).
///
/// The engine draws its views with DirectComposition (`WS_EX_NOREDIRECTIONBITMAP`): while a view
/// is resized and has no frame at the new size yet, what shows there is the frame's own surface
/// under it. The frame never painted beneath its children (they clip its `WM_PAINT`), and the
/// surface made for a new window size is white: that was the white seen while resizing. A
/// cache DC without `DCX_CLIPCHILDREN` reaches beneath the children; the composed views on top
/// hide it wherever they have content.
fn fill_background(hwnd: HWND) {
    use windows::Win32::Graphics::Gdi::{
        CreateSolidBrush, DeleteObject, FillRect, GetDCEx, ReleaseDC, DCX_CACHE, HGDIOBJ,
    };
    let mut rc = RECT::default();
    // SAFETY: a cache DC of our own window, released here; the brush is deleted.
    unsafe {
        let _ = GetClientRect(hwnd, &mut rc);
        let dc = GetDCEx(Some(hwnd), None, DCX_CACHE);
        if dc.is_invalid() {
            return;
        }
        let brush = CreateSolidBrush(frame_bg(hwnd));
        let _ = FillRect(dc, &rc, brush);
        let _ = DeleteObject(HGDIOBJ(brush.0));
        ReleaseDC(Some(hwnd), dc);
    }
}

/// Paints the DevTools splitter and strip (`WM_PAINT` of a frame). Everything else in the
/// client area is covered by child windows.
fn paint_chrome(hwnd: HWND) {
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, CreatePen, CreateSolidBrush, DeleteObject, EndPaint, FillRect, LineTo, MoveToEx, Rectangle,
        RoundRect, SelectObject, HGDIOBJ, PAINTSTRUCT, PS_SOLID,
    };
    let mut ps = PAINTSTRUCT::default();
    // SAFETY: BeginPaint/EndPaint pair on our own window; GDI objects are released.
    unsafe {
        let dc = BeginPaint(hwnd, &mut ps);
        let ev = events(hwnd);
        let chrome = ev.as_ref().and_then(|e| e.devtools_chrome());
        // The theme, also without DevTools: this is what shows between two views (R7).
        let dark = chrome.map_or_else(|| ev.as_ref().is_some_and(|e| e.background_dark()), |c| c.dark);
        let rgb = |r: u32, g: u32, b: u32| windows::Win32::Foundation::COLORREF(r | (g << 8) | (b << 16));
        let bg_brush = CreateSolidBrush(frame_bg(hwnd));
        let _ = FillRect(dc, &ps.rcPaint, bg_brush);
        let _ = DeleteObject(HGDIOBJ(bg_brush.0));
        if let Some(c) = chrome {
            let (hover, pressed) = STRIP.with(|s| s.borrow().get(&(hwnd.0 as isize)).copied()).unwrap_or((None, false));
            let fg = if dark { rgb(232, 234, 237) } else { rgb(60, 64, 67) };
            let hover_bg = if dark { rgb(60, 60, 64) } else { rgb(218, 220, 224) };
            let pressed_bg = if dark { rgb(80, 80, 86) } else { rgb(200, 202, 206) };
            for (button, r) in [(StripButton::Dock, c.strip.dock), (StripButton::Close, c.strip.close)] {
                if hover == Some(button) {
                    let brush = CreateSolidBrush(if pressed { pressed_bg } else { hover_bg });
                    let pen = CreatePen(PS_SOLID, 1, if pressed { pressed_bg } else { hover_bg });
                    let ob = SelectObject(dc, HGDIOBJ(brush.0));
                    let op = SelectObject(dc, HGDIOBJ(pen.0));
                    let _ = RoundRect(dc, r.x, r.y, r.x + r.width, r.y + r.height, r.width / 4, r.width / 4);
                    SelectObject(dc, ob);
                    SelectObject(dc, op);
                    let _ = DeleteObject(HGDIOBJ(brush.0));
                    let _ = DeleteObject(HGDIOBJ(pen.0));
                }
                // Glyphs on a 20-unit grid scaled to the button.
                let u = |v: i32| v * r.width / 20;
                let pen = CreatePen(PS_SOLID, (r.width / 14).max(1), fg);
                let op = SelectObject(dc, HGDIOBJ(pen.0));
                match button {
                    StripButton::Close => {
                        let _ = MoveToEx(dc, r.x + u(6), r.y + u(6), None);
                        let _ = LineTo(dc, r.x + u(14) + 1, r.y + u(14) + 1);
                        let _ = MoveToEx(dc, r.x + u(14), r.y + u(6), None);
                        let _ = LineTo(dc, r.x + u(6) - 1, r.y + u(14) + 1);
                    }
                    StripButton::Dock => {
                        // An outlined window with the target side filled.
                        let hollow =
                            windows::Win32::Graphics::Gdi::GetStockObject(windows::Win32::Graphics::Gdi::NULL_BRUSH);
                        let ob = SelectObject(dc, hollow);
                        let _ = Rectangle(dc, r.x + u(4), r.y + u(5), r.x + u(16) + 1, r.y + u(15) + 1);
                        SelectObject(dc, ob);
                        let fill = CreateSolidBrush(fg);
                        let side = if c.dock_right {
                            // Docked right: the button offers the bottom.
                            RECT { left: r.x + u(4), top: r.y + u(12), right: r.x + u(16) + 1, bottom: r.y + u(15) + 1 }
                        } else {
                            RECT { left: r.x + u(12), top: r.y + u(5), right: r.x + u(16) + 1, bottom: r.y + u(15) + 1 }
                        };
                        let _ = FillRect(dc, &side, fill);
                        let _ = DeleteObject(HGDIOBJ(fill.0));
                    }
                }
                SelectObject(dc, op);
                let _ = DeleteObject(HGDIOBJ(pen.0));
            }
        }
        let _ = EndPaint(hwnd, &ps);
    }
}

/// Is `hwnd` shown full screen?
fn is_fullscreen(hwnd: HWND) -> bool {
    FULLSCREEN.with(|f| f.borrow().contains(&(hwnd.0 as isize)))
}

/// Subclass of the shell browser's windows: points that are not client area
/// (tab-row background, caption buttons, top resize band) fall through to
/// the main window's `WM_NCHITTEST`.
extern "system" fn transparent_subclass(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GA_ROOT, HTTRANSPARENT, WM_NCDESTROY, WM_NCHITTEST};
    if msg == WM_NCHITTEST {
        // SAFETY: plain window queries on our own windows.
        let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
        if let Some(ev) = events(root) {
            let (sx, sy) = ((lparam.0 & 0xffff) as i16 as i32, ((lparam.0 >> 16) & 0xffff) as i16 as i32);
            let mut p = POINT { x: sx, y: sy };
            // SAFETY: valid window handle and point.
            let _ = unsafe { windows::Win32::Graphics::Gdi::ScreenToClient(root, &mut p) };
            if ev.frame_hit(p.x, p.y) != Hit::Client {
                return LRESULT(HTTRANSPARENT as isize);
            }
        }
    }
    if msg == WM_NCDESTROY {
        // SAFETY: removing our own subclass from a window being destroyed.
        unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(transparent_subclass), SUBCLASS_ID);
        }
    }
    // SAFETY: default subclass processing.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
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
                let active = (wparam.0 & 0xffff) as u32 != WA_INACTIVE;
                // lParam: the window being activated. Our own popup taking
                // activation (it is created lazily) is not the user leaving.
                if !active && is_class(HWND(lparam.0 as *mut std::ffi::c_void), POPUP_CLASS) {
                    ev.activation_to_popup();
                } else {
                    ev.activated(active);
                }
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
                mmi.ptMinTrackSize = POINT { x: (MIN_WIDTH_DIP * scale) as i32, y: (MIN_HEIGHT_DIP * scale) as i32 };
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
            let key = hwnd.0 as isize;
            EVENTS.with(|e| e.borrow_mut().remove(&key));
            MAXIMIZED.with(|m| m.borrow_mut().remove(&key));
            FULLSCREEN.with(|f| f.borrow_mut().remove(&key));
            SAVED.with(|s| s.borrow_mut().remove(&key));
            CAPTION.with(|c| c.borrow_mut().remove(&key));
            LRESULT(0)
        }
        // SAFETY: default processing for everything else.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Is `hwnd` a window of this class?
fn is_class(hwnd: HWND, class: PCWSTR) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::GetClassNameW;
    if hwnd.is_invalid() {
        return false;
    }
    let mut buf = [0u16; 64];
    // SAFETY: a local buffer of the given size.
    let n = unsafe { GetClassNameW(hwnd, &mut buf) } as usize;
    // SAFETY: our class names are static NUL-terminated strings.
    let want = unsafe { class.as_wide() };
    n > 0 && buf[..n] == *want
}

/// The work area of every monitor (screen pixels), the primary one first.
pub fn monitor_work_areas() -> Vec<ScreenRect> {
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
    extern "system" fn each(mon: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> windows::core::BOOL {
        // SAFETY: `data` is the Vec below, alive for the whole call; the struct is sized.
        unsafe {
            let out = &mut *(data.0 as *mut Vec<(bool, ScreenRect)>);
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            if GetMonitorInfoW(mon, &mut mi).as_bool() {
                let r = mi.rcWork;
                let primary = mi.dwFlags & 1 != 0; // MONITORINFOF_PRIMARY
                out.push((
                    primary,
                    ScreenRect { x: r.left, y: r.top, width: r.right - r.left, height: r.bottom - r.top },
                ));
            }
        }
        true.into()
    }
    let mut found: Vec<(bool, ScreenRect)> = Vec::new();
    // SAFETY: the callback only appends to `found`.
    let _ = unsafe { EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut found as *mut _ as isize)) };
    found.sort_by_key(|(primary, _)| !primary);
    found.into_iter().map(|(_, r)| r).collect()
}

/// `windows` sorted by z-order, topmost first (hidden ones left out).
pub fn in_z_order(windows: &[Window]) -> Vec<Window> {
    use windows::Win32::UI::WindowsAndMessaging::EnumWindows;
    extern "system" fn each(h: HWND, data: LPARAM) -> windows::core::BOOL {
        // SAFETY: `data` is the Vec below, alive for the whole EnumWindows call.
        let order = unsafe { &mut *(data.0 as *mut Vec<isize>) };
        order.push(h.0 as isize);
        true.into()
    }
    let mut order: Vec<isize> = Vec::new();
    // SAFETY: the callback only appends to `order`.
    let _ = unsafe { EnumWindows(Some(each), LPARAM(&mut order as *mut Vec<isize> as isize)) };
    order
        .into_iter()
        .filter_map(|h| windows.iter().copied().find(|w| w.0 .0 as isize == h))
        .filter(|w| w.is_visible())
        .collect()
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

/// Is the foreground window one of this process's (any Krunk window or popup)?
pub fn foreground_is_ours() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    // SAFETY: plain queries; a null foreground window gives process id 0.
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
        pid == std::process::id()
    }
}

/// Wakes the UI thread once per display refresh while it runs (0.7.0, R10): a thread waits for
/// each composition (`DwmFlush`) and posts `code`. At most one wake-up is queued at a time
/// ([`VsyncPump::ack`]), so a busy UI thread is never flooded. Stops when dropped.
pub struct VsyncPump {
    run: std::sync::Arc<std::sync::atomic::AtomicBool>,
    queued: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl VsyncPump {
    pub fn start(notifier: AppNotifier, code: usize) -> VsyncPump {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let run = Arc::new(AtomicBool::new(true));
        let queued = Arc::new(AtomicBool::new(false));
        let (r, q) = (run.clone(), queued.clone());
        let _ = std::thread::Builder::new().name("krunk-vsync".into()).spawn(move || {
            while r.load(Ordering::Relaxed) {
                // SAFETY: plain call; waits for the next desktop composition.
                if unsafe { windows::Win32::Graphics::Dwm::DwmFlush() }.is_err() {
                    std::thread::sleep(std::time::Duration::from_millis(16));
                }
                if r.load(Ordering::Relaxed) && !q.swap(true, Ordering::AcqRel) {
                    notifier.notify(code);
                }
            }
        });
        VsyncPump { run, queued }
    }

    /// The UI thread took the wake-up: the next one may be posted.
    pub fn ack(&self) {
        self.queued.store(false, std::sync::atomic::Ordering::Release);
    }
}

/// Milliseconds until the display's next refresh (0.7.1, R10): what an animation frame drawn
/// now should show (`lb_core::overlay::ms_to_next_refresh`). `None` without compositor timing.
pub fn ms_to_next_refresh() -> Option<f64> {
    use windows::Win32::Graphics::Dwm::{DwmGetCompositionTimingInfo, DWM_TIMING_INFO};
    use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    let mut info = DWM_TIMING_INFO { cbSize: std::mem::size_of::<DWM_TIMING_INFO>() as u32, ..Default::default() };
    let (mut now, mut freq) = (0i64, 0i64);
    // SAFETY: plain queries into properly sized locals (a null window asks for the desktop's
    // composition timing, as Windows 8.1+ requires).
    unsafe {
        DwmGetCompositionTimingInfo(HWND::default(), &mut info).ok()?;
        QueryPerformanceFrequency(&mut freq).ok()?;
        QueryPerformanceCounter(&mut now).ok()?;
    }
    lb_core::overlay::ms_to_next_refresh(now as u64, info.qpcVBlank, info.qpcRefreshPeriod, freq as u64)
}

impl Drop for VsyncPump {
    fn drop(&mut self) {
        self.run.store(false, std::sync::atomic::Ordering::Relaxed);
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
