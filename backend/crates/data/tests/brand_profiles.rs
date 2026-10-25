#![allow(clippy::unwrap_used)]

use postit_core::BrandProfileId;
use postit_data::brand_profiles::{BrandProfileFields, BrandProfileRepo, MAX_EXAMPLES};
use postit_data::testkit::{callers, seed_user};
use postit_data::{DataError, ScopeError};
use sqlx::PgPool;
use uuid::Uuid;

fn id() -> BrandProfileId {
    BrandProfileId::from_uuid(Uuid::now_v7())
}

fn profile(description: &str, examples: usize) -> BrandProfileFields {
    BrandProfileFields {
        description: description.to_string(),
        audience: "home bakers".to_string(),
        offerings: "flour".to_string(),
        avoid_words: vec!["cheap".to_string()],
        default_language: Some("en".to_string()),
        examples: (0..examples).map(|n| format!("example {n}")).collect(),
    }
}

#[sqlx::test]
async fn brand_profile_isolation_matrix(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();

    assert!(
        BrandProfileRepo::get(&mut c, &who.owner)
            .await
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        BrandProfileRepo::upsert(&mut c, &who.view, id(), profile("x", 0)).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    let written = BrandProfileRepo::upsert(&mut c, &who.edit, id(), profile("Bakery", 2))
        .await
        .unwrap();
    assert_eq!(written.fields.examples.len(), 2);
    assert_eq!(written.fields.avoid_words, vec!["cheap".to_string()]);

    for scope in [&who.owner, &who.view, &who.edit, &who.publish, &who.limited] {
        assert_eq!(
            BrandProfileRepo::get(&mut c, scope)
                .await
                .unwrap()
                .unwrap()
                .fields
                .description,
            "Bakery"
        );
    }
    assert!(
        BrandProfileRepo::get(&mut c, &who.stranger)
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test]
async fn upsert_keeps_one_profile_per_owner(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let first = BrandProfileRepo::upsert(&mut c, &who.owner, id(), profile("one", 1))
        .await
        .unwrap();

    let second = BrandProfileRepo::upsert(&mut c, &who.edit, id(), profile("two", 0))
        .await
        .unwrap();

    assert_eq!(second.id, first.id);
    assert_eq!(second.fields.description, "two");
    assert!(second.fields.examples.is_empty());
    let (rows,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM brand_profiles")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 1);
}

#[sqlx::test]
async fn brand_profile_rejects_six_examples(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();

    let at_limit = BrandProfileRepo::upsert(&mut c, &who.owner, id(), profile("x", MAX_EXAMPLES))
        .await
        .unwrap();
    assert_eq!(at_limit.fields.examples.len(), MAX_EXAMPLES);
    assert!(matches!(
        BrandProfileRepo::upsert(&mut c, &who.owner, id(), profile("x", MAX_EXAMPLES + 1)).await,
        Err(DataError::Sql(_))
    ));
    let kept = BrandProfileRepo::get(&mut c, &who.owner)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(kept.fields.examples.len(), MAX_EXAMPLES);
}
