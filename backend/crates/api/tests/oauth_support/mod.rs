//! What the OAuth route tests share: the app with `mock_oauth` pointed at the mock provider,
//! users, and the steps of a connection over HTTP.
#![allow(dead_code, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use hmac::{Hmac, Mac};
use postit_agent_kit::testkit::MockOAuthProvider;
use postit_agent_mock::{MockAgent, MockScript};
use postit_api::ApiSettings;
use postit_api::testkit::{ADMIN_SUB, TestApp, TestResponse};
use postit_core::{Agent, PlatformId, UserId};
use postit_data::platforms::PlatformRepo;
use postit_oauth::testkit::TestOptions;
use serde_json::json;
use sha2::Sha256;
use sqlx::PgPool;
use url::Url;
use uuid::Uuid;

pub const OAUTH: &str = "mock_oauth";
pub const CREDENTIALS: &str = "mock";
pub const DONE: &str = "https://postit.local:44315/oauth-done";
pub const CLIENT_ID: &str = "postit-app";
pub const CLIENT_SECRET: &str = "postit-app-secret-value";
pub const ACT_AS: &str = "x-postit-act-as";
pub const SIGNATURE: &str = "X-Mock-Signature";

pub struct Fixture {
    pub app: TestApp,
    pub pool: PgPool,
    pub provider: MockOAuthProvider,
    pub script: MockScript,
    pub owner: String,
    pub owner_id: UserId,
}

pub async fn fixture(pool: PgPool) -> Fixture {
    fixture_with(pool, |_| {}, postit_oauth::CALLBACK_DEADLINE).await
}

/// The app with `mock_oauth` (configured with [`CLIENT_ID`] and [`CLIENT_SECRET`]) and the
/// credentials mock, both registered, and one active user.
pub async fn fixture_with(
    pool: PgPool,
    configure: impl FnOnce(&mut ApiSettings),
    callback_deadline: Duration,
) -> Fixture {
    let provider = MockOAuthProvider::start().await;
    provider.set_client(CLIENT_ID, CLIENT_SECRET);
    let oauth = MockAgent::oauth(&provider.base_url()).with_platform(platform(OAUTH));
    let script = oauth.script();
    let credentials = MockAgent::broadcast().with_platform(platform(CREDENTIALS));
    let mut configs = BTreeMap::new();
    configs.insert(
        platform(OAUTH),
        Some(json!({"client_id": CLIENT_ID, "client_secret": CLIENT_SECRET})),
    );
    let options = TestOptions {
        configs,
        callback_deadline,
        ..TestOptions::default()
    };
    let app = TestApp::start_with_oauth(
        pool.clone(),
        vec![
            Arc::new(oauth) as Arc<dyn Agent>,
            Arc::new(credentials) as Arc<dyn Agent>,
        ],
        configure,
        options,
    )
    .await;
    PlatformRepo::upsert_registered(
        &pool,
        &[
            (platform(OAUTH), "Mock OAuth".to_owned()),
            (platform(CREDENTIALS), "Mock".to_owned()),
        ],
    )
    .await
    .unwrap();
    let (owner, owner_id) = active_user(&app, "owner").await;
    Fixture {
        app,
        pool,
        provider,
        script,
        owner,
        owner_id,
    }
}

pub fn platform(id: &str) -> PlatformId {
    PlatformId::new(id).unwrap()
}

/// Signs `sub` in, approves them through the admin API, and returns their token and ID.
pub async fn active_user(app: &TestApp, sub: &str) -> (String, UserId) {
    let token = app.token(sub);
    let me = app
        .call(Method::GET, "/api/v1/me", Some(&token), None)
        .await;
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
    (token, UserId::from(id))
}

/// One request with a bearer token, acting as `owner` when given.
pub async fn call_as(
    app: &TestApp,
    method: Method,
    path: &str,
    token: &str,
    owner: Option<UserId>,
    body: Option<serde_json::Value>,
) -> TestResponse {
    let mut b = Request::builder()
        .method(method)
        .uri(path)
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    if let Some(owner) = owner {
        b = b.header(ACT_AS, owner.to_string());
    }
    let body = match body {
        Some(json) => {
            b = b.header(header::CONTENT_TYPE, "application/json");
            Body::from(json.to_string())
        }
        None => Body::empty(),
    };
    app.send(b.body(body).unwrap()).await
}

/// `POST /accounts/connect/mock_oauth` returning to `return_to`.
pub async fn start_as(
    app: &TestApp,
    token: &str,
    owner: Option<UserId>,
    return_to: &str,
) -> TestResponse {
    call_as(
        app,
        Method::POST,
        &format!("/api/v1/accounts/connect/{OAUTH}"),
        token,
        owner,
        Some(json!({ "return_to": return_to })),
    )
    .await
}

/// The authorize URL of a successful start.
pub fn authorize_url(started: &TestResponse) -> Url {
    assert_eq!(started.status, StatusCode::OK, "{:?}", started.body);
    Url::parse(started.body["authorize_url"].as_str().unwrap()).unwrap()
}

pub fn query(url: &Url, name: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
}

/// The callback URL the provider redirects the browser to.
pub fn callback_path(params: &[(&str, &str)]) -> String {
    let query: String = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params)
        .finish();
    format!("/api/v1/platforms/{OAUTH}/oauth/callback?{query}")
}

/// The browser following the provider's redirect: no bearer token, no body.
pub async fn callback(app: &TestApp, params: &[(&str, &str)]) -> TestResponse {
    app.call(Method::GET, &callback_path(params), None, None)
        .await
}

/// The `Location` of a redirect.
pub fn location(res: &TestResponse) -> Url {
    assert_eq!(res.status, StatusCode::SEE_OTHER, "{:?}", res.body);
    Url::parse(
        res.headers
            .get(header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .unwrap(),
    )
    .unwrap()
}

/// Starts as `token`, has the provider approve, and follows the callback. Returns the
/// callback's response.
pub async fn authorize(f: &Fixture, token: &str) -> TestResponse {
    let url = authorize_url(&start_as(&f.app, token, None, DONE).await);
    let code = f.provider.approve(&url);
    let state = query(&url, "state").unwrap();
    callback(&f.app, &[("code", &code), ("state", &state)]).await
}

/// A pending result of `token`'s user: its `connect_id`.
pub async fn pending_result(f: &Fixture, token: &str) -> String {
    let redirect = location(&authorize(f, token).await);
    assert_eq!(
        query(&redirect, "result").as_deref(),
        Some("ok"),
        "{redirect}"
    );
    query(&redirect, "connect_id").unwrap()
}

/// The hex HMAC-SHA256 of `body` keyed with `secret`, as `mock_oauth` verifies events.
pub fn sign(body: &[u8], secret: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    hex::encode(mac.finalize().into_bytes())
}

/// `POST /platforms/{platform}/events/{kind}` with `body` and an optional signature header.
pub async fn event(
    app: &TestApp,
    platform: &str,
    kind: &str,
    body: Vec<u8>,
    signature: Option<&str>,
) -> TestResponse {
    let mut b = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/v1/platforms/{platform}/events/{kind}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CONTENT_LENGTH, body.len());
    if let Some(signature) = signature {
        b = b.header(SIGNATURE, signature);
    }
    app.send(b.body(Body::from(body)).unwrap()).await
}

/// A signed `deauthorize` event body naming `user`, and its signature.
pub fn deauthorize(user: &str) -> (Vec<u8>, String) {
    let body = serde_json::to_vec(&json!({ "user_id": user })).unwrap();
    let signature = sign(&body, CLIENT_SECRET);
    (body, signature)
}
