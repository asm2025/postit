//! The brand profile: what the drafter should know about the owner's business. One per owner.
//!
//! Every method takes the connection first and the caller's [`OwnerScope`] second. Reads need
//! [`Capability::View`] and writes need [`Capability::Edit`]. The profile is not bound to an
//! account, so a delegate's account limit does not hide it.

use chrono::{DateTime, Utc};
use postit_core::BrandProfileId;
use sqlx::PgConnection;

use crate::error::DataError;
use crate::{Capability, OwnerScope};

/// The most example posts a profile holds.
pub const MAX_EXAMPLES: usize = 5;

/// What a profile says about the business.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrandProfileFields {
    /// What the business is.
    pub description: String,
    /// Who it writes for.
    pub audience: String,
    /// What it sells or offers.
    pub offerings: String,
    /// Words the drafter must not use.
    pub avoid_words: Vec<String>,
    /// The language to write in by default.
    pub default_language: Option<String>,
    /// Posts that sound right, at most [`MAX_EXAMPLES`].
    pub examples: Vec<String>,
}

/// A stored brand profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrandProfile {
    /// The profile.
    pub id: BrandProfileId,
    /// What it says.
    pub fields: BrandProfileFields,
    /// When it was created.
    pub created_at: DateTime<Utc>,
    /// When it last changed.
    pub updated_at: DateTime<Utc>,
}

struct ProfileRow {
    id: uuid::Uuid,
    description: String,
    audience: String,
    offerings: String,
    avoid_words: Vec<String>,
    default_language: Option<String>,
    examples: Vec<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ProfileRow> for BrandProfile {
    fn from(row: ProfileRow) -> Self {
        Self {
            id: BrandProfileId::from_uuid(row.id),
            fields: BrandProfileFields {
                description: row.description,
                audience: row.audience,
                offerings: row.offerings,
                avoid_words: row.avoid_words,
                default_language: row.default_language,
                examples: row.examples,
            },
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// The brand profile.
pub struct BrandProfileRepo;

impl BrandProfileRepo {
    /// The owner's profile, if they wrote one.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] on a database failure.
    pub async fn get(
        conn: &mut PgConnection,
        scope: &OwnerScope,
    ) -> Result<Option<BrandProfile>, DataError> {
        scope.require(Capability::View)?;
        Ok(sqlx::query_as!(
            ProfileRow,
            "SELECT id, description, audience, offerings, avoid_words, default_language,
                    examples, created_at, updated_at
             FROM brand_profiles WHERE owner_id = $1",
            scope.owner.as_uuid(),
        )
        .fetch_optional(&mut *conn)
        .await?
        .map(BrandProfile::from))
    }

    /// Writes the owner's profile, replacing any earlier one. `id` is used only when there is
    /// none yet.
    ///
    /// # Errors
    ///
    /// [`DataError::Sql`] when `examples` holds more than [`MAX_EXAMPLES`] entries, or on a
    /// database failure.
    pub async fn upsert(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: BrandProfileId,
        fields: BrandProfileFields,
    ) -> Result<BrandProfile, DataError> {
        scope.require(Capability::Edit)?;
        let row = sqlx::query_as!(
            ProfileRow,
            "INSERT INTO brand_profiles
                 (id, owner_id, description, audience, offerings, avoid_words,
                  default_language, examples, created_by, updated_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9)
             ON CONFLICT (owner_id) DO UPDATE
             SET description = EXCLUDED.description, audience = EXCLUDED.audience,
                 offerings = EXCLUDED.offerings, avoid_words = EXCLUDED.avoid_words,
                 default_language = EXCLUDED.default_language, examples = EXCLUDED.examples,
                 updated_at = now(), updated_by = EXCLUDED.updated_by
             RETURNING id, description, audience, offerings, avoid_words, default_language,
                       examples, created_at, updated_at",
            id.as_uuid(),
            scope.owner.as_uuid(),
            fields.description,
            fields.audience,
            fields.offerings,
            &fields.avoid_words,
            fields.default_language,
            &fields.examples,
            scope.actor.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(row.into())
    }
}
