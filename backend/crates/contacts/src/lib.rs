//! `postit-contacts`: the contact book behind email campaigns (plan 03 C1).
//!
//! [`ContactService`] owns the rules the repositories in `postit-data` do not: an address must
//! be well formed, every contact carries its consent record, a new contact respects the
//! owner's contacts cap, and deleting a list or removing a suppression leaves an audit event.
//! CSV imports are staged by the request and applied by the `contact_import` job on the
//! `contacts` queue; exports stream. The `email_event_apply` job records what happened to a sent
//! email and suppresses bounced and complained addresses. Contact data never reaches a log, an error, a job, or an audit event.
#![warn(missing_docs)]

mod error;
mod events;
mod export;
mod import;
pub mod jobs;
mod service;

pub use error::ContactError;
pub use import::{ColumnMap, ImportOptions};
pub use jobs::{ContactImportJob, EmailEventApply};
pub use service::{
    ContactPatch, ContactService, ContactsDeps, NewContactInput, SettableStatus, parse_address,
};
