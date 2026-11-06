//! Error types of the drafter.

use std::time::Duration;

use postit_core::Violation;
use postit_data::{Capability, DataError};

/// Why one provider call failed. The reason names the class; text from the provider's body is
/// never carried, since it may echo the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureReason {
    /// The provider refused the key (401, 403).
    KeyRejected,
    /// The model does not exist for this provider (404).
    ModelNotFound,
    /// The provider is rate limiting the key (429).
    RateLimited,
    /// The provider rejected the request (other 4xx).
    BadRequest,
    /// The provider is down or unreachable (5xx, connection refused).
    Unavailable,
    /// The deadline passed.
    Timeout,
    /// The address policy refused the destination.
    AddressRefused,
    /// The body was not the protocol's shape, was a redirect, or was too large.
    InvalidResponse,
    /// The model's answer was not usable, even after one repair.
    InvalidOutput,
}

impl FailureReason {
    /// The wire name used in `ai_provider_failed` details.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::KeyRejected => "key_rejected",
            Self::ModelNotFound => "model_not_found",
            Self::RateLimited => "rate_limited",
            Self::BadRequest => "bad_request",
            Self::Unavailable => "unavailable",
            Self::Timeout => "timeout",
            Self::AddressRefused => "address_refused",
            Self::InvalidResponse => "invalid_response",
            Self::InvalidOutput => "invalid_output",
        }
    }
}

/// A failed provider call: its reason, and the provider's `Retry-After` when it sent one.
#[derive(Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{}", .reason.as_str())]
pub struct LlmError {
    /// What went wrong.
    pub reason: FailureReason,
    /// How long the provider asked to wait.
    pub retry_after: Option<Duration>,
}

impl std::fmt::Debug for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LlmError({})", self.reason.as_str())
    }
}

impl LlmError {
    /// An error with no `Retry-After`.
    #[must_use]
    pub const fn new(reason: FailureReason) -> Self {
        Self {
            reason,
            retry_after: None,
        }
    }
}

impl From<FailureReason> for LlmError {
    fn from(reason: FailureReason) -> Self {
        Self::new(reason)
    }
}

/// What a `DrafterService` call can fail with.
#[derive(Debug, thiserror::Error)]
pub enum DrafterError {
    /// No provider was given and the owner has no default, or the given one is gone.
    #[error("an AI provider is required")]
    ProviderRequired,
    /// The provider call failed.
    #[error("the AI provider failed: {0}")]
    ProviderFailed(LlmError),
    /// The request or the pasted configuration broke rules.
    #[error("validation failed")]
    Validation(Vec<Violation>),
    /// The row does not exist for this scope.
    #[error("not found")]
    NotFound,
    /// The scope lacks the capability.
    #[error("not allowed: {0:?}")]
    Forbidden(Capability),
    /// A database call failed.
    #[error(transparent)]
    Data(#[from] DataError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasons_have_snake_case_names_and_display_names_only() {
        let all = [
            (FailureReason::KeyRejected, "key_rejected"),
            (FailureReason::ModelNotFound, "model_not_found"),
            (FailureReason::RateLimited, "rate_limited"),
            (FailureReason::BadRequest, "bad_request"),
            (FailureReason::Unavailable, "unavailable"),
            (FailureReason::Timeout, "timeout"),
            (FailureReason::AddressRefused, "address_refused"),
            (FailureReason::InvalidResponse, "invalid_response"),
            (FailureReason::InvalidOutput, "invalid_output"),
        ];
        for (reason, name) in all {
            assert_eq!(reason.as_str(), name);
            assert_eq!(LlmError::new(reason).to_string(), name);
        }
    }
}
