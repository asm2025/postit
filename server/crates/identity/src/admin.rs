use std::sync::Arc;

use postit_core::{AuditEventId, IdGenerator, UserId};
use postit_data::DataError;
use postit_data::audit::{AuditEvent, AuditEventKind, AuditLog};
use postit_data::users::{UserRecord, UserRole, UserStatus, UsersRepo};
use sqlx::PgPool;

use crate::error::IdentityError;

#[derive(Clone)]
pub struct UserAdminService {
    pool: PgPool,
    ids: Arc<dyn IdGenerator>,
}

impl UserAdminService {
    #[must_use]
    pub fn new(pool: PgPool, ids: Arc<dyn IdGenerator>) -> Self {
        Self { pool, ids }
    }

    /// `pending -> active`. Sets `approved_by` to `actor`.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::Data`] with [`DataError::NotFound`] if `target` doesn't
    /// exist, or [`IdentityError::InvalidTransition`] if `target` isn't `pending`.
    pub async fn approve(
        &self,
        actor: UserId,
        target: UserId,
    ) -> Result<UserRecord, IdentityError> {
        let mut conn = self.pool.acquire().await.map_err(DataError::from)?;
        let user = UsersRepo::find_by_id(&mut conn, target)
            .await?
            .ok_or(DataError::NotFound)?;
        if user.status != UserStatus::Pending {
            return Err(IdentityError::InvalidTransition(
                user.status.as_str(),
                "active",
            ));
        }

        let updated =
            UsersRepo::set_status(&mut conn, target, UserStatus::Active, Some(actor)).await?;
        let audit_id = AuditEventId::from(self.ids.generate());
        AuditLog::record(
            &mut conn,
            audit_id,
            AuditEvent::new(AuditEventKind::UserApproved)
                .actor(actor)
                .subject(target),
        )
        .await?;
        Ok(updated)
    }

    /// `active -> disabled`. Refuses on the last active admin.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::Data`] with [`DataError::NotFound`] if `target` doesn't
    /// exist, [`IdentityError::InvalidTransition`] if `target` isn't `active`, or
    /// [`IdentityError::LastAdmin`] if `target` is the last active admin.
    pub async fn disable(
        &self,
        actor: UserId,
        target: UserId,
    ) -> Result<UserRecord, IdentityError> {
        let mut conn = self.pool.acquire().await.map_err(DataError::from)?;
        let user = UsersRepo::find_by_id(&mut conn, target)
            .await?
            .ok_or(DataError::NotFound)?;
        if user.status != UserStatus::Active {
            return Err(IdentityError::InvalidTransition(
                user.status.as_str(),
                "disabled",
            ));
        }
        if user.role == UserRole::Admin && UsersRepo::count_active_admins(&mut conn).await? <= 1 {
            return Err(IdentityError::LastAdmin);
        }

        let updated = UsersRepo::set_status(&mut conn, target, UserStatus::Disabled, None).await?;
        let audit_id = AuditEventId::from(self.ids.generate());
        AuditLog::record(
            &mut conn,
            audit_id,
            AuditEvent::new(AuditEventKind::UserDisabled)
                .actor(actor)
                .subject(target),
        )
        .await?;
        Ok(updated)
    }

    /// `disabled -> active`.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::Data`] with [`DataError::NotFound`] if `target` doesn't
    /// exist, or [`IdentityError::InvalidTransition`] if `target` isn't `disabled`.
    pub async fn enable(&self, actor: UserId, target: UserId) -> Result<UserRecord, IdentityError> {
        let mut conn = self.pool.acquire().await.map_err(DataError::from)?;
        let user = UsersRepo::find_by_id(&mut conn, target)
            .await?
            .ok_or(DataError::NotFound)?;
        if user.status != UserStatus::Disabled {
            return Err(IdentityError::InvalidTransition(
                user.status.as_str(),
                "active",
            ));
        }

        let updated = UsersRepo::set_status(&mut conn, target, UserStatus::Active, None).await?;
        let audit_id = AuditEventId::from(self.ids.generate());
        AuditLog::record(
            &mut conn,
            audit_id,
            AuditEvent::new(AuditEventKind::UserEnabled)
                .actor(actor)
                .subject(target),
        )
        .await?;
        Ok(updated)
    }

    /// Allowed on `active` or `disabled` users. Refuses demoting the last active admin.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::Data`] with [`DataError::NotFound`] if `target` doesn't
    /// exist, [`IdentityError::InvalidTransition`] if `target` is `pending` or `deleting`,
    /// or [`IdentityError::LastAdmin`] if `target` is the last active admin being demoted.
    pub async fn change_role(
        &self,
        actor: UserId,
        target: UserId,
        role: UserRole,
    ) -> Result<UserRecord, IdentityError> {
        let mut conn = self.pool.acquire().await.map_err(DataError::from)?;
        let user = UsersRepo::find_by_id(&mut conn, target)
            .await?
            .ok_or(DataError::NotFound)?;
        if !matches!(user.status, UserStatus::Active | UserStatus::Disabled) {
            return Err(IdentityError::InvalidTransition(
                user.status.as_str(),
                role.as_str(),
            ));
        }
        let demoting_last_active_admin = user.role == UserRole::Admin
            && role == UserRole::Member
            && user.status == UserStatus::Active
            && UsersRepo::count_active_admins(&mut conn).await? <= 1;
        if demoting_last_active_admin {
            return Err(IdentityError::LastAdmin);
        }

        let updated = UsersRepo::set_role(&mut conn, target, role).await?;
        let audit_id = AuditEventId::from(self.ids.generate());
        let event = AuditEvent::new(AuditEventKind::RoleChanged)
            .actor(actor)
            .subject(target)
            .detail("new_role", role.as_str())?;
        AuditLog::record(&mut conn, audit_id, event).await?;
        Ok(updated)
    }
}
