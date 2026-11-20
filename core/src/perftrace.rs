//! 0.25.0: the switch/open/close trace (`KRUNK_PERF_TRACE=1`, off by default). One line per
//! tab change in `perf.log`, read by `scripts/perf/trace-summary.py`: where the time between the
//! input and the new tab on top goes (the tab model, the shell messages, layout, the wait for the
//! new page's frame). Times are QPC ticks; the log's first line gives their frequency.
//!
//! 0.26.0: also one `load` line per top-level navigation of a tab ([`Load`]): when each of
//! Krunk's own steps happened, from the input (or the engine's start, for a link) to the end
//! of loading, the UI-thread work done for it, and what request filtering cost.

use crate::tabs::TabId;

/// Is the trace asked for (the value of `KRUNK_PERF_TRACE`)?
pub fn enabled(value: Option<&str>) -> bool {
    matches!(value.map(str::trim), Some("1" | "true" | "on" | "yes"))
}

/// The first line of `perf.log`.
pub fn header(qpf: i64) -> String {
    format!("qpf {qpf}")
}

/// One tab change, filled in as it happens: the input, the activation, the reveal.
#[derive(Debug, Clone, PartialEq)]
pub struct Rec {
    /// `switch`, `click`, `open`, `link`, `close`, or `other` (no input seen: a restore, a drop).
    pub kind: &'static str,
    pub tab: TabId,
    /// The input (shortcut or shell request) that started it.
    pub qpc_in: Option<i64>,
    /// The reveal path: `ready` (swapped at once), `out_of_sight` (painted before, hidden from
    /// the engine), `not_loaded` (new or discarded), `minimized`.
    pub path: &'static str,
    pub act_ms: f64,
    pub apply_ms: f64,
    pub send_tabs_ms: f64,
    pub send_nav_ms: f64,
    pub layout_ms: f64,
    /// How the wait ended: `painted`, `frames`, `timeout`, or `none` (no wait).
    pub wait_end: &'static str,
    pub wait_ms: f64,
    /// The new view brought on top.
    pub swap_qpc: Option<i64>,
    /// When the wait began (not written out).
    pub wait_qpc: Option<i64>,
    /// `apply` has finished (its timings are in).
    pub applied: bool,
}

impl Rec {
    pub fn new(kind: &'static str, tab: TabId, qpc_in: Option<i64>) -> Rec {
        Rec {
            kind,
            tab,
            qpc_in,
            path: "ready",
            act_ms: 0.0,
            apply_ms: 0.0,
            send_tabs_ms: 0.0,
            send_nav_ms: 0.0,
            layout_ms: 0.0,
            wait_end: "none",
            wait_ms: 0.0,
            swap_qpc: None,
            wait_qpc: None,
            applied: false,
        }
    }

    /// Complete: applied and on top.
    pub fn done(&self) -> bool {
        self.applied && self.swap_qpc.is_some()
    }

    pub fn line(&self) -> String {
        let q = |v: Option<i64>| v.map(|v| v.to_string()).unwrap_or_else(|| "-".into());
        format!(
            "{} qpc_in={} tab={} path={} act_ms={:.2} apply_ms={:.2} send_tabs_ms={:.2} send_nav_ms={:.2} \
             layout_ms={:.2} wait_end={} wait_ms={:.1} swap_qpc={}",
            self.kind,
            q(self.qpc_in),
            self.tab,
            self.path,
            self.act_ms,
            self.apply_ms,
            self.send_tabs_ms,
            self.send_nav_ms,
            self.layout_ms,
            self.wait_end,
            self.wait_ms,
            q(self.swap_qpc),
        )
    }
}

/// The steps of a navigation, in the order they normally happen (also the order in the line).
pub const LOAD_MARKS: [&str; 9] = [
    "input",
    "load_url",
    "before_browse",
    "main_request",
    "load_start",
    "commit",
    "painted",
    "load_end",
    "loading_done",
];

/// Request filtering during one navigation, from the IO thread's counters.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LoadNet {
    /// Requests that went through the policy (`check` + `started`) and how many it stopped
    /// (blocked or answered with a stand-in).
    pub requests: u32,
    pub blocked: u32,
    pub filter_us: f64,
    pub filter_max_us: f64,
    /// Cookie access filter calls (send and save).
    pub cookies: u32,
    pub cookie_us: f64,
    pub cookie_max_us: f64,
    /// Requests by their filter time: below each of [`FILTER_HIST_US`], and the rest (0.26.0).
    pub filter_hist: [u32; 5],
}

/// The upper bounds (µs) of the filter-time buckets in a load line's `filter_hist`.
pub const FILTER_HIST_US: [f64; 4] = [10.0, 50.0, 200.0, 1000.0];

/// The bucket of a filter time.
pub fn filter_bucket(us: f64) -> usize {
    FILTER_HIST_US.iter().position(|b| us < *b).unwrap_or(FILTER_HIST_US.len())
}

/// One top-level navigation of a tab, filled in as it happens and written once loading ends
/// (or when the next navigation replaces it).
#[derive(Debug, Clone, PartialEq)]
pub struct Load {
    pub tab: TabId,
    /// What started it: `omnibox`, `suggest`, `ntp`, `open_url`, `back`, `forward`, `reload`
    /// (an input), `host` (Krunk loaded it without an input seen: a restore, a rewrite), `link`
    /// (the engine started it: a link, a script, a redirect of the page).
    pub kind: &'static str,
    /// QPC of each of [`LOAD_MARKS`], when seen.
    pub marks: [Option<i64>; 9],
    /// Server redirects of the main request.
    pub redirects: u32,
    /// The main frame's HTTP status at its load end.
    pub status: Option<i32>,
    /// UI-thread time in Krunk's handlers of the engine's events (no nesting counted twice).
    pub ui_ms: f64,
    /// Named UI-thread steps: name, count, total ms (handlers and steps inside them).
    pub ui: Vec<(&'static str, u32, f64)>,
    pub net: LoadNet,
    /// 0.26.0: the page contexts the renderer made for the navigation's documents (main frame
    /// and iframes) and the time it spent in Krunk's scripts at their start: uBO's scriptlets
    /// and Krunk's page scripts (`renderer::context_created`). Renderer-side clock, µs.
    pub contexts: u32,
    pub scriptlet_us: f64,
    pub script_us: f64,
}

impl Load {
    pub fn new(tab: TabId, kind: &'static str) -> Load {
        Load {
            tab,
            kind,
            marks: [None; 9],
            redirects: 0,
            status: None,
            ui_ms: 0.0,
            ui: Vec::new(),
            net: LoadNet::default(),
            contexts: 0,
            scriptlet_us: 0.0,
            script_us: 0.0,
        }
    }

    /// The index of a mark in [`LOAD_MARKS`].
    pub fn index(mark: &str) -> Option<usize> {
        LOAD_MARKS.iter().position(|m| *m == mark)
    }

    /// Sets a mark unless it was seen already (the first one counts).
    pub fn mark(&mut self, i: usize, qpc: i64) {
        if let Some(m) = self.marks.get_mut(i) {
            m.get_or_insert(qpc);
        }
    }

    pub fn has(&self, mark: &str) -> bool {
        Load::index(mark).is_some_and(|i| self.marks[i].is_some())
    }

    /// Adds `ms` of the UI-thread step `name`.
    pub fn add_ui(&mut self, name: &'static str, ms: f64) {
        match self.ui.iter_mut().find(|u| u.0 == name) {
            Some(u) => {
                u.1 += 1;
                u.2 += ms;
            }
            None => self.ui.push((name, 1, ms)),
        }
    }

    /// The first mark seen: the times in the line are relative to it.
    pub fn start(&self) -> Option<i64> {
        self.marks.iter().flatten().min().copied()
    }

    /// `end`: `done` (loading ended) or `replaced` (another navigation began first).
    pub fn line(&self, qpf: i64, end: &str) -> String {
        let start = self.start().unwrap_or(0);
        let ms = |q: i64| (q - start) as f64 * 1000.0 / qpf.max(1) as f64;
        let mut l = format!("load tab={} kind={} end={} qpc0={start}", self.tab, self.kind, sanitize(end));
        for (name, m) in LOAD_MARKS.iter().zip(self.marks) {
            match m {
                Some(q) => l.push_str(&format!(" {name}={:.2}", ms(q))),
                None => l.push_str(&format!(" {name}=-")),
            }
        }
        let status = self.status.map(|s| s.to_string()).unwrap_or_else(|| "-".into());
        l.push_str(&format!(" redirects={} status={status} ui_ms={:.2} ui=", self.redirects, self.ui_ms));
        if self.ui.is_empty() {
            l.push('-');
        }
        for (i, (name, n, total)) in self.ui.iter().enumerate() {
            let sep = if i == 0 { "" } else { "," };
            l.push_str(&format!("{sep}{}:{n}:{total:.2}", sanitize(name).replace([',', ':'], "_")));
        }
        let n = &self.net;
        l.push_str(&format!(
            " req={} blocked={} filter_us={:.0} filter_max_us={:.0} cookie={} cookie_us={:.0} cookie_max_us={:.0}",
            n.requests, n.blocked, n.filter_us, n.filter_max_us, n.cookies, n.cookie_us, n.cookie_max_us
        ));
        let h = n.filter_hist;
        l.push_str(&format!(
            " filter_hist={}/{}/{}/{}/{} ctx={} scriptlet_us={:.0} script_us={:.0}",
            h[0], h[1], h[2], h[3], h[4], self.contexts, self.scriptlet_us, self.script_us
        ));
        l
    }
}

/// A UI-thread task or message that ran longer than [`STALL_MS`].
pub const STALL_MS: f64 = 16.0;

pub fn stall_line(qpc: i64, kind: &str, ms: f64, depth: u32) -> String {
    format!("stall qpc={qpc} kind={} ms={ms:.1} depth={depth}", sanitize(kind))
}

/// One window move or drag step and how long Krunk's handling of it took.
pub fn move_line(qpc: i64, what: &str, ms: f64) -> String {
    format!("move qpc={qpc} what={} ms={ms:.2}", sanitize(what))
}

/// A hover-card capture: sent at `qpc`, answered `ms` later.
pub fn capture_line(qpc: i64, tab: TabId, ms: f64) -> String {
    format!("capture qpc={qpc} tab={tab} ms={ms:.1}")
}

/// The variant name of a request's `Debug` form (never its fields: they can hold secrets).
pub fn variant_name(debug: &str) -> &str {
    let end = debug.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(debug.len());
    &debug[..end]
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_graphic() && c != '=' { c } else { '_' }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_unless_asked_for() {
        assert!(!enabled(None));
        assert!(!enabled(Some("")));
        assert!(!enabled(Some("0")));
        assert!(enabled(Some("1")));
        assert!(enabled(Some(" on ")));
    }

    #[test]
    fn a_record_is_one_line_of_key_values() {
        let mut r = Rec::new("switch", 7, Some(100));
        assert!(!r.done());
        r.applied = true;
        r.swap_qpc = Some(250);
        r.path = "out_of_sight";
        r.wait_end = "frames";
        r.wait_ms = 123.45;
        assert!(r.done());
        let l = r.line();
        assert!(l.starts_with("switch qpc_in=100 tab=7 path=out_of_sight "), "{l}");
        assert!(l.contains(" wait_end=frames wait_ms=123.5 swap_qpc=250"), "{l}");
        assert!(!l.contains('\n'));
        assert_eq!(Rec::new("other", 1, None).line().split(' ').nth(1), Some("qpc_in=-"));
    }

    #[test]
    fn only_the_variant_name_of_a_request() {
        assert_eq!(variant_name("VaultReveal { id: \"1\", password: x }"), "VaultReveal");
        assert_eq!(variant_name("Ready"), "Ready");
        assert_eq!(stall_line(5, "req:a b=c", 20.0, 1), "stall qpc=5 kind=req:a_b_c ms=20.0 depth=1");
    }

    #[test]
    fn a_load_line_has_every_mark_relative_to_the_first() {
        // 10 MHz: 10 000 ticks a millisecond.
        let mut l = Load::new(3, "omnibox");
        l.mark(Load::index("before_browse").unwrap(), 1_005_000);
        l.mark(Load::index("input").unwrap(), 1_000_000);
        l.mark(Load::index("load_end").unwrap(), 4_000_000);
        // The first one counts.
        l.mark(Load::index("load_end").unwrap(), 5_000_000);
        assert!(l.has("input") && !l.has("commit"));
        l.add_ui("address", 0.25);
        l.add_ui("address", 0.5);
        l.add_ui("title", 1.0);
        l.ui_ms = 1.75;
        l.status = Some(200);
        l.net = LoadNet {
            requests: 12,
            blocked: 2,
            filter_us: 340.4,
            filter_max_us: 90.0,
            cookies: 3,
            cookie_us: 5.0,
            cookie_max_us: 2.0,
            filter_hist: [9, 2, 1, 0, 0],
        };
        l.contexts = 4;
        l.scriptlet_us = 812.4;
        l.script_us = 1530.0;
        let line = l.line(10_000_000, "done");
        assert_eq!(
            line,
            "load tab=3 kind=omnibox end=done qpc0=1000000 input=0.00 load_url=- before_browse=0.50 main_request=- \
             load_start=- commit=- painted=- load_end=300.00 loading_done=- redirects=0 status=200 ui_ms=1.75 \
             ui=address:2:0.75,title:1:1.00 req=12 blocked=2 filter_us=340 filter_max_us=90 cookie=3 cookie_us=5 \
             cookie_max_us=2 filter_hist=9/2/1/0/0 ctx=4 scriptlet_us=812 script_us=1530"
        );
    }

    #[test]
    fn filter_times_fall_in_their_bucket() {
        assert_eq!(filter_bucket(0.0), 0);
        assert_eq!(filter_bucket(9.9), 0);
        assert_eq!(filter_bucket(10.0), 1);
        assert_eq!(filter_bucket(199.0), 2);
        assert_eq!(filter_bucket(999.0), 3);
        assert_eq!(filter_bucket(5000.0), 4);
    }

    #[test]
    fn an_empty_load_line_still_has_every_key() {
        let line = Load::new(1, "link").line(0, "replaced");
        assert!(line.starts_with("load tab=1 kind=link end=replaced qpc0=0 input=- "), "{line}");
        assert!(line.contains(" status=- ui_ms=0.00 ui=- req=0 "), "{line}");
        assert!(!line.contains('\n'));
        for m in LOAD_MARKS {
            assert!(line.contains(&format!(" {m}=")), "{m}");
        }
    }
}
