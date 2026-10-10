//! `postit-http`: the only place outbound `reqwest` clients are built (retries, redaction).
#![warn(missing_docs)]

mod client;
mod error;
mod redact;
#[cfg(feature = "testkit")]
pub mod testkit;

pub use client::{build_client, execute_traced, new_request_id, with_request_id};
pub use error::{HttpError, classify_status, retry_after_from_headers};
pub use redact::{
    REDACTED, SENSITIVE_QUERY_PARAMS, redact_header_value, redact_query_params, redact_secrets,
};
