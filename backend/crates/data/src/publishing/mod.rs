//! Publish jobs, deliveries, and the queries the publishing engine and REST services run.
//!
//! Nothing here enqueues work: functions that create it return the IDs and run times the
//! caller passes to `JobQueue::enqueue_in` in the same transaction.
//!
//! # Which job calls what
//!
//! | D1 job | Calls |
//! | --- | --- |
//! | `publish_delivery` | [`DeliveryRepo::claim`], for a direct delivery [`direct::DirectRepo::materialize`] or, in a dry run, [`direct::DirectRepo::audience_count`], [`DeliveryRepo::renew_lease`], [`DeliveryRepo::apply`], [`DeliveryRepo::set_container_ref`], [`DeliveryRepo::record_late_receipt`], [`AttemptRepo`], [`JobRepo::delivery_counts`] |
//! | `poll_delivery_status`, `reconcile_delivery` | [`DeliveryRepo::check_target`], [`DeliveryRepo::take_check`], [`DeliveryRepo::set_check_due`], [`DeliveryRepo::apply`], [`DeliveryRepo::record_late_receipt`], [`JobRepo::snapshot`] |
//! | `delivery_due_sweep` | [`sweep::Sweep`] (`overdue_pending`, `expired_claims`, `expired_in_flight`, `orphaned_direct`, `unpark`, `stalled_checks`), and [`DeliveryRepo::check_target`] for the notices; passes `publishing.sweep_grace` to `overdue_pending`, `orphaned_direct`, and `stalled_checks` |
//! | `account_preflight` | [`sweep::Sweep::preflight_accounts`], then `AccountService::check` per account |
//! | `direct_send_batch` | [`direct::DirectRepo::claim_batch`] (stops on `BatchClaim::Stale`), [`direct::DirectRepo::renew_batch`], [`direct::DirectRepo::recipients`], [`direct::DirectRepo::record`], [`direct::DirectRepo::requeue`], [`direct::DirectRepo::release_batch`], [`direct::DirectRepo::wait_for_cap`] (when a claim returns `CapReached`), [`direct::DirectRepo::finish_if_done`] |
//! | `recurrence_tick` | [`crate::recurrences::RecurrenceRepo::due`], [`JobRepo::create_for_occurrence`], [`crate::recurrences::RecurrenceRepo::advance`] |

use chrono::{DateTime, Utc};
use postit_core::DeliveryId;

pub mod deliveries;
pub mod direct;
pub mod jobs;
pub mod quota;
pub mod sweep;

pub use deliveries::{
    Applied, AttemptRepo, CancelOutcome, CancelResult, CheckTarget, ClaimedDelivery, Delivery,
    DeliveryRepo, Fence, JobStatusChange, LeaseRenewal, Receipt, TransitionData,
};
pub use jobs::{
    CreatedJob, DeliveryCounts, JobFilter, JobRepo, NewDelivery, NewJob, Operation, PublishJob,
    Rescheduled, delivery_idempotency_key,
};

/// A delivery to enqueue: the generation the queued job must carry and when to run it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dispatch {
    /// The delivery to run.
    pub delivery: DeliveryId,
    /// The generation the queued job carries; a claim with any other generation finds nothing.
    pub dispatch_gen: i32,
    /// When the queued job becomes due.
    pub run_at: DateTime<Utc>,
}
