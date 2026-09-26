mod advisory_lock;
mod error;
mod pool;
mod scope;
pub mod users;

pub use error::DataError;
pub use pool::Db;
pub use scope::{Access, Capability, OwnerScope, ScopeError};

pub mod locks {
    pub use crate::advisory_lock::{BOOTSTRAP_ADMIN_LOCK_KEY, MIGRATIONS_LOCK_KEY, xact_lock};
}
