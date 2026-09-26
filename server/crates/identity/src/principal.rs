use postit_core::UserId;
use postit_data::users::{UserRole, UserStatus};

#[derive(Debug, Clone, Copy)]
pub struct Principal {
    pub user_id: UserId,
    pub role: UserRole,
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
