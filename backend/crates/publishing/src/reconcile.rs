//! The `reconcile_delivery` handler: finds out whether an ambiguous publish (or comment)
//! landed, and settles the `outcome_unknown` delivery accordingly.
//!
//! Each round is one step of the delivery's chain (`deliveries.check_seq`), taken and
//! committed with its successor's due time before the platform call, like a poll (see
//! [`crate::poll`]).

use chrono::{DateTime, Utc};
use postit_core::{
    AgentError, Capability, DeliveryEvent, DeliveryId, DeliveryState, NotificationKind,
    PublishReceipt, ReconcileOutcome, ReconcileRequest, RetryClass,
};
use postit_data::publishing::{CheckTarget, Fence, JobRepo, Operation, TransitionData};
use postit_jobs::JobError;

use crate::backoff::poll_delay;
use crate::outcome::{self, later};
use crate::worker::{self, Wrote, data, enqueue, sql};
use crate::{PublishDelivery, PublishingService, ReconcileDelivery};

/// What a reconcile round decided.
enum Verdict {
    /// The post is on the platform.
    Found(PublishReceipt),
    /// The platform has no such post.
    Absent,
    /// Nobody can tell; the owner decides.
    Unresolvable,
    /// Ask again later: at the platform's `Retry-After` time when it gave one.
    Retry(Option<DateTime<Utc>>),
}

/// Runs one `reconcile_delivery` job.
///
/// A delivery that is not `outcome_unknown` at the job's generation is left alone (a late
/// receipt may already have published it). The agent's `reconcile` (or `reconcile_comment`
/// for a comment) is asked with the last attempt's start time and container reference:
/// `Found` publishes the delivery with the receipt (a direct delivery is `Absent` without
/// asking: nothing is sent before it reaches `processing`); `Absent` sends it back to `pending` with
/// the next generation, or ends it `dead_letter` once `attempt_count` reached
/// `publishing.max_attempts`; `Unresolvable`, `Unsupported`, and any other permanent error park
/// it as `outcome_unresolvable`. A transient failure asks again after `poll_delay(attempt + 1)`
/// (or the platform's `Retry-After`) until `processing_timeout` has passed since the delivery
/// entered `outcome_unknown`, then parks it too.
///
/// # Errors
///
/// [`JobError::Retry`] on a database failure. A failure after the step was taken leaves the
/// chain to the due sweep's backstop.
pub async fn run_reconcile(
    service: &PublishingService,
    job: ReconcileDelivery,
) -> Result<(), JobError> {
    let deps = &service.deps;
    let now = deps.clock.now();
    let Some(target) = worker::check_target(service, DeliveryId::from_uuid(job.delivery)).await?
    else {
        return Ok(());
    };
    let claimed = &target.delivery;
    if target.state != DeliveryState::OutcomeUnknown || claimed.dispatch_gen != job.dispatch_gen {
        return Ok(());
    }
    let fence = worker::fence(claimed, DeliveryState::OutcomeUnknown);
    let next = job.attempt.saturating_add(1);
    let next_due = later(now, poll_delay(next));
    if !worker::take_check(service, fence, job.attempt, next_due).await? {
        return Ok(());
    }
    let attempted_at = target.attempt_started_at.unwrap_or(now);
    if claimed.operation == Operation::Direct {
        // A direct delivery sends nothing before it reaches `processing`, so an ambiguous one
        // left nothing on the platform: no agent is asked.
        return settle(
            service,
            &target,
            fence,
            Verdict::Absent,
            next,
            next_due,
            now,
        )
        .await;
    }
    let asked = ask(service, &target, attempted_at).await?;
    let now = deps.clock.now();
    let verdict = match asked {
        Some(Ok(ReconcileOutcome::Found(receipt))) => Verdict::Found(receipt),
        Some(Ok(ReconcileOutcome::Absent)) => Verdict::Absent,
        Some(Ok(ReconcileOutcome::Unresolvable)) => Verdict::Unresolvable,
        // `Unsupported` is permanent: the platform has no way to look the post up.
        Some(Err(error)) if error.retry_class() == RetryClass::Permanent => Verdict::Unresolvable,
        // A transient failure asks again until `processing_timeout` has passed since the
        // delivery entered `outcome_unknown` (or since the attempt started, if later).
        failed @ (Some(Err(_)) | None) => {
            let since = target.state_since.max(attempted_at);
            if now >= later(since, deps.settings.processing_timeout) {
                Verdict::Unresolvable
            } else {
                Verdict::Retry(match &failed {
                    Some(Err(error)) => outcome::retry_after_at(error, now),
                    _ => None,
                })
            }
        }
    };
    settle(service, &target, fence, verdict, next, next_due, now).await
}

/// The agent's answer. `None` when it cannot be asked now: the account cannot bind a client,
/// or the job's snapshot has no variant for it.
async fn ask(
    service: &PublishingService,
    target: &CheckTarget,
    attempted_at: DateTime<Utc>,
) -> Result<Option<Result<ReconcileOutcome, AgentError>>, JobError> {
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
    let request = ReconcileRequest {
        variant,
        container_ref: claimed.container_ref.as_deref(),
        attempted_at,
        idempotency_key: claimed.id,
    };
    let answer = match claimed.operation {
        Operation::Comment => match bound.client.commenter() {
            Some(commenter) => commenter.reconcile_comment(request).await,
            None => Err(AgentError::Unsupported {
                capability: Capability::Commenter,
            }),
        },
        // `run_reconcile` never asks for a direct delivery.
        Operation::Publish | Operation::Direct => bound.client.reconcile(request).await,
    };
    Ok(Some(answer))
}

/// Writes the verdict under the fence, with its follow-up job and notices, in one transaction.
async fn settle(
    service: &PublishingService,
    target: &CheckTarget,
    fence: Fence,
    verdict: Verdict,
    next: i32,
    next_due: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<(), JobError> {
    let deps = &service.deps;
    let claimed = &target.delivery;
    let mut tx = deps.pool.begin().await.map_err(sql)?;
    match verdict {
        Verdict::Found(receipt) => {
            let wrote = worker::apply_and_tell(
                service,
                &mut tx,
                claimed,
                fence,
                DeliveryEvent::ReconcileFound,
                outcome::receipt_data(&receipt),
                Some(NotificationKind::DeliveryPublished),
                now,
            )
            .await?;
            if matches!(wrote, Wrote::Stale) {
                worker::late_receipt(service, &mut tx, claimed, &receipt, now).await?;
            }
        }
        Verdict::Absent => {
            // `Absent` proves nothing went out, so ending is safe once the budget is spent;
            // the owner can retry with a fresh count.
            let spent = i64::from(claimed.attempt_count) >= i64::from(deps.settings.max_attempts);
            let (event, notice) = if spent {
                (
                    DeliveryEvent::RetryBudgetExhausted,
                    Some(NotificationKind::DeliveryDeadLettered),
                )
            } else {
                (DeliveryEvent::ReconcileAbsent, None)
            };
            let wrote = worker::apply_and_tell(
                service,
                &mut tx,
                claimed,
                fence,
                event,
                TransitionData::default(),
                notice,
                now,
            )
            .await?;
            if let Wrote::Done {
                redispatch: Some(dispatch),
            } = wrote
            {
                deps.queue
                    .enqueue_in(
                        &mut tx,
                        &PublishDelivery::from(dispatch),
                        Some(dispatch.run_at),
                    )
                    .await
                    .map_err(enqueue)?;
            }
        }
        Verdict::Unresolvable => {
            worker::apply_and_tell(
                service,
                &mut tx,
                claimed,
                fence,
                DeliveryEvent::ReconcileUnresolvable,
                TransitionData::default(),
                Some(NotificationKind::DeliveryParked),
                now,
            )
            .await?;
        }
        Verdict::Retry(retry_at) => {
            let successor = ReconcileDelivery {
                delivery: claimed.id.as_uuid(),
                dispatch_gen: claimed.dispatch_gen,
                attempt: next,
            };
            worker::enqueue_successor(
                service, &mut tx, fence, next, &successor, next_due, retry_at,
            )
            .await?;
        }
    }
    tx.commit().await.map_err(sql)
}
