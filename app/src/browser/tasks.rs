//! krunk://tasks (Krunk 0.6.0): the engine's tasks with what Krunk knows on top: the tabs each
//! renderer hosts, process ids, working set and private bytes, and a network rate from the
//! tabs' byte counters. Refreshed by the page every second; "End process" ends a task.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

use lb_core::protocol::Event;
use lb_core::tabs::TabId;
use serde::Serialize;

use super::Browser;
use crate::ffi::cef::tasks::{self, TaskKind};
use crate::ffi::procmem::{self, Kind};
use crate::log::info;

/// One row of the task manager page.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TaskRow {
    pub id: i64,
    pub kind: &'static str,
    pub title: String,
    pub pid: Option<u32>,
    /// Percent of all processors.
    pub cpu: f64,
    /// The engine's private memory footprint.
    pub memory: u64,
    pub working_set: u64,
    pub private: u64,
    /// Bytes per second received by the task's tabs since the previous sample.
    pub network: u64,
    /// The tabs' requests under way (0.18.0) and the age of the oldest in milliseconds.
    pub pending: u32,
    pub pending_oldest_ms: i64,
    pub killable: bool,
    pub tabs: Vec<TaskTab>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TaskTab {
    pub id: TabId,
    pub title: String,
}

/// Per task: the tabs' byte total and when it was sampled (for the rate).
#[derive(Default)]
pub(super) struct TaskSamples {
    last: RefCell<HashMap<i64, (u64, Instant)>>,
}

/// A utility task's title keyword ("Utility: Network Service") against its process command
/// line (`--utility-sub-type=network.mojom.NetworkService`).
fn utility_matches(title: &str, command_line: &str) -> bool {
    let key = title.trim_start_matches("Utility:").trim().to_ascii_lowercase().replace(' ', "");
    let sub = command_line
        .split_whitespace()
        .find_map(|a| a.strip_prefix("--utility-sub-type="))
        .unwrap_or("")
        .to_ascii_lowercase()
        .replace(['.', '_'], "");
    !key.is_empty() && sub.contains(key.trim_end_matches("service"))
}

impl Browser {
    pub(super) fn task_rows(&self) -> Vec<TaskRow> {
        let raw = tasks::tasks();
        // Tabs per task, from CEF's browser id → task id map.
        let mut tabs_of: HashMap<i64, Vec<TaskTab>> = HashMap::new();
        let mut pid_of: HashMap<i64, u32> = HashMap::new();
        let mut bytes_of: HashMap<i64, u64> = HashMap::new();
        let mut pending_of: HashMap<i64, (u32, i64)> = HashMap::new();
        let now_ms = lb_core::storage::now_ms();
        {
            let views = self.views.borrow();
            let ws = self.ws.borrow();
            for (id, v) in views.iter() {
                let Some(task) = v.view.browser_id().and_then(tasks::task_for_browser) else { continue };
                let title = ws
                    .tab(*id)
                    .map(|t| if t.title.trim().is_empty() { t.url.clone() } else { t.title.clone() })
                    .unwrap_or_default();
                tabs_of.entry(task).or_default().push(TaskTab { id: *id, title });
                let pid = v.view.renderer_pid();
                if pid != 0 {
                    pid_of.insert(task, pid);
                }
                *bytes_of.entry(task).or_default() += v.net.received();
                let p = v.net.pending(now_ms);
                let e = pending_of.entry(task).or_default();
                *e = (e.0 + p.count, e.1.max(p.oldest_ms));
            }
        }
        // Processes by kind for the tasks without a renderer: the browser, GPU and utilities.
        let me = std::process::id();
        let mut pids = lb_core::memory::descendants(me, &procmem::snapshot());
        pids.push(me);
        let procs: Vec<(u32, Kind, String)> = pids
            .into_iter()
            .map(|pid| {
                let cl = procmem::command_line(pid).unwrap_or_default();
                (pid, Kind::from_command_line(&cl), cl)
            })
            .collect();
        let now = Instant::now();
        let mut last = self.task_samples.last.borrow_mut();
        let mut rows = Vec::with_capacity(raw.len());
        for t in raw {
            let pid = match t.kind {
                TaskKind::Browser => Some(me),
                TaskKind::Gpu => procs.iter().find(|p| p.1 == Kind::Gpu).map(|p| p.0),
                TaskKind::Utility => {
                    procs.iter().find(|p| p.1 == Kind::Utility && utility_matches(&t.title, &p.2)).map(|p| p.0)
                }
                _ => pid_of.get(&t.id).copied(),
            };
            let mem = pid.map(procmem::memory).unwrap_or_default();
            let bytes = bytes_of.get(&t.id).copied().unwrap_or(0);
            let network = match last.insert(t.id, (bytes, now)) {
                Some((prev, at)) => {
                    let secs = now.duration_since(at).as_secs_f64().max(0.05);
                    ((bytes.saturating_sub(prev)) as f64 / secs) as u64
                }
                None => 0,
            };
            let (pending, pending_oldest_ms) = pending_of.get(&t.id).copied().unwrap_or_default();
            rows.push(TaskRow {
                id: t.id,
                kind: t.kind.name(),
                title: t.title,
                pid,
                cpu: (t.cpu * 10.0).round() / 10.0,
                memory: t.memory,
                working_set: mem.working_set,
                private: mem.private,
                network,
                pending,
                pending_oldest_ms,
                killable: t.killable && t.kind != TaskKind::Browser,
                tabs: tabs_of.remove(&t.id).unwrap_or_default(),
            });
        }
        // Forget tasks that are gone.
        let live: std::collections::HashSet<i64> = rows.iter().map(|r| r.id).collect();
        last.retain(|id, _| live.contains(id));
        rows
    }

    /// The page's "End process": never the browser process; a renderer's tabs show the
    /// crashed-tab page through the existing `on_render_process_terminated` path.
    pub(super) fn kill_task(&self, tab: Option<TabId>, req_id: Option<u32>, id: i64) {
        let row = self.task_rows().into_iter().find(|r| r.id == id);
        let Some(row) = row else {
            return self.reply(tab, Event::err(req_id, "That task is gone"));
        };
        if !row.killable {
            return self.reply(tab, Event::err(req_id, "This process cannot be ended"));
        }
        info!("task manager: ending task {id} ({} '{}', pid {:?})", row.kind, row.title, row.pid);
        if tasks::kill(id) {
            self.reply(tab, Event::ok(req_id, serde_json::json!({ "ended": true })));
        } else {
            self.reply(tab, Event::err(req_id, "The engine refused to end this process"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::utility_matches;

    #[test]
    fn utility_titles_match_their_command_lines() {
        let net = "Krunk.exe --type=utility --utility-sub-type=network.mojom.NetworkService --lang=en-US";
        let audio = "Krunk.exe --type=utility --utility-sub-type=audio.mojom.AudioService";
        assert!(utility_matches("Utility: Network Service", net));
        assert!(!utility_matches("Utility: Network Service", audio));
        assert!(utility_matches("Utility: Audio Service", audio));
        assert!(!utility_matches("GPU Process", net));
    }
}
