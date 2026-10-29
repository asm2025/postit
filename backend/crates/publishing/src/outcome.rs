//! What a publish call's result does to its delivery: the event, the payload columns, the work
//! that follows, and the notice the owner gets.

use chrono::{DateTime, TimeDelta, Utc};
use postit_core::{
    AgentDescriptor, AgentError, DeliveryEvent, NotificationKind, ParkReason, PublishOutcome,
    PublishReceipt, RetryClass,
};
use postit_data::publishing::TransitionData;
use serde_json::{Value, json};

use crate::backoff;

/// The longest a platform's `Retry-After` may delay a retry. A longer one (the header is
/// parsed as uncapped seconds) is cut to this: a day is longer than any real rate limit, and
/// a delivery that waits longer belongs to the owner, not to the retry loop.
pub const MAX_RETRY_AFTER: std::time::Duration = std::time::Duration::from_hours(24);

/// Why a running publish was asked to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelCause {
    /// Nobody asked.
    None,
    /// The owner cancelled the delivery.
    User,
    /// The server is shutting down.
    Shutdown,
    /// The lease is gone: the sweep or another worker owns the delivery.
    LeaseLost,
}

/// What the worker does after the result is written, in the same transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowUp {
    /// Nothing.
    None,
    /// Enqueue the first status poll at `at`.
    Poll {
        /// When the poll runs.
        at: DateTime<Utc>,
    },
    /// Enqueue the first reconcile round.
    Reconcile,
    /// Enqueue the delivery's next run (the write's redispatch).
    Redispatch,
    /// Mark the account's grant as needing a reconnect.
    MarkGrant,
}

/// A result mapped onto the delivery.
#[derive(Debug, Clone)]
pub struct Mapped {
    /// The event to apply from `in_flight`.
    pub event: DeliveryEvent,
    /// The payload columns it writes.
    pub data: TransitionData,
    /// What follows in the same transaction.
    pub follow_up: FollowUp,
    /// The notice a live delivery records, if any.
    pub notice: Option<NotificationKind>,
}

impl Mapped {
    fn new(event: DeliveryEvent, data: TransitionData) -> Self {
        Self {
            event,
            data,
            follow_up: FollowUp::None,
            notice: None,
        }
    }

    fn follow(mut self, follow_up: FollowUp) -> Self {
        self.follow_up = follow_up;
        self
    }

    fn notify(mut self, notice: NotificationKind) -> Self {
        self.notice = Some(notice);
        self
    }
}

/// Maps a publish result onto the delivery that is `in_flight` after `attempt_count` counted
/// attempts. `None` means nothing may be written: the lease was lost and the sweep owns the
/// delivery, so any write would be stale.
///
/// A `Quota` error without `resets_at` parks until the next period of the descriptor's bucket
/// it names, or the next midnight UTC for a bucket the descriptor does not declare, so a quota
/// park always has a time to resume.
#[must_use]
pub fn map(
    result: &Result<PublishOutcome, AgentError>,
    attempt_count: i32,
    max_attempts: u32,
    now: DateTime<Utc>,
    cancel: CancelCause,
    descriptor: &AgentDescriptor,
) -> Option<Mapped> {
    let error = match result {
        Ok(PublishOutcome::Published(receipt)) => {
            return Some(
                Mapped::new(DeliveryEvent::Published, receipt_data(receipt))
                    .notify(NotificationKind::DeliveryPublished),
            );
        }
        Ok(PublishOutcome::Processing(receipt)) => {
            let first_poll = later(now, backoff::poll_delay(0));
            let at = receipt
                .remote_publish_at
                .map_or(first_poll, |live| live.max(first_poll));
            // The first poll's time is the chain's backstop.
            return Some(
                Mapped::new(
                    DeliveryEvent::Accepted,
                    TransitionData {
                        check_due_at: Some(at),
                        ..receipt_data(receipt)
                    },
                )
                .follow(FollowUp::Poll { at }),
            );
        }
        Ok(PublishOutcome::Simulated { .. }) => {
            return Some(Mapped::new(
                DeliveryEvent::Simulated,
                TransitionData::default(),
            ));
        }
        Err(error) => error,
    };
    let class = error.retry_class();
    let failed = |count_attempt| TransitionData {
        last_error: Some(last_error(error)),
        count_attempt,
        ..TransitionData::default()
    };
    let retry = |delay: std::time::Duration| {
        if i64::from(attempt_count) + 1 >= i64::from(max_attempts) {
            return Mapped::new(DeliveryEvent::RetryBudgetExhausted, failed(true))
                .notify(NotificationKind::DeliveryDeadLettered);
        }
        Mapped::new(
            DeliveryEvent::RetryableFailure,
            TransitionData {
                next_attempt_at: Some(later(now, delay)),
                ..failed(true)
            },
        )
        .follow(FollowUp::Redispatch)
    };
    let mapped = match class {
        RetryClass::Retryable => retry(backoff::retry_delay(attempt_count)),
        RetryClass::RetryAfter(after) => retry(after.min(MAX_RETRY_AFTER)),
        RetryClass::Permanent => Mapped::new(DeliveryEvent::PermanentFailure, failed(false))
            .notify(NotificationKind::DeliveryFailed),
        RetryClass::NeedsReconnect => Mapped::new(
            DeliveryEvent::Park(ParkReason::AccountNeedsReconnect),
            failed(false),
        )
        .follow(FollowUp::MarkGrant)
        .notify(NotificationKind::DeliveryParked),
        RetryClass::QuotaExceeded => {
            let parked_until = match error {
                AgentError::Quota {
                    resets_at: Some(at),
                    ..
                } => *at,
                AgentError::Quota { bucket, .. } => bucket_reset(descriptor, bucket, now),
                _ => next_utc_midnight(now),
            };
            Mapped::new(
                DeliveryEvent::Park(ParkReason::QuotaExceeded),
                TransitionData {
                    parked_until: Some(parked_until),
                    ..failed(false)
                },
            )
            .notify(NotificationKind::DeliveryParked)
        }
        RetryClass::Ambiguous => Mapped::new(DeliveryEvent::Ambiguous, failed(true))
            .follow(FollowUp::Reconcile)
            .notify(NotificationKind::DeliveryOutcomeUnknown),
        RetryClass::Cancelled => match cancel {
            CancelCause::LeaseLost => return None,
            CancelCause::User => Mapped::new(DeliveryEvent::Cancel, TransitionData::default()),
            CancelCause::Shutdown => {
                Mapped::new(DeliveryEvent::Shutdown, TransitionData::default())
                    .follow(FollowUp::Redispatch)
            }
            // The agent stopped without being asked: retry it like any transient failure.
            CancelCause::None => retry(backoff::retry_delay(attempt_count)),
        },
    };
    Some(mapped)
}

/// When to ask again after `error`, if the platform said: `now` plus its `Retry-After`, capped
/// at [`MAX_RETRY_AFTER`]. It wins over the computed delay.
#[must_use]
pub fn retry_after_at(error: &AgentError, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    match error.retry_class() {
        RetryClass::RetryAfter(after) => Some(later(now, after.min(MAX_RETRY_AFTER))),
        _ => None,
    }
}

/// The error as `last_error` keeps it: its code and retry class, never the platform's text.
#[must_use]
pub fn last_error(error: &AgentError) -> Value {
    json!({"code": error.code(), "class": error.retry_class().as_str()})
}

/// The payload columns a receipt writes.
pub(crate) fn receipt_data(receipt: &PublishReceipt) -> TransitionData {
    TransitionData {
        remote_id: receipt.remote_id.clone(),
        remote_url: receipt.remote_url.as_ref().map(ToString::to_string),
        remote_publish_at: receipt.remote_publish_at,
        ..TransitionData::default()
    }
}

/// When the descriptor's `bucket` next resets, or the next midnight UTC for an unknown one.
fn bucket_reset(descriptor: &AgentDescriptor, bucket: &str, now: DateTime<Utc>) -> DateTime<Utc> {
    descriptor
        .quota_buckets
        .iter()
        .find(|b| b.id == bucket)
        .map_or_else(
            || next_utc_midnight(now),
            |b| crate::quota::period(b, now).1,
        )
}

fn next_utc_midnight(now: DateTime<Utc>) -> DateTime<Utc> {
    now.date_naive()
        .succ_opt()
        .and_then(|day| day.and_hms_opt(0, 0, 0))
        .map_or_else(
            || later(now, std::time::Duration::from_hours(24)),
            |midnight| midnight.and_utc(),
        )
}

/// `now + duration`, saturating at the latest time chrono represents, so no duration (least of
/// all one a platform sent) can overflow.
pub(crate) fn later(now: DateTime<Utc>, duration: std::time::Duration) -> DateTime<Utc> {
    TimeDelta::from_std(duration)
        .ok()
        .and_then(|delta| now.checked_add_signed(delta))
        .unwrap_or(DateTime::<Utc>::MAX_UTC)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use postit_core::{QuotaBucket, QuotaCosts, QuotaPeriod, QuotaScope};

    use super::*;

    fn descriptor(buckets: Vec<QuotaBucket>) -> AgentDescriptor {
        use postit_core::Agent as _;
        let mut descriptor = postit_agent_mock::MockAgent::broadcast()
            .descriptor()
            .clone();
        descriptor.quota_buckets = buckets;
        descriptor
    }

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn quota(resets_at: Option<DateTime<Utc>>, bucket: &str) -> Result<PublishOutcome, AgentError> {
        Err(AgentError::Quota {
            bucket: bucket.to_owned(),
            resets_at,
        })
    }

    #[test]
    fn quota_without_reset_time_parks_until_the_buckets_next_period() {
        let now = utc("2026-10-29T22:30:00Z");
        let riyadh = QuotaBucket {
            id: "posts".to_owned(),
            scope: QuotaScope::Account,
            period: QuotaPeriod::Day,
            default_limit: 10,
            reset_time_zone: "Asia/Riyadh".to_owned(),
            costs: QuotaCosts {
                publish: 1,
                comment: 0,
                inbox_poll: 0,
                metrics: 0,
            },
        };
        let d = descriptor(vec![riyadh]);

        let known = map(&quota(None, "posts"), 0, 5, now, CancelCause::None, &d).unwrap();
        assert_eq!(known.event, DeliveryEvent::Park(ParkReason::QuotaExceeded));
        assert_eq!(known.data.parked_until, Some(utc("2026-10-30T21:00:00Z")));
        assert_eq!(known.data.next_attempt_at, None, "the due time stays");

        let unknown = map(&quota(None, "other"), 0, 5, now, CancelCause::None, &d).unwrap();
        assert_eq!(unknown.data.parked_until, Some(utc("2026-10-30T00:00:00Z")));

        let given = utc("2026-11-01T00:00:00Z");
        let told = map(
            &quota(Some(given), "posts"),
            0,
            5,
            now,
            CancelCause::None,
            &d,
        )
        .unwrap();
        assert_eq!(told.data.parked_until, Some(given));
    }

    #[test]
    fn retry_budget_counts_this_attempt() {
        let d = descriptor(Vec::new());
        let now = Utc::now();
        let transient = Err(AgentError::Transport {
            message: "timed out".to_owned(),
        });
        let early = map(&transient, 0, 2, now, CancelCause::None, &d).unwrap();
        assert_eq!(early.event, DeliveryEvent::RetryableFailure);
        assert!(early.data.count_attempt);
        assert_eq!(early.follow_up, FollowUp::Redispatch);
        let last = map(&transient, 1, 2, now, CancelCause::None, &d).unwrap();
        assert_eq!(last.event, DeliveryEvent::RetryBudgetExhausted);
        assert!(last.data.count_attempt);
        assert_eq!(last.notice, Some(NotificationKind::DeliveryDeadLettered));
    }

    #[test]
    fn cancelled_follows_its_cause() {
        let d = descriptor(Vec::new());
        let now = Utc::now();
        let cancelled = Err(AgentError::Cancelled);
        assert!(map(&cancelled, 0, 5, now, CancelCause::LeaseLost, &d).is_none());
        let user = map(&cancelled, 0, 5, now, CancelCause::User, &d).unwrap();
        assert_eq!(user.event, DeliveryEvent::Cancel);
        assert_eq!(user.notice, None);
        let shutdown = map(&cancelled, 0, 5, now, CancelCause::Shutdown, &d).unwrap();
        assert_eq!(shutdown.event, DeliveryEvent::Shutdown);
        assert!(!shutdown.data.count_attempt);
        assert_eq!(shutdown.follow_up, FollowUp::Redispatch);
    }

    #[test]
    fn processing_polls_no_earlier_than_a_native_publish_time() {
        let d = descriptor(Vec::new());
        let now = utc("2026-10-29T10:00:00Z");
        let receipt = |at| PublishReceipt {
            remote_id: Some("v1".to_owned()),
            remote_url: None,
            container_ref: None,
            remote_publish_at: at,
        };
        let soon = map(
            &Ok(PublishOutcome::Processing(receipt(None))),
            0,
            5,
            now,
            CancelCause::None,
            &d,
        )
        .unwrap();
        assert_eq!(
            soon.follow_up,
            FollowUp::Poll {
                at: utc("2026-10-29T10:00:15Z")
            }
        );
        let later = utc("2026-10-29T12:00:00Z");
        let native = map(
            &Ok(PublishOutcome::Processing(receipt(Some(later)))),
            0,
            5,
            now,
            CancelCause::None,
            &d,
        )
        .unwrap();
        assert_eq!(native.follow_up, FollowUp::Poll { at: later });
        assert_eq!(native.data.remote_publish_at, Some(later));
        assert_eq!(
            native.data.check_due_at,
            Some(later),
            "the backstop is the first poll"
        );
    }

    #[test]
    fn huge_retry_after_is_capped_and_never_overflows() {
        let d = descriptor(Vec::new());
        let now = utc("2026-10-29T10:00:00Z");
        // `u64::MAX` seconds, and one just past chrono's range (about 2^63 milliseconds).
        for secs in [u64::MAX, i64::MAX.unsigned_abs() / 1000 + 1] {
            let limited = Err(AgentError::RateLimited {
                retry_after: Some(std::time::Duration::from_secs(secs)),
            });
            let mapped = map(&limited, 0, 5, now, CancelCause::None, &d).unwrap();
            assert_eq!(mapped.event, DeliveryEvent::RetryableFailure);
            assert_eq!(
                mapped.data.next_attempt_at,
                Some(utc("2026-10-30T10:00:00Z")),
                "{secs}"
            );
        }
    }

    #[test]
    fn later_saturates_at_the_end_of_time() {
        let end = DateTime::<Utc>::MAX_UTC;
        assert_eq!(later(end, std::time::Duration::from_secs(1)), end);
        assert_eq!(later(Utc::now(), std::time::Duration::MAX), end);
    }

    #[test]
    fn last_error_keeps_codes_only() {
        let error = AgentError::Remote {
            status: Some(400),
            code: Some("rejected".to_owned()),
            message: "a secret-looking platform message".to_owned(),
        };
        assert_eq!(
            last_error(&error),
            json!({"code": "remote", "class": "permanent"})
        );
    }
}
