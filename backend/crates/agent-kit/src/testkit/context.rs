//! A ready-made agent context over the fakes and real `postit-http` clients.

use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use postit_config::HttpSettings;
use postit_core::{
    AgentContext, CredentialCheckContext, Credentials, EventContext, HttpHandles, TestClock,
};
use postit_http::{AddressPolicy, agent_handles, build_client, build_public_client};
use serde_json::Value;

use super::fakes::{FakeLinkSigner, MemoryCredentialStore, MemoryMediaHost, MemoryMediaStore};

/// The request timeout of the test HTTP clients; `responders::dropped` delays past it.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

/// Everything an agent test needs, with the fakes exposed for assertions.
pub struct TestContext {
    /// A clock frozen at 2026-01-01T00:00:00Z.
    pub clock: Arc<TestClock>,
    /// Media bytes by asset ID.
    pub media_store: Arc<MemoryMediaStore>,
    /// Records the URLs issued.
    pub media_host: Arc<MemoryMediaHost>,
    /// The grant's credentials; empty until [`TestContext::with_credentials`].
    pub credentials: Arc<MemoryCredentialStore>,
    /// The standard client and a public client that allows loopback, so wiremock works.
    pub http: HttpHandles,
}

impl Default for TestContext {
    fn default() -> Self {
        Self::new()
    }
}

impl TestContext {
    /// A context with empty fakes. Requests time out after [`REQUEST_TIMEOUT`].
    ///
    /// # Panics
    ///
    /// When the HTTP clients cannot be built, which the fixed test settings do not allow.
    #[must_use]
    pub fn new() -> Self {
        let settings = HttpSettings {
            connect_timeout: REQUEST_TIMEOUT,
            request_timeout: REQUEST_TIMEOUT,
            user_agent: "postit-agent-kit-testkit/0".to_owned(),
            extra_ca_files: Vec::new(),
        };
        let standard = build_client(&settings).expect("test client builds");
        let public = build_public_client(
            &settings,
            AddressPolicy {
                allow_private: true,
            },
        )
        .expect("test public client builds");
        Self {
            clock: Arc::new(TestClock::new(
                Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
                    .single()
                    .expect("a valid instant"),
            )),
            media_store: Arc::new(MemoryMediaStore::new()),
            media_host: Arc::new(MemoryMediaHost::new()),
            credentials: Arc::new(MemoryCredentialStore::empty()),
            http: agent_handles(standard, public),
        }
    }

    /// Replaces the credential store with one holding `credentials`.
    #[must_use]
    pub fn with_credentials(mut self, credentials: Credentials) -> Self {
        self.credentials = Arc::new(MemoryCredentialStore::new(credentials));
        self
    }

    /// The context for publishing and other grant calls.
    #[must_use]
    pub fn agent_context(&self, config: Value) -> AgentContext {
        AgentContext {
            config,
            http: self.http.clone(),
            credentials: self.credentials.clone(),
            media_store: self.media_store.clone(),
            media_host: self.media_host.clone(),
            link_signer: Arc::new(FakeLinkSigner),
            clock: self.clock.clone(),
        }
    }

    /// The context for `verify_credentials`.
    #[must_use]
    pub fn check_context(&self, config: Value) -> CredentialCheckContext {
        CredentialCheckContext {
            config,
            http: self.http.clone(),
            clock: self.clock.clone(),
        }
    }

    /// The context for `handle_event`.
    #[must_use]
    pub fn event_context(&self, config: Value) -> EventContext {
        EventContext {
            config,
            http: self.http.clone(),
            clock: self.clock.clone(),
        }
    }
}
