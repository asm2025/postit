//! Inbound events: webhooks and callbacks the platform sends to us.

use std::collections::BTreeMap;

use bytes::Bytes;

/// A request the platform sent to the agent's event endpoint, exactly as received. The agent
/// verifies the signature against `raw_body`, so nothing here is parsed or re-encoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundEvent {
    /// The agent's name for the endpoint that received it.
    pub kind: String,
    /// Parameters captured from the URL path.
    pub path_params: BTreeMap<String, String>,
    /// Header names and raw values, in arrival order.
    pub headers: Vec<(String, Vec<u8>)>,
    /// The unmodified body.
    pub raw_body: Bytes,
}

impl InboundEvent {
    /// The first header named `name`, compared without regard to case.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&[u8]> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_slice())
    }
}

/// What to send back to the platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventResponse {
    /// HTTP status.
    pub status: u16,
    /// `Content-Type`, when there is a body.
    pub content_type: Option<String>,
    /// The body.
    pub body: Bytes,
}

impl EventResponse {
    /// A response with a status and no body.
    #[must_use]
    pub const fn empty(status: u16) -> Self {
        Self {
            status,
            content_type: None,
            body: Bytes::new(),
        }
    }
}

/// What handling an event produced: the reply, plus grants the platform says are revoked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventOutcome {
    /// What to send back to the platform.
    pub response: EventResponse,
    /// Remote user IDs whose grants the platform deauthorized.
    pub deauthorized: Vec<String>,
}

impl EventOutcome {
    /// An outcome that only replies.
    #[must_use]
    pub const fn respond(response: EventResponse) -> Self {
        Self {
            response,
            deauthorized: Vec::new(),
        }
    }
}
