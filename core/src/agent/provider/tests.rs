//! Provider mapping tests: request bodies, streamed answers, errors and model lists.

use super::*;
use crate::agent::{Image, Message, Part, ToolSpec};
use serde_json::json;

fn img() -> Image {
    Image { mime: "image/jpeg".into(), base64: "QUJD".into() }
}

fn call(id: &str, name: &str, args: &str) -> ToolCall {
    ToolCall { id: id.into(), name: name.into(), arguments: args.into() }
}

/// A conversation with everything: system, user, an assistant with two parallel calls, two
/// tool results (one with a screenshot) and a follow-up user message.
fn sample() -> ChatRequest {
    ChatRequest {
        model: "m-1".into(),
        system: "Be brief.".into(),
        messages: vec![
            Message::user("Hi"),
            Message::assistant("Looking.", vec![call("c1", "list_links", "{}"), call("c2", "screenshot", "")]),
            Message::tool("c1", "3 links", None),
            Message::tool("c2", "ok", Some(img())),
            Message::user("Thanks"),
        ],
        tools: vec![ToolSpec {
            name: "list_links".into(),
            description: "Lists links".into(),
            parameters: json!({"type": "object", "properties": {}}),
        }],
        max_tokens: 1000,
    }
}

fn body_of(r: &HttpRequest) -> Value {
    serde_json::from_str(r.body.as_deref().unwrap()).unwrap()
}

fn header<'a>(r: &'a HttpRequest, name: &str) -> Option<&'a str> {
    r.headers.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

#[test]
fn openai_request_body() {
    let r = chat_request(ProviderKind::OpenRouter, "https://openrouter.ai/api/v1", &sample());
    assert_eq!((r.method, r.url.as_str()), ("POST", "https://openrouter.ai/api/v1/chat/completions"));
    assert_eq!(header(&r, "X-Title"), Some("Krunk"));
    assert_eq!(header(&r, "Content-Type"), Some("application/json"));
    let b = body_of(&r);
    assert_eq!(b["model"], "m-1");
    assert_eq!(b["stream"], true);
    assert_eq!(b["stream_options"], json!({"include_usage": true}));
    assert_eq!(b["max_tokens"], 1000);
    assert_eq!(b["tool_choice"], "auto");
    assert_eq!(
        b["tools"],
        json!([{"type": "function", "function": {"name": "list_links", "description": "Lists links",
                 "parameters": {"type": "object", "properties": {}}}}])
    );
    let m = b["messages"].as_array().unwrap();
    assert_eq!(m.len(), 7);
    assert_eq!(m[0], json!({"role": "system", "content": "Be brief."}));
    assert_eq!(m[1], json!({"role": "user", "content": "Hi"}));
    assert_eq!(
        m[2],
        json!({"role": "assistant", "content": "Looking.", "tool_calls": [
            {"id": "c1", "type": "function", "function": {"name": "list_links", "arguments": "{}"}},
            {"id": "c2", "type": "function", "function": {"name": "screenshot", "arguments": "{}"}},
        ]})
    );
    assert_eq!(m[3], json!({"role": "tool", "tool_call_id": "c1", "content": "3 links"}));
    assert_eq!(m[4], json!({"role": "tool", "tool_call_id": "c2", "content": "ok"}));
    // The image follows the run of tool messages as a user message.
    assert_eq!(
        m[5],
        json!({"role": "user", "content": [
            {"type": "text", "text": "Screenshot from tool call c2"},
            {"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,QUJD"}},
        ]})
    );
    assert_eq!(m[6], json!({"role": "user", "content": "Thanks"}));
}

#[test]
fn openai_variants_per_kind() {
    let mut req = sample();
    let b = body_of(&chat_request(ProviderKind::OpenAi, "https://api.openai.com/v1", &req));
    assert_eq!(b["max_completion_tokens"], 1000);
    assert!(b.get("max_tokens").is_none());
    let r = chat_request(ProviderKind::Custom, "http://127.0.0.1:11434/v1", &req);
    assert_eq!(r.url, "http://127.0.0.1:11434/v1/chat/completions");
    assert_eq!(header(&r, "X-Title"), None);
    let b = body_of(&r);
    assert!(b.get("stream_options").is_none(), "custom servers may reject it");
    assert_eq!(b["max_tokens"], 1000);
    // No tools, no system, an assistant with only calls, a user image, a trailing image.
    req.tools.clear();
    req.system.clear();
    req.max_tokens = 0;
    req.messages = vec![
        Message { parts: vec![Part::Text("look".into()), Part::Image(img())], ..Message::user("") },
        Message::assistant("", vec![call("c9", "screenshot", "{}")]),
        Message::tool("c9", "shot", Some(img())),
    ];
    let b = body_of(&chat_request(ProviderKind::OpenAi, "https://api.openai.com/v1", &req));
    assert!(b.get("tools").is_none() && b.get("tool_choice").is_none() && b.get("max_completion_tokens").is_none());
    let m = b["messages"].as_array().unwrap();
    assert_eq!(m.len(), 4);
    assert_eq!(m[0]["content"][0], json!({"type": "text", "text": "look"}));
    assert_eq!(m[0]["content"][1]["image_url"]["url"], "data:image/jpeg;base64,QUJD");
    assert_eq!(m[1]["content"], Value::Null);
    assert_eq!(m[3]["role"], "user");
    assert_eq!(m[3]["content"][0]["text"], "Screenshot from tool call c9");
}

#[test]
fn anthropic_request_body() {
    let r = chat_request(ProviderKind::Anthropic, "https://api.anthropic.com/v1", &sample());
    assert_eq!(r.url, "https://api.anthropic.com/v1/messages");
    assert_eq!(header(&r, "anthropic-version"), Some("2023-06-01"));
    assert_eq!(header(&r, "content-type"), Some("application/json"));
    let b = body_of(&r);
    assert_eq!(b["system"], "Be brief.");
    assert_eq!(b["max_tokens"], 1000);
    assert_eq!(b["stream"], true);
    assert_eq!(
        b["tools"],
        json!([{"name": "list_links", "description": "Lists links",
                "input_schema": {"type": "object", "properties": {}}}])
    );
    let m = b["messages"].as_array().unwrap();
    assert_eq!(m.len(), 3, "user / assistant / user (tool results + text merged)");
    assert_eq!(m[0], json!({"role": "user", "content": [{"type": "text", "text": "Hi"}]}));
    assert_eq!(
        m[1],
        json!({"role": "assistant", "content": [
            {"type": "text", "text": "Looking."},
            {"type": "tool_use", "id": "c1", "name": "list_links", "input": {}},
            {"type": "tool_use", "id": "c2", "name": "screenshot", "input": {}},
        ]})
    );
    assert_eq!(
        m[2],
        json!({"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "c1", "content": [{"type": "text", "text": "3 links"}]},
            {"type": "tool_result", "tool_use_id": "c2", "content": [
                {"type": "text", "text": "ok"},
                {"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": "QUJD"}},
            ]},
            {"type": "text", "text": "Thanks"},
        ]})
    );
}

#[test]
fn anthropic_merges_and_orders_blocks() {
    let req = ChatRequest {
        model: "claude".into(),
        system: String::new(),
        messages: vec![
            Message::user("a"),
            Message::user("b"),
            Message::assistant("", vec![call("t1", "click", r#"{"x":1,"y":2}"#), call("t2", "x", "not json")]),
            Message::assistant("", vec![]),
            Message::user("note first"),
            Message::tool("t1", "", None),
            Message::tool("t2", "done", None),
        ],
        tools: vec![],
        max_tokens: 0,
    };
    let b = anthropic::body(&req);
    assert!(b.get("system").is_none() && b.get("tools").is_none());
    assert_eq!(b["max_tokens"], 32000, "0.15.0: the default cap, not 4096");
    let m = b["messages"].as_array().unwrap();
    assert_eq!(m.len(), 3);
    assert_eq!(m[0]["content"].as_array().unwrap().len(), 2);
    assert_eq!(m[1]["content"][0]["input"], json!({"x": 1, "y": 2}));
    assert_eq!(m[1]["content"][1]["input"], json!({}));
    let c = m[2]["content"].as_array().unwrap();
    assert_eq!(c[0], json!({"type": "tool_result", "tool_use_id": "t1"}));
    assert_eq!(c[1]["tool_use_id"], "t2");
    assert_eq!(c[2], json!({"type": "text", "text": "note first"}));
}

/// Feeds `stream` to a decoder in pieces of `step` bytes; returns the events and the assembly.
fn decode(kind: ProviderKind, stream: &str, step: usize) -> (Vec<StreamEvent>, Assembled) {
    let mut d = Decoder::new(kind);
    let mut evs = Vec::new();
    for piece in stream.as_bytes().chunks(step) {
        evs.extend(d.feed(piece));
    }
    evs.extend(d.finish());
    let mut a = Assembled::default();
    for e in &evs {
        a.apply(e);
    }
    (evs, a)
}

const OPENAI_STREAM: &str = concat!(
    ": OPENROUTER PROCESSING\n\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Hel\"}}]}\n\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"lö\"}}]}\r\n\r\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[",
    "{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"click\",\"arguments\":\"\"}},",
    "{\"index\":1,\"id\":\"call_b\",\"type\":\"function\",\"function\":{\"name\":\"scroll\",\"arguments\":\"{\\\"dy\\\"\"}}]}}]}\n\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"selector\\\":\"}}]}}]}\n\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":1,\"function\":{\"arguments\":\":200}\"}}]}}]}\n\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"#go\\\"}\"}}]}}]}\n\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
    "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":34}}\n\n",
    "data: [DONE]\n\n",
);

#[test]
fn openai_stream_with_parallel_tool_calls() {
    let (evs, a) = decode(ProviderKind::OpenRouter, OPENAI_STREAM, 7);
    assert_eq!(a.text, "Hellö");
    assert_eq!(
        a.calls,
        vec![call("call_a", "click", r##"{"selector":"#go"}"##), call("call_b", "scroll", r#"{"dy":200}"#)]
    );
    assert_eq!(a.stop, Some(StopReason::ToolUse));
    assert_eq!(a.error, None);
    assert!(evs.contains(&StreamEvent::Usage { input: 12, output: 34 }));
    assert_eq!(evs.iter().filter(|e| matches!(e, StreamEvent::Done(_))).count(), 1);
    assert!(evs.contains(&StreamEvent::ToolCallStart { index: 1, id: "call_b".into(), name: "scroll".into() }));
    // Every chunking gives the same answer.
    for step in [1, 2, 3, 64, 4096] {
        assert_eq!(decode(ProviderKind::OpenRouter, OPENAI_STREAM, step).1, a, "step {step}");
    }
}

#[test]
fn openai_stream_edge_cases() {
    // No finish_reason and no [DONE]: Done(EndTurn) at the end; text only.
    let (evs, a) = decode(ProviderKind::Custom, "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}", 5);
    assert_eq!(a.text, "hi");
    assert_eq!(evs.last(), Some(&StreamEvent::Done(StopReason::EndTurn)));
    // Calls without an index (some servers), with an empty argument string.
    let s = concat!(
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"id\":\"x1\",\"function\":{\"name\":\"list_tabs\",\"arguments\":\"\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"id\":\"x2\",\"function\":{\"name\":\"back\",\"arguments\":\"{}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\n",
    );
    let (_, a) = decode(ProviderKind::Custom, s, 9);
    assert_eq!(a.calls, vec![call("x1", "list_tabs", "{}"), call("x2", "back", "{}")]);
    assert_eq!(a.stop, Some(StopReason::MaxTokens));
    // Arguments before the name; no id at all.
    let s = concat!(
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"a\\\":\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"t\",\"arguments\":\"1}\"}}]}}]}\n\n",
    );
    let (_, a) = decode(ProviderKind::OpenAi, s, 100);
    assert_eq!(a.calls, vec![call("call_0", "t", r#"{"a":1}"#)]);
    assert_eq!(a.stop, Some(StopReason::ToolUse));
}

#[test]
fn errors_in_and_instead_of_the_stream() {
    // OpenRouter: an error chunk mid-stream.
    let s = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"par\"}}]}\n\n",
        "data: {\"error\":{\"code\":502,\"message\":\"Upstream\\nfailed\"},\"choices\":[{\"delta\":{},\"finish_reason\":\"error\"}]}\n\n",
    );
    let (evs, a) = decode(ProviderKind::OpenRouter, s, 11);
    assert_eq!(a.error.as_deref(), Some("Upstream failed"));
    assert!(!evs.iter().any(|e| matches!(e, StreamEvent::Done(_))), "an error ends it without Done");
    // Anthropic error event.
    let s = "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n";
    let (evs, _) = decode(ProviderKind::Anthropic, s, 4);
    assert_eq!(evs, vec![StreamEvent::Error("Overloaded".into())]);
    // A JSON error body instead of SSE.
    let (evs, _) = decode(ProviderKind::OpenAi, "  {\"error\":{\"message\":\"Invalid model\",\"type\":\"x\"}}", 6);
    assert_eq!(evs, vec![StreamEvent::Error("Invalid model".into())]);
    let (evs, _) = decode(ProviderKind::Custom, "{\"detail\":\"Not Found\"}", 100);
    assert_eq!(evs, vec![StreamEvent::Error("Not Found".into())]);
    let (evs, _) = decode(ProviderKind::Custom, "{broken", 100);
    assert!(matches!(&evs[..], [StreamEvent::Error(_)]));
}

#[test]
fn non_streamed_answers_are_understood() {
    // A server that ignored stream:true.
    let body = json!({"choices": [{"message": {"role": "assistant", "content": "Hi", "tool_calls": [
        {"id": "q", "type": "function", "function": {"name": "finish", "arguments": "{\"answer\":\"x\"}"}}]},
        "finish_reason": "tool_calls"}], "usage": {"prompt_tokens": 1, "completion_tokens": 2}})
    .to_string();
    let (evs, a) = decode(ProviderKind::Custom, &body, 13);
    assert_eq!(a.text, "Hi");
    assert_eq!(a.calls, vec![call("q", "finish", r#"{"answer":"x"}"#)]);
    assert_eq!(a.stop, Some(StopReason::ToolUse));
    assert!(evs.contains(&StreamEvent::Usage { input: 1, output: 2 }));
    let body = json!({"type": "message", "content": [{"type": "text", "text": "Yo"},
        {"type": "tool_use", "id": "tu", "name": "back", "input": {}}], "stop_reason": "tool_use",
        "usage": {"input_tokens": 3, "output_tokens": 4}})
    .to_string();
    let (_, a) = decode(ProviderKind::Anthropic, &body, 50);
    assert_eq!((a.text.as_str(), a.calls.clone()), ("Yo", vec![call("tu", "back", "{}")]));
    assert_eq!(a.stop, Some(StopReason::ToolUse));
}

const ANTHROPIC_STREAM: &str = concat!(
    "event: message_start\n",
    "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{\"input_tokens\":25,\"output_tokens\":1}}}\n\n",
    "event: content_block_start\n",
    "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
    "event: ping\ndata: {\"type\": \"ping\"}\n\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Let me \"}}\n\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"check ✓\"}}\n\n",
    "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
    "event: content_block_start\n",
    "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_01\",\"name\":\"navigate\",\"input\":{}}}\n\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\"}}\n\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"url\\\": \\\"https://a.\"}}\n\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"example/\\\"}\"}}\n\n",
    "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
    "event: content_block_start\n",
    "data: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_02\",\"name\":\"list_tabs\",\"input\":{}}}\n\n",
    "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":2}\n\n",
    "event: message_delta\n",
    "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":89}}\n\n",
    "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
);

#[test]
fn anthropic_stream_text_and_tool_use() {
    let (evs, a) = decode(ProviderKind::Anthropic, ANTHROPIC_STREAM, 5);
    assert_eq!(a.text, "Let me check ✓");
    assert_eq!(
        a.calls,
        vec![call("toolu_01", "navigate", r#"{"url": "https://a.example/"}"#), call("toolu_02", "list_tabs", "{}")]
    );
    assert_eq!(a.stop, Some(StopReason::ToolUse));
    assert!(evs.contains(&StreamEvent::Usage { input: 25, output: 89 }));
    assert_eq!(evs.iter().filter(|e| matches!(e, StreamEvent::Done(_))).count(), 1);
    for step in [1, 3, 1000] {
        assert_eq!(decode(ProviderKind::Anthropic, ANTHROPIC_STREAM, step).1, a);
    }
    // End of the stream before message_stop: the stop reason from message_delta is used.
    let cut = ANTHROPIC_STREAM.split("event: message_stop").next().unwrap();
    assert_eq!(decode(ProviderKind::Anthropic, cut, 64).1.stop, Some(StopReason::ToolUse));
}

#[test]
fn error_messages() {
    assert_eq!(
        error_message(401, r#"{"error":{"message":"Incorrect API key provided.","type":"invalid_request_error"}}"#),
        "HTTP 401: Incorrect API key provided. Check the API key."
    );
    assert_eq!(
        error_message(404, r#"{"type":"error","error":{"type":"not_found_error","message":"model: x"}}"#),
        "HTTP 404: model: x. Check the model id and the server address."
    );
    assert_eq!(
        error_message(429, r#"{"error":{"message":"Rate limit exceeded","code":429}}"#),
        "HTTP 429: Rate limit exceeded. Too many requests, or no credit left: wait a moment or check your account."
    );
    assert_eq!(error_message(403, ""), "HTTP 403 from the provider. Check the API key.");
    assert_eq!(error_message(502, "<html><body>Bad gateway</body></html>"), "HTTP 502 from the provider");
    assert_eq!(error_message(500, "upstream\tdown\r\n"), "HTTP 500: upstream down");
    assert_eq!(error_message(400, "{\"error\":\"bad\\u0007 \u{202e}evil\"}"), "HTTP 400: bad evil");
    let long = error_message(400, &json!({"error": {"message": "y".repeat(1000)}}).to_string());
    assert_eq!(long.chars().count(), "HTTP 400: ".len() + MAX_MESSAGE_CHARS + 1);
    assert!(long.ends_with('…'));
}

#[test]
fn model_lists() {
    let or = json!({"data": [
        {"id": "openai/gpt-4o", "name": "OpenAI: GPT-4o", "context_length": 128000,
         "top_provider": {"context_length": 128000, "max_completion_tokens": 16384, "is_moderated": true},
         "architecture": {"input_modalities": ["text", "image"], "modality": "text+image->text"},
         "supported_parameters": ["tools", "temperature"]},
        {"id": "meta/llama", "name": "Meta: Llama", "context_length": 8192,
         "architecture": {"input_modalities": ["text"]}, "supported_parameters": ["temperature"]},
        {"id": "old/one", "name": "Ancient", "architecture": {"modality": "text->image"}},
        {"id": "openai/gpt-4o", "name": "dup"},
        {"id": ""},
    ]});
    let m = parse_models(ProviderKind::OpenRouter, &or.to_string()).unwrap();
    assert_eq!(m.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["old/one", "meta/llama", "openai/gpt-4o"]);
    assert_eq!(
        m[2],
        ModelInfo {
            id: "openai/gpt-4o".into(),
            name: "OpenAI: GPT-4o".into(),
            vision: Some(true),
            tools: Some(true),
            context: Some(128000),
            max_output: Some(16384),
        }
    );
    assert_eq!((m[1].vision, m[1].tools), (Some(false), Some(false)));
    assert_eq!((m[0].vision, m[0].tools, m[0].context, m[0].max_output), (Some(false), None, None, None));

    let oa = r#"{"object":"list","data":[{"id":"gpt-4o","object":"model"},{"id":"dall-e-3","object":"model"}]}"#;
    let m = parse_models(ProviderKind::OpenAi, oa).unwrap();
    assert_eq!(m.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["dall-e-3", "gpt-4o"]);
    assert_eq!((m[1].vision, m[1].tools), (None, None));

    let an = r#"{"data":[{"type":"model","id":"claude-z","display_name":"Claude Z","max_input_tokens":200000,"max_tokens":64000},
                 {"type":"model","id":"claude-a","display_name":"Claude A"}],"has_more":false}"#;
    let m = parse_models(ProviderKind::Anthropic, an).unwrap();
    assert_eq!(
        m.iter().map(|x| (x.id.as_str(), x.name.as_str())).collect::<Vec<_>>(),
        [("claude-a", "Claude A"), ("claude-z", "Claude Z")]
    );
    // 0.15.0: Anthropic's list gives the context window and the output limit.
    assert_eq!((m[1].context, m[1].max_output), (Some(200000), Some(64000)));
    assert_eq!((m[0].context, m[0].max_output), (None, None));

    let ollama = r#"{"models":[{"name":"qwen2.5:7b","model":"qwen2.5:7b"},{"name":"llama3:latest"}]}"#;
    let m = parse_models(ProviderKind::Custom, ollama).unwrap();
    assert_eq!(m.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["llama3:latest", "qwen2.5:7b"]);
    let lms = r#"{"data":[{"id":"local-model","max_model_len":32768}]}"#;
    assert_eq!(parse_models(ProviderKind::Custom, lms).unwrap()[0].context, Some(32768));
    // Only Anthropic's `max_tokens` is an output limit; servers that name it plainly count too.
    let other = r#"{"data":[{"id":"a","max_tokens":5},{"id":"b","max_output_tokens":8192},{"id":"c","max_completion_tokens":0}]}"#;
    let m = parse_models(ProviderKind::Custom, other).unwrap();
    assert_eq!(m.iter().map(|x| x.max_output).collect::<Vec<_>>(), [None, Some(8192), None]);

    assert_eq!(parse_models(ProviderKind::OpenAi, r#"{"error":{"message":"bad key"}}"#).unwrap_err(), "bad key");
    assert!(parse_models(ProviderKind::OpenAi, "nope").is_err());
    assert!(parse_models(ProviderKind::OpenAi, r#"{"models":[]}"#).is_err(), "only custom servers use that shape");
    assert_eq!(parse_models(ProviderKind::OpenAi, r#"{"data":[]}"#).unwrap(), vec![]);
}

#[test]
fn base_url_rules() {
    use ProviderKind::*;
    for (kind, base, ok) in [
        (OpenRouter, "", true),
        (OpenRouter, "https://openrouter.ai/api/v1", true),
        (OpenAi, "https://api.openai.com/v1", true),
        (OpenAi, "https://evil.example/v1", false),
        (Anthropic, "https://api.openai.com/v1", false),
        (Custom, "", false),
        (Custom, "https://llm.example.com/v1", true),
        (Custom, "https://llm.example.com:8443/v1/", true),
        (Custom, "http://localhost:11434/v1", true),
        (Custom, "http://LOCALHOST:1234", true),
        (Custom, "http://127.0.0.1:1234/v1", true),
        (Custom, "http://127.8.9.10/v1", true),
        (Custom, "http://[::1]:8080/v1", true),
        (Custom, "http://10.0.0.5/v1", true),
        (Custom, "http://172.16.0.1/v1", true),
        (Custom, "http://172.31.255.255/v1", true),
        (Custom, "http://172.32.0.1/v1", false),
        (Custom, "http://192.168.1.20:8000/v1", true),
        (Custom, "http://192.169.1.20/v1", false),
        (Custom, "http://[fd12:3456::1]/v1", true),
        (Custom, "http://[fe80::1]/v1", false),
        (Custom, "http://gpu-box.local:8000/v1", true),
        (Custom, "http://.local/v1", false),
        (Custom, "http://example.com/v1", false),
        (Custom, "http://localhost.example.com/v1", false),
        (Custom, "http://8.8.8.8/v1", false),
        (Custom, "ftp://127.0.0.1/v1", false),
        (Custom, "file:///etc/passwd", false),
        (Custom, "javascript:alert(1)", false),
        (Custom, "https://user:pw@llm.example.com/v1", false),
        (Custom, "https://key@llm.example.com/v1", false),
        (Custom, "https://llm.example.com/v1?key=1", false),
        (Custom, "https://llm.example.com/v1#x", false),
        (Custom, "https://llm.exa mple.com/v1", false),
        (Custom, "not a url", false),
    ] {
        assert_eq!(check_base_url(kind, base).is_ok(), ok, "{kind:?} {base}");
    }
    assert_eq!(base_url(Custom, " http://localhost:11434/v1/ "), "http://localhost:11434/v1");
    assert_eq!(base_url(OpenAi, "ignored"), "https://api.openai.com/v1");
}

#[test]
fn auth_and_model_requests() {
    assert_eq!(auth_headers(ProviderKind::OpenRouter, "sk-or"), vec![("Authorization".into(), "Bearer sk-or".into())]);
    assert_eq!(auth_headers(ProviderKind::Custom, "k"), vec![("Authorization".into(), "Bearer k".into())]);
    assert_eq!(auth_headers(ProviderKind::Anthropic, "sk-ant"), vec![("x-api-key".into(), "sk-ant".into())]);
    assert!(auth_headers(ProviderKind::Custom, "").is_empty());
    let r = models_request(ProviderKind::Anthropic, "https://api.anthropic.com/v1");
    assert_eq!((r.method, r.url.as_str()), ("GET", "https://api.anthropic.com/v1/models?limit=1000"));
    assert_eq!(header(&r, "anthropic-version"), Some(anthropic::VERSION));
    let r = models_request(ProviderKind::OpenAi, "https://api.openai.com/v1");
    assert_eq!((r.url.as_str(), r.headers.len(), r.body.is_none()), ("https://api.openai.com/v1/models", 0, true));
    // The key is never part of a built request.
    let c = chat_request(ProviderKind::OpenAi, "https://api.openai.com/v1", &sample());
    assert!(header(&c, "Authorization").is_none());
}

#[test]
fn assembled_pads_and_keeps_order() {
    let mut a = Assembled::default();
    a.apply(&StreamEvent::ToolCallStart { index: 1, id: "b".into(), name: "y".into() });
    a.apply(&StreamEvent::ToolCallStart { index: 0, id: "a".into(), name: "x".into() });
    a.apply(&StreamEvent::ToolCallArgs { index: 0, fragment: "{\"k\":".into() });
    a.apply(&StreamEvent::ToolCallArgs { index: 0, fragment: "1}".into() });
    a.apply(&StreamEvent::ToolCallArgs { index: 7, fragment: "lost".into() });
    a.apply(&StreamEvent::Usage { input: 1, output: 1 });
    a.apply(&StreamEvent::Done(StopReason::Other("refusal".into())));
    assert_eq!(a.calls, vec![call("a", "x", r#"{"k":1}"#), call("b", "y", "{}")]);
    assert_eq!(a.stop, Some(StopReason::Other("refusal".into())));
}
