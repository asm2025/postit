//! The authenticated caller.

use postit_core::UserId;
use postit_data::users::{UserRole, UserStatus};

/// The authenticated caller: the minimum the API needs to authorise a request.
#[derive(Debug, Clone, Copy)]
pub struct Principal {
    /// The user's ID.
    pub user_id: UserId,
    /// The user's role.
    pub role: UserRole,
    /// The user's status.
    pub status: UserStatus,
}

impl From<&postit_data::users::UserRecord> for Principal {
    fn from(record: &postit_data::users::UserRecord) -> Self {
        Self {
            user_id: record.id,
            role: record.role,
            status: record.status,
        }
    }
}
