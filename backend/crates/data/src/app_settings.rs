//! Deployment settings an admin can change at runtime, kept in `app_settings`.

use postit_core::UserId;
use sqlx::PgConnection;

use crate::error::DataError;

const PUBLISHING_LIVE: &str = "publishing.live";

/// Admin persistence for `app_settings`. Typed accessors only; there is no generic key API.
pub struct AppSettingsRepo;

impl AppSettingsRepo {
    /// Inserts the settings that are missing, taking `live` from config. Never overwrites a
    /// value an admin has changed.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn seed_defaults(conn: &mut PgConnection, live: bool) -> Result<(), DataError> {
        sqlx::query!(
            "INSERT INTO app_settings (key, value) VALUES ($1, $2)
             ON CONFLICT (key) DO NOTHING",
            PUBLISHING_LIVE,
            serde_json::Value::Bool(live),
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// Whether live publishing is on: the runtime switch. While it is `false`, the worker parks
    /// live deliveries with `live_publish_disabled` and dry runs still run; it never turns a
    /// live delivery into a dry run.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] if the setting was never seeded, [`DataError::Sql`] on a
    /// database failure.
    pub async fn live_publishing(conn: &mut PgConnection) -> Result<bool, DataError> {
        let value = sqlx::query_scalar!(
            "SELECT value FROM app_settings WHERE key = $1",
            PUBLISHING_LIVE
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?;
        Ok(value.as_bool().unwrap_or(false))
    }

    /// Turns live publishing on or off and records `admin` as the editor.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn set_live_publishing(
        conn: &mut PgConnection,
        live: bool,
        admin: UserId,
    ) -> Result<(), DataError> {
        sqlx::query!(
            "INSERT INTO app_settings (key, value, updated_by) VALUES ($1, $2, $3)
             ON CONFLICT (key) DO UPDATE
             SET value = EXCLUDED.value, updated_at = now(), updated_by = EXCLUDED.updated_by",
            PUBLISHING_LIVE,
            serde_json::Value::Bool(live),
            admin.as_uuid(),
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }
}
