//! Shared fixtures for the D3 route tests: an app over the mock agent, an owner with two
//! connected accounts, and one caller of each kind for the four-caller isolation matrix.
#![allow(dead_code, clippy::unwrap_used)]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, header};
use postit_agent_mock::{MockAgent, MockScript, VerifyScript};
use postit_api::testkit::{ADMIN_SUB, TestApp, TestResponse};
use postit_core::{AccountId, Agent, DiscoveredAccount, PlatformId, UserId};
use postit_data::app_settings::AppSettingsRepo;
use postit_data::audit::AuditContext;
use postit_data::platforms::PlatformRepo;
use postit_data::testkit::grant;
use postit_data::{AccountLimit, Level, OwnerScope};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

pub const ACT_AS: &str = "x-postit-act-as";

/// A signed-in user.
#[derive(Clone)]
pub struct Actor {
    pub token: String,
    pub id: UserId,
}

/// Which caller of the matrix a request is sent as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    Owner,
    /// A `view` delegate of every account.
    View,
    /// An `edit` delegate of every account.
    Edit,
    /// A `publish` delegate of every account.
    Publish,
    /// A `publish` delegate limited to account `b`, so account `a` is hidden from them.
    Limited,
    /// A user with no grant.
    Stranger,
}

pub struct World {
    pub app: TestApp,
    pub pool: PgPool,
    pub script: MockScript,
    pub owner: Actor,
    pub view: Actor,
    pub edit: Actor,
    pub publish: Actor,
    pub limited: Actor,
    pub stranger: Actor,
    /// The owner's first connected account.
    pub a: AccountId,
    /// The owner's second connected account.
    pub b: AccountId,
}

pub fn mock() -> PlatformId {
    PlatformId::new("mock").unwrap()
}

/// Signs `sub` in, approves them through the admin API, and returns them.
pub async fn active_user(app: &TestApp, sub: &str) -> Actor {
    let token = app.token(sub);
    let me = app
        .call(Method::GET, "/api/v1/me", Some(&token), None)
        .await;
    let id = Uuid::parse_str(me.body["id"].as_str().unwrap()).unwrap();
    let approved = app
        .call(
            Method::PATCH,
            &format!("/api/v1/users/{id}"),
            Some(&app.token(ADMIN_SUB)),
            Some(json!({"status": "active"})),
        )
        .await;
    assert_eq!(approved.status, 200, "{:?}", approved.body);
    Actor {
        token,
        id: UserId::from(id),
    }
}

/// Connects a `mock` account with remote ID `remote` for `owner`.
pub async fn connect(app: &TestApp, script: &MockScript, owner: UserId, remote: &str) -> AccountId {
    script.set_verify(VerifyScript::Accounts(vec![DiscoveredAccount {
        remote_id: remote.to_owned(),
        display_name: format!("Account {remote}"),
        handle: None,
        avatar_url: None,
    }]));
    let scope = OwnerScope::own(owner);
    let Value::Object(values) = json!({"account_name": remote, "token": "s3cret-token-value"})
    else {
        unreachable!()
    };
    let check = app
        .state
        .accounts
        .start_credentials(&scope, &mock(), values)
        .await
        .unwrap();
    let confirmed = app
        .state
        .accounts
        .confirm(
            &scope,
            check.pending.unwrap().0.as_str(),
            None,
            AuditContext::default(),
        )
        .await
        .unwrap();
    confirmed.connected[0].0.id
}

pub async fn world(pool: PgPool) -> World {
    world_with(pool, |_| {}).await
}

pub async fn world_with(
    pool: PgPool,
    configure: impl FnOnce(&mut postit_api::ApiSettings),
) -> World {
    let agent = MockAgent::broadcast();
    let script = agent.script();
    let app = TestApp::start_with_agents(
        pool.clone(),
        vec![Arc::new(agent) as Arc<dyn Agent>],
        configure,
    )
    .await;
    PlatformRepo::upsert_registered(&pool, &[(mock(), "Mock".to_owned())])
        .await
        .unwrap();
    AppSettingsRepo::seed_defaults(&mut pool.acquire().await.unwrap(), true)
        .await
        .unwrap();
    let owner = active_user(&app, "owner").await;
    let view = active_user(&app, "viewer").await;
    let edit = active_user(&app, "editor").await;
    let publish = active_user(&app, "publisher").await;
    let limited = active_user(&app, "limited").await;
    let stranger = active_user(&app, "stranger").await;
    let a = connect(&app, &script, owner.id, "acct-a").await;
    let b = connect(&app, &script, owner.id, "acct-b").await;
    grant(&pool, owner.id, view.id, Level::View, AccountLimit::All).await;
    grant(&pool, owner.id, edit.id, Level::Edit, AccountLimit::All).await;
    grant(
        &pool,
        owner.id,
        publish.id,
        Level::Publish,
        AccountLimit::All,
    )
    .await;
    grant(
        &pool,
        owner.id,
        limited.id,
        Level::Publish,
        AccountLimit::only([b]),
    )
    .await;
    World {
        app,
        pool,
        script,
        owner,
        view,
        edit,
        publish,
        limited,
        stranger,
        a,
        b,
    }
}

impl World {
    pub fn actor(&self, who: Who) -> &Actor {
        match who {
            Who::Owner => &self.owner,
            Who::View => &self.view,
            Who::Edit => &self.edit,
            Who::Publish => &self.publish,
            Who::Limited => &self.limited,
            Who::Stranger => &self.stranger,
        }
    }

    /// Sends a JSON request as `who`; delegates act in the owner's workspace.
    pub async fn call(
        &self,
        who: Who,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> TestResponse {
        self.call_with(who, method, path, body, &[]).await
    }

    /// Like [`Self::call`], with extra headers.
    pub async fn call_with(
        &self,
        who: Who,
        method: Method,
        path: &str,
        body: Option<Value>,
        headers: &[(&str, &str)],
    ) -> TestResponse {
        let actor = self.actor(who);
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {}", actor.token));
        if matches!(who, Who::View | Who::Edit | Who::Publish | Who::Limited) {
            builder = builder.header(ACT_AS, self.owner.id.to_string());
        }
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        let request = match body {
            Some(json) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json.to_string())),
            None => builder.body(Body::empty()),
        }
        .unwrap();
        self.app.send(request).await
    }
}

/// The `ETag` header of a response.
pub fn etag(res: &TestResponse) -> String {
    res.headers
        .get(header::ETAG)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned()
}

/// The version an entity tag names: `7` for `"v7"` or `"v7-<hash>"`.
#[allow(dead_code)]
pub fn tag_version(tag: &str) -> i64 {
    let inner = tag.trim_matches('"').trim_start_matches('v');
    inner
        .split_once('-')
        .map_or(inner, |(version, _)| version)
        .parse()
        .unwrap()
}

/// A small valid PNG.
pub fn tiny_png() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([10, 20, 30, 255]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// Uploads a PNG as `token` (the caller's own workspace) and returns the media ID.
pub async fn upload_png(app: &TestApp, token: &str) -> String {
    let bytes = tiny_png();
    let req = Request::post("/api/v1/media?filename=a.png")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "image/png")
        .header(header::CONTENT_LENGTH, bytes.len())
        .body(Body::from(bytes))
        .unwrap();
    let res = app.send(req).await;
    assert_eq!(res.status, 201, "{:?}", res.body);
    res.body["id"].as_str().unwrap().to_owned()
}

impl World {
    /// Creates a post as the owner with `body` and returns the response.
    pub async fn create_post(&self, body: Value) -> TestResponse {
        self.call(Who::Owner, Method::POST, "/api/v1/posts", Some(body))
            .await
    }

    /// Creates a post as the owner targeting `targets` and returns its ID and `ETag`.
    pub async fn post_to(&self, text: &str, targets: &[AccountId]) -> (String, String) {
        let ids: Vec<String> = targets.iter().map(ToString::to_string).collect();
        let res = self
            .create_post(json!({"text": text, "targets": ids}))
            .await;
        assert_eq!(res.status, 201, "{:?}", res.body);
        (res.body["id"].as_str().unwrap().to_owned(), etag(&res))
    }
}

/// Asserts a problem response's status and code.
pub fn assert_problem(res: &TestResponse, status: u16, code: &str) {
    assert_eq!(res.status.as_u16(), status, "{code}: {:?}", res.body);
    assert_eq!(res.body["code"], code, "{:?}", res.body);
}
