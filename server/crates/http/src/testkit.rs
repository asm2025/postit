//! Shared wiremock setup for crates that stub HTTP APIs in tests (plan 03 platform plugins,
//! `postit-oauth`, and this crate's own tests). Gated behind the `testkit` feature so
//! `wiremock` never reaches a non-test build.

use wiremock::MockServer;

/// Starts a fresh wiremock server for one test. A thin wrapper today; it exists so every
/// caller starts a server the same way and picks up any shared defaults added later
/// (request logging, default responders) in one place.
pub async fn test_server() -> MockServer {
    MockServer::start().await
}
