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
    for path in [
        "/api/v1/ai/providers",
        "/api/v1/ai/providers/{id}",
        "/api/v1/ai/generate",
    ] {
        // The `ai` feature compiles the AI routes, and the contract lists them only then.
        #[cfg(feature = "ai")]
        assert!(spec["paths"].get(path).is_some(), "missing {path}");
        #[cfg(not(feature = "ai"))]
        assert!(spec["paths"].get(path).is_none(), "{path} without `ai`");
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

/// Strict client generators (the mobile app's `swagger_parser`) reject a response without a
/// `description`, which `OpenAPI` requires, and the `allOf` utoipa wraps around a documented
/// inline field.
#[test]
fn spec_is_valid_for_strict_generators() {
    fn walk(node: &serde_json::Value, path: &str, bad: &mut Vec<String>) {
        match node {
            serde_json::Value::Object(map) => {
                // utoipa's wrapper for a documented `inline` field: the schema plus an object
                // branch holding only the field's description.
                let wrapper = |b: &serde_json::Value| {
                    b.as_object().is_some_and(|b| {
                        b.keys().all(|k| k == "type" || k == "description")
                            && b.get("type").is_some_and(|t| t == "object")
                    })
                };
                if let Some(branches) = map.get("allOf").and_then(|v| v.as_array())
                    && branches.iter().any(wrapper)
                {
                    bad.push(format!("{path}: allOf wraps a field description"));
                }
                for (key, value) in map {
                    walk(value, &format!("{path}/{key}"), bad);
                }
            }
            serde_json::Value::Array(items) => {
                for (i, value) in items.iter().enumerate() {
                    walk(value, &format!("{path}/{i}"), bad);
                }
            }
            _ => {}
        }
    }

    let spec = serde_json::to_value(postit_api::openapi::openapi()).unwrap_or_default();
    let mut bad = Vec::new();
    for (route, item) in spec["paths"].as_object().into_iter().flatten() {
        for (method, operation) in item.as_object().into_iter().flatten() {
            for (status, response) in operation["responses"].as_object().into_iter().flatten() {
                let described = response
                    .get("description")
                    .and_then(|d| d.as_str())
                    .is_some_and(|d| !d.is_empty());
                if !described && response.get("$ref").is_none() {
                    bad.push(format!("{method} {route} {status}: no description"));
                }
            }
        }
    }
    walk(&spec, "", &mut bad);
    assert!(bad.is_empty(), "{bad:#?}");

    // The unwrapped inline schema keeps the field's own description.
    let notification = &spec["components"]["schemas"]["NotificationDto"]["properties"];
    assert_eq!(
        notification["payload"]["description"],
        "The details of what happened; its shape depends on `kind`."
    );
    assert!(notification["payload"]["oneOf"].is_array());
}

#[test]
fn committed_spec_is_current() {
    let committed =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../openapi.json"))
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

#[test]
fn spec_types_audit_kinds_as_an_enum_and_user_refs_carry_names() {
    let spec = serde_json::to_value(postit_api::openapi::openapi()).unwrap_or_default();
    let schemas = &spec["components"]["schemas"];
    let values: Vec<&str> = schemas["AuditEventKindDto"]["enum"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str())
        .collect();
    let expected: Vec<&str> = postit_data::audit::AuditEventKind::ALL
        .iter()
        .map(|k| k.as_str())
        .collect();
    assert_eq!(values, expected);
    let kind = &schemas["AuditEventDto"]["properties"]["kind"];
    assert!(kind.to_string().contains("AuditEventKindDto"), "{kind}");
    assert!(
        schemas["UserRef"]["properties"]
            .get("display_name")
            .is_some()
    );
    let params = spec["paths"]["/api/v1/admin/audit"]["get"]["parameters"].to_string();
    assert!(params.contains("AuditEventKindDto"), "{params}");
}
