//! Model lists of the providers (WP-A).
//!
//! * OpenRouter: `data[]` with `id`, `name`, `context_length`, `architecture.input_modalities`
//!   (images → vision) and `supported_parameters` (`tools`).
//! * OpenAI and custom servers: `data[].id`; a custom server may also answer in Ollama's
//!   `{"models": [{"name": …}]}` shape.
//! * Anthropic: `data[]` with `id`, `display_name`, `max_input_tokens` and `max_tokens` (the
//!   output limit).
//!
//! 0.15.0: the output limit ([`ModelInfo::max_output`]) is OpenRouter's
//! `top_provider.max_completion_tokens`, Anthropic's `max_tokens`, or a plainly named
//! `max_output_tokens` / `max_completion_tokens` of any server.
//!
//! The result is sorted by name, without duplicate ids.

use crate::agent::{ModelInfo, ProviderKind};
use serde_json::Value;

/// The longest model id kept (the settings allow 200 characters).
const MAX_ID: usize = 200;

pub fn parse(kind: ProviderKind, body: &str) -> Result<Vec<ModelInfo>, String> {
    let v: Value = serde_json::from_str(body).map_err(|_| "The provider's model list could not be read".to_string())?;
    if v.get("error").is_some_and(|e| !e.is_null()) {
        return Err(super::provider_text(&v).unwrap_or_else(|| "The provider reported an error".into()));
    }
    let list = v
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| (kind == ProviderKind::Custom).then(|| v.get("models").and_then(Value::as_array)).flatten())
        .or_else(|| v.as_array())
        .ok_or_else(|| "The provider sent no model list".to_string())?;
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<ModelInfo> =
        list.iter().filter_map(|m| model(kind, m)).filter(|m| seen.insert(m.id.clone())).collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
    Ok(out)
}

fn model(kind: ProviderKind, m: &Value) -> Option<ModelInfo> {
    let s = |key: &str| m.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let id = s("id").or_else(|| s("name")).or_else(|| s("model"))?;
    if id.chars().count() > MAX_ID || id.chars().any(char::is_control) {
        return None;
    }
    let name = match kind {
        ProviderKind::OpenRouter => s("name"),
        ProviderKind::Anthropic => s("display_name"),
        _ => None,
    }
    .map(|n| n.chars().filter(|c| !c.is_control()).take(MAX_ID).collect::<String>())
    .unwrap_or_else(|| id.to_string());
    let num = |n: u64| u32::try_from(n).unwrap_or(u32::MAX);
    let context = ["context_length", "context_window", "max_model_len", "max_input_tokens"]
        .iter()
        .find_map(|k| m.get(*k).and_then(Value::as_u64))
        .map(num);
    let mut outputs = vec!["/max_output_tokens", "/max_completion_tokens"];
    match kind {
        ProviderKind::OpenRouter => outputs.insert(0, "/top_provider/max_completion_tokens"),
        ProviderKind::Anthropic => outputs.insert(0, "/max_tokens"),
        _ => {}
    }
    let max_output = outputs.iter().find_map(|p| m.pointer(p).and_then(Value::as_u64)).filter(|&n| n > 0).map(num);
    let (vision, tools) = if kind == ProviderKind::OpenRouter {
        let vision = match m.pointer("/architecture/input_modalities").and_then(Value::as_array) {
            Some(a) => Some(a.iter().any(|x| x.as_str() == Some("image"))),
            // Older shape: "text+image->text".
            None => m
                .pointer("/architecture/modality")
                .and_then(Value::as_str)
                .map(|x| x.split("->").next().unwrap_or("").contains("image")),
        };
        let tools = m
            .get("supported_parameters")
            .and_then(Value::as_array)
            .map(|a| a.iter().any(|x| x.as_str() == Some("tools")));
        (vision, tools)
    } else {
        (None, None)
    };
    Some(ModelInfo { id: id.to_string(), name, vision, tools, context, max_output })
}
