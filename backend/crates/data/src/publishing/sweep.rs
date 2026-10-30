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
use super::deliveries::{Applied, DeliveryRepo, Fence, JobStatusChange, TransitionData, touch_job};
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

/// A delivery a sweep write moved, with what the caller enqueues and tells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Swept {
    /// The delivery.
    pub delivery: DeliveryId,
    /// The generation a job for it carries: the new one for a delivery back in `pending`.
    pub dispatch_gen: i32,
    /// When that job becomes due.
    pub run_at: DateTime<Utc>,
    /// The delivery's new state. `pending` and `outcome_unknown` get a job; a delivery whose
    /// job was cancelled meanwhile ends `cancelled` and gets none.
    pub state: DeliveryState,
    /// The job's status before and after the write, so the caller records `job_partial` only
    /// when this write first made a live job `partial`.
    pub job: JobStatusChange,
}

impl Swept {
    /// The dispatch the delivery's next job carries.
    #[must_use]
    pub fn dispatch(&self) -> Dispatch {
        Dispatch {
            delivery: self.delivery,
            dispatch_gen: self.dispatch_gen,
            run_at: self.run_at,
        }
    }

    /// The landed write `applied` made with `fence`, at `now`; `None` when it was stale.
    fn from_applied(fence: Fence, applied: Applied, now: DateTime<Utc>) -> Option<Self> {
        let Applied::Done {
            state,
            redispatch,
            job,
        } = applied
        else {
            return None;
        };
        Some(Self {
            delivery: fence.id,
            dispatch_gen: redispatch.map_or(fence.dispatch_gen, |d| d.dispatch_gen),
            run_at: redispatch.map_or(now, |d| d.run_at),
            state,
            job,
        })
    }
}

/// A poll or reconcile chain whose next job is overdue, listed by [`Sweep::stalled_checks`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StalledCheck {
    /// The delivery.
    pub delivery: DeliveryId,
    /// Its generation, which the restarted job carries.
    pub dispatch_gen: i32,
    /// The chain's position (`check_seq`): the step the restarted job takes.
    pub seq: i32,
    /// `processing` (a poll) or `outcome_unknown` (a reconcile round).
    pub state: DeliveryState,
}

/// What [`Sweep::unpark`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnparkResult {
    /// Deliveries that left `parked`: back to `pending` to enqueue, or `cancelled` when their
    /// job was cancelled meanwhile.
    pub resumed: Vec<Swept>,
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
    /// Under a queue backlog a job that is only late (not lost) is listed too: it gets a second
    /// job at a new generation, and one more per `grace` while the backlog lasts. That is
    /// amplification, not duplication: the old job still carries the retired generation, so when
    /// it finally runs its claim is refused and it exits without a platform call (a cheap no-op),
    /// and only the newest generation can publish.
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
    /// they go back to `pending` and are returned to enqueue (or end `cancelled` when their job
    /// was cancelled meanwhile).
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn expired_claims(
        conn: &mut PgConnection,
        now: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<Swept>, DataError> {
        let mut tx = conn.begin().await?;
        let mut swept = Vec::new();
        for (id, generation) in expired(&mut tx, "claimed", now, limit).await? {
            let fence = Fence {
                id,
                dispatch_gen: generation,
                expected: DeliveryState::Claimed,
            };
            let applied = DeliveryRepo::apply(
                &mut tx,
                fence,
                DeliveryEvent::LeaseExpired,
                TransitionData::default(),
                now,
            )
            .await?;
            swept.extend(Swept::from_applied(fence, applied, now));
        }
        tx.commit().await?;
        Ok(swept)
    }

    /// `in_flight` deliveries whose lease ran out: the worker may have reached the platform, so
    /// each becomes `outcome_unknown`, counting an attempt (the retry budget bounds this path
    /// too), and is returned with the generation its reconcile job carries.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn expired_in_flight(
        conn: &mut PgConnection,
        now: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<Swept>, DataError> {
        let mut tx = conn.begin().await?;
        let mut swept = Vec::new();
        for (id, generation) in expired(&mut tx, "in_flight", now, limit).await? {
            let fence = Fence {
                id,
                dispatch_gen: generation,
                expected: DeliveryState::InFlight,
            };
            let applied = DeliveryRepo::apply(
                &mut tx,
                fence,
                DeliveryEvent::LeaseExpired,
                TransitionData {
                    count_attempt: true,
                    ..TransitionData::default()
                },
                now,
            )
            .await?;
            swept.extend(Swept::from_applied(fence, applied, now));
        }
        tx.commit().await?;
        Ok(swept)
    }

    /// Non-direct `processing` and `outcome_unknown` deliveries whose poll or reconcile chain's
    /// next job was due more than `grace` ago (`check_due_at`): that job was lost, or the job
    /// before it failed before enqueuing it. Each is locked, its `check_due_at` stamped `now`
    /// so it is listed at most once per `grace`, and returned for the caller to enqueue the
    /// chain's step `check_seq` again; the chain's `take_check` admits that step once, so a job
    /// that was only late and the restarted one never both run. This is not a state change:
    /// `state_changed_at` and `updated_at` stay as they are.
    ///
    /// A direct delivery in `processing` is left to [`Sweep::orphaned_direct`].
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn stalled_checks(
        conn: &mut PgConnection,
        now: DateTime<Utc>,
        grace: Duration,
        limit: i64,
    ) -> Result<Vec<StalledCheck>, DataError> {
        let cutoff = now - grace;
        let rows = sqlx::query!(
            "UPDATE deliveries SET check_due_at = $1
             WHERE id IN (
                 SELECT d.id FROM deliveries d
                 WHERE ((d.state = 'processing' AND d.operation <> 'direct')
                        OR d.state = 'outcome_unknown')
                   AND d.check_due_at <= $2
                 ORDER BY d.check_due_at
                 LIMIT $3
                 FOR UPDATE OF d SKIP LOCKED)
             RETURNING id, dispatch_gen, check_seq, state, parked_reason",
            now,
            cutoff,
            limit,
        )
        .fetch_all(&mut *conn)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(StalledCheck {
                    delivery: DeliveryId::from_uuid(row.id),
                    dispatch_gen: row.dispatch_gen,
                    seq: row.check_seq,
                    state: DeliveryState::from_parts(&row.state, row.parked_reason.as_deref())
                        .map_err(corrupt)?,
                })
            })
            .collect()
    }

    /// The owners and IDs of `active` accounts with a live, uncancelled `pending` delivery due
    /// by `due_by` and no successful check since `checked_before` (or never checked), for the
    /// account preflight. A read: the preflight's check stamps `last_checked_at`.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn preflight_accounts(
        conn: &mut PgConnection,
        due_by: DateTime<Utc>,
        checked_before: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<(UserId, AccountId)>, DataError> {
        let rows = sqlx::query!(
            "SELECT a.owner_id, a.id FROM connected_accounts a
             WHERE a.status = 'active'
               AND (a.last_checked_at IS NULL OR a.last_checked_at < $2)
               AND EXISTS (
                   SELECT 1 FROM deliveries d
                   JOIN publish_jobs j ON j.id = d.job_id
                   WHERE d.account_id = a.id AND d.state = 'pending'
                     AND d.next_attempt_at <= $1
                     AND NOT j.dry_run AND j.cancelled_at IS NULL)
             ORDER BY a.last_checked_at NULLS FIRST, a.id
             LIMIT $3",
            due_by,
            checked_before,
            limit,
        )
        .fetch_all(&mut *conn)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| {
                (
                    UserId::from_uuid(row.owner_id),
                    AccountId::from_uuid(row.id),
                )
            })
            .collect())
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
    /// A delivery whose account is not `active` is skipped: a batch the provider refused for its
    /// credentials leaves its rows `queued` and marks the account, and handing it a batch every
    /// `grace` would only call the provider again. It is listed once the account is `active`.
    /// Such a delivery never parks, so `unpark`'s `park_grace` and `delivery_stale` do not apply:
    /// the owner has the critical `account_needs_reconnect` notice, and the campaign resumes on
    /// reconnect however late that is.
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
                   AND EXISTS (SELECT 1 FROM connected_accounts a
                               WHERE a.id = d.account_id AND a.status = 'active')
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
    /// switch is on, or the quota bucket's `parked_until` has come. Staleness still counts from
    /// `next_attempt_at`, the delivery's due time, for every reason. `outcome_unresolvable` is
    /// never cleared here; only the user settles it.
    ///
    /// A cleared delivery due within `park_grace` goes back to `pending` (or ends `cancelled`
    /// when its job was cancelled meanwhile) and is returned in `resumed`. An older one stays
    /// parked, gets `parked_notified_at`, and is returned in `needs_owner` that one time.
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
                      OR (d.parked_reason = 'quota_exceeded' AND d.parked_until <= $1))
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
            let fence = Fence {
                id,
                dispatch_gen: row.dispatch_gen,
                expected: DeliveryState::Parked(reason),
            };
            let applied = DeliveryRepo::apply(
                &mut tx,
                fence,
                DeliveryEvent::Unpark,
                TransitionData::default(),
                now,
            )
            .await?;
            result
                .resumed
                .extend(Swept::from_applied(fence, applied, now));
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
