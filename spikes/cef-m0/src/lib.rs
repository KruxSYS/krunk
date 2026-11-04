//! M0 spike (not part of the product): a minimal CEF browser hosted as an Alloy-style child
//! window in a plain Win32 window, started through CEF's sandbox bootstrap
//! (`LightBrowser.exe` = renamed `bootstrap.exe`, loading this `LightBrowser.dll`).
#![cfg(windows)]
// The cdylib must be named LightBrowser.dll for the renamed bootstrap.
#![allow(non_snake_case)]

use cef::*;
use std::sync::atomic::{AtomicIsize, Ordering};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{COLOR_WINDOW, HBRUSH};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// The browser's child HWND, for resizing (0 until created).
static CHILD: AtomicIsize = AtomicIsize::new(0);

wrap_life_span_handler! {
    struct SpikeLifeSpan;
    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            if let Some(host) = browser.and_then(|b| b.host()) {
                CHILD.store(host.window_handle().0 as isize, Ordering::SeqCst);
            }
        }
        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            quit_message_loop();
        }
    }
}

wrap_client! {
    struct SpikeClient;
    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(SpikeLifeSpan::new())
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_SIZE => {
            let child = CHILD.load(Ordering::SeqCst) as HWND;
            if !child.is_null() {
                let mut r: RECT = unsafe { std::mem::zeroed() };
                unsafe {
                    GetClientRect(hwnd, &mut r);
                    SetWindowPos(child, std::ptr::null_mut(), 0, 0, r.right, r.bottom, SWP_NOZORDER);
                }
            }
            0
        }
        WM_CLOSE => {
            // Let CEF close the browser first; on_before_close quits the loop.
            let child = CHILD.load(Ordering::SeqCst) as HWND;
            if child.is_null() {
                unsafe { DestroyWindow(hwnd) };
            } else {
                unsafe { PostMessageW(child, WM_CLOSE, 0, 0) };
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn create_main_window() -> HWND {
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class = wide("LightBrowserSpike");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            hbrBackground: (COLOR_WINDOW + 1) as usize as HBRUSH,
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
        let title = wide("LightBrowser CEF spike (M0)");
        CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1200,
            800,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        )
    }
}

/// The runtime files the bundled engine needs next to the exe.
const REQUIRED: &[&str] = &[
    "libcef.dll",
    "chrome_elf.dll",
    "icudtl.dat",
    "resources.pak",
    "v8_context_snapshot.bin",
    "locales\\en-US.pak",
];

fn missing_files() -> Vec<&'static str> {
    let dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()));
    let Some(dir) = dir else { return REQUIRED.to_vec() };
    REQUIRED.iter().copied().filter(|f| !dir.join(f).is_file()).collect()
}

fn run(instance: *mut core::ffi::c_void, sandbox_info: *mut u8) -> i32 {
    let missing = missing_files();
    if !missing.is_empty() {
        let text = wide(&format!(
            "LightBrowser cannot start because some of its files are missing or damaged:\n\n{}\n\n\
             Re-extract the LightBrowser folder from the download (keep all files together).",
            missing.join("\n")
        ));
        let title = wide("LightBrowser");
        unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_ICONERROR) };
        return 2;
    }
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let main_args = MainArgs { instance: sys::HINSTANCE(instance.cast()) };
    // Subprocesses (renderer, GPU, utility) return here.
    let code = execute_process(Some(&main_args), None, sandbox_info);
    if code >= 0 {
        return code;
    }
    let data = std::env::var("LOCALAPPDATA").unwrap_or_default() + "\\LightBrowserSpike";
    let settings = Settings {
        no_sandbox: 0,
        root_cache_path: CefString::from(data.as_str()),
        cache_path: CefString::from(format!("{data}\\default").as_str()),
        log_file: CefString::from(format!("{data}\\cef.log").as_str()),
        log_severity: LogSeverity::WARNING,
        ..Default::default()
    };
    if initialize(Some(&main_args), Some(&settings), None, sandbox_info) != 1 {
        let text = wide("CEF failed to initialize");
        unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), text.as_ptr(), MB_ICONERROR) };
        return 1;
    }
    let hwnd = create_main_window();
    let mut r: RECT = unsafe { std::mem::zeroed() };
    unsafe { GetClientRect(hwnd, &mut r) };
    let window_info = WindowInfo {
        runtime_style: RuntimeStyle::ALLOY,
        ..Default::default()
    }
    .set_as_child(sys::HWND(hwnd.cast()), &Rect { x: 0, y: 0, width: r.right, height: r.bottom });
    let mut client = SpikeClient::new();
    let url = std::env::args()
        .skip(1)
        .find(|a| a.starts_with("http"))
        .unwrap_or_else(|| "https://example.com/".into());
    browser_host_create_browser(
        Some(&window_info),
        Some(&mut client),
        Some(&CefString::from(url.as_str())),
        Some(&BrowserSettings::default()),
        None,
        None,
    );
    run_message_loop();
    shutdown();
    0
}

/// Entry point called by the renamed CEF `bootstrap.exe` (see include/cef_sandbox_win.h).
#[unsafe(no_mangle)]
pub extern "C" fn RunWinMain(
    instance: *mut core::ffi::c_void,
    _command_line: *const u16,
    _show: i32,
    sandbox_info: *mut u8,
    _version_info: *mut core::ffi::c_void,
) -> i32 {
    run(instance, sandbox_info)
}
