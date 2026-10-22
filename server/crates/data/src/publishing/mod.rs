//! Publish jobs, deliveries, and the queries the publishing engine and REST services run.
//!
//! Nothing here enqueues work: functions that create it return the IDs and run times the
//! caller passes to `JobQueue::enqueue_in` in the same transaction.

use chrono::{DateTime, Utc};
use postit_core::DeliveryId;

pub mod jobs;

pub use jobs::{
    CreatedJob, JobFilter, JobRepo, NewDelivery, NewJob, Operation, PublishJob,
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
