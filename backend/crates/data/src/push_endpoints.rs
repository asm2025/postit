//! Web Push and mobile push endpoints, one row per device. The endpoint URL and both keys
//! are sealed: an endpoint URL is a capability to push to the device.

use chrono::{DateTime, Utc};
use postit_core::{PushEndpointId, UserId};
use postit_vault::{SealContext, Sealed};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::error::DataError;
use crate::text::{parse, text_enum};

text_enum! {
    /// Which push service a device is reached through.
    PushPlatform {
        /// Android through its push service.
        Android => "android",
        /// iOS through its push service.
        Ios => "ios",
        /// A browser through Web Push.
        Web => "web",
    }
}

/// A device to register.
#[derive(Clone)]
pub struct NewPushEndpoint {
    /// The row's ID, which the sealed values are bound to. Call [`PushEndpointRepo::find_id`]
    /// first and use its result, or a fresh ID when it finds none. If the device is stored
    /// under a different ID (another registration won the race, or the caller skipped
    /// `find_id`), [`PushEndpointRepo::upsert`] fails with [`DataError::Conflict`]
    /// (`push_endpoint_id_mismatch`) and changes nothing: retry from `find_id`.
    pub id: PushEndpointId,
    /// The device's user.
    pub user: UserId,
    /// The sealed endpoint URL, sealed with [`PushEndpointRepo::seal_context`] `(id, "endpoint")`.
    pub endpoint: Sealed<String>,
    /// SHA-256 of the endpoint URL, so the same device is recognized without opening it.
    pub endpoint_hash: [u8; 32],
    /// The sealed client public key, column `p256dh`.
    pub p256dh: Sealed<String>,
    /// The sealed authentication secret, column `auth`.
    pub auth: Sealed<String>,
    /// The push service.
    pub platform: PushPlatform,
    /// A name for the device, shown in the profile.
    pub device_label: String,
}

impl std::fmt::Debug for NewPushEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewPushEndpoint")
            .field("id", &self.id)
            .field("user", &self.user)
            .field("platform", &self.platform)
            .field("device_label", &self.device_label)
            .finish_non_exhaustive()
    }
}

/// A registered device.
#[derive(Clone)]
pub struct PushEndpointRow {
    /// The row.
    pub id: PushEndpointId,
    /// The device's user.
    pub user: UserId,
    /// The sealed endpoint URL.
    pub endpoint: Sealed<String>,
    /// SHA-256 of the endpoint URL.
    pub endpoint_hash: Vec<u8>,
    /// The sealed client public key.
    pub p256dh: Sealed<String>,
    /// The sealed authentication secret.
    pub auth: Sealed<String>,
    /// The push service.
    pub platform: PushPlatform,
    /// The device's name.
    pub device_label: Option<String>,
    /// When a push last reached the device.
    pub last_success_at: Option<DateTime<Utc>>,
    /// When the device was registered.
    pub created_at: DateTime<Utc>,
}

impl std::fmt::Debug for PushEndpointRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PushEndpointRow")
            .field("id", &self.id)
            .field("user", &self.user)
            .field("platform", &self.platform)
            .field("device_label", &self.device_label)
            .finish_non_exhaustive()
    }
}

struct Row {
    id: Uuid,
    user_id: Uuid,
    endpoint: Sealed<String>,
    endpoint_hash: Vec<u8>,
    p256dh: Sealed<String>,
    auth: Sealed<String>,
    platform: String,
    device_label: Option<String>,
    last_success_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

impl TryFrom<Row> for PushEndpointRow {
    type Error = DataError;

    fn try_from(row: Row) -> Result<Self, DataError> {
        Ok(Self {
            id: PushEndpointId::from(row.id),
            user: UserId::from(row.user_id),
            endpoint: row.endpoint,
            endpoint_hash: row.endpoint_hash,
            p256dh: row.p256dh,
            auth: row.auth,
            platform: parse(&row.platform)?,
            device_label: row.device_label,
            last_success_at: row.last_success_at,
            created_at: row.created_at,
        })
    }
}

/// Persistence for `push_endpoints`.
pub struct PushEndpointRepo;

impl PushEndpointRepo {
    /// Where a value of the row is sealed: `column` (`endpoint`, `p256dh`, or `auth`) of the
    /// row `id`.
    #[must_use]
    pub fn seal_context(id: PushEndpointId, column: &'static str) -> SealContext {
        SealContext {
            table: "push_endpoints",
            column,
            row: id.as_uuid(),
        }
    }

    /// The ID of the user's row for this endpoint hash, if the device is already registered.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn find_id(
        conn: &mut PgConnection,
        user: UserId,
        endpoint_hash: &[u8; 32],
    ) -> Result<Option<PushEndpointId>, DataError> {
        let hash: &[u8] = endpoint_hash;
        let id = sqlx::query_scalar!(
            "SELECT id FROM push_endpoints WHERE user_id = $1 AND endpoint_hash = $2",
            user.as_uuid(),
            hash,
        )
        .fetch_optional(&mut *conn)
        .await?;
        Ok(id.map(PushEndpointId::from))
    }

    /// Registers a device. The same `(user, endpoint_hash)` with the stored ID replaces the
    /// keys, label, and platform of the stored row, which keeps its ID.
    ///
    /// # Errors
    ///
    /// [`DataError::Conflict`] (`push_endpoint_id_mismatch`) when the device is stored under a
    /// different ID than `new.id`; nothing is changed. [`DataError::Sql`] on a database failure.
    pub async fn upsert(
        conn: &mut PgConnection,
        new: &NewPushEndpoint,
    ) -> Result<PushEndpointId, DataError> {
        let hash: &[u8] = &new.endpoint_hash;
        let id = sqlx::query_scalar!(
            r#"INSERT INTO push_endpoints
                 (id, user_id, endpoint, p256dh, auth, platform, endpoint_hash, device_label)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (user_id, endpoint_hash) DO UPDATE
             SET endpoint = EXCLUDED.endpoint, p256dh = EXCLUDED.p256dh,
                 auth = EXCLUDED.auth, platform = EXCLUDED.platform,
                 device_label = EXCLUDED.device_label
             WHERE push_endpoints.id = EXCLUDED.id
             RETURNING id"#,
            new.id.as_uuid(),
            new.user.as_uuid(),
            &new.endpoint as &Sealed<String>,
            &new.p256dh as &Sealed<String>,
            &new.auth as &Sealed<String>,
            new.platform.as_str(),
            hash,
            new.device_label,
        )
        .fetch_optional(&mut *conn)
        .await?;
        id.map(PushEndpointId::from)
            .ok_or_else(|| DataError::Conflict("push_endpoint_id_mismatch".to_owned()))
    }

    /// The user's devices, oldest first.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn list_for_user(
        conn: &mut PgConnection,
        user: UserId,
    ) -> Result<Vec<PushEndpointRow>, DataError> {
        sqlx::query_as!(
            Row,
            r#"SELECT id, user_id,
                      endpoint AS "endpoint: Sealed<String>", endpoint_hash,
                      p256dh AS "p256dh: Sealed<String>", auth AS "auth: Sealed<String>",
                      platform, device_label, last_success_at, created_at
               FROM push_endpoints WHERE user_id = $1
               ORDER BY created_at, id"#,
            user.as_uuid(),
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(PushEndpointRow::try_from)
        .collect()
    }

    /// Removes one of the user's devices; `false` when the user has no such device.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn delete(
        conn: &mut PgConnection,
        user: UserId,
        id: PushEndpointId,
    ) -> Result<bool, DataError> {
        let result = sqlx::query!(
            "DELETE FROM push_endpoints WHERE id = $1 AND user_id = $2",
            id.as_uuid(),
            user.as_uuid(),
        )
        .execute(&mut *conn)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Removes a device by ID alone, for the sender when the push service reports it gone.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn delete_by_id(
        conn: &mut PgConnection,
        id: PushEndpointId,
    ) -> Result<(), DataError> {
        sqlx::query!("DELETE FROM push_endpoints WHERE id = $1", id.as_uuid())
            .execute(&mut *conn)
            .await?;
        Ok(())
    }

    /// A device by ID alone, for the sender.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn get(
        conn: &mut PgConnection,
        id: PushEndpointId,
    ) -> Result<Option<PushEndpointRow>, DataError> {
        sqlx::query_as!(
            Row,
            r#"SELECT id, user_id,
                      endpoint AS "endpoint: Sealed<String>", endpoint_hash,
                      p256dh AS "p256dh: Sealed<String>", auth AS "auth: Sealed<String>",
                      platform, device_label, last_success_at, created_at
               FROM push_endpoints WHERE id = $1"#,
            id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .map(PushEndpointRow::try_from)
        .transpose()
    }

    /// Records that a push reached the device.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn mark_success(
        conn: &mut PgConnection,
        id: PushEndpointId,
        at: DateTime<Utc>,
    ) -> Result<(), DataError> {
        sqlx::query!(
            "UPDATE push_endpoints SET last_success_at = $2 WHERE id = $1",
            id.as_uuid(),
            at,
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }
}
