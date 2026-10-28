//! OAuth authorizations in flight, and the pending results they leave for the client to confirm.
//!
//! A row is a `state` from the start until the callback consumes it ([`OAuthStateRepo::consume_state`]).
//! After a successful exchange it holds a pending result: the sealed tokens, the discovered
//! accounts, and the hash of the `connect_id` handed to the client. A result postit will not use
//! is **discarded** rather than deleted: `connect_id_hash` is cleared and the row expires, with
//! its tokens and `remote_user_id` kept, so the purge job ([`OAuthStateRepo::take_expired_result`])
//! revokes the tokens at the provider before the row goes. Every call here is a system call: the
//! service compares the user and the expiry itself.

use chrono::{DateTime, Utc};
use postit_core::{GrantId, OAuthStateId, PlatformId, UserId};
use postit_vault::{SealContext, Sealed};
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::constraint::translate;
use crate::error::DataError;
use crate::text::corrupt;

/// A state to store when an authorization starts.
#[derive(Clone)]
pub struct NewOAuthState {
    /// The new row's ID. Sealed values use [`OAuthStateRepo::seal_context`] with this ID.
    pub id: OAuthStateId,
    /// SHA-256 of the `state` sent to the provider. The `state` itself is never stored.
    pub state_hash: [u8; 32],
    /// The user who started the authorization.
    pub user: UserId,
    /// The platform being authorized.
    pub platform: PlatformId,
    /// The sealed PKCE verifier, when the platform uses PKCE.
    pub pkce_verifier: Option<Sealed<Value>>,
    /// Where the callback sends the browser back to.
    pub return_to: String,
    /// When the state stops being accepted.
    pub expires_at: DateTime<Utc>,
}

impl std::fmt::Debug for NewOAuthState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewOAuthState")
            .field("id", &self.id)
            .field("user", &self.user)
            .field("platform", &self.platform)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

/// A stored state, or the pending result it became.
#[derive(Clone)]
pub struct OAuthState {
    /// The row.
    pub id: OAuthStateId,
    /// The user who started the authorization.
    pub user: UserId,
    /// The platform being authorized.
    pub platform: PlatformId,
    /// The sealed PKCE verifier, if any.
    pub pkce_verifier: Option<Sealed<Value>>,
    /// Where the callback sends the browser back to.
    pub return_to: String,
    /// When the state, or the pending result, stops being accepted.
    pub expires_at: DateTime<Utc>,
    /// SHA-256 of the `connect_id`; `None` before the callback and once discarded.
    pub connect_id_hash: Option<Vec<u8>>,
    /// The sealed tokens of the exchange, once the callback stored them.
    pub grant_tokens: Option<Sealed<Value>>,
    /// The accounts discovery found, as the service serialized them.
    pub discovered: Option<Value>,
    /// The provider's ID for the authorizing user, if it gave one.
    pub remote_user_id: Option<String>,
}

impl std::fmt::Debug for OAuthState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthState")
            .field("id", &self.id)
            .field("user", &self.user)
            .field("platform", &self.platform)
            .field("expires_at", &self.expires_at)
            .field("has_result", &self.connect_id_hash.is_some())
            .field("has_tokens", &self.grant_tokens.is_some())
            .finish_non_exhaustive()
    }
}

/// What [`OAuthStateRepo::remote_user_in_use`] leaves out of its check: the row being revoked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteUserExcept {
    /// A grant being revoked.
    Grant(GrantId),
    /// A pending result being revoked.
    State(OAuthStateId),
    /// Nothing.
    Nothing,
}

struct StateRow {
    id: Uuid,
    user_id: Uuid,
    platform_id: String,
    pkce_verifier: Option<Sealed<Value>>,
    return_to: Option<String>,
    expires_at: DateTime<Utc>,
    connect_id_hash: Option<Vec<u8>>,
    grant_tokens: Option<Sealed<Value>>,
    discovered: Option<Value>,
    remote_user_id: Option<String>,
}

impl TryFrom<StateRow> for OAuthState {
    type Error = DataError;

    fn try_from(row: StateRow) -> Result<Self, DataError> {
        Ok(Self {
            id: OAuthStateId::from(row.id),
            user: UserId::from(row.user_id),
            platform: PlatformId::new(&row.platform_id).map_err(corrupt)?,
            pkce_verifier: row.pkce_verifier,
            return_to: row.return_to.unwrap_or_default(),
            expires_at: row.expires_at,
            connect_id_hash: row.connect_id_hash,
            grant_tokens: row.grant_tokens,
            discovered: row.discovered,
            remote_user_id: row.remote_user_id,
        })
    }
}

/// Persistence for `oauth_states`.
pub struct OAuthStateRepo;

impl OAuthStateRepo {
    /// Where a value of the row is sealed: `column` (`pkce_verifier` or `grant_tokens`) of the
    /// row `id`.
    #[must_use]
    pub fn seal_context(id: OAuthStateId, column: &'static str) -> SealContext {
        SealContext {
            table: "oauth_states",
            column,
            row: id.as_uuid(),
        }
    }

    /// Stores a new state.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure, including a duplicate state hash.
    pub async fn insert(conn: &mut PgConnection, new: NewOAuthState) -> Result<(), DataError> {
        let hash: &[u8] = &new.state_hash;
        sqlx::query!(
            "INSERT INTO oauth_states
                 (id, state_hash, user_id, platform_id, pkce_verifier, return_to, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            new.id.as_uuid(),
            hash,
            new.user.as_uuid(),
            new.platform.as_str(),
            new.pkce_verifier.as_ref() as Option<&Sealed<Value>>,
            new.return_to,
            new.expires_at,
        )
        .execute(&mut *conn)
        .await
        .map_err(translate)?;
        Ok(())
    }

    /// Marks the state with this hash consumed and returns it, once: a second call for the same
    /// hash returns `None`. An expired state is still returned, for the service to refuse.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn consume_state(
        conn: &mut PgConnection,
        state_hash: &[u8; 32],
        now: DateTime<Utc>,
    ) -> Result<Option<OAuthState>, DataError> {
        let hash: &[u8] = state_hash;
        sqlx::query_as!(
            StateRow,
            r#"UPDATE oauth_states SET consumed_at = $2
               WHERE state_hash = $1 AND consumed_at IS NULL
               RETURNING id, user_id, platform_id,
                         pkce_verifier AS "pkce_verifier?: Sealed<Value>",
                         return_to, expires_at, connect_id_hash,
                         grant_tokens AS "grant_tokens?: Sealed<Value>",
                         discovered, remote_user_id"#,
            hash,
            now,
        )
        .fetch_optional(&mut *conn)
        .await?
        .map(OAuthState::try_from)
        .transpose()
    }

    /// Stores the pending result of a successful callback on the row: the `connect_id` hash,
    /// the sealed tokens, the discovered accounts, and the provider's user ID.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the row is gone, and [`DataError::Sql`] on a database
    /// failure.
    pub async fn store_result(
        conn: &mut PgConnection,
        id: OAuthStateId,
        connect_id_hash: &[u8; 32],
        grant_tokens: &Sealed<Value>,
        discovered: &Value,
        remote_user_id: Option<&str>,
    ) -> Result<(), DataError> {
        let hash: &[u8] = connect_id_hash;
        let updated = sqlx::query!(
            "UPDATE oauth_states
             SET connect_id_hash = $2, grant_tokens = $3, discovered = $4, remote_user_id = $5
             WHERE id = $1",
            id.as_uuid(),
            hash,
            grant_tokens as &Sealed<Value>,
            discovered,
            remote_user_id,
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

    /// Stores the tokens of an exchange whose callback then failed, already discarded: no
    /// `connect_id_hash`, and expiring at `now`, so the next purge revokes them.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the row is gone, and [`DataError::Sql`] on a database
    /// failure.
    pub async fn store_discarded(
        conn: &mut PgConnection,
        id: OAuthStateId,
        grant_tokens: &Sealed<Value>,
        remote_user_id: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<(), DataError> {
        let updated = sqlx::query!(
            "UPDATE oauth_states
             SET connect_id_hash = NULL, grant_tokens = $2, remote_user_id = $3, expires_at = $4
             WHERE id = $1",
            id.as_uuid(),
            grant_tokens as &Sealed<Value>,
            remote_user_id,
            now,
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if updated == 0 {
            return Err(DataError::NotFound);
        }
        Ok(())
    }

    /// Deletes the row. The service deletes a row holding tokens only once they moved into a
    /// grant; a result it will not use is discarded instead.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn delete(conn: &mut PgConnection, id: OAuthStateId) -> Result<(), DataError> {
        sqlx::query!("DELETE FROM oauth_states WHERE id = $1", id.as_uuid())
            .execute(&mut *conn)
            .await?;
        Ok(())
    }

    /// The pending result with this `connect_id` hash, without locking or consuming it,
    /// whoever started it and whether or not it has expired.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn find_result(
        conn: &mut PgConnection,
        connect_id_hash: &[u8; 32],
    ) -> Result<Option<OAuthState>, DataError> {
        let hash: &[u8] = connect_id_hash;
        sqlx::query_as!(
            StateRow,
            r#"SELECT id, user_id, platform_id,
                      pkce_verifier AS "pkce_verifier?: Sealed<Value>",
                      return_to, expires_at, connect_id_hash,
                      grant_tokens AS "grant_tokens?: Sealed<Value>",
                      discovered, remote_user_id
               FROM oauth_states WHERE connect_id_hash = $1"#,
            hash,
        )
        .fetch_optional(&mut *conn)
        .await?
        .map(OAuthState::try_from)
        .transpose()
    }

    /// The pending result with this `connect_id` hash, locked `FOR UPDATE` until `tx` ends, so
    /// a confirm decides between [`OAuthStateRepo::delete`] and [`OAuthStateRepo::discard`] in
    /// the same transaction.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn lock_result(
        tx: &mut PgConnection,
        connect_id_hash: &[u8; 32],
    ) -> Result<Option<OAuthState>, DataError> {
        let hash: &[u8] = connect_id_hash;
        sqlx::query_as!(
            StateRow,
            r#"SELECT id, user_id, platform_id,
                      pkce_verifier AS "pkce_verifier?: Sealed<Value>",
                      return_to, expires_at, connect_id_hash,
                      grant_tokens AS "grant_tokens?: Sealed<Value>",
                      discovered, remote_user_id
               FROM oauth_states WHERE connect_id_hash = $1
               FOR UPDATE"#,
            hash,
        )
        .fetch_optional(&mut *tx)
        .await?
        .map(OAuthState::try_from)
        .transpose()
    }

    /// Discards a pending result: it can no longer be found by its `connect_id`, and it
    /// expires at `now` at the latest, with its tokens and `remote_user_id` kept for the purge
    /// to revoke.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn discard(
        conn: &mut PgConnection,
        id: OAuthStateId,
        now: DateTime<Utc>,
    ) -> Result<(), DataError> {
        sqlx::query!(
            "UPDATE oauth_states
             SET connect_id_hash = NULL, expires_at = LEAST(expires_at, $2)
             WHERE id = $1",
            id.as_uuid(),
            now,
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// Deletes and returns one expired row that holds tokens, skipping rows another purge has
    /// locked. Each call removes the row it returns, so a loop over it ends and two purges
    /// never take the same row.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn take_expired_result(
        conn: &mut PgConnection,
        now: DateTime<Utc>,
    ) -> Result<Option<OAuthState>, DataError> {
        sqlx::query_as!(
            StateRow,
            r#"DELETE FROM oauth_states
               WHERE id = (SELECT id FROM oauth_states
                           WHERE expires_at <= $1 AND grant_tokens IS NOT NULL
                           ORDER BY id LIMIT 1
                           FOR UPDATE SKIP LOCKED)
               RETURNING id, user_id, platform_id,
                         pkce_verifier AS "pkce_verifier?: Sealed<Value>",
                         return_to, expires_at, connect_id_hash,
                         grant_tokens AS "grant_tokens?: Sealed<Value>",
                         discovered, remote_user_id"#,
            now,
        )
        .fetch_optional(&mut *conn)
        .await?
        .map(OAuthState::try_from)
        .transpose()
    }

    /// Deletes and returns every row of `user` that holds tokens, live or discarded, for the
    /// user deletion step to revoke.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn take_results_of(
        conn: &mut PgConnection,
        user: UserId,
    ) -> Result<Vec<OAuthState>, DataError> {
        sqlx::query_as!(
            StateRow,
            r#"DELETE FROM oauth_states
               WHERE user_id = $1 AND grant_tokens IS NOT NULL
               RETURNING id, user_id, platform_id,
                         pkce_verifier AS "pkce_verifier?: Sealed<Value>",
                         return_to, expires_at, connect_id_hash,
                         grant_tokens AS "grant_tokens?: Sealed<Value>",
                         discovered, remote_user_id"#,
            user.as_uuid(),
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(OAuthState::try_from)
        .collect()
    }

    /// Deletes expired states that hold no tokens and expired pending credential connections;
    /// returns how many rows it deleted in all. Expired rows holding tokens are left for
    /// [`OAuthStateRepo::take_expired_result`], which the purge job drains first.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn purge_expired(
        conn: &mut PgConnection,
        now: DateTime<Utc>,
    ) -> Result<u64, DataError> {
        let states = sqlx::query!(
            "DELETE FROM oauth_states WHERE expires_at <= $1 AND grant_tokens IS NULL",
            now,
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        let pending = sqlx::query!(
            "DELETE FROM pending_connections WHERE expires_at <= $1",
            now,
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        Ok(states + pending)
    }

    /// Whether the provider's user `remote_user_id` on `platform` is still in use, other than
    /// by `except`: an `active` grant of any owner, or a pending result that is live (not
    /// expired at `now`) and not discarded. The revoke-skip rule: revoking tokens of a remote
    /// user still in use could end that other authorization too.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn remote_user_in_use(
        conn: &mut PgConnection,
        platform: &PlatformId,
        remote_user_id: &str,
        except: RemoteUserExcept,
        now: DateTime<Utc>,
    ) -> Result<bool, DataError> {
        let (except_grant, except_state) = match except {
            RemoteUserExcept::Grant(id) => (Some(id.as_uuid()), None),
            RemoteUserExcept::State(id) => (None, Some(id.as_uuid())),
            RemoteUserExcept::Nothing => (None, None),
        };
        let in_use = sqlx::query_scalar!(
            r#"SELECT EXISTS (
                   SELECT 1 FROM account_grants
                   WHERE platform_id = $1 AND remote_user_id = $2 AND status = 'active'
                     AND ($3::uuid IS NULL OR id <> $3))
               OR EXISTS (
                   SELECT 1 FROM oauth_states
                   WHERE platform_id = $1 AND remote_user_id = $2
                     AND grant_tokens IS NOT NULL AND connect_id_hash IS NOT NULL
                     AND expires_at > $5
                     AND ($4::uuid IS NULL OR id <> $4))
               AS "in_use!""#,
            platform.as_str(),
            remote_user_id,
            except_grant,
            except_state,
            now,
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(in_use)
    }
}
