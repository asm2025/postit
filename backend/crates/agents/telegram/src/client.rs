//! The per-grant client.

use std::sync::Arc;

use async_trait::async_trait;
use postit_agent_kit::standard;
use postit_core::{
    AccountInfo, AccountRef, AccountStatus, AgentClient, AgentContext, AgentError, Capability,
    Credentials, Discovered, MediaAsset, OptionsSchema, PublishOutcome, PublishProgress,
    PublishReceipt, PublishRequest, ReconcileOutcome, ReconcileRequest, Variant, Violation,
};
use secrecy::SecretString;

use crate::Shared;
use crate::api::Api;
use crate::credentials::{self, ChatRef, Parsed};
use crate::discover::{self, can_post};

/// A Telegram client bound to one grant.
pub struct TelegramClient {
    pub(crate) shared: Arc<Shared>,
    pub(crate) ctx: AgentContext,
}

fn unsupported() -> AgentError {
    AgentError::Unsupported {
        capability: Capability::Commenter,
    }
}

impl TelegramClient {
    /// The grant's stored credentials.
    async fn parsed(&self) -> Result<Parsed, AgentError> {
        match self.ctx.credentials.credentials().await? {
            Credentials::Fields(fields) => credentials::parse(&fields),
            Credentials::OAuth(_) => Err(AgentError::Authentication {
                message: "this grant holds no bot token".to_owned(),
            }),
        }
    }

    pub(crate) fn api<'a>(&'a self, http: &'a reqwest::Client, token: &'a SecretString) -> Api<'a> {
        Api::new(
            http,
            &self.shared.config.api_base,
            token,
            Arc::clone(&self.ctx.clock),
        )
    }
}

#[async_trait]
impl AgentClient for TelegramClient {
    async fn account_info(&self, account: &AccountRef) -> Result<AccountInfo, AgentError> {
        let parsed = self.parsed().await?;
        let http = standard(&self.ctx.http)?;
        let api = self.api(http, &parsed.token);
        let unreachable = || AgentError::AccountUnreachable {
            message: "the bot cannot post to the chat".to_owned(),
        };
        let id: i64 = account.remote_id.parse().map_err(|_| unreachable())?;
        let bot = api.get_me().await.map_err(|err| {
            // A malformed token answers 404; to a stored grant that is a revoked token.
            match AgentError::from(err) {
                AgentError::Remote {
                    status: Some(404), ..
                } => AgentError::Authentication {
                    message: "the bot token is not accepted".to_owned(),
                },
                other => other,
            }
        })?;
        let chat = api.get_chat(&ChatRef::Id(id)).await?;
        let member = api.get_chat_member(chat.id, bot.id).await?;
        if !can_post(&chat.kind, &member) {
            return Err(unreachable());
        }
        Ok(AccountInfo {
            remote_id: account.remote_id.clone(),
            display_name: chat
                .title
                .clone()
                .or_else(|| chat.username.clone())
                .unwrap_or_else(|| account.remote_id.clone()),
            handle: chat.username.as_ref().map(|name| format!("@{name}")),
            avatar_url: None,
            status: AccountStatus::Active,
        })
    }

    async fn discover_accounts(&self) -> Result<Discovered, AgentError> {
        let parsed = self.parsed().await?;
        let http = standard(&self.ctx.http)?;
        discover::discover(&self.api(http, &parsed.token), &parsed).await
    }

    async fn publish_options(&self, _account: &AccountRef) -> Result<OptionsSchema, AgentError> {
        Ok(self.shared.descriptor.options_schema.clone())
    }

    async fn validate(
        &self,
        _variant: &Variant,
        _media: &[MediaAsset],
    ) -> Result<Vec<Violation>, AgentError> {
        Ok(Vec::new())
    }

    async fn publish(
        &self,
        _request: PublishRequest<'_>,
        _progress: &dyn PublishProgress,
    ) -> Result<PublishOutcome, AgentError> {
        Err(unsupported())
    }

    async fn status(
        &self,
        _account: &AccountRef,
        receipt: &PublishReceipt,
    ) -> Result<PublishOutcome, AgentError> {
        // A Telegram post has no processing step.
        Ok(PublishOutcome::Published(receipt.clone()))
    }

    async fn reconcile(
        &self,
        _request: ReconcileRequest<'_>,
    ) -> Result<ReconcileOutcome, AgentError> {
        // The Bot API cannot look a message up, so an ambiguous send parks.
        Err(unsupported())
    }
}
