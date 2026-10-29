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
mod deletion;
mod deps;
pub mod direct;
mod disconnect;
mod error;
mod jobs;
pub mod notices;
pub mod outcome;
pub mod poll;
pub mod preflight;
pub mod progress;
pub mod quota;
pub mod reconcile;
pub mod request;
mod service;
pub mod sweep;
pub mod worker;

pub use deletion::PublishingDeletionStep;
pub use deps::PublishingDeps;
pub use disconnect::PublishingDisconnect;
pub use error::PublishingError;
pub use jobs::{
    AccountPreflight, DeliveryDueSweep, DirectSendBatch, PollDeliveryStatus, PublishDelivery,
    ReconcileDelivery, console, register,
};
pub use request::{AssembleError, Assembled, assemble};
pub use service::{CancelReport, PublishInput, PublishingService, RescheduleOutcome};
pub use worker::run_publish;
