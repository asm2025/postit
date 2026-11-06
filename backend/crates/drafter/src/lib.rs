//! The AI drafter: provider protocols, the preset catalog, the opencode-shaped provider
//! configuration, and `DrafterService`. Drafts text only; it never publishes and never touches
//! publishing credentials.

pub mod anthropic;
pub mod config;
pub mod error;
pub mod factory;
pub mod generate;
pub mod openai;
pub mod output;
pub mod presets;
pub mod prompt;
pub mod provider;
mod send;
pub mod service;
#[cfg(feature = "testkit")]
pub mod testkit;

pub use anthropic::Anthropic;
pub use config::{
    ModelConfig, Protocol, ProviderConfig, ProviderOptions, ProviderSecret, TokenParam,
    normalize_base, parse_pasted,
};
pub use error::{DrafterError, FailureReason, LlmError};
pub use factory::{HttpProviderFactory, ProviderFactory};
pub use generate::{GenerateRequest, Generation};
pub use openai::OpenAiCompatible;
pub use presets::{Preset, StructuredOutput, preset, presets};
pub use prompt::Mode;
pub use provider::{
    ChatMessage, ChatRequest, ChatResponse, Finish, LlmProvider, OutputMode, Part, RemoteModel,
    Role, Usage,
};
pub use service::{
    CreateProvider, DrafterDeps, DrafterService, ModelList, ModelSource, ModelView, PresetView,
    ProviderView, TestResult, UpdateProvider,
};
