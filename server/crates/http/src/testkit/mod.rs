//! Shared wiremock setup for crates that stub HTTP APIs in tests (plan 03 platform plugins,
//! `postit-oauth`, and this crate's own tests). Gated behind the `testkit` feature so
//! `wiremock` never reaches a non-test build.

use std::io;
use std::net::SocketAddr;

use wiremock::MockServer;

use crate::error::HttpError;
use crate::policy::{AddressPolicy, Lookup, resolve_checked_with};

pub mod leak;

/// Starts a fresh wiremock server for one test. A thin wrapper today; it exists so every
/// caller starts a server the same way and picks up any shared defaults added later
/// (request logging, default responders) in one place.
pub async fn test_server() -> MockServer {
    MockServer::start().await
}

/// A stub resolver that answers every lookup with fixed addresses, for testing the address
/// policy's resolve-and-filter logic without DNS.
#[derive(Clone, Debug)]
pub struct StubLookup {
    addrs: Vec<SocketAddr>,
}

impl StubLookup {
    /// A stub that answers every lookup with `addrs`.
    #[must_use]
    pub fn new(addrs: Vec<SocketAddr>) -> Self {
        Self { addrs }
    }

    /// [`crate::resolve_checked`], resolving through this stub instead of the system.
    ///
    /// # Errors
    ///
    /// Returns [`HttpError::AddressRejected`] when no allowed address remains.
    pub async fn resolve_checked(
        &self,
        host: &str,
        port: u16,
        policy: AddressPolicy,
    ) -> Result<Vec<SocketAddr>, HttpError> {
        resolve_checked_with(self, host, port, policy).await
    }
}

#[async_trait::async_trait]
impl Lookup for StubLookup {
    async fn lookup(&self, _host: &str, _port: u16) -> io::Result<Vec<SocketAddr>> {
        Ok(self.addrs.clone())
    }
}
