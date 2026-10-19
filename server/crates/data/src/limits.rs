//! Per-user limits: defaults copied in at provisioning, edited per user by an admin.

use chrono::{DateTime, Utc};
use postit_config::Settings;
use postit_core::UserId;
use sqlx::PgConnection;

use crate::constraint::translate;
use crate::error::DataError;

/// The limits a new user starts with. `0` means unlimited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserLimitDefaults {
    /// Emails the user may send per day.
    pub email_daily_cap: u32,
    /// Contacts the user may keep.
    pub contacts_cap: u32,
    /// Bytes of media the user may store.
    pub storage_quota_bytes: u64,
}

impl UserLimitDefaults {
    /// Reads `[limits]`; `storage_quota_bytes` falls back to `media.user_quota_bytes`.
    #[must_use]
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            email_daily_cap: settings.limits.email_daily_cap,
            contacts_cap: settings.limits.contacts_cap,
            storage_quota_bytes: settings
                .limits
                .storage_quota_bytes
                .unwrap_or(settings.media.user_quota_bytes),
        }
    }
}

impl Default for UserLimitDefaults {
    /// The shipped `[limits]` defaults, for tests and tools that have no settings.
    fn default() -> Self {
        Self {
            email_daily_cap: 500,
            contacts_cap: 10_000,
            storage_quota_bytes: 21_474_836_480,
        }
    }
}

/// A row of `user_limits`. `0` means unlimited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserLimits {
    /// Emails the user may send per day.
    pub email_daily_cap: u32,
    /// Contacts the user may keep.
    pub contacts_cap: u32,
    /// Bytes of media the user may store.
    pub storage_quota_bytes: u64,
    /// When the row last changed. Ignored by [`UserLimitsRepo::update`].
    pub updated_at: DateTime<Utc>,
    /// The admin who last changed it, if any. Ignored by [`UserLimitsRepo::update`], which
    /// records its `admin` argument.
    pub updated_by: Option<UserId>,
}

/// Admin persistence for `user_limits`. Not owner-scoped: limits are deployment data about a
/// user, not the user's content.
pub struct UserLimitsRepo;

impl UserLimitsRepo {
    /// Inserts the limits row for a newly created user; a no-op if one exists.
    pub(crate) async fn insert_defaults(
        conn: &mut PgConnection,
        user: UserId,
        defaults: UserLimitDefaults,
    ) -> Result<(), DataError> {
        sqlx::query!(
            "INSERT INTO user_limits (user_id, email_daily_cap, contacts_cap, storage_quota_bytes)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (user_id) DO NOTHING",
            user.as_uuid(),
            to_i32(defaults.email_daily_cap),
            to_i32(defaults.contacts_cap),
            to_i64(defaults.storage_quota_bytes),
        )
        .execute(&mut *conn)
        .await
        .map_err(translate)?;
        Ok(())
    }

    /// The limits of `user`.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] if the user has no limits row, [`DataError::Sql`] otherwise.
    pub async fn get(conn: &mut PgConnection, user: UserId) -> Result<UserLimits, DataError> {
        let row = sqlx::query!(
            "SELECT email_daily_cap, contacts_cap, storage_quota_bytes, updated_at, updated_by
             FROM user_limits WHERE user_id = $1",
            user.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?;
        Ok(UserLimits {
            email_daily_cap: to_u32(row.email_daily_cap),
            contacts_cap: to_u32(row.contacts_cap),
            storage_quota_bytes: to_u64(row.storage_quota_bytes),
            updated_at: row.updated_at,
            updated_by: row.updated_by.map(UserId::from),
        })
    }

    /// Replaces the three limits of `user` and records `admin` as the editor.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] if the user has no limits row, [`DataError::Sql`] otherwise.
    pub async fn update(
        conn: &mut PgConnection,
        user: UserId,
        limits: UserLimits,
        admin: UserId,
    ) -> Result<(), DataError> {
        let updated = sqlx::query!(
            "UPDATE user_limits
             SET email_daily_cap = $2, contacts_cap = $3, storage_quota_bytes = $4,
                 updated_at = now(), updated_by = $5
             WHERE user_id = $1",
            user.as_uuid(),
            to_i32(limits.email_daily_cap),
            to_i32(limits.contacts_cap),
            to_i64(limits.storage_quota_bytes),
            admin.as_uuid(),
        )
        .execute(&mut *conn)
        .await
        .map_err(translate)?
        .rows_affected();
        if updated == 0 {
            return Err(DataError::NotFound);
        }
        Ok(())
    }
}

// The columns are checked non-negative, and the values written are clamped to what the column
// holds, so these only saturate on a cap above `i32::MAX` or a quota above `i64::MAX`.
fn to_i32(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

fn to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn to_u32(value: i32) -> u32 {
    u32::try_from(value).unwrap_or(0)
}

fn to_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}
