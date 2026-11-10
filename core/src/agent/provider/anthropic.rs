//! Anthropic Messages API (WP-A).

use super::HttpRequest;
use crate::agent::ChatRequest;

/// The `anthropic-version` header value.
pub const VERSION: &str = "2023-06-01";

pub fn chat_request(base: &str, req: &ChatRequest) -> HttpRequest {
    let _ = req;
    HttpRequest {
        method: "POST",
        url: format!("{base}/messages"),
        headers: vec![("Content-Type".into(), "application/json".into()), ("anthropic-version".into(), VERSION.into())],
        body: Some(String::new()),
    }
}
