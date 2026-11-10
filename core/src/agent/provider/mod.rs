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

/// A request for the host's HTTP client. Never carries the API key (see [`auth_headers`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
}

/// Is `base` usable for `kind`? Named providers must use their fixed URL; a custom server must
/// be `https://`, or `http://` on a loopback or private-network host (Ollama, LM Studio).
pub fn check_base_url(kind: ProviderKind, base: &str) -> Result<(), String> {
    let _ = (kind, base);
    Ok(()) // WP-A
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
    HttpRequest { method: "GET", url: format!("{base}/models"), headers, body: None }
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
    let _ = body;
    format!("The provider answered with HTTP {status}") // WP-A
}

/// Turns response bytes into [`StreamEvent`]s for one provider style.
pub struct Decoder {
    kind: ProviderKind,
    sse: super::sse::SseParser,
}

impl Decoder {
    pub fn new(kind: ProviderKind) -> Decoder {
        Decoder { kind, sse: super::sse::SseParser::new() }
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Vec<StreamEvent> {
        let _ = (self.kind, self.sse.feed(chunk));
        Vec::new() // WP-A
    }

    /// The response ended (also emits `Done` if the provider never said why it stopped).
    pub fn finish(&mut self) -> Vec<StreamEvent> {
        Vec::new() // WP-A
    }
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
        let _ = ev; // WP-A
    }
}
