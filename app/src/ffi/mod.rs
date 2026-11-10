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
