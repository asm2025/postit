//! The `platforms` table: one row per registered platform agent, upserted at startup.

use chrono::{DateTime, Utc};
use postit_core::PlatformId;
use sqlx::PgPool;

use crate::error::DataError;

/// A platform as stored.
#[derive(Debug, Clone)]
pub struct PlatformRow {
    /// The agent's platform ID.
    pub id: PlatformId,
    /// Name shown to people.
    pub display_name: String,
    /// Whether publishing to this platform is switched on.
    pub enabled: bool,
    /// When this platform was first registered.
    pub first_seen_at: DateTime<Utc>,
}

/// Admin-level persistence for platforms. Not owner-scoped: platforms are deployment data.
pub struct PlatformRepo;

impl PlatformRepo {
    /// Inserts newly registered platforms and refreshes the display name of known ones. It
    /// never touches `enabled` and never deletes rows missing from `registered`.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn upsert_registered(
        pool: &PgPool,
        registered: &[(PlatformId, String)],
    ) -> Result<(), DataError> {
        let mut tx = pool.begin().await?;
        for (id, display_name) in registered {
            sqlx::query(
                "INSERT INTO platforms (id, display_name) VALUES ($1, $2) \
                 ON CONFLICT (id) DO UPDATE SET display_name = EXCLUDED.display_name, \
                 updated_at = now() \
                 WHERE platforms.display_name IS DISTINCT FROM EXCLUDED.display_name",
            )
            .bind(id.as_str())
            .bind(display_name)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Every platform, ordered by ID.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure, or [`DataError::Conflict`] if a
    /// stored ID is not a valid platform ID.
    pub async fn list(pool: &PgPool) -> Result<Vec<PlatformRow>, DataError> {
        let rows: Vec<(String, String, bool, DateTime<Utc>)> = sqlx::query_as(
            "SELECT id, display_name, enabled, first_seen_at FROM platforms ORDER BY id",
        )
        .fetch_all(pool)
        .await?;
        rows.into_iter()
            .map(|(id, display_name, enabled, first_seen_at)| {
                Ok(PlatformRow {
                    id: PlatformId::new(&id)
                        .map_err(|_| DataError::Conflict("stored platform id is invalid".into()))?,
                    display_name,
                    enabled,
                    first_seen_at,
                })
            })
            .collect()
    }

    /// Switches a platform on or off. Returns whether a row matched.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn set_enabled(
        pool: &PgPool,
        id: &PlatformId,
        enabled: bool,
    ) -> Result<bool, DataError> {
        let result =
            sqlx::query("UPDATE platforms SET enabled = $2, updated_at = now() WHERE id = $1")
                .bind(id.as_str())
                .bind(enabled)
                .execute(pool)
                .await?;
        Ok(result.rows_affected() > 0)
    }
}
