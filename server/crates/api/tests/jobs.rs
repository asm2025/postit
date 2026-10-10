//! Admin job console read endpoints.

use axum::http::{Method, StatusCode};
use postit_api::testkit::{ADMIN_SUB, TestApp};
use sqlx::PgPool;

/// Signs `sub` in (provisioned `pending`) and, when `approve`, has the admin set it `active`.
async fn user_token(app: &TestApp, sub: &str, approve: bool) -> String {
    let token = app.token(sub);
    let me = app
        .call(Method::GET, "/api/v1/me", Some(&token), None)
        .await;
    if approve {
        let id = me.body["id"].as_str().unwrap_or_default().to_string();
        let res = app
            .call(
                Method::PATCH,
                &format!("/api/v1/users/{id}"),
                Some(&app.token(ADMIN_SUB)),
                Some(serde_json::json!({ "status": "active" })),
            )
            .await;
        assert_eq!(res.status, StatusCode::OK);
    }
    token
}

#[sqlx::test(migrations = "../data/migrations")]
async fn members_pending_users_and_anonymous_callers_are_refused_on_read_routes(pool: PgPool) {
    let app = TestApp::start(pool).await;
    app.call(Method::GET, "/api/v1/me", Some(&app.token(ADMIN_SUB)), None)
        .await; // bootstrap admin first
    let member = user_token(&app, "mallory", true).await;
    let pending = user_token(&app, "pat", false).await;
    let id = uuid::Uuid::now_v7();
    let routes = [
        "/api/v1/admin/jobs".to_string(),
        "/api/v1/admin/jobs/stats".to_string(),
        format!("/api/v1/admin/jobs/{id}"),
        "/api/v1/admin/jobs/recurring".to_string(),
    ];
    for path in routes {
        let anon = app.call(Method::GET, &path, None, None).await;
        assert_eq!(anon.status, StatusCode::UNAUTHORIZED, "{path}");
        for token in [&member, &pending] {
            let denied = app.call(Method::GET, &path, Some(token), None).await;
            assert_eq!(denied.status, StatusCode::FORBIDDEN, "{path}");
        }
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn list_rejects_bad_filters_with_422(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    for query in [
        "state=nope",
        "from=2026-02-01T00:00:00Z&to=2026-01-01T00:00:00Z",
        "page=0",
        "page_size=101",
        "bogus=1",
    ] {
        let res = app
            .call(
                Method::GET,
                &format!("/api/v1/admin/jobs?{query}"),
                Some(&admin),
                None,
            )
            .await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{query}");
        assert_eq!(res.body["code"], "validation_failed");
    }
    for query in ["window_hours=0", "window_hours=169"] {
        let res = app
            .call(
                Method::GET,
                &format!("/api/v1/admin/jobs/stats?{query}"),
                Some(&admin),
                None,
            )
            .await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{query}");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn an_unknown_job_id_is_404(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    let res = app
        .call(
            Method::GET,
            &format!("/api/v1/admin/jobs/{}", uuid::Uuid::now_v7()),
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(
        (res.status, res.body["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("not_found"))
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn list_filters_paginates_and_detail_maps_actions(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let admin = app.token(ADMIN_SUB);
    let mut dead_id = uuid::Uuid::nil();
    for i in 0..3 {
        let id = uuid::Uuid::now_v7();
        if i > 0 {
            dead_id = id;
        }
        sqlx::query("INSERT INTO jobs (id, job_type, queue, payload, state, run_at, finished_at) VALUES ($1, 'send_email', 'mail', '{}', $2, now(), CASE WHEN $2 = 'succeeded' THEN now() END)")
            .bind(id)
            .bind(if i == 0 { "succeeded" } else { "dead" })
            .execute(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let dead = app
        .call(
            Method::GET,
            "/api/v1/admin/jobs?state=dead&page_size=1&page=2",
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(dead.status, StatusCode::OK);
    assert_eq!(dead.body["total"], 2);
    assert_eq!(dead.body["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(dead.body["data"][0]["state"], "dead");

    let detail = app
        .call(
            Method::GET,
            &format!("/api/v1/admin/jobs/{dead_id}"),
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(detail.status, StatusCode::OK);
    assert_eq!(detail.body["job"]["state"], "dead");
    assert_eq!(detail.body["job"]["actions"]["retry"], true);
    assert_eq!(detail.body["job"]["actions"]["cancel"], false);
    assert!(detail.body["attempts"].is_array());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn stats_and_recurring_return_the_documented_shapes(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    let stats = app
        .call(Method::GET, "/api/v1/admin/jobs/stats", Some(&admin), None)
        .await;
    assert_eq!(stats.status, StatusCode::OK);
    assert!(stats.body["states"].is_array() && stats.body["throughput"].is_array());
    assert_eq!(stats.body["window_hours"], 24);
    let recurring = app
        .call(
            Method::GET,
            "/api/v1/admin/jobs/recurring",
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(recurring.status, StatusCode::OK);
    assert!(recurring.body.is_array());
}
