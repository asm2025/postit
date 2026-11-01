#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use postit_agent_email::testkit::{Script, Sink};
use postit_agent_email::{EmailAgent, EmailAgentConfig, LookupError, TxtLookup};
use postit_agent_mock::{MockAgent, MockScript, VerifyScript};
use postit_api::testkit::{ADMIN_SUB, TestApp, TestResponse};
use postit_config::RateBucket;
use postit_core::{Agent, DiscoveredAccount, MediaAssetId, PlatformId, TEST_SEND_ID, UserId};
use postit_data::app_settings::AppSettingsRepo;
use postit_data::platforms::PlatformRepo;
use postit_data::testkit::grant;
use postit_data::{AccountLimit, Level};
use postit_http::testkit::leak::assert_no_leak;
use serde_json::{Value, json};
use sqlx::PgPool;
use url::Url;

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
    script: MockScript,
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
    let agent = MockAgent::broadcast().with_platform(PlatformId::new("fake").unwrap());
    let script = agent.script();
    script.set_verify(VerifyScript::Accounts(
        accounts.iter().map(|id| found(id)).collect(),
    ));
    let app = TestApp::start_with_agents(
        pool.clone(),
        vec![Arc::new(agent) as Arc<dyn Agent>],
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
    Fixture { app, script, token }
}

fn creds(token: &str) -> Value {
    json!({"account_name": "test-account", "token": token})
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
    f.script
        .set_verify(VerifyScript::Invalid(Some("token".to_owned())));

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
        vec![Arc::new(
            MockAgent::oauth(&Url::parse("https://example.test/").unwrap())
                .with_platform(PlatformId::new("fake").unwrap()),
        ) as Arc<dyn Agent>],
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
    let csv = format!("account_name,token\na,{SECRET}\na,\n");

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
    let csv = format!("account_name,token\na,{SECRET}\na,\n");

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
    let csv = format!("account_name,token\n{}\n", "a,x\n".repeat(20));

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

// --- POST /accounts/{id}/test-email ---

const DIRECT: &str = "mock_direct";
const ACT_AS: &str = "x-postit-act-as";

/// An app with the direct mock (`mock_direct`) and the broadcast mock (`fake`), both
/// registered, and live publishing switched on.
struct Mail {
    app: TestApp,
    script: MockScript,
    admin: String,
    admin_id: UserId,
}

async fn mail_fixture(pool: PgPool) -> Mail {
    mail_fixture_with(pool, |_| {}).await
}

async fn mail_fixture_with(
    pool: PgPool,
    configure: impl FnOnce(&mut postit_api::ApiSettings),
) -> Mail {
    let direct = MockAgent::direct();
    let script = direct.script();
    script.set_verify(VerifyScript::Accounts(vec![found("a")]));
    let broadcast = MockAgent::broadcast().with_platform(PlatformId::new("fake").unwrap());
    broadcast
        .script()
        .set_verify(VerifyScript::Accounts(vec![found("b")]));
    let app = TestApp::start_with_agents(
        pool.clone(),
        vec![
            Arc::new(direct) as Arc<dyn Agent>,
            Arc::new(broadcast) as Arc<dyn Agent>,
        ],
        configure,
    )
    .await;
    PlatformRepo::upsert_registered(
        &pool,
        &[
            (PlatformId::new(DIRECT).unwrap(), "Mock direct".to_owned()),
            (PlatformId::new("fake").unwrap(), "Fake".to_owned()),
        ],
    )
    .await
    .unwrap();
    let admin = app.token(ADMIN_SUB);
    let me = app
        .call(Method::GET, "/api/v1/me", Some(&admin), None)
        .await;
    let admin_id = UserId::from(uuid::Uuid::parse_str(me.body["id"].as_str().unwrap()).unwrap());
    set_live(&pool, true, admin_id).await;
    Mail {
        app,
        script,
        admin,
        admin_id,
    }
}

async fn set_live(pool: &PgPool, live: bool, admin: UserId) {
    let mut conn = pool.acquire().await.unwrap();
    AppSettingsRepo::set_live_publishing(&mut conn, live, admin)
        .await
        .unwrap();
}

/// Connects `platform` for the holder of `token` with `secret` as the pasted token, and
/// returns the account's ID.
async fn connect(app: &TestApp, token: &str, platform: &str, secret: &str) -> String {
    let started = app
        .call(
            Method::POST,
            &format!("/api/v1/accounts/credentials/{platform}"),
            Some(token),
            Some(creds(secret)),
        )
        .await;
    assert_eq!(started.status, StatusCode::OK, "{}", started.body);
    let connect_id = started.body["connect_id"].as_str().unwrap();
    let confirmed = app
        .call(
            Method::POST,
            &format!("/api/v1/accounts/connect/{connect_id}/confirm"),
            Some(token),
            Some(json!({})),
        )
        .await;
    assert_eq!(confirmed.status, StatusCode::OK, "{}", confirmed.body);
    confirmed.body["connected"][0]["account"]["id"]
        .as_str()
        .unwrap_or_else(|| unreachable!("{}", confirmed.body))
        .to_owned()
}

/// Another approved user, signing in with `email` (verified) or none, and their ID.
async fn user_with(app: &TestApp, sub: &str, email: Option<&str>) -> (String, UserId) {
    let token = app
        .issuer
        .token_and_claims(sub, email, email.map(|_| true), Some(sub))
        .0;
    let me = app
        .call(Method::GET, "/api/v1/me", Some(&token), None)
        .await;
    let id = me.body["id"].as_str().unwrap().to_owned();
    let approved = app
        .call(
            Method::PATCH,
            &format!("/api/v1/users/{id}"),
            Some(&app.token(ADMIN_SUB)),
            Some(json!({"status": "active"})),
        )
        .await;
    assert_eq!(approved.status, StatusCode::OK, "{}", approved.body);
    (token, UserId::from(uuid::Uuid::parse_str(&id).unwrap()))
}

fn document() -> Value {
    json!({"email": {
        "subject": "Hello",
        "language": "en-US",
        "blocks": [{"type": "text", "text": "Hi {{first_name}}"}]
    }})
}

async fn test_email(
    app: &TestApp,
    token: Option<&str>,
    act_as: Option<UserId>,
    account: &str,
    body: &Value,
) -> TestResponse {
    let mut b = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/v1/accounts/{account}/test-email"))
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(owner) = act_as {
        b = b.header(ACT_AS, owner.to_string());
    }
    app.send(b.body(Body::from(body.to_string())).unwrap())
        .await
}

fn assert_problem(res: &TestResponse, status: StatusCode, code: &str) {
    assert_eq!(res.status, status, "{}", res.body);
    assert_eq!(res.body["code"], code, "{}", res.body);
}

/// A small PNG for an image block.
fn png() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([10, 20, 30, 255]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

async fn upload_png(app: &TestApp, token: &str) -> String {
    let body = png();
    let res = app
        .send(
            Request::post("/api/v1/media?filename=photo.png")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "image/png")
                .header(header::CONTENT_LENGTH, body.len())
                .body(Body::from(body))
                .unwrap(),
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{}", res.body);
    res.body["id"].as_str().unwrap().to_owned()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn sends_to_the_acting_users_address(pool: PgPool) {
    let m = mail_fixture(pool).await;
    let (bob, _) = user_with(&m.app, "bob", Some("bob@example.test")).await;
    let account = connect(&m.app, &bob, DIRECT, SECRET).await;
    let image = upload_png(&m.app, &bob).await;
    let body = json!({"email": {
        "subject": "Hello",
        "language": "ar",
        "blocks": [
            {"type": "heading", "text": "Hi", "level": "h1"},
            {"type": "image", "asset": image, "alt": "A photo"}
        ]
    }});

    let res = test_email(&m.app, Some(&bob), None, &account, &body).await;

    assert_eq!(res.status, StatusCode::NO_CONTENT, "{}", res.body);
    assert!(res.bytes.is_empty());
    let sends = m.script.sends();
    assert_eq!(sends.len(), 1, "{sends:?}");
    assert_eq!(sends[0].send_id, TEST_SEND_ID);
    assert_eq!(sends[0].address, "bob@example.test");
    assert_eq!(
        m.script.batch_assets(),
        vec![vec![MediaAssetId::from(
            uuid::Uuid::parse_str(&image).unwrap()
        )]],
        "the image block's asset reaches the agent"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn no_row_and_no_cap_used(pool: PgPool) {
    let m = mail_fixture(pool.clone()).await;
    let account = connect(&m.app, &m.admin, DIRECT, SECRET).await;

    let res = test_email(&m.app, Some(&m.admin), None, &account, &document()).await;

    assert_eq!(res.status, StatusCode::NO_CONTENT, "{}", res.body);
    for count in [
        "SELECT count(*) FROM direct_sends",
        "SELECT count(*) FROM platform_quota_usage",
        "SELECT count(*) FROM platform_app_quota_usage",
        "SELECT count(*) FROM email_events",
        "SELECT count(*) FROM suppressions",
    ] {
        let rows: i64 = sqlx::query_scalar(count).fetch_one(&pool).await.unwrap();
        assert_eq!(rows, 0, "{count}");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn delegate_refused(pool: PgPool) {
    let m = mail_fixture(pool.clone()).await;
    let account = connect(&m.app, &m.admin, DIRECT, SECRET).await;
    for (sub, level) in [
        ("viewer", Level::View),
        ("editor", Level::Edit),
        ("publisher", Level::Publish),
    ] {
        let (token, id) = user_with(&m.app, sub, Some(&format!("{sub}@postit.test"))).await;
        grant(&pool, m.admin_id, id, level, AccountLimit::All).await;
        let res = test_email(
            &m.app,
            Some(&token),
            Some(m.admin_id),
            &account,
            &document(),
        )
        .await;
        assert_problem(&res, StatusCode::FORBIDDEN, "owner_only");
    }

    // The other callers: a stranger in their own workspace, a stranger naming the owner's,
    // and no token at all.
    let (stranger, _) = user_with(&m.app, "stranger", Some("stranger@postit.test")).await;
    let own = test_email(&m.app, Some(&stranger), None, &account, &document()).await;
    let named = test_email(
        &m.app,
        Some(&stranger),
        Some(m.admin_id),
        &account,
        &document(),
    )
    .await;
    let anonymous = test_email(&m.app, None, None, &account, &document()).await;

    assert_problem(&own, StatusCode::NOT_FOUND, "not_found");
    assert_problem(&named, StatusCode::FORBIDDEN, "delegation_invalid");
    assert_problem(&anonymous, StatusCode::UNAUTHORIZED, "unauthenticated");
    assert!(m.script.sends().is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn non_direct_account_409(pool: PgPool) {
    let m = mail_fixture(pool).await;
    let account = connect(&m.app, &m.admin, "fake", SECRET).await;

    let res = test_email(&m.app, Some(&m.admin), None, &account, &document()).await;

    assert_problem(&res, StatusCode::CONFLICT, "capability_unsupported");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn user_without_address_409(pool: PgPool) {
    let m = mail_fixture(pool).await;
    let (token, _) = user_with(&m.app, "noaddr", None).await;
    let account = connect(&m.app, &token, DIRECT, SECRET).await;

    let res = test_email(&m.app, Some(&token), None, &account, &document()).await;

    assert_problem(&res, StatusCode::CONFLICT, "email_address_missing");
    assert!(m.script.sends().is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn kill_switch_409(pool: PgPool) {
    let m = mail_fixture(pool.clone()).await;
    let account = connect(&m.app, &m.admin, DIRECT, SECRET).await;
    set_live(&pool, false, m.admin_id).await;

    let off = test_email(&m.app, Some(&m.admin), None, &account, &document()).await;
    sqlx::query("DELETE FROM app_settings")
        .execute(&pool)
        .await
        .unwrap();
    let unseeded = test_email(&m.app, Some(&m.admin), None, &account, &document()).await;

    assert_problem(&off, StatusCode::CONFLICT, "live_publish_disabled");
    assert_problem(&unseeded, StatusCode::CONFLICT, "live_publish_disabled");
    assert!(m.script.sends().is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disabled_platform_409(pool: PgPool) {
    let m = mail_fixture(pool.clone()).await;
    let account = connect(&m.app, &m.admin, DIRECT, SECRET).await;
    PlatformRepo::set_enabled(&pool, &PlatformId::new(DIRECT).unwrap(), false)
        .await
        .unwrap();

    let res = test_email(&m.app, Some(&m.admin), None, &account, &document()).await;

    assert_problem(&res, StatusCode::CONFLICT, "platform_disabled");
    assert!(m.script.sends().is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn invalid_document_422(pool: PgPool) {
    let m = mail_fixture(pool).await;
    let account = connect(&m.app, &m.admin, DIRECT, SECRET).await;
    let mut blank = document();
    blank["email"]["subject"] = json!(" ");

    let refused = test_email(&m.app, Some(&m.admin), None, &account, &blank).await;
    let malformed = test_email(
        &m.app,
        Some(&m.admin),
        None,
        &account,
        &json!({"email": {"subject": "Hi", "blocks": []}}),
    )
    .await;

    assert_problem(
        &refused,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
    );
    let violations = refused.body["violations"].as_array().unwrap();
    assert!(
        violations.iter().any(|v| v["path"] == "email.subject"),
        "{violations:?}"
    );
    assert_problem(
        &malformed,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
    );
    assert!(m.script.sends().is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn agent_error_code_surfaces(pool: PgPool) {
    let m = mail_fixture(pool).await;
    // The mock connects an `expired` token, then rejects every later call made with it.
    let expired = connect(&m.app, &m.admin, DIRECT, "expired").await;
    let res = test_email(&m.app, Some(&m.admin), None, &expired, &document()).await;
    assert_problem(
        &res,
        StatusCode::UNPROCESSABLE_ENTITY,
        "credentials_invalid",
    );
    assert_eq!(res.body["detail"], "authentication");

    // A remote account belongs to one owner, so Bob's credential reaches another one.
    m.script
        .set_verify(VerifyScript::Accounts(vec![found("c")]));
    let (bob, _) = user_with(&m.app, "bob", Some("bob@postit.test")).await;
    let busy = connect(&m.app, &bob, DIRECT, SECRET).await;
    m.script
        .set_retry_after(Some(std::time::Duration::from_secs(30)));
    let res = test_email(&m.app, Some(&bob), None, &busy, &document()).await;
    assert_problem(&res, StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    assert_eq!(res.body["detail"], "rate_limited");
    assert_eq!(
        res.headers
            .get(header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok()),
        Some("30")
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn refused_recipient_409(pool: PgPool) {
    let m = mail_fixture(pool.clone()).await;
    // The mock bounces any address containing `bounce`.
    let address = "bounce@example.test";
    let (token, _) = user_with(&m.app, "bouncer", Some(address)).await;
    let account = connect(&m.app, &token, DIRECT, SECRET).await;

    let res = test_email(&m.app, Some(&token), None, &account, &document()).await;

    assert_problem(&res, StatusCode::CONFLICT, "test_send_refused");
    assert_eq!(res.body["detail"], "mock_bounce");
    assert_no_leak(&[address], &[("body", &res.body.to_string())]);
    let suppressed: i64 = sqlx::query_scalar("SELECT count(*) FROM suppressions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(suppressed, 0, "a test send suppresses nobody");
}

fn image_document(asset: &str) -> Value {
    json!({"email": {
        "subject": "Hello",
        "language": "en-US",
        "blocks": [{"type": "image", "asset": asset, "alt": "A photo"}]
    }})
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unknown_media_asset_404(pool: PgPool) {
    let m = mail_fixture(pool.clone()).await;
    let account = connect(&m.app, &m.admin, DIRECT, SECRET).await;
    // The same account and an image of the caller's own send: the 404s below are the images'.
    let own = upload_png(&m.app, &m.admin).await;
    let control = test_email(
        &m.app,
        Some(&m.admin),
        None,
        &account,
        &image_document(&own),
    )
    .await;
    assert_eq!(control.status, StatusCode::NO_CONTENT, "{}", control.body);

    let (bob, _) = user_with(&m.app, "bob", Some("bob@example.test")).await;
    let theirs = upload_png(&m.app, &bob).await;
    let not_ready = upload_png(&m.app, &m.admin).await;
    sqlx::query("UPDATE media_assets SET status = 'processing' WHERE id = $1")
        .bind(uuid::Uuid::parse_str(&not_ready).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    for (what, asset) in [
        ("unknown", uuid::Uuid::now_v7().to_string()),
        ("another owner's", theirs),
        ("not ready", not_ready),
    ] {
        let res = test_email(
            &m.app,
            Some(&m.admin),
            None,
            &account,
            &image_document(&asset),
        )
        .await;
        assert_eq!(res.status, StatusCode::NOT_FOUND, "{what}: {}", res.body);
        assert_eq!(res.body["code"], "not_found", "{what}");
    }
    assert_eq!(m.script.sends().len(), 1, "only the control was sent");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unverified_address_409(pool: PgPool) {
    let m = mail_fixture(pool).await;
    let token = m
        .app
        .issuer
        .token_and_claims(
            "unverified",
            Some("unverified@example.test"),
            Some(false),
            Some("unverified"),
        )
        .0;
    let me = m
        .app
        .call(Method::GET, "/api/v1/me", Some(&token), None)
        .await;
    let id = me.body["id"].as_str().unwrap().to_owned();
    let approved = m
        .app
        .call(
            Method::PATCH,
            &format!("/api/v1/users/{id}"),
            Some(&m.admin),
            Some(json!({"status": "active"})),
        )
        .await;
    assert_eq!(approved.status, StatusCode::OK, "{}", approved.body);
    let account = connect(&m.app, &token, DIRECT, SECRET).await;

    let res = test_email(&m.app, Some(&token), None, &account, &document()).await;

    assert_problem(&res, StatusCode::CONFLICT, "email_address_missing");
    assert!(m.script.sends().is_empty());
}

/// The route's own timeout replaces `server.request_timeout`, so a send slower than that
/// still answers.
#[sqlx::test(migrations = "../data/migrations")]
async fn send_may_outlast_the_request_timeout(pool: PgPool) {
    let m = mail_fixture_with(pool, |s| {
        s.request_timeout = std::time::Duration::from_secs(1);
    })
    .await;
    let account = connect(&m.app, &m.admin, DIRECT, SECRET).await;
    let gate = m.script.hold_publishes();

    let body = document();
    let sending = test_email(&m.app, Some(&m.admin), None, &account, &body);
    let releasing = async {
        assert!(
            gate.wait_for_held(1, std::time::Duration::from_secs(10))
                .await
        );
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        gate.release();
    };
    let (res, ()) = tokio::join!(sending, releasing);

    assert_eq!(res.status, StatusCode::NO_CONTENT, "{}", res.body);
    assert_eq!(m.script.sends().len(), 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn every_test_send_is_its_own_message(pool: PgPool) {
    let m = mail_fixture(pool).await;
    let (bob, _) = user_with(&m.app, "bob", Some("bob@example.test")).await;
    let admins = connect(&m.app, &m.admin, DIRECT, SECRET).await;
    m.script
        .set_verify(VerifyScript::Accounts(vec![found("c")]));
    let bobs = connect(&m.app, &bob, DIRECT, SECRET).await;

    for (token, account) in [(&m.admin, &admins), (&bob, &bobs), (&m.admin, &admins)] {
        let res = test_email(&m.app, Some(token), None, account, &document()).await;
        assert_eq!(res.status, StatusCode::NO_CONTENT, "{}", res.body);
    }

    let sends = m.script.sends();
    assert_eq!(sends.len(), 3, "{sends:?}");
    assert!(sends.iter().all(|s| s.send_id == TEST_SEND_ID));
    assert_eq!(sends[1].address, "bob@example.test");
    assert_eq!(sends[0].address, sends[2].address);
    assert_ne!(sends[0].address, sends[1].address);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn rate_limited(pool: PgPool) {
    let m = mail_fixture_with(pool, |s| {
        s.rate_limit.email_checks = RateBucket {
            rate_per_minute: 1,
            burst: 1,
        };
    })
    .await;
    let account = connect(&m.app, &m.admin, DIRECT, SECRET).await;

    let first = test_email(&m.app, Some(&m.admin), None, &account, &document()).await;
    let second = test_email(&m.app, Some(&m.admin), None, &account, &document()).await;

    assert_eq!(first.status, StatusCode::NO_CONTENT, "{}", first.body);
    assert_problem(&second, StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    assert!(second.headers.get(header::RETRY_AFTER).is_some());
}

// --- GET /accounts/{id}/dns-check ---

const EMAIL: &str = "email";
const EMAIL_FROM: &str = "news@bakery.example";
const EMAIL_DOMAIN: &str = "bakery.example";

/// `TXT` records for `bakery.example`: SPF that fails strangers, a DKIM key under `s1`, and a
/// DMARC policy of `none`. Records every name asked.
#[derive(Default)]
struct Records {
    asked: std::sync::Mutex<Vec<String>>,
}

impl Records {
    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl TxtLookup for Records {
    async fn txt(&self, name: &str) -> Result<Vec<String>, LookupError> {
        self.asked.lock().unwrap().push(name.to_owned());
        Ok(match name {
            "bakery.example" => vec!["v=spf1 mx -all".to_owned()],
            "s1._domainkey.bakery.example" => vec!["v=DKIM1; k=rsa; p=MIIBIjANBgkq".to_owned()],
            "_dmarc.bakery.example" => vec!["v=DMARC1; p=none".to_owned()],
            _ => Vec::new(),
        })
    }
}

/// An app with the direct mock (`mock_direct`) and the email agent over [`Records`], and an
/// SMTP sink the email agent connects to.
struct Dns {
    app: TestApp,
    admin: String,
    admin_id: UserId,
    sink: Sink,
    records: Arc<Records>,
}

async fn dns_fixture(pool: PgPool) -> Dns {
    dns_fixture_with(pool, |_| {}).await
}

async fn dns_fixture_with(
    pool: PgPool,
    configure: impl FnOnce(&mut postit_api::ApiSettings),
) -> Dns {
    let sink = Sink::start(Script::default()).await;
    let records = Arc::new(Records::default());
    let email = EmailAgent::new(EmailAgentConfig {
        allow_private: true,
        image_url_ttl: std::time::Duration::from_secs(3600),
        command_timeout: std::time::Duration::from_secs(5),
        hello_name: None,
    })
    .with_txt_lookup(Arc::clone(&records) as Arc<dyn TxtLookup>);
    let direct = MockAgent::direct();
    direct
        .script()
        .set_verify(VerifyScript::Accounts(vec![found("a")]));
    let app = TestApp::start_with_agents(
        pool.clone(),
        vec![
            Arc::new(direct) as Arc<dyn Agent>,
            Arc::new(email) as Arc<dyn Agent>,
        ],
        configure,
    )
    .await;
    PlatformRepo::upsert_registered(
        &pool,
        &[
            (PlatformId::new(DIRECT).unwrap(), "Mock direct".to_owned()),
            (PlatformId::new(EMAIL).unwrap(), "Email".to_owned()),
        ],
    )
    .await
    .unwrap();
    let admin = app.token(ADMIN_SUB);
    let me = app
        .call(Method::GET, "/api/v1/me", Some(&admin), None)
        .await;
    let admin_id = UserId::from(uuid::Uuid::parse_str(me.body["id"].as_str().unwrap()).unwrap());
    Dns {
        app,
        admin,
        admin_id,
        sink,
        records,
    }
}

/// Connects an email account sending from [`EMAIL_FROM`] through the sink.
async fn connect_email(d: &Dns, token: &str) -> String {
    connect_email_from(d, token, EMAIL_FROM).await
}

/// Connects an email account sending from `from` through the sink.
async fn connect_email_from(d: &Dns, token: &str, from: &str) -> String {
    let started = d
        .app
        .call(
            Method::POST,
            "/api/v1/accounts/credentials/email",
            Some(token),
            Some(json!({
                "transport": "smtp",
                "from_address": from,
                "postal_address": "12 Main St",
                "host": "127.0.0.1",
                "port": d.sink.port(),
                "tls": "none",
            })),
        )
        .await;
    assert_eq!(started.status, StatusCode::OK, "{}", started.body);
    let connect_id = started.body["connect_id"].as_str().unwrap();
    let confirmed = d
        .app
        .call(
            Method::POST,
            &format!("/api/v1/accounts/connect/{connect_id}/confirm"),
            Some(token),
            Some(json!({})),
        )
        .await;
    assert_eq!(confirmed.status, StatusCode::OK, "{}", confirmed.body);
    confirmed.body["connected"][0]["account"]["id"]
        .as_str()
        .unwrap_or_else(|| unreachable!("{}", confirmed.body))
        .to_owned()
}

async fn dns_check(
    app: &TestApp,
    token: Option<&str>,
    act_as: Option<UserId>,
    account: &str,
    query: &str,
) -> TestResponse {
    let mut b = Request::builder()
        .method(Method::GET)
        .uri(format!("/api/v1/accounts/{account}/dns-check{query}"));
    if let Some(token) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(owner) = act_as {
        b = b.header(ACT_AS, owner.to_string());
    }
    app.send(b.body(Body::empty()).unwrap()).await
}

#[sqlx::test(migrations = "../data/migrations")]
async fn dns_check_reports_findings(pool: PgPool) {
    let d = dns_fixture(pool).await;
    let account = connect_email(&d, &d.admin).await;

    let with = dns_check(&d.app, Some(&d.admin), None, &account, "?dkim_selector=s1").await;
    let without = dns_check(&d.app, Some(&d.admin), None, &account, "").await;

    assert_eq!(with.status, StatusCode::OK, "{}", with.body);
    assert_eq!(
        with.body,
        json!({"findings": [
            {"record": "spf", "status": "ok", "code": "spf_ok"},
            {"record": "dkim", "status": "ok", "code": "dkim_ok"},
            {"record": "dmarc", "status": "weak", "code": "dmarc_weak"},
        ]})
    );
    assert_eq!(without.status, StatusCode::OK, "{}", without.body);
    assert_eq!(
        without.body["findings"][1],
        json!({"record": "dkim", "status": "unknown", "code": "dkim_unknown"})
    );
    assert!(
        d.records
            .asked()
            .contains(&format!("s1._domainkey.{EMAIL_DOMAIN}"))
    );
    for res in [&with, &without] {
        assert_no_leak(
            &[EMAIL_FROM, EMAIL_DOMAIN, "spf1", "DMARC1"],
            &[("body", &res.body.to_string())],
        );
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn mock_account_409_capability_unsupported(pool: PgPool) {
    let d = dns_fixture(pool).await;
    let account = connect(&d.app, &d.admin, DIRECT, SECRET).await;

    let res = dns_check(&d.app, Some(&d.admin), None, &account, "").await;

    assert_problem(&res, StatusCode::CONFLICT, "capability_unsupported");
    assert!(d.records.asked().is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn dns_check_delegate_refused(pool: PgPool) {
    let d = dns_fixture(pool.clone()).await;
    let account = connect_email(&d, &d.admin).await;
    for (sub, level) in [
        ("viewer", Level::View),
        ("editor", Level::Edit),
        ("publisher", Level::Publish),
    ] {
        let (token, id) = user_with(&d.app, sub, Some(&format!("{sub}@postit.test"))).await;
        grant(&pool, d.admin_id, id, level, AccountLimit::All).await;
        let res = dns_check(&d.app, Some(&token), Some(d.admin_id), &account, "").await;
        assert_problem(&res, StatusCode::FORBIDDEN, "owner_only");
    }

    let (stranger, _) = user_with(&d.app, "stranger", Some("stranger@postit.test")).await;
    let own = dns_check(&d.app, Some(&stranger), None, &account, "").await;
    let named = dns_check(&d.app, Some(&stranger), Some(d.admin_id), &account, "").await;
    let anonymous = dns_check(&d.app, None, None, &account, "").await;

    assert_problem(&own, StatusCode::NOT_FOUND, "not_found");
    assert_problem(&named, StatusCode::FORBIDDEN, "delegation_invalid");
    assert_problem(&anonymous, StatusCode::UNAUTHORIZED, "unauthenticated");
    assert!(d.records.asked().is_empty());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn bad_selector_422(pool: PgPool) {
    let d = dns_fixture(pool).await;
    let account = connect_email(&d, &d.admin).await;
    let long = format!("?dkim_selector={}", "s".repeat(64));
    for query in [
        "?dkim_selector=",
        "?dkim_selector=a.b",
        "?dkim_selector=-s1",
        "?dkim_selector=s_1",
        "?dkim_selector=s%201",
        "?dkim_selector=%2E%2E",
        long.as_str(),
    ] {
        let res = dns_check(&d.app, Some(&d.admin), None, &account, query).await;
        assert_problem(&res, StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
        assert_eq!(res.body["field"], "dkim_selector", "{query}: {}", res.body);
    }
    let unknown = dns_check(&d.app, Some(&d.admin), None, &account, "?selector=s1").await;
    assert_problem(
        &unknown,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
    );
    assert!(d.records.asked().is_empty());
}

/// A bad selector and a delegate's refusal come before the charge: with a bucket of one,
/// a valid check still answers after them.
#[sqlx::test(migrations = "../data/migrations")]
async fn refusals_before_the_charge_cost_nothing(pool: PgPool) {
    let d = dns_fixture_with(pool.clone(), |s| {
        s.rate_limit.email_checks = RateBucket {
            rate_per_minute: 1,
            burst: 1,
        };
    })
    .await;
    let account = connect_email(&d, &d.admin).await;
    for _ in 0..3 {
        let bad = dns_check(&d.app, Some(&d.admin), None, &account, "?dkim_selector=a.b").await;
        assert_problem(&bad, StatusCode::UNPROCESSABLE_ENTITY, "validation_failed");
    }
    let valid = dns_check(&d.app, Some(&d.admin), None, &account, "").await;
    assert_eq!(valid.status, StatusCode::OK, "{}", valid.body);

    let (token, id) = user_with(&d.app, "publisher", Some("publisher@postit.test")).await;
    grant(&pool, d.admin_id, id, Level::Publish, AccountLimit::All).await;
    let theirs = connect_email_from(&d, &token, "news@other.example").await;
    for _ in 0..3 {
        let refused = dns_check(&d.app, Some(&token), Some(d.admin_id), &account, "").await;
        assert_problem(&refused, StatusCode::FORBIDDEN, "owner_only");
    }
    let own = dns_check(&d.app, Some(&token), None, &theirs, "").await;
    assert_eq!(own.status, StatusCode::OK, "{}", own.body);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn dns_check_rate_limited(pool: PgPool) {
    let d = dns_fixture_with(pool, |s| {
        s.rate_limit.email_checks = RateBucket {
            rate_per_minute: 1,
            burst: 1,
        };
    })
    .await;
    let account = connect_email(&d, &d.admin).await;

    let first = dns_check(&d.app, Some(&d.admin), None, &account, "").await;
    let second = dns_check(&d.app, Some(&d.admin), None, &account, "").await;
    // The test email shares the bucket.
    let test = test_email(&d.app, Some(&d.admin), None, &account, &document()).await;

    assert_eq!(first.status, StatusCode::OK, "{}", first.body);
    assert_problem(&second, StatusCode::TOO_MANY_REQUESTS, "rate_limited");
    assert!(second.headers.get(header::RETRY_AFTER).is_some());
    assert_problem(&test, StatusCode::TOO_MANY_REQUESTS, "rate_limited");
}
