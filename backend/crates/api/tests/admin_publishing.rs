#![allow(clippy::unwrap_used, clippy::too_many_lines)]

mod common;
mod oauth_support;

use axum::body::Body;
use axum::http::{Method, Request, header};
use common::{World, assert_problem, world};
use postit_api::testkit::{ADMIN_SUB, TestApp, TestResponse};
use serde_json::{Value, json};
use sqlx::PgPool;

async fn admin(app: &TestApp, method: Method, path: &str, body: Option<Value>) -> TestResponse {
    app.call(method, path, Some(&app.token(ADMIN_SUB)), body)
        .await
}

async fn audit_details(pool: &PgPool, kind: &str) -> Vec<Value> {
    sqlx::query_scalar("SELECT details FROM audit_events WHERE kind = $1 ORDER BY at")
        .bind(kind)
        .fetch_all(pool)
        .await
        .unwrap()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn admin_lists_platforms_with_redirect_uri(pool: PgPool) {
    let f = oauth_support::fixture(pool).await;
    let res = admin(&f.app, Method::GET, "/api/v1/admin/platforms", None).await;
    assert_eq!(res.status, 200, "{:?}", res.body);
    let platforms = res.body.as_array().unwrap();
    let find = |id: &str| platforms.iter().find(|p| p["id"] == id).unwrap();
    let oauth = find(oauth_support::OAUTH);
    assert_eq!(oauth["compiled_in"], true);
    assert_eq!(oauth["configured"], true);
    assert_eq!(oauth["enabled"], true);
    assert!(
        oauth["redirect_uri"]
            .as_str()
            .unwrap()
            .ends_with("/api/v1/platforms/mock_oauth/oauth/callback"),
        "{oauth:?}"
    );
    assert!(find(oauth_support::CREDENTIALS)["redirect_uri"].is_null());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disabling_platform_writes_audit(pool: PgPool) {
    let w = world(pool.clone()).await;
    let res = admin(
        &w.app,
        Method::PUT,
        "/api/v1/admin/platforms/mock",
        Some(json!({"enabled": false})),
    )
    .await;
    assert_eq!(res.status, 200, "{:?}", res.body);
    assert_eq!(res.body["enabled"], false);
    let events = audit_details(&pool, "platform_enabled_changed").await;
    assert_eq!(
        events,
        vec![json!({"platform_id": "mock", "old": true, "new": false})]
    );
    let unknown = admin(
        &w.app,
        Method::PUT,
        "/api/v1/admin/platforms/nope",
        Some(json!({"enabled": false})),
    )
    .await;
    assert_problem(&unknown, 404, "not_found");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn live_switch_round_trip_writes_audit(pool: PgPool) {
    let w = world(pool.clone()).await;
    let before = admin(
        &w.app,
        Method::GET,
        "/api/v1/admin/settings/publishing",
        None,
    )
    .await;
    assert_eq!(before.status, 200, "{:?}", before.body);
    assert_eq!(before.body["live"], true);
    assert_eq!(before.body["native_scheduled_count"], 0);
    let off = admin(
        &w.app,
        Method::PUT,
        "/api/v1/admin/settings/publishing",
        Some(json!({"live": false})),
    )
    .await;
    assert_eq!(off.body["live"], false);
    let after = admin(
        &w.app,
        Method::GET,
        "/api/v1/admin/settings/publishing",
        None,
    )
    .await;
    assert_eq!(after.body["live"], false);
    assert_eq!(
        audit_details(&pool, "live_publishing_changed").await,
        vec![json!({"old": true, "new": false})]
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn native_scheduled_count_counts_waiting_uploads(pool: PgPool) {
    let w = world(pool).await;
    let (post, tag) = w.post_to("hello", &[w.a]).await;
    let at = (chrono::Utc::now() + chrono::Duration::minutes(30)).to_rfc3339();
    let res = w
        .call_with(
            common::Who::Owner,
            Method::POST,
            &format!("/api/v1/posts/{post}/publish"),
            Some(json!({
                "targets": [w.a.to_string()],
                "confirm": true,
                "native_schedule": true,
                "at": at,
            })),
            &[("if-match", &tag)],
        )
        .await;
    assert_eq!(res.status, 201, "{:?}", res.body);
    assert!(w.app.drain_publishing().await >= 1);
    let settings = admin(
        &w.app,
        Method::GET,
        "/api/v1/admin/settings/publishing",
        None,
    )
    .await;
    assert_eq!(settings.body["native_scheduled_count"], 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn user_limits_round_trip_writes_audit(pool: PgPool) {
    let w = world(pool.clone()).await;
    let path = format!("/api/v1/admin/users/{}/limits", w.owner.id);
    let before = admin(&w.app, Method::GET, &path, None).await;
    assert_eq!(before.status, 200, "{:?}", before.body);
    let put = admin(
        &w.app,
        Method::PUT,
        &path,
        Some(json!({
            "email_daily_cap": 7,
            "contacts_cap": before.body["contacts_cap"],
            "storage_quota_bytes": 0,
        })),
    )
    .await;
    assert_eq!(put.status, 200, "{:?}", put.body);
    assert_eq!(put.body["email_daily_cap"], 7);
    assert_eq!(put.body["storage_quota_bytes"], 0);
    let events = audit_details(&pool, "user_limits_changed").await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["target_user_id"], w.owner.id.to_string());
    assert_eq!(events[0]["email_daily_cap"]["new"], 7);
    assert_eq!(
        events[0]["email_daily_cap"]["old"],
        before.body["email_daily_cap"]
    );
    assert!(
        events[0].get("contacts_cap").is_none(),
        "unchanged fields are left out"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn negative_limit_is_422(pool: PgPool) {
    let w = world(pool).await;
    let path = format!("/api/v1/admin/users/{}/limits", w.owner.id);
    let res = admin(
        &w.app,
        Method::PUT,
        &path,
        Some(json!({"email_daily_cap": -1, "contacts_cap": 1, "storage_quota_bytes": 1})),
    )
    .await;
    assert_problem(&res, 422, "validation_failed");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn limits_of_unknown_user_is_404(pool: PgPool) {
    let w = world(pool).await;
    let path = format!("/api/v1/admin/users/{}/limits", uuid::Uuid::now_v7());
    assert_problem(
        &admin(&w.app, Method::GET, &path, None).await,
        404,
        "not_found",
    );
    let put = admin(
        &w.app,
        Method::PUT,
        &path,
        Some(json!({"email_daily_cap": 1, "contacts_cap": 1, "storage_quota_bytes": 1})),
    )
    .await;
    assert_problem(&put, 404, "not_found");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn vault_rotate_enqueues_once(pool: PgPool) {
    let w = world(pool.clone()).await;
    let first = admin(&w.app, Method::POST, "/api/v1/admin/vault/rotate", None).await;
    assert_eq!(first.status, 202, "{:?}", first.body);
    assert!(first.body["job_id"].is_string());
    let second = admin(&w.app, Method::POST, "/api/v1/admin/vault/rotate", None).await;
    assert_problem(&second, 409, "rotation_in_progress");
    let queued: i64 =
        sqlx::query_scalar("SELECT count(*) FROM jobs WHERE job_type = 'vault_rotate'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(queued, 1);
}

fn admin_routes(w: &World) -> Vec<(Method, String, Option<Value>)> {
    let limits = format!("/api/v1/admin/users/{}/limits", w.owner.id);
    vec![
        (Method::GET, "/api/v1/admin/platforms".to_owned(), None),
        (
            Method::PUT,
            "/api/v1/admin/platforms/mock".to_owned(),
            Some(json!({"enabled": true})),
        ),
        (
            Method::GET,
            "/api/v1/admin/settings/publishing".to_owned(),
            None,
        ),
        (
            Method::PUT,
            "/api/v1/admin/settings/publishing".to_owned(),
            Some(json!({"live": true})),
        ),
        (Method::GET, limits.clone(), None),
        (
            Method::PUT,
            limits,
            Some(json!({"email_daily_cap": 1, "contacts_cap": 1, "storage_quota_bytes": 1})),
        ),
        (Method::POST, "/api/v1/admin/vault/rotate".to_owned(), None),
    ]
}

#[sqlx::test(migrations = "../data/migrations")]
async fn admin_routes_refuse_members(pool: PgPool) {
    let w = world(pool).await;
    for (method, path, body) in admin_routes(&w) {
        let res = w
            .app
            .call(method.clone(), &path, Some(&w.owner.token), body)
            .await;
        assert_problem(&res, 403, "forbidden");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn admin_routes_refuse_act_as(pool: PgPool) {
    let w = world(pool).await;
    for (method, path, body) in admin_routes(&w) {
        let mut builder = Request::builder()
            .method(method.clone())
            .uri(&path)
            .header(
                header::AUTHORIZATION,
                format!("Bearer {}", w.app.token(ADMIN_SUB)),
            )
            .header(common::ACT_AS, w.owner.id.to_string());
        let request = match body {
            Some(json) => {
                builder = builder.header(header::CONTENT_TYPE, "application/json");
                builder.body(Body::from(json.to_string()))
            }
            None => builder.body(Body::empty()),
        }
        .unwrap();
        let res = w.app.send(request).await;
        assert_problem(&res, 403, "owner_only");
    }
}
