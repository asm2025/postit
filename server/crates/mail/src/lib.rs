//! `postit-mail`: templated email rendering and the outbox-backed mailer.
#![warn(missing_docs)]

mod error;
mod identity;
mod loaders;
mod mailer;
mod outbox;
mod send;
mod templates;

pub use error::MailError;
pub use identity::{
    IdentityArgs, IdentityContext, IdentityMessage, IdentityTemplate, identity_locale,
    render_identity,
};
pub use loaders::{LoadOutcome, MailContextLoader, MailLoaders};
pub use mailer::{Mailer, MemoryMailer, RenderedMessage, SmtpMailer};
pub use outbox::{MailKind, MailOutbox, MailParams, SendEmail};
pub use send::{SendEmailDeps, SendEmailHandler, console, register};
pub use templates::{MailContent, PendingUser, Rendered, render};
