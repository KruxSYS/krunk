//! Measurement builds only (`--features perf-probe`): `KRUNK_PERF_OFF` names page scripts to
//! leave out (gpc, dnt, tokens, login, audio, pointer, notify, media) and `cdp` (no per-tab
//! DevTools domains), so a benchmark can show what each costs (R24, `speedometer-0.7.0.md`).
//! 0.26.0 adds `omnibox-preconnect` (no early connection for the address bar's top row),
//! `preconnect-block` (the engine gets an empty blocked-host set) and `preload` (network
//! prediction off, as with the setting off). F-filter, F-scripts, F-writes, F-cosmetic and F-keys
//! had switches too; they were measured, dropped and reverted (docs/perf-0.26.0/fixes.md).

use crate::ffi::cef::Scripts;

fn list() -> Vec<String> {
    std::env::var("KRUNK_PERF_OFF").unwrap_or_default().split(',').map(|s| s.trim().to_string()).collect()
}

pub(super) fn off(name: &str) -> bool {
    list().iter().any(|n| n == name || n == "all")
}

pub(super) fn apply(s: &mut Scripts) {
    s.page_tokens &= !off("tokens");
    s.login &= !off("login");
    s.audio &= !off("audio");
    s.pointer_lock &= !off("pointer");
    s.notify &= !off("notify");
    s.media &= !off("media");
}
