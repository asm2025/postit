mod advisory_lock;
mod error;
mod pool;

pub use error::DataError;
pub use pool::Db;

pub mod locks {
    pub use crate::advisory_lock::{BOOTSTRAP_ADMIN_LOCK_KEY, MIGRATIONS_LOCK_KEY, xact_lock};
}
