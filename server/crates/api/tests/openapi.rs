use axum::http::{Method, StatusCode};
use postit_api::testkit::TestApp;
use postit_config::Environment;
use sqlx::PgPool;

#[test]
fn spec_lists_every_route_and_the_oauth_scheme() {
    let spec = serde_json::to_value(postit_api::openapi::openapi()).unwrap_or_default();
    for path in [
        "/api/v1/auth/config",
        "/api/v1/me",
        "/api/v1/users",
        "/api/v1/users/{id}",
        "/api/v1/admin/audit",
        "/health",
        "/ready",
    ] {
        assert!(spec["paths"].get(path).is_some(), "missing {path}");
    }
    assert!(
        spec["paths"].get("/api/v1/_test/echo").is_none(),
        "test routes stay out of the contract"
    );
    let flow = &spec["components"]["securitySchemes"]["oidc"]["flows"]["authorizationCode"];
    assert_eq!(flow["authorizationUrl"], "https://idp.invalid/authorize");
    assert_eq!(flow["tokenUrl"], "https://idp.invalid/token");
}

#[test]
fn spec_carries_the_problem_schema_and_every_code() {
    let spec = serde_json::to_value(postit_api::openapi::openapi()).unwrap_or_default();
    let schemas = &spec["components"]["schemas"];
    for name in [
        "ProblemDetails",
        "ErrorCode",
        "AuditEventDto",
        "UserRef",
        "Page_UserDto",
    ] {
        assert!(schemas.get(name).is_some(), "missing schema {name}");
    }
    let codes = schemas["ErrorCode"]["enum"].as_array().map_or(0, Vec::len);
    assert_eq!(codes, postit_api::error::ErrorCode::ALL.len());
    let delete_me = &spec["paths"]["/api/v1/me"]["delete"]["responses"];
    for status in ["202", "401", "403", "409", "422", "429", "503"] {
        assert!(
            delete_me.get(status).is_some(),
            "DELETE /me missing {status}"
        );
    }
}

#[test]
fn committed_spec_is_current() {
    let committed = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../api/openapi.json"
    ))
    .unwrap_or_default()
    .replace("\r\n", "\n");
    assert_eq!(
        committed,
        postit_api::openapi::openapi_json_pretty(),
        "run `cargo xtask openapi`"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn served_spec_carries_the_discovery_urls(pool: PgPool) {
    let app = TestApp::start(pool).await;
    app.state
        .auth
        .prefetch()
        .await
        .unwrap_or_else(|e| unreachable!("prefetch: {e}"));
    let res = app.call(Method::GET, "/api/openapi.json", None, None).await;
    assert_eq!(res.status, StatusCode::OK);
    let flow = &res.body["components"]["securitySchemes"]["oidc"]["flows"]["authorizationCode"];
    assert!(
        flow["authorizationUrl"]
            .as_str()
            .is_some_and(|u| u.ends_with("/authorize") && !u.contains("idp.invalid"))
    );
    assert!(
        flow["tokenUrl"]
            .as_str()
            .is_some_and(|u| u.ends_with("/token") && !u.contains("idp.invalid"))
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn docs_are_served_outside_production_only(pool: PgPool) {
    let dev = TestApp::start(pool.clone()).await;
    let res = dev.call(Method::GET, "/docs/", None, None).await;
    assert_eq!(res.status, StatusCode::OK);

    let prod = TestApp::start_with(pool, |s| s.environment = Environment::Production).await;
    assert_eq!(
        prod.call(Method::GET, "/docs/", None, None).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        prod.call(Method::GET, "/api/openapi.json", None, None)
            .await
            .status,
        StatusCode::OK
    );
}
