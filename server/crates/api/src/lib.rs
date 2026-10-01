//! `postit-api`: the HTTP surface. Thin handlers over `postit-identity` services and the
//! `postit-data` admin repositories; see plan 02 `postit-api`.

pub mod client_ip;
pub mod dto;
pub mod error;
pub mod extract;
pub mod json;
pub mod limit;
pub mod middleware;
pub mod openapi;
pub mod pagination;
pub mod router;
pub mod routes;
pub mod settings;
pub mod state;
#[cfg(feature = "testkit")]
pub mod testkit;

pub use router::{api_router, probe_router};
pub use settings::ApiSettings;
pub use state::{AppState, Limits, Readiness};
