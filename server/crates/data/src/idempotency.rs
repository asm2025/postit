use chrono::{DateTime, Utc};
use postit_core::UserId;
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::DataError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdempotencyState {
    InProgress,
    Completed,
}

impl IdempotencyState {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "completed" => Self::Completed,
            _ => Self::InProgress,
        }
    }
}

#[derive(Debug, Clone)]
pub struct IdempotencyRecord {
    pub id: Uuid,
    pub owner_id: UserId,
    pub actor_id: UserId,
    pub key: String,
    pub route: String,
    pub request_hash: String,
    pub state: IdempotencyState,
    pub response_status: Option<i16>,
    pub response_body: Option<Value>,
    pub expires_at: DateTime<Utc>,
}

struct Row {
    id: Uuid,
    owner_id: Uuid,
    actor_id: Uuid,
    key: String,
    route: String,
    request_hash: String,
    state: String,
    response_status: Option<i16>,
    response_body: Option<Value>,
    expires_at: DateTime<Utc>,
}

impl From<Row> for IdempotencyRecord {
    fn from(row: Row) -> Self {
        Self {
            id: row.id,
            owner_id: UserId::from(row.owner_id),
            actor_id: UserId::from(row.actor_id),
            key: row.key,
            route: row.route,
            request_hash: row.request_hash,
            state: IdempotencyState::parse(&row.state),
            response_status: row.response_status,
            response_body: row.response_body,
            expires_at: row.expires_at,
        }
    }
}

#[derive(Debug, Clone)]
pub enum BeginOutcome {
    /// A new `in_progress` row was inserted; the caller should do the work.
    Started(IdempotencyRecord),
    /// A row for this `(owner_id, actor_id, key)` already exists (`in_progress` or
    /// `completed`). The caller decides what that means — `postit-api` (P6) returns 409
    /// `idempotency_in_progress` or replays the stored response, per plan 01.
    Conflict(IdempotencyRecord),
}

pub struct IdempotencyRepo;

impl IdempotencyRepo {
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure other than the unique-violation
    /// this function itself turns into `BeginOutcome::Conflict`.
    #[allow(clippy::too_many_arguments)]
    pub async fn begin(
        conn: &mut PgConnection,
        id: Uuid,
        owner_id: UserId,
        actor_id: UserId,
        key: &str,
        route: &str,
        request_hash: &str,
        expires_at: DateTime<Utc>,
    ) -> Result<BeginOutcome, DataError> {
        let inserted = sqlx::query_as!(
            Row,
            r#"INSERT INTO idempotency_keys (id, owner_id, actor_id, key, route, request_hash, state, expires_at)
               VALUES ($1, $2, $3, $4, $5, $6, 'in_progress', $7)
               ON CONFLICT (owner_id, actor_id, key) DO NOTHING
               RETURNING id, owner_id, actor_id, key, route, request_hash, state, response_status, response_body, expires_at"#,
            id,
            owner_id.as_uuid(),
            actor_id.as_uuid(),
            key,
            route,
            request_hash,
            expires_at,
        )
        .fetch_optional(&mut *conn)
        .await?;

        if let Some(row) = inserted {
            return Ok(BeginOutcome::Started(row.into()));
        }

        let existing = sqlx::query_as!(
            Row,
            r#"SELECT id, owner_id, actor_id, key, route, request_hash, state, response_status, response_body, expires_at
               FROM idempotency_keys WHERE owner_id = $1 AND actor_id = $2 AND key = $3"#,
            owner_id.as_uuid(),
            actor_id.as_uuid(),
            key,
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?;

        Ok(BeginOutcome::Conflict(existing.into()))
    }

    /// # Errors
    ///
    /// Returns [`DataError::NotFound`] if `id` doesn't exist, [`DataError::Sql`] otherwise.
    pub async fn complete(
        conn: &mut PgConnection,
        id: Uuid,
        response_status: i16,
        response_body: Value,
    ) -> Result<(), DataError> {
        let result = sqlx::query!(
            r#"UPDATE idempotency_keys
               SET state = 'completed', response_status = $2, response_body = $3
               WHERE id = $1"#,
            id,
            response_status,
            response_body,
        )
        .execute(&mut *conn)
        .await?;

        if result.rows_affected() == 0 {
            return Err(DataError::NotFound);
        }
        Ok(())
    }

    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn find(
        conn: &mut PgConnection,
        owner_id: UserId,
        actor_id: UserId,
        key: &str,
    ) -> Result<Option<IdempotencyRecord>, DataError> {
        let row = sqlx::query_as!(
            Row,
            r#"SELECT id, owner_id, actor_id, key, route, request_hash, state, response_status, response_body, expires_at
               FROM idempotency_keys WHERE owner_id = $1 AND actor_id = $2 AND key = $3"#,
            owner_id.as_uuid(),
            actor_id.as_uuid(),
            key,
        )
        .fetch_optional(&mut *conn)
        .await?;
        Ok(row.map(Into::into))
    }

    /// Deletes an `in_progress` row whose request failed without producing a response, so
    /// the client can retry with the same key. Deleting a `completed` row is not this
    /// function's job — `postit-api` (P6) never calls it for one.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn delete(conn: &mut PgConnection, id: Uuid) -> Result<(), DataError> {
        sqlx::query!("DELETE FROM idempotency_keys WHERE id = $1", id)
            .execute(&mut *conn)
            .await?;
        Ok(())
    }
}
