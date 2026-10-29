//! Per-user preferences.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use postit_core::{NotificationKind, UserId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Connection, PgConnection};

use crate::error::DataError;
use crate::text::corrupt;

/// A user's choice for one kind of notification; `None` means "use the kind's default".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelSwitch {
    /// Whether to push this kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub push: Option<bool>,
    /// Whether to email this kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<bool>,
}

/// The user's per-kind notification overrides.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NotificationSwitches(pub BTreeMap<NotificationKind, ChannelSwitch>);

impl NotificationSwitches {
    /// Whether `kind` is pushed: the user's choice, else the kind's default.
    #[must_use]
    pub fn push(&self, kind: NotificationKind) -> bool {
        self.0
            .get(&kind)
            .and_then(|s| s.push)
            .unwrap_or_else(|| kind.default_push())
    }

    /// Whether `kind` is emailed: the user's choice, else the kind's default. Always `false`
    /// for a kind that may not be emailed.
    #[must_use]
    pub fn email(&self, kind: NotificationKind) -> bool {
        kind.email_allowed()
            && self
                .0
                .get(&kind)
                .and_then(|s| s.email)
                .unwrap_or_else(|| kind.default_email())
    }
}

/// The fields of [`UserPreferencesRepo::upsert`]; only `Some` fields change.
#[derive(Debug, Clone, Default)]
pub struct PreferencesUpdate {
    /// A BCP 47 language tag.
    pub locale: Option<String>,
    /// An IANA time zone name.
    pub timezone: Option<String>,
    /// The master switch for emails about activity.
    pub email_notifications: Option<bool>,
    /// Per-kind overrides, merged per kind: a kind given here replaces that kind's stored
    /// choice (a choice with neither channel set removes it); other kinds stay.
    pub switches: Option<NotificationSwitches>,
    /// Whether comment notifications are emailed as a daily digest.
    pub comment_digest: Option<bool>,
}

/// A user's settings, one row per user.
#[derive(Debug, Clone)]
pub struct UserPreferences {
    /// The user these preferences belong to.
    pub user_id: UserId,
    /// Whether the user wants emails about their activity.
    pub email_notifications: bool,
    /// IANA time zone name used to display and schedule times.
    pub timezone: String,
    /// BCP 47 language tag for the user's interface and emails.
    pub locale: String,
    /// Whether AI prompts are kept after generation.
    pub store_ai_prompts: bool,
    /// When this user, as an admin, last received an approval-request email.
    pub last_approval_email_at: Option<DateTime<Utc>>,
    /// Per-kind push and email overrides.
    pub notification_switches: NotificationSwitches,
    /// Whether comment notifications are emailed as a daily digest.
    pub comment_digest: bool,
    /// When this user last received the comment digest.
    pub last_comment_digest_at: Option<DateTime<Utc>>,
}

struct Row {
    user_id: uuid::Uuid,
    email_notifications: bool,
    timezone: String,
    locale: String,
    store_ai_prompts: bool,
    last_approval_email_at: Option<DateTime<Utc>>,
    notification_switches: Value,
    comment_digest: bool,
    last_comment_digest_at: Option<DateTime<Utc>>,
}

impl TryFrom<Row> for UserPreferences {
    type Error = DataError;

    fn try_from(row: Row) -> Result<Self, DataError> {
        Ok(Self {
            user_id: UserId::from(row.user_id),
            email_notifications: row.email_notifications,
            timezone: row.timezone,
            locale: row.locale,
            store_ai_prompts: row.store_ai_prompts,
            last_approval_email_at: row.last_approval_email_at,
            notification_switches: serde_json::from_value(row.notification_switches)
                .map_err(corrupt)?,
            comment_digest: row.comment_digest,
            last_comment_digest_at: row.last_comment_digest_at,
        })
    }
}

/// Persistence for user preferences.
pub struct UserPreferencesRepo;

impl UserPreferencesRepo {
    /// Creates the default preferences row for a newly provisioned user. Called by
    /// `postit-identity` inside the same transaction as `UsersRepo::provision`, so a user
    /// never exists without preferences.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure (including a unique-violation if
    /// called twice for the same user).
    pub async fn create_default(
        conn: &mut PgConnection,
        user_id: UserId,
        timezone: &str,
        locale: &str,
    ) -> Result<UserPreferences, DataError> {
        let row = sqlx::query_as!(
            Row,
            r#"INSERT INTO user_preferences (user_id, timezone, locale)
               VALUES ($1, $2, $3)
               RETURNING user_id, email_notifications, timezone, locale, store_ai_prompts, last_approval_email_at,
                      notification_switches, comment_digest, last_comment_digest_at"#,
            user_id.as_uuid(),
            timezone,
            locale,
        )
        .fetch_one(&mut *conn)
        .await?;
        row.try_into()
    }

    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure.
    pub async fn get(
        conn: &mut PgConnection,
        user_id: UserId,
    ) -> Result<Option<UserPreferences>, DataError> {
        let row = sqlx::query_as!(
            Row,
            r#"SELECT user_id, email_notifications, timezone, locale, store_ai_prompts, last_approval_email_at,
                      notification_switches, comment_digest, last_comment_digest_at
               FROM user_preferences WHERE user_id = $1"#,
            user_id.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?;
        row.map(TryInto::try_into).transpose()
    }

    /// # Errors
    ///
    /// Returns [`DataError::NotFound`] if no preferences row exists for `user_id`,
    /// [`DataError::Sql`] otherwise.
    pub async fn update(
        conn: &mut PgConnection,
        user_id: UserId,
        email_notifications: bool,
        timezone: &str,
        store_ai_prompts: bool,
    ) -> Result<UserPreferences, DataError> {
        let row = sqlx::query_as!(
            Row,
            r#"UPDATE user_preferences
               SET email_notifications = $2, timezone = $3, store_ai_prompts = $4
               WHERE user_id = $1
               RETURNING user_id, email_notifications, timezone, locale, store_ai_prompts, last_approval_email_at,
                      notification_switches, comment_digest, last_comment_digest_at"#,
            user_id.as_uuid(),
            email_notifications,
            timezone,
            store_ai_prompts,
        )
        .fetch_optional(&mut *conn)
        .await?;
        row.ok_or(DataError::NotFound)?.try_into()
    }

    /// Sets only the user's locale.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::NotFound`] if no preferences row exists for `user_id`,
    /// [`DataError::Sql`] otherwise.
    pub async fn set_locale(
        conn: &mut PgConnection,
        user_id: UserId,
        locale: &str,
    ) -> Result<UserPreferences, DataError> {
        let row = sqlx::query_as!(
            Row,
            r#"UPDATE user_preferences SET locale = $2
               WHERE user_id = $1
               RETURNING user_id, email_notifications, timezone, locale, store_ai_prompts, last_approval_email_at,
                      notification_switches, comment_digest, last_comment_digest_at"#,
            user_id.as_uuid(),
            locale,
        )
        .fetch_optional(&mut *conn)
        .await?;
        row.ok_or(DataError::NotFound)?.try_into()
    }

    /// Records when `user_id` (an admin) was last sent the coalesced approval email. Upserts:
    /// a user without a preferences row gets one with the column defaults, so the marker is
    /// never silently lost.
    ///
    /// # Errors
    ///
    /// Returns [`DataError::Sql`] on a database failure (including a foreign-key violation
    /// if `user_id` does not exist).
    pub async fn set_last_approval_email_at(
        conn: &mut PgConnection,
        user_id: UserId,
        at: DateTime<Utc>,
    ) -> Result<(), DataError> {
        sqlx::query!(
            r#"INSERT INTO user_preferences (user_id, last_approval_email_at)
               VALUES ($1, $2)
               ON CONFLICT (user_id) DO UPDATE SET last_approval_email_at = EXCLUDED.last_approval_email_at"#,
            user_id.as_uuid(),
            at,
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// Applies the `Some` fields of `update`, creating the row with the column defaults when
    /// the user has none, and returns the result.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure (including a foreign-key violation if `user_id`
    /// does not exist).
    pub async fn upsert(
        conn: &mut PgConnection,
        user_id: UserId,
        update: &PreferencesUpdate,
    ) -> Result<UserPreferences, DataError> {
        let mut tx = conn.begin().await?;
        sqlx::query!(
            "INSERT INTO user_preferences (user_id) VALUES ($1) ON CONFLICT (user_id) DO NOTHING",
            user_id.as_uuid(),
        )
        .execute(&mut *tx)
        .await?;
        let stored = sqlx::query_scalar!(
            "SELECT notification_switches FROM user_preferences WHERE user_id = $1 FOR UPDATE",
            user_id.as_uuid(),
        )
        .fetch_one(&mut *tx)
        .await?;
        let switches = match &update.switches {
            None => None,
            Some(given) => {
                let mut merged: NotificationSwitches =
                    serde_json::from_value(stored).map_err(corrupt)?;
                for (kind, choice) in &given.0 {
                    if *choice == ChannelSwitch::default() {
                        merged.0.remove(kind);
                    } else {
                        merged.0.insert(*kind, *choice);
                    }
                }
                Some(serde_json::to_value(&merged).map_err(corrupt)?)
            }
        };
        let row = sqlx::query_as!(
            Row,
            r#"UPDATE user_preferences
               SET locale = COALESCE($2, locale),
                   timezone = COALESCE($3, timezone),
                   email_notifications = COALESCE($4, email_notifications),
                   notification_switches = COALESCE($5, notification_switches),
                   comment_digest = COALESCE($6, comment_digest)
               WHERE user_id = $1
               RETURNING user_id, email_notifications, timezone, locale, store_ai_prompts,
                         last_approval_email_at,
                         notification_switches, comment_digest, last_comment_digest_at"#,
            user_id.as_uuid(),
            update.locale.as_deref(),
            update.timezone.as_deref(),
            update.email_notifications,
            switches,
            update.comment_digest,
        )
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        row.try_into()
    }

    /// Records when `user_id` was last sent the comment digest. Upserts, like
    /// [`Self::set_last_approval_email_at`].
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure (including a foreign-key violation if `user_id`
    /// does not exist).
    pub async fn set_last_comment_digest_at(
        conn: &mut PgConnection,
        user_id: UserId,
        at: DateTime<Utc>,
    ) -> Result<(), DataError> {
        sqlx::query!(
            r#"INSERT INTO user_preferences (user_id, last_comment_digest_at)
               VALUES ($1, $2)
               ON CONFLICT (user_id) DO UPDATE SET last_comment_digest_at = EXCLUDED.last_comment_digest_at"#,
            user_id.as_uuid(),
            at,
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }
}
