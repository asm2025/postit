//! Downcasting the opaque HTTP handles an agent receives into concrete clients.

use postit_core::{AgentError, HttpHandles, Message, Violation};
use postit_http::{PublicClient, StandardClient};

fn wrong_handle() -> AgentError {
    AgentError::Validation(vec![Violation::agent_rule(Message::new_static(
        "agent_kit.wrong_http_handle",
        "The agent was given the wrong kind of HTTP client.",
    ))])
}

/// The client for the platform's own API hosts.
///
/// # Errors
///
/// A permanent [`AgentError::Validation`] when the default handle is not a standard client, which
/// is a wiring mistake in the composition root.
pub fn standard(handles: &HttpHandles) -> Result<&reqwest::Client, AgentError> {
    handles
        .default
        .as_any()
        .downcast_ref::<StandardClient>()
        .map(|client| &client.0)
        .ok_or_else(wrong_handle)
}

/// The client for hosts the user supplied; it enforces the outbound address policy.
///
/// # Errors
///
/// A permanent [`AgentError::Validation`] when the user-hosts handle is not a public client.
pub fn public(handles: &HttpHandles) -> Result<&PublicClient, AgentError> {
    handles
        .user_hosts
        .as_any()
        .downcast_ref::<PublicClient>()
        .ok_or_else(wrong_handle)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::time::Duration;

    use postit_config::HttpSettings;
    use postit_core::RetryClass;
    use postit_http::{AddressPolicy, agent_handles, build_client, build_public_client};

    use super::*;

    fn parts() -> (reqwest::Client, PublicClient) {
        let settings = HttpSettings {
            connect_timeout: Duration::from_secs(1),
            request_timeout: Duration::from_secs(1),
            user_agent: "agent-kit-test/0".to_owned(),
            extra_ca_files: Vec::new(),
        };
        (
            build_client(&settings).expect("standard client"),
            build_public_client(&settings, AddressPolicy::strict()).expect("public client"),
        )
    }

    #[test]
    fn downcasts_each_handle() {
        let (standard_client, public_client) = parts();
        let handles = agent_handles(standard_client, public_client);
        assert!(standard(&handles).is_ok());
        assert!(public(&handles).is_ok());
    }

    #[test]
    fn wrong_handle_is_permanent() {
        let (standard_client, public_client) = parts();
        let right = agent_handles(standard_client, public_client);
        let swapped = HttpHandles {
            default: right.user_hosts.clone(),
            user_hosts: right.default.clone(),
        };
        let err = standard(&swapped).unwrap_err();
        assert_eq!(err.retry_class(), RetryClass::Permanent);
        assert!(public(&swapped).is_err());
    }
}
