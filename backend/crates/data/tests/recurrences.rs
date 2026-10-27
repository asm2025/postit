#![allow(clippy::unwrap_used)]

use chrono::{Duration, SubsecRound, Utc};
use postit_core::{AccountId, PostId, PostKind, RecurrenceId, UserId};
use postit_data::posts::{LockedPost, NewPost, PostContent, PostRepo};
use postit_data::recurrences::{NewRecurrence, PauseReason, RecurrenceRepo};
use postit_data::testkit::{callers, seed_user};
use postit_data::{DataError, OwnerScope, ScopeError};
use sqlx::PgPool;
use uuid::Uuid;

async fn account(pool: &PgPool, owner: UserId, remote: &str) -> AccountId {
    sqlx::query(
        "INSERT INTO platforms (id, display_name) VALUES ('mastodon', 'Mastodon') \
         ON CONFLICT DO NOTHING",
    )
    .execute(pool)
    .await
    .unwrap();
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO connected_accounts (id, owner_id, platform_id, remote_account_id, display_name) \
         VALUES ($1, $2, 'mastodon', $3, $3)",
    )
    .bind(id)
    .bind(owner.as_uuid())
    .bind(remote)
    .execute(pool)
    .await
    .unwrap();
    AccountId::from_uuid(id)
}

async fn post(pool: &PgPool, scope: &OwnerScope) -> PostId {
    let mut c = pool.acquire().await.unwrap();
    PostRepo::create(
        &mut c,
        scope,
        NewPost {
            id: PostId::from_uuid(Uuid::now_v7()),
            kind: PostKind::Post,
            reply_to: None,
            campaign_id: None,
            text: "weekly".to_string(),
            link: None,
            tags: vec![],
            disclose_ai_generated: false,
        },
    )
    .await
    .unwrap()
    .id
}

async fn lock_in(
    pool: &PgPool,
    scope: &OwnerScope,
    id: PostId,
) -> (sqlx::Transaction<'static, sqlx::Postgres>, LockedPost) {
    let mut tx = pool.begin().await.unwrap();
    let lock = PostRepo::lock(&mut tx, scope, id, None).await.unwrap();
    (tx, lock)
}

fn recurrence(targets: &[AccountId]) -> NewRecurrence {
    NewRecurrence {
        id: RecurrenceId::from_uuid(Uuid::now_v7()),
        rule: "FREQ=WEEKLY;BYDAY=MO".to_string(),
        timezone: "Asia/Riyadh".to_string(),
        targets: targets.to_vec(),
        rewrite: false,
        next_occurrence_at: Utc::now() + Duration::days(1),
        confirmed_version: 2,
    }
}

async fn version(pool: &PgPool, scope: &OwnerScope, id: PostId) -> i64 {
    let mut c = pool.acquire().await.unwrap();
    PostRepo::get(&mut c, scope, id).await.unwrap().version
}

#[sqlx::test]
async fn set_requires_publish(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let who = callers(&pool, owner, &[]).await;
    let id = post(&pool, &who.owner).await;

    let (mut tx, lock) = lock_in(&pool, &who.edit, id).await;
    assert!(matches!(
        RecurrenceRepo::set(&mut tx, &lock, recurrence(&[a])).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    assert!(matches!(
        RecurrenceRepo::pause(&mut tx, &lock, PauseReason::User).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    assert!(matches!(
        RecurrenceRepo::delete(&mut tx, &lock).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
}

#[sqlx::test]
async fn set_bumps_post_version_and_is_active(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let b = account(&pool, owner, "b").await;
    let who = callers(&pool, owner, &[]).await;
    let id = post(&pool, &who.owner).await;
    assert_eq!(version(&pool, &who.owner, id).await, 1);

    let (mut tx, lock) = lock_in(&pool, &who.publish, id).await;
    let set = RecurrenceRepo::set(&mut tx, &lock, recurrence(&[a, b, a]))
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(version(&pool, &who.owner, id).await, 2);
    assert_eq!(set.paused, None);
    assert_eq!(set.targets.len(), 2);
    assert_eq!(set.confirmed_by, Some(who.publish.actor));
    assert!(set.confirmed_at.is_some());
    assert_eq!(set.confirmed_version, Some(2));
    assert_eq!(set.rule, "FREQ=WEEKLY;BYDAY=MO");
    let mut c = pool.acquire().await.unwrap();
    let read = RecurrenceRepo::get(&mut c, &who.view, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(read, set);
}

#[sqlx::test]
async fn set_replaces_an_earlier_recurrence(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let b = account(&pool, owner, "b").await;
    let who = callers(&pool, owner, &[]).await;
    let id = post(&pool, &who.owner).await;
    let (mut tx, lock) = lock_in(&pool, &who.owner, id).await;
    let first = RecurrenceRepo::set(&mut tx, &lock, recurrence(&[a]))
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let (mut tx, lock) = lock_in(&pool, &who.owner, id).await;
    let second = RecurrenceRepo::set(&mut tx, &lock, recurrence(&[b]))
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert_ne!(first.id, second.id);
    assert_eq!(second.targets, vec![b]);
    assert_eq!(second.paused, None);
    assert_eq!(version(&pool, &who.owner, id).await, 3);
    let (rows,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM recurrences")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 1);
}

#[sqlx::test]
async fn later_post_edit_pauses_it(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let who = callers(&pool, owner, &[]).await;
    let id = post(&pool, &who.owner).await;
    let (mut tx, lock) = lock_in(&pool, &who.owner, id).await;
    RecurrenceRepo::set(&mut tx, &lock, recurrence(&[a]))
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let (mut tx, lock) = lock_in(&pool, &who.edit, id).await;
    PostRepo::update_content(
        &mut tx,
        &lock,
        PostContent {
            reply_to: None,
            campaign_id: None,
            text: "changed".to_string(),
            link: None,
            tags: vec![],
            disclose_ai_generated: false,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let mut c = pool.acquire().await.unwrap();
    let paused = RecurrenceRepo::get(&mut c, &who.owner, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(paused.paused, Some(PauseReason::PostEdited));
}

#[sqlx::test]
async fn resume_unpauses_and_records_confirmation(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let who = callers(&pool, owner, &[]).await;
    let id = post(&pool, &who.owner).await;
    let (mut tx, lock) = lock_in(&pool, &who.owner, id).await;
    RecurrenceRepo::set(&mut tx, &lock, recurrence(&[a]))
        .await
        .unwrap();
    RecurrenceRepo::pause(&mut tx, &lock, PauseReason::User)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let before = version(&pool, &who.owner, id).await;

    let next = (Utc::now() + Duration::days(7)).trunc_subsecs(0);
    let (mut tx, lock) = lock_in(&pool, &who.publish, id).await;
    let resumed = RecurrenceRepo::resume(&mut tx, &lock, next, 3)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(resumed.paused, None);
    assert_eq!(resumed.next_occurrence_at, Some(next));
    assert_eq!(resumed.confirmed_version, Some(3));
    assert_eq!(resumed.confirmed_by, Some(who.publish.actor));
    assert_eq!(version(&pool, &who.owner, id).await, before + 1);
}

#[sqlx::test]
async fn pause_records_the_reason_and_keeps_the_post_version(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let who = callers(&pool, owner, &[]).await;
    let id = post(&pool, &who.owner).await;
    let (mut tx, lock) = lock_in(&pool, &who.owner, id).await;
    RecurrenceRepo::set(&mut tx, &lock, recurrence(&[a]))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let before = version(&pool, &who.owner, id).await;

    let (mut tx, lock) = lock_in(&pool, &who.publish, id).await;
    let paused = RecurrenceRepo::pause(&mut tx, &lock, PauseReason::NoTargets)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(paused.paused, Some(PauseReason::NoTargets));
    assert_eq!(version(&pool, &who.owner, id).await, before);
}

#[sqlx::test]
async fn delete_removes_it_and_bumps_the_post(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let who = callers(&pool, owner, &[]).await;
    let id = post(&pool, &who.owner).await;
    let (mut tx, lock) = lock_in(&pool, &who.owner, id).await;
    RecurrenceRepo::set(&mut tx, &lock, recurrence(&[a]))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let before = version(&pool, &who.owner, id).await;

    let (mut tx, lock) = lock_in(&pool, &who.publish, id).await;
    RecurrenceRepo::delete(&mut tx, &lock).await.unwrap();
    tx.commit().await.unwrap();

    let mut c = pool.acquire().await.unwrap();
    assert!(
        RecurrenceRepo::get(&mut c, &who.owner, id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(version(&pool, &who.owner, id).await, before + 1);

    let (mut tx, lock) = lock_in(&pool, &who.publish, id).await;
    assert!(matches!(
        RecurrenceRepo::delete(&mut tx, &lock).await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        RecurrenceRepo::pause(&mut tx, &lock, PauseReason::User).await,
        Err(DataError::NotFound)
    ));
}

#[sqlx::test]
async fn targets_from_other_owner_rejected(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let other = seed_user(&pool, "active").await;
    let theirs = account(&pool, other, "theirs").await;
    let who = callers(&pool, owner, &[]).await;
    let id = post(&pool, &who.owner).await;
    let before = version(&pool, &who.owner, id).await;

    let (mut tx, lock) = lock_in(&pool, &who.owner, id).await;
    assert!(matches!(
        RecurrenceRepo::set(&mut tx, &lock, recurrence(&[theirs])).await,
        Err(DataError::NotFound)
    ));
    drop(tx);

    let mut c = pool.acquire().await.unwrap();
    assert!(
        RecurrenceRepo::get(&mut c, &who.owner, id)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(version(&pool, &who.owner, id).await, before);
}

#[sqlx::test]
async fn limited_delegate_sees_only_in_limit_targets(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let b = account(&pool, owner, "b").await;
    let who = callers(&pool, owner, &[a]).await;
    let id = post(&pool, &who.owner).await;
    let (mut tx, lock) = lock_in(&pool, &who.owner, id).await;
    RecurrenceRepo::set(&mut tx, &lock, recurrence(&[a, b]))
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let mut c = pool.acquire().await.unwrap();
    let seen = RecurrenceRepo::get(&mut c, &who.limited, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(seen.targets, vec![a]);
    let full = RecurrenceRepo::get(&mut c, &who.owner, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(full.targets.len(), 2);

    assert!(matches!(
        RecurrenceRepo::get(&mut c, &who.stranger, id).await,
        Err(DataError::NotFound)
    ));
}

#[sqlx::test]
async fn limited_delegate_cannot_replace_hidden_targets(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let b = account(&pool, owner, "b").await;
    let who = callers(&pool, owner, &[a]).await;
    let id = post(&pool, &who.owner).await;
    let (mut tx, lock) = lock_in(&pool, &who.owner, id).await;
    RecurrenceRepo::set(&mut tx, &lock, recurrence(&[a, b]))
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let (mut tx, lock) = lock_in(&pool, &who.limited, id).await;
    assert!(matches!(
        RecurrenceRepo::set(&mut tx, &lock, recurrence(&[b])).await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        RecurrenceRepo::set(&mut tx, &lock, recurrence(&[a])).await,
        Err(DataError::AccountNotInScope)
    ));
    drop(tx);

    let mut c = pool.acquire().await.unwrap();
    let kept = RecurrenceRepo::get(&mut c, &who.owner, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(kept.targets.len(), 2);
}

#[sqlx::test]
async fn limited_delegate_cannot_pause_resume_or_delete_hidden_targets(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let b = account(&pool, owner, "b").await;
    let who = callers(&pool, owner, &[a]).await;
    let id = post(&pool, &who.owner).await;
    let (mut tx, lock) = lock_in(&pool, &who.owner, id).await;
    RecurrenceRepo::set(&mut tx, &lock, recurrence(&[a, b]))
        .await
        .unwrap();
    RecurrenceRepo::pause(&mut tx, &lock, PauseReason::User)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let before = version(&pool, &who.owner, id).await;

    let (mut tx, lock) = lock_in(&pool, &who.limited, id).await;
    let outside =
        |result: Result<_, DataError>| matches!(result, Err(DataError::AccountNotInScope));
    assert!(outside(
        RecurrenceRepo::pause(&mut tx, &lock, PauseReason::User)
            .await
            .map(|_| ())
    ));
    assert!(outside(
        RecurrenceRepo::resume(&mut tx, &lock, Utc::now() + Duration::days(1), 3)
            .await
            .map(|_| ())
    ));
    assert!(outside(RecurrenceRepo::delete(&mut tx, &lock).await));
    drop(tx);

    let mut c = pool.acquire().await.unwrap();
    let kept = RecurrenceRepo::get(&mut c, &who.owner, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(kept.paused, Some(PauseReason::User));
    assert_eq!(kept.targets.len(), 2);
    assert_eq!(version(&pool, &who.owner, id).await, before);
}
