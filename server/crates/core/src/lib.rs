//! `postit-core`: shared domain types and the agent contract (plan 03 phase A1), plus the
//! injectable `Clock` and `IdGenerator` seams. No platform, HTTP, or database code.
#![warn(missing_docs)]

mod account;
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
mod oauth;
mod platform;
mod post;
mod receipt;
mod recurrence;
mod schema;
mod validation;

pub use account::AccountRef;
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
pub use error::{AgentError, RetryClass};
pub use format::{
    AspectRange, FormatId, FormatSelector, FormatSpec, MediaRules, SafeZone, TextRules,
};
pub use id_generator::{IdGenerator, SystemIdGenerator, TestIdGenerator};
pub use ids::{
    AccountId, AuditEventId, CampaignId, DeliveryId, DirectSendId, MediaAssetId, PostId,
    PublishJobId, UserId,
};
pub use inbox::{InboxItem, InboxPage};
pub use media::{
    ByteStream, MediaAsset, MediaAssetState, MediaError, MediaHost, MediaKind, MediaProbe,
    MediaSource, MediaStore, ProbeInfo,
};
pub use message::Message;
pub use metrics::{AccountMetrics, MetricSnapshot};
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
