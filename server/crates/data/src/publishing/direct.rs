//! Direct sends: the email recipients of one `direct` delivery, worked through in leased batches.
//!
//! A direct delivery fans out into one `direct_sends` row per subscribed, unsuppressed contact
//! ([`DirectRepo::materialize`]). Workers lease batches of queued rows ([`DirectRepo::claim_batch`]),
//! send them, and record each result ([`DirectRepo::record`]). When no row is queued the delivery
//! is published ([`DirectRepo::finish_if_done`]). Rows hold address hashes, never addresses.

use chrono::{DateTime, Duration, Utc};
use postit_core::{
    ContactId, ContactListId, DeliveryEvent, DeliveryId, DirectBatchId, DirectSendId, PublishJobId,
    UserId,
};
use serde::Serialize;
use sqlx::{Connection, PgConnection};

use super::deliveries::{Applied, ClaimedDelivery, DeliveryRepo, Fence, TransitionData, touch_job};
use super::quota::{AccountQuotaKey, QuotaRepo};
use crate::OwnerScope;
use crate::contacts::{SuppressionReason, SuppressionRepo};
use crate::error::DataError;
use crate::text::corrupt;

/// Who a direct send goes to.
#[derive(Debug, Clone, Default)]
pub struct Audience {
    /// The contact lists to send to. A contact in several is sent to once. No lists, no one.
    pub lists: Vec<ContactListId>,
    /// Tags a contact must all carry. Empty means no tag filter.
    pub tags: Vec<String>,
}

/// A queued row leased to a batch. It holds no address; load addresses through the contacts
/// repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectSendRow {
    /// The row.
    pub id: DirectSendId,
    /// The contact, or `None` once the contact was deleted.
    pub contact_id: Option<ContactId>,
    /// The address hash, as stored on the contact and in suppressions.
    pub email_hash: Vec<u8>,
}

/// What [`DirectRepo::claim_batch`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchClaim {
    /// Rows leased to a new batch, ordered by ID.
    Rows {
        /// The batch the rows are leased to; pass it to [`DirectRepo::record`] and
        /// [`DirectRepo::renew_batch`].
        batch: DirectBatchId,
        /// The leased rows.
        rows: Vec<DirectSendRow>,
    },
    /// Rows are still queued but the owner's daily cap has no room left.
    CapReached,
    /// No queued row is free to lease.
    Empty,
    /// The delivery is no longer `processing` under the caller's dispatch generation: it was
    /// cancelled, swept, or re-dispatched. Nothing was leased and no cap unit was used.
    Stale,
}

/// What happened to one recipient.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectResult {
    /// The provider accepted the message.
    Sent {
        /// The provider's message ID.
        provider_message_id: Option<String>,
    },
    /// Sending failed for good.
    Failed {
        /// A short reason code, never an address.
        code: String,
    },
    /// The address was suppressed after the rows were queued.
    Suppressed,
    /// The address bounced.
    Bounced {
        /// A short reason code, never an address.
        code: String,
    },
}

/// How far a direct delivery has got.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct DirectCounts {
    /// Recipients the delivery was materialized for.
    pub audience: i64,
    /// Rows still waiting.
    pub queued: i64,
    /// Rows the provider accepted.
    pub sent: i64,
    /// Rows that failed, including cancelled ones.
    pub failed: i64,
    /// Rows skipped because the address is suppressed.
    pub suppressed: i64,
    /// Rows whose address bounced.
    pub bounced: i64,
}

/// Direct-send queries.
pub struct DirectRepo;

impl DirectRepo {
    /// Creates the delivery's rows from `audience` and returns how many recipients there are.
    ///
    /// Only `subscribed` contacts that are not on the suppression list are included, each once.
    /// The count is stored in `direct_counts`. Calling it again inserts nothing and returns the
    /// stored audience.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure, [`DataError::NotFound`] when the delivery is
    /// gone.
    pub async fn materialize(
        conn: &mut PgConnection,
        delivery: &ClaimedDelivery,
        audience: &Audience,
        now: DateTime<Utc>,
    ) -> Result<i64, DataError> {
        let mut tx = conn.begin().await?;
        let stored = sqlx::query_scalar!(
            "SELECT direct_counts FROM deliveries WHERE id = $1 FOR UPDATE",
            delivery.id.as_uuid(),
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DataError::NotFound)?;
        if let Some(existing) = stored
            .as_ref()
            .and_then(|counts| counts.get("audience"))
            .and_then(serde_json::Value::as_i64)
        {
            return Ok(existing);
        }
        let lists: Vec<_> = audience.lists.iter().map(ContactListId::as_uuid).collect();
        let inserted = sqlx::query!(
            "INSERT INTO direct_sends (id, owner_id, delivery_id, contact_id, email_hash)
             SELECT DISTINCT ON (c.id) uuidv7(), c.owner_id, $2, c.id, c.email_hash
             FROM contacts c
             JOIN contact_list_members m ON m.contact_id = c.id AND m.owner_id = c.owner_id
             WHERE c.owner_id = $1 AND m.list_id = ANY($3) AND c.status = 'subscribed'
               AND c.tags @> $4
               AND NOT EXISTS (SELECT 1 FROM suppressions s
                               WHERE s.owner_id = c.owner_id AND s.email_hash = c.email_hash)
             ORDER BY c.id
             ON CONFLICT (delivery_id, contact_id) DO NOTHING",
            delivery.owner_id.as_uuid(),
            delivery.id.as_uuid(),
            &lists,
            &audience.tags,
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        let audience_size = i64::try_from(inserted).map_err(corrupt)?;
        sqlx::query!(
            "UPDATE deliveries SET direct_counts = $2 WHERE id = $1",
            delivery.id.as_uuid(),
            serde_json::json!({ "audience": audience_size, "queued": audience_size }),
        )
        .execute(&mut *tx)
        .await?;
        touch_job(&mut tx, delivery.job_id, now).await?;
        tx.commit().await?;
        Ok(audience_size)
    }

    /// Leases the next batch of up to `size` queued rows to a new batch until `now + lease`.
    ///
    /// The batch ID is allocated here, so it cannot repeat across workers, restarts, or retries
    /// of a job. The claim is fenced: unless the delivery is still `processing` under
    /// `fence.dispatch_gen` it returns [`BatchClaim::Stale`] and leases nothing. The fence is held
    /// with a share lock, so concurrent claims of one delivery run in parallel while a cancel or
    /// transition waits for them. A worker that sends for longer than `lease` calls
    /// [`DirectRepo::renew_batch`].
    ///
    /// With a `cap` of `(key, limit)` it reserves up to `size` units from the daily cap and
    /// leases no more rows than it got, returning the units it did not use. A `limit` of `0` is
    /// unlimited. Rows whose lease expired are leased again, so a crashed worker's batch is not
    /// lost; the units that batch reserved are not given back, because some of its mail may have
    /// gone out but not been recorded, and the claim that takes the rows over reserves again. The
    /// cap is therefore over-counted after a crash, never under-counted, which keeps the owner
    /// inside the provider's limit. The cost is visible: after a crash a user may see their day's
    /// cap used up with recipients left for tomorrow.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn claim_batch(
        conn: &mut PgConnection,
        fence: Fence,
        size: i64,
        now: DateTime<Utc>,
        lease: Duration,
        cap: Option<(&AccountQuotaKey, i64)>,
    ) -> Result<BatchClaim, DataError> {
        let delivery = fence.id;
        let mut tx = conn.begin().await?;
        let current = sqlx::query_scalar!(
            "SELECT 1 FROM deliveries
             WHERE id = $1 AND dispatch_gen = $2 AND state = 'processing' FOR SHARE",
            delivery.as_uuid(),
            fence.dispatch_gen,
        )
        .fetch_optional(&mut *tx)
        .await?;
        if current.is_none() {
            return Ok(BatchClaim::Stale);
        }
        let granted = match cap {
            Some((key, limit)) => QuotaRepo::reserve_up_to(&mut tx, key, size, limit).await?,
            None => size,
        };
        if granted <= 0 {
            let queued = has_queued(&mut tx, delivery).await?;
            tx.commit().await?;
            return Ok(if queued && size > 0 {
                BatchClaim::CapReached
            } else {
                BatchClaim::Empty
            });
        }
        let batch = sqlx::query_scalar!(r#"SELECT uuidv7() AS "id!""#)
            .fetch_one(&mut *tx)
            .await?;
        let mut rows: Vec<DirectSendRow> = sqlx::query!(
            "UPDATE direct_sends SET batch = $2, lease_expires_at = $4, updated_at = $3
             WHERE id IN (
                 SELECT id FROM direct_sends
                 WHERE delivery_id = $1 AND state = 'queued'
                   AND (lease_expires_at IS NULL OR lease_expires_at < $3)
                 ORDER BY id
                 LIMIT $5
                 FOR UPDATE SKIP LOCKED)
             RETURNING id, contact_id, email_hash",
            delivery.as_uuid(),
            batch,
            now,
            now + lease,
            granted,
        )
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|row| DirectSendRow {
            id: DirectSendId::from_uuid(row.id),
            contact_id: row.contact_id.map(ContactId::from_uuid),
            email_hash: row.email_hash,
        })
        .collect();
        rows.sort_by_key(|row| row.id.as_uuid());
        let leased = i64::try_from(rows.len()).map_err(corrupt)?;
        if let Some((key, limit)) = cap
            && limit != 0
            && leased < granted
        {
            QuotaRepo::release(&mut tx, key, granted - leased).await?;
        }
        tx.commit().await?;
        Ok(if rows.is_empty() {
            BatchClaim::Empty
        } else {
            BatchClaim::Rows {
                batch: DirectBatchId::from_uuid(batch),
                rows,
            }
        })
    }

    /// Extends the lease of the rows `batch` still holds to `now + lease`, and returns how many
    /// it renewed. `0` means the batch lost all its rows (reclaimed, recorded, or cancelled), and
    /// the worker stops.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn renew_batch(
        conn: &mut PgConnection,
        delivery: DeliveryId,
        batch: DirectBatchId,
        now: DateTime<Utc>,
        lease: Duration,
    ) -> Result<u64, DataError> {
        Ok(sqlx::query!(
            "UPDATE direct_sends SET lease_expires_at = $3, updated_at = $4
             WHERE delivery_id = $1 AND batch = $2 AND state = 'queued'
               AND lease_expires_at IS NOT NULL",
            delivery.as_uuid(),
            batch.as_uuid(),
            now + lease,
            now,
        )
        .execute(&mut *conn)
        .await?
        .rows_affected())
    }

    /// Notes that the campaign has hit the owner's daily cap and continues at `resume_at`, which
    /// the delivery shows as "continues at". The delivery stays `processing` (plan 03: this is not
    /// a parking state) and its queued rows stay as they are. Fenced like a transition, but
    /// changes no state. Returns `false` when the fence no longer matches.
    ///
    /// `resume_at` is computed by the caller from the same period definition it used to build the
    /// [`AccountQuotaKey`].
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn wait_for_cap(
        conn: &mut PgConnection,
        fence: Fence,
        resume_at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<bool, DataError> {
        let mut tx = conn.begin().await?;
        let Some(job) = sqlx::query_scalar!(
            "UPDATE deliveries SET next_attempt_at = $3, updated_at = $4
             WHERE id = $1 AND dispatch_gen = $2 AND state = 'processing'
             RETURNING job_id",
            fence.id.as_uuid(),
            fence.dispatch_gen,
            resume_at,
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

    /// Records what happened to a row leased to `batch`. Returns `false`, changing nothing, when
    /// the row is no longer queued under that batch: another worker took it over, or it was
    /// already recorded. A `Bounced` result also suppresses the row's address.
    ///
    /// A late result wins over a cancel: cancelling the delivery marks every queued row `failed`
    /// with code `cancelled`, including rows a live batch is sending, and when that batch
    /// reports what really happened the result replaces the cancel and the delivery's stored
    /// counts are refreshed. A row that was never leased has no batch and stays cancelled.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn record(
        conn: &mut PgConnection,
        id: DirectSendId,
        batch: DirectBatchId,
        result: DirectResult,
        now: DateTime<Utc>,
    ) -> Result<bool, DataError> {
        let (state, provider_message_id, code) = match &result {
            DirectResult::Sent {
                provider_message_id,
            } => ("sent", provider_message_id.clone(), None),
            DirectResult::Failed { code } => ("failed", None, Some(code.clone())),
            DirectResult::Suppressed => ("suppressed", None, None),
            DirectResult::Bounced { code } => ("bounced", None, Some(code.clone())),
        };
        let mut tx = conn.begin().await?;
        let Some(row) = sqlx::query!(
            r#"WITH old AS (
                   SELECT id, state FROM direct_sends WHERE id = $1 FOR UPDATE
               )
               UPDATE direct_sends s
               SET state = $3, provider_message_id = $4, error_code = $5, lease_expires_at = NULL,
                   sent_at = CASE WHEN $3 = 'sent' THEN $6::timestamptz END,
                   updated_at = $6::timestamptz
               FROM old
               WHERE s.id = old.id AND s.batch = $2
                 AND ((s.state = 'queued' AND s.lease_expires_at IS NOT NULL)
                      OR (s.state = 'failed' AND s.error_code = 'cancelled'))
               RETURNING s.owner_id, s.delivery_id, s.email_hash,
                         (old.state = 'failed') AS "replaced_cancel!""#,
            id.as_uuid(),
            batch.as_uuid(),
            state,
            provider_message_id,
            code,
            now,
        )
        .fetch_optional(&mut *tx)
        .await?
        else {
            return Ok(false);
        };
        if matches!(result, DirectResult::Bounced { .. }) {
            let hash: [u8; 32] = row.email_hash.try_into().map_err(|_| {
                DataError::Sql(sqlx::Error::Decode(
                    "direct send email_hash is not 32 bytes".into(),
                ))
            })?;
            let owner = OwnerScope::own(UserId::from_uuid(row.owner_id));
            SuppressionRepo::add_hash(&mut tx, &owner, hash, SuppressionReason::Bounced).await?;
        }
        if row.replaced_cancel {
            // The cancel is committed and its locks are gone, so taking the delivery row after
            // the direct send cannot deadlock with it.
            Self::store_counts(&mut tx, DeliveryId::from_uuid(row.delivery_id)).await?;
        }
        tx.commit().await?;
        Ok(true)
    }

    /// Stores the live counts in the delivery's `direct_counts`.
    pub(crate) async fn store_counts(
        conn: &mut PgConnection,
        delivery: DeliveryId,
    ) -> Result<(), DataError> {
        let counts = Self::progress(conn, delivery).await?;
        sqlx::query!(
            "UPDATE deliveries SET direct_counts = $2 WHERE id = $1",
            delivery.as_uuid(),
            serde_json::to_value(counts).map_err(corrupt)?,
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// Where the delivery stands: the stored audience and the live count of each row state.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn progress(
        conn: &mut PgConnection,
        delivery: DeliveryId,
    ) -> Result<DirectCounts, DataError> {
        let row = sqlx::query!(
            r#"SELECT
                   COALESCE((d.direct_counts ->> 'audience')::bigint, 0) AS "audience!",
                   count(*) FILTER (WHERE s.state = 'queued') AS "queued!",
                   count(*) FILTER (WHERE s.state = 'sent') AS "sent!",
                   count(*) FILTER (WHERE s.state = 'failed') AS "failed!",
                   count(*) FILTER (WHERE s.state = 'suppressed') AS "suppressed!",
                   count(*) FILTER (WHERE s.state = 'bounced') AS "bounced!"
               FROM deliveries d
               LEFT JOIN direct_sends s ON s.delivery_id = d.id
               WHERE d.id = $1
               GROUP BY d.id"#,
            delivery.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?;
        Ok(DirectCounts {
            audience: row.audience,
            queued: row.queued,
            sent: row.sent,
            failed: row.failed,
            suppressed: row.suppressed,
            bounced: row.bounced,
        })
    }

    /// Publishes the delivery, storing the final counts, when no row is left queued. Returns
    /// `None` while rows are still queued.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure, or the errors of [`DeliveryRepo::apply`].
    pub async fn finish_if_done(
        conn: &mut PgConnection,
        fence: Fence,
        now: DateTime<Utc>,
    ) -> Result<Option<Applied>, DataError> {
        let mut tx = conn.begin().await?;
        let counts = Self::progress(&mut tx, fence.id).await?;
        if counts.queued > 0 {
            return Ok(None);
        }
        let applied = DeliveryRepo::apply(
            &mut tx,
            fence,
            DeliveryEvent::Published,
            TransitionData {
                direct_counts: Some(serde_json::to_value(counts).map_err(corrupt)?),
                ..TransitionData::default()
            },
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(Some(applied))
    }

    /// Marks every queued row `failed` with code `cancelled`, so a cancelled delivery stops
    /// sending. Returns how many rows it stopped. [`DeliveryRepo::apply`] already calls it on
    /// every cancel of a direct delivery.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn cancel_queued(
        conn: &mut PgConnection,
        delivery: DeliveryId,
        now: DateTime<Utc>,
    ) -> Result<i64, DataError> {
        let stopped = sqlx::query!(
            "UPDATE direct_sends
             SET state = 'failed', error_code = 'cancelled', lease_expires_at = NULL,
                 updated_at = $2
             WHERE delivery_id = $1 AND state = 'queued'",
            delivery.as_uuid(),
            now,
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        i64::try_from(stopped).map_err(corrupt)
    }
}

async fn has_queued(conn: &mut PgConnection, delivery: DeliveryId) -> Result<bool, DataError> {
    Ok(sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM direct_sends WHERE delivery_id = $1 AND state = 'queued'
           ) AS "queued!""#,
        delivery.as_uuid(),
    )
    .fetch_one(&mut *conn)
    .await?)
}
