#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use postit_api::testkit::{ADMIN_SUB, TestApp, TestResponse};
use postit_config::RateBucket;
use postit_core::{Agent, DiscoveredAccount, PlatformId};
use postit_data::platforms::PlatformRepo;
use postit_http::testkit::leak::assert_no_leak;
use postit_oauth::testkit::{Script, TestCredentialAgent, VerifyScript};
use serde_json::{Value, json};
use sqlx::PgPool;

const SECRET: &str = "s3cret-token-value";

fn found(id: &str) -> DiscoveredAccount {
    DiscoveredAccount {
        remote_id: id.to_owned(),
        display_name: format!("Account {id}"),
        handle: None,
        avatar_url: None,
    }
}

struct Fixture {
    app: TestApp,
    agent: Arc<TestCredentialAgent>,
    token: String,
}

async fn fixture(pool: PgPool, accounts: &[&str]) -> Fixture {
    fixture_with(pool, accounts, |_| {}).await
}

async fn fixture_with(
    pool: PgPool,
    accounts: &[&str],
    configure: impl FnOnce(&mut postit_api::ApiSettings),
) -> Fixture {
    let agent = TestCredentialAgent::new("fake");
    agent.set_script(Script {
        verify: VerifyScript::Accounts(accounts.iter().map(|id| found(id)).collect()),
        ..Script::default()
    });
    let app = TestApp::start_with_agents(
        pool.clone(),
        vec![agent.clone() as Arc<dyn Agent>],
        configure,
    )
    .await;
    PlatformRepo::upsert_registered(
        &pool,
        &[(PlatformId::new("fake").unwrap(), "Fake".to_owned())],
    )
    .await
    .unwrap();
    let token = app.token(ADMIN_SUB);
    Fixture { app, agent, token }
}

fn creds(token: &str) -> Value {
    json!({"server_url": "https://example.test", "token": token})
}

async fn start(f: &Fixture) -> String {
    let res = f
        .app
        .call(
            Method::POST,
            "/api/v1/accounts/credentials/fake",
            Some(&f.token),
            Some(creds(SECRET)),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    res.body["connect_id"].as_str().unwrap().to_owned()
}

async fn confirm(f: &Fixture, connect_id: &str) -> TestResponse {
    f.app
        .call(
            Method::POST,
            &format!("/api/v1/accounts/connect/{connect_id}/confirm"),
            Some(&f.token),
            Some(json!({})),
        )
        .await
}

/// A second signed-in, approved user.
async fn member(app: &TestApp, sub: &str) -> String {
    let admin = app.token(ADMIN_SUB);
    let me = app
        .call(Method::GET, "/api/v1/me", Some(&app.token(sub)), None)
        .await;
    let id = me.body["id"].as_str().unwrap().to_owned();
    app.call(
        Method::PATCH,
        &format!("/api/v1/users/{id}"),
        Some(&admin),
        Some(json!({"status": "active"})),
    )
    .await;
    app.token(sub)
}

#[sqlx::test(migrations = "../data/migrations")]
async fn full_flow_connect_list_get_patch_check_delete(pool: PgPool) {
    let f = fixture(pool, &["a"]).await;
    let connect_id = start(&f).await;

    let confirmed = confirm(&f, &connect_id).await;
    assert_eq!(confirmed.status, StatusCode::OK, "{}", confirmed.body);
    assert_eq!(confirmed.body["connected"][0]["change"], "created");
    let id = confirmed.body["connected"][0]["account"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let listed = f
        .app
        .call(Method::GET, "/api/v1/accounts", Some(&f.token), None)
        .await;
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.body["total"], 1);
    assert_eq!(listed.body["data"][0]["platform"], "fake");

    let path = format!("/api/v1/accounts/{id}");
    let got = f.app.call(Method::GET, &path, Some(&f.token), None).await;
    assert_eq!(got.status, StatusCode::OK);
    assert_eq!(got.body["connect"], "credentials");

    let patched = f
        .app
        .call(
            Method::PATCH,
            &path,
            Some(&f.token),
            Some(json!({"test_account": true, "display_name": "Mine"})),
        )
        .await;
    assert_eq!(patched.status, StatusCode::OK, "{}", patched.body);
    assert_eq!(patched.body["display_name"], "Mine");
    assert_eq!(patched.body["test_account"], true);

    let checked = f
        .app
        .call(Method::POST, &format!("{path}/check"), Some(&f.token), None)
        .await;
    assert_eq!(checked.status, StatusCode::OK, "{}", checked.body);
    assert!(checked.body["last_checked_at"].is_string());

    let deleted = f
        .app
        .call(Method::DELETE, &path, Some(&f.token), None)
        .await;
    assert_eq!(deleted.status, StatusCode::OK, "{}", deleted.body);
    assert_eq!(deleted.body["cancelled_deliveries"], 0);

    let hidden = f
        .app
        .call(Method::GET, "/api/v1/accounts", Some(&f.token), None)
        .await;
    assert_eq!(hidden.body["total"], 0);
    let shown = f
        .app
        .call(
            Method::GET,
            "/api/v1/accounts?include_disconnected=true",
            Some(&f.token),
            None,
        )
        .await;
    assert_eq!(shown.body["total"], 1);
    assert_eq!(shown.body["data"][0]["status"], "disconnected");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn invalid_values_list_every_violation(pool: PgPool) {
    let f = fixture(pool, &["a"]).await;

    let res = f
        .app
        .call(
            Method::POST,
            "/api/v1/accounts/credentials/fake",
            Some(&f.token),
            Some(json!({"extra": SECRET})),
        )
        .await;

    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(res.body["code"], "validation_failed");
    let violations = res.body["violations"].as_array().unwrap();
    assert!(violations.len() >= 3, "{violations:?}");
    assert_no_leak(&[SECRET], &[("body", &res.body.to_string())]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn rejected_credentials_name_the_field(pool: PgPool) {
    let f = fixture(pool, &["a"]).await;
    f.agent.set_script(Script {
        verify: VerifyScript::Invalid(Some("token".to_owned())),
        ..Script::default()
    });

    let res = f
        .app
        .call(
            Method::POST,
            "/api/v1/accounts/credentials/fake",
            Some(&f.token),
            Some(creds(SECRET)),
        )
        .await;

    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(res.body["code"], "credentials_invalid");
    assert_eq!(res.body["field"], "token");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unknown_platform_is_404(pool: PgPool) {
    let f = fixture(pool, &["a"]).await;

    let res = f
        .app
        .call(
            Method::POST,
            "/api/v1/accounts/credentials/nothere",
            Some(&f.token),
            Some(creds(SECRET)),
        )
        .await;

    assert_eq!(res.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn oauth_platform_is_capability_unsupported(pool: PgPool) {
    let app = TestApp::start_with_agents(
        pool.clone(),
        vec![TestCredentialAgent::oauth("fake") as Arc<dyn Agent>],
        |_| {},
    )
    .await;
    PlatformRepo::upsert_registered(
        &pool,
        &[(PlatformId::new("fake").unwrap(), "Fake".to_owned())],
    )
    .await
    .unwrap();

    let res = app
        .call(
            Method::POST,
            "/api/v1/accounts/credentials/fake",
            Some(&app.token(ADMIN_SUB)),
            Some(creds(SECRET)),
        )
        .await;

    assert_eq!(res.status, StatusCode::CONFLICT);
    assert_eq!(res.body["code"], "capability_unsupported");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disabled_platform_is_409(pool: PgPool) {
    let f = fixture(pool.clone(), &["a"]).await;
    PlatformRepo::set_enabled(&pool, &PlatformId::new("fake").unwrap(), false)
        .await
        .unwrap();

    let res = f
        .app
        .call(
            Method::POST,
            "/api/v1/accounts/credentials/fake",
            Some(&f.token),
            Some(creds(SECRET)),
        )
        .await;

    assert_eq!(res.status, StatusCode::CONFLICT);
    assert_eq!(res.body["code"], "platform_disabled");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn confirm_by_another_user_is_404(pool: PgPool) {
    let f = fixture(pool, &["a"]).await;
    let connect_id = start(&f).await;
    let bob = member(&f.app, "bob").await;

    let theirs = f
        .app
        .call(
            Method::POST,
            &format!("/api/v1/accounts/connect/{connect_id}/confirm"),
            Some(&bob),
            Some(json!({})),
        )
        .await;
    let mine = confirm(&f, &connect_id).await;

    assert_eq!(theirs.status, StatusCode::NOT_FOUND);
    assert_eq!(mine.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn another_user_cannot_see_my_account(pool: PgPool) {
    let f = fixture(pool, &["a"]).await;
    let connect_id = start(&f).await;
    let id = confirm(&f, &connect_id).await.body["connected"][0]["account"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let bob = member(&f.app, "bob").await;

    let got = f
        .app
        .call(
            Method::GET,
            &format!("/api/v1/accounts/{id}"),
            Some(&bob),
            None,
        )
        .await;
    let listed = f
        .app
        .call(Method::GET, "/api/v1/accounts", Some(&bob), None)
        .await;
    let deleted = f
        .app
        .call(
            Method::DELETE,
            &format!("/api/v1/accounts/{id}"),
            Some(&bob),
            None,
        )
        .await;

    assert_eq!(got.status, StatusCode::NOT_FOUND);
    assert_eq!(listed.body["total"], 0);
    assert_eq!(deleted.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delete_while_publishing_is_409_job_active(pool: PgPool) {
    let f = fixture(pool.clone(), &["a"]).await;
    let connect_id = start(&f).await;
    let id = confirm(&f, &connect_id).await.body["connected"][0]["account"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    // A delivery claimed by a worker: seeded with SQL, the way the data tests force states.
    let owner: uuid::Uuid = sqlx::query_scalar("SELECT owner_id FROM connected_accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    let post = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO posts (id, owner_id, kind, text) VALUES ($1, $2, 'post', 'hi')")
        .bind(post)
        .bind(owner)
        .execute(&pool)
        .await
        .unwrap();
    let job = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO publish_jobs (id, owner_id, post_id, post_version, content_snapshot, scheduled_at) \
         VALUES ($1, $2, $3, 1, '{}'::jsonb, now())",
    )
    .bind(job)
    .bind(owner)
    .bind(post)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO deliveries (id, owner_id, job_id, post_id, account_id, operation, state, \
         idempotency_key, lease_expires_at) \
         VALUES ($1, $2, $3, $4, $5, 'publish', 'claimed', 'k'::bytea, now() + interval '1 minute')",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(owner)
    .bind(job)
    .bind(post)
    .bind(uuid::Uuid::parse_str(&id).unwrap())
    .execute(&pool)
    .await
    .unwrap();

    let res = f
        .app
        .call(
            Method::DELETE,
            &format!("/api/v1/accounts/{id}"),
            Some(&f.token),
            None,
        )
        .await;

    assert_eq!(res.status, StatusCode::CONFLICT, "{}", res.body);
    assert_eq!(res.body["code"], "job_active");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn credential_checks_are_rate_limited(pool: PgPool) {
    let f = fixture_with(pool, &["a"], |s| {
        s.rate_limit.credential_checks = RateBucket {
            rate_per_minute: 1,
            burst: 1,
        };
    })
    .await;
    let call = || {
        f.app.call(
            Method::POST,
            "/api/v1/accounts/credentials/fake",
            Some(&f.token),
            Some(creds(SECRET)),
        )
    };

    let first = call().await;
    let second = call().await;

    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(second.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(second.body["code"], "rate_limited");
    assert!(second.headers.get(header::RETRY_AFTER).is_some());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn no_secret_in_any_response(pool: PgPool) {
    let f = fixture(pool, &["a", "b"]).await;
    let mut bodies = Vec::new();
    let started = f
        .app
        .call(
            Method::POST,
            "/api/v1/accounts/credentials/fake",
            Some(&f.token),
            Some(creds(SECRET)),
        )
        .await;
    let connect_id = started.body["connect_id"].as_str().unwrap().to_owned();
    bodies.push(("credentials", started.body.to_string()));
    let confirmed = confirm(&f, &connect_id).await;
    let id = confirmed.body["connected"][0]["account"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    bodies.push(("confirm", confirmed.body.to_string()));
    for (label, method, path) in [
        ("list", Method::GET, "/api/v1/accounts".to_owned()),
        ("get", Method::GET, format!("/api/v1/accounts/{id}")),
        (
            "check",
            Method::POST,
            format!("/api/v1/accounts/{id}/check"),
        ),
        ("delete", Method::DELETE, format!("/api/v1/accounts/{id}")),
    ] {
        let res = f.app.call(method, &path, Some(&f.token), None).await;
        bodies.push((label, res.body.to_string()));
    }
    let outputs: Vec<(&str, &str)> = bodies.iter().map(|(l, b)| (*l, b.as_str())).collect();

    assert_no_leak(&[SECRET], &outputs);
    // The connect ID is shown once, in the first response only.
    let later: Vec<(&str, &str)> = outputs.iter().skip(1).copied().collect();
    assert_no_leak(&[connect_id.as_str()], &later);
}

fn csv_request(path: &str, token: &str, content_type: &str, csv: &str) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(csv.to_owned()))
        .unwrap()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn import_dry_run_reports_without_creating(pool: PgPool) {
    let f = fixture(pool.clone(), &["a"]).await;
    let csv = format!("server_url,token\nhttps://a.test,{SECRET}\nhttps://a.test,\n");

    let res = f
        .app
        .send(csv_request(
            "/api/v1/accounts/import/fake",
            &f.token,
            "text/csv",
            &csv,
        ))
        .await;

    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    assert_eq!(res.body["commit"], false);
    assert_eq!(res.body["rows"][0]["status"], "ok");
    assert_eq!(res.body["rows"][1]["status"], "invalid_fields");
    assert_eq!(res.body["counts"]["ok"], 1);
    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM connected_accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(accounts, 0);
    assert_no_leak(&[SECRET], &[("body", &res.body.to_string())]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn import_commit_creates_only_valid_rows(pool: PgPool) {
    let f = fixture(pool.clone(), &["a"]).await;
    let csv = format!("server_url,token\nhttps://a.test,{SECRET}\nhttps://a.test,\n");

    let res = f
        .app
        .send(csv_request(
            "/api/v1/accounts/import/fake?commit=true",
            &f.token,
            "text/csv; charset=utf-8",
            &csv,
        ))
        .await;

    assert_eq!(res.status, StatusCode::OK, "{}", res.body);
    assert_eq!(res.body["commit"], true);
    assert_eq!(
        res.body["rows"][0]["account_ids"].as_array().unwrap().len(),
        1
    );
    let accounts: i64 = sqlx::query_scalar("SELECT count(*) FROM connected_accounts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(accounts, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn import_past_the_size_limit_is_413(pool: PgPool) {
    let f = fixture_with(pool, &["a"], |s| s.csv_max_bytes = 64).await;
    let csv = format!("server_url,token\n{}\n", "https://a.test,x\n".repeat(20));

    let res = f
        .app
        .send(csv_request(
            "/api/v1/accounts/import/fake",
            &f.token,
            "text/csv",
            &csv,
        ))
        .await;

    assert_eq!(res.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(res.body["code"], "payload_too_large");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn import_needs_a_csv_content_type(pool: PgPool) {
    let f = fixture(pool, &["a"]).await;

    let res = f
        .app
        .send(csv_request(
            "/api/v1/accounts/import/fake",
            &f.token,
            "application/json",
            "{}",
        ))
        .await;

    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(res.body["code"], "validation_failed");
}
