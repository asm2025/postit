//! Wire types shared by routes (and the `OpenAPI` document).

use chrono::{DateTime, Utc};
use postit_data::users::{UserRecord, UserRole, UserStatus};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RoleDto {
    Admin,
    Member,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StatusDto {
    Pending,
    Active,
    Disabled,
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
    pub id: Uuid,
    pub email: Option<String>,
    pub email_verified: bool,
    pub display_name: String,
    pub role: RoleDto,
    pub status: StatusDto,
    pub approved_at: Option<DateTime<Utc>>,
    pub approved_by: Option<Uuid>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
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

#[derive(Debug, Serialize, ToSchema)]
pub struct MeDto {
    #[serde(flatten)]
    pub user: UserDto,
    /// The identity provider's account page (`auth.oidc.account_url`), when configured.
    pub account_url: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuthConfigDto {
    pub issuer: String,
    pub client_id: String,
    pub scopes: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct DeleteMeRequest {
    /// Must equal the caller's current display name.
    pub display_name: String,
}

/// Exactly one of `status` or `role`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PatchUserRequest {
    pub status: Option<StatusDto>,
    pub role: Option<RoleDto>,
}

/// A user reference in the audit log. `deleted` is true for a pseudonym left by user
/// deletion (a UUID v8, which postit never generates otherwise).
#[derive(Debug, Serialize, ToSchema)]
pub struct UserRef {
    pub id: Uuid,
    pub deleted: bool,
}

impl From<Uuid> for UserRef {
    fn from(id: Uuid) -> Self {
        Self {
            id,
            deleted: id.get_version_num() == 8,
        }
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AuditEventDto {
    pub id: Uuid,
    pub at: DateTime<Utc>,
    pub kind: String,
    pub actor: Option<UserRef>,
    pub owner: Option<UserRef>,
    pub subject: Option<UserRef>,
    pub ip: Option<String>,
    pub request_id: Option<Uuid>,
    /// Event details. Every `*_user_id` value is a `UserRef` object.
    #[schema(value_type = Object)]
    pub details: serde_json::Value,
}

impl From<postit_data::audit_repo::AuditEventRow> for AuditEventDto {
    fn from(row: postit_data::audit_repo::AuditEventRow) -> Self {
        let mut details = row.details;
        if let Some(map) = details.as_object_mut() {
            for (key, value) in &mut *map {
                if key.ends_with("_user_id")
                    && let Some(id) = value.as_str().and_then(|s| Uuid::parse_str(s).ok())
                {
                    *value =
                        serde_json::to_value(UserRef::from(id)).unwrap_or(serde_json::Value::Null);
                }
            }
        }
        Self {
            id: row.id.as_uuid(),
            at: row.at,
            kind: row.kind,
            actor: row.actor_user_id.map(|u| UserRef::from(u.as_uuid())),
            owner: row.owner_id.map(|u| UserRef::from(u.as_uuid())),
            subject: row.subject_user_id.map(|u| UserRef::from(u.as_uuid())),
            ip: row.ip,
            request_id: row.request_id,
            details,
        }
    }
}
