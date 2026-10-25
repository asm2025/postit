//! Its own test binary: tracing's callsite-interest cache is process-wide, so a log-capturing
//! test cannot share a binary with others.

#![allow(clippy::unwrap_used)]

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use postit_api::testkit::{ADMIN_SUB, TestApp};
use postit_http::testkit::leak::{LogCapture, assert_no_leak};
use sqlx::PgPool;

fn png() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(32, 32, image::Rgba([1, 2, 3, 255]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

#[sqlx::test(migrations = "../data/migrations")]
async fn public_route_logs_no_signature(pool: PgPool) {
    let (logs, _guard) = LogCapture::install();
    let app = TestApp::start(pool).await;
    let token = app.token(ADMIN_SUB);
    let bytes = png();
    let uploaded = app
        .send(
            Request::post("/api/v1/media?filename=a.png")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_LENGTH, bytes.len())
                .body(Body::from(bytes))
                .unwrap(),
        )
        .await;
    assert_eq!(uploaded.status, StatusCode::CREATED, "{:?}", uploaded.body);
    let id = uploaded.body["id"].as_str().unwrap().to_string();
    let urls = app
        .call(
            Method::GET,
            &format!("/api/v1/media/{id}/view-url"),
            Some(&token),
            None,
        )
        .await;
    let url = url::Url::parse(urls.body["content_url"].as_str().unwrap()).unwrap();
    let good = format!("{}?{}", url.path(), url.query().unwrap());
    let good_sig = url
        .query_pairs()
        .find(|(k, _)| k == "sig")
        .map(|(_, v)| v.into_owned())
        .unwrap();

    let ok = app
        .send(Request::get(&good).body(Body::empty()).unwrap())
        .await;
    assert_eq!(ok.status, StatusCode::OK);
    let forged = format!("/public/media/{id}?exp=4102444800&sig=FORGEDSIGNATURE0123456789");
    let res = app
        .send(Request::get(&forged).body(Body::empty()).unwrap())
        .await;
    assert_eq!(res.status, StatusCode::FORBIDDEN);

    let logs = logs.text();
    assert!(
        logs.contains("route="),
        "trace spans were not captured: {logs}"
    );
    assert_no_leak(
        &[good_sig.as_str(), "FORGEDSIGNATURE0123456789"],
        &[("logs", &logs), ("problem body", &res.body.to_string())],
    );
}
