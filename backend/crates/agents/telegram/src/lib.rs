//! `postit-agent-telegram`: the Telegram agent (platform ID `telegram`). It posts text,
//! images, video, and albums to channels and groups through the Bot API, as a broadcast agent
//! connected with a bot token.
//!
//! The bot token sits in the URL path of every call, so the `api` module owns every call and
//! every error mapping, and sends only through `postit_http::send_with_retry` with
//! `Sensitivity::Opaque`. Of the postit crates, the agent may depend on `postit-core`,
//! `postit-http`, and `postit-agent-kit` only.

mod api;
mod client;
mod compose;
pub mod credentials;
pub mod descriptor;
mod discover;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use postit_agent_kit::standard;
use postit_core::{
    Agent, AgentClient, AgentContext, AgentDescriptor, AgentError, CredentialCheckContext,
    CredentialFields, Discovered,
};

use crate::api::Api;
use url::Url;

pub use client::TelegramClient;
pub use descriptor::TELEGRAM;

/// The agent's settings. A credentials agent has no deployment configuration, so the server
/// builds this from [`Default`] and tests override it.
#[derive(Debug, Clone)]
pub struct TelegramConfig {
    /// The Bot API's address.
    pub api_base: Url,
    /// How long one upload may take; an album gets this much per item.
    pub upload_timeout: Duration,
}

impl Default for TelegramConfig {
    fn default() -> Self {
        Self {
            api_base: Url::parse("https://api.telegram.org")
                .unwrap_or_else(|e| unreachable!("a fixed URL parses: {e}")),
            upload_timeout: Duration::from_mins(10),
        }
    }
}

/// What the agent and its clients share.
pub(crate) struct Shared {
    pub(crate) descriptor: AgentDescriptor,
    pub(crate) config: TelegramConfig,
}

/// The Telegram agent.
pub struct TelegramAgent {
    shared: Arc<Shared>,
}

impl TelegramAgent {
    /// The agent with `config`.
    #[must_use]
    pub fn new(config: TelegramConfig) -> Self {
        Self {
            shared: Arc::new(Shared {
                descriptor: descriptor::descriptor(),
                config,
            }),
        }
    }
}

#[async_trait]
impl Agent for TelegramAgent {
    fn descriptor(&self) -> &AgentDescriptor {
        &self.shared.descriptor
    }

    fn create_client(&self, ctx: AgentContext) -> Result<Box<dyn AgentClient>, AgentError> {
        Ok(Box::new(TelegramClient {
            shared: Arc::clone(&self.shared),
            ctx,
        }))
    }

    /// Reads the bot and the chats it reaches. Nothing is sent.
    async fn verify_credentials(
        &self,
        ctx: &CredentialCheckContext,
        fields: &CredentialFields,
    ) -> Result<Discovered, AgentError> {
        let parsed = credentials::parse(fields)?;
        let http = standard(&ctx.http)?;
        let api = Api::new(
            http,
            &self.shared.config.api_base,
            &parsed.token,
            Arc::clone(&ctx.clock),
        );
        discover::discover(&api, &parsed).await
    }
}
