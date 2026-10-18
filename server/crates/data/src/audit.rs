//! Audit event model and writer.

use std::net::IpAddr;

use postit_core::{AuditEventId, UserId};
use serde_json::{Map, Value};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::DataError;

/// The kinds of audited event. Each is stored by its `snake_case` name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditEventKind {
    /// A user record was created at first sign-in.
    UserProvisioned,
    /// An admin approved a pending user.
    UserApproved,
    /// An admin disabled a user.
    UserDisabled,
    /// An admin re-enabled a disabled user.
    UserEnabled,
    /// An admin changed a user's role.
    RoleChanged,
    /// The first admin was granted through `auth.bootstrap`.
    BootstrapAdminGranted,
    /// A user and their data were deleted.
    UserDeleted,
    /// An email was accepted by the SMTP server.
    EmailSent,
    /// An email was dropped without sending, for example because the recipient became inactive.
    EmailDropped,
    /// An email could not be sent and will not be retried.
    EmailFailed,
    /// An admin retried a job.
    JobRetried,
    /// An admin cancelled a job.
    JobCancelled,
    /// An admin deleted a job.
    JobDeleted,
    /// An admin triggered a recurring job.
    JobTriggered,
    /// A vault key rotation run began.
    VaultRotationStarted,
    /// A vault key rotation run finished.
    VaultRotationCompleted,
}

impl AuditEventKind {
    /// Every kind, in declaration order.
    pub const ALL: [Self; 16] = [
        Self::UserProvisioned,
        Self::UserApproved,
        Self::UserDisabled,
        Self::UserEnabled,
        Self::RoleChanged,
        Self::BootstrapAdminGranted,
        Self::UserDeleted,
        Self::EmailSent,
        Self::EmailDropped,
        Self::EmailFailed,
        Self::JobRetried,
        Self::JobCancelled,
        Self::JobDeleted,
        Self::JobTriggered,
        Self::VaultRotationStarted,
        Self::VaultRotationCompleted,
    ];

    /// The stable name stored in the `kind` column.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserProvisioned => "user_provisioned",
            Self::UserApproved => "user_approved",
            Self::UserDisabled => "user_disabled",
            Self::UserEnabled => "user_enabled",
            Self::RoleChanged => "role_changed",
            Self::BootstrapAdminGranted => "bootstrap_admin_granted",
            Self::UserDeleted => "user_deleted",
            Self::EmailSent => "email_sent",
            Self::EmailDropped => "email_dropped",
            Self::EmailFailed => "email_failed",
            Self::JobRetried => "job_retried",
            Self::JobCancelled => "job_cancelled",
            Self::JobDeleted => "job_deleted",
            Self::JobTriggered => "job_triggered",
            Self::VaultRotationStarted => "vault_rotation_started",
            Self::VaultRotationCompleted => "vault_rotation_completed",
        }
    }
}

impl std::str::FromStr for AuditEventKind {
    type Err = crate::users::ParseEnumError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == value)
            .ok_or_else(|| crate::users::ParseEnumError(value.to_string()))
    }
}

/// A misuse of the audit-event builder's user-reference keys.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AuditError {
    /// A user-reference detail key lacks the `_user_id` suffix.
    #[error("detail key `{0}` must end with `_user_id`")]
    KeyMustEndUserId(String),
    /// A plain detail key carries the reserved `_user_id` suffix.
    #[error("detail key `{0}` must not end with `_user_id`; use detail_user_id")]
    KeyMustNotEndUserId(String),
}

/// Where an audited action came from: the caller's address (already resolved through the
/// trusted-proxy rules) and the request ID. Built once per request by the API layer and passed
/// into every service call that writes an audit event; background jobs pass
/// [`AuditContext::default`], which records neither.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AuditContext {
    /// The caller's address, after trusted-proxy resolution.
    pub ip: Option<IpAddr>,
    /// ID of the request, from `X-Request-Id`.
    pub request_id: Option<Uuid>,
}

/// Builds one audit event. The only way to attach a user reference other than `actor`,
/// `owner`, or `subject` is [`AuditEvent::detail_user_id`], whose key must end in
/// `_user_id` — this is how a pseudonymization pass (plan 02 P5) can find every user
/// reference in `details` without knowing each event kind's shape. `detail` refuses a key
/// with that suffix for the same reason, in the other direction. Neither method can accept
/// a `secrecy`/`RedactedSecret` value — they take `impl Into<serde_json::Value>` /
/// `impl Serialize`, and those secret types deliberately don't implement `Serialize`
/// (`RedactedSecret` serializes to the literal string `"[redacted]"`, `SecretString`
/// doesn't implement `Serialize` at all), so passing one here is a compile error, not a
/// runtime leak.
#[derive(Debug, Clone)]
pub struct AuditEvent {
    kind: AuditEventKind,
    actor_user_id: Option<UserId>,
    owner_id: Option<UserId>,
    subject_user_id: Option<UserId>,
    ip: Option<IpAddr>,
    request_id: Option<Uuid>,
    details: Map<String, Value>,
}

impl AuditEvent {
    /// A new event of `kind` with no references or details.
    #[must_use]
    pub fn new(kind: AuditEventKind) -> Self {
        Self {
            kind,
            actor_user_id: None,
            owner_id: None,
            subject_user_id: None,
            ip: None,
            request_id: None,
            details: Map::new(),
        }
    }

    /// The user who performed the action.
    #[must_use]
    pub fn actor(mut self, id: UserId) -> Self {
        self.actor_user_id = Some(id);
        self
    }

    /// The owner of the workspace the action touched.
    #[must_use]
    pub fn owner(mut self, id: UserId) -> Self {
        self.owner_id = Some(id);
        self
    }

    /// The user the action was about.
    #[must_use]
    pub fn subject(mut self, id: UserId) -> Self {
        self.subject_user_id = Some(id);
        self
    }

    /// The caller's address.
    #[must_use]
    pub fn ip(mut self, ip: IpAddr) -> Self {
        self.ip = Some(ip);
        self
    }

    /// The ID of the originating request.
    #[must_use]
    pub fn request_id(mut self, id: Uuid) -> Self {
        self.request_id = Some(id);
        self
    }

    /// Records the request that caused the event; absent parts stay null.
    #[must_use]
    pub fn context(mut self, ctx: AuditContext) -> Self {
        self.ip = ctx.ip.or(self.ip);
        self.request_id = ctx.request_id.or(self.request_id);
        self
    }

    /// # Errors
    ///
    /// Returns [`AuditError::KeyMustEndUserId`] if `key` doesn't end with `_user_id`.
    pub fn detail_user_id(mut self, key: &str, id: UserId) -> Result<Self, AuditError> {
        if !key.ends_with("_user_id") {
            return Err(AuditError::KeyMustEndUserId(key.to_string()));
        }
        self.details
            .insert(key.to_string(), Value::String(id.as_uuid().to_string()));
        Ok(self)
    }

    /// # Errors
    ///
    /// Returns [`AuditError::KeyMustNotEndUserId`] if `key` ends with `_user_id` — use
    /// [`AuditEvent::detail_user_id`] instead, so pseudonymization can find it.
    pub fn detail(mut self, key: &str, value: impl Into<Value>) -> Result<Self, AuditError> {
        if key.ends_with("_user_id") {
            return Err(AuditError::KeyMustNotEndUserId(key.to_string()));
        }
        self.details.insert(key.to_string(), value.into());
        Ok(self)
    }
}

/// Write access to the audit log.
pub struct AuditLog;

impl AuditLog {
    /// # Errors
    ///
    /// Returns [`DataError::Conflict`] if any referenced user row no longer exists, and
    /// [`DataError::Sql`] on a database failure.
    pub async fn record(
        conn: &mut PgConnection,
        id: AuditEventId,
        event: AuditEvent,
    ) -> Result<(), DataError> {
        let mut referenced: Vec<Uuid> =
            [event.actor_user_id, event.owner_id, event.subject_user_id]
                .into_iter()
                .flatten()
                .map(|id| id.as_uuid())
                .collect();
        referenced.extend(
            event
                .details
                .iter()
                .filter(|(key, _)| key.ends_with("_user_id"))
                .filter_map(|(_, value)| value.as_str())
                .filter_map(|s| Uuid::parse_str(s).ok()),
        );
        referenced.sort_unstable();
        referenced.dedup();
        if !referenced.is_empty() {
            // FOR KEY SHARE: a concurrent delete of any referenced user (delete_user's final
            // step takes FOR UPDATE, UsersRepo::delete deletes) waits for this transaction,
            // and a user already gone fails the write, so no raw ID of a deleted user can
            // land in audit_events after its pseudonymization.
            let locked = sqlx::query_scalar!(
                "SELECT id FROM users WHERE id = ANY($1) FOR KEY SHARE",
                &referenced
            )
            .fetch_all(&mut *conn)
            .await?;
            if locked.len() != referenced.len() {
                return Err(DataError::Conflict(
                    "audit references a deleted user".into(),
                ));
            }
        }
        sqlx::query!(
            r#"INSERT INTO audit_events (id, actor_user_id, owner_id, subject_user_id, kind, ip, request_id, details)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
            id.as_uuid(),
            event.actor_user_id.map(|u| u.as_uuid()),
            event.owner_id.map(|u| u.as_uuid()),
            event.subject_user_id.map(|u| u.as_uuid()),
            event.kind.as_str(),
            event.ip.map(|ip| ip.to_string()) as Option<String>,
            event.request_id,
            Value::Object(event.details),
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// Replaces every reference to `user` in `audit_events` with its pseudonym
    /// ([`crate::pseudonym::pseudonym_for`]): `actor_user_id`, `owner_id`, `subject_user_id`,
    /// and every top-level `details` key ending in `_user_id`. Clears `ip` on events `user`
    /// performed (as the actor), before the actor column is rewritten. Idempotent: a second
    /// run finds no raw reference and returns 0. Returns the number of row updates made.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn pseudonymize_user(
        conn: &mut PgConnection,
        user: UserId,
        key: &[u8],
    ) -> Result<u64, DataError> {
        let raw = user.as_uuid();
        let pseudo = crate::pseudonym::pseudonym_for(key, user);
        let raw_text = raw.to_string();
        let pseudo_text = pseudo.to_string();

        let mut touched = sqlx::query!(
            "UPDATE audit_events SET ip = NULL WHERE actor_user_id = $1 AND ip IS NOT NULL",
            raw
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();

        touched += sqlx::query!(
            "UPDATE audit_events SET actor_user_id = $2 WHERE actor_user_id = $1",
            raw,
            pseudo
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();

        touched += sqlx::query!(
            "UPDATE audit_events SET owner_id = $2 WHERE owner_id = $1",
            raw,
            pseudo
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();

        touched += sqlx::query!(
            "UPDATE audit_events SET subject_user_id = $2 WHERE subject_user_id = $1",
            raw,
            pseudo
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();

        touched += sqlx::query!(
            r#"UPDATE audit_events AS a SET details = (
                   SELECT jsonb_object_agg(
                       e.key,
                       CASE WHEN e.key LIKE '%\_user\_id' AND e.value = to_jsonb($1::text)
                            THEN to_jsonb($2::text)
                            ELSE e.value END)
                   FROM jsonb_each(a.details) AS e(key, value))
               WHERE EXISTS (
                   SELECT 1 FROM jsonb_each(a.details) AS e(key, value)
                   WHERE e.key LIKE '%\_user\_id' AND e.value = to_jsonb($1::text))"#,
            raw_text,
            pseudo_text
        )
        .execute(&mut *conn)
        .await?
        .rows_affected();

        Ok(touched)
    }
}
