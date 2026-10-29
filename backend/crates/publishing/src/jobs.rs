//! The publishing jobs: payloads, registration, and console specs.
//!
//! Every job runs on [`Queue::Publishing`] with [`RetryPolicy::None`]: the delivery state
//! machine owns retries, so the console offers neither retry nor cancel
//! ([`DisabledReason::PublishingManaged`]). Payloads hold IDs and counters only.

use postit_config::JobSchedules;
use postit_data::publishing::Dispatch;
use postit_jobs::{
    ConsoleCatalog, ConsoleSpec, DisabledReason, Job, JobContext, JobRegistry, JobsError, Queue,
    RetryPolicy,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::PublishingService;

/// Publishes one delivery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishDelivery {
    /// The delivery.
    pub delivery: Uuid,
    /// The generation the queued job carries; a claim with any other generation finds nothing.
    pub dispatch_gen: i32,
}

impl Job for PublishDelivery {
    const JOB_TYPE: &'static str = "publish_delivery";
    const QUEUE: Queue = Queue::Publishing;
}

/// Asks the platform whether a processing delivery is live yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PollDeliveryStatus {
    /// The delivery.
    pub delivery: Uuid,
    /// The generation the delivery was dispatched with.
    pub dispatch_gen: i32,
    /// How many polls came before this one.
    pub poll: i32,
}

impl Job for PollDeliveryStatus {
    const JOB_TYPE: &'static str = "poll_delivery_status";
    const QUEUE: Queue = Queue::Publishing;
}

/// Finds out whether an ambiguous publish landed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReconcileDelivery {
    /// The delivery.
    pub delivery: Uuid,
    /// The generation the delivery was dispatched with.
    pub dispatch_gen: i32,
    /// How many reconcile rounds came before this one.
    pub attempt: i32,
}

impl Job for ReconcileDelivery {
    const JOB_TYPE: &'static str = "reconcile_delivery";
    const QUEUE: Queue = Queue::Publishing;
}

/// Sends one batch of a direct (email) delivery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectSendBatch {
    /// The delivery.
    pub delivery: Uuid,
    /// The generation the delivery was dispatched with.
    pub dispatch_gen: i32,
}

impl Job for DirectSendBatch {
    const JOB_TYPE: &'static str = "direct_send_batch";
    const QUEUE: Queue = Queue::Publishing;
}

/// The recurring sweep that resumes, reclaims, and flags due deliveries.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryDueSweep {}

impl Job for DeliveryDueSweep {
    const JOB_TYPE: &'static str = "delivery_due_sweep";
    const QUEUE: Queue = Queue::Publishing;
}

/// The recurring check that connected accounts still work.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountPreflight {}

impl Job for AccountPreflight {
    const JOB_TYPE: &'static str = "account_preflight";
    const QUEUE: Queue = Queue::Publishing;
}

impl From<Dispatch> for PublishDelivery {
    fn from(dispatch: Dispatch) -> Self {
        Self {
            delivery: dispatch.delivery.as_uuid(),
            dispatch_gen: dispatch.dispatch_gen,
        }
    }
}

/// The first poll of a processing delivery.
impl From<Dispatch> for PollDeliveryStatus {
    fn from(dispatch: Dispatch) -> Self {
        Self {
            delivery: dispatch.delivery.as_uuid(),
            dispatch_gen: dispatch.dispatch_gen,
            poll: 0,
        }
    }
}

/// The first reconcile round of an ambiguous delivery.
impl From<Dispatch> for ReconcileDelivery {
    fn from(dispatch: Dispatch) -> Self {
        Self {
            delivery: dispatch.delivery.as_uuid(),
            dispatch_gen: dispatch.dispatch_gen,
            attempt: 0,
        }
    }
}

impl From<Dispatch> for DirectSendBatch {
    fn from(dispatch: Dispatch) -> Self {
        Self {
            delivery: dispatch.delivery.as_uuid(),
            dispatch_gen: dispatch.dispatch_gen,
        }
    }
}

/// Registers the six publishing jobs. `publish_delivery` runs [`crate::worker::run_publish`],
/// `poll_delivery_status` [`crate::poll::run_poll`], `reconcile_delivery`
/// [`crate::reconcile::run_reconcile`], `direct_send_batch` [`crate::direct::run_batch`],
/// `delivery_due_sweep` [`crate::sweep::run_sweep`], and `account_preflight`
/// [`crate::preflight::run_preflight`].
///
/// # Errors
///
/// [`JobsError`] when a schedule is invalid or a job type is already registered.
pub fn register(
    registry: &mut JobRegistry,
    service: PublishingService,
    schedules: &JobSchedules,
) -> Result<(), JobsError> {
    let publishes = service.clone();
    registry.register(
        RetryPolicy::None,
        move |job: PublishDelivery, _: JobContext| {
            let service = publishes.clone();
            async move { crate::worker::run_publish(&service, job).await }
        },
    )?;
    let polls = service.clone();
    registry.register(
        RetryPolicy::None,
        move |job: PollDeliveryStatus, _: JobContext| {
            let service = polls.clone();
            async move { crate::poll::run_poll(&service, job).await }
        },
    )?;
    let reconciles = service.clone();
    registry.register(
        RetryPolicy::None,
        move |job: ReconcileDelivery, _: JobContext| {
            let service = reconciles.clone();
            async move { crate::reconcile::run_reconcile(&service, job).await }
        },
    )?;
    let batches = service.clone();
    registry.register(
        RetryPolicy::None,
        move |job: DirectSendBatch, _: JobContext| {
            let service = batches.clone();
            async move { crate::direct::run_batch(&service, job).await }
        },
    )?;
    let sweeps = service.clone();
    registry.register_recurring(
        &schedules.delivery_due_sweep,
        RetryPolicy::None,
        move |_: DeliveryDueSweep, _: JobContext| {
            let service = sweeps.clone();
            async move { crate::sweep::run_sweep(&service).await.map(|_| ()) }
        },
    )?;
    let preflights = service;
    registry.register_recurring(
        &schedules.account_preflight,
        RetryPolicy::None,
        move |_: AccountPreflight, _: JobContext| {
            let service = preflights.clone();
            async move { crate::preflight::run_preflight(&service).await.map(|_| ()) }
        },
    )
}

/// Describes the publishing jobs to the admin console: no retry or cancel, because the
/// delivery state machine owns both.
pub fn console(catalog: &mut ConsoleCatalog, schedules: &JobSchedules) {
    let managed = || {
        ConsoleSpec::new()
            .id_field("delivery", "delivery_id")
            .disabled(DisabledReason::PublishingManaged)
    };
    catalog.add::<PublishDelivery>(managed());
    catalog.add::<PollDeliveryStatus>(managed());
    catalog.add::<ReconcileDelivery>(managed());
    catalog.add::<DirectSendBatch>(managed());
    catalog.add::<DeliveryDueSweep>(
        ConsoleSpec::new()
            .disabled(DisabledReason::PublishingManaged)
            .recurring(&schedules.delivery_due_sweep),
    );
    catalog.add::<AccountPreflight>(
        ConsoleSpec::new()
            .disabled(DisabledReason::PublishingManaged)
            .recurring(&schedules.account_preflight),
    );
}
