//! The FFI layer: the only place in the app with `unsafe` code. Everything
//! here wraps Win32/WebView2 calls in safe functions for the rest of the app.
#![allow(unsafe_code)]

pub mod dialogs;
pub mod http;
pub mod loader;
pub mod process;
pub mod webview;
pub mod window;
