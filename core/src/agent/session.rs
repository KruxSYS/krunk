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
//! * the first call of a gated tool per conversation and category asks for consent (the
//!   answer, allow or deny, holds for the rest of the conversation);
//! * after `max_steps` tool calls in a task the session asks whether to continue (another
//!   `max_steps`);
//! * `Stop` cancels the request and any running tool, releases held input and ends the task
//!   ("Stopped"); a pending question is answered with `{"cancelled":true}`; results that arrive
//!   later are ignored;
//! * only the last 2 images stay in the history sent to the provider; tool results older than
//!   the last 8 are shortened;
//! * every assistant tool call in the history gets exactly one tool result (missing ones are
//!   filled with `{"cancelled":true}`), so the history is always valid for the providers;
//! * 0.15.0, the output limit (`max-tokens` stop): an answer with neither text nor a tool call
//!   is asked for once more with a higher cap ([`limits::raised_max_tokens`]), then fails with
//!   an error that names the cap used. A tool call cut off mid-arguments is never run: the
//!   request is retried once with a higher cap, then the model is told (as the call's result)
//!   that it was cut off and nothing ran, at most [`MAX_CUT_CALLS`] times in a row. A text
//!   answer cut off is continued once ([`CONTINUE_NOTE`]);
//! * 0.15.0, long scripts: `save_userscript` / `save_macro` with `more: true` keep their source
//!   in a draft of the conversation ([`super::drafts`]); the call with the draft id and without
//!   `more` sends the whole script to the host, which checks it only then.
//!
//! The user message sent to the provider is the context block, a blank line, then
//! `User message:` and the user's text on the next line (the mock provider looks for that marker).
//!
//! `AgentState.steps` and `AgentContinueAsk.steps` count the tool calls of the current task;
//! [`StepView::n`] and [`Session::steps`] count those of the whole conversation.

use super::drafts::Drafts;
use super::provider::limits;
use super::tools::{self, Group, Offer};
use super::{
    context, gate, AskAnswer, AskSpec, Category, CategorySet, ChatRequest, Message, Part, Role, RunState, ScriptKind,
    StepStatus, StepView, StopReason, StreamEvent, ToolCall, ToolOutcome,
};
use crate::protocol::Event;
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};

/// The marker between the context block and the user's own words.
pub const USER_MARKER: &str = "User message:";
/// Images kept in the history sent to the provider (the newest).
pub const KEEP_IMAGES: usize = 2;
/// Tool results kept whole (the newest); older ones are shortened.
pub const KEEP_TOOL_RESULTS: usize = 8;
/// Older tool results are cut to this many characters.
pub const SHORTENED_CHARS: usize = 500;
/// What replaces an image that was dropped from the history.
pub const IMAGE_REMOVED: &str = "[an earlier screenshot was removed]";
const SHORTENED_TAIL: &str = "…[shortened]";
const CANCELLED: &str = r#"{"cancelled":true}"#;
const NOT_OFFERED: &str = "this tool is not available for this task";
const DENIED: &str = "the user denied access";
/// The user message that asks for the rest of a text answer cut off by the output limit.
pub const CONTINUE_NOTE: &str =
    "[Krunk] Your answer was cut off at the output limit. Continue exactly where it stopped, without repeating anything.";
/// Answers in a row with a cut-off tool call that are handed back to the model.
pub const MAX_CUT_CALLS: u32 = 2;

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
    /// The requests' output cap (0 = left out: the provider's default); see
    /// [`limits::request_max_tokens`].
    pub max_tokens: u32,
    /// 0.15.0: the model's known output limit (it bounds a retry's higher cap).
    pub known_max_output: Option<u32>,
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

/// The assistant's answer collected from the stream.
#[derive(Debug, Default)]
struct Answer {
    text: String,
    calls: BTreeMap<usize, ToolCall>,
    stop: Option<StopReason>,
    error: Option<String>,
}

/// What the session waits for.
#[derive(Debug, Default)]
enum Phase {
    #[default]
    Idle,
    /// A provider request streams.
    Requesting,
    /// The host runs a tool.
    Tool { call_id: String, step: StepView, started: u64 },
    /// An `ask_user` question is open.
    Ask { ask: u32, call_id: String, spec: AskSpec },
    /// A gated tool waits for Allow/Deny.
    Consent { ask: u32, call_id: String, name: &'static str, args: Value, category: Category },
    /// The step limit was reached: "continue?".
    Continue { ask: u32 },
}

/// One window's conversation.
#[derive(Debug)]
pub struct Session {
    cfg: Config,
    state: RunState,
    /// Tool calls in this conversation.
    steps: u32,
    history: Vec<Message>,
    /// Consent decisions of this conversation.
    allowed: CategorySet,
    denied: CategorySet,
    /// The tools of the current task.
    offer: Offer,
    phase: Phase,
    answer: Answer,
    /// Tool calls of the current answer not handled yet (in order).
    pending: VecDeque<ToolCall>,
    last_ask: u32,
    /// Tool calls in the current task, and when the next "continue?" comes.
    task_steps: u32,
    step_limit: u32,
    task_started: u64,
    /// 0.15.0: the higher output cap of this task after a retry; whether the current answer
    /// was already retried; cut-off tool calls handed back in a row; the text of a cut-off
    /// answer being continued; drafts of long scripts.
    raise: Option<u32>,
    raised: bool,
    cut_calls: u32,
    carry: String,
    drafts: Drafts,
}

/// The arguments are whole JSON (empty counts as `{}`).
fn complete_json(arguments: &str) -> bool {
    let t = arguments.trim();
    t.is_empty() || serde_json::from_str::<Value>(t).is_ok()
}

/// "its output limit (32,000 tokens)", or the provider's default when no cap was sent.
fn limit_phrase(max_tokens: u32) -> String {
    if max_tokens == 0 {
        "the provider's default output limit".into()
    } else {
        format!("its output limit ({} tokens)", limits::thousands(max_tokens))
    }
}

fn emit(out: &mut Vec<Action>, ev: Event) {
    out.push(Action::Emit(Box::new(ev)));
}

fn status_name(s: StepStatus) -> &'static str {
    match s {
        StepStatus::Running => "running",
        StepStatus::Ok => "ok",
        StepStatus::Error => "error",
        StepStatus::Refused => "refused",
        StepStatus::Cancelled => "cancelled",
    }
}

fn error_json(message: &str) -> String {
    json!({ "error": message }).to_string()
}

/// The first `max` characters of `s`, with "…" when cut.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max).collect();
        t.push('…');
        t
    }
}

impl Session {
    pub fn new(cfg: Config) -> Session {
        Session {
            cfg,
            state: RunState::Idle,
            steps: 0,
            history: Vec::new(),
            allowed: CategorySet::EMPTY,
            denied: CategorySet::EMPTY,
            offer: Offer { userscripts: false, macros: false, gated: CategorySet::EMPTY },
            phase: Phase::Idle,
            answer: Answer::default(),
            pending: VecDeque::new(),
            last_ask: 0,
            task_steps: 0,
            step_limit: 0,
            task_started: 0,
            raise: None,
            raised: false,
            cut_calls: 0,
            carry: String::new(),
            drafts: Drafts::default(),
        }
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

    /// The conversation as it is stored (for tests and diagnostics; the provider gets
    /// [`request_messages`] of it).
    pub fn history(&self) -> &[Message] {
        &self.history
    }

    /// Handles one input; `now_ms` is a monotonic clock for step durations.
    pub fn handle(&mut self, input: Input, now_ms: u64) -> Vec<Action> {
        let mut out = Vec::new();
        let requesting = matches!(self.phase, Phase::Requesting);
        match input {
            Input::User { text, context } => self.on_user(&text, &context, now_ms, &mut out),
            Input::Stream(ev) if requesting => self.on_stream(ev, &mut out),
            Input::StreamEnd if requesting => self.on_end(now_ms, &mut out),
            Input::StreamFailed(msg) if requesting => self.fail(msg, now_ms, &mut out),
            Input::Stream(_) | Input::StreamEnd | Input::StreamFailed(_) => {} // late: ignored
            Input::ToolDone { call_id, outcome } => self.on_tool_done(&call_id, outcome, now_ms, &mut out),
            Input::Answer { ask, answer } => self.on_answer(ask, answer, now_ms, &mut out),
            Input::Consent { ask, allow } => self.on_consent(ask, allow, now_ms, &mut out),
            Input::Continue { ask, go } => self.on_continue(ask, go, now_ms, &mut out),
            Input::Stop => self.stop(now_ms, &mut out),
            Input::New => {
                self.stop(now_ms, &mut out);
                self.history.clear();
                self.allowed = CategorySet::EMPTY;
                self.denied = CategorySet::EMPTY;
                self.steps = 0;
                self.task_steps = 0;
                self.drafts.clear();
                emit(&mut out, Event::AgentClear);
                self.set_state(RunState::Idle, None, &mut out);
            }
        }
        out
    }

    fn set_state(&mut self, state: RunState, ms: Option<u64>, out: &mut Vec<Action>) {
        self.state = state;
        emit(out, Event::AgentState { state, steps: self.task_steps, ms });
    }

    fn elapsed(&self, now: u64) -> Option<u64> {
        Some(now.saturating_sub(self.task_started))
    }

    fn next_ask(&mut self) -> u32 {
        self.last_ask += 1;
        self.last_ask
    }

    fn push_result(&mut self, call_id: &str, text: impl Into<String>) {
        self.history.push(Message::tool(call_id, text, None));
    }

    // ---- a task ----

    fn on_user(&mut self, text: &str, context: &str, now: u64, out: &mut Vec<Action>) {
        if self.is_busy() {
            emit(
                out,
                Event::AgentMessage {
                    role: "notice".into(),
                    text: "The agent is still working on the last task. Wait for it or press Stop.".into(),
                },
            );
            return;
        }
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.offer = Offer { userscripts: self.cfg.userscripts, macros: self.cfg.macros, gated: gate::requested(text) };
        let body = if context.trim().is_empty() {
            format!("{USER_MARKER}\n{text}")
        } else {
            format!("{}\n\n{USER_MARKER}\n{text}", context.trim_end())
        };
        self.history.push(Message::user(body));
        self.pending.clear();
        self.task_steps = 0;
        self.raise = None;
        self.answered();
        self.step_limit = self.cfg.max_steps.max(1);
        self.task_started = now;
        emit(out, Event::AgentMessage { role: "user".into(), text: text.to_string() });
        self.set_state(RunState::Running, None, out);
        self.send(out);
    }

    fn send(&mut self, out: &mut Vec<Action>) {
        self.answer = Answer::default();
        self.phase = Phase::Requesting;
        out.push(Action::Send(ChatRequest {
            model: self.cfg.model.clone(),
            system: context::system_prompt(&self.cfg.system_extra, self.cfg.vision),
            messages: request_messages(&self.history),
            tools: tools::specs(self.offer),
            max_tokens: self.max_tokens(),
        }));
    }

    /// The output cap of the next request.
    fn max_tokens(&self) -> u32 {
        self.raise.unwrap_or(self.cfg.max_tokens)
    }

    /// An answer was handled normally: the next cut-off one gets its retry again.
    fn answered(&mut self) {
        self.raised = false;
        self.cut_calls = 0;
        self.carry.clear();
    }

    /// `Raise Max output tokens for OpenRouter in Settings → AI.`
    fn raise_hint(&self) -> String {
        let name = if self.cfg.provider_name.trim().is_empty() { "this provider" } else { &self.cfg.provider_name };
        format!("Raise Max output tokens for {name} in Settings → AI.")
    }

    /// Retries the request once with a higher output cap; false when that was done already
    /// or the cap can't go higher.
    fn retry_higher(&mut self, why: &str, out: &mut Vec<Action>) -> bool {
        let used = self.max_tokens();
        let next = if self.raised { None } else { limits::raised_max_tokens(used, self.cfg.known_max_output) };
        let Some(next) = next else { return false };
        self.raised = true;
        self.raise = Some(next);
        let text = format!("{why} Trying again with up to {} tokens.", limits::thousands(next));
        emit(out, Event::AgentMessage { role: "notice".into(), text });
        out.push(Action::Log(format!("agent output limit: retrying with {next}")));
        self.send(out);
        true
    }

    fn on_stream(&mut self, ev: StreamEvent, out: &mut Vec<Action>) {
        let a = &mut self.answer;
        match ev {
            StreamEvent::TextDelta(t) => {
                if !t.is_empty() {
                    a.text.push_str(&t);
                    emit(out, Event::AgentDelta { text: t });
                }
            }
            StreamEvent::ToolCallStart { index, id, name } => {
                let c = a.calls.entry(index).or_insert_with(|| ToolCall {
                    id: String::new(),
                    name: String::new(),
                    arguments: String::new(),
                });
                if !id.is_empty() {
                    c.id = id;
                }
                if !name.is_empty() {
                    c.name = name;
                }
            }
            StreamEvent::ToolCallArgs { index, fragment } => {
                a.calls
                    .entry(index)
                    .or_insert_with(|| ToolCall { id: String::new(), name: String::new(), arguments: String::new() })
                    .arguments
                    .push_str(&fragment);
            }
            StreamEvent::Usage { .. } => {}
            StreamEvent::Done(r) => a.stop = Some(r),
            StreamEvent::Error(m) => a.error = Some(m),
        }
    }

    fn on_end(&mut self, now: u64, out: &mut Vec<Action>) {
        let a = std::mem::take(&mut self.answer);
        if let Some(e) = a.error {
            return self.fail(e, now, out);
        }
        let cut = a.stop == Some(StopReason::MaxTokens);
        if !a.calls.is_empty() {
            let mut calls: Vec<ToolCall> = a.calls.into_values().collect();
            // Every call needs a unique id for its result.
            let base = self.history.len();
            for i in 0..calls.len() {
                if calls[i].id.is_empty() || calls[..i].iter().any(|c| c.id == calls[i].id) {
                    calls[i].id = format!("krunk_{base}_{i}");
                }
            }
            if cut && calls.iter().any(|c| !complete_json(&c.arguments)) {
                return self.on_cut_calls(a.text, calls, now, out);
            }
            self.answered();
            self.history.push(Message::assistant(a.text, calls.clone()));
            self.pending = calls.into();
            self.process(now, out);
        } else if !a.text.trim().is_empty() {
            if cut && self.carry.is_empty() {
                // Cut off: the model goes on once; the panel keeps streaming into the same message.
                self.history.push(Message::assistant(a.text.clone(), Vec::new()));
                self.history.push(Message::user(CONTINUE_NOTE));
                self.carry = a.text;
                out.push(Action::Log("agent answer cut off: continuing".into()));
                return self.send(out);
            }
            let full = std::mem::take(&mut self.carry) + &a.text;
            self.answered();
            self.history.push(Message::assistant(a.text, Vec::new()));
            self.done(Some(full), now, out);
            if cut {
                let text = format!(
                    "The answer was cut off at {}. {}",
                    limit_phrase(self.max_tokens()).replacen("its ", "the model's ", 1),
                    self.raise_hint()
                );
                emit(out, Event::AgentMessage { role: "notice".into(), text });
            }
        } else if !self.carry.is_empty() {
            // The continuation added nothing: the first part is the answer.
            let full = std::mem::take(&mut self.carry);
            self.answered();
            self.done(Some(full), now, out);
        } else if cut {
            let used = self.max_tokens();
            let why = format!("The model stopped at {} before answering.", limit_phrase(used));
            if !self.retry_higher(&why, out) {
                let message = format!("{why} {}", self.raise_hint());
                self.fail(message, now, out);
            }
        } else {
            self.fail("The model sent an empty answer.".into(), now, out);
        }
    }

    /// An answer cut off by the output limit in the middle of a tool call's arguments: nothing
    /// of it runs. Retried once with a higher cap; then handed back to the model (each call
    /// gets a result saying so) at most [`MAX_CUT_CALLS`] times in a row; then an error.
    fn on_cut_calls(&mut self, text: String, mut calls: Vec<ToolCall>, now: u64, out: &mut Vec<Action>) {
        let used = self.max_tokens();
        let at = limit_phrase(used);
        let why = format!("The model's answer was cut off at {at} in the middle of a tool call, so nothing ran.");
        if self.retry_higher(&why, out) {
            return;
        }
        if self.cut_calls >= MAX_CUT_CALLS {
            let message = format!("{why} {}", self.raise_hint());
            return self.fail(message, now, out);
        }
        self.cut_calls += 1;
        let cut: Vec<bool> = calls.iter().map(|c| !complete_json(&c.arguments)).collect();
        for (c, &was_cut) in calls.iter_mut().zip(&cut) {
            if was_cut {
                c.arguments = "{}".into();
            }
        }
        self.history.push(Message::assistant(text, calls.clone()));
        let tokens = if used == 0 { String::new() } else { format!(" ({} tokens)", limits::thousands(used)) };
        for (c, was_cut) in calls.iter().zip(cut) {
            let result = if was_cut {
                format!(
                    "this call was cut off at the output limit{tokens} before its arguments were complete, so nothing ran. \
                     Send it again with shorter arguments; save a long userscript or macro in parts \
                     (more=true, then the returned draft id)."
                )
            } else {
                "not run: the answer was cut off at the output limit after this call; call it again if it is still needed"
                    .to_string()
            };
            self.push_result(&c.id, error_json(&result));
        }
        let text = format!("{why} The model was asked to send it in smaller parts.");
        emit(out, Event::AgentMessage { role: "notice".into(), text });
        out.push(Action::Log("agent tool call cut off: handed back".into()));
        self.send(out);
    }

    /// Handles the pending calls of the current answer until one has to wait (a tool runs, a
    /// question is open) or all have results; then the next request (or the step-limit question).
    fn process(&mut self, now: u64, out: &mut Vec<Action>) {
        while let Some(call) = self.pending.pop_front() {
            let (tool, args) = match tools::parse_call(&call.name, &call.arguments) {
                Ok(x) => x,
                Err(e) => {
                    self.push_result(&call.id, error_json(&e));
                    continue;
                }
            };
            if !tools::is_offered(tool.name, self.offer) {
                self.push_result(&call.id, error_json(NOT_OFFERED));
                continue;
            }
            let args = if matches!(tool.name, "save_userscript" | "save_macro") {
                match self.script_part(&call.id, tool.name, args, out) {
                    Some(a) => a,
                    None => continue,
                }
            } else {
                args
            };
            match tool.group {
                Group::Finish => {
                    let answer = args.get("answer").and_then(Value::as_str).unwrap_or_default().to_string();
                    self.push_result(&call.id, r#"{"ok":true}"#);
                    while let Some(rest) = self.pending.pop_front() {
                        self.push_result(&rest.id, CANCELLED);
                    }
                    return self.done(Some(answer), now, out);
                }
                Group::Ask => {
                    let a: tools::args::AskUser = match tools::args::parse(&args) {
                        Ok(a) => a,
                        Err(e) => {
                            self.push_result(&call.id, error_json(&e));
                            continue;
                        }
                    };
                    let id = self.next_ask();
                    let spec = AskSpec {
                        id,
                        question: a.question,
                        options: a.options,
                        multi: a.multi,
                        allow_other: a.allow_other,
                    };
                    emit(out, Event::AgentAsk { ask: spec.clone() });
                    self.phase = Phase::Ask { ask: id, call_id: call.id, spec };
                    return self.set_state(RunState::Asking, None, out);
                }
                Group::Sensitive(category) => {
                    if self.denied.contains(category) {
                        self.push_result(&call.id, error_json(DENIED));
                        continue;
                    }
                    if !self.allowed.contains(category) {
                        let id = self.next_ask();
                        emit(out, Event::AgentConsentAsk { ask: id, category, label: category.label().into() });
                        self.phase = Phase::Consent { ask: id, call_id: call.id, name: tool.name, args, category };
                        return self.set_state(RunState::Asking, None, out);
                    }
                    return self.run_tool(call.id, tool.name, args, now, out);
                }
                _ => return self.run_tool(call.id, tool.name, args, now, out),
            }
        }
        self.phase = Phase::Idle;
        if self.task_steps >= self.step_limit {
            let id = self.next_ask();
            self.phase = Phase::Continue { ask: id };
            emit(out, Event::AgentContinueAsk { ask: id, steps: self.task_steps });
            return self.set_state(RunState::MaxSteps, None, out);
        }
        if self.state != RunState::Running {
            self.set_state(RunState::Running, None, out);
        }
        self.send(out);
    }

    /// A `save_userscript` / `save_macro` call: a part of a long script is kept here (`None`);
    /// the last part makes the whole script the call's `source`. `draft` and `more` are taken
    /// out of what the host gets.
    fn script_part(
        &mut self,
        call_id: &str,
        name: &'static str,
        mut args: Value,
        out: &mut Vec<Action>,
    ) -> Option<Value> {
        let more = args.get("more").and_then(Value::as_bool).unwrap_or(false);
        let draft = args.get("draft").and_then(Value::as_str).map(str::to_string);
        if let Some(o) = args.as_object_mut() {
            o.remove("more");
            o.remove("draft");
        }
        if !more && draft.is_none() {
            return Some(args);
        }
        let kind = if name == "save_macro" { ScriptKind::Macro } else { ScriptKind::Userscript };
        let source = args.get("source").and_then(Value::as_str).unwrap_or_default().to_string();
        if !more {
            return match self.drafts.finish(kind, draft.as_deref().unwrap_or_default(), &source) {
                Ok(whole) => {
                    args["source"] = Value::String(whole);
                    Some(args)
                }
                Err(e) => {
                    self.push_result(call_id, error_json(&e));
                    None
                }
            };
        }
        let what = if kind == ScriptKind::Macro { "macro" } else { "userscript" };
        let outcome = match self.drafts.append(kind, draft.as_deref(), &source) {
            Ok(p) => {
                let next = format!(
                    "send the next part with draft \"{}\" and more=true, the last one with draft \"{}\" and without more",
                    p.draft, p.draft
                );
                let v = json!({ "draft": p.draft, "part": p.parts, "bytes": p.bytes, "saved": false, "next": next });
                let summary = format!("Part {} kept ({} bytes so far)", p.parts, limits::thousands(p.bytes as u32));
                ToolOutcome::ok(v.to_string(), summary)
            }
            Err(e) => ToolOutcome::error(e),
        };
        self.steps += 1;
        self.task_steps += 1;
        let status = outcome.status.unwrap_or(StepStatus::Ok);
        let step = StepView {
            n: self.steps,
            tool: name.to_string(),
            label: format!("Keep a part of a long {what}"),
            status,
            summary: clip(&outcome.summary, 200),
            detail: format!("{}\n\nResult:\n{}", clip(&args.to_string(), 1000), clip(&outcome.text, 500)),
            image: None,
            ms: 0,
        };
        out.push(Action::Log(format!("agent step {} {name} {} 0 ms", step.n, status_name(status))));
        emit(out, Event::AgentStep { step });
        self.push_result(call_id, outcome.text);
        None
    }

    fn run_tool(&mut self, call_id: String, name: &'static str, args: Value, now: u64, out: &mut Vec<Action>) {
        if self.state != RunState::Running {
            self.set_state(RunState::Running, None, out);
        }
        self.steps += 1;
        self.task_steps += 1;
        let step = StepView {
            n: self.steps,
            tool: name.to_string(),
            label: describe(name, &args),
            status: StepStatus::Running,
            summary: String::new(),
            detail: clip(&args.to_string(), 1000),
            image: None,
            ms: 0,
        };
        emit(out, Event::AgentStep { step: step.clone() });
        out.push(Action::RunTool { call_id: call_id.clone(), name, args, step: self.steps });
        self.phase = Phase::Tool { call_id, step, started: now };
    }

    fn on_tool_done(&mut self, call_id: &str, outcome: ToolOutcome, now: u64, out: &mut Vec<Action>) {
        if !matches!(&self.phase, Phase::Tool { call_id: c, .. } if c == call_id) {
            return; // a cancelled or unknown tool: ignored
        }
        let Phase::Tool { mut step, started, .. } = std::mem::take(&mut self.phase) else { return };
        let status = outcome.status.unwrap_or(StepStatus::Ok);
        step.status = status;
        step.summary = clip(&outcome.summary, 200);
        step.detail = format!("{}\n\nResult:\n{}", step.detail, clip(&outcome.text, 500));
        step.image = outcome.image_url;
        step.ms = now.saturating_sub(started);
        out.push(Action::Log(format!("agent step {} {} {} {} ms", step.n, step.tool, status_name(status), step.ms)));
        emit(out, Event::AgentStep { step });
        if let Some(card) = outcome.script {
            emit(out, Event::AgentScript { card });
        }
        let text = if outcome.text.is_empty() { r#"{"ok":true}"#.to_string() } else { outcome.text };
        let image = if self.cfg.vision { outcome.image } else { None };
        self.history.push(Message::tool(call_id, text, image));
        self.process(now, out);
    }

    fn on_answer(&mut self, ask: u32, answer: AskAnswer, now: u64, out: &mut Vec<Action>) {
        if !matches!(self.phase, Phase::Ask { ask: a, .. } if a == ask) {
            return;
        }
        let Phase::Ask { call_id, spec, .. } = std::mem::take(&mut self.phase) else { return };
        let mut choices: Vec<String> = Vec::new();
        for &i in &answer.choices {
            if let Some(o) = spec.options.get(usize::from(i)) {
                if !choices.contains(o) {
                    choices.push(o.clone());
                }
            }
        }
        if !spec.multi {
            choices.truncate(1);
        }
        let other = answer.other.filter(|_| spec.allow_other).map(|o| clip(o.trim(), 500)).filter(|o| !o.is_empty());
        let mut result = json!({ "choices": choices });
        if let Some(o) = other {
            result["other"] = json!(o);
        }
        self.push_result(&call_id, result.to_string());
        self.set_state(RunState::Running, None, out);
        self.process(now, out);
    }

    fn on_consent(&mut self, ask: u32, allow: bool, now: u64, out: &mut Vec<Action>) {
        if !matches!(self.phase, Phase::Consent { ask: a, .. } if a == ask) {
            return;
        }
        let Phase::Consent { call_id, name, args, category, .. } = std::mem::take(&mut self.phase) else { return };
        if allow {
            self.allowed.insert(category);
            self.run_tool(call_id, name, args, now, out);
        } else {
            self.denied.insert(category);
            self.push_result(&call_id, error_json(DENIED));
            self.set_state(RunState::Running, None, out);
            self.process(now, out);
        }
    }

    fn on_continue(&mut self, ask: u32, go: bool, now: u64, out: &mut Vec<Action>) {
        if !matches!(self.phase, Phase::Continue { ask: a } if a == ask) {
            return;
        }
        self.phase = Phase::Idle;
        if go {
            self.step_limit = self.task_steps + self.cfg.max_steps.max(1);
            self.set_state(RunState::Running, None, out);
            self.send(out);
        } else {
            emit(
                out,
                Event::AgentMessage {
                    role: "notice".into(),
                    text: format!("Stopped after {} steps.", self.task_steps),
                },
            );
            self.done(None, now, out);
        }
    }

    /// The task ended with an answer (or after the user declined to continue).
    fn done(&mut self, answer: Option<String>, now: u64, out: &mut Vec<Action>) {
        self.phase = Phase::Idle;
        self.pending.clear();
        if let Some(a) = answer {
            emit(out, Event::AgentMessage { role: "assistant".into(), text: a });
        }
        out.push(Action::ReleaseInput);
        out.push(Action::Log(format!("agent done after {} steps", self.task_steps)));
        self.set_state(RunState::Done, self.elapsed(now), out);
    }

    /// The request failed; the conversation stays usable.
    fn fail(&mut self, message: String, now: u64, out: &mut Vec<Action>) {
        self.phase = Phase::Idle;
        self.pending.clear();
        repair(&mut self.history);
        emit(out, Event::AgentError { message });
        out.push(Action::ReleaseInput);
        out.push(Action::Log("agent request failed".into()));
        self.set_state(RunState::Error, self.elapsed(now), out);
    }

    fn stop(&mut self, now: u64, out: &mut Vec<Action>) {
        if !self.is_busy() {
            return;
        }
        match std::mem::take(&mut self.phase) {
            Phase::Requesting => out.push(Action::CancelRequest),
            Phase::Tool { call_id, mut step, started } => {
                out.push(Action::CancelTool);
                step.status = StepStatus::Cancelled;
                step.ms = now.saturating_sub(started);
                emit(out, Event::AgentStep { step });
                self.push_result(&call_id, CANCELLED);
            }
            Phase::Ask { call_id, .. } | Phase::Consent { call_id, .. } => self.push_result(&call_id, CANCELLED),
            Phase::Continue { .. } | Phase::Idle => {}
        }
        self.answer = Answer::default();
        self.carry.clear();
        while let Some(rest) = self.pending.pop_front() {
            self.push_result(&rest.id, CANCELLED);
        }
        repair(&mut self.history);
        out.push(Action::ReleaseInput);
        out.push(Action::Log("agent stopped".into()));
        self.set_state(RunState::Stopped, self.elapsed(now), out);
    }
}

/// Makes the history valid for the providers: every assistant tool call is followed by exactly
/// one result (missing ones become `{"cancelled":true}`, in call order); tool messages that
/// answer no call of the assistant message before them are dropped.
pub fn repair(msgs: &mut Vec<Message>) {
    let old = std::mem::take(msgs);
    let mut it = old.into_iter().peekable();
    while let Some(m) = it.next() {
        match m.role {
            Role::Assistant if !m.tool_calls.is_empty() => {
                let mut results: Vec<Message> = Vec::new();
                while it.peek().is_some_and(|n| n.role == Role::Tool) {
                    results.extend(it.next());
                }
                let calls = m.tool_calls.clone();
                msgs.push(m);
                for c in calls {
                    match results.iter().position(|r| r.tool_call_id.as_deref() == Some(c.id.as_str())) {
                        Some(i) => msgs.push(results.swap_remove(i)),
                        None => msgs.push(Message::tool(c.id, CANCELLED, None)),
                    }
                }
            }
            Role::Tool => {} // answers no call
            _ => msgs.push(m),
        }
    }
}

/// The history as it is sent: repaired, only the last [`KEEP_IMAGES`] images, tool results
/// older than the last [`KEEP_TOOL_RESULTS`] cut to [`SHORTENED_CHARS`].
pub fn request_messages(history: &[Message]) -> Vec<Message> {
    let mut msgs = history.to_vec();
    repair(&mut msgs);
    let mut images = 0;
    for m in msgs.iter_mut().rev() {
        for p in m.parts.iter_mut().rev() {
            if matches!(p, Part::Image(_)) {
                images += 1;
                if images > KEEP_IMAGES {
                    *p = Part::Text(IMAGE_REMOVED.into());
                }
            }
        }
    }
    let tool_msgs: Vec<usize> = msgs.iter().enumerate().filter(|(_, m)| m.role == Role::Tool).map(|(i, _)| i).collect();
    let old = tool_msgs.len().saturating_sub(KEEP_TOOL_RESULTS);
    for &i in &tool_msgs[..old] {
        for p in &mut msgs[i].parts {
            if let Part::Text(t) = p {
                if t.chars().count() > SHORTENED_CHARS {
                    let mut s: String = t.chars().take(SHORTENED_CHARS).collect();
                    s.push_str(SHORTENED_TAIL);
                    *t = s;
                }
            }
        }
    }
    msgs
}

/// A short label for the step log ("Click #go", "Read the page's links", "Type 12 characters").
/// Selectors and names are cut short; typed text is never shown, only its length.
pub fn describe(name: &str, args: &Value) -> String {
    let s = |k: &str| args.get(k).and_then(Value::as_str).map(|v| clip(v.trim(), 60)).filter(|v| !v.is_empty());
    let n = |k: &str| args.get(k).and_then(Value::as_f64);
    let host = |k: &str| {
        args.get(k)
            .and_then(Value::as_str)
            .and_then(|u| url::Url::parse(u.trim()).ok())
            .and_then(|u| u.host_str().map(str::to_string))
            .map(|h| clip(&h, 60))
    };
    match name {
        "get_page_context" => "Look at the page".into(),
        "get_html" => s("selector").map_or("Read the page's HTML".into(), |x| format!("Read the HTML of {x}")),
        "get_text" => s("selector").map_or("Read the page's text".into(), |x| format!("Read the text of {x}")),
        "query" => s("selector").map_or("Find elements".into(), |x| format!("Find {x}")),
        "list_links" => "Read the page's links".into(),
        "list_forms" => "Read the page's forms".into(),
        "list_media" => "Look for video and audio".into(),
        "screenshot" => "Take a screenshot".into(),
        "save_screenshot" => {
            if args.get("full_page").and_then(Value::as_bool) == Some(true) {
                "Save a screenshot of the whole page".into()
            } else {
                "Save a screenshot".into()
            }
        }
        "click" => match (s("selector"), n("x"), n("y")) {
            (Some(x), _, _) => format!("Click {x}"),
            (None, Some(x), Some(y)) => format!("Click at {}, {}", x.round(), y.round()),
            _ => "Click".into(),
        },
        "type_text" => {
            let len = args.get("text").and_then(Value::as_str).map_or(0, |t| t.chars().count());
            let what = if len == 1 { "1 character".to_string() } else { format!("{len} characters") };
            match s("selector") {
                Some(x) => format!("Type {what} into {x}"),
                None => format!("Type {what}"),
            }
        }
        "fill_fields" => {
            let k = args.get("fields").and_then(Value::as_object).map_or(0, |o| o.len());
            if k == 1 {
                "Fill in 1 field".into()
            } else {
                format!("Fill in {k} fields")
            }
        }
        "select_option" => s("selector").map_or("Choose an option".into(), |x| format!("Choose an option in {x}")),
        "press_keys" => {
            let keys = s("keys").unwrap_or_default();
            match args.get("action").and_then(Value::as_str) {
                Some("down") => format!("Hold {keys}"),
                Some("up") => format!("Release {keys}"),
                _ => format!("Press {keys}"),
            }
        }
        "scroll" => match (args.get("to").and_then(Value::as_str), s("selector")) {
            (Some("top"), _) => "Scroll to the top".into(),
            (Some("bottom"), _) => "Scroll to the bottom".into(),
            (_, Some(x)) => format!("Scroll to {x}"),
            _ => "Scroll".into(),
        },
        "wait_for" => match (s("selector"), args.get("navigation").and_then(Value::as_bool), n("ms")) {
            (Some(x), _, _) => format!("Wait for {x}"),
            (None, Some(true), _) => "Wait for the page to load".into(),
            (None, _, Some(ms)) => format!("Wait {} ms", ms.round()),
            _ => "Wait".into(),
        },
        "eval_js" => "Run JavaScript on the page".into(),
        "edit_page" => {
            let sel = s("selector").unwrap_or_else(|| "the page".into());
            match args.get("action").and_then(Value::as_str) {
                Some("set_text") => format!("Change the text of {sel}"),
                Some("set_attribute") | Some("remove_attribute") => format!("Change an attribute of {sel}"),
                Some("set_style") => format!("Change the style of {sel}"),
                Some("remove") => format!("Remove {sel}"),
                Some("insert_css") => "Add styles to the page".into(),
                _ => "Edit the page".into(),
            }
        }
        "set_title" => "Set the page title".into(),
        "navigate" => host("url").map_or("Open a page".into(), |h| format!("Open {h}")),
        "back" => "Go back".into(),
        "forward" => "Go forward".into(),
        "reload" => "Reload the page".into(),
        "open_tab" => match (host("url"), s("query")) {
            (Some(h), _) => format!("Open {h} in a new tab"),
            (None, Some(_)) => "Search the web in a new tab".into(),
            _ => "Open a new tab".into(),
        },
        "switch_tab" => n("tab_id").map_or("Switch tabs".into(), |t| format!("Switch to tab {t}")),
        "close_tab" => n("tab_id").map_or("Close the tab".into(), |t| format!("Close tab {t}")),
        "list_tabs" => "List the tabs".into(),
        "get_settings" => "Read the settings".into(),
        "set_setting" => s("key").map_or("Change a setting".into(), |k| format!("Change the setting {k}")),
        "set_new_tab_background_url" => {
            host("url").map_or("Set the new tab background".into(), |h| format!("Set the new tab background from {h}"))
        }
        "list_userscripts" => "List the userscripts".into(),
        "save_userscript" => {
            if s("id").is_some() {
                "Update a userscript".into()
            } else {
                "Create a userscript".into()
            }
        }
        "toggle_userscript" => match args.get("enabled").and_then(Value::as_bool) {
            Some(true) => "Turn on a userscript".into(),
            _ => "Turn off a userscript".into(),
        },
        "list_macros" => "List the macros".into(),
        "save_macro" => s("name").map_or("Save a macro".into(), |x| format!("Save the macro {x}")),
        "run_macro" => "Run a macro".into(),
        "ask_user" => "Ask you a question".into(),
        "get_history" => "Read your browsing history".into(),
        "get_bookmarks" => "Read your bookmarks".into(),
        "get_downloads" => "Read your downloads list".into(),
        "get_cookies_for_site" => {
            s("site").map_or("Read a site's cookies".into(), |x| format!("Read the cookies of {x}"))
        }
        "finish" => "Finish".into(),
        other => clip(other, 40),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{Image, ScriptCard, ScriptKind};

    fn cfg() -> Config {
        Config {
            model: "test/model".into(),
            provider_name: "Test".into(),
            vision: true,
            max_steps: 30,
            system_extra: String::new(),
            userscripts: true,
            macros: true,
            max_tokens: 32_000,
            known_max_output: Some(64_000),
        }
    }

    /// A scripted provider answer with tool calls (arguments arrive in two fragments).
    fn calls(list: &[(&str, &str, &str)]) -> Vec<Input> {
        let mut v = Vec::new();
        for (i, (id, name, args)) in list.iter().enumerate() {
            v.push(Input::Stream(StreamEvent::ToolCallStart { index: i, id: id.to_string(), name: name.to_string() }));
            let mid = args.char_indices().nth(args.chars().count() / 2).map_or(0, |(b, _)| b);
            v.push(Input::Stream(StreamEvent::ToolCallArgs { index: i, fragment: args[..mid].into() }));
            v.push(Input::Stream(StreamEvent::ToolCallArgs { index: i, fragment: args[mid..].into() }));
        }
        v.push(Input::Stream(StreamEvent::Usage { input: 10, output: 5 }));
        v.push(Input::Stream(StreamEvent::Done(StopReason::ToolUse)));
        v.push(Input::StreamEnd);
        v
    }

    fn text(t: &str) -> Vec<Input> {
        let mid = t.char_indices().nth(t.chars().count() / 2).map_or(0, |(b, _)| b);
        vec![
            Input::Stream(StreamEvent::TextDelta(t[..mid].into())),
            Input::Stream(StreamEvent::TextDelta(t[mid..].into())),
            Input::Stream(StreamEvent::Done(StopReason::EndTurn)),
            Input::StreamEnd,
        ]
    }

    fn user(t: &str) -> Input {
        Input::User { text: t.into(), context: "Context: tab 1 \"Shop\"".into() }
    }

    fn done(id: &str, text: &str) -> Input {
        Input::ToolDone { call_id: id.into(), outcome: ToolOutcome::ok(text, "ok") }
    }

    fn feed(s: &mut Session, inputs: Vec<Input>, now: u64) -> Vec<Action> {
        inputs.into_iter().flat_map(|i| s.handle(i, now)).collect()
    }

    fn sends(a: &[Action]) -> Vec<&ChatRequest> {
        a.iter()
            .filter_map(|x| match x {
                Action::Send(r) => Some(r),
                _ => None,
            })
            .collect()
    }

    fn runs(a: &[Action]) -> Vec<(String, &'static str, Value, u32)> {
        a.iter()
            .filter_map(|x| match x {
                Action::RunTool { call_id, name, args, step } => Some((call_id.clone(), *name, args.clone(), *step)),
                _ => None,
            })
            .collect()
    }

    fn events(a: &[Action]) -> Vec<&Event> {
        a.iter()
            .filter_map(|x| match x {
                Action::Emit(e) => Some(&**e),
                _ => None,
            })
            .collect()
    }

    fn states(a: &[Action]) -> Vec<RunState> {
        events(a)
            .into_iter()
            .filter_map(|e| match e {
                Event::AgentState { state, .. } => Some(*state),
                _ => None,
            })
            .collect()
    }

    fn logs(a: &[Action]) -> Vec<&str> {
        a.iter()
            .filter_map(|x| match x {
                Action::Log(l) => Some(l.as_str()),
                _ => None,
            })
            .collect()
    }

    fn messages_of(a: &[Action], role: &str) -> Vec<String> {
        events(a)
            .into_iter()
            .filter_map(|e| match e {
                Event::AgentMessage { role: r, text } if r == role => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn steps_of(a: &[Action]) -> Vec<StepView> {
        events(a)
            .into_iter()
            .filter_map(|e| match e {
                Event::AgentStep { step } => Some(step.clone()),
                _ => None,
            })
            .collect()
    }

    /// Every assistant tool call is followed by exactly its results, in order.
    fn assert_valid(msgs: &[Message]) {
        let mut i = 0;
        while i < msgs.len() {
            let m = &msgs[i];
            assert!(m.role != Role::Tool, "orphan tool message at {i}: {msgs:?}");
            i += 1;
            for c in &m.tool_calls {
                let r = msgs.get(i).unwrap_or_else(|| panic!("missing result for {}", c.id));
                assert_eq!(r.role, Role::Tool);
                assert_eq!(r.tool_call_id.as_deref(), Some(c.id.as_str()));
                i += 1;
            }
        }
    }

    fn tool_texts(msgs: &[Message]) -> Vec<String> {
        msgs.iter().filter(|m| m.role == Role::Tool).map(Message::text).collect()
    }

    #[test]
    fn tool_call_result_finish() {
        let mut s = Session::new(cfg());
        let a = s.handle(user("List the links"), 1000);
        assert_eq!(messages_of(&a, "user"), vec!["List the links"]);
        assert_eq!(states(&a), vec![RunState::Running]);
        let r = sends(&a);
        assert_eq!(r.len(), 1);
        let r = r[0];
        assert_eq!(r.model, "test/model");
        assert_eq!(r.max_tokens, 32_000);
        assert!(r.system.contains("untrusted data"));
        assert_eq!(r.messages.len(), 1);
        assert_eq!(r.messages[0].text(), "Context: tab 1 \"Shop\"\n\nUser message:\nList the links");
        let names: Vec<_> = r.tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"list_links") && names.contains(&"save_macro") && !names.contains(&"get_history"));
        assert!(s.is_busy());

        let a = feed(&mut s, calls(&[("c1", "list_links", "{}")]), 1100);
        assert_eq!(runs(&a), vec![("c1".into(), "list_links", json!({}), 1)]);
        let st = steps_of(&a);
        assert_eq!(st.len(), 1);
        assert_eq!((st[0].n, st[0].status, st[0].label.as_str()), (1, StepStatus::Running, "Read the page's links"));
        assert!(sends(&a).is_empty());

        let mut outcome = ToolOutcome::ok(r#"{"links":[{"text":"A","href":"https://a.example/"}]}"#, "1 link");
        outcome.image_url = Some("/agent-shot/t/1.jpg".into());
        outcome.script =
            Some(ScriptCard { id: "u1".into(), kind: ScriptKind::Userscript, name: "x".into(), enabled: true });
        let a = s.handle(Input::ToolDone { call_id: "c1".into(), outcome }, 1185);
        let st = steps_of(&a);
        assert_eq!((st[0].status, st[0].ms, st[0].summary.as_str()), (StepStatus::Ok, 85, "1 link"));
        assert_eq!(st[0].image.as_deref(), Some("/agent-shot/t/1.jpg"));
        assert!(st[0].detail.contains("Result:"));
        assert!(events(&a).iter().any(|e| matches!(e, Event::AgentScript { .. })));
        assert_eq!(logs(&a), vec!["agent step 1 list_links ok 85 ms"]);
        let r = sends(&a);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].messages.len(), 3);
        assert_eq!(r[0].messages[1].tool_calls[0].name, "list_links");
        assert_eq!(r[0].messages[2].tool_call_id.as_deref(), Some("c1"));
        assert_valid(&r[0].messages);

        let a = feed(&mut s, calls(&[("c2", "finish", r#"{"answer":"The page has **1** link."}"#)]), 1300);
        assert_eq!(messages_of(&a, "assistant"), vec!["The page has **1** link."]);
        assert!(runs(&a).is_empty() && sends(&a).is_empty());
        assert!(a.contains(&Action::ReleaseInput));
        assert_eq!(states(&a), vec![RunState::Done]);
        assert!(events(&a).contains(&&Event::AgentState { state: RunState::Done, steps: 1, ms: Some(300) }));
        assert_eq!(logs(&a), vec!["agent done after 1 steps"]);
        assert!(!s.is_busy());
        assert_eq!(s.steps(), 1);
        assert_valid(s.history());
    }

    #[test]
    fn text_answer_is_final_and_deltas_stream() {
        let mut s = Session::new(cfg());
        s.handle(Input::User { text: "Hi".into(), context: String::new() }, 0);
        let a = feed(&mut s, text("Hello there"), 10);
        let deltas: Vec<_> = events(&a)
            .into_iter()
            .filter_map(|e| match e {
                Event::AgentDelta { text } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(deltas.concat(), "Hello there");
        assert_eq!(messages_of(&a, "assistant"), vec!["Hello there"]);
        assert_eq!(s.state(), RunState::Done);
        assert_eq!(s.history()[0].text(), "User message:\nHi");
        // A follow-up in the same conversation sends the whole history.
        let a = s.handle(user("And now?"), 20);
        assert_eq!(sends(&a)[0].messages.len(), 3);
    }

    #[test]
    fn two_calls_in_one_answer_run_in_order() {
        let mut s = Session::new(cfg());
        s.handle(user("Fill and submit"), 0);
        let a = feed(
            &mut s,
            calls(&[("a", "fill_fields", r##"{"fields":{"#n":"Ann"}}"##), ("b", "click", r##"{"selector":"#go"}"##)]),
            0,
        );
        assert_eq!(runs(&a).len(), 1);
        assert_eq!(runs(&a)[0].1, "fill_fields");
        let a = s.handle(done("b", "early"), 5);
        assert!(a.is_empty(), "a result for a tool that doesn't run is ignored");
        let a = s.handle(done("a", "filled"), 10);
        assert!(sends(&a).is_empty());
        assert_eq!(runs(&a), vec![("b".into(), "click", json!({"selector":"#go"}), 2)]);
        assert_eq!(steps_of(&a).last().unwrap().label, "Click #go");
        let a = s.handle(done("b", "clicked"), 20);
        let r = sends(&a);
        assert_eq!(r.len(), 1);
        assert_eq!(tool_texts(&r[0].messages), vec!["filled", "clicked"]);
        assert_valid(&r[0].messages);
    }

    #[test]
    fn invalid_unknown_and_not_offered_calls() {
        let mut s = Session::new(Config { userscripts: false, ..cfg() });
        s.handle(user("Do things"), 0);
        let a = feed(
            &mut s,
            calls(&[
                ("1", "click", r#"{"count":3}"#),
                ("2", "nope", "{}"),
                ("3", "get_history", "{}"),
                ("4", "save_userscript", r#"{"source":"x"}"#),
                ("5", "click", "{not json"),
                ("6", "list_tabs", ""),
            ]),
            0,
        );
        let r = runs(&a);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].1, "list_tabs");
        assert_eq!(r[0].3, 1, "only real runs count as steps");
        let a = s.handle(done("6", "[]"), 1);
        let r = sends(&a);
        let texts = tool_texts(&r[0].messages);
        assert_eq!(texts.len(), 6);
        assert!(texts[0].contains("count must be at most 2"), "{}", texts[0]);
        assert_eq!(texts[1], r#"{"error":"there is no such tool"}"#);
        assert_eq!(texts[2], r#"{"error":"this tool is not available for this task"}"#);
        assert_eq!(texts[3], r#"{"error":"this tool is not available for this task"}"#);
        assert_eq!(texts[4], r#"{"error":"the arguments are not valid JSON"}"#);
        assert_eq!(texts[5], "[]");
        assert_valid(&r[0].messages);
        assert_eq!(s.steps(), 1);
    }

    #[test]
    fn ids_missing_or_repeated_are_made_unique() {
        let mut s = Session::new(cfg());
        s.handle(user("x"), 0);
        let a = feed(&mut s, calls(&[("", "list_tabs", "{}"), ("same", "reload", "{}"), ("same", "back", "{}")]), 0);
        let id1 = runs(&a)[0].0.clone();
        assert!(!id1.is_empty());
        let a = s.handle(done(&id1, "tabs"), 1);
        let id2 = runs(&a)[0].0.clone();
        let a = s.handle(done(&id2, "ok"), 2);
        let id3 = runs(&a)[0].0.clone();
        assert_ne!(id2, id3);
        let a = s.handle(done(&id3, "ok"), 3);
        assert_valid(&sends(&a)[0].messages);
    }

    #[test]
    fn ask_user_answer() {
        let mut s = Session::new(cfg());
        s.handle(user("ask me"), 0);
        let a = feed(
            &mut s,
            calls(&[("q", "ask_user", r#"{"question":"Which colour?","options":["red","green","blue"]}"#)]),
            0,
        );
        assert!(runs(&a).is_empty());
        let ask = events(&a)
            .into_iter()
            .find_map(|e| match e {
                Event::AgentAsk { ask } => Some(ask.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(ask.options, vec!["red", "green", "blue"]);
        assert!(!ask.multi && !ask.allow_other);
        assert_eq!(s.state(), RunState::Asking);
        assert!(s.is_busy());
        // A wrong id and a new user message are not answers.
        assert!(s
            .handle(Input::Answer { ask: ask.id + 1, answer: AskAnswer { choices: vec![0], other: None } }, 1)
            .is_empty());
        let a = s.handle(user("hello?"), 1);
        assert_eq!(messages_of(&a, "notice").len(), 1);
        assert!(sends(&a).is_empty());
        // Single choice: only the first index counts; "other" is not allowed here.
        let a = s.handle(
            Input::Answer { ask: ask.id, answer: AskAnswer { choices: vec![2, 0, 9], other: Some("pink".into()) } },
            2,
        );
        assert_eq!(states(&a), vec![RunState::Running]);
        let r = sends(&a);
        assert_eq!(tool_texts(&r[0].messages), vec![r#"{"choices":["blue"]}"#]);
        assert_eq!(s.steps(), 0, "ask_user is not a step");

        // Multi with other.
        let a = feed(
            &mut s,
            calls(&[(
                "q2",
                "ask_user",
                r#"{"question":"Which?","options":["a","b","c"],"multi":true,"allow_other":true}"#,
            )]),
            3,
        );
        let id = events(&a)
            .into_iter()
            .find_map(|e| match e {
                Event::AgentAsk { ask } => Some(ask.id),
                _ => None,
            })
            .unwrap();
        assert!(id > ask.id, "ids increase");
        let a = s.handle(
            Input::Answer { ask: id, answer: AskAnswer { choices: vec![2, 0, 2], other: Some("  d  ".into()) } },
            4,
        );
        assert_eq!(tool_texts(&sends(&a)[0].messages)[1], r#"{"choices":["c","a"],"other":"d"}"#);
    }

    #[test]
    fn stop_while_asking_cancels_the_question() {
        let mut s = Session::new(cfg());
        s.handle(user("ask me"), 0);
        let a = feed(
            &mut s,
            calls(&[
                ("q", "ask_user", r#"{"question":"Which?","options":["a","b"]}"#),
                ("later", "click", r##"{"selector":"#x"}"##),
            ]),
            0,
        );
        let id = events(&a)
            .into_iter()
            .find_map(|e| match e {
                Event::AgentAsk { ask } => Some(ask.id),
                _ => None,
            })
            .unwrap();
        let a = s.handle(Input::Stop, 50);
        assert!(!a.contains(&Action::CancelRequest) && !a.contains(&Action::CancelTool));
        assert!(a.contains(&Action::ReleaseInput));
        assert_eq!(states(&a), vec![RunState::Stopped]);
        assert_eq!(logs(&a), vec!["agent stopped"]);
        assert_valid(s.history());
        assert_eq!(tool_texts(s.history()), vec![CANCELLED, CANCELLED]);
        // The late answer is ignored; the conversation goes on.
        assert!(s
            .handle(Input::Answer { ask: id, answer: AskAnswer { choices: vec![0], other: None } }, 60)
            .is_empty());
        let a = s.handle(user("next"), 70);
        assert_valid(&sends(&a)[0].messages);
        assert_eq!(sends(&a)[0].messages.len(), 5);
    }

    #[test]
    fn consent_allow_is_remembered() {
        let mut s = Session::new(cfg());
        let a = s.handle(user("What is in my browsing history?"), 0);
        assert!(sends(&a)[0].tools.iter().any(|t| t.name == "get_history"));
        assert!(!sends(&a)[0].tools.iter().any(|t| t.name == "get_cookies_for_site"));
        let a = feed(&mut s, calls(&[("h", "get_history", r#"{"days":7}"#)]), 0);
        assert!(runs(&a).is_empty());
        let (ask, cat) = events(&a)
            .into_iter()
            .find_map(|e| match e {
                Event::AgentConsentAsk { ask, category, label } => {
                    assert_eq!(label, "your browsing history");
                    Some((*ask, *category))
                }
                _ => None,
            })
            .unwrap();
        assert_eq!(cat, Category::History);
        assert_eq!(s.state(), RunState::Asking);
        assert!(s.handle(Input::Consent { ask: ask + 5, allow: true }, 1).is_empty());
        let a = s.handle(Input::Consent { ask, allow: true }, 1);
        assert_eq!(states(&a), vec![RunState::Running]);
        assert_eq!(runs(&a), vec![("h".into(), "get_history", json!({"days":7}), 1)]);
        s.handle(done("h", "[]"), 2);
        // Asked again in the same conversation: runs without asking.
        let a = feed(&mut s, calls(&[("h2", "get_history", "{}")]), 3);
        assert_eq!(runs(&a).len(), 1);
        assert!(!events(&a).iter().any(|e| matches!(e, Event::AgentConsentAsk { .. })));
        s.handle(done("h2", "[]"), 4);
        feed(&mut s, text("Done."), 5);
        // A later task in the same conversation still needs the gate, but not the consent.
        let a = s.handle(user("list the links"), 6);
        assert!(!sends(&a)[0].tools.iter().any(|t| t.name == "get_history"));
        let a = feed(&mut s, calls(&[("h3", "get_history", "{}")]), 7);
        assert!(runs(&a).is_empty());
        let a = s.handle(Input::Stop, 8);
        assert_eq!(states(&a), vec![RunState::Stopped], "the not-offered call didn't wait");
        let a = s.handle(user("my history again"), 9);
        let a2 = feed(&mut s, calls(&[("h4", "get_history", "{}")]), 10);
        assert_eq!(sends(&a).len(), 1);
        assert_eq!(runs(&a2).len(), 1);
        // New forgets the consent.
        s.handle(Input::New, 11);
        s.handle(user("history please"), 12);
        let a = feed(&mut s, calls(&[("h5", "get_history", "{}")]), 13);
        assert!(events(&a).iter().any(|e| matches!(e, Event::AgentConsentAsk { .. })));
    }

    #[test]
    fn consent_deny_is_remembered() {
        let mut s = Session::new(cfg());
        s.handle(user("show my bookmarks and cookies for a.com"), 0);
        let a = feed(
            &mut s,
            calls(&[("b", "get_bookmarks", "{}"), ("c", "get_cookies_for_site", r#"{"site":"a.com"}"#)]),
            0,
        );
        let ask = events(&a)
            .into_iter()
            .find_map(|e| match e {
                Event::AgentConsentAsk { ask, .. } => Some(*ask),
                _ => None,
            })
            .unwrap();
        let a = s.handle(Input::Consent { ask, allow: false }, 1);
        assert!(runs(&a).is_empty());
        // The next call is another category: its own question.
        let ask2 = events(&a)
            .into_iter()
            .find_map(|e| match e {
                Event::AgentConsentAsk { ask, category, .. } => {
                    assert_eq!(*category, Category::Cookies);
                    Some(*ask)
                }
                _ => None,
            })
            .unwrap();
        let a = s.handle(Input::Consent { ask: ask2, allow: true }, 2);
        assert_eq!(runs(&a)[0].1, "get_cookies_for_site");
        let a = s.handle(done("c", "[]"), 3);
        let texts = tool_texts(&sends(&a)[0].messages);
        assert_eq!(texts, vec![r#"{"error":"the user denied access"}"#.to_string(), "[]".into()]);
        // Denied stays denied without asking again.
        let a = feed(&mut s, calls(&[("b2", "get_bookmarks", "{}")]), 4);
        assert!(!events(&a).iter().any(|e| matches!(e, Event::AgentConsentAsk { .. })));
        assert_eq!(tool_texts(&sends(&a)[0].messages).last().unwrap(), r#"{"error":"the user denied access"}"#);
        assert_eq!(s.steps(), 1);
    }

    #[test]
    fn max_steps_continue_yes_then_no() {
        let mut s = Session::new(Config { max_steps: 2, ..cfg() });
        s.handle(user("scroll a lot"), 0);
        let mut ask = 0;
        for round in 0..2 {
            for k in 0..2 {
                let id = format!("s{round}{k}");
                let a = feed(&mut s, calls(&[(&id, "scroll", r#"{"dy":100}"#)]), 0);
                assert_eq!(runs(&a).len(), 1);
                let a = s.handle(done(&id, "{}"), 1);
                if k == 0 {
                    assert_eq!(sends(&a).len(), 1);
                } else {
                    assert!(sends(&a).is_empty(), "no request at the limit");
                    assert_eq!(states(&a), vec![RunState::MaxSteps]);
                    let (id, steps) = events(&a)
                        .into_iter()
                        .find_map(|e| match e {
                            Event::AgentContinueAsk { ask, steps } => Some((*ask, *steps)),
                            _ => None,
                        })
                        .unwrap();
                    assert_eq!(steps, 2 * (round + 1));
                    ask = id;
                }
            }
            assert!(s.is_busy());
            if round == 0 {
                assert!(s.handle(Input::Continue { ask: ask + 1, go: true }, 2).is_empty());
                let a = s.handle(Input::Continue { ask, go: true }, 2);
                assert_eq!(states(&a), vec![RunState::Running]);
                assert_eq!(sends(&a).len(), 1);
            }
        }
        let a = s.handle(Input::Continue { ask, go: false }, 3);
        assert_eq!(messages_of(&a, "notice"), vec!["Stopped after 4 steps."]);
        assert_eq!(states(&a), vec![RunState::Done]);
        assert!(sends(&a).is_empty() && a.contains(&Action::ReleaseInput));
        assert_valid(s.history());
        // The next task starts a new count.
        s.handle(user("once more"), 4);
        let a = feed(&mut s, calls(&[("t", "scroll", "{}")]), 5);
        assert_eq!(runs(&a)[0].3, 5);
        let a = s.handle(done("t", "{}"), 6);
        assert_eq!(sends(&a).len(), 1);
    }

    #[test]
    fn stop_mid_stream() {
        let mut s = Session::new(cfg());
        s.handle(user("slow"), 100);
        s.handle(Input::Stream(StreamEvent::TextDelta("one ".into())), 110);
        s.handle(Input::Stream(StreamEvent::ToolCallStart { index: 0, id: "c".into(), name: "click".into() }), 120);
        let a = s.handle(Input::Stop, 400);
        assert_eq!(a[0], Action::CancelRequest);
        assert!(a.contains(&Action::ReleaseInput));
        assert!(events(&a).contains(&&Event::AgentState { state: RunState::Stopped, steps: 0, ms: Some(300) }));
        assert!(!s.is_busy());
        // Late stream events are ignored.
        for i in [
            Input::Stream(StreamEvent::TextDelta("two".into())),
            Input::Stream(StreamEvent::ToolCallArgs { index: 0, fragment: "{}".into() }),
            Input::StreamEnd,
            Input::StreamFailed("closed".into()),
        ] {
            assert!(s.handle(i, 500).is_empty());
        }
        assert!(s.handle(Input::Stop, 600).is_empty(), "stop when idle does nothing");
        assert_eq!(s.history().len(), 1);
        let a = s.handle(user("again"), 700);
        assert_eq!(sends(&a)[0].messages.len(), 2);
        assert!(feed(&mut s, text("fine"), 800).iter().any(|x| matches!(x, Action::Emit(_))));
        assert_eq!(s.state(), RunState::Done);
    }

    #[test]
    fn stop_mid_tool() {
        let mut s = Session::new(cfg());
        s.handle(user("two things"), 0);
        let a =
            feed(&mut s, calls(&[("c1", "wait_for", r#"{"ms":5000}"#), ("c2", "click", r##"{"selector":"#b"}"##)]), 10);
        assert_eq!(runs(&a).len(), 1);
        let a = s.handle(Input::Stop, 500);
        assert_eq!(a[0], Action::CancelTool);
        assert!(!a.contains(&Action::CancelRequest));
        let st = steps_of(&a);
        assert_eq!((st[0].status, st[0].ms, st[0].n), (StepStatus::Cancelled, 490, 1));
        assert_eq!(states(&a), vec![RunState::Stopped]);
        assert_valid(s.history());
        assert_eq!(tool_texts(s.history()), vec![CANCELLED, CANCELLED]);
        // The late result of the cancelled tool is ignored.
        assert!(s.handle(done("c1", "waited"), 600).is_empty());
        assert_eq!(tool_texts(s.history()), vec![CANCELLED, CANCELLED]);
        let a = s.handle(user("go on"), 700);
        assert_valid(&sends(&a)[0].messages);
    }

    #[test]
    fn stream_failure_keeps_the_conversation() {
        let mut s = Session::new(cfg());
        s.handle(user("hi"), 0);
        let a = s.handle(Input::StreamFailed("The provider answered with HTTP 401: bad key".into()), 50);
        assert!(
            events(&a).contains(&&Event::AgentError { message: "The provider answered with HTTP 401: bad key".into() })
        );
        assert_eq!(states(&a), vec![RunState::Error]);
        assert!(a.contains(&Action::ReleaseInput));
        assert!(!s.is_busy());
        assert!(!logs(&a).iter().any(|l| l.contains("401") || l.contains("key")));
        let a = s.handle(user("hi again"), 60);
        assert_eq!(sends(&a).len(), 1);
        // An error inside the stream fails the same way, also after tool calls.
        let a = feed(&mut s, calls(&[("c", "list_tabs", "{}")]), 70);
        assert_eq!(runs(&a).len(), 1);
        s.handle(done("c", "[]"), 80);
        s.handle(Input::Stream(StreamEvent::TextDelta("par".into())), 90);
        s.handle(Input::Stream(StreamEvent::Error("overloaded".into())), 91);
        let a = s.handle(Input::StreamEnd, 92);
        assert!(events(&a).contains(&&Event::AgentError { message: "overloaded".into() }));
        assert_eq!(s.state(), RunState::Error);
        assert_valid(s.history());
        let a = s.handle(user("retry"), 100);
        assert_valid(&sends(&a)[0].messages);
    }

    fn limit_stop() -> Vec<Input> {
        vec![Input::Stream(StreamEvent::Done(StopReason::MaxTokens)), Input::StreamEnd]
    }

    fn errors(a: &[Action]) -> Vec<String> {
        events(a)
            .into_iter()
            .filter_map(|e| match e {
                Event::AgentError { message } => Some(message.clone()),
                _ => None,
            })
            .collect()
    }

    /// Tool calls whose last arguments were cut off by the output limit.
    fn cut_calls(list: &[(&str, &str, &str)]) -> Vec<Input> {
        let mut v = calls(list);
        let at = v.len() - 2;
        v[at] = Input::Stream(StreamEvent::Done(StopReason::MaxTokens));
        v
    }

    #[test]
    fn empty_answer_at_the_limit_retries_once_then_names_the_limit() {
        let mut s = Session::new(cfg());
        s.handle(user("x"), 0);
        // First stop: the same request again with twice the cap (at most the known 64,000).
        let a = feed(&mut s, limit_stop(), 1);
        assert!(errors(&a).is_empty());
        let r = sends(&a);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].max_tokens, 64_000);
        assert_eq!(r[0].messages.len(), 1, "nothing of the empty answer is kept");
        let notes = messages_of(&a, "notice");
        assert!(notes[0].contains("32,000 tokens") && notes[0].contains("64,000"), "{notes:?}");
        assert_eq!(s.state(), RunState::Running);
        // Second stop: the error names the cap used and where to raise it.
        let a = feed(&mut s, limit_stop(), 2);
        assert!(sends(&a).is_empty());
        assert_eq!(
            errors(&a),
            vec!["The model stopped at its output limit (64,000 tokens) before answering. Raise Max output tokens for Test in Settings → AI."]
        );
        assert_eq!(s.state(), RunState::Error);
        // The next task starts at the configured cap again.
        let a = s.handle(user("y"), 3);
        assert_eq!(sends(&a)[0].max_tokens, 32_000);
        let a = feed(&mut s, vec![Input::Stream(StreamEvent::TextDelta("  ".into())), Input::StreamEnd], 4);
        assert!(errors(&a)[0].contains("empty"));
    }

    #[test]
    fn no_retry_when_the_cap_is_the_models_maximum() {
        let mut s = Session::new(Config { max_tokens: 16_384, known_max_output: Some(16_384), ..cfg() });
        s.handle(user("x"), 0);
        let a = feed(&mut s, limit_stop(), 1);
        assert!(sends(&a).is_empty());
        assert!(errors(&a)[0].starts_with("The model stopped at its output limit (16,384 tokens)"));
        // Auto (no cap sent): the retry sends the known maximum.
        let mut s = Session::new(Config { max_tokens: 0, known_max_output: None, ..cfg() });
        assert_eq!(sends(&s.handle(user("x"), 0))[0].max_tokens, 0);
        let a = feed(&mut s, limit_stop(), 1);
        assert_eq!(sends(&a)[0].max_tokens, 32_000);
        assert!(messages_of(&a, "notice")[0].contains("the provider's default output limit"));
    }

    #[test]
    fn a_text_answer_cut_off_is_continued_once() {
        let mut s = Session::new(cfg());
        s.handle(user("z"), 0);
        let a = feed(
            &mut s,
            vec![
                Input::Stream(StreamEvent::TextDelta("first half, ".into())),
                Input::Stream(StreamEvent::Done(StopReason::MaxTokens)),
                Input::StreamEnd,
            ],
            1,
        );
        assert!(messages_of(&a, "assistant").is_empty());
        let r = sends(&a);
        assert_eq!(r.len(), 1);
        let last = r[0].messages.last().unwrap();
        assert_eq!((last.role, last.text()), (Role::User, CONTINUE_NOTE.to_string()));
        assert_eq!(r[0].messages[r[0].messages.len() - 2].text(), "first half, ");
        // Cut again: the answer is what came, with a notice naming the limit.
        let a = feed(
            &mut s,
            vec![
                Input::Stream(StreamEvent::TextDelta("second half".into())),
                Input::Stream(StreamEvent::Done(StopReason::MaxTokens)),
                Input::StreamEnd,
            ],
            2,
        );
        assert!(sends(&a).is_empty());
        assert_eq!(messages_of(&a, "assistant"), vec!["first half, second half"]);
        assert!(messages_of(&a, "notice")[0].contains("32,000 tokens"));
        assert_eq!(s.state(), RunState::Done);
        assert_valid(s.history());
    }

    #[test]
    fn a_cut_off_tool_call_never_runs() {
        let mut s = Session::new(cfg());
        s.handle(user("Make a userscript"), 0);
        let cut = r#"{"source":"// ==UserScript==
// @name X"#;
        // 1: retried with a higher cap, nothing runs, nothing of it is kept.
        let a = feed(&mut s, cut_calls(&[("c1", "save_userscript", cut)]), 1);
        assert!(runs(&a).is_empty());
        let r = sends(&a);
        assert_eq!((r.len(), r[0].max_tokens, r[0].messages.len()), (1, 64_000, 1));
        assert!(messages_of(&a, "notice")[0].contains("nothing ran"));
        // 2: still cut off (now at the model's maximum): handed back with a result saying so.
        let a = feed(&mut s, cut_calls(&[("c0", "list_tabs", "{}"), ("c2", "save_userscript", cut)]), 2);
        assert!(runs(&a).is_empty(), "the complete call of a cut-off answer doesn't run either");
        let r = sends(&a);
        assert_eq!(r.len(), 1);
        assert_valid(&r[0].messages);
        let n = r[0].messages.len();
        let assistant = &r[0].messages[n - 3];
        assert_eq!(assistant.tool_calls[1].arguments, "{}", "incomplete arguments are not sent back");
        let results = tool_texts(&r[0].messages);
        assert!(results[0].contains("not run"), "{results:?}");
        assert!(results[1].contains("cut off at the output limit (64,000 tokens)") && results[1].contains("more=true"));
        assert!(messages_of(&a, "notice")[0].contains("smaller parts"));
        // 3: once more, then an error naming the limit.
        let a = feed(&mut s, cut_calls(&[("c3", "save_userscript", cut)]), 3);
        assert!(runs(&a).is_empty() && sends(&a).len() == 1);
        let a = feed(&mut s, cut_calls(&[("c4", "save_userscript", cut)]), 4);
        assert!(runs(&a).is_empty() && sends(&a).is_empty());
        assert_eq!(
            errors(&a),
            vec!["The model's answer was cut off at its output limit (64,000 tokens) in the middle of a tool call, so nothing ran. Raise Max output tokens for Test in Settings → AI."]
        );
        assert_valid(s.history());
        // Complete calls that end at the limit still run.
        s.handle(user("again"), 5);
        let a = feed(&mut s, cut_calls(&[("c5", "list_tabs", "{}")]), 6);
        assert_eq!(runs(&a).len(), 1);
    }

    #[test]
    fn long_scripts_are_saved_in_parts() {
        let mut s = Session::new(cfg());
        s.handle(user("Write a long userscript"), 0);
        let head = "// ==UserScript==
// @name Long
// @match *://*/*
// ==/UserScript==
";
        let part1 = json!({ "source": head, "more": true }).to_string();
        // Part 1: kept, not saved: the host sees nothing.
        let a = feed(&mut s, calls(&[("p1", "save_userscript", &part1)]), 1);
        assert!(runs(&a).is_empty());
        let st = steps_of(&a);
        assert_eq!(
            (st[0].n, st[0].status, st[0].label.as_str()),
            (1, StepStatus::Ok, "Keep a part of a long userscript")
        );
        let r = sends(&a);
        let res: Value = serde_json::from_str(&tool_texts(&r[0].messages)[0]).unwrap();
        assert_eq!(
            (res["draft"].as_str(), res["part"].as_u64(), res["saved"].as_bool()),
            (Some("draft-1"), Some(1), Some(false))
        );
        assert_eq!(res["bytes"].as_u64(), Some(head.len() as u64));
        // Part 2 (a wrong draft id first: an error, nothing lost).
        let bad = json!({ "source": "x", "draft": "draft-7", "more": true }).to_string();
        let a = feed(&mut s, calls(&[("p2x", "save_userscript", &bad)]), 2);
        assert!(runs(&a).is_empty());
        assert!(tool_texts(&sends(&a)[0].messages).last().unwrap().contains("no such draft"));
        let part2 = json!({ "source": "const a = 1;
", "draft": "draft-1", "more": true })
        .to_string();
        let a = feed(&mut s, calls(&[("p2", "save_userscript", &part2)]), 3);
        assert!(runs(&a).is_empty());
        assert_eq!(steps_of(&a)[0].summary, format!("Part 2 kept ({} bytes so far)", head.len() + 13));
        // A macro can't finish a userscript draft.
        let wrong = json!({ "name": "m", "source": "x", "draft": "draft-1" }).to_string();
        let a = feed(&mut s, calls(&[("p3x", "save_macro", &wrong)]), 4);
        assert!(runs(&a).is_empty());
        // The last part: the host gets the whole source (and checks it only now).
        let part3 = json!({ "source": "console.log(a);", "draft": "draft-1" }).to_string();
        let a = feed(&mut s, calls(&[("p3", "save_userscript", &part3)]), 5);
        let r = runs(&a);
        assert_eq!(r.len(), 1);
        assert_eq!(
            r[0].2,
            json!({ "source": format!("{head}const a = 1;
console.log(a);") })
        );
        // The draft is gone once finished.
        s.handle(done("p3", r#"{"id":"u1"}"#), 6);
        let again = json!({ "source": "y", "draft": "draft-1" }).to_string();
        let a = feed(&mut s, calls(&[("p4", "save_userscript", &again)]), 7);
        assert!(runs(&a).is_empty());
        // A plain save is unchanged.
        let a = feed(&mut s, calls(&[("p5", "save_userscript", r#"{"source":"z","more":false}"#)]), 8);
        assert_eq!(runs(&a)[0].2, json!({ "source": "z" }));
    }

    #[test]
    fn new_conversation() {
        let mut s = Session::new(cfg());
        s.handle(user("first"), 0);
        feed(&mut s, calls(&[("c", "reload", "{}")]), 1);
        let a = s.handle(Input::New, 2);
        assert_eq!(a[0], Action::CancelTool);
        assert!(events(&a).contains(&&Event::AgentClear));
        assert_eq!(states(&a), vec![RunState::Stopped, RunState::Idle]);
        assert_eq!(s.state(), RunState::Idle);
        assert!(s.history().is_empty());
        assert_eq!(s.steps(), 0);
        assert!(s.handle(done("c", "late"), 3).is_empty());
        let a = s.handle(user("second"), 4);
        assert_eq!(sends(&a)[0].messages.len(), 1);
        // New while idle just clears.
        feed(&mut s, text("ok"), 5);
        let a = s.handle(Input::New, 6);
        assert_eq!(states(&a), vec![RunState::Idle]);
        assert!(!a.contains(&Action::ReleaseInput));
    }

    fn shot(n: u8) -> ToolOutcome {
        ToolOutcome {
            text: format!("screenshot {n}"),
            image: Some(Image { mime: "image/jpeg".into(), base64: format!("IMG{n}") }),
            image_url: Some(format!("/agent-shot/t/{n}.jpg")),
            summary: "Screenshot".into(),
            status: Some(StepStatus::Ok),
            script: None,
        }
    }

    fn images(msgs: &[Message]) -> Vec<String> {
        msgs.iter()
            .flat_map(|m| m.parts.iter())
            .filter_map(|p| match p {
                Part::Image(i) => Some(i.base64.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn only_the_last_two_images_are_sent() {
        let mut s = Session::new(cfg());
        s.handle(user("look"), 0);
        let mut last = Vec::new();
        for n in 1..=4u8 {
            let id = format!("s{n}");
            feed(&mut s, calls(&[(&id, "screenshot", "{}")]), 0);
            last = s.handle(Input::ToolDone { call_id: id, outcome: shot(n) }, 1);
        }
        let r = sends(&last)[0];
        assert_eq!(images(&r.messages), vec!["IMG3", "IMG4"]);
        let removed = r.messages.iter().filter(|m| m.text().contains(IMAGE_REMOVED)).count();
        assert_eq!(removed, 2);
        assert_eq!(images(s.history()).len(), 4, "the stored history keeps them; only the request drops them");
        // Without vision no image is kept at all.
        let mut s = Session::new(Config { vision: false, ..cfg() });
        s.handle(user("look"), 0);
        feed(&mut s, calls(&[("s", "screenshot", "{}")]), 0);
        let a = s.handle(Input::ToolDone { call_id: "s".into(), outcome: shot(1) }, 1);
        assert!(images(&sends(&a)[0].messages).is_empty());
        assert!(sends(&a)[0].system.contains("does not take images"));
    }

    #[test]
    fn old_tool_results_are_shortened() {
        let mut s = Session::new(cfg());
        s.handle(user("read a lot"), 0);
        let mut last = Vec::new();
        for n in 0..10 {
            let id = format!("r{n}");
            feed(&mut s, calls(&[(&id, "get_text", "{}")]), 0);
            last = s.handle(done(&id, &format!("{n}{}", "x".repeat(1999))), 1);
        }
        let r = sends(&last)[0];
        let texts = tool_texts(&r.messages);
        assert_eq!(texts.len(), 10);
        for t in &texts[..2] {
            assert_eq!(t.chars().count(), SHORTENED_CHARS + SHORTENED_TAIL.chars().count());
            assert!(t.ends_with("…[shortened]"));
        }
        for t in &texts[2..] {
            assert_eq!(t.chars().count(), 2000);
        }
        // The user message and the assistant messages are kept whole.
        assert_eq!(r.messages.len(), 21);
    }

    #[test]
    fn finish_ends_the_answer_and_cancels_the_rest() {
        let mut s = Session::new(cfg());
        s.handle(user("x"), 0);
        let a = feed(
            &mut s,
            calls(&[("f", "finish", r#"{"answer":"All done."}"#), ("c", "click", r##"{"selector":"#a"}"##)]),
            0,
        );
        assert!(runs(&a).is_empty());
        assert_eq!(messages_of(&a, "assistant"), vec!["All done."]);
        assert_valid(s.history());
        assert_eq!(tool_texts(s.history()), vec![r#"{"ok":true}"#.to_string(), CANCELLED.into()]);
    }

    #[test]
    fn logs_never_carry_content() {
        let mut s = Session::new(cfg());
        let mut all = s.handle(user("type hunter2 into https://secret.example/?token=abc"), 0);
        all.extend(feed(
            &mut s,
            calls(&[
                ("t", "type_text", r##"{"text":"hunter2","selector":"#pw"}"##),
                ("n", "navigate", r#"{"url":"https://secret.example/?token=abc"}"#),
            ]),
            0,
        ));
        all.extend(s.handle(done("t", "typed hunter2"), 1));
        all.extend(
            s.handle(Input::ToolDone { call_id: "n".into(), outcome: ToolOutcome::refused("nope token=abc") }, 2),
        );
        all.extend(s.handle(Input::StreamFailed("bad key sk-123".into()), 3));
        let l = logs(&all);
        assert_eq!(
            l,
            vec!["agent step 1 type_text ok 1 ms", "agent step 2 navigate refused 1 ms", "agent request failed"]
        );
        // Step labels show no typed text and no full address.
        let st = steps_of(&all);
        assert_eq!(st[0].label, "Type 7 characters into #pw");
        assert_eq!(st[2].label, "Open secret.example");
    }

    #[test]
    fn config_applies_to_the_next_request() {
        let mut s = Session::new(cfg());
        s.handle(user("x"), 0);
        s.set_config(Config { model: "other".into(), system_extra: "Be brief.".into(), macros: false, ..cfg() });
        let a = feed(&mut s, calls(&[("c", "list_tabs", "{}")]), 0);
        let a2 = s.handle(done("c", "[]"), 1);
        assert!(sends(&a).is_empty());
        let r = sends(&a2)[0];
        assert_eq!(r.model, "other");
        assert!(r.system.contains("Be brief."));
        // The offer is fixed per task (macros were on when it started).
        assert!(r.tools.iter().any(|t| t.name == "save_macro"));
        assert_eq!(s.config().model, "other");
        feed(&mut s, text("ok"), 2);
        let a = s.handle(user("y"), 3);
        assert!(!sends(&a)[0].tools.iter().any(|t| t.name == "save_macro"));
    }

    #[test]
    fn repair_fills_and_drops() {
        let call = |id: &str| ToolCall { id: id.into(), name: "reload".into(), arguments: "{}".into() };
        let mut h = vec![
            Message::tool("orphan", "x", None),
            Message::user("u"),
            Message::assistant("", vec![call("a"), call("b")]),
            Message::tool("b", "B", None),
            Message::tool("zzz", "Z", None),
            Message::user("u2"),
        ];
        repair(&mut h);
        assert_valid(&h);
        assert_eq!(h.len(), 5);
        assert_eq!(tool_texts(&h), vec![CANCELLED.to_string(), "B".into()]);
    }

    #[test]
    fn labels() {
        let d = |n: &str, a: Value| describe(n, &a);
        assert_eq!(d("click", json!({"selector":"#go"})), "Click #go");
        assert_eq!(d("click", json!({"x":10.4,"y":20})), "Click at 10, 20");
        assert_eq!(d("list_links", json!({})), "Read the page's links");
        assert_eq!(d("type_text", json!({"text":"hello world!"})), "Type 12 characters");
        assert_eq!(d("type_text", json!({"text":"a"})), "Type 1 character");
        assert_eq!(d("fill_fields", json!({"fields":{"#a":"1","#b":"2"}})), "Fill in 2 fields");
        assert_eq!(d("press_keys", json!({"keys":"a","action":"down"})), "Hold a");
        assert_eq!(d("press_keys", json!({"keys":"Control+A"})), "Press Control+A");
        assert_eq!(d("scroll", json!({"to":"bottom"})), "Scroll to the bottom");
        assert_eq!(d("wait_for", json!({"navigation":true})), "Wait for the page to load");
        assert_eq!(d("navigate", json!({"url":"krunk://settings"})), "Open settings");
        assert_eq!(d("navigate", json!({"url":"about:blank"})), "Open a page");
        assert_eq!(d("open_tab", json!({"query":"weather"})), "Search the web in a new tab");
        assert_eq!(d("edit_page", json!({"action":"remove","selector":".ad"})), "Remove .ad");
        assert_eq!(d("set_setting", json!({"key":"theme","value":"dark"})), "Change the setting theme");
        assert_eq!(d("switch_tab", json!({"tab_id":3})), "Switch to tab 3");
        let long = describe("click", &json!({"selector": "x".repeat(100)}));
        assert!(long.chars().count() <= 67, "{long}");
        for t in super::super::tools::TOOLS {
            assert!(!describe(t.name, &json!({})).is_empty(), "{}", t.name);
        }
    }
}
