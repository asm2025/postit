//! Wire types of the admin platform, publishing, limits, and vault routes.

use chrono::{DateTime, Utc};
use postit_data::limits::UserLimits;
use postit_oauth::AdminPlatformView;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// A platform as an admin sees it.
#[derive(Debug, Serialize, ToSchema)]
pub struct AdminPlatformDto {
    /// The platform ID.
    pub id: String,
    /// Name shown to people.
    pub name: String,
    /// Whether this build includes the platform's agent.
    pub compiled_in: bool,
    /// Whether the deployment gives the platform the configuration it needs.
    pub configured: bool,
    /// Whether the platform is switched on.
    pub enabled: bool,
    /// For an OAuth platform, the callback URL to register in its developer portal.
    pub redirect_uri: Option<String>,
}

impl From<AdminPlatformView> for AdminPlatformDto {
    fn from(view: AdminPlatformView) -> Self {
        Self {
            id: view.id.to_string(),
            name: view.display_name,
            compiled_in: view.compiled_in,
            configured: view.configured,
            enabled: view.enabled,
            redirect_uri: view.redirect_uri,
        }
    }
}

/// Switching a platform on or off.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetPlatformEnabledRequest {
    /// Whether the platform is switched on.
    pub enabled: bool,
}

/// The live publishing switch.
#[derive(Debug, Serialize, ToSchema)]
pub struct PublishingSettingsDto {
    /// Whether live publishing is on. While off, live deliveries park and dry runs still run.
    pub live: bool,
    /// Uploads that platforms hold for a later publish time: the switch cannot stop them.
    pub native_scheduled_count: i64,
}

/// Turning live publishing on or off.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetLivePublishingRequest {
    /// Whether live publishing is on.
    pub live: bool,
}

/// A user's limits. `0` means unlimited.
#[derive(Debug, Serialize, ToSchema)]
pub struct UserLimitsDto {
    /// Emails the user may send per day; `0` is unlimited.
    pub email_daily_cap: u32,
    /// Contacts the user may keep; `0` is unlimited.
    pub contacts_cap: u32,
    /// Bytes of media the user may store; `0` is unlimited.
    pub storage_quota_bytes: u64,
    /// When the limits last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<UserLimits> for UserLimitsDto {
    fn from(limits: UserLimits) -> Self {
        Self {
            email_daily_cap: limits.email_daily_cap,
            contacts_cap: limits.contacts_cap,
            storage_quota_bytes: limits.storage_quota_bytes,
            updated_at: limits.updated_at,
        }
    }
}

/// New limits for a user. `0` means unlimited; a negative number is `validation_failed`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PutUserLimitsRequest {
    /// Emails the user may send per day; `0` is unlimited.
    pub email_daily_cap: u32,
    /// Contacts the user may keep; `0` is unlimited.
    pub contacts_cap: u32,
    /// Bytes of media the user may store; `0` is unlimited.
    pub storage_quota_bytes: u64,
}

/// A vault rotation that was queued.
#[derive(Debug, Serialize, ToSchema)]
pub struct VaultRotateResponse {
    /// The queued `vault_rotate` job; follow it in the job console.
    pub job_id: Uuid,
}
