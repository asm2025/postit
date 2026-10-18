//! The optional abilities an agent client may expose.

use std::collections::BTreeMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use url::Url;

use super::client::{PublishProgress, PublishRequest, ReconcileRequest};
use crate::{
    AccountMetrics, AccountRef, AgentError, DirectSendId, InboxPage, MetricSnapshot,
    PublishOutcome, PublishReceipt, ReconcileOutcome, ReplyTo, Variant,
};

/// What a pasted link points at, in the platform's terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTarget {
    /// What a comment on it would answer.
    pub reply_to: ReplyTo,
}

/// Posts comments and replies.
#[async_trait]
pub trait Commenter: Send + Sync {
    /// Works out what `url` points at.
    ///
    /// # Errors
    /// A target the platform refuses is [`AgentError::Validation`] carrying a
    /// `Violation::agent_rule`.
    async fn resolve_target(
        &self,
        account: &AccountRef,
        url: &Url,
    ) -> Result<RemoteTarget, AgentError>;

    /// Posts the comment in `request.variant`.
    ///
    /// # Errors
    /// An [`AgentError`] classified by `retry_class`.
    async fn comment(
        &self,
        request: PublishRequest<'_>,
        progress: &dyn PublishProgress,
    ) -> Result<PublishOutcome, AgentError>;

    /// Finds out whether an ambiguous comment landed.
    ///
    /// # Errors
    /// An [`AgentError`] classified by `retry_class`.
    async fn reconcile_comment(
        &self,
        request: ReconcileRequest<'_>,
    ) -> Result<ReconcileOutcome, AgentError>;
}

/// Reads comments on the account's posts.
#[async_trait]
pub trait InboxReader: Send + Sync {
    /// Comments newer than `cursor`, or the most recent ones when `cursor` is `None`.
    ///
    /// # Errors
    /// An [`AgentError`] classified by `retry_class`.
    async fn fetch_since(
        &self,
        account: &AccountRef,
        cursor: Option<&str>,
    ) -> Result<InboxPage, AgentError>;
}

/// Reads engagement numbers.
#[async_trait]
pub trait MetricsReader: Send + Sync {
    /// Numbers for one published post.
    ///
    /// # Errors
    /// An [`AgentError`] classified by `retry_class`.
    async fn post_metrics(
        &self,
        account: &AccountRef,
        receipt: &PublishReceipt,
    ) -> Result<MetricSnapshot, AgentError>;

    /// Numbers about the account.
    ///
    /// # Errors
    /// An [`AgentError`] classified by `retry_class`.
    async fn account_metrics(&self, account: &AccountRef) -> Result<AccountMetrics, AgentError>;
}

/// One person a direct message goes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectRecipient {
    /// Identifies this send, for tracking and results.
    pub send_id: DirectSendId,
    /// Where to send: an email address, for the email agent.
    pub address: String,
    /// Values substituted into the message's merge tags.
    pub merge_fields: BTreeMap<String, String>,
    /// The recipient's language tag.
    pub locale: String,
}

/// What happened to one direct send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectSendOutcome {
    /// Handed to the platform.
    Sent {
        /// The platform's ID for the message.
        remote_id: Option<String>,
    },
    /// The address refused it.
    Bounced {
        /// Whether the address will never work.
        permanent: bool,
        /// What the server said, redacted.
        reason: String,
    },
    /// The send failed before reaching the address.
    Failed {
        /// Whether trying again may work.
        retryable: bool,
        /// What went wrong, redacted.
        reason: String,
    },
}

/// The result for one recipient of a batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectSendResult {
    /// Which send this answers.
    pub send_id: DirectSendId,
    /// What happened.
    pub outcome: DirectSendOutcome,
}

/// Sends to individual recipients.
#[async_trait]
pub trait DirectSender: Send + Sync {
    /// Sends `variant` to each recipient and reports each result.
    ///
    /// # Errors
    /// An [`AgentError`] when the whole batch failed; per-recipient failures are results.
    async fn send_batch(
        &self,
        account: &AccountRef,
        recipients: &[DirectRecipient],
        variant: &Variant,
    ) -> Result<Vec<DirectSendResult>, AgentError>;
}

/// Hands scheduled posts to the platform.
#[async_trait]
pub trait NativeScheduler: Send + Sync {
    /// Moves a natively scheduled post to a new time.
    ///
    /// # Errors
    /// An [`AgentError`] classified by `retry_class`.
    async fn reschedule(
        &self,
        account: &AccountRef,
        receipt: &PublishReceipt,
        at: DateTime<Utc>,
    ) -> Result<PublishReceipt, AgentError>;

    /// Removes a natively scheduled post.
    ///
    /// # Errors
    /// An [`AgentError`] classified by `retry_class`.
    async fn delete(
        &self,
        account: &AccountRef,
        receipt: &PublishReceipt,
    ) -> Result<(), AgentError>;
}
