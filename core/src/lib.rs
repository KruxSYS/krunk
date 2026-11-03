//! Platform-independent core of LightBrowser.
//!
//! Everything here builds and is tested natively on Linux; the Windows host
//! (`app`) only adds the WebView2/Win32 glue on top.
#![forbid(unsafe_code)]
