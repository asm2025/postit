//! The OAuth routes' redirects, problems, responses, logs, and audit details carry no token,
//! client secret, PKCE verifier, or `state`, and the `connect_id` only in the success redirect.
//! Its own test binary: tracing's callsite-interest cache is process-wide, so a log-capturing
//! test cannot share a binary with others.
#![allow(clippy::unwrap_used)]

mod oauth_support;

use axum::http::{Method, StatusCode};
use oauth_support::{
    CLIENT_SECRET, DONE, OAUTH, active_user, authorize_url, call_as, callback, event, fixture,
    location, query, sign, start_as,
};
use postit_agent_kit::testkit::OAuthFailure;
use postit_http::testkit::leak::{LogCapture, assert_no_leak};
use serde_json::{Value, json};
use sqlx::PgPool;

#[sqlx::test(migrations = "../data/migrations")]
async fn oauth_redirects_and_problems_leak_nothing(pool: PgPool) {
    let (logs, _guard) = LogCapture::install();
    let f = fixture(pool).await;
    let mut states = Vec::new();
    let mut codes = Vec::new();

    // Success.
    let url = authorize_url(&start_as(&f.app, &f.owner, None, DONE).await);
    let state = query(&url, "state").unwrap();
    let code = f.provider.approve(&url);
    let ok = callback(&f.app, &[("code", &code), ("state", &state)]).await;
    let success = location(&ok);
    let connect_id = query(&success, "connect_id").unwrap();
    states.push(state.clone());
    codes.push(code.clone());

    // A replay of the same callback.
    let replay = location(&callback(&f.app, &[("code", &code), ("state", &state)]).await);

    // A denial.
    let url = authorize_url(&start_as(&f.app, &f.owner, None, DONE).await);
    let state = query(&url, "state").unwrap();
    let denied = location(
        &callback(
            &f.app,
            &[
                ("error", "access_denied"),
                ("error_description", "no"),
                ("state", &state),
            ],
        )
        .await,
    );
    states.push(state);

    // A failed exchange.
    let url = authorize_url(&start_as(&f.app, &f.owner, None, DONE).await);
    let state = query(&url, "state").unwrap();
    let code = f.provider.approve(&url);
    f.provider.fail_next(OAuthFailure::InvalidGrant);
    let failed = location(&callback(&f.app, &[("code", &code), ("state", &state)]).await);
    states.push(state);
    codes.push(code);

    // Reading and confirming, then a stranger's read and a bad return_to.
    let path = format!("/api/v1/accounts/connect/{connect_id}");
    let read = call_as(&f.app, Method::GET, &path, &f.owner, None, None).await;
    assert_eq!(read.status, StatusCode::OK, "{:?}", read.body);
    let confirmed = call_as(
        &f.app,
        Method::POST,
        &format!("{path}/confirm"),
        &f.owner,
        None,
        Some(json!({})),
    )
    .await;
    assert_eq!(confirmed.status, StatusCode::OK, "{:?}", confirmed.body);
    let (stranger, _) = active_user(&f.app, "stranger").await;
    let foreign = call_as(&f.app, Method::GET, &path, &stranger, None, None).await;
    assert_eq!(foreign.status, StatusCode::NOT_FOUND);
    let bad_return = start_as(&f.app, &f.owner, None, "https://evil.test/oauth-done").await;
    assert_eq!(bad_return.status, StatusCode::UNPROCESSABLE_ENTITY);

    // Events: a bad signature, an unreadable signed body, and a good deauthorization.
    let body = serde_json::to_vec(&json!({ "user_id": "mock-user" })).unwrap();
    let rejected = event(&f.app, OAUTH, "deauthorize", body.clone(), Some("00")).await;
    assert_eq!(rejected.status, StatusCode::UNAUTHORIZED);
    let unreadable = b"not json".to_vec();
    let signature = sign(&unreadable, CLIENT_SECRET);
    let malformed = event(&f.app, OAUTH, "deauthorize", unreadable, Some(&signature)).await;
    assert_eq!(malformed.status, StatusCode::UNPROCESSABLE_ENTITY);
    let signature = sign(&body, CLIENT_SECRET);
    let applied = event(&f.app, OAUTH, "deauthorize", body, Some(&signature)).await;
    assert_eq!(applied.status, StatusCode::OK);

    // Every secret the provider exchanged: its tokens and the PKCE verifiers it was sent.
    let mut verifiers = Vec::new();
    for request in f.provider.requests() {
        for (key, value) in url::form_urlencoded::parse(&request.body) {
            if key == "code_verifier" {
                verifiers.push(value.into_owned());
            }
        }
    }
    assert_eq!(verifiers.len(), 2, "two exchanges");
    let tokens = ["access-1", "refresh-1"];

    let audit: Vec<Value> = sqlx::query_scalar("SELECT details FROM audit_events")
        .fetch_all(&f.pool)
        .await
        .unwrap();
    let logs = logs.text();
    // The capture works: the failed callbacks were logged (by their code only).
    assert!(logs.contains("state_invalid"), "{logs}");
    assert!(logs.contains("exchange_failed"), "{logs}");
    let text = |res: &postit_api::testkit::TestResponse| {
        format!("{:?} {}", res.headers, String::from_utf8_lossy(&res.bytes))
    };
    let outputs = [
        ("logs", logs.clone()),
        ("success redirect", success.to_string()),
        ("success response", text(&ok)),
        ("replay redirect", replay.to_string()),
        ("denied redirect", denied.to_string()),
        ("failed redirect", failed.to_string()),
        ("pending read", text(&read)),
        ("confirm", text(&confirmed)),
        ("foreign read", text(&foreign)),
        ("bad return_to", text(&bad_return)),
        ("rejected event", text(&rejected)),
        ("malformed event", text(&malformed)),
        ("applied event", text(&applied)),
        ("audit", format!("{audit:?}")),
    ];
    let outputs: Vec<(&str, &str)> = outputs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let mut needles: Vec<&str> = vec![CLIENT_SECRET];
    needles.extend(tokens);
    needles.extend(verifiers.iter().map(String::as_str));
    needles.extend(states.iter().map(String::as_str));
    needles.extend(codes.iter().map(String::as_str));
    assert_no_leak(&needles, &outputs);

    // The connect ID appears in the success redirect and nowhere else.
    assert!(success.as_str().contains(&connect_id));
    let elsewhere: Vec<(&str, &str)> = outputs
        .iter()
        .copied()
        .filter(|(name, _)| !name.starts_with("success"))
        .collect();
    assert_no_leak(&[connect_id.as_str()], &elsewhere);
}
