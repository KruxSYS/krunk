//! Platform-independent core of LightBrowser.
//!
//! Everything here builds and is tested natively on Linux; the Windows host
//! (`app`) only adds the WebView2/Win32 glue on top.
#![forbid(unsafe_code)]

pub mod blocker;
pub mod bookmarks;
pub mod frecency;
pub mod history;
pub mod integrity;
pub mod netscape;
pub mod pages;
pub mod passgen;
pub mod protocol;
pub mod settings;
pub mod storage;
pub mod urlclean;
pub mod vault;
