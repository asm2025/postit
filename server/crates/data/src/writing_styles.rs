//! Writing styles: how the drafter should write for one connected account.
//!
//! Every method takes the connection first and the caller's [`OwnerScope`] second. Reads need
//! [`Capability::View`] and writes need [`Capability::Edit`]. A style is bound to its account,
//! so an account outside the scope's limit reads as [`DataError::NotFound`].

use chrono::{DateTime, Utc};
use postit_core::{AccountId, WritingStyleId};
use sqlx::PgConnection;

use crate::error::DataError;
use crate::text::{parse, text_enum};
use crate::{Capability, OwnerScope};

text_enum! {
    /// How much emoji a draft uses.
    EmojiUse {
        /// None at all.
        None => "none",
        /// A little.
        Light => "light",
        /// A lot.
        Heavy => "heavy",
    }
}

text_enum! {
    /// How many hashtags a draft carries.
    HashtagUse {
        /// None at all.
        None => "none",
        /// A few.
        Few => "few",
        /// Many.
        Many => "many",
    }
}

/// The writing guidance for an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WritingStyleFields {
    /// The tone, in the owner's words.
    pub tone: String,
    /// How much emoji to use.
    pub emoji: EmojiUse,
    /// How many hashtags to use.
    pub hashtags: HashtagUse,
    /// The language to write in, when it differs from the brand's default.
    pub language: Option<String>,
    /// Anything else the drafter should know.
    pub notes: String,
}

/// A stored writing style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WritingStyle {
    /// The style.
    pub id: WritingStyleId,
    /// The account it guides.
    pub account_id: AccountId,
    /// The guidance.
    pub fields: WritingStyleFields,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

struct StyleRow {
    id: uuid::Uuid,
    account_id: uuid::Uuid,
    tone: String,
    emoji: String,
    hashtags: String,
    language: Option<String>,
    notes: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TryFrom<StyleRow> for WritingStyle {
    type Error = DataError;

    fn try_from(row: StyleRow) -> Result<Self, DataError> {
        Ok(Self {
            id: WritingStyleId::from_uuid(row.id),
            account_id: AccountId::from_uuid(row.account_id),
            fields: WritingStyleFields {
                tone: row.tone,
                emoji: parse(&row.emoji)?,
                hashtags: parse(&row.hashtags)?,
                language: row.language,
                notes: row.notes,
            },
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

/// Writing styles.
pub struct WritingStyleRepo;

impl WritingStyleRepo {
    /// Sets the style for an account, replacing any earlier one. `id` is used only when the
    /// account has no style yet.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the account is outside the caller's limit, not the
    /// owner's, or disconnected for good.
    pub async fn upsert(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: WritingStyleId,
        account: AccountId,
        fields: WritingStyleFields,
    ) -> Result<WritingStyle, DataError> {
        scope.require(Capability::Edit)?;
        if !scope.allows_account(account) {
            return Err(DataError::NotFound);
        }
        sqlx::query_as!(
            StyleRow,
            "INSERT INTO writing_styles
                 (id, owner_id, account_id, tone, emoji, hashtags, language, notes,
                  created_by, updated_by)
             SELECT $3, owner_id, id, $4, $5, $6, $7, $8, $9, $9 FROM connected_accounts
             WHERE owner_id = $1 AND id = $2
             ON CONFLICT (account_id) DO UPDATE
             SET tone = EXCLUDED.tone, emoji = EXCLUDED.emoji, hashtags = EXCLUDED.hashtags,
                 language = EXCLUDED.language, notes = EXCLUDED.notes,
                 updated_at = now(), updated_by = EXCLUDED.updated_by
             RETURNING id, account_id, tone, emoji, hashtags, language, notes,
                       created_at, updated_at",
            scope.owner.as_uuid(),
            account.as_uuid(),
            id.as_uuid(),
            fields.tone,
            fields.emoji.as_str(),
            fields.hashtags.as_str(),
            fields.language,
            fields.notes,
            scope.actor.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?
        .try_into()
    }

    /// The style for an account.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the account is outside the caller's limit or has no style.
    pub async fn get(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        account: AccountId,
    ) -> Result<WritingStyle, DataError> {
        scope.require(Capability::View)?;
        if !scope.allows_account(account) {
            return Err(DataError::NotFound);
        }
        sqlx::query_as!(
            StyleRow,
            "SELECT id, account_id, tone, emoji, hashtags, language, notes, created_at, updated_at
             FROM writing_styles WHERE owner_id = $1 AND account_id = $2",
            scope.owner.as_uuid(),
            account.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(DataError::NotFound)?
        .try_into()
    }

    /// The styles the caller's limit reaches.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn list(
        conn: &mut PgConnection,
        scope: &OwnerScope,
    ) -> Result<Vec<WritingStyle>, DataError> {
        scope.require(Capability::View)?;
        let filter = scope.account_filter();
        sqlx::query_as!(
            StyleRow,
            "SELECT id, account_id, tone, emoji, hashtags, language, notes, created_at, updated_at
             FROM writing_styles
             WHERE owner_id = $1 AND ($2::uuid[] IS NULL OR account_id = ANY($2))
             ORDER BY created_at, id",
            scope.owner.as_uuid(),
            filter.as_deref(),
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(WritingStyle::try_from)
        .collect()
    }
}
