//! Maps named database constraints to the errors callers act on.

use crate::error::DataError;

/// Whether `err` is a foreign-key violation: a referenced row is missing or belongs to
/// another owner, or a row still references the one being deleted.
pub(crate) fn is_foreign_key_violation(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_foreign_key_violation)
}

/// Turns a failed query into the `DataError` its constraint names. Unique violations on the
/// indexes callers branch on get their own variant or code, other unique violations become
/// [`DataError::Conflict`] with the constraint name, the self-delegation check becomes
/// `Conflict("self_delegation")`, and everything else stays [`DataError::Sql`].
pub(crate) fn translate(err: sqlx::Error) -> DataError {
    let Some(db) = err.as_database_error() else {
        return DataError::Sql(err);
    };
    if db.is_check_violation() && db.constraint() == Some("delegations_not_self") {
        return DataError::Conflict("self_delegation".to_string());
    }
    if !db.is_unique_violation() {
        return DataError::Sql(err);
    }
    match db.constraint() {
        Some("delegations_live_pair_idx") => DataError::Conflict("delegation_exists".to_string()),
        Some("publish_jobs_one_live_job_idx") => DataError::JobActive,
        Some("connected_accounts_remote_key_idx") => DataError::AccountConnectedElsewhere,
        Some("connected_accounts_one_email_idx") => DataError::SendingMethodExists,
        Some(name) => DataError::Conflict(name.to_string()),
        None => DataError::Sql(err),
    }
}

#[cfg(test)]
mod tests {
    use sqlx::PgPool;
    use uuid::Uuid;

    use super::translate;
    use crate::error::DataError;

    async fn account(
        pool: &PgPool,
        owner: Uuid,
        platform: &str,
        remote: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO connected_accounts (id, owner_id, platform_id, remote_account_id, display_name) \
             VALUES ($1, $2, $3, $4, 'Account')",
        )
        .bind(Uuid::now_v7())
        .bind(owner)
        .bind(platform)
        .bind(remote)
        .execute(pool)
        .await
        .map(|_| ())
    }

    async fn user(pool: &PgPool) -> Uuid {
        crate::testkit::seed_user(pool, "active").await.as_uuid()
    }

    #[sqlx::test]
    async fn translate_maps_named_constraints(pool: PgPool) {
        for platform in ["email", "mastodon"] {
            sqlx::query("INSERT INTO platforms (id, display_name) VALUES ($1, $1)")
                .bind(platform)
                .execute(&pool)
                .await
                .unwrap_or_else(|e| unreachable!("platform: {e}"));
        }
        let first = user(&pool).await;
        let second = user(&pool).await;

        account(&pool, first, "email", "smtp-a")
            .await
            .unwrap_or_else(|e| unreachable!("first email: {e}"));
        let err = account(&pool, first, "email", "smtp-b")
            .await
            .err()
            .unwrap_or_else(|| unreachable!("a second sending method"));
        assert!(matches!(translate(err), DataError::SendingMethodExists));

        account(&pool, first, "mastodon", "@a@example.social")
            .await
            .unwrap_or_else(|e| unreachable!("first mastodon: {e}"));
        let err = account(&pool, second, "mastodon", "@a@example.social")
            .await
            .err()
            .unwrap_or_else(|| unreachable!("the same remote account for another owner"));
        assert!(matches!(
            translate(err),
            DataError::AccountConnectedElsewhere
        ));

        let err = sqlx::query("INSERT INTO platforms (id, display_name) VALUES ('email', 'x')")
            .execute(&pool)
            .await
            .err()
            .unwrap_or_else(|| unreachable!("a duplicate platform"));
        assert!(matches!(translate(err), DataError::Conflict(name) if name == "platforms_pkey"));

        let err = sqlx::query("SELECT 1/0")
            .execute(&pool)
            .await
            .err()
            .unwrap_or_else(|| unreachable!("division by zero"));
        assert!(matches!(translate(err), DataError::Sql(_)));
    }
}
