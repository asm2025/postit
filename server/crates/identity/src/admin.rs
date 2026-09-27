use std::sync::Arc;

use postit_core::{AuditEventId, IdGenerator, UserId};
use postit_data::DataError;
use postit_data::audit::{AuditEvent, AuditEventKind, AuditLog};
use postit_data::users::{UserRecord, UserRole, UserStatus, UsersRepo};
use postit_jobs::JobQueue;
use postit_mail::{MailKind, MailOutbox, MailParams};
use sqlx::PgPool;

use crate::error::IdentityError;

#[derive(Clone)]
pub struct UserAdminService {
    pool: PgPool,
    ids: Arc<dyn IdGenerator>,
    jobs: JobQueue,
    outbox: MailOutbox,
}

impl UserAdminService {
    #[must_use]
    pub fn new(
        pool: PgPool,
        ids: Arc<dyn IdGenerator>,
        jobs: JobQueue,
        outbox: MailOutbox,
    ) -> Self {
        Self {
            pool,
            ids,
            jobs,
            outbox,
        }
    }

    /// `pending -> active`. Sets `approved_by` to `actor`.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::Data`] with [`DataError::NotFound`] if `target` doesn't
    /// exist, [`IdentityError::UserDeleting`] if `target` is `deleting`, or
    /// [`IdentityError::InvalidTransition`] if `target` isn't `pending`.
    pub async fn approve(
        &self,
        actor: UserId,
        target: UserId,
    ) -> Result<UserRecord, IdentityError> {
        let mut tx = self.pool.begin().await.map_err(DataError::from)?;
        let user = UsersRepo::lock_by_id(&mut tx, target)
            .await?
            .ok_or(DataError::NotFound)?;
        if user.status == UserStatus::Deleting {
            return Err(IdentityError::UserDeleting);
        }
        if user.status != UserStatus::Pending {
            return Err(IdentityError::InvalidTransition(
                user.status.as_str(),
                "active",
            ));
        }

        let updated =
            UsersRepo::set_status(&mut tx, target, UserStatus::Active, Some(actor)).await?;
        let audit_id = AuditEventId::from(self.ids.generate());
        AuditLog::record(
            &mut tx,
            audit_id,
            AuditEvent::new(AuditEventKind::UserApproved)
                .actor(actor)
                .subject(target),
        )
        .await?;
        self.outbox
            .send(
                &mut tx,
                MailKind::UserApproved,
                target,
                MailParams::None,
                None,
            )
            .await?;
        tx.commit().await.map_err(DataError::from)?;
        Ok(updated)
    }

    /// `active -> disabled`. Refuses on the last active admin.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::Data`] with [`DataError::NotFound`] if `target` doesn't
    /// exist, [`IdentityError::UserDeleting`] if `target` is `deleting`,
    /// [`IdentityError::InvalidTransition`] if `target` isn't `active`, or
    /// [`IdentityError::LastAdmin`] if `target` is the last active admin.
    pub async fn disable(
        &self,
        actor: UserId,
        target: UserId,
    ) -> Result<UserRecord, IdentityError> {
        let mut tx = self.pool.begin().await.map_err(DataError::from)?;
        let user = UsersRepo::lock_by_id(&mut tx, target)
            .await?
            .ok_or(DataError::NotFound)?;
        if user.status == UserStatus::Deleting {
            return Err(IdentityError::UserDeleting);
        }
        if user.status != UserStatus::Active {
            return Err(IdentityError::InvalidTransition(
                user.status.as_str(),
                "disabled",
            ));
        }
        if user.role == UserRole::Admin && UsersRepo::count_active_admins(&mut tx).await? <= 1 {
            return Err(IdentityError::LastAdmin);
        }

        let updated = UsersRepo::set_status(&mut tx, target, UserStatus::Disabled, None).await?;
        let audit_id = AuditEventId::from(self.ids.generate());
        AuditLog::record(
            &mut tx,
            audit_id,
            AuditEvent::new(AuditEventKind::UserDisabled)
                .actor(actor)
                .subject(target),
        )
        .await?;
        tx.commit().await.map_err(DataError::from)?;
        Ok(updated)
    }

    /// `disabled -> active`.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::Data`] with [`DataError::NotFound`] if `target` doesn't
    /// exist, [`IdentityError::UserDeleting`] if `target` is `deleting`, or
    /// [`IdentityError::InvalidTransition`] if `target` isn't `disabled`.
    pub async fn enable(&self, actor: UserId, target: UserId) -> Result<UserRecord, IdentityError> {
        let mut tx = self.pool.begin().await.map_err(DataError::from)?;
        let user = UsersRepo::lock_by_id(&mut tx, target)
            .await?
            .ok_or(DataError::NotFound)?;
        if user.status == UserStatus::Deleting {
            return Err(IdentityError::UserDeleting);
        }
        if user.status != UserStatus::Disabled {
            return Err(IdentityError::InvalidTransition(
                user.status.as_str(),
                "active",
            ));
        }

        let updated = UsersRepo::set_status(&mut tx, target, UserStatus::Active, None).await?;
        let audit_id = AuditEventId::from(self.ids.generate());
        AuditLog::record(
            &mut tx,
            audit_id,
            AuditEvent::new(AuditEventKind::UserEnabled)
                .actor(actor)
                .subject(target),
        )
        .await?;
        tx.commit().await.map_err(DataError::from)?;
        Ok(updated)
    }

    /// Allowed on `active` or `disabled` users. Refuses demoting the last active admin.
    ///
    /// # Errors
    ///
    /// Returns [`IdentityError::Data`] with [`DataError::NotFound`] if `target` doesn't
    /// exist, [`IdentityError::UserDeleting`] if `target` is `deleting`,
    /// [`IdentityError::InvalidTransition`] if `target` is `pending`, or
    /// [`IdentityError::LastAdmin`] if `target` is the last active admin being demoted.
    pub async fn change_role(
        &self,
        actor: UserId,
        target: UserId,
        role: UserRole,
    ) -> Result<UserRecord, IdentityError> {
        let mut tx = self.pool.begin().await.map_err(DataError::from)?;
        let user = UsersRepo::lock_by_id(&mut tx, target)
            .await?
            .ok_or(DataError::NotFound)?;
        if user.status == UserStatus::Deleting {
            return Err(IdentityError::UserDeleting);
        }
        if !matches!(user.status, UserStatus::Active | UserStatus::Disabled) {
            return Err(IdentityError::InvalidTransition(
                user.status.as_str(),
                role.as_str(),
            ));
        }
        let demoting_last_active_admin = user.role == UserRole::Admin
            && role == UserRole::Member
            && user.status == UserStatus::Active
            && UsersRepo::count_active_admins(&mut tx).await? <= 1;
        if demoting_last_active_admin {
            return Err(IdentityError::LastAdmin);
        }

        let updated = UsersRepo::set_role(&mut tx, target, role).await?;
        let audit_id = AuditEventId::from(self.ids.generate());
        let event = AuditEvent::new(AuditEventKind::RoleChanged)
            .actor(actor)
            .subject(target)
            .detail("new_role", role.as_str())?;
        AuditLog::record(&mut tx, audit_id, event).await?;
        tx.commit().await.map_err(DataError::from)?;
        Ok(updated)
    }

    /// Admin path: delete another user, or reject a pending one.
    ///
    /// # Errors
    ///
    /// [`IdentityError::CannotDeleteSelf`] when `actor == target`;
    /// [`DataError::NotFound`]; [`IdentityError::UserDeleting`]; [`IdentityError::LastAdmin`].
    pub async fn delete_user(&self, actor: UserId, target: UserId) -> Result<(), IdentityError> {
        if actor == target {
            return Err(IdentityError::CannotDeleteSelf);
        }
        self.delete(target, Some(actor)).await
    }

    /// `DELETE /me`: the user deletes their own account. The display-name confirmation is
    /// checked by `postit-api` before calling this.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`]; [`IdentityError::UserDeleting`]; [`IdentityError::LastAdmin`].
    pub async fn delete_self(&self, user: UserId) -> Result<(), IdentityError> {
        self.delete(user, Some(user)).await
    }

    async fn delete(&self, target: UserId, actor: Option<UserId>) -> Result<(), IdentityError> {
        let mut tx = self.pool.begin().await.map_err(DataError::from)?;
        let user = UsersRepo::lock_by_id(&mut tx, target)
            .await?
            .ok_or(DataError::NotFound)?;
        if user.status == UserStatus::Deleting {
            return Err(IdentityError::UserDeleting);
        }
        if user.role == UserRole::Admin
            && user.status == UserStatus::Active
            && UsersRepo::count_active_admins(&mut tx).await? <= 1
        {
            return Err(IdentityError::LastAdmin);
        }
        crate::deletion::start_deletion(&mut tx, self.ids.as_ref(), &self.jobs, target, actor)
            .await?;
        tx.commit().await.map_err(DataError::from)?;
        Ok(())
    }
}
