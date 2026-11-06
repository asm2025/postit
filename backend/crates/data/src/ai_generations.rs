//! AI generations: one row per generate request the provider answered, for the owner's spend
//! view. The prompt itself is stored only when the owner has chosen to keep prompts.
//!
//! `prompt_hash` marks repeats of one owner's prompt without keeping the text.

use chrono::{DateTime, Utc};
use emixdb::dto::{Pagination, ResultSet};
use hmac::{Hmac, Mac};
use postit_core::{AiGenerationId, AiProviderId, PostId, UserId};
use sha2::Sha256;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::constraint::is_foreign_key_violation;
use crate::error::DataError;
use crate::text::{parse, text_enum};
use crate::{Capability, OwnerScope, paging};

text_enum! {
    /// How a generation ended.
    GenerationOutcome {
        /// The answer was usable.
        Ok => "ok",
        /// The provider answered, but the output could not be used, even after one repair.
        InvalidOutput => "invalid_output",
        /// The provider answered with a failure after taking the request.
        Failed => "failed",
    }
}

/// `HMAC-SHA256(key, owner UUID bytes ++ prompt UTF-8 bytes)`. The key comes from
/// `Vault::derive_key("ai_prompt_hash")`, so the hash cannot be rebuilt from a guessed prompt,
/// and the owner's ID keeps two owners' identical prompts apart.
#[must_use]
pub fn prompt_hash(key: &[u8], owner: UserId, prompt: &str) -> [u8; 32] {
    // HMAC accepts keys of any length; `new_from_slice` only fails for fixed-size MACs.
    let Ok(mut mac) = <Hmac<Sha256> as Mac>::new_from_slice(key) else {
        unreachable!("HMAC-SHA256 accepts keys of any length")
    };
    mac.update(owner.as_uuid().as_bytes());
    mac.update(prompt.as_bytes());
    mac.finalize().into_bytes().into()
}

/// A generation to record.
#[derive(Clone)]
pub struct NewAiGeneration {
    /// The row's ID.
    pub id: AiGenerationId,
    /// The provider that answered.
    pub provider_id: Option<AiProviderId>,
    /// The model it answered with.
    pub model: String,
    /// `post`, `reply`, or `rewrite`.
    pub mode: String,
    /// The keyed hash of the prompt, from [`prompt_hash`].
    pub prompt_hash: [u8; 32],
    /// The prompt, only when the owner keeps prompts.
    pub prompt: Option<String>,
    /// Tokens sent, summed over the repair call.
    pub input_tokens: Option<i32>,
    /// Tokens received, summed over the repair call.
    pub output_tokens: Option<i32>,
    /// How it ended.
    pub outcome: GenerationOutcome,
}

impl std::fmt::Debug for NewAiGeneration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewAiGeneration")
            .field("id", &self.id)
            .field("provider_id", &self.provider_id)
            .field("model", &self.model)
            .field("mode", &self.mode)
            .field("outcome", &self.outcome)
            .finish_non_exhaustive()
    }
}

/// A stored generation, as the history lists it.
#[derive(Clone)]
pub struct AiGeneration {
    /// The generation.
    pub id: AiGenerationId,
    /// The provider, or `None` once it was deleted.
    pub provider_id: Option<AiProviderId>,
    /// The provider's name, or `None` once it was deleted.
    pub provider_name: Option<String>,
    /// The model.
    pub model: String,
    /// `post`, `reply`, or `rewrite`.
    pub mode: String,
    /// The prompt, when the owner kept it and retention has not cleared it.
    pub prompt: Option<String>,
    /// Tokens sent.
    pub input_tokens: Option<i32>,
    /// Tokens received.
    pub output_tokens: Option<i32>,
    /// How it ended.
    pub outcome: GenerationOutcome,
    /// The post saved from it, when there is one.
    pub post_id: Option<PostId>,
    /// Who made the request (a delegate, or the owner).
    pub actor: Option<UserId>,
    /// When it was made.
    pub created_at: DateTime<Utc>,
}

impl std::fmt::Debug for AiGeneration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AiGeneration")
            .field("id", &self.id)
            .field("provider_id", &self.provider_id)
            .field("provider_name", &self.provider_name)
            .field("model", &self.model)
            .field("mode", &self.mode)
            .field("outcome", &self.outcome)
            .field("post_id", &self.post_id)
            .field("actor", &self.actor)
            .field("created_at", &self.created_at)
            .finish_non_exhaustive()
    }
}

struct Row {
    id: Uuid,
    provider_id: Option<Uuid>,
    provider_name: Option<String>,
    model: String,
    mode: String,
    prompt: Option<String>,
    input_tokens: Option<i32>,
    output_tokens: Option<i32>,
    outcome: String,
    post_id: Option<Uuid>,
    created_by: Option<Uuid>,
    created_at: DateTime<Utc>,
}

impl TryFrom<Row> for AiGeneration {
    type Error = DataError;

    fn try_from(row: Row) -> Result<Self, DataError> {
        Ok(Self {
            id: AiGenerationId::from_uuid(row.id),
            provider_id: row.provider_id.map(AiProviderId::from_uuid),
            provider_name: row.provider_name,
            model: row.model,
            mode: row.mode,
            prompt: row.prompt,
            input_tokens: row.input_tokens,
            output_tokens: row.output_tokens,
            outcome: parse(&row.outcome)?,
            post_id: row.post_id.map(PostId::from_uuid),
            actor: row.created_by.map(UserId::from_uuid),
            created_at: row.created_at,
        })
    }
}

/// AI generations.
pub struct AiGenerationRepo;

impl AiGenerationRepo {
    /// Records a generation for the scope's owner. `created_by` is the actor.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] below the edit level, and [`DataError::Sql`] on a database failure
    /// (a `mode` outside `post`, `reply`, `rewrite` is refused by the table).
    pub async fn insert(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        new: NewAiGeneration,
    ) -> Result<(), DataError> {
        scope.require(Capability::Edit)?;
        let hash: &[u8] = &new.prompt_hash;
        sqlx::query!(
            "INSERT INTO ai_generations
                 (id, owner_id, provider_id, model, mode, prompt_hash, prompt,
                  input_tokens, output_tokens, outcome, created_by, updated_by)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $11)",
            new.id.as_uuid(),
            scope.owner.as_uuid(),
            new.provider_id.map(|p| p.as_uuid()),
            new.model,
            new.mode,
            hash,
            new.prompt,
            new.input_tokens,
            new.output_tokens,
            new.outcome.as_str(),
            scope.actor.as_uuid(),
        )
        .execute(&mut *conn)
        .await?;
        Ok(())
    }

    /// Links a generation to the post saved from it. Returns whether the owner has such a
    /// generation.
    ///
    /// # Errors
    ///
    /// [`DataError::NotFound`] when the owner has no such post, [`DataError::Scope`] below the
    /// edit level, and [`DataError::Sql`] on a database failure.
    pub async fn link_post(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        id: AiGenerationId,
        post: PostId,
    ) -> Result<bool, DataError> {
        scope.require(Capability::Edit)?;
        let done = sqlx::query!(
            "UPDATE ai_generations SET post_id = $3, updated_at = now(), updated_by = $4
             WHERE owner_id = $1 AND id = $2",
            scope.owner.as_uuid(),
            id.as_uuid(),
            post.as_uuid(),
            scope.actor.as_uuid(),
        )
        .execute(&mut *conn)
        .await
        .map_err(|e| {
            // The post is not the owner's (the composite foreign key names the owner).
            if is_foreign_key_violation(&e) {
                DataError::NotFound
            } else {
                e.into()
            }
        })?;
        Ok(done.rows_affected() > 0)
    }

    /// The owner's generations, newest first.
    ///
    /// # Errors
    ///
    /// [`DataError::Scope`] unless the caller is the owner, and [`DataError::Sql`] on a
    /// database failure.
    pub async fn list(
        conn: &mut PgConnection,
        scope: &OwnerScope,
        pagination: Pagination,
    ) -> Result<ResultSet<AiGeneration>, DataError> {
        scope.require(Capability::OwnerOnly)?;
        let (limit, offset) = paging::bounds(pagination);
        let rows = sqlx::query_as!(
            Row,
            r#"SELECT g.id, g.provider_id, p.config->>'name' AS provider_name, g.model, g.mode,
                      g.prompt, g.input_tokens, g.output_tokens, g.outcome, g.post_id,
                      g.created_by, g.created_at
               FROM ai_generations g
               LEFT JOIN ai_providers p ON p.owner_id = g.owner_id AND p.id = g.provider_id
               WHERE g.owner_id = $1
               ORDER BY g.created_at DESC, g.id DESC
               LIMIT $2 OFFSET $3"#,
            scope.owner.as_uuid(),
            limit,
            offset,
        )
        .fetch_all(&mut *conn)
        .await?;
        let total = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM ai_generations WHERE owner_id = $1",
            scope.owner.as_uuid(),
        )
        .fetch_one(&mut *conn)
        .await?;
        Ok(ResultSet {
            data: rows
                .into_iter()
                .map(AiGeneration::try_from)
                .collect::<Result<_, _>>()?,
            total: paging::total(total),
            pagination: Some(pagination),
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use postit_core::UserId;
    use sha2::{Digest, Sha256};

    use super::prompt_hash;

    #[test]
    fn prompt_hash_is_keyed_and_per_owner() {
        let key = b"derived-key-for-tests-0123456789";
        let a = UserId::from_uuid(uuid::Uuid::now_v7());
        let b = UserId::from_uuid(uuid::Uuid::now_v7());
        let hash = prompt_hash(key, a, "write a post");
        assert_eq!(hash, prompt_hash(key, a, "write a post"));
        assert_ne!(hash, prompt_hash(key, b, "write a post"));
        assert_ne!(
            hash,
            prompt_hash(b"another-key-another-key-another!", a, "write a post")
        );
        assert_ne!(hash, prompt_hash(key, a, "write a post."));
        let bare: [u8; 32] = Sha256::digest(b"write a post").into();
        assert_ne!(hash, bare);
        let mut with_owner = a.as_uuid().as_bytes().to_vec();
        with_owner.extend_from_slice(b"write a post");
        let unkeyed: [u8; 32] = Sha256::digest(&with_owner).into();
        assert_ne!(hash, unkeyed);
    }
}
