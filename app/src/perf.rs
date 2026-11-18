//! 0.25.0: the tab-change and UI-stall trace (`KRUNK_PERF_TRACE=1`, off by default), written to
//! `perf.log` next to `krunk.log`. Record formats are in `lb_core::perftrace`. When the trace is
//! off every hook is one relaxed atomic load.

use lb_core::perftrace;
use std::cell::Cell;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Mutex;

static ON: AtomicBool = AtomicBool::new(false);
static QPF: AtomicI64 = AtomicI64::new(1);
static FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

thread_local! {
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Opens `perf.log` when `KRUNK_PERF_TRACE` asks for it.
pub fn init(path: &std::path::Path) {
    if !perftrace::enabled(std::env::var("KRUNK_PERF_TRACE").ok().as_deref()) {
        return;
    }
    let Ok(f) = std::fs::File::create(path) else { return };
    QPF.store(crate::ffi::qpf(), Ordering::Relaxed);
    if let Ok(mut g) = FILE.lock() {
        *g = Some(f);
    }
    ON.store(true, Ordering::Relaxed);
    line(&perftrace::header(QPF.load(Ordering::Relaxed)));
}

#[inline]
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Now in QPC ticks, or 0 when the trace is off.
#[inline]
pub fn now() -> i64 {
    if on() {
        crate::ffi::qpc()
    } else {
        0
    }
}

/// Milliseconds since `from` (a [`now`]).
pub fn ms_since(from: i64) -> f64 {
    if !on() || from == 0 {
        return 0.0;
    }
    (crate::ffi::qpc() - from) as f64 * 1000.0 / QPF.load(Ordering::Relaxed) as f64
}

pub fn ms_between(from: i64, to: i64) -> f64 {
    (to - from) as f64 * 1000.0 / QPF.load(Ordering::Relaxed).max(1) as f64
}

pub fn line(s: &str) {
    if !on() {
        return;
    }
    if let Ok(mut g) = FILE.lock() {
        if let Some(f) = g.as_mut() {
            let _ = writeln!(f, "{s}");
        }
    }
}

/// Times a UI-thread task or message; logs it as a stall when it ran longer than 16 ms.
pub struct Stall {
    start: i64,
    kind: Option<&'static str>,
    name: Option<String>,
}

impl Stall {
    #[inline]
    pub fn new(kind: &'static str) -> Stall {
        if !on() {
            return Stall { start: 0, kind: None, name: None };
        }
        DEPTH.with(|d| d.set(d.get() + 1));
        Stall { start: crate::ffi::qpc(), kind: Some(kind), name: None }
    }

    /// With a name computed only while tracing (a window message, a request's variant).
    #[inline]
    pub fn named(kind: &'static str, name: impl FnOnce() -> String) -> Stall {
        let mut s = Stall::new(kind);
        if s.kind.is_some() {
            s.name = Some(name());
        }
        s
    }
}

impl Drop for Stall {
    fn drop(&mut self) {
        let Some(kind) = self.kind else { return };
        let depth = DEPTH.with(|d| {
            let v = d.get();
            d.set(v.saturating_sub(1));
            v
        });
        let ms = ms_since(self.start);
        if ms >= perftrace::STALL_MS {
            let k = match &self.name {
                Some(n) => format!("{kind}:{n}"),
                None => kind.to_string(),
            };
            line(&perftrace::stall_line(self.start, &k, ms, depth));
        }
    }
}
