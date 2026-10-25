//! Malformed tokens through the JWT parsing path: no panic, always an error, and a mutated
//! genuine token is either refused or still carries the genuine claims.
#![cfg(feature = "testkit")]

use std::sync::LazyLock;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::Utc;
use jsonwebtoken::Algorithm;
use postit_identity::testkit::Keys;
use postit_identity::verifier::Verifier;
use proptest::prelude::*;

const ISSUER: &str = "https://issuer.test";

fn verifier() -> Verifier {
    Verifier::new(
        ISSUER,
        vec!["postit".to_string()],
        vec![Algorithm::RS256, Algorithm::ES256],
        Duration::from_secs(0),
    )
    .require_client("client_id", vec!["postit-app".to_string()])
}

/// One RSA key pair for the whole binary: generating one per case (4 properties x 256
/// cases) takes minutes, and the properties do not depend on which key signs.
static KEYS: LazyLock<Keys> = LazyLock::new(Keys::generate);

fn genuine(keys: &Keys) -> String {
    let now = Utc::now().timestamp();
    keys.mint(
        &serde_json::json!({
            "sub": "s", "iss": ISSUER, "aud": "postit", "exp": now + 3600,
            "client_id": "postit-app",
        }),
        Algorithm::RS256,
    )
}

fn segment() -> impl Strategy<Value = String> {
    prop::collection::vec(any::<u8>(), 0..512).prop_map(|b| URL_SAFE_NO_PAD.encode(b))
}

fn header_json() -> impl Strategy<Value = String> {
    (
        prop::sample::select(vec![
            "none", "HS256", "RS256", "ES256", "PS256", "", "rs256",
        ]),
        prop::option::of("[ -~]{0,64}"),
        prop::option::of("[ -~]{0,16}"),
    )
        .prop_map(|(alg, kid, typ)| {
            let mut h = serde_json::json!({ "alg": alg });
            if let Some(kid) = kid {
                h["kid"] = kid.into();
            }
            if let Some(typ) = typ {
                h["typ"] = typ.into();
            }
            URL_SAFE_NO_PAD.encode(h.to_string())
        })
}

proptest! {
    #[test]
    fn arbitrary_text_is_refused(token in "[ -~]{0,2048}") {
        prop_assert!(verifier().verify(&token, &KEYS.jwks()).is_err());
    }

    #[test]
    fn three_random_segments_are_refused(h in segment(), p in segment(), s in segment()) {
        let token = format!("{h}.{p}.{s}");
        prop_assert!(verifier().verify(&token, &KEYS.jwks()).is_err());
    }

    #[test]
    fn forged_headers_with_random_signatures_are_refused(h in header_json(), p in segment(), s in segment()) {
        let token = format!("{h}.{p}.{s}");
        prop_assert!(verifier().verify(&token, &KEYS.jwks()).is_err());
    }

    #[test]
    fn a_mutated_genuine_token_is_refused_or_unchanged(index in any::<prop::sample::Index>(), byte in 0x21u8..0x7f) {
        let keys = &*KEYS;
        let token = genuine(keys);
        let mut bytes = token.into_bytes();
        let i = index.index(bytes.len());
        bytes[i] = byte;
        let mutated = String::from_utf8_lossy(&bytes).into_owned();
        if let Ok(claims) = verifier().verify(&mutated, &keys.jwks()) {
            // Changing only base64 padding bits can leave the decoded token identical.
            prop_assert_eq!(claims.sub, "s");
            prop_assert_eq!(claims.iss, ISSUER);
        }
    }
}
