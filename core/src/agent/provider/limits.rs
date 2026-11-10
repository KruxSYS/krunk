//! Output caps (0.15.0): the `max_tokens` a request carries, the known output limits of
//! well-known model families, and the higher cap of a retry.
//!
//! The user's setting is `AiProvider::max_output_tokens` (0 = auto). A model's known maximum
//! comes from its provider's model list ([`ModelInfo::max_output`]) or, failing that, from the
//! small table below. The table is conservative: a family is only listed when its limit is
//! certain, and the lowest limit of a family's versions is used.

use crate::agent::{ApiStyle, ModelInfo, ProviderKind};
use crate::settings::{DEFAULT_MAX_OUTPUT_TOKENS, MAX_OUTPUT_TOKENS_LIMIT};

/// (id prefix, max output tokens); the first match wins, so longer prefixes come first. Claude
/// ids are matched with dots made dashes (`claude-3.5-sonnet` = `claude-3-5-sonnet`).
static FAMILIES: &[(&str, u32)] = &[
    // Anthropic
    ("claude-instant", 4_096),
    ("claude-2", 4_096),
    ("claude-3-haiku", 4_096),
    ("claude-3-sonnet", 4_096),
    ("claude-3-opus", 4_096),
    ("claude-3-5-", 8_192),
    ("claude-3-7-sonnet", 64_000),
    ("claude-opus-4-5", 64_000),
    ("claude-sonnet-4-5", 64_000),
    ("claude-haiku-4-5", 64_000),
    ("claude-opus-4-6", 128_000),
    ("claude-opus-4-7", 128_000),
    ("claude-opus-4-8", 128_000),
    ("claude-sonnet-4-6", 128_000),
    ("claude-opus-4", 32_000),
    ("claude-sonnet-4", 64_000),
    ("claude-opus-5", 128_000),
    ("claude-sonnet-5", 128_000),
    ("claude-haiku-5", 128_000),
    ("claude-fable-5", 128_000),
    ("claude-mythos-5", 128_000),
    // OpenAI
    ("gpt-3.5", 4_096),
    ("gpt-4o-2024-05-13", 4_096),
    ("gpt-4o", 16_384),
    ("chatgpt-4o", 16_384),
    ("gpt-4.1", 32_768),
    ("gpt-4.5", 16_384),
    ("gpt-4", 4_096),
    ("gpt-5", 128_000),
    ("o1-mini", 65_536),
    ("o1-preview", 32_768),
    ("o1", 100_000),
    ("o3", 100_000),
    ("o4-mini", 100_000),
    // DeepSeek's own API
    ("deepseek-chat", 8_192),
];

/// The table's limit for `model` (an id as a provider lists it: `claude-sonnet-4-5`,
/// `anthropic/claude-3.5-sonnet`, `openai/gpt-4o-mini`, `anthropic.claude-3-haiku-…`).
pub fn builtin_max_output(model: &str) -> Option<u32> {
    let id = model.trim().to_ascii_lowercase();
    let id = id.rsplit('/').next().unwrap_or("");
    let id = match id.find("claude-") {
        Some(at) => id[at..].replace('.', "-"),
        None => id.to_string(),
    };
    FAMILIES.iter().find(|(prefix, _)| id.starts_with(prefix)).map(|&(_, n)| n)
}

/// The model's known output limit: its model list's, else the table's.
pub fn known_max_output(model: &str, info: Option<&ModelInfo>) -> Option<u32> {
    info.and_then(|m| m.max_output).filter(|&n| n > 0).or_else(|| builtin_max_output(model))
}

/// The `max_tokens` of a request (0 = the field is left out). `setting` is the provider's
/// Max output tokens (0 = auto): OpenAI-style providers then omit the field, Anthropic (where
/// it is required) gets the known maximum or [`DEFAULT_MAX_OUTPUT_TOKENS`]. A set value is
/// capped by the known maximum.
pub fn request_max_tokens(kind: ProviderKind, setting: u32, known: Option<u32>) -> u32 {
    let wanted = match (kind.style(), setting) {
        (ApiStyle::OpenAi, 0) => return 0,
        (ApiStyle::Anthropic, 0) => known.unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS),
        (_, n) => n,
    };
    known.map_or(wanted, |k| wanted.min(k)).min(MAX_OUTPUT_TOKENS_LIMIT)
}

/// The cap of one retry after the model stopped at `used` (0 = the provider's default): twice
/// as much, at most the known maximum (or [`MAX_OUTPUT_TOKENS_LIMIT`]). `None` when it can't
/// go higher.
pub fn raised_max_tokens(used: u32, known: Option<u32>) -> Option<u32> {
    let ceiling = known.unwrap_or(MAX_OUTPUT_TOKENS_LIMIT).min(MAX_OUTPUT_TOKENS_LIMIT);
    if used == 0 {
        return Some(known.unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS).min(ceiling));
    }
    let next = used.saturating_mul(2).min(ceiling);
    (next > used).then_some(next)
}

/// `32,000` for 32000.
pub fn thousands(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table() {
        assert_eq!(builtin_max_output("claude-3-haiku-20240307"), Some(4_096));
        assert_eq!(builtin_max_output("anthropic/claude-3.5-sonnet"), Some(8_192));
        assert_eq!(builtin_max_output("anthropic.claude-3-5-haiku-20241022-v1:0"), Some(8_192));
        assert_eq!(builtin_max_output("claude-3-7-sonnet-latest"), Some(64_000));
        assert_eq!(builtin_max_output("claude-opus-4-20250514"), Some(32_000));
        assert_eq!(builtin_max_output("claude-opus-4-1"), Some(32_000));
        assert_eq!(builtin_max_output("anthropic/claude-opus-4.5"), Some(64_000));
        assert_eq!(builtin_max_output("claude-sonnet-4-5-20250929"), Some(64_000));
        assert_eq!(builtin_max_output("claude-sonnet-4-20250514"), Some(64_000));
        assert_eq!(builtin_max_output("claude-opus-4-6"), Some(128_000));
        assert_eq!(builtin_max_output("claude-opus-5-5"), Some(128_000));
        assert_eq!(builtin_max_output("openai/gpt-4o-mini"), Some(16_384));
        assert_eq!(builtin_max_output("gpt-4o-2024-05-13"), Some(4_096));
        assert_eq!(builtin_max_output("gpt-4.1-nano"), Some(32_768));
        assert_eq!(builtin_max_output("gpt-4-1106-preview"), Some(4_096));
        assert_eq!(builtin_max_output("gpt-5-mini"), Some(128_000));
        assert_eq!(builtin_max_output("o3-mini"), Some(100_000));
        assert_eq!(builtin_max_output("o1-mini"), Some(65_536));
        for unknown in ["llama3.1:8b", "mock-agent", "google/gemini-2.5-pro", "", "qwen/qwen3-coder"] {
            assert_eq!(builtin_max_output(unknown), None, "{unknown}");
        }
    }

    #[test]
    fn model_list_wins() {
        let info = ModelInfo {
            id: "gpt-4o".into(),
            name: "x".into(),
            vision: None,
            tools: None,
            context: None,
            max_output: Some(9_000),
        };
        assert_eq!(known_max_output("gpt-4o", Some(&info)), Some(9_000));
        let zero = ModelInfo { max_output: Some(0), ..info.clone() };
        assert_eq!(known_max_output("gpt-4o", Some(&zero)), Some(16_384));
        assert_eq!(known_max_output("mock-agent", None), None);
    }

    #[test]
    fn cap_per_provider() {
        // Auto: OpenAI-compatible providers leave the field out; Anthropic needs one.
        for k in [ProviderKind::OpenRouter, ProviderKind::OpenAi, ProviderKind::Custom] {
            assert_eq!(request_max_tokens(k, 0, None), 0);
            assert_eq!(request_max_tokens(k, 0, Some(8_192)), 0);
            assert_eq!(request_max_tokens(k, DEFAULT_MAX_OUTPUT_TOKENS, None), 32_000);
            assert_eq!(request_max_tokens(k, DEFAULT_MAX_OUTPUT_TOKENS, Some(16_384)), 16_384);
            assert_eq!(request_max_tokens(k, 100_000, Some(128_000)), 100_000);
        }
        let a = ProviderKind::Anthropic;
        assert_eq!(request_max_tokens(a, 0, None), 32_000);
        assert_eq!(request_max_tokens(a, 0, Some(64_000)), 64_000);
        assert_eq!(request_max_tokens(a, 0, Some(4_096)), 4_096);
        assert_eq!(request_max_tokens(a, 32_000, Some(8_192)), 8_192);
        assert_eq!(request_max_tokens(a, 2_000_000, None), MAX_OUTPUT_TOKENS_LIMIT);
    }

    #[test]
    fn raise() {
        assert_eq!(raised_max_tokens(32_000, None), Some(64_000));
        assert_eq!(raised_max_tokens(32_000, Some(64_000)), Some(64_000));
        assert_eq!(raised_max_tokens(32_000, Some(40_000)), Some(40_000));
        assert_eq!(raised_max_tokens(64_000, Some(64_000)), None);
        assert_eq!(raised_max_tokens(16_384, Some(16_384)), None);
        assert_eq!(raised_max_tokens(800_000, None), Some(1_000_000));
        assert_eq!(raised_max_tokens(1_000_000, None), None);
        assert_eq!(raised_max_tokens(0, None), Some(32_000));
        assert_eq!(raised_max_tokens(0, Some(16_384)), Some(16_384));
    }

    #[test]
    fn number_format() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(4096), "4,096");
        assert_eq!(thousands(32_000), "32,000");
        assert_eq!(thousands(1_000_000), "1,000,000");
    }
}
