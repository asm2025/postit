//! Wire types of the notification routes. Payloads are IDs, counts, and display labels only.

use chrono::{DateTime, Utc};
use postit_core::{
    AccountNotice, CommentsNotice, DelegationNotice, DeliveryNotice, JobNotice, Notice,
    NotificationKind, ReconnectNotice, RecurrenceNotice, Severity, UserPendingNotice,
};
use postit_data::notifications::NotificationRow;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

/// The payload of a notification: the object `kind` names (`delivery_*` and
/// `delivery_published` carry a delivery payload, `job_partial` a job payload, and so on).
/// Serialized as the bare object. Order matters to generated clients that try each shape in
/// turn: shapes that are a subset of another come last.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(untagged)]
pub enum NoticePayloadDto {
    /// `delivery_dead_lettered`, `delivery_failed`, `delivery_outcome_unknown`,
    /// `delivery_parked`, `delivery_stale`, `delivery_published`.
    Delivery(DeliveryNotice),
    /// `job_partial`.
    Job(JobNotice),
    /// `reconnect_by_passed`, `reconnect_reminder`.
    Reconnect(ReconnectNotice),
    /// `recurrence_skipped`.
    Recurrence(RecurrenceNotice),
    /// `new_comments`.
    Comments(CommentsNotice),
    /// `delegation_changed`.
    Delegation(DelegationNotice),
    /// `user_pending`.
    UserPending(UserPendingNotice),
    /// `account_needs_reconnect`, `account_revoked_by_platform`.
    Account(AccountNotice),
}

impl From<Notice> for NoticePayloadDto {
    fn from(notice: Notice) -> Self {
        match notice {
            Notice::DeliveryDeadLettered(n)
            | Notice::DeliveryFailed(n)
            | Notice::DeliveryOutcomeUnknown(n)
            | Notice::DeliveryParked(n)
            | Notice::DeliveryStale(n)
            | Notice::DeliveryPublished(n) => Self::Delivery(n),
            Notice::JobPartial(n) => Self::Job(n),
            Notice::AccountNeedsReconnect(n) | Notice::AccountRevokedByPlatform(n) => {
                Self::Account(n)
            }
            Notice::ReconnectByPassed(n) | Notice::ReconnectReminder(n) => Self::Reconnect(n),
            Notice::RecurrenceSkipped(n) => Self::Recurrence(n),
            Notice::DelegationChanged(n) => Self::Delegation(n),
            Notice::NewComments(n) => Self::Comments(n),
            Notice::UserPending(n) => Self::UserPending(n),
        }
    }
}

/// One notification. `payload` is the object `kind` names. Labels in it (account name, platform, the other party's name) are the ones
/// recorded with the notification, so an old row reads the same after a rename or a revoked
/// grant. Clients must tolerate kinds added later.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct NotificationDto {
    /// The notification.
    pub id: Uuid,
    /// What happened.
    pub kind: NotificationKind,
    /// The details of what happened; its shape depends on `kind`.
    #[schema(inline)]
    pub payload: NoticePayloadDto,
    /// How urgent it is.
    pub severity: Severity,
    /// The workspace owner the notice is about; absent for deployment notices. A delegate
    /// uses it to tell the workspaces they act in apart.
    pub owner_id: Option<Uuid>,
    /// When the caller read it.
    pub read_at: Option<DateTime<Utc>>,
    /// When it was recorded.
    pub created_at: DateTime<Utc>,
}

impl From<NotificationRow> for NotificationDto {
    fn from(row: NotificationRow) -> Self {
        Self {
            id: row.id.as_uuid(),
            kind: row.notice.kind(),
            payload: row.notice.into(),
            severity: row.severity,
            owner_id: row.owner.map(|o| o.as_uuid()),
            read_at: row.read_at,
            created_at: row.created_at,
        }
    }
}

/// Which notifications to mark read: exactly one of `ids` (1 to 200 of the caller's own;
/// others are ignored) or `all: true`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkReadRequest {
    /// These notifications.
    pub ids: Option<Vec<Uuid>>,
    /// Every unread notification of the caller. Only `true` selects anything.
    pub all: Option<bool>,
}

/// How many notifications changed.
#[derive(Debug, Serialize, ToSchema)]
pub struct MarkReadResponse {
    /// Notifications newly marked read.
    pub updated: u64,
}

/// The bell badge.
#[derive(Debug, Serialize, ToSchema)]
pub struct UnreadCountDto {
    /// Unread notifications.
    pub count: i64,
}
