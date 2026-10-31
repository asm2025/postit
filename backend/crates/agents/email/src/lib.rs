//! `postit-agent-email`: the email agent (platform ID `email`). It renders an
//! [`EmailDocument`] to responsive HTML and plain text, and sends campaigns to consented
//! contacts over SMTP.
//!
//! # Rendering
//!
//! [`Skeleton::compile`] turns a document into HTML once per batch through `mrml`, with
//! sentinel placeholders where merge values and signed links go. [`Skeleton::for_recipient`]
//! fills them for one recipient in a single pass, so `mrml` never runs per recipient and a
//! value is never read again once written. The footer, with the sender's postal address and
//! the unsubscribe link, is added by the renderer and no block can remove it.
//!
//! # Sending
//!
//! [`EmailClient`] is a direct agent: `send_batch` opens one [`EmailSession`] per batch
//! through an [`EmailTransport`] ([`SmtpTransport`] today) and sends each recipient's mail in
//! turn. Refusals are classified by stage ([`classify`]), reasons are fixed codes such as
//! `smtp_5_1_1`, and no server text, address, or password reaches a result or an error. The
//! module docs of [`client`] list the batch rules.
//!
//! # Sender-domain check
//!
//! The client also offers [`SenderDomainCheck`]: SPF, DKIM, and DMARC for the from
//! address's domain, read through a [`TxtLookup`] ([`HickoryLookup`] over the system
//! resolver). The module docs of [`dns`] list the rules.
//!
//! Of the postit crates, the agent may depend on `postit-core`, `postit-http`, and
//! `postit-agent-kit` only, never on the database or the contacts service.
//!
//! [`EmailDocument`]: postit_core::EmailDocument
//! [`SenderDomainCheck`]: postit_core::SenderDomainCheck

mod classify;
pub mod client;
mod credentials;
pub mod descriptor;
pub mod dns;
mod render;
mod smtp;
#[cfg(feature = "testkit")]
pub mod testkit;
mod transport;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use postit_core::{
    Agent, AgentClient, AgentContext, AgentDescriptor, AgentError, CredentialCheckContext,
    CredentialFields, Discovered, Message, PlatformId,
};
use postit_http::AddressPolicy;

pub use classify::{Classified, classify, parse_enhanced};
pub use client::EmailClient;
pub use descriptor::EMAIL;
pub use dns::{HickoryLookup, LookupError, TxtLookup};
pub use render::{Escape, MergeError, MergeTemplate, RenderError, RenderedMail, Sender, Skeleton};
pub use smtp::{Resolve, SmtpServer, SmtpTransport, SystemResolve, TlsMode};
pub use transport::{
    EmailSession, EmailTransport, OutgoingMail, SendFailure, SendRefusal, SentMail, SmtpStage,
    TransportError,
};

use credentials::Account;

/// The deployment settings the agent reads.
#[derive(Debug, Clone)]
pub struct EmailAgentConfig {
    /// `email.allow_private_hosts`: SMTP hosts on private or loopback addresses are allowed,
    /// and so is `tls = none`.
    pub allow_private: bool,
    /// `email.image_url_ttl`: how long the image URLs in a sent mail stay valid.
    pub image_url_ttl: Duration,
    /// `email.command_timeout`: the limit on the connect and on each SMTP command.
    pub command_timeout: Duration,
    /// The name sent in `EHLO`, such as the host of `server.public_url`. `None` sends the
    /// machine's host name.
    pub hello_name: Option<String>,
}

/// What the agent and its clients share.
pub(crate) struct Shared {
    pub(crate) descriptor: AgentDescriptor,
    pub(crate) config: EmailAgentConfig,
    resolver: Arc<dyn Resolve>,
    pub(crate) txt: Arc<dyn TxtLookup>,
}

impl Shared {
    pub(crate) fn transport(&self, account: &Account) -> SmtpTransport {
        let transport = SmtpTransport::new(
            account.smtp.clone(),
            AddressPolicy {
                allow_private: self.config.allow_private,
            },
            self.config.command_timeout,
            Arc::clone(&self.resolver),
        );
        match &self.config.hello_name {
            Some(name) => transport.with_hello_name(name),
            None => transport,
        }
    }
}

/// A message keyed `email.<key>`.
pub(crate) fn message(key: &str, en: &str) -> Message {
    let platform = PlatformId::new(EMAIL)
        .unwrap_or_else(|e| unreachable!("the email platform ID is valid: {e}"));
    postit_agent_kit::message(&platform, key, en)
}

/// The email agent.
pub struct EmailAgent {
    shared: Arc<Shared>,
}

impl EmailAgent {
    /// The agent with `config`, resolving SMTP hosts and looking up `TXT` records through
    /// the system resolver.
    #[must_use]
    pub fn new(config: EmailAgentConfig) -> Self {
        Self {
            shared: Arc::new(Shared {
                descriptor: descriptor::descriptor(),
                config,
                resolver: Arc::new(SystemResolve),
                txt: Arc::new(HickoryLookup::new()),
            }),
        }
    }

    /// The same agent resolving SMTP hosts through `resolver`, for tests that stub DNS.
    #[must_use]
    pub fn with_resolver(self, resolver: Arc<dyn Resolve>) -> Self {
        Self {
            shared: Arc::new(Shared {
                descriptor: self.shared.descriptor.clone(),
                config: self.shared.config.clone(),
                resolver,
                txt: Arc::clone(&self.shared.txt),
            }),
        }
    }

    /// The same agent reading `TXT` records through `txt`, for tests of the sender-domain
    /// check.
    #[must_use]
    pub fn with_txt_lookup(self, txt: Arc<dyn TxtLookup>) -> Self {
        Self {
            shared: Arc::new(Shared {
                descriptor: self.shared.descriptor.clone(),
                config: self.shared.config.clone(),
                resolver: Arc::clone(&self.shared.resolver),
                txt,
            }),
        }
    }
}

/// The error `verify_credentials` reports for a session that could not be opened: a refused
/// login names the password field.
fn verify_error(err: TransportError) -> AgentError {
    match err {
        TransportError::Authentication => AgentError::InvalidCredentials {
            field: Some("password".to_owned()),
        },
        other => client::open_error(other),
    }
}

#[async_trait]
impl Agent for EmailAgent {
    fn descriptor(&self) -> &AgentDescriptor {
        &self.shared.descriptor
    }

    fn create_client(&self, ctx: AgentContext) -> Result<Box<dyn AgentClient>, AgentError> {
        Ok(Box::new(EmailClient {
            shared: Arc::clone(&self.shared),
            ctx,
        }))
    }

    /// Opens a session and logs in, then closes it. Nothing is sent.
    async fn verify_credentials(
        &self,
        _ctx: &CredentialCheckContext,
        fields: &CredentialFields,
    ) -> Result<Discovered, AgentError> {
        let account = credentials::parse(fields)?;
        let mut session = self
            .shared
            .transport(&account)
            .open()
            .await
            .map_err(verify_error)?;
        session.close().await;
        Ok(client::discovered(&account))
    }
}
