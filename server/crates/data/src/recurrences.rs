//! A post's recurrence: the rule that makes the post repeat, and who confirmed it.
//!
//! Writes take the [`LockedPost`] and need [`Capability::Publish`], because a recurrence
//! publishes on its own later. Reads take the caller's [`OwnerScope`] and need
//! [`Capability::View`]. A recurrence's targets are filtered by the scope's account limit.
//!
//! Every write bumps the post's version through `bump_version`, which also pauses an active
//! recurrence. `set` and `resume` therefore bump first and write the unpaused row last, so the
//! recurrence they leave behind is the one the caller confirmed. The tick queries live in the
//! publishing module.

use chrono::{DateTime, Utc};
use postit_core::{AccountId, PostId, RecurrenceId, UserId};
use serde_json::Value;
use sqlx::{Connection, PgConnection};
use uuid::Uuid;

use crate::constraint::translate;
use crate::error::DataError;
use crate::posts::{LockedPost, PostRepo, bump_version};
use crate::text::{parse, text_enum};
use crate::{Capability, OwnerScope};

text_enum! {
    /// Why a recurrence is not running.
    PauseReason {
        /// The post changed after the recurrence was confirmed.
        PostEdited => "post_edited",
        /// Every target was removed or disconnected.
        NoTargets => "no_targets",
        /// The owner or a delegate paused it.
        User => "user",
        /// The rule has no occurrences left.
        Ended => "ended",
    }
}

/// A recurrence with the targets the caller's limit reaches.
#[derive(Debug, Clone, PartialEq)]
pub struct Recurrence {
    /// The recurrence.
    pub id: RecurrenceId,
    /// The post it repeats.
    pub post_id: PostId,
    /// The recurrence rule, as the scheduler reads it.
    pub rule: String,
    /// The IANA time zone the rule runs in.
    pub timezone: String,
    /// Whether each occurrence is rewritten by the drafter before it goes out.
    pub rewrite: bool,
    /// When the next occurrence is due, if it is running.
    pub next_occurrence_at: Option<DateTime<Utc>>,
    /// How many occurrences have been created.
    pub occurrences_created: i32,
    /// Why it is paused, or `None` while it runs.
    pub paused: Option<PauseReason>,
    /// The post version the confirmation covered.
    pub confirmed_version: Option<i32>,
    /// Who confirmed it.
    pub confirmed_by: Option<UserId>,
    /// When it was confirmed.
    pub confirmed_at: Option<DateTime<Utc>>,
    /// Occurrences the owner skipped.
    pub skipped: Value,
    /// The recurrence's own version.
    pub version: i32,
    /// The accounts each occurrence goes to, inside the caller's limit.
    pub targets: Vec<AccountId>,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

/// A recurrence to set on a post.
#[derive(Debug, Clone)]
pub struct NewRecurrence {
    /// The recurrence's ID.
    pub id: RecurrenceId,
    /// The recurrence rule.
    pub rule: String,
    /// The IANA time zone the rule runs in.
    pub timezone: String,
    /// The accounts each occurrence goes to.
    pub targets: Vec<AccountId>,
    /// Whether each occurrence is rewritten by the drafter.
    pub rewrite: bool,
    /// When the first occurrence is due.
    pub next_occurrence_at: DateTime<Utc>,
    /// The post version the caller confirmed.
    pub confirmed_version: i32,
}

struct RecurrenceRow {
    id: Uuid,
    post_id: Uuid,
    rule: String,
    timezone: String,
    rewrite: bool,
    next_occurrence_at: Option<DateTime<Utc>>,
    occurrences_created: i32,
    paused_reason: Option<String>,
    confirmed_version: Option<i32>,
    confirmed_by: Option<Uuid>,
    confirmed_at: Option<DateTime<Utc>>,
    skipped: Value,
    version: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl RecurrenceRow {
    fn with(self, targets: Vec<AccountId>) -> Result<Recurrence, DataError> {
        Ok(Recurrence {
            id: RecurrenceId::from_uuid(self.id),
            post_id: PostId::from_uuid(self.post_id),
            rule: self.rule,
            timezone: self.timezone,
            rewrite: self.rewrite,
            next_occurrence_at: self.next_occurrence_at,
            occurrences_created: self.occurrences_created,
            paused: self.paused_reason.as_deref().map(parse).transpose()?,
            confirmed_version: self.confirmed_version,
            confirmed_by: self.confirmed_by.map(UserId::from_uuid),
            confirmed_at: self.confirmed_at,
            skipped: self.skipped,
            version: self.version,
            targets,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

/// Recurrences.
pub struct RecurrenceRepo;

impl RecurrenceRepo {
    /// Sets the post's recurrence, replacing any earlier one, and records the actor as the
    /// confirmer. The post's version goes up once and the new recurrence runs.
    ///
    /// # Errors
    ///
    /// - [`DataError::NotFound`] when a target is outside the caller's limit, not the
    ///   owner's, or disconnected.
    /// - [`DataError::Conflict`] (`recurrence_outside_limit`) when the caller has an account
    ///   limit and the recurrence being replaced has targets outside it.
    pub async fn set(
        conn: &mut PgConnection,
        locked: &LockedPost,
        new: NewRecurrence,
    ) -> Result<Recurrence, DataError> {
        locked.scope.require(Capability::Publish)?;
        let targets = reachable(&locked.scope, &new.targets)?;
        let mut tx = conn.begin().await?;
        if let Some(filter) = locked.scope.account_filter() {
            let hidden = sqlx::query_scalar!(
                r#"SELECT EXISTS (
                       SELECT 1 FROM recurrence_targets t
                       JOIN recurrences r ON r.owner_id = t.owner_id AND r.id = t.recurrence_id
                       WHERE r.owner_id = $1 AND r.post_id = $2 AND t.account_id <> ALL($3)
                   ) AS "hidden!""#,
                locked.scope.owner.as_uuid(),
                locked.post().id.as_uuid(),
                &filter,
            )
            .fetch_one(&mut *tx)
            .await?;
            if hidden {
                return Err(DataError::Conflict("recurrence_outside_limit".to_string()));
            }
        }
        sqlx::query!(
            "DELETE FROM recurrences WHERE owner_id = $1 AND post_id = $2",
            locked.scope.owner.as_uuid(),
            locked.post().id.as_uuid(),
        )
        .execute(&mut *tx)
        .await?;
        bump_version(&mut tx, locked).await?;
        sqlx::query!(
            "INSERT INTO recurrences
                 (id, owner_id, post_id, rule, timezone, rewrite, next_occurrence_at,
                  confirmed_version, confirmed_by, confirmed_at, created_by, updated_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, now(), $9, $9)",
            new.id.as_uuid(),
            locked.scope.owner.as_uuid(),
            locked.post().id.as_uuid(),
            new.rule,
            new.timezone,
            new.rewrite,
            new.next_occurrence_at,
            new.confirmed_version,
            locked.scope.actor.as_uuid(),
        )
        .execute(&mut *tx)
        .await
        .map_err(translate)?;
        for account in &targets {
            let inserted = sqlx::query!(
                "INSERT INTO recurrence_targets (recurrence_id, account_id, owner_id)
                 SELECT $2, id, owner_id FROM connected_accounts
                 WHERE owner_id = $1 AND id = $3 AND status <> 'disconnected'",
                locked.scope.owner.as_uuid(),
                new.id.as_uuid(),
                account,
            )
            .execute(&mut *tx)
            .await?
            .rows_affected();
            if inserted == 0 {
                return Err(DataError::NotFound);
            }
        }
        let recurrence = load(&mut tx, &locked.scope, locked.post().id)
            .await?
            .ok_or(DataError::NotFound)?;
        tx.commit().await?;
        Ok(recurrence)
    }

    /// The post's recurrence, if it has one.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such post.
    pub async fn get(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        post: PostId,
    ) -> Result<Option<Recurrence>, DataError> {
        PostRepo::get(conn, scope, post).await?;
        load(conn, scope, post).await
    }

    /// Pauses the post's recurrence. The post's version stays.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the post has no recurrence.
    pub async fn pause(
        conn: &mut PgConnection,
        locked: &LockedPost,
        reason: PauseReason,
    ) -> Result<Recurrence, DataError> {
        locked.scope.require(Capability::Publish)?;
        let paused = sqlx::query!(
            "UPDATE recurrences
             SET paused = true, paused_reason = $3, version = version + 1,
                 updated_at = now(), updated_by = $4
             WHERE owner_id = $1 AND post_id = $2",
            locked.scope.owner.as_uuid(),
            locked.post().id.as_uuid(),
            reason.as_str(),
            locked.scope.actor.as_uuid(),
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();
        if paused == 0 {
            return Err(DataError::NotFound);
        }
        load(conn, &locked.scope, locked.post().id)
            .await?
            .ok_or(DataError::NotFound)
    }

    /// Resumes a paused recurrence and records the actor as the confirmer. The post's version
    /// goes up once.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the post has no recurrence.
    pub async fn resume(
        conn: &mut PgConnection,
        locked: &LockedPost,
        next_occurrence_at: DateTime<Utc>,
        confirmed_version: i32,
    ) -> Result<Recurrence, DataError> {
        locked.scope.require(Capability::Publish)?;
        let mut tx = conn.begin().await?;
        ensure_exists(&mut tx, locked).await?;
        bump_version(&mut tx, locked).await?;
        sqlx::query!(
            "UPDATE recurrences
             SET paused = false, paused_reason = NULL, next_occurrence_at = $3,
                 confirmed_version = $4, confirmed_by = $5, confirmed_at = now(),
                 version = version + 1, updated_at = now(), updated_by = $5
             WHERE owner_id = $1 AND post_id = $2",
            locked.scope.owner.as_uuid(),
            locked.post().id.as_uuid(),
            next_occurrence_at,
            confirmed_version,
            locked.scope.actor.as_uuid(),
        )
        .execute(&mut *tx)
        .await?;
        let recurrence = load(&mut tx, &locked.scope, locked.post().id)
            .await?
            .ok_or(DataError::NotFound)?;
        tx.commit().await?;
        Ok(recurrence)
    }

    /// Deletes the post's recurrence. The post's version goes up once.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the post has no recurrence.
    pub async fn delete(conn: &mut PgConnection, locked: &LockedPost) -> Result<(), DataError> {
        locked.scope.require(Capability::Publish)?;
        let mut tx = conn.begin().await?;
        let deleted = sqlx::query!(
            "DELETE FROM recurrences WHERE owner_id = $1 AND post_id = $2",
            locked.scope.owner.as_uuid(),
            locked.post().id.as_uuid(),
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
}

/// `accounts` as distinct UUIDs, or `NotFound` when one is outside the scope's limit.
fn reachable(scope: &OwnerScope, accounts: &[AccountId]) -> Result<Vec<Uuid>, DataError> {
    if accounts
        .iter()
        .any(|account| !scope.allows_account(*account))
    {
        return Err(DataError::NotFound);
    }
    let mut ids: Vec<Uuid> = accounts.iter().map(AccountId::as_uuid).collect();
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

async fn ensure_exists(conn: &mut PgConnection, locked: &LockedPost) -> Result<(), DataError> {
    let present = sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM recurrences WHERE owner_id = $1 AND post_id = $2
           ) AS "present!""#,
        locked.scope.owner.as_uuid(),
        locked.post().id.as_uuid(),
    )
    .fetch_one(&mut *conn)
    .await?;
    if present {
        Ok(())
    } else {
        Err(DataError::NotFound)
    }
}

async fn load(
    conn: &mut PgConnection,
    scope: &OwnerScope,
    post: PostId,
) -> Result<Option<Recurrence>, DataError> {
    let Some(row) = sqlx::query_as!(
        RecurrenceRow,
        "SELECT id, post_id, rule, timezone, rewrite, next_occurrence_at, occurrences_created,
                paused_reason, confirmed_version, confirmed_by, confirmed_at, skipped, version,
                created_at, updated_at
         FROM recurrences WHERE owner_id = $1 AND post_id = $2",
        scope.owner.as_uuid(),
        post.as_uuid(),
    )
    .fetch_optional(&mut *conn)
    .await?
    else {
        return Ok(None);
    };
    let filter = scope.account_filter();
    let targets = sqlx::query_scalar!(
        "SELECT account_id FROM recurrence_targets
         WHERE owner_id = $1 AND recurrence_id = $2
           AND ($3::uuid[] IS NULL OR account_id = ANY($3))
         ORDER BY created_at, account_id",
        scope.owner.as_uuid(),
        row.id,
        filter.as_deref(),
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(AccountId::from_uuid)
    .collect();
    row.with(targets).map(Some)
}
