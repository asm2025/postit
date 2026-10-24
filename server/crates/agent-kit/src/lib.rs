//! `postit-agent-kit`: the machinery every platform agent shares, so no agent repeats it.
//!
//! - [`http`] turns the opaque [`postit_core::HttpHandles`] an agent receives into the concrete
//!   clients from `postit-http`.
//! - [`error`] maps an [`postit_http::HttpError`] to the [`postit_core::AgentError`] the engine
//!   reacts to, including the rule that a failure after the request was sent may have changed the
//!   platform.
//!
//! Agent crates depend on `postit-core`, `postit-http`, and this crate. They never depend on
//! `postit-oauth` or `postit-media`: credentials and media reach an agent only through the traits
//! in its agent context.
//!
//! # Rate limits
//!
//! `postit-http` limits requests per host. An agent that needs a short-window limit of its own
//! (a platform allowing a few posts per minute) owns one [`postit_http::RateLimiters`], built in
//! its constructor, and shares it with its clients through an `Arc`. The agent context does not
//! change for this.
//!
//! The `testkit` feature is reserved for the shared conformance suite.
#![warn(missing_docs)]

pub mod error;
pub mod http;
pub mod poll;
pub mod upload;

pub use error::{ErrorHook, Phase, classify};
pub use http::{public, standard};
pub use poll::{Poll, poll_until};
pub use upload::{Chunk, upload_chunks};
