//! Deliveries: claims, leases, fenced state changes, late receipts, attempts, and the job
//! status they roll up into.
//!
//! The worker, the sweep, and the crons hold no [`crate::OwnerScope`]; their writes are system
//! writes and leave `updated_by` alone. Every write after a claim is conditioned on the
//! delivery's `dispatch_gen` and expected state, so a worker whose lease was taken over cannot
//! change the row. The only unfenced write is [`DeliveryRepo::record_late_receipt`], which is
//! conditioned on the state alone.

use chrono::{DateTime, Duration, Utc};
use postit_core::{
    AccountId, DeliveryAttemptId, DeliveryEvent, DeliveryId, DeliveryState, DeliveryView,
    JobStatus, PostId, PublishJobId, TransitionError, UserId, derive_job_status, transition,
};
use serde_json::Value;
use sqlx::{Connection, PgConnection};

use super::Dispatch;
use super::jobs::Operation;
use crate::error::DataError;
use crate::text::{corrupt, parse};

/// A delivery a worker claimed, with what it needs to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedDelivery {
    /// The delivery.
    pub id: DeliveryId,
    /// The job it belongs to.
    pub job_id: PublishJobId,
    /// The post it publishes.
    pub post_id: PostId,
    /// Whose workspace it runs in.
    pub owner_id: UserId,
    /// The account it goes to.
    pub account_id: AccountId,
    /// Whether the job only simulates.
    pub dry_run: bool,
    /// Whether the platform holds the post until its time.
    pub native_schedule: bool,
    /// What it does on the platform.
    pub operation: Operation,
    /// The generation the claim matched.
    pub dispatch_gen: i32,
    /// Attempts counted so far.
    pub attempt_count: i32,
    /// Where a multi-step upload stands.
    pub container_ref: Option<String>,
    /// Whether the owner asked to stop while it ran.
    pub cancel_requested: bool,
}

/// What a fenced write is conditioned on.
#[derive(Debug, Clone, Copy)]
pub struct Fence {
    /// The delivery.
    pub id: DeliveryId,
    /// The generation the writer was dispatched with.
    pub dispatch_gen: i32,
    /// The state the writer believes the delivery is in.
    pub expected: DeliveryState,
}

/// The payload columns a transition may write. Absent fields keep the column's value.
#[derive(Debug, Clone, Default)]
pub struct TransitionData {
    /// The platform's ID for the post.
    pub remote_id: Option<String>,
    /// The post's public URL.
    pub remote_url: Option<String>,
    /// When a natively scheduled post goes live.
    pub remote_publish_at: Option<DateTime<Utc>>,
    /// The classified error behind a failure or park.
    pub last_error: Option<Value>,
    /// When a delivery going back to `pending` is due; defaults to now.
    pub next_attempt_at: Option<DateTime<Utc>>,
    /// Whether this transition counts as an attempt.
    pub count_attempt: bool,
    /// Whether to start the attempt count over (a user retry).
    pub reset_attempts: bool,
    /// Progress of a direct send.
    pub direct_counts: Option<Value>,
    /// The user behind a user action, recorded as `updated_by`.
    pub actor: Option<UserId>,
}

/// What a fenced write did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applied {
    /// The write landed.
    Done {
        /// The delivery's new state.
        state: DeliveryState,
        /// Set when the delivery went back to `pending`: the caller enqueues it.
        redispatch: Option<Dispatch>,
    },
    /// The delivery was re-dispatched, swept, or cancelled; nothing changed.
    Stale,
}

/// What a platform returned for a post whose outcome was unknown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    /// The platform's ID for the post.
    pub remote_id: String,
    /// The post's public URL.
    pub remote_url: Option<String>,
}

/// Deliveries.
pub struct DeliveryRepo;

impl DeliveryRepo {
    /// Claims a due `pending` delivery for the worker holding the dispatch for `dispatch_gen`,
    /// leasing it until `now + lease`. Returns `None` when the delivery moved on: another
    /// worker claimed it, it was re-dispatched, it is not due, or its job was cancelled.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn claim(
        conn: &mut PgConnection,
        id: DeliveryId,
        dispatch_gen: i32,
        now: DateTime<Utc>,
        lease: Duration,
    ) -> Result<Option<ClaimedDelivery>, DataError> {
        let mut tx = conn.begin().await?;
        let Some(row) = sqlx::query!(
            r#"UPDATE deliveries d
               SET state = 'claimed', lease_expires_at = $4, updated_at = $3
               FROM publish_jobs j
               WHERE d.id = $1 AND d.dispatch_gen = $2 AND d.state = 'pending'
                 AND d.next_attempt_at <= $3
                 AND j.id = d.job_id AND j.cancelled_at IS NULL
               RETURNING d.id, d.job_id, d.post_id, d.owner_id, d.account_id, j.dry_run,
                         j.native_schedule, d.operation, d.dispatch_gen, d.attempt_count,
                         d.container_ref, (d.cancel_requested_at IS NOT NULL) AS "cancel_requested!""#,
            id.as_uuid(),
            dispatch_gen,
            now,
            now + lease,
        )
        .fetch_optional(&mut *tx)
        .await?
        else {
            return Ok(None);
        };
        let job = PublishJobId::from_uuid(row.job_id);
        touch_job(&mut tx, job, now).await?;
        tx.commit().await?;
        Ok(Some(ClaimedDelivery {
            id: DeliveryId::from_uuid(row.id),
            job_id: job,
            post_id: PostId::from_uuid(row.post_id),
            owner_id: UserId::from_uuid(row.owner_id),
            account_id: AccountId::from_uuid(row.account_id),
            dry_run: row.dry_run,
            native_schedule: row.native_schedule,
            operation: parse(&row.operation)?,
            dispatch_gen: row.dispatch_gen,
            attempt_count: row.attempt_count,
            container_ref: row.container_ref,
            cancel_requested: row.cancel_requested,
        }))
    }

    /// Extends the lease of a `claimed` or `in_flight` delivery to `now + lease`. Returns
    /// `false` when the delivery is no longer the caller's.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn renew_lease(
        conn: &mut PgConnection,
        id: DeliveryId,
        dispatch_gen: i32,
        now: DateTime<Utc>,
        lease: Duration,
    ) -> Result<bool, DataError> {
        let renewed = sqlx::query!(
            "UPDATE deliveries SET lease_expires_at = $4, updated_at = $3
             WHERE id = $1 AND dispatch_gen = $2 AND state IN ('claimed', 'in_flight')",
            id.as_uuid(),
            dispatch_gen,
            now,
            now + lease,
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        Ok(renewed > 0)
    }

    /// Moves the delivery from `fence.expected` by `event`, as core's transition table allows,
    /// writing `data`'s payload columns, bumping the job's version, and refreshing the job and
    /// post status, all in the caller's transaction.
    ///
    /// A delivery that goes back to `pending` gets the next generation and a due time, and the
    /// result carries the dispatch for the caller to enqueue.
    ///
    /// # Errors
    ///
    /// [`DataError::Conflict`] (`transition_not_allowed`, or `acknowledgement_required` for a
    /// retry that needs the user's acknowledgement) when core refuses the event in the expected
    /// state. A fence that no longer matches the row is [`Applied::Stale`], not an error.
    pub async fn apply(
        conn: &mut PgConnection,
        fence: Fence,
        event: DeliveryEvent,
        data: TransitionData,
        now: DateTime<Utc>,
    ) -> Result<Applied, DataError> {
        let to = transition(fence.expected, event).map_err(|err| match err {
            TransitionError::NotAllowed { .. } => {
                DataError::Conflict("transition_not_allowed".to_string())
            }
            TransitionError::AcknowledgementRequired => {
                DataError::Conflict("acknowledgement_required".to_string())
            }
        })?;
        let mut tx = conn.begin().await?;
        let Some(row) = sqlx::query!(
            r#"UPDATE deliveries SET
                   state = $4,
                   parked_reason = $5,
                   dispatch_gen = dispatch_gen + CASE WHEN $4 = 'pending' THEN 1 ELSE 0 END,
                   next_attempt_at = CASE WHEN $4 = 'pending'
                                          THEN COALESCE($6::timestamptz, $14::timestamptz)
                                          ELSE COALESCE($6::timestamptz, next_attempt_at) END,
                   lease_expires_at = CASE WHEN $4 IN ('claimed', 'in_flight')
                                           THEN lease_expires_at END,
                   remote_id = COALESCE($7, remote_id),
                   remote_url = COALESCE($8, remote_url),
                   remote_publish_at = COALESCE($9::timestamptz, remote_publish_at),
                   last_error = COALESCE($10::jsonb, last_error),
                   direct_counts = COALESCE($11::jsonb, direct_counts),
                   attempt_count = CASE WHEN $12 THEN 0
                                        WHEN $13 THEN attempt_count + 1
                                        ELSE attempt_count END,
                   parked_notified_at = CASE WHEN $4 = 'parked' THEN parked_notified_at END,
                   updated_by = COALESCE($15::uuid, updated_by),
                   updated_at = $14::timestamptz
               WHERE id = $1 AND dispatch_gen = $2 AND state = $3
                 AND parked_reason IS NOT DISTINCT FROM $16::text
               RETURNING job_id, dispatch_gen, next_attempt_at"#,
            fence.id.as_uuid(),
            fence.dispatch_gen,
            fence.expected.as_str(),
            to.as_str(),
            parked_reason(to),
            data.next_attempt_at,
            data.remote_id,
            data.remote_url,
            data.remote_publish_at,
            data.last_error,
            data.direct_counts,
            data.reset_attempts,
            data.count_attempt,
            now,
            data.actor.map(|actor| actor.as_uuid()),
            parked_reason(fence.expected),
        )
        .fetch_optional(&mut *tx)
        .await?
        else {
            return Ok(Applied::Stale);
        };
        touch_job(&mut tx, PublishJobId::from_uuid(row.job_id), now).await?;
        tx.commit().await?;
        let redispatch = match (to, row.next_attempt_at) {
            (DeliveryState::Pending, Some(run_at)) => Some(Dispatch {
                delivery: fence.id,
                dispatch_gen: row.dispatch_gen,
                run_at,
            }),
            _ => None,
        };
        Ok(Applied::Done {
            state: to,
            redispatch,
        })
    }

    /// Records where a multi-step upload stands, so a retry resumes there. Fenced like
    /// [`Self::apply`] but changes no state. Returns `false` when the fence no longer matches.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn set_container_ref(
        conn: &mut PgConnection,
        fence: Fence,
        container_ref: &str,
    ) -> Result<bool, DataError> {
        let written = sqlx::query!(
            "UPDATE deliveries SET container_ref = $4
             WHERE id = $1 AND dispatch_gen = $2 AND state = $3
               AND parked_reason IS NOT DISTINCT FROM $5::text",
            fence.id.as_uuid(),
            fence.dispatch_gen,
            fence.expected.as_str(),
            container_ref,
            parked_reason(fence.expected),
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        Ok(written > 0)
    }

    /// Publishes a delivery that was swept to `outcome_unknown` after all, because the worker
    /// that lost its lease reported the platform's receipt. Conditioned on the state alone:
    /// whatever the generation, a late success is never lost, and a second receipt changes
    /// nothing. Returns whether this call published it.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn record_late_receipt(
        conn: &mut PgConnection,
        id: DeliveryId,
        receipt: Receipt,
        now: DateTime<Utc>,
    ) -> Result<bool, DataError> {
        let to = transition(DeliveryState::OutcomeUnknown, DeliveryEvent::ReconcileFound)
            .map_err(|_| DataError::Conflict("transition_not_allowed".to_string()))?;
        let mut tx = conn.begin().await?;
        let Some(job) = sqlx::query_scalar!(
            "UPDATE deliveries
             SET state = $2, remote_id = $3, remote_url = $4, lease_expires_at = NULL,
                 parked_notified_at = NULL, updated_at = $5
             WHERE id = $1 AND state = 'outcome_unknown' AND remote_id IS NULL
             RETURNING job_id",
            id.as_uuid(),
            to.as_str(),
            receipt.remote_id,
            receipt.remote_url,
            now,
        )
        .fetch_optional(&mut *tx)
        .await?
        else {
            return Ok(false);
        };
        touch_job(&mut tx, PublishJobId::from_uuid(job), now).await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Recomputes the job's status from its deliveries and writes it. For a live job whose
    /// post is still at the job's version, the post shows the same status; if the post was
    /// edited since, it shows `draft` (unless a newer live job is running).
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when there is no such job.
    pub async fn refresh_job_status(
        conn: &mut PgConnection,
        job: PublishJobId,
        now: DateTime<Utc>,
    ) -> Result<JobStatus, DataError> {
        let rows = sqlx::query!(
            "SELECT state, parked_reason, next_attempt_at, remote_publish_at
             FROM deliveries WHERE job_id = $1",
            job.as_uuid(),
        )
        .fetch_all(&mut *conn)
        .await?;
        let views = rows
            .into_iter()
            .map(|row| {
                let state = DeliveryState::from_parts(&row.state, row.parked_reason.as_deref())
                    .map_err(corrupt)?;
                Ok(DeliveryView {
                    state,
                    next_attempt_at: row.next_attempt_at.unwrap_or(now),
                    awaiting_native_publish: state == DeliveryState::Processing
                        && row.remote_publish_at.is_some_and(|at| at > now),
                })
            })
            .collect::<Result<Vec<_>, DataError>>()?;
        let status = derive_job_status(&views, now);
        let info = sqlx::query!(
            "UPDATE publish_jobs SET status = $2, updated_at = $3
             WHERE id = $1 RETURNING owner_id, post_id, post_version, dry_run",
            job.as_uuid(),
            status.as_str(),
            now,
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?;
        if !info.dry_run {
            sqlx::query!(
                "UPDATE posts
                 SET status = CASE WHEN version = $3 THEN $4 ELSE 'draft' END
                 WHERE owner_id = $1 AND id = $2
                   AND (version = $3 OR NOT EXISTS (
                       SELECT 1 FROM publish_jobs j
                       WHERE j.post_id = posts.id AND NOT j.dry_run
                         AND j.status IN ('scheduled', 'publishing') AND j.id <> $5))",
                info.owner_id,
                info.post_id,
                info.post_version,
                status.as_str(),
                job.as_uuid(),
            )
            .execute(&mut *conn)
            .await?;
        }
        Ok(status)
    }
}

/// Delivery attempts: one row per try, with the classified error and a redacted summary.
pub struct AttemptRepo;

impl AttemptRepo {
    /// Starts an attempt of the delivery at `now`.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when there is no such delivery.
    pub async fn start(
        conn: &mut PgConnection,
        id: DeliveryAttemptId,
        delivery: DeliveryId,
        now: DateTime<Utc>,
    ) -> Result<(), DataError> {
        let inserted = sqlx::query!(
            "INSERT INTO delivery_attempts (id, owner_id, delivery_id, started_at)
             SELECT $1, owner_id, id, $3 FROM deliveries WHERE id = $2",
            id.as_uuid(),
            delivery.as_uuid(),
            now,
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if inserted == 0 {
            return Err(DataError::NotFound);
        }
        Ok(())
    }

    /// Ends an attempt with its outcome. `error` and `request_summary` must already be
    /// redacted.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when there is no such attempt.
    pub async fn finish(
        conn: &mut PgConnection,
        id: DeliveryAttemptId,
        outcome: &str,
        error: Option<Value>,
        request_summary: Option<Value>,
        now: DateTime<Utc>,
    ) -> Result<(), DataError> {
        let finished = sqlx::query!(
            "UPDATE delivery_attempts
             SET finished_at = $5, outcome = $2, error = $3, request_summary = $4, updated_at = $5
             WHERE id = $1",
            id.as_uuid(),
            outcome,
            error,
            request_summary,
            now,
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if finished == 0 {
            return Err(DataError::NotFound);
        }
        Ok(())
    }
}

/// The `parked_reason` column of `state`.
fn parked_reason(state: DeliveryState) -> Option<&'static str> {
    match state {
        DeliveryState::Parked(reason) => Some(reason.as_str()),
        _ => None,
    }
}

/// Bumps the job's version after a delivery change and refreshes its status. Every write to a
/// delivery calls it in the same transaction.
pub(crate) async fn touch_job(
    conn: &mut PgConnection,
    job: PublishJobId,
    now: DateTime<Utc>,
) -> Result<JobStatus, DataError> {
    sqlx::query!(
        "UPDATE publish_jobs SET version = version + 1 WHERE id = $1",
        job.as_uuid(),
    )
    .execute(&mut *conn)
    .await?;
    DeliveryRepo::refresh_job_status(conn, job, now).await
}
