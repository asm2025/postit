//! `postit-http`: the only place outbound `reqwest` clients are built (retries, redaction).
//!
//! Which entry point to use:
//!
//! - **Trusted client** ([`build_client`]): hosts the operator configures (the OIDC issuer,
//!   platform APIs with fixed hosts). Honors proxy variables and follows redirects.
//! - **Public client** ([`build_public_client`] with an [`AddressPolicy`], returning a
//!   [`PublicClient`]): any host a user supplies. Its resolver drops forbidden addresses, it
//!   follows no redirects and uses no proxy. It keeps its policy and exposes no `reqwest`
//!   send method of its own: send through [`execute_public`] or [`send_with_retry`], both of
//!   which also refuse forbidden IP literals.
//!   Callers pass `AddressPolicy { allow_private }` from the matching config flag; Mastodon
//!   and Bluesky hosts use [`AddressPolicy::strict`]. `allow_private` never lifts
//!   unspecified, multicast, broadcast or reserved ranges. SMTP, which does not use
//!   `reqwest`, calls [`resolve_checked`].
//! - **Retries** ([`send_with_retry`]): wraps either client ([`ClientRef`]). Mark each request
//!   [`Idempotency::Idempotent`] or [`Idempotency::NonIdempotent`]; a non-idempotent request
//!   that may have been acted on fails as [`HttpError::OutcomeUnknown`], never retried.
//! - **Rate limits** ([`RateLimiters`]): named buckets from [`BucketSpec`], keyed per
//!   account, so one platform's limit is enforced in one place.
//! - **Large bodies** ([`stream_body`]): upload media as a stream, never buffered.
//! - **Logging URLs** ([`Redactor`]): the server installs one at startup with
//!   [`install_redactor`], before any agent exists. Slack and Discord webhooks and Telegram's
//!   `/bot<token>` paths are built into [`Redactor::with_defaults`]; any other host that
//!   carries a secret in the path gets a [`SensitiveUrl`] added in the server's
//!   `compose::redactor` before it is installed. A URL that is secret as a whole (a push
//!   endpoint) is rendered with [`Sensitivity::Opaque`].
#![warn(missing_docs)]

mod body;
mod client;
mod error;
mod policy;
mod rate_limit;
mod redact;
mod retry;
#[cfg(feature = "testkit")]
pub mod testkit;

pub use body::stream_body;
pub use client::{
    ClientRef, PublicClient, build_client, build_public_client, execute_public, execute_traced,
    new_request_id, with_request_id,
};
pub use error::{
    HttpError, classify_status, retry_after_at, retry_after_from_headers, status_error,
};
pub use policy::{AddressPolicy, is_allowed, resolve_checked};
pub use rate_limit::{BucketSpec, RateLimiters};
pub use redact::{
    REDACTED, REDACTED_URL, Redactor, SENSITIVE_QUERY_PARAMS, SensitiveUrl, Sensitivity,
    install_redactor, redact_header_value, redact_query_params, redact_secrets, redactor,
};
pub use retry::{
    Idempotency, Jitter, Now, RateLimitHeaders, ResetHeader, RetryPolicy, RetryRequest,
    send_with_retry,
};
