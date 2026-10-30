//! VAPID (RFC 8292): the deployment's P-256 key pair and the `Authorization` header that
//! identifies it to push services.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, TimeDelta, Utc};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use p256::SecretKey;
use p256::elliptic_curve::Generate as _;
use p256::elliptic_curve::sec1::ToSec1Point as _;
use p256::pkcs8::EncodePrivateKey as _;
use postit_config::PushSettings;
use secrecy::SecretString;
use serde::Serialize;
use url::Url;

use crate::error::NotifyError;

/// How long a VAPID token stays valid. RFC 8292 allows at most 24 hours; half of that leaves
/// room for clock skew between postit and the push service.
pub const TOKEN_LIFETIME: TimeDelta = TimeDelta::hours(12);

/// The deployment's VAPID key pair and contact. Its `Debug` shows neither key.
pub struct VapidKeys {
    encoding: EncodingKey,
    public: String,
    subject: String,
}

impl std::fmt::Debug for VapidKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VapidKeys")
            .field("subject", &self.subject)
            .finish_non_exhaustive()
    }
}

#[derive(Serialize)]
struct Claims<'a> {
    aud: &'a str,
    exp: i64,
    sub: &'a str,
}

fn bad_key() -> NotifyError {
    NotifyError::Push(
        "push.vapid_private_key is not a P-256 private key in base64url (run `cargo xtask \
         vapid-keygen`)"
            .to_owned(),
    )
}

impl VapidKeys {
    /// The keys `settings` configure; `None` when push is off.
    ///
    /// # Errors
    ///
    /// [`NotifyError::Push`] naming the setting when push is on and the key or subject is
    /// missing or the key is not a P-256 private key. The message never holds the key.
    pub fn from_settings(settings: &PushSettings) -> Result<Option<Self>, NotifyError> {
        if !settings.enabled {
            return Ok(None);
        }
        let key = settings.vapid_private_key.as_ref().ok_or_else(|| {
            NotifyError::Push("push.vapid_private_key is required when push is on".to_owned())
        })?;
        let subject = settings.vapid_subject.as_deref().ok_or_else(|| {
            NotifyError::Push("push.vapid_subject is required when push is on".to_owned())
        })?;
        Self::new(key.expose(), subject).map(Some)
    }

    /// Keys from the raw 32-byte private scalar in base64url (padding optional) and the
    /// contact `subject` (`mailto:` or `https:`).
    ///
    /// # Errors
    ///
    /// [`NotifyError::Push`] when the key does not decode to a P-256 private key.
    pub fn new(private_key_b64url: &str, subject: &str) -> Result<Self, NotifyError> {
        let raw = URL_SAFE_NO_PAD
            .decode(private_key_b64url.trim().trim_end_matches('='))
            .map_err(|_| bad_key())?;
        let secret = SecretKey::from_slice(&raw).map_err(|_| bad_key())?;
        let der = secret.to_pkcs8_der().map_err(|_| bad_key())?;
        let public = secret.public_key();
        Ok(Self {
            encoding: EncodingKey::from_ec_der(der.as_bytes()),
            public: URL_SAFE_NO_PAD.encode(public.as_affine().to_sec1_point(false).as_bytes()),
            subject: subject.to_owned(),
        })
    }

    /// The public key browsers subscribe with (`applicationServerKey`): the uncompressed
    /// P-256 point, base64url without padding.
    #[must_use]
    pub fn public_key_b64url(&self) -> String {
        self.public.clone()
    }

    /// The `Authorization` header for a push to `endpoint` at `now`:
    /// `vapid t=<ES256 JWT>, k=<public key>`, the token's audience being the endpoint's
    /// origin (with its port when not the default) and its expiry [`TOKEN_LIFETIME`] away.
    ///
    /// # Errors
    ///
    /// [`NotifyError::Push`] when the endpoint has no origin or signing fails.
    pub fn authorization(&self, endpoint: &Url, now: DateTime<Utc>) -> Result<String, NotifyError> {
        let origin = endpoint.origin();
        if !origin.is_tuple() {
            return Err(NotifyError::Push(
                "the push endpoint has no origin".to_owned(),
            ));
        }
        let aud = origin.ascii_serialization();
        let claims = Claims {
            aud: &aud,
            exp: (now + TOKEN_LIFETIME).timestamp(),
            sub: &self.subject,
        };
        let token = jsonwebtoken::encode(&Header::new(Algorithm::ES256), &claims, &self.encoding)
            .map_err(|_| NotifyError::Push("signing the VAPID token failed".to_owned()))?;
        Ok(format!("vapid t={token}, k={}", self.public))
    }
}

/// A new VAPID key pair: the private scalar and the public point, both base64url without
/// padding. `cargo xtask vapid-keygen` prints them.
///
/// # Errors
///
/// [`NotifyError::Push`] when the system random source fails.
pub fn generate_key_pair() -> Result<(SecretString, String), NotifyError> {
    let secret =
        SecretKey::try_generate().map_err(|_| NotifyError::Push("no random source".to_owned()))?;
    let private = URL_SAFE_NO_PAD.encode(secret.to_bytes());
    let public = URL_SAFE_NO_PAD.encode(
        secret
            .public_key()
            .as_affine()
            .to_sec1_point(false)
            .as_bytes(),
    );
    Ok((SecretString::from(private), public))
}
