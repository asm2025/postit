//! The provider seam: one trait, two protocols behind it, and the request and response types.

use std::fmt;

use async_trait::async_trait;
use bytes::Bytes;
use serde_json::Value;
use tokio::time::Instant;

use crate::error::LlmError;

/// A chat model service. `chat` and `models` share the caller's deadline across every request
/// they send.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Sends one chat request.
    ///
    /// # Errors
    /// Returns the failure's reason.
    async fn chat(&self, request: &ChatRequest) -> Result<ChatResponse, LlmError>;

    /// Lists the provider's models, following pages.
    ///
    /// # Errors
    /// Returns the failure's reason.
    async fn models(&self, deadline: Instant) -> Result<Vec<RemoteModel>, LlmError>;
}

/// Who wrote a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The user.
    User,
    /// The model.
    Assistant,
}

/// One piece of a message.
#[derive(Clone, PartialEq, Eq)]
pub enum Part {
    /// Text.
    Text(String),
    /// A JPEG still.
    Image {
        /// The encoded image.
        jpeg: Bytes,
    },
}

impl fmt::Debug for Part {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(t) => write!(f, "Text({} chars)", t.chars().count()),
            Self::Image { jpeg } => write!(f, "Image({} bytes)", jpeg.len()),
        }
    }
}

/// One message of the conversation.
#[derive(Clone, PartialEq, Eq)]
pub struct ChatMessage {
    /// Who wrote it.
    pub role: Role,
    /// Its parts, in order.
    pub parts: Vec<Part>,
}

impl fmt::Debug for ChatMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatMessage")
            .field("role", &self.role)
            .field("parts", &self.parts)
            .finish()
    }
}

/// How the answer should be shaped.
#[derive(Clone, PartialEq)]
pub enum OutputMode {
    /// A JSON object matching a schema.
    JsonSchema {
        /// The schema's name.
        name: String,
        /// The JSON schema.
        schema: Value,
    },
    /// Any JSON object.
    JsonObject,
    /// Free text.
    Text,
}

impl OutputMode {
    /// The mode's name, as used in logs.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::JsonSchema { .. } => "json_schema",
            Self::JsonObject => "json_object",
            Self::Text => "text",
        }
    }
}

/// One chat call. Its `Debug` shows sizes only: never the prompt, the system text, or images.
#[derive(Clone)]
pub struct ChatRequest {
    /// The model ID.
    pub model: String,
    /// The system prompt.
    pub system: String,
    /// The conversation.
    pub messages: Vec<ChatMessage>,
    /// Sent only when the provider's config sets one.
    pub temperature: Option<f32>,
    /// The output token limit.
    pub max_tokens: u32,
    /// The answer shape.
    pub output: OutputMode,
    /// When the whole call must be over.
    pub deadline: Instant,
}

impl fmt::Debug for ChatRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatRequest")
            .field("model", &self.model)
            .field("max_tokens", &self.max_tokens)
            .field("output", &self.output.name())
            .field("system_chars", &self.system.chars().count())
            .field("message_count", &self.messages.len())
            .field("messages", &self.messages)
            .finish_non_exhaustive()
    }
}

/// Token counts the provider reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    /// Prompt tokens.
    pub input_tokens: u32,
    /// Answer tokens.
    pub output_tokens: u32,
}

/// Why the model stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finish {
    /// It finished.
    Stop,
    /// It hit the token limit.
    Length,
    /// It declined to answer.
    Refused,
    /// Anything else.
    Other,
}

/// The model's answer. Its `Debug` prints the text's length only.
#[derive(Clone, PartialEq, Eq)]
pub struct ChatResponse {
    /// The answer text.
    pub text: String,
    /// Token counts, when sent.
    pub usage: Option<Usage>,
    /// Why it stopped.
    pub finish: Finish,
}

impl fmt::Debug for ChatResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatResponse")
            .field("text_chars", &self.text.chars().count())
            .field("usage", &self.usage)
            .field("finish", &self.finish)
            .finish()
    }
}

/// One model a provider lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteModel {
    /// The model ID.
    pub id: String,
    /// A display name, when the provider has one.
    pub name: Option<String>,
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use serde_json::json;
    use tokio::time::Instant;

    use super::*;

    #[test]
    fn chat_request_debug_hides_content() {
        let req = ChatRequest {
            model: "m-1".into(),
            system: "SYSTEM-SECRET-TEXT".into(),
            messages: vec![ChatMessage {
                role: Role::User,
                parts: vec![
                    Part::Text("PROMPT-SECRET-TEXT".into()),
                    Part::Image {
                        jpeg: Bytes::from_static(b"IMGBYTES-SECRET"),
                    },
                ],
            }],
            temperature: None,
            max_tokens: 99,
            output: OutputMode::JsonSchema {
                name: "variants".into(),
                schema: json!({"x": "SCHEMA"}),
            },
            deadline: Instant::now(),
        };
        let dbg = format!("{req:?}");
        for hidden in [
            "SYSTEM-SECRET",
            "PROMPT-SECRET",
            "IMGBYTES",
            "SCHEMA",
            "variants",
        ] {
            assert!(!dbg.contains(hidden), "{dbg}");
        }
        // The whole output, so a field added later must be added here on purpose.
        assert_eq!(
            dbg,
            "ChatRequest { model: \"m-1\", max_tokens: 99, output: \"json_schema\", \
             system_chars: 18, message_count: 1, messages: [ChatMessage { role: User, \
             parts: [Text(18 chars), Image(15 bytes)] }], .. }"
        );
        let resp = ChatResponse {
            text: "ANSWER-SECRET".into(),
            usage: None,
            finish: Finish::Stop,
        };
        assert_eq!(
            format!("{resp:?}"),
            "ChatResponse { text_chars: 13, usage: None, finish: Stop }"
        );
    }
}
