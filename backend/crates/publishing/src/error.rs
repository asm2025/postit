//! The publishing service's error. No variant carries post text, an address, or a credential.

use postit_core::Violation;
use postit_data::{DataError, ScopeError};
use postit_jobs::JobsError;

/// Why a publishing service call failed. Each variant is one of the plan's problem codes;
/// `postit-api` maps them to responses.
#[derive(Debug, thiserror::Error)]
pub enum PublishingError {
    /// The caller's access does not allow the action (`capability_required`, `owner_only`).
    #[error(transparent)]
    Scope(#[from] ScopeError),
    /// No such post, job, delivery, or account in the caller's view.
    #[error("not found")]
    NotFound,
    /// A target or a job's delivery is on an account outside the caller's delegation limit
    /// (`account_not_in_scope`).
    #[error("the action reaches accounts outside the caller's delegation limit")]
    AccountNotInScope,
    /// The post or job changed since the caller read it (`version_conflict`).
    #[error("the post or job changed since it was read")]
    VersionConflict,
    /// A live publish or a retry without `confirm` (`confirmation_required`).
    #[error("a live publish needs the caller's confirmation")]
    ConfirmationRequired,
    /// A retry of a delivery whose outcome is unknown without the acknowledgement that it may
    /// already be live (`acknowledgement_required`).
    #[error("the retry needs the caller's acknowledgement that the post may be live")]
    AcknowledgementRequired,
    /// Every violation found (`validation_failed`).
    #[error("validation failed with {} violation(s)", .0.len())]
    ValidationFailed(Vec<Violation>),
    /// A job with a delivery past `pending` that is not a natively scheduled upload, or a
    /// cancelled job (`not_reschedulable`).
    #[error("the job can no longer be rescheduled")]
    NotReschedulable,
    /// The delivery cannot be sent again: a direct campaign that already materialized its
    /// audience, a delivery in a state that cannot retry, or one whose job was cancelled
    /// (`not_retryable`).
    #[error("the delivery cannot be retried")]
    NotRetryable,
    /// The delivery is in a state the caller's word cannot settle: only a dead-lettered,
    /// outcome-unknown, or `outcome_unresolvable`-parked delivery can be resolved, and one that
    /// changed while the call ran is refused too (`not_resolvable`).
    #[error("the delivery cannot be resolved from its current state")]
    NotResolvable,
    /// The post already has a live job that has not ended (`job_active`).
    #[error("the post already has a live publish job")]
    JobActive,
    /// The post has a job that has not ended, so a new one cannot start (`post_locked`).
    #[error("the post is locked by an active publish job")]
    PostLocked,
    /// An admin switched a target's platform off (`platform_disabled`).
    #[error("platform disabled")]
    PlatformDisabled,
    /// A target's platform is not in this build or lacks its deployment configuration
    /// (`platform_unavailable`).
    #[error("platform unavailable")]
    PlatformUnavailable,
    /// A repository failed.
    #[error(transparent)]
    Data(DataError),
    /// Recording a notification failed.
    #[error(transparent)]
    Notify(#[from] postit_notify::NotifyError),
    /// Stored data no longer reads as the engine wrote it (a post's link or override), or a
    /// dependency failed in a way the engine never expects; the text names which, and holds no
    /// user content. A server error.
    #[error("unexpected stored data or dependency failure: {0}")]
    Corrupt(String),
    /// The job queue failed.
    #[error(transparent)]
    Jobs(#[from] JobsError),
}

impl From<DataError> for PublishingError {
    fn from(error: DataError) -> Self {
        match error {
            DataError::NotFound => Self::NotFound,
            DataError::AccountNotInScope => Self::AccountNotInScope,
            DataError::Scope(scope) => Self::Scope(scope),
            DataError::VersionConflict => Self::VersionConflict,
            DataError::PostLocked => Self::PostLocked,
            DataError::JobActive => Self::JobActive,
            DataError::Conflict(code) if code == "acknowledgement_required" => {
                Self::AcknowledgementRequired
            }
            other => Self::Data(other),
        }
    }
}

impl From<sqlx::Error> for PublishingError {
    fn from(error: sqlx::Error) -> Self {
        Self::Data(DataError::Sql(error))
    }
}

/// Maps a `DeliveryRepo::retry` failure: a conflict that is not the acknowledgement is a
/// delivery that cannot be retried.
pub(crate) fn retry_error(error: DataError) -> PublishingError {
    match error {
        DataError::Conflict(code)
            if matches!(
                code.as_str(),
                "not_retryable" | "transition_not_allowed" | "job_cancelled" | "delivery_changed"
            ) =>
        {
            PublishingError::NotRetryable
        }
        other => other.into(),
    }
}

/// Maps a `DeliveryRepo::resolve_with_status` failure: a state the transition refuses, or a
/// delivery that changed under the lock, cannot be resolved.
pub(crate) fn resolve_error(error: DataError) -> PublishingError {
    match error {
        DataError::Conflict(code)
            if matches!(code.as_str(), "transition_not_allowed" | "delivery_changed") =>
        {
            PublishingError::NotResolvable
        }
        other => other.into(),
    }
}

/// Maps a `JobRepo::reschedule_pending` failure.
pub(crate) fn reschedule_error(error: DataError) -> PublishingError {
    match error {
        DataError::Conflict(code) if code == "not_reschedulable" || code == "job_cancelled" => {
            PublishingError::NotReschedulable
        }
        other => other.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_conflicts_map_to_their_codes() {
        assert!(matches!(
            retry_error(DataError::Conflict("not_retryable".to_owned())),
            PublishingError::NotRetryable
        ));
        assert!(matches!(
            retry_error(DataError::Conflict("acknowledgement_required".to_owned())),
            PublishingError::AcknowledgementRequired
        ));
        assert!(matches!(
            retry_error(DataError::JobActive),
            PublishingError::JobActive
        ));
        assert!(matches!(
            resolve_error(DataError::Conflict("transition_not_allowed".to_owned())),
            PublishingError::NotResolvable
        ));
        assert!(matches!(
            resolve_error(DataError::Conflict("delivery_changed".to_owned())),
            PublishingError::NotResolvable
        ));
        assert!(matches!(
            resolve_error(DataError::NotFound),
            PublishingError::NotFound
        ));
        assert!(matches!(
            reschedule_error(DataError::Conflict("not_reschedulable".to_owned())),
            PublishingError::NotReschedulable
        ));
        assert!(matches!(
            reschedule_error(DataError::AccountNotInScope),
            PublishingError::AccountNotInScope
        ));
        assert!(matches!(
            PublishingError::from(DataError::Conflict("other".to_owned())),
            PublishingError::Data(_)
        ));
    }
}
