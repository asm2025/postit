#![cfg(feature = "testkit")]

use std::time::Duration;

use jsonwebtoken::Algorithm;
use postit_identity::discovery::{HttpJwksSource, OidcDiscovery};
use postit_identity::testkit::TestIssuer;
use postit_identity::verifier::Verifier;
use serde::Serialize;

#[derive(Serialize)]
struct Claims {
    sub: String,
    iss: String,
    aud: String,
    exp: i64,
}

#[tokio::test]
async fn serves_discovery_and_jwks_that_the_real_client_can_verify_against() {
    let issuer = TestIssuer::start().await;
    let token = issuer.mint(
        &Claims {
            sub: "test-subject".to_string(),
            iss: issuer.issuer_url().to_string(),
            aud: "postit".to_string(),
            exp: chrono::Utc::now().timestamp() + 3600,
        },
        Algorithm::RS256,
    );

    let client = postit_http::build_client(&postit_config::HttpSettings {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        user_agent: "postit-identity-test/0".to_string(),
        extra_ca_files: Vec::new(),
    })
    .unwrap_or_else(|err| unreachable!("building test http client: {err}"));
    let source = HttpJwksSource::new(client, issuer.issuer_url());
    let discovery = OidcDiscovery::new(source, Duration::from_secs(3600));
    let jwks = discovery
        .jwks()
        .await
        .unwrap_or_else(|err| unreachable!("jwks: {err}"));

    let verifier = Verifier::new(
        issuer.issuer_url().to_string(),
        vec!["postit".to_string()],
        vec![Algorithm::RS256, Algorithm::ES256],
        Duration::from_secs(0),
    );
    let verified_claims = verifier
        .verify(&token, &jwks)
        .unwrap_or_else(|err| unreachable!("verify: {err}"));

    assert_eq!(verified_claims.sub, "test-subject");
}

#[tokio::test]
async fn mounted_userinfo_only_answers_the_matching_bearer_token() {
    let issuer = TestIssuer::start().await;
    issuer
        .mount_userinfo(
            "right-token",
            serde_json::json!({"sub": "test-subject", "email": "a@example.test"}),
        )
        .await;

    let client = postit_http::build_client(&postit_config::HttpSettings {
        connect_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(5),
        user_agent: "postit-identity-test/0".to_string(),
        extra_ca_files: Vec::new(),
    })
    .unwrap_or_else(|err| unreachable!("building test http client: {err}"));

    let ok = client
        .get(format!("{}userinfo", issuer.issuer_url()))
        .bearer_auth("right-token")
        .send()
        .await
        .unwrap_or_else(|err| unreachable!("request: {err}"));
    assert_eq!(ok.status(), 200);

    let wrong = client
        .get(format!("{}userinfo", issuer.issuer_url()))
        .bearer_auth("wrong-token")
        .send()
        .await
        .unwrap_or_else(|err| unreachable!("request: {err}"));
    assert_eq!(wrong.status(), 404);
}
