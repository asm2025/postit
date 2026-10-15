//! `postit-data`: Postgres pool, migrations, and the repositories every owned table goes through.
#![warn(missing_docs)]

mod advisory_lock;
pub mod audit;
pub mod audit_repo;
mod error;
pub mod idempotency;
pub mod job_outbox;
pub mod jobs;
mod pool;
pub mod preferences;
pub mod pseudonym;
pub mod recurring_runs;
pub mod retention;
mod scope;
pub mod users;

pub use error::DataError;
pub use pool::Db;
pub use scope::{Access, Capability, OwnerScope, ScopeError};
pub use users::ParseEnumError;

/// Postgres advisory-lock keys and helpers shared across crates.
pub mod locks {
    pub use crate::advisory_lock::{
        ADMIN_SET_LOCK_KEY, JOBS_MIGRATIONS_LOCK_KEY, MIGRATIONS_LOCK_KEY, with_session_lock,
        xact_lock,
    };
}
