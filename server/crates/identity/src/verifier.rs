use std::time::Duration;

use jsonwebtoken::errors::ErrorKind;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde_json::{Map, Value};

use crate::error::VerifyError;

/// Never accepted, whatever `accepted_algorithms` says — a symmetric algorithm verified
/// with a key meant to be public (the RSA/EC public key material published in the JWKS)
/// lets an attacker who knows that public key forge a token, the classic
/// "algorithm confusion" attack. This list is checked before the configured allowlist, not
/// instead of it, so misconfiguring `auth.oidc.accepted_algorithms` to include one of these
/// still can't open the hole.
const NEVER_ACCEPTED: &[Algorithm] = &[Algorithm::HS256, Algorithm::HS384, Algorithm::HS512];

#[derive(Debug, Clone)]
pub struct VerifiedClaims {
    pub sub: String,
    pub raw: Map<String, Value>,
}

pub struct Verifier {
    issuer: String,
    audiences: Vec<String>,
    accepted_algorithms: Vec<Algorithm>,
    leeway: Duration,
}

impl Verifier {
    #[must_use]
    pub fn new(
        issuer: impl Into<String>,
        audiences: Vec<String>,
        accepted_algorithms: Vec<Algorithm>,
        leeway: Duration,
    ) -> Self {
        Self {
            issuer: issuer.into(),
            audiences,
            accepted_algorithms,
            leeway,
        }
    }

    /// # Errors
    ///
    /// Returns the specific [`VerifyError`] variant for the first check that fails:
    /// malformed token or header, an algorithm not in [`NEVER_ACCEPTED`]'s complement of
    /// `accepted_algorithms`, an unknown `kid`, a bad signature, wrong issuer or audience,
    /// or an expired/not-yet-valid token.
    pub fn verify(&self, token: &str, jwks: &JwkSet) -> Result<VerifiedClaims, VerifyError> {
        let header = decode_header(token).map_err(|_| VerifyError::Malformed)?;

        if NEVER_ACCEPTED.contains(&header.alg) || !self.accepted_algorithms.contains(&header.alg) {
            return Err(VerifyError::UnacceptedAlgorithm);
        }

        let kid = header.kid.as_deref().ok_or(VerifyError::UnknownKid)?;
        let jwk = jwks
            .keys
            .iter()
            .find(|k| k.common.key_id.as_deref() == Some(kid))
            .ok_or(VerifyError::UnknownKid)?;
        let decoding_key = DecodingKey::from_jwk(jwk).map_err(|_| VerifyError::BadSignature)?;

        let mut validation = Validation::new(header.alg);
        validation.set_issuer(&[self.issuer.as_str()]);
        validation.set_audience(&self.audiences);
        validation.leeway = self.leeway.as_secs();
        // `Validation::validate_nbf` defaults to `false` in jsonwebtoken; this verifier
        // requires it so a not-yet-valid token is rejected rather than silently accepted.
        validation.validate_nbf = true;

        let token_data = decode::<Value>(token, &decoding_key, &validation)
            .map_err(|err| map_jsonwebtoken_error(&err))?;

        let raw = token_data.claims.as_object().cloned().unwrap_or_default();
        let sub = raw
            .get("sub")
            .and_then(Value::as_str)
            .ok_or(VerifyError::Malformed)?
            .to_string();

        Ok(VerifiedClaims { sub, raw })
    }
}

fn map_jsonwebtoken_error(err: &jsonwebtoken::errors::Error) -> VerifyError {
    match err.kind() {
        ErrorKind::InvalidSignature => VerifyError::BadSignature,
        ErrorKind::InvalidIssuer => VerifyError::WrongIssuer,
        ErrorKind::InvalidAudience => VerifyError::WrongAudience,
        ErrorKind::ExpiredSignature => VerifyError::Expired,
        ErrorKind::ImmatureSignature => VerifyError::NotYetValid,
        ErrorKind::InvalidAlgorithm => VerifyError::UnacceptedAlgorithm,
        _ => VerifyError::Malformed,
    }
}
