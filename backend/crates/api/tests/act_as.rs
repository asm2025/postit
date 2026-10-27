//! `X-Postit-Act-As`: `Scope` resolves it against a live grant, every other authenticated
//! extractor refuses it with `owner_only`, and every route without an auth extractor refuses
//! it in a route layer.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use postit_agent_mock::{MockAgent, MockScript, VerifyScript};
use postit_api::testkit::{ADMIN_SUB, TestApp, TestResponse};
use postit_core::{Agent, DiscoveredAccount, PlatformId, UserId};
use postit_data::delegations::{DelegationRepo, EndedReason};
use postit_data::platforms::PlatformRepo;
use postit_data::testkit::grant;
use postit_data::{AccountLimit, Level};
use serde_json::json;
use sqlx::PgPool;
use uuid::Uuid;

const ACT_AS: &str = "x-postit-act-as";

/// Signs `sub` in, approves them through the admin API, and returns their token and ID.
async fn active_user(app: &TestApp, sub: &str) -> (String, UserId) {
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

fn request(method: &Method, path: &str, token: Option<&str>, act_as: &[&str]) -> Request<Body> {
    let mut b = Request::builder().method(method.clone()).uri(path);
    if let Some(t) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    for value in act_as {
        b = b.header(ACT_AS, *value);
    }
    let body = if [Method::PUT, Method::POST, Method::PATCH].contains(method) {
        b = b.header(header::CONTENT_TYPE, "application/json");
        Body::from(json!({ "locale": "ar" }).to_string())
    } else {
        Body::empty()
    };
    b.body(body).unwrap_or_else(|e| unreachable!("{e}"))
}

fn tiny_png() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([10, 20, 30, 255]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)
        .unwrap_or_else(|e| unreachable!("png: {e}"));
    out.into_inner()
}

async fn upload(app: &TestApp, token: &str) -> String {
    let bytes = tiny_png();
    let req = Request::post("/api/v1/media?filename=a.png")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, "image/png")
        .header(header::CONTENT_LENGTH, bytes.len())
        .body(Body::from(bytes))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let res = app.send(req).await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    res.body["id"].as_str().unwrap_or_default().to_owned()
}

fn without_request_id(res: &TestResponse) -> serde_json::Value {
    let mut body = res.body.clone();
    if let Some(object) = body.as_object_mut() {
        object.remove("request_id");
        object.remove("instance");
    }
    body
}

fn assert_code(res: &TestResponse, status: StatusCode, code: &str, what: &str) {
    assert_eq!(res.status, status, "{what}: {:?}", res.body);
    assert_eq!(res.body["code"], code, "{what}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn own_scope_without_header(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let res = app
        .call(
            Method::GET,
            "/api/v1/accounts",
            Some(&app.token(ADMIN_SUB)),
            None,
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn valid_header_acts_in_owner_workspace(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let (alice, alice_id) = active_user(&app, "alice").await;
    let (bob, bob_id) = active_user(&app, "bob").await;
    let media = upload(&app, &alice).await;
    grant(&pool, alice_id, bob_id, Level::View, AccountLimit::All).await;

    let header = alice_id.to_string();
    let acting = app
        .send(request(
            &Method::GET,
            "/api/v1/media",
            Some(&bob),
            &[&header],
        ))
        .await;
    assert_eq!(acting.status, StatusCode::OK, "{:?}", acting.body);
    assert_eq!(acting.body["data"][0]["id"], media.as_str());

    let own = app
        .call(Method::GET, "/api/v1/media", Some(&bob), None)
        .await;
    assert_eq!(own.status, StatusCode::OK);
    assert_eq!(own.body["data"].as_array().map_or(0, Vec::len), 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn uppercase_uuid_header_is_accepted(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let (_, alice_id) = active_user(&app, "alice").await;
    let (bob, bob_id) = active_user(&app, "bob").await;
    grant(&pool, alice_id, bob_id, Level::View, AccountLimit::All).await;
    let header = alice_id.to_string().to_uppercase();
    let res = app
        .send(request(
            &Method::GET,
            "/api/v1/media",
            Some(&bob),
            &[&header],
        ))
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn invalid_headers_are_delegation_invalid_with_identical_bodies(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let (_, alice_id) = active_user(&app, "alice").await;
    let (bob, bob_id) = active_user(&app, "bob").await;
    let (_, carol_id) = active_user(&app, "carol").await;
    grant(&pool, alice_id, bob_id, Level::View, AccountLimit::All).await;
    let revoked = grant(&pool, carol_id, bob_id, Level::View, AccountLimit::All).await;
    let mut conn = pool.acquire().await.unwrap_or_else(|e| unreachable!("{e}"));
    DelegationRepo::end(
        &mut conn,
        revoked,
        EndedReason::Revoked,
        Some(carol_id),
        chrono::Utc::now(),
    )
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));
    // A grant made while the owner was active, then the owner went back to pending: the
    // refusal comes from the owner's status, not from a missing grant.
    let (_, dora_id) = active_user(&app, "dora").await;
    grant(&pool, dora_id, bob_id, Level::View, AccountLimit::All).await;
    let reachable = app
        .send(request(
            &Method::GET,
            "/api/v1/media",
            Some(&bob),
            &[&dora_id.to_string()],
        ))
        .await;
    assert_eq!(reachable.status, StatusCode::OK, "{:?}", reachable.body);
    sqlx::query("UPDATE users SET status = 'pending' WHERE id = $1")
        .bind(dora_id.as_uuid())
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let pending = dora_id;

    let alice = alice_id.to_string();
    let spaced = format!(" {alice}");
    let braced = format!("{{{alice}}}");
    let urn = format!("urn:uuid:{alice}");
    let simple = alice.replace('-', "");
    let own = bob_id.to_string();
    let unknown = Uuid::now_v7().to_string();
    let pending = pending.to_string();
    let carol = carol_id.to_string();
    let cases: Vec<(&str, Vec<&str>)> = vec![
        ("not a uuid", vec!["not-a-uuid"]),
        ("empty", vec![""]),
        ("leading space", vec![&spaced]),
        ("braced", vec![&braced]),
        ("urn", vec![&urn]),
        ("unhyphenated", vec![&simple]),
        ("two values", vec![&alice, &alice]),
        ("own id", vec![&own]),
        ("unknown id", vec![&unknown]),
        ("pending user", vec![&pending]),
        ("revoked grant", vec![&carol]),
    ];
    let mut first: Option<serde_json::Value> = None;
    for (what, values) in cases {
        let res = app
            .send(request(&Method::GET, "/api/v1/media", Some(&bob), &values))
            .await;
        assert_code(&res, StatusCode::FORBIDDEN, "delegation_invalid", what);
        assert!(res.body["detail"].is_null(), "{what}: detail leaked");
        let body = without_request_id(&res);
        match &first {
            None => first = Some(body),
            Some(expected) => assert_eq!(&body, expected, "{what}"),
        }
    }
    // The valid grant still resolves, so the refusals above are about the header alone.
    let ok = app
        .send(request(
            &Method::GET,
            "/api/v1/media",
            Some(&bob),
            &[&alice],
        ))
        .await;
    assert_eq!(ok.status, StatusCode::OK);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn self_routes_refuse_header_as_owner_only(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let (_, alice_id) = active_user(&app, "alice").await;
    let (bob, bob_id) = active_user(&app, "bob").await;
    grant(&pool, alice_id, bob_id, Level::Publish, AccountLimit::All).await;
    let header = alice_id.to_string();
    for (method, path) in [
        (Method::GET, "/api/v1/me"),
        (Method::GET, "/api/v1/me/preferences"),
        (Method::PUT, "/api/v1/me/preferences"),
    ] {
        let what = format!("{method} {path}");
        let res = app
            .send(request(&method, path, Some(&bob), &[&header]))
            .await;
        assert_code(&res, StatusCode::FORBIDDEN, "owner_only", &what);
    }
    let prefs = app
        .call(Method::GET, "/api/v1/me/preferences", Some(&bob), None)
        .await;
    assert_eq!(
        prefs.body["locale"], "en-US",
        "the refused PUT changed nothing"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn admin_routes_refuse_header_as_owner_only(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let admin = app.token(ADMIN_SUB);
    let admin_id = app
        .call(Method::GET, "/api/v1/me", Some(&admin), None)
        .await
        .body["id"]
        .as_str()
        .and_then(|s| Uuid::parse_str(s).ok())
        .map_or_else(|| unreachable!("admin id"), UserId::from);
    let (_, alice_id) = active_user(&app, "alice").await;
    grant(&pool, alice_id, admin_id, Level::Publish, AccountLimit::All).await;
    let header = alice_id.to_string();
    for path in ["/api/v1/admin/jobs", "/api/v1/users", "/api/v1/admin/audit"] {
        let res = app
            .send(request(&Method::GET, path, Some(&admin), &[&header]))
            .await;
        assert_code(&res, StatusCode::FORBIDDEN, "owner_only", path);
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn bad_token_with_header_is_unauthenticated(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let header = Uuid::now_v7().to_string();
    for path in ["/api/v1/media", "/api/v1/me", "/api/v1/admin/jobs"] {
        let res = app
            .send(request(&Method::GET, path, Some("not-a-jwt"), &[&header]))
            .await;
        assert_code(&res, StatusCode::UNAUTHORIZED, "unauthenticated", path);
        let anon = app
            .send(request(&Method::GET, path, None, &[&header]))
            .await;
        assert_code(&anon, StatusCode::UNAUTHORIZED, "unauthenticated", path);
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unauthenticated_routes_refuse_header(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let header = Uuid::now_v7().to_string();
    let media = format!("/public/media/{}", Uuid::now_v7());
    for (method, path) in [
        (Method::GET, "/health"),
        (Method::GET, "/ready"),
        (Method::GET, "/api/v1/auth/config"),
        (Method::GET, "/api/openapi.json"),
        (Method::GET, "/docs/"),
        (Method::GET, media.as_str()),
        (Method::GET, "/api/v1/_test/sleep?ms=1"),
        (Method::POST, "/api/v1/_test/echo"),
        (Method::GET, "/api/v1/_test/marked"),
        (Method::GET, "/api/v1/_test/raised/sleep?ms=1"),
        (Method::POST, "/api/v1/_test/raised/echo"),
    ] {
        let what = format!("{method} {path}");
        let res = app.send(request(&method, path, None, &[&header])).await;
        assert_code(&res, StatusCode::FORBIDDEN, "owner_only", &what);
    }
    // Without the header they still answer.
    let health = app.call(Method::GET, "/health", None, None).await;
    assert_eq!(health.status, StatusCode::OK);
}

/// Every operation in the contract with no `security` requirement has no auth extractor, so
/// only the route layer can refuse the header. A new unauthenticated route that misses the
/// layer fails here.
#[sqlx::test(migrations = "../data/migrations")]
async fn every_unsecured_route_refuses_header(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let header = Uuid::now_v7().to_string();
    let spec: serde_json::Value = serde_json::from_str(&postit_api::openapi::openapi_json_pretty())
        .unwrap_or_else(|e| unreachable!("spec: {e}"));
    let paths = spec["paths"]
        .as_object()
        .unwrap_or_else(|| unreachable!("no paths"));
    let placeholder = Uuid::now_v7().to_string();
    let mut checked = 0;
    for (path, item) in paths {
        let concrete = fill_params(path, &placeholder);
        for method in ["get", "post", "put", "patch", "delete", "head"] {
            let Some(operation) = item.get(method) else {
                continue;
            };
            let secured = operation
                .get("security")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|s| !s.is_empty());
            if secured {
                continue;
            }
            let m = Method::from_bytes(method.to_uppercase().as_bytes())
                .unwrap_or_else(|e| unreachable!("{e}"));
            let res = app.send(request(&m, &concrete, None, &[&header])).await;
            assert_eq!(res.status, StatusCode::FORBIDDEN, "{method} {path}");
            if method != "head" {
                assert_eq!(res.body["code"], "owner_only", "{method} {path}");
            }
            checked += 1;
        }
    }
    assert!(checked >= 5, "only {checked} unsecured operations found");
}

fn fill_params(path: &str, placeholder: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('}') else {
            break;
        };
        out.push_str(placeholder);
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    out
}

// The HTTP matrix: every level, a limited grant, and a stranger, with grants made through
// `POST /delegations`.

fn found(id: &str) -> DiscoveredAccount {
    DiscoveredAccount {
        remote_id: id.to_owned(),
        display_name: format!("Account {id}"),
        handle: None,
        avatar_url: None,
    }
}

/// One request with an optional JSON body, acting as `owner` when given.
async fn call_as(
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
    app.send(b.body(body).unwrap_or_else(|e| unreachable!("{e}")))
        .await
}

/// An owner with two connected accounts, one delegate per level, a `publish` delegate limited
/// to account B, and a stranger.
struct World {
    app: TestApp,
    pool: PgPool,
    owner: String,
    owner_id: UserId,
    a: String,
    b: String,
    view: (String, UserId),
    edit: (String, UserId),
    publish: (String, UserId),
    limited: (String, UserId),
    stranger: (String, UserId),
    /// The grant IDs, in the order view, edit, publish, limited.
    grants: Vec<String>,
}

async fn world(pool: PgPool) -> World {
    let agent = MockAgent::broadcast();
    let script: MockScript = agent.script();
    script.set_verify(VerifyScript::Accounts(vec![found("a"), found("b")]));
    let app = TestApp::start_with_agents(
        pool.clone(),
        vec![Arc::new(agent) as Arc<dyn Agent>],
        |_| {},
    )
    .await;
    PlatformRepo::upsert_registered(
        &pool,
        &[(
            PlatformId::new("mock").unwrap_or_else(|e| unreachable!("{e}")),
            "Mock".to_owned(),
        )],
    )
    .await
    .unwrap_or_else(|e| unreachable!("{e}"));

    let (owner, owner_id) = active_user(&app, "owner").await;
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
    let id_of = |name: &str| {
        confirmed.body["connected"]
            .as_array()
            .and_then(|all| {
                all.iter()
                    .find(|c| c["account"]["display_name"] == format!("Account {name}"))
            })
            .and_then(|c| c["account"]["id"].as_str())
            .unwrap_or_else(|| unreachable!("no account {name}: {:?}", confirmed.body))
            .to_owned()
    };
    let (a, b) = (id_of("a"), id_of("b"));

    let mut delegates = Vec::new();
    let mut grants = Vec::new();
    for (sub, level, accounts) in [
        ("viewer", "view", None),
        ("editor", "edit", None),
        ("publisher", "publish", None),
        ("limited", "publish", Some(vec![b.clone()])),
    ] {
        let user = active_user(&app, sub).await;
        let mut body = json!({ "delegate_id": user.1, "level": level });
        if let Some(ids) = accounts {
            body["account_ids"] = json!(ids);
        }
        let res = app
            .call(
                Method::POST,
                "/api/v1/delegations",
                Some(&owner),
                Some(body),
            )
            .await;
        assert_eq!(res.status, StatusCode::CREATED, "{sub}: {:?}", res.body);
        grants.push(res.body["id"].as_str().unwrap_or_default().to_owned());
        delegates.push(user);
    }
    let stranger = active_user(&app, "stranger").await;
    let mut it = delegates.into_iter();
    let mut next = || it.next().unwrap_or_else(|| unreachable!("four delegates"));
    let (view, edit, publish, limited) = (next(), next(), next(), next());
    World {
        app,
        pool,
        owner,
        owner_id,
        a,
        b,
        view,
        edit,
        publish,
        limited,
        stranger,
        grants,
    }
}

impl World {
    /// Every caller of the matrix: its label, token, and the workspace header it sends.
    fn callers(&self) -> Vec<(&'static str, &str, Option<UserId>)> {
        vec![
            ("owner", &self.owner, None),
            ("view", &self.view.0, Some(self.owner_id)),
            ("edit", &self.edit.0, Some(self.owner_id)),
            ("publish", &self.publish.0, Some(self.owner_id)),
            ("limited", &self.limited.0, Some(self.owner_id)),
        ]
    }

    async fn go(
        &self,
        method: Method,
        path: &str,
        who: (&str, Option<UserId>),
        body: Option<serde_json::Value>,
    ) -> TestResponse {
        call_as(&self.app, method, path, who.0, who.1, body).await
    }

    async fn set_status(&self, user: UserId, status: &str) {
        let res = self
            .app
            .call(
                Method::PATCH,
                &format!("/api/v1/users/{user}"),
                Some(&self.app.token(ADMIN_SUB)),
                Some(json!({ "status": status })),
            )
            .await;
        assert_eq!(res.status, StatusCode::OK, "{status}: {:?}", res.body);
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn matrix_unrelated_user_acting_as_owner_is_delegation_invalid(pool: PgPool) {
    let w = world(pool).await;
    for (method, path) in [
        (Method::GET, "/api/v1/accounts".to_owned()),
        (Method::GET, format!("/api/v1/accounts/{}", w.a)),
        (Method::GET, "/api/v1/media".to_owned()),
        (Method::POST, "/api/v1/media/uploads".to_owned()),
    ] {
        let res = w
            .go(
                method.clone(),
                &path,
                (&w.stranger.0, Some(w.owner_id)),
                Some(json!({ "filename": "a.png", "size": 10 })),
            )
            .await;
        assert_code(
            &res,
            StatusCode::FORBIDDEN,
            "delegation_invalid",
            &format!("{method} {path}"),
        );
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn matrix_account_reads_follow_the_account_limit(pool: PgPool) {
    let w = world(pool).await;
    for (who, token, owner) in w.callers() {
        let list = w
            .go(Method::GET, "/api/v1/accounts", (token, owner), None)
            .await;
        assert_eq!(list.status, StatusCode::OK, "{who}: {:?}", list.body);
        let ids: Vec<&str> = list.body["data"]
            .as_array()
            .map(|all| all.iter().filter_map(|a| a["id"].as_str()).collect())
            .unwrap_or_default();
        if who == "limited" {
            assert_eq!(ids, vec![w.b.as_str()], "{who} sees only B");
        } else {
            assert_eq!(ids.len(), 2, "{who}: {ids:?}");
        }

        let got_a = w
            .go(
                Method::GET,
                &format!("/api/v1/accounts/{}", w.a),
                (token, owner),
                None,
            )
            .await;
        if who == "limited" {
            assert_code(&got_a, StatusCode::NOT_FOUND, "not_found", "limited GET A");
        } else {
            assert_eq!(got_a.status, StatusCode::OK, "{who}: {:?}", got_a.body);
        }
        let got_b = w
            .go(
                Method::GET,
                &format!("/api/v1/accounts/{}", w.b),
                (token, owner),
                None,
            )
            .await;
        assert_eq!(got_b.status, StatusCode::OK, "{who}: {:?}", got_b.body);
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn matrix_account_management_is_owner_only_for_every_delegate(pool: PgPool) {
    let w = world(pool).await;
    for (who, token, owner) in w.callers() {
        let requests = [
            (
                Method::PATCH,
                format!("/api/v1/accounts/{}", w.b),
                json!({ "display_name": format!("renamed-{who}") }),
            ),
            (
                Method::POST,
                format!("/api/v1/accounts/{}/check", w.b),
                json!({}),
            ),
            (
                Method::POST,
                "/api/v1/accounts/credentials/mock".to_owned(),
                json!({ "account_name": "x", "token": "pasted-secret" }),
            ),
        ];
        for (method, path, body) in requests {
            let res = w
                .go(method.clone(), &path, (token, owner), Some(body))
                .await;
            let what = format!("{who} {method} {path}");
            if owner.is_some() {
                assert_code(&res, StatusCode::FORBIDDEN, "owner_only", &what);
            } else {
                assert!(res.status.is_success(), "{what}: {:?}", res.body);
            }
        }
    }
    let got = w
        .app
        .call(
            Method::GET,
            &format!("/api/v1/accounts/{}", w.b),
            Some(&w.owner),
            None,
        )
        .await;
    assert_eq!(
        got.body["display_name"], "renamed-owner",
        "only the rename by the owner went through"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn matrix_media_reads_for_everyone_writes_from_edit(pool: PgPool) {
    let w = world(pool).await;
    for (who, token, owner) in w.callers() {
        let list = w
            .go(Method::GET, "/api/v1/media", (token, owner), None)
            .await;
        assert_eq!(list.status, StatusCode::OK, "{who}: {:?}", list.body);

        let upload_start = w
            .go(
                Method::POST,
                "/api/v1/media/uploads",
                (token, owner),
                Some(json!({ "filename": "a.png", "size": 64 })),
            )
            .await;
        // A file the owner uploaded, for this caller to try to delete.
        let id = upload(&w.app, &w.owner).await;
        let deleted = w
            .go(
                Method::DELETE,
                &format!("/api/v1/media/{id}"),
                (token, owner),
                None,
            )
            .await;
        if who == "view" {
            assert_code(
                &upload_start,
                StatusCode::FORBIDDEN,
                "capability_required",
                "view upload",
            );
            assert_code(
                &deleted,
                StatusCode::FORBIDDEN,
                "capability_required",
                "view delete",
            );
        } else {
            assert_eq!(
                upload_start.status,
                StatusCode::CREATED,
                "{who}: {:?}",
                upload_start.body
            );
            assert_eq!(
                deleted.status,
                StatusCode::NO_CONTENT,
                "{who}: {:?}",
                deleted.body
            );
        }
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn revocation_takes_effect_on_next_request(pool: PgPool) {
    let w = world(pool).await;
    let who = (&w.edit.0[..], Some(w.owner_id));
    let before = w.go(Method::GET, "/api/v1/media", who, None).await;
    assert_eq!(before.status, StatusCode::OK, "{:?}", before.body);

    let revoked = w
        .app
        .call(
            Method::DELETE,
            &format!("/api/v1/delegations/{}", w.grants[1]),
            Some(&w.owner),
            None,
        )
        .await;
    assert!(revoked.status.is_success(), "{:?}", revoked.body);

    let after = w.go(Method::GET, "/api/v1/media", who, None).await;
    assert_code(
        &after,
        StatusCode::FORBIDDEN,
        "delegation_invalid",
        "after revoke",
    );
    // The other delegates are untouched.
    let other = w
        .go(
            Method::GET,
            "/api/v1/media",
            (&w.view.0, Some(w.owner_id)),
            None,
        )
        .await;
    assert_eq!(other.status, StatusCode::OK, "{:?}", other.body);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn level_change_takes_effect_on_next_request(pool: PgPool) {
    let w = world(pool).await;
    let who = (&w.edit.0[..], Some(w.owner_id));
    let start = json!({ "filename": "a.png", "size": 64 });
    let before = w
        .go(
            Method::POST,
            "/api/v1/media/uploads",
            who,
            Some(start.clone()),
        )
        .await;
    assert_eq!(before.status, StatusCode::CREATED, "{:?}", before.body);

    let changed = w
        .app
        .call(
            Method::PATCH,
            &format!("/api/v1/delegations/{}", w.grants[1]),
            Some(&w.owner),
            Some(json!({ "level": "view" })),
        )
        .await;
    assert_eq!(changed.status, StatusCode::OK, "{:?}", changed.body);

    let after = w
        .go(Method::POST, "/api/v1/media/uploads", who, Some(start))
        .await;
    assert_code(
        &after,
        StatusCode::FORBIDDEN,
        "capability_required",
        "after downgrade",
    );
    let read = w.go(Method::GET, "/api/v1/media", who, None).await;
    assert_eq!(read.status, StatusCode::OK, "{:?}", read.body);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disabled_owner_or_delegate_blocks_act_as(pool: PgPool) {
    let w = world(pool).await;
    let who = (&w.publish.0[..], Some(w.owner_id));
    let ok = w.go(Method::GET, "/api/v1/media", who, None).await;
    assert_eq!(ok.status, StatusCode::OK, "{:?}", ok.body);

    w.set_status(w.owner_id, "disabled").await;
    let owner_off = w.go(Method::GET, "/api/v1/media", who, None).await;
    assert_code(
        &owner_off,
        StatusCode::FORBIDDEN,
        "delegation_invalid",
        "owner disabled",
    );
    w.set_status(w.owner_id, "active").await;
    let back = w.go(Method::GET, "/api/v1/media", who, None).await;
    assert_eq!(
        back.status,
        StatusCode::OK,
        "owner re-enabled: {:?}",
        back.body
    );

    w.set_status(w.publish.1, "disabled").await;
    let delegate_off = w.go(Method::GET, "/api/v1/media", who, None).await;
    assert_eq!(
        delegate_off.status,
        StatusCode::FORBIDDEN,
        "{:?}",
        delegate_off.body
    );
    assert!(
        ["delegation_invalid", "account_disabled"]
            .contains(&delegate_off.body["code"].as_str().unwrap_or_default()),
        "{:?}",
        delegate_off.body
    );
    w.set_status(w.publish.1, "active").await;
    let again = w.go(Method::GET, "/api/v1/media", who, None).await;
    assert_eq!(
        again.status,
        StatusCode::OK,
        "delegate re-enabled: {:?}",
        again.body
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn disconnected_last_account_leaves_limited_grant_with_media_only(pool: PgPool) {
    let w = world(pool).await;
    let who = (&w.limited.0[..], Some(w.owner_id));
    let seen = w.go(Method::GET, "/api/v1/accounts", who, None).await;
    assert_eq!(seen.body["total"], 1, "{:?}", seen.body);

    // The owner disconnects B, the only account the grant reaches.
    let gone = w
        .app
        .call(
            Method::DELETE,
            &format!("/api/v1/accounts/{}", w.b),
            Some(&w.owner),
            None,
        )
        .await;
    assert_eq!(gone.status, StatusCode::OK, "{:?}", gone.body);

    let accounts = w.go(Method::GET, "/api/v1/accounts", who, None).await;
    assert_eq!(accounts.status, StatusCode::OK, "{:?}", accounts.body);
    assert_eq!(accounts.body["total"], 0, "{:?}", accounts.body);
    let by_id = w
        .go(Method::GET, &format!("/api/v1/accounts/{}", w.b), who, None)
        .await;
    assert_code(&by_id, StatusCode::NOT_FOUND, "not_found", "disconnected B");
    let other = w
        .go(Method::GET, &format!("/api/v1/accounts/{}", w.a), who, None)
        .await;
    assert_code(
        &other,
        StatusCode::NOT_FOUND,
        "not_found",
        "A is outside the grant",
    );

    // The grant is still live: the media library works, and no account is reachable.
    let media = w.go(Method::GET, "/api/v1/media", who, None).await;
    assert_eq!(media.status, StatusCode::OK, "{:?}", media.body);
    let started = w
        .go(
            Method::POST,
            "/api/v1/media/uploads",
            who,
            Some(json!({ "filename": "a.png", "size": 64 })),
        )
        .await;
    assert_eq!(started.status, StatusCode::CREATED, "{:?}", started.body);
    let live: i64 = sqlx::query_scalar("SELECT count(*) FROM delegations WHERE ended_at IS NULL")
        .fetch_one(&w.pool)
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(live, 4, "no grant ended");
}
