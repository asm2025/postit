use axum::http::{Method, StatusCode};
use postit_api::testkit::{ADMIN_SUB, TestApp};
use postit_core::UserId;
use sqlx::PgPool;

#[sqlx::test(migrations = "../data/migrations")]
async fn audit_lists_filters_and_marks_pseudonyms(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let admin = app.token(ADMIN_SUB);
    let me = app
        .call(Method::GET, "/api/v1/me", Some(&admin), None)
        .await;
    let admin_id = me.body["id"].as_str().unwrap_or_default().to_string();

    // A row referencing a pseudonymized (UUID v8) user, as delete_user leaves behind. Written
    // with raw SQL because AuditLog::record refuses references to users that do not exist.
    let pseudo = postit_data::pseudonym::pseudonym_for(b"k", UserId::from(uuid::Uuid::now_v7()));
    sqlx::query(
        "INSERT INTO audit_events (id, actor_user_id, subject_user_id, kind, details)
         VALUES ($1, $2, $3, 'user_deleted', jsonb_build_object('other_user_id', $2::text))",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(pseudo)
    .bind(uuid::Uuid::parse_str(&admin_id).unwrap_or_else(|e| unreachable!("{e}")))
    .execute(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("insert: {e}"));

    let all = app
        .call(Method::GET, "/api/v1/admin/audit", Some(&admin), None)
        .await;
    assert_eq!(all.status, StatusCode::OK);
    let kinds: Vec<&str> = all.body["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| e["kind"].as_str())
        .collect();
    assert!(kinds.contains(&"user_provisioned"));
    assert!(kinds.contains(&"bootstrap_admin_granted"));

    let deleted = app
        .call(
            Method::GET,
            "/api/v1/admin/audit?kind=user_deleted",
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(deleted.body["total"], 1);
    let event = &deleted.body["data"][0];
    assert_eq!(event["actor"]["deleted"], true);
    assert_eq!(event["subject"]["deleted"], false);
    assert_eq!(event["details"]["other_user_id"]["deleted"], true);

    let by_subject = app
        .call(
            Method::GET,
            &format!("/api/v1/admin/audit?subject_user_id={admin_id}"),
            Some(&admin),
            None,
        )
        .await;
    assert!(by_subject.body["total"].as_u64().is_some_and(|t| t >= 2));

    let future = app
        .call(
            Method::GET,
            "/api/v1/admin/audit?from=2999-01-01T00:00:00Z",
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(future.body["total"], 0);
    let past = app
        .call(
            Method::GET,
            "/api/v1/admin/audit?to=2000-01-01T00:00:00Z",
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(past.body["total"], 0);
    // An offset must be percent-encoded in a query string (`+` would decode as a space).
    let offset = app
        .call(
            Method::GET,
            "/api/v1/admin/audit?to=2999-01-01T00:00:00%2B02:00",
            Some(&admin),
            None,
        )
        .await;
    assert_eq!(offset.status, StatusCode::OK);
    assert!(offset.body["total"].as_u64().is_some_and(|t| t >= 3));

    for bad in [
        "/api/v1/admin/audit?kind=nope",
        "/api/v1/admin/audit?from=2020-01-02T00:00:00Z&to=2020-01-01T00:00:00Z",
        "/api/v1/admin/audit?from=yesterday",
    ] {
        let res = app.call(Method::GET, bad, Some(&admin), None).await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
        assert_eq!(res.body["code"], "validation_failed");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn audit_is_admin_only(pool: PgPool) {
    let app = TestApp::start(pool).await;
    app.call(Method::GET, "/api/v1/me", Some(&app.token(ADMIN_SUB)), None)
        .await;
    let res = app
        .call(
            Method::GET,
            "/api/v1/admin/audit",
            Some(&app.token("bob")),
            None,
        )
        .await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);
}
