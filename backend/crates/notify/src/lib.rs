//! Notifications: who a notice reaches, and the rows that record it.
//!
//! [`Notifier::record`] runs in the caller's transaction, so a notice exists exactly when the
//! change it reports does. This crate depends on `core`, `data`, `jobs`, and `mail` only;
//! `identity`, `oauth`, and `publishing` call it, never the reverse.

mod email;
mod error;
mod record;

pub use email::{NotificationAlertLoader, register_loaders};
pub use error::NotifyError;
#[cfg(feature = "testkit")]
pub use record::notifications_for;
pub use record::{ALERT_DELAY, Notifier, NotifierDeps, NotifyEvent, Recipients};
