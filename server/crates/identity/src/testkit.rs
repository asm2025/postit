use std::sync::atomic::{AtomicU64, Ordering};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jsonwebtoken::jwk::{
    AlgorithmParameters, CommonParameters, EllipticCurve, EllipticCurveKeyParameters,
    EllipticCurveKeyType, Jwk, JwkSet, KeyAlgorithm, PublicKeyUse, RSAKeyParameters, RSAKeyType,
};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use p256::ecdsa::SigningKey as EcSigningKey;
use p256::elliptic_curve::pkcs8::EncodePrivateKey;
use p256::pkcs8::LineEnding as EcLineEnding;
use rand_core::OsRng;
use rsa::RsaPrivateKey;
use rsa::pkcs1::EncodeRsaPrivateKey;
use rsa::pkcs8::LineEnding as RsaLineEnding;
use rsa::traits::PublicKeyParts;
use serde::Serialize;

/// Monotonic counter so each `Keys::generate()` call mints kids unique to that instance —
/// callers (e.g. key-rotation tests) build a combined `JwkSet` from multiple `Keys`
/// instances and rely on `kid` values not colliding across them.
static KID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generates one RSA and one P-256 keypair and mints RS256/ES256 test tokens from them.
/// Test-only (`testkit` feature): no production code path constructs one.
pub struct Keys {
    rsa_encoding_key: EncodingKey,
    rsa_jwk: Jwk,
    ec_encoding_key: EncodingKey,
    ec_jwk: Jwk,
}

impl Keys {
    #[must_use]
    pub fn generate() -> Self {
        let instance = KID_COUNTER.fetch_add(1, Ordering::Relaxed);
        let rsa_kid = format!("test-rsa-{instance}");
        let ec_kid = format!("test-ec-{instance}");

        let rsa_private = RsaPrivateKey::new(&mut OsRng, 2048)
            .unwrap_or_else(|err| unreachable!("generating rsa test key: {err}"));
        let rsa_pem = rsa_private
            .to_pkcs1_pem(RsaLineEnding::LF)
            .unwrap_or_else(|err| unreachable!("encoding rsa test key: {err}"));
        let rsa_encoding_key = EncodingKey::from_rsa_pem(rsa_pem.as_bytes())
            .unwrap_or_else(|err| unreachable!("loading rsa encoding key: {err}"));
        let rsa_jwk = Jwk {
            common: CommonParameters {
                public_key_use: Some(PublicKeyUse::Signature),
                key_algorithm: Some(KeyAlgorithm::RS256),
                key_id: Some(rsa_kid),
                ..CommonParameters::default()
            },
            algorithm: AlgorithmParameters::RSA(RSAKeyParameters {
                key_type: RSAKeyType::RSA,
                n: URL_SAFE_NO_PAD.encode(rsa_private.n().to_bytes_be()),
                e: URL_SAFE_NO_PAD.encode(rsa_private.e().to_bytes_be()),
            }),
        };

        let ec_signing = EcSigningKey::random(&mut OsRng);
        let ec_pem = ec_signing
            .to_pkcs8_pem(EcLineEnding::LF)
            .unwrap_or_else(|err| unreachable!("encoding ec test key: {err}"));
        let ec_encoding_key = EncodingKey::from_ec_pem(ec_pem.as_bytes())
            .unwrap_or_else(|err| unreachable!("loading ec encoding key: {err}"));
        let point = ec_signing.verifying_key().to_encoded_point(false);
        let ec_jwk = Jwk {
            common: CommonParameters {
                public_key_use: Some(PublicKeyUse::Signature),
                key_algorithm: Some(KeyAlgorithm::ES256),
                key_id: Some(ec_kid),
                ..CommonParameters::default()
            },
            algorithm: AlgorithmParameters::EllipticCurve(EllipticCurveKeyParameters {
                key_type: EllipticCurveKeyType::EC,
                curve: EllipticCurve::P256,
                x: URL_SAFE_NO_PAD.encode(
                    point
                        .x()
                        .unwrap_or_else(|| unreachable!("uncompressed point has x")),
                ),
                y: URL_SAFE_NO_PAD.encode(
                    point
                        .y()
                        .unwrap_or_else(|| unreachable!("uncompressed point has y")),
                ),
            }),
        };

        Self {
            rsa_encoding_key,
            rsa_jwk,
            ec_encoding_key,
            ec_jwk,
        }
    }

    /// Mints a JWT with `claims` signed under `algorithm`, using this issuer's matching
    /// key and setting its `kid` header to that key's JWKS entry.
    ///
    /// # Panics
    ///
    /// Panics if `algorithm` is neither `RS256` nor `ES256` — this test issuer mints only
    /// those two, matching plan 02's test-issuer requirement.
    #[must_use]
    pub fn mint<C: Serialize>(&self, claims: &C, algorithm: Algorithm) -> String {
        let (kid, encoding_key) = match algorithm {
            Algorithm::RS256 => (&self.rsa_jwk.common.key_id, &self.rsa_encoding_key),
            Algorithm::ES256 => (&self.ec_jwk.common.key_id, &self.ec_encoding_key),
            other => unreachable!("test issuer only mints RS256 and ES256, got {other:?}"),
        };
        let mut header = Header::new(algorithm);
        header.kid.clone_from(kid);
        encode(&header, claims, encoding_key)
            .unwrap_or_else(|err| unreachable!("signing test token: {err}"))
    }

    #[must_use]
    pub fn jwks(&self) -> JwkSet {
        JwkSet {
            keys: vec![self.rsa_jwk.clone(), self.ec_jwk.clone()],
        }
    }
}

use jsonwebtoken::Algorithm as JwtAlgorithm;
use url::Url;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A mock OIDC provider serving discovery, JWKS, and (once mounted) `userinfo` over real
/// HTTP through wiremock, so tests exercise `postit-identity`'s actual HTTP code path
/// (`discovery::HttpJwksSource` / `discovery::OidcDiscovery`) instead of stubbing it out.
pub struct TestIssuer {
    keys: Keys,
    server: MockServer,
}

impl TestIssuer {
    pub async fn start() -> Self {
        let server = postit_http::testkit::test_server().await;
        let keys = Keys::generate();
        let jwks = keys.jwks();

        let discovery_body = serde_json::json!({
            "issuer": server.uri(),
            "jwks_uri": format!("{}/jwks.json", server.uri()),
            "userinfo_endpoint": format!("{}/userinfo", server.uri()),
        });

        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(200).set_body_json(discovery_body))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/jwks.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&jwks))
            .mount(&server)
            .await;

        Self { keys, server }
    }

    #[must_use]
    pub fn issuer_url(&self) -> Url {
        Url::parse(&self.server.uri())
            .unwrap_or_else(|err| unreachable!("wiremock uri is always a valid url: {err}"))
    }

    #[must_use]
    pub fn mint<C: serde::Serialize>(&self, claims: &C, algorithm: JwtAlgorithm) -> String {
        self.keys.mint(claims, algorithm)
    }

    /// Mounts a `GET /userinfo` response that returns `body` only when the request carries
    /// `Authorization: Bearer {bearer_token}`, so a test can verify the transform layer
    /// (Task 14) sends the same token it was given, not a different one.
    pub async fn mount_userinfo(&self, bearer_token: &str, body: serde_json::Value) {
        Mock::given(method("GET"))
            .and(path("/userinfo"))
            .and(header(
                "Authorization",
                format!("Bearer {bearer_token}").as_str(),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&self.server)
            .await;
    }
}
