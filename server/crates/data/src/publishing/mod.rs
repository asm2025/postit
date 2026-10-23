//! Publish jobs, deliveries, and the queries the publishing engine and REST services run.
//!
//! Nothing here enqueues work: functions that create it return the IDs and run times the
//! caller passes to `JobQueue::enqueue_in` in the same transaction.
//!
//! # Which job calls what
//!
//! | D1 job | Calls |
//! | --- | --- |
//! | `publish_delivery` | [`DeliveryRepo::claim`], [`DeliveryRepo::apply`], [`DeliveryRepo::set_container_ref`] |
//! | `delivery_due_sweep` | [`sweep::Sweep`] (`overdue_pending`, `expired_claims`, `expired_in_flight`, `orphaned_direct`, `unpark`) |
//! | `direct_send_batch` | [`direct::DirectRepo::claim_batch`], [`direct::DirectRepo::record`], [`direct::DirectRepo::finish_if_done`] |
//! | `recurrence_tick` | [`crate::recurrences::RecurrenceRepo::due`], [`JobRepo::create_for_occurrence`], [`crate::recurrences::RecurrenceRepo::advance`] |

use chrono::{DateTime, Utc};
use postit_core::DeliveryId;

pub mod deliveries;
pub mod direct;
pub mod jobs;
pub mod quota;
pub mod sweep;

pub use deliveries::{
    Applied, AttemptRepo, CancelOutcome, ClaimedDelivery, Delivery, DeliveryRepo, Fence, Receipt,
    TransitionData,
};
pub use jobs::{
    CreatedJob, JobFilter, JobRepo, NewDelivery, NewJob, Operation, PublishJob, Rescheduled,
    delivery_idempotency_key,
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
