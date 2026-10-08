use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::routing::get;
use http_body_util::BodyExt;
use postit_server::web::{WebConfig, dispatch, router};
use tower::ServiceExt;

fn site() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap_or_else(|e| unreachable!("tempdir: {e}"));
    std::fs::write(dir.path().join("index.html"), "<html>app</html>").unwrap_or_default();
    std::fs::create_dir(dir.path().join("assets")).unwrap_or_default();
    std::fs::write(dir.path().join("assets/app-abc123.js"), "console.log(1)").unwrap_or_default();
    // Vite copies public/config.json into the build; the server must ignore it.
    std::fs::write(
        dir.path().join("config.json"),
        r#"{"API_BASE_URL":"stale"}"#,
    )
    .unwrap_or_default();
    dir
}

fn cfg() -> WebConfig {
    WebConfig {
        api_base_url: "https://postit.local:44310".to_string(),
        env: "development",
    }
}

async fn get_path(app: &Router, path: &str) -> (StatusCode, axum::http::HeaderMap, String) {
    let res = app
        .clone()
        .oneshot(
            Request::get(path)
                .body(Body::empty())
                .unwrap_or_else(|e| unreachable!("{e}")),
        )
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let (parts, body) = res.into_parts();
    let bytes = body
        .collect()
        .await
        .map(http_body_util::Collected::to_bytes)
        .unwrap_or_default();
    (
        parts.status,
        parts.headers,
        String::from_utf8_lossy(&bytes).into_owned(),
    )
}

#[tokio::test]
async fn deep_links_fall_back_to_index_html_with_no_cache() {
    let dir = site();
    let app = router(dir.path(), cfg());
    for path in ["/", "/users/abc", "/auth/callback?code=x&state=y"] {
        let (status, headers, body) = get_path(&app, path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert_eq!(body, "<html>app</html>", "{path}");
        assert_eq!(headers[header::CACHE_CONTROL], "no-cache", "{path}");
        assert!(headers.contains_key("x-request-id"), "{path}");
    }
}

#[tokio::test]
async fn hashed_assets_are_immutable_and_missing_assets_are_404() {
    let dir = site();
    let app = router(dir.path(), cfg());
    let (status, headers, body) = get_path(&app, "/assets/app-abc123.js").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "console.log(1)");
    assert_eq!(
        headers[header::CACHE_CONTROL],
        "public, max-age=31536000, immutable"
    );
    let (status, headers, body) = get_path(&app, "/assets/missing.js").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(!body.contains("<html>"));
    assert_eq!(headers[header::CACHE_CONTROL], "no-cache");
}

#[tokio::test]
async fn config_json_is_generated_not_read_from_root() {
    let dir = site();
    let app = router(dir.path(), cfg());
    let (status, headers, body) = get_path(&app, "/config.json").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CACHE_CONTROL], "no-cache");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    assert_eq!(
        v,
        serde_json::json!({"API_BASE_URL": "https://postit.local:44310", "ENV": "development"})
    );
}

#[tokio::test]
async fn reserved_prefixes_never_get_the_spa_fallback() {
    let dir = site();
    let app = router(dir.path(), cfg());
    for path in ["/api/v1/nope", "/api", "/docs/x", "/health", "/ready"] {
        let (status, _, body) = get_path(&app, path).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        assert!(!body.contains("<html>"), "{path}");
    }
}

#[tokio::test]
async fn dispatch_sends_reserved_paths_to_the_api_router() {
    let dir = site();
    let api = Router::new()
        .route("/health", get(|| async { "ok" }))
        .fallback(|| async {
            (
                StatusCode::NOT_FOUND,
                [(header::CONTENT_TYPE, "application/problem+json")],
                r#"{"code":"not_found"}"#,
            )
        });
    let app = dispatch(api, router(dir.path(), cfg()));
    let (status, _, body) = get_path(&app, "/health").await;
    assert_eq!((status, body.as_str()), (StatusCode::OK, "ok"));
    let (status, headers, _) = get_path(&app, "/api/v1/nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(headers[header::CONTENT_TYPE], "application/problem+json");
    let (status, _, body) = get_path(&app, "/users/1").await;
    assert_eq!(
        (status, body.as_str()),
        (StatusCode::OK, "<html>app</html>")
    );
}
