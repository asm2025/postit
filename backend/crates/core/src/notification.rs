//! Notification vocabulary: what the engine tells people, how urgent it is, and the
//! payload each kind carries. Payloads hold IDs, counts, and display labels only, never post
//! or comment text, addresses, tokens, or endpoint URLs.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    AccountId, DelegationId, DeliveryId, GrantId, ParkReason, PlatformId, PostId, PublishJobId,
    RecurrenceId, UserId,
};

/// How urgent a notification is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Worth knowing; never emailed as an alert.
    Info,
    /// Needs the owner's attention; pushed and emailed by default.
    Critical,
}

impl Severity {
    /// The value stored in `notifications.severity`.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Critical => "critical",
        }
    }
}

/// Every kind of notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum NotificationKind {
    /// A delivery ran out of attempts.
    DeliveryDeadLettered,
    /// A delivery failed for good.
    DeliveryFailed,
    /// A delivery may or may not have gone out.
    DeliveryOutcomeUnknown,
    /// A delivery is waiting on something outside the engine.
    DeliveryParked,
    /// A parked delivery stayed blocked past the grace period.
    DeliveryStale,
    /// A job finished with some deliveries published and some failed.
    JobPartial,
    /// An account must be reconnected.
    AccountNeedsReconnect,
    /// The platform revoked an account's access.
    AccountRevokedByPlatform,
    /// An account's reconnect-by date passed.
    ReconnectByPassed,
    /// A recurrence skipped an occurrence.
    RecurrenceSkipped,
    /// A delivery was published.
    DeliveryPublished,
    /// An account's reconnect-by date is near.
    ReconnectReminder,
    /// A delegation was granted, changed, or ended.
    DelegationChanged,
    /// New comments arrived on an account.
    NewComments,
    /// A user is waiting for approval.
    UserPending,
}

impl NotificationKind {
    /// Every kind, critical first.
    pub const ALL: [Self; 15] = [
        Self::DeliveryDeadLettered,
        Self::DeliveryFailed,
        Self::DeliveryOutcomeUnknown,
        Self::DeliveryParked,
        Self::DeliveryStale,
        Self::JobPartial,
        Self::AccountNeedsReconnect,
        Self::AccountRevokedByPlatform,
        Self::ReconnectByPassed,
        Self::RecurrenceSkipped,
        Self::DeliveryPublished,
        Self::ReconnectReminder,
        Self::DelegationChanged,
        Self::NewComments,
        Self::UserPending,
    ];

    /// The kind's name as stored in `notifications.kind`.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::DeliveryDeadLettered => "delivery_dead_lettered",
            Self::DeliveryFailed => "delivery_failed",
            Self::DeliveryOutcomeUnknown => "delivery_outcome_unknown",
            Self::DeliveryParked => "delivery_parked",
            Self::DeliveryStale => "delivery_stale",
            Self::JobPartial => "job_partial",
            Self::AccountNeedsReconnect => "account_needs_reconnect",
            Self::AccountRevokedByPlatform => "account_revoked_by_platform",
            Self::ReconnectByPassed => "reconnect_by_passed",
            Self::RecurrenceSkipped => "recurrence_skipped",
            Self::DeliveryPublished => "delivery_published",
            Self::ReconnectReminder => "reconnect_reminder",
            Self::DelegationChanged => "delegation_changed",
            Self::NewComments => "new_comments",
            Self::UserPending => "user_pending",
        }
    }

    /// How urgent the kind is.
    #[must_use]
    pub const fn severity(&self) -> Severity {
        match self {
            Self::DeliveryPublished
            | Self::ReconnectReminder
            | Self::DelegationChanged
            | Self::NewComments
            | Self::UserPending => Severity::Info,
            _ => Severity::Critical,
        }
    }

    /// Whether push is on when the user has not chosen.
    #[must_use]
    pub const fn default_push(&self) -> bool {
        matches!(self.severity(), Severity::Critical) || matches!(self, Self::NewComments)
    }

    /// Whether email is on when the user has not chosen.
    #[must_use]
    pub const fn default_email(&self) -> bool {
        self.email_allowed()
    }

    /// Whether the kind may be emailed at all (critical kinds only).
    #[must_use]
    pub const fn email_allowed(&self) -> bool {
        matches!(self.severity(), Severity::Critical)
    }
}

/// An account as a notification names it, so it still reads correctly after the account or
/// the delegation is gone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AccountLabel {
    /// The account.
    pub account_id: AccountId,
    /// Its display name when the notification was recorded.
    pub account_name: String,
    /// Its platform.
    pub platform: PlatformId,
}

/// Payload of the delivery kinds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DeliveryNotice {
    /// The delivery.
    pub delivery_id: DeliveryId,
    /// Its job.
    pub job_id: PublishJobId,
    /// Its post.
    pub post_id: PostId,
    /// The target account.
    pub account: AccountLabel,
    /// Why it is parked, for parked and stale notices.
    pub park_reason: Option<ParkReason>,
}

/// Payload of `job_partial`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct JobNotice {
    /// The job.
    pub job_id: PublishJobId,
    /// Its post.
    pub post_id: PostId,
    /// Deliveries published so far.
    pub published: u32,
    /// Deliveries failed so far.
    pub failed: u32,
}

/// Payload of the account kinds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AccountNotice {
    /// The account.
    pub account: AccountLabel,
}

/// How near the reconnect-by date a reminder is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ReminderStage {
    /// Seven days left.
    SevenDays,
    /// One day left.
    OneDay,
}

/// Payload of the reconnect-by kinds; one per account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ReconnectNotice {
    /// The grant that needs reconnecting.
    pub grant_id: GrantId,
    /// The account.
    pub account: AccountLabel,
    /// The date the platform stops honoring the connection.
    pub reconnect_by: DateTime<Utc>,
    /// The reminder stage; `None` once the date has passed.
    pub stage: Option<ReminderStage>,
}

/// Why a recurrence skipped an occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// The recurrence rewrites with AI and no provider is configured.
    AiProviderMissing,
    /// The AI rewrite failed validation.
    RewriteInvalid,
    /// The previous occurrence's job is still running.
    JobActive,
}

/// Payload of `recurrence_skipped`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RecurrenceNotice {
    /// The recurrence.
    pub recurrence_id: RecurrenceId,
    /// Its post.
    pub post_id: PostId,
    /// Why it was skipped.
    pub reason: SkipReason,
}

/// Payload of `new_comments`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CommentsNotice {
    /// The account the comments are on.
    pub account: AccountLabel,
    /// How many new comments.
    pub count: u32,
}

/// What happened to a delegation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum DelegationEvent {
    /// Access was granted.
    Granted,
    /// Access level, accounts, or expiry changed.
    Changed,
    /// The owner revoked access.
    Revoked,
    /// The delegate left.
    Left,
    /// The grant expired.
    Expired,
    /// The account limit became empty.
    LimitEmptied,
}

/// Payload of `delegation_changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DelegationNotice {
    /// The delegation.
    pub delegation_id: DelegationId,
    /// What happened.
    pub event: DelegationEvent,
    /// The other party's display name.
    pub party_name: String,
}

/// Payload of `user_pending`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UserPendingNotice {
    /// The user waiting for approval.
    pub user_id: UserId,
}

/// A notification with its payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum Notice {
    /// See [`NotificationKind::DeliveryDeadLettered`].
    DeliveryDeadLettered(DeliveryNotice),
    /// See [`NotificationKind::DeliveryFailed`].
    DeliveryFailed(DeliveryNotice),
    /// See [`NotificationKind::DeliveryOutcomeUnknown`].
    DeliveryOutcomeUnknown(DeliveryNotice),
    /// See [`NotificationKind::DeliveryParked`].
    DeliveryParked(DeliveryNotice),
    /// See [`NotificationKind::DeliveryStale`].
    DeliveryStale(DeliveryNotice),
    /// See [`NotificationKind::JobPartial`].
    JobPartial(JobNotice),
    /// See [`NotificationKind::AccountNeedsReconnect`].
    AccountNeedsReconnect(AccountNotice),
    /// See [`NotificationKind::AccountRevokedByPlatform`].
    AccountRevokedByPlatform(AccountNotice),
    /// See [`NotificationKind::ReconnectByPassed`].
    ReconnectByPassed(ReconnectNotice),
    /// See [`NotificationKind::RecurrenceSkipped`].
    RecurrenceSkipped(RecurrenceNotice),
    /// See [`NotificationKind::DeliveryPublished`].
    DeliveryPublished(DeliveryNotice),
    /// See [`NotificationKind::ReconnectReminder`].
    ReconnectReminder(ReconnectNotice),
    /// See [`NotificationKind::DelegationChanged`].
    DelegationChanged(DelegationNotice),
    /// See [`NotificationKind::NewComments`].
    NewComments(CommentsNotice),
    /// See [`NotificationKind::UserPending`].
    UserPending(UserPendingNotice),
}

impl Notice {
    /// The notice's kind.
    #[must_use]
    pub const fn kind(&self) -> NotificationKind {
        match self {
            Self::DeliveryDeadLettered(_) => NotificationKind::DeliveryDeadLettered,
            Self::DeliveryFailed(_) => NotificationKind::DeliveryFailed,
            Self::DeliveryOutcomeUnknown(_) => NotificationKind::DeliveryOutcomeUnknown,
            Self::DeliveryParked(_) => NotificationKind::DeliveryParked,
            Self::DeliveryStale(_) => NotificationKind::DeliveryStale,
            Self::JobPartial(_) => NotificationKind::JobPartial,
            Self::AccountNeedsReconnect(_) => NotificationKind::AccountNeedsReconnect,
            Self::AccountRevokedByPlatform(_) => NotificationKind::AccountRevokedByPlatform,
            Self::ReconnectByPassed(_) => NotificationKind::ReconnectByPassed,
            Self::RecurrenceSkipped(_) => NotificationKind::RecurrenceSkipped,
            Self::DeliveryPublished(_) => NotificationKind::DeliveryPublished,
            Self::ReconnectReminder(_) => NotificationKind::ReconnectReminder,
            Self::DelegationChanged(_) => NotificationKind::DelegationChanged,
            Self::NewComments(_) => NotificationKind::NewComments,
            Self::UserPending(_) => NotificationKind::UserPending,
        }
    }

    /// The account the notice is about, when it has one; recipient resolution uses it.
    #[must_use]
    pub const fn account(&self) -> Option<AccountId> {
        match self {
            Self::DeliveryDeadLettered(n)
            | Self::DeliveryFailed(n)
            | Self::DeliveryOutcomeUnknown(n)
            | Self::DeliveryParked(n)
            | Self::DeliveryStale(n)
            | Self::DeliveryPublished(n) => Some(n.account.account_id),
            Self::AccountNeedsReconnect(n) | Self::AccountRevokedByPlatform(n) => {
                Some(n.account.account_id)
            }
            Self::ReconnectByPassed(n) | Self::ReconnectReminder(n) => Some(n.account.account_id),
            Self::NewComments(n) => Some(n.account.account_id),
            Self::JobPartial(_)
            | Self::RecurrenceSkipped(_)
            | Self::DelegationChanged(_)
            | Self::UserPending(_) => None,
        }
    }

    /// The inner payload only, as stored in `notifications.payload`.
    #[must_use]
    pub fn payload_json(&self) -> Value {
        // Serializing plain structs of IDs, strings, and numbers cannot fail.
        match serde_json::to_value(self) {
            Ok(Value::Object(mut map)) => map.remove("payload").unwrap_or(Value::Null),
            _ => Value::Null,
        }
    }

    /// Rebuilds a notice from a stored `kind` and `payload`.
    ///
    /// # Errors
    /// The kind is unknown or the payload does not match it.
    pub fn from_row(kind: &str, payload: Value) -> Result<Self, serde_json::Error> {
        let mut map = serde_json::Map::with_capacity(2);
        map.insert("kind".into(), Value::String(kind.into()));
        map.insert("payload".into(), payload);
        serde_json::from_value(Value::Object(map))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use chrono::{TimeZone, Utc};
    use serde_json::{Value, json};
    use uuid::Uuid;

    use super::*;
    use crate::{
        AccountId, DelegationId, DeliveryId, GrantId, ParkReason, PlatformId, PostId, PublishJobId,
        RecurrenceId, UserId,
    };

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn label() -> AccountLabel {
        AccountLabel {
            account_id: AccountId::from_uuid(id(1)),
            account_name: "Main".into(),
            platform: PlatformId::new("mastodon").unwrap(),
        }
    }

    fn delivery() -> DeliveryNotice {
        DeliveryNotice {
            delivery_id: DeliveryId::from_uuid(id(2)),
            job_id: PublishJobId::from_uuid(id(3)),
            post_id: PostId::from_uuid(id(4)),
            account: label(),
            park_reason: Some(ParkReason::QuotaExceeded),
        }
    }

    fn reconnect(stage: Option<ReminderStage>) -> ReconnectNotice {
        ReconnectNotice {
            grant_id: GrantId::from_uuid(id(5)),
            account: label(),
            reconnect_by: Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap(),
            stage,
        }
    }

    fn one_of_each() -> Vec<Notice> {
        vec![
            Notice::DeliveryDeadLettered(delivery()),
            Notice::DeliveryFailed(delivery()),
            Notice::DeliveryOutcomeUnknown(delivery()),
            Notice::DeliveryParked(delivery()),
            Notice::DeliveryStale(delivery()),
            Notice::JobPartial(JobNotice {
                job_id: PublishJobId::from_uuid(id(3)),
                post_id: PostId::from_uuid(id(4)),
                published: 2,
                failed: 1,
            }),
            Notice::AccountNeedsReconnect(AccountNotice { account: label() }),
            Notice::AccountRevokedByPlatform(AccountNotice { account: label() }),
            Notice::ReconnectByPassed(reconnect(None)),
            Notice::RecurrenceSkipped(RecurrenceNotice {
                recurrence_id: RecurrenceId::from_uuid(id(6)),
                post_id: PostId::from_uuid(id(4)),
                reason: SkipReason::JobActive,
            }),
            Notice::DeliveryPublished(delivery()),
            Notice::ReconnectReminder(reconnect(Some(ReminderStage::OneDay))),
            Notice::DelegationChanged(DelegationNotice {
                delegation_id: DelegationId::from_uuid(id(7)),
                event: DelegationEvent::Granted,
                party_name: "Sam".into(),
            }),
            Notice::NewComments(CommentsNotice {
                account: label(),
                count: 3,
            }),
            Notice::UserPending(UserPendingNotice {
                user_id: UserId::from_uuid(id(8)),
            }),
        ]
    }

    #[test]
    fn every_kind_has_one_variant_with_its_payload() {
        let all = one_of_each();
        assert_eq!(all.len(), NotificationKind::ALL.len());
        for kind in NotificationKind::ALL {
            let notice = all.iter().find(|n| n.kind() == kind).unwrap();
            let account_bound = matches!(
                kind,
                NotificationKind::DeliveryDeadLettered
                    | NotificationKind::DeliveryFailed
                    | NotificationKind::DeliveryOutcomeUnknown
                    | NotificationKind::DeliveryParked
                    | NotificationKind::DeliveryStale
                    | NotificationKind::DeliveryPublished
                    | NotificationKind::AccountNeedsReconnect
                    | NotificationKind::AccountRevokedByPlatform
                    | NotificationKind::ReconnectReminder
                    | NotificationKind::ReconnectByPassed
                    | NotificationKind::NewComments
            );
            assert_eq!(
                notice.account(),
                account_bound.then(|| AccountId::from_uuid(id(1))),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn notice_round_trips_with_kind_tag() {
        let notice = Notice::DeliveryFailed(delivery());
        let value = serde_json::to_value(&notice).unwrap();
        assert_eq!(value["kind"], "delivery_failed");
        assert_eq!(value["payload"], notice.payload_json());
        assert_eq!(value["payload"]["delivery_id"], id(2).to_string());
        let back: Notice = serde_json::from_value(value).unwrap();
        assert_eq!(back, notice);
    }

    #[test]
    fn from_row_reads_stored_payload() {
        for notice in one_of_each() {
            let back = Notice::from_row(notice.kind().as_str(), notice.payload_json()).unwrap();
            assert_eq!(back, notice);
        }
        assert!(Notice::from_row("bogus", json!({})).is_err());
        assert!(Notice::from_row("delivery_failed", json!({})).is_err());
    }

    #[test]
    fn defaults_match_the_plan() {
        use NotificationKind as K;
        use Severity::{Critical, Info};
        let table = [
            (K::DeliveryDeadLettered, Critical, true, true),
            (K::DeliveryFailed, Critical, true, true),
            (K::DeliveryOutcomeUnknown, Critical, true, true),
            (K::DeliveryParked, Critical, true, true),
            (K::DeliveryStale, Critical, true, true),
            (K::JobPartial, Critical, true, true),
            (K::AccountNeedsReconnect, Critical, true, true),
            (K::AccountRevokedByPlatform, Critical, true, true),
            (K::ReconnectByPassed, Critical, true, true),
            (K::RecurrenceSkipped, Critical, true, true),
            (K::DeliveryPublished, Info, false, false),
            (K::ReconnectReminder, Info, false, false),
            (K::DelegationChanged, Info, false, false),
            (K::NewComments, Info, true, false),
            (K::UserPending, Info, false, false),
        ];
        assert_eq!(table.len(), K::ALL.len());
        for (kind, severity, push, email) in table {
            assert_eq!(kind.severity(), severity, "{kind:?}");
            assert_eq!(kind.default_push(), push, "{kind:?}");
            assert_eq!(kind.default_email(), email, "{kind:?}");
            assert_eq!(kind.email_allowed(), severity == Critical, "{kind:?}");
            assert_eq!(
                serde_json::to_value(kind).unwrap(),
                Value::String(kind.as_str().into())
            );
        }
    }

    #[test]
    fn payload_has_no_free_text_fields_except_labels() {
        let allowed = [
            "delivery_id",
            "job_id",
            "post_id",
            "account",
            "park_reason",
            "published",
            "failed",
            "grant_id",
            "reconnect_by",
            "stage",
            "recurrence_id",
            "reason",
            "count",
            "delegation_id",
            "event",
            "party_name",
            "user_id",
        ];
        for notice in one_of_each() {
            let payload = notice.payload_json();
            for key in payload.as_object().unwrap().keys() {
                assert!(
                    allowed.contains(&key.as_str()),
                    "{key} in {:?}",
                    notice.kind()
                );
            }
            if let Some(account) = payload.get("account") {
                let mut keys: Vec<_> = account.as_object().unwrap().keys().cloned().collect();
                keys.sort();
                assert_eq!(keys, ["account_id", "account_name", "platform"]);
            }
        }
    }
}
