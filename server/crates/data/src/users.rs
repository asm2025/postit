use chrono::{DateTime, Utc};
use emixdb::dto::{Pagination, ResultSet};
use postit_core::UserId;
use sqlx::PgConnection;

use crate::error::DataError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserRole {
    Admin,
    Member,
}

impl UserRole {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Member => "member",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "admin" => Self::Admin,
            _ => Self::Member,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserStatus {
    Pending,
    Active,
    Disabled,
    Deleting,
}

impl UserStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Disabled => "disabled",
            Self::Deleting => "deleting",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "active" => Self::Active,
            "disabled" => Self::Disabled,
            "deleting" => Self::Deleting,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UserRecord {
    pub id: UserId,
    pub oidc_issuer: String,
    pub oidc_subject: String,
    pub email: Option<String>,
    pub email_verified: bool,
    pub display_name: String,
    pub role: UserRole,
    pub status: UserStatus,
    pub approved_at: Option<DateTime<Utc>>,
    pub approved_by: Option<UserId>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

struct UserRow {
    id: uuid::Uuid,
    oidc_issuer: String,
    oidc_subject: String,
    email: Option<String>,
    email_verified: bool,
    display_name: String,
    role: String,
    status: String,
    approved_at: Option<DateTime<Utc>>,
    approved_by: Option<uuid::Uuid>,
    last_seen_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<UserRow> for UserRecord {
    fn from(row: UserRow) -> Self {
        Self {
            id: UserId::from(row.id),
            oidc_issuer: row.oidc_issuer,
            oidc_subject: row.oidc_subject,
            email: row.email,
            email_verified: row.email_verified,
            display_name: row.display_name,
            role: UserRole::parse(&row.role),
            status: UserStatus::parse(&row.status),
            approved_at: row.approved_at,
            approved_by: row.approved_by.map(UserId::from),
            last_seen_at: row.last_seen_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ProvisionOutcome {
    Created,
    Existing,
}

pub struct UsersRepo;

impl UsersRepo {
    /// Inserts a new `pending`/`member` user for `(oidc_issuer, oidc_subject)`, or does
    /// nothing if one already exists. Always returns the row either way.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn provision(
        conn: &mut PgConnection,
        id: UserId,
        oidc_issuer: &str,
        oidc_subject: &str,
        display_name: &str,
    ) -> Result<(ProvisionOutcome, UserRecord), DataError> {
        let inserted = sqlx::query_as!(
            UserRow,
            r#"
            INSERT INTO users (id, oidc_issuer, oidc_subject, display_name, role, status)
            VALUES ($1, $2, $3, $4, 'member', 'pending')
            ON CONFLICT (oidc_issuer, oidc_subject) DO NOTHING
            RETURNING id, oidc_issuer, oidc_subject, email, email_verified, display_name,
                      role, status, approved_at, approved_by, last_seen_at, created_at, updated_at
            "#,
            id.as_uuid(),
            oidc_issuer,
            oidc_subject,
            display_name,
        )
        .fetch_optional(&mut *conn)
        .await?;

        if let Some(row) = inserted {
            return Ok((ProvisionOutcome::Created, row.into()));
        }

        let existing = Self::find_by_oidc(conn, oidc_issuer, oidc_subject)
            .await?
            .ok_or(DataError::NotFound)?;
        Ok((ProvisionOutcome::Existing, existing))
    }

    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn find_by_oidc(
        conn: &mut PgConnection,
        oidc_issuer: &str,
        oidc_subject: &str,
    ) -> Result<Option<UserRecord>, DataError> {
        let row = sqlx::query_as!(
            UserRow,
            r#"SELECT id, oidc_issuer, oidc_subject, email, email_verified, display_name,
                      role, status, approved_at, approved_by, last_seen_at, created_at, updated_at
               FROM users WHERE oidc_issuer = $1 AND oidc_subject = $2"#,
            oidc_issuer,
            oidc_subject,
        )
        .fetch_optional(&mut *conn)
        .await?;
        Ok(row.map(Into::into))
    }

    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn find_by_id(
        conn: &mut PgConnection,
        id: UserId,
    ) -> Result<Option<UserRecord>, DataError> {
        let row = sqlx::query_as!(
            UserRow,
            r#"SELECT id, oidc_issuer, oidc_subject, email, email_verified, display_name,
                      role, status, approved_at, approved_by, last_seen_at, created_at, updated_at
               FROM users WHERE id = $1"#,
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?;
        Ok(row.map(Into::into))
    }

    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn count_active_admins(conn: &mut PgConnection) -> Result<i64, DataError> {
        let count: Option<i64> = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM users WHERE role = 'admin' AND status = 'active'"
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(count.unwrap_or(0))
    }

    /// Promotes `id` to an active admin. Used only by the bootstrap flow, which has already
    /// verified `id` is a newly created user matching the bootstrap rule, under
    /// [`crate::locks::BOOTSTRAP_ADMIN_LOCK_KEY`].
    ///
    /// # Errors
    ///
    /// Returns [`DataError::NotFound`] if `id` doesn't exist, [`DataError::Sql`] otherwise.
    pub async fn grant_admin(conn: &mut PgConnection, id: UserId) -> Result<UserRecord, DataError> {
        let row = sqlx::query_as!(
            UserRow,
            r#"UPDATE users SET role = 'admin', status = 'active', approved_at = now(), updated_at = now()
               WHERE id = $1
               RETURNING id, oidc_issuer, oidc_subject, email, email_verified, display_name,
                         role, status, approved_at, approved_by, last_seen_at, created_at, updated_at"#,
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?;
        row.map(Into::into).ok_or(DataError::NotFound)
    }

    /// Sets `id`'s status and, on the `pending -> active` transition, `approved_at` /
    /// `approved_by`. Issues `NOTIFY postit_user_changed` in the same statement batch, so a
    /// caller running this inside a transaction has the notification queued for delivery on
    /// commit, per Postgres `NOTIFY` semantics.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::NotFound`] if `id` doesn't exist, [`DataError::Sql`] otherwise.
    pub async fn set_status(
        conn: &mut PgConnection,
        id: UserId,
        status: UserStatus,
        approved_by: Option<UserId>,
    ) -> Result<UserRecord, DataError> {
        let approved_by_uuid = approved_by.map(|u| u.as_uuid());
        let row = sqlx::query_as!(
            UserRow,
            r#"UPDATE users SET
                   status = $2,
                   approved_at = CASE WHEN $2 = 'active' AND status = 'pending' THEN now() ELSE approved_at END,
                   approved_by = COALESCE($3, approved_by),
                   updated_at = now()
               WHERE id = $1
               RETURNING id, oidc_issuer, oidc_subject, email, email_verified, display_name,
                         role, status, approved_at, approved_by, last_seen_at, created_at, updated_at"#,
            id.as_uuid(),
            status.as_str(),
            approved_by_uuid,
        )
        .fetch_optional(&mut *conn)
        .await?;
        let row = row.ok_or(DataError::NotFound)?;

        Self::notify_changed(conn, id).await?;

        Ok(row.into())
    }

    /// # Errors
    ///
    /// Returns [`DataError::NotFound`] if `id` doesn't exist, [`DataError::Sql`] otherwise.
    pub async fn set_role(
        conn: &mut PgConnection,
        id: UserId,
        role: UserRole,
    ) -> Result<UserRecord, DataError> {
        let row = sqlx::query_as!(
            UserRow,
            r#"UPDATE users SET role = $2, updated_at = now() WHERE id = $1
               RETURNING id, oidc_issuer, oidc_subject, email, email_verified, display_name,
                         role, status, approved_at, approved_by, last_seen_at, created_at, updated_at"#,
            id.as_uuid(),
            role.as_str(),
        )
        .fetch_optional(&mut *conn)
        .await?;
        let row = row.ok_or(DataError::NotFound)?;

        Self::notify_changed(conn, id).await?;

        Ok(row.into())
    }

    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure. Not finding `id` is not an error
    /// here — this is a best-effort timestamp touch, called on every cache-miss sign-in.
    pub async fn touch_last_seen(conn: &mut PgConnection, id: UserId) -> Result<(), DataError> {
        sqlx::query!(
            "UPDATE users SET last_seen_at = now() WHERE id = $1",
            id.as_uuid()
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn update_profile_claims(
        conn: &mut PgConnection,
        id: UserId,
        email: Option<&str>,
        email_verified: bool,
        display_name: &str,
    ) -> Result<(), DataError> {
        sqlx::query!(
            r#"UPDATE users SET email = $2, email_verified = $3, display_name = $4, updated_at = now()
               WHERE id = $1"#,
            id.as_uuid(),
            email,
            email_verified,
            display_name,
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// Admin list: paginated, optionally filtered by status and a case-insensitive
    /// substring match against display name or email.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn list(
        conn: &mut PgConnection,
        status: Option<UserStatus>,
        search: Option<&str>,
        pagination: Pagination,
    ) -> Result<ResultSet<UserRecord>, DataError> {
        let limit = i64::try_from(pagination.page_size).unwrap_or(10);
        let offset = i64::try_from(
            pagination
                .page
                .saturating_sub(1)
                .saturating_mul(pagination.page_size),
        )
        .unwrap_or(0);
        let status_filter = status.map(UserStatus::as_str);
        let search_pattern = search.map(|s| format!("%{s}%"));

        let rows = sqlx::query_as!(
            UserRow,
            r#"SELECT id, oidc_issuer, oidc_subject, email, email_verified, display_name,
                      role, status, approved_at, approved_by, last_seen_at, created_at, updated_at
               FROM users
               WHERE ($1::text IS NULL OR status = $1)
                 AND ($2::text IS NULL OR display_name ILIKE $2 OR email::text ILIKE $2)
               ORDER BY created_at DESC
               LIMIT $3 OFFSET $4"#,
            status_filter,
            search_pattern,
            limit,
            offset,
        )
        .fetch_all(&mut *conn)
        .await?;

        let total: Option<i64> = sqlx::query_scalar!(
            r#"SELECT COUNT(*) FROM users
               WHERE ($1::text IS NULL OR status = $1)
                 AND ($2::text IS NULL OR display_name ILIKE $2 OR email::text ILIKE $2)"#,
            status_filter,
            search_pattern,
        )
        .fetch_one(&mut *conn)
        .await?;

        Ok(ResultSet {
            data: rows.into_iter().map(Into::into).collect(),
            total: u64::try_from(total.unwrap_or(0)).unwrap_or(0),
            pagination: Some(pagination),
        })
    }

    async fn notify_changed(conn: &mut PgConnection, id: UserId) -> Result<(), DataError> {
        sqlx::query!(
            "SELECT pg_notify('postit_user_changed', $1)",
            id.as_uuid().to_string()
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }
}
