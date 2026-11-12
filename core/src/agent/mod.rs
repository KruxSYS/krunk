//! The optional AI agent (0.12.0): everything about it that is not Windows or engine glue.
//!
//! The agent loop runs in the browser process. This module holds its pure parts, so they are
//! tested on Linux:
//! * [`tools`]: the tool table with JSON schemas and typed arguments;
//! * [`provider`] and [`sse`]: provider request bodies and streamed responses (OpenAI-style for
//!   OpenRouter, OpenAI and custom servers; Anthropic's Messages API);
//! * [`session`]: the conversation state machine (`Input` in, `Action`s out);
//! * [`context`]: the automatic context block and the system prompt;
//! * [`redact`]: sensitive form fields, scrubbing and paging of tool results;
//! * [`gate`]: the keyword gate for history, bookmarks, downloads, cookies and (0.16.0)
//!   entering passwords;
//! * [`settings_allow`]: the settings the agent may read and change;
//! * [`pages`]: which addresses the agent may open or act on;
//! * [`drafts`] (0.15.0): long userscripts and macros saved in parts;
//! * [`scratch`] (0.16.0): the scratchpad, the model's notes and plan (in memory only);
//! * [`ntp_image`] (0.15.0): the checks of a new tab background downloaded for the agent.
//!
//! Nothing here can reach the vault, payment cards, sync or account secrets, or API keys: those
//! types are not imported by any agent module (a test in [`tools`] checks the tool table too).
//! 0.16.0: one tool, `fill_password`, writes password fields, on the user's explicit request
//! and after the user allowed the site; it reads nothing, and a password Krunk generates for it
//! never passes through this module.

pub mod context;
pub mod drafts;
pub mod gate;
pub mod ntp_image;
pub mod pages;
pub mod provider;
pub mod redact;
pub mod scratch;
pub mod session;
pub mod settings_allow;
pub mod sse;
pub mod tools;

use serde::{Deserialize, Serialize};

/// A model provider the user configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    OpenRouter,
    OpenAi,
    Anthropic,
    /// Any OpenAI-compatible server (Ollama, LM Studio, vLLM, …) at the user's base URL.
    Custom,
}

/// The wire format a provider speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiStyle {
    /// `POST {base}/chat/completions`, `GET {base}/models`, Bearer key.
    OpenAi,
    /// `POST {base}/messages`, `GET {base}/models`, `x-api-key` + `anthropic-version`.
    Anthropic,
}

impl ProviderKind {
    pub const ALL: [ProviderKind; 4] =
        [ProviderKind::OpenRouter, ProviderKind::OpenAi, ProviderKind::Anthropic, ProviderKind::Custom];

    /// The fixed base URL of a named provider; empty for `Custom` (the user's).
    pub fn default_base_url(self) -> &'static str {
        match self {
            ProviderKind::OpenRouter => "https://openrouter.ai/api/v1",
            ProviderKind::OpenAi => "https://api.openai.com/v1",
            ProviderKind::Anthropic => "https://api.anthropic.com/v1",
            ProviderKind::Custom => "",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ProviderKind::OpenRouter => "OpenRouter",
            ProviderKind::OpenAi => "OpenAI",
            ProviderKind::Anthropic => "Anthropic",
            ProviderKind::Custom => "Custom (OpenAI-compatible)",
        }
    }

    pub fn style(self) -> ApiStyle {
        match self {
            ProviderKind::Anthropic => ApiStyle::Anthropic,
            _ => ApiStyle::OpenAi,
        }
    }

    /// A custom server may run without a key (a local Ollama); the others need one.
    pub fn needs_key(self) -> bool {
        self != ProviderKind::Custom
    }
}

/// A model from a provider's model list (or typed by hand).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    /// Display name; the id when the provider gives none.
    pub name: String,
    /// Takes images: `None` = the provider doesn't say.
    pub vision: Option<bool>,
    /// Supports tool (function) calls: `None` = the provider doesn't say.
    pub tools: Option<bool>,
    /// Context window in tokens, when known.
    pub context: Option<u32>,
    /// 0.15.0: the most tokens one answer may have, when the model list says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output: Option<u32>,
}

/// Sensitive data the agent sees only when the user asks for it (keyword gate plus a
/// per-conversation Allow/Deny). 0.16.0: `Passwords` is not data it sees but something it may
/// do on request, entering a password (`fill_password`); its Allow/Deny is per site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    History,
    Bookmarks,
    Downloads,
    Cookies,
    Passwords,
}

impl Category {
    pub const ALL: [Category; 5] =
        [Category::History, Category::Bookmarks, Category::Downloads, Category::Cookies, Category::Passwords];

    fn bit(self) -> u8 {
        1 << (self as u8)
    }

    pub fn label(self) -> &'static str {
        match self {
            Category::History => "your browsing history",
            Category::Bookmarks => "your bookmarks",
            Category::Downloads => "your downloads list",
            Category::Cookies => "cookies of a site",
            Category::Passwords => "entering a password on a site",
        }
    }
}

/// A small set of [`Category`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CategorySet(u8);

impl CategorySet {
    pub const EMPTY: CategorySet = CategorySet(0);

    pub fn insert(&mut self, c: Category) {
        self.0 |= c.bit();
    }
    pub fn contains(self, c: Category) -> bool {
        self.0 & c.bit() != 0
    }
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
    pub fn iter(self) -> impl Iterator<Item = Category> {
        Category::ALL.into_iter().filter(move |c| self.contains(*c))
    }
}

impl FromIterator<Category> for CategorySet {
    fn from_iter<I: IntoIterator<Item = Category>>(iter: I) -> Self {
        let mut s = CategorySet::EMPTY;
        for c in iter {
            s.insert(c);
        }
        s
    }
}

// ---- the provider-neutral conversation ----

/// Who said a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    /// A tool result (answers `tool_call_id`).
    Tool,
}

/// An image for the model (base64, no `data:` prefix).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    /// `image/jpeg` or `image/png`.
    pub mime: String,
    pub base64: String,
}

/// A part of a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Part {
    Text(String),
    Image(Image),
}

/// A tool call the model made: `arguments` is its raw JSON text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

/// One message of the conversation as it is sent to the provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    pub parts: Vec<Part>,
    /// Assistant messages: the tool calls it made.
    pub tool_calls: Vec<ToolCall>,
    /// Tool messages: the call this answers.
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Message {
        Message { role: Role::User, parts: vec![Part::Text(text.into())], tool_calls: Vec::new(), tool_call_id: None }
    }
    pub fn assistant(text: impl Into<String>, tool_calls: Vec<ToolCall>) -> Message {
        let text = text.into();
        let parts = if text.is_empty() { Vec::new() } else { vec![Part::Text(text)] };
        Message { role: Role::Assistant, parts, tool_calls, tool_call_id: None }
    }
    pub fn tool(call_id: impl Into<String>, text: impl Into<String>, image: Option<Image>) -> Message {
        let mut parts = vec![Part::Text(text.into())];
        if let Some(i) = image {
            parts.push(Part::Image(i));
        }
        Message { role: Role::Tool, parts, tool_calls: Vec::new(), tool_call_id: Some(call_id.into()) }
    }
    /// The message's text parts joined.
    pub fn text(&self) -> String {
        self.parts
            .iter()
            .filter_map(|p| match p {
                Part::Text(t) => Some(t.as_str()),
                Part::Image(_) => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// A tool as the provider sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON schema of the arguments (an object schema).
    pub parameters: serde_json::Value,
}

/// Everything one provider call needs except the endpoint and the key.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatRequest {
    pub model: String,
    pub system: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
    pub max_tokens: u32,
}

/// Why the model stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    Other(String),
}

/// A provider's streamed answer, mapped to one vocabulary (see [`provider`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    TextDelta(String),
    /// A tool call starts; `index` orders the calls of one answer.
    ToolCallStart {
        index: usize,
        id: String,
        name: String,
    },
    /// More of a tool call's JSON arguments.
    ToolCallArgs {
        index: usize,
        fragment: String,
    },
    Usage {
        input: u32,
        output: u32,
    },
    Done(StopReason),
    /// An error inside the stream (the provider's message, shortened).
    Error(String),
}

// ---- what the panel shows ----

/// State of a window's agent, for the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunState {
    Idle,
    Running,
    /// Waiting for the user: an `ask_user` question, a consent or a "continue?".
    Asking,
    Stopped,
    Done,
    Error,
    /// The step limit was reached; the panel asks whether to continue.
    MaxSteps,
}

/// Status of one step in the step log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StepStatus {
    Running,
    Ok,
    Error,
    /// The tool refused (built-in page, sensitive field, denied by the user).
    Refused,
    Cancelled,
}

/// One tool call in the step log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepView {
    /// 1-based step number within the conversation.
    pub n: u32,
    pub tool: String,
    /// A short human label ("Click #submit", "Read the page's links").
    pub label: String,
    pub status: StepStatus,
    /// One line of result ("12 links", "Title set").
    #[serde(default)]
    pub summary: String,
    /// Expandable details: the (redacted) arguments and the start of the result.
    #[serde(default)]
    pub detail: String,
    /// A screenshot of this step, served to the panel only (`/agent-shot/<token>/<n>.jpg`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Milliseconds the tool took.
    #[serde(default)]
    pub ms: u64,
}

/// An `ask_user` question as the panel renders it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AskSpec {
    pub id: u32,
    pub question: String,
    pub options: Vec<String>,
    pub multi: bool,
    pub allow_other: bool,
}

/// What the user answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskAnswer {
    /// Indexes into `AskSpec::options`.
    pub choices: Vec<u8>,
    pub other: Option<String>,
}

/// Userscript or macro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScriptKind {
    Userscript,
    Macro,
}

/// A script the agent created or changed, shown in the panel with Disable / Open in editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptCard {
    pub id: String,
    pub kind: ScriptKind,
    pub name: String,
    pub enabled: bool,
}

/// A model the panel's quick switch offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelChoice {
    pub provider: String,
    pub provider_name: String,
    pub model: String,
}

/// The result of running one tool, handed back to the session.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolOutcome {
    /// What the model reads: already redacted, scrubbed and capped by the app.
    pub text: String,
    /// A screenshot for the model (only when the model takes images).
    pub image: Option<Image>,
    /// The panel's copy of the screenshot (`/agent-shot/…`), if one was taken.
    pub image_url: Option<String>,
    /// One line for the step log.
    pub summary: String,
    pub status: Option<StepStatus>,
    /// A userscript or macro the tool created or changed.
    pub script: Option<ScriptCard>,
}

impl ToolOutcome {
    pub fn ok(text: impl Into<String>, summary: impl Into<String>) -> ToolOutcome {
        ToolOutcome { text: text.into(), summary: summary.into(), status: Some(StepStatus::Ok), ..Default::default() }
    }
    /// A failure the model can adapt to: `{"error": "…"}`.
    pub fn error(message: impl Into<String>) -> ToolOutcome {
        let m = message.into();
        ToolOutcome {
            text: serde_json::json!({ "error": m }).to_string(),
            summary: m,
            status: Some(StepStatus::Error),
            ..Default::default()
        }
    }
    /// A refusal (built-in page, sensitive field, denied).
    pub fn refused(message: impl Into<String>) -> ToolOutcome {
        ToolOutcome { status: Some(StepStatus::Refused), ..ToolOutcome::error(message) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_kinds_round_trip_and_defaults() {
        for k in ProviderKind::ALL {
            let j = serde_json::to_string(&k).unwrap();
            assert_eq!(serde_json::from_str::<ProviderKind>(&j).unwrap(), k);
        }
        assert_eq!(serde_json::to_string(&ProviderKind::OpenRouter).unwrap(), "\"openrouter\"");
        assert_eq!(ProviderKind::Anthropic.style(), ApiStyle::Anthropic);
        assert_eq!(ProviderKind::Custom.style(), ApiStyle::OpenAi);
        assert!(ProviderKind::Custom.default_base_url().is_empty());
        assert!(ProviderKind::OpenAi.needs_key() && !ProviderKind::Custom.needs_key());
    }

    #[test]
    fn category_set() {
        let mut s = CategorySet::EMPTY;
        assert!(s.is_empty());
        s.insert(Category::Cookies);
        s.insert(Category::History);
        assert!(s.contains(Category::Cookies) && !s.contains(Category::Bookmarks));
        assert_eq!(s.iter().collect::<Vec<_>>(), vec![Category::History, Category::Cookies]);
        let all: CategorySet = Category::ALL.into_iter().collect();
        assert_eq!(all.iter().count(), 5);
        assert_eq!(serde_json::to_string(&Category::Passwords).unwrap(), "\"passwords\"");
    }

    #[test]
    fn outcomes() {
        let e = ToolOutcome::refused("password field");
        assert_eq!(e.status, Some(StepStatus::Refused));
        assert_eq!(e.text, r#"{"error":"password field"}"#);
        assert_eq!(Message::tool("c1", "x", None).tool_call_id.as_deref(), Some("c1"));
        assert_eq!(Message::assistant("", vec![]).parts.len(), 0);
    }
}
