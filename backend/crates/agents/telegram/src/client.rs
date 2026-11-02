//! The per-grant client.

use std::sync::Arc;

use async_trait::async_trait;
use postit_agent_kit::post_text;
use postit_agent_kit::standard;
use postit_core::{
    AccountInfo, AccountRef, AccountStatus, AgentClient, AgentContext, AgentError, Capability,
    Credentials, Discovered, MediaAsset, OptionsSchema, PublishMode, PublishOutcome,
    PublishProgress, PublishReceipt, PublishRequest, ReconcileOutcome, ReconcileRequest, Variant,
    Violation,
};
use secrecy::SecretString;

use crate::Shared;
use crate::api::{Api, ApiError, SendText, SentMessage};
use crate::compose::{self, Method, Options};
use crate::credentials::{self, ChatRef, Parsed};
use crate::discover::{self, can_post};
use crate::upload::{self, Outgoing};

/// A Telegram client bound to one grant.
pub struct TelegramClient {
    pub(crate) shared: Arc<Shared>,
    pub(crate) ctx: AgentContext,
}

/// A message keyed `telegram.<key>`.
pub(crate) fn message(key: &str, en: &'static str) -> postit_core::Message {
    postit_agent_kit::message(&crate::descriptor::platform_id(), key, en)
}

/// The receipt for a sent message: its ID, and a link where Telegram has a public one.
pub(crate) fn receipt(sent: &SentMessage) -> PublishReceipt {
    let id = sent.message_id;
    let remote_url = if let Some(name) = &sent.chat.username {
        Some(format!("https://t.me/{name}/{id}"))
    } else if matches!(sent.chat.kind.as_str(), "channel" | "supergroup") {
        // A private chat: `-1001234567890` is chat `1234567890`.
        sent.chat
            .id
            .to_string()
            .strip_prefix("-100")
            .map(|chat| format!("https://t.me/c/{chat}/{id}"))
    } else {
        None
    };
    PublishReceipt {
        remote_id: Some(id.to_string()),
        remote_url: remote_url.and_then(|url| url::Url::parse(&url).ok()),
        container_ref: None,
        remote_publish_at: None,
    }
}

fn unsupported() -> AgentError {
    AgentError::Unsupported {
        capability: Capability::Commenter,
    }
}

impl TelegramClient {
    /// One send: the single request for the variant, to `chat_id`. Returns the first message.
    async fn send(
        &self,
        api: &Api<'_>,
        chat_id: i64,
        out: Outgoing<'_>,
    ) -> Result<SentMessage, ApiError> {
        if out.method == Method::Message {
            return api
                .send_message(&SendText {
                    chat_id,
                    text: out.text,
                    silent: out.options.silent,
                    protect_content: out.options.protect_content,
                    no_link_preview: out.options.no_link_preview,
                })
                .await;
        }
        let form = upload::form(self.ctx.media_store.as_ref(), chat_id, out).await?;
        if out.method == Method::Album {
            let sent = api.send_album(form, out.assets.len()).await?;
            return sent.into_iter().next().ok_or_else(|| {
                ApiError::Agent(AgentError::OutcomeUnknown {
                    message: "the Bot API answered an album with no messages".to_owned(),
                })
            });
        }
        api.send_media(out.method.name(), form).await
    }

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
            &self.shared.config,
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
            display_name: chat.display_name(),
            handle: chat.handle(),
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
        variant: &Variant,
        media: &[MediaAsset],
    ) -> Result<Vec<Violation>, AgentError> {
        Ok(compose::violations(variant, media))
    }

    async fn publish(
        &self,
        request: PublishRequest<'_>,
        progress: &dyn PublishProgress,
    ) -> Result<PublishOutcome, AgentError> {
        let variant = request.variant;
        // Only the variant's own media counts, however many assets the caller holds.
        let assets = upload::resolve(variant, request.assets)?;
        let method = compose::choose(variant, &assets);
        if request.mode == PublishMode::DryRun {
            return Ok(PublishOutcome::Simulated {
                planned: compose::plan(variant, &assets, method),
            });
        }
        if progress.cancel_requested() {
            return Err(AgentError::Cancelled);
        }
        let parsed = self.parsed().await?;
        let http = standard(&self.ctx.http)?;
        let api = self.api(http, &parsed.token);
        let chat_id: i64 = variant.target.remote_id.parse().map_err(|_| {
            AgentError::Validation(vec![Violation::agent_rule(message(
                "chat_id_invalid",
                "The stored chat ID is not a number. Reconnect the account.",
            ))])
        })?;
        let options = Options::read(variant);
        let text = post_text(variant);
        let outgoing = Outgoing {
            method,
            assets: &assets,
            text: &text,
            options,
        };
        // A group upgraded to a supergroup refuses the old ID and does nothing, so repeating
        // once on the new ID cannot post twice. The repeat opens the media again.
        let sent = match self.send(&api, chat_id, outgoing).await {
            Err(ApiError::Migrated(new)) => self.send(&api, new, outgoing).await,
            other => other,
        }?;
        Ok(PublishOutcome::Published(receipt(&sent)))
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
