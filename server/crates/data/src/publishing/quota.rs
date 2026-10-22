//! Quota reservations: per-account platform buckets, the deployment-wide platform buckets, and
//! the per-owner email daily cap.
//!
//! A reservation is one `INSERT … ON CONFLICT DO UPDATE … WHERE` statement, so concurrent
//! workers can never reserve more than the limit between them. A limit of `0` means unlimited:
//! the call grants everything and writes nothing. These are system calls and hold no
//! [`crate::OwnerScope`].

use chrono::NaiveDate;
use postit_core::{AccountId, PlatformId, UserId};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::DataError;
use crate::text::corrupt;

/// The bucket the per-owner email daily cap is counted in.
pub const EMAIL_CAP_BUCKET: &str = "daily_cap";

/// One account's usage bucket for one period.
#[derive(Debug, Clone)]
pub struct AccountQuotaKey {
    /// The account's owner.
    pub owner: UserId,
    /// The account.
    pub account: AccountId,
    /// The account's platform.
    pub platform: PlatformId,
    /// The bucket, named by the platform's agent.
    pub bucket: String,
    /// The first day of the period the bucket counts.
    pub period_start: NaiveDate,
}

/// One deployment-wide usage bucket for one period.
#[derive(Debug, Clone)]
pub struct AppQuotaKey {
    /// The platform.
    pub platform: PlatformId,
    /// The bucket, named by the platform's agent.
    pub bucket: String,
    /// The first day of the period the bucket counts.
    pub period_start: NaiveDate,
}

/// Per-account quota buckets.
pub struct QuotaRepo;

impl QuotaRepo {
    /// Reserves `cost` units against `limit`. Returns `false`, changing nothing, when that would
    /// pass the limit. A `limit` of `0` is unlimited and a `cost` of `0` or less is free; neither
    /// writes a row.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn reserve(
        conn: &mut PgConnection,
        key: &AccountQuotaKey,
        cost: i64,
        limit: i64,
    ) -> Result<bool, DataError> {
        if limit == 0 || cost <= 0 {
            return Ok(true);
        }
        let reserved = sqlx::query_scalar!(
            "INSERT INTO platform_quota_usage
                 (id, owner_id, account_id, platform_id, bucket, period_start, units_used)
             SELECT $1, $2, $3, $4, $5, $6, $7::bigint WHERE $7::bigint <= $8::bigint
             ON CONFLICT ON CONSTRAINT platform_quota_usage_key DO UPDATE
                 SET units_used = platform_quota_usage.units_used + $7, updated_at = now()
                 WHERE platform_quota_usage.units_used + $7::bigint <= $8::bigint
             RETURNING units_used",
            Uuid::now_v7(),
            key.owner.as_uuid(),
            key.account.as_uuid(),
            key.platform.as_str(),
            key.bucket,
            key.period_start,
            cost,
            limit,
        )
        .fetch_optional(&mut *conn)
        .await?;
        Ok(reserved.is_some())
    }

    /// Gives back `cost` units, never taking the bucket below zero.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn release(
        conn: &mut PgConnection,
        key: &AccountQuotaKey,
        cost: i64,
    ) -> Result<(), DataError> {
        sqlx::query!(
            "UPDATE platform_quota_usage
             SET units_used = GREATEST(units_used - $5, 0), updated_at = now()
             WHERE account_id = $1 AND platform_id = $2 AND bucket = $3 AND period_start = $4",
            key.account.as_uuid(),
            key.platform.as_str(),
            key.bucket,
            key.period_start,
            cost.max(0),
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// Reserves as many of `want` units as `limit` still allows and returns that many, possibly
    /// none. A `limit` of `0` is unlimited: it returns `want` and writes nothing.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn reserve_up_to(
        conn: &mut PgConnection,
        key: &AccountQuotaKey,
        want: i64,
        limit: i64,
    ) -> Result<i64, DataError> {
        if limit == 0 || want <= 0 {
            return Ok(want.max(0));
        }
        // Make sure the row exists, then lock it with `FOR UPDATE` so the grant is computed from
        // a value no other reservation can change before this statement commits.
        sqlx::query!(
            "INSERT INTO platform_quota_usage
                 (id, owner_id, account_id, platform_id, bucket, period_start)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT ON CONSTRAINT platform_quota_usage_key DO NOTHING",
            Uuid::now_v7(),
            key.owner.as_uuid(),
            key.account.as_uuid(),
            key.platform.as_str(),
            key.bucket,
            key.period_start,
        )
        .execute(&mut *conn)
        .await?;
        let granted = sqlx::query_scalar!(
            r#"UPDATE platform_quota_usage t
               SET units_used = o.used + LEAST($5, GREATEST($6 - o.used, 0)), updated_at = now()
               FROM (SELECT id, units_used AS used FROM platform_quota_usage
                     WHERE account_id = $1 AND platform_id = $2 AND bucket = $3
                       AND period_start = $4
                     FOR UPDATE) o
               WHERE t.id = o.id
               RETURNING t.units_used - o.used AS "granted!""#,
            key.account.as_uuid(),
            key.platform.as_str(),
            key.bucket,
            key.period_start,
            want,
            limit,
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(granted)
    }
}

/// Deployment-wide platform quota buckets, shared by every user. Rows hold no user reference.
pub struct PlatformAppQuotaRepo;

impl PlatformAppQuotaRepo {
    /// Reserves `cost` units against `limit`. Returns `false`, changing nothing, when that would
    /// pass the limit. A `limit` of `0` is unlimited and writes nothing.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn reserve(
        conn: &mut PgConnection,
        key: &AppQuotaKey,
        cost: i64,
        limit: i64,
    ) -> Result<bool, DataError> {
        if limit == 0 || cost <= 0 {
            return Ok(true);
        }
        let reserved = sqlx::query_scalar!(
            "INSERT INTO platform_app_quota_usage (platform_id, bucket, period_start, units_used)
             SELECT $1, $2, $3, $4::bigint WHERE $4::bigint <= $5::bigint
             ON CONFLICT (platform_id, bucket, period_start) DO UPDATE
                 SET units_used = platform_app_quota_usage.units_used + $4, updated_at = now()
                 WHERE platform_app_quota_usage.units_used + $4::bigint <= $5::bigint
             RETURNING units_used",
            key.platform.as_str(),
            key.bucket,
            key.period_start,
            cost,
            limit,
        )
        .fetch_optional(&mut *conn)
        .await?;
        Ok(reserved.is_some())
    }

    /// Gives back `cost` units, never taking the bucket below zero.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn release(
        conn: &mut PgConnection,
        key: &AppQuotaKey,
        cost: i64,
    ) -> Result<(), DataError> {
        sqlx::query!(
            "UPDATE platform_app_quota_usage
             SET units_used = GREATEST(units_used - $4, 0), updated_at = now()
             WHERE platform_id = $1 AND bucket = $2 AND period_start = $3",
            key.platform.as_str(),
            key.bucket,
            key.period_start,
            cost.max(0),
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// Every bucket's usage, newest period first.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure, or on a stored platform ID that does not parse.
    pub async fn usage(
        conn: &mut PgConnection,
    ) -> Result<Vec<(PlatformId, String, NaiveDate, i64)>, DataError> {
        let rows = sqlx::query!(
            "SELECT platform_id, bucket, period_start, units_used
             FROM platform_app_quota_usage
             ORDER BY period_start DESC, platform_id, bucket",
        )
        .fetch_all(&mut *conn)
        .await?;
        rows.into_iter()
            .map(|row| {
                PlatformId::new(&row.platform_id)
                    .map(|platform| (platform, row.bucket, row.period_start, row.units_used))
                    .map_err(corrupt)
            })
            .collect()
    }
}
