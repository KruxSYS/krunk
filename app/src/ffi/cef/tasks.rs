//! The engine's task manager (CEF 156): every task (browser, GPU, renderers with their
//! tabs, utilities, workers) with its CPU and memory, and ending one. The observer must stay
//! alive for the list to fill, so one instance is kept for the process.

use cef::*;
use std::cell::RefCell;

/// What a task is (CEF's task types, grouped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    Browser,
    Gpu,
    Renderer,
    Utility,
    Extension,
    Worker,
    Other,
}

impl TaskKind {
    pub fn name(self) -> &'static str {
        match self {
            TaskKind::Browser => "browser",
            TaskKind::Gpu => "gpu",
            TaskKind::Renderer => "renderer",
            TaskKind::Utility => "utility",
            TaskKind::Extension => "extension",
            TaskKind::Worker => "worker",
            TaskKind::Other => "other",
        }
    }
}

/// One task as the engine reports it.
#[derive(Debug, Clone)]
pub struct TaskRaw {
    pub id: i64,
    pub kind: TaskKind,
    pub killable: bool,
    pub title: String,
    /// Percent of all processors (0–100).
    pub cpu: f64,
    /// Private memory footprint in bytes.
    pub memory: u64,
}

thread_local! {
    static MANAGER: RefCell<Option<TaskManager>> = const { RefCell::new(None) };
}

fn manager() -> Option<TaskManager> {
    MANAGER.with(|m| {
        if m.borrow().is_none() {
            *m.borrow_mut() = task_manager_get();
        }
        m.borrow().clone()
    })
}

/// Every task the engine knows.
pub fn tasks() -> Vec<TaskRaw> {
    let Some(tm) = manager() else { return Vec::new() };
    // The binding passes the vector's length as the capacity.
    let mut ids = vec![0i64; tm.tasks_count()];
    tm.task_ids_list(Some(&mut ids));
    ids.into_iter().filter_map(|id| task_info(&tm, id)).collect()
}

/// The task of a browser (shell, popup or tab), by `Browser::identifier()`.
pub fn task_for_browser(browser_id: i32) -> Option<i64> {
    manager().map(|tm| tm.task_id_for_browser_id(browser_id)).filter(|id| *id >= 0)
}

/// Ends the process behind a task; the engine refuses the browser process itself.
pub fn kill(id: i64) -> bool {
    manager().is_some_and(|tm| tm.kill_task(id) != 0)
}

/// The cef crate's `task_info` wrapper copies the struct and never writes it back, so the
/// C function is called directly.
fn task_info(tm: &TaskManager, id: i64) -> Option<TaskRaw> {
    use cef::sys::{_cef_task_info_t, cef_string_utf16_clear, cef_task_type_t as T};
    let raw = tm.get_raw();
    // SAFETY: `raw` is the live task manager object (the wrapper keeps a reference); the
    // struct is zeroed with its size set, as the C API requires; the title string the
    // engine filled is read within its length and freed with the engine's own clear.
    unsafe {
        let f = (*raw).get_task_info?;
        let mut info: _cef_task_info_t = std::mem::zeroed();
        info.size = std::mem::size_of::<_cef_task_info_t>();
        if f(raw, id, &mut info) == 0 {
            return None;
        }
        let title = if info.title.str_.is_null() {
            String::new()
        } else {
            String::from_utf16_lossy(std::slice::from_raw_parts(info.title.str_, info.title.length))
        };
        cef_string_utf16_clear(&mut info.title);
        let kind = match info.type_ {
            T::CEF_TASK_TYPE_BROWSER => TaskKind::Browser,
            T::CEF_TASK_TYPE_GPU => TaskKind::Gpu,
            T::CEF_TASK_TYPE_RENDERER | T::CEF_TASK_TYPE_GUEST => TaskKind::Renderer,
            T::CEF_TASK_TYPE_UTILITY | T::CEF_TASK_TYPE_ZYGOTE | T::CEF_TASK_TYPE_SANDBOX_HELPER => TaskKind::Utility,
            T::CEF_TASK_TYPE_EXTENSION => TaskKind::Extension,
            T::CEF_TASK_TYPE_DEDICATED_WORKER | T::CEF_TASK_TYPE_SHARED_WORKER | T::CEF_TASK_TYPE_SERVICE_WORKER => {
                TaskKind::Worker
            }
            _ => TaskKind::Other,
        };
        let processors = info.number_of_processors.max(1) as f64;
        Some(TaskRaw {
            id: info.id,
            kind,
            killable: info.is_killable != 0,
            title,
            cpu: if info.cpu_usage.is_finite() { (info.cpu_usage / processors).clamp(0.0, 100.0) } else { 0.0 },
            memory: info.memory.max(0) as u64,
        })
    }
}
