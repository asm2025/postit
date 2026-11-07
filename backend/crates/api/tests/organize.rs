#![allow(clippy::unwrap_used)]

mod common;

use axum::http::Method;
use common::{Who, assert_problem, connect, world};
use postit_core::{CampaignId, PostId, PostKind};
use postit_data::OwnerScope;
use postit_data::posts::{NewPost, PostRepo};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

fn ids(body: &Value) -> Vec<String> {
    let mut ids: Vec<String> = body["account_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    ids.sort();
    ids
}

fn sorted(a: &str, b: &str) -> Vec<String> {
    let mut v = vec![a.to_owned(), b.to_owned()];
    v.sort();
    v
}

#[sqlx::test(migrations = "../data/migrations")]
async fn target_group_crud(pool: PgPool) {
    let w = world(pool).await;
    let created = w
        .call(
            Who::Owner,
            Method::POST,
            "/api/v1/target-groups",
            Some(json!({"name": "Everywhere", "account_ids": [w.a.to_string()]})),
        )
        .await;
    assert_eq!(created.status, 201, "{:?}", created.body);
    let id = created.body["id"].as_str().unwrap().to_owned();
    assert_eq!(created.body["account_ids"], json!([w.a.to_string()]));

    let put = w
        .call(
            Who::Owner,
            Method::PUT,
            &format!("/api/v1/target-groups/{id}"),
            Some(json!({"name": "Both", "account_ids": [w.a.to_string(), w.b.to_string()]})),
        )
        .await;
    assert_eq!(put.status, 200, "{:?}", put.body);
    assert_eq!(put.body["name"], "Both");
    assert_eq!(ids(&put.body), sorted(&w.a.to_string(), &w.b.to_string()));

    let list = w
        .call(Who::Owner, Method::GET, "/api/v1/target-groups", None)
        .await;
    assert_eq!(list.body["total"], 1);
    assert_eq!(list.body["data"][0]["name"], "Both");

    let del = w
        .call(
            Who::Owner,
            Method::DELETE,
            &format!("/api/v1/target-groups/{id}"),
            None,
        )
        .await;
    assert_eq!(del.status, 204);
    let again = w
        .call(
            Who::Owner,
            Method::DELETE,
            &format!("/api/v1/target-groups/{id}"),
            None,
        )
        .await;
    assert_problem(&again, 404, "not_found");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn limited_delegate_put_keeps_hidden_accounts(pool: PgPool) {
    let w = world(pool).await;
    // The limited delegate sees only account b; the owner's group holds a and b.
    let created = w
        .call(
            Who::Owner,
            Method::POST,
            "/api/v1/target-groups",
            Some(json!({"name": "g", "account_ids": [w.a.to_string(), w.b.to_string()]})),
        )
        .await;
    let id = created.body["id"].as_str().unwrap().to_owned();
    let path = format!("/api/v1/target-groups/{id}");
    let put = w
        .call(
            Who::Limited,
            Method::PUT,
            &path,
            Some(json!({"name": "g2", "account_ids": [w.b.to_string()]})),
        )
        .await;
    assert_eq!(put.status, 200, "{:?}", put.body);
    assert_eq!(ids(&put.body), vec![w.b.to_string()]);
    let owner_view = w
        .call(Who::Owner, Method::GET, "/api/v1/target-groups", None)
        .await;
    assert_eq!(
        ids(&owner_view.body["data"][0]),
        sorted(&w.a.to_string(), &w.b.to_string())
    );
    let delegate_view = w
        .call(Who::Limited, Method::GET, "/api/v1/target-groups", None)
        .await;
    assert_eq!(ids(&delegate_view.body["data"][0]), vec![w.b.to_string()]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn limited_delegate_naming_hidden_account_is_403(pool: PgPool) {
    let w = world(pool).await;
    let created = w
        .call(
            Who::Owner,
            Method::POST,
            "/api/v1/target-groups",
            Some(json!({"name": "g", "account_ids": [w.b.to_string()]})),
        )
        .await;
    let id = created.body["id"].as_str().unwrap().to_owned();
    let put = w
        .call(
            Who::Limited,
            Method::PUT,
            &format!("/api/v1/target-groups/{id}"),
            Some(json!({"name": "g", "account_ids": [w.a.to_string()]})),
        )
        .await;
    assert_problem(&put, 403, "account_not_in_scope");
    let post = w
        .call(
            Who::Limited,
            Method::POST,
            "/api/v1/target-groups",
            Some(json!({"name": "h", "account_ids": [w.a.to_string()]})),
        )
        .await;
    assert_problem(&post, 403, "account_not_in_scope");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn target_group_with_foreign_account_is_404(pool: PgPool) {
    let w = world(pool).await;
    let foreign = connect(&w.app, &w.script, w.stranger.id, "acct-foreign").await;
    let res = w
        .call(
            Who::Owner,
            Method::POST,
            "/api/v1/target-groups",
            Some(json!({"name": "g", "account_ids": [foreign.to_string()]})),
        )
        .await;
    assert_problem(&res, 404, "not_found");
    let list = w
        .call(Who::Owner, Method::GET, "/api/v1/target-groups", None)
        .await;
    assert_eq!(list.body["total"], 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn target_group_access(pool: PgPool) {
    let w = world(pool).await;
    let body = Some(json!({"name": "g", "account_ids": []}));
    let post = |who| w.call(who, Method::POST, "/api/v1/target-groups", body.clone());
    assert_problem(&post(Who::View).await, 403, "capability_required");
    assert_eq!(post(Who::Edit).await.status, 201);
    assert_eq!(post(Who::Publish).await.status, 201);
    let list = |who| w.call(who, Method::GET, "/api/v1/target-groups", None);
    for who in [Who::Owner, Who::View, Who::Edit, Who::Publish, Who::Limited] {
        assert_eq!(list(who).await.body["total"], 2, "{who:?}");
    }
    // A stranger sees their own (empty) groups, never the owner's.
    assert_eq!(list(Who::Stranger).await.body["total"], 0);
    let id = list(Who::Owner).await.body["data"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let path = format!("/api/v1/target-groups/{id}");
    assert_problem(
        &w.call(Who::Stranger, Method::DELETE, &path, None).await,
        404,
        "not_found",
    );
    assert_problem(
        &w.call(Who::View, Method::DELETE, &path, None).await,
        403,
        "capability_required",
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn campaign_crud(pool: PgPool) {
    let w = world(pool).await;
    let created = w
        .call(
            Who::Owner,
            Method::POST,
            "/api/v1/campaigns",
            Some(json!({"name": "Launch", "starts_on": "2026-11-01", "ends_on": "2026-11-30", "color": "#ff8800"})),
        )
        .await;
    assert_eq!(created.status, 201, "{:?}", created.body);
    assert_eq!(created.body["archived"], false);
    let id = created.body["id"].as_str().unwrap().to_owned();
    let list = w
        .call(Who::Owner, Method::GET, "/api/v1/campaigns", None)
        .await;
    assert_eq!(list.body["total"], 1);
    let del = w
        .call(
            Who::Owner,
            Method::DELETE,
            &format!("/api/v1/campaigns/{id}"),
            None,
        )
        .await;
    assert_eq!(del.status, 204);
    let gone = w
        .call(
            Who::Owner,
            Method::PATCH,
            &format!("/api/v1/campaigns/{id}"),
            Some(json!({"name": "x"})),
        )
        .await;
    assert_problem(&gone, 404, "not_found");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn campaign_patch_changes_only_sent_fields(pool: PgPool) {
    let w = world(pool).await;
    let created = w
        .call(
            Who::Owner,
            Method::POST,
            "/api/v1/campaigns",
            Some(json!({"name": "Launch", "starts_on": "2026-11-01", "ends_on": "2026-11-30"})),
        )
        .await;
    let path = format!("/api/v1/campaigns/{}", created.body["id"].as_str().unwrap());
    let renamed = w
        .call(
            Who::Owner,
            Method::PATCH,
            &path,
            Some(json!({"name": "Relaunch"})),
        )
        .await;
    assert_eq!(renamed.body["name"], "Relaunch");
    assert_eq!(renamed.body["starts_on"], "2026-11-01");
    assert_eq!(renamed.body["ends_on"], "2026-11-30");
    let cleared = w
        .call(
            Who::Owner,
            Method::PATCH,
            &path,
            Some(json!({"ends_on": null})),
        )
        .await;
    assert_eq!(cleared.body["name"], "Relaunch");
    assert_eq!(cleared.body["starts_on"], "2026-11-01");
    assert!(cleared.body["ends_on"].is_null());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn campaign_ends_before_start_is_422(pool: PgPool) {
    let w = world(pool).await;
    let res = w
        .call(
            Who::Owner,
            Method::POST,
            "/api/v1/campaigns",
            Some(json!({"name": "x", "starts_on": "2026-11-30", "ends_on": "2026-11-01"})),
        )
        .await;
    assert_problem(&res, 422, "validation_failed");
    assert_eq!(res.body["field"], "ends_on");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn campaign_delete_unlinks_posts(pool: PgPool) {
    let w = world(pool.clone()).await;
    let created = w
        .call(
            Who::Owner,
            Method::POST,
            "/api/v1/campaigns",
            Some(json!({"name": "Launch"})),
        )
        .await;
    let campaign = Uuid::parse_str(created.body["id"].as_str().unwrap()).unwrap();
    let post = PostId::from_uuid(Uuid::now_v7());
    let mut conn = pool.acquire().await.unwrap();
    PostRepo::create(
        &mut conn,
        &OwnerScope::own(w.owner.id),
        NewPost {
            id: post,
            kind: PostKind::Post,
            reply_to: None,
            campaign_id: Some(CampaignId::from_uuid(campaign)),
            text: "hi".to_owned(),
            link: None,
            tags: vec![],
            disclose_ai_generated: false,
        },
    )
    .await
    .unwrap();
    drop(conn);
    let del = w
        .call(
            Who::Owner,
            Method::DELETE,
            &format!("/api/v1/campaigns/{campaign}"),
            None,
        )
        .await;
    assert_eq!(del.status, 204);
    let linked: Option<Uuid> = sqlx::query_scalar("SELECT campaign_id FROM posts WHERE id = $1")
        .bind(post.as_uuid())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(linked, None);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn campaign_include_archived_filter(pool: PgPool) {
    let w = world(pool).await;
    let created = w
        .call(
            Who::Owner,
            Method::POST,
            "/api/v1/campaigns",
            Some(json!({"name": "Old"})),
        )
        .await;
    let path = format!("/api/v1/campaigns/{}", created.body["id"].as_str().unwrap());
    let archived = w
        .call(
            Who::Owner,
            Method::PATCH,
            &path,
            Some(json!({"archived": true})),
        )
        .await;
    assert_eq!(archived.body["archived"], true);
    let default = w
        .call(Who::Owner, Method::GET, "/api/v1/campaigns", None)
        .await;
    assert_eq!(default.body["total"], 0);
    let all = w
        .call(
            Who::Owner,
            Method::GET,
            "/api/v1/campaigns?include_archived=true",
            None,
        )
        .await;
    assert_eq!(all.body["total"], 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn campaign_access(pool: PgPool) {
    let w = world(pool).await;
    let body = Some(json!({"name": "c"}));
    let post = |who| w.call(who, Method::POST, "/api/v1/campaigns", body.clone());
    assert_problem(&post(Who::View).await, 403, "capability_required");
    let created = post(Who::Edit).await;
    assert_eq!(created.status, 201);
    let path = format!("/api/v1/campaigns/{}", created.body["id"].as_str().unwrap());
    let list = |who| w.call(who, Method::GET, "/api/v1/campaigns", None);
    for who in [Who::Owner, Who::View, Who::Edit, Who::Publish, Who::Limited] {
        assert_eq!(list(who).await.body["total"], 1, "{who:?}");
    }
    assert_eq!(list(Who::Stranger).await.body["total"], 0);
    let patch = |who| w.call(who, Method::PATCH, &path, Some(json!({"name": "d"})));
    assert_problem(&patch(Who::View).await, 403, "capability_required");
    assert_problem(&patch(Who::Stranger).await, 404, "not_found");
    assert_eq!(patch(Who::Publish).await.status, 200);
}
