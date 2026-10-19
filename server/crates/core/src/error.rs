//! What can go wrong when talking to a platform, and how the engine should react.

use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::{Capability, Violation};

/// A stored enum string that matches no variant. Carries the rejected value's length only, so
/// no stored content reaches a log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("unrecognized {kind} value ({len} bytes)")]
pub struct ParseEnumError {
    kind: &'static str,
    len: usize,
}

impl ParseEnumError {
    pub(crate) fn new(kind: &'static str, value: &str) -> Self {
        Self {
            kind,
            len: value.len(),
        }
    }
}

/// A failure from an agent call. Messages never carry tokens or other secrets; agents redact
/// before constructing these.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AgentError {
    /// The platform rejected the account's credentials; the user must reconnect.
    #[error("authentication failed: {message}")]
    Authentication {
        /// What the platform said, redacted.
        message: String,
    },
    /// The credentials are valid but lack permission.
    #[error("missing permissions: {}", missing_scopes.join(", "))]
    Authorization {
        /// Scopes the call needs and the account lacks.
        missing_scopes: Vec<String>,
    },
    /// Credentials the user entered are wrong.
    #[error("invalid credentials{}", field.as_deref().map(|f| format!(" ({f})")).unwrap_or_default())]
    InvalidCredentials {
        /// The credential field at fault, when known.
        field: Option<String>,
    },
    /// The platform (or the agent, before calling it) found rule violations.
    #[error("validation failed with {} violation(s)", .0.len())]
    Validation(Vec<Violation>),
    /// The platform asked the caller to slow down.
    #[error("rate limited")]
    RateLimited {
        /// How long to wait, when the platform said.
        retry_after: Option<Duration>,
    },
    /// A quota bucket is used up.
    #[error("quota exhausted: {bucket}")]
    Quota {
        /// The bucket's ID from the descriptor.
        bucket: String,
        /// When the bucket resets, when known.
        resets_at: Option<DateTime<Utc>>,
    },
    /// The request did not complete: connection, DNS, or timeout. Only for a request that cannot
    /// have changed the platform; see [`AgentError::after_send`].
    #[error("transport error: {message}")]
    Transport {
        /// What failed, redacted.
        message: String,
    },
    /// The agent does not support this call. Also used for optional methods the agent does
    /// not implement.
    #[error("{capability:?} is not supported")]
    Unsupported {
        /// The capability that is missing.
        capability: Capability,
    },
    /// The platform answered with an error.
    #[error("remote error{}: {message}", status.map(|s| format!(" {s}")).unwrap_or_default())]
    Remote {
        /// HTTP status, when there was one.
        status: Option<u16>,
        /// The platform's error code, when it gave one.
        code: Option<String>,
        /// What the platform said, redacted.
        message: String,
    },
    /// A call that may have changed the platform did not finish and its effect is unknown. The
    /// engine reconciles; it never blindly retries.
    #[error("outcome unknown: {message}")]
    OutcomeUnknown {
        /// What happened, redacted.
        message: String,
    },
}

/// How the engine reacts to an [`AgentError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryClass {
    /// Try again with backoff.
    Retryable,
    /// Try again after this long.
    RetryAfter(Duration),
    /// Never succeeds without a change; fail the delivery.
    Permanent,
    /// Park the delivery until the user reconnects the account.
    NeedsReconnect,
    /// Park the delivery until the quota resets.
    QuotaExceeded,
    /// Reconcile with the platform before doing anything else.
    Ambiguous,
}

impl AgentError {
    /// How the engine should treat this error.
    #[must_use]
    pub fn retry_class(&self) -> RetryClass {
        match self {
            Self::Authentication { .. } => RetryClass::NeedsReconnect,
            Self::Authorization { .. }
            | Self::InvalidCredentials { .. }
            | Self::Validation(_)
            | Self::Unsupported { .. } => RetryClass::Permanent,
            Self::RateLimited {
                retry_after: Some(after),
            } => RetryClass::RetryAfter(*after),
            Self::RateLimited { retry_after: None } | Self::Transport { .. } => {
                RetryClass::Retryable
            }
            Self::Quota { .. } => RetryClass::QuotaExceeded,
            Self::Remote { status, .. } => match status {
                Some(501 | 505) => RetryClass::Permanent,
                None | Some(429 | 500..) => RetryClass::Retryable,
                Some(_) => RetryClass::Permanent,
            },
            Self::OutcomeUnknown { .. } => RetryClass::Ambiguous,
        }
    }

    /// The error a mutating call reports once its request may have reached the platform.
    ///
    /// A transport failure or a retryable remote error after that point does not say whether
    /// the platform acted, so it becomes [`AgentError::OutcomeUnknown`] and the engine
    /// reconciles instead of retrying, which could post twice. Rate limiting (`429`) means the
    /// platform refused the request, so it stays retryable, as does every other error.
    #[must_use]
    pub fn after_send(self) -> Self {
        let refused = matches!(
            self,
            Self::RateLimited { .. }
                | Self::Remote {
                    status: Some(429),
                    ..
                }
        );
        if refused || self.retry_class() != RetryClass::Retryable {
            return self;
        }
        Self::OutcomeUnknown {
            message: self.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ViolationCode;

    fn remote(status: Option<u16>) -> AgentError {
        AgentError::Remote {
            status,
            code: None,
            message: "m".to_owned(),
        }
    }

    #[test]
    fn retry_classification_table() {
        let secs = Duration::from_secs(30);
        let cases = [
            (
                AgentError::Authentication {
                    message: "m".into(),
                },
                RetryClass::NeedsReconnect,
            ),
            (
                AgentError::Authorization {
                    missing_scopes: vec!["a".into()],
                },
                RetryClass::Permanent,
            ),
            (
                AgentError::InvalidCredentials { field: None },
                RetryClass::Permanent,
            ),
            (
                AgentError::Validation(vec![Violation::new(ViolationCode::TextRequired)]),
                RetryClass::Permanent,
            ),
            (
                AgentError::Unsupported {
                    capability: Capability::Commenter,
                },
                RetryClass::Permanent,
            ),
            (
                AgentError::RateLimited {
                    retry_after: Some(secs),
                },
                RetryClass::RetryAfter(secs),
            ),
            (
                AgentError::RateLimited { retry_after: None },
                RetryClass::Retryable,
            ),
            (
                AgentError::Quota {
                    bucket: "b".into(),
                    resets_at: None,
                },
                RetryClass::QuotaExceeded,
            ),
            (
                AgentError::Transport {
                    message: "m".into(),
                },
                RetryClass::Retryable,
            ),
            (remote(Some(500)), RetryClass::Retryable),
            (remote(Some(503)), RetryClass::Retryable),
            (remote(Some(429)), RetryClass::Retryable),
            (remote(Some(403)), RetryClass::Permanent),
            (remote(Some(400)), RetryClass::Permanent),
            (remote(Some(501)), RetryClass::Permanent),
            (remote(Some(505)), RetryClass::Permanent),
            (remote(None), RetryClass::Retryable),
            (
                AgentError::OutcomeUnknown {
                    message: "m".into(),
                },
                RetryClass::Ambiguous,
            ),
        ];
        for (error, want) in cases {
            assert_eq!(error.retry_class(), want, "{error:?}");
        }
    }

    #[test]
    fn after_send_turns_unverdicted_failures_into_outcome_unknown() {
        let transport = AgentError::Transport {
            message: "timed out".into(),
        };
        assert_eq!(
            transport.after_send(),
            AgentError::OutcomeUnknown {
                message: "transport error: timed out".into(),
            }
        );
        assert_eq!(
            remote(Some(502)).after_send().retry_class(),
            RetryClass::Ambiguous
        );
        assert_eq!(
            remote(None).after_send().retry_class(),
            RetryClass::Ambiguous
        );
        for kept in [
            remote(Some(429)),
            remote(Some(400)),
            AgentError::RateLimited { retry_after: None },
            AgentError::Authentication {
                message: "m".into(),
            },
        ] {
            assert_eq!(kept.clone().after_send(), kept);
        }
    }

    #[test]
    fn agent_error_display_has_no_secret_fields() {
        let errors = [
            AgentError::Authentication {
                message: "expired".into(),
            },
            AgentError::Authorization {
                missing_scopes: vec!["a".into(), "b".into()],
            },
            AgentError::InvalidCredentials {
                field: Some("token".into()),
            },
            AgentError::InvalidCredentials { field: None },
            AgentError::Validation(Vec::new()),
            AgentError::RateLimited { retry_after: None },
            AgentError::Quota {
                bucket: "day".into(),
                resets_at: None,
            },
            AgentError::Transport {
                message: "timeout".into(),
            },
            AgentError::Unsupported {
                capability: Capability::InboxReader,
            },
            remote(Some(502)),
            remote(None),
            AgentError::OutcomeUnknown {
                message: "cut".into(),
            },
        ];
        for error in errors {
            assert!(!error.to_string().is_empty(), "{error:?}");
        }
        assert_eq!(
            AgentError::Authorization {
                missing_scopes: vec!["a".into(), "b".into()]
            }
            .to_string(),
            "missing permissions: a, b"
        );
        assert_eq!(remote(Some(502)).to_string(), "remote error 502: m");
    }
}
