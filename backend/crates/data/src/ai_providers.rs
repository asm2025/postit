//! AI providers: the owner's own model endpoints and keys, for the drafter.
//!
//! Every method takes the connection first and the caller's [`OwnerScope`] second. Reads need
//! [`Capability::Edit`], because a delegate who can edit drafts through the owner's provider;
//! writes are owner-only. The API key (and any secret header value) is sealed as one JSON value
//! in `api_key`, bound to the row, and is never part of [`AiProvider`]'s `Debug`.

use chrono::{DateTime, Utc};
use postit_core::AiProviderId;
use postit_vault::{SealContext, Sealed};
use serde_json::Value;
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::error::DataError;
use crate::{Capability, OwnerScope};

/// A stored provider.
#[derive(Clone)]
pub struct AiProvider {
    /// The provider.
    pub id: AiProviderId,
    /// The preset or protocol it was made from.
    pub provider_key: String,
    /// The settings that are not secret: name, base URL, models, and the rest.
    pub config: Value,
    /// The sealed key and secret header values, when the provider has any.
    pub secret: Option<Sealed<Value>>,
    /// Whether it is the owner's default.
    pub is_default: bool,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

impl std::fmt::Debug for AiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AiProvider")
            .field("id", &self.id)
            .field("provider_key", &self.provider_key)
            .field("config", &self.config)
            .field("is_default", &self.is_default)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish_non_exhaustive()
    }
}

/// A provider to store. The caller makes the ID first, since the secret is sealed for it with
/// [`AiProviderRepo::seal_context`].
#[derive(Clone)]
pub struct NewAiProvider {
    /// The row's ID.
    pub id: AiProviderId,
    /// The preset or protocol.
    pub provider_key: String,
    /// The settings that are not secret.
    pub config: Value,
    /// The sealed secret, when there is one.
    pub secret: Option<Sealed<Value>>,
}

impl std::fmt::Debug for NewAiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewAiProvider")
            .field("id", &self.id)
            .field("provider_key", &self.provider_key)
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

/// What to change on a provider. A `None` leaves a part as it is.
#[derive(Clone, Default)]
pub struct AiProviderChange {
    /// New settings.
    pub config: Option<Value>,
    /// `Some(Some(_))` replaces the secret, `Some(None)` removes it.
    pub secret: Option<Option<Sealed<Value>>>,
}

impl std::fmt::Debug for AiProviderChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AiProviderChange")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

struct Row {
    id: Uuid,
    provider_key: String,
    config: Value,
    api_key: Option<Sealed<Value>>,
    is_default: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<Row> for AiProvider {
    fn from(row: Row) -> Self {
        Self {
            id: AiProviderId::from_uuid(row.id),
            provider_key: row.provider_key,
            config: row.config,
            secret: row.api_key,
            is_default: row.is_default,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// Locks the owner's `users` row, so two writers that decide on the default take turns. `NO
/// KEY UPDATE` is enough for that and still lets other transactions insert rows that reference
/// the user (their foreign keys take `KEY SHARE`).
async fn lock_owner(conn: &mut PgConnection, scope: &OwnerScope) -> Result<(), DataError> {
    sqlx::query_scalar!(
        "SELECT id FROM users WHERE id = $1 FOR NO KEY UPDATE",
        scope.owner.as_uuid(),
    )
    .fetch_one(&mut *conn)
    .await?;
    Ok(())
}

/// AI providers.
pub struct AiProviderRepo;

impl AiProviderRepo {
    /// Locks the owner's `users` row for the rest of the transaction, as [`Self::create`] and
    /// [`Self::set_default`] do, so a read-then-write of the owner's providers takes turns with
    /// every other.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] unless the caller is the owner, and [`DataError::Sql`] on a
    /// database failure.
    pub async fn lock_owner(conn: &mut PgConnection, scope: &OwnerScope) -> Result<(), DataError> {
        scope.require(Capability::OwnerOnly)?;
        lock_owner(conn, scope).await
    }

    /// Where the secret of the row `id` is sealed.
    #[must_use]
    pub fn seal_context(id: AiProviderId) -> SealContext {
        SealContext {
            table: "ai_providers",
            column: "api_key",
            row: id.as_uuid(),
        }
    }

    /// Stores a provider. It becomes the owner's default when the owner has none.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] unless the caller is the owner, and [`DataError::Sql`] on a
    /// database failure.
    pub async fn create(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        new: NewAiProvider,
    ) -> Result<AiProvider, DataError> {
        scope.require(Capability::OwnerOnly)?;
        let mut tx = conn.begin().await?;
        lock_owner(&mut tx, scope).await?;
        let row = sqlx::query_as!(
            Row,
            r#"INSERT INTO ai_providers
                 (id, owner_id, provider_key, config, api_key, is_default, created_by, updated_by)
               VALUES ($1, $2, $3, $4, $5,
                       NOT EXISTS (SELECT 1 FROM ai_providers WHERE owner_id = $2 AND is_default),
                       $6, $6)
               RETURNING id, provider_key, config, api_key AS "api_key: Sealed<Value>",
                         is_default, created_at, updated_at"#,
            new.id.as_uuid(),
            scope.owner.as_uuid(),
            new.provider_key,
            new.config,
            new.secret.as_ref() as Option<&Sealed<Value>>,
            scope.actor.as_uuid(),
        )
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(row.into())
    }

    /// One provider.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] below the edit level, and [`DataError::Sql`] on a database failure.
    pub async fn get(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: AiProviderId,
    ) -> Result<Option<AiProvider>, DataError> {
        scope.require(Capability::Edit)?;
        let row = sqlx::query_as!(
            Row,
            r#"SELECT id, provider_key, config, api_key AS "api_key: Sealed<Value>",
                      is_default, created_at, updated_at
               FROM ai_providers WHERE owner_id = $1 AND id = $2"#,
            scope.owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?;
        Ok(row.map(Into::into))
    }

    /// The owner's providers, oldest first.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] below the edit level, and [`DataError::Sql`] on a database failure.
    pub async fn list(
        conn: &mut PgConnection,
        scope: &OwnerScope,
    ) -> Result<Vec<AiProvider>, DataError> {
        scope.require(Capability::Edit)?;
        let rows = sqlx::query_as!(
            Row,
            r#"SELECT id, provider_key, config, api_key AS "api_key: Sealed<Value>",
                      is_default, created_at, updated_at
               FROM ai_providers WHERE owner_id = $1
               ORDER BY created_at, id"#,
            scope.owner.as_uuid(),
        )
        .fetch_all(&mut *conn)
        .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    /// The owner's default provider, when there is one.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] below the edit level, and [`DataError::Sql`] on a database failure.
    pub async fn default(
        conn: &mut PgConnection,
        scope: &OwnerScope,
    ) -> Result<Option<AiProvider>, DataError> {
        scope.require(Capability::Edit)?;
        let row = sqlx::query_as!(
            Row,
            r#"SELECT id, provider_key, config, api_key AS "api_key: Sealed<Value>",
                      is_default, created_at, updated_at
               FROM ai_providers WHERE owner_id = $1 AND is_default"#,
            scope.owner.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?;
        Ok(row.map(Into::into))
    }

    /// Changes a provider's settings, its secret, or both.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such provider, [`DataError::Scope`] unless
    /// the caller is the owner, and [`DataError::Sql`] on a database failure.
    pub async fn update(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: AiProviderId,
        change: AiProviderChange,
    ) -> Result<AiProvider, DataError> {
        scope.require(Capability::OwnerOnly)?;
        let replace_secret = change.secret.is_some();
        let secret = change.secret.flatten();
        let row = sqlx::query_as!(
            Row,
            r#"UPDATE ai_providers
               SET config = COALESCE($3, config),
                   api_key = CASE WHEN $4 THEN $5 ELSE api_key END,
                   updated_at = now(), updated_by = $6
               WHERE owner_id = $1 AND id = $2
               RETURNING id, provider_key, config, api_key AS "api_key: Sealed<Value>",
                         is_default, created_at, updated_at"#,
            scope.owner.as_uuid(),
            id.as_uuid(),
            change.config as Option<Value>,
            replace_secret,
            secret.as_ref() as Option<&Sealed<Value>>,
            scope.actor.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?;
        Ok(row.into())
    }

    /// Makes a provider the owner's default and clears the old default.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such provider, [`DataError::Scope`] unless
    /// the caller is the owner, and [`DataError::Sql`] on a database failure.
    pub async fn set_default(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: AiProviderId,
    ) -> Result<(), DataError> {
        scope.require(Capability::OwnerOnly)?;
        let mut tx = conn.begin().await?;
        lock_owner(&mut tx, scope).await?;
        let exists = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM ai_providers WHERE owner_id = $1 AND id = $2) AS "exists!""#,
            scope.owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_one(&mut *tx)
        .await?;
        if !exists {
            return Err(DataError::NotFound);
        }
        sqlx::query!(
            "UPDATE ai_providers SET is_default = FALSE, updated_at = now(), updated_by = $2
             WHERE owner_id = $1 AND is_default AND id <> $3",
            scope.owner.as_uuid(),
            scope.actor.as_uuid(),
            id.as_uuid(),
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "UPDATE ai_providers SET is_default = TRUE, updated_at = now(), updated_by = $3
             WHERE owner_id = $1 AND id = $2 AND NOT is_default",
            scope.owner.as_uuid(),
            id.as_uuid(),
            scope.actor.as_uuid(),
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Deletes a provider. The owner's generations stay, with no provider. Deleting the default
    /// leaves the owner with none. Returns whether a row went.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] unless the caller is the owner, and [`DataError::Sql`] on a
    /// database failure.
    pub async fn delete(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: AiProviderId,
    ) -> Result<bool, DataError> {
        scope.require(Capability::OwnerOnly)?;
        let done = sqlx::query!(
            "DELETE FROM ai_providers WHERE owner_id = $1 AND id = $2",
            scope.owner.as_uuid(),
            id.as_uuid(),
        )
        .execute(&mut *conn)
        .await?;
        Ok(done.rows_affected() > 0)
    }
}
