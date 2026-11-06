//! The provider catalog, read from each provider's docs on 2026-11-06 (the design spec's
//! phase-start findings). Base URLs are prefilled and editable by the user.

use serde_json::{Value, json};

use crate::config::{Protocol, TokenParam};

/// The best structured output mode a preset supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuredOutput {
    /// A JSON schema.
    JsonSchema,
    /// Any JSON object.
    JsonObject,
    /// Free text only.
    Text,
}

/// One entry of the catalog.
#[allow(clippy::struct_excessive_bools)] // the catalog's flags are independent facts
#[derive(Debug, Clone, Copy)]
pub struct Preset {
    /// Stable key, stored as `ai_providers.provider_key`.
    pub key: &'static str,
    /// Display name.
    pub name: &'static str,
    /// The wire protocol.
    pub protocol: Protocol,
    /// Prefilled base URL.
    pub base_url: &'static str,
    /// A server on the user's own machine.
    pub local: bool,
    /// Where the user gets a key.
    pub guide_url: &'static str,
    /// Whether models take images when the config says nothing.
    pub images_by_default: bool,
    /// Best structured output mode.
    pub structured_output: StructuredOutput,
    /// Whether the service has a model listing.
    pub lists_models: bool,
    /// A consumer subscription from the same company cannot be used here; the guide says so.
    pub subscription_note: bool,
    /// Which field carries the token limit.
    pub token_param: TokenParam,
    /// Headers sent on every request.
    pub extra_headers: &'static [(&'static str, &'static str)],
    /// Fields merged into every chat body.
    pub extra_body: Option<fn() -> Value>,
}

fn minimax_body() -> Value {
    json!({"reasoning_split": true})
}

const BASE: Preset = Preset {
    key: "",
    name: "",
    protocol: Protocol::OpenAiCompatible,
    base_url: "",
    local: false,
    guide_url: "",
    images_by_default: false,
    structured_output: StructuredOutput::JsonSchema,
    lists_models: true,
    subscription_note: false,
    token_param: TokenParam::MaxTokens,
    extra_headers: &[],
    extra_body: None,
};

static PRESETS: [Preset; 11] = [
    Preset {
        key: "gemini",
        name: "Google Gemini",
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai/",
        guide_url: "https://aistudio.google.com/apikey",
        images_by_default: true,
        subscription_note: true,
        ..BASE
    },
    Preset {
        key: "openai",
        name: "OpenAI",
        base_url: "https://api.openai.com/v1",
        guide_url: "https://platform.openai.com/api-keys",
        images_by_default: true,
        subscription_note: true,
        token_param: TokenParam::MaxCompletionTokens,
        ..BASE
    },
    Preset {
        key: "anthropic",
        name: "Anthropic",
        protocol: Protocol::Anthropic,
        base_url: "https://api.anthropic.com",
        guide_url: "https://console.anthropic.com/settings/keys",
        images_by_default: true,
        subscription_note: true,
        ..BASE
    },
    Preset {
        key: "openrouter",
        name: "OpenRouter",
        base_url: "https://openrouter.ai/api/v1",
        guide_url: "https://openrouter.ai/settings/keys",
        images_by_default: true,
        extra_headers: &[("X-OpenRouter-Title", "postit")],
        ..BASE
    },
    Preset {
        key: "routellm",
        name: "Abacus AI RouteLLM",
        base_url: "https://routellm.abacus.ai/v1",
        guide_url: "https://abacus.ai/app/route-llm-apis",
        ..BASE
    },
    Preset {
        key: "minimax",
        name: "MiniMax",
        base_url: "https://api.minimax.io/v1",
        guide_url: "https://platform.minimax.io/user-center/basic-information/interface-key",
        structured_output: StructuredOutput::Text,
        lists_models: false,
        extra_body: Some(minimax_body),
        ..BASE
    },
    Preset {
        key: "moonshot",
        name: "Moonshot (Kimi)",
        base_url: "https://api.moonshot.ai/v1",
        guide_url: "https://platform.kimi.ai/console/api-keys",
        lists_models: false,
        ..BASE
    },
    Preset {
        key: "ollama",
        name: "Ollama (local)",
        base_url: "http://localhost:11434/v1",
        local: true,
        guide_url: "https://ollama.com/download",
        structured_output: StructuredOutput::JsonObject,
        ..BASE
    },
    Preset {
        key: "llamacpp",
        name: "llama.cpp server (local)",
        base_url: "http://localhost:9931/v1",
        local: true,
        guide_url: "https://github.com/ggml-org/llama.cpp/tree/master/tools/server",
        structured_output: StructuredOutput::Text,
        ..BASE
    },
    Preset {
        key: "vllm",
        name: "vLLM (local)",
        base_url: "http://localhost:8000/v1",
        local: true,
        guide_url: "https://docs.vllm.ai/en/latest/serving/openai_compatible_server.html",
        ..BASE
    },
    Preset {
        key: "lmstudio",
        name: "LM Studio (local)",
        base_url: "http://localhost:1234/v1",
        local: true,
        guide_url: "https://lmstudio.ai/docs/developer/openai-compat",
        images_by_default: false,
        ..BASE
    },
];

/// Every preset, in display order.
#[must_use]
pub fn presets() -> &'static [Preset] {
    &PRESETS
}

/// The preset with this key.
#[must_use]
pub fn preset(key: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.key == key)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::config::{Protocol, TokenParam};

    #[test]
    #[allow(clippy::too_many_lines)]
    fn preset_table_matches_spec() {
        use Protocol::{Anthropic, OpenAiCompatible as Oai};
        use StructuredOutput::{JsonObject, JsonSchema, Text};
        use TokenParam::{MaxCompletionTokens, MaxTokens};
        // key, protocol, base url, local, structured output, token param
        let expected = [
            (
                "gemini",
                Oai,
                "https://generativelanguage.googleapis.com/v1beta/openai/",
                false,
                JsonSchema,
                MaxTokens,
            ),
            (
                "openai",
                Oai,
                "https://api.openai.com/v1",
                false,
                JsonSchema,
                MaxCompletionTokens,
            ),
            (
                "anthropic",
                Anthropic,
                "https://api.anthropic.com",
                false,
                JsonSchema,
                MaxTokens,
            ),
            (
                "openrouter",
                Oai,
                "https://openrouter.ai/api/v1",
                false,
                JsonSchema,
                MaxTokens,
            ),
            (
                "routellm",
                Oai,
                "https://routellm.abacus.ai/v1",
                false,
                JsonSchema,
                MaxTokens,
            ),
            (
                "minimax",
                Oai,
                "https://api.minimax.io/v1",
                false,
                Text,
                MaxTokens,
            ),
            (
                "moonshot",
                Oai,
                "https://api.moonshot.ai/v1",
                false,
                JsonSchema,
                MaxTokens,
            ),
            (
                "ollama",
                Oai,
                "http://localhost:11434/v1",
                true,
                JsonObject,
                MaxTokens,
            ),
            (
                "llamacpp",
                Oai,
                "http://localhost:9931/v1",
                true,
                Text,
                MaxTokens,
            ),
            (
                "vllm",
                Oai,
                "http://localhost:8000/v1",
                true,
                JsonSchema,
                MaxTokens,
            ),
            (
                "lmstudio",
                Oai,
                "http://localhost:1234/v1",
                true,
                JsonSchema,
                MaxTokens,
            ),
        ];
        assert_eq!(presets().len(), expected.len());
        for (i, (key, protocol, base, local, structured, token)) in expected.into_iter().enumerate()
        {
            let p = &presets()[i];
            assert_eq!(p.key, key);
            assert_eq!(p.protocol, protocol, "{key}");
            assert_eq!(p.base_url, base, "{key}");
            assert_eq!(p.local, local, "{key}");
            assert_eq!(p.structured_output, structured, "{key}");
            assert_eq!(p.token_param, token, "{key}");
            assert!(p.guide_url.starts_with("https://"), "{key}");
            assert_eq!(preset(key).unwrap().key, key);
        }
        assert!(preset("nope").is_none());
        assert_eq!(
            preset("openrouter").unwrap().extra_headers,
            &[("X-OpenRouter-Title", "postit")]
        );
        assert_eq!(
            (preset("minimax").unwrap().extra_body.unwrap())(),
            json!({"reasoning_split": true})
        );
        for p in presets() {
            if p.key != "openrouter" {
                assert!(p.extra_headers.is_empty(), "{}", p.key);
            }
            if p.key != "minimax" {
                assert!(p.extra_body.is_none(), "{}", p.key);
            }
        }
    }

    #[test]
    fn image_defaults_and_listing_follow_the_spec() {
        for key in ["gemini", "openai", "anthropic", "openrouter"] {
            assert!(preset(key).unwrap().images_by_default, "{key}");
        }
        for key in [
            "routellm", "minimax", "moonshot", "ollama", "llamacpp", "vllm",
        ] {
            assert!(!preset(key).unwrap().images_by_default, "{key}");
        }
        assert!(!preset("minimax").unwrap().lists_models);
        assert!(preset("gemini").unwrap().lists_models);
        assert!(preset("openai").unwrap().subscription_note);
        assert!(!preset("ollama").unwrap().subscription_note);
    }
}
