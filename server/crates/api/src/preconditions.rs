//! `If-Match` preconditions over strong `"v{n}"` entity tags.

use axum::http::{HeaderMap, HeaderValue, header};

use crate::error::{ApiError, ErrorCode};

/// A strong `ETag` over a resource version: `"v{n}"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ETag(pub i64);

impl ETag {
    #[must_use]
    pub fn header_value(self) -> HeaderValue {
        HeaderValue::from_str(&format!("\"v{}\"", self.0))
            .unwrap_or_else(|_| HeaderValue::from_static("\"v0\""))
    }
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
