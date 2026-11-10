//! The AI agent of each window (0.12.0, WP-B2): the conversation (`lb_core::agent::session`),
//! the provider call (`provider.rs`, WP-A) and the tools (`tools.rs`).
//!
//! The host side of the loop: `Session::handle` returns actions, this module carries them out
//! (send a provider request, run a tool, post to the panel) and feeds the results back. While
//! AI is off nothing here runs: there is no panel to send `agent*` requests.
//!
//! M2 spike: until WP-B2 replaces this file, "/spike …" messages from the panel run the
//! engine experiments of the plan (streaming and cancel, key repeat, compile check,
//! screenshot) and report in the panel.

// 0.12.0 spine: parts of this are used once the work packages are wired in (M4).
#![allow(dead_code)]

pub(super) mod provider;
mod tools;

use super::Browser;
use crate::ffi::cef::{fetch, View};
use crate::log::info;
use lb_core::agent::RunState;
use lb_core::protocol::{Event, Request};
use lb_core::tabs::TabId;
use lb_core::windows::WindowId;
use serde_json::json;
use std::cell::{Cell, RefCell};
use std::time::Instant;

/// Every window's agent.
#[derive(Default)]
pub(super) struct AgentHost {
    /// Spike: the running stream and when Stop was pressed.
    spike_job: Cell<Option<fetch::JobId>>,
    spike_stop: RefCell<Option<Instant>>,
}

impl Browser {
    /// Window `win`'s panel has loaded: its configuration and current conversation.
    pub(super) fn agent_panel_loaded(&self, win: WindowId) {
        self.post_panel(
            win,
            &Event::AgentConfig {
                provider: String::new(),
                provider_name: "Spike".into(),
                model: "none".into(),
                models: Vec::new(),
                vision: false,
                problem: None,
            },
        );
    }

    /// The window closed (or AI went off): stop its task and forget its conversation.
    pub(super) fn agent_window_closed(&self, win: WindowId) {
        let _ = win;
        if let Some(job) = self.agent.spike_job.take() {
            fetch::cancel(job);
        }
    }

    /// The AI settings changed (provider, model, prompt, steps): every panel's configuration.
    pub(super) fn agent_config_changed(&self) {}

    /// A request from window `win`'s panel (`agent*` commands).
    pub(super) fn agent_request(&self, win: WindowId, req_id: Option<u32>, request: Request) {
        let _ = req_id;
        match request {
            Request::AgentSend { text } => {
                self.post_panel(win, &Event::AgentMessage { role: "user".into(), text: text.clone() });
                self.spike(win, text.trim());
            }
            Request::AgentStop => {
                if let Some(job) = self.agent.spike_job.get() {
                    *self.agent.spike_stop.borrow_mut() = Some(Instant::now());
                    fetch::cancel(job);
                }
            }
            Request::AgentNew => self.post_panel(win, &Event::AgentClear),
            Request::AgentClose => self.set_agent_panel(win, false),
            _ => {}
        }
    }

    /// A tab view is set up (its DevTools listeners, e.g. the media log).
    pub(super) fn agent_attach_tab(&self, tab: TabId, view: &View) {
        let _ = (tab, view);
    }

    /// A tab view is gone.
    pub(super) fn agent_tab_gone(&self, tab: TabId) {
        let _ = tab;
    }

    fn spike_note(&self, win: WindowId, text: impl Into<String>) {
        let text = text.into();
        info!("window {win}: spike: {text}");
        self.post_panel(win, &Event::AgentMessage { role: "notice".into(), text });
    }

    fn spike(&self, win: WindowId, text: &str) {
        let mut parts = text.split_whitespace();
        if parts.next() != Some("/spike") {
            self.spike_note(win, "M2 spike build: use /spike stream URL | keys | ctrlt | compile | shot");
            return;
        }
        let what = parts.next().unwrap_or("");
        let view = self.active_view_of(win);
        match (what, view) {
            ("stream", _) => {
                let url = parts.next().unwrap_or("http://127.0.0.1:18111/v1/chat/completions").to_string();
                let body = json!({"model": "mock", "stream": true, "messages": [{"role": "user", "content": "slow"}]})
                    .to_string();
                let headers = vec![("Content-Type".to_string(), "application/json".to_string())];
                let start = Instant::now();
                self.post_panel(win, &Event::AgentState { state: RunState::Running, steps: 0, ms: None });
                let weak = self.me.clone();
                let weak2 = self.me.clone();
                let job = fetch::request(
                    fetch::Req {
                        method: "POST",
                        url: &url,
                        headers: &headers,
                        body: Some(body.as_bytes()),
                        idle_timeout_ms: 30_000,
                        max_bytes: 4 << 20,
                    },
                    move |status, chunk| {
                        if let Some(b) = weak.upgrade() {
                            let ms = start.elapsed().as_millis();
                            let text = String::from_utf8_lossy(chunk).replace('\n', "⏎");
                            b.post_panel(
                                win,
                                &Event::AgentDelta {
                                    text: format!("[+{ms} ms, {status}, {} B] {text}\n", chunk.len()),
                                },
                            );
                        }
                    },
                    move |r| {
                        let Some(b) = weak2.upgrade() else { return };
                        b.agent.spike_job.set(None);
                        let stop = b.agent.spike_stop.borrow_mut().take();
                        let after =
                            stop.map(|t| format!(", {} ms after Stop", t.elapsed().as_millis())).unwrap_or_default();
                        let what = match &r {
                            Ok(f) => format!("done: HTTP {}", f.status),
                            Err(e) => format!("ended: {e:?}"),
                        };
                        b.spike_note(win, format!("stream {what} at {} ms{after}", start.elapsed().as_millis()));
                        let state = if stop.is_some() { RunState::Stopped } else { RunState::Done };
                        b.post_panel(win, &Event::AgentState { state, steps: 0, ms: None });
                    },
                );
                self.agent.spike_job.set(Some(job));
            }
            ("keys", Some(v)) => {
                // 'a' held: one keyDown, then auto-repeats every 33 ms for a second, then keyUp.
                let key = |kind: &str, repeat: bool| {
                    json!({"type": kind, "key": "a", "code": "KeyA", "windowsVirtualKeyCode": 65,
                           "nativeVirtualKeyCode": 65, "text": if kind == "keyUp" { "" } else { "a" },
                           "autoRepeat": repeat})
                };
                v.devtools_method("Input.dispatchKeyEvent", &key("keyDown", false));
                for i in 1..=30 {
                    let v2 = v.clone();
                    let ev = key("keyDown", true);
                    crate::ffi::cef::post_ui_delayed(250 + i * 33, move || {
                        v2.devtools_method("Input.dispatchKeyEvent", &ev)
                    });
                }
                let v2 = v.clone();
                let up = key("keyUp", false);
                let weak = self.me.clone();
                crate::ffi::cef::post_ui_delayed(250 + 31 * 33, move || {
                    v2.devtools_method("Input.dispatchKeyEvent", &up);
                    if let Some(b) = weak.upgrade() {
                        b.spike_note(win, "keys: sent keyDown a, 30 repeats, keyUp");
                    }
                });
            }
            ("ctrlt", Some(v)) => {
                // Does an engine key event reach Krunk's shortcuts? (A new tab would open.)
                let tabs = self.ws.borrow().model(win).map_or(0, |m| m.tabs().len());
                let ev = |kind: &str| json!({"type": kind, "key": "t", "code": "KeyT", "windowsVirtualKeyCode": 84, "modifiers": 2});
                v.devtools_method("Input.dispatchKeyEvent", &ev("rawKeyDown"));
                v.devtools_method("Input.dispatchKeyEvent", &ev("keyUp"));
                let weak = self.me.clone();
                crate::ffi::cef::post_ui_delayed(800, move || {
                    if let Some(b) = weak.upgrade() {
                        let now = b.ws.borrow().model(win).map_or(0, |m| m.tabs().len());
                        b.spike_note(win, format!("ctrlt: tabs before {tabs}, after {now}"));
                    }
                });
            }
            ("compile", Some(v)) => {
                v.devtools_method("Runtime.enable", &json!({}));
                let weak = self.me.clone();
                for (name, src) in [("bad", "let x = ;\nfoo("), ("good", "const a = 1;\nconsole.log(a);")] {
                    let weak = weak.clone();
                    let params = json!({"expression": src, "sourceURL": "spike.js", "persistScript": false});
                    v.devtools_call("Runtime.compileScript", &params, move |r| {
                        if let Some(b) = weak.upgrade() {
                            let text = match r {
                                Ok(v) => match v.get("exceptionDetails") {
                                    Some(e) => format!(
                                        "error at {}:{}: {}",
                                        e["lineNumber"],
                                        e["columnNumber"],
                                        e["exception"]["description"].as_str().unwrap_or("?")
                                    ),
                                    None => "compiles".into(),
                                },
                                Err(e) => format!("call failed: {e}"),
                            };
                            b.spike_note(win, format!("compile {name}: {text}"));
                        }
                    });
                }
            }
            ("shot", Some(v)) => {
                let weak = self.me.clone();
                let start = Instant::now();
                v.devtools_call("Page.captureScreenshot", &json!({"format": "png"}), move |r| {
                    let Some(b) = weak.upgrade() else { return };
                    let text = match r.map(|v| v["data"].as_str().unwrap_or("").to_string()) {
                        Ok(data) => {
                            use base64ct::Encoding;
                            match base64ct::Base64::decode_vec(&data) {
                                Ok(png) => {
                                    let dir = crate::ffi::dialogs::downloads_folder().unwrap_or_default();
                                    let path = dir.join("krunk-spike.png");
                                    match std::fs::write(&path, &png) {
                                        Ok(()) => format!(
                                            "{} bytes in {} ms → {}",
                                            png.len(),
                                            start.elapsed().as_millis(),
                                            path.display()
                                        ),
                                        Err(e) => format!("not written: {e}"),
                                    }
                                }
                                Err(_) => "bad base64".into(),
                            }
                        }
                        Err(e) => format!("failed: {e}"),
                    };
                    b.spike_note(win, format!("shot: {text}"));
                });
            }
            (_, None) => self.spike_note(win, "no active tab"),
            _ => self.spike_note(win, "unknown spike"),
        }
    }
}
