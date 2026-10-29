//! Wire types shared by routes (and the `OpenAPI` document).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use postit_data::users::{UserRecord, UserRole, UserStatus};
use postit_i18n::Locale;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// A user's role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RoleDto {
    /// Manages users and deployment settings.
    Admin,
    /// An ordinary user.
    Member,
}

/// A user's account status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StatusDto {
    /// Signed in, waiting for admin approval.
    Pending,
    /// Approved and allowed to use the API.
    Active,
    /// Blocked by an admin.
    Disabled,
    /// Being deleted.
    Deleting,
}

impl From<UserRole> for RoleDto {
    fn from(r: UserRole) -> Self {
        match r {
            UserRole::Admin => Self::Admin,
            UserRole::Member => Self::Member,
        }
    }
}

impl From<RoleDto> for UserRole {
    fn from(r: RoleDto) -> Self {
        match r {
            RoleDto::Admin => Self::Admin,
            RoleDto::Member => Self::Member,
        }
    }
}

impl From<UserStatus> for StatusDto {
    fn from(s: UserStatus) -> Self {
        match s {
            UserStatus::Pending => Self::Pending,
            UserStatus::Active => Self::Active,
            UserStatus::Disabled => Self::Disabled,
            UserStatus::Deleting => Self::Deleting,
        }
    }
}

impl From<StatusDto> for UserStatus {
    fn from(s: StatusDto) -> Self {
        match s {
            StatusDto::Pending => Self::Pending,
            StatusDto::Active => Self::Active,
            StatusDto::Disabled => Self::Disabled,
            StatusDto::Deleting => Self::Deleting,
        }
    }
}

/// A postit user as admins and the user themselves see it. Profile fields come from the identity provider.
#[derive(Debug, Serialize, ToSchema)]
pub struct UserDto {
    /// User ID.
    pub id: Uuid,
    /// Email address from the identity provider, when it supplied one.
    pub email: Option<String>,
    /// Whether the provider verified the email address.
    pub email_verified: bool,
    /// Name shown in the UI.
    pub display_name: String,
    /// The user's role.
    pub role: RoleDto,
    /// The user's account status.
    pub status: StatusDto,
    /// When an admin approved the user.
    pub approved_at: Option<DateTime<Utc>>,
    /// The admin who approved the user.
    pub approved_by: Option<Uuid>,
    /// When the user last made an authenticated request.
    pub last_seen_at: Option<DateTime<Utc>>,
    /// When the user was created.
    pub created_at: DateTime<Utc>,
    /// When the user was last changed.
    pub updated_at: DateTime<Utc>,
}

impl From<&UserRecord> for UserDto {
    fn from(u: &UserRecord) -> Self {
        Self {
            id: u.id.as_uuid(),
            email: u.email.clone(),
            email_verified: u.email_verified,
            display_name: u.display_name.clone(),
            role: u.role.into(),
            status: u.status.into(),
            approved_at: u.approved_at,
            approved_by: u.approved_by.map(|id| id.as_uuid()),
            last_seen_at: u.last_seen_at,
            created_at: u.created_at,
            updated_at: u.updated_at,
        }
    }
}

/// The signed-in user.
#[derive(Debug, Serialize, ToSchema)]
pub struct MeDto {
    /// The user's own record.
    #[serde(flatten)]
    pub user: UserDto,
    /// The identity provider's account page (`auth.oidc.account_url`), when configured.
    pub account_url: Option<String>,
}

/// A language the interface and emails can be shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
pub enum LocaleDto {
    /// American English (the default).
    #[serde(rename = "en-US")]
    EnUs,
    /// Arabic (right to left).
    #[serde(rename = "ar")]
    Ar,
}

impl From<Locale> for LocaleDto {
    fn from(locale: Locale) -> Self {
        match locale {
            Locale::EnUs => Self::EnUs,
            Locale::Ar => Self::Ar,
        }
    }
}

/// The caller's preferences.
#[derive(Debug, Serialize, ToSchema)]
pub struct PreferencesDto {
    /// The language of the interface and of emails sent to the caller.
    pub locale: LocaleDto,
}

/// Changes the caller's preferences.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdatePreferences {
    /// `en-US` or `ar`; anything else is rejected with `validation_failed`.
    pub locale: String,
}

/// What a client needs to start the OIDC sign-in.
#[derive(Debug, Serialize, ToSchema)]
pub struct AuthConfigDto {
    /// Issuer URL of the identity provider.
    pub issuer: String,
    /// OAuth client ID of the web client.
    pub client_id: String,
    /// OAuth client ID of the Android app, or null when the deployment has none.
    pub android_client_id: Option<String>,
    /// OAuth client ID of the iOS app, or null when the deployment has none.
    pub ios_client_id: Option<String>,
    /// Scopes to request at sign-in.
    pub scopes: Vec<String>,
}

/// Confirmation for deleting one's own account.
#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteMeRequest {
    /// Must equal the caller's current display name.
    pub display_name: String,
}

/// Exactly one of `status` or `role`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchUserRequest {
    /// The new status.
    pub status: Option<StatusDto>,
    /// The new role.
    pub role: Option<RoleDto>,
}

/// A user reference in the audit log. `deleted` is true for a pseudonym left by user
/// deletion (a UUID v8, which postit never generates otherwise). `display_name` is the
/// user's current name, or null for a pseudonym or a user no longer present.
#[derive(Debug, Serialize, ToSchema)]
pub struct UserRef {
    /// User ID, or the pseudonym that replaced it.
    pub id: Uuid,
    /// Whether the user has been deleted.
    pub deleted: bool,
    /// The user's current name, when known.
    pub display_name: Option<String>,
}

impl UserRef {
    /// Resolves `id` against `names`, a map of user ID to display name.
    #[must_use]
    pub fn resolve(id: Uuid, names: &HashMap<Uuid, String>) -> Self {
        let deleted = id.get_version_num() == 8;
        Self {
            id,
            deleted,
            display_name: if deleted {
                None
            } else {
                names.get(&id).cloned()
            },
        }
    }
}

/// Every audit event kind (`AuditEventKind` in `postit-data`); the `OpenAPI` enum the web
/// client's kind filter is typed from. Clients must tolerate values added later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventKindDto {
    /// A user record was created at first sign-in.
    UserProvisioned,
    /// An admin approved a pending user.
    UserApproved,
    /// An admin disabled a user.
    UserDisabled,
    /// An admin re-enabled a disabled user.
    UserEnabled,
    /// An admin changed a user's role.
    RoleChanged,
    /// The first admin was granted through bootstrap.
    BootstrapAdminGranted,
    /// A user and their data were deleted.
    UserDeleted,
    /// An email was sent.
    EmailSent,
    /// An email was dropped without sending.
    EmailDropped,
    /// An email could not be sent.
    EmailFailed,
    /// An admin retried a job.
    JobRetried,
    /// An admin cancelled a job.
    JobCancelled,
    /// An admin deleted a job.
    JobDeleted,
    /// An admin triggered a recurring job.
    JobTriggered,
    /// A vault key rotation run began.
    VaultRotationStarted,
    /// A vault key rotation run finished.
    VaultRotationCompleted,
    /// An account was connected, reactivated, or moved to a new credential.
    AccountConnected,
    /// The owner disconnected an account.
    AccountDisconnected,
    /// An account's credential stopped working.
    AccountNeedsReconnect,
    /// A CSV import of credential accounts ran.
    AccountsImported,
    /// The platform reported that the user revoked access; the account was revoked.
    AccountRevokedByPlatform,
    /// An owner granted a user delegated access.
    DelegationGranted,
    /// An owner changed a grant.
    DelegationChanged,
    /// An owner revoked a grant.
    DelegationRevoked,
    /// A delegate left a grant.
    DelegationLeft,
    /// A grant expired.
    DelegationExpired,
    /// An owner or delegate confirmed a live publish.
    PublishConfirmed,
    /// A publish job's time was changed.
    PublishJobRescheduled,
    /// A publish job was cancelled.
    PublishJobCancelled,
    /// A delivery was cancelled.
    DeliveryCancelled,
    /// A delivery was retried by hand.
    DeliveryRetried,
    /// A delivery with an unknown outcome was resolved by hand.
    DeliveryResolved,
    /// A delivery was published.
    DeliveryPublished,
    /// A delivery ran out of attempts.
    DeliveryDeadLettered,
    /// A delivery's outcome could not be determined.
    DeliveryOutcomeUnknown,
}

/// One entry of the audit log.
#[derive(Debug, Serialize, ToSchema)]
pub struct AuditEventDto {
    /// Event ID.
    pub id: Uuid,
    /// When the event was recorded.
    pub at: DateTime<Utc>,
    /// The event kind.
    #[schema(value_type = AuditEventKindDto)]
    pub kind: String,
    /// Who performed the action; none for system actions.
    pub actor: Option<UserRef>,
    /// Owner of the workspace the action touched.
    pub owner: Option<UserRef>,
    /// Who the action was about.
    pub subject: Option<UserRef>,
    /// Client IP, until it is cleared by retention.
    pub ip: Option<String>,
    /// `X-Request-Id` of the request that caused the event.
    pub request_id: Option<Uuid>,
    /// Event details. Every `*_user_id` value is a `UserRef` object.
    #[schema(value_type = Object)]
    pub details: serde_json::Value,
}

impl AuditEventDto {
    /// IDs this row references (actor, owner, subject, and every `*_user_id` detail).
    #[must_use]
    pub fn referenced_ids(row: &postit_data::audit_repo::AuditEventRow) -> Vec<Uuid> {
        let mut ids: Vec<Uuid> = [row.actor_user_id, row.owner_id, row.subject_user_id]
            .into_iter()
            .flatten()
            .map(|u| u.as_uuid())
            .collect();
        if let Some(map) = row.details.as_object() {
            ids.extend(
                map.iter()
                    .filter(|(k, _)| k.ends_with("_user_id"))
                    .filter_map(|(_, v)| v.as_str().and_then(|s| Uuid::parse_str(s).ok())),
            );
        }
        ids
    }

    /// Builds the DTO for `row`, resolving user references through `names`.
    #[must_use]
    pub fn from_row(
        row: postit_data::audit_repo::AuditEventRow,
        names: &HashMap<Uuid, String>,
    ) -> Self {
        let mut details = row.details;
        if let Some(map) = details.as_object_mut() {
            for (key, value) in &mut *map {
                if key.ends_with("_user_id")
                    && let Some(id) = value.as_str().and_then(|s| Uuid::parse_str(s).ok())
                {
                    *value = serde_json::to_value(UserRef::resolve(id, names))
                        .unwrap_or(serde_json::Value::Null);
                }
            }
        }
        let user = |u: Option<postit_core::UserId>| u.map(|u| UserRef::resolve(u.as_uuid(), names));
        Self {
            id: row.id.as_uuid(),
            at: row.at,
            kind: row.kind,
            actor: user(row.actor_user_id),
            owner: user(row.owner_id),
            subject: user(row.subject_user_id),
            ip: row.ip,
            request_id: row.request_id,
            details,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AuditEventKindDto;
    use postit_data::audit::AuditEventKind;

    const ALL: [AuditEventKindDto; 35] = [
        AuditEventKindDto::UserProvisioned,
        AuditEventKindDto::UserApproved,
        AuditEventKindDto::UserDisabled,
        AuditEventKindDto::UserEnabled,
        AuditEventKindDto::RoleChanged,
        AuditEventKindDto::BootstrapAdminGranted,
        AuditEventKindDto::UserDeleted,
        AuditEventKindDto::EmailSent,
        AuditEventKindDto::EmailDropped,
        AuditEventKindDto::EmailFailed,
        AuditEventKindDto::JobRetried,
        AuditEventKindDto::JobCancelled,
        AuditEventKindDto::JobDeleted,
        AuditEventKindDto::JobTriggered,
        AuditEventKindDto::VaultRotationStarted,
        AuditEventKindDto::VaultRotationCompleted,
        AuditEventKindDto::AccountConnected,
        AuditEventKindDto::AccountDisconnected,
        AuditEventKindDto::AccountNeedsReconnect,
        AuditEventKindDto::AccountsImported,
        AuditEventKindDto::AccountRevokedByPlatform,
        AuditEventKindDto::DelegationGranted,
        AuditEventKindDto::DelegationChanged,
        AuditEventKindDto::DelegationRevoked,
        AuditEventKindDto::DelegationLeft,
        AuditEventKindDto::DelegationExpired,
        AuditEventKindDto::PublishConfirmed,
        AuditEventKindDto::PublishJobRescheduled,
        AuditEventKindDto::PublishJobCancelled,
        AuditEventKindDto::DeliveryCancelled,
        AuditEventKindDto::DeliveryRetried,
        AuditEventKindDto::DeliveryResolved,
        AuditEventKindDto::DeliveryPublished,
        AuditEventKindDto::DeliveryDeadLettered,
        AuditEventKindDto::DeliveryOutcomeUnknown,
    ];

    /// A new `AuditEventKind` fails this until the DTO enum gains it.
    #[test]
    fn kind_dto_matches_audit_event_kind() {
        assert_eq!(ALL.len(), AuditEventKind::ALL.len());
        for (dto, kind) in ALL.iter().zip(AuditEventKind::ALL) {
            let json = serde_json::to_value(dto).unwrap_or_default();
            assert_eq!(json, kind.as_str());
        }
    }
}
