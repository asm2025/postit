use std::collections::BTreeSet;

use postit_data::DataError;
use postit_data::vault::{SealedColumn, key_versions_in_use, vault_check_startup};
use postit_vault::{SealContext, Vault};
use sqlx::PgPool;
use uuid::Uuid;

const KEY: &[u8] = b"0123456789abcdef0123456789abcdef-test-key";

fn vault(versions: &[u32]) -> Vault {
    Vault::from_keys(versions.iter().map(|v| (*v, KEY.to_vec())))
        .unwrap_or_else(|e| unreachable!("vault: {e}"))
}

const SCRATCH: SealedColumn = SealedColumn {
    table: "scratch_secrets",
    column: "secret",
    id_column: "id",
};

async fn scratch(pool: &PgPool) {
    sqlx::query("CREATE TABLE scratch_secrets (id UUID PRIMARY KEY, secret BYTEA)")
        .execute(pool)
        .await
        .unwrap_or_else(|e| unreachable!("create scratch: {e}"));
}

async fn insert_sealed(pool: &PgPool, vault: &Vault) {
    let id = Uuid::now_v7();
    let sealed = vault
        .seal(
            &"s3cret".to_owned(),
            &SealContext {
                table: "scratch_secrets",
                column: "secret",
                row: id,
            },
        )
        .unwrap_or_else(|e| unreachable!("seal: {e}"));
    sqlx::query("INSERT INTO scratch_secrets (id, secret) VALUES ($1, $2)")
        .bind(id)
        .bind(sealed.as_bytes())
        .execute(pool)
        .await
        .unwrap_or_else(|e| unreachable!("insert: {e}"));
}

#[sqlx::test]
async fn vault_state_raised_then_lower_refused(pool: PgPool) {
    vault_check_startup(&pool, &vault(&[1, 2]), &[])
        .await
        .unwrap_or_else(|e| unreachable!("check 1: {e}"));
    let stored: i32 = sqlx::query_scalar("SELECT newest_key_version FROM vault_state")
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("stored: {e}"));
    assert_eq!(stored, 2);

    let err = vault_check_startup(&pool, &vault(&[1]), &[]).await.err();
    assert!(matches!(
        err,
        Some(DataError::VaultKeyBehind {
            configured: 1,
            stored: 2
        })
    ));
}

#[sqlx::test]
async fn missing_version_in_use_refused(pool: PgPool) {
    scratch(&pool).await;
    insert_sealed(&pool, &vault(&[1])).await;
    let err = vault_check_startup(&pool, &vault(&[2]), &[SCRATCH])
        .await
        .err();
    assert!(matches!(
        err,
        Some(DataError::VaultKeyMissing { version: 1 })
    ));
}

#[sqlx::test]
async fn versions_in_use_reports_per_column(pool: PgPool) {
    scratch(&pool).await;
    insert_sealed(&pool, &vault(&[1])).await;
    insert_sealed(&pool, &vault(&[1, 3])).await;
    sqlx::query("INSERT INTO scratch_secrets (id, secret) VALUES ($1, NULL)")
        .bind(Uuid::now_v7())
        .execute(&pool)
        .await
        .unwrap_or_else(|e| unreachable!("insert null: {e}"));
    let in_use = key_versions_in_use(&pool, &[SCRATCH])
        .await
        .unwrap_or_else(|e| unreachable!("in use: {e}"));
    assert_eq!(in_use.len(), 1);
    assert_eq!(
        in_use["scratch_secrets.secret"],
        BTreeSet::from([1_u32, 3_u32])
    );
}
