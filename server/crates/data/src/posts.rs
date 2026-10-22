//! Posts, their media, targets, and per-account overrides.
//!
//! A post is edited only through a [`LockedPost`], which [`PostRepo::lock`] hands out after
//! it has row-locked the post, checked the caller's version, and made sure no publish job is
//! scheduled or running for it. Every write through the token bumps the post's version once and
//! pauses the post's recurrence, so a recurrence never publishes content nobody confirmed.
//!
//! Reads need [`Capability::View`] and writes need [`Capability::Edit`]. A post is visible whole
//! to a delegate whose account limit is narrower than its targets; the account-bound rows
//! (targets and overrides) are filtered by the limit. Writes run in a savepoint, so a failed
//! write leaves the caller's transaction as it was.

use chrono::{DateTime, Utc};
use emixdb::dto::{Pagination, ResultSet};
use postit_core::{AccountId, CampaignId, MediaAssetId, PostId, PostKind};
use serde_json::Value;
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::constraint::{is_foreign_key_violation, translate};
use crate::error::DataError;
use crate::paging;
use crate::text::{corrupt, parse, text_enum};
use crate::{Capability, OwnerScope};

text_enum! {
    /// Where a post is in its publishing life.
    PostStatus {
        /// Being written.
        Draft => "draft",
        /// A publish job waits for its time.
        Scheduled => "scheduled",
        /// A publish job is running.
        Publishing => "publishing",
        /// Every delivery went out.
        Published => "published",
        /// Some deliveries went out.
        Partial => "partial",
        /// No delivery went out.
        Failed => "failed",
        /// The job was cancelled.
        Cancelled => "cancelled",
    }
}

/// A post.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Post {
    /// The post.
    pub id: PostId,
    /// Whether it starts a conversation or answers one.
    pub kind: PostKind,
    /// What a comment answers, in the platform's terms.
    pub reply_to: Option<Value>,
    /// The campaign it belongs to.
    pub campaign_id: Option<CampaignId>,
    /// The text.
    pub text: String,
    /// The link to attach.
    pub link: Option<String>,
    /// The tags.
    pub tags: Vec<String>,
    /// Whether to disclose that AI wrote it.
    pub disclose_ai_generated: bool,
    /// Counts content changes, starting at 1.
    pub version: i64,
    /// Where it is in its publishing life.
    pub status: PostStatus,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

struct PostRow {
    id: Uuid,
    kind: String,
    reply_to: Option<Value>,
    campaign_id: Option<Uuid>,
    text: String,
    link: Option<String>,
    tags: Vec<String>,
    disclose_ai_generated: bool,
    version: i32,
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<PostRow> for Post {
    type Error = DataError;

    fn try_from(row: PostRow) -> Result<Self, DataError> {
        Ok(Self {
            id: PostId::from_uuid(row.id),
            kind: row.kind.parse().map_err(corrupt)?,
            reply_to: row.reply_to,
            campaign_id: row.campaign_id.map(CampaignId::from_uuid),
            text: row.text,
            link: row.link,
            tags: row.tags,
            disclose_ai_generated: row.disclose_ai_generated,
            version: i64::from(row.version),
            status: parse(&row.status)?,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

/// A post to create.
#[derive(Debug, Clone)]
pub struct NewPost {
    /// The post's ID.
    pub id: PostId,
    /// Whether it starts a conversation or answers one.
    pub kind: PostKind,
    /// What a comment answers.
    pub reply_to: Option<Value>,
    /// The campaign it belongs to.
    pub campaign_id: Option<CampaignId>,
    /// The text.
    pub text: String,
    /// The link to attach.
    pub link: Option<String>,
    /// The tags.
    pub tags: Vec<String>,
    /// Whether to disclose that AI wrote it.
    pub disclose_ai_generated: bool,
}

/// The editable content of a post.
#[derive(Debug, Clone)]
pub struct PostContent {
    /// What a comment answers.
    pub reply_to: Option<Value>,
    /// The campaign it belongs to.
    pub campaign_id: Option<CampaignId>,
    /// The text.
    pub text: String,
    /// The link to attach.
    pub link: Option<String>,
    /// The tags.
    pub tags: Vec<String>,
    /// Whether to disclose that AI wrote it.
    pub disclose_ai_generated: bool,
}

/// Which posts [`PostRepo::list`] returns. A `None` field does not filter.
#[derive(Debug, Clone, Copy, Default)]
pub struct PostFilter {
    /// Only posts in this status.
    pub status: Option<PostStatus>,
    /// Only posts in this campaign.
    pub campaign_id: Option<CampaignId>,
    /// Only posts of this kind.
    pub kind: Option<PostKind>,
}

/// One media attachment of a post.
#[derive(Debug, Clone)]
pub struct PostMediaItem {
    /// The asset.
    pub media_id: MediaAssetId,
    /// Its place among the attachments, counting from 0.
    pub position: i32,
    /// Alternative text.
    pub alt_text: Option<String>,
}

/// A per-account override of a post.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostOverrideRow {
    /// The account it applies to.
    pub account_id: AccountId,
    /// The fields that differ from the post.
    pub patch: Value,
    /// Platform options for this account.
    pub options: Value,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

/// A post held for editing. Only [`PostRepo::lock`] makes one.
///
/// It holds the row lock for the rest of the caller's transaction. [`LockedPost::post`] is the
/// post as it was when locked; the writes bump the stored version without changing it.
#[derive(Debug)]
pub struct LockedPost {
    post: Post,
    scope: OwnerScope,
}

impl LockedPost {
    /// The post as it was when locked.
    #[must_use]
    pub fn post(&self) -> &Post {
        &self.post
    }
}

/// Posts.
pub struct PostRepo;

impl PostRepo {
    /// Creates a draft at version 1.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when `campaign_id` is not the owner's campaign.
    pub async fn create(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        new: NewPost,
    ) -> Result<Post, DataError> {
        scope.require(Capability::Edit)?;
        sqlx::query_as!(
            PostRow,
            "INSERT INTO posts
                 (id, owner_id, kind, reply_to, campaign_id, text, link, tags,
                  disclose_ai_generated, created_by, updated_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $10)
             RETURNING id, kind, reply_to, campaign_id, text, link, tags,
                       disclose_ai_generated, version, status, created_at, updated_at",
            new.id.as_uuid(),
            scope.owner.as_uuid(),
            new.kind.as_str(),
            new.reply_to,
            new.campaign_id.map(|id| id.as_uuid()),
            new.text,
            new.link,
            &new.tags,
            new.disclose_ai_generated,
            scope.actor.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(reference_missing)?
        .try_into()
    }

    /// One post, whole, whatever the scope's account limit.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such post.
    pub async fn get(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: PostId,
    ) -> Result<Post, DataError> {
        scope.require(Capability::View)?;
        sqlx::query_as!(
            PostRow,
            "SELECT id, kind, reply_to, campaign_id, text, link, tags,
                    disclose_ai_generated, version, status, created_at, updated_at
             FROM posts WHERE owner_id = $1 AND id = $2",
            scope.owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?
        .try_into()
    }

    /// The owner's posts, newest first.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn list(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        pagination: Pagination,
        filter: PostFilter,
    ) -> Result<ResultSet<Post>, DataError> {
        scope.require(Capability::View)?;
        let (limit, offset) = paging::bounds(pagination);
        let status = filter.status.map(PostStatus::as_str);
        let campaign = filter.campaign_id.map(|id| id.as_uuid());
        let kind = filter.kind.map(|kind| kind.as_str());
        let rows = sqlx::query_as!(
            PostRow,
            "SELECT id, kind, reply_to, campaign_id, text, link, tags,
                    disclose_ai_generated, version, status, created_at, updated_at
             FROM posts
             WHERE owner_id = $1
               AND ($2::text IS NULL OR status = $2)
               AND ($3::uuid IS NULL OR campaign_id = $3)
               AND ($4::text IS NULL OR kind = $4)
             ORDER BY created_at DESC, id DESC
             LIMIT $5 OFFSET $6",
            scope.owner.as_uuid(),
            status,
            campaign,
            kind,
            limit,
            offset,
        )
        .fetch_all(&mut *conn)
        .await?;
        let total = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM posts
             WHERE owner_id = $1
               AND ($2::text IS NULL OR status = $2)
               AND ($3::uuid IS NULL OR campaign_id = $3)
               AND ($4::text IS NULL OR kind = $4)",
            scope.owner.as_uuid(),
            status,
            campaign,
            kind,
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(ResultSet {
            data: rows
                .into_iter()
                .map(Post::try_from)
                .collect::<Result<_, _>>()?,
            total: paging::total(total),
            pagination: Some(pagination),
        })
    }

    /// Locks a post for editing until the caller's transaction ends. Call it inside a
    /// transaction; outside one the row lock lasts only for this statement.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such post. [`DataError::VersionConflict`]
    /// when `expected_version` is set and is not the stored version; it comes before
    /// [`DataError::PostLocked`], which means a publish job of the post, dry run included, is
    /// scheduled or running.
    pub async fn lock(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: PostId,
        expected_version: Option<i64>,
    ) -> Result<LockedPost, DataError> {
        scope.require(Capability::Edit)?;
        let post: Post = sqlx::query_as!(
            PostRow,
            "SELECT id, kind, reply_to, campaign_id, text, link, tags,
                    disclose_ai_generated, version, status, created_at, updated_at
             FROM posts WHERE owner_id = $1 AND id = $2 FOR UPDATE",
            scope.owner.as_uuid(),
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?
        .try_into()?;
        if expected_version.is_some_and(|expected| expected != post.version) {
            return Err(DataError::VersionConflict);
        }
        let active = sqlx::query_scalar!(
            r#"SELECT EXISTS (
                   SELECT 1 FROM publish_jobs
                   WHERE post_id = $1 AND status IN ('scheduled', 'publishing')
               ) AS "active!""#,
            id.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await?;
        if active {
            return Err(DataError::PostLocked);
        }
        Ok(LockedPost {
            post,
            scope: scope.clone(),
        })
    }

    /// Replaces the post's content.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when `campaign_id` is not the owner's campaign.
    pub async fn update_content(
        conn: &mut PgConnection,
        locked: &LockedPost,
        content: PostContent,
    ) -> Result<(), DataError> {
        let mut tx = conn.begin().await?;
        sqlx::query!(
            "UPDATE posts
             SET reply_to = $3, campaign_id = $4, text = $5, link = $6, tags = $7,
                 disclose_ai_generated = $8
             WHERE owner_id = $1 AND id = $2",
            locked.scope.owner.as_uuid(),
            locked.post.id.as_uuid(),
            content.reply_to,
            content.campaign_id.map(|id| id.as_uuid()),
            content.text,
            content.link,
            &content.tags,
            content.disclose_ai_generated,
        )
        .execute(&mut *tx)
        .await
        .map_err(reference_missing)?;
        bump_version(&mut tx, locked).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Replaces the post's media with `items`.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when an item is not the owner's asset, and
    /// [`DataError::Conflict`] when two items share a position or an asset.
    pub async fn set_media(
        conn: &mut PgConnection,
        locked: &LockedPost,
        items: &[PostMediaItem],
    ) -> Result<(), DataError> {
        let mut tx = conn.begin().await?;
        sqlx::query!(
            "DELETE FROM post_media WHERE owner_id = $1 AND post_id = $2",
            locked.scope.owner.as_uuid(),
            locked.post.id.as_uuid(),
        )
        .execute(&mut *tx)
        .await?;
        for item in items {
            let inserted = sqlx::query!(
                "INSERT INTO post_media (post_id, media_id, owner_id, position, alt_text)
                 SELECT $2, id, owner_id, $4, $5 FROM media_assets
                 WHERE owner_id = $1 AND id = $3",
                locked.scope.owner.as_uuid(),
                locked.post.id.as_uuid(),
                item.media_id.as_uuid(),
                item.position,
                item.alt_text,
            )
            .execute(&mut *tx)
            .await
            .map_err(translate)?
            .rows_affected();
            if inserted == 0 {
                return Err(DataError::NotFound);
            }
        }
        bump_version(&mut tx, locked).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Replaces the post's targets inside the caller's account limit with `accounts`. Targets
    /// outside the limit stay.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when an account is outside the limit, not the owner's, or
    /// disconnected.
    pub async fn set_targets(
        conn: &mut PgConnection,
        locked: &LockedPost,
        accounts: &[AccountId],
    ) -> Result<(), DataError> {
        if accounts
            .iter()
            .any(|account| !locked.scope.allows_account(*account))
        {
            return Err(DataError::NotFound);
        }
        let filter = locked.scope.account_filter();
        let mut tx = conn.begin().await?;
        sqlx::query!(
            "DELETE FROM post_targets
             WHERE owner_id = $1 AND post_id = $2
               AND ($3::uuid[] IS NULL OR account_id = ANY($3))",
            locked.scope.owner.as_uuid(),
            locked.post.id.as_uuid(),
            filter.as_deref(),
        )
        .execute(&mut *tx)
        .await?;
        for account in accounts {
            let inserted = sqlx::query!(
                "INSERT INTO post_targets (post_id, account_id, owner_id)
                 SELECT $2, id, owner_id FROM connected_accounts
                 WHERE owner_id = $1 AND id = $3 AND status <> 'disconnected'
                 ON CONFLICT DO NOTHING",
                locked.scope.owner.as_uuid(),
                locked.post.id.as_uuid(),
                account.as_uuid(),
            )
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if inserted == 0 && !target_exists(&mut tx, locked, *account).await? {
                return Err(DataError::NotFound);
            }
        }
        bump_version(&mut tx, locked).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Sets the override for one account, replacing any earlier one.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the account is outside the caller's limit, not the owner's,
    /// or disconnected.
    pub async fn upsert_override(
        conn: &mut PgConnection,
        locked: &LockedPost,
        account: AccountId,
        patch: Value,
        options: Value,
    ) -> Result<(), DataError> {
        if !locked.scope.allows_account(account) {
            return Err(DataError::NotFound);
        }
        let mut tx = conn.begin().await?;
        let written = sqlx::query!(
            "INSERT INTO post_overrides
                 (post_id, account_id, owner_id, patch, options, created_by, updated_by)
             SELECT $2, id, owner_id, $4, $5, $6, $6 FROM connected_accounts
             WHERE owner_id = $1 AND id = $3 AND status <> 'disconnected'
             ON CONFLICT (post_id, account_id) DO UPDATE
             SET patch = EXCLUDED.patch, options = EXCLUDED.options,
                 updated_at = now(), updated_by = EXCLUDED.updated_by",
            locked.scope.owner.as_uuid(),
            locked.post.id.as_uuid(),
            account.as_uuid(),
            patch,
            options,
            locked.scope.actor.as_uuid(),
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if written == 0 {
            return Err(DataError::NotFound);
        }
        bump_version(&mut tx, locked).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Removes the override for one account.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the account is outside the caller's limit or has no
    /// override on this post.
    pub async fn delete_override(
        conn: &mut PgConnection,
        locked: &LockedPost,
        account: AccountId,
    ) -> Result<(), DataError> {
        if !locked.scope.allows_account(account) {
            return Err(DataError::NotFound);
        }
        let mut tx = conn.begin().await?;
        let deleted = sqlx::query!(
            "DELETE FROM post_overrides
             WHERE owner_id = $1 AND post_id = $2 AND account_id = $3",
            locked.scope.owner.as_uuid(),
            locked.post.id.as_uuid(),
            account.as_uuid(),
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if deleted == 0 {
            return Err(DataError::NotFound);
        }
        bump_version(&mut tx, locked).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Deletes the post with its media links, targets, overrides, recurrence, jobs, and
    /// deliveries. It needs `Publish` when the post has any live publish job, since the
    /// delivery history goes with it, and `Edit` otherwise.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] when the caller's level is too low.
    pub async fn delete(conn: &mut PgConnection, locked: LockedPost) -> Result<(), DataError> {
        let has_live_history = sqlx::query_scalar!(
            r#"SELECT EXISTS (
                   SELECT 1 FROM publish_jobs WHERE post_id = $1 AND NOT dry_run
               ) AS "live!""#,
            locked.post.id.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await?;
        locked.scope.require(if has_live_history {
            Capability::Publish
        } else {
            Capability::Edit
        })?;
        sqlx::query!(
            "DELETE FROM posts WHERE owner_id = $1 AND id = $2",
            locked.scope.owner.as_uuid(),
            locked.post.id.as_uuid(),
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// The post's targets the scope's account limit reaches.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such post.
    pub async fn targets(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: PostId,
    ) -> Result<Vec<AccountId>, DataError> {
        Self::get(conn, scope, id).await?;
        let filter = scope.account_filter();
        let rows = sqlx::query_scalar!(
            "SELECT account_id FROM post_targets
             WHERE owner_id = $1 AND post_id = $2
               AND ($3::uuid[] IS NULL OR account_id = ANY($3))
             ORDER BY created_at, account_id",
            scope.owner.as_uuid(),
            id.as_uuid(),
            filter.as_deref(),
        )
        .fetch_all(&mut *conn)
        .await?;
        Ok(rows.into_iter().map(AccountId::from_uuid).collect())
    }

    /// The post's overrides the scope's account limit reaches.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such post.
    pub async fn overrides(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: PostId,
    ) -> Result<Vec<PostOverrideRow>, DataError> {
        Self::get(conn, scope, id).await?;
        let filter = scope.account_filter();
        let rows = sqlx::query!(
            "SELECT account_id, patch, options, updated_at FROM post_overrides
             WHERE owner_id = $1 AND post_id = $2
               AND ($3::uuid[] IS NULL OR account_id = ANY($3))
             ORDER BY created_at, account_id",
            scope.owner.as_uuid(),
            id.as_uuid(),
            filter.as_deref(),
        )
        .fetch_all(&mut *conn)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| PostOverrideRow {
                account_id: AccountId::from_uuid(row.account_id),
                patch: row.patch,
                options: row.options,
                updated_at: row.updated_at,
            })
            .collect())
    }

    /// Whether anything on the post belongs to an account outside the scope's limit: a target,
    /// an override, or a delivery that has not ended. Always `false` without a limit.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such post.
    pub async fn outside_limit(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: PostId,
    ) -> Result<bool, DataError> {
        Self::get(conn, scope, id).await?;
        let filter = scope.account_filter();
        let hidden = sqlx::query_scalar!(
            r#"SELECT (
                   EXISTS (SELECT 1 FROM post_targets
                           WHERE owner_id = $1 AND post_id = $2
                             AND $3::uuid[] IS NOT NULL AND account_id <> ALL($3))
                   OR EXISTS (SELECT 1 FROM post_overrides
                              WHERE owner_id = $1 AND post_id = $2
                                AND $3::uuid[] IS NOT NULL AND account_id <> ALL($3))
                   OR EXISTS (SELECT 1 FROM deliveries
                              WHERE owner_id = $1 AND post_id = $2
                                AND state NOT IN ('published', 'failed', 'dead_letter',
                                                  'cancelled', 'simulated')
                                AND $3::uuid[] IS NOT NULL AND account_id <> ALL($3))
               ) AS "hidden!""#,
            scope.owner.as_uuid(),
            id.as_uuid(),
            filter.as_deref(),
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(hidden)
    }
}

/// Bumps the version once, records the actor, and pauses the post's recurrence.
async fn bump_version(conn: &mut PgConnection, locked: &LockedPost) -> Result<(), DataError> {
    sqlx::query!(
        "UPDATE posts SET version = version + 1, updated_at = now(), updated_by = $3
         WHERE owner_id = $1 AND id = $2",
        locked.scope.owner.as_uuid(),
        locked.post.id.as_uuid(),
        locked.scope.actor.as_uuid(),
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!(
        "UPDATE recurrences
         SET paused = true, paused_reason = 'post_edited', version = version + 1,
             updated_at = now(), updated_by = $2
         WHERE post_id = $1 AND NOT paused",
        locked.post.id.as_uuid(),
        locked.scope.actor.as_uuid(),
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Whether the post already targets `account`, so an insert that skipped a duplicate is fine.
async fn target_exists(
    conn: &mut PgConnection,
    locked: &LockedPost,
    account: AccountId,
) -> Result<bool, DataError> {
    Ok(sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM post_targets
               WHERE owner_id = $1 AND post_id = $2 AND account_id = $3
           ) AS "present!""#,
        locked.scope.owner.as_uuid(),
        locked.post.id.as_uuid(),
        account.as_uuid(),
    )
    .fetch_one(&mut *conn)
    .await?)
}

/// A foreign-key violation means a referenced row is missing or belongs to another owner.
fn reference_missing(err: sqlx::Error) -> DataError {
    if is_foreign_key_violation(&err) {
        DataError::NotFound
    } else {
        translate(err)
    }
}
