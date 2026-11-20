//! 0.25.0: the tab-change and UI-stall trace (`KRUNK_PERF_TRACE=1`, off by default), written to
//! `perf.log` next to `krunk.log`. Record formats are in `lb_core::perftrace`. When the trace is
//! off every hook is one relaxed atomic load.
//!
//! 0.26.0: the load trace. One `load` line per top-level navigation of a tab: the input
//! ([`load_input`]), Krunk's call into the engine ([`load_url`]), the engine's steps
//! ([`load_before_browse`], [`load_mark`], [`load_end`]), the main document request and the
//! request filtering cost from the IO thread ([`NetCost`]), and Krunk's UI-thread work for it
//! ([`LoadSpan`]). Written by [`load_done`] once the tab's end-of-loading handlers have run.

use lb_core::perftrace;
use lb_core::tabs::TabId;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
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

/// One step of a tab change, logged as `span name=… ms=…` when it took 0.5 ms or more.
pub struct Span(i64, &'static str);

impl Span {
    #[inline]
    pub fn new(name: &'static str) -> Span {
        Span(now(), name)
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        if self.0 == 0 {
            return;
        }
        let ms = ms_since(self.0);
        if ms >= 0.5 {
            line(&format!("span qpc={} name={} ms={ms:.2}", self.0, self.1));
        }
    }
}

// ------------------------------------------------------------ 0.26.0: the load trace

/// A step of a navigation the engine reports (its index in `perftrace::LOAD_MARKS`).
#[derive(Clone, Copy)]
pub enum Mark {
    LoadStart = 4,
    Commit = 5,
    Painted = 6,
    LoadingDone = 8,
}

const INPUT: usize = 0;
const LOAD_URL: usize = 1;
const BEFORE_BROWSE: usize = 2;
const MAIN_REQUEST: usize = 3;
const LOAD_END: usize = 7;

thread_local! {
    /// The navigation under way in each tab (UI thread only; empty while the trace is off).
    static LOADS: RefCell<HashMap<TabId, perftrace::Load>> = RefCell::default();
}

fn with_load<R>(tab: TabId, f: impl FnOnce(Option<&mut perftrace::Load>) -> R) -> R {
    LOADS.with(|l| f(l.borrow_mut().get_mut(&tab)))
}

fn insert(rec: perftrace::Load) {
    LOADS.with(|l| l.borrow_mut().insert(rec.tab, rec));
}

/// Writes out tab `tab`'s navigation, if it began in the engine, and forgets it.
fn flush(tab: TabId, end: &str, net: Option<&NetCost>) {
    let Some(mut rec) = LOADS.with(|l| l.borrow_mut().remove(&tab)) else { return };
    if rec.marks[BEFORE_BROWSE].is_none() {
        // An input the engine never acted on (a same-document move, a refused address).
        return;
    }
    if let Some(net) = net {
        let (main, cost) = net.take();
        rec.marks[MAIN_REQUEST] = main;
        rec.net = cost;
    }
    line(&rec.line(QPF.load(Ordering::Relaxed), end));
}

/// The user asked tab `tab` to go somewhere (`kind`: `omnibox`, `back` …) at `qpc` (a [`now`]).
pub fn load_input(tab: TabId, kind: &'static str, qpc: i64) {
    if !on() || qpc == 0 {
        return;
    }
    flush(tab, "replaced", None);
    let mut rec = perftrace::Load::new(tab, kind);
    rec.mark(INPUT, qpc);
    insert(rec);
}

/// Krunk calls the engine's load (or back, forward, reload) for tab `tab`.
pub fn load_url(tab: TabId) {
    if !on() {
        return;
    }
    let t = crate::ffi::qpc();
    let waiting = with_load(tab, |r| match r {
        Some(r) if r.marks[BEFORE_BROWSE].is_none() => {
            r.mark(LOAD_URL, t);
            true
        }
        _ => false,
    });
    if !waiting {
        flush(tab, "replaced", None);
        let mut rec = perftrace::Load::new(tab, "host");
        rec.mark(LOAD_URL, t);
        insert(rec);
    }
}

/// The engine asked whether tab `tab`'s main frame may navigate (asked at `qpc`, a [`now`])
/// and Krunk allowed it. A redirect only counts; any other one after a navigation began in the
/// engine is a new navigation. `host`: Krunk started it. `net`: the tab's request counters,
/// which start again here.
pub fn load_before_browse(tab: TabId, qpc: i64, redirected: bool, host: bool, net: &NetCost) {
    if !on() || qpc == 0 {
        return;
    }
    let ms = ms_since(qpc);
    let added = with_load(tab, |r| match r {
        Some(r) if redirected => {
            r.redirects += 1;
            add_ui(r, "before_browse", ms, true);
            true
        }
        Some(r) if r.marks[BEFORE_BROWSE].is_none() => {
            r.mark(BEFORE_BROWSE, qpc);
            add_ui(r, "before_browse", ms, true);
            true
        }
        _ => false,
    });
    if !added {
        // The one before ends here, with the requests counted so far.
        flush(tab, "replaced", Some(net));
        let mut rec = perftrace::Load::new(tab, if host { "host" } else { "link" });
        rec.mark(BEFORE_BROWSE, qpc);
        add_ui(&mut rec, "before_browse", ms, true);
        insert(rec);
    }
    if !redirected {
        net.take();
    }
}

/// A step of tab `tab`'s navigation happened now (the first time counts).
pub fn load_mark(tab: TabId, mark: Mark) {
    if !on() {
        return;
    }
    let t = crate::ffi::qpc();
    with_load(tab, |r| {
        if let Some(r) = r.filter(|r| r.marks[BEFORE_BROWSE].is_some()) {
            r.mark(mark as usize, t);
        }
    });
}

/// The main frame's load ended with HTTP `status`.
pub fn load_end(tab: TabId, status: i32) {
    if !on() {
        return;
    }
    let t = crate::ffi::qpc();
    with_load(tab, |r| {
        if let Some(r) = r.filter(|r| r.marks[BEFORE_BROWSE].is_some() && r.marks[LOAD_END].is_none()) {
            r.mark(LOAD_END, t);
            r.status = Some(status);
        }
    });
}

/// 0.26.0: a renderer made a page context for tab `tab` (its main frame or an iframe) and spent
/// `scriptlet_us` in uBO's scriptlets and `script_us` in Krunk's page scripts at its start.
pub fn load_scripts(tab: TabId, scriptlet_us: u32, script_us: u32) {
    if !on() {
        return;
    }
    with_load(tab, |r| {
        if let Some(r) = r {
            r.contexts += 1;
            r.scriptlet_us += f64::from(scriptlet_us);
            r.script_us += f64::from(script_us);
        }
    });
}

/// Tab `tab`'s handlers of the end of loading have run: its line is written.
pub fn load_done(tab: TabId, net: &NetCost) {
    if !on() {
        return;
    }
    if with_load(tab, |r| r.is_some_and(|r| r.marks[Mark::LoadingDone as usize].is_some())) {
        flush(tab, "done", Some(net));
    }
}

fn add_ui(r: &mut perftrace::Load, name: &'static str, ms: f64, handler: bool) {
    r.add_ui(name, ms);
    if handler {
        r.ui_ms += ms;
    }
}

/// Times Krunk's UI-thread work for tab `tab`'s navigation, added to its load line under
/// `name` (nothing when no navigation is under way there).
pub struct LoadSpan {
    start: i64,
    tab: TabId,
    name: &'static str,
    handler: bool,
}

impl LoadSpan {
    /// A handler of an engine event (counted in `ui_ms`).
    #[inline]
    pub fn handler(tab: TabId, name: &'static str) -> LoadSpan {
        LoadSpan { start: now(), tab, name, handler: true }
    }

    /// A step inside a handler (listed, not added to `ui_ms` again).
    #[inline]
    pub fn step(tab: TabId, name: &'static str) -> LoadSpan {
        LoadSpan { start: now(), tab, name, handler: false }
    }
}

impl Drop for LoadSpan {
    fn drop(&mut self) {
        if self.start == 0 {
            return;
        }
        let ms = ms_since(self.start);
        // `try_`: a span may end while the map is borrowed further up (never the case today).
        LOADS.with(|l| {
            if let Some(r) = l.try_borrow_mut().ok().as_mut().and_then(|l| l.get_mut(&self.tab)) {
                add_ui(r, self.name, ms, self.handler);
            }
        });
    }
}

/// A tab's request filtering cost, counted on the IO thread with relaxed atomics (nothing
/// while the trace is off) and taken by the UI thread when a navigation ends.
#[derive(Default)]
pub struct NetCost {
    /// QPC of the first main-frame document request since the navigation began.
    main: AtomicI64,
    requests: AtomicU32,
    blocked: AtomicU32,
    filter: AtomicI64,
    filter_max: AtomicI64,
    cookies: AtomicU32,
    cookie: AtomicI64,
    cookie_max: AtomicI64,
    /// Requests per filter-time bucket (`perftrace::FILTER_HIST_US`).
    hist: [AtomicU32; 5],
}

impl NetCost {
    /// A request went through the policy from `start` (a [`now`]) until now.
    pub fn request(&self, start: i64, main: bool, blocked: bool) {
        if start == 0 {
            return;
        }
        let d = crate::ffi::qpc() - start;
        if main {
            let _ = self.main.compare_exchange(0, start, Ordering::Relaxed, Ordering::Relaxed);
        }
        self.requests.fetch_add(1, Ordering::Relaxed);
        if blocked {
            self.blocked.fetch_add(1, Ordering::Relaxed);
        }
        self.filter.fetch_add(d, Ordering::Relaxed);
        self.filter_max.fetch_max(d, Ordering::Relaxed);
        let us = d as f64 * 1_000_000.0 / QPF.load(Ordering::Relaxed).max(1) as f64;
        self.hist[perftrace::filter_bucket(us)].fetch_add(1, Ordering::Relaxed);
    }

    /// A cookie access check ran from `start` (a [`now`]) until now.
    pub fn cookie(&self, start: i64) {
        if start == 0 {
            return;
        }
        let d = crate::ffi::qpc() - start;
        self.cookies.fetch_add(1, Ordering::Relaxed);
        self.cookie.fetch_add(d, Ordering::Relaxed);
        self.cookie_max.fetch_max(d, Ordering::Relaxed);
    }

    /// The counts so far, which start again from zero.
    fn take(&self) -> (Option<i64>, perftrace::LoadNet) {
        let us = |ticks: i64| ticks as f64 * 1_000_000.0 / QPF.load(Ordering::Relaxed).max(1) as f64;
        let main = self.main.swap(0, Ordering::Relaxed);
        let net = perftrace::LoadNet {
            requests: self.requests.swap(0, Ordering::Relaxed),
            blocked: self.blocked.swap(0, Ordering::Relaxed),
            filter_us: us(self.filter.swap(0, Ordering::Relaxed)),
            filter_max_us: us(self.filter_max.swap(0, Ordering::Relaxed)),
            cookies: self.cookies.swap(0, Ordering::Relaxed),
            cookie_us: us(self.cookie.swap(0, Ordering::Relaxed)),
            cookie_max_us: us(self.cookie_max.swap(0, Ordering::Relaxed)),
            filter_hist: std::array::from_fn(|i| self.hist[i].swap(0, Ordering::Relaxed)),
        };
        ((main != 0).then_some(main), net)
    }
}
