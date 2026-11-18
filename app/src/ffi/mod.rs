//! The FFI layer: the only place in the app with `unsafe` code. Everything
//! here wraps Win32 and CEF calls in safe functions for the rest of the app.
#![allow(unsafe_code)]

pub mod appid;
pub mod cef;
pub mod cursor;
pub mod default_browser;
pub mod dialogs;
pub mod http;
pub mod icon;
pub mod image;
pub mod keyboard;
pub mod overlay;
pub mod peer;
pub mod pipwin;
pub mod process;
pub mod procmem;
pub mod scroll_lock;
pub mod secure;
pub mod window;

/// The performance counter now, in ticks (0.25.0 trace).
pub fn qpc() -> i64 {
    use windows::Win32::System::Performance::QueryPerformanceCounter;
    let mut now = 0i64;
    // SAFETY: a plain query into a local.
    let _ = unsafe { QueryPerformanceCounter(&mut now) };
    now
}

/// The performance counter's ticks per second.
pub fn qpf() -> i64 {
    use windows::Win32::System::Performance::QueryPerformanceFrequency;
    let mut f = 0i64;
    // SAFETY: a plain query into a local.
    let _ = unsafe { QueryPerformanceFrequency(&mut f) };
    f.max(1)
}
