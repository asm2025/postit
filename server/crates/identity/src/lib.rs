pub mod discovery;
mod error;
#[cfg(feature = "testkit")]
pub mod testkit;
pub mod verifier;

pub use error::{IdentityError, VerifyError};
