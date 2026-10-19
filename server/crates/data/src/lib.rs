//! `postit-data`: Postgres pool, migrations, and the repositories every owned table goes through.
#![warn(missing_docs)]

mod advisory_lock;
pub mod app_settings;
pub mod audit;
pub mod audit_repo;
mod constraint;
mod error;
pub mod idempotency;
pub mod job_outbox;
pub mod jobs;
pub mod limits;
pub mod platforms;
mod pool;
pub mod preferences;
pub mod pseudonym;
pub mod recurring_runs;
pub mod reseal;
pub mod retention;
mod scope;
#[cfg(feature = "testkit")]
pub mod testkit;
pub mod users;
pub mod vault;

pub use error::DataError;
pub use pool::Db;
pub use scope::{Access, AccountLimit, Capability, Level, OwnerScope, ScopeError};
pub use users::ParseEnumError;

/// Postgres advisory-lock keys and helpers shared across crates.
pub mod locks {
    pub use crate::advisory_lock::{
        ADMIN_SET_LOCK_KEY, JOBS_MIGRATIONS_LOCK_KEY, MIGRATIONS_LOCK_KEY, VAULT_ROTATE_LOCK_KEY,
        VAULT_STATE_LOCK_KEY, with_session_lock, xact_lock,
    };
}
