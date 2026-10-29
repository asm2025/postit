//! The `delivery_due_sweep` handler: finds deliveries nobody is working on and hands each to
//! the job that moves it on.
//!
//! Each duty runs in its own transaction: the [`Sweep`] query runs inside it (its own `begin`
//! becomes a savepoint), the jobs it calls for are enqueued in it, and the notices its writes
//! imply are recorded in it, so a dispatch, its job, and its notices commit together or not at
//! all. No duty calls a platform.

use chrono::{DateTime, TimeDelta, Utc};
use postit_core::{DeliveryId, DeliveryState, Notice, NotificationKind};
use postit_data::publishing::DeliveryRepo;
use postit_data::publishing::sweep::{StalledCheck, Sweep, Swept, UnparkInput};
use postit_jobs::{Job, JobError};
use sqlx::PgConnection;

use crate::notices::{self, NoticeSubject};
use crate::worker::{self, data, enqueue, notify, sql};
use crate::{
    DirectSendBatch, PollDeliveryStatus, PublishDelivery, PublishingService, ReconcileDelivery,
};

/// The most deliveries one duty takes in one sweep; the next sweep, a minute later, takes the
/// rest.
pub const SWEEP_LIMIT: i64 = 500;

/// What one sweep did, by duty.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SweepReport {
    /// Overdue `pending` deliveries re-dispatched with a new generation.
    pub requeued: usize,
    /// `claimed` deliveries whose lease ran out, back in `pending`.
    pub reclaimed: usize,
    /// `in_flight` deliveries whose lease ran out, now `outcome_unknown` with a reconcile job.
    pub unknown: usize,
    /// Direct deliveries whose batch job is gone, with a new one.
    pub orphaned_direct: usize,
    /// Parked deliveries whose cause cleared inside `park_grace`, back in `pending`.
    pub unparked: usize,
    /// Parked deliveries whose cause cleared too late; the owner was asked.
    pub stale: usize,
    /// Poll and reconcile chains whose next job was lost, restarted.
    pub restarted: usize,
}

/// Runs one `delivery_due_sweep`, every duty in turn, with [`SWEEP_LIMIT`] per duty:
///
/// - overdue `pending` deliveries (`sweep_grace`) and `claimed` ones whose lease ran out get a
///   `publish_delivery` job with their new generation;
/// - `in_flight` ones whose lease ran out become `outcome_unknown`, counting an attempt, and
///   get a `reconcile_delivery` job with their generation (a multi-step agent resumes from
///   `container_ref`), and the owner is told the outcome is unknown;
/// - direct deliveries whose batch job is gone get a `direct_send_batch` job;
/// - parked deliveries whose cause cleared within `park_grace` of their due time get a
///   `publish_delivery` job, and those whose cause cleared later record `delivery_stale` once;
/// - poll and reconcile chains whose next job is more than `sweep_grace` overdue get that step
///   enqueued again, which the chain's `take_check` admits once.
///
/// A write that first makes a live job `partial` records `job_partial`, as the worker's do.
///
/// # Errors
///
/// [`JobError::Retry`] on a database failure; the duties that committed before it stay done,
/// and the next sweep picks up the rest.
pub async fn run_sweep(service: &PublishingService) -> Result<SweepReport, JobError> {
    let deps = &service.deps;
    let now = deps.clock.now();
    let sweep_grace = grace(deps.settings.sweep_grace)?;
    let park_grace = grace(deps.settings.park_grace)?;
    let mut report = SweepReport::default();

    let mut tx = deps.pool.begin().await.map_err(sql)?;
    let overdue = Sweep::overdue_pending(&mut tx, now, sweep_grace, SWEEP_LIMIT)
        .await
        .map_err(data)?;
    for dispatch in &overdue {
        put(
            service,
            &mut tx,
            &PublishDelivery::from(*dispatch),
            dispatch.run_at,
        )
        .await?;
    }
    tx.commit().await.map_err(sql)?;
    report.requeued = overdue.len();

    let mut tx = deps.pool.begin().await.map_err(sql)?;
    let claims = Sweep::expired_claims(&mut tx, now, SWEEP_LIMIT)
        .await
        .map_err(data)?;
    report.reclaimed = redispatch(service, &mut tx, &claims).await?;
    tx.commit().await.map_err(sql)?;

    let mut tx = deps.pool.begin().await.map_err(sql)?;
    let in_flight = Sweep::expired_in_flight(&mut tx, now, SWEEP_LIMIT)
        .await
        .map_err(data)?;
    for swept in &in_flight {
        if swept.state == DeliveryState::OutcomeUnknown {
            let dispatch = swept.dispatch();
            put(
                service,
                &mut tx,
                &ReconcileDelivery::from(dispatch),
                dispatch.run_at,
            )
            .await?;
            report.unknown += 1;
        }
        tell(
            service,
            &mut tx,
            swept,
            Some(NotificationKind::DeliveryOutcomeUnknown),
        )
        .await?;
    }
    tx.commit().await.map_err(sql)?;

    let mut tx = deps.pool.begin().await.map_err(sql)?;
    let orphans = Sweep::orphaned_direct(&mut tx, now, sweep_grace, SWEEP_LIMIT)
        .await
        .map_err(data)?;
    for dispatch in &orphans {
        put(
            service,
            &mut tx,
            &DirectSendBatch::from(*dispatch),
            dispatch.run_at,
        )
        .await?;
    }
    tx.commit().await.map_err(sql)?;
    report.orphaned_direct = orphans.len();

    let mut tx = deps.pool.begin().await.map_err(sql)?;
    let unparked = Sweep::unpark(
        &mut tx,
        UnparkInput {
            now,
            park_grace,
            compiled_platforms: &deps.compiled_platforms,
            limit: SWEEP_LIMIT,
        },
    )
    .await
    .map_err(data)?;
    report.unparked = redispatch(service, &mut tx, &unparked.resumed).await?;
    for (_, delivery) in &unparked.needs_owner {
        stale(service, &mut tx, *delivery).await?;
    }
    tx.commit().await.map_err(sql)?;
    report.stale = unparked.needs_owner.len();

    let mut tx = deps.pool.begin().await.map_err(sql)?;
    let stalled = Sweep::stalled_checks(&mut tx, now, sweep_grace, SWEEP_LIMIT)
        .await
        .map_err(data)?;
    for check in &stalled {
        restart(service, &mut tx, check, now).await?;
    }
    tx.commit().await.map_err(sql)?;
    report.restarted = stalled.len();

    if report != SweepReport::default() {
        tracing::info!(?report, "the delivery sweep moved deliveries on");
    }
    Ok(report)
}

/// A `[publishing]` grace period as a chrono duration. Settings validation keeps it small; one
/// too large for `now - grace` is refused rather than allowed to overflow.
fn grace(duration: std::time::Duration) -> Result<TimeDelta, JobError> {
    TimeDelta::from_std(duration)
        .ok()
        .filter(|delta| *delta <= TimeDelta::days(36_500))
        .ok_or_else(|| JobError::Fatal("a publishing grace period is out of range".to_owned()))
}

/// Enqueues `job` at `at` in the duty's transaction.
async fn put<J: Job>(
    service: &PublishingService,
    tx: &mut PgConnection,
    job: &J,
    at: DateTime<Utc>,
) -> Result<(), JobError> {
    service
        .deps
        .queue
        .enqueue_in(tx, job, Some(at))
        .await
        .map(|_| ())
        .map_err(enqueue)
}

/// Enqueues a `publish_delivery` job for each delivery `swept` sent back to `pending`, and
/// records the notices of every write (a delivery whose job was cancelled meanwhile ends
/// `cancelled` and may finish the job). Returns how many were enqueued.
async fn redispatch(
    service: &PublishingService,
    tx: &mut PgConnection,
    swept: &[Swept],
) -> Result<usize, JobError> {
    let mut enqueued = 0;
    for one in swept {
        if one.state == DeliveryState::Pending {
            let dispatch = one.dispatch();
            put(
                service,
                tx,
                &PublishDelivery::from(dispatch),
                dispatch.run_at,
            )
            .await?;
            enqueued += 1;
        }
        tell(service, tx, one, None).await?;
    }
    Ok(enqueued)
}

/// Records what the worker records for the same write: `notice` and the audit event of the
/// new state, and `job_partial` when this write first made a live job `partial`. Nothing for a
/// dry run (see [`worker::tell`]).
async fn tell(
    service: &PublishingService,
    tx: &mut PgConnection,
    swept: &Swept,
    notice: Option<NotificationKind>,
) -> Result<(), JobError> {
    if notice.is_none() && !swept.job.became_partial() {
        return Ok(());
    }
    let Some(target) = DeliveryRepo::check_target(&mut *tx, swept.delivery)
        .await
        .map_err(data)?
    else {
        return Ok(());
    };
    worker::tell(
        service,
        tx,
        &target.delivery,
        notice,
        swept.state,
        swept.job,
    )
    .await
}

/// Records `delivery_stale` for a live delivery whose cause cleared too long after it was due.
async fn stale(
    service: &PublishingService,
    tx: &mut PgConnection,
    delivery: DeliveryId,
) -> Result<(), JobError> {
    let Some(target) = DeliveryRepo::check_target(&mut *tx, delivery)
        .await
        .map_err(data)?
    else {
        return Ok(());
    };
    if target.delivery.dry_run {
        return Ok(());
    }
    let reason = match target.state {
        DeliveryState::Parked(reason) => Some(reason),
        _ => None,
    };
    notices::record(
        &service.deps.notifier,
        tx,
        NoticeSubject::from(&target.delivery),
        reason,
        Notice::DeliveryStale,
    )
    .await
    .map_err(notify)
}

/// Enqueues the chain's step `check.seq` again: a poll for a `processing` delivery, a
/// reconcile round for an `outcome_unknown` one.
async fn restart(
    service: &PublishingService,
    tx: &mut PgConnection,
    check: &StalledCheck,
    now: DateTime<Utc>,
) -> Result<(), JobError> {
    let delivery = check.delivery.as_uuid();
    match check.state {
        DeliveryState::Processing => {
            let job = PollDeliveryStatus {
                delivery,
                dispatch_gen: check.dispatch_gen,
                poll: check.seq,
            };
            put(service, tx, &job, now).await
        }
        DeliveryState::OutcomeUnknown => {
            let job = ReconcileDelivery {
                delivery,
                dispatch_gen: check.dispatch_gen,
                attempt: check.seq,
            };
            put(service, tx, &job, now).await
        }
        _ => Ok(()),
    }
}
