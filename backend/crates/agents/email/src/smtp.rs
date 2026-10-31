//! The SMTP transport over `lettre`'s low-level connection.
//!
//! The host is resolved once per session through the [`Resolve`] seam, which applies the
//! outbound address policy, and each checked address is tried in turn as a `SocketAddr`, so
//! `lettre` never looks a name up itself (a second lookup would reopen DNS rebinding). The TLS
//! name is the original host. `starttls` without STARTTLS on the server is an error, never a
//! plain fallback, and `tls = none` is refused unless private hosts are allowed.
//!
//! A session never calls `AsyncSmtpConnection::send`, which aborts the session on any refusal
//! and hides the stage. It issues `MAIL`, `RCPT`, `DATA`, and the message itself, tags a
//! refusal with its stage, and sends `RSET` after one. Every step runs under the command
//! timeout, because `lettre`'s own timeout covers the TCP connect only.

use std::fmt::Display;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::transport::smtp::client::{AsyncSmtpConnection, TlsParameters};
use lettre::transport::smtp::commands::{Data, Mail, Rcpt, Rset};
use lettre::transport::smtp::extension::{ClientId, Extension, MailBodyParameter, MailParameter};
use lettre::transport::smtp::response::Response;
use postit_http::{AddressPolicy, HttpError};
use secrecy::{ExposeSecret, SecretString};
use tokio::time::timeout;

use crate::classify::parse_enhanced;
use crate::transport::{
    EmailSession, EmailTransport, OutgoingMail, SendFailure, SendRefusal, SentMail, SmtpStage,
    TransportError,
};

/// Name resolution under the address policy, behind a seam so tests can stub the answers.
#[async_trait]
pub trait Resolve: Send + Sync {
    /// The addresses `host` resolves to that `policy` allows, with `port`.
    ///
    /// # Errors
    /// [`HttpError::AddressRejected`] when the policy refuses every address, another
    /// [`HttpError`] when the lookup fails.
    async fn resolve(
        &self,
        host: &str,
        port: u16,
        policy: AddressPolicy,
    ) -> Result<Vec<SocketAddr>, HttpError>;
}

/// The system resolver, through [`postit_http::resolve_checked`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemResolve;

#[async_trait]
impl Resolve for SystemResolve {
    async fn resolve(
        &self,
        host: &str,
        port: u16,
        policy: AddressPolicy,
    ) -> Result<Vec<SocketAddr>, HttpError> {
        postit_http::resolve_checked(host, port, policy).await
    }
}

/// How the connection is encrypted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsMode {
    /// No encryption; refused unless private hosts are allowed.
    None,
    /// A plain connection upgraded with STARTTLS.
    Starttls,
    /// Implicit TLS from the first byte.
    Tls,
}

/// Where and how to reach one SMTP server.
#[derive(Clone)]
pub struct SmtpServer {
    /// The host as the user typed it; also the TLS name.
    pub host: String,
    /// The port.
    pub port: u16,
    /// The encryption.
    pub tls: TlsMode,
    /// Username and password, when the server needs a login.
    pub login: Option<(String, SecretString)>,
}

impl std::fmt::Debug for SmtpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmtpServer")
            .field("port", &self.port)
            .field("tls", &self.tls)
            .field("login", &self.login.as_ref().map(|_| "[redacted]"))
            .finish_non_exhaustive()
    }
}

/// An [`EmailTransport`] for one SMTP server.
pub struct SmtpTransport {
    server: SmtpServer,
    policy: AddressPolicy,
    command_timeout: Duration,
    resolver: Arc<dyn Resolve>,
    hello: ClientId,
}

/// The `EHLO` name for `name`: an address literal for an IP address, a domain otherwise.
fn client_id(name: &str) -> ClientId {
    match name.trim_matches(['[', ']']).parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ClientId::Ipv4(ip),
        Ok(std::net::IpAddr::V6(ip)) => ClientId::Ipv6(ip),
        Err(_) => ClientId::Domain(name.to_owned()),
    }
}

impl SmtpTransport {
    /// A transport for `server` under `policy`, with every command limited to
    /// `command_timeout`, resolving through `resolver`. It greets with the machine's host
    /// name until [`Self::with_hello_name`] sets one.
    #[must_use]
    pub fn new(
        server: SmtpServer,
        policy: AddressPolicy,
        command_timeout: Duration,
        resolver: Arc<dyn Resolve>,
    ) -> Self {
        Self {
            server,
            policy,
            command_timeout,
            resolver,
            hello: ClientId::default(),
        }
    }

    /// The same transport greeting with `name` in `EHLO`, such as the deployment's public
    /// host.
    #[must_use]
    pub fn with_hello_name(mut self, name: &str) -> Self {
        self.hello = client_id(name);
        self
    }

    fn tls_parameters(&self) -> Result<TlsParameters, TransportError> {
        TlsParameters::new(self.server.host.clone()).map_err(|_| TransportError::Tls)
    }

    /// Connects to one checked address, greets, and upgrades to TLS as configured.
    async fn connect(
        &self,
        addr: SocketAddr,
        hello: &ClientId,
    ) -> Result<AsyncSmtpConnection, TransportError> {
        let implicit = match self.server.tls {
            TlsMode::Tls => Some(self.tls_parameters()?),
            TlsMode::None | TlsMode::Starttls => None,
        };
        let connecting = AsyncSmtpConnection::connect_tokio1(
            addr,
            Some(self.command_timeout),
            hello,
            implicit,
            None,
        );
        match timeout(self.command_timeout, connecting).await {
            Err(_) => Err(TransportError::Timeout),
            Ok(Err(err)) if err.is_tls() => Err(TransportError::Tls),
            Ok(Err(err)) if err.is_timeout() => Err(TransportError::Timeout),
            Ok(Err(_)) => Err(TransportError::Connect),
            Ok(Ok(conn)) => Ok(conn),
        }
    }
}

#[async_trait]
impl EmailTransport for SmtpTransport {
    async fn open(&self) -> Result<Box<dyn EmailSession>, TransportError> {
        if self.server.tls == TlsMode::None && !self.policy.allow_private {
            return Err(TransportError::PlainRefused);
        }
        let resolving = self
            .resolver
            .resolve(&self.server.host, self.server.port, self.policy);
        let addrs = match timeout(self.command_timeout, resolving).await {
            Err(_) => return Err(TransportError::Timeout),
            Ok(Err(HttpError::AddressRejected { .. })) => {
                return Err(TransportError::AddressRejected);
            }
            Ok(Err(_)) => return Err(TransportError::Lookup),
            Ok(Ok(addrs)) => addrs,
        };
        let hello = &self.hello;
        let mut last = TransportError::Connect;
        let mut connected = None;
        for addr in addrs {
            match self.connect(addr, hello).await {
                Ok(conn) => {
                    connected = Some(conn);
                    break;
                }
                Err(err) => last = err,
            }
        }
        let Some(conn) = connected else {
            return Err(last);
        };
        let mut session = SmtpSession {
            conn,
            command_timeout: self.command_timeout,
            broken: false,
        };
        if self.server.tls == TlsMode::Starttls {
            if !session
                .conn
                .server_info()
                .supports_feature(Extension::StartTls)
            {
                session.close().await;
                return Err(TransportError::StarttlsMissing);
            }
            let params = self.tls_parameters()?;
            match timeout(self.command_timeout, session.conn.starttls(params, hello)).await {
                Err(_) => return Err(TransportError::Timeout),
                Ok(Err(_)) => return Err(TransportError::Tls),
                Ok(Ok(())) => {}
            }
        }
        if let Some((user, password)) = &self.server.login {
            let credentials = Credentials::new(user.clone(), password.expose_secret().to_owned());
            let login = session
                .conn
                .auth(&[Mechanism::Plain, Mechanism::Login], &credentials);
            match timeout(self.command_timeout, login).await {
                Err(_) => return Err(TransportError::Timeout),
                // A temporary refusal says nothing about the password.
                Ok(Err(err)) if err.is_transient() => {
                    session.close().await;
                    return Err(TransportError::Protocol);
                }
                Ok(Err(err)) if err.status().is_some() || err.is_client() => {
                    session.close().await;
                    return Err(TransportError::Authentication);
                }
                Ok(Err(_)) => return Err(TransportError::Protocol),
                Ok(Ok(_)) => {}
            }
        }
        Ok(Box::new(session))
    }
}

/// One open SMTP connection.
struct SmtpSession {
    conn: AsyncSmtpConnection,
    command_timeout: Duration,
    broken: bool,
}

type Reply = Result<Result<Response, lettre::transport::smtp::Error>, tokio::time::error::Elapsed>;

impl SmtpSession {
    async fn command<C: Display + Send>(&mut self, command: C) -> Reply {
        timeout(self.command_timeout, self.conn.command(command)).await
    }

    /// Turns one reply into the step's result. A refusal is followed by `RSET`, so the
    /// session can carry the next mail. A `421` means the server is closing, so it ends the
    /// session like a broken connection. `body_sent` says whether the message was written.
    async fn judge(
        &mut self,
        stage: SmtpStage,
        reply: Reply,
        body_sent: bool,
    ) -> Result<(), SendFailure> {
        let lost = |error| SendFailure::Connection { error, body_sent };
        match reply {
            Err(_) => {
                self.broken = true;
                Err(lost(TransportError::Timeout))
            }
            Ok(Ok(_)) => Ok(()),
            Ok(Err(err)) => {
                let Some(code) = err.status() else {
                    self.broken = true;
                    return Err(lost(TransportError::Protocol));
                };
                // The reply text is read for its enhanced code and dropped.
                let enhanced = std::error::Error::source(&err)
                    .and_then(|text| parse_enhanced(&text.to_string()));
                let refusal = SendRefusal {
                    stage,
                    code: u16::from(code),
                    enhanced,
                };
                if refusal.code == 421 {
                    self.broken = true;
                    return Err(lost(TransportError::Closing));
                }
                self.reset().await;
                Err(SendFailure::Refused(refusal))
            }
        }
    }

    async fn reset(&mut self) {
        if !matches!(self.command(Rset).await, Ok(Ok(_))) {
            self.broken = true;
        }
    }
}

#[async_trait]
impl EmailSession for SmtpSession {
    async fn send(&mut self, mail: &OutgoingMail) -> Result<SentMail, SendFailure> {
        if self.broken {
            return Err(SendFailure::Connection {
                error: TransportError::Protocol,
                body_sent: false,
            });
        }
        // The parameters `AsyncSmtpConnection::send` would add.
        let mut params = Vec::new();
        let ascii = |address: &lettre::Address| AsRef::<str>::as_ref(address).is_ascii();
        if !(ascii(&mail.from) && ascii(&mail.to)) {
            if !self
                .conn
                .server_info()
                .supports_feature(Extension::SmtpUtfEight)
            {
                return Err(SendFailure::Unsupported("smtputf8_unsupported"));
            }
            params.push(MailParameter::SmtpUtfEight);
        }
        if !mail.message.is_ascii() {
            if !self
                .conn
                .server_info()
                .supports_feature(Extension::EightBitMime)
            {
                return Err(SendFailure::Unsupported("8bitmime_unsupported"));
            }
            params.push(MailParameter::Body(MailBodyParameter::EightBitMime));
        }
        let reply = self
            .command(Mail::new(Some(mail.from.clone()), params))
            .await;
        self.judge(SmtpStage::Mail, reply, false).await?;
        let reply = self.command(Rcpt::new(mail.to.clone(), Vec::new())).await;
        self.judge(SmtpStage::Rcpt, reply, false).await?;
        let reply = self.command(Data).await;
        self.judge(SmtpStage::Data, reply, false).await?;
        // From here a failure may come after the server took the whole message.
        let reply = timeout(self.command_timeout, self.conn.message(&mail.message)).await;
        self.judge(SmtpStage::Data, reply, true).await?;
        Ok(SentMail)
    }

    fn is_broken(&self) -> bool {
        self.broken
    }

    async fn close(&mut self) {
        if !self.broken {
            let _ = timeout(self.command_timeout, self.conn.quit()).await;
            self.broken = true;
        }
    }
}
