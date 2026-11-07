//! `If-Match` preconditions over strong `"v{n}"` entity tags.

use axum::http::{HeaderMap, HeaderValue, header};

use crate::error::{ApiError, ErrorCode};

/// A strong `ETag` over a resource version: `"v{n}"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ETag(pub i64);

impl ETag {
    /// The `ETag` header value, `"v{n}"`.
    #[must_use]
    pub fn header_value(self) -> HeaderValue {
        HeaderValue::from_str(&format!("\"v{}\"", self.0))
            .unwrap_or_else(|_| HeaderValue::from_static("\"v0\""))
    }
}

/// A strong entity tag over the SHA-256 of `bytes`: `"<hex>"`. For responses with no version
/// column to tag them (the calendar, an icon).
#[must_use]
pub fn content_etag(bytes: &[u8]) -> HeaderValue {
    use emixcrypto::HashAlgorithm as _;
    use std::fmt::Write as _;

    let digest = emixcrypto::Sha256Hash::new()
        .compute_hash_bytes(bytes)
        .unwrap_or_default();
    let hex = digest.iter().fold(String::with_capacity(64), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    });
    HeaderValue::from_str(&format!("\"{hex}\""))
        .unwrap_or_else(|_| HeaderValue::from_static("\"0\""))
}

/// Whether `If-None-Match` matches `tag`, by strong comparison: `*` matches, a list of tags
/// matches when one equals `tag`, and a weak tag (`W/"..."`) never matches. A `true` answers
/// `304 Not Modified` on a `GET`.
#[must_use]
pub fn if_none_match(headers: &HeaderMap, tag: &HeaderValue) -> bool {
    let Some(value) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let Ok(tag) = tag.to_str() else {
        return false;
    };
    value.trim() == "*"
        || value
            .split(',')
            .map(str::trim)
            .any(|candidate| candidate == tag)
}

/// # Errors
///
/// `precondition_required` (428) without `If-Match`; `version_conflict` (412) when it names
/// neither `*` nor the current version. Weak tags (`W/"v7"`) never match: RFC 9110 requires
/// strong comparison for `If-Match`.
pub fn check_if_match(headers: &HeaderMap, current: ETag) -> Result<(), ApiError> {
    let Some(value) = headers.get(header::IF_MATCH) else {
        return Err(ApiError::new(ErrorCode::PreconditionRequired));
    };
    let expected = format!("\"v{}\"", current.0);
    let matches = value
        .to_str()
        .is_ok_and(|v| v.trim() == "*" || v.split(',').map(str::trim).any(|tag| tag == expected));
    if matches {
        Ok(())
    } else {
        Err(ApiError::new(ErrorCode::VersionConflict))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn headers(value: &str) -> HeaderMap {
        let mut map = HeaderMap::new();
        map.insert(header::IF_NONE_MATCH, HeaderValue::from_str(value).unwrap());
        map
    }

    #[test]
    fn if_none_match_compares_strongly() {
        let tag = HeaderValue::from_static("\"v3\"");
        assert!(if_none_match(&headers("\"v3\""), &tag));
        assert!(if_none_match(&headers("*"), &tag));
        assert!(if_none_match(&headers("\"v1\", \"v3\""), &tag));
        assert!(!if_none_match(&headers("\"v2\""), &tag));
        assert!(!if_none_match(&headers("W/\"v3\""), &tag));
        assert!(!if_none_match(&HeaderMap::new(), &tag));
    }

    #[test]
    fn content_etag_is_a_quoted_hex_digest() {
        let tag = content_etag(b"abc");
        assert_eq!(
            tag.to_str().unwrap(),
            "\"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\""
        );
    }
}
