#![cfg(feature = "testkit")]

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use postit_identity::testkit::Keys;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: usize,
}

fn claims() -> Claims {
    Claims {
        sub: "test-subject".to_string(),
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
    }
}

#[test]
fn mints_a_verifiable_rs256_token() {
    let keys = Keys::generate();
    let token = keys.mint(&claims(), Algorithm::RS256);
    let header = decode_header(&token).unwrap_or_else(|e| unreachable!("decode_header: {e}"));
    assert_eq!(header.alg, Algorithm::RS256);

    let jwks = keys.jwks();
    let jwk = jwks
        .keys
        .iter()
        .find(|k| k.common.key_id.as_deref() == header.kid.as_deref())
        .unwrap_or_else(|| unreachable!("matching jwk not found"));
    let decoding_key = DecodingKey::from_jwk(jwk).unwrap_or_else(|e| unreachable!("from_jwk: {e}"));

    let mut validation = Validation::new(Algorithm::RS256);
    validation.validate_aud = false;
    let decoded = decode::<Claims>(&token, &decoding_key, &validation)
        .unwrap_or_else(|e| unreachable!("decode: {e}"));
    assert_eq!(decoded.claims.sub, "test-subject");
}

#[test]
fn mints_a_verifiable_es256_token() {
    let keys = Keys::generate();
    let token = keys.mint(&claims(), Algorithm::ES256);
    let header = decode_header(&token).unwrap_or_else(|e| unreachable!("decode_header: {e}"));
    assert_eq!(header.alg, Algorithm::ES256);

    let jwks = keys.jwks();
    let jwk = jwks
        .keys
        .iter()
        .find(|k| k.common.key_id.as_deref() == header.kid.as_deref())
        .unwrap_or_else(|| unreachable!("matching jwk not found"));
    let decoding_key = DecodingKey::from_jwk(jwk).unwrap_or_else(|e| unreachable!("from_jwk: {e}"));

    let mut validation = Validation::new(Algorithm::ES256);
    validation.validate_aud = false;
    let decoded = decode::<Claims>(&token, &decoding_key, &validation)
        .unwrap_or_else(|e| unreachable!("decode: {e}"));
    assert_eq!(decoded.claims.sub, "test-subject");
}
