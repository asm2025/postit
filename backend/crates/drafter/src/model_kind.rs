//! Guesses whether a model listed by a provider can draft text. Provider listings carry only
//! IDs, so this reads the ID; it labels a model and never hides one.

use serde::Serialize;

/// What a listed model is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelKind {
    /// A chat model, or an ID not recognized as anything else.
    Text,
    /// Probably not a text drafting model: images, speech, audio, embeddings, rerankers.
    Other,
}

/// ID words, by prefix, that name a model of another kind.
const OTHER_STEMS: [&str; 17] = [
    "image",
    "audio",
    "transcri",
    "whisper",
    "embed",
    "rerank",
    "moderation",
    "flux",
    "midjourney",
    "ideogram",
    "recraft",
    "seedream",
    "banana",
    "imagine",
    "vectorize",
    "magnific",
    "background",
];

/// Guesses the kind of a model from its ID. The ID is split into words at everything that is
/// not a letter or digit, so `gpt_image2_edit` has the word `image2` and `gpt-oss-120b` has no
/// match.
#[must_use]
pub fn guess(id: &str) -> ModelKind {
    let id = id.to_ascii_lowercase();
    let other = id
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .any(|w| {
            w == "tts"
                || w.starts_with("dall")
                || is_wan(w)
                || OTHER_STEMS.iter().any(|stem| w.starts_with(stem))
        });
    if other {
        ModelKind::Other
    } else {
        ModelKind::Text
    }
}

/// `wan27`: the Wan video and image models, a name followed by digits only.
fn is_wan(word: &str) -> bool {
    word.strip_prefix("wan")
        .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// IDs from the Abacus `RouteLLM` listing (2026-11-22) that cannot draft text.
    #[test]
    fn non_text_ids_are_other() {
        for id in [
            "gpt_image2",
            "gpt_image15_edit",
            "flux_pro_ultra",
            "flux_kontext",
            "midjourney",
            "ideogram_character",
            "recraft_svg",
            "recraft_vectorize",
            "remove_background",
            "magnific",
            "seedream5_pro",
            "nano_banana21",
            "gemini-nano-banana-2.1",
            "gemini-3-pro-image",
            "gemini-2.5-flash-image",
            "hunyuan_image",
            "grok_imagine_image2",
            "meta_muse_image",
            "wan27",
            "gpt-transcribe",
            "gpt-4o-mini-transcribe",
            "gpt-realtime-whisper",
            "gpt-audio-mini",
            "gemini-2.5-flash-preview-tts",
            "text-embedding-3-small",
            "dall-e-3",
        ] {
            assert_eq!(guess(id), ModelKind::Other, "{id}");
        }
    }

    #[test]
    fn chat_ids_stay_text() {
        for id in [
            "route-llm",
            "route-llm-code",
            "gpt-5",
            "gpt-5.3-codex",
            "gpt-4o",
            "gpt-4o-mini",
            "claude-opus-5-5",
            "claude-fable-5-2",
            "gemini-3.5-flash",
            "gemini-flash-lite-latest",
            "models/gemini-flash-lite-latest",
            "gemini-4-neon",
            "gemini-decision-1",
            "grok-4.7",
            "qwen3.8-max",
            "Qwen/Qwen3.8-27B",
            "moonshotai/Kimi-K3",
            "zai-org/GLM-5.3-Flash",
            "deepseek-ai/DeepSeek-V4-Pro-0813",
            "openai/gpt-oss-120b",
            "MiniMaxAI/MiniMax-M3",
            "google/gemma-4-31b-it",
            "muse-spark-1.3",
            "thinkingmachines/Inkling",
            "elorian-labs/glimpse-1.0",
            "mimo-v2.6-pro",
            "jev-latest",
            "llama3.2:3b",
            "qwen3.5:4b",
            "swan-1",
        ] {
            assert_eq!(guess(id), ModelKind::Text, "{id}");
        }
    }
}
