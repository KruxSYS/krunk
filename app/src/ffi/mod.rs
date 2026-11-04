//! The FFI layer: the only place in the app with `unsafe` code. Everything
//! here wraps Win32 and CEF calls in safe functions for the rest of the app.
#![allow(unsafe_code)]

pub mod cef;
pub mod dialogs;
pub mod http;
pub mod process;
pub mod procmem;
pub mod secure;
pub mod window;
