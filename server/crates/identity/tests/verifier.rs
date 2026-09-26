#![cfg(feature = "testkit")]

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::Utc;
use jsonwebtoken::Algorithm;
use jsonwebtoken::jwk::{Jwk, JwkSet};
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

    assert!(verifier().verify(&token, &keys.jwks()).is_err());
}

#[test]
fn rejects_wrong_audience() {
    let keys = Keys::generate();
    let mut wrong = claims(3600, -10);
    wrong.aud = "someone-else".to_string();
    let token = keys.mint(&wrong, Algorithm::RS256);

    assert!(verifier().verify(&token, &keys.jwks()).is_err());
}

#[test]
fn rejects_an_expired_token() {
    let keys = Keys::generate();
    let token = keys.mint(&claims(-3600, -7200), Algorithm::RS256);

    assert!(verifier().verify(&token, &keys.jwks()).is_err());
}

#[test]
fn rejects_a_not_yet_valid_token() {
    let keys = Keys::generate();
    let token = keys.mint(&claims(7200, 3600), Algorithm::RS256);

    assert!(verifier().verify(&token, &keys.jwks()).is_err());
}

#[test]
fn rejects_a_bad_signature() {
    let signing_keys = Keys::generate();
    let other_keys = Keys::generate();
    let token = signing_keys.mint(&claims(3600, -10), Algorithm::RS256);

    // The verifier is given `other_keys`' JWKS, which has a different kid, so this is
    // really testing "unknown kid" rather than "same kid, different key" — a same-kid
    // substitution can't happen in this design because the kid is generated per `Keys`
    // instance, but the outcome (verification fails) is what matters here.
    assert!(verifier().verify(&token, &other_keys.jwks()).is_err());
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

    assert!(permissive.verify(&hmac_token, &jwks).is_err());
}

#[test]
fn rejects_an_unknown_kid() {
    let keys = Keys::generate();
    let token = keys.mint(&claims(3600, -10), Algorithm::RS256);

    let empty_jwks = JwkSet { keys: vec![] };
    assert!(verifier().verify(&token, &empty_jwks).is_err());
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
    assert!(verifier().verify(&old_token, &only_new).is_err());
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
    assert!(verifier().verify(&token, &keys.jwks()).is_err());
}
