//! The post service: create, edit, delete, overrides, and the target check `create_job` shares
//! with the validate routes.
#![allow(clippy::unwrap_used, clippy::too_many_lines)]

mod common;

use common::{Fixture, fixture, run};
use postit_core::{AiGenerationId, PostId, PostKind, PostVersion, ViolationCode};
use postit_data::audit::AuditContext;

use postit_data::ScopeError;
use postit_data::testkit::callers;
use postit_publishing::{PostDraft, PostPatch, PublishInput, PublishingError};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

fn draft(text: &str, targets: &[postit_core::AccountId]) -> PostDraft {
    PostDraft {
        kind: PostKind::Post,
        reply_to: None,
        campaign_id: None,
        text: text.to_owned(),
        link: None,
        tags: vec![],
        disclose_ai_generated: false,
        media: vec![],
        targets: targets.to_vec(),
    }
}

async fn generation(pool: &PgPool, owner: postit_core::UserId) -> AiGenerationId {
    let id = AiGenerationId::from(Uuid::now_v7());
    sqlx::query(
        "INSERT INTO ai_generations (id, owner_id, model, mode, prompt_hash, outcome) \
         VALUES ($1, $2, 'm', 'post', '\\x00', 'ok')",
    )
    .bind(id.as_uuid())
    .bind(owner.as_uuid())
    .execute(pool)
    .await
    .unwrap();
    id
}

fn publish_now(targets: &[postit_core::AccountId], version: i64, dry_run: bool) -> PublishInput {
    PublishInput {
        targets: targets.to_vec(),
        at: None,
        native_schedule: false,
        dry_run,
        confirm: !dry_run,
        if_match: PostVersion(version),
    }
}

/// A post published live to the fixture's account, with its job ended.
async fn published_post(f: &Fixture, dry_run: bool) -> PostId {
    let posts = f.service.posts();
    let view = posts
        .create(&f.env.scope, draft("hello", &[f.account]), None)
        .await
        .unwrap();
    let id = view.post.id;
    let created = f
        .service
        .create_job(
            &f.env.scope,
            id,
            publish_now(&[f.account], view.post.version, dry_run),
            AuditContext::default(),
        )
        .await
        .unwrap();
    for dispatch in created.deliveries {
        run(&f.service, dispatch).await;
    }
    id
}

#[sqlx::test(migrations = "../data/migrations")]
async fn create_links_generation(pool: PgPool) {
    let f = fixture(&pool).await;
    let generation = generation(&pool, f.env.owner).await;
    let view = f
        .service
        .posts()
        .create(&f.env.scope, draft("hi", &[f.account]), Some(generation))
        .await
        .unwrap();
    let linked: Option<Uuid> =
        sqlx::query_scalar("SELECT post_id FROM ai_generations WHERE id = $1")
            .bind(generation.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(linked, Some(view.post.id.as_uuid()));
    assert_eq!(view.targets, vec![f.account]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn create_with_linked_generation_is_not_found(pool: PgPool) {
    let f = fixture(&pool).await;
    let generation = generation(&pool, f.env.owner).await;
    let posts = f.service.posts();
    posts
        .create(&f.env.scope, draft("one", &[]), Some(generation))
        .await
        .unwrap();
    let again = posts
        .create(&f.env.scope, draft("two", &[]), Some(generation))
        .await;
    assert!(matches!(again, Err(PublishingError::NotFound)));
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM posts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1, "the refused create left no post behind");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn update_bumps_version_and_refuses_stale(pool: PgPool) {
    let f = fixture(&pool).await;
    let posts = f.service.posts();
    let view = posts
        .create(&f.env.scope, draft("hello", &[f.account]), None)
        .await
        .unwrap();
    let v1 = view.post.version;
    let updated = posts
        .update(
            &f.env.scope,
            view.post.id,
            PostVersion(v1),
            PostPatch {
                text: Some("changed".to_owned()),
                ..PostPatch::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.post.text, "changed");
    assert!(updated.post.version > v1);
    let stale = posts
        .update(
            &f.env.scope,
            view.post.id,
            PostVersion(v1),
            PostPatch::default(),
        )
        .await;
    assert!(matches!(stale, Err(PublishingError::VersionConflict)));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn one_update_is_one_version_however_many_parts_change(pool: PgPool) {
    let f = fixture(&pool).await;
    let posts = f.service.posts();
    let view = posts
        .create(&f.env.scope, draft("hello", &[f.account]), None)
        .await
        .unwrap();
    let v1 = view.post.version;
    let updated = posts
        .update(
            &f.env.scope,
            view.post.id,
            PostVersion(v1),
            PostPatch {
                text: Some("changed".to_owned()),
                media: Some(Vec::new()),
                targets: Some(vec![f.account]),
                ..PostPatch::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.post.version, v1 + 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delete_with_live_history_needs_publish(pool: PgPool) {
    let f = fixture(&pool).await;
    let who = callers(&pool, f.env.owner, &[]).await;
    let id = published_post(&f, false).await;
    let posts = f.service.posts();
    let version = posts.get(&f.env.scope, id).await.unwrap().post.version;
    let edit = posts.delete(&who.edit, id, PostVersion(version)).await;
    assert!(matches!(
        edit,
        Err(PublishingError::Scope(ScopeError::InsufficientAccess))
    ));
    posts
        .delete(&who.publish, id, PostVersion(version))
        .await
        .unwrap();
    assert!(matches!(
        posts.get(&f.env.scope, id).await,
        Err(PublishingError::NotFound)
    ));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delete_dry_run_only_history_needs_edit(pool: PgPool) {
    let f = fixture(&pool).await;
    let who = callers(&pool, f.env.owner, &[]).await;
    let id = published_post(&f, true).await;
    let posts = f.service.posts();
    let version = posts.get(&f.env.scope, id).await.unwrap().post.version;
    posts
        .delete(&who.edit, id, PostVersion(version))
        .await
        .unwrap();
}

#[sqlx::test(migrations = "../data/migrations")]
async fn put_override_bumps_version(pool: PgPool) {
    let f = fixture(&pool).await;
    let posts = f.service.posts();
    let view = posts
        .create(&f.env.scope, draft("hello", &[f.account]), None)
        .await
        .unwrap();
    let before = view.post.version;
    let after = posts
        .put_override(
            &f.env.scope,
            view.post.id,
            f.account,
            PostVersion(before),
            json!({"text": "shorter"}),
            json!({}),
        )
        .await
        .unwrap();
    assert!(after.post.version > before);
    assert_eq!(after.overrides.len(), 1);
    let removed = posts
        .delete_override(
            &f.env.scope,
            view.post.id,
            f.account,
            PostVersion(after.post.version),
        )
        .await
        .unwrap();
    assert!(removed.overrides.is_empty());
    assert!(removed.post.version > after.post.version);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn last_published_reports_newest_live_delivery(pool: PgPool) {
    let f = fixture(&pool).await;
    let id = published_post(&f, false).await;
    let view = f.service.posts().get(&f.env.scope, id).await.unwrap();
    assert_eq!(view.last_published.len(), 1);
    assert_eq!(view.last_published[0].account_id, f.account);
    assert_eq!(view.last_published[0].post_version, view.post.version);
    // A dry run is not live history.
    let dry = published_post(&f, true).await;
    let dry_view = f.service.posts().get(&f.env.scope, dry).await.unwrap();
    assert!(dry_view.last_published.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn create_job_still_flattens_violations(pool: PgPool) {
    let f = fixture(&pool).await;
    let posts = f.service.posts();
    let view = posts
        .create(&f.env.scope, draft("", &[f.account]), None)
        .await
        .unwrap();
    let result = f
        .service
        .create_job(
            &f.env.scope,
            view.post.id,
            publish_now(&[f.account], view.post.version, true),
            AuditContext::default(),
        )
        .await;
    let Err(PublishingError::ValidationFailed(violations)) = result else {
        unreachable!("expected violations");
    };
    assert!(
        violations
            .iter()
            .any(|v| v.code == ViolationCode::TextRequired
                && v.params["account_id"] == f.account.to_string())
    );
}
