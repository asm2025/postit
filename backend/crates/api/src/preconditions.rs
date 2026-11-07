//! `If-Match` preconditions over strong `"v{n}"` entity tags, and `If-None-Match` over tags
//! that also cover the representation.

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

    /// The `ETag` of a representation that can change while the version stays put (a post's
    /// status, a job's send progress): `"v{n}-{hash}"`, where `hash` is the first 16 hex digits
    /// of the SHA-256 of `body`. `If-None-Match` compares the whole tag, so a changed body is
    /// never answered 304; [`check_if_match`] compares only the version part, so the tag still
    /// guards writes on the version.
    #[must_use]
    pub fn of_body(self, body: &[u8]) -> HeaderValue {
        let hex = sha256_hex(body);
        HeaderValue::from_str(&format!("\"v{}-{}\"", self.0, &hex[..16]))
            .unwrap_or_else(|_| self.header_value())
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use emixcrypto::HashAlgorithm as _;
    use std::fmt::Write as _;

    let digest = emixcrypto::Sha256Hash::new()
        .compute_hash_bytes(bytes)
        .unwrap_or_default();
    let mut hex = digest.iter().fold(String::with_capacity(64), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    });
    if hex.len() < 64 {
        hex = format!("{hex:0<64}");
    }
    hex
}

/// A strong entity tag over the SHA-256 of `bytes`: `"<hex>"`. For responses with no version
/// column to tag them (the calendar, an icon).
#[must_use]
pub fn content_etag(bytes: &[u8]) -> HeaderValue {
    HeaderValue::from_str(&format!("\"{}\"", sha256_hex(bytes)))
        .unwrap_or_else(|_| HeaderValue::from_static("\"0\""))
}

/// Whether `If-None-Match` matches `tag`, by weak comparison as RFC 9110 requires: `*`
/// matches, and a list of tags matches when one equals `tag` once any `W/` prefix is dropped
/// from both. A `true` answers `304 Not Modified` on a `GET`.
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
    let opaque = |t: &str| t.strip_prefix("W/").unwrap_or(t).to_owned();
    let tag = opaque(tag);
    value.trim() == "*"
        || value
            .split(',')
            .map(str::trim)
            .any(|candidate| opaque(candidate) == tag)
}

/// The version a single strong `If-Match` tag names (`"v{n}"` or `"v{n}-{hash}"`), so a write
/// can lock the resource at it and let the lock refuse a stale one, with no read first.
/// `Ok(None)` for anything else (`*`, a list, a tag that names no version): the caller reads
/// the current version and runs [`check_if_match`].
///
/// # Errors
///
/// `precondition_required` (428) without `If-Match`.
pub fn if_match_version(headers: &HeaderMap) -> Result<Option<i64>, ApiError> {
    let Some(value) = headers.get(header::IF_MATCH) else {
        return Err(ApiError::new(ErrorCode::PreconditionRequired));
    };
    let Ok(value) = value.to_str() else {
        return Ok(None);
    };
    let Some(inner) = value
        .trim()
        .strip_prefix("\"v")
        .and_then(|rest| rest.strip_suffix('"'))
    else {
        return Ok(None);
    };
    let digits = inner.split_once('-').map_or(inner, |(version, _)| version);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) || inner.contains('"') {
        return Ok(None);
    }
    Ok(digits.parse().ok())
}

/// # Errors
///
/// `precondition_required` (428) without `If-Match`; `version_conflict` (412) when it names
/// neither `*` nor the current version. A tag names the version as `"v{n}"` or as
/// `"v{n}-{hash}"` ([`ETag::of_body`]); only the version part is compared. Weak tags
/// (`W/"v7"`) never match: RFC 9110 requires strong comparison for `If-Match`.
pub fn check_if_match(headers: &HeaderMap, current: ETag) -> Result<(), ApiError> {
    let Some(value) = headers.get(header::IF_MATCH) else {
        return Err(ApiError::new(ErrorCode::PreconditionRequired));
    };
    let bare = format!("\"v{}\"", current.0);
    let prefix = format!("\"v{}-", current.0);
    let names_current = |tag: &str| tag == bare || (tag.starts_with(&prefix) && tag.ends_with('"'));
    let matches = value
        .to_str()
        .is_ok_and(|v| v.trim() == "*" || v.split(',').map(str::trim).any(names_current));
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
    fn if_none_match_compares_weakly() {
        let tag = HeaderValue::from_static("\"v3\"");
        assert!(if_none_match(&headers("\"v3\""), &tag));
        assert!(if_none_match(&headers("*"), &tag));
        assert!(if_none_match(&headers("\"v1\", \"v3\""), &tag));
        assert!(!if_none_match(&headers("\"v2\""), &tag));
        assert!(if_none_match(&headers("W/\"v3\""), &tag));
        assert!(!if_none_match(&HeaderMap::new(), &tag));
    }

    fn if_match(value: &str) -> HeaderMap {
        let mut map = HeaderMap::new();
        map.insert(header::IF_MATCH, HeaderValue::from_str(value).unwrap());
        map
    }

    #[test]
    fn body_tag_names_the_version_and_the_body() {
        let a = ETag(3).of_body(b"draft");
        let b = ETag(3).of_body(b"published");
        let a = a.to_str().unwrap();
        assert!(a.starts_with("\"v3-") && a.ends_with('"'));
        assert_eq!(a.len(), "\"v3-\"".len() + 16);
        assert_ne!(a, b.to_str().unwrap());
        assert!(!if_none_match(&headers(a), &b));
    }

    #[test]
    fn if_match_compares_the_version_part() {
        let body_tag = ETag(3).of_body(b"draft");
        let body_tag = body_tag.to_str().unwrap();
        assert!(check_if_match(&if_match(body_tag), ETag(3)).is_ok());
        assert!(check_if_match(&if_match("\"v3\""), ETag(3)).is_ok());
        assert!(check_if_match(&if_match("\"v3-0123456789abcdef\""), ETag(3)).is_ok());
        assert!(check_if_match(&if_match(body_tag), ETag(4)).is_err());
        assert!(check_if_match(&if_match("\"v33\""), ETag(3)).is_err());
        assert!(check_if_match(&if_match("W/\"v3\""), ETag(3)).is_err());
    }

    #[test]
    fn if_match_version_reads_a_single_tag() {
        let body_tag = ETag(12).of_body(b"x");
        assert_eq!(
            if_match_version(&if_match(body_tag.to_str().unwrap())).unwrap(),
            Some(12)
        );
        assert_eq!(if_match_version(&if_match("\"v7\"")).unwrap(), Some(7));
        assert_eq!(if_match_version(&if_match("*")).unwrap(), None);
        assert_eq!(if_match_version(&if_match("\"v1\", \"v2\"")).unwrap(), None);
        assert_eq!(if_match_version(&if_match("W/\"v7\"")).unwrap(), None);
        assert_eq!(if_match_version(&if_match("\"vx\"")).unwrap(), None);
        assert!(if_match_version(&HeaderMap::new()).is_err());
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
