//! Platform-independent core of Krunk.
//!
//! Everything here builds and is tested natively on Linux; the Windows host
//! (`app`) only adds the CEF/Win32 glue on top.
#![forbid(unsafe_code)]

pub mod assets;
pub mod blocker;
pub mod bookmarks;
pub mod cookies;
pub mod downloads;
pub mod filterlists;
pub mod frame;
pub mod frecency;
pub mod history;
pub mod keys;
pub mod layout;
pub mod memory;
pub mod migrate;
pub mod netscape;
pub mod pages;
pub mod passgen;
pub mod pointer_lock;
pub mod preprocess;
pub mod protocol;
pub mod secret;
pub mod segdl;
pub mod settings;
pub mod sitedata;
pub mod storage;
pub mod sync;
pub mod tabs;
pub mod update;
pub mod urlclean;
pub mod vault;
pub mod windows;
pub mod zoom;
