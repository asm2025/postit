//! `postit-core`: shared domain types and the agent contract (plan 03 phase A1), plus the
//! injectable `Clock` and `IdGenerator` seams. No platform, HTTP, or database code.
//!
//! - **Identity and time.** ID newtypes, [`PlatformId`], [`AccountRef`], [`Clock`], and
//!   [`IdGenerator`] are the vocabulary every crate shares.
//! - **Content.** [`Post`], [`PostOverrides`], [`Variant`], [`PublishSnapshot`], the email
//!   document, and media types describe what is written and what is frozen for a publish.
//! - **Platform description.** [`AgentDescriptor`], [`FormatSpec`], [`OptionsSchema`] (the
//!   JSON Schema subset forms render from), and the OAuth provider spec say what a platform
//!   accepts, so the engine validates before any network call.
//! - **Validation.** [`validate_target`] and [`pick_format`] check a variant against its
//!   descriptor and report coded [`Violation`]s that clients localize.
//! - **Agent contract.** [`Agent`], [`AgentClient`], the capability traits, the contexts the
//!   engine passes in, and the [`AgentRegistry`]. Platform API types never cross this
//!   boundary.
//! - **Delivery.** [`DeliveryState`], [`transition`], and [`derive_job_status`] are the one
//!   state machine for publishing; [`AgentError::retry_class`] says how to react to failures.
//! - **Recurrence.** [`RecurrenceRule`] is the supported `RRULE` subset.
//!
//! The roadmap is plan 03 (`docs/plans/03. publishing roadmap.md`); the design is
//! `docs/specs/2026-10-18-a1-core-domain-design.md`. The `openapi` feature derives
//! `utoipa::ToSchema` for the wire types.
#![warn(missing_docs)]

mod account;
mod agent;
#[cfg(test)]
mod async_trait_check;
mod clock;
mod delivery;
mod descriptor;
mod email;
mod error;
mod format;
mod id_generator;
mod ids;
mod inbox;
mod media;
mod message;
mod metrics;
mod notification;
mod oauth;
mod platform;
mod post;
mod receipt;
mod recurrence;
mod schema;
mod validation;

pub use account::AccountRef;
pub use agent::{
    AccountToken, Agent, AgentClient, AgentContext, AgentRegistry, Commenter,
    CredentialCheckContext, CredentialStore, Credentials, DirectRecipient, DirectSendOutcome,
    DirectSendResult, DirectSender, EventContext, EventOutcome, EventResponse, ExchangeContext,
    GrantExchange, HttpHandle, HttpHandles, InboundEvent, InboxReader, LinkSigner, MetricsReader,
    NativeScheduler, OAuthTokens, PublishMode, PublishProgress, PublishRequest, RawTokenResponse,
    ReconcileRequest, RegistryError, RemoteTarget, TokenExchange, exposed_capabilities,
};
pub use clock::{Clock, SystemClock, TestClock};
pub use delivery::{
    DeliveryEvent, DeliveryState, DeliveryView, JobStatus, ParkReason, ResolvedOutcome,
    TransitionError, derive_job_status, transition,
};
pub use descriptor::{
    AgentDescriptor, AgentKind, Capability, ConnectMethod, DescriptorError, NativeScheduleWindow,
    QuotaBucket, QuotaCosts, QuotaPeriod, QuotaScope,
};
pub use email::{EmailBlock, EmailDocument, HeadingLevel};
pub use error::{AgentError, ParseEnumError, RetryClass};
pub use format::{
    AspectRange, FormatId, FormatSelector, FormatSpec, MediaRules, SafeZone, TextRules,
};
pub use id_generator::{IdGenerator, SystemIdGenerator, TestIdGenerator};
pub use ids::{
    AccountId, AiGenerationId, AiProviderId, AuditEventId, BrandProfileId, CampaignId, ContactId,
    ContactImportId, ContactListId, DelegationId, DeliveryAttemptId, DeliveryId, DirectBatchId,
    DirectSendId, GrantId, InboxItemId, MediaAssetId, NotificationId, OAuthStateId,
    PendingConnectionId, PostId, PublishJobId, PushEndpointId, RecurrenceId, TargetGroupId, UserId,
    WritingStyleId,
};
pub use inbox::{InboxItem, InboxPage};
pub use media::{
    ByteStream, MediaAsset, MediaAssetState, MediaError, MediaHost, MediaKind, MediaProbe,
    MediaSource, MediaStore, ProbeInfo,
};
pub use message::Message;
pub use metrics::{AccountMetrics, MetricSnapshot};
pub use notification::{
    AccountLabel, AccountNotice, CommentsNotice, DelegationEvent, DelegationNotice, DeliveryNotice,
    JobNotice, Notice, NotificationKind, ReconnectNotice, RecurrenceNotice, ReminderStage,
    Severity, SkipReason, UserPendingNotice,
};
pub use oauth::{ClientAuth, OAuthProviderSpec, Pkce, RefreshPolicy};
pub use platform::{PlatformId, PlatformIdError};
pub use post::{
    MediaItem, Post, PostKind, PostOverride, PostOverrides, PostVersion, PublishSnapshot, ReplyTo,
    Variant, resolve_variant,
};
pub use receipt::{
    AccountInfo, AccountStatus, Discovered, DiscoveredAccount, PublishOutcome, PublishReceipt,
    ReconcileOutcome,
};
pub use recurrence::{Frequency, RecurrenceEnd, RecurrenceError, RecurrenceRule, Weekday};
pub use schema::{
    CredentialFields, CredentialValue, DependsOn, EnumValue, FieldKind, OptionsSchema, SchemaError,
    SchemaField,
};
pub use validation::{AspectRatio, AspectRatioError, SuggestedFix, Violation, ViolationCode};
pub use validation::{Schedule, ValidationInput, pick_format, validate_target};
pub use validation::{TextMeasure, measure};
