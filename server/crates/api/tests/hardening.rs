//! Cross-cutting API hardening: `X-Postit-Act-As` is refused until delegation exists, API
//! responses are neither cached nor sniffed, and every admin operation refuses a member.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use postit_api::testkit::{ADMIN_SUB, TestApp};
use postit_config::RateBucket;
use sqlx::PgPool;

fn with_act_as(path: &str, token: Option<&str>) -> Request<Body> {
    let mut b = Request::builder()
        .method(Method::GET)
        .uri(path)
        .header("x-postit-act-as", "0190f000-0000-7000-8000-000000000000");
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    b.body(Body::empty())
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[sqlx::test(migrations = "../data/migrations")]
async fn act_as_is_forbidden_everywhere_until_delegation_exists(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    for path in [
        "/api/v1/me",
        "/api/v1/users",
        "/api/v1/admin/jobs",
        "/api/v1/auth/config",
    ] {
        let res = app.send(with_act_as(path, Some(&token))).await;
        assert_eq!(res.status, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(res.body["code"], "forbidden", "{path}");
    }
    let anon = app.send(with_act_as("/api/v1/me", None)).await;
    assert_eq!(anon.status, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn act_as_is_charged_to_the_ip_bucket(pool: PgPool) {
    let app = TestApp::start_with(pool, |s| {
        s.rate_limit.unauthenticated = RateBucket {
            rate_per_minute: 1,
            burst: 1,
        };
    })
    .await;
    assert_eq!(
        app.send(with_act_as("/api/v1/me", None)).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.send(with_act_as("/api/v1/me", None)).await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn api_responses_are_not_cached_or_sniffed(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let ok = app
        .call(Method::GET, "/api/v1/me", Some(&token), None)
        .await;
    let err = app.call(Method::GET, "/api/v1/me", None, None).await;
    for res in [&ok, &err] {
        let h = |n: &str| {
            res.headers
                .get(n)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        assert_eq!(h("x-content-type-options").as_deref(), Some("nosniff"));
        assert_eq!(h("cache-control").as_deref(), Some("no-store"));
        assert_eq!(h("referrer-policy").as_deref(), Some("no-referrer"));
    }
    let spec = app.call(Method::GET, "/api/openapi.json", None, None).await;
    assert_eq!(
        spec.headers
            .get("x-content-type-options")
            .and_then(|v| v.to_str().ok()),
        Some("nosniff")
    );
}

/// Every operation under `/api/v1/users` and `/api/v1/admin` refuses an active member with
/// 403 `forbidden` before it reads the body or the path.
#[sqlx::test(migrations = "../data/migrations")]
async fn every_admin_operation_refuses_a_member(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    let me = app
        .call(Method::GET, "/api/v1/me", Some(&app.token("bob")), None)
        .await;
    let bob_id = me.body["id"].as_str().unwrap_or_default().to_string();
    app.call(
        Method::PATCH,
        &format!("/api/v1/users/{bob_id}"),
        Some(&admin),
        Some(serde_json::json!({ "status": "active" })),
    )
    .await;
    let member = app.token("bob");

    let spec: serde_json::Value = serde_json::from_str(&postit_api::openapi::openapi_json_pretty())
        .unwrap_or_else(|e| unreachable!("spec: {e}"));
    let paths = spec["paths"]
        .as_object()
        .unwrap_or_else(|| unreachable!("no paths"));
    let mut checked = 0;
    for (path, item) in paths {
        if !(path.starts_with("/api/v1/users") || path.starts_with("/api/v1/admin")) {
            continue;
        }
        let concrete = path
            .replace("{id}", "0190f000-0000-7000-8000-000000000000")
            .replace("{name}", "x");
        for method in ["get", "post", "put", "patch", "delete"] {
            if item.get(method).is_none() {
                continue;
            }
            let m = Method::from_bytes(method.to_uppercase().as_bytes())
                .unwrap_or_else(|e| unreachable!("{e}"));
            let body = (m != Method::GET && m != Method::DELETE).then(|| serde_json::json!({}));
            let res = app.call(m, &concrete, Some(&member), body).await;
            assert_eq!(res.status, StatusCode::FORBIDDEN, "{method} {path}");
            assert_eq!(res.body["code"], "forbidden", "{method} {path}");
            checked += 1;
        }
    }
    assert!(checked >= 10, "only {checked} admin operations found");
}
