//! Anthropic Messages API (WP-A).
//!
//! The system prompt is a top-level field; the messages alternate user/assistant, so
//! consecutive messages of one role are merged. Tool results are `tool_result` blocks in a user
//! message (first in it, as the API asks), and they can carry images.

use super::HttpRequest;
use crate::agent::{ChatRequest, Image, Part, Role};
use serde_json::{json, Map, Value};

/// The `anthropic-version` header value.
pub const VERSION: &str = "2023-06-01";

/// `max_tokens` is required here; used when the request leaves it at 0.
const DEFAULT_MAX_TOKENS: u32 = 4096;

pub fn chat_request(base: &str, req: &ChatRequest) -> HttpRequest {
    HttpRequest {
        method: "POST",
        url: format!("{base}/messages"),
        headers: vec![("Content-Type".into(), "application/json".into()), ("anthropic-version".into(), VERSION.into())],
        body: Some(body(req).to_string()),
    }
}

/// The JSON body of a streamed Messages call.
pub fn body(req: &ChatRequest) -> Value {
    let mut b = Map::new();
    b.insert("model".into(), req.model.clone().into());
    let max = if req.max_tokens == 0 { DEFAULT_MAX_TOKENS } else { req.max_tokens };
    b.insert("max_tokens".into(), max.into());
    if !req.system.is_empty() {
        b.insert("system".into(), req.system.clone().into());
    }
    b.insert("messages".into(), Value::Array(messages(req)));
    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.parameters}))
            .collect();
        b.insert("tools".into(), Value::Array(tools));
    }
    b.insert("stream".into(), true.into());
    Value::Object(b)
}

fn image(i: &Image) -> Value {
    json!({"type": "image", "source": {"type": "base64", "media_type": i.mime, "data": i.base64}})
}

/// Text and image blocks of `parts` (empty text is not allowed by the API).
fn blocks(parts: &[Part]) -> Vec<Value> {
    parts
        .iter()
        .filter_map(|p| match p {
            Part::Text(t) if t.is_empty() => None,
            Part::Text(t) => Some(json!({"type": "text", "text": t})),
            Part::Image(i) => Some(image(i)),
        })
        .collect()
}

fn messages(req: &ChatRequest) -> Vec<Value> {
    let mut turns: Vec<(&'static str, Vec<Value>)> = Vec::new();
    for m in &req.messages {
        let (role, content) = match m.role {
            Role::User => ("user", blocks(&m.parts)),
            Role::Assistant => {
                let mut c = blocks(&m.parts);
                for call in &m.tool_calls {
                    // `input` must be an object; arguments that aren't one are sent as `{}`.
                    let input = serde_json::from_str::<Value>(&call.arguments)
                        .ok()
                        .filter(Value::is_object)
                        .unwrap_or_else(|| json!({}));
                    c.push(json!({"type": "tool_use", "id": call.id, "name": call.name, "input": input}));
                }
                ("assistant", c)
            }
            Role::Tool => {
                let mut r = Map::new();
                r.insert("type".into(), "tool_result".into());
                r.insert("tool_use_id".into(), m.tool_call_id.clone().unwrap_or_default().into());
                let inner = blocks(&m.parts);
                if !inner.is_empty() {
                    r.insert("content".into(), Value::Array(inner));
                }
                ("user", vec![Value::Object(r)])
            }
        };
        if content.is_empty() {
            continue;
        }
        match turns.last_mut() {
            Some((r, c)) if *r == role => c.extend(content),
            _ => turns.push((role, content)),
        }
    }
    turns
        .into_iter()
        .map(|(role, mut content)| {
            if role == "user" {
                // tool_result blocks first, the rest in their order.
                let is_result = |b: &Value| b.get("type").and_then(Value::as_str) == Some("tool_result");
                let (mut results, rest): (Vec<Value>, Vec<Value>) = content.into_iter().partition(is_result);
                results.extend(rest);
                content = results;
            }
            json!({"role": role, "content": content})
        })
        .collect()
}
