use std::time::Duration;

use chrono::{DateTime, Utc};
use thiserror::Error;

use crate::policy::rejected_host;
use crate::redact::{redact_secrets, redacted_error_text};

const BODY_EXCERPT_LIMIT: usize = 512;

/// A `reqwest` failure or non-2xx response, classified for retry decisions. Plugins in
/// plan 03 map this to `PlatformError`; `postit-http` knows nothing about platforms.
#[derive(Debug, Error)]
pub enum HttpError {
    /// Worth retrying with backoff: connect/timeout failures and 5xx responses.
    #[error("transient http error: {0}")]
    Transient(String),

    /// A 429 response. `retry_after` comes from a numeric `Retry-After` header when present.
    #[error("rate limited")]
    RateLimited {
        /// How long to wait before retrying, when the server said.
        retry_after: Option<Duration>,
    },

    /// Not worth retrying: 4xx responses (other than 429) and request-building failures.
    #[error("permanent http error: {0}")]
    Permanent(String),

    /// A non-idempotent request may or may not have taken effect (for example the connection
    /// dropped after the body was sent). Never retried blindly; callers reconcile.
    #[error("outcome unknown: {0}")]
    OutcomeUnknown(String),

    /// The destination address was refused by the outbound address policy. Carries the host
    /// only, never a path or query.
    #[error("address rejected: {host}")]
    AddressRejected {
        /// The rejected host name.
        host: String,
    },

    /// A non-2xx response with its status, `Retry-After` hint, and a redacted body excerpt.
    #[error("http status {status}")]
    Status {
        /// The HTTP status code.
        status: u16,
        /// How long the server asked clients to wait, when it said.
        retry_after: Option<Duration>,
        /// Up to 512 bytes of the response body, with secrets redacted.
        body_excerpt: Option<String>,
    },
}

impl HttpError {
    /// Whether retrying the request could succeed: transient failures and rate limiting.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transient(_) | Self::RateLimited { .. } => true,
            Self::Status { status, .. } => *status == 429 || *status >= 500,
            Self::Permanent(_) | Self::OutcomeUnknown(_) | Self::AddressRejected { .. } => false,
        }
    }
}

impl From<reqwest::Error> for HttpError {
    fn from(err: reqwest::Error) -> Self {
        if let Some(host) = rejected_host(&err) {
            return Self::AddressRejected { host };
        }

        if err.is_timeout() || err.is_connect() {
            return Self::Transient(redacted_error_text(&err));
        }

        match err.status() {
            Some(status) if status == reqwest::StatusCode::TOO_MANY_REQUESTS => {
                Self::RateLimited { retry_after: None }
            }
            Some(status) if status.is_server_error() => Self::Transient(redacted_error_text(&err)),
            _ => Self::Permanent(redacted_error_text(&err)),
        }
    }
}

/// Classifies a response status that has already been read (so a numeric `Retry-After`
/// can be attached to a 429). Returns `None` for a successful status.
#[must_use]
pub fn classify_status(
    status: reqwest::StatusCode,
    retry_after: Option<Duration>,
) -> Option<HttpError> {
    if status.is_success() {
        return None;
    }

    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Some(HttpError::RateLimited { retry_after });
    }

    if status.is_server_error() {
        return Some(HttpError::Transient(format!("server error: {status}")));
    }

    Some(HttpError::Permanent(format!("client error: {status}")))
}

/// Parses a `Retry-After` header, either delta-seconds or an HTTP-date, relative to `now`.
/// A date in the past gives [`Duration::ZERO`]; anything unparsable gives `None`.
#[must_use]
pub fn retry_after_at(
    headers: &reqwest::header::HeaderMap,
    now: DateTime<Utc>,
) -> Option<Duration> {
    let value = headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let date = DateTime::parse_from_rfc2822(value)
        .ok()?
        .with_timezone(&Utc);
    Some((date - now).to_std().unwrap_or(Duration::ZERO))
}

/// Parses a `Retry-After` header against the current time. See [`retry_after_at`].
#[must_use]
pub fn retry_after_from_headers(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    retry_after_at(headers, Utc::now())
}

/// Builds an [`HttpError::Status`] from a non-success response, reading at most 512 bytes of
/// the body and redacting secrets from it.
pub async fn status_error(mut response: reqwest::Response, now: DateTime<Utc>) -> HttpError {
    let status = response.status().as_u16();
    let retry_after = retry_after_at(response.headers(), now);
    let mut bytes = Vec::new();
    while bytes.len() < BODY_EXCERPT_LIMIT {
        match response.chunk().await {
            Ok(Some(chunk)) => bytes.extend_from_slice(&chunk),
            _ => break,
        }
    }
    bytes.truncate(BODY_EXCERPT_LIMIT);
    let body_excerpt =
        (!bytes.is_empty()).then(|| redact_secrets(&String::from_utf8_lossy(&bytes)));
    HttpError::Status {
        status,
        retry_after,
        body_excerpt,
    }
}
