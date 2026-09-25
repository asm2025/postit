use std::time::Duration;

use thiserror::Error;

/// A `reqwest` failure or non-2xx response, classified for retry decisions. Plugins in
/// plan 03 map this to `PlatformError`; `postit-http` knows nothing about platforms.
#[derive(Debug, Error)]
pub enum HttpError {
    /// Worth retrying with backoff: connect/timeout failures and 5xx responses.
    #[error("transient http error: {0}")]
    Transient(String),

    /// A 429 response. `retry_after` comes from a numeric `Retry-After` header when present.
    #[error("rate limited")]
    RateLimited { retry_after: Option<Duration> },

    /// Not worth retrying: 4xx responses (other than 429) and request-building failures.
    #[error("permanent http error: {0}")]
    Permanent(String),
}

impl HttpError {
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Transient(_) | Self::RateLimited { .. })
    }
}

impl From<reqwest::Error> for HttpError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() || err.is_connect() {
            return Self::Transient(err.to_string());
        }

        match err.status() {
            Some(status) if status == reqwest::StatusCode::TOO_MANY_REQUESTS => {
                Self::RateLimited { retry_after: None }
            }
            Some(status) if status.is_server_error() => Self::Transient(err.to_string()),
            _ => Self::Permanent(err.to_string()),
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

/// Parses a numeric (delta-seconds) `Retry-After` header. The HTTP-date form is out of
/// scope for the foundation; plan 03 phase B1 extends this.
#[must_use]
pub fn retry_after_from_headers(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}
