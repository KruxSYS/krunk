//! The agent's conversation state machine (WP-B1).
//!
//! The host feeds [`Input`]s (the user's message, provider stream events, tool results, the
//! user's answers, Stop) and carries out the [`Action`]s that come back (send a provider request,
//! run a tool, cancel, show something in the panel). Everything here is synchronous and pure, so
//! the whole loop is tested on Linux with a scripted provider.
//!
//! Rules it implements:
//! * a task = one user message; its tools come from [`super::tools::offered`] with the gate
//!   ([`super::gate::requested`]) applied to that message;
//! * every tool call is validated ([`super::tools::parse_call`]); unknown, not-offered or
//!   invalid calls get an error result without reaching the host;
//! * `ask_user` and `finish` are handled here; a plain-text answer without tool calls ends the
//!   task like `finish`;
//! * the first call of a gated tool per conversation and category asks for consent;
//! * after `max_steps` tool calls the session asks whether to continue (another `max_steps`);
//! * `Stop` cancels the request and any running tool, releases held input and ends the task
//!   ("Stopped"); a pending question is answered with `{"cancelled":true}`;
//! * only the last 2 images stay in the history sent to the provider; old tool results are
//!   shortened.

use super::{AskAnswer, RunState, StreamEvent, ToolOutcome};
use crate::protocol::Event;
use serde_json::Value;

/// How the session is set up (from settings and the chosen model).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub model: String,
    /// Shown in the panel's model indicator ("OpenRouter · openai/gpt-4.1-mini").
    pub provider_name: String,
    /// The model takes images (screenshots are sent as images).
    pub vision: bool,
    pub max_steps: u32,
    /// The user's addition to the system prompt.
    pub system_extra: String,
    pub userscripts: bool,
    pub macros: bool,
    pub max_tokens: u32,
}

/// Something that happened.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// The user sent a message; `context` is the automatic context block the host built
    /// ([`super::context::context_block`]).
    User { text: String, context: String },
    /// A streamed event of the running provider request.
    Stream(StreamEvent),
    /// The provider response finished normally.
    StreamEnd,
    /// The provider request failed (network error or HTTP status: a message for the panel).
    StreamFailed(String),
    /// A tool the host ran finished.
    ToolDone { call_id: String, outcome: ToolOutcome },
    /// The user answered an `ask_user` question.
    Answer { ask: u32, answer: AskAnswer },
    /// The user allowed or denied a sensitive category.
    Consent { ask: u32, allow: bool },
    /// "Continue for another N steps?" after the step limit.
    Continue { ask: u32, go: bool },
    /// Stop button, Esc in the panel, or the window closes.
    Stop,
    /// "New conversation": stop, then forget everything.
    New,
}

/// What the host has to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Send this request to the active provider (streamed).
    Send(super::ChatRequest),
    /// Cancel the running provider request.
    CancelRequest,
    /// Run a tool. `args` are validated; `step` is its step number.
    RunTool { call_id: String, name: &'static str, args: Value, step: u32 },
    /// Cancel the running tool (its result is no longer wanted).
    CancelTool,
    /// Release every key and mouse button a tool or agent-started macro holds.
    ReleaseInput,
    /// Post this event to the window's panel (boxed: events are large).
    Emit(Box<Event>),
    /// A log line (step names, tools and statuses only; never content).
    Log(String),
}

/// One window's conversation.
#[derive(Debug)]
pub struct Session {
    cfg: Config,
    state: RunState,
    steps: u32,
}

impl Session {
    pub fn new(cfg: Config) -> Session {
        Session { cfg, state: RunState::Idle, steps: 0 }
    }

    /// The model or settings changed (applies to the next request).
    pub fn set_config(&mut self, cfg: Config) {
        self.cfg = cfg;
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    pub fn state(&self) -> RunState {
        self.state
    }

    /// Tool calls made in this conversation so far.
    pub fn steps(&self) -> u32 {
        self.steps
    }

    /// A task is running or waiting for the user.
    pub fn is_busy(&self) -> bool {
        matches!(self.state, RunState::Running | RunState::Asking | RunState::MaxSteps)
    }

    /// Handles one input; `now_ms` is a monotonic clock for step durations.
    pub fn handle(&mut self, input: Input, now_ms: u64) -> Vec<Action> {
        let _ = (input, now_ms);
        Vec::new() // WP-B1
    }
}
