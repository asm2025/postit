//! Credential connections waiting for the user to choose accounts.
//!
//! A row holds the verified, sealed credential and the accounts it reached until the owner
//! confirms. Rows are single use: [`PendingConnectionRepo::take`] deletes the row it returns.

use chrono::{DateTime, Utc};
use postit_core::{PendingConnectionId, PlatformId, UserId};
use postit_vault::{SealContext, Sealed};
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::constraint::translate;
use crate::error::DataError;
use crate::text::corrupt;
use crate::{Capability, OwnerScope};

/// A pending connection to store.
#[derive(Debug, Clone)]
pub struct NewPendingConnection {
    /// The new row's ID. The credential must be sealed with [`PendingConnectionRepo::seal_context`].
    pub id: PendingConnectionId,
    /// The platform the credential is for.
    pub platform: PlatformId,
    /// SHA-256 of the `connect_id` handed to the user. The `connect_id` itself is never stored.
    pub connect_id_hash: [u8; 32],
    /// The sealed secret part of the credential.
    pub credential: Sealed<Value>,
    /// The non-secret part of the credential.
    pub connection_config: Value,
    /// The accounts the credential reached, as the service serialized them.
    pub discovered: Value,
    /// When the connection stops being confirmable.
    pub expires_at: DateTime<Utc>,
}

/// A pending connection taken from the table.
#[derive(Clone)]
pub struct PendingConnection {
    /// The row.
    pub id: PendingConnectionId,
    /// The user who started the connection.
    pub owner: UserId,
    /// The platform the credential is for.
    pub platform: PlatformId,
    /// The sealed secret part of the credential.
    pub credential: Sealed<Value>,
    /// The non-secret part of the credential.
    pub connection_config: Value,
    /// The accounts the credential reached.
    pub discovered: Value,
    /// When the connection stops being confirmable.
    pub expires_at: DateTime<Utc>,
}

impl std::fmt::Debug for PendingConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingConnection")
            .field("id", &self.id)
            .field("owner", &self.owner)
            .field("platform", &self.platform)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

/// A row for the owner's list of unfinished connections: when and for which platform, never the
/// discovered accounts or a count of them, and never the `connect_id`.
#[derive(Debug, Clone)]
pub struct PendingRowSummary {
    /// The row ID, of a credential row or of an OAuth state.
    pub id: Uuid,
    /// The platform being connected.
    pub platform: PlatformId,
    /// When the connection started.
    pub created_at: DateTime<Utc>,
    /// When the connection stops being confirmable.
    pub expires_at: DateTime<Utc>,
}

/// Persistence for `pending_connections`.
pub struct PendingConnectionRepo;

impl PendingConnectionRepo {
    /// Where a pending credential is sealed: the `credential` column of its own row.
    #[must_use]
    pub fn seal_context(id: PendingConnectionId) -> SealContext {
        SealContext {
            table: "pending_connections",
            column: "credential",
            row: id.as_uuid(),
        }
    }

    /// Stores a pending connection for the scope's owner.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] unless the caller is the owner, and [`DataError::Sql`] on a
    /// database failure.
    pub async fn insert(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        new: NewPendingConnection,
    ) -> Result<(), DataError> {
        scope.require(Capability::OwnerOnly)?;
        let hash: &[u8] = &new.connect_id_hash;
        sqlx::query!(
            "INSERT INTO pending_connections
                 (id, owner_id, connect_id_hash, platform_id, credential, connection_config,
                  discovered, expires_at, created_by, updated_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9)",
            new.id.as_uuid(),
            scope.owner.as_uuid(),
            hash,
            new.platform.as_str(),
            &new.credential as &Sealed<Value>,
            new.connection_config,
            new.discovered,
            new.expires_at,
            scope.actor.as_uuid(),
        )
        .execute(&mut *conn)
        .await
        .map_err(translate)?;
        Ok(())
    }

    /// Deletes and returns the row with this `connect_id` hash, whoever owns it and whether or
    /// not it has expired. A system call: the service compares the owner and the expiry, so a
    /// confirm by the wrong user still consumes the row.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn take(
        conn: &mut PgConnection,
        connect_id_hash: &[u8; 32],
    ) -> Result<Option<PendingConnection>, DataError> {
        let hash: &[u8] = connect_id_hash;
        sqlx::query_as!(
            PendingRow,
            r#"DELETE FROM pending_connections WHERE connect_id_hash = $1
               RETURNING id, owner_id, platform_id,
                         credential AS "credential: Sealed<Value>",
                         connection_config, discovered, expires_at"#,
            hash,
        )
        .fetch_optional(&mut *conn)
        .await?
        .map(PendingConnection::try_from)
        .transpose()
    }

    /// Returns the row with this `connect_id` hash without consuming it, whoever owns it and
    /// whether or not it has expired. A system call for the pending read: the service compares
    /// the owner and the expiry.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn find(
        conn: &mut PgConnection,
        connect_id_hash: &[u8; 32],
    ) -> Result<Option<PendingConnection>, DataError> {
        let hash: &[u8] = connect_id_hash;
        sqlx::query_as!(
            PendingRow,
            r#"SELECT id, owner_id, platform_id,
                      credential AS "credential: Sealed<Value>",
                      connection_config, discovered, expires_at
               FROM pending_connections WHERE connect_id_hash = $1"#,
            hash,
        )
        .fetch_optional(&mut *conn)
        .await?
        .map(PendingConnection::try_from)
        .transpose()
    }

    /// The scope owner's live rows (`expires_at > now`), newest first.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] unless the caller is the owner, and [`DataError::Sql`] on a
    /// database failure.
    pub async fn list_live(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        now: DateTime<Utc>,
    ) -> Result<Vec<PendingRowSummary>, DataError> {
        scope.require(Capability::OwnerOnly)?;
        sqlx::query!(
            "SELECT id, platform_id, created_at, expires_at FROM pending_connections
             WHERE owner_id = $1 AND expires_at > $2
             ORDER BY created_at DESC, id DESC",
            scope.owner.as_uuid(),
            now,
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(|r| {
            Ok(PendingRowSummary {
                id: r.id,
                platform: PlatformId::new(&r.platform_id).map_err(corrupt)?,
                created_at: r.created_at,
                expires_at: r.expires_at,
            })
        })
        .collect()
    }

    /// Deletes the scope owner's live row `id` by row ID, in one statement. Returns whether a
    /// row went: `false` for an unknown, foreign, or expired row, which the caller cannot tell
    /// apart.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] unless the caller is the owner, and [`DataError::Sql`] on a
    /// database failure.
    pub async fn delete_owned(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: PendingConnectionId,
        now: DateTime<Utc>,
    ) -> Result<bool, DataError> {
        scope.require(Capability::OwnerOnly)?;
        Ok(sqlx::query!(
            "DELETE FROM pending_connections
             WHERE id = $1 AND owner_id = $2 AND expires_at > $3",
            id.as_uuid(),
            scope.owner.as_uuid(),
            now,
        )
        .execute(&mut *conn)
        .await?
        .rows_affected()
            == 1)
    }
}

struct PendingRow {
    id: uuid::Uuid,
    owner_id: uuid::Uuid,
    platform_id: String,
    credential: Sealed<Value>,
    connection_config: Value,
    discovered: Value,
    expires_at: DateTime<Utc>,
}

impl TryFrom<PendingRow> for PendingConnection {
    type Error = DataError;

    fn try_from(r: PendingRow) -> Result<Self, DataError> {
        Ok(Self {
            id: PendingConnectionId::from(r.id),
            owner: UserId::from(r.owner_id),
            platform: PlatformId::new(&r.platform_id).map_err(corrupt)?,
            credential: r.credential,
            connection_config: r.connection_config,
            discovered: r.discovered,
            expires_at: r.expires_at,
        })
    }
}
