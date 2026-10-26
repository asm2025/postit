#![allow(clippy::unwrap_used)]

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use futures::StreamExt;
use postit_api::testkit::{ADMIN_SUB, TestApp, TestResponse};
use serde_json::json;
use sqlx::PgPool;

/// A noisy PNG a few hundred KiB long, so it splits into chunks.
fn png() -> Vec<u8> {
    let mut state = 0x9e37_79b9_u32;
    let img = image::RgbaImage::from_fn(300, 300, |_, _| {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let b = state.to_le_bytes();
        image::Rgba([b[0], b[1], b[2], 255])
    });
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn upload_request(token: &str, filename: &str, content_type: &str, body: Vec<u8>) -> Request<Body> {
    Request::post(format!("/api/v1/media?filename={filename}"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, body.len())
        .body(Body::from(body))
        .unwrap()
}

fn authed(method: Method, uri: &str, token: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
}

async fn upload_png(app: &TestApp, token: &str) -> (String, Vec<u8>) {
    let bytes = png();
    let res = app
        .send(upload_request(
            token,
            "photo.png",
            "image/png",
            bytes.clone(),
        ))
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    let id = res.body["id"].as_str().unwrap().to_string();
    (id, bytes)
}

fn header_str<'a>(res: &'a TestResponse, name: &str) -> &'a str {
    res.headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn upload_list_get_delete_round_trip(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let bytes = png();
    let res = app
        .send(upload_request(
            &token,
            "photo.png",
            "image/png",
            bytes.clone(),
        ))
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    let id = res.body["id"].as_str().unwrap().to_string();
    assert_eq!(header_str(&res, "location"), format!("/api/v1/media/{id}"));
    assert_eq!(res.body["status"], "ready");
    assert_eq!(res.body["kind"], "image");
    assert_eq!(res.body["mime"], "image/png");
    assert_eq!(res.body["width"], 300);
    assert_eq!(res.body["has_thumbnail"], true);
    assert_eq!(res.body["original_filename"], "photo.png");
    assert!(res.body.get("storage_key").is_none());

    let list = app
        .call(Method::GET, "/api/v1/media", Some(&token), None)
        .await;
    assert_eq!(list.status, StatusCode::OK);
    assert_eq!(list.body["total"], 1);
    assert_eq!(list.body["data"][0]["id"], id.as_str());

    let got = app
        .call(
            Method::GET,
            &format!("/api/v1/media/{id}"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(got.status, StatusCode::OK);

    let usage = app
        .call(Method::GET, "/api/v1/media/usage", Some(&token), None)
        .await;
    assert_eq!(usage.body["used_bytes"], bytes.len());

    let thumb = app
        .send(
            authed(
                Method::GET,
                &format!("/api/v1/media/{id}/thumbnail"),
                &token,
            )
            .body(Body::empty())
            .unwrap(),
        )
        .await;
    assert_eq!(thumb.status, StatusCode::OK);
    assert_eq!(header_str(&thumb, "content-type"), "image/jpeg");

    let deleted = app
        .call(
            Method::DELETE,
            &format!("/api/v1/media/{id}"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT);
    let gone = app
        .call(
            Method::GET,
            &format!("/api/v1/media/{id}"),
            Some(&token),
            None,
        )
        .await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn content_supports_ranges(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let (id, bytes) = upload_png(&app, &token).await;
    let uri = format!("/api/v1/media/{id}/content");

    let partial = app
        .send(
            authed(Method::GET, &uri, &token)
                .header(header::RANGE, "bytes=0-9")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(partial.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(partial.bytes.len(), 10);
    assert_eq!(&partial.bytes[..], &bytes[..10]);
    assert_eq!(
        header_str(&partial, "content-range"),
        format!("bytes 0-9/{}", bytes.len())
    );

    let past = app
        .send(
            authed(Method::GET, &uri, &token)
                .header(header::RANGE, "bytes=99999999-")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(past.status, StatusCode::RANGE_NOT_SATISFIABLE);

    let whole = app
        .send(
            authed(Method::GET, &uri, &token)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(whole.status, StatusCode::OK);
    assert_eq!(&whole.bytes[..], &bytes[..]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn content_headers(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    // The upload claims text/html; the stored type is what the bytes are.
    let res = app
        .send(upload_request(&token, "photo.png", "text/html", png()))
        .await;
    assert_eq!(res.status, StatusCode::CREATED, "{:?}", res.body);
    let id = res.body["id"].as_str().unwrap().to_string();
    let content = app
        .send(
            authed(Method::GET, &format!("/api/v1/media/{id}/content"), &token)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(content.status, StatusCode::OK);
    assert_eq!(header_str(&content, "content-type"), "image/png");
    assert_eq!(header_str(&content, "x-content-type-options"), "nosniff");
    assert_eq!(header_str(&content, "content-security-policy"), "sandbox");
    assert_eq!(header_str(&content, "cache-control"), "no-store");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn missing_content_length_refused(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let res = app
        .send(
            authed(Method::POST, "/api/v1/media?filename=a.png", &token)
                .body(Body::from(png()))
                .unwrap(),
        )
        .await;
    assert_eq!(res.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(res.body["code"], "validation_failed");
    assert_eq!(res.body["field"], "content_length");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn session_flow_with_head(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let bytes = png();
    let total = bytes.len();
    let start = app
        .call(
            Method::POST,
            "/api/v1/media/uploads",
            Some(&token),
            Some(json!({"filename": "big.png", "size": total})),
        )
        .await;
    assert_eq!(start.status, StatusCode::CREATED, "{:?}", start.body);
    let id = start.body["id"].as_str().unwrap().to_string();
    assert_eq!(start.body["offset"], 0);
    assert_eq!(start.body["size"], total);
    assert_eq!(start.body["chunk_max"], 1024 * 1024);
    let uri = format!("/api/v1/media/uploads/{id}");

    let half = total / 2;
    for (from, to) in [(0, half), (half, total)] {
        let res = app
            .send(
                authed(Method::PUT, &uri, &token)
                    .header(
                        header::CONTENT_RANGE,
                        format!("bytes {from}-{}/{total}", to - 1),
                    )
                    .body(Body::from(bytes[from..to].to_vec()))
                    .unwrap(),
            )
            .await;
        assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
        assert_eq!(res.body["offset"], to);
        if from == 0 {
            let head = app
                .send(
                    authed(Method::HEAD, &uri, &token)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await;
            assert_eq!(head.status, StatusCode::OK);
            assert_eq!(header_str(&head, "upload-offset"), half.to_string());
            assert_eq!(header_str(&head, "upload-length"), total.to_string());
        }
    }

    let done = app
        .call(Method::POST, &format!("{uri}/complete"), Some(&token), None)
        .await;
    assert_eq!(done.status, StatusCode::OK, "{:?}", done.body);
    assert_eq!(done.body["status"], "ready");
    assert_eq!(done.body["id"], id.as_str());
    assert_eq!(done.body["mime"], "image/png");

    // The session is closed.
    let head = app
        .send(
            authed(Method::HEAD, &uri, &token)
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(head.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn offset_mismatch_is_409_with_offset(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let start = app
        .call(
            Method::POST,
            "/api/v1/media/uploads",
            Some(&token),
            Some(json!({"filename": "a.png", "size": 100})),
        )
        .await;
    let id = start.body["id"].as_str().unwrap().to_string();
    let res = app
        .send(
            authed(Method::PUT, &format!("/api/v1/media/uploads/{id}"), &token)
                .header(header::CONTENT_RANGE, "bytes 10-19/100")
                .body(Body::from(vec![0u8; 10]))
                .unwrap(),
        )
        .await;
    assert_eq!(res.status, StatusCode::CONFLICT);
    assert_eq!(res.body["code"], "upload_offset_mismatch");
    assert_eq!(res.body["offset"], 0);

    let bad = app
        .send(
            authed(Method::PUT, &format!("/api/v1/media/uploads/{id}"), &token)
                .header(header::CONTENT_RANGE, "bytes nonsense")
                .body(Body::from(vec![0u8; 10]))
                .unwrap(),
        )
        .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(bad.body["field"], "content_range");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn quota_is_413_with_extensions(pool: PgPool) {
    let app = TestApp::start(pool.clone()).await;
    let token = app.token(ADMIN_SUB);
    let me = app
        .call(Method::GET, "/api/v1/me", Some(&token), None)
        .await;
    let user: uuid::Uuid = me.body["id"].as_str().unwrap().parse().unwrap();
    sqlx::query(
        "INSERT INTO user_limits (user_id, email_daily_cap, contacts_cap, storage_quota_bytes)
         VALUES ($1, 1, 1, 1000)
         ON CONFLICT (user_id) DO UPDATE SET storage_quota_bytes = 1000",
    )
    .bind(user)
    .execute(&pool)
    .await
    .unwrap();
    let res = app
        .call(
            Method::POST,
            "/api/v1/media/uploads",
            Some(&token),
            Some(json!({"filename": "a.png", "size": 1001})),
        )
        .await;
    assert_eq!(res.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(res.body["code"], "storage_quota_exceeded");
    assert_eq!(res.body["quota_bytes"], 1000);
    assert_eq!(res.body["used_bytes"], 0);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn svg_is_415(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let by_name = app
        .send(upload_request(
            &token,
            "x.svg",
            "image/svg+xml",
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec(),
        ))
        .await;
    assert_eq!(by_name.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(by_name.body["code"], "unsupported_media_type");
    let by_bytes = app
        .send(upload_request(
            &token,
            "x.png",
            "image/png",
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".to_vec(),
        ))
        .await;
    assert_eq!(by_bytes.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn cors_exposes_upload_headers(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let start = app
        .call(
            Method::POST,
            "/api/v1/media/uploads",
            Some(&token),
            Some(json!({"filename": "a.png", "size": 100})),
        )
        .await;
    let id = start.body["id"].as_str().unwrap().to_string();
    let head = app
        .send(
            authed(Method::HEAD, &format!("/api/v1/media/uploads/{id}"), &token)
                .header(header::ORIGIN, "https://app.postit.test")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    let exposed = header_str(&head, "access-control-expose-headers").to_ascii_lowercase();
    assert!(exposed.contains("upload-offset"), "{exposed}");
    assert!(exposed.contains("upload-length"), "{exposed}");

    let preflight = app
        .send(
            Request::options(format!("/api/v1/media/uploads/{id}"))
                .header(header::ORIGIN, "https://app.postit.test")
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "PUT")
                .header(
                    header::ACCESS_CONTROL_REQUEST_HEADERS,
                    "authorization,content-range",
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    let allowed = header_str(&preflight, "access-control-allow-headers").to_ascii_lowercase();
    assert!(allowed.contains("content-range"), "{allowed}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn idempotent_session_start_replays(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let send = || async {
        app.send(
            authed(Method::POST, "/api/v1/media/uploads", &token)
                .header(header::CONTENT_TYPE, "application/json")
                .header("idempotency-key", "start-1")
                .body(Body::from(
                    json!({"filename": "a.png", "size": 100}).to_string(),
                ))
                .unwrap(),
        )
        .await
    };
    let first = send().await;
    let second = send().await;
    assert_eq!(first.status, StatusCode::CREATED);
    assert_eq!(second.status, StatusCode::CREATED);
    assert_eq!(first.body["id"], second.body["id"]);
    assert_eq!(
        header_str(&second, "location"),
        format!(
            "/api/v1/media/uploads/{}",
            first.body["id"].as_str().unwrap()
        )
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn chunk_over_chunk_max_refused(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let size = 3 * 1024 * 1024;
    let start = app
        .call(
            Method::POST,
            "/api/v1/media/uploads",
            Some(&token),
            Some(json!({"filename": "a.png", "size": size})),
        )
        .await;
    let id = start.body["id"].as_str().unwrap().to_string();
    let chunk = 1024 * 1024 + 1;
    let res = app
        .send(
            authed(Method::PUT, &format!("/api/v1/media/uploads/{id}"), &token)
                .header(
                    header::CONTENT_RANGE,
                    format!("bytes 0-{}/{size}", chunk - 1),
                )
                .header(header::CONTENT_LENGTH, chunk)
                .body(Body::from(vec![0u8; chunk]))
                .unwrap(),
        )
        .await;
    assert_eq!(res.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(res.body["code"], "payload_too_large");
    assert_eq!(res.body["max_bytes"], 1024 * 1024);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn single_upload_over_the_request_limit_names_the_limit(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let res = app
        .send(upload_request(
            &token,
            "clip.mp4",
            "video/mp4",
            vec![0u8; 8 * 1024 * 1024 + 1],
        ))
        .await;
    assert_eq!(res.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(res.body["code"], "payload_too_large");
    assert_eq!(res.body["max_bytes"], 8 * 1024 * 1024);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn an_idle_chunk_body_is_408(pool: PgPool) {
    let app = TestApp::start_with(pool, |s| {
        s.media_idle_timeout = std::time::Duration::from_millis(200);
    })
    .await;
    let token = app.token(ADMIN_SUB);
    let start = app
        .call(
            Method::POST,
            "/api/v1/media/uploads",
            Some(&token),
            Some(json!({"filename": "a.png", "size": 10})),
        )
        .await;
    let id = start.body["id"].as_str().unwrap().to_string();
    // One byte, then nothing.
    let stalled = futures::stream::once(async {
        Ok::<_, std::io::Error>(axum::body::Bytes::from_static(b"x"))
    })
    .chain(futures::stream::pending());
    let res = app
        .send(
            authed(Method::PUT, &format!("/api/v1/media/uploads/{id}"), &token)
                .header(header::CONTENT_RANGE, "bytes 0-9/10")
                .header(header::CONTENT_LENGTH, 10)
                .body(Body::from_stream(stalled))
                .unwrap(),
        )
        .await;
    assert_eq!(res.status, StatusCode::REQUEST_TIMEOUT, "{:?}", res.body);
    assert_eq!(res.body["code"], "request_timeout");
}

fn path_and_query(url: &str) -> String {
    let parsed = url::Url::parse(url).unwrap();
    format!("{}?{}", parsed.path(), parsed.query().unwrap_or_default())
}

fn get_public(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

async fn view_urls(app: &TestApp, token: &str, id: &str) -> serde_json::Value {
    let res = app
        .call(
            Method::GET,
            &format!("/api/v1/media/{id}/view-url"),
            Some(token),
            None,
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{:?}", res.body);
    res.body
}

#[sqlx::test(migrations = "../data/migrations")]
async fn public_route_serves_signed_url(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let (id, bytes) = upload_png(&app, &token).await;
    let urls = view_urls(&app, &token, &id).await;
    let uri = path_and_query(urls["content_url"].as_str().unwrap());
    assert!(
        uri.starts_with(&format!("/public/media/{id}?exp=")),
        "{uri}"
    );

    let res = app.send(get_public(&uri)).await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(&res.bytes[..], &bytes[..]);
    assert_eq!(header_str(&res, "content-type"), "image/png");
    assert_eq!(header_str(&res, "content-security-policy"), "sandbox");
    assert_eq!(header_str(&res, "x-content-type-options"), "nosniff");
    let cache = header_str(&res, "cache-control");
    let max_age: u64 = cache
        .strip_prefix("private, max-age=")
        .unwrap_or_else(|| unreachable!("cache-control: {cache}"))
        .parse()
        .unwrap();
    assert!(max_age > 0 && max_age <= 600, "{cache}");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn public_route_ranges(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let (id, bytes) = upload_png(&app, &token).await;
    let urls = view_urls(&app, &token, &id).await;
    let uri = path_and_query(urls["content_url"].as_str().unwrap());
    let res = app
        .send(
            Request::get(&uri)
                .header(header::RANGE, "bytes=5-14")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(res.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(&res.bytes[..], &bytes[5..15]);
}

#[sqlx::test(migrations = "../data/migrations")]
async fn public_refuses_expired_and_tampered(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let (id, _) = upload_png(&app, &token).await;
    let urls = view_urls(&app, &token, &id).await;
    let good = path_and_query(urls["content_url"].as_str().unwrap());

    let tampered = good.replacen("exp=", "exp=9", 1);
    let tampered_res = app.send(get_public(&tampered)).await;
    assert_eq!(tampered_res.status, StatusCode::FORBIDDEN);
    assert_eq!(tampered_res.body["code"], "forbidden");

    // Signed with the right key but already expired.
    let signer = postit_media::UrlSigner::new(
        b"media-test-signing-key-0123456789abcdef",
        "https://media.test/".parse().unwrap(),
    );
    let asset = postit_core::MediaAssetId::from_uuid(id.parse().unwrap());
    let expired_url = signer.sign(
        &postit_media::SignedTarget::Content(asset),
        chrono::Utc::now() - chrono::Duration::minutes(1),
    );
    let expired_res = app
        .send(get_public(&path_and_query(expired_url.as_str())))
        .await;
    assert_eq!(expired_res.status, StatusCode::FORBIDDEN);

    let missing = app.send(get_public(&format!("/public/media/{id}"))).await;
    assert_eq!(missing.status, StatusCode::FORBIDDEN);

    // The same answer for all three: nothing says which part was wrong.
    let strip = |mut v: serde_json::Value| {
        v.as_object_mut().unwrap().remove("request_id");
        v
    };
    assert_eq!(
        strip(tampered_res.body.clone()),
        strip(expired_res.body.clone())
    );
    assert_eq!(strip(tampered_res.body), strip(missing.body));
}

#[sqlx::test(migrations = "../data/migrations")]
async fn public_thumbnail_needs_its_own_signature(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let (id, _) = upload_png(&app, &token).await;
    let urls = view_urls(&app, &token, &id).await;

    let content = path_and_query(urls["content_url"].as_str().unwrap());
    let query = content.split_once('?').unwrap().1;
    let crossed = app
        .send(get_public(&format!("/public/media/{id}/thumbnail?{query}")))
        .await;
    assert_eq!(crossed.status, StatusCode::FORBIDDEN);

    let thumb = path_and_query(urls["thumbnail_url"].as_str().unwrap());
    let res = app.send(get_public(&thumb)).await;
    assert_eq!(res.status, StatusCode::OK);
    assert_eq!(header_str(&res, "content-type"), "image/jpeg");
}

#[sqlx::test(migrations = "../data/migrations")]
async fn valid_public_urls_are_not_charged_to_the_ip_bucket(pool: PgPool) {
    let app = TestApp::start_with(pool, |s| {
        s.rate_limit.unauthenticated = postit_config::RateBucket {
            rate_per_minute: 1,
            burst: 2,
        };
    })
    .await;
    let token = app.token(ADMIN_SUB);
    let (id, _) = upload_png(&app, &token).await;
    let urls = view_urls(&app, &token, &id).await;
    let uri = path_and_query(urls["thumbnail_url"].as_str().unwrap());
    // A page of thumbnails: far more fetches than the unauthenticated burst.
    for _ in 0..10 {
        let res = app.send(get_public(&uri)).await;
        assert_eq!(res.status, StatusCode::OK);
    }
    // Forged URLs are charged, so guessing is bounded per address.
    let forged = format!("/public/media/{id}?exp=4102444800&sig=nope");
    let mut statuses = Vec::new();
    for _ in 0..6 {
        statuses.push(app.send(get_public(&forged)).await.status);
    }
    assert!(
        statuses.contains(&StatusCode::TOO_MANY_REQUESTS),
        "{statuses:?}"
    );
}

#[sqlx::test(migrations = "../data/migrations")]
async fn unmatched_requests_with_a_body_stay_404(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let res = app
        .send(
            authed(Method::POST, "/api/v1/media/nothing/here", &token)
                .body(Body::from(vec![1u8; 64]))
                .unwrap(),
        )
        .await;
    assert_eq!(res.status, StatusCode::NOT_FOUND);
}
