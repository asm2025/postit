mod error;
mod mailer;
mod templates;

pub use error::MailError;
pub use mailer::{Mailer, MemoryMailer, RenderedMessage, SmtpMailer};
pub use templates::{MailContent, PendingUser, Rendered, render};
