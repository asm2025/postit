//! The contacts, lists, import, export, and suppression routes.
#![allow(clippy::unwrap_used)]

use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::{Method, Request, StatusCode, header};
use futures::StreamExt as _;
use postit_api::testkit::{ADMIN_SUB, TEST_EMAIL_SIGNING_KEY, TestApp, TestResponse};
use postit_core::UserId;
use postit_data::contacts::{SuppressionReason, SuppressionRepo};
use postit_data::testkit::grant;
use postit_data::{AccountLimit, Level, OwnerScope};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt as _;
use uuid::Uuid;

const ACT_AS: &str = "x-postit-act-as";
const BOUNDARY: &str = "postit-test-boundary";

async fn active_user(app: &TestApp, sub: &str) -> (String, UserId) {
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
            Some(json!({ "status": "active" })),
        )
        .await;
    assert_eq!(approved.status, StatusCode::OK, "{:?}", approved.body);
    (token, UserId::from(id))
}

async fn call_as(
    app: &TestApp,
    method: Method,
    path: &str,
    token: Option<&str>,
    owner: Option<UserId>,
    body: Option<Value>,
) -> TestResponse {
    let mut b = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        b = b.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
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

fn assert_code(res: &TestResponse, status: StatusCode, code: &str, what: &str) {
    assert_eq!(res.status, status, "{what}: {:?}", res.body);
    assert_eq!(res.body["code"], code, "{what}");
}

fn options(attest: bool) -> Value {
    json!({
        "attest_consent": attest,
        "columns": { "email": "email", "consent_source": "consent" }
    })
}

/// A multipart body with the given parts, in order: `(name, filename, content type, bytes)`.
fn multipart(parts: &[(&str, Option<&str>, &str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, filename, content_type, bytes) in parts {
        out.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        let disposition = match filename {
            Some(f) => format!("form-data; name=\"{name}\"; filename=\"{f}\""),
            None => format!("form-data; name=\"{name}\""),
        };
        out.extend_from_slice(format!("Content-Disposition: {disposition}\r\n").as_bytes());
        out.extend_from_slice(format!("Content-Type: {content_type}\r\n\r\n").as_bytes());
        out.extend_from_slice(bytes);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    out
}

fn import_body(options: &Value, csv: &[u8]) -> Vec<u8> {
    multipart(&[
        (
            "options",
            None,
            "application/json",
            options.to_string().as_bytes(),
        ),
        ("file", Some("contacts.csv"), "text/csv", csv),
    ])
}

fn import_request(
    token: &str,
    owner: Option<UserId>,
    key: Option<&str>,
    body: Body,
    length: usize,
) -> Request<Body> {
    let mut b = Request::post("/api/v1/contacts/import")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .header(header::CONTENT_LENGTH, length);
    if let Some(owner) = owner {
        b = b.header(ACT_AS, owner.to_string());
    }
    if let Some(key) = key {
        b = b.header("idempotency-key", key);
    }
    b.body(body).unwrap()
}

async fn import(app: &TestApp, token: &str, owner: Option<UserId>, csv: &[u8]) -> TestResponse {
    let body = import_body(&options(true), csv);
    let len = body.len();
    app.send(import_request(token, owner, None, Body::from(body), len))
        .await
}

fn export_request(token: &str, owner: Option<UserId>) -> Request<Body> {
    let mut b = Request::get("/api/v1/contacts/export")
        .header(header::AUTHORIZATION, format!("Bearer {token}"));
    if let Some(owner) = owner {
        b = b.header(ACT_AS, owner.to_string());
    }
    b.body(Body::empty()).unwrap()
}

async fn create_contact(app: &TestApp, token: &str, email: &str) -> String {
    let res = app
        .call(
            Method::POST,
            "/api/v1/contacts",
            Some(token),
            Some(json!({ "email": email, "consent_source": "signup form" })),
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    res.body["id"].as_str().unwrap().to_owned()
}

async fn create_list(app: &TestApp, token: &str, name: &str) -> String {
    let res = app
        .call(
            Method::POST,
            "/api/v1/contact-lists",
            Some(token),
            Some(json!({ "name": name })),
        )
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    res.body["id"].as_str().unwrap().to_owned()
}

async fn suppress(pool: &PgPool, owner: UserId, email: &str, reason: SuppressionReason) {
    let mut conn = pool.acquire().await.unwrap();
    SuppressionRepo::add(
        &mut conn,
        &OwnerScope::own(owner),
        TEST_EMAIL_SIGNING_KEY,
        email,
        reason,
    )
    .await
    .unwrap();
}

/// The ID of the suppression entry the owner's list shows for `email`.
async fn suppression_id(app: &TestApp, token: &str, email: &str) -> String {
    let res = app
        .call(Method::GET, "/api/v1/suppressions", Some(token), None)
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    res.body["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["email"] == email)
        .and_then(|s| s["id"].as_str())
        .unwrap_or_else(|| unreachable!("no entry for {email}: {:?}", res.body))
        .to_owned()
}

/// What the owner's workspace holds for one caller of the matrix to act on.
struct Fixture {
    contact: String,
    list: String,
    member: String,
    suppression: String,
    import: String,
}

async fn fixture(app: &TestApp, owner: &str, owner_id: UserId, tag: &str) -> Fixture {
    let contact = create_contact(app, owner, &format!("{tag}-c@example.com")).await;
    let member = create_contact(app, owner, &format!("{tag}-m@example.com")).await;
    let list = create_list(app, owner, &format!("list {tag}")).await;
    let added = app
        .call(
            Method::PUT,
            &format!("/api/v1/contact-lists/{list}/members/{member}"),
            Some(owner),
            None,
        )
        .await;
    assert_eq!(added.status, StatusCode::NO_CONTENT, "{:?}", added.body);
    let bounced = format!("{tag}-b@example.com");
    create_contact(app, owner, &bounced).await;
    suppress(&app.pool, owner_id, &bounced, SuppressionReason::Bounced).await;
    let suppression = suppression_id(app, owner, &bounced).await;
    let imported = import(app, owner, None, b"email,consent\nx@example.com,event\n").await;
    assert_eq!(imported.status, StatusCode::ACCEPTED, "{:?}", imported.body);
    Fixture {
        contact,
        list,
        member,
        suppression,
        import: imported.body["id"].as_str().unwrap().to_owned(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Access {
    View,
    Edit,
    Owner,
}

/// Every route of the REST surface with its Access column.
fn routes(f: &Fixture, tag: &str) -> Vec<(Method, String, Option<Value>, Access)> {
    vec![
        (Method::GET, "/api/v1/contacts".into(), None, Access::View),
        (
            Method::POST,
            "/api/v1/contacts".into(),
            Some(json!({ "email": format!("{tag}-new@example.com"), "consent_source": "event" })),
            Access::Edit,
        ),
        (
            Method::GET,
            format!("/api/v1/contacts/{}", f.contact),
            None,
            Access::View,
        ),
        (
            Method::PATCH,
            format!("/api/v1/contacts/{}", f.contact),
            Some(json!({ "first_name": "Renamed" })),
            Access::Edit,
        ),
        (
            Method::GET,
            "/api/v1/contact-lists".into(),
            None,
            Access::View,
        ),
        (
            Method::POST,
            "/api/v1/contact-lists".into(),
            Some(json!({ "name": format!("{tag} new") })),
            Access::Edit,
        ),
        (
            Method::PATCH,
            format!("/api/v1/contact-lists/{}", f.list),
            Some(json!({ "name": format!("{tag} renamed") })),
            Access::Edit,
        ),
        (
            Method::PUT,
            format!("/api/v1/contact-lists/{}/members/{}", f.list, f.contact),
            None,
            Access::Edit,
        ),
        (
            Method::DELETE,
            format!("/api/v1/contact-lists/{}/members/{}", f.list, f.member),
            None,
            Access::Edit,
        ),
        (
            Method::GET,
            format!("/api/v1/contacts/imports/{}", f.import),
            None,
            Access::Owner,
        ),
        (
            Method::GET,
            "/api/v1/suppressions".into(),
            None,
            Access::View,
        ),
        (
            Method::DELETE,
            format!("/api/v1/suppressions/{}", f.suppression),
            None,
            Access::Owner,
        ),
        (
            Method::DELETE,
            format!("/api/v1/contact-lists/{}", f.list),
            None,
            Access::Edit,
        ),
        (
            Method::DELETE,
            format!("/api/v1/contacts/{}", f.contact),
            None,
            Access::Edit,
        ),
    ]
}

/// The four callers against every route: the owner, a `view` delegate, an `edit` delegate,
/// and an unrelated user in their own workspace. Unauthenticated calls answer 401 and an
/// unrelated user acting as the owner answers `delegation_invalid`.
#[sqlx::test(migrations = "../data/migrations")]
async fn matrix_follows_the_access_column(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let (owner, owner_id) = active_user(&app, "owner").await;
    let (viewer, viewer_id) = active_user(&app, "viewer").await;
    let (editor, editor_id) = active_user(&app, "editor").await;
    let (stranger, _) = active_user(&app, "stranger").await;
    grant(&pool, owner_id, viewer_id, Level::View, AccountLimit::All).await;
    grant(&pool, owner_id, editor_id, Level::Edit, AccountLimit::All).await;

    let callers: [(&str, &str, Option<UserId>); 4] = [
        ("owner", &owner, None),
        ("view", &viewer, Some(owner_id)),
        ("edit", &editor, Some(owner_id)),
        ("stranger", &stranger, None),
    ];
    for (who, token, act_as) in callers {
        let f = fixture(&app, &owner, owner_id, who).await;
        for (method, path, body, access) in routes(&f, who) {
            let what = format!("{who} {method} {path}");
            let res = call_as(&app, method.clone(), &path, Some(token), act_as, body).await;
            let by_id = path.contains(&f.contact)
                || path.contains(&f.list)
                || path.contains(&f.import)
                || path.contains(&f.suppression);
            match (who, access) {
                ("owner", _) | ("edit", Access::View | Access::Edit) | ("view", Access::View) => {
                    assert!(res.status.is_success(), "{what}: {:?}", res.body);
                }
                ("view", Access::Edit) => {
                    assert_code(&res, StatusCode::FORBIDDEN, "capability_required", &what);
                }
                ("view" | "edit", Access::Owner) => {
                    assert_code(&res, StatusCode::FORBIDDEN, "owner_only", &what);
                }
                _ if by_id => assert_code(&res, StatusCode::NOT_FOUND, "not_found", &what),
                _ => {
                    assert!(res.status.is_success(), "{what}: {:?}", res.body);
                    if method == Method::GET {
                        assert_eq!(res.body["total"], 0, "{what} sees nothing of the owner's");
                    }
                }
            }
        }

        let imported = import(&app, token, act_as, b"email,consent\ny@example.com,event\n").await;
        let exported = app.send(export_request(token, act_as)).await;
        if act_as.is_some() {
            assert_code(&imported, StatusCode::FORBIDDEN, "owner_only", "import");
            assert_code(&exported, StatusCode::FORBIDDEN, "owner_only", "export");
        } else {
            assert_eq!(
                imported.status,
                StatusCode::ACCEPTED,
                "{who}: {:?}",
                imported.body
            );
            assert_eq!(exported.status, StatusCode::OK, "{who}");
            let text = String::from_utf8_lossy(&exported.bytes).into_owned();
            assert_eq!(
                text.contains(&format!("{who}-m@example.com")),
                who == "owner",
                "{who}: {text}"
            );
        }
    }

    // No bearer token at all, and an unrelated user naming the owner's workspace.
    let f = fixture(&app, &owner, owner_id, "anon").await;
    for (method, path, body, _) in routes(&f, "anon") {
        let what = format!("{method} {path}");
        let res = call_as(&app, method.clone(), &path, None, None, body.clone()).await;
        assert_code(&res, StatusCode::UNAUTHORIZED, "unauthenticated", &what);
        let res = call_as(&app, method, &path, Some(&stranger), Some(owner_id), body).await;
        assert_code(&res, StatusCode::FORBIDDEN, "delegation_invalid", &what);
    }
    let res = app.send(export_request(&stranger, Some(owner_id))).await;
    assert_code(&res, StatusCode::FORBIDDEN, "delegation_invalid", "export");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn contact_round_trip(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (owner, _) = active_user(&app, "owner").await;
    let created = app
        .call(
            Method::POST,
            "/api/v1/contacts",
            Some(&owner),
            Some(json!({
                "email": " Ada@Example.com ",
                "first_name": "Ada",
                "fields": { "plan": "gold" },
                "tags": ["vip"],
                "consent_source": "signup form",
            })),
        )
        .await;
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let id = created.body["id"].as_str().unwrap();
    assert_eq!(
        created.headers.get(header::LOCATION).unwrap(),
        &format!("/api/v1/contacts/{id}")
    );
    assert_eq!(created.body["email"], "ada@example.com");
    assert_eq!(created.body["status"], "subscribed");
    assert_eq!(created.body["fields"]["plan"], "gold");
    assert_eq!(created.body["tags"], json!(["vip"]));

    let listed = app
        .call(
            Method::GET,
            "/api/v1/contacts?tag=vip&status=subscribed",
            Some(&owner),
            None,
        )
        .await;
    assert_eq!(listed.status, StatusCode::OK, "{:?}", listed.body);
    assert_eq!(listed.body["total"], 1);

    let patched = app
        .call(
            Method::PATCH,
            &format!("/api/v1/contacts/{id}"),
            Some(&owner),
            Some(json!({ "first_name": null, "status": "unsubscribed" })),
        )
        .await;
    assert_eq!(patched.status, StatusCode::OK, "{:?}", patched.body);
    assert!(patched.body["first_name"].is_null());
    assert_eq!(patched.body["status"], "unsubscribed");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn create_validates_and_respects_the_cap(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let (owner, owner_id) = active_user(&app, "owner").await;
    let bad = app
        .call(
            Method::POST,
            "/api/v1/contacts",
            Some(&owner),
            Some(json!({ "email": "not an address", "consent_source": "x" })),
        )
        .await;
    assert_code(
        &bad,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
        "email",
    );
    assert_eq!(bad.body["field"], "email");
    let no_consent = app
        .call(
            Method::POST,
            "/api/v1/contacts",
            Some(&owner),
            Some(json!({ "email": "a@example.com", "consent_source": " " })),
        )
        .await;
    assert_code(
        &no_consent,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
        "consent",
    );
    assert_eq!(no_consent.body["field"], "consent_source");

    sqlx::query("UPDATE user_limits SET contacts_cap = 1 WHERE user_id = $1")
        .bind(owner_id.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    create_contact(&app, &owner, "one@example.com").await;
    let over = app
        .call(
            Method::POST,
            "/api/v1/contacts",
            Some(&owner),
            Some(json!({ "email": "two@example.com", "consent_source": "x" })),
        )
        .await;
    assert_code(&over, StatusCode::CONFLICT, "contact_limit_exceeded", "cap");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn patch_cannot_change_email(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (owner, _) = active_user(&app, "owner").await;
    let id = create_contact(&app, &owner, "keep@example.com").await;
    let res = app
        .call(
            Method::PATCH,
            &format!("/api/v1/contacts/{id}"),
            Some(&owner),
            Some(json!({ "email": "other@example.com" })),
        )
        .await;
    assert_code(
        &res,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
        "email",
    );
    let got = app
        .call(
            Method::GET,
            &format!("/api/v1/contacts/{id}"),
            Some(&owner),
            None,
        )
        .await;
    assert_eq!(got.body["email"], "keep@example.com");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn patch_bounced_status_refused(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let (owner, owner_id) = active_user(&app, "owner").await;
    let id = create_contact(&app, &owner, "c@example.com").await;
    let res = app
        .call(
            Method::PATCH,
            &format!("/api/v1/contacts/{id}"),
            Some(&owner),
            Some(json!({ "status": "bounced" })),
        )
        .await;
    assert_code(
        &res,
        StatusCode::CONFLICT,
        "status_not_settable",
        "to bounced",
    );

    // A contact whose address bounced can be neither unsubscribed nor resubscribed by hand.
    suppress(&pool, owner_id, "c@example.com", SuppressionReason::Bounced).await;
    let res = app
        .call(
            Method::PATCH,
            &format!("/api/v1/contacts/{id}"),
            Some(&owner),
            Some(json!({ "status": "unsubscribed" })),
        )
        .await;
    assert_code(
        &res,
        StatusCode::CONFLICT,
        "status_not_settable",
        "bounced contact",
    );
    let res = app
        .call(
            Method::PATCH,
            &format!("/api/v1/contacts/{id}"),
            Some(&owner),
            Some(json!({ "status": "subscribed" })),
        )
        .await;
    assert_code(
        &res,
        StatusCode::CONFLICT,
        "suppressed",
        "resubscribe bounced",
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn suppression_delete_non_bounced_409(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let (owner, owner_id) = active_user(&app, "owner").await;
    create_contact(&app, &owner, "gone@example.com").await;
    suppress(
        &pool,
        owner_id,
        "gone@example.com",
        SuppressionReason::Unsubscribed,
    )
    .await;
    let id = suppression_id(&app, &owner, "gone@example.com").await;
    let res = app
        .call(
            Method::DELETE,
            &format!("/api/v1/suppressions/{id}"),
            Some(&owner),
            None,
        )
        .await;
    assert_code(
        &res,
        StatusCode::CONFLICT,
        "suppression_not_removable",
        "unsubscribed",
    );

    let listed = app
        .call(Method::GET, "/api/v1/suppressions", Some(&owner), None)
        .await;
    let entry = &listed.body["data"][0];
    assert_eq!(entry["reason"], "unsubscribed");
    assert!(entry.get("email_hash").is_none(), "{entry}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn import_without_attestation_400(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (owner, _) = active_user(&app, "owner").await;
    let body = import_body(&options(false), b"email,consent\na@example.com,x\n");
    let len = body.len();
    let res = app
        .send(import_request(&owner, None, None, Body::from(body), len))
        .await;
    assert_code(
        &res,
        StatusCode::BAD_REQUEST,
        "consent_attestation_required",
        "attestation",
    );
}

/// A CSV of roughly `size` bytes: a few rows, each padded by an unmapped column.
fn padded_csv(size: usize) -> Vec<u8> {
    let mut csv = b"email,consent,notes\n".to_vec();
    let mut n = 0;
    while csv.len() < size {
        n += 1;
        csv.extend_from_slice(format!("p{n}@example.com,event,").as_bytes());
        csv.extend(std::iter::repeat_n(b'x', 64 * 1024));
        csv.push(b'\n');
    }
    csv
}

#[sqlx::test(migrations = "../data/migrations")]
async fn import_over_2_mib_succeeds(pool: PgPool) {
    let app = TestApp::start_with(pool, |s| s.contacts_csv_max_bytes = 4 * 1024 * 1024).await;
    let (owner, _) = active_user(&app, "owner").await;
    let csv = padded_csv(2 * 1024 * 1024 + 100 * 1024);
    let res = import(&app, &owner, None, &csv).await;
    assert_eq!(res.status, StatusCode::ACCEPTED, "{:?}", res.body);
    assert_eq!(res.body["status"], "processing");
    assert!(res.body["rows_total"].as_i64().unwrap() > 30);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn import_over_max_bytes_413(pool: PgPool) {
    let app = TestApp::start_with(pool, |s| s.contacts_csv_max_bytes = 4096).await;
    let (owner, _) = active_user(&app, "owner").await;
    let res = import(&app, &owner, None, &padded_csv(10_000)).await;
    assert_code(
        &res,
        StatusCode::PAYLOAD_TOO_LARGE,
        "csv_too_large",
        "reader",
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn import_declared_far_over_limit_413(pool: PgPool) {
    let app = TestApp::start_with(pool, |s| s.contacts_csv_max_bytes = 4096).await;
    let (owner, _) = active_user(&app, "owner").await;
    let body = import_body(&options(true), &padded_csv(1024 * 1024));
    let len = body.len();
    let res = app
        .send(import_request(&owner, None, None, Body::from(body), len))
        .await;
    assert_code(
        &res,
        StatusCode::PAYLOAD_TOO_LARGE,
        "payload_too_large",
        "layer",
    );
    assert_eq!(res.body["max_bytes"], 4096 + 64 * 1024);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn import_file_before_options_422(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (owner, _) = active_user(&app, "owner").await;
    let body = multipart(&[
        (
            "file",
            Some("c.csv"),
            "text/csv",
            b"email,consent\na@example.com,x\n",
        ),
        (
            "options",
            None,
            "application/json",
            options(true).to_string().as_bytes(),
        ),
    ]);
    let len = body.len();
    let res = app
        .send(import_request(&owner, None, None, Body::from(body), len))
        .await;
    assert_code(
        &res,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
        "order",
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn import_needs_multipart_and_valid_options(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (owner, _) = active_user(&app, "owner").await;
    let res = app
        .send(
            Request::post("/api/v1/contacts/import")
                .header(header::AUTHORIZATION, format!("Bearer {owner}"))
                .header(header::CONTENT_TYPE, "text/csv")
                .body(Body::from("email,consent\n"))
                .unwrap(),
        )
        .await;
    assert_code(
        &res,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
        "not multipart",
    );

    let body = multipart(&[
        (
            "options",
            None,
            "application/json",
            b"{\"attest_consent\":true}",
        ),
        ("file", Some("c.csv"), "text/csv", b"email,consent\n"),
    ]);
    let len = body.len();
    let res = app
        .send(import_request(&owner, None, None, Body::from(body), len))
        .await;
    assert_code(
        &res,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
        "options",
    );

    let res = import(&app, &owner, None, b"mail,consent\na@example.com,x\n").await;
    assert_code(
        &res,
        StatusCode::UNPROCESSABLE_ENTITY,
        "csv_invalid",
        "header",
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn import_replay_returns_the_same_import(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let (owner, _) = active_user(&app, "owner").await;
    let body = import_body(&options(true), b"email,consent\na@example.com,x\n");
    let len = body.len();
    let first = app
        .send(import_request(
            &owner,
            None,
            Some("import-1"),
            Body::from(body.clone()),
            len,
        ))
        .await;
    assert_eq!(first.status, StatusCode::ACCEPTED, "{:?}", first.body);
    let again = app
        .send(import_request(
            &owner,
            None,
            Some("import-1"),
            Body::from(body),
            len,
        ))
        .await;
    assert_eq!(again.status, StatusCode::ACCEPTED, "{:?}", again.body);
    assert_eq!(again.body["id"], first.body["id"]);
    let imports: i64 = sqlx::query_scalar("SELECT count(*) FROM contact_imports")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(imports, 1);

    let mut changed = options(true);
    changed["columns"]["first_name"] = json!("name");
    let changed = import_body(&changed, b"email,consent,name\na@example.com,x,A\n");
    let len = changed.len();
    let reused = app
        .send(import_request(
            &owner,
            None,
            Some("import-1"),
            Body::from(changed),
            len,
        ))
        .await;
    assert_code(
        &reused,
        StatusCode::UNPROCESSABLE_ENTITY,
        "idempotency_key_reused",
        "other options",
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn import_body_outlives_request_timeout(pool: PgPool) {
    let app = TestApp::start_with(pool, |s| {
        s.request_timeout = Duration::from_secs(1);
        s.media_idle_timeout = Duration::from_secs(5);
    })
    .await;
    let (owner, _) = active_user(&app, "owner").await;
    let body = import_body(&options(true), b"email,consent\nslow@example.com,x\n");
    let len = body.len();
    let (head, tail) = body.split_at(body.len() - 20);
    let (head, tail) = (Bytes::copy_from_slice(head), Bytes::copy_from_slice(tail));
    let slow = futures::stream::iter([Ok::<_, std::io::Error>(head)]).chain(futures::stream::once(
        async move {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            Ok(tail)
        },
    ));
    let res = app
        .send(import_request(
            &owner,
            None,
            None,
            Body::from_stream(slow),
            len,
        ))
        .await;
    assert_eq!(res.status, StatusCode::ACCEPTED, "{:?}", res.body);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn export_outlives_request_timeout(pool: PgPool) {
    let app = TestApp::start_with(pool, |s| {
        s.request_timeout = Duration::from_secs(1);
        s.media_idle_timeout = Duration::from_secs(5);
    })
    .await;
    let (owner, _) = active_user(&app, "owner").await;
    for n in 0..3 {
        create_contact(&app, &owner, &format!("e{n}@example.com")).await;
    }
    let mut req = export_request(&owner, None);
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo(postit_api::testkit::PEER));
    let res = app.router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        res.headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap()
            .starts_with("text/csv")
    );
    let mut body = res.into_body().into_data_stream();
    let mut text = Vec::new();
    while let Some(chunk) = body.next().await {
        tokio::time::sleep(Duration::from_millis(1200)).await;
        text.extend_from_slice(&chunk.unwrap());
    }
    let text = String::from_utf8(text).unwrap();
    assert!(text.starts_with("email,"), "{text}");
    for n in 0..3 {
        assert!(text.contains(&format!("e{n}@example.com")), "{text}");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn import_progress_visible(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (owner, _) = active_user(&app, "owner").await;
    let res = import(
        &app,
        &owner,
        None,
        b"email,consent\na@example.com,x\nnot-an-address,x\nb@example.com,x\n",
    )
    .await;
    assert_eq!(res.status, StatusCode::ACCEPTED, "{:?}", res.body);
    let id = res.body["id"].as_str().unwrap();
    let got = app
        .call(
            Method::GET,
            &format!("/api/v1/contacts/imports/{id}"),
            Some(&owner),
            None,
        )
        .await;
    assert_eq!(got.status, StatusCode::OK, "{:?}", got.body);
    assert_eq!(got.body["status"], "processing");
    assert_eq!(got.body["rows_total"], 3);
    assert_eq!(got.body["rows_rejected"], 1);
    assert_eq!(got.body["report"]["entries"][0]["code"], "email_invalid");
    assert_eq!(got.body["report"]["entries"][0]["row"], 3);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn export_by_list(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (owner, _) = active_user(&app, "owner").await;
    let inside = create_contact(&app, &owner, "in@example.com").await;
    create_contact(&app, &owner, "out@example.com").await;
    let list = create_list(&app, &owner, "news").await;
    app.call(
        Method::PUT,
        &format!("/api/v1/contact-lists/{list}/members/{inside}"),
        Some(&owner),
        None,
    )
    .await;
    let res = app
        .send(
            Request::get(format!("/api/v1/contacts/export?list_id={list}"))
                .header(header::AUTHORIZATION, format!("Bearer {owner}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK);
    let text = String::from_utf8_lossy(&res.bytes);
    assert!(
        text.contains("in@example.com") && !text.contains("out@example.com"),
        "{text}"
    );
    let res = app
        .send(
            Request::get(format!(
                "/api/v1/contacts/export?list_id={}",
                Uuid::now_v7()
            ))
            .header(header::AUTHORIZATION, format!("Bearer {owner}"))
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_code(&res, StatusCode::NOT_FOUND, "not_found", "unknown list");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn import_and_export_are_rate_limited(pool: PgPool) {
    let app = TestApp::start_with(pool, |s| {
        s.rate_limit.contact_transfers = postit_config::RateBucket {
            rate_per_minute: 1,
            burst: 2,
        };
    })
    .await;
    let (owner, _) = active_user(&app, "owner").await;
    let csv = b"email,consent\na@example.com,x\n";
    assert_eq!(
        import(&app, &owner, None, csv).await.status,
        StatusCode::ACCEPTED
    );
    assert_eq!(
        app.send(export_request(&owner, None)).await.status,
        StatusCode::OK
    );
    let res = import(&app, &owner, None, csv).await;
    assert_code(
        &res,
        StatusCode::TOO_MANY_REQUESTS,
        "rate_limited",
        "import",
    );
    assert!(res.headers.contains_key(header::RETRY_AFTER));
    let res = app.send(export_request(&owner, None)).await;
    assert_code(
        &res,
        StatusCode::TOO_MANY_REQUESTS,
        "rate_limited",
        "export",
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn list_members_and_delete(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let (owner, _) = active_user(&app, "owner").await;
    let contact = create_contact(&app, &owner, "m@example.com").await;
    let list = create_list(&app, &owner, "  Weekly  ").await;
    let blank = app
        .call(
            Method::POST,
            "/api/v1/contact-lists",
            Some(&owner),
            Some(json!({ "name": " " })),
        )
        .await;
    assert_code(
        &blank,
        StatusCode::UNPROCESSABLE_ENTITY,
        "validation_failed",
        "name",
    );
    assert_eq!(blank.body["field"], "name");
    let path = format!("/api/v1/contact-lists/{list}/members/{contact}");
    for _ in 0..2 {
        let res = app.call(Method::PUT, &path, Some(&owner), None).await;
        assert_eq!(res.status, StatusCode::NO_CONTENT, "{:?}", res.body);
    }
    let lists = app
        .call(Method::GET, "/api/v1/contact-lists", Some(&owner), None)
        .await;
    assert_eq!(lists.body["data"][0]["name"], "Weekly");
    assert_eq!(lists.body["data"][0]["members"], 1);
    let res = app
        .call(
            Method::PUT,
            &format!("/api/v1/contact-lists/{list}/members/{}", Uuid::now_v7()),
            Some(&owner),
            None,
        )
        .await;
    assert_code(&res, StatusCode::NOT_FOUND, "not_found", "unknown contact");
    let res = app
        .call(
            Method::DELETE,
            &format!("/api/v1/contact-lists/{list}"),
            Some(&owner),
            None,
        )
        .await;
    assert_eq!(res.status, StatusCode::NO_CONTENT);
    let got = app
        .call(
            Method::GET,
            &format!("/api/v1/contacts/{contact}"),
            Some(&owner),
            None,
        )
        .await;
    assert_eq!(got.status, StatusCode::OK, "the contact stays");
}
