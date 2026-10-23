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
    JobStatus, PostId, PublishJobId, ResolvedOutcome, TransitionError, UserId, derive_job_status,
    transition,
};
use serde_json::Value;
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use super::Dispatch;
use super::direct::DirectRepo;
use super::jobs::Operation;
use crate::constraint::translate;
use crate::error::DataError;
use crate::text::{corrupt, parse};
use crate::{Capability, OwnerScope};

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
    /// Whether to start over (a user retry): the attempt count goes back to zero and a delivery
    /// that returns to `pending` forgets an earlier cancel request.
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

/// A delivery as the owner sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Delivery {
    /// The delivery.
    pub id: DeliveryId,
    /// The job it belongs to.
    pub job_id: PublishJobId,
    /// The post it publishes.
    pub post_id: PostId,
    /// The account it goes to.
    pub account_id: AccountId,
    /// What it does on the platform.
    pub operation: Operation,
    /// Where it is.
    pub state: DeliveryState,
    /// The generation of its latest dispatch.
    pub dispatch_gen: i32,
    /// Attempts counted so far.
    pub attempt_count: i32,
    /// When it is next due.
    pub next_attempt_at: Option<DateTime<Utc>>,
    /// The platform's ID for the post.
    pub remote_id: Option<String>,
    /// The post's public URL.
    pub remote_url: Option<String>,
    /// When a natively scheduled post goes live.
    pub remote_publish_at: Option<DateTime<Utc>>,
    /// The classified error behind the last failure or park.
    pub last_error: Option<Value>,
    /// When the owner asked to stop it while it ran.
    pub cancel_requested_at: Option<DateTime<Utc>>,
    /// Progress of a direct send.
    pub direct_counts: Option<Value>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

struct DeliveryRow {
    id: Uuid,
    job_id: Uuid,
    post_id: Uuid,
    account_id: Uuid,
    operation: String,
    state: String,
    parked_reason: Option<String>,
    dispatch_gen: i32,
    attempt_count: i32,
    next_attempt_at: Option<DateTime<Utc>>,
    remote_id: Option<String>,
    remote_url: Option<String>,
    remote_publish_at: Option<DateTime<Utc>>,
    last_error: Option<Value>,
    cancel_requested_at: Option<DateTime<Utc>>,
    direct_counts: Option<Value>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<DeliveryRow> for Delivery {
    type Error = DataError;

    fn try_from(row: DeliveryRow) -> Result<Self, DataError> {
        Ok(Self {
            id: DeliveryId::from_uuid(row.id),
            job_id: PublishJobId::from_uuid(row.job_id),
            post_id: PostId::from_uuid(row.post_id),
            account_id: AccountId::from_uuid(row.account_id),
            operation: parse(&row.operation)?,
            state: DeliveryState::from_parts(&row.state, row.parked_reason.as_deref())
                .map_err(corrupt)?,
            dispatch_gen: row.dispatch_gen,
            attempt_count: row.attempt_count,
            next_attempt_at: row.next_attempt_at,
            remote_id: row.remote_id,
            remote_url: row.remote_url,
            remote_publish_at: row.remote_publish_at,
            last_error: row.last_error,
            cancel_requested_at: row.cancel_requested_at,
            direct_counts: row.direct_counts,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

/// What cancelling one delivery did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelOutcome {
    /// It is now `cancelled`: it had not started, or it was a direct campaign whose queued rows
    /// were stopped with it.
    Cancelled,
    /// A worker is publishing it: the request is recorded and the worker stops at its next
    /// check. The job stays open until the worker finishes.
    Requested,
    /// A natively scheduled upload sits on the platform: the caller deletes it through the
    /// agent, then cancels the delivery.
    NeedsAgent {
        /// When the upload was due to go live.
        remote_publish_at: DateTime<Utc>,
    },
    /// The delivery is in a state that cannot be cancelled: ended, or `processing` without being
    /// a direct campaign or a natively scheduled upload.
    NotCancellable,
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
        let mut to = transition(fence.expected, event).map_err(|err| match err {
            TransitionError::NotAllowed { .. } => {
                DataError::Conflict("transition_not_allowed".to_string())
            }
            TransitionError::AcknowledgementRequired => {
                DataError::Conflict("acknowledgement_required".to_string())
            }
        })?;
        let mut tx = conn.begin().await?;
        // A job cancelled while this delivery was in flight cannot be retried: sending the
        // delivery back to `pending` would strand it, because nothing claims a cancelled job's
        // deliveries, and the job would never finish. It ends cancelled instead, which is what
        // `pending` plus the cancel would have become.
        if to == DeliveryState::Pending && job_is_cancelled(&mut tx, fence.id).await? {
            to = DeliveryState::Cancelled;
        }
        let Some(row) = sqlx::query!(
            r#"UPDATE deliveries SET
                   state = $4,
                   parked_reason = $5,
                   dispatch_gen = dispatch_gen + CASE WHEN $4 IN ('pending', 'cancelled') THEN 1 ELSE 0 END,
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
                   cancel_requested_at = CASE WHEN $12 AND $4 = 'pending' THEN NULL
                                              ELSE cancel_requested_at END,
                   attempt_count = CASE WHEN $12 THEN 0
                                        WHEN $13 THEN attempt_count + 1
                                        ELSE attempt_count END,
                   parked_notified_at = CASE WHEN $4 = 'parked' THEN parked_notified_at END,
                   updated_by = COALESCE($15::uuid, updated_by),
                   updated_at = $14::timestamptz
               WHERE id = $1 AND dispatch_gen = $2 AND state = $3
                 AND parked_reason IS NOT DISTINCT FROM $16::text
               RETURNING job_id, dispatch_gen, next_attempt_at, operation"#,
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
        if to == DeliveryState::Cancelled && row.operation == Operation::Direct.as_str() {
            // Every cancel path ends here, so a cancelled campaign never keeps queued rows.
            DirectRepo::cancel_queued(&mut tx, fence.id, now).await?;
            DirectRepo::store_counts(&mut tx, fence.id).await?;
        }
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

    /// One delivery the scope can see.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such delivery in the scope's account
    /// limit.
    pub async fn get(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: DeliveryId,
    ) -> Result<Delivery, DataError> {
        scope.require(Capability::View)?;
        let accounts = scope.account_filter();
        sqlx::query_as!(
            DeliveryRow,
            "SELECT id, job_id, post_id, account_id, operation, state, parked_reason,
                    dispatch_gen, attempt_count, next_attempt_at, remote_id, remote_url,
                    remote_publish_at, last_error, cancel_requested_at, direct_counts,
                    created_at, updated_at
             FROM deliveries
             WHERE owner_id = $1 AND id = $2
               AND ($3::uuid[] IS NULL OR account_id = ANY($3))",
            scope.owner.as_uuid(),
            id.as_uuid(),
            accounts.as_deref(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?
        .try_into()
    }

    /// The job's deliveries the scope's account limit reaches, oldest first.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn list_for_job(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        job: PublishJobId,
    ) -> Result<Vec<Delivery>, DataError> {
        scope.require(Capability::View)?;
        let accounts = scope.account_filter();
        sqlx::query_as!(
            DeliveryRow,
            "SELECT id, job_id, post_id, account_id, operation, state, parked_reason,
                    dispatch_gen, attempt_count, next_attempt_at, remote_id, remote_url,
                    remote_publish_at, last_error, cancel_requested_at, direct_counts,
                    created_at, updated_at
             FROM deliveries
             WHERE owner_id = $1 AND job_id = $2
               AND ($3::uuid[] IS NULL OR account_id = ANY($3))
             ORDER BY id",
            scope.owner.as_uuid(),
            job.as_uuid(),
            accounts.as_deref(),
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(Delivery::try_from)
        .collect()
    }

    /// Cancels one delivery. A `pending`, `claimed`, or `parked` delivery becomes `cancelled`
    /// (a worker holding a claim finds its next fenced write stale), and so does a `processing`
    /// direct campaign, whose queued rows stop with it; an `in_flight` one only records the
    /// request, because the post may already be on its way out.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the delivery is missing or outside the account limit.
    pub async fn cancel(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: DeliveryId,
        now: DateTime<Utc>,
    ) -> Result<CancelOutcome, DataError> {
        scope.require(Capability::Publish)?;
        let delivery = lock_one(conn, scope, id).await?;
        cancel_loaded(conn, &delivery, Some(scope.actor), now).await
    }

    /// Whether any delivery of the account is claimed or in flight, so a worker may be
    /// using its credential right now.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn account_has_in_flight(
        conn: &mut PgConnection,
        account: AccountId,
    ) -> Result<bool, DataError> {
        let found = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM deliveries
                              WHERE account_id = $1 AND state IN ('claimed', 'in_flight'))
               AS "found!""#,
            account.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(found)
    }

    /// Cancels every `pending` and `parked` delivery of `account`, for disconnecting it. A
    /// system call.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn cancel_for_account(
        conn: &mut PgConnection,
        account: AccountId,
        now: DateTime<Utc>,
    ) -> Result<Vec<(DeliveryId, CancelOutcome)>, DataError> {
        let rows = sqlx::query_as!(
            DeliveryRow,
            "SELECT id, job_id, post_id, account_id, operation, state, parked_reason,
                    dispatch_gen, attempt_count, next_attempt_at, remote_id, remote_url,
                    remote_publish_at, last_error, cancel_requested_at, direct_counts,
                    created_at, updated_at
             FROM deliveries
             WHERE account_id = $1 AND state IN ('pending', 'parked')
             ORDER BY id
             FOR UPDATE",
            account.as_uuid(),
        )
        .fetch_all(&mut *conn)
        .await?;
        let mut outcomes = Vec::with_capacity(rows.len());
        for row in rows {
            let delivery = Delivery::try_from(row)?;
            let outcome = cancel_loaded(conn, &delivery, None, now).await?;
            outcomes.push((delivery.id, outcome));
        }
        Ok(outcomes)
    }

    /// Sends a failed, dead-lettered, parked, or outcome-unknown delivery through again with
    /// a fresh attempt count, and returns the dispatch to enqueue. A delivery whose outcome is
    /// unknown may already be live, so retrying it needs `acknowledged`.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the delivery is missing or outside the account limit.
    /// [`DataError::Conflict`] (`acknowledgement_required`) without the acknowledgement,
    /// (`transition_not_allowed`) in a state that cannot retry, and (`job_cancelled`) when its
    /// job was cancelled. [`DataError::JobActive`] when the job has ended and the post has
    /// since gone live in another job.
    pub async fn retry(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: DeliveryId,
        acknowledged: bool,
        now: DateTime<Utc>,
    ) -> Result<Dispatch, DataError> {
        scope.require(Capability::Publish)?;
        let delivery = lock_one(conn, scope, id).await?;
        let job = sqlx::query!(
            "SELECT status, dry_run, cancelled_at FROM publish_jobs WHERE id = $1",
            delivery.job_id.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await?;
        if job.cancelled_at.is_some() {
            return Err(DataError::Conflict("job_cancelled".to_string()));
        }
        let ended = !matches!(job.status.as_str(), "scheduled" | "publishing");
        if ended && !job.dry_run {
            // Job creation locks the post; so does this, so the check cannot race it.
            sqlx::query!(
                "SELECT id FROM posts WHERE id = $1 FOR UPDATE",
                delivery.post_id.as_uuid(),
            )
            .fetch_optional(&mut *conn)
            .await?;
            let live = sqlx::query_scalar!(
                r#"SELECT EXISTS (
                       SELECT 1 FROM publish_jobs
                       WHERE post_id = $1 AND NOT dry_run AND id <> $2
                         AND status IN ('scheduled', 'publishing')
                   ) AS "live!""#,
                delivery.post_id.as_uuid(),
                delivery.job_id.as_uuid(),
            )
            .fetch_one(&mut *conn)
            .await?;
            if live {
                return Err(DataError::JobActive);
            }
        }
        let applied = Self::apply(
            conn,
            Fence {
                id,
                dispatch_gen: delivery.dispatch_gen,
                expected: delivery.state,
            },
            DeliveryEvent::UserRetry {
                acknowledged_outcome_unknown: acknowledged,
            },
            TransitionData {
                reset_attempts: true,
                actor: Some(scope.actor),
                ..TransitionData::default()
            },
            now,
        )
        .await?;
        match applied {
            Applied::Done {
                redispatch: Some(dispatch),
                ..
            } => Ok(dispatch),
            Applied::Done { .. } | Applied::Stale => {
                Err(DataError::Conflict("delivery_changed".to_string()))
            }
        }
    }

    /// Settles a dead-lettered, parked, or outcome-unknown delivery by the owner's word that
    /// the post did or did not go out. Returns the delivery's new state.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the delivery is missing or outside the account limit, and
    /// [`DataError::Conflict`] (`transition_not_allowed`) in a state that cannot be resolved.
    pub async fn resolve(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: DeliveryId,
        outcome: ResolvedOutcome,
        now: DateTime<Utc>,
    ) -> Result<DeliveryState, DataError> {
        scope.require(Capability::Publish)?;
        let delivery = lock_one(conn, scope, id).await?;
        let applied = Self::apply(
            conn,
            Fence {
                id,
                dispatch_gen: delivery.dispatch_gen,
                expected: delivery.state,
            },
            DeliveryEvent::UserResolve(outcome),
            TransitionData {
                actor: Some(scope.actor),
                ..TransitionData::default()
            },
            now,
        )
        .await?;
        match applied {
            Applied::Done { state, .. } => Ok(state),
            Applied::Stale => Err(DataError::Conflict("delivery_changed".to_string())),
        }
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
        .await
        .map_err(translate)?
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

/// Locks one delivery in the scope's account limit for an action on it.
async fn lock_one(
    conn: &mut PgConnection,
    scope: &OwnerScope,
    id: DeliveryId,
) -> Result<Delivery, DataError> {
    let accounts = scope.account_filter();
    sqlx::query_as!(
        DeliveryRow,
        "SELECT id, job_id, post_id, account_id, operation, state, parked_reason,
                dispatch_gen, attempt_count, next_attempt_at, remote_id, remote_url,
                remote_publish_at, last_error, cancel_requested_at, direct_counts,
                created_at, updated_at
             FROM deliveries
         WHERE owner_id = $1 AND id = $2
           AND ($3::uuid[] IS NULL OR account_id = ANY($3))
         FOR UPDATE",
        scope.owner.as_uuid(),
        id.as_uuid(),
        accounts.as_deref(),
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(DataError::NotFound)?
    .try_into()
}

/// Locks every delivery of the job, whatever the account limit.
pub(crate) async fn lock_job_deliveries(
    conn: &mut PgConnection,
    job: PublishJobId,
) -> Result<Vec<Delivery>, DataError> {
    sqlx::query_as!(
        DeliveryRow,
        "SELECT id, job_id, post_id, account_id, operation, state, parked_reason,
                dispatch_gen, attempt_count, next_attempt_at, remote_id, remote_url,
                remote_publish_at, last_error, cancel_requested_at, direct_counts,
                created_at, updated_at
             FROM deliveries
         WHERE job_id = $1
         ORDER BY id
         FOR UPDATE",
        job.as_uuid(),
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(Delivery::try_from)
    .collect()
}

/// Cancels a delivery the caller has locked. `actor` is `None` for a system cancel.
pub(crate) async fn cancel_loaded(
    conn: &mut PgConnection,
    delivery: &Delivery,
    actor: Option<UserId>,
    now: DateTime<Utc>,
) -> Result<CancelOutcome, DataError> {
    match delivery.state {
        DeliveryState::Pending | DeliveryState::Claimed | DeliveryState::Parked(_) => {
            cancel_now(conn, delivery, actor, now).await
        }
        // A direct delivery has no remote upload to delete: its rows stop with it.
        DeliveryState::Processing if delivery.operation == Operation::Direct => {
            cancel_now(conn, delivery, actor, now).await
        }
        DeliveryState::InFlight => {
            sqlx::query!(
                "UPDATE deliveries
                 SET cancel_requested_at = $2, updated_at = $2,
                     updated_by = COALESCE($3::uuid, updated_by)
                 WHERE id = $1",
                delivery.id.as_uuid(),
                now,
                actor.map(|actor| actor.as_uuid()),
            )
            .execute(&mut *conn)
            .await?;
            touch_job(conn, delivery.job_id, now).await?;
            Ok(CancelOutcome::Requested)
        }
        DeliveryState::Processing => Ok(delivery
            .remote_publish_at
            .map_or(CancelOutcome::NotCancellable, |remote_publish_at| {
                CancelOutcome::NeedsAgent { remote_publish_at }
            })),
        _ => Ok(CancelOutcome::NotCancellable),
    }
}

/// Moves a locked delivery to `cancelled`.
async fn cancel_now(
    conn: &mut PgConnection,
    delivery: &Delivery,
    actor: Option<UserId>,
    now: DateTime<Utc>,
) -> Result<CancelOutcome, DataError> {
    let applied = DeliveryRepo::apply(
        conn,
        Fence {
            id: delivery.id,
            dispatch_gen: delivery.dispatch_gen,
            expected: delivery.state,
        },
        DeliveryEvent::Cancel,
        TransitionData {
            actor,
            ..TransitionData::default()
        },
        now,
    )
    .await?;
    Ok(match applied {
        Applied::Done { .. } => CancelOutcome::Cancelled,
        Applied::Stale => CancelOutcome::NotCancellable,
    })
}

/// Whether the delivery's job was cancelled. Locks the delivery row first, so a cancel that is
/// committing right now is seen.
async fn job_is_cancelled(conn: &mut PgConnection, id: DeliveryId) -> Result<bool, DataError> {
    Ok(sqlx::query_scalar!(
        r#"SELECT j.cancelled_at IS NOT NULL AS "cancelled!"
           FROM deliveries d
           JOIN publish_jobs j ON j.id = d.job_id
           WHERE d.id = $1
           FOR UPDATE OF d"#,
        id.as_uuid(),
    )
    .fetch_optional(&mut *conn)
    .await?
    .unwrap_or(false))
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
