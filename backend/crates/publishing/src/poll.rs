//! The `poll_delivery_status` handler: asks the platform whether a `processing` delivery is
//! live yet, waits for a natively scheduled upload's publish time, and gives up on a delivery
//! that stays `processing` past `publishing.processing_timeout`.
//!
//! Each poll is one step of the delivery's chain (`deliveries.check_seq`): a job carrying
//! `poll = k` proceeds only when it takes step `k`, and its successor carries `k + 1`, so a
//! duplicate job exits instead of forking the chain. The step is committed, with the
//! successor's due time as the backstop (`check_due_at`), before the platform call; no
//! connection is held across it.

use chrono::{DateTime, Utc};
use postit_core::{
    AgentError, DeliveryEvent, DeliveryId, DeliveryState, NotificationKind, PublishOutcome,
    PublishReceipt, RetryClass,
};
use postit_data::publishing::{
    CheckTarget, DeliveryRepo, Fence, JobRepo, Operation, TransitionData,
};
use postit_jobs::JobError;
use serde_json::json;
use sqlx::PgConnection;

use crate::backoff::poll_delay;
use crate::outcome::{self, later};
use crate::worker::{self, Wrote, data, enqueue, sql};
use crate::{PollDeliveryStatus, PublishingService, ReconcileDelivery};

/// Runs one `poll_delivery_status` job.
///
/// A delivery that is not `processing` at the job's generation, or is a direct send, is left
/// alone. Past `processing_timeout` (counted from `remote_publish_at` for a native schedule,
/// else from the attempt's start) it becomes `outcome_unknown` through `ProcessingTimeout`,
/// counting an attempt, and its first reconcile runs at once. A natively scheduled upload
/// whose publish time is still ahead waits for it without asking the platform. Otherwise the
/// platform's answer decides: `Published` publishes it, a permanent error fails it, and
/// anything else polls again after `poll_delay(poll + 1)`, or at the platform's `Retry-After`.
///
/// # Errors
///
/// [`JobError::Retry`] on a database failure. A failure after the step was taken leaves the
/// chain to the due sweep's backstop.
pub async fn run_poll(
    service: &PublishingService,
    job: PollDeliveryStatus,
) -> Result<(), JobError> {
    let deps = &service.deps;
    let now = deps.clock.now();
    let Some(target) = worker::check_target(service, DeliveryId::from_uuid(job.delivery)).await?
    else {
        return Ok(());
    };
    let claimed = &target.delivery;
    if target.state != DeliveryState::Processing
        || claimed.dispatch_gen != job.dispatch_gen
        || claimed.operation == Operation::Direct
    {
        return Ok(());
    }
    let fence = worker::fence(claimed, DeliveryState::Processing);
    if timed_out(&target, deps.settings.processing_timeout, now) {
        return time_out(service, &target, fence, now).await;
    }
    let next = job.poll.saturating_add(1);
    if let Some(live_at) = target.remote_publish_at.filter(|at| *at > now) {
        // A natively scheduled upload: there is nothing to ask before it goes live.
        let mut tx = deps.pool.begin().await.map_err(sql)?;
        if DeliveryRepo::take_check(&mut tx, fence, job.poll, live_at)
            .await
            .map_err(data)?
        {
            enqueue_poll(service, &mut tx, &target, next, live_at).await?;
        }
        tx.commit().await.map_err(sql)?;
        return Ok(());
    }
    let next_due = later(now, poll_delay(next));
    if !worker::take_check(service, fence, job.poll, next_due).await? {
        return Ok(());
    }
    match ask(service, &target).await? {
        Some(Ok(PublishOutcome::Published(receipt))) => {
            publish(service, &target, fence, &receipt).await
        }
        Some(Err(error)) if error.retry_class() == RetryClass::Permanent => {
            fail(service, &target, fence, &error).await
        }
        // Still processing, a transient failure, or no client right now: ask again, when the
        // platform said if it did.
        asked => {
            let retry_at = match &asked {
                Some(Err(error)) => outcome::retry_after_at(error, deps.clock.now()),
                _ => None,
            };
            let successor = PollDeliveryStatus {
                delivery: claimed.id.as_uuid(),
                dispatch_gen: claimed.dispatch_gen,
                poll: next,
            };
            let mut tx = deps.pool.begin().await.map_err(sql)?;
            worker::enqueue_successor(
                service, &mut tx, fence, next, &successor, next_due, retry_at,
            )
            .await?;
            tx.commit().await.map_err(sql)
        }
    }
}

/// Whether the delivery has been `processing` for longer than `timeout`: since the platform's
/// publish time for a native schedule, else since the attempt started.
fn timed_out(target: &CheckTarget, timeout: std::time::Duration, now: DateTime<Utc>) -> bool {
    target
        .remote_publish_at
        .or(target.attempt_started_at)
        .is_some_and(|since| now >= later(since, timeout))
}

/// The platform's answer about the delivery's receipt. `None` when it cannot be asked now:
/// the account cannot bind a client, or the job's snapshot has no variant for it.
async fn ask(
    service: &PublishingService,
    target: &CheckTarget,
) -> Result<Option<Result<PublishOutcome, AgentError>>, JobError> {
    let claimed = &target.delivery;
    let snapshot = {
        let mut conn = service.deps.pool.acquire().await.map_err(sql)?;
        JobRepo::snapshot(&mut conn, claimed.job_id)
            .await
            .map_err(data)?
    };
    let Some(variant) = snapshot.variant_for(claimed.account_id) else {
        return Ok(None);
    };
    let Ok(bound) = service
        .deps
        .accounts
        .bound_client(claimed.owner_id, claimed.account_id)
        .await
    else {
        return Ok(None);
    };
    let receipt = PublishReceipt {
        remote_id: target.remote_id.clone(),
        remote_url: target
            .remote_url
            .as_deref()
            .and_then(|url| url::Url::parse(url).ok()),
        container_ref: claimed.container_ref.clone(),
        remote_publish_at: target.remote_publish_at,
    };
    Ok(Some(bound.client.status(&variant.target, &receipt).await))
}

/// `Published` with the platform's receipt. A stale fence hands the receipt to
/// `record_late_receipt`.
async fn publish(
    service: &PublishingService,
    target: &CheckTarget,
    fence: Fence,
    receipt: &PublishReceipt,
) -> Result<(), JobError> {
    let deps = &service.deps;
    let now = deps.clock.now();
    let claimed = &target.delivery;
    let mut tx = deps.pool.begin().await.map_err(sql)?;
    let wrote = worker::apply_and_tell(
        service,
        &mut tx,
        claimed,
        fence,
        DeliveryEvent::Published,
        outcome::receipt_data(receipt),
        Some(NotificationKind::DeliveryPublished),
        now,
    )
    .await?;
    if matches!(wrote, Wrote::Stale) {
        worker::late_receipt(service, &mut tx, claimed, receipt, now).await?;
    }
    tx.commit().await.map_err(sql)
}

/// `PermanentFailure` with the error's code and class.
async fn fail(
    service: &PublishingService,
    target: &CheckTarget,
    fence: Fence,
    error: &AgentError,
) -> Result<(), JobError> {
    let deps = &service.deps;
    let mut tx = deps.pool.begin().await.map_err(sql)?;
    worker::apply_and_tell(
        service,
        &mut tx,
        &target.delivery,
        fence,
        DeliveryEvent::PermanentFailure,
        TransitionData {
            last_error: Some(outcome::last_error(error)),
            ..TransitionData::default()
        },
        Some(NotificationKind::DeliveryFailed),
        deps.clock.now(),
    )
    .await?;
    tx.commit().await.map_err(sql)
}

/// `ProcessingTimeout` into `outcome_unknown`, counting an attempt so the retry budget bounds
/// a platform that keeps accepting and never finishing, and the first reconcile at once.
async fn time_out(
    service: &PublishingService,
    target: &CheckTarget,
    fence: Fence,
    now: DateTime<Utc>,
) -> Result<(), JobError> {
    let deps = &service.deps;
    let claimed = &target.delivery;
    let mut tx = deps.pool.begin().await.map_err(sql)?;
    let wrote = worker::apply_and_tell(
        service,
        &mut tx,
        claimed,
        fence,
        DeliveryEvent::ProcessingTimeout,
        TransitionData {
            last_error: Some(json!({"code": "processing_timeout", "class": "ambiguous"})),
            count_attempt: true,
            ..TransitionData::default()
        },
        Some(NotificationKind::DeliveryOutcomeUnknown),
        now,
    )
    .await?;
    if matches!(wrote, Wrote::Done { .. }) {
        let first = ReconcileDelivery {
            delivery: claimed.id.as_uuid(),
            dispatch_gen: claimed.dispatch_gen,
            attempt: 0,
        };
        deps.queue
            .enqueue_in(&mut tx, &first, Some(now))
            .await
            .map_err(enqueue)?;
    }
    tx.commit().await.map_err(sql)
}

/// Enqueues the chain's poll `poll` at `at`, in `tx`.
async fn enqueue_poll(
    service: &PublishingService,
    tx: &mut PgConnection,
    target: &CheckTarget,
    poll: i32,
    at: DateTime<Utc>,
) -> Result<(), JobError> {
    let job = PollDeliveryStatus {
        delivery: target.delivery.id.as_uuid(),
        dispatch_gen: target.delivery.dispatch_gen,
        poll,
    };
    service
        .deps
        .queue
        .enqueue_in(tx, &job, Some(at))
        .await
        .map(|_| ())
        .map_err(enqueue)
}
