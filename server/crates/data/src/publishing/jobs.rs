//! Publish jobs: creation with a frozen snapshot, and reads.
//!
//! A job is created while its post is locked ([`LockedPost`]), so no edit commits between the
//! lock and the snapshot, and the snapshot is never written again.

use chrono::{DateTime, Utc};
use emixdb::dto::{Pagination, ResultSet};
use postit_core::{
    AccountId, DeliveryId, JobStatus, PostId, PublishJobId, PublishSnapshot, RecurrenceId, UserId,
};
use sha2::{Digest, Sha256};
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use super::Dispatch;
use crate::constraint::translate;
use crate::error::DataError;
use crate::paging;
use crate::posts::{LockedPost, Post};
use crate::text::{corrupt, text_enum};
use crate::{Capability, OwnerScope};

text_enum! {
    /// What a delivery does on its platform.
    Operation {
        /// Publishes a post.
        Publish => "publish",
        /// Publishes a comment on someone's post.
        Comment => "comment",
        /// Sends to each contact of an audience.
        Direct => "direct",
    }
}

/// The idempotency key of the delivery of `job` to `account`: SHA-256 of the job's 16 UUID
/// bytes followed by the account's 16. Platforms that accept a client key receive it, and the
/// unique column keeps a job from holding two deliveries for one account.
#[must_use]
pub fn delivery_idempotency_key(job: PublishJobId, account: AccountId) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(job.as_uuid().as_bytes());
    hasher.update(account.as_uuid().as_bytes());
    let mut key = [0_u8; 32];
    key.copy_from_slice(&hasher.finalize());
    key
}

/// One delivery of a new job.
#[derive(Debug, Clone)]
pub struct NewDelivery {
    /// The delivery's ID.
    pub id: DeliveryId,
    /// The account it goes to.
    pub account: AccountId,
    /// What it does there.
    pub operation: Operation,
}

/// A job to create.
#[derive(Debug, Clone)]
pub struct NewJob {
    /// The job's ID.
    pub id: PublishJobId,
    /// What every target publishes, frozen now.
    pub snapshot: PublishSnapshot,
    /// One delivery per target account.
    pub targets: Vec<NewDelivery>,
    /// When the deliveries become due.
    pub scheduled_at: DateTime<Utc>,
    /// Whether the platforms hold the post until its time.
    pub native_schedule: bool,
    /// Whether the job only simulates.
    pub dry_run: bool,
    /// The recurrence that made the job.
    pub recurrence_id: Option<RecurrenceId>,
}

/// A created job and the deliveries to enqueue.
#[derive(Debug, Clone)]
pub struct CreatedJob {
    /// The job.
    pub job: PublishJobId,
    /// One dispatch per delivery.
    pub deliveries: Vec<Dispatch>,
}

/// A publish job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishJob {
    /// The job.
    pub id: PublishJobId,
    /// The post it publishes.
    pub post_id: PostId,
    /// The post revision it froze.
    pub post_version: i64,
    /// The recurrence that made it.
    pub recurrence_id: Option<RecurrenceId>,
    /// When its deliveries become due.
    pub scheduled_at: DateTime<Utc>,
    /// Whether the platforms hold the post until its time.
    pub native_schedule: bool,
    /// Whether it only simulates.
    pub dry_run: bool,
    /// Where it is, derived from its deliveries.
    pub status: JobStatus,
    /// Bumped on every delivery change.
    pub version: i64,
    /// When it was cancelled.
    pub cancelled_at: Option<DateTime<Utc>>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
}

struct JobRow {
    id: Uuid,
    post_id: Uuid,
    post_version: i32,
    recurrence_id: Option<Uuid>,
    scheduled_at: DateTime<Utc>,
    native_schedule: bool,
    dry_run: bool,
    status: String,
    version: i32,
    cancelled_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl TryFrom<JobRow> for PublishJob {
    type Error = DataError;

    fn try_from(row: JobRow) -> Result<Self, DataError> {
        Ok(Self {
            id: PublishJobId::from_uuid(row.id),
            post_id: PostId::from_uuid(row.post_id),
            post_version: i64::from(row.post_version),
            recurrence_id: row.recurrence_id.map(RecurrenceId::from_uuid),
            scheduled_at: row.scheduled_at,
            native_schedule: row.native_schedule,
            dry_run: row.dry_run,
            status: row.status.parse().map_err(corrupt)?,
            version: i64::from(row.version),
            cancelled_at: row.cancelled_at,
            created_at: row.created_at,
        })
    }
}

/// Narrows [`JobRepo::list`].
#[derive(Debug, Clone, Copy, Default)]
pub struct JobFilter {
    /// Only this post's jobs.
    pub post_id: Option<PostId>,
    /// Only jobs in this status.
    pub status: Option<JobStatus>,
}

/// Publish jobs.
pub struct JobRepo;

impl JobRepo {
    /// Creates a job for the locked post, with one `pending` delivery per target, due at
    /// `scheduled_at`. The job starts `publishing` when it is already due, else `scheduled`,
    /// and a live job puts the same status on the post. The post version is not bumped.
    ///
    /// A live job needs [`Capability::Publish`] and a dry run [`Capability::Edit`]. Every
    /// target must be in the scope's account limit.
    ///
    /// # Errors
    ///
    /// [`DataError::AccountNotInScope`] for a target outside the limit,
    /// [`DataError::JobActive`] when a concurrent job took the post's live slot, and
    /// [`DataError::Conflict`] when the snapshot is of another post or two targets share an
    /// account.
    pub async fn create(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        locked: &LockedPost,
        new: NewJob,
    ) -> Result<CreatedJob, DataError> {
        scope.require(if new.dry_run {
            Capability::Edit
        } else {
            Capability::Publish
        })?;
        if new
            .targets
            .iter()
            .any(|target| !scope.allows_account(target.account))
        {
            return Err(DataError::AccountNotInScope);
        }
        insert(conn, scope.owner, Some(scope.actor), locked.post(), new).await
    }

    /// One job the scope can see: a delegate limited to some accounts sees a job only when one
    /// of its deliveries is in the limit.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such job in view.
    pub async fn get(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: PublishJobId,
    ) -> Result<PublishJob, DataError> {
        scope.require(Capability::View)?;
        let filter = scope.account_filter();
        sqlx::query_as!(
            JobRow,
            "SELECT j.id, j.post_id, j.post_version, j.recurrence_id, j.scheduled_at,
                    j.native_schedule, j.dry_run, j.status, j.version, j.cancelled_at,
                    j.created_at
             FROM publish_jobs j
             WHERE j.owner_id = $1 AND j.id = $2
               AND ($3::uuid[] IS NULL OR EXISTS (
                   SELECT 1 FROM deliveries d
                   WHERE d.job_id = j.id AND d.account_id = ANY($3)))",
            scope.owner.as_uuid(),
            id.as_uuid(),
            filter.as_deref(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?
        .try_into()
    }

    /// The jobs the scope can see, newest first.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn list(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        pagination: Pagination,
        filter: JobFilter,
    ) -> Result<ResultSet<PublishJob>, DataError> {
        scope.require(Capability::View)?;
        let (limit, offset) = paging::bounds(pagination);
        let accounts = scope.account_filter();
        let post = filter.post_id.map(|id| id.as_uuid());
        let status = filter.status.map(|status| status.as_str());
        let rows = sqlx::query_as!(
            JobRow,
            "SELECT j.id, j.post_id, j.post_version, j.recurrence_id, j.scheduled_at,
                    j.native_schedule, j.dry_run, j.status, j.version, j.cancelled_at,
                    j.created_at
             FROM publish_jobs j
             WHERE j.owner_id = $1
               AND ($2::uuid IS NULL OR j.post_id = $2)
               AND ($3::text IS NULL OR j.status = $3)
               AND ($4::uuid[] IS NULL OR EXISTS (
                   SELECT 1 FROM deliveries d
                   WHERE d.job_id = j.id AND d.account_id = ANY($4)))
             ORDER BY j.created_at DESC, j.id DESC
             LIMIT $5 OFFSET $6",
            scope.owner.as_uuid(),
            post,
            status,
            accounts.as_deref(),
            limit,
            offset,
        )
        .fetch_all(&mut *conn)
        .await?;
        let total = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM publish_jobs j
             WHERE j.owner_id = $1
               AND ($2::uuid IS NULL OR j.post_id = $2)
               AND ($3::text IS NULL OR j.status = $3)
               AND ($4::uuid[] IS NULL OR EXISTS (
                   SELECT 1 FROM deliveries d
                   WHERE d.job_id = j.id AND d.account_id = ANY($4)))",
            scope.owner.as_uuid(),
            post,
            status,
            accounts.as_deref(),
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(ResultSet {
            data: rows
                .into_iter()
                .map(PublishJob::try_from)
                .collect::<Result<_, _>>()?,
            total: paging::total(total),
            pagination: Some(pagination),
        })
    }

    /// The content the job froze. A system read for the worker, which holds no scope.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when there is no such job.
    pub async fn snapshot(
        conn: &mut PgConnection,
        id: PublishJobId,
    ) -> Result<PublishSnapshot, DataError> {
        let value = sqlx::query_scalar!(
            "SELECT content_snapshot FROM publish_jobs WHERE id = $1",
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?;
        serde_json::from_value(value).map_err(corrupt)
    }
}

/// Inserts the job and its deliveries for `post`, in a savepoint. `actor` is `None` for a
/// system write. Shared by [`JobRepo::create`] and the recurrence tick.
pub(crate) async fn insert(
    conn: &mut PgConnection,
    owner: UserId,
    actor: Option<UserId>,
    post: &Post,
    new: NewJob,
) -> Result<CreatedJob, DataError> {
    if new.snapshot.post_id != post.id {
        return Err(DataError::Conflict("snapshot_post_mismatch".to_string()));
    }
    let post_version = i32::try_from(post.version)
        .map_err(|_| DataError::Conflict("post_version_out_of_range".to_string()))?;
    let snapshot = serde_json::to_value(&new.snapshot).map_err(corrupt)?;
    let actor = actor.map(|actor| actor.as_uuid());
    let mut tx = conn.begin().await?;
    let status = sqlx::query_scalar!(
        r#"INSERT INTO publish_jobs
               (id, owner_id, post_id, post_version, recurrence_id, content_snapshot,
                scheduled_at, native_schedule, dry_run, status, created_by, updated_by)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9,
                   CASE WHEN $7 <= now() THEN 'publishing' ELSE 'scheduled' END, $10, $10)
           RETURNING status AS "status!""#,
        new.id.as_uuid(),
        owner.as_uuid(),
        post.id.as_uuid(),
        post_version,
        new.recurrence_id.map(|id| id.as_uuid()),
        snapshot,
        new.scheduled_at,
        new.native_schedule,
        new.dry_run,
        actor,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(translate)?;
    let mut deliveries = Vec::with_capacity(new.targets.len());
    for target in &new.targets {
        let key = delivery_idempotency_key(new.id, target.account);
        sqlx::query!(
            "INSERT INTO deliveries
                 (id, owner_id, job_id, post_id, account_id, operation, idempotency_key,
                  next_attempt_at, created_by, updated_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9)",
            target.id.as_uuid(),
            owner.as_uuid(),
            new.id.as_uuid(),
            post.id.as_uuid(),
            target.account.as_uuid(),
            target.operation.as_str(),
            &key[..],
            new.scheduled_at,
            actor,
        )
        .execute(&mut *tx)
        .await
        .map_err(translate)?;
        deliveries.push(Dispatch {
            delivery: target.id,
            dispatch_gen: 0,
            run_at: new.scheduled_at,
        });
    }
    if !new.dry_run {
        sqlx::query!(
            "UPDATE posts SET status = $3 WHERE owner_id = $1 AND id = $2",
            owner.as_uuid(),
            post.id.as_uuid(),
            status,
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(CreatedJob {
        job: new.id,
        deliveries,
    })
}

#[cfg(test)]
mod tests {
    use sqlx::PgPool;
    use uuid::Uuid;

    use crate::constraint::translate;
    use crate::error::DataError;
    use crate::testkit::seed_user;

    async fn live_job(pool: &PgPool, owner: Uuid, post: Uuid) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO publish_jobs              (id, owner_id, post_id, post_version, content_snapshot, scheduled_at, status)              VALUES ($1, $2, $3, 1, '{}', now(), 'publishing')",
        )
        .bind(Uuid::now_v7())
        .bind(owner)
        .bind(post)
        .execute(pool)
        .await
        .map(|_| ())
    }

    #[sqlx::test]
    async fn one_live_job_index_maps_to_job_active(pool: PgPool) {
        let owner = seed_user(&pool, "active").await.as_uuid();
        let post = Uuid::now_v7();
        sqlx::query("INSERT INTO posts (id, owner_id, text) VALUES ($1, $2, 'hi')")
            .bind(post)
            .bind(owner)
            .execute(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("post: {e}"));
        live_job(&pool, owner, post)
            .await
            .unwrap_or_else(|e| unreachable!("first job: {e}"));

        let err = live_job(&pool, owner, post)
            .await
            .err()
            .unwrap_or_else(|| unreachable!("a second live job"));

        assert!(matches!(translate(err), DataError::JobActive));
    }
}
