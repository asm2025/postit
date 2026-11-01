//! The public email routes: unsubscribe pages, click redirects, and the open pixel.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use postit_api::testkit::{TEST_EMAIL_SIGNING_KEY, TestApp, TestResponse};
use postit_contacts::NewContactInput;
use postit_core::{
    DirectRecipient, DirectSendId, EmailLanguage, LinkSignerSource as _, TEST_SEND_ID, UserId,
};
use postit_data::OwnerScope;
use postit_data::email_hash::email_hash;
use postit_data::testkit::seed_user;
use sqlx::PgPool;
use url::Url;
use uuid::Uuid;

const ADDRESS: &str = "reader@example.com";
const CSP: &str =
    "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'";

/// An active owner with a limits row, so contacts can be added.
async fn owner(pool: &PgPool) -> UserId {
    let owner = seed_user(pool, "active").await;
    sqlx::query(
        "INSERT INTO user_limits (user_id, email_daily_cap, contacts_cap, storage_quota_bytes)
         VALUES ($1, 100, 0, 1000000)",
    )
    .bind(owner.as_uuid())
    .execute(pool)
    .await
    .unwrap();
    owner
}

async fn add_contact(app: &TestApp, owner: UserId) -> Uuid {
    let contact = app
        .state
        .contacts
        .create(
            &OwnerScope::own(owner),
            NewContactInput {
                email: ADDRESS.to_string(),
                first_name: None,
                last_name: None,
                fields: None,
                tags: None,
                locale: None,
                consent_source: "test".to_string(),
                consent_at: None,
            },
        )
        .await
        .unwrap();
    contact.id.as_uuid()
}

/// A `direct_sends` row for [`ADDRESS`], without the delivery chain above it.
async fn seed_send(pool: &PgPool, owner: UserId) -> DirectSendId {
    let id = Uuid::now_v7();
    let mut conn = pool.acquire().await.unwrap();
    sqlx::query("SET session_replication_role = replica")
        .execute(&mut *conn)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO direct_sends (id, owner_id, delivery_id, email_hash, state)
         VALUES ($1, $2, $3, $4, 'sent')",
    )
    .bind(id)
    .bind(owner.as_uuid())
    .bind(Uuid::now_v7())
    .bind(email_hash(TEST_EMAIL_SIGNING_KEY, ADDRESS).to_vec())
    .execute(&mut *conn)
    .await
    .unwrap();
    sqlx::query("SET session_replication_role = DEFAULT")
        .execute(&mut *conn)
        .await
        .unwrap();
    DirectSendId::from_uuid(id)
}

fn recipient(send: DirectSendId) -> DirectRecipient {
    DirectRecipient {
        send_id: send,
        address: ADDRESS.to_string(),
        merge_fields: BTreeMap::new(),
        locale: String::new(),
    }
}

/// The path and query of a signed URL, as the router sees it.
fn local(url: &Url) -> String {
    match url.query() {
        Some(q) => format!("{}?{q}", url.path()),
        None => url.path().to_string(),
    }
}

fn unsub_path(app: &TestApp, owner: UserId, send: DirectSendId, lang: EmailLanguage) -> String {
    local(
        &app.state
            .email_links
            .for_owner(owner)
            .unsubscribe_url(&recipient(send), lang),
    )
}

fn click_path(app: &TestApp, owner: UserId, send: DirectSendId, to: &str) -> String {
    local(
        &app.state
            .email_links
            .for_owner(owner)
            .click_url(send, &Url::parse(to).unwrap()),
    )
}

fn open_path(app: &TestApp, owner: UserId, send: DirectSendId) -> String {
    local(&app.state.email_links.for_owner(owner).open_pixel_url(send))
}

/// The same path with one bit of the token flipped.
fn tampered(path: &str) -> String {
    let (prefix, token) = path.rsplit_once('/').unwrap();
    let mut raw = URL_SAFE_NO_PAD.decode(token).unwrap();
    raw[0] ^= 1;
    format!("{prefix}/{}", URL_SAFE_NO_PAD.encode(raw))
}

async fn get(app: &TestApp, path: &str) -> TestResponse {
    app.send(Request::get(path).body(Body::empty()).unwrap())
        .await
}

async fn post(app: &TestApp, path: &str) -> TestResponse {
    app.send(
        Request::post(path)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from("List-Unsubscribe=One-Click"))
            .unwrap(),
    )
    .await
}

fn text(res: &TestResponse) -> String {
    String::from_utf8_lossy(&res.bytes).into_owned()
}

fn header_str<'a>(res: &'a TestResponse, name: &str) -> &'a str {
    res.headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
}

async fn suppression(pool: &PgPool, owner: UserId) -> Option<String> {
    sqlx::query_scalar("SELECT reason FROM suppressions WHERE owner_id = $1 AND email_hash = $2")
        .bind(owner.as_uuid())
        .bind(email_hash(TEST_EMAIL_SIGNING_KEY, ADDRESS).to_vec())
        .fetch_optional(pool)
        .await
        .unwrap()
}

async fn contact_status(pool: &PgPool, contact: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM contacts WHERE id = $1")
        .bind(contact)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn events(pool: &PgPool, send: DirectSendId, kind: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM email_events WHERE direct_send_id = $1 AND kind = $2")
        .bind(send.as_uuid())
        .bind(kind)
        .fetch_one(pool)
        .await
        .unwrap()
}

fn tight(app_settings: &mut postit_api::ApiSettings) {
    app_settings.rate_limit.unauthenticated = postit_config::RateBucket {
        rate_per_minute: 1,
        burst: 2,
    };
}

#[sqlx::test(migrations = "../data/migrations")]
async fn get_changes_nothing(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let owner = owner(&pool).await;
    let contact = add_contact(&app, owner).await;
    let send = seed_send(&pool, owner).await;
    let res = get(&app, &unsub_path(&app, owner, send, EmailLanguage::EnUs)).await;
    assert_eq!(res.status, StatusCode::OK);
    assert!(header_str(&res, "content-type").starts_with("text/html"));
    let body = text(&res);
    assert!(body.contains("<form method=\"post\""), "{body}");
    assert!(body.contains("Unsubscribe"), "{body}");
    assert_eq!(suppression(&pool, owner).await, None);
    assert_eq!(contact_status(&pool, contact).await, "subscribed");
    assert_eq!(events(&pool, send, "unsubscribed").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn post_suppresses_and_marks_contact(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let owner = owner(&pool).await;
    let contact = add_contact(&app, owner).await;
    let send = seed_send(&pool, owner).await;
    let res = post(&app, &unsub_path(&app, owner, send, EmailLanguage::EnUs)).await;
    assert_eq!(res.status, StatusCode::OK);
    assert!(
        text(&res).contains("You are unsubscribed"),
        "{}",
        text(&res)
    );
    assert_eq!(
        suppression(&pool, owner).await.as_deref(),
        Some("unsubscribed")
    );
    assert_eq!(contact_status(&pool, contact).await, "unsubscribed");
    assert_eq!(events(&pool, send, "unsubscribed").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn rfc8058_one_click_post(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let owner = owner(&pool).await;
    let send = seed_send(&pool, owner).await;
    // What a mail client sends for `List-Unsubscribe-Post: List-Unsubscribe=One-Click`.
    let res = app
        .send(
            Request::post(unsub_path(&app, owner, send, EmailLanguage::EnUs))
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(header::CONTENT_LENGTH, "26")
                .body(Body::from("List-Unsubscribe=One-Click"))
                .unwrap(),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(
        suppression(&pool, owner).await.as_deref(),
        Some("unsubscribed")
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn one_click_ignores_the_body(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let owner = owner(&pool).await;
    let send = seed_send(&pool, owner).await;
    let path = unsub_path(&app, owner, send, EmailLanguage::EnUs);
    let bodies: [(Option<&str>, Body); 4] = [
        (None, Body::empty()),
        (Some("application/json"), Body::from("{\"not\": \"a form\"")),
        (
            Some("application/octet-stream"),
            Body::from(vec![0xffu8; 300]),
        ),
        (Some("text/plain"), Body::from("List-Unsubscribe=No")),
    ];
    for (content_type, body) in bodies {
        let mut req = Request::post(&path);
        if let Some(ct) = content_type {
            req = req.header(header::CONTENT_TYPE, ct);
        }
        let res = app.send(req.body(body).unwrap()).await;
        assert_eq!(res.status, StatusCode::OK, "{content_type:?}");
        assert!(text(&res).contains("You are unsubscribed"));
    }
    assert_eq!(
        suppression(&pool, owner).await.as_deref(),
        Some("unsubscribed")
    );
    assert_eq!(events(&pool, send, "unsubscribed").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn tampered_token_404_page(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let owner = owner(&pool).await;
    let send = seed_send(&pool, owner).await;
    let bad = tampered(&unsub_path(&app, owner, send, EmailLanguage::EnUs));
    for res in [get(&app, &bad).await, post(&app, &bad).await] {
        assert_eq!(res.status, StatusCode::NOT_FOUND);
        assert!(header_str(&res, "content-type").starts_with("text/html"));
        assert!(
            text(&res).contains("This link does not work"),
            "{}",
            text(&res)
        );
    }
    // A token of another kind is not an unsubscribe token.
    let open = open_path(&app, owner, send);
    let token = open.rsplit_once('/').unwrap().1;
    let crossed = post(&app, &format!("/public/u/{token}")).await;
    assert_eq!(crossed.status, StatusCode::NOT_FOUND);
    assert_eq!(
        get(&app, "/public/u/not-a-token").await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(suppression(&pool, owner).await, None);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn page_in_token_language(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let owner = owner(&pool).await;
    let send = seed_send(&pool, owner).await;

    let ar = get(&app, &unsub_path(&app, owner, send, EmailLanguage::Ar)).await;
    let body = text(&ar);
    assert!(body.contains("lang=\"ar\" dir=\"rtl\""), "{body}");
    assert!(body.contains("إلغاء الاشتراك"), "{body}");
    // The token decides, not the browser.
    let en = app
        .send(
            Request::get(unsub_path(&app, owner, send, EmailLanguage::EnUs))
                .header(header::ACCEPT_LANGUAGE, "ar")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert!(
        text(&en).contains("lang=\"en-US\" dir=\"ltr\""),
        "{}",
        text(&en)
    );

    let done = post(&app, &unsub_path(&app, owner, send, EmailLanguage::Ar)).await;
    assert!(text(&done).contains("dir=\"rtl\""));
    assert!(text(&done).contains("تم إلغاء اشتراكك"), "{}", text(&done));

    // A bad token has no language: the browser's is used.
    let invalid = app
        .send(
            Request::get("/public/u/nothing")
                .header(header::ACCEPT_LANGUAGE, "ar")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert!(text(&invalid).contains("dir=\"rtl\""), "{}", text(&invalid));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn pages_carry_security_headers(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let owner = owner(&pool).await;
    let send = seed_send(&pool, owner).await;
    let path = unsub_path(&app, owner, send, EmailLanguage::EnUs);
    let test = unsub_path(&app, owner, TEST_SEND_ID, EmailLanguage::EnUs);
    for res in [
        get(&app, &path).await,
        post(&app, &path).await,
        get(&app, &test).await,
        get(&app, &tampered(&path)).await,
        get(&app, "/public/t/c/nothing").await,
    ] {
        assert_eq!(header_str(&res, "content-security-policy"), CSP);
        assert_eq!(header_str(&res, "referrer-policy"), "no-referrer");
        assert_eq!(header_str(&res, "cache-control"), "no-store");
        assert_eq!(header_str(&res, "x-content-type-options"), "nosniff");
        assert_eq!(header_str(&res, "content-type"), "text/html; charset=utf-8");
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn test_token_suppresses_nothing(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let owner = owner(&pool).await;
    let contact = add_contact(&app, owner).await;
    let path = unsub_path(&app, owner, TEST_SEND_ID, EmailLanguage::EnUs);
    for res in [get(&app, &path).await, post(&app, &path).await] {
        assert_eq!(res.status, StatusCode::OK);
        assert!(
            text(&res).contains("This was a test email"),
            "{}",
            text(&res)
        );
        assert!(!text(&res).contains("<form"), "{}", text(&res));
    }
    assert_eq!(suppression(&pool, owner).await, None);
    assert_eq!(contact_status(&pool, contact).await, "subscribed");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn click_redirects_to_signed_url_only(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let owner = owner(&pool).await;
    let send = seed_send(&pool, owner).await;
    let to = "https://shop.example/sale?item=7&ref=mail";
    let path = click_path(&app, owner, send, to);
    let res = get(&app, &path).await;
    assert_eq!(res.status, StatusCode::SEE_OTHER);
    assert_eq!(header_str(&res, "location"), to);
    assert_eq!(header_str(&res, "referrer-policy"), "no-referrer");
    assert_eq!(header_str(&res, "cache-control"), "no-store");
    // Nothing outside the token steers the redirect.
    let steered = get(&app, &format!("{path}?url=https://evil.example/")).await;
    assert_eq!(steered.status, StatusCode::SEE_OTHER);
    assert_eq!(header_str(&steered, "location"), to);
    let bad = get(&app, &tampered(&path)).await;
    assert_eq!(bad.status, StatusCode::NOT_FOUND);
    assert!(bad.headers.get(header::LOCATION).is_none());
    // An unsubscribe token is not a click token.
    let unsub = unsub_path(&app, owner, send, EmailLanguage::EnUs);
    let token = unsub.rsplit_once('/').unwrap().1;
    let crossed = get(&app, &format!("/public/t/c/{token}")).await;
    assert_eq!(crossed.status, StatusCode::NOT_FOUND);
    assert!(crossed.headers.get(header::LOCATION).is_none());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn click_records_event(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let owner = owner(&pool).await;
    let send = seed_send(&pool, owner).await;
    let path = click_path(&app, owner, send, "https://shop.example/");
    for _ in 0..2 {
        assert_eq!(get(&app, &path).await.status, StatusCode::SEE_OTHER);
    }
    // Clicks repeat and every one is kept.
    assert_eq!(events(&pool, send, "clicked").await, 2);
    // A forged owner cannot record against someone else's send: the token carries no owner.
    let other = seed_user(&pool, "active").await;
    let theirs = click_path(&app, other, send, "https://shop.example/");
    assert_eq!(get(&app, &theirs).await.status, StatusCode::SEE_OTHER);
    let owner_of: Uuid =
        sqlx::query_scalar("SELECT DISTINCT owner_id FROM email_events WHERE direct_send_id = $1")
            .bind(send.as_uuid())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(owner_of, owner.as_uuid());
}

#[sqlx::test(migrations = "../data/migrations")]
async fn click_after_purge_still_redirects(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let owner = owner(&pool).await;
    let send = seed_send(&pool, owner).await;
    let path = click_path(&app, owner, send, "https://shop.example/x");
    sqlx::query("DELETE FROM direct_sends WHERE id = $1")
        .bind(send.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let res = get(&app, &path).await;
    assert_eq!(res.status, StatusCode::SEE_OTHER);
    assert_eq!(header_str(&res, "location"), "https://shop.example/x");
    let open = get(&app, &open_path(&app, owner, send)).await;
    assert_eq!(open.status, StatusCode::OK);
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM email_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn open_pixel_returns_gif_and_records(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let owner = owner(&pool).await;
    let send = seed_send(&pool, owner).await;
    let res = get(&app, &open_path(&app, owner, send)).await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(header_str(&res, "content-type"), "image/gif");
    assert_eq!(header_str(&res, "cache-control"), "no-store");
    assert!(res.bytes.starts_with(b"GIF89a"));
    let img = image::load_from_memory_with_format(&res.bytes, image::ImageFormat::Gif).unwrap();
    assert_eq!((img.width(), img.height()), (1, 1));
    assert_eq!(events(&pool, send, "opened").await, 1);
    let bad = get(&app, &tampered(&open_path(&app, owner, send))).await;
    assert_eq!(bad.status, StatusCode::NOT_FOUND);
    assert_eq!(events(&pool, send, "opened").await, 1);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn bad_tokens_penalize_the_ip(pool: PgPool) {
    let app = TestApp::start_with(pool.clone(), tight).await;
    for route in [
        "/public/u/forged",
        "/public/t/c/forged",
        "/public/t/o/forged",
    ] {
        let mut statuses = Vec::new();
        for _ in 0..6 {
            statuses.push(get(&app, route).await.status);
        }
        assert!(
            statuses.contains(&StatusCode::TOO_MANY_REQUESTS),
            "{route}: {statuses:?}"
        );
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn good_tokens_are_never_limited(pool: PgPool) {
    let app = TestApp::start_with(pool.clone(), tight).await;
    let owner = owner(&pool).await;
    let send = seed_send(&pool, owner).await;
    // The address is already over its limit from forged tokens.
    let mut statuses = Vec::new();
    for _ in 0..6 {
        statuses.push(get(&app, "/public/u/forged").await.status);
    }
    assert!(statuses.contains(&StatusCode::TOO_MANY_REQUESTS));
    // Everyone behind one mail proxy still gets through, many times over.
    let unsub = unsub_path(&app, owner, send, EmailLanguage::EnUs);
    let click = click_path(&app, owner, send, "https://shop.example/");
    let open = open_path(&app, owner, send);
    for _ in 0..10 {
        assert_eq!(get(&app, &unsub).await.status, StatusCode::OK);
        assert_eq!(post(&app, &unsub).await.status, StatusCode::OK);
        assert_eq!(get(&app, &click).await.status, StatusCode::SEE_OTHER);
        assert_eq!(get(&app, &open).await.status, StatusCode::OK);
    }
}

#[sqlx::test(migrations = "../data/migrations")]
async fn over_limit_click_redirects_without_recording(pool: PgPool) {
    let app = TestApp::start_with(pool.clone(), tight).await;
    let owner = owner(&pool).await;
    let send = seed_send(&pool, owner).await;
    let click = click_path(&app, owner, send, "https://shop.example/");
    for _ in 0..6 {
        assert_eq!(get(&app, &click).await.status, StatusCode::SEE_OTHER);
    }
    // The burst of two records; the rest redirect without a row.
    assert_eq!(events(&pool, send, "clicked").await, 2);
    let open = open_path(&app, owner, send);
    assert_eq!(get(&app, &open).await.status, StatusCode::OK);
    assert_eq!(events(&pool, send, "opened").await, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unsubscribe_survives_everything(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let owner = owner(&pool).await;
    let contact = add_contact(&app, owner).await;
    let send = seed_send(&pool, owner).await;
    let path = unsub_path(&app, owner, send, EmailLanguage::EnUs);
    let done = |res: TestResponse| {
        assert_eq!(res.status, StatusCode::OK, "{}", text(&res));
        assert!(
            text(&res).contains("You are unsubscribed"),
            "{}",
            text(&res)
        );
    };

    done(post(&app, &path).await);
    done(post(&app, &path).await);
    assert_eq!(events(&pool, send, "unsubscribed").await, 1);

    // The contact is gone.
    app.state
        .contacts
        .delete(
            &OwnerScope::own(owner),
            postit_core::ContactId::from_uuid(contact),
        )
        .await
        .unwrap();
    done(post(&app, &path).await);
    assert_eq!(
        suppression(&pool, owner).await.as_deref(),
        Some("unsubscribed")
    );

    // The sends are purged, and the suppression is cleared first so this stage proves the
    // POST itself suppresses with no send row, rather than finding an earlier entry.
    sqlx::query("DELETE FROM suppressions WHERE owner_id = $1")
        .bind(owner.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM direct_sends WHERE owner_id = $1")
        .bind(owner.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(suppression(&pool, owner).await, None);
    done(post(&app, &path).await);
    assert_eq!(
        suppression(&pool, owner).await.as_deref(),
        Some("unsubscribed")
    );
    let page = get(&app, &path).await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(text(&page).contains("<form"));

    // The owner's account is gone: the insert fails on the owner's foreign key, and that is
    // treated as done. No row is left behind.
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(owner.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    done(post(&app, &path).await);
    done(post(&app, &path).await);
    assert_eq!(suppression(&pool, owner).await, None);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unsubscribe_after_purge_suppresses(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let owner = owner(&pool).await;
    let contact = add_contact(&app, owner).await;
    let send = seed_send(&pool, owner).await;
    let path = unsub_path(&app, owner, send, EmailLanguage::EnUs);
    // Purged before anyone unsubscribed: there is no send row and no suppression.
    sqlx::query("DELETE FROM direct_sends WHERE id = $1")
        .bind(send.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let res = post(&app, &path).await;
    assert_eq!(res.status, StatusCode::OK);
    assert!(text(&res).contains("You are unsubscribed"));
    assert_eq!(
        suppression(&pool, owner).await.as_deref(),
        Some("unsubscribed")
    );
    assert_eq!(contact_status(&pool, contact).await, "unsubscribed");
    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM email_events")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn act_as_header_is_refused(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let owner = owner(&pool).await;
    let send = seed_send(&pool, owner).await;
    let res = app
        .send(
            Request::builder()
                .method(Method::POST)
                .uri(unsub_path(&app, owner, send, EmailLanguage::EnUs))
                .header("x-postit-act-as", Uuid::now_v7().to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);
    assert_eq!(suppression(&pool, owner).await, None);
}
