//! Quota reservations for one delivery: one unit per descriptor bucket the operation costs
//! something in, all or nothing.

use chrono::{DateTime, Datelike, Days, LocalResult, Months, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use postit_core::{AgentDescriptor, QuotaBucket, QuotaPeriod, QuotaScope};
use postit_data::DataError;
use postit_data::publishing::ClaimedDelivery;
use postit_data::publishing::quota::{
    AccountQuotaKey, AppQuotaKey, PlatformAppQuotaRepo, QuotaRepo,
};
use postit_oauth::QUOTA_KEY;
use serde_json::Value;
use sqlx::{Connection, PgConnection};

/// What a delivery does that may cost quota.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaOp {
    /// Publishes a post.
    Publish,
    /// Posts a comment.
    Comment,
}

/// One reservation in one bucket, kept so it can be given back.
#[derive(Debug, Clone)]
pub enum ReservedUnit {
    /// In the account's own bucket.
    Account {
        /// The bucket and period.
        key: AccountQuotaKey,
        /// Units taken.
        cost: i64,
    },
    /// In a bucket shared by the whole deployment.
    App {
        /// The bucket and period.
        key: AppQuotaKey,
        /// Units taken.
        cost: i64,
    },
}

/// What [`reserve`] did.
#[derive(Debug, Clone)]
pub enum Reservation {
    /// Every bucket had room; these units are now taken (none for a dry run or a free op).
    Reserved(Vec<ReservedUnit>),
    /// A bucket is used up, and nothing was taken.
    Exhausted {
        /// When the exhausted bucket's next period starts; the latest one when several are
        /// exhausted.
        retry_at: DateTime<Utc>,
    },
}

/// Reserves one unit per bucket of `descriptor` that charges `op`, for `claimed`'s account, in
/// the period that holds `now` in the bucket's reset time zone. The limit is
/// `[platforms.<id>].quota.<bucket>` in `config` when the deployment sets it (`0` means
/// unlimited), else the bucket's default. Either every bucket is charged or none is. A dry run
/// reserves nothing.
///
/// # Errors
///
/// [`DataError::Sql`] on a database failure.
pub async fn reserve(
    conn: &mut PgConnection,
    claimed: &ClaimedDelivery,
    descriptor: &AgentDescriptor,
    op: QuotaOp,
    config: &Value,
    now: DateTime<Utc>,
) -> Result<Reservation, DataError> {
    if claimed.dry_run {
        return Ok(Reservation::Reserved(Vec::new()));
    }
    let mut tx = conn.begin().await?;
    let mut units = Vec::new();
    let mut retry_at: Option<DateTime<Utc>> = None;
    for bucket in &descriptor.quota_buckets {
        let cost = i64::from(match op {
            QuotaOp::Publish => bucket.costs.publish,
            QuotaOp::Comment => bucket.costs.comment,
        });
        if cost == 0 {
            continue;
        }
        let limit = limit(bucket, config);
        let (period_start, next) = period(bucket, now);
        let (granted, unit) = match bucket.scope {
            QuotaScope::Account => {
                let key = AccountQuotaKey {
                    owner: claimed.owner_id,
                    account: claimed.account_id,
                    platform: descriptor.platform.clone(),
                    bucket: bucket.id.clone(),
                    period_start,
                };
                let granted = QuotaRepo::reserve(&mut tx, &key, cost, limit).await?;
                (granted, ReservedUnit::Account { key, cost })
            }
            QuotaScope::App => {
                let key = AppQuotaKey {
                    platform: descriptor.platform.clone(),
                    bucket: bucket.id.clone(),
                    period_start,
                };
                let granted = PlatformAppQuotaRepo::reserve(&mut tx, &key, cost, limit).await?;
                (granted, ReservedUnit::App { key, cost })
            }
        };
        if !granted {
            retry_at = Some(retry_at.map_or(next, |at| at.max(next)));
        } else if limit != 0 {
            // An unlimited bucket writes no row, so there is nothing to give back.
            units.push(unit);
        }
    }
    if let Some(retry_at) = retry_at {
        tx.rollback().await?;
        return Ok(Reservation::Exhausted { retry_at });
    }
    tx.commit().await?;
    Ok(Reservation::Reserved(units))
}

/// Gives back units [`reserve`] took, for a delivery that never reached the platform.
///
/// # Errors
///
/// [`DataError::Sql`] on a database failure.
pub async fn release(conn: &mut PgConnection, units: &[ReservedUnit]) -> Result<(), DataError> {
    for unit in units {
        match unit {
            ReservedUnit::Account { key, cost } => QuotaRepo::release(conn, key, *cost).await?,
            ReservedUnit::App { key, cost } => {
                PlatformAppQuotaRepo::release(conn, key, *cost).await?;
            }
        }
    }
    Ok(())
}

/// The deployment's override for `bucket` in `config.quota`, else the bucket's default. An
/// override that is not a whole number of zero or more is ignored.
fn limit(bucket: &QuotaBucket, config: &Value) -> i64 {
    let configured = config
        .get(QUOTA_KEY)
        .and_then(|quota| quota.get(&bucket.id))
        .and_then(|value| match value {
            Value::Number(n) => n.as_i64(),
            Value::String(s) => s.trim().parse().ok(),
            _ => None,
        })
        .filter(|limit| *limit >= 0);
    configured.unwrap_or_else(|| i64::from(bucket.default_limit))
}

/// The first day of the period holding `now` in the bucket's reset time zone, and when the
/// next period starts. An unknown time zone counts as UTC.
#[must_use]
pub fn period(bucket: &QuotaBucket, now: DateTime<Utc>) -> (NaiveDate, DateTime<Utc>) {
    let zone: Tz = bucket.reset_time_zone.parse().unwrap_or(Tz::UTC);
    let today = now.with_timezone(&zone).date_naive();
    let (start, next) = match bucket.period {
        QuotaPeriod::Day => (today, today + Days::new(1)),
        QuotaPeriod::Month => {
            let first = today.with_day(1).unwrap_or(today);
            (first, first + Months::new(1))
        }
    };
    (start, start_of(zone, next))
}

/// Midnight starting `day` in `zone`, in UTC. Where a time change skips midnight, the first
/// instant of the day that exists.
fn start_of(zone: Tz, day: NaiveDate) -> DateTime<Utc> {
    let midnight = day.and_time(chrono::NaiveTime::MIN);
    match zone.from_local_datetime(&midnight) {
        LocalResult::Single(at) | LocalResult::Ambiguous(at, _) => at.with_timezone(&Utc),
        LocalResult::None => (1..=24)
            .find_map(|hours| {
                zone.from_local_datetime(&(midnight + chrono::Duration::hours(hours)))
                    .earliest()
            })
            .map_or_else(|| midnight.and_utc(), |at| at.with_timezone(&Utc)),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use postit_core::QuotaCosts;

    fn bucket(zone: &str, period: QuotaPeriod) -> QuotaBucket {
        QuotaBucket {
            id: "posts".to_owned(),
            scope: QuotaScope::Account,
            period,
            default_limit: 10,
            reset_time_zone: zone.to_owned(),
            costs: QuotaCosts {
                publish: 1,
                comment: 0,
                inbox_poll: 0,
                metrics: 0,
            },
        }
    }

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn day_period_follows_the_reset_zone() {
        // 22:30 UTC is already the next day in Riyadh (UTC+3).
        let (start, next) = period(
            &bucket("Asia/Riyadh", QuotaPeriod::Day),
            utc("2026-10-29T22:30:00Z"),
        );
        assert_eq!(start, NaiveDate::from_ymd_opt(2026, 10, 30).unwrap());
        assert_eq!(next, utc("2026-10-30T21:00:00Z"));
    }

    #[test]
    fn month_period_starts_on_the_first() {
        let (start, next) = period(
            &bucket("America/Los_Angeles", QuotaPeriod::Month),
            utc("2026-11-15T12:00:00Z"),
        );
        assert_eq!(start, NaiveDate::from_ymd_opt(2026, 11, 1).unwrap());
        // December 1 at midnight, Pacific Standard Time (UTC-8).
        assert_eq!(next, utc("2026-12-01T08:00:00Z"));
    }

    #[test]
    fn unknown_zone_counts_as_utc() {
        let (_, next) = period(
            &bucket("Not/AZone", QuotaPeriod::Day),
            utc("2026-10-29T10:00:00Z"),
        );
        assert_eq!(next, utc("2026-10-30T00:00:00Z"));
    }

    #[test]
    fn config_overrides_the_default_limit() {
        let b = bucket("UTC", QuotaPeriod::Day);
        assert_eq!(limit(&b, &serde_json::json!({})), 10);
        assert_eq!(limit(&b, &serde_json::json!({"quota": {"posts": 3}})), 3);
        assert_eq!(limit(&b, &serde_json::json!({"quota": {"posts": "0"}})), 0);
        assert_eq!(limit(&b, &serde_json::json!({"quota": {"posts": -1}})), 10);
    }
}
