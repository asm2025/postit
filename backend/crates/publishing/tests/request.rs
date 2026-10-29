#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use common::{env, snapshot, upload, variant};
use postit_core::{AccountId, MediaAssetId};
use postit_publishing::{AssembleError, assemble};
use sqlx::PgPool;
use uuid::Uuid;

fn account() -> AccountId {
    AccountId::from_uuid(Uuid::now_v7())
}

#[sqlx::test(migrations = "../data/migrations")]
async fn assemble_resolves_assets_in_variant_order(pool: PgPool) {
    let env = env(&pool).await;
    let first = upload(&env, "a.png").await;
    let second = upload(&env, "b.png").await;
    let third = upload(&env, "c.png").await;
    let account = account();
    // Not the order the assets were uploaded in.
    let order = [third, first, second];
    let snap = snapshot(vec![variant(account, &order)]);

    let assembled = assemble(&snap, account, &env.media, env.owner)
        .await
        .unwrap();

    let ids: Vec<_> = assembled.assets.iter().map(|a| a.id).collect();
    assert_eq!(ids, order);
    assert_eq!(assembled.variant.target.account, account);
    assert!(assembled.assets.iter().all(|a| a.owner_id == env.owner));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn assemble_without_media_has_no_assets(pool: PgPool) {
    let env = env(&pool).await;
    let account = account();
    let snap = snapshot(vec![variant(account, &[])]);

    let assembled = assemble(&snap, account, &env.media, env.owner)
        .await
        .unwrap();

    assert!(assembled.assets.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn assemble_reports_deleted_media(pool: PgPool) {
    let env = env(&pool).await;
    let kept = upload(&env, "a.png").await;
    let deleted = upload(&env, "b.png").await;
    let never_existed = MediaAssetId::from_uuid(Uuid::now_v7());
    env.media.delete(&env.scope, deleted).await.unwrap();
    let account = account();
    let snap = snapshot(vec![variant(account, &[kept, deleted, never_existed])]);

    let result = assemble(&snap, account, &env.media, env.owner).await;

    match result {
        Err(AssembleError::MediaMissing(ids)) => assert_eq!(ids, [deleted, never_existed]),
        other => panic!("expected MediaMissing, got {other:?}"),
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn assemble_reports_media_that_is_not_ready(pool: PgPool) {
    let env = env(&pool).await;
    // An open resumable upload has no bytes yet.
    let pending = env
        .media
        .start_session(&env.scope, "c.png", 1000)
        .await
        .unwrap()
        .id;
    let account = account();
    let snap = snapshot(vec![variant(account, &[pending])]);

    let result = assemble(&snap, account, &env.media, env.owner).await;

    match result {
        Err(AssembleError::MediaMissing(ids)) => assert_eq!(ids, [pending]),
        other => panic!("expected MediaMissing, got {other:?}"),
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn assemble_reports_missing_variant(pool: PgPool) {
    let env = env(&pool).await;
    let snap = snapshot(vec![variant(account(), &[])]);

    let result = assemble(&snap, account(), &env.media, env.owner).await;

    assert!(matches!(result, Err(AssembleError::VariantMissing)));
}
