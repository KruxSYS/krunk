//! The automatic context block and the system prompt (WP-B1).

use serde::{Deserialize, Serialize};

/// A tab as the context block lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabFacts {
    pub id: u32,
    pub title: String,
    pub url: String,
    pub active: bool,
    pub private: bool,
    pub loading: bool,
}

/// What the host knows when a task starts.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ContextInput {
    /// The target (active) tab.
    pub tab: Option<TabFacts>,
    /// All tabs of the window.
    pub tabs: Vec<TabFacts>,
    /// The probe's `context()` result for the target tab (selection, viewport, scroll, outline);
    /// `None` on built-in pages or when the page didn't answer.
    pub page: Option<serde_json::Value>,
    /// Why `page` is missing ("Krunk's own page: its content is not available to the agent").
    pub page_note: Option<String>,
    /// Names of the tools offered for this task.
    pub tools: Vec<&'static str>,
}

/// The block prepended to the user's message (Markdown-ish plain text, capped at 6,000 chars).
/// Page-provided strings (titles, labels, selection) are quoted as data.
pub fn context_block(input: &ContextInput) -> String {
    let _ = input;
    String::new() // WP-B1
}

/// The system prompt: role, the untrusted-data rule, tool usage rules, Krunk facts (built-in
/// pages the agent may open, the settings it may change), and the user's addition.
pub fn system_prompt(extra: &str, vision: bool) -> String {
    let _ = (extra, vision);
    String::new() // WP-B1
}
