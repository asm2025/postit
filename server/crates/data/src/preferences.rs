use chrono::{DateTime, Utc};
use postit_core::UserId;
use sqlx::PgConnection;

use crate::error::DataError;

#[derive(Debug, Clone)]
pub struct UserPreferences {
    pub user_id: UserId,
    pub email_notifications: bool,
    pub timezone: String,
    pub store_ai_prompts: bool,
    pub last_approval_email_at: Option<DateTime<Utc>>,
}

struct Row {
    user_id: uuid::Uuid,
    email_notifications: bool,
    timezone: String,
    store_ai_prompts: bool,
    last_approval_email_at: Option<DateTime<Utc>>,
}

impl From<Row> for UserPreferences {
    fn from(row: Row) -> Self {
        Self {
            user_id: UserId::from(row.user_id),
            email_notifications: row.email_notifications,
            timezone: row.timezone,
            store_ai_prompts: row.store_ai_prompts,
            last_approval_email_at: row.last_approval_email_at,
        }
    }
}

pub struct UserPreferencesRepo;

impl UserPreferencesRepo {
    /// Creates the default preferences row for a newly provisioned user. Called by
    /// `postit-identity` inside the same transaction as `UsersRepo::provision`, so a user
    /// never exists without preferences.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure (including a unique-violation if
    /// called twice for the same user).
    pub async fn create_default(
        conn: &mut PgConnection,
        user_id: UserId,
        timezone: &str,
    ) -> Result<UserPreferences, DataError> {
        let row = sqlx::query_as!(
            Row,
            r#"INSERT INTO user_preferences (user_id, timezone)
               VALUES ($1, $2)
               RETURNING user_id, email_notifications, timezone, store_ai_prompts, last_approval_email_at"#,
            user_id.as_uuid(),
            timezone,
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(row.into())
    }

    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn get(
        conn: &mut PgConnection,
        user_id: UserId,
    ) -> Result<Option<UserPreferences>, DataError> {
        let row = sqlx::query_as!(
            Row,
            r#"SELECT user_id, email_notifications, timezone, store_ai_prompts, last_approval_email_at
               FROM user_preferences WHERE user_id = $1"#,
            user_id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?;
        Ok(row.map(Into::into))
    }

    /// # Errors
    ///
    /// Returns [`DataError::NotFound`] if no preferences row exists for `user_id`,
    /// [`DataError::Sql`] otherwise.
    pub async fn update(
        conn: &mut PgConnection,
        user_id: UserId,
        email_notifications: bool,
        timezone: &str,
        store_ai_prompts: bool,
    ) -> Result<UserPreferences, DataError> {
        let row = sqlx::query_as!(
            Row,
            r#"UPDATE user_preferences
               SET email_notifications = $2, timezone = $3, store_ai_prompts = $4
               WHERE user_id = $1
               RETURNING user_id, email_notifications, timezone, store_ai_prompts, last_approval_email_at"#,
            user_id.as_uuid(),
            email_notifications,
            timezone,
            store_ai_prompts,
        )
        .fetch_optional(&mut *conn)
        .await?;
        row.map(Into::into).ok_or(DataError::NotFound)
    }

    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure. Not finding `user_id` is not an
    /// error — this is a best-effort marker write from the coalesced-approval-email flow
    /// (plan 02 P5), called only after that row is known to exist.
    pub async fn set_last_approval_email_at(
        conn: &mut PgConnection,
        user_id: UserId,
        at: DateTime<Utc>,
    ) -> Result<(), DataError> {
        sqlx::query!(
            "UPDATE user_preferences SET last_approval_email_at = $2 WHERE user_id = $1",
            user_id.as_uuid(),
            at,
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }
}
