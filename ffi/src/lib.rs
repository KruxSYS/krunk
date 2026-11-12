//! Krunk for Android: the UniFFI surface over `lb_core`.
//!
//! Thin wrappers only: every decision (URL vs search, suggestion ranking, frecency, the SQLite
//! schema, settings validation, the tab model, the memory policy) stays in `core`, so Android and
//! Windows behave the same. Kotlin bindings are generated from the built library by
//! `android/scripts/build-rust.sh` (package `org.krunk.ffi`).
#![forbid(unsafe_code)]

uniffi::setup_scaffolding!();

mod blocker;
mod error;
mod memory;
mod navmenu;
mod settings;
mod store;
mod tabs;
mod theme;
mod urls;

pub use blocker::*;
pub use error::{FfiError, SettingsError, SettingsFieldError};
pub use memory::*;
pub use navmenu::*;
pub use settings::*;
pub use store::*;
pub use tabs::*;
pub use theme::*;
pub use urls::*;

/// The shared core's version (the Windows release it came from), e.g. `0.15.3`.
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Unix time in milliseconds, as `core` stores it.
#[uniffi::export]
pub fn now_ms() -> i64 {
    lb_core::storage::now_ms()
}

/// `YYYY-MM-DD` (UTC) of a Unix-millisecond time, for grouping history by day.
#[uniffi::export]
pub fn date_ymd(ms: i64) -> String {
    lb_core::storage::date_ymd(ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_workspace() {
        assert_eq!(core_version(), env!("CARGO_PKG_VERSION"));
        assert!(core_version().split('.').count() >= 3);
    }

    #[test]
    fn dates() {
        assert_eq!(date_ymd(0), "1970-01-01");
        assert_eq!(date_ymd(1_762_819_200_000), "2025-11-11");
        assert!(now_ms() > 1_700_000_000_000);
    }
}
