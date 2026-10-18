//! The life of one delivery (one post to one account) and the status of the job it belongs to.
//!
//! [`transition`] is the only place that decides whether a delivery may move from one state to
//! another. The publishing engine applies the result with a fenced write; nothing else changes
//! a delivery's state.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Why a delivery is waiting for something outside the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ParkReason {
    /// The owner is disabled.
    OwnerDisabled,
    /// The account must be reconnected.
    AccountNeedsReconnect,
    /// The platform is down or unreachable.
    PlatformUnavailable,
    /// An admin turned the platform off.
    PlatformDisabled,
    /// The live-publish kill switch is on.
    LivePublishDisabled,
    /// A quota bucket is used up.
    QuotaExceeded,
    /// Reconciliation could not tell whether the post went out; the user must decide.
    OutcomeUnresolvable,
}

/// Where a delivery is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "state", content = "reason", rename_all = "snake_case")]
pub enum DeliveryState {
    /// Waiting for its time or a worker.
    Pending,
    /// A worker holds the lease and has not called the platform yet.
    Claimed,
    /// A platform call is under way.
    InFlight,
    /// The platform accepted the post and is still processing it.
    Processing,
    /// A call may have changed the platform and its effect is unknown.
    OutcomeUnknown,
    /// Waiting for something outside the engine.
    Parked(ParkReason),
    /// Live on the platform.
    Published,
    /// Failed for good.
    Failed,
    /// Retries ran out.
    DeadLetter,
    /// Cancelled by the user.
    Cancelled,
    /// A dry run completed.
    Simulated,
}

impl DeliveryState {
    /// Whether no automatic transition leaves this state. A user can still retry or resolve
    /// `Failed` and `DeadLetter`.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Published | Self::Failed | Self::DeadLetter | Self::Cancelled | Self::Simulated
        )
    }

    /// The state's name as stored in the database. A parked delivery's reason is stored apart.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Claimed => "claimed",
            Self::InFlight => "in_flight",
            Self::Processing => "processing",
            Self::OutcomeUnknown => "outcome_unknown",
            Self::Parked(_) => "parked",
            Self::Published => "published",
            Self::Failed => "failed",
            Self::DeadLetter => "dead_letter",
            Self::Cancelled => "cancelled",
            Self::Simulated => "simulated",
        }
    }
}

/// How a user settles a delivery whose outcome nobody can verify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ResolvedOutcome {
    /// The user confirmed the post is live.
    Published,
    /// The user confirmed the post is not live and will not be retried.
    Failed,
}

impl From<ResolvedOutcome> for DeliveryState {
    fn from(outcome: ResolvedOutcome) -> Self {
        match outcome {
            ResolvedOutcome::Published => Self::Published,
            ResolvedOutcome::Failed => Self::Failed,
        }
    }
}

/// Something that happened to a delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeliveryEvent {
    /// A worker took the lease.
    Claim,
    /// The user cancelled. Cancelling `Parked(OutcomeUnresolvable)` leaves it unknown whether the
    /// post is live, so the API confirms that with the user first.
    Cancel,
    /// The worker is about to call the platform.
    StartRemote,
    /// Something outside the engine blocks the delivery.
    Park(ParkReason),
    /// The checks run just before sending found the post no longer valid.
    RevalidationFailed,
    /// The lease ran out before the worker finished.
    LeaseExpired,
    /// The platform confirmed the post is live. Also reports a late worker's receipt after the
    /// delivery was reconciled (plan 03 "Fenced writes"); see [`Self::ReconcileFound`].
    Published,
    /// The platform accepted the post and is processing it.
    Accepted,
    /// A call failed in a way worth retrying.
    RetryableFailure,
    /// The worker is shutting down and releases its lease.
    Shutdown,
    /// Retries ran out.
    RetryBudgetExhausted,
    /// A call failed in a way retries cannot fix.
    PermanentFailure,
    /// A mutating call ended without a verdict.
    Ambiguous,
    /// A dry run completed.
    Simulated,
    /// The platform took too long to finish processing.
    ProcessingTimeout,
    /// Reconciliation found the post on the platform. Also covers a late worker reporting its
    /// receipt after reconciliation already started.
    ReconcileFound,
    /// Reconciliation found no post; publishing again is safe.
    ReconcileAbsent,
    /// Reconciliation could not tell.
    ReconcileUnresolvable,
    /// The blocker went away.
    Unpark,
    /// The user asked to try again. The acknowledgement matters only where the post may already
    /// be live; from other states it is ignored.
    UserRetry {
        /// The user confirmed the post may already be live and chose to risk a duplicate. Needed
        /// from `OutcomeUnknown` and `Parked(OutcomeUnresolvable)`.
        acknowledged_outcome_unknown: bool,
    },
    /// The user settled the delivery by hand.
    UserResolve(ResolvedOutcome),
}

/// Why a transition was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransitionError {
    /// The event cannot happen in that state.
    #[error("{event:?} is not allowed while the delivery is {from:?}")]
    NotAllowed {
        /// The state the delivery is in.
        from: DeliveryState,
        /// The event that was refused.
        event: DeliveryEvent,
    },
    /// Retrying could duplicate a post that is already live; the user must acknowledge that.
    #[error("retrying may duplicate the post; the user must acknowledge that first")]
    AcknowledgementRequired,
}

/// The state a delivery moves to, or why it may not.
///
/// # Errors
/// [`TransitionError::NotAllowed`] when the pair is not in the table (there are no
/// self-transitions), and [`TransitionError::AcknowledgementRequired`] for a user retry
/// without acknowledgement from `OutcomeUnknown` or `Parked(OutcomeUnresolvable)`.
#[allow(clippy::match_same_arms)] // one arm per row of the plan 03 table keeps it checkable
pub fn transition(
    from: DeliveryState,
    event: DeliveryEvent,
) -> Result<DeliveryState, TransitionError> {
    use DeliveryEvent as E;
    use DeliveryState as S;
    use ParkReason::{AccountNeedsReconnect, OutcomeUnresolvable, QuotaExceeded};

    let to = match (from, event) {
        (S::Pending, E::Claim) => S::Claimed,
        (S::Pending, E::Cancel) => S::Cancelled,

        (S::Claimed, E::StartRemote) => S::InFlight,
        (S::Claimed, E::Park(r)) if r != OutcomeUnresolvable => S::Parked(r),
        (S::Claimed, E::RevalidationFailed) => S::Failed,
        (S::Claimed, E::LeaseExpired | E::Shutdown) => S::Pending,
        (S::Claimed, E::Cancel) => S::Cancelled,

        (S::InFlight, E::Published) => S::Published,
        (S::InFlight, E::Park(r @ (QuotaExceeded | AccountNeedsReconnect))) => S::Parked(r),
        (S::InFlight, E::Accepted) => S::Processing,
        (S::InFlight, E::RetryableFailure | E::Shutdown) => S::Pending,
        (S::InFlight, E::RetryBudgetExhausted) => S::DeadLetter,
        (S::InFlight, E::PermanentFailure) => S::Failed,
        (S::InFlight, E::Ambiguous | E::LeaseExpired) => S::OutcomeUnknown,
        (S::InFlight, E::Cancel) => S::Cancelled,
        (S::InFlight, E::Simulated) => S::Simulated,

        (S::Processing, E::Published) => S::Published,
        (S::Processing, E::PermanentFailure) => S::Failed,
        (S::Processing, E::ProcessingTimeout) => S::OutcomeUnknown,
        (S::Processing, E::Cancel) => S::Cancelled,

        (S::OutcomeUnknown, E::ReconcileFound) => S::Published,
        (S::OutcomeUnknown, E::ReconcileAbsent) => S::Pending,
        (S::OutcomeUnknown, E::ReconcileUnresolvable) => S::Parked(OutcomeUnresolvable),
        (
            S::OutcomeUnknown | S::Parked(OutcomeUnresolvable),
            E::UserRetry {
                acknowledged_outcome_unknown: false,
            },
        ) => return Err(TransitionError::AcknowledgementRequired),
        (
            S::OutcomeUnknown | S::Parked(OutcomeUnresolvable),
            E::UserRetry {
                acknowledged_outcome_unknown: true,
            },
        ) => S::Pending,
        (S::OutcomeUnknown | S::Parked(OutcomeUnresolvable), E::UserResolve(o)) => o.into(),

        (S::Parked(r), E::Unpark) if r != OutcomeUnresolvable => S::Pending,
        (S::Parked(r), E::UserRetry { .. }) if r != OutcomeUnresolvable => S::Pending,
        (S::Parked(_), E::Cancel) => S::Cancelled,

        (S::DeadLetter, E::UserRetry { .. }) | (S::Failed, E::UserRetry { .. }) => S::Pending,
        (S::DeadLetter, E::UserResolve(o)) => o.into(),

        _ => return Err(TransitionError::NotAllowed { from, event }),
    };
    Ok(to)
}

/// The status of a publish job, derived from its deliveries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    /// Everything left is waiting for its time.
    Scheduled,
    /// At least one delivery is active, waiting on the platform, or parked.
    Publishing,
    /// Every delivery went out (or was simulated).
    Published,
    /// Every delivery was cancelled.
    Cancelled,
    /// Every delivery ended and none went out.
    Failed,
    /// Every delivery ended and some went out.
    Partial,
}

/// What [`derive_job_status`] needs to know about one delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeliveryView {
    /// Where the delivery is.
    pub state: DeliveryState,
    /// When a pending delivery becomes due.
    pub next_attempt_at: DateTime<Utc>,
    /// Whether the platform will publish it by itself at the scheduled time.
    pub awaiting_native_publish: bool,
}

/// The job's status. An empty slice is `Failed`: a job always has deliveries, so none means
/// something went wrong.
#[must_use]
pub fn derive_job_status(deliveries: &[DeliveryView], now: DateTime<Utc>) -> JobStatus {
    if deliveries.is_empty() {
        return JobStatus::Failed;
    }
    if deliveries.iter().all(|d| d.state.is_terminal()) {
        let went_out = |d: &DeliveryView| {
            matches!(d.state, DeliveryState::Published | DeliveryState::Simulated)
        };
        return if deliveries.iter().all(went_out) {
            JobStatus::Published
        } else if deliveries
            .iter()
            .all(|d| d.state == DeliveryState::Cancelled)
        {
            JobStatus::Cancelled
        } else if !deliveries.iter().any(went_out) {
            JobStatus::Failed
        } else {
            JobStatus::Partial
        };
    }
    let waiting = |d: &DeliveryView| match d.state {
        DeliveryState::Pending => d.next_attempt_at > now,
        DeliveryState::Processing => d.awaiting_native_publish,
        state => state.is_terminal(),
    };
    if deliveries.iter().all(waiting) {
        JobStatus::Scheduled
    } else {
        JobStatus::Publishing
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use chrono::{Duration, TimeZone};
    use serde_json::json;

    use super::DeliveryEvent as E;
    use super::DeliveryState as S;
    use super::ParkReason::{self, OutcomeUnresolvable, QuotaExceeded};
    use super::ResolvedOutcome::{Failed, Published};
    use super::*;

    const REASONS: [ParkReason; 7] = [
        ParkReason::OwnerDisabled,
        ParkReason::AccountNeedsReconnect,
        ParkReason::PlatformUnavailable,
        ParkReason::PlatformDisabled,
        ParkReason::LivePublishDisabled,
        QuotaExceeded,
        OutcomeUnresolvable,
    ];

    fn retry(acknowledged: bool) -> E {
        E::UserRetry {
            acknowledged_outcome_unknown: acknowledged,
        }
    }

    fn all_states() -> Vec<S> {
        let mut states = vec![
            S::Pending,
            S::Claimed,
            S::InFlight,
            S::Processing,
            S::OutcomeUnknown,
            S::Published,
            S::Failed,
            S::DeadLetter,
            S::Cancelled,
            S::Simulated,
        ];
        states.extend(REASONS.map(S::Parked));
        states
    }

    fn all_events() -> Vec<E> {
        let mut events = vec![
            E::Claim,
            E::Cancel,
            E::StartRemote,
            E::RevalidationFailed,
            E::LeaseExpired,
            E::Published,
            E::Accepted,
            E::RetryableFailure,
            E::Shutdown,
            E::RetryBudgetExhausted,
            E::PermanentFailure,
            E::Ambiguous,
            E::Simulated,
            E::ProcessingTimeout,
            E::ReconcileFound,
            E::ReconcileAbsent,
            E::ReconcileUnresolvable,
            E::Unpark,
            retry(true),
            retry(false),
            E::UserResolve(Published),
            E::UserResolve(Failed),
        ];
        events.extend(REASONS.map(E::Park));
        events
    }

    /// The allowed transitions, written out from the plan 03 table.
    fn table() -> Vec<(S, E, S)> {
        let mut rows = vec![
            (S::Pending, E::Claim, S::Claimed),
            (S::Pending, E::Cancel, S::Cancelled),
            (S::Claimed, E::StartRemote, S::InFlight),
            (S::Claimed, E::RevalidationFailed, S::Failed),
            (S::Claimed, E::LeaseExpired, S::Pending),
            (S::Claimed, E::Cancel, S::Cancelled),
            (S::Claimed, E::Shutdown, S::Pending),
            (S::InFlight, E::Published, S::Published),
            (
                S::InFlight,
                E::Park(QuotaExceeded),
                S::Parked(QuotaExceeded),
            ),
            (
                S::InFlight,
                E::Park(ParkReason::AccountNeedsReconnect),
                S::Parked(ParkReason::AccountNeedsReconnect),
            ),
            (S::InFlight, E::Accepted, S::Processing),
            (S::InFlight, E::RetryableFailure, S::Pending),
            (S::InFlight, E::Shutdown, S::Pending),
            (S::InFlight, E::RetryBudgetExhausted, S::DeadLetter),
            (S::InFlight, E::PermanentFailure, S::Failed),
            (S::InFlight, E::Ambiguous, S::OutcomeUnknown),
            (S::InFlight, E::LeaseExpired, S::OutcomeUnknown),
            (S::InFlight, E::Cancel, S::Cancelled),
            (S::InFlight, E::Simulated, S::Simulated),
            (S::Processing, E::Published, S::Published),
            (S::Processing, E::PermanentFailure, S::Failed),
            (S::Processing, E::ProcessingTimeout, S::OutcomeUnknown),
            (S::Processing, E::Cancel, S::Cancelled),
            (S::OutcomeUnknown, E::ReconcileFound, S::Published),
            (S::OutcomeUnknown, E::ReconcileAbsent, S::Pending),
            (
                S::OutcomeUnknown,
                E::ReconcileUnresolvable,
                S::Parked(OutcomeUnresolvable),
            ),
            (S::OutcomeUnknown, retry(true), S::Pending),
            (S::OutcomeUnknown, E::UserResolve(Published), S::Published),
            (S::OutcomeUnknown, E::UserResolve(Failed), S::Failed),
            (S::Parked(OutcomeUnresolvable), retry(true), S::Pending),
            (
                S::Parked(OutcomeUnresolvable),
                E::UserResolve(Published),
                S::Published,
            ),
            (
                S::Parked(OutcomeUnresolvable),
                E::UserResolve(Failed),
                S::Failed,
            ),
            (S::DeadLetter, retry(true), S::Pending),
            (S::DeadLetter, retry(false), S::Pending),
            (S::DeadLetter, E::UserResolve(Published), S::Published),
            (S::DeadLetter, E::UserResolve(Failed), S::Failed),
            (S::Failed, retry(true), S::Pending),
            (S::Failed, retry(false), S::Pending),
        ];
        for r in REASONS {
            rows.push((S::Parked(r), E::Cancel, S::Cancelled));
            if r != OutcomeUnresolvable {
                rows.push((S::Claimed, E::Park(r), S::Parked(r)));
                rows.push((S::Parked(r), E::Unpark, S::Pending));
                rows.push((S::Parked(r), retry(true), S::Pending));
                rows.push((S::Parked(r), retry(false), S::Pending));
            }
        }
        rows
    }

    #[test]
    fn allowed_transitions() {
        for (from, event, to) in table() {
            assert_eq!(transition(from, event), Ok(to), "{from:?} + {event:?}");
        }
    }

    #[test]
    fn everything_else_is_refused() {
        let allowed = table();
        for from in all_states() {
            for event in all_events() {
                if allowed.iter().any(|(f, e, _)| *f == from && *e == event) {
                    continue;
                }
                let needs_ack = matches!(from, S::OutcomeUnknown | S::Parked(OutcomeUnresolvable))
                    && event == retry(false);
                let want = if needs_ack {
                    TransitionError::AcknowledgementRequired
                } else {
                    TransitionError::NotAllowed { from, event }
                };
                assert_eq!(transition(from, event), Err(want), "{from:?} + {event:?}");
            }
        }
    }

    #[test]
    fn retry_without_ack_requires_acknowledgement() {
        for from in [S::OutcomeUnknown, S::Parked(OutcomeUnresolvable)] {
            assert_eq!(
                transition(from, retry(false)),
                Err(TransitionError::AcknowledgementRequired)
            );
            assert_eq!(transition(from, retry(true)), Ok(S::Pending));
        }
    }

    #[test]
    fn parked_outcome_unresolvable_never_unparks() {
        let from = S::Parked(OutcomeUnresolvable);
        assert!(matches!(
            transition(from, E::Unpark),
            Err(TransitionError::NotAllowed { .. })
        ));
        assert_eq!(transition(from, E::UserResolve(Failed)), Ok(S::Failed));
    }

    #[test]
    fn state_wire_format() {
        assert_eq!(
            serde_json::to_value(S::Pending).unwrap(),
            json!({"state": "pending"})
        );
        let parked = S::Parked(QuotaExceeded);
        let value = serde_json::to_value(parked).unwrap();
        assert_eq!(
            value,
            json!({"state": "parked", "reason": "quota_exceeded"})
        );
        assert_eq!(serde_json::from_value::<S>(value).unwrap(), parked);
    }

    #[test]
    fn terminal_states() {
        let terminal: Vec<S> = all_states().into_iter().filter(S::is_terminal).collect();
        assert_eq!(
            terminal,
            vec![
                S::Published,
                S::Failed,
                S::DeadLetter,
                S::Cancelled,
                S::Simulated
            ]
        );
        assert_eq!(S::Parked(QuotaExceeded).as_str(), "parked");
        assert_eq!(S::DeadLetter.as_str(), "dead_letter");
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 18, 12, 0, 0).unwrap()
    }

    fn view(state: S, due_in_secs: i64, native: bool) -> DeliveryView {
        DeliveryView {
            state,
            next_attempt_at: now() + Duration::seconds(due_in_secs),
            awaiting_native_publish: native,
        }
    }

    #[test]
    fn job_status_table() {
        let status = |views: &[DeliveryView]| derive_job_status(views, now());

        assert_eq!(status(&[]), JobStatus::Failed);

        // Every delivery ended.
        assert_eq!(
            status(&[view(S::Published, 0, false), view(S::Simulated, 0, false)]),
            JobStatus::Published
        );
        assert_eq!(
            status(&[view(S::Cancelled, 0, false), view(S::Cancelled, 0, false)]),
            JobStatus::Cancelled
        );
        assert_eq!(
            status(&[view(S::Failed, 0, false), view(S::Cancelled, 0, false)]),
            JobStatus::Failed
        );
        assert_eq!(
            status(&[view(S::DeadLetter, 0, false), view(S::Failed, 0, false)]),
            JobStatus::Failed
        );
        assert_eq!(
            status(&[view(S::Published, 0, false), view(S::Failed, 0, false)]),
            JobStatus::Partial
        );
        assert_eq!(
            status(&[view(S::Published, 0, false), view(S::Cancelled, 0, false)]),
            JobStatus::Partial
        );

        // Something is still open.
        assert_eq!(
            status(&[view(S::Pending, 3600, false), view(S::Pending, 60, false)]),
            JobStatus::Scheduled
        );
        assert_eq!(
            status(&[view(S::Pending, 3600, false), view(S::Published, 0, false)]),
            JobStatus::Scheduled
        );
        assert_eq!(status(&[view(S::Pending, 0, false)]), JobStatus::Publishing);
        assert_eq!(
            status(&[view(S::Pending, -5, false), view(S::Pending, 3600, false)]),
            JobStatus::Publishing
        );
        assert_eq!(
            status(&[view(S::Processing, 0, true), view(S::Pending, 3600, false)]),
            JobStatus::Scheduled
        );
        assert_eq!(
            status(&[view(S::Processing, 0, false)]),
            JobStatus::Publishing
        );
        for open in [
            S::Claimed,
            S::InFlight,
            S::OutcomeUnknown,
            S::Parked(QuotaExceeded),
        ] {
            assert_eq!(
                status(&[view(open, 3600, false), view(S::Pending, 3600, false)]),
                JobStatus::Publishing,
                "{open:?}"
            );
        }
    }
}
