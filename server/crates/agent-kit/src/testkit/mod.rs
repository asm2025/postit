//! Fakes, test contexts, and a mock OAuth provider for agent crates' tests. Gated behind the
//! `testkit` feature so none of it reaches a production build.
//!
//! - [`fakes`]: in-memory media, credential, and link implementations of the `postit-core`
//!   traits an agent context carries.
//! - [`context`]: [`TestContext`] builds the three agent contexts over those fakes and real
//!   `postit-http` clients.
//! - [`oauth_provider`]: [`MockOAuthProvider`] is a scriptable OAuth server on loopback.
//! - [`responders`]: wiremock response templates for the failure shapes agents classify.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::missing_panics_doc)]

pub mod context;
pub mod fakes;
pub mod oauth_provider;
pub mod responders;

pub use context::TestContext;
pub use fakes::{FakeLinkSigner, MemoryCredentialStore, MemoryMediaHost, MemoryMediaStore};
pub use oauth_provider::{MockOAuthProvider, OAuthFailure};
