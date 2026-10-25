//! `postit-agent-mock`: an in-memory reference agent with scriptable results, and the template
//! for writing a new agent.
//!
//! Development and CI builds register it (`postit-server` feature `mock-agent`, never in
//! production), so the publishing engine, the REST surface, and the clients have a platform to
//! run against before any real agent exists. Its state is per process and lost on restart.
//!
//! # Three agents from one crate
//!
//! - [`MockAgent::broadcast`] (`mock`): posts to an audience, connected with pasted
//!   credentials.
//! - [`MockAgent::direct`] (`mock_direct`): sends to individual recipients, same credentials.
//! - [`MockAgent::oauth`] (`mock_oauth`): `mock` connected through an OAuth provider, for tests
//!   against the agent kit's `MockOAuthProvider`. It is not registered in a running server.
//!
//! # Scripting without code
//!
//! - **Token** (trimmed, ASCII-lowercased): `invalid` fails verification on the `token` field,
//!   `rate_limited` is rate limited for 30 seconds, `expired` connects but later calls need a
//!   reconnect, and anything else works.
//! - **`account_name`**: comma-separated names, each one account `mock-account-{name}`.
//! - **Post option `outcome`**: `published` (default), `processing`, `retryable_failure`,
//!   `permanent_failure`, `outcome_unknown_landed`, `outcome_unknown_lost`, `needs_reconnect`,
//!   `rate_limited`, or `quota_exhausted`. **`comments`** seeds that many comments on the post.
//!
//! In Rust tests, [`MockAgent::script`] returns a [`MockScript`] whose overrides win over the
//! token and the options, and which counts calls.
//!
//! # Writing a new agent from this template
//!
//! A new platform is one crate under `backend/crates/agents/<platform>/` depending on
//! `postit-core`, `postit-http`, and `postit-agent-kit` only (never `postit-oauth` or
//! `postit-media`). Organize it the way this crate is:
//!
//! 1. **Descriptor** ([`descriptor`]): the platform ID, formats (`postit_agent_kit::text_format`
//!    and `media_format`), capabilities, native schedule window, quota buckets, and the
//!    credentials and options schemas (`postit_agent_kit::schema`). Every label is a
//!    `postit_agent_kit::message` keyed `<platform>.…` with English text. Call
//!    `AgentDescriptor::validate` in a test.
//! 2. **Agent** (here [`MockAgent`]): built once at startup. It holds the descriptor and
//!    anything its clients share (a real agent's `postit_http::RateLimiters`, behind an `Arc`),
//!    implements `verify_credentials` for a credentials platform, and creates clients.
//! 3. **Client** ([`client`]): one per grant, holding the `AgentContext`. Reach the network
//!    through `postit_agent_kit::standard` or `public`, map failures with
//!    `postit_agent_kit::classify` (a failure after the request was sent becomes
//!    `OutcomeUnknown`), check `PublishProgress::cancel_requested` before the first mutating
//!    call, pass the idempotency key to the platform when it deduplicates, and make dry runs
//!    call nothing. Long uploads go through `postit_agent_kit::upload_chunks` and processing
//!    waits through `poll_until`.
//! 4. **One module per capability** ([`capabilities`]): `Commenter`, `InboxReader`,
//!    `MetricsReader`, `NativeScheduler`, `DirectSender`, each exposed through the client's
//!    accessor of the same name. Declared capabilities must equal the exposed ones.
//! 5. **Faults for conformance**: implement `postit_agent_kit::testkit::conformance::Faults`
//!    over your wiremock server (the mock implements it on [`MockScript`], behind its `testkit`
//!    feature) and pass `run_conformance` in a test.
//! 6. **Registration**: one optional dependency and one feature named after the platform in
//!    `postit-server`, and one line in `compose::agent_registry()`:
//!
//!    ```ignore
//!    #[cfg(feature = "mock-agent")]
//!    registry.register(Arc::new(postit_agent_mock::MockAgent::broadcast()))?;
//!    ```
//!
//! Secrets (pasted tokens, OAuth tokens, signed URLs) never reach a log, an error, an outcome,
//! or a receipt; read them through `secrecy` and keep them inside the call that uses them.
#![warn(missing_docs)]

pub mod capabilities;
pub mod client;
pub mod credentials;
pub mod descriptor;
pub mod options;
pub mod platform;
pub mod script;

use async_trait::async_trait;
use postit_core::{
    Agent, AgentClient, AgentContext, AgentDescriptor, AgentError, Capability, ConnectMethod,
    CredentialCheckContext, CredentialFields, Discovered, PlatformId,
};
use url::Url;

pub use client::MockClient;
pub use options::Outcome;
pub use platform::{MockComment, MockPost, MockSend};
pub use script::{Decide, InfoScript, MockScript, VerifyScript};

use platform::Call;

/// The mock platform's agent. Clients it creates share one in-memory platform.
pub struct MockAgent {
    descriptor: AgentDescriptor,
    script: MockScript,
}

impl MockAgent {
    fn new(descriptor: AgentDescriptor) -> Self {
        Self {
            descriptor,
            script: MockScript::default(),
        }
    }

    /// `mock`: a broadcast platform connected with pasted credentials.
    #[must_use]
    pub fn broadcast() -> Self {
        Self::new(descriptor::broadcast())
    }

    /// `mock_direct`: sends to individual recipients, connected with pasted credentials.
    #[must_use]
    pub fn direct() -> Self {
        Self::new(descriptor::direct())
    }

    /// `mock_oauth`: the broadcast mock connected through the OAuth provider rooted at
    /// `provider` (its `authorize`, `token`, and `revoke` endpoints). It does not verify pasted
    /// credentials.
    #[must_use]
    pub fn oauth(provider: &Url) -> Self {
        Self::new(descriptor::oauth(provider))
    }

    /// The same agent under another platform ID, for tests that register several platforms.
    /// Message keys keep their original prefix, so such an agent is not run through
    /// conformance.
    #[must_use]
    pub fn with_platform(mut self, id: PlatformId) -> Self {
        self.descriptor.platform = id;
        self
    }

    /// A handle to this agent's platform state and overrides.
    #[must_use]
    pub fn script(&self) -> MockScript {
        self.script.clone()
    }
}

#[async_trait]
impl Agent for MockAgent {
    fn descriptor(&self) -> &AgentDescriptor {
        &self.descriptor
    }

    fn create_client(&self, ctx: AgentContext) -> Result<Box<dyn AgentClient>, AgentError> {
        Ok(Box::new(MockClient {
            platform_id: self.descriptor.platform.clone(),
            options: self.descriptor.options_schema.clone(),
            script: self.script.clone(),
            ctx,
            capabilities: self.descriptor.capabilities.clone(),
        }))
    }

    async fn verify_credentials(
        &self,
        _ctx: &CredentialCheckContext,
        fields: &CredentialFields,
    ) -> Result<Discovered, AgentError> {
        if matches!(self.descriptor.connect, ConnectMethod::OAuth(_)) {
            return Err(AgentError::Unsupported {
                capability: Capability::DirectSender,
            });
        }
        self.script.platform().call(Call::Read).await;
        match self.script.verify() {
            Some(script) => credentials::scripted(script, fields).await,
            None => credentials::discover(fields),
        }
    }
}
