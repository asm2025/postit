//! `postit-core`: shared domain types and the agent contract (plan 03 phase A1), plus the
//! injectable `Clock` and `IdGenerator` seams. No platform, HTTP, or database code.
#![warn(missing_docs)]

#[cfg(test)]
mod async_trait_check;
mod clock;
mod id_generator;
mod ids;
mod message;
mod platform;
mod schema;
mod validation;

pub use clock::{Clock, SystemClock, TestClock};
pub use id_generator::{IdGenerator, SystemIdGenerator, TestIdGenerator};
pub use ids::{
    AccountId, AuditEventId, CampaignId, DeliveryId, DirectSendId, MediaAssetId, PostId,
    PublishJobId, UserId,
};
pub use message::Message;
pub use platform::{PlatformId, PlatformIdError};
pub use schema::{
    CredentialFields, CredentialValue, DependsOn, EnumValue, FieldKind, OptionsSchema, SchemaError,
    SchemaField,
};
pub use validation::{AspectRatio, AspectRatioError, SuggestedFix, Violation, ViolationCode};
pub use validation::{TextMeasure, measure};
