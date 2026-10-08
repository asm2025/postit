use axum::http::{Method, StatusCode};
use postit_api::testkit::{ADMIN_SUB, TestApp};
use serde_json::json;
use sqlx::PgPool;

async fn signed_up(app: &TestApp, sub: &str) -> String {
    let res = app
        .call(Method::GET, "/api/v1/me", Some(&app.token(sub)), None)
        .await;
    assert_eq!(res.status, StatusCode::OK);
    res.body["id"].as_str().unwrap_or_default().to_string()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn members_cannot_use_admin_routes(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    let bob_id = signed_up(&app, "bob").await;
    app.call(
        Method::PATCH,
        &format!("/api/v1/users/{bob_id}"),
        Some(&admin),
        Some(json!({ "status": "active" })),
    )
    .await;

    let res = app
        .call(Method::GET, "/api/v1/users", Some(&app.token("bob")), None)
        .await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);
    assert_eq!(res.body["code"], "forbidden");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn pending_users_get_account_pending(pool: PgPool) {
    let app = TestApp::start(pool).await;
    signed_up(&app, "bob").await;
    let res = app
        .call(Method::GET, "/api/v1/users", Some(&app.token("bob")), None)
        .await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);
    assert_eq!(res.body["code"], "account_pending");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn list_filters_searches_and_paginates(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    app.call(Method::GET, "/api/v1/me", Some(&admin), None)
        .await;
    for sub in ["carol", "dave", "erin"] {
        signed_up(&app, sub).await;
    }

    let pending = app
        .call(
            Method::GET,
            "/api/v1/users?status=pending&page_size=2",
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(pending.status, StatusCode::OK);
    assert_eq!(pending.body["total"], 3);
    assert_eq!(pending.body["data"].as_array().map(Vec::len), Some(2));
    assert_eq!(pending.body["page"], 1);
    assert_eq!(pending.body["page_size"], 2);

    let search = app
        .call(Method::GET, "/api/v1/users?search=dav", Some(&admin), None)
        .await;
    assert_eq!(search.body["total"], 1);

    for bad in [
        "/api/v1/users?status=banned",
        "/api/v1/users?page=0",
        "/api/v1/users?page=1000001",
        "/api/v1/users?page_size=101",
        "/api/v1/users?page_size=abc",
    ] {
        let res = app.call(Method::GET, bad, Some(&admin), None).await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
        assert_eq!(res.body["code"], "validation_failed");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unknown_query_parameters_are_rejected_without_echo(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    for path in [
        "/api/v1/users?zzsecretkey=zzsecretvalue",
        "/api/v1/admin/audit?zzsecretkey=zzsecretvalue",
    ] {
        let res = app.call(Method::GET, path, Some(&admin), None).await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{path}");
        assert_eq!(res.body["code"], "validation_failed");
        let text = res.body.to_string();
        assert!(!text.contains("zzsecret"), "{path}: {text}");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn get_user_returns_404_for_unknown_ids(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    let res = app
        .call(
            Method::GET,
            &format!("/api/v1/users/{}", uuid::Uuid::now_v7()),
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
    let bad = app
        .call(Method::GET, "/api/v1/users/not-a-uuid", Some(&admin), None)
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!bad.body.to_string().contains("not-a-uuid"));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn patch_requires_exactly_one_field_and_changes_nothing_otherwise(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    app.call(Method::GET, "/api/v1/me", Some(&admin), None)
        .await;
    let bob = signed_up(&app, "bob").await;
    let path = format!("/api/v1/users/{bob}");

    for body in [
        json!({}),
        json!({ "status": "active", "role": "admin" }),
        json!({ "status": "active", "extra": 1 }),
    ] {
        let res = app
            .call(Method::PATCH, &path, Some(&admin), Some(body.clone()))
            .await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    }
    let after = app.call(Method::GET, &path, Some(&admin), None).await;
    assert_eq!(after.body["status"], "pending");
    assert_eq!(after.body["role"], "member");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn patch_rejects_disallowed_transitions(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    app.call(Method::GET, "/api/v1/me", Some(&admin), None)
        .await;
    let bob = signed_up(&app, "bob").await;
    let path = format!("/api/v1/users/{bob}");
    for status in ["pending", "deleting", "disabled"] {
        let res = app
            .call(
                Method::PATCH,
                &path,
                Some(&admin),
                Some(json!({ "status": status })),
            )
            .await;
        assert_eq!(
            res.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "pending -> {status}"
        );
    }
    let role_on_pending = app
        .call(
            Method::PATCH,
            &path,
            Some(&admin),
            Some(json!({ "role": "admin" })),
        )
        .await;
    assert_eq!(role_on_pending.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn patch_errors_never_echo_client_input(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    let bob = signed_up(&app, "bob").await;
    let path = format!("/api/v1/users/{bob}");
    for body in [
        json!({ "status": "zz<hacked>" }),
        json!({ "role": "zz<hacked>" }),
        json!({ "status": 12_345 }),
        json!({ "zz<hacked>": 1 }),
    ] {
        let res = app
            .call(Method::PATCH, &path, Some(&admin), Some(body))
            .await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(!res.body.to_string().contains("hacked"), "{}", res.body);
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn approve_then_role_change_works_and_takes_effect(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    let bob = signed_up(&app, "bob").await;
    let path = format!("/api/v1/users/{bob}");
    let res = app
        .call(
            Method::PATCH,
            &path,
            Some(&admin),
            Some(json!({ "status": "active" })),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body["status"], "active");
    let res = app
        .call(
            Method::PATCH,
            &path,
            Some(&admin),
            Some(json!({ "role": "admin" })),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(res.body["role"], "admin");
    let list = app
        .call(Method::GET, "/api/v1/users", Some(&app.token("bob")), None)
        .await;
    assert_eq!(list.status, StatusCode::OK);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn an_admin_cannot_delete_themselves_through_users(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    let me = app
        .call(Method::GET, "/api/v1/me", Some(&admin), None)
        .await;
    let id = me.body["id"].as_str().unwrap_or_default();
    let res = app
        .call(
            Method::DELETE,
            &format!("/api/v1/users/{id}"),
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);
    assert_eq!(res.body["code"], "forbidden");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn list_filters_by_role_alone_and_combined(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    app.call(Method::GET, "/api/v1/me", Some(&admin), None)
        .await;
    signed_up(&app, "carol").await;

    let call = |path: &'static str| app.call(Method::GET, path, Some(&admin), None);

    let admins = call("/api/v1/users?role=admin").await;
    assert_eq!(admins.status, StatusCode::OK);
    assert_eq!(admins.body["total"], 1);
    assert_eq!(admins.body["data"][0]["role"], "admin");

    let active_admins = call("/api/v1/users?role=admin&status=active").await;
    assert_eq!(active_admins.body["total"], 1);

    let pending_admins = call("/api/v1/users?role=admin&status=pending").await;
    assert_eq!(pending_admins.body["total"], 0);

    let members = call("/api/v1/users?role=member").await;
    assert_eq!(members.body["total"], 1);
    assert_eq!(members.body["data"][0]["role"], "member");

    let carol = call("/api/v1/users?role=member&search=carol").await;
    assert_eq!(carol.body["total"], 1);
    let nobody = call("/api/v1/users?role=admin&search=carol").await;
    assert_eq!(nobody.body["total"], 0);

    let bad = call("/api/v1/users?role=root").await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(bad.body["code"], "validation_failed");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn list_sorts_by_created_at_in_both_directions(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    app.call(Method::GET, "/api/v1/me", Some(&admin), None)
        .await;
    let first = signed_up(&app, "first").await;
    let second = signed_up(&app, "second").await;

    let ids = |body: &serde_json::Value| -> Vec<String> {
        body["data"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter_map(|r| r["id"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };

    let oldest_first = app
        .call(
            Method::GET,
            "/api/v1/users?status=pending&sort=created_at",
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(oldest_first.status, StatusCode::OK);
    assert_eq!(ids(&oldest_first.body), vec![first.clone(), second.clone()]);

    let newest_first = app
        .call(
            Method::GET,
            "/api/v1/users?status=pending&sort=-created_at",
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(ids(&newest_first.body), vec![second.clone(), first.clone()]);

    let default = app
        .call(
            Method::GET,
            "/api/v1/users?status=pending",
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(ids(&default.body), vec![second, first]);

    let bad = app
        .call(Method::GET, "/api/v1/users?sort=name", Some(&admin), None)
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(bad.body["code"], "validation_failed");
}
