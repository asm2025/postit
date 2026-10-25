//! Mapping `postit-http` failures to the [`AgentError`] the engine reacts to.

use postit_core::{AgentError, Message, Violation};
use postit_http::HttpError;

/// Whether the request had reached the platform when it failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Nothing was sent that could change the platform (reads, validation, upload init).
    BeforeSend,
    /// The request that creates or changes something was sent; see [`AgentError::after_send`].
    AfterSend,
}

/// A platform-specific mapping tried first for a status response: the status and the redacted
/// body excerpt in, a more precise error out, or `None` to use the default mapping.
pub type ErrorHook = dyn Fn(u16, Option<&str>) -> Option<AgentError> + Send + Sync;

fn rule(key: &'static str, en: &'static str) -> AgentError {
    AgentError::Validation(vec![Violation::agent_rule(Message::new_static(key, en))])
}

fn map(err: HttpError, hook: Option<&ErrorHook>) -> AgentError {
    match err {
        HttpError::Status {
            status,
            retry_after,
            body_excerpt,
        } => {
            if let Some(mapped) = hook.and_then(|hook| hook(status, body_excerpt.as_deref())) {
                return mapped;
            }
            match status {
                401 => AgentError::Authentication {
                    message: "http status 401".to_owned(),
                },
                403 => AgentError::Authorization {
                    missing_scopes: Vec::new(),
                },
                429 => AgentError::RateLimited { retry_after },
                _ => AgentError::Remote {
                    status: Some(status),
                    code: None,
                    message: body_excerpt.unwrap_or_else(|| format!("http status {status}")),
                },
            }
        }
        HttpError::RateLimited { retry_after } => AgentError::RateLimited { retry_after },
        HttpError::Transient(message) => AgentError::Transport { message },
        HttpError::Permanent(message) => {
            tracing::warn!(
                detail = %postit_http::redact_secrets(&message),
                "the platform request was rejected"
            );
            rule(
                "agent_kit.request_rejected",
                "The platform rejected the request.",
            )
        }
        HttpError::OutcomeUnknown(message) => AgentError::OutcomeUnknown { message },
        HttpError::AddressRejected { .. } => rule(
            "agent_kit.address_rejected",
            "The server address is not allowed.",
        ),
    }
}

/// Maps an HTTP failure to an agent error. The `hook` runs first for status responses so an agent
/// can recognize its platform's error codes. In [`Phase::AfterSend`] the result also goes through
/// [`AgentError::after_send`], so an ambiguous failure becomes `OutcomeUnknown`.
#[must_use]
pub fn classify(err: HttpError, phase: Phase, hook: Option<&ErrorHook>) -> AgentError {
    let mapped = map(err, hook);
    match phase {
        Phase::BeforeSend => mapped,
        Phase::AfterSend => mapped.after_send(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use postit_core::RetryClass;

    use super::*;

    fn status(code: u16, body: Option<&str>) -> HttpError {
        HttpError::Status {
            status: code,
            retry_after: None,
            body_excerpt: body.map(str::to_owned),
        }
    }

    #[test]
    fn classify_table() {
        let c = |e| classify(e, Phase::BeforeSend, None);

        let e = c(status(401, Some("nope")));
        assert_eq!(
            e,
            AgentError::Authentication {
                message: "http status 401".to_owned()
            }
        );
        assert_eq!(e.retry_class(), RetryClass::NeedsReconnect);

        let e = c(status(403, None));
        assert_eq!(
            e,
            AgentError::Authorization {
                missing_scopes: vec![]
            }
        );
        assert_eq!(e.retry_class(), RetryClass::Permanent);

        let after = Some(Duration::from_secs(7));
        let e = c(HttpError::Status {
            status: 429,
            retry_after: after,
            body_excerpt: None,
        });
        assert_eq!(e, AgentError::RateLimited { retry_after: after });
        assert_eq!(
            e.retry_class(),
            RetryClass::RetryAfter(Duration::from_secs(7))
        );
        let e = c(HttpError::RateLimited { retry_after: after });
        assert_eq!(e, AgentError::RateLimited { retry_after: after });

        let e = c(status(404, Some("gone")));
        assert_eq!(
            e,
            AgentError::Remote {
                status: Some(404),
                code: None,
                message: "gone".to_owned()
            }
        );
        assert_eq!(e.retry_class(), RetryClass::Permanent);
        let e = c(status(502, None));
        assert_eq!(
            e,
            AgentError::Remote {
                status: Some(502),
                code: None,
                message: "http status 502".to_owned()
            }
        );
        assert_eq!(e.retry_class(), RetryClass::Retryable);

        let e = c(HttpError::Transient("reset".to_owned()));
        assert_eq!(
            e,
            AgentError::Transport {
                message: "reset".to_owned()
            }
        );
        assert_eq!(e.retry_class(), RetryClass::Retryable);

        let e = c(HttpError::Permanent("bad url".to_owned()));
        assert!(matches!(e, AgentError::Validation(_)));
        assert_eq!(e.retry_class(), RetryClass::Permanent);

        let e = c(HttpError::OutcomeUnknown("dropped".to_owned()));
        assert_eq!(
            e,
            AgentError::OutcomeUnknown {
                message: "dropped".to_owned()
            }
        );
        assert_eq!(e.retry_class(), RetryClass::Ambiguous);

        let e = c(HttpError::AddressRejected {
            host: "10.0.0.1".to_owned(),
        });
        assert!(matches!(e, AgentError::Validation(_)));
        assert_eq!(e.retry_class(), RetryClass::Permanent);
    }

    #[test]
    fn hook_runs_first() {
        let hook = |code: u16, body: Option<&str>| {
            (code == 400 && body == Some("token_revoked")).then(|| AgentError::Authentication {
                message: "revoked".to_owned(),
            })
        };
        let e = classify(
            status(400, Some("token_revoked")),
            Phase::BeforeSend,
            Some(&hook),
        );
        assert_eq!(e.retry_class(), RetryClass::NeedsReconnect);
        let e = classify(status(400, Some("other")), Phase::BeforeSend, Some(&hook));
        assert_eq!(e.retry_class(), RetryClass::Permanent);
    }

    #[test]
    fn after_send_makes_5xx_ambiguous() {
        let e = classify(status(500, None), Phase::AfterSend, None);
        assert_eq!(e.retry_class(), RetryClass::Ambiguous);
        let e = classify(status(500, None), Phase::BeforeSend, None);
        assert_eq!(e.retry_class(), RetryClass::Retryable);
    }

    #[test]
    fn after_send_keeps_429_retryable() {
        let e = classify(status(429, None), Phase::AfterSend, None);
        assert_eq!(e.retry_class(), RetryClass::Retryable);
    }
}
