#![allow(clippy::unwrap_used)]

mod common;

use axum::http::Method;
use common::{Who, assert_problem, world};
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test(migrations = "../data/migrations")]
async fn brand_profile_is_empty_until_written(pool: PgPool) {
    let w = world(pool).await;
    let res = w
        .call(Who::Owner, Method::GET, "/api/v1/brand-profile", None)
        .await;
    assert_eq!(res.status, 200, "{:?}", res.body);
    assert!(res.body["id"].is_null());
    assert_eq!(res.body["description"], "");
    assert_eq!(res.body["examples"], json!([]));
    assert!(res.body["updated_at"].is_null());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn brand_profile_put_then_get(pool: PgPool) {
    let w = world(pool).await;
    let body = json!({
        "description": "A bakery",
        "audience": "Neighbors",
        "offerings": "Bread",
        "avoid_words": ["cheap"],
        "default_language": "en-US",
        "examples": ["Fresh bread today"],
    });
    let put = w
        .call(
            Who::Owner,
            Method::PUT,
            "/api/v1/brand-profile",
            Some(body.clone()),
        )
        .await;
    assert_eq!(put.status, 200, "{:?}", put.body);
    assert!(put.body["id"].is_string());
    let got = w
        .call(Who::Owner, Method::GET, "/api/v1/brand-profile", None)
        .await;
    assert_eq!(got.body["description"], "A bakery");
    assert_eq!(got.body["avoid_words"], json!(["cheap"]));
    assert_eq!(got.body["id"], put.body["id"]);
    // A second PUT replaces it and keeps the ID.
    let again = w
        .call(
            Who::Owner,
            Method::PUT,
            "/api/v1/brand-profile",
            Some(json!({"description": "A cafe"})),
        )
        .await;
    assert_eq!(again.body["id"], put.body["id"]);
    assert_eq!(again.body["audience"], "");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn brand_profile_more_than_five_examples_is_422(pool: PgPool) {
    let w = world(pool).await;
    let res = w
        .call(
            Who::Owner,
            Method::PUT,
            "/api/v1/brand-profile",
            Some(json!({"examples": ["1", "2", "3", "4", "5", "6"]})),
        )
        .await;
    assert_problem(&res, 422, "validation_failed");
    assert_eq!(res.body["field"], "examples");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn brand_profile_access(pool: PgPool) {
    let w = world(pool).await;
    let put = |who| {
        w.call(
            who,
            Method::PUT,
            "/api/v1/brand-profile",
            Some(json!({"description": "Owned by the owner"})),
        )
    };
    assert_problem(&put(Who::View).await, 403, "capability_required");
    assert_eq!(put(Who::Edit).await.status, 200);
    let get = |who| w.call(who, Method::GET, "/api/v1/brand-profile", None);
    for who in [Who::Owner, Who::View, Who::Edit, Who::Publish, Who::Limited] {
        let res = get(who).await;
        assert_eq!(res.body["description"], "Owned by the owner", "{who:?}");
    }
    // A stranger has no grant, so they read their own (empty) profile, never the owner's.
    let res = get(Who::Stranger).await;
    assert_eq!(res.status, 200);
    assert!(res.body["id"].is_null());
    assert_eq!(res.body["description"], "");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn writing_style_is_empty_until_written(pool: PgPool) {
    let w = world(pool).await;
    let res = w
        .call(
            Who::Owner,
            Method::GET,
            &format!("/api/v1/accounts/{}/writing-style", w.a),
            None,
        )
        .await;
    assert_eq!(res.status, 200, "{:?}", res.body);
    assert!(res.body["id"].is_null());
    assert_eq!(res.body["account_id"], w.a.to_string());
    assert_eq!(res.body["emoji"], "none");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn writing_style_round_trip(pool: PgPool) {
    let w = world(pool).await;
    let path = format!("/api/v1/accounts/{}/writing-style", w.a);
    let put = w
        .call(
            Who::Owner,
            Method::PUT,
            &path,
            Some(json!({"tone": "warm", "emoji": "light", "hashtags": "few", "notes": "short"})),
        )
        .await;
    assert_eq!(put.status, 200, "{:?}", put.body);
    let got = w.call(Who::Owner, Method::GET, &path, None).await;
    assert_eq!(got.body["tone"], "warm");
    assert_eq!(got.body["emoji"], "light");
    assert_eq!(got.body["hashtags"], "few");
    assert_eq!(got.body["id"], put.body["id"]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn writing_style_access(pool: PgPool) {
    let w = world(pool).await;
    let path = format!("/api/v1/accounts/{}/writing-style", w.a);
    let body = Some(json!({"tone": "calm"}));
    let get = |who| w.call(who, Method::GET, &path, None);
    let put = |who| w.call(who, Method::PUT, &path, body.clone());
    assert_problem(&put(Who::View).await, 403, "capability_required");
    assert_eq!(put(Who::Edit).await.status, 200);
    assert_eq!(put(Who::Publish).await.status, 200);
    for who in [Who::Owner, Who::View, Who::Edit, Who::Publish] {
        assert_eq!(get(who).await.status, 200, "{who:?}");
    }
    // Account a is outside the limited delegate's limit, and the stranger has no grant.
    for who in [Who::Limited, Who::Stranger] {
        assert_problem(&get(who).await, 404, "not_found");
        assert_problem(&put(who).await, 404, "not_found");
    }
}
