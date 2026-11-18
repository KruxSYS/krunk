//! OpenAI-style chat completions (OpenRouter, OpenAI, custom servers) (WP-A).
//!
//! Tool messages can't carry images in this API, so a tool result with a screenshot becomes the
//! tool message's text plus a user message with the image. That user message goes after the
//! whole run of tool messages: the API wants every answer to an assistant's parallel tool
//! calls right after it.

use super::HttpRequest;
use crate::agent::{ChatRequest, Document, Image, Message, Part, ProviderKind, Role};
use serde_json::{json, Map, Value};

pub fn chat_request(kind: ProviderKind, base: &str, req: &ChatRequest) -> HttpRequest {
    let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];
    if kind == ProviderKind::OpenRouter {
        // OpenRouter's app attribution (no referrer is sent).
        headers.push(("X-Title".into(), "Krunk".into()));
    }
    HttpRequest {
        method: "POST",
        url: format!("{base}/chat/completions"),
        headers,
        body: Some(body(kind, req).to_string()),
    }
}

/// The JSON body of a streamed chat call.
pub fn body(kind: ProviderKind, req: &ChatRequest) -> Value {
    let mut b = Map::new();
    b.insert("model".into(), req.model.clone().into());
    b.insert("messages".into(), Value::Array(messages(req)));
    if !req.tools.is_empty() {
        let tools: Vec<Value> = req
            .tools
            .iter()
            .map(|t| {
                json!({"type": "function", "function": {
                    "name": t.name, "description": t.description, "parameters": t.parameters,
                }})
            })
            .collect();
        b.insert("tools".into(), Value::Array(tools));
        b.insert("tool_choice".into(), "auto".into());
    }
    b.insert("stream".into(), true.into());
    // Some OpenAI-compatible servers reject the field they don't know.
    if kind != ProviderKind::Custom {
        b.insert("stream_options".into(), json!({"include_usage": true}));
    }
    if req.max_tokens > 0 {
        // OpenAI's newer models refuse `max_tokens`; the others don't know the new name.
        let name = if kind == ProviderKind::OpenAi { "max_completion_tokens" } else { "max_tokens" };
        b.insert(name.into(), req.max_tokens.into());
    }
    Value::Object(b)
}

fn data_url(i: &Image) -> String {
    format!("data:{};base64,{}", i.mime, i.base64)
}

/// An attached PDF as a `file` content part (OpenAI's chat completions and OpenRouter).
fn file_part(d: &Document) -> Value {
    json!({"type": "file", "file": {"filename": d.name, "file_data": format!("data:{};base64,{}", d.mime, d.base64)}})
}

fn messages(req: &ChatRequest) -> Vec<Value> {
    let mut out = Vec::new();
    if !req.system.is_empty() {
        out.push(json!({"role": "system", "content": req.system}));
    }
    // Images of the current run of tool messages, sent as one user message after it.
    let mut images: Vec<Value> = Vec::new();
    for m in &req.messages {
        if m.role != Role::Tool && !images.is_empty() {
            out.push(json!({"role": "user", "content": std::mem::take(&mut images)}));
        }
        match m.role {
            Role::User => out.push(json!({"role": "user", "content": user_content(m)})),
            Role::Assistant => out.push(assistant(m)),
            Role::Tool => {
                let id = m.tool_call_id.clone().unwrap_or_default();
                out.push(json!({"role": "tool", "tool_call_id": id, "content": m.text()}));
                for p in &m.parts {
                    if let Part::Image(i) = p {
                        images.push(json!({"type": "text", "text": format!("Screenshot from tool call {id}")}));
                        images.push(json!({"type": "image_url", "image_url": {"url": data_url(i)}}));
                    }
                }
            }
        }
    }
    if !images.is_empty() {
        out.push(json!({"role": "user", "content": images}));
    }
    out
}

/// Plain text, or content parts when the message has images or documents.
fn user_content(m: &Message) -> Value {
    if m.parts.iter().all(|p| matches!(p, Part::Text(_))) {
        return m.text().into();
    }
    let parts: Vec<Value> = m
        .parts
        .iter()
        .map(|p| match p {
            Part::Text(t) => json!({"type": "text", "text": t}),
            Part::Image(i) => json!({"type": "image_url", "image_url": {"url": data_url(i)}}),
            Part::Document(d) => file_part(d),
        })
        .collect();
    Value::Array(parts)
}

fn assistant(m: &Message) -> Value {
    let text = m.text();
    let mut o = Map::new();
    o.insert("role".into(), "assistant".into());
    if m.tool_calls.is_empty() {
        o.insert("content".into(), text.into());
        return Value::Object(o);
    }
    o.insert("content".into(), if text.is_empty() { Value::Null } else { text.into() });
    let calls: Vec<Value> = m
        .tool_calls
        .iter()
        .map(|c| {
            let args = if c.arguments.trim().is_empty() { "{}" } else { c.arguments.as_str() };
            json!({"id": c.id, "type": "function", "function": {"name": c.name, "arguments": args}})
        })
        .collect();
    o.insert("tool_calls".into(), Value::Array(calls));
    Value::Object(o)
}
