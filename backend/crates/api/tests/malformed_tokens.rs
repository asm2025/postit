//! Every malformed bearer gets the same 401: same body (ignoring `request_id`), same
//! challenge, no reason leaked.

use axum::http::{Method, StatusCode, header};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use postit_api::testkit::{TestApp, TestResponse};
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::{Config, TestRunner};
use sqlx::PgPool;

fn cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(256)
}

fn shape(res: &TestResponse) -> (StatusCode, Option<String>, serde_json::Value) {
    let mut body = res.body.clone();
    if let Some(map) = body.as_object_mut() {
        map.remove("request_id");
    }
    let challenge = res
        .headers
        .get(header::WWW_AUTHENTICATE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    (res.status, challenge, body)
}

fn tokens() -> impl Strategy<Value = String> {
    let seg = || prop::collection::vec(any::<u8>(), 1..256).prop_map(|b| URL_SAFE_NO_PAD.encode(b));
    prop_oneof![
        "[!-~]{1,512}",
        (seg(), seg(), seg()).prop_map(|(h, p, s)| format!("{h}.{p}.{s}")),
        (
            prop::sample::select(vec!["none", "HS256", "RS256"]),
            seg(),
            seg()
        )
            .prop_map(|(alg, p, s)| {
                let h = URL_SAFE_NO_PAD.encode(format!(r#"{{"alg":"{alg}","kid":"k"}}"#));
                format!("{h}.{p}.{s}")
            }),
        Just("a".repeat(9 * 1024)),
    ]
}

#[sqlx::test(migrations = "../data/migrations")]
async fn every_malformed_bearer_gets_the_same_401(pool: PgPool) {
    let app = TestApp::start(pool).await;
    let reference = shape(
        &app.call(Method::GET, "/api/v1/me", Some("garbage"), None)
            .await,
    );
    assert_eq!(reference.0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        reference.1.as_deref(),
        Some("Bearer error=\"invalid_token\"")
    );

    let mut runner = TestRunner::new(Config::with_cases(cases()));
    let strategy = tokens();
    for _ in 0..cases() {
        let token = strategy
            .new_tree(&mut runner)
            .unwrap_or_else(|e| unreachable!("strategy: {e}"))
            .current();
        let got = shape(
            &app.call(Method::GET, "/api/v1/me", Some(&token), None)
                .await,
        );
        assert_eq!(got, reference, "token {:?}…", token.get(..40));
    }
}
