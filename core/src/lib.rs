//! Platform-independent core of Krunk.
//!
//! Everything here builds and is tested natively on Linux; the Windows host
//! (`app`) only adds the CEF/Win32 glue on top.
#![forbid(unsafe_code)]

pub mod assets;
pub mod blocker;
pub mod blocklog;
pub mod bookmarks;
pub mod certinfo;
pub mod cookies;
pub mod downloads;
pub mod filterlists;
pub mod frame;
pub mod frecency;
pub mod history;
pub mod import;
pub mod keys;
pub mod layout;
pub mod memory;
pub mod migrate;
pub mod netscape;
pub mod omnibox;
pub mod overlay;
pub mod pages;
pub mod passgen;
pub mod permissions;
pub mod pointer_lock;
pub mod preprocess;
pub mod protection;
pub mod protocol;
pub mod secret;
pub mod segdl;
pub mod settings;
pub mod sitedata;
pub mod storage;
pub mod sync;
pub mod tabs;
pub mod toggle;
pub mod update;
pub mod urlclean;
pub mod useragents;
pub mod vault;
pub mod windows;
pub mod zoom;
