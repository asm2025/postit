//! `postit-oauth`: connecting accounts to platforms, and listing, checking, and disconnecting
//! them. It knows no platform: everything comes from the agent contract in `postit-core`.
#![warn(missing_docs)]

pub mod catalog;
mod connect;
mod disconnect;
mod error;
mod import;
mod manage;
mod service;
mod store;
#[cfg(feature = "testkit")]
pub mod testkit;

pub use catalog::{Availability, PlatformCatalog, PlatformConfigs, ResolvedPlatform};
pub use connect::{AccountChange, Confirmed, ConnectId, CredentialCheck};
pub use disconnect::DisconnectOutcome;
pub use error::AccountError;
pub use import::{ImportReport, ImportRow, ImportStatus};
pub use manage::{AccountView, ConnectKind};
pub use service::{AccountService, AccountServiceDeps};
pub use store::PgCredentialStore;
