//! `postit-api`: the HTTP surface. Thin handlers over `postit-identity` services and the
//! `postit-data` admin repositories; see plan 02 `postit-api`.

pub mod client_ip;
pub mod error;
pub mod limit;
