#![allow(clippy::unwrap_used)]

use postit_core::{AccountId, UserId, WritingStyleId};
use postit_data::testkit::{callers, seed_user};
use postit_data::writing_styles::{EmojiUse, HashtagUse, WritingStyleFields, WritingStyleRepo};
use postit_data::{DataError, ScopeError};
use sqlx::PgPool;
use uuid::Uuid;

fn id() -> WritingStyleId {
    WritingStyleId::from_uuid(Uuid::now_v7())
}

fn style(tone: &str) -> WritingStyleFields {
    WritingStyleFields {
        tone: tone.to_string(),
        emoji: EmojiUse::Light,
        hashtags: HashtagUse::Few,
        language: Some("ar".to_string()),
        notes: String::new(),
    }
}

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

#[sqlx::test]
async fn writing_style_isolation_matrix(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let who = callers(&pool, owner, &[a]).await;
    let mut c = pool.acquire().await.unwrap();

    assert!(matches!(
        WritingStyleRepo::upsert(&mut c, &who.view, id(), a, style("warm")).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    let written = WritingStyleRepo::upsert(&mut c, &who.edit, id(), a, style("warm"))
        .await
        .unwrap();
    assert_eq!(written.account_id, a);
    assert_eq!(written.fields.emoji, EmojiUse::Light);

    for scope in [&who.owner, &who.view, &who.edit, &who.publish, &who.limited] {
        assert_eq!(
            WritingStyleRepo::get(&mut c, scope, a)
                .await
                .unwrap()
                .fields
                .tone,
            "warm"
        );
        assert_eq!(
            WritingStyleRepo::list(&mut c, scope).await.unwrap().len(),
            1
        );
    }
    assert!(matches!(
        WritingStyleRepo::get(&mut c, &who.stranger, a).await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        WritingStyleRepo::upsert(&mut c, &who.stranger, id(), a, style("x")).await,
        Err(DataError::NotFound)
    ));
    assert!(
        WritingStyleRepo::list(&mut c, &who.stranger)
            .await
            .unwrap()
            .is_empty()
    );
}

#[sqlx::test]
async fn writing_style_outside_limit_is_not_found(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let b = account(&pool, owner, "b").await;
    let who = callers(&pool, owner, &[a]).await;
    let mut c = pool.acquire().await.unwrap();
    WritingStyleRepo::upsert(&mut c, &who.owner, id(), a, style("a"))
        .await
        .unwrap();
    WritingStyleRepo::upsert(&mut c, &who.owner, id(), b, style("b"))
        .await
        .unwrap();

    assert!(matches!(
        WritingStyleRepo::get(&mut c, &who.limited, b).await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        WritingStyleRepo::upsert(&mut c, &who.limited, id(), b, style("x")).await,
        Err(DataError::NotFound)
    ));
    let seen = WritingStyleRepo::list(&mut c, &who.limited).await.unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].account_id, a);
    assert_eq!(
        WritingStyleRepo::get(&mut c, &who.owner, b)
            .await
            .unwrap()
            .fields
            .tone,
        "b"
    );
}

#[sqlx::test]
async fn upsert_replaces_and_keeps_the_first_id(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let a = account(&pool, owner, "a").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let first = WritingStyleRepo::upsert(&mut c, &who.owner, id(), a, style("one"))
        .await
        .unwrap();

    let mut next = style("two");
    next.emoji = EmojiUse::None;
    next.hashtags = HashtagUse::Many;
    next.language = None;
    let second = WritingStyleRepo::upsert(&mut c, &who.edit, id(), a, next)
        .await
        .unwrap();

    assert_eq!(second.id, first.id);
    assert_eq!(second.fields.tone, "two");
    assert_eq!(second.fields.emoji, EmojiUse::None);
    assert_eq!(second.fields.hashtags, HashtagUse::Many);
    assert_eq!(second.fields.language, None);
    assert_eq!(
        WritingStyleRepo::list(&mut c, &who.owner)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[sqlx::test]
async fn another_owners_account_is_not_found(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let other = seed_user(&pool, "active").await;
    let theirs = account(&pool, other, "theirs").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();

    assert!(matches!(
        WritingStyleRepo::upsert(&mut c, &who.owner, id(), theirs, style("x")).await,
        Err(DataError::NotFound)
    ));
}
