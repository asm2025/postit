//! `postit-publishing`: the publishing engine. It turns a publish job's deliveries into posts
//! on platforms through the agent contract, and owns the delivery state machine's retries,
//! parks, and reconciliation; the job runtime only carries its wake-ups.
//!
//! This crate consumes `postit-core`'s agent contract and models, `postit-data`'s delivery
//! repositories, bound clients from `postit-oauth`, and assets from `postit-media`. It
//! depends on neither `contacts` nor `drafter`.
#![warn(missing_docs)]

pub mod backoff;
pub mod checks;
mod deps;
mod jobs;
pub mod notices;
pub mod outcome;
pub mod progress;
pub mod quota;
pub mod request;
mod service;
pub mod worker;

pub use deps::PublishingDeps;
pub use jobs::{
    AccountPreflight, DeliveryDueSweep, DirectSendBatch, PollDeliveryStatus, PublishDelivery,
    ReconcileDelivery, console, register,
};
pub use request::{AssembleError, Assembled, assemble};
pub use service::PublishingService;
pub use worker::run_publish;
