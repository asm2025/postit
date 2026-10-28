//! Settings sections reserved for plan 03. They are validated at load and read by later
//! phases.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Publishing engine settings. Reserved: validated at load, not yet read by any job.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishingSettings {
    /// Reserved: whether live publishing is on; `false` forces dry runs everywhere.
    pub live: bool,
    /// Reserved: attempts made to publish one delivery before it fails for good.
    pub max_attempts: u32,
    /// Reserved: how long a worker holds a delivery before another may take it.
    #[serde(with = "crate::duration")]
    pub lease: Duration,
    /// Reserved: how long past its lease a delivery waits before the sweep reclaims it.
    #[serde(with = "crate::duration")]
    pub sweep_grace: Duration,
    /// Reserved: how long a delivery may sit parked before it is surfaced to the owner.
    #[serde(with = "crate::duration")]
    pub park_grace: Duration,
    /// Reserved: how long a platform may take to process an uploaded post.
    #[serde(with = "crate::duration")]
    pub processing_timeout: Duration,
}

/// OAuth redirect settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OAuthSettings {
    /// Extra URLs an account connection may return to; each is an absolute URL without a
    /// query or fragment. Empty means only `app.public_url` and the app's custom URL scheme.
    #[serde(default)]
    pub return_to_allowlist: Vec<String>,
}

/// AI drafter settings. Reserved: validated at load, not yet read.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiSettings {
    /// Reserved: timeout of one request to an AI provider.
    #[serde(with = "crate::duration")]
    pub request_timeout: Duration,
    /// Reserved: whether provider endpoints may be private or loopback hosts.
    pub allow_private_endpoints: bool,
}

/// Per-deployment caps. Reserved: validated at load, not yet read. `0` means unlimited.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitsSettings {
    /// Reserved: emails one user may send per day; 0 means unlimited.
    pub email_daily_cap: u32,
    /// Reserved: contacts one user may keep; 0 means unlimited.
    pub contacts_cap: u32,
    /// Reserved: per-user storage quota in bytes; 0 means unlimited, unset uses
    /// `media.user_quota_bytes`.
    #[serde(default)]
    pub storage_quota_bytes: Option<u64>,
}

/// Recurring-post settings. Reserved: validated at load, not yet read.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecurrenceSettings {
    /// Reserved: how far ahead of its time an occurrence is materialized.
    #[serde(with = "crate::duration")]
    pub lead: Duration,
}

/// Inbox polling settings. Reserved: validated at load, not yet read.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InboxSettings {
    /// Reserved: polling interval for an account with recent activity.
    #[serde(with = "crate::duration")]
    pub active_interval: Duration,
    /// Reserved: polling interval for an idle account.
    #[serde(with = "crate::duration")]
    pub idle_interval: Duration,
}

/// Legal pages served by the app. Reserved: validated at load, not yet read.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegalSettings {
    /// Reserved: file served as the privacy policy.
    #[serde(default)]
    pub privacy_path: Option<PathBuf>,
    /// Reserved: file served as the terms of service.
    #[serde(default)]
    pub terms_path: Option<PathBuf>,
    /// Reserved: file served as the data-deletion instructions.
    #[serde(default)]
    pub data_deletion_path: Option<PathBuf>,
}
