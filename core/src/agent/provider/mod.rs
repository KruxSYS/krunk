//! Provider requests and responses (WP-A).
//!
//! The host builds an [`HttpRequest`] here, adds [`auth_headers`] (the key is unwrapped only for
//! that call), sends it with the engine's URL loader and feeds the response bytes to a
//! [`Decoder`], which yields [`StreamEvent`]s. [`Assembled`] collects them into the assistant's
//! message. Model lists come from [`models_request`] + [`parse_models`].

pub mod anthropic;
pub mod models;
pub mod openai;

use super::{ChatRequest, ModelInfo, ProviderKind, StopReason, StreamEvent, ToolCall};
use serde_json::Value;

/// A request for the host's HTTP client. Never carries the API key (see [`auth_headers`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

/// The longest provider message shown in the panel or on the settings page.
pub const MAX_MESSAGE_CHARS: usize = 300;

/// Is `base` usable for `kind`? Named providers must use their fixed URL; a custom server must
/// be `https://`, or `http://` on a loopback or private-network host (Ollama, LM Studio).
pub fn check_base_url(kind: ProviderKind, base: &str) -> Result<(), String> {
    if kind != ProviderKind::Custom {
        return if base.is_empty() || base == kind.default_base_url() {
            Ok(())
        } else {
            Err(format!("{} uses its own address", kind.label()))
        };
    }
    let base = base.trim();
    if base.is_empty() {
        return Err("Enter the server's address".into());
    }
    if base.len() > 500 || base.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("The server's address is not valid".into());
    }
    let url = url::Url::parse(base).map_err(|_| "The server's address is not valid".to_string())?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err("Leave the user name and password out of the address: put a key in the key field".into());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("The server's address cannot have a query (?…) or a fragment (#…)".into());
    }
    let Some(host) = url.host() else { return Err("The server's address needs a host".into()) };
    match url.scheme() {
        "https" => Ok(()),
        "http" if is_local_host(&host) => Ok(()),
        "http" => Err("Use https:// (http:// only for this computer or the local network)".into()),
        _ => Err("The address must start with https://".into()),
    }
}

/// Loopback, private IPv4 ranges, IPv6 unique-local (fd00::/8), `localhost` and `*.local`.
fn is_local_host(host: &url::Host<&str>) -> bool {
    match host {
        url::Host::Domain(d) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            d == "localhost" || d.ends_with(".localhost") || (d.ends_with(".local") && d.len() > ".local".len())
        }
        url::Host::Ipv4(ip) => ip.is_loopback() || ip.is_private(),
        url::Host::Ipv6(ip) => ip.is_loopback() || ip.octets()[0] == 0xfd,
    }
}

/// The base URL actually used: the fixed one for named providers, the user's (without a
/// trailing slash) for `Custom`.
pub fn base_url(kind: ProviderKind, custom: &str) -> String {
    match kind {
        ProviderKind::Custom => custom.trim().trim_end_matches('/').to_string(),
        k => k.default_base_url().to_string(),
    }
}

/// The streamed chat call (`stream: true`), without authentication headers.
pub fn chat_request(kind: ProviderKind, base: &str, req: &ChatRequest) -> HttpRequest {
    match kind.style() {
        super::ApiStyle::OpenAi => openai::chat_request(kind, base, req),
        super::ApiStyle::Anthropic => anthropic::chat_request(base, req),
    }
}

/// `GET {base}/models`, without authentication headers.
pub fn models_request(kind: ProviderKind, base: &str) -> HttpRequest {
    let mut headers = Vec::new();
    if kind == ProviderKind::Anthropic {
        headers.push(("anthropic-version".to_string(), anthropic::VERSION.to_string()));
    }
    // Anthropic pages its list (20 by default); 1000 is its maximum and fits every model.
    let query = if kind == ProviderKind::Anthropic { "?limit=1000" } else { "" };
    HttpRequest { method: "GET", url: format!("{base}/models{query}"), headers, body: None }
}

/// The headers that carry the key. The caller keeps the result in zeroizing memory and drops it
/// right after handing it to the engine. An empty key (custom server) gives no header.
pub fn auth_headers(kind: ProviderKind, key: &str) -> Vec<(String, String)> {
    if key.is_empty() {
        return Vec::new();
    }
    match kind {
        ProviderKind::Anthropic => vec![("x-api-key".into(), key.into())],
        _ => vec![("Authorization".into(), format!("Bearer {key}"))],
    }
}

/// Parses a model list response.
pub fn parse_models(kind: ProviderKind, body: &str) -> Result<Vec<ModelInfo>, String> {
    models::parse(kind, body)
}

/// A short message for the panel from an error response (status plus the provider's own
/// message, at most 300 characters). Never logged.
pub fn error_message(status: u16, body: &str) -> String {
    let message = match serde_json::from_str::<Value>(body) {
        Ok(v) => provider_text(&v),
        // Plain text is shown as it is; an HTML error page is not.
        Err(_) if !body.trim_start().starts_with('<') => Some(clean(body)),
        Err(_) => None,
    }
    .filter(|m| !m.is_empty());
    let hint = match status {
        401 | 403 => Some("Check the API key."),
        404 => Some("Check the model id and the server address."),
        429 => Some("Too many requests, or no credit left: wait a moment or check your account."),
        _ => None,
    };
    let mut out = match message {
        Some(m) => format!("HTTP {status}: {m}"),
        None => format!("HTTP {status} from the provider"),
    };
    if let Some(h) = hint {
        if !out.ends_with(['.', '!', '?', '…']) {
            out.push('.');
        }
        out.push(' ');
        out.push_str(h);
    }
    out
}

/// The provider's own message in an error body (OpenAI, OpenRouter, Anthropic and common
/// server shapes), cleaned and shortened.
pub(crate) fn provider_text(v: &Value) -> Option<String> {
    let s = |x: Option<&Value>| x.and_then(Value::as_str).map(str::to_string);
    let text = s(v.pointer("/error/message"))
        .or_else(|| s(v.get("error")))
        .or_else(|| s(v.get("message")))
        .or_else(|| s(v.get("detail")))
        .or_else(|| s(v.pointer("/error/type")))?;
    Some(clean(&text))
}

/// Control characters out, runs of white space made one space, at most
/// [`MAX_MESSAGE_CHARS`] characters.
pub(crate) fn clean(text: &str) -> String {
    let mut out = String::new();
    let mut count = 0;
    for word in text.split(|c: char| c.is_whitespace() || c.is_control()).filter(|w| !w.is_empty()) {
        let word: String = word.chars().filter(|c| !is_invisible(*c)).collect();
        if word.is_empty() {
            continue;
        }
        if count > 0 {
            out.push(' ');
            count += 1;
        }
        for c in word.chars() {
            if count >= MAX_MESSAGE_CHARS {
                out.push('…');
                return out;
            }
            out.push(c);
            count += 1;
        }
    }
    out
}

/// Format characters that can reorder or hide text (bidi controls, zero-width marks).
fn is_invisible(c: char) -> bool {
    matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2069}' | '\u{feff}')
}

/// How the response body is read: Server-Sent Events, or one JSON document (an error, or a
/// server that ignored `stream: true`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Unknown,
    Sse,
    Json,
}

/// A JSON body bigger than this is not read.
const MAX_JSON_BODY: usize = 4 << 20;

/// An OpenAI-style tool call being streamed.
#[derive(Debug, Default)]
struct Call {
    /// The provider's `index` (or our position when it gives none).
    key: Option<u64>,
    id: String,
    name: String,
    started: bool,
    /// Argument text that came before the name.
    pending: String,
}

/// Turns response bytes into [`StreamEvent`]s for one provider style.
pub struct Decoder {
    kind: ProviderKind,
    sse: super::sse::SseParser,
    mode: Mode,
    raw: Vec<u8>,
    /// `Done` was emitted.
    stopped: bool,
    /// An `Error` was emitted.
    failed: bool,
    /// OpenAI-style: the tool calls by our index.
    calls: Vec<Call>,
    /// Anthropic: content block index → our tool call index.
    blocks: Vec<(u64, usize)>,
    tools: usize,
    stop: Option<StopReason>,
    input_tokens: u32,
}

impl Decoder {
    pub fn new(kind: ProviderKind) -> Decoder {
        Decoder {
            kind,
            sse: super::sse::SseParser::new(),
            mode: Mode::Unknown,
            raw: Vec::new(),
            stopped: false,
            failed: false,
            calls: Vec::new(),
            blocks: Vec::new(),
            tools: 0,
            stop: None,
            input_tokens: 0,
        }
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Vec<StreamEvent> {
        if self.mode == Mode::Unknown {
            match chunk.iter().find(|b| !b.is_ascii_whitespace()) {
                Some(b'{') | Some(b'[') => self.mode = Mode::Json,
                Some(_) => self.mode = Mode::Sse,
                None => return Vec::new(),
            }
        }
        if self.mode == Mode::Json {
            if self.raw.len() + chunk.len() <= MAX_JSON_BODY {
                self.raw.extend_from_slice(chunk);
            }
            return Vec::new();
        }
        let events = self.sse.feed(chunk);
        let mut out = Vec::new();
        for e in events {
            self.event(e, &mut out);
        }
        out
    }

    /// The response ended (also emits `Done` if the provider never said why it stopped).
    pub fn finish(&mut self) -> Vec<StreamEvent> {
        let mut out = Vec::new();
        for e in self.sse.finish() {
            self.event(e, &mut out);
        }
        if self.mode == Mode::Json {
            let raw = std::mem::take(&mut self.raw);
            match serde_json::from_slice::<Value>(&raw) {
                Ok(v) => self.whole(&v, &mut out),
                Err(_) => self.fail("The provider sent an answer Krunk could not read".into(), &mut out),
            }
        }
        if !self.stopped && !self.failed {
            self.start_pending(&mut out);
            let why = self.stop.take().unwrap_or(if self.tools > 0 || !self.calls.is_empty() {
                StopReason::ToolUse
            } else {
                StopReason::EndTurn
            });
            self.done(why, &mut out);
        }
        out
    }

    fn event(&mut self, e: super::sse::SseEvent, out: &mut Vec<StreamEvent>) {
        let data = e.data.trim();
        if data.is_empty() || data == "[DONE]" {
            return;
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else { return };
        match self.kind.style() {
            super::ApiStyle::OpenAi => self.openai(&v, out),
            super::ApiStyle::Anthropic => self.anthropic(e.event.as_deref(), &v, out),
        }
    }

    fn done(&mut self, why: StopReason, out: &mut Vec<StreamEvent>) {
        if !self.stopped && !self.failed {
            self.stopped = true;
            out.push(StreamEvent::Done(why));
        }
    }

    fn fail(&mut self, message: String, out: &mut Vec<StreamEvent>) {
        if !self.failed {
            self.failed = true;
            let m = if message.is_empty() { "The provider reported an error".to_string() } else { message };
            out.push(StreamEvent::Error(m));
        }
    }

    // ---- OpenAI style ----

    fn openai(&mut self, v: &Value, out: &mut Vec<StreamEvent>) {
        if v.get("error").is_some_and(|e| !e.is_null()) {
            let m = provider_text(v).unwrap_or_default();
            self.fail(m, out);
            return;
        }
        for choice in v.get("choices").and_then(Value::as_array).into_iter().flatten() {
            if let Some(delta) = choice.get("delta") {
                if let Some(t) = delta.get("content").and_then(Value::as_str) {
                    if !t.is_empty() {
                        out.push(StreamEvent::TextDelta(t.to_string()));
                    }
                }
                for (pos, tc) in delta.get("tool_calls").and_then(Value::as_array).into_iter().flatten().enumerate() {
                    self.openai_call(tc, pos, out);
                }
            }
            if let Some(r) = choice.get("finish_reason").and_then(Value::as_str) {
                self.start_pending(out);
                let why = match r {
                    "stop" => StopReason::EndTurn,
                    "tool_calls" | "function_call" => StopReason::ToolUse,
                    "length" => StopReason::MaxTokens,
                    other => StopReason::Other(other.to_string()),
                };
                self.done(why, out);
            }
        }
        if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            out.push(StreamEvent::Usage { input: num(u, "prompt_tokens"), output: num(u, "completion_tokens") });
        }
    }

    /// One `tool_calls` delta item; `pos` is its place in this chunk's array.
    fn openai_call(&mut self, tc: &Value, pos: usize, out: &mut Vec<StreamEvent>) {
        let key = tc.get("index").and_then(Value::as_u64);
        let id = tc.get("id").and_then(Value::as_str).filter(|s| !s.is_empty());
        let name = tc.pointer("/function/name").and_then(Value::as_str).filter(|s| !s.is_empty());
        let args = tc.pointer("/function/arguments").and_then(Value::as_str).unwrap_or("");
        let found = match key {
            Some(k) => self.calls.iter().position(|c| c.key == Some(k)),
            // No index (some servers): a new id is a new call, otherwise the same position in
            // the array as before, or the last call.
            None => match id {
                Some(id) => self.calls.iter().position(|c| c.id == id),
                None => (pos < self.calls.len()).then_some(pos).or(self.calls.len().checked_sub(1)),
            },
        };
        let index = found.unwrap_or_else(|| {
            self.calls.push(Call { key, ..Call::default() });
            self.calls.len() - 1
        });
        let call = &mut self.calls[index];
        if let Some(id) = id {
            if call.id.is_empty() {
                call.id = id.to_string();
            }
        }
        if let Some(n) = name {
            if call.name.is_empty() {
                call.name = n.to_string();
            }
        }
        if !call.started && !call.name.is_empty() {
            self.start(index, out);
        }
        let call = &mut self.calls[index];
        if !args.is_empty() {
            if call.started {
                out.push(StreamEvent::ToolCallArgs { index, fragment: args.to_string() });
            } else {
                call.pending.push_str(args);
            }
        }
    }

    fn start(&mut self, index: usize, out: &mut Vec<StreamEvent>) {
        let call = &mut self.calls[index];
        call.started = true;
        if call.id.is_empty() {
            call.id = format!("call_{index}");
        }
        out.push(StreamEvent::ToolCallStart { index, id: call.id.clone(), name: call.name.clone() });
        if !call.pending.is_empty() {
            out.push(StreamEvent::ToolCallArgs { index, fragment: std::mem::take(&mut call.pending) });
        }
    }

    /// Calls whose name never came start now (the session reports the unknown tool).
    fn start_pending(&mut self, out: &mut Vec<StreamEvent>) {
        for i in 0..self.calls.len() {
            if !self.calls[i].started {
                self.start(i, out);
            }
        }
    }

    // ---- Anthropic ----

    fn anthropic(&mut self, name: Option<&str>, v: &Value, out: &mut Vec<StreamEvent>) {
        let kind = v.get("type").and_then(Value::as_str).or(name).unwrap_or("");
        match kind {
            "message_start" => {
                self.input_tokens = num(v.pointer("/message/usage").unwrap_or(&Value::Null), "input_tokens");
            }
            "content_block_start" => {
                let block = v.get("content_block").unwrap_or(&Value::Null);
                let at = v.get("index").and_then(Value::as_u64).unwrap_or(0);
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(t) = block.get("text").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                            out.push(StreamEvent::TextDelta(t.to_string()));
                        }
                    }
                    Some("tool_use") => {
                        let index = self.tools;
                        self.tools += 1;
                        self.blocks.push((at, index));
                        let id = block.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                        let id = if id.is_empty() { format!("toolu_{index}") } else { id };
                        let name = block.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                        out.push(StreamEvent::ToolCallStart { index, id, name });
                        // The input normally streams as deltas; a non-empty one given here is kept.
                        if let Some(input) = block.get("input").filter(|i| i.as_object().is_some_and(|o| !o.is_empty()))
                        {
                            out.push(StreamEvent::ToolCallArgs { index, fragment: input.to_string() });
                        }
                    }
                    _ => {} // thinking, redacted_thinking, server tools
                }
            }
            "content_block_delta" => {
                let at = v.get("index").and_then(Value::as_u64).unwrap_or(0);
                let delta = v.get("delta").unwrap_or(&Value::Null);
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        if let Some(t) = delta.get("text").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                            out.push(StreamEvent::TextDelta(t.to_string()));
                        }
                    }
                    Some("input_json_delta") => {
                        let frag = delta.get("partial_json").and_then(Value::as_str).unwrap_or("");
                        if let Some(&(_, index)) = self.blocks.iter().find(|(b, _)| *b == at) {
                            if !frag.is_empty() {
                                out.push(StreamEvent::ToolCallArgs { index, fragment: frag.to_string() });
                            }
                        }
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                if let Some(r) = v.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    self.stop = Some(anthropic_stop(r));
                }
                if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
                    let input = u.get("input_tokens").and_then(Value::as_u64).map_or(self.input_tokens, sat);
                    out.push(StreamEvent::Usage { input, output: num(u, "output_tokens") });
                }
            }
            "message_stop" => {
                let why = self.stop.take().unwrap_or(StopReason::EndTurn);
                self.done(why, out);
            }
            "error" => {
                let m = provider_text(v).unwrap_or_default();
                self.fail(m, out);
            }
            _ => {} // ping, content_block_stop
        }
    }

    // ---- a whole JSON body ----

    /// An error document, or a complete (non-streamed) answer.
    fn whole(&mut self, v: &Value, out: &mut Vec<StreamEvent>) {
        let is_error = v.get("error").is_some_and(|e| !e.is_null())
            || v.get("type").and_then(Value::as_str) == Some("error")
            || (v.get("choices").is_none() && v.get("content").is_none());
        if is_error {
            let m = provider_text(v).unwrap_or_else(|| "The provider sent an answer Krunk could not read".into());
            self.fail(m, out);
            return;
        }
        match self.kind.style() {
            super::ApiStyle::OpenAi => {
                let choice = v.pointer("/choices/0").unwrap_or(&Value::Null);
                let msg = choice.get("message").unwrap_or(&Value::Null);
                if let Some(t) = msg.get("content").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                    out.push(StreamEvent::TextDelta(t.to_string()));
                }
                for (pos, tc) in msg.get("tool_calls").and_then(Value::as_array).into_iter().flatten().enumerate() {
                    // Each is complete: it gets its own index.
                    let mut tc = tc.clone();
                    if let Some(o) = tc.as_object_mut() {
                        o.insert("index".into(), Value::from(pos));
                    }
                    self.openai_call(&tc, pos, out);
                }
                let wrapped = serde_json::json!({
                    "choices": [{"finish_reason": choice.get("finish_reason").cloned().unwrap_or(Value::Null)}],
                    "usage": v.get("usage").cloned().unwrap_or(Value::Null),
                });
                self.openai(&wrapped, out);
            }
            super::ApiStyle::Anthropic => {
                for block in v.get("content").and_then(Value::as_array).into_iter().flatten() {
                    match block.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            if let Some(t) = block.get("text").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                                out.push(StreamEvent::TextDelta(t.to_string()));
                            }
                        }
                        Some("tool_use") => {
                            let index = self.tools;
                            self.tools += 1;
                            let id = block.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                            let name = block.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                            out.push(StreamEvent::ToolCallStart { index, id, name });
                            let input = block.get("input").cloned().unwrap_or_else(|| serde_json::json!({}));
                            out.push(StreamEvent::ToolCallArgs { index, fragment: input.to_string() });
                        }
                        _ => {}
                    }
                }
                if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
                    out.push(StreamEvent::Usage { input: num(u, "input_tokens"), output: num(u, "output_tokens") });
                }
                if let Some(r) = v.get("stop_reason").and_then(Value::as_str) {
                    let why = anthropic_stop(r);
                    self.done(why, out);
                }
            }
        }
    }
}

fn anthropic_stop(r: &str) -> StopReason {
    match r {
        "end_turn" | "stop_sequence" => StopReason::EndTurn,
        "tool_use" => StopReason::ToolUse,
        "max_tokens" => StopReason::MaxTokens,
        other => StopReason::Other(other.to_string()),
    }
}

fn sat(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn num(v: &Value, key: &str) -> u32 {
    v.get(key).and_then(Value::as_u64).map_or(0, sat)
}

/// The assistant's answer collected from the stream.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Assembled {
    pub text: String,
    pub calls: Vec<ToolCall>,
    pub stop: Option<StopReason>,
    pub error: Option<String>,
}

impl Assembled {
    pub fn apply(&mut self, ev: &StreamEvent) {
        match ev {
            StreamEvent::TextDelta(t) => self.text.push_str(t),
            StreamEvent::ToolCallStart { index, id, name } => {
                if self.calls.len() <= *index {
                    self.calls.resize_with(index + 1, || ToolCall {
                        id: String::new(),
                        name: String::new(),
                        arguments: String::new(),
                    });
                }
                let c = &mut self.calls[*index];
                c.id.clone_from(id);
                c.name.clone_from(name);
            }
            StreamEvent::ToolCallArgs { index, fragment } => {
                if let Some(c) = self.calls.get_mut(*index) {
                    c.arguments.push_str(fragment);
                }
            }
            StreamEvent::Usage { .. } => {}
            StreamEvent::Done(why) => {
                // A call without arguments has an empty object, not empty text.
                for c in &mut self.calls {
                    if c.arguments.trim().is_empty() {
                        c.arguments = "{}".into();
                    }
                }
                self.stop = Some(why.clone());
            }
            StreamEvent::Error(m) => self.error = Some(m.clone()),
        }
    }
}

#[cfg(test)]
mod tests;
