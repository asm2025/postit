//! Credential connections waiting for the user to choose accounts.
//!
//! A row holds the verified, sealed credential and the accounts it reached until the owner
//! confirms. Rows are single use: [`PendingConnectionRepo::take`] deletes the row it returns.

use chrono::{DateTime, Utc};
use postit_core::{PendingConnectionId, PlatformId, UserId};
use postit_vault::{SealContext, Sealed};
use serde_json::Value;
use sqlx::PgConnection;

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
