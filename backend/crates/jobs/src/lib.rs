//! `postit-jobs`: the job queue facade over apalis, the worker, and the admin console backend.
#![warn(missing_docs)]

mod apalis_sql;
mod backend;
mod console;
mod console_spec;
mod dispatch;
mod error;
mod health;
mod job;
mod ledger;
pub mod maintenance;
mod queue;
mod recurring;
mod registry;
mod relay;
#[cfg(feature = "testkit")]
pub mod testkit;
mod worker;

pub use console::{
    AttemptView, ConsoleError, JobConsole, JobDetail, JobView, RecurringView, StatsView,
};
pub use console_spec::{Actions, ConsoleCatalog, ConsoleSpec, DisabledReason, JobSummary};
pub use error::{JobError, JobsError};
pub use health::WorkerHealth;
pub use job::{Job, JobContext, JobId, Queue, RetryPolicy};
pub use queue::JobQueue;
pub use registry::{JobRegistry, RegisteredTypes};
pub use worker::{Worker, migrate};
