//! Verification of the `Zitadel-Signature` header on Zitadel webhook calls.
//!
//! The header is `t=<unix seconds>,v1=<hex>`; the value is HMAC-SHA256 over `"{t}.{raw body}"`
//! keyed by the signing key's UTF-8 bytes. The body must be the raw bytes as received (Zitadel
//! escapes `&` as `&`, so re-serializing would break the signature).

use chrono::{DateTime, Duration, Utc};
use hmac::{Hmac, Mac};
use sha2::Sha256;

/// Lowercase header name, as `axum`/`http` store it.
pub const HEADER: &str = "zitadel-signature";

/// Why a signature was rejected.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum SignatureError {
    /// The header is absent.
    #[error("signature header missing")]
    Missing,
    /// The header cannot be parsed.
    #[error("signature header malformed")]
    Malformed,
    /// The timestamp is outside the tolerance window.
    #[error("signature timestamp outside tolerance")]
    Expired,
    /// No signature matches the body and key.
    #[error("signature does not match")]
    Mismatch,
}

type HmacSha256 = Hmac<Sha256>;

/// Verifies `header` against `body` and `key`.
///
/// Accepts `t=<unix seconds>,v1=<hex>[,v1=<hex>...]` with optional whitespace around parts.
/// Valid when `|now - t| <= tolerance` and any `v1` equals HMAC-SHA256(key, `"{t}.{body}"`),
/// compared in constant time.
///
/// # Errors
///
/// Returns a [`SignatureError`] naming why the header was rejected.
pub fn verify(
    header: Option<&str>,
    body: &[u8],
    key: &[u8],
    now: DateTime<Utc>,
    tolerance: Duration,
) -> Result<(), SignatureError> {
    let header = header.ok_or(SignatureError::Missing)?;
    let mut timestamp: Option<&str> = None;
    let mut signatures: Vec<Vec<u8>> = Vec::new();
    for part in header.split(',') {
        let (name, value) = part
            .trim()
            .split_once('=')
            .ok_or(SignatureError::Malformed)?;
        match name.trim() {
            "t" => timestamp = Some(value.trim()),
            "v1" => {
                signatures.push(hex::decode(value.trim()).map_err(|_| SignatureError::Malformed)?);
            }
            _ => {}
        }
    }
    let timestamp = timestamp.ok_or(SignatureError::Malformed)?;
    if signatures.is_empty() {
        return Err(SignatureError::Malformed);
    }
    let seconds: i64 = timestamp.parse().map_err(|_| SignatureError::Malformed)?;
    let signed_at = DateTime::from_timestamp(seconds, 0).ok_or(SignatureError::Malformed)?;
    if (now - signed_at).abs() > tolerance {
        return Err(SignatureError::Expired);
    }
    let mut mac = HmacSha256::new_from_slice(key).map_err(|_| SignatureError::Mismatch)?;
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(body);
    let matched = signatures
        .iter()
        .any(|candidate| mac.clone().verify_slice(candidate).is_ok());
    if matched {
        Ok(())
    } else {
        Err(SignatureError::Mismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &[u8] = b"signing-key";
    const BODY: &[u8] = br#"{"url":"https://x/?a=1&b=2"}"#;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap_or_else(|| unreachable!("valid"))
    }

    fn mac_hex(key: &[u8], t: i64, body: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(key).unwrap_or_else(|e| unreachable!("{e}"));
        mac.update(format!("{t}.").as_bytes());
        mac.update(body);
        hex::encode(mac.finalize().into_bytes())
    }

    fn header(key: &[u8], t: i64, body: &[u8]) -> String {
        format!("t={t},v1={}", mac_hex(key, t, body))
    }

    fn check(h: Option<&str>, body: &[u8]) -> Result<(), SignatureError> {
        verify(h, body, KEY, now(), Duration::seconds(300))
    }

    #[test]
    fn accepts_a_valid_signature() {
        let h = header(KEY, now().timestamp(), BODY);
        assert_eq!(check(Some(&h), BODY), Ok(()));
    }

    #[test]
    fn missing_header() {
        assert_eq!(check(None, BODY), Err(SignatureError::Missing));
    }

    #[test]
    fn malformed_headers() {
        for h in ["garbage", "t=abc,v1=00", "t=1"] {
            assert_eq!(check(Some(h), BODY), Err(SignatureError::Malformed), "{h}");
        }
    }

    #[test]
    fn wrong_key_and_tampered_body() {
        let t = now().timestamp();
        let wrong_key = header(b"other", t, BODY);
        assert_eq!(check(Some(&wrong_key), BODY), Err(SignatureError::Mismatch));
        let good = header(KEY, t, BODY);
        assert_eq!(check(Some(&good), b"{}"), Err(SignatureError::Mismatch));
    }

    #[test]
    fn stale_and_future_timestamps() {
        let t = now().timestamp();
        for skew in [-301, 301] {
            let h = header(KEY, t + skew, BODY);
            assert_eq!(
                check(Some(&h), BODY),
                Err(SignatureError::Expired),
                "{skew}"
            );
        }
        for skew in [-300, 300] {
            let h = header(KEY, t + skew, BODY);
            assert_eq!(check(Some(&h), BODY), Ok(()), "{skew}");
        }
    }

    #[test]
    fn any_of_several_v1_matches() {
        let t = now().timestamp();
        let h = format!(
            "t={t},v1={},v1={}",
            mac_hex(b"other", t, BODY),
            mac_hex(KEY, t, BODY)
        );
        assert_eq!(check(Some(&h), BODY), Ok(()));
    }

    #[test]
    fn tolerates_spaces() {
        let t = now().timestamp();
        let h = format!("t={t}, v1={}", mac_hex(KEY, t, BODY));
        assert_eq!(check(Some(&h), BODY), Ok(()));
    }

    #[test]
    fn odd_length_or_non_hex_v1() {
        let t = now().timestamp();
        for v in ["abc", "zz"] {
            let h = format!("t={t},v1={v}");
            assert_eq!(check(Some(&h), BODY), Err(SignatureError::Malformed), "{v}");
        }
    }
}
