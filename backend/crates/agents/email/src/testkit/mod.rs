//! Test support for crates that send through the email agent: an in-process SMTP sink.
//! Gated behind the `testkit` feature so none of it reaches a production build.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::missing_panics_doc)]

pub mod sink;

pub use sink::{Received, Script, Sink, closed_port};
