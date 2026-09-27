mod error;
mod loaders;
mod mailer;
mod outbox;
mod send;
mod templates;

pub use error::MailError;
pub use loaders::{LoadOutcome, MailContextLoader, MailLoaders};
pub use mailer::{Mailer, MemoryMailer, RenderedMessage, SmtpMailer};
pub use outbox::{MailKind, MailOutbox, MailParams, SendEmail};
pub use send::{SendEmailDeps, SendEmailHandler, register};
pub use templates::{MailContent, PendingUser, Rendered, render};
