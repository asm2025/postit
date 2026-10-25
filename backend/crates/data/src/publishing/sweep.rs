//! The delivery-due sweep: system queries that find deliveries nobody is working on.
//!
//! The sweep holds no [`crate::OwnerScope`]. Every function takes `now` and a `limit`, picks its
//! rows with `FOR UPDATE SKIP LOCKED` so two overlapping sweeps never take the same delivery,
//! and returns what the caller must enqueue. Nothing here enqueues work.

use chrono::{DateTime, Duration, Utc};
use postit_core::{
    AccountId, DeliveryEvent, DeliveryId, DeliveryState, ParkReason, PlatformId, PublishJobId,
    UserId,
};
use sqlx::{Connection, PgConnection};

use super::Dispatch;
use super::deliveries::{Applied, DeliveryRepo, Fence, TransitionData, touch_job};
use crate::error::DataError;
use crate::text::corrupt;

/// What [`Sweep::unpark`] looks at.
#[derive(Debug, Clone, Copy)]
pub struct UnparkInput<'a> {
    /// The time of the sweep.
    pub now: DateTime<Utc>,
    /// How long after its due time a cleared delivery is still sent. A delivery older than this
    /// stays parked and the owner is asked.
    pub park_grace: Duration,
    /// The platforms this build has an agent for.
    pub compiled_platforms: &'a [PlatformId],
    /// The most deliveries to examine.
    pub limit: i64,
}

/// What [`Sweep::unpark`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnparkResult {
    /// Deliveries that went back to `pending`, to enqueue.
    pub resumed: Vec<Dispatch>,
    /// Deliveries whose blocker cleared too long after their due time. They stay parked, and each
    /// is returned here once so the owner can be told.
    pub needs_owner: Vec<(UserId, DeliveryId)>,
}

/// System queries over deliveries nobody is working on.
pub struct Sweep;

impl Sweep {
    /// `pending` deliveries that were due more than `grace` ago and untouched since, meaning
    /// their queued job was lost. Each gets a new generation, which retires any job still
    /// carrying the old one, and is returned to enqueue at `now`.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn overdue_pending(
        conn: &mut PgConnection,
        now: DateTime<Utc>,
        grace: Duration,
        limit: i64,
    ) -> Result<Vec<Dispatch>, DataError> {
        let cutoff = now - grace;
        let mut tx = conn.begin().await?;
        let rows = sqlx::query!(
            "UPDATE deliveries SET dispatch_gen = dispatch_gen + 1, updated_at = $1
             WHERE id IN (
                 SELECT d.id FROM deliveries d
                 JOIN publish_jobs j ON j.id = d.job_id
                 WHERE d.state = 'pending' AND d.next_attempt_at <= $2 AND d.updated_at <= $2
                   AND j.cancelled_at IS NULL
                 ORDER BY d.next_attempt_at
                 LIMIT $3
                 FOR UPDATE OF d SKIP LOCKED)
             RETURNING id, job_id, dispatch_gen",
            now,
            cutoff,
            limit,
        )
        .fetch_all(&mut *tx)
        .await?;
        let mut jobs: Vec<_> = rows.iter().map(|row| row.job_id).collect();
        jobs.sort_unstable();
        jobs.dedup();
        for job in jobs {
            touch_job(&mut tx, PublishJobId::from_uuid(job), now).await?;
        }
        tx.commit().await?;
        Ok(rows
            .into_iter()
            .map(|row| Dispatch {
                delivery: DeliveryId::from_uuid(row.id),
                dispatch_gen: row.dispatch_gen,
                run_at: now,
            })
            .collect())
    }

    /// `claimed` deliveries whose lease ran out: the worker died before calling the platform, so
    /// they go back to `pending` and are returned to enqueue.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn expired_claims(
        conn: &mut PgConnection,
        now: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<Dispatch>, DataError> {
        let mut tx = conn.begin().await?;
        let mut dispatches = Vec::new();
        for (id, generation) in expired(&mut tx, "claimed", now, limit).await? {
            let applied = DeliveryRepo::apply(
                &mut tx,
                Fence {
                    id,
                    dispatch_gen: generation,
                    expected: DeliveryState::Claimed,
                },
                DeliveryEvent::LeaseExpired,
                TransitionData::default(),
                now,
            )
            .await?;
            if let Applied::Done {
                redispatch: Some(dispatch),
                ..
            } = applied
            {
                dispatches.push(dispatch);
            }
        }
        tx.commit().await?;
        Ok(dispatches)
    }

    /// `in_flight` deliveries whose lease ran out: the worker may have reached the platform, so
    /// each becomes `outcome_unknown` and is returned for a reconcile job.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn expired_in_flight(
        conn: &mut PgConnection,
        now: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<DeliveryId>, DataError> {
        let mut tx = conn.begin().await?;
        let mut moved = Vec::new();
        for (id, generation) in expired(&mut tx, "in_flight", now, limit).await? {
            let applied = DeliveryRepo::apply(
                &mut tx,
                Fence {
                    id,
                    dispatch_gen: generation,
                    expected: DeliveryState::InFlight,
                },
                DeliveryEvent::LeaseExpired,
                TransitionData::default(),
                now,
            )
            .await?;
            if matches!(applied, Applied::Done { .. }) {
                moved.push(id);
            }
        }
        tx.commit().await?;
        Ok(moved)
    }

    /// `processing` direct deliveries that still have `queued` rows but none under a live lease:
    /// the batch job that was working through them is gone, so the caller enqueues the next one.
    ///
    /// A delivery is skipped until it has been quiet for `grace`: one that just reached
    /// `processing` has no lease yet, and one waiting for its daily cap
    /// ([`crate::publishing::direct::DirectRepo::wait_for_cap`]) carries the time it continues in
    /// `next_attempt_at`, so it is listed only once that time plus `grace` has passed. Each listed
    /// delivery is locked, stamped, and its job touched, so overlapping sweeps never list one
    /// twice and a delivery is listed at most once per `grace`.
    ///
    /// Each comes back as the dispatch the next `direct_send_batch` job carries: the delivery
    /// and its current generation, to run at `now`.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn orphaned_direct(
        conn: &mut PgConnection,
        now: DateTime<Utc>,
        grace: Duration,
        limit: i64,
    ) -> Result<Vec<Dispatch>, DataError> {
        let cutoff = now - grace;
        let mut tx = conn.begin().await?;
        let rows = sqlx::query!(
            "UPDATE deliveries SET updated_at = $1
             WHERE id IN (
                 SELECT d.id FROM deliveries d
                 WHERE d.operation = 'direct' AND d.state = 'processing'
                   AND d.updated_at <= $2
                   AND (d.next_attempt_at IS NULL OR d.next_attempt_at <= $2)
                   AND EXISTS (SELECT 1 FROM direct_sends s
                               WHERE s.delivery_id = d.id AND s.state = 'queued')
                   AND NOT EXISTS (SELECT 1 FROM direct_sends s
                                   WHERE s.delivery_id = d.id AND s.state = 'queued'
                                     AND s.lease_expires_at > $1)
                 ORDER BY d.updated_at
                 LIMIT $3
                 FOR UPDATE OF d SKIP LOCKED)
             RETURNING id, job_id, dispatch_gen",
            now,
            cutoff,
            limit,
        )
        .fetch_all(&mut *tx)
        .await?;
        let mut jobs: Vec<_> = rows.iter().map(|row| row.job_id).collect();
        jobs.sort_unstable();
        jobs.dedup();
        for job in jobs {
            touch_job(&mut tx, PublishJobId::from_uuid(job), now).await?;
        }
        tx.commit().await?;
        Ok(rows
            .into_iter()
            .map(|row| Dispatch {
                delivery: DeliveryId::from_uuid(row.id),
                dispatch_gen: row.dispatch_gen,
                run_at: now,
            })
            .collect())
    }

    /// Resumes `parked` deliveries whose blocker is gone.
    ///
    /// A blocker is gone when the owner is `active` again, the account is `active` again, the
    /// platform has an agent in this build, an admin re-enabled the platform, the live-publish
    /// switch is on, or the quota bucket's retry time has come. `outcome_unresolvable` is never
    /// cleared here; only the user settles it.
    ///
    /// A cleared delivery due within `park_grace` goes back to `pending` and is returned in
    /// `resumed`. An older one stays parked, gets `parked_notified_at`, and is returned in
    /// `needs_owner` that one time.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn unpark(
        conn: &mut PgConnection,
        input: UnparkInput<'_>,
    ) -> Result<UnparkResult, DataError> {
        let UnparkInput {
            now,
            park_grace,
            compiled_platforms,
            limit,
        } = input;
        let compiled: Vec<String> = compiled_platforms
            .iter()
            .map(|platform| platform.as_str().to_string())
            .collect();
        let mut tx = conn.begin().await?;
        let candidates = sqlx::query!(
            r#"SELECT d.id, d.owner_id, d.dispatch_gen, d.next_attempt_at, d.parked_notified_at,
                      d.parked_reason AS "parked_reason!"
               FROM deliveries d
               JOIN users u ON u.id = d.owner_id
               JOIN connected_accounts a ON a.id = d.account_id
               JOIN publish_jobs j ON j.id = d.job_id
               LEFT JOIN platforms p ON p.id = a.platform_id
               WHERE d.state = 'parked'
                 AND NOT (d.parked_notified_at IS NOT NULL AND d.next_attempt_at < $4)
                 AND (   (d.parked_reason = 'owner_disabled' AND u.status = 'active')
                      OR (d.parked_reason = 'account_needs_reconnect' AND a.status = 'active')
                      OR (d.parked_reason = 'platform_unavailable'
                          AND a.platform_id = ANY($2::text[]))
                      OR (d.parked_reason = 'platform_disabled' AND COALESCE(p.enabled, FALSE))
                      OR (d.parked_reason = 'live_publish_disabled'
                          AND (j.dry_run OR COALESCE(
                                (SELECT value = 'true'::jsonb FROM app_settings
                                 WHERE key = 'publishing.live'), FALSE)))
                      OR (d.parked_reason = 'quota_exceeded' AND d.next_attempt_at <= $1))
               ORDER BY d.next_attempt_at NULLS FIRST
               LIMIT $3
               FOR UPDATE OF d SKIP LOCKED"#,
            now,
            &compiled,
            limit,
            now - park_grace,
        )
        .fetch_all(&mut *tx)
        .await?;

        let mut result = UnparkResult::default();
        for row in candidates {
            let id = DeliveryId::from_uuid(row.id);
            let reason: ParkReason = row.parked_reason.parse().map_err(corrupt)?;
            let stale = row
                .next_attempt_at
                .is_some_and(|due| now - due > park_grace);
            if stale {
                if row.parked_notified_at.is_none() {
                    sqlx::query!(
                        "UPDATE deliveries SET parked_notified_at = $2 WHERE id = $1",
                        row.id,
                        now,
                    )
                    .execute(&mut *tx)
                    .await?;
                    result
                        .needs_owner
                        .push((UserId::from_uuid(row.owner_id), id));
                }
                continue;
            }
            let applied = DeliveryRepo::apply(
                &mut tx,
                Fence {
                    id,
                    dispatch_gen: row.dispatch_gen,
                    expected: DeliveryState::Parked(reason),
                },
                DeliveryEvent::Unpark,
                TransitionData::default(),
                now,
            )
            .await?;
            if let Applied::Done {
                redispatch: Some(dispatch),
                ..
            } = applied
            {
                result.resumed.push(dispatch);
            }
        }
        tx.commit().await?;
        Ok(result)
    }

    /// How many of one account's deliveries are held for a later publish time on the
    /// platform's own side. Same predicate as [`Sweep::native_scheduled_counts`], for the
    /// disconnect response.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn native_scheduled_count_for_account(
        conn: &mut PgConnection,
        account: AccountId,
    ) -> Result<i64, DataError> {
        let count = sqlx::query_scalar!(
            r#"SELECT count(*) AS "count!"
               FROM deliveries d
               JOIN publish_jobs j ON j.id = d.job_id
               WHERE d.account_id = $1 AND j.native_schedule AND d.remote_id IS NOT NULL
                 AND d.remote_publish_at > now()
                 AND d.state NOT IN ('cancelled', 'failed', 'dead_letter')"#,
            account.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(count)
    }

    /// How many deliveries each platform is holding for a later publish time on its own side.
    /// Counts only, for the kill-switch dialog and the doctor; it never lists the deliveries.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn native_scheduled_counts(
        conn: &mut PgConnection,
    ) -> Result<Vec<(PlatformId, i64)>, DataError> {
        let rows = sqlx::query!(
            r#"SELECT a.platform_id, count(*) AS "count!"
               FROM deliveries d
               JOIN publish_jobs j ON j.id = d.job_id
               JOIN connected_accounts a ON a.id = d.account_id
               WHERE j.native_schedule AND d.remote_id IS NOT NULL
                 AND d.remote_publish_at > now()
                 AND d.state NOT IN ('cancelled', 'failed', 'dead_letter')
               GROUP BY a.platform_id
               ORDER BY a.platform_id"#,
        )
        .fetch_all(&mut *conn)
        .await?;
        rows.into_iter()
            .map(|row| {
                PlatformId::new(&row.platform_id)
                    .map(|platform| (platform, row.count))
                    .map_err(corrupt)
            })
            .collect()
    }
}

/// The IDs and generations of `state` deliveries whose lease ended before `now`, locked.
async fn expired(
    conn: &mut PgConnection,
    state: &str,
    now: DateTime<Utc>,
    limit: i64,
) -> Result<Vec<(DeliveryId, i32)>, DataError> {
    let rows = sqlx::query!(
        "SELECT id, dispatch_gen FROM deliveries
         WHERE state = $1 AND lease_expires_at < $2
         ORDER BY lease_expires_at
         LIMIT $3
         FOR UPDATE SKIP LOCKED",
        state,
        now,
        limit,
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| (DeliveryId::from_uuid(row.id), row.dispatch_gen))
        .collect())
}
