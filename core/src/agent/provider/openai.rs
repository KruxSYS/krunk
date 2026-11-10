//! OpenAI-style chat completions (OpenRouter, OpenAI, custom servers) (WP-A).

use super::HttpRequest;
use crate::agent::{ChatRequest, ProviderKind};

pub fn chat_request(kind: ProviderKind, base: &str, req: &ChatRequest) -> HttpRequest {
    let _ = (kind, req);
    HttpRequest {
        method: "POST",
        url: format!("{base}/chat/completions"),
        headers: vec![("Content-Type".into(), "application/json".into())],
        body: Some(String::new()),
    }
}
