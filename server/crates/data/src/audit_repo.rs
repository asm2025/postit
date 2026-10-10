//! Read access to the audit log.

use chrono::{DateTime, Utc};
use emixdb::dto::{Pagination, ResultSet};
use postit_core::{AuditEventId, UserId};
use serde_json::Value;
use sqlx::PgConnection;

use crate::error::DataError;

/// One audit event as read back for the admin audit log.
#[derive(Debug, Clone)]
pub struct AuditEventRow {
    /// Event ID.
    pub id: AuditEventId,
    /// When the event was recorded.
    pub at: DateTime<Utc>,
    /// User who performed the action; none for system actions.
    pub actor_user_id: Option<UserId>,
    /// Owner of the workspace the action touched, when there is one.
    pub owner_id: Option<UserId>,
    /// User the action was about, when there is one.
    pub subject_user_id: Option<UserId>,
    /// Dotted event name such as `user.approved`.
    pub kind: String,
    /// Client IP; cleared once past `audit.ip_retention`.
    pub ip: Option<String>,
    /// `X-Request-Id` of the request that caused the event.
    pub request_id: Option<uuid::Uuid>,
    /// Event-specific details, never secrets.
    pub details: Value,
}

struct Row {
    id: uuid::Uuid,
    at: DateTime<Utc>,
    actor_user_id: Option<uuid::Uuid>,
    owner_id: Option<uuid::Uuid>,
    subject_user_id: Option<uuid::Uuid>,
    kind: String,
    ip: Option<String>,
    request_id: Option<uuid::Uuid>,
    details: Value,
}

impl From<Row> for AuditEventRow {
    fn from(row: Row) -> Self {
        Self {
            id: AuditEventId::from(row.id),
            at: row.at,
            actor_user_id: row.actor_user_id.map(UserId::from),
            owner_id: row.owner_id.map(UserId::from),
            subject_user_id: row.subject_user_id.map(UserId::from),
            kind: row.kind,
            ip: row.ip,
            request_id: row.request_id,
            details: row.details,
        }
    }
}

/// Optional narrowing of an audit listing; unset fields match everything.
#[derive(Debug, Clone, Default)]
pub struct AuditFilter {
    /// Exact event kind.
    pub kind: Option<String>,
    /// Earliest event time, inclusive.
    pub from: Option<DateTime<Utc>>,
    /// Latest event time, inclusive.
    pub to: Option<DateTime<Utc>>,
    /// User who performed the action.
    pub actor_user_id: Option<UserId>,
    /// User the action was about.
    pub subject_user_id: Option<UserId>,
}

/// Read access to the audit log.
pub struct AuditRepo;

impl AuditRepo {
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn list(
        conn: &mut PgConnection,
        filter: &AuditFilter,
        pagination: Pagination,
    ) -> Result<ResultSet<AuditEventRow>, DataError> {
        let limit = i64::try_from(pagination.page_size).unwrap_or(10);
        let offset = i64::try_from(
            pagination
                .page
                .saturating_sub(1)
                .saturating_mul(pagination.page_size),
        )
        .unwrap_or(0);
        let actor = filter.actor_user_id.map(|u| u.as_uuid());
        let subject = filter.subject_user_id.map(|u| u.as_uuid());

        let rows = sqlx::query_as!(
            Row,
            r#"SELECT id, at, actor_user_id, owner_id, subject_user_id, kind, ip, request_id, details
               FROM audit_events
               WHERE ($1::text IS NULL OR kind = $1)
                 AND ($2::timestamptz IS NULL OR at >= $2)
                 AND ($3::timestamptz IS NULL OR at <= $3)
                 AND ($4::uuid IS NULL OR actor_user_id = $4)
                 AND ($5::uuid IS NULL OR subject_user_id = $5)
               ORDER BY at DESC, id DESC
               LIMIT $6 OFFSET $7"#,
            filter.kind,
            filter.from,
            filter.to,
            actor,
            subject,
            limit,
            offset,
        )
        .fetch_all(&mut *conn)
        .await?;

        let total: Option<i64> = sqlx::query_scalar!(
            r#"SELECT COUNT(*) FROM audit_events
               WHERE ($1::text IS NULL OR kind = $1)
                 AND ($2::timestamptz IS NULL OR at >= $2)
                 AND ($3::timestamptz IS NULL OR at <= $3)
                 AND ($4::uuid IS NULL OR actor_user_id = $4)
                 AND ($5::uuid IS NULL OR subject_user_id = $5)"#,
            filter.kind,
            filter.from,
            filter.to,
            actor,
            subject,
        )
        .fetch_one(&mut *conn)
        .await?;

        Ok(ResultSet {
            data: rows.into_iter().map(Into::into).collect(),
            total: u64::try_from(total.unwrap_or(0)).unwrap_or(0),
            pagination: Some(pagination),
        })
    }
}
