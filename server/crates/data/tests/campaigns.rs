#![allow(clippy::unwrap_used)]

use chrono::NaiveDate;
use emixdb::dto::Pagination;
use postit_core::{CampaignId, PostId, PostKind};
use postit_data::campaigns::{CampaignFields, CampaignRepo};
use postit_data::posts::{NewPost, PostRepo};
use postit_data::testkit::{callers, seed_user};
use postit_data::{DataError, ScopeError};
use sqlx::PgPool;
use uuid::Uuid;

fn page() -> Pagination {
    Pagination {
        page: 1,
        page_size: 50,
    }
}

fn fields(name: &str) -> CampaignFields {
    CampaignFields {
        name: name.to_string(),
        starts_on: NaiveDate::from_ymd_opt(2026, 11, 1),
        ends_on: NaiveDate::from_ymd_opt(2026, 11, 30),
        color: Some("#ff8800".to_string()),
    }
}

fn id() -> CampaignId {
    CampaignId::from_uuid(Uuid::now_v7())
}

#[sqlx::test]
async fn campaign_isolation_matrix(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();

    assert!(matches!(
        CampaignRepo::create(&mut c, &who.view, id(), fields("x")).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    let campaign = CampaignRepo::create(&mut c, &who.edit, id(), fields("Launch"))
        .await
        .unwrap();
    assert_eq!(campaign.name, "Launch");
    assert!(!campaign.archived);

    for scope in [&who.owner, &who.view, &who.edit, &who.publish, &who.limited] {
        assert_eq!(
            CampaignRepo::get(&mut c, scope, campaign.id)
                .await
                .unwrap()
                .name,
            "Launch"
        );
        assert_eq!(
            CampaignRepo::list(&mut c, scope, page(), false)
                .await
                .unwrap()
                .total,
            1
        );
    }
    assert!(matches!(
        CampaignRepo::update(&mut c, &who.view, campaign.id, fields("y")).await,
        Err(DataError::Scope(ScopeError::InsufficientAccess))
    ));
    assert!(matches!(
        CampaignRepo::get(&mut c, &who.stranger, campaign.id).await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        CampaignRepo::update(&mut c, &who.stranger, campaign.id, fields("y")).await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        CampaignRepo::archive(&mut c, &who.stranger, campaign.id, true).await,
        Err(DataError::NotFound)
    ));
    assert!(matches!(
        CampaignRepo::delete(&mut c, &who.stranger, campaign.id).await,
        Err(DataError::NotFound)
    ));
    assert_eq!(
        CampaignRepo::list(&mut c, &who.stranger, page(), true)
            .await
            .unwrap()
            .total,
        0
    );
}

#[sqlx::test]
async fn archived_campaigns_leave_the_default_list(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let keep = CampaignRepo::create(&mut c, &who.owner, id(), fields("keep"))
        .await
        .unwrap();
    let old = CampaignRepo::create(&mut c, &who.owner, id(), fields("old"))
        .await
        .unwrap();

    let archived = CampaignRepo::archive(&mut c, &who.edit, old.id, true)
        .await
        .unwrap();
    assert!(archived.archived);

    let shown = CampaignRepo::list(&mut c, &who.owner, page(), false)
        .await
        .unwrap();
    assert_eq!(shown.total, 1);
    assert_eq!(shown.data[0].id, keep.id);
    let all = CampaignRepo::list(&mut c, &who.owner, page(), true)
        .await
        .unwrap();
    assert_eq!(all.total, 2);

    let restored = CampaignRepo::archive(&mut c, &who.edit, old.id, false)
        .await
        .unwrap();
    assert!(!restored.archived);
}

#[sqlx::test]
async fn update_replaces_name_dates_and_color(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let campaign = CampaignRepo::create(&mut c, &who.owner, id(), fields("a"))
        .await
        .unwrap();

    let updated = CampaignRepo::update(
        &mut c,
        &who.edit,
        campaign.id,
        CampaignFields {
            name: "b".to_string(),
            starts_on: None,
            ends_on: None,
            color: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.name, "b");
    assert_eq!(updated.starts_on, None);
    assert_eq!(updated.color, None);
    assert!(updated.updated_at >= campaign.updated_at);
}

#[sqlx::test]
async fn dates_out_of_order_are_rejected(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let mut backwards = fields("x");
    backwards.ends_on = NaiveDate::from_ymd_opt(2026, 10, 1);

    assert!(matches!(
        CampaignRepo::create(&mut c, &who.owner, id(), backwards).await,
        Err(DataError::Sql(_))
    ));
}

#[sqlx::test]
async fn campaign_delete_detaches_posts(pool: PgPool) {
    let owner = seed_user(&pool, "active").await;
    let who = callers(&pool, owner, &[]).await;
    let mut c = pool.acquire().await.unwrap();
    let campaign = CampaignRepo::create(&mut c, &who.owner, id(), fields("Launch"))
        .await
        .unwrap();
    let post = PostRepo::create(
        &mut c,
        &who.owner,
        NewPost {
            id: PostId::from_uuid(Uuid::now_v7()),
            kind: PostKind::Post,
            reply_to: None,
            campaign_id: Some(campaign.id),
            text: "hello".to_string(),
            link: None,
            tags: vec![],
            disclose_ai_generated: false,
        },
    )
    .await
    .unwrap();
    assert_eq!(post.campaign_id, Some(campaign.id));

    CampaignRepo::delete(&mut c, &who.edit, campaign.id)
        .await
        .unwrap();

    let after = PostRepo::get(&mut c, &who.owner, post.id).await.unwrap();
    assert_eq!(after.campaign_id, None);
    assert!(matches!(
        CampaignRepo::get(&mut c, &who.owner, campaign.id).await,
        Err(DataError::NotFound)
    ));
}
