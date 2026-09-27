use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use lettre::message::{Mailbox, MultiPart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};
use postit_config::{MailSettings, SmtpTls};

use crate::error::MailError;

#[derive(Clone)]
pub struct RenderedMessage {
    pub to: String,
    pub subject: String,
    pub text: String,
    pub html: String,
}

impl fmt::Debug for RenderedMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let domain = self.to.rsplit_once('@').map_or("?", |(_, d)| d);
        f.debug_struct("RenderedMessage")
            .field("to_domain", &domain)
            .field("subject_len", &self.subject.len())
            .finish_non_exhaustive()
    }
}

#[async_trait]
pub trait Mailer: Send + Sync {
    async fn send(&self, message: RenderedMessage) -> Result<(), MailError>;
}

/// Records messages instead of sending them. Used by every test that sends mail.
#[derive(Clone, Default)]
pub struct MemoryMailer {
    sent: Arc<Mutex<Vec<RenderedMessage>>>,
}

impl MemoryMailer {
    #[must_use]
    pub fn sent(&self) -> Vec<RenderedMessage> {
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

#[async_trait]
impl Mailer for MemoryMailer {
    async fn send(&self, message: RenderedMessage) -> Result<(), MailError> {
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(message);
        Ok(())
    }
}

pub struct SmtpMailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
    host: String,
    port: u16,
}

impl fmt::Debug for SmtpMailer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SmtpMailer")
            .field("host", &self.host)
            .field("port", &self.port)
            .finish_non_exhaustive()
    }
}

impl SmtpMailer {
    /// # Errors
    ///
    /// Returns [`MailError::Config`] if `from_address` isn't a valid mailbox or the relay
    /// can't be configured.
    pub fn new(settings: &MailSettings) -> Result<Self, MailError> {
        let smtp = &settings.smtp;
        let builder = match smtp.tls {
            SmtpTls::None => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&smtp.host),
            SmtpTls::Starttls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&smtp.host)
                .map_err(|e| MailError::Config(e.to_string()))?,
            SmtpTls::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&smtp.host)
                .map_err(|e| MailError::Config(e.to_string()))?,
        };
        let mut builder = builder.port(smtp.port);
        if let (Some(username), Some(password)) = (&smtp.username, &smtp.password) {
            builder = builder.credentials(Credentials::new(
                username.clone(),
                password.expose().to_string(),
            ));
        }
        let from: Mailbox = settings
            .from_address
            .parse()
            .map_err(|_| MailError::Config("mail.from_address is not a valid mailbox".into()))?;
        Ok(Self {
            transport: builder.build(),
            from,
            host: smtp.host.clone(),
            port: smtp.port,
        })
    }
}

#[async_trait]
impl Mailer for SmtpMailer {
    async fn send(&self, message: RenderedMessage) -> Result<(), MailError> {
        let to: Mailbox = message
            .to
            .parse()
            .map_err(|_| MailError::Permanent("recipient address is not a valid mailbox".into()))?;
        let email = Message::builder()
            .from(self.from.clone())
            .to(to)
            .subject(message.subject)
            .multipart(MultiPart::alternative_plain_html(
                message.text,
                message.html,
            ))
            .map_err(|e| MailError::Permanent(format!("message build: {e}")))?;
        self.transport.send(email).await.map(|_| ()).map_err(|err| {
            // Only the class and status code: a server's response text can echo the
            // recipient address, and job errors must not carry it.
            let code = err
                .status()
                .map_or_else(|| "none".to_string(), |c| c.to_string());
            if err.is_permanent() {
                MailError::Permanent(format!("smtp rejected the message (status {code})"))
            } else {
                MailError::Transient(format!("smtp unavailable or deferred (status {code})"))
            }
        })
    }
}
