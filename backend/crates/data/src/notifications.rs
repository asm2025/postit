//! In-app notifications: what was recorded for a person, what they have read, and what is
//! still waiting to be emailed or pushed.
//!
//! `emailed_at` means "settled for email": set when the row was emailed (alert or digest), or
//! at insert when the recipient does not want it by email, so such a row never waits for,
//! appears in, or blocks an email.

use chrono::{DateTime, Utc};
use emixdb::dto::{Pagination, ResultSet};
use postit_core::{Notice, NotificationId, NotificationKind, Severity, UserId};
use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::DataError;
use crate::paging;
use crate::text::corrupt;

/// A notification to record.
#[derive(Debug, Clone)]
pub struct NewNotification {
    /// The new row's ID.
    pub id: NotificationId,
    /// The recipient.
    pub user: UserId,
    /// The workspace the notice is about; `None` for deployment notices.
    pub owner: Option<UserId>,
    /// What happened. Its kind decides the stored severity.
    pub notice: Notice,
    /// Whether the recipient wants this by email. `false` settles the row for email at insert.
    pub email_wanted: bool,
    /// When it was recorded.
    pub created_at: DateTime<Utc>,
}

/// A stored notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationRow {
    /// The row.
    pub id: NotificationId,
    /// The recipient.
    pub user: UserId,
    /// The workspace the notice is about.
    pub owner: Option<UserId>,
    /// What happened.
    pub notice: Notice,
    /// How urgent it is.
    pub severity: Severity,
    /// When the recipient read it.
    pub read_at: Option<DateTime<Utc>>,
    /// When it was settled for email.
    pub emailed_at: Option<DateTime<Utc>>,
    /// When it was first pushed.
    pub pushed_at: Option<DateTime<Utc>>,
    /// When it was recorded.
    pub created_at: DateTime<Utc>,
}

/// Which notifications [`NotificationRepo::mark_read`] marks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadSelection {
    /// These notifications; IDs that belong to other users are ignored.
    Ids(Vec<NotificationId>),
    /// Every unread notification of the user.
    All,
}

/// What the bell shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnreadState {
    /// How many notifications are unread.
    pub count: i64,
    /// The newest unread notification, if any.
    pub newest: Option<NotificationId>,
}

struct Row {
    id: Uuid,
    user_id: Uuid,
    owner_id: Option<Uuid>,
    kind: String,
    severity: String,
    payload: Value,
    read_at: Option<DateTime<Utc>>,
    emailed_at: Option<DateTime<Utc>>,
    pushed_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl TryFrom<Row> for NotificationRow {
    type Error = DataError;

    fn try_from(row: Row) -> Result<Self, DataError> {
        let severity = match row.severity.as_str() {
            "info" => Severity::Info,
            "critical" => Severity::Critical,
            other => return Err(corrupt(crate::users::ParseEnumError(other.to_owned()))),
        };
        Ok(Self {
            id: NotificationId::from(row.id),
            user: UserId::from(row.user_id),
            owner: row.owner_id.map(UserId::from),
            notice: Notice::from_row(&row.kind, row.payload).map_err(corrupt)?,
            severity,
            read_at: row.read_at,
            emailed_at: row.emailed_at,
            pushed_at: row.pushed_at,
            created_at: row.created_at,
        })
    }
}

fn convert(rows: Vec<Row>) -> Result<Vec<NotificationRow>, DataError> {
    rows.into_iter().map(NotificationRow::try_from).collect()
}

fn oldest_first(taken: Vec<Row>) -> Result<Vec<NotificationRow>, DataError> {
    let mut taken = convert(taken)?;
    // `RETURNING` keeps no order.
    taken.sort_by_key(|n| (n.created_at, n.id.as_uuid()));
    Ok(taken)
}

/// Persistence for `notifications`. Every read and write is scoped to the recipient; there is
/// no owner scope because a notification belongs to the person it is for.
pub struct NotificationRepo;

impl NotificationRepo {
    /// Records a notification.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure, including a duplicate ID.
    pub async fn insert(conn: &mut PgConnection, new: &NewNotification) -> Result<(), DataError> {
        let emailed_at = (!new.email_wanted).then_some(new.created_at);
        sqlx::query!(
            "INSERT INTO notifications
                 (id, user_id, owner_id, kind, severity, payload, emailed_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            new.id.as_uuid(),
            new.user.as_uuid(),
            new.owner.map(|o| o.as_uuid()),
            new.notice.kind().as_str(),
            new.notice.kind().severity().as_str(),
            new.notice.payload_json(),
            emailed_at,
            new.created_at,
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// The user's notifications, newest first.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure, or a decode error for a stored payload that
    /// no longer matches its kind.
    pub async fn list(
        conn: &mut PgConnection,
        user: UserId,
        pagination: Pagination,
    ) -> Result<ResultSet<NotificationRow>, DataError> {
        let (limit, offset) = paging::bounds(pagination);
        let found = sqlx::query_as!(
            Row,
            "SELECT id, user_id, owner_id, kind, severity, payload, read_at, emailed_at,
                    pushed_at, created_at
             FROM notifications
             WHERE user_id = $1
             ORDER BY created_at DESC, id DESC
             LIMIT $2 OFFSET $3",
            user.as_uuid(),
            limit,
            offset,
        )
        .fetch_all(&mut *conn)
        .await?;
        let total = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM notifications WHERE user_id = $1",
            user.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(ResultSet {
            data: convert(found)?,
            total: paging::total(total),
            pagination: Some(pagination),
        })
    }

    /// Marks unread notifications of `user` read at `now` and returns how many changed.
    /// Already-read rows and other users' IDs are left alone.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn mark_read(
        conn: &mut PgConnection,
        user: UserId,
        selection: &ReadSelection,
        now: DateTime<Utc>,
    ) -> Result<u64, DataError> {
        let result = match selection {
            ReadSelection::All => {
                sqlx::query!(
                    "UPDATE notifications SET read_at = $2
                     WHERE user_id = $1 AND read_at IS NULL",
                    user.as_uuid(),
                    now,
                )
                .execute(&mut *conn)
                .await?
            }
            ReadSelection::Ids(ids) => {
                let ids: Vec<Uuid> = ids.iter().map(NotificationId::as_uuid).collect();
                sqlx::query!(
                    "UPDATE notifications SET read_at = $2
                     WHERE user_id = $1 AND read_at IS NULL AND id = ANY($3)",
                    user.as_uuid(),
                    now,
                    &ids,
                )
                .execute(&mut *conn)
                .await?
            }
        };
        Ok(result.rows_affected())
    }

    /// How many notifications are unread and which is the newest of them.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn unread_state(
        conn: &mut PgConnection,
        user: UserId,
    ) -> Result<UnreadState, DataError> {
        let count = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM notifications WHERE user_id = $1 AND read_at IS NULL",
            user.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await?
        .unwrap_or(0);
        let newest = sqlx::query_scalar!(
            "SELECT id FROM notifications WHERE user_id = $1 AND read_at IS NULL
             ORDER BY created_at DESC, id DESC LIMIT 1",
            user.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?;
        Ok(UnreadState {
            count,
            newest: newest.map(NotificationId::from),
        })
    }

    /// Locks the user's critical rows not yet settled for email, marks exactly those rows
    /// emailed at `now`, and returns them oldest first. A row recorded after the lock is not
    /// touched and is returned by the next call.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn take_unemailed_critical(
        conn: &mut PgConnection,
        user: UserId,
        now: DateTime<Utc>,
    ) -> Result<Vec<NotificationRow>, DataError> {
        let taken = sqlx::query_as!(
            Row,
            "WITH due AS (
                 SELECT id FROM notifications
                 WHERE user_id = $1 AND emailed_at IS NULL AND severity = 'critical'
                 FOR UPDATE)
             UPDATE notifications n SET emailed_at = $2
             FROM due WHERE n.id = due.id
             RETURNING n.id, n.user_id, n.owner_id, n.kind, n.severity, n.payload, n.read_at,
                       n.emailed_at, n.pushed_at, n.created_at",
            user.as_uuid(),
            now,
        )
        .fetch_all(&mut *conn)
        .await?;
        oldest_first(taken)
    }

    /// The same as [`Self::take_unemailed_critical`] for `new_comments` rows, which the daily
    /// digest sends. Marking the rows, instead of comparing times, means a row whose
    /// transaction commits late is never skipped.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn take_undigested_comments(
        conn: &mut PgConnection,
        user: UserId,
        now: DateTime<Utc>,
    ) -> Result<Vec<NotificationRow>, DataError> {
        let taken = sqlx::query_as!(
            Row,
            "WITH due AS (
                 SELECT id FROM notifications
                 WHERE user_id = $1 AND emailed_at IS NULL AND kind = $3
                 FOR UPDATE)
             UPDATE notifications n SET emailed_at = $2
             FROM due WHERE n.id = due.id
             RETURNING n.id, n.user_id, n.owner_id, n.kind, n.severity, n.payload, n.read_at,
                       n.emailed_at, n.pushed_at, n.created_at",
            user.as_uuid(),
            now,
            NotificationKind::NewComments.as_str(),
        )
        .fetch_all(&mut *conn)
        .await?;
        oldest_first(taken)
    }

    /// One of the user's notifications.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the user has no such notification.
    pub async fn get_for_user(
        conn: &mut PgConnection,
        user: UserId,
        id: NotificationId,
    ) -> Result<NotificationRow, DataError> {
        sqlx::query_as!(
            Row,
            "SELECT id, user_id, owner_id, kind, severity, payload, read_at, emailed_at,
                    pushed_at, created_at
             FROM notifications WHERE id = $1 AND user_id = $2",
            id.as_uuid(),
            user.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .map(NotificationRow::try_from)
        .transpose()?
        .ok_or(DataError::NotFound)
    }

    /// Records the first successful push; later calls change nothing.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn mark_pushed(
        conn: &mut PgConnection,
        id: NotificationId,
        at: DateTime<Utc>,
    ) -> Result<(), DataError> {
        sqlx::query!(
            "UPDATE notifications SET pushed_at = $2 WHERE id = $1 AND pushed_at IS NULL",
            id.as_uuid(),
            at,
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }
}
