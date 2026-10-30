//! The notification list, read, and unread-count routes.

use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode, header};
use chrono::{Duration, SubsecRound as _, Utc};
use postit_api::testkit::{ADMIN_SUB, TestApp, TestResponse};
use postit_core::{
    AccountId, AccountLabel, DeliveryId, DeliveryNotice, Notice, NotificationId, PlatformId,
    PostId, PublishJobId, UserId,
};
use postit_data::notifications::{NewNotification, NotificationRepo};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

const LIST: &str = "/api/v1/notifications";
const READ: &str = "/api/v1/notifications/read";
const COUNT: &str = "/api/v1/notifications/unread-count";
const ACT_AS: &str = "x-postit-act-as";

async fn approve(app: &TestApp, token: &str) -> Uuid {
    let me = app.call(Method::GET, "/api/v1/me", Some(token), None).await;
    let id = me.body["id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .unwrap_or_else(|| unreachable!("no id: {:?}", me.body));
    let res = app
        .call(
            Method::PATCH,
            &format!("/api/v1/users/{id}"),
            Some(&app.token(ADMIN_SUB)),
            Some(json!({ "status": "active" })),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    id
}

async fn active_user(app: &TestApp, sub: &str) -> (String, Uuid) {
    let token = app.token(sub);
    let id = approve(app, &token).await;
    (token, id)
}

async fn send(
    app: &TestApp,
    method: Method,
    path: &str,
    token: &str,
    extra: &[(&str, String)],
    body: Option<Value>,
) -> TestResponse {
    let mut b = Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    for (k, v) in extra {
        b = b.header(*k, v);
    }
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

fn label(name: &str) -> AccountLabel {
    AccountLabel {
        account_id: AccountId::from_uuid(Uuid::new_v4()),
        account_name: name.to_owned(),
        platform: PlatformId::new("mock").unwrap_or_else(|e| unreachable!("{e}")),
    }
}

fn failed(account: AccountLabel) -> Notice {
    Notice::DeliveryFailed(DeliveryNotice {
        delivery_id: DeliveryId::from_uuid(Uuid::new_v4()),
        job_id: PublishJobId::from_uuid(Uuid::new_v4()),
        post_id: PostId::from_uuid(Uuid::new_v4()),
        account,
        park_reason: None,
    })
}

/// Records a notification for `user` about `owner`'s workspace, `age_secs` ago.
async fn record(
    pool: &PgPool,
    user: Uuid,
    owner: Option<Uuid>,
    notice: Notice,
    age_secs: i64,
) -> Uuid {
    let id = Uuid::new_v4();
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    NotificationRepo::insert(
        &mut conn,
        &NewNotification {
            id: NotificationId::from_uuid(id),
            user: UserId::from_uuid(user),
            owner: owner.map(UserId::from_uuid),
            notice,
            email_wanted: false,
            created_at: (Utc::now() - Duration::seconds(age_secs)).trunc_subsecs(3),
        },
    )
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    id
}

fn ids(body: &Value) -> Vec<String> {
    body["data"]
        .as_array()
        .unwrap_or_else(|| unreachable!("no data: {body:?}"))
        .iter()
        .map(|n| n["id"].as_str().unwrap_or_default().to_owned())
        .collect()
}

/// IDs of the `delivery_failed` rows only: real delegation changes also notify the other party.
fn failed_ids(body: &Value) -> Vec<String> {
    body["data"]
        .as_array()
        .unwrap_or_else(|| unreachable!("no data: {body:?}"))
        .iter()
        .filter(|n| n["kind"] == "delivery_failed")
        .map(|n| n["id"].as_str().unwrap_or_default().to_owned())
        .collect()
}

fn etag(headers: &HeaderMap) -> String {
    headers
        .get(header::ETAG)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn list_is_newest_first_and_paginated(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let (alice, alice_id) = active_user(&app, "alice").await;
    let oldest = record(&pool, alice_id, None, failed(label("A")), 30).await;
    let middle = record(&pool, alice_id, None, failed(label("B")), 20).await;
    let newest = record(&pool, alice_id, None, failed(label("C")), 10).await;

    let all = app.call(Method::GET, LIST, Some(&alice), None).await;
    assert_eq!(all.status, StatusCode::OK, "{:?}", all.body);
    assert_eq!(
        ids(&all.body),
        [newest, middle, oldest].map(|i| i.to_string())
    );
    assert_eq!(all.body["total"], 3);
    let first = &all.body["data"][0];
    assert_eq!(first["kind"], "delivery_failed");
    assert_eq!(first["severity"], "critical");
    assert_eq!(first["payload"]["account"]["account_name"], "C");
    assert!(first["read_at"].is_null());
    assert!(first["owner_id"].is_null());

    let page2 = app
        .call(
            Method::GET,
            &format!("{LIST}?page=2&page_size=2"),
            Some(&alice),
            None,
        )
        .await;
    assert_eq!(ids(&page2.body), [oldest.to_string()]);
    assert_eq!(page2.body["total"], 3);
    assert_eq!(page2.body["page"], 2);

    let bad = app
        .call(
            Method::GET,
            &format!("{LIST}?page_size=0"),
            Some(&alice),
            None,
        )
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn read_by_ids_ignores_other_users(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let (alice, alice_id) = active_user(&app, "alice").await;
    let (bob, bob_id) = active_user(&app, "bob").await;
    let mine = record(&pool, alice_id, None, failed(label("A")), 10).await;
    let other_mine = record(&pool, alice_id, None, failed(label("A2")), 5).await;
    let theirs = record(&pool, bob_id, None, failed(label("B")), 10).await;

    let res = app
        .call(
            Method::POST,
            READ,
            Some(&alice),
            Some(json!({ "ids": [mine, theirs] })),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert_eq!(res.body["updated"], 1);

    // Reading again changes nothing.
    let again = app
        .call(
            Method::POST,
            READ,
            Some(&alice),
            Some(json!({ "ids": [mine] })),
        )
        .await;
    assert_eq!(again.body["updated"], 0);

    let alice_list = app.call(Method::GET, LIST, Some(&alice), None).await;
    for n in alice_list.body["data"].as_array().unwrap_or(&vec![]) {
        let read = !n["read_at"].is_null();
        assert_eq!(read, n["id"] == mine.to_string(), "{n:?}");
    }
    assert!(ids(&alice_list.body).contains(&other_mine.to_string()));
    let bob_list = app.call(Method::GET, LIST, Some(&bob), None).await;
    assert!(bob_list.body["data"][0]["read_at"].is_null());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn read_all(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let (alice, alice_id) = active_user(&app, "alice").await;
    let (bob, bob_id) = active_user(&app, "bob").await;
    for age in [3, 2, 1] {
        record(&pool, alice_id, None, failed(label("A")), age).await;
    }
    record(&pool, bob_id, None, failed(label("B")), 1).await;

    let res = app
        .call(
            Method::POST,
            READ,
            Some(&alice),
            Some(json!({ "all": true })),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert_eq!(res.body["updated"], 3);
    let count = app.call(Method::GET, COUNT, Some(&alice), None).await;
    assert_eq!(count.body["count"], 0);
    let theirs = app.call(Method::GET, COUNT, Some(&bob), None).await;
    assert_eq!(theirs.body["count"], 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn read_body_must_pick_exactly_one_selection(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (alice, _) = active_user(&app, "alice").await;
    for body in [
        json!({}),
        json!({ "all": false }),
        json!({ "all": true, "ids": [Uuid::new_v4()] }),
        json!({ "ids": [] }),
        json!({ "ids": (0..201).map(|_| Uuid::new_v4()).collect::<Vec<_>>() }),
    ] {
        let res = app
            .call(Method::POST, READ, Some(&alice), Some(body.clone()))
            .await;
        assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(res.body["code"], "validation_failed", "{body}");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unread_count_etag_and_304(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let (alice, alice_id) = active_user(&app, "alice").await;

    let empty = app.call(Method::GET, COUNT, Some(&alice), None).await;
    assert_eq!(empty.status, StatusCode::OK);
    assert_eq!(empty.body["count"], 0);
    assert_eq!(etag(&empty.headers), "W/\"0-0\"");

    let first = record(&pool, alice_id, None, failed(label("A")), 10).await;
    let one = app.call(Method::GET, COUNT, Some(&alice), None).await;
    assert_eq!(one.body["count"], 1);
    let tag = etag(&one.headers);
    assert_eq!(tag, format!("W/\"1-{first}\""));

    let cached = send(
        &app,
        Method::GET,
        COUNT,
        &alice,
        &[("if-none-match", tag.clone())],
        None,
    )
    .await;
    assert_eq!(cached.status, StatusCode::NOT_MODIFIED);
    assert!(cached.bytes.is_empty());
    assert_eq!(etag(&cached.headers), tag);

    let second = record(&pool, alice_id, None, failed(label("B")), 5).await;
    let changed = send(
        &app,
        Method::GET,
        COUNT,
        &alice,
        &[("if-none-match", tag)],
        None,
    )
    .await;
    assert_eq!(changed.status, StatusCode::OK);
    assert_eq!(changed.body["count"], 2);
    assert_eq!(etag(&changed.headers), format!("W/\"2-{second}\""));

    // A list of tags matches when one of them is current.
    let list = send(
        &app,
        Method::GET,
        COUNT,
        &alice,
        &[(
            "if-none-match",
            format!("\"x\", {}", etag(&changed.headers)),
        )],
        None,
    )
    .await;
    assert_eq!(list.status, StatusCode::NOT_MODIFIED);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn act_as_is_owner_only(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (alice, _) = active_user(&app, "alice").await;
    let (_, bob_id) = active_user(&app, "bob").await;
    for (method, path, body) in [
        (Method::GET, LIST, None),
        (Method::POST, READ, Some(json!({ "all": true }))),
        (Method::GET, COUNT, None),
    ] {
        let res = send(
            &app,
            method,
            path,
            &alice,
            &[(ACT_AS, bob_id.to_string())],
            body,
        )
        .await;
        assert_eq!(res.status, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(res.body["code"], "owner_only", "{path}");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unauthenticated_is_401(pool: PgPool) {
    let app = TestApp::start(pool).await;
    for (method, path) in [
        (Method::GET, LIST),
        (Method::POST, READ),
        (Method::GET, COUNT),
    ] {
        let res = app
            .call(method, path, None, Some(json!({ "all": true })))
            .await;
        assert_eq!(res.status, StatusCode::UNAUTHORIZED, "{path}");
    }
}

/// `ActiveUser` refuses a pending user with 403 `account_pending` on every route.
#[sqlx::test(migrations = "../data/migrations")]
async fn pending_user_cannot_list(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let carol = app.token("carol");
    let me = app
        .call(Method::GET, "/api/v1/me", Some(&carol), None)
        .await;
    assert_eq!(me.body["status"], "pending");
    for (method, path, body) in [
        (Method::GET, LIST, None),
        (Method::POST, READ, Some(json!({ "all": true }))),
        (Method::GET, COUNT, None),
    ] {
        let res = app.call(method, path, Some(&carol), body).await;
        assert_eq!(res.status, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(res.body["code"], "account_pending", "{path}");
    }
}

async fn grant(app: &TestApp, owner: &str, delegate: Uuid, level: &str, accounts: Option<Value>) {
    let mut body = json!({ "delegate_id": delegate, "level": level });
    if let Some(accounts) = accounts {
        body["account_ids"] = accounts;
    }
    let res = app
        .call(Method::POST, "/api/v1/delegations", Some(owner), Some(body))
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
}

/// Four callers: the owner, a delegate at each level, a delegate limited to no accounts, and
/// an unrelated user. Notifications are per-recipient rows, so each sees only their own, and
/// the delegates' include rows recorded for them in the owner's workspace.
#[sqlx::test(migrations = "../data/migrations")]
async fn delegate_sees_workspace_notifications_with_owner_id(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let (owner, owner_id) = active_user(&app, "owner").await;
    let mut delegates = Vec::new();
    for level in ["view", "edit", "publish"] {
        let (token, id) = active_user(&app, level).await;
        grant(&app, &owner, id, level, None).await;
        delegates.push((token, id));
    }
    let (limited, limited_id) = active_user(&app, "limited").await;
    grant(&app, &owner, limited_id, "publish", Some(json!([]))).await;
    let (stranger, stranger_id) = active_user(&app, "stranger").await;

    let account = label("Shop");
    let owner_row = record(&pool, owner_id, Some(owner_id), failed(account.clone()), 10).await;
    let mut rows = Vec::new();
    for (_, id) in &delegates {
        rows.push(record(&pool, *id, Some(owner_id), failed(account.clone()), 9).await);
    }
    let own = record(
        &pool,
        delegates[0].1,
        Some(delegates[0].1),
        failed(label("Mine")),
        8,
    )
    .await;
    let stranger_row = record(&pool, stranger_id, None, failed(label("S")), 7).await;

    let res = app.call(Method::GET, LIST, Some(&owner), None).await;
    assert_eq!(failed_ids(&res.body), [owner_row.to_string()]);
    assert_eq!(res.body["data"][0]["owner_id"], owner_id.to_string());
    for (i, (token, _)) in delegates.iter().enumerate() {
        let res = app.call(Method::GET, LIST, Some(token), None).await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        let got = ids(&res.body);
        assert!(got.contains(&rows[i].to_string()), "level {i}");
        assert!(!got.contains(&owner_row.to_string()), "level {i}");
        assert!(!got.contains(&stranger_row.to_string()), "level {i}");
        let workspace = res.body["data"]
            .as_array()
            .and_then(|d| d.iter().find(|n| n["id"] == rows[i].to_string()))
            .unwrap_or_else(|| unreachable!("row missing"));
        assert_eq!(workspace["owner_id"], owner_id.to_string());
        if i == 0 {
            assert!(got.contains(&own.to_string()));
        }
    }
    let res = app.call(Method::GET, LIST, Some(&limited), None).await;
    assert!(failed_ids(&res.body).is_empty(), "{:?}", res.body);
    let res = app.call(Method::GET, LIST, Some(&stranger), None).await;
    assert_eq!(ids(&res.body), [stranger_row.to_string()]);

    // Reading is per recipient: a delegate cannot read the owner's row by ID.
    let res = app
        .call(
            Method::POST,
            READ,
            Some(&delegates[1].0),
            Some(json!({ "ids": [owner_row] })),
        )
        .await;
    assert_eq!(res.body["updated"], 0);
}

/// Review Focus 4: the delegate was notified, then the grant was revoked. The old row still
/// reads from the labels stored with it, whatever the account is called now.
#[sqlx::test(migrations = "../data/migrations")]
async fn old_notifications_keep_their_labels(pool: PgPool) {
    let agent = postit_agent_mock::MockAgent::broadcast();
    agent
        .script()
        .set_verify(postit_agent_mock::VerifyScript::Accounts(vec![
            postit_core::DiscoveredAccount {
                remote_id: "a".to_owned(),
                display_name: "Account a".to_owned(),
                handle: None,
                avatar_url: None,
            },
        ]));
    let app = TestApp::start_with_agents(
        pool.clone(),
        vec![std::sync::Arc::new(agent) as std::sync::Arc<dyn postit_core::Agent>],
        |_| {},
    )
    .await;
    postit_data::platforms::PlatformRepo::upsert_registered(
        &pool,
        &[(
            PlatformId::new("mock").unwrap_or_else(|e| unreachable!("{e}")),
            "Mock".to_owned(),
        )],
    )
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    let (owner, owner_id) = active_user(&app, "owner").await;
    let (delegate, delegate_id) = active_user(&app, "delegate").await;

    // A real account, named "Old name" when the notice is recorded.
    let started = app
        .call(
            Method::POST,
            "/api/v1/accounts/credentials/mock",
            Some(&owner),
            Some(json!({ "account_name": "mine", "token": "pasted-secret" })),
        )
        .await;
    assert_eq!(started.status, StatusCode::OK, "{:?}", started.body);
    let connect_id = started.body["connect_id"].as_str().unwrap_or_default();
    let confirmed = app
        .call(
            Method::POST,
            &format!("/api/v1/accounts/connect/{connect_id}/confirm"),
            Some(&owner),
            Some(json!({})),
        )
        .await;
    assert_eq!(confirmed.status, StatusCode::OK, "{:?}", confirmed.body);
    let account_id = confirmed.body["connected"][0]["account"]["id"]
        .as_str()
        .unwrap_or_else(|| unreachable!("no account: {:?}", confirmed.body))
        .to_owned();
    let account_path = format!("/api/v1/accounts/{account_id}");
    let rename = |name: &'static str| {
        let (app, owner, path) = (&app, &owner, &account_path);
        async move {
            let res = app
                .call(
                    Method::PATCH,
                    path,
                    Some(owner),
                    Some(json!({ "display_name": name })),
                )
                .await;
            assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        }
    };
    rename("Old name").await;

    let granted = app
        .call(
            Method::POST,
            "/api/v1/delegations",
            Some(&owner),
            Some(json!({ "delegate_id": delegate_id, "level": "publish" })),
        )
        .await;
    assert_eq!(granted.status, StatusCode::CREATED, "{:?}", granted.body);
    let grant_id = granted.body["id"].as_str().unwrap_or_default().to_owned();

    // The label is taken from the account as it is now, as the engine takes it.
    let account = {
        let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
        let id = AccountId::from_uuid(
            Uuid::parse_str(&account_id).unwrap_or_else(|e| unreachable!("{e}")),
        );
        postit_data::accounts::AccountRepo::labels(&mut conn, UserId::from_uuid(owner_id), &[id])
            .await
            .unwrap_or_else(|e| unreachable!("{e}"))
            .pop()
            .unwrap_or_else(|| unreachable!("no label"))
    };
    assert_eq!(account.account_name, "Old name");
    let row = record(&pool, delegate_id, Some(owner_id), failed(account), 10).await;

    // The account is renamed and the grant revoked afterwards.
    rename("New name").await;
    let revoked = app
        .call(
            Method::DELETE,
            &format!("/api/v1/delegations/{grant_id}"),
            Some(&owner),
            None,
        )
        .await;
    assert!(revoked.status.is_success(), "{:?}", revoked.body);

    let res = app.call(Method::GET, LIST, Some(&delegate), None).await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    assert_eq!(failed_ids(&res.body), [row.to_string()]);
    let n = res.body["data"]
        .as_array()
        .and_then(|d| d.iter().find(|n| n["id"] == row.to_string()))
        .unwrap_or_else(|| unreachable!("row missing"));
    assert_eq!(n["payload"]["account"]["account_name"], "Old name");
    assert_eq!(n["payload"]["account"]["account_id"], account_id);
    assert_eq!(n["payload"]["account"]["platform"], "mock");
    assert_eq!(n["owner_id"], owner_id.to_string());
    // The former delegate can still read and clear it.
    let read = app
        .call(
            Method::POST,
            READ,
            Some(&delegate),
            Some(json!({ "all": true })),
        )
        .await;
    assert!(read.body["updated"].as_u64() >= Some(1), "{:?}", read.body);
}
