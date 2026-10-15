#![cfg(feature = "testkit")]

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::Utc;
use jsonwebtoken::Algorithm;
use jsonwebtoken::jwk::{Jwk, JwkSet};
use postit_identity::VerifyError;
use postit_identity::testkit::Keys;
use postit_identity::verifier::Verifier;
use serde::Serialize;

const ISSUER: &str = "https://issuer.test";
const AUDIENCE: &str = "postit";

#[derive(Serialize)]
struct Claims {
    sub: String,
    iss: String,
    aud: String,
    exp: i64,
    nbf: i64,
}

fn claims(exp_offset_secs: i64, nbf_offset_secs: i64) -> Claims {
    let now = Utc::now().timestamp();
    Claims {
        sub: "test-subject".to_string(),
        iss: ISSUER.to_string(),
        aud: AUDIENCE.to_string(),
        exp: now + exp_offset_secs,
        nbf: now + nbf_offset_secs,
    }
}

fn verifier() -> Verifier {
    Verifier::new(
        ISSUER,
        vec![AUDIENCE.to_string()],
        vec![Algorithm::RS256, Algorithm::ES256],
        Duration::from_secs(0),
    )
}

#[test]
fn accepts_a_valid_rs256_token() {
    let keys = Keys::generate();
    let token = keys.mint(&claims(3600, -10), Algorithm::RS256);

    let verified = verifier()
        .verify(&token, &keys.jwks())
        .unwrap_or_else(|e| unreachable!("verify: {e}"));

    assert_eq!(verified.sub, "test-subject");
}

#[test]
fn accepts_a_valid_es256_token() {
    let keys = Keys::generate();
    let token = keys.mint(&claims(3600, -10), Algorithm::ES256);

    let verified = verifier()
        .verify(&token, &keys.jwks())
        .unwrap_or_else(|e| unreachable!("verify: {e}"));

    assert_eq!(verified.sub, "test-subject");
}

#[test]
fn rejects_wrong_issuer() {
    let keys = Keys::generate();
    let mut wrong = claims(3600, -10);
    wrong.iss = "https://someone-else.test".to_string();
    let token = keys.mint(&wrong, Algorithm::RS256);

    assert!(matches!(
        verifier().verify(&token, &keys.jwks()),
        Err(VerifyError::WrongIssuer)
    ));
}

#[test]
fn rejects_wrong_audience() {
    let keys = Keys::generate();
    let mut wrong = claims(3600, -10);
    wrong.aud = "someone-else".to_string();
    let token = keys.mint(&wrong, Algorithm::RS256);

    assert!(matches!(
        verifier().verify(&token, &keys.jwks()),
        Err(VerifyError::WrongAudience)
    ));
}

#[test]
fn rejects_an_expired_token() {
    let keys = Keys::generate();
    let token = keys.mint(&claims(-3600, -7200), Algorithm::RS256);

    assert!(matches!(
        verifier().verify(&token, &keys.jwks()),
        Err(VerifyError::Expired)
    ));
}

#[test]
fn expiry_honours_the_leeway_boundary() {
    let keys = Keys::generate();
    let leeway = Verifier::new(
        ISSUER,
        vec![AUDIENCE.to_string()],
        vec![Algorithm::RS256],
        Duration::from_secs(60),
    );
    // Expired 30 s ago: inside the 60 s leeway. Expired 120 s ago: outside it. The margins
    // are wide so a second boundary tick cannot flip either outcome.
    let inside = keys.mint(&claims(-30, -3600), Algorithm::RS256);
    assert!(leeway.verify(&inside, &keys.jwks()).is_ok());
    let outside = keys.mint(&claims(-120, -3600), Algorithm::RS256);
    assert!(matches!(
        leeway.verify(&outside, &keys.jwks()),
        Err(VerifyError::Expired)
    ));
}

#[test]
fn rejects_a_not_yet_valid_token() {
    let keys = Keys::generate();
    let token = keys.mint(&claims(7200, 3600), Algorithm::RS256);

    assert!(matches!(
        verifier().verify(&token, &keys.jwks()),
        Err(VerifyError::NotYetValid)
    ));
}

#[test]
fn rejects_a_bad_signature() {
    let keys = Keys::generate();
    let token = keys.mint(&claims(3600, -10), Algorithm::RS256);

    // Same kid, valid signature over the original payload, but the payload is swapped for a
    // different subject: the kid is found and the signature is what fails.
    let mut parts = token.split('.');
    let (header, _payload, signature) = (
        parts.next().unwrap_or_default(),
        parts.next(),
        parts.next().unwrap_or_default(),
    );
    let mut forged = claims(3600, -10);
    forged.sub = "attacker".to_string();
    let forged_payload = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&forged).unwrap_or_else(|e| unreachable!("serialize claims: {e}")),
    );
    let tampered = format!("{header}.{forged_payload}.{signature}");

    assert!(matches!(
        verifier().verify(&tampered, &keys.jwks()),
        Err(VerifyError::BadSignature)
    ));
}

#[test]
fn rejects_an_hmac_token_signed_with_the_rsa_public_key_bytes() {
    // Review Focus: algorithm confusion. Even though HS256 encodes fine and even if a
    // caller misconfigures accepted_algorithms to include it, NEVER_ACCEPTED must still
    // reject it.
    let keys = Keys::generate();
    let jwks = keys.jwks();
    let rsa_jwk: &Jwk = jwks
        .keys
        .iter()
        .find(|k| matches!(k.algorithm, jsonwebtoken::jwk::AlgorithmParameters::RSA(_)))
        .unwrap_or_else(|| unreachable!("rsa jwk present"));
    let public_key_bytes =
        serde_json::to_vec(rsa_jwk).unwrap_or_else(|e| unreachable!("serialize jwk: {e}"));

    let mut header = jsonwebtoken::Header::new(Algorithm::HS256);
    header.kid = rsa_jwk.common.key_id.clone();
    let hmac_token = jsonwebtoken::encode(
        &header,
        &claims(3600, -10),
        &jsonwebtoken::EncodingKey::from_secret(&public_key_bytes),
    )
    .unwrap_or_else(|e| unreachable!("encode hmac token: {e}"));

    // A misconfigured verifier that (wrongly) lists HS256 as accepted must still reject.
    let permissive = Verifier::new(
        ISSUER,
        vec![AUDIENCE.to_string()],
        vec![Algorithm::RS256, Algorithm::ES256, Algorithm::HS256],
        Duration::from_secs(0),
    );

    assert!(matches!(
        permissive.verify(&hmac_token, &jwks),
        Err(VerifyError::UnacceptedAlgorithm)
    ));
}

#[test]
fn rejects_an_unknown_kid() {
    let keys = Keys::generate();
    let token = keys.mint(&claims(3600, -10), Algorithm::RS256);

    let empty_jwks = JwkSet { keys: vec![] };
    assert!(matches!(
        verifier().verify(&token, &empty_jwks),
        Err(VerifyError::UnknownKid)
    ));
}

#[test]
fn key_rotation_old_kid_still_valid_until_dropped_new_kid_valid_once_present() {
    let old_keys = Keys::generate();
    let new_keys = Keys::generate();
    let old_token = old_keys.mint(&claims(3600, -10), Algorithm::RS256);
    let new_token = new_keys.mint(&claims(3600, -10), Algorithm::RS256);

    let mut combined = old_keys.jwks();
    combined.keys.extend(new_keys.jwks().keys);

    assert!(verifier().verify(&old_token, &combined).is_ok());
    assert!(verifier().verify(&new_token, &combined).is_ok());

    let only_new = new_keys.jwks();
    assert!(matches!(
        verifier().verify(&old_token, &only_new),
        Err(VerifyError::UnknownKid)
    ));
    assert!(verifier().verify(&new_token, &only_new).is_ok());
}

#[test]
fn rejects_a_token_with_alg_none_in_the_header() {
    // Review Focus: algorithm confusion via `alg: none`. `jsonwebtoken` 9.3.1's
    // `Algorithm` enum has no `None` variant, so a header claiming `alg: none` cannot be
    // minted through the crate's own API — it has to be hand-built to prove the wire-level
    // behavior a future `jsonwebtoken` upgrade or refactor could silently change.
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
    let payload = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&claims(3600, -10))
            .unwrap_or_else(|e| unreachable!("serialize claims: {e}")),
    );
    // `alg: none` tokens are unsigned: an empty third segment.
    let token = format!("{header}.{payload}.");

    // `decode_header` is the first thing `Verifier::verify` calls, and where this must be
    // rejected: it can't even determine an `Algorithm` to look up in `NEVER_ACCEPTED` or
    // `accepted_algorithms` for a header jsonwebtoken can't deserialize.
    assert!(jsonwebtoken::decode_header(&token).is_err());

    let keys = Keys::generate();
    assert!(matches!(
        verifier().verify(&token, &keys.jwks()),
        Err(VerifyError::Malformed)
    ));
}

fn client_verifier() -> Verifier {
    verifier().require_client("client_id", vec!["postit-app".to_string()])
}

fn claims_with(extra: &serde_json::Value) -> serde_json::Value {
    let now = Utc::now().timestamp();
    let mut c = serde_json::json!({
        "sub": "test-subject",
        "iss": ISSUER,
        "aud": AUDIENCE,
        "exp": now + 3600,
        "nbf": now - 10,
    });
    if let (Some(map), Some(more)) = (c.as_object_mut(), extra.as_object()) {
        map.extend(more.clone());
    }
    c
}

#[test]
fn token_from_an_allowed_client_is_accepted() {
    let keys = Keys::generate();
    let token = keys.mint(
        &claims_with(&serde_json::json!({"client_id": "postit-app"})),
        Algorithm::RS256,
    );
    assert!(client_verifier().verify(&token, &keys.jwks()).is_ok());
}

#[test]
fn token_from_another_client_is_refused() {
    let keys = Keys::generate();
    let token = keys.mint(
        &claims_with(&serde_json::json!({"client_id": "other-app"})),
        Algorithm::RS256,
    );
    assert!(matches!(
        client_verifier().verify(&token, &keys.jwks()),
        Err(VerifyError::WrongClient)
    ));
}

#[test]
fn client_falls_back_to_azp() {
    let keys = Keys::generate();
    let token = keys.mint(
        &claims_with(&serde_json::json!({"azp": "postit-app"})),
        Algorithm::RS256,
    );
    assert!(client_verifier().verify(&token, &keys.jwks()).is_ok());
}

#[test]
fn token_without_client_claim_is_refused() {
    let keys = Keys::generate();
    let token = keys.mint(&claims_with(&serde_json::json!({})), Algorithm::RS256);
    assert!(matches!(
        client_verifier().verify(&token, &keys.jwks()),
        Err(VerifyError::WrongClient)
    ));
}

#[test]
fn verifier_without_client_rule_ignores_the_claim() {
    let keys = Keys::generate();
    let token = keys.mint(
        &claims_with(&serde_json::json!({"client_id": "anything"})),
        Algorithm::RS256,
    );
    assert!(verifier().verify(&token, &keys.jwks()).is_ok());
}

#[test]
fn token_without_aud_is_refused() {
    let keys = Keys::generate();
    let mut c = claims_with(&serde_json::json!({}));
    if let Some(map) = c.as_object_mut() {
        map.remove("aud");
    }
    let token = keys.mint(&c, Algorithm::RS256);
    assert!(verifier().verify(&token, &keys.jwks()).is_err());
}
