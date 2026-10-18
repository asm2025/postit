//! `postit-core`: shared domain types and the agent contract (plan 03 phase A1), plus the
//! injectable `Clock` and `IdGenerator` seams. No platform, HTTP, or database code.
#![warn(missing_docs)]

mod account;
#[cfg(test)]
mod async_trait_check;
mod clock;
mod descriptor;
mod email;
mod format;
mod id_generator;
mod ids;
mod media;
mod message;
mod oauth;
mod platform;
mod post;
mod schema;
mod validation;

pub use account::AccountRef;
pub use clock::{Clock, SystemClock, TestClock};
pub use descriptor::{
    AgentDescriptor, AgentKind, Capability, ConnectMethod, DescriptorError, NativeScheduleWindow,
    QuotaBucket, QuotaCosts, QuotaPeriod, QuotaScope,
};
pub use email::{EmailBlock, EmailDocument, HeadingLevel};
pub use format::{
    AspectRange, FormatId, FormatSelector, FormatSpec, MediaRules, SafeZone, TextRules,
};
pub use id_generator::{IdGenerator, SystemIdGenerator, TestIdGenerator};
pub use ids::{
    AccountId, AuditEventId, CampaignId, DeliveryId, DirectSendId, MediaAssetId, PostId,
    PublishJobId, UserId,
};
pub use media::{
    ByteStream, MediaAsset, MediaAssetState, MediaError, MediaHost, MediaKind, MediaProbe,
    MediaSource, MediaStore, ProbeInfo,
};
pub use message::Message;
pub use oauth::{ClientAuth, OAuthProviderSpec, Pkce, RefreshPolicy};
pub use platform::{PlatformId, PlatformIdError};
pub use post::{
    MediaItem, Post, PostKind, PostOverride, PostOverrides, PostVersion, PublishSnapshot, ReplyTo,
    Variant, resolve_variant,
};
pub use schema::{
    CredentialFields, CredentialValue, DependsOn, EnumValue, FieldKind, OptionsSchema, SchemaError,
    SchemaField,
};
pub use validation::{AspectRatio, AspectRatioError, SuggestedFix, Violation, ViolationCode};
pub use validation::{TextMeasure, measure};
