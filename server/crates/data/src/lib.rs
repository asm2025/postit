//! `postit-data`: Postgres pool, migrations, and the repositories every owned table goes through.
#![warn(missing_docs)]

pub mod accounts;
mod advisory_lock;
pub mod app_settings;
pub mod audit;
pub mod audit_repo;
pub mod brand_profiles;
pub mod campaigns;
mod constraint;
mod error;
pub mod idempotency;
pub mod job_outbox;
pub mod jobs;
pub mod limits;
pub mod media;
mod paging;
pub mod platforms;
mod pool;
pub mod posts;
pub mod preferences;
pub mod pseudonym;
pub mod recurrences;
pub mod recurring_runs;
pub mod reseal;
pub mod retention;
mod scope;
pub mod target_groups;
#[cfg(feature = "testkit")]
pub mod testkit;
mod text;
pub mod users;
pub mod vault;
pub mod writing_styles;

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
