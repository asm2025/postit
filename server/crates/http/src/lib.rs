//! `postit-http`: the only place outbound `reqwest` clients are built (retries, redaction).
#![warn(missing_docs)]

mod client;
mod error;
mod policy;
mod redact;
mod retry;
#[cfg(feature = "testkit")]
pub mod testkit;

pub use client::{
    build_client, build_public_client, execute_public, execute_traced, new_request_id,
    with_request_id,
};
pub use error::{
    HttpError, classify_status, retry_after_at, retry_after_from_headers, status_error,
};
pub use policy::{AddressPolicy, is_allowed, resolve_checked};
pub use redact::{
    REDACTED, SENSITIVE_QUERY_PARAMS, redact_header_value, redact_query_params, redact_secrets,
};
pub use retry::{
    Idempotency, Jitter, Now, RateLimitHeaders, ResetHeader, RetryPolicy, RetryRequest,
    send_with_retry,
};
