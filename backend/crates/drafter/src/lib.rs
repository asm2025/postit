//! The AI drafter: provider protocols, the preset catalog, and the opencode-shaped provider
//! configuration. Drafts text only; it never publishes and never touches publishing credentials.

pub mod config;
pub mod error;
pub mod openai;
pub mod presets;
pub mod provider;
mod send;

pub use config::{
    ModelConfig, Protocol, ProviderConfig, ProviderOptions, ProviderSecret, TokenParam,
    normalize_base, parse_pasted,
};
pub use error::{DrafterError, FailureReason, LlmError};
pub use openai::OpenAiCompatible;
pub use presets::{Preset, StructuredOutput, preset, presets};
pub use provider::{
    ChatMessage, ChatRequest, ChatResponse, Finish, LlmProvider, OutputMode, Part, RemoteModel,
    Role, Usage,
};
