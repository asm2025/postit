pub mod admin;
pub mod cache;
pub mod claims;
pub mod deletion;
pub mod discovery;
mod error;
pub mod mail;
pub mod principal;
#[cfg(feature = "testkit")]
pub mod testkit;
pub mod verifier;

pub use error::{IdentityError, VerifyError};
