//! Signed media URLs.
//!
//! A URL names the target and its expiry, and the signature is an HMAC-SHA256 over
//! `"{target}|{exp}"`. The signing key, signatures, and expiry values never reach a log or an
//! error text.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use postit_core::{Clock, MediaAsset, MediaAssetId, MediaAssetState, MediaError, MediaHost};
use secrecy::{ExposeSecret, SecretSlice};
use sha2::Sha256;
use url::Url;

type HmacSha256 = Hmac<Sha256>;

/// What a signed URL serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignedTarget {
    /// The stored file.
    Content(MediaAssetId),
    /// Its JPEG thumbnail.
    Thumbnail(MediaAssetId),
}

impl SignedTarget {
    /// The path below `/public/media/`: `{id}` or `{id}/thumbnail`.
    #[must_use]
    pub fn path(&self) -> String {
        match self {
            Self::Content(id) => id.as_uuid().hyphenated().to_string(),
            Self::Thumbnail(id) => format!("{}/thumbnail", id.as_uuid().hyphenated()),
        }
    }
}

/// Signs and verifies media URLs.
#[derive(Clone)]
pub struct UrlSigner {
    key: SecretSlice<u8>,
    base: Url,
}

impl fmt::Debug for UrlSigner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UrlSigner")
            .field("base", &self.base.as_str())
            .finish_non_exhaustive()
    }
}

impl UrlSigner {
    /// A signer for `key` that builds URLs on `base`.
    #[must_use]
    pub fn new(key: &[u8], base: Url) -> Self {
        Self {
            key: SecretSlice::from(key.to_vec()),
            base,
        }
    }

    fn mac(&self, target: &SignedTarget, exp: i64) -> HmacSha256 {
        // HMAC accepts keys of any length, so this cannot fail.
        let mut mac = HmacSha256::new_from_slice(self.key.expose_secret())
            .unwrap_or_else(|_| unreachable!("HMAC takes any key length"));
        mac.update(format!("{}|{exp}", target.path()).as_bytes());
        mac
    }

    /// The URL serving `target` until `exp`.
    #[must_use]
    pub fn sign(&self, target: &SignedTarget, exp: DateTime<Utc>) -> Url {
        let exp = exp.timestamp();
        let sig = URL_SAFE_NO_PAD.encode(self.mac(target, exp).finalize().into_bytes());
        let mut base = self.base.clone();
        if !base.path().ends_with('/') {
            let path = format!("{}/", base.path());
            base.set_path(&path);
        }
        let mut url = base
            .join(&format!("public/media/{}", target.path()))
            .unwrap_or_else(|_| unreachable!("a generated path is a valid relative URL"));
        url.query_pairs_mut()
            .append_pair("exp", &exp.to_string())
            .append_pair("sig", &sig);
        url
    }

    /// Whether `sig` is the signature of `target` and `exp`, and `exp` is still ahead of `now`.
    /// The comparison takes constant time.
    #[must_use]
    pub fn verify(&self, target: &SignedTarget, exp: i64, sig: &str, now: DateTime<Utc>) -> bool {
        if exp <= now.timestamp() {
            return false;
        }
        let Ok(sig) = URL_SAFE_NO_PAD.decode(sig) else {
            return false;
        };
        self.mac(target, exp).verify_slice(&sig).is_ok()
    }
}

/// Hosts stored media at signed public URLs, for platforms that fetch media themselves.
#[derive(Clone)]
pub struct SignedUrlMediaHost {
    signer: Arc<UrlSigner>,
    clock: Arc<dyn Clock>,
}

impl SignedUrlMediaHost {
    /// A host that signs with `signer` and takes the time from `clock`.
    #[must_use]
    pub fn new(signer: Arc<UrlSigner>, clock: Arc<dyn Clock>) -> Self {
        Self { signer, clock }
    }
}

impl MediaHost for SignedUrlMediaHost {
    fn public_url(&self, asset: &MediaAsset, ttl: Duration) -> Result<Url, MediaError> {
        if asset.state != MediaAssetState::Ready {
            return Err(MediaError::NotReady);
        }
        let ttl = chrono::Duration::from_std(ttl).map_err(|e| MediaError::Io(e.to_string()))?;
        Ok(self
            .signer
            .sign(&SignedTarget::Content(asset.id), self.clock.now() + ttl))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use chrono::TimeZone;
    use postit_core::{MediaKind, TestClock, UserId};
    use uuid::Uuid;

    use super::*;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 26, 12, 0, 0).unwrap()
    }

    fn signer() -> UrlSigner {
        UrlSigner::new(
            b"0123456789abcdef0123456789abcdef",
            "https://media.example/".parse().unwrap(),
        )
    }

    fn id() -> MediaAssetId {
        MediaAssetId::from_uuid(Uuid::now_v7())
    }

    fn query(url: &Url, key: &str) -> String {
        url.query_pairs()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.into_owned())
            .unwrap()
    }

    fn exp_of(url: &Url) -> i64 {
        query(url, "exp").parse().unwrap()
    }

    #[test]
    fn signed_url_shape() {
        let a = id();
        let exp = now() + chrono::Duration::minutes(10);
        let url = signer().sign(&SignedTarget::Content(a), exp);
        assert_eq!(url.host_str(), Some("media.example"));
        assert_eq!(url.path(), format!("/public/media/{}", a.as_uuid()));
        assert_eq!(exp_of(&url), exp.timestamp());
        assert!(!query(&url, "sig").is_empty());
        let thumb = signer().sign(&SignedTarget::Thumbnail(a), exp);
        assert_eq!(
            thumb.path(),
            format!("/public/media/{}/thumbnail", a.as_uuid())
        );
    }

    #[test]
    fn verify_accepts_its_own_signature() {
        let t = SignedTarget::Content(id());
        let url = signer().sign(&t, now() + chrono::Duration::minutes(1));
        assert!(signer().verify(&t, exp_of(&url), &query(&url, "sig"), now()));
    }

    #[test]
    fn verify_refuses_expired() {
        let t = SignedTarget::Content(id());
        let url = signer().sign(&t, now() + chrono::Duration::minutes(1));
        let later = now() + chrono::Duration::minutes(2);
        assert!(!signer().verify(&t, exp_of(&url), &query(&url, "sig"), later));
    }

    #[test]
    fn verify_refuses_tampered_exp() {
        let t = SignedTarget::Content(id());
        let url = signer().sign(&t, now() + chrono::Duration::minutes(1));
        assert!(!signer().verify(&t, exp_of(&url) + 3600, &query(&url, "sig"), now()));
    }

    #[test]
    fn verify_refuses_other_id() {
        let url = signer().sign(
            &SignedTarget::Content(id()),
            now() + chrono::Duration::minutes(1),
        );
        assert!(!signer().verify(
            &SignedTarget::Content(id()),
            exp_of(&url),
            &query(&url, "sig"),
            now()
        ));
    }

    #[test]
    fn verify_refuses_content_signature_on_thumbnail() {
        let a = id();
        let url = signer().sign(
            &SignedTarget::Content(a),
            now() + chrono::Duration::minutes(1),
        );
        assert!(!signer().verify(
            &SignedTarget::Thumbnail(a),
            exp_of(&url),
            &query(&url, "sig"),
            now()
        ));
    }

    #[test]
    fn verify_refuses_non_base64_sig() {
        let t = SignedTarget::Content(id());
        let exp = (now() + chrono::Duration::minutes(1)).timestamp();
        for sig in ["", "!!!", "not base64 at all", "AAAA"] {
            assert!(!signer().verify(&t, exp, sig, now()), "{sig}");
        }
    }

    #[test]
    fn base_with_path_prefix_is_kept() {
        let s = UrlSigner::new(b"k", "https://h/app/".parse().unwrap());
        let a = id();
        let url = s.sign(&SignedTarget::Content(a), now());
        assert_eq!(url.path(), format!("/app/public/media/{}", a.as_uuid()));
        let no_slash = UrlSigner::new(b"k", "https://h/app".parse().unwrap());
        let url = no_slash.sign(&SignedTarget::Content(a), now());
        assert_eq!(url.path(), format!("/app/public/media/{}", a.as_uuid()));
    }

    #[test]
    fn debug_hides_key() {
        let text = format!("{:?}", signer());
        assert!(!text.contains("0123456789abcdef"), "{text}");
    }

    fn asset(state: MediaAssetState) -> MediaAsset {
        MediaAsset {
            id: id(),
            owner_id: UserId::from_uuid(Uuid::now_v7()),
            kind: MediaKind::Image,
            mime: "image/png".into(),
            bytes: 1,
            width: None,
            height: None,
            duration_ms: None,
            sha256: None,
            state,
            derived_from: None,
        }
    }

    #[test]
    fn host_signs_ready_assets_and_refuses_the_rest() {
        let host = SignedUrlMediaHost::new(Arc::new(signer()), Arc::new(TestClock::new(now())));
        let ready = asset(MediaAssetState::Ready);
        let url = host.public_url(&ready, Duration::from_secs(3600)).unwrap();
        assert_eq!(
            exp_of(&url),
            (now() + chrono::Duration::hours(1)).timestamp()
        );
        for state in [MediaAssetState::Uploading, MediaAssetState::Failed] {
            assert_eq!(
                host.public_url(&asset(state), Duration::from_secs(60)),
                Err(MediaError::NotReady)
            );
        }
    }
}
