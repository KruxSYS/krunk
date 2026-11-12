//! The AI agent of each window (0.12.0, WP-B2): the conversation (`lb_core::agent::session`),
//! the provider call (`provider.rs`, WP-A) and the tools (`tools.rs`).
//!
//! The host side of the loop: `Session::handle` returns actions, this module carries them out
//! (send a provider request, run a tool, post to the panel) and feeds the results back. While
//! AI is off nothing here runs: there is no panel to send `agent*` requests.
//!
//! Inputs go through one queue: a result that arrives while actions are being carried out (a
//! tool that answers at once, a provider error before the request left) waits until those
//! actions are done, so the session always sees its inputs in order and no borrow of the host
//! state is held while the session's actions call back into the browser.
//!
//! Late answers are dropped by generation: Stop (or a cancelled tool) bumps the window's
//! request or tool generation, and stream events or tool results of an older one never reach
//! the session.
//!
//! In memory only: the conversation, the step screenshots (at most 40 per window, served to
//! that window's panel under a random 128-bit token), the consent decisions and (0.16.0) the
//! scratchpad, which lives in the session and never passes through this module except as the
//! panel's `agentScratch` event; all go with the window. The per-tab media log of the plan is
//! not kept here: the engine reports no network events to the browser without the Network
//! domain, so `list_media` relies on the page's own resource timing (read by the probe).

mod page;
pub(super) mod provider;
mod tools;

use super::Browser;
use crate::ffi::cef::fetch::JobId;
use crate::log::{info, warning};
use lb_core::agent::context::{self, ContextInput, TabFacts};
use lb_core::agent::provider::limits;
use lb_core::agent::session::{Action, Config, Input, Session};
use lb_core::agent::tools::{self as agent_tools, Offer};
use lb_core::agent::{gate, pages as agent_pages, redact, AskAnswer, ModelChoice, ModelInfo, RunState, ScriptKind};
use lb_core::pages::Page;
use lb_core::protocol::{Event, Request};
use lb_core::tabs::TabId;
use lb_core::windows::WindowId;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::time::Instant;

/// Step screenshots kept per window.
const MAX_SHOTS: usize = 40;
/// Models in the panel's quick switch.
const QUICK_MODELS: usize = 50;
/// Where the panel loads step screenshots: `/agent-shot/<token>/<n>.jpg`.
const SHOT_PREFIX: &str = "/agent-shot/";
/// How long the automatic context waits for the page before it goes without it.
const CONTEXT_WAIT_MS: i64 = 3000;

/// One window's agent.
struct AgentWin {
    session: Session,
    /// The panel's key to its screenshots (random, 128 bits, hex).
    token: String,
    /// Step screenshots (JPEG) by step number, oldest first.
    shots: VecDeque<(u32, Vec<u8>)>,
    /// The provider request: its generation, whether it is still running, its job.
    req_gen: u64,
    req_open: bool,
    job: Option<JobId>,
    /// The running tool: its generation, call id and step number.
    tool_gen: u64,
    tool: Option<(String, u32)>,
    /// A task being prepared (the context is read from the page): Stop or New drop it.
    send_gen: u64,
    /// When Stop was asked (for the "stopped in N ms" log line).
    stop_at: Option<Instant>,
}

impl AgentWin {
    fn new(cfg: Config) -> AgentWin {
        AgentWin {
            session: Session::new(cfg),
            token: lb_core::scripts_store::new_id(),
            shots: VecDeque::new(),
            req_gen: 0,
            req_open: false,
            job: None,
            tool_gen: 0,
            tool: None,
            send_gen: 0,
            stop_at: None,
        }
    }
}

/// Every window's agent.
#[derive(Default)]
pub(super) struct AgentHost {
    wins: RefCell<HashMap<WindowId, AgentWin>>,
    /// Inputs waiting for the session (see the module comment) and whether they are being
    /// handled now.
    queue: RefCell<VecDeque<(WindowId, Input)>>,
    pumping: Cell<bool>,
    /// The providers' model lists (vision flags and the quick switch), and the providers
    /// already asked since the settings last changed.
    models: RefCell<HashMap<String, Vec<ModelInfo>>>,
    models_asked: RefCell<HashSet<String>>,
    /// Windows whose panel has loaded once (a second load is a reload after a crash).
    loaded: RefCell<HashSet<WindowId>>,
    /// The probe's isolated world per tab (its execution context id; tools.rs).
    probes: RefCell<HashMap<TabId, i64>>,
    /// Tabs whose Page and Runtime domains the agent holds.
    cdp_tabs: RefCell<HashSet<TabId>>,
}

/// What the agent of every window runs with now.
struct Setup {
    cfg: Config,
    provider: String,
    provider_name: String,
    models: Vec<ModelChoice>,
    /// Why the agent can't run, for the panel.
    problem: Option<String>,
}

/// A monotonic clock for the session's step durations.
fn mono_ms() -> u64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// A callback run once: the first of several callers (an answer or a timer) wins.
type Once<T> = Rc<RefCell<Option<Box<dyn FnOnce(&Browser, T)>>>>;
type PageFacts = (Option<serde_json::Value>, Option<String>);

impl Browser {
    // ------------------------------------------------------------ configuration

    fn agent_setup(&self) -> Setup {
        let s = self.settings.borrow();
        let ai = &s.ai;
        let provider = ai.provider(&ai.active_provider);
        let provider_name = provider
            .map(|p| if p.name.trim().is_empty() { p.kind.label().to_string() } else { p.name.clone() })
            .unwrap_or_default();
        let model = ai.active_model.trim().to_string();
        let cached = provider.and_then(|p| self.agent.models.borrow().get(&p.id).cloned());
        let info = cached.as_ref().and_then(|list| list.iter().find(|m| m.id == model));
        let vision = provider.and_then(|p| p.vision).or_else(|| info.and_then(|m| m.vision)).unwrap_or(false);
        // 0.15.0: the provider's Max output tokens, capped by what the model is known to allow.
        let known_max_output = limits::known_max_output(&model, info);
        let max_tokens =
            provider.map_or(0, |p| limits::request_max_tokens(p.kind, p.max_output_tokens, known_max_output));
        let problem = if !ai.enabled {
            Some("Turn on AI features in Settings → AI.".to_string())
        } else if provider.is_none() || model.is_empty() {
            Some("Choose a provider and a model in Settings → AI.".to_string())
        } else if provider.is_some_and(|p| p.kind.needs_key() && !self.ai_key_has(&p.id)) {
            Some(format!("Add an API key for {provider_name} in Settings → AI."))
        } else {
            None
        };
        let mut models = Vec::new();
        if let Some(p) = provider {
            let choice =
                |m: &str| ModelChoice { provider: p.id.clone(), provider_name: provider_name.clone(), model: m.into() };
            if !model.is_empty() {
                models.push(choice(&model));
            }
            for m in cached.iter().flatten() {
                if models.len() >= QUICK_MODELS {
                    break;
                }
                if m.id != model {
                    models.push(choice(&m.id));
                }
            }
        }
        let cfg = Config {
            model: model.clone(),
            provider_name: provider_name.clone(),
            vision,
            max_steps: ai.max_steps,
            system_extra: ai.system_prompt_extra.clone(),
            userscripts: s.userscripts.enabled,
            macros: s.macros.enabled,
            max_tokens,
            known_max_output,
        };
        Setup { cfg, provider: ai.active_provider.clone(), provider_name, models, problem }
    }

    fn agent_config_event(setup: &Setup) -> Event {
        Event::AgentConfig {
            provider: setup.provider.clone(),
            provider_name: setup.provider_name.clone(),
            model: setup.cfg.model.clone(),
            models: setup.models.clone(),
            vision: setup.cfg.vision,
            problem: setup.problem.clone(),
        }
    }

    /// Window `win`'s agent exists (created with the current settings).
    fn agent_ensure(&self, win: WindowId) {
        if self.agent.wins.borrow().contains_key(&win) {
            return;
        }
        let cfg = self.agent_setup().cfg;
        self.agent.wins.borrow_mut().entry(win).or_insert_with(|| AgentWin::new(cfg));
    }

    /// Every session gets the current settings and every panel its configuration.
    fn agent_push_config(&self) {
        let setup = self.agent_setup();
        for w in self.agent.wins.borrow_mut().values_mut() {
            w.session.set_config(setup.cfg.clone());
        }
        let event = Self::agent_config_event(&setup);
        for f in self.all_frames() {
            self.post_panel(f.id, &event);
        }
    }

    /// Asks the active provider for its model list (vision flags and the quick switch) once
    /// per settings change.
    fn agent_fetch_models(&self) {
        let id = {
            let s = self.settings.borrow();
            if !s.ai.enabled {
                return;
            }
            match s.ai.provider(&s.ai.active_provider) {
                Some(p) if !p.kind.needs_key() || self.ai_key_has(&p.id) => p.id.clone(),
                _ => return,
            }
        };
        if !self.agent.models_asked.borrow_mut().insert(id.clone()) {
            return;
        }
        let weak = self.me.clone();
        let id2 = id.clone();
        self.provider_models(&id, false, move |r| {
            let Some(b) = weak.upgrade() else { return };
            match r {
                Ok(list) => {
                    b.agent.models.borrow_mut().insert(id2, list);
                    b.agent_push_config();
                }
                Err(_) => warning!("agent: the model list is not available"),
            }
        });
    }

    /// Window `win`'s panel has loaded: its configuration. The conversation is in the panel
    /// already; a panel that loaded again (after its renderer ended) starts empty, and so does
    /// the conversation.
    pub(super) fn agent_panel_loaded(&self, win: WindowId) {
        let reloaded = !self.agent.loaded.borrow_mut().insert(win);
        self.agent_ensure(win);
        if reloaded {
            self.agent_input(win, Input::New);
            self.post_panel(win, &Event::AgentClear);
        }
        self.post_panel(win, &Self::agent_config_event(&self.agent_setup()));
        self.agent_fetch_models();
    }

    /// The window closed (or AI went off): stop its task and forget its conversation.
    pub(super) fn agent_window_closed(&self, win: WindowId) {
        self.agent.loaded.borrow_mut().remove(&win);
        // Stopped right here (not through the queue): the window's state goes next.
        let actions = self.agent.wins.borrow_mut().get_mut(&win).map(|w| {
            w.send_gen += 1;
            w.stop_at = None;
            w.session.handle(Input::Stop, mono_ms())
        });
        if let Some(actions) = actions {
            self.agent_actions(win, actions);
        }
        self.agent.wins.borrow_mut().remove(&win);
        self.agent.queue.borrow_mut().retain(|(w, _)| *w != win);
        // The domains it held for this window's tabs (all tabs when AI is off).
        let all = !self.ai_enabled();
        let tabs: Vec<TabId> = self
            .agent
            .cdp_tabs
            .borrow()
            .iter()
            .copied()
            .filter(|t| all || self.window_of(*t).is_none_or(|w| w == win))
            .collect();
        for tab in tabs {
            self.agent.cdp_tabs.borrow_mut().remove(&tab);
            self.agent.probes.borrow_mut().remove(&tab);
            if let Some(v) = self.tab_view(tab) {
                self.cdp_release(tab, &v, super::cdp::CdpUser::Agent);
            }
        }
        if all {
            self.agent.models.borrow_mut().clear();
            self.agent.models_asked.borrow_mut().clear();
        }
    }

    /// The AI settings changed (provider, model, prompt, steps): every panel's configuration.
    pub(super) fn agent_config_changed(&self) {
        self.agent.models_asked.borrow_mut().clear();
        self.agent_push_config();
        self.agent_fetch_models();
    }

    // ------------------------------------------------------------ panel requests

    /// A request from window `win`'s panel (`agent*` commands).
    pub(super) fn agent_request(&self, win: WindowId, req_id: Option<u32>, request: Request) {
        let result: Result<(), String> = match request {
            Request::AgentSend { text } => {
                self.agent_send(win, text);
                Ok(())
            }
            Request::AgentStop => {
                if let Some(w) = self.agent.wins.borrow_mut().get_mut(&win) {
                    w.stop_at = Some(Instant::now());
                    w.send_gen += 1;
                }
                self.agent_input(win, Input::Stop);
                Ok(())
            }
            Request::AgentNew => {
                if let Some(w) = self.agent.wins.borrow_mut().get_mut(&win) {
                    w.send_gen += 1;
                    w.shots.clear();
                }
                self.post_panel(win, &Event::AgentClear);
                self.agent_input(win, Input::New);
                Ok(())
            }
            Request::AgentScratchClear => {
                self.agent_input(win, Input::ScratchClear);
                Ok(())
            }
            Request::AgentAnswer { ask, choices, other } => {
                self.agent_input(win, Input::Answer { ask, answer: AskAnswer { choices, other } });
                Ok(())
            }
            Request::AgentConsent { ask, allow } => {
                self.agent_input(win, Input::Consent { ask, allow });
                Ok(())
            }
            Request::AgentContinue { ask, go } => {
                self.agent_input(win, Input::Continue { ask, go });
                Ok(())
            }
            Request::AgentSelectModel { provider, model } => self.agent_select_model(provider, model),
            Request::AgentModels => {
                let active = self.settings.borrow().ai.active_provider.clone();
                self.agent.models_asked.borrow_mut().remove(&active);
                self.post_panel(win, &Self::agent_config_event(&self.agent_setup()));
                self.agent_fetch_models();
                Ok(())
            }
            Request::AgentClose => {
                self.set_agent_panel(win, false);
                Ok(())
            }
            Request::AgentOpenScript { id, kind: _ } => {
                if lb_core::scripts_store::valid_id(&id) {
                    self.make_current(win);
                    let section = lb_core::pages::valid_section(&id).then_some(id.as_str());
                    self.open_page_at(Page::Scripts, section);
                    Ok(())
                } else {
                    Err("No such script".into())
                }
            }
            Request::AgentToggleScript { id, kind, enabled } => self.agent_toggle_script(win, &id, kind, enabled),
            _ => Ok(()),
        };
        match result {
            Ok(()) => {
                if req_id.is_some() {
                    self.post_panel(win, &Event::ok(req_id, serde_json::Value::Null));
                }
            }
            Err(e) => {
                if req_id.is_some() {
                    self.post_panel(win, &Event::err(req_id, e.clone()));
                }
                self.post_panel(win, &Event::AgentError { message: e });
            }
        }
    }

    /// The panel's quick switch: the active provider and model (saved like any setting).
    fn agent_select_model(&self, provider: String, model: String) -> Result<(), String> {
        let model = model.trim().to_string();
        if model.is_empty() || model.len() > 300 || self.settings.borrow().ai.provider(&provider).is_none() {
            return Err("That model is not available.".into());
        }
        self.try_change_settings(|s| {
            s.ai.active_provider = provider;
            s.ai.active_model = model;
        })
    }

    /// Disable / enable of a script card in the panel.
    fn agent_toggle_script(&self, win: WindowId, id: &str, kind: ScriptKind, enabled: bool) -> Result<(), String> {
        let card = match kind {
            ScriptKind::Userscript => self.userscript_toggle(id, enabled)?,
            ScriptKind::Macro => self.agent_macro_toggle(id, enabled)?,
        };
        self.post_panel(win, &Event::AgentScript { card });
        Ok(())
    }

    /// Turns a macro on or off (its hotkey with it).
    // TODO(integration): use WP-F's macro toggle once it has one (hotkeys, runner state).
    fn agent_macro_toggle(&self, id: &str, enabled: bool) -> Result<lb_core::agent::ScriptCard, String> {
        if !lb_core::scripts_store::valid_id(id) {
            return Err("No such macro".into());
        }
        let row = {
            let store = self.store.borrow();
            match store.set_macro_enabled(id, enabled, lb_core::storage::now_ms()) {
                Ok(true) => store.macro_by_id(id).ok().flatten(),
                Ok(false) => None,
                Err(e) => {
                    warning!("agent: macro not changed: {e}");
                    return Err("The macro could not be changed".into());
                }
            }
        };
        let Some(row) = row else { return Err("No such macro".into()) };
        self.notify_scripts_changed();
        Ok(lb_core::agent::ScriptCard { id: row.id, kind: ScriptKind::Macro, name: row.name, enabled: row.enabled })
    }

    /// The user's message: the context is read from the target tab, then the task starts.
    fn agent_send(&self, win: WindowId, text: String) {
        let setup = self.agent_setup();
        if let Some(problem) = setup.problem {
            self.post_panel(win, &Event::AgentError { message: problem });
            return;
        }
        self.agent_ensure(win);
        let gen = {
            let mut wins = self.agent.wins.borrow_mut();
            let Some(w) = wins.get_mut(&win) else { return };
            w.session.set_config(setup.cfg.clone());
            w.stop_at = None;
            w.send_gen += 1;
            w.send_gen
        };
        let offer =
            Offer { userscripts: setup.cfg.userscripts, macros: setup.cfg.macros, gated: gate::requested(&text) };
        let tools: Vec<&'static str> = agent_tools::offered(offer).iter().map(|t| t.name).collect();
        self.agent_context(win, tools, move |b, context| {
            let live = b.agent.wins.borrow().get(&win).is_some_and(|w| w.send_gen == gen);
            if live {
                b.agent_input(win, Input::User { text, context });
            }
        });
    }

    // ------------------------------------------------------------ the context block

    /// The window's tabs for the context and `list_tabs`, the active one first in `.0`.
    fn agent_tab_facts(&self, win: WindowId) -> (Option<TabFacts>, Vec<TabFacts>) {
        let Ok(ws) = self.ws.try_borrow() else { return (None, Vec::new()) };
        let Some(m) = ws.model(win) else { return (None, Vec::new()) };
        let active = m.active_id();
        let tabs: Vec<TabFacts> = m
            .tabs()
            .iter()
            .map(|t| TabFacts {
                id: t.id,
                title: t.title.clone(),
                url: lb_core::pages::display_url(&t.url),
                active: Some(t.id) == active,
                private: t.private,
                loading: t.loading,
            })
            .collect();
        (tabs.iter().find(|t| t.active).cloned(), tabs)
    }

    /// Builds the automatic context block for window `win` (its tabs and the target tab's page
    /// facts from the probe, unless it is one of Krunk's own pages); `done` gets it.
    fn agent_context(&self, win: WindowId, tools: Vec<&'static str>, done: impl FnOnce(&Browser, String) + 'static) {
        let (tab, tabs) = self.agent_tab_facts(win);
        let target = self.agent_target(win);
        let build = move |b: &Browser, page: Option<serde_json::Value>, note: Option<String>| {
            let input = ContextInput { tab, tabs, page, page_note: note, tools };
            // Card-like digit runs in titles or the selection never leave either.
            let text = redact::scrub(&context::context_block(&input), &[]);
            done(b, text);
        };
        let Some(target) = target else {
            build(self, None, Some("No tab is open in this window.".into()));
            return;
        };
        if let Err(why) = agent_pages::may_act(&target.url) {
            let note = if why.is_empty() { "Krunk's own page: its content is not available to the agent" } else { why };
            build(self, None, Some(note.into()));
            return;
        }
        // The page gets a few seconds; a busy page doesn't hold the task up.
        let once: Once<PageFacts> =
            Rc::new(RefCell::new(Some(Box::new(move |b: &Browser, (page, note): PageFacts| build(b, page, note)))));
        let once2 = once.clone();
        let weak = self.me.clone();
        crate::ffi::cef::post_ui_delayed(CONTEXT_WAIT_MS, move || {
            let f = once2.borrow_mut().take();
            if let (Some(f), Some(b)) = (f, weak.upgrade()) {
                f(&b, (None, Some("The page did not answer in time (it may be busy).".into())));
            }
        });
        let weak = self.me.clone();
        self.agent_probe(target.tab, "context", serde_json::json!([]), move |r| {
            let f = once.borrow_mut().take();
            let (Some(f), Some(b)) = (f, weak.upgrade()) else { return };
            match r {
                Ok(v) => f(&b, (Some(v), None)),
                Err(_) => f(&b, (None, Some("The page did not answer (it may be loading).".into()))),
            }
        });
    }

    // ------------------------------------------------------------ the loop

    /// Hands `input` to window `win`'s session and carries out what it asks (see the module
    /// comment on the queue).
    fn agent_input(&self, win: WindowId, input: Input) {
        self.agent.queue.borrow_mut().push_back((win, input));
        if self.agent.pumping.replace(true) {
            return;
        }
        loop {
            let next = self.agent.queue.borrow_mut().pop_front();
            let Some((win, input)) = next else { break };
            let actions = match self.agent.wins.borrow_mut().get_mut(&win) {
                Some(w) => w.session.handle(input, mono_ms()),
                None => continue,
            };
            self.agent_actions(win, actions);
        }
        self.agent.pumping.set(false);
    }

    fn agent_actions(&self, win: WindowId, actions: Vec<Action>) {
        for action in actions {
            match action {
                Action::Send(request) => self.agent_send_request(win, request),
                Action::CancelRequest => {
                    let job = self.agent.wins.borrow_mut().get_mut(&win).and_then(|w| {
                        w.req_gen += 1;
                        w.req_open = false;
                        w.job.take()
                    });
                    if let Some(job) = job {
                        self.provider_cancel(job);
                    }
                }
                Action::RunTool { call_id, name, args, step } => {
                    let gen = {
                        let mut wins = self.agent.wins.borrow_mut();
                        let Some(w) = wins.get_mut(&win) else { continue };
                        w.tool_gen += 1;
                        w.tool = Some((call_id.clone(), step));
                        w.tool_gen
                    };
                    let weak = self.me.clone();
                    self.run_agent_tool(
                        win,
                        name,
                        args,
                        Box::new(move |outcome| {
                            if let Some(b) = weak.upgrade() {
                                b.agent_tool_done(win, gen, call_id, outcome);
                            }
                        }),
                    );
                }
                Action::CancelTool => {
                    if let Some(w) = self.agent.wins.borrow_mut().get_mut(&win) {
                        w.tool_gen += 1;
                        w.tool = None;
                    }
                }
                Action::ReleaseInput => {
                    // A task that finished normally leaves a macro it started running (holding
                    // its keys) until the user stops it (toolbar, Ctrl+Shift+.); Stop, an error
                    // or a closed window end it and release everything.
                    let done = self.agent.wins.borrow().get(&win).is_some_and(|w| w.session.state() == RunState::Done);
                    if !done {
                        self.stop_agent_macros();
                    } else if !self.macro_running() {
                        self.input_release_all();
                    }
                }
                Action::Emit(event) => {
                    if let Event::AgentState { state: RunState::Stopped, .. } = &*event {
                        let asked = self.agent.wins.borrow_mut().get_mut(&win).and_then(|w| w.stop_at.take());
                        if let Some(t) = asked {
                            info!("window {win}: agent: stopped in {} ms", t.elapsed().as_millis());
                        }
                    }
                    self.post_panel(win, &event);
                }
                Action::Log(line) => info!("window {win}: {line}"),
            }
        }
    }

    /// Streams `request` to the active provider; its events and end go to the session unless
    /// the request was cancelled meanwhile.
    fn agent_send_request(&self, win: WindowId, request: lb_core::agent::ChatRequest) {
        let provider = self.settings.borrow().ai.active_provider.clone();
        let gen = {
            let mut wins = self.agent.wins.borrow_mut();
            let Some(w) = wins.get_mut(&win) else { return };
            w.req_gen += 1;
            w.req_open = true;
            w.job = None;
            w.req_gen
        };
        let weak = self.me.clone();
        let weak2 = self.me.clone();
        let job = self.provider_stream(
            &provider,
            &request,
            move |event| {
                let Some(b) = weak.upgrade() else { return };
                if b.agent.wins.borrow().get(&win).is_some_and(|w| w.req_gen == gen && w.req_open) {
                    b.agent_input(win, Input::Stream(event));
                }
            },
            move |result| {
                let Some(b) = weak2.upgrade() else { return };
                let live = b.agent.wins.borrow_mut().get_mut(&win).is_some_and(|w| {
                    let live = w.req_gen == gen && w.req_open;
                    if live {
                        w.req_open = false;
                        w.job = None;
                    }
                    live
                });
                if live {
                    b.agent_input(
                        win,
                        match result {
                            Ok(()) => Input::StreamEnd,
                            Err(message) => Input::StreamFailed(message),
                        },
                    );
                }
            },
        );
        if let Some(job) = job {
            // Ended already (nothing to keep), or cancelled before the job was known.
            let cancelled = match self.agent.wins.borrow_mut().get_mut(&win) {
                Some(w) if w.req_gen == gen => {
                    if w.req_open {
                        w.job = Some(job);
                    }
                    false
                }
                _ => true,
            };
            if cancelled {
                self.provider_cancel(job);
            }
        }
    }

    /// A tool ended: its outcome goes to the session if it is still the running tool.
    fn agent_tool_done(&self, win: WindowId, gen: u64, call_id: String, outcome: lb_core::agent::ToolOutcome) {
        let live = self.agent.wins.borrow_mut().get_mut(&win).is_some_and(|w| {
            let live = w.tool_gen == gen && w.tool.as_ref().is_some_and(|(id, _)| *id == call_id);
            if live {
                w.tool = None;
            }
            live
        });
        if live {
            self.agent_input(win, Input::ToolDone { call_id, outcome });
        }
    }

    /// The running tool of window `win`: its generation and step number.
    fn agent_running_tool(&self, win: WindowId) -> Option<(u64, u32)> {
        let wins = self.agent.wins.borrow();
        let w = wins.get(&win)?;
        w.tool.as_ref().map(|(_, step)| (w.tool_gen, *step))
    }

    /// Is the tool of generation `gen` still wanted? (Waits and polls end early otherwise.)
    fn agent_tool_live(&self, win: WindowId, gen: u64) -> bool {
        self.agent.wins.borrow().get(&win).is_some_and(|w| w.tool_gen == gen && w.tool.is_some())
    }

    /// The model of window `win` takes images.
    fn agent_vision(&self, win: WindowId) -> bool {
        self.agent.wins.borrow().get(&win).is_some_and(|w| w.session.config().vision)
    }

    /// Keeps a step screenshot for the panel; returns its address there.
    fn agent_store_shot(&self, win: WindowId, step: u32, jpeg: Vec<u8>) -> Option<String> {
        let mut wins = self.agent.wins.borrow_mut();
        let w = wins.get_mut(&win)?;
        w.shots.retain(|(n, _)| *n != step);
        w.shots.push_back((step, jpeg));
        while w.shots.len() > MAX_SHOTS {
            w.shots.pop_front();
        }
        Some(format!("{SHOT_PREFIX}{}/{step}.jpg", w.token))
    }

    /// A step screenshot for the shell origin's server: `/agent-shot/<token>/<n>.jpg` (the
    /// token is known only to that window's panel).
    pub(super) fn agent_shot(&self, path: &str) -> Option<Vec<u8>> {
        let rest = path.strip_prefix(SHOT_PREFIX)?;
        let (token, file) = rest.split_once('/')?;
        let n: u32 = file.strip_suffix(".jpg")?.parse().ok()?;
        if token.len() != 32 {
            return None;
        }
        let wins = self.agent.wins.borrow();
        let w = wins.values().find(|w| w.token == token)?;
        w.shots.iter().find(|(s, _)| *s == n).map(|(_, b)| b.clone())
    }

    // ------------------------------------------------------------ tabs

    /// A tab view is set up. Nothing to hook: the agent's probe is set up on first use, and
    /// the media log of the plan needs network events the browser doesn't get (see the module
    /// comment).
    pub(super) fn agent_attach_tab(&self, tab: TabId, view: &crate::ffi::cef::View) {
        let _ = (tab, view);
    }

    /// A tab view is gone: its probe world and domain use with it (`cdp_forget` follows).
    pub(super) fn agent_tab_gone(&self, tab: TabId) {
        self.agent.probes.borrow_mut().remove(&tab);
        self.agent.cdp_tabs.borrow_mut().remove(&tab);
    }
}
