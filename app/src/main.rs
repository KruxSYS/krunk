//! LightBrowser host executable.
//!
//! The browser itself only exists on Windows (WebView2). On other platforms
//! the binary is a stub so that `cargo build --workspace` works everywhere.
#![cfg_attr(windows, windows_subsystem = "windows")]
#![deny(unsafe_code)]

#[cfg(windows)]
fn main() {
    // Filled in by milestone 2 (window + WebView2).
}

#[cfg(not(windows))]
fn main() {
    eprintln!("LightBrowser runs on Windows only; build with --target x86_64-pc-windows-gnu.");
    std::process::exit(1);
}
