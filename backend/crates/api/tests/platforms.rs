#![allow(clippy::unwrap_used)]

mod common;

use axum::http::{Method, StatusCode};
use common::{Who, assert_problem, etag, mock, world, world_with};
use postit_data::platforms::PlatformRepo;
use sqlx::PgPool;

#[sqlx::test(migrations = "../data/migrations")]
async fn platforms_lists_enabled_compiled_platforms(pool: PgPool) {
    let w = world(pool.clone()).await;
    let res = w
        .call(Who::Owner, Method::GET, "/api/v1/platforms", None)
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    let ids: Vec<&str> = res
        .body
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["platform"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"mock"), "{ids:?}");
    assert!(res.body[0].get("icon_png").is_none());

    PlatformRepo::set_enabled(&pool, &mock(), false)
        .await
        .unwrap();
    let res = w
        .call(Who::Owner, Method::GET, "/api/v1/platforms", None)
        .await;
    assert!(res.body.as_array().unwrap().is_empty(), "{:?}", res.body);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn platform_icon_is_png_with_cache_headers(pool: PgPool) {
    let w = world(pool).await;
    let res = w
        .call(Who::Owner, Method::GET, "/api/v1/platforms/mock/icon", None)
        .await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.headers["content-type"], "image/png");
    assert_eq!(res.headers["cache-control"], "public, max-age=86400");
    assert!(res.bytes.starts_with(&[0x89, b'P', b'N', b'G']));
    let tag = etag(&res);
    assert!(tag.starts_with('"') && tag.ends_with('"') && !tag.starts_with("\"v"));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn platform_icon_304_on_matching_etag(pool: PgPool) {
    let w = world(pool).await;
    let first = w
        .call(Who::Owner, Method::GET, "/api/v1/platforms/mock/icon", None)
        .await;
    let tag = etag(&first);
    let again = w
        .call_with(
            Who::Owner,
            Method::GET,
            "/api/v1/platforms/mock/icon",
            None,
            &[("if-none-match", &tag)],
        )
        .await;
    assert_eq!(again.status, StatusCode::NOT_MODIFIED);
    assert!(again.bytes.is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn icon_of_disabled_platform_is_404(pool: PgPool) {
    let w = world(pool.clone()).await;
    PlatformRepo::set_enabled(&pool, &mock(), false)
        .await
        .unwrap();
    for id in ["mock", "nope", "Not_A_Platform"] {
        let res = w
            .call(
                Who::Owner,
                Method::GET,
                &format!("/api/v1/platforms/{id}/icon"),
                None,
            )
            .await;
        assert_problem(&res, 404, "not_found");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn publish_options_returns_the_agents_schema(pool: PgPool) {
    let w = world(pool).await;
    let res = w
        .call(
            Who::Owner,
            Method::GET,
            &format!("/api/v1/accounts/{}/publish-options", w.a),
            None,
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert!(res.body.is_object(), "{:?}", res.body);
    assert_eq!(res.body["type"], "object", "{:?}", res.body);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn publish_options_access(pool: PgPool) {
    let w = world(pool).await;
    let path = format!("/api/v1/accounts/{}/publish-options", w.a);
    let get = |who| w.call(who, Method::GET, &path, None);
    assert_problem(&get(Who::View).await, 403, "capability_required");
    assert_eq!(get(Who::Edit).await.status, StatusCode::OK);
    assert_eq!(get(Who::Publish).await.status, StatusCode::OK);
    assert_problem(&get(Who::Limited).await, 404, "not_found");
    assert_problem(&get(Who::Stranger).await, 404, "not_found");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn publish_options_is_rate_limited(pool: PgPool) {
    let w = world_with(pool, |s| {
        s.rate_limit.credential_checks = postit_config::RateBucket {
            rate_per_minute: 1,
            burst: 1,
        };
    })
    .await;
    let path = format!("/api/v1/accounts/{}/publish-options", w.a);
    let first = w.call(Who::Owner, Method::GET, &path, None).await;
    assert_eq!(first.status, StatusCode::OK, "{:?}", first.body);
    let second = w.call(Who::Owner, Method::GET, &path, None).await;
    assert_problem(&second, 429, "rate_limited");
}
