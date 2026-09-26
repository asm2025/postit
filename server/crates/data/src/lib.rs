mod advisory_lock;
pub mod audit;
pub mod audit_repo;
mod error;
pub mod idempotency;
mod pool;
pub mod preferences;
pub mod retention;
mod scope;
pub mod users;

pub use error::DataError;
pub use pool::Db;
pub use scope::{Access, Capability, OwnerScope, ScopeError};

pub mod locks {
    pub use crate::advisory_lock::{BOOTSTRAP_ADMIN_LOCK_KEY, MIGRATIONS_LOCK_KEY, xact_lock};
}
