use axum::http::{Method, StatusCode};
use postit_api::testkit::{ADMIN_SUB, TestApp};
use serde_json::json;
use sqlx::PgPool;

async fn outbox_kinds(pool: &PgPool) -> Vec<String> {
    sqlx::query_scalar("SELECT job_type FROM job_outbox ORDER BY created_at")
        .fetch_all(pool)
        .await
        .unwrap_or_else(|e| unreachable!("outbox: {e}"))
}

/// `send_email` rows for one mail kind. Provisioning bob already enqueues a
/// `user_pending_approval` mail to the admin, so "any `send_email`" would prove nothing.
async fn mails_of_kind(pool: &PgPool, kind: &str) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM job_outbox WHERE job_type = 'send_email' AND payload->>'kind' = $1",
    )
    .bind(kind)
    .fetch_one(pool)
    .await
    .unwrap_or_else(|e| unreachable!("outbox: {e}"))
}

#[sqlx::test(migrations = "../data/migrations")]
async fn plan_02_p6_exit_flow(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let admin = app.token(ADMIN_SUB);

    // 1. Bootstrap admin.
    let me = app
        .call(Method::GET, "/api/v1/me", Some(&admin), None)
        .await;
    assert_eq!(
        (me.body["role"].as_str(), me.body["status"].as_str()),
        (Some("admin"), Some("active"))
    );
    let admin_id = me.body["id"].as_str().unwrap_or_default().to_string();

    // 2. A second user's first request provisions them as pending.
    let bob = app.token("bob");
    let bob_me = app.call(Method::GET, "/api/v1/me", Some(&bob), None).await;
    assert_eq!(bob_me.body["status"], "pending");
    let bob_id = bob_me.body["id"].as_str().unwrap_or_default().to_string();
    let bob_path = format!("/api/v1/users/{bob_id}");

    // 3. 403 account_pending on every route except GET /me.
    for (method, path, body) in [
        (Method::GET, "/api/v1/users".to_string(), None),
        (Method::GET, bob_path.clone(), None),
        (
            Method::PATCH,
            bob_path.clone(),
            Some(json!({ "role": "admin" })),
        ),
        (Method::DELETE, bob_path.clone(), None),
        (Method::GET, "/api/v1/admin/audit".to_string(), None),
        (
            Method::DELETE,
            "/api/v1/me".to_string(),
            Some(json!({ "display_name": "bob" })),
        ),
    ] {
        let res = app.call(method.clone(), &path, Some(&bob), body).await;
        assert_eq!(res.status, StatusCode::FORBIDDEN, "{method} {path}");
        assert_eq!(res.body["code"], "account_pending", "{method} {path}");
    }

    // 4. The admin approves; the user_approved email is enqueued.
    assert_eq!(mails_of_kind(&pool, "user_approved").await, 0);
    let approved = app
        .call(
            Method::PATCH,
            &bob_path,
            Some(&admin),
            Some(json!({ "status": "active" })),
        )
        .await;
    assert_eq!(approved.status, StatusCode::OK);
    assert_eq!(approved.body["status"], "active");
    assert_eq!(approved.body["approved_by"], admin_id.as_str());
    assert_eq!(mails_of_kind(&pool, "user_approved").await, 1);

    // 5. The user gets access (cache evicted by the approval).
    let bob_after = app.call(Method::GET, "/api/v1/me", Some(&bob), None).await;
    assert_eq!(bob_after.body["status"], "active");

    // 6. Disable, then re-enable.
    let disabled = app
        .call(
            Method::PATCH,
            &bob_path,
            Some(&admin),
            Some(json!({ "status": "disabled" })),
        )
        .await;
    assert_eq!(disabled.body["status"], "disabled");
    let blocked = app.call(Method::GET, "/api/v1/me", Some(&bob), None).await;
    assert_eq!(blocked.status, StatusCode::FORBIDDEN);
    assert_eq!(blocked.body["code"], "account_disabled");
    let enabled = app
        .call(
            Method::PATCH,
            &bob_path,
            Some(&admin),
            Some(json!({ "status": "active" })),
        )
        .await;
    assert_eq!(enabled.body["status"], "active");
    assert_eq!(
        app.call(Method::GET, "/api/v1/me", Some(&bob), None)
            .await
            .status,
        StatusCode::OK
    );

    // 7. Role change: bob becomes admin, then back to member.
    let promoted = app
        .call(
            Method::PATCH,
            &bob_path,
            Some(&admin),
            Some(json!({ "role": "admin" })),
        )
        .await;
    assert_eq!(promoted.body["role"], "admin");
    assert_eq!(
        app.call(Method::GET, "/api/v1/users", Some(&bob), None)
            .await
            .status,
        StatusCode::OK
    );
    let demoted = app
        .call(
            Method::PATCH,
            &bob_path,
            Some(&admin),
            Some(json!({ "role": "member" })),
        )
        .await;
    assert_eq!(demoted.body["role"], "member");

    // 8. Last-admin guard on demote, disable, and DELETE /me.
    let admin_path = format!("/api/v1/users/{admin_id}");
    for body in [json!({ "role": "member" }), json!({ "status": "disabled" })] {
        let res = app
            .call(Method::PATCH, &admin_path, Some(&admin), Some(body.clone()))
            .await;
        assert_eq!(res.status, StatusCode::CONFLICT, "{body}");
        assert_eq!(res.body["code"], "last_admin");
    }
    let self_delete = app
        .call(
            Method::DELETE,
            "/api/v1/me",
            Some(&admin),
            Some(json!({ "display_name": ADMIN_SUB })),
        )
        .await;
    assert_eq!(self_delete.body["code"], "last_admin");

    // 9. Delete bob: 202, then bob is locked out (deleting) and status changes are refused.
    let deleted = app
        .call(Method::DELETE, &bob_path, Some(&admin), None)
        .await;
    assert_eq!(deleted.status, StatusCode::ACCEPTED);
    let gone = app.call(Method::GET, "/api/v1/me", Some(&bob), None).await;
    assert_eq!(gone.body["code"], "account_disabled");
    let refused = app
        .call(
            Method::PATCH,
            &bob_path,
            Some(&admin),
            Some(json!({ "status": "active" })),
        )
        .await;
    assert_eq!(refused.status, StatusCode::CONFLICT);
    assert_eq!(refused.body["code"], "user_deleting");
    assert!(outbox_kinds(&pool).await.iter().any(|k| k == "delete_user"));

    // 10. The audit list shows the flow's events.
    let audit = app
        .call(
            Method::GET,
            "/api/v1/admin/audit?page_size=100",
            Some(&admin),
            None,
        )
        .await;
    let kinds: Vec<&str> = audit.body["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| e["kind"].as_str())
        .collect();
    for kind in [
        "user_provisioned",
        "bootstrap_admin_granted",
        "user_approved",
        "user_disabled",
        "user_enabled",
        "role_changed",
        "user_deleted",
    ] {
        assert!(kinds.contains(&kind), "audit is missing {kind}: {kinds:?}");
    }
    let by_actor = app
        .call(
            Method::GET,
            &format!("/api/v1/admin/audit?actor_user_id={admin_id}&kind=role_changed"),
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(by_actor.body["total"], 2);
}
