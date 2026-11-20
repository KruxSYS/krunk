//! Measurement builds only (`--features perf-probe`): `KRUNK_PERF_OFF` names page scripts to
//! leave out (gpc, dnt, tokens, login, audio, pointer, notify, media) and `cdp` (no per-tab
//! DevTools domains), so a benchmark can show what each costs (R24, `speedometer-0.7.0.md`).
//! 0.26.0 adds `omnibox-preconnect` (no early connection for the address bar's top row) and
//! `preconnect-block` (the engine gets an empty blocked-host set) and `filter-fast` (the request
//! filter takes the page's address per use, as in 0.25.5) and `preload` (network prediction off,
//! as with the setting off).

use crate::ffi::cef::Scripts;

/// Read once: some names are checked per request on the IO thread (`filter-fast`), and both
/// sides of an A/B must pay the same for the check.
fn list() -> &'static [String] {
    static LIST: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    LIST.get_or_init(|| {
        std::env::var("KRUNK_PERF_OFF").unwrap_or_default().split(',').map(|s| s.trim().to_string()).collect()
    })
}

pub(super) fn off(name: &str) -> bool {
    list().iter().any(|n| n == name || n == "all")
}

pub(super) fn apply(s: &mut Scripts) {
    s.gpc &= !off("gpc");
    s.dnt &= !off("dnt");
    s.page_tokens &= !off("tokens");
    s.login &= !off("login");
    s.audio &= !off("audio");
    s.pointer_lock &= !off("pointer");
    s.notify &= !off("notify");
    s.media &= !off("media");
}
