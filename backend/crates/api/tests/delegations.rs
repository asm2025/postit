//! The delegation, workspace, and user lookup routes.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use chrono::{DateTime, Duration, SubsecRound as _, Utc};
use postit_api::testkit::{ADMIN_SUB, TestApp, TestResponse};
use postit_config::RateBucket;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const ACT_AS: &str = "x-postit-act-as";

/// `days` from now, in whole seconds so it survives Postgres's microsecond precision.
fn in_days(days: i64) -> DateTime<Utc> {
    (Utc::now() + Duration::days(days)).trunc_subsecs(0)
}

/// Approves the user behind `token` through the admin API and returns their ID.
async fn approve(app: &TestApp, token: &str) -> Uuid {
    let me = app.call(Method::GET, "/api/v1/me", Some(token), None).await;
    let id = me.body["id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .unwrap_or_else(|| unreachable!("no id: {:?}", me.body));
    let approved = app
        .call(
            Method::PATCH,
            &format!("/api/v1/users/{id}"),
            Some(&app.token(ADMIN_SUB)),
            Some(json!({ "status": "active" })),
        )
        .await;
    assert_eq!(approved.status, StatusCode::OK, "{:?}", approved.body);
    id
}

/// Signs `sub` in with the verified email `sub@postit.test`, approves them, and returns their
/// token and ID.
async fn active_user(app: &TestApp, sub: &str) -> (String, Uuid) {
    let token = app.token(sub);
    let id = approve(app, &token).await;
    (token, id)
}

/// Like [`active_user`], with this email and verification flag.
async fn active_user_with(app: &TestApp, sub: &str, email: &str, verified: bool) -> (String, Uuid) {
    let token = app
        .issuer
        .token_and_claims(sub, Some(email), Some(verified), Some(sub))
        .0;
    let id = approve(app, &token).await;
    (token, id)
}

async fn send_as(
    app: &TestApp,
    method: Method,
    path: &str,
    token: &str,
    act_as: Uuid,
    body: Option<Value>,
) -> TestResponse {
    let mut b = Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(ACT_AS, act_as.to_string());
    let body = match body {
        Some(json) => {
            b = b.header(header::CONTENT_TYPE, "application/json");
            Body::from(json.to_string())
        }
        None => Body::empty(),
    };
    app.send(b.body(body).unwrap_or_else(|e| unreachable!("{e}")))
        .await
}

async fn grant(app: &TestApp, owner: &str, body: Value) -> TestResponse {
    app.call(Method::POST, "/api/v1/delegations", Some(owner), Some(body))
        .await
}

async fn granted(app: &TestApp, owner: &str, body: Value) -> Value {
    let res = grant(app, owner, body).await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    res.body
}

fn assert_code(res: &TestResponse, status: StatusCode, code: &str, what: &str) {
    assert_eq!(res.status, status, "{what}: {:?}", res.body);
    assert_eq!(res.body["code"], code, "{what}: {:?}", res.body);
}

fn time(value: &Value) -> DateTime<Utc> {
    value
        .as_str()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map_or_else(
            || unreachable!("not a timestamp: {value:?}"),
            |t| t.with_timezone(&Utc),
        )
}

fn id_of(body: &Value) -> String {
    body["id"].as_str().unwrap_or_default().to_owned()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn grant_list_change_revoke_round_trip(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (alice, alice_id) = active_user(&app, "alice").await;
    let (_, bob_id) = active_user(&app, "bob").await;
    let expiry = in_days(7);

    let created = granted(
        &app,
        &alice,
        json!({ "delegate_id": bob_id, "level": "view", "expires_at": expiry }),
    )
    .await;
    assert_eq!(created["owner"]["id"], alice_id.to_string());
    assert_eq!(created["owner"]["display_name"], "alice");
    assert_eq!(created["owner"]["status"], "active");
    assert_eq!(created["delegate"]["id"], bob_id.to_string());
    assert_eq!(created["delegate"]["display_name"], "bob");
    assert_eq!(created["delegate"]["status"], "active");
    assert_eq!(created["level"], "view");
    assert_eq!(created["all_accounts"], true);
    assert_eq!(created["account_ids"], json!([]));
    assert_eq!(time(&created["expires_at"]), expiry);
    assert_eq!(created["ended_at"], Value::Null);
    assert_eq!(created["ended_reason"], Value::Null);
    assert_eq!(created["created_by"], alice_id.to_string());
    assert_eq!(created["updated_by"], alice_id.to_string());
    let id = id_of(&created);

    let list = app
        .call(Method::GET, "/api/v1/delegations", Some(&alice), None)
        .await;
    assert_eq!(list.status, StatusCode::OK, "{:?}", list.body);
    assert_eq!(list.body["total"], 1);
    assert_eq!(list.body["data"][0]["id"], id);
    assert_eq!(list.body["data"][0]["delegate"]["display_name"], "bob");
    assert_eq!(list.body["data"][0]["owner"]["display_name"], "alice");

    let path = format!("/api/v1/delegations/{id}");
    let limited = app
        .call(
            Method::PATCH,
            &path,
            Some(&alice),
            Some(json!({ "level": "edit", "account_ids": [] })),
        )
        .await;
    assert_eq!(limited.status, StatusCode::OK, "{:?}", limited.body);
    assert_eq!(limited.body["level"], "edit");
    assert_eq!(limited.body["all_accounts"], false);
    assert_eq!(limited.body["account_ids"], json!([]));

    let all = app
        .call(
            Method::PATCH,
            &path,
            Some(&alice),
            Some(json!({ "all_accounts": true })),
        )
        .await;
    assert_eq!(all.status, StatusCode::OK, "{:?}", all.body);
    assert_eq!(all.body["all_accounts"], true);
    assert_eq!(all.body["level"], "edit");

    let revoked = app.call(Method::DELETE, &path, Some(&alice), None).await;
    assert_eq!(revoked.status, StatusCode::NO_CONTENT, "{:?}", revoked.body);
    let again = app.call(Method::DELETE, &path, Some(&alice), None).await;
    assert_eq!(again.status, StatusCode::NO_CONTENT, "{:?}", again.body);

    let live = app
        .call(Method::GET, "/api/v1/delegations", Some(&alice), None)
        .await;
    assert_eq!(live.body["total"], 0, "{:?}", live.body);
    let history = app
        .call(
            Method::GET,
            "/api/v1/delegations?include_ended=true",
            Some(&alice),
            None,
        )
        .await;
    assert_eq!(history.status, StatusCode::OK, "{:?}", history.body);
    assert_eq!(history.body["total"], 1);
    assert_eq!(history.body["data"][0]["ended_reason"], "revoked");
    assert!(history.body["data"][0]["ended_at"].is_string());

    let gone = app
        .call(
            Method::PATCH,
            &path,
            Some(&alice),
            Some(json!({ "level": "view" })),
        )
        .await;
    assert_code(
        &gone,
        StatusCode::NOT_FOUND,
        "not_found",
        "patch ended grant",
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn change_with_both_account_fields_is_validation_failed(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (alice, _) = active_user(&app, "alice").await;
    let (_, bob_id) = active_user(&app, "bob").await;
    let id = id_of(
        &granted(
            &app,
            &alice,
            json!({ "delegate_id": bob_id, "level": "view" }),
        )
        .await,
    );

    let res = app
        .call(
            Method::PATCH,
            &format!("/api/v1/delegations/{id}"),
            Some(&alice),
            Some(json!({ "all_accounts": true, "account_ids": [] })),
        )
        .await;
    assert_code(
        &res,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
        "both",
    );
    assert_eq!(res.body["field"], "account_ids");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn change_with_all_accounts_false_is_validation_failed(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (alice, _) = active_user(&app, "alice").await;
    let (_, bob_id) = active_user(&app, "bob").await;
    let id = id_of(
        &granted(
            &app,
            &alice,
            json!({ "delegate_id": bob_id, "level": "view" }),
        )
        .await,
    );

    let res = app
        .call(
            Method::PATCH,
            &format!("/api/v1/delegations/{id}"),
            Some(&alice),
            Some(json!({ "all_accounts": false })),
        )
        .await;
    assert_code(
        &res,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
        "false",
    );
    assert_eq!(res.body["field"], "all_accounts");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn empty_patch_is_a_no_op_without_an_audit_event(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (alice, _) = active_user(&app, "alice").await;
    let (_, bob_id) = active_user(&app, "bob").await;
    let created = granted(
        &app,
        &alice,
        json!({ "delegate_id": bob_id, "level": "edit" }),
    )
    .await;
    let id = id_of(&created);
    let count = || async {
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM audit_events WHERE kind = 'delegation_changed'",
        )
        .fetch_one(&app.pool)
        .await
        .unwrap_or_else(|e| unreachable!("count: {e}"))
    };

    let res = app
        .call(
            Method::PATCH,
            &format!("/api/v1/delegations/{id}"),
            Some(&alice),
            Some(json!({})),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert_eq!(res.body["level"], "edit");
    assert_eq!(res.body["id"], created["id"]);
    assert_eq!(count().await, 0, "no event for an empty change");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delegate_patch_is_owner_only_before_body_checks(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (alice, alice_id) = active_user(&app, "alice").await;
    let (bob, bob_id) = active_user(&app, "bob").await;
    let id = id_of(
        &granted(
            &app,
            &alice,
            json!({ "delegate_id": bob_id, "level": "publish" }),
        )
        .await,
    );

    for body in [
        json!({ "all_accounts": false }),
        json!({ "all_accounts": true, "account_ids": [] }),
    ] {
        let res = send_as(
            &app,
            Method::PATCH,
            &format!("/api/v1/delegations/{id}"),
            &bob,
            alice_id,
            Some(body),
        )
        .await;
        assert_code(&res, StatusCode::FORBIDDEN, "owner_only", "delegate patch");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn change_expiry_absent_null_and_set(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (alice, _) = active_user(&app, "alice").await;
    let (_, bob_id) = active_user(&app, "bob").await;
    let first = in_days(3);
    let id = id_of(
        &granted(
            &app,
            &alice,
            json!({ "delegate_id": bob_id, "level": "view", "expires_at": first }),
        )
        .await,
    );
    let path = format!("/api/v1/delegations/{id}");

    let absent = app
        .call(
            Method::PATCH,
            &path,
            Some(&alice),
            Some(json!({ "level": "edit" })),
        )
        .await;
    assert_eq!(absent.status, StatusCode::OK, "{:?}", absent.body);
    assert_eq!(time(&absent.body["expires_at"]), first, "absent keeps it");

    let cleared = app
        .call(
            Method::PATCH,
            &path,
            Some(&alice),
            Some(json!({ "expires_at": null })),
        )
        .await;
    assert_eq!(cleared.status, StatusCode::OK, "{:?}", cleared.body);
    assert_eq!(cleared.body["expires_at"], Value::Null, "null clears it");
    assert_eq!(cleared.body["level"], "edit");

    let second = in_days(9);
    let set = app
        .call(
            Method::PATCH,
            &path,
            Some(&alice),
            Some(json!({ "expires_at": second })),
        )
        .await;
    assert_eq!(set.status, StatusCode::OK, "{:?}", set.body);
    assert_eq!(time(&set.body["expires_at"]), second, "a timestamp sets it");

    let past = app
        .call(
            Method::PATCH,
            &path,
            Some(&alice),
            Some(json!({ "expires_at": Utc::now() - Duration::hours(1) })),
        )
        .await;
    assert_code(
        &past,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
        "past",
    );
    assert_eq!(past.body["field"], "expires_at");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn grant_with_all_accounts_field_is_validation_failed(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (alice, _) = active_user(&app, "alice").await;
    let (_, bob_id) = active_user(&app, "bob").await;

    let res = grant(
        &app,
        &alice,
        json!({ "delegate_id": bob_id, "level": "view", "all_accounts": true }),
    )
    .await;
    assert_code(
        &res,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
        "stray field",
    );
    let list = app
        .call(Method::GET, "/api/v1/delegations", Some(&alice), None)
        .await;
    assert_eq!(list.body["total"], 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn grant_twice_is_delegation_exists(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (alice, _) = active_user(&app, "alice").await;
    let (_, bob_id) = active_user(&app, "bob").await;
    granted(
        &app,
        &alice,
        json!({ "delegate_id": bob_id, "level": "view" }),
    )
    .await;

    let res = grant(
        &app,
        &alice,
        json!({ "delegate_id": bob_id, "level": "edit" }),
    )
    .await;
    assert_code(
        &res,
        StatusCode::CONFLICT,
        "delegation_exists",
        "second grant",
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delegate_cannot_reach_delegation_routes_through_act_as(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (alice, alice_id) = active_user(&app, "alice").await;
    let (bob, bob_id) = active_user(&app, "bob").await;
    let (_, carol_id) = active_user(&app, "carol").await;
    let id = id_of(
        &granted(
            &app,
            &alice,
            json!({ "delegate_id": bob_id, "level": "publish" }),
        )
        .await,
    );

    let cases = [
        (Method::GET, "/api/v1/delegations".to_owned(), None),
        (
            Method::POST,
            "/api/v1/delegations".to_owned(),
            Some(json!({ "delegate_id": carol_id, "level": "view" })),
        ),
        (
            Method::PATCH,
            format!("/api/v1/delegations/{id}"),
            Some(json!({ "level": "view" })),
        ),
        (Method::DELETE, format!("/api/v1/delegations/{id}"), None),
        (Method::GET, "/api/v1/delegations/received".to_owned(), None),
        (
            Method::DELETE,
            format!("/api/v1/delegations/received/{id}"),
            None,
        ),
        (Method::GET, "/api/v1/me/workspaces".to_owned(), None),
        (
            Method::GET,
            "/api/v1/users/lookup?email=carol%40postit.test".to_owned(),
            None,
        ),
    ];
    for (method, path, body) in cases {
        let what = format!("{method} {path}");
        let res = send_as(&app, method, &path, &bob, alice_id, body).await;
        assert_code(&res, StatusCode::FORBIDDEN, "owner_only", &what);
    }

    // Nothing changed: the grant is still live at its level.
    let list = app
        .call(Method::GET, "/api/v1/delegations", Some(&alice), None)
        .await;
    assert_eq!(list.body["total"], 1);
    assert_eq!(list.body["data"][0]["level"], "publish");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn expired_but_not_ended_grants_show_only_in_history(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let (alice, _) = active_user(&app, "alice").await;
    let (bob, bob_id) = active_user(&app, "bob").await;
    let id = id_of(
        &granted(
            &app,
            &alice,
            json!({ "delegate_id": bob_id, "level": "view", "expires_at": in_days(1) }),
        )
        .await,
    );
    // The expiry passes while the expiry job has not ended the grant yet.
    sqlx::query(
        "UPDATE delegations SET created_at = now() - interval '2 days',
                expires_at = now() - interval '1 day' WHERE id = $1",
    )
    .bind(Uuid::parse_str(&id).unwrap_or_else(|e| unreachable!("{e}")))
    .execute(&pool)
    .await
    .unwrap_or_else(|e| unreachable!("expire: {e}"));

    let live = app
        .call(Method::GET, "/api/v1/delegations", Some(&alice), None)
        .await;
    assert_eq!(live.body["total"], 0, "{:?}", live.body);
    assert_eq!(live.body["data"], json!([]));
    let history = app
        .call(
            Method::GET,
            "/api/v1/delegations?include_ended=true",
            Some(&alice),
            None,
        )
        .await;
    assert_eq!(history.body["total"], 1, "{:?}", history.body);
    assert_eq!(history.body["data"][0]["id"], id);
    let received = app
        .call(
            Method::GET,
            "/api/v1/delegations/received",
            Some(&bob),
            None,
        )
        .await;
    assert_eq!(received.body["total"], 0, "{:?}", received.body);
    assert_eq!(received.body["data"], json!([]));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delegate_lists_and_leaves_received_grant(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (alice, alice_id) = active_user(&app, "alice").await;
    let (bob, bob_id) = active_user(&app, "bob").await;
    let id = id_of(
        &granted(
            &app,
            &alice,
            json!({ "delegate_id": bob_id, "level": "edit" }),
        )
        .await,
    );

    let received = app
        .call(
            Method::GET,
            "/api/v1/delegations/received",
            Some(&bob),
            None,
        )
        .await;
    assert_eq!(received.status, StatusCode::OK, "{:?}", received.body);
    assert_eq!(received.body["total"], 1);
    assert_eq!(received.body["data"][0]["id"], id);
    assert_eq!(
        received.body["data"][0]["owner"]["id"],
        alice_id.to_string()
    );
    assert_eq!(received.body["data"][0]["owner"]["display_name"], "alice");
    assert_eq!(received.body["data"][0]["delegate"]["display_name"], "bob");

    let acting = send_as(&app, Method::GET, "/api/v1/accounts", &bob, alice_id, None).await;
    assert_eq!(acting.status, StatusCode::OK, "{:?}", acting.body);

    // The owner cannot leave through the delegate route.
    let foreign = app
        .call(
            Method::DELETE,
            &format!("/api/v1/delegations/received/{id}"),
            Some(&alice),
            None,
        )
        .await;
    assert_code(&foreign, StatusCode::NOT_FOUND, "not_found", "owner leaves");

    let left = app
        .call(
            Method::DELETE,
            &format!("/api/v1/delegations/received/{id}"),
            Some(&bob),
            None,
        )
        .await;
    assert_eq!(left.status, StatusCode::NO_CONTENT, "{:?}", left.body);

    let after = send_as(&app, Method::GET, "/api/v1/accounts", &bob, alice_id, None).await;
    assert_code(
        &after,
        StatusCode::FORBIDDEN,
        "delegation_invalid",
        "after leaving",
    );
    let empty = app
        .call(
            Method::GET,
            "/api/v1/delegations/received",
            Some(&bob),
            None,
        )
        .await;
    assert_eq!(empty.body["total"], 0, "{:?}", empty.body);
    let history = app
        .call(
            Method::GET,
            "/api/v1/delegations?include_ended=true",
            Some(&alice),
            None,
        )
        .await;
    assert_eq!(history.body["data"][0]["ended_reason"], "left");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn workspaces_lists_own_first_then_received(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (alice, alice_id) = active_user(&app, "alice").await;
    let (bob, bob_id) = active_user(&app, "bob").await;
    let (carol, carol_id) = active_user(&app, "carol").await;
    let (dave, _) = active_user(&app, "dave").await;
    let from_alice = id_of(
        &granted(
            &app,
            &alice,
            json!({ "delegate_id": bob_id, "level": "view" }),
        )
        .await,
    );
    let from_carol = id_of(
        &granted(
            &app,
            &carol,
            json!({ "delegate_id": bob_id, "level": "publish", "account_ids": [] }),
        )
        .await,
    );
    // An ended grant is not a workspace.
    let from_dave = id_of(
        &granted(
            &app,
            &dave,
            json!({ "delegate_id": bob_id, "level": "edit" }),
        )
        .await,
    );
    let revoked = app
        .call(
            Method::DELETE,
            &format!("/api/v1/delegations/{from_dave}"),
            Some(&dave),
            None,
        )
        .await;
    assert_eq!(revoked.status, StatusCode::NO_CONTENT);

    let res = app
        .call(Method::GET, "/api/v1/me/workspaces", Some(&bob), None)
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    let list = res.body.as_array().cloned().unwrap_or_default();
    assert_eq!(list.len(), 3, "{list:?}");

    assert_eq!(list[0]["own"], true);
    assert_eq!(list[0]["owner"]["id"], bob_id.to_string());
    assert_eq!(list[0]["owner"]["display_name"], "bob");
    assert_eq!(list[0]["delegation_id"], Value::Null);
    assert_eq!(list[0]["level"], Value::Null);

    // Newest grant first.
    assert_eq!(list[1]["own"], false);
    assert_eq!(list[1]["owner"]["id"], carol_id.to_string());
    assert_eq!(list[1]["delegation_id"], from_carol);
    assert_eq!(list[1]["level"], "publish");
    assert_eq!(list[1]["all_accounts"], false);
    assert_eq!(list[1]["account_ids"], json!([]));

    assert_eq!(list[2]["owner"]["id"], alice_id.to_string());
    assert_eq!(list[2]["owner"]["display_name"], "alice");
    assert_eq!(list[2]["delegation_id"], from_alice);
    assert_eq!(list[2]["level"], "view");
    assert_eq!(list[2]["all_accounts"], true);
}

async fn lookup(app: &TestApp, token: &str, query: &str) -> TestResponse {
    app.call(
        Method::GET,
        &format!("/api/v1/users/lookup?{query}"),
        Some(token),
        None,
    )
    .await
}

#[sqlx::test(migrations = "../data/migrations")]
async fn lookup_matches_exactly_and_hides_ambiguity(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (alice, _) = active_user(&app, "alice").await;
    let (_, bob_id) = active_user(&app, "bob").await;
    active_user_with(&app, "carol", "carol@postit.test", false).await;
    active_user_with(&app, "dup1", "dup@postit.test", true).await;
    active_user_with(&app, "dup2", "dup@postit.test", true).await;

    for query in [
        "email=bob%40postit.test",
        "email=BOB%40POSTIT.TEST",
        "email=%20%20bob%40postit.test%20%20",
    ] {
        let res = lookup(&app, &alice, query).await;
        assert_eq!(res.status, StatusCode::OK, "{query}: {:?}", res.body);
        assert_eq!(res.body["user"]["id"], bob_id.to_string(), "{query}");
        assert_eq!(res.body["user"]["display_name"], "bob", "{query}");
        let keys: Vec<&String> = res.body["user"]
            .as_object()
            .map(|o| o.keys().collect())
            .unwrap_or_default();
        assert_eq!(keys.len(), 2, "only id and display_name: {keys:?}");
    }
    for query in [
        "email=bob%40postit",
        "email=bob",
        "email=carol%40postit.test",
        "email=alice%40postit.test",
        "email=dup%40postit.test",
        "email=nobody%40postit.test",
    ] {
        let res = lookup(&app, &alice, query).await;
        assert_eq!(res.status, StatusCode::OK, "{query}: {:?}", res.body);
        assert_eq!(res.body, json!({ "user": null }), "{query}");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn lookup_blank_or_missing_email_is_validation_failed(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (alice, _) = active_user(&app, "alice").await;
    for query in ["email=", "email=%20%20", ""] {
        let res = lookup(&app, &alice, query).await;
        assert_code(
            &res,
            StatusCode::UNPROCESSABLE_ENTITY,
            "validation_failed",
            query,
        );
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn lookup_is_rate_limited(pool: PgPool) {
    let app = TestApp::start_with(pool, |s| {
        s.rate_limit.lookup = RateBucket {
            rate_per_minute: 1,
            burst: 1,
        };
    })
    .await;
    let (alice, _) = active_user(&app, "alice").await;
    let first = lookup(&app, &alice, "email=bob%40postit.test").await;
    assert_eq!(first.status, StatusCode::OK, "{:?}", first.body);
    // A blank email is still charged.
    let second = lookup(&app, &alice, "email=").await;
    assert_code(
        &second,
        StatusCode::TOO_MANY_REQUESTS,
        "rate_limited",
        "second",
    );
    assert!(second.headers.get(header::RETRY_AFTER).is_some());
    // Another user has their own bucket.
    let (bob, _) = active_user(&app, "bob").await;
    let other = lookup(&app, &bob, "email=alice%40postit.test").await;
    assert_eq!(other.status, StatusCode::OK, "{:?}", other.body);
}
