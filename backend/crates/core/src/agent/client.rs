//! The agent and its per-grant client.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use super::capability::{Commenter, DirectSender, InboxReader, MetricsReader, NativeScheduler};
use super::context::{AgentContext, CredentialCheckContext, EventContext};
use super::event::{EventOutcome, InboundEvent};
use super::exchange::{ExchangeContext, GrantExchange, TokenExchange};
use crate::{
    AccountInfo, AccountRef, AgentDescriptor, AgentError, Capability, CredentialFields, DeliveryId,
    Discovered, MediaAsset, OptionsSchema, PublishOutcome, PublishReceipt, ReconcileOutcome,
    Variant, Violation,
};

/// Whether a publish touches the platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishMode {
    /// Send for real.
    Live,
    /// Send nothing; report what would have been sent as [`PublishOutcome::Simulated`].
    DryRun,
}

/// One publish call.
#[derive(Debug, Clone, Copy)]
pub struct PublishRequest<'a> {
    /// What to publish.
    pub variant: &'a Variant,
    /// Live or dry run.
    pub mode: PublishMode,
    /// When the platform should publish it, for a native schedule.
    pub native_publish_at: Option<DateTime<Utc>>,
    /// A container the platform is already processing, from an earlier attempt.
    pub container_ref: Option<&'a str>,
    /// Stable across retries of one delivery; agents pass it to platforms that deduplicate.
    pub idempotency_key: DeliveryId,
}

/// Lets an agent report progress and learn about cancellation mid-publish.
#[async_trait]
pub trait PublishProgress: Send + Sync {
    /// Saves a platform container or upload ID the moment it exists, so a retry resumes it
    /// instead of starting over.
    ///
    /// # Errors
    /// An [`AgentError`] when the save fails.
    async fn save_container_ref(&self, container_ref: &str) -> Result<(), AgentError>;

    /// Whether the user asked to cancel. Agents check it only at safe checkpoints: before the
    /// first mutating call and between upload chunks.
    fn cancel_requested(&self) -> bool;
}

/// A question about an ambiguous earlier publish.
#[derive(Debug, Clone, Copy)]
pub struct ReconcileRequest<'a> {
    /// What was being published.
    pub variant: &'a Variant,
    /// The container the earlier attempt saved, if any.
    pub container_ref: Option<&'a str>,
    /// When the earlier attempt started.
    pub attempted_at: DateTime<Utc>,
    /// The earlier attempt's idempotency key.
    pub idempotency_key: DeliveryId,
}

/// A platform. Built once at startup and registered in the `AgentRegistry`.
///
/// `Unsupported` from `handle_event`, `verify_credentials`, or `revoke` means "not offered", and
/// the conformance suite treats it that way. The `capability` those defaults carry is only the
/// closest variant [`Capability`] has; callers match on `Unsupported` and ignore it.
#[async_trait]
pub trait Agent: Send + Sync {
    /// Everything the system learns about the platform.
    fn descriptor(&self) -> &AgentDescriptor;

    /// The shape of the agent's deployment-level configuration, if it has any.
    fn config_schema(&self) -> Option<&OptionsSchema> {
        None
    }

    /// A client bound to one grant.
    ///
    /// # Errors
    /// An [`AgentError`] when the context's configuration is unusable.
    fn create_client(&self, ctx: AgentContext) -> Result<Box<dyn AgentClient>, AgentError>;

    /// Handles a webhook or callback from the platform.
    ///
    /// # Errors
    /// [`AgentError::Unsupported`] by default.
    async fn handle_event(
        &self,
        ctx: &EventContext,
        event: InboundEvent,
    ) -> Result<EventOutcome, AgentError> {
        let _ = (ctx, event);
        Err(AgentError::Unsupported {
            capability: Capability::InboxReader,
        })
    }

    /// Lets the agent adjust a finished OAuth code exchange before it is stored: swap in a
    /// longer-lived token, read the remote user ID from the raw response, or collect
    /// per-account tokens.
    ///
    /// # Errors
    /// An [`AgentError`] when the follow-up calls fail. The default passes the tokens through.
    async fn after_exchange(
        &self,
        ctx: &ExchangeContext,
        exchange: TokenExchange,
    ) -> Result<GrantExchange, AgentError> {
        let _ = ctx;
        Ok(GrantExchange::from_tokens(exchange.tokens))
    }

    /// Checks credentials the user typed in and lists the accounts they reach.
    ///
    /// # Errors
    /// [`AgentError::Unsupported`] by default; [`AgentError::InvalidCredentials`] when the
    /// platform rejects them.
    async fn verify_credentials(
        &self,
        ctx: &CredentialCheckContext,
        fields: &CredentialFields,
    ) -> Result<Discovered, AgentError> {
        let _ = (ctx, fields);
        Err(AgentError::Unsupported {
            capability: Capability::DirectSender,
        })
    }
}

/// An agent bound to one grant.
#[async_trait]
pub trait AgentClient: Send + Sync {
    /// Reads the account's profile and connection status.
    ///
    /// # Errors
    /// An [`AgentError`] classified by `retry_class`.
    async fn account_info(&self, account: &AccountRef) -> Result<AccountInfo, AgentError>;

    /// Lists the accounts this grant can post to.
    ///
    /// # Errors
    /// An [`AgentError`] classified by `retry_class`.
    async fn discover_accounts(&self) -> Result<Discovered, AgentError>;

    /// The options a post to this account can set, which may depend on the account.
    ///
    /// # Errors
    /// An [`AgentError`] classified by `retry_class`.
    async fn publish_options(&self, account: &AccountRef) -> Result<OptionsSchema, AgentError>;

    /// Platform-side checks the engine cannot do from the descriptor alone.
    ///
    /// # Errors
    /// An [`AgentError`] when the check itself could not run.
    async fn validate(
        &self,
        variant: &Variant,
        media: &[MediaAsset],
    ) -> Result<Vec<Violation>, AgentError>;

    /// Publishes (or, in dry-run mode, simulates publishing) the variant.
    ///
    /// Once a mutating request may have reached the platform, a failure that does not say
    /// whether the platform acted is [`AgentError::OutcomeUnknown`], never `Transport` or a
    /// retryable `Remote` error: pass such errors through [`AgentError::after_send`]. The
    /// engine retries the others, which would post twice.
    ///
    /// # Errors
    /// An [`AgentError`] classified by `retry_class`.
    async fn publish(
        &self,
        request: PublishRequest<'_>,
        progress: &dyn PublishProgress,
    ) -> Result<PublishOutcome, AgentError>;

    /// Asks the platform whether a processing post is live yet.
    ///
    /// # Errors
    /// An [`AgentError`] classified by `retry_class`.
    async fn status(
        &self,
        account: &AccountRef,
        receipt: &PublishReceipt,
    ) -> Result<PublishOutcome, AgentError>;

    /// Finds out whether an ambiguous publish landed.
    ///
    /// # Errors
    /// An [`AgentError`] classified by `retry_class`. [`AgentError::Unsupported`] means the
    /// platform has no way to look the post up; the engine parks the delivery as
    /// `outcome_unresolvable` rather than failing it.
    async fn reconcile(
        &self,
        request: ReconcileRequest<'_>,
    ) -> Result<ReconcileOutcome, AgentError>;

    /// Revokes the grant at the platform.
    ///
    /// # Errors
    /// [`AgentError::Unsupported`] by default, meaning the platform has no revocation call.
    async fn revoke(&self) -> Result<(), AgentError> {
        Err(AgentError::Unsupported {
            capability: Capability::Commenter,
        })
    }

    /// The comment ability, when offered.
    fn commenter(&self) -> Option<&dyn Commenter> {
        None
    }

    /// The inbox ability, when offered.
    fn inbox(&self) -> Option<&dyn InboxReader> {
        None
    }

    /// The metrics ability, when offered.
    fn metrics(&self) -> Option<&dyn MetricsReader> {
        None
    }

    /// The direct-send ability, when offered.
    fn direct(&self) -> Option<&dyn DirectSender> {
        None
    }

    /// The native-schedule ability, when offered.
    fn native_scheduler(&self) -> Option<&dyn NativeScheduler> {
        None
    }
}
