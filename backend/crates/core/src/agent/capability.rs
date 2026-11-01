//! The optional abilities an agent client may expose.

use std::collections::BTreeMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;

use super::client::{PublishProgress, PublishRequest, ReconcileRequest};
use crate::{
    AccountMetrics, AccountRef, AgentError, DirectSendId, InboxPage, MediaAsset, MetricSnapshot,
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

    /// Posts the comment in `request.variant`. Failures after the request may have reached the
    /// platform follow the rule on [`AgentClient::publish`](super::AgentClient::publish).
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
    /// An [`AgentError`] classified by `retry_class`. [`AgentError::Unsupported`] parks the
    /// delivery as `outcome_unresolvable`.
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

/// The send ID reserved for a test email. A send carrying it has no `direct_sends` row, counts
/// against no cap, gets no tracking rewrite, carries no unsubscribe headers, and takes a random
/// `Message-ID`. Real send IDs are version 7 UUIDs, never nil.
pub const TEST_SEND_ID: DirectSendId = DirectSendId::from_uuid(uuid::Uuid::nil());

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
        /// A short fixed code such as `smtp_5_1_1`, never the server's text: the engine
        /// stores it in `direct_sends.error_code`, and replies often echo the address.
        reason: String,
    },
    /// The send failed before reaching the address.
    Failed {
        /// Whether trying again may work.
        retryable: bool,
        /// A short fixed code, never the server's text, for the same reason as
        /// [`DirectSendOutcome::Bounced`]'s.
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
    /// Sends `variant` to each recipient and reports each result. `assets` are the stored
    /// media the variant refers to, its attachments and its email document's images, as
    /// `validate` gets them.
    ///
    /// # Errors
    /// An [`AgentError`] when the whole batch failed; per-recipient failures are results.
    async fn send_batch(
        &self,
        account: &AccountRef,
        recipients: &[DirectRecipient],
        variant: &Variant,
        assets: &[MediaAsset],
    ) -> Result<Vec<DirectSendResult>, AgentError>;
}

/// Which sender-domain DNS record a finding is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum DomainRecord {
    /// The SPF `TXT` record.
    Spf,
    /// The DKIM key record for a selector.
    Dkim,
    /// The `_dmarc` policy record.
    Dmarc,
}

/// How a sender-domain record looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum DomainStatus {
    /// Present and sound.
    Ok,
    /// Not published.
    Missing,
    /// Published, but too permissive to protect the domain.
    Weak,
    /// Could not be decided: the lookup failed, or (for DKIM) no selector was given.
    Unknown,
}

/// One result of a sender-domain check. Clients translate it by `code`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DomainFinding {
    /// Which record.
    pub record: DomainRecord,
    /// How it looks.
    pub status: DomainStatus,
    /// A short fixed code naming the finding, such as `spf_missing`.
    pub code: String,
}

/// Whether `selector` can be a sender-domain check's `dkim_selector`: one DNS label of 1 to
/// 63 ASCII letters, digits, and inner hyphens.
#[must_use]
pub fn is_dkim_selector(selector: &str) -> bool {
    (1..=63).contains(&selector.len())
        && selector
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !selector.starts_with('-')
        && !selector.ends_with('-')
}

/// Checks the DNS records of the domain an account sends from.
#[async_trait]
pub trait SenderDomainCheck: Send + Sync {
    /// Looks up SPF, DKIM (under `dkim_selector`, when given), and DMARC for `account`'s
    /// sending domain.
    ///
    /// # Errors
    /// An [`AgentError`] when the check could not run at all, `Validation` for a
    /// `dkim_selector` that fails [`is_dkim_selector`]; a failed lookup is a finding.
    async fn check(
        &self,
        account: &AccountRef,
        dkim_selector: Option<&str>,
    ) -> Result<Vec<DomainFinding>, AgentError>;
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
