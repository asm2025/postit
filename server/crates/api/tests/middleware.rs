use std::net::SocketAddr;
use std::time::Duration;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Method, Request, StatusCode, header};
use postit_api::testkit::TestApp;
use postit_config::RateBucket;
use sqlx::PgPool;

fn get(uri: &str) -> axum::http::request::Builder {
    Request::get(uri)
}

fn build(b: axum::http::request::Builder, body: Body) -> Request<Body> {
    b.body(body).unwrap_or_else(|e| unreachable!("{e}"))
}

#[sqlx::test(migrations = "../data/migrations")]
async fn health_is_ok_and_ready_waits_for_the_jwks(pool: PgPool) {
    let app = TestApp::start(pool).await;
    assert_eq!(
        app.call(Method::GET, "/health", None, None).await.status,
        StatusCode::OK
    );

    let not_ready = app.call(Method::GET, "/ready", None, None).await;
    assert_eq!(not_ready.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(not_ready.body["code"], "unavailable");
    assert!(
        not_ready.body["detail"]
            .as_str()
            .is_some_and(|d| d.contains("jwks"))
    );

    app.state
        .auth
        .prefetch()
        .await
        .unwrap_or_else(|e| unreachable!("prefetch: {e}"));
    assert_eq!(
        app.call(Method::GET, "/ready", None, None).await.status,
        StatusCode::OK
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn request_ids_are_generated_propagated_and_sanitized(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let generated = app.call(Method::GET, "/health", None, None).await;
    let id = generated
        .headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert_eq!(
        uuid::Uuid::parse_str(id).map(|u| u.get_version_num()).ok(),
        Some(7)
    );

    let kept_id = "0190d5a6-0000-7000-8000-00000000abcd";
    let req = build(
        get("/health").header("x-request-id", kept_id),
        Body::empty(),
    );
    assert_eq!(
        app.send(req)
            .await
            .headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok()),
        Some(kept_id)
    );

    let req = build(
        get("/health").header("x-request-id", "<script>"),
        Body::empty(),
    );
    let replaced = app.send(req).await;
    assert_ne!(
        replaced
            .headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok()),
        Some("<script>")
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unknown_route_is_a_not_found_problem(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let res = app.call(Method::GET, "/api/v1/nope", None, None).await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
    assert_eq!(res.body["code"], "not_found");
    assert!(res.body["request_id"].is_string());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn cors_preflight_allows_exactly_the_listed_headers(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let req = build(
        Request::builder()
            .method(Method::OPTIONS)
            .uri("/api/v1/_test/echo")
            .header(header::ORIGIN, "https://app.postit.test")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .header(
                header::ACCESS_CONTROL_REQUEST_HEADERS,
                "authorization,idempotency-key",
            ),
        Body::empty(),
    );
    let res = app.send(req).await;
    let allow = res
        .headers
        .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_lowercase();
    for h in [
        "authorization",
        "content-type",
        "x-postit-act-as",
        "if-match",
        "if-none-match",
        "idempotency-key",
    ] {
        assert!(allow.contains(h), "missing {h} in {allow}");
    }
    assert!(
        res.headers
            .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
            .is_none()
    );

    let ok = build(
        get("/health").header(header::ORIGIN, "https://app.postit.test"),
        Body::empty(),
    );
    let res = app.send(ok).await;
    let expose = res
        .headers
        .get(header::ACCESS_CONTROL_EXPOSE_HEADERS)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_lowercase();
    for h in ["etag", "location", "retry-after", "x-request-id"] {
        assert!(expose.contains(h), "missing {h} in {expose}");
    }

    let evil = build(
        get("/health").header(header::ORIGIN, "https://evil.test"),
        Body::empty(),
    );
    assert!(
        app.send(evil)
            .await
            .headers
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none()
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn oversized_body_is_payload_too_large(pool: PgPool) {
    let app = TestApp::start_with(pool, |s| s.body_limit = 16).await;
    let req = build(
        Request::post("/api/v1/_test/echo"),
        Body::from(vec![b'x'; 64]),
    );
    let res = app.send(req).await;
    assert_eq!(res.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(res.body["code"], "payload_too_large");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn slow_request_is_request_timeout(pool: PgPool) {
    let app = TestApp::start_with(pool, |s| s.request_timeout = Duration::from_millis(100)).await;
    let res = app
        .call(Method::GET, "/api/v1/_test/sleep?ms=2000", None, None)
        .await;
    assert_eq!(res.status, StatusCode::REQUEST_TIMEOUT);
    assert_eq!(res.body["code"], "request_timeout");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unauthenticated_requests_share_a_per_ip_bucket(pool: PgPool) {
    let app = TestApp::start_with(pool, |s| {
        s.rate_limit.unauthenticated = RateBucket {
            rate_per_minute: 1,
            burst: 2,
        };
    })
    .await;
    assert_eq!(
        app.call(Method::GET, "/health", None, None).await.status,
        StatusCode::OK
    );
    assert_eq!(
        app.call(Method::GET, "/health", None, None).await.status,
        StatusCode::OK
    );
    let limited = app.call(Method::GET, "/health", None, None).await;
    assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(limited.headers.get(header::RETRY_AFTER).is_some());

    let mut other = build(get("/health"), Body::empty());
    other
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 50], 1))));
    assert_eq!(
        app.send(other).await.status,
        StatusCode::OK,
        "another IP has its own bucket"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_junk_authorization_header_does_not_bypass_the_ip_bucket(pool: PgPool) {
    // Public routes never authenticate, so a bearer there must still be charged per IP;
    // so must an empty bearer and a non-Bearer scheme.
    let app = TestApp::start_with(pool, |s| {
        s.rate_limit.unauthenticated = RateBucket {
            rate_per_minute: 1,
            burst: 2,
        };
    })
    .await;
    let with_auth = |value: &str| {
        build(
            get("/health").header(header::AUTHORIZATION, value),
            Body::empty(),
        )
    };
    assert_eq!(app.send(with_auth("Bearer x")).await.status, StatusCode::OK);
    assert_eq!(
        app.send(with_auth("Bearer    ")).await.status,
        StatusCode::OK
    );
    assert_eq!(
        app.send(with_auth("Basic eDp5")).await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        app.send(with_auth("Bearer x")).await.status,
        StatusCode::TOO_MANY_REQUESTS,
        "a blocked IP is refused again (the pre-handler refusal is proven in the marked-route test)"
    );
}

#[test]
fn a_wildcard_cors_origin_is_a_config_error() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config");
    let mut settings = postit_config::load_with(
        postit_config::Environment::Development,
        &dir,
        [
            ("POSTIT__DATABASE__USERNAME".to_string(), "u".to_string()),
            ("POSTIT__DATABASE__PASSWORD".to_string(), "p".to_string()),
            ("POSTIT__AUDIT__PSEUDONYM_KEY".to_string(), "k".to_string()),
        ],
    )
    .unwrap_or_else(|e| unreachable!("load: {e}"));
    settings.cors.allowed_origins = vec!["*".into()];
    let err =
        postit_api::ApiSettings::from_settings(postit_config::Environment::Development, &settings)
            .err();
    assert!(err.is_some_and(|e| e.contains("cors.allowed_origins")));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn forwarded_for_is_honored_only_from_a_trusted_proxy(pool: PgPool) {
    let app = TestApp::start_with(pool, |s| {
        s.rate_limit.unauthenticated = RateBucket {
            rate_per_minute: 1,
            burst: 1,
        };
        s.trusted_proxies = postit_api::client_ip::parse_trusted_proxies(&["10.0.0.0/8".into()])
            .unwrap_or_default();
    })
    .await;
    let via = |peer: [u8; 4], client: &str| {
        let mut req = build(
            get("/health").header("x-forwarded-for", client),
            Body::empty(),
        );
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from((peer, 1))));
        req
    };
    assert_eq!(
        app.send(via([10, 0, 0, 2], "192.0.2.1")).await.status,
        StatusCode::OK
    );
    assert_eq!(
        app.send(via([10, 0, 0, 2], "192.0.2.2")).await.status,
        StatusCode::OK,
        "different client behind the proxy"
    );
    assert_eq!(
        app.send(via([10, 0, 0, 2], "192.0.2.1")).await.status,
        StatusCode::TOO_MANY_REQUESTS
    );

    // An untrusted peer's X-Forwarded-For is ignored: both requests are charged to the peer.
    assert_eq!(
        app.send(via([203, 0, 113, 7], "192.0.2.9")).await.status,
        StatusCode::OK
    );
    assert_eq!(
        app.send(via([203, 0, 113, 7], "192.0.2.10")).await.status,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn authenticated_requests_are_never_charged_and_a_blocked_ip_never_reaches_the_handler(
    pool: PgPool,
) {
    let app = TestApp::start_with(pool, |s| {
        s.rate_limit.unauthenticated = RateBucket {
            rate_per_minute: 1,
            burst: 1,
        };
    })
    .await;
    let marked = || {
        build(
            get("/api/v1/_test/marked").header(header::AUTHORIZATION, "Bearer ok"),
            Body::empty(),
        )
    };
    // (i) Marked requests never touch the IP bucket, however many there are.
    for _ in 0..5 {
        assert_eq!(app.send(marked()).await.status, StatusCode::OK);
    }
    // (ii) Block the IP with unmarked junk-token requests (bucket burst is 1).
    let junk = || {
        build(
            get("/health").header(header::AUTHORIZATION, "Bearer junk"),
            Body::empty(),
        )
    };
    let mut blocked = false;
    for _ in 0..3 {
        blocked |= app.send(junk()).await.status == StatusCode::TOO_MANY_REQUESTS;
    }
    assert!(blocked, "junk tokens must exhaust the IP bucket");
    let before = postit_api::routes::testing::marked_hits();
    let refused = app.send(marked()).await;
    assert_eq!(refused.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        postit_api::routes::testing::marked_hits(),
        before,
        "a blocked IP is refused before the handler runs"
    );
}
