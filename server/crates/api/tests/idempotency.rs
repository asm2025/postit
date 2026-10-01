use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use postit_api::testkit::{ADMIN_SUB, TestApp};
use sqlx::PgPool;

fn post(token: &str, key: Option<&str>, body: &str) -> Request<Body> {
    let mut b = Request::post("/api/v1/_test/idempotent")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(key) = key {
        b = b.header("idempotency-key", key);
    }
    b.body(Body::from(body.to_string()))
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_replayed_key_returns_the_stored_response(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let first = app.send(post(&token, Some("k1"), r#"{"n":1}"#)).await;
    assert_eq!(first.status, StatusCode::CREATED);
    let replay = app.send(post(&token, Some("k1"), r#"{"n":1}"#)).await;
    assert_eq!(replay.status, StatusCode::CREATED);
    assert_eq!(
        replay.body, first.body,
        "same run id means the handler did not run again"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_key_reused_with_a_different_body_is_rejected(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    app.send(post(&token, Some("k1"), r#"{"n":1}"#)).await;
    let reused = app.send(post(&token, Some("k1"), r#"{"n":2}"#)).await;
    assert_eq!(reused.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reused.body["code"], "idempotency_key_reused");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn concurrent_requests_with_one_key_run_once(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    app.call(Method::GET, "/api/v1/me", Some(&token), None)
        .await; // provision first
    // The test route sleeps 300 ms inside the idempotent section when body has "slow".
    let (a, b) = tokio::join!(
        app.send(post(&token, Some("k2"), r#"{"slow":true}"#)),
        app.send(post(&token, Some("k2"), r#"{"slow":true}"#)),
    );
    let mut statuses = [a.status, b.status];
    statuses.sort();
    assert_eq!(statuses, [StatusCode::CREATED, StatusCode::CONFLICT]);
    let conflict = if a.status == StatusCode::CONFLICT {
        a
    } else {
        b
    };
    assert_eq!(conflict.body["code"], "idempotency_in_progress");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn the_same_key_for_two_users_runs_twice(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let admin = app.token(ADMIN_SUB);
    let bob = app.token("bob");
    let me = app.call(Method::GET, "/api/v1/me", Some(&bob), None).await;
    let bob_id = me.body["id"].as_str().unwrap_or_default().to_string();
    app.call(
        Method::PATCH,
        &format!("/api/v1/users/{bob_id}"),
        Some(&admin),
        Some(serde_json::json!({"status":"active"})),
    )
    .await;

    let a = app.send(post(&admin, Some("same"), r#"{"n":1}"#)).await;
    let b = app.send(post(&bob, Some("same"), r#"{"n":1}"#)).await;
    assert_eq!(a.status, StatusCode::CREATED);
    assert_eq!(b.status, StatusCode::CREATED);
    assert_ne!(a.body["run"], b.body["run"]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_failed_run_frees_the_key(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let failed = app.send(post(&token, Some("k3"), r#"{"fail":true}"#)).await;
    assert_eq!(failed.status, StatusCode::UNPROCESSABLE_ENTITY);
    let retry = app.send(post(&token, Some("k3"), r#"{"fail":true}"#)).await;
    assert_eq!(
        retry.body["code"], "validation_failed",
        "the key was released, not replayed"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn if_match_is_required_and_checked(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let put = |if_match: Option<&str>| {
        let mut b = Request::put("/api/v1/_test/versioned")
            .header(header::AUTHORIZATION, format!("Bearer {token}"));
        if let Some(v) = if_match {
            b = b.header(header::IF_MATCH, v);
        }
        b.body(Body::empty())
            .unwrap_or_else(|e| unreachable!("{e}"))
    };
    let missing = app.send(put(None)).await;
    assert_eq!(missing.status, StatusCode::PRECONDITION_REQUIRED);
    assert_eq!(missing.body["code"], "precondition_required");
    let stale = app.send(put(Some("\"v6\""))).await;
    assert_eq!(stale.status, StatusCode::PRECONDITION_FAILED);
    assert_eq!(stale.body["code"], "version_conflict");
    let ok = app.send(put(Some("\"v7\""))).await;
    assert_eq!(ok.status, StatusCode::OK);
    assert_eq!(
        ok.headers.get(header::ETAG).and_then(|v| v.to_str().ok()),
        Some("\"v8\"")
    );
    // RFC 9110: `*` matches any current representation.
    assert_eq!(app.send(put(Some("*"))).await.status, StatusCode::OK);
    assert_eq!(
        app.send(put(Some("\"v1\", \"v7\""))).await.status,
        StatusCode::OK
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_request_cancelled_by_the_timeout_frees_its_key(pool: PgPool) {
    // The run sleeps 5 s; the 500 ms request timeout drops its future mid-run, so neither
    // `complete` nor the error path runs. The key must still be released rather than
    // answering 409 `idempotency_in_progress` until it expires. The first `/me` call loads
    // the JWKS and provisions the admin, so the timed request spends its budget in the run.
    let app = TestApp::start_with(pool.clone(), |s| {
        s.request_timeout = std::time::Duration::from_millis(500);
    })
    .await;
    let token = app.token(ADMIN_SUB);
    // Warm-up under a 500 ms budget can itself time out on a loaded machine; retry until it lands.
    let mut warmed = false;
    for _ in 0..10 {
        if app
            .call(Method::GET, "/api/v1/me", Some(&token), None)
            .await
            .status
            == StatusCode::OK
        {
            warmed = true;
            break;
        }
    }
    assert!(
        warmed,
        "the admin must be provisioned before the timed request"
    );
    // Non-vacuous: observe the in_progress row while the run sleeps, then the 408 and the release.
    let observer = {
        let pool = pool.clone();
        tokio::spawn(async move {
            for _ in 0..200 {
                let n: i64 = sqlx::query_scalar(
                    "SELECT COUNT(*) FROM idempotency_keys WHERE key = 'k4' AND state = 'in_progress'",
                )
                .fetch_one(&pool)
                .await
                .unwrap_or(0);
                if n == 1 {
                    return true;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            false
        })
    };
    let timed_out = app
        .send(post(&token, Some("k4"), r#"{"sleep_ms":5000}"#))
        .await;
    assert_eq!(timed_out.status, StatusCode::REQUEST_TIMEOUT);
    assert!(
        observer.await.unwrap_or(false),
        "the key must be in_progress while the run is cancelled"
    );

    let mut released = false;
    for _ in 0..100 {
        let rows: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM idempotency_keys WHERE key = 'k4'")
                .fetch_one(&pool)
                .await
                .unwrap_or_else(|e| unreachable!("count: {e}"));
        if rows == 0 {
            released = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(released, "a cancelled run must release its idempotency key");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn an_expired_key_starts_a_fresh_run(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let token = app.token(ADMIN_SUB);
    let first = app.send(post(&token, Some("k5"), r#"{"n":1}"#)).await;
    sqlx::query(
        "UPDATE idempotency_keys SET expires_at = now() - interval '1 second' WHERE key = 'k5'",
    )
    .execute(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("expire: {e}"));
    let second = app.send(post(&token, Some("k5"), r#"{"n":2}"#)).await;
    assert_eq!(
        second.status,
        StatusCode::CREATED,
        "an expired key is not `reused`"
    );
    assert_ne!(second.body["run"], first.body["run"]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn a_run_dropped_while_completing_still_completes_and_replays(pool: PgPool) {
    use postit_api::idempotency::{StoredResponse, run};
    use postit_core::UserId;
    use postit_data::OwnerScope;

    let app = TestApp::start(pool.clone()).await;
    let token = app.token(ADMIN_SUB);
    app.call(Method::GET, "/api/v1/me", Some(&token), None)
        .await; // provision
    let id: uuid::Uuid = sqlx::query_scalar("SELECT id FROM users LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("user: {e}"));
    let scope = OwnerScope::own(UserId::from(id));

    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let runs = std::sync::atomic::AtomicUsize::new(0);
    let first = run(&pool, &scope, Some("k6"), "/r", "h", || async {
        runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let _ = tx.send(());
        Ok(StoredResponse {
            status: StatusCode::CREATED,
            body: serde_json::json!({"v": 1}),
        })
    });
    // `f` signals, then the future is dropped while complete is still pending.
    tokio::select! {
        _ = first => {}
        _ = rx => {}
    }
    let mut state = String::new();
    for _ in 0..200 {
        state = sqlx::query_scalar("SELECT state::text FROM idempotency_keys WHERE key = 'k6'")
            .fetch_optional(&pool)
            .await
            .unwrap_or_else(|e| unreachable!("state: {e}"))
            .unwrap_or_default();
        if state == "completed" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(
        state, "completed",
        "the guard must not release a completed key"
    );
    let replay = run(&pool, &scope, Some("k6"), "/r", "h", || async {
        runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(StoredResponse {
            status: StatusCode::OK,
            body: serde_json::json!({"v": 2}),
        })
    })
    .await
    .unwrap_or_else(|e| unreachable!("replay: {e:?}"));
    assert_eq!(replay.body["v"], 1);
    assert_eq!(runs.load(std::sync::atomic::Ordering::SeqCst), 1);
}
