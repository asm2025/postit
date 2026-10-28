//! `postit-oauth`: connecting accounts to platforms, and listing, checking, and disconnecting
//! them. It knows no platform: everything comes from the agent contract in `postit-core`.
#![warn(missing_docs)]

pub mod catalog;
mod connect;
mod disconnect;
// The token client is crate-private; the testkit build exposes it to the engine tests.
#[cfg(feature = "testkit")]
pub mod engine;
#[cfg(not(feature = "testkit"))]
mod engine;
mod error;
mod flow;
mod import;
mod manage;
mod return_to;
mod service;
mod store;
#[cfg(feature = "testkit")]
pub mod testkit;

pub use catalog::{Availability, PlatformCatalog, PlatformConfigs, ResolvedPlatform};
pub use connect::{AccountChange, Confirmed, ConnectId, CredentialCheck};
pub use disconnect::DisconnectOutcome;
pub use error::AccountError;
pub use flow::{CallbackCode, CallbackParams, OAuthStart, PendingView};
pub use import::{ImportReport, ImportRow, ImportStatus};
pub use manage::{AccountView, ConnectKind};
pub use return_to::ReturnToAllowlist;
pub use service::{AccountService, AccountServiceDeps, CALLBACK_DEADLINE};
pub use store::PgCredentialStore;
