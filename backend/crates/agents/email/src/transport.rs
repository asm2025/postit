//! How mail leaves postit: an [`EmailTransport`] opens an [`EmailSession`], and the session
//! sends one recipient's mail at a time. SMTP ([`crate::SmtpTransport`]) is the one
//! implementation; C6 adds HTTP mail services behind the same traits.
//!
//! Nothing here holds server text. A refusal keeps its stage and codes only, because replies
//! often echo the address, and an error names what failed, never the host's reply.

use async_trait::async_trait;
use lettre::Address;

/// The SMTP command a refusal answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtpStage {
    /// `MAIL FROM`: the sender.
    Mail,
    /// `RCPT TO`: the recipient.
    Rcpt,
    /// `DATA`, or the message after it.
    Data,
}

/// A negative reply to one step of a send. It holds no server text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendRefusal {
    /// Which command was refused.
    pub stage: SmtpStage,
    /// The basic reply code, such as `550`.
    pub code: u16,
    /// The enhanced status code (RFC 3463), such as `(5, 1, 1)`, when the reply had one.
    pub enhanced: Option<(u8, u16, u16)>,
}

impl SendRefusal {
    /// The fixed code stored for this refusal: `smtp_5_1_1` from the enhanced code, otherwise
    /// `smtp_550` from the basic one.
    #[must_use]
    pub fn reason(&self) -> String {
        match self.enhanced {
            Some((class, subject, detail)) => format!("smtp_{class}_{subject}_{detail}"),
            None => format!("smtp_{}", self.code),
        }
    }

    /// Whether the refusal is a permanent `5.7.x` policy refusal, which often speaks about
    /// the sender's reputation or setup rather than the recipient.
    #[must_use]
    pub fn is_sender_policy(&self) -> bool {
        matches!(self.enhanced, Some((5, 7, _)))
    }
}

/// Why a transport could not open a session or keep one. It names what failed, never the
/// server's reply or an address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    /// The address policy refused every address the host resolved to.
    #[error("the mail server's address is not allowed")]
    AddressRejected,
    /// The host name could not be resolved.
    #[error("the mail server's name could not be resolved")]
    Lookup,
    /// No address of the host accepted a connection.
    #[error("the mail server could not be reached")]
    Connect,
    /// The server did not answer within the command timeout.
    #[error("the mail server did not answer in time")]
    Timeout,
    /// The TLS handshake failed.
    #[error("the TLS handshake with the mail server failed")]
    Tls,
    /// The connection asks for STARTTLS and the server does not offer it.
    #[error("the mail server does not offer STARTTLS")]
    StarttlsMissing,
    /// An unencrypted connection to a host on the public network was refused.
    #[error("an unencrypted connection to the mail server is not allowed")]
    PlainRefused,
    /// The server refused the username or password, or offers no usable mechanism.
    #[error("the mail server refused the login")]
    Authentication,
    /// The connection broke, or the server answered something unreadable.
    #[error("the connection to the mail server broke")]
    Protocol,
    /// The server answered `421`: it is closing the connection.
    #[error("the mail server is closing the connection")]
    Closing,
}

/// Why one recipient's mail was not sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendFailure {
    /// The server refused a step.
    Refused(SendRefusal),
    /// The connection broke, timed out, or the server is closing it (`421`); the session
    /// cannot be used again.
    Connection {
        /// What failed.
        error: TransportError,
        /// Whether any of the message after `DATA` was written. When it was not, the server
        /// cannot have accepted the mail, so sending it again on a new session is safe.
        body_sent: bool,
    },
    /// The mail needs an extension the server does not offer. Carries a fixed reason code.
    Unsupported(&'static str),
}

/// One recipient's mail, ready to send.
#[derive(Clone)]
pub struct OutgoingMail {
    /// The envelope sender.
    pub from: Address,
    /// The envelope recipient.
    pub to: Address,
    /// The formatted message, headers and body.
    pub message: Vec<u8>,
}

/// A mail the server accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SentMail;

/// Opens sessions to a mail service.
#[async_trait]
pub trait EmailTransport: Send + Sync {
    /// Connects and logs in.
    ///
    /// # Errors
    /// A [`TransportError`] saying what failed.
    async fn open(&self) -> Result<Box<dyn EmailSession>, TransportError>;
}

/// One open connection, sending one mail at a time. A refusal leaves it usable; a
/// [`SendFailure::Connection`] does not.
#[async_trait]
pub trait EmailSession: Send {
    /// Sends `mail`.
    ///
    /// # Errors
    /// A [`SendFailure`] saying which step failed.
    async fn send(&mut self, mail: &OutgoingMail) -> Result<SentMail, SendFailure>;

    /// Whether the session can no longer send, such as after a reset that failed.
    fn is_broken(&self) -> bool {
        false
    }

    /// Ends the session politely. Failures are ignored.
    async fn close(&mut self) {}
}
