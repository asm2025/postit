#![allow(clippy::unwrap_used)]

mod common;

use axum::http::Method;
use common::{Who, assert_problem, etag, upload_png, world};
use postit_core::PostVersion;
use postit_data::audit::AuditContext;
use postit_publishing::PublishInput;
use serde_json::{Value, json};
use sqlx::PgPool;

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

fn strings(value: &Value) -> Vec<String> {
    sorted(
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect(),
    )
}

#[sqlx::test(migrations = "../data/migrations")]
async fn post_crud_round_trip(pool: PgPool) {
    let w = world(pool).await;
    let media = upload_png(&w.app, &w.owner.token).await;
    let created = w
        .create_post(json!({
            "text": "hello",
            "tags": ["news"],
            "link": "https://example.com/a",
            "media": [{"media_id": media, "alt_text": "a square"}],
            "targets": [w.a.to_string()],
        }))
        .await;
    assert_eq!(created.status, 201, "{:?}", created.body);
    assert!(created.headers.contains_key("location"));
    let id = created.body["id"].as_str().unwrap().to_owned();
    assert_eq!(created.body["status"], "draft");
    assert_eq!(created.body["media"][0]["alt_text"], "a square");
    assert_eq!(created.body["targets"], json!([w.a.to_string()]));
    let tag = etag(&created);
    assert_eq!(tag, format!("\"v{}\"", created.body["version"]));

    let path = format!("/api/v1/posts/{id}");
    let patched = w
        .call_with(
            Who::Owner,
            Method::PATCH,
            &path,
            Some(json!({"text": "hello again", "link": null, "targets": [w.b.to_string()]})),
            &[("if-match", &tag)],
        )
        .await;
    assert_eq!(patched.status, 200, "{:?}", patched.body);
    assert_eq!(patched.body["text"], "hello again");
    assert!(patched.body["link"].is_null());
    assert_eq!(patched.body["targets"], json!([w.b.to_string()]));
    assert_ne!(etag(&patched), tag);

    let list = w.call(Who::Owner, Method::GET, "/api/v1/posts", None).await;
    assert_eq!(list.body["total"], 1);
    assert!(list.body["data"][0].get("media").is_none());

    let new_tag = etag(&patched);
    let put = w
        .call_with(
            Who::Owner,
            Method::PUT,
            &format!("{path}/overrides/{}", w.b),
            Some(json!({"patch": {"text": "short"}, "options": {}})),
            &[("if-match", &new_tag)],
        )
        .await;
    assert_eq!(put.status, 200, "{:?}", put.body);
    assert_eq!(put.body["overrides"][0]["patch"]["text"], "short");
    assert_ne!(etag(&put), new_tag);

    let del = w
        .call_with(
            Who::Owner,
            Method::DELETE,
            &path,
            None,
            &[("if-match", &etag(&put))],
        )
        .await;
    assert_eq!(del.status, 204);
    let gone = w.call(Who::Owner, Method::GET, &path, None).await;
    assert_problem(&gone, 404, "not_found");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn get_post_304_on_matching_etag(pool: PgPool) {
    let w = world(pool).await;
    let (id, tag) = w.post_to("hello", &[w.a]).await;
    let path = format!("/api/v1/posts/{id}");
    let same = w
        .call_with(
            Who::Owner,
            Method::GET,
            &path,
            None,
            &[("if-none-match", &tag)],
        )
        .await;
    assert_eq!(same.status, 304);
    assert_eq!(etag(&same), tag);
    let stale = w
        .call_with(
            Who::Owner,
            Method::GET,
            &path,
            None,
            &[("if-none-match", "\"v0\"")],
        )
        .await;
    assert_eq!(stale.status, 200);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn patch_without_if_match_is_428(pool: PgPool) {
    let w = world(pool).await;
    let (id, _) = w.post_to("hello", &[w.a]).await;
    let res = w
        .call(
            Who::Owner,
            Method::PATCH,
            &format!("/api/v1/posts/{id}"),
            Some(json!({"text": "x"})),
        )
        .await;
    assert_problem(&res, 428, "precondition_required");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn patch_stale_if_match_is_412(pool: PgPool) {
    let w = world(pool).await;
    let (id, tag) = w.post_to("hello", &[w.a]).await;
    let path = format!("/api/v1/posts/{id}");
    let first = w
        .call_with(
            Who::Owner,
            Method::PATCH,
            &path,
            Some(json!({"text": "one"})),
            &[("if-match", &tag)],
        )
        .await;
    assert_eq!(first.status, 200);
    let stale = w
        .call_with(
            Who::Owner,
            Method::PATCH,
            &path,
            Some(json!({"text": "two"})),
            &[("if-match", &tag)],
        )
        .await;
    assert_problem(&stale, 412, "version_conflict");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn patch_scheduled_post_is_409_post_locked(pool: PgPool) {
    let w = world(pool).await;
    let (id, tag) = w.post_to("hello", &[w.a]).await;
    let version: i64 = tag
        .trim_matches('"')
        .trim_start_matches('v')
        .parse()
        .unwrap();
    let scope = postit_data::OwnerScope::own(w.owner.id);
    w.app
        .state
        .publishing
        .create_job(
            &scope,
            postit_core::PostId::from(id.parse::<uuid::Uuid>().unwrap()),
            PublishInput {
                targets: vec![w.a],
                at: Some(chrono::Utc::now() + chrono::Duration::hours(1)),
                native_schedule: false,
                dry_run: false,
                confirm: true,
                if_match: PostVersion(version),
            },
            AuditContext::default(),
        )
        .await
        .unwrap();
    let res = w
        .call_with(
            Who::Owner,
            Method::PATCH,
            &format!("/api/v1/posts/{id}"),
            Some(json!({"text": "x"})),
            &[("if-match", &tag)],
        )
        .await;
    assert_problem(&res, 409, "post_locked");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn create_post_with_foreign_media_is_404(pool: PgPool) {
    let w = world(pool).await;
    let foreign = upload_png(&w.app, &w.stranger.token).await;
    let res = w
        .create_post(json!({"text": "x", "media": [{"media_id": foreign}], "targets": []}))
        .await;
    assert_problem(&res, 404, "not_found");
    let list = w.call(Who::Owner, Method::GET, "/api/v1/posts", None).await;
    assert_eq!(list.body["total"], 0, "no post row is left behind");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn create_post_with_target_outside_limit_is_403(pool: PgPool) {
    let w = world(pool).await;
    let res = w
        .call(
            Who::Limited,
            Method::POST,
            "/api/v1/posts",
            Some(json!({"text": "x", "targets": [w.a.to_string()]})),
        )
        .await;
    assert_problem(&res, 403, "account_not_in_scope");
    let ok = w
        .call(
            Who::Limited,
            Method::POST,
            "/api/v1/posts",
            Some(json!({"text": "x", "targets": [w.b.to_string()]})),
        )
        .await;
    assert_eq!(ok.status, 201, "{:?}", ok.body);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn hidden_target_alone_allows_patch_and_survives_set_targets(pool: PgPool) {
    let w = world(pool).await;
    let (id, tag) = w.post_to("hello", &[w.a, w.b]).await;
    let res = w
        .call_with(
            Who::Limited,
            Method::PATCH,
            &format!("/api/v1/posts/{id}"),
            Some(json!({"text": "edited", "targets": [w.b.to_string()]})),
            &[("if-match", &tag)],
        )
        .await;
    assert_eq!(res.status, 200, "{:?}", res.body);
    assert_eq!(res.body["text"], "edited");
    assert_eq!(res.body["targets"], json!([w.b.to_string()]));
    let owner = w
        .call(
            Who::Owner,
            Method::GET,
            &format!("/api/v1/posts/{id}"),
            None,
        )
        .await;
    assert_eq!(
        strings(&owner.body["targets"]),
        sorted(vec![w.a.to_string(), w.b.to_string()])
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn hidden_override_blocks_patch_and_delete(pool: PgPool) {
    let w = world(pool).await;
    let (id, tag) = w.post_to("hello", &[w.a, w.b]).await;
    let path = format!("/api/v1/posts/{id}");
    let put = w
        .call_with(
            Who::Owner,
            Method::PUT,
            &format!("{path}/overrides/{}", w.a),
            Some(json!({"patch": {"text": "for a"}})),
            &[("if-match", &tag)],
        )
        .await;
    let tag = etag(&put);
    let patch = w
        .call_with(
            Who::Limited,
            Method::PATCH,
            &path,
            Some(json!({"text": "x"})),
            &[("if-match", &tag)],
        )
        .await;
    assert_problem(&patch, 403, "account_not_in_scope");
    let delete = w
        .call_with(
            Who::Limited,
            Method::DELETE,
            &path,
            None,
            &[("if-match", &tag)],
        )
        .await;
    assert_problem(&delete, 403, "account_not_in_scope");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn limited_delegate_sees_only_their_overrides(pool: PgPool) {
    let w = world(pool).await;
    let (id, tag) = w.post_to("hello", &[w.a, w.b]).await;
    let path = format!("/api/v1/posts/{id}");
    let first = w
        .call_with(
            Who::Owner,
            Method::PUT,
            &format!("{path}/overrides/{}", w.a),
            Some(json!({"patch": {"text": "a"}})),
            &[("if-match", &tag)],
        )
        .await;
    let second = w
        .call_with(
            Who::Owner,
            Method::PUT,
            &format!("{path}/overrides/{}", w.b),
            Some(json!({"patch": {"text": "b"}})),
            &[("if-match", &etag(&first))],
        )
        .await;
    assert_eq!(second.status, 200);
    let owner = w.call(Who::Owner, Method::GET, &path, None).await;
    assert_eq!(owner.body["overrides"].as_array().unwrap().len(), 2);
    let limited = w.call(Who::Limited, Method::GET, &path, None).await;
    let overrides = limited.body["overrides"].as_array().unwrap();
    assert_eq!(overrides.len(), 1);
    assert_eq!(overrides[0]["account_id"], w.b.to_string());
    assert_eq!(limited.body["targets"], json!([w.b.to_string()]));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn post_access(pool: PgPool) {
    let w = world(pool).await;
    let (id, tag) = w.post_to("hello", &[w.a]).await;
    let path = format!("/api/v1/posts/{id}");
    for who in [Who::Owner, Who::View, Who::Edit, Who::Publish, Who::Limited] {
        let res = w.call(who, Method::GET, &path, None).await;
        assert_eq!(res.status, 200, "{who:?}");
    }
    assert_problem(
        &w.call(Who::Stranger, Method::GET, &path, None).await,
        404,
        "not_found",
    );
    let list = w
        .call(Who::Stranger, Method::GET, "/api/v1/posts", None)
        .await;
    assert_eq!(list.body["total"], 0);

    // A view delegate cannot write; an edit delegate can.
    let create = |who| {
        w.call(
            who,
            Method::POST,
            "/api/v1/posts",
            Some(json!({"text": "x"})),
        )
    };
    assert_problem(&create(Who::View).await, 403, "capability_required");
    assert_eq!(create(Who::Edit).await.status, 201);
    let body = Some(json!({"text": "y"}));
    let view = matched(&w, Who::View, Method::PATCH, &path, body.clone(), &tag).await;
    assert_problem(&view, 403, "capability_required");
    let stranger = matched(&w, Who::Stranger, Method::PATCH, &path, body.clone(), &tag).await;
    assert_problem(&stranger, 404, "not_found");
    let edited = matched(&w, Who::Edit, Method::PATCH, &path, body, &tag).await;
    assert_eq!(edited.status, 200, "{:?}", edited.body);

    // Live history makes deleting a `publish` action.
    let scope = postit_data::OwnerScope::own(w.owner.id);
    let post_id = postit_core::PostId::from(id.parse::<uuid::Uuid>().unwrap());
    let version: i64 = etag(&edited)
        .trim_matches('"')
        .trim_start_matches('v')
        .parse()
        .unwrap();
    w.app
        .state
        .publishing
        .create_job(
            &scope,
            post_id,
            PublishInput {
                targets: vec![w.a],
                at: None,
                native_schedule: false,
                dry_run: false,
                confirm: true,
                if_match: PostVersion(version),
            },
            AuditContext::default(),
        )
        .await
        .unwrap();
    assert!(w.app.drain_publishing().await >= 1);
    let current = w.call(Who::Owner, Method::GET, &path, None).await;
    let tag = etag(&current);
    let edit = matched(&w, Who::Edit, Method::DELETE, &path, None, &tag).await;
    assert_problem(&edit, 403, "capability_required");
    let publish = matched(&w, Who::Publish, Method::DELETE, &path, None, &tag).await;
    assert_eq!(publish.status, 204);
}

/// A request as `who` with `If-Match: tag`.
async fn matched(
    w: &common::World,
    who: Who,
    method: Method,
    path: &str,
    body: Option<Value>,
    tag: &str,
) -> postit_api::testkit::TestResponse {
    w.call_with(who, method, path, body, &[("if-match", tag)])
        .await
}

fn codes(violations: &Value) -> Vec<String> {
    violations
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["code"].as_str().unwrap().to_owned())
        .collect()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn validate_draft_reports_per_target(pool: PgPool) {
    let w = world(pool).await;
    let long = "x".repeat(501);
    let res = w
        .call(
            Who::Owner,
            Method::POST,
            "/api/v1/posts/validate",
            Some(json!({
                "text": "hello",
                "targets": [w.a.to_string(), w.b.to_string()],
                "overrides": {w.b.to_string(): {"patch": {"text": long}}},
            })),
        )
        .await;
    assert_eq!(res.status, 200, "{:?}", res.body);
    assert_eq!(res.body["valid"], false);
    let targets = res.body["targets"].as_array().unwrap();
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0]["account_id"], w.a.to_string());
    assert_eq!(targets[0]["violations"], json!([]));
    assert_eq!(targets[0]["format"], "text");
    assert_eq!(codes(&targets[1]["violations"]), ["text_too_long"]);
    assert_eq!(res.body["violations"], json!([]));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn validate_reports_schedule_violation_at_top_level(pool: PgPool) {
    let w = world(pool).await;
    let res = w
        .call(
            Who::Owner,
            Method::POST,
            "/api/v1/posts/validate",
            Some(json!({
                "text": "hello",
                "targets": [w.a.to_string()],
                "at": {"local": "2026-03-29T02:30", "zone": "Europe/Berlin"},
            })),
        )
        .await;
    assert_eq!(res.status, 200, "{:?}", res.body);
    assert_eq!(res.body["valid"], false);
    assert_eq!(
        codes(&res.body["violations"]),
        ["schedule_time_nonexistent"]
    );
    assert_eq!(res.body["targets"][0]["violations"], json!([]));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn validate_draft_foreign_media_is_violation(pool: PgPool) {
    let w = world(pool).await;
    let foreign = upload_png(&w.app, &w.stranger.token).await;
    let res = w
        .call(
            Who::Owner,
            Method::POST,
            "/api/v1/posts/validate",
            Some(json!({
                "text": "hello",
                "media": [{"media_id": foreign}],
                "targets": [w.a.to_string(), w.b.to_string()],
            })),
        )
        .await;
    assert_eq!(res.status, 200, "{:?}", res.body);
    for target in res.body["targets"].as_array().unwrap() {
        assert!(
            codes(&target["violations"]).contains(&"media_missing".to_owned()),
            "{target:?}"
        );
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn validate_post_defaults_to_post_targets(pool: PgPool) {
    let w = world(pool).await;
    let (id, _) = w.post_to("hello", &[w.b]).await;
    let path = format!("/api/v1/posts/{id}/validate");
    let res = w
        .call(Who::Owner, Method::POST, &path, Some(json!({})))
        .await;
    assert_eq!(res.status, 200, "{:?}", res.body);
    assert_eq!(res.body["valid"], true);
    assert_eq!(res.body["targets"].as_array().unwrap().len(), 1);
    assert_eq!(res.body["targets"][0]["account_id"], w.b.to_string());
    let both = w
        .call(
            Who::Owner,
            Method::POST,
            &path,
            Some(json!({"targets": [w.a.to_string(), w.b.to_string()]})),
        )
        .await;
    assert_eq!(both.body["targets"].as_array().unwrap().len(), 2);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn validate_matches_publish_verdict(pool: PgPool) {
    let w = world(pool).await;
    let scope = postit_data::OwnerScope::own(w.owner.id);
    for text in ["hello".to_owned(), "x".repeat(501), String::new()] {
        let (id, tag) = w.post_to(&text, &[w.a]).await;
        let validated = w
            .call(
                Who::Owner,
                Method::POST,
                &format!("/api/v1/posts/{id}/validate"),
                Some(json!({})),
            )
            .await;
        let version: i64 = tag
            .trim_matches('"')
            .trim_start_matches('v')
            .parse()
            .unwrap();
        let publish = w
            .app
            .state
            .publishing
            .create_job(
                &scope,
                postit_core::PostId::from(id.parse::<uuid::Uuid>().unwrap()),
                PublishInput {
                    targets: vec![w.a],
                    at: None,
                    native_schedule: false,
                    dry_run: true,
                    confirm: false,
                    if_match: PostVersion(version),
                },
                AuditContext::default(),
            )
            .await;
        assert_eq!(
            validated.body["valid"].as_bool().unwrap(),
            publish.is_ok(),
            "{} characters",
            text.len()
        );
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn validate_is_rate_limited(pool: PgPool) {
    let w = common::world_with(pool, |s| {
        s.rate_limit.publish_actions = postit_config::RateBucket {
            rate_per_minute: 1,
            burst: 1,
        };
    })
    .await;
    let body = Some(json!({"text": "hi", "targets": [w.a.to_string()]}));
    let first = w
        .call(
            Who::Owner,
            Method::POST,
            "/api/v1/posts/validate",
            body.clone(),
        )
        .await;
    assert_eq!(first.status, 200, "{:?}", first.body);
    let second = w
        .call(Who::Owner, Method::POST, "/api/v1/posts/validate", body)
        .await;
    assert_problem(&second, 429, "rate_limited");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn validate_access(pool: PgPool) {
    let w = world(pool).await;
    let (id, _) = w.post_to("hello", &[w.a]).await;
    let draft = |who, target: String| {
        w.call(
            who,
            Method::POST,
            "/api/v1/posts/validate",
            Some(json!({"text": "hi", "targets": [target]})),
        )
    };
    assert_problem(
        &draft(Who::View, w.a.to_string()).await,
        403,
        "capability_required",
    );
    assert_eq!(draft(Who::Edit, w.a.to_string()).await.status, 200);
    assert_eq!(draft(Who::Publish, w.a.to_string()).await.status, 200);
    assert_problem(
        &draft(Who::Limited, w.a.to_string()).await,
        403,
        "account_not_in_scope",
    );
    assert_eq!(draft(Who::Limited, w.b.to_string()).await.status, 200);
    assert_problem(
        &draft(Who::Stranger, w.a.to_string()).await,
        404,
        "not_found",
    );

    let path = format!("/api/v1/posts/{id}/validate");
    let saved = |who| w.call(who, Method::POST, &path, Some(json!({})));
    assert_problem(&saved(Who::View).await, 403, "capability_required");
    assert_eq!(saved(Who::Edit).await.status, 200);
    // The post targets account a, which the limited delegate cannot see: naming it is refused.
    let named = w
        .call(
            Who::Limited,
            Method::POST,
            &path,
            Some(json!({"targets": [w.a.to_string()]})),
        )
        .await;
    assert_problem(&named, 403, "account_not_in_scope");
    assert_problem(&saved(Who::Stranger).await, 404, "not_found");
}
